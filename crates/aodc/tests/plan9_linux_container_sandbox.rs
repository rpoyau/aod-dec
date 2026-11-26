#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::Command;

use aod_llvm::{OptLevel, OrcJitSession, SandboxTier};
use aodc::{RtMountSpec, RtNamespace};

fn aodc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aodc"))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Raw LLVM IR module that completely bypasses `aod_rt` and attempts direct `libc` syscalls
/// (`open("/etc/passwd")`, `open("/home")`, `open("/evil_root", O_CREAT|O_WRONLY)`, `socket(AF_INET, SOCK_STREAM, 0)`)
/// inside `OrcJitSession` to prove real OS kernel container confinement.
const HOSTILE_RAW_SYSCALL_IR: &str = r#"
@.p_passwd = private unnamed_addr constant [12 x i8] c"/etc/passwd\00"
@.p_home   = private unnamed_addr constant [6 x i8]  c"/home\00"
@.p_root_w = private unnamed_addr constant [11 x i8] c"/evil_root\00"
@.p_in_ro  = private unnamed_addr constant [14 x i8] c"/in/probe.txt\00"

declare i32 @open(ptr, i32, ...)
declare i32 @close(i32)
declare i32 @socket(i32, i32, i32)
declare void @aod_rt_emit_bool(ptr, i8)

define void @aod_jit_entry(ptr %ctx) {
entry:
  ; 1. Attempt to open /etc/passwd (O_RDONLY = 0) -> must fail (< 0)
  %fd_pw = call i32 (ptr, i32, ...) @open(ptr @.p_passwd, i32 0)
  %pw_blocked = icmp slt i32 %fd_pw, 0
  %pw_b = zext i1 %pw_blocked to i8
  call void @aod_rt_emit_bool(ptr %ctx, i8 %pw_b)

  ; 2. Attempt to open /home (O_RDONLY = 0) -> must fail (< 0)
  %fd_hm = call i32 (ptr, i32, ...) @open(ptr @.p_home, i32 0)
  %hm_blocked = icmp slt i32 %fd_hm, 0
  %hm_b = zext i1 %hm_blocked to i8
  call void @aod_rt_emit_bool(ptr %ctx, i8 %hm_b)

  ; 3. Attempt to create /evil_root on `/` (O_WRONLY|O_CREAT = 65, 0644 = 420) -> must fail (< 0)
  %fd_rw = call i32 (ptr, i32, ...) @open(ptr @.p_root_w, i32 65, i32 420)
  %rw_blocked = icmp slt i32 %fd_rw, 0
  %rw_b = zext i1 %rw_blocked to i8
  call void @aod_rt_emit_bool(ptr %ctx, i8 %rw_b)

  ; 4. Attempt to open TCP socket(AF_INET=2, SOCK_STREAM=1, 0) -> must fail (< 0 via seccomp-bpf + CLONE_NEWNET)
  %fd_sk = call i32 @socket(i32 2, i32 1, i32 0)
  %sk_blocked = icmp slt i32 %fd_sk, 0
  %sk_b = zext i1 %sk_blocked to i8
  call void @aod_rt_emit_bool(ptr %ctx, i8 %sk_b)

  ; 5. Attempt to read /in/probe.txt (O_RDONLY = 0) -> succeeds ONLY when `/in` is mounted!
  %fd_in_r = call i32 (ptr, i32, ...) @open(ptr @.p_in_ro, i32 0)
  %in_readable = icmp sge i32 %fd_in_r, 0
  br i1 %in_readable, label %close_in, label %after_in
close_in:
  %c_rc = call i32 @close(i32 %fd_in_r)
  br label %after_in
after_in:
  %in_r_b = zext i1 %in_readable to i8
  call void @aod_rt_emit_bool(ptr %ctx, i8 %in_r_b)

  ; 6. Attempt to open /in/probe.txt for writing (O_WRONLY = 1) -> must fail (< 0 via MOUNT_ATTR_RDONLY / Landlock RO)!
  %fd_in_w = call i32 (ptr, i32, ...) @open(ptr @.p_in_ro, i32 1)
  %in_w_blocked = icmp slt i32 %fd_in_w, 0
  %in_w_b = zext i1 %in_w_blocked to i8
  call void @aod_rt_emit_bool(ptr %ctx, i8 %in_w_b)

  ret void
}
"#;

#[test]
#[cfg(target_os = "linux")]
fn orc_jit_enforces_linux_plan9_kernel_container_by_default_against_raw_libc_syscalls() {
    let sess = OrcJitSession::compile(HOSTILE_RAW_SYSCALL_IR, OptLevel::O2)
        .expect("compile hostile syscall probe");

    let tier = sess
        .ensure_default_sandbox_tier()
        .expect("spawn default Linux container worker");
    assert!(
        matches!(
            tier,
            SandboxTier::LinuxPlan9PivotRoot | SandboxTier::LinuxLandlockSeccomp
        ),
        "expected active kernel sandbox tier on Linux, got {tier:?}"
    );

    // 1. Default empty namespace: `/etc/passwd` blocked, `/home` blocked, `/` write blocked,
    //    `socket` blocked, `/in/probe.txt` NOT readable (`false`), `/in/probe.txt` write blocked!
    let (stdout_empty, res_empty) = sess.run_captured(10_000, 64);
    res_empty.expect("default container probe run");
    assert_eq!(
        stdout_empty.lines().collect::<Vec<_>>(),
        vec!["true", "true", "true", "true", "false", "true"],
        "unexpected default container probe output:\n{stdout_empty}"
    );

    // Verify persistent worker reuse across repeated invocations (`bench` / `learn` fast path)
    for _ in 0..5 {
        let (out_again, res_again) = sess.run_captured(10_000, 64);
        res_again.unwrap();
        assert_eq!(out_again, stdout_empty);
    }

    // 2. Mount a temporary host directory read-only at `/in`:
    //    If tier is `LinuxPlan9PivotRoot`, `/in/probe.txt` is kernel bind-mounted at `/in` (`true`),
    //    while `/etc/passwd`, `/home`, `/` write, `socket`, and `/in/probe.txt` write (`EROFS`) stay `true` (blocked)!
    if tier == SandboxTier::LinuxPlan9PivotRoot {
        let tmp = std::env::temp_dir().join(format!("aod_jit_jail_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("probe.txt"), b"ok\n").unwrap();

        let mut ns = RtNamespace::empty();
        ns.apply_spec(&RtMountSpec::parse_cli(&format!("ro:{}:/in", tmp.display())).unwrap())
            .unwrap();
        let (stdout_mounted, res_mounted) = sess.run_captured_with_ns(10_000, 64, ns, Vec::new());
        let _ = std::fs::remove_dir_all(&tmp);
        res_mounted.expect("mounted container probe run");
        assert_eq!(
            stdout_mounted.lines().collect::<Vec<_>>(),
            vec!["true", "true", "true", "true", "true", "true"],
            "unexpected ro-mounted container probe output:\n{stdout_mounted}"
        );
    }
}

#[test]
fn cli_run_jit_and_aot_enforce_default_plan9_sandbox_and_explicit_mounts_3way_parity() {
    let tmp = std::env::temp_dir().join(format!("aod_cli_jail_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let in_dir = tmp.join("in_data");
    let out_dir = tmp.join("out_bins");
    std::fs::create_dir_all(&in_dir).unwrap();
    std::fs::create_dir_all(&out_dir).unwrap();

    let prog_path = tmp.join("reader_prog.dec");
    std::fs::write(
        &prog_path,
        "\
use aod::sys:
    cli_args, read_file_bytes

dec main() -> int:
    let args = cli_args()
    let payload = read_file_bytes(args[1])
    emit len(payload)
    0
",
    )
    .unwrap();
    let sample_file = in_dir.join("payload.bin");
    std::fs::write(&sample_file, b"plan9-inferno-29bytes-payload").unwrap();

    // 1. Without `--mount`, all 3 execution modes (`run`, `jit`, `build` AOT) MUST deny host path & unmounted `/in`!
    let aot_exe = tmp.join("reader_aot");
    let build_out = Command::new(aodc_bin())
        .current_dir(repo_root())
        .args([
            "build",
            "-O2",
            &prog_path.to_string_lossy(),
            "-o",
            &aot_exe.to_string_lossy(),
        ])
        .output()
        .unwrap();
    assert!(
        build_out.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&build_out.stderr)
    );

    for (label, out) in [
        (
            "run-unmounted",
            Command::new(aodc_bin())
                .current_dir(repo_root())
                .args([
                    "run",
                    &prog_path.to_string_lossy(),
                    "--",
                    "/in/payload.bin",
                ])
                .output()
                .unwrap(),
        ),
        (
            "jit-unmounted",
            Command::new(aodc_bin())
                .current_dir(repo_root())
                .args([
                    "jit",
                    "-O2",
                    &prog_path.to_string_lossy(),
                    "--",
                    "/in/payload.bin",
                ])
                .output()
                .unwrap(),
        ),
        (
            "aot-unmounted",
            Command::new(&aot_exe)
                .args(["/in/payload.bin"])
                .output()
                .unwrap(),
        ),
        (
            "jit-host-passwd-denied",
            Command::new(aodc_bin())
                .current_dir(repo_root())
                .args([
                    "jit",
                    "-O2",
                    &prog_path.to_string_lossy(),
                    "--",
                    "/etc/passwd",
                ])
                .output()
                .unwrap(),
        ),
    ] {
        assert!(
            !out.status.success(),
            "{label} unexpectedly succeeded without mount!"
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("not mounted"),
            "{label} expected 'not mounted' error, got: {stderr}"
        );
    }

    // 2. With `--mount ro:<in_dir>:/in`, all 3 modes (`run`, `jit`, `aot`) succeed with exact stdout parity!
    let mount_ro = format!("ro:{}:/in", in_dir.display());
    let run_ok = Command::new(aodc_bin())
        .current_dir(repo_root())
        .args([
            "run",
            "--mount",
            &mount_ro,
            &prog_path.to_string_lossy(),
            "--",
            "/in/payload.bin",
        ])
        .output()
        .unwrap();
    assert!(
        run_ok.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&run_ok.stderr)
    );

    let jit_ok = Command::new(aodc_bin())
        .current_dir(repo_root())
        .args([
            "jit",
            "-O2",
            "--mount",
            &mount_ro,
            &prog_path.to_string_lossy(),
            "--",
            "/in/payload.bin",
        ])
        .output()
        .unwrap();
    assert!(
        jit_ok.status.success(),
        "jit failed: {}",
        String::from_utf8_lossy(&jit_ok.stderr)
    );

    let aot_ok = Command::new(&aot_exe)
        .args(["--mount", &mount_ro, "/in/payload.bin"])
        .output()
        .unwrap();
    assert!(
        aot_ok.status.success(),
        "aot failed: {}",
        String::from_utf8_lossy(&aot_ok.stderr)
    );

    assert_eq!(run_ok.stdout, b"29\n0\n");
    assert_eq!(jit_ok.stdout, b"29\n0\n");
    assert_eq!(aot_ok.stdout, b"29\n0\n");

    let _ = std::fs::remove_dir_all(&tmp);
}
