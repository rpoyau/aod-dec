use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use aod_core::ns::ContentHash;
use aodc::diagnostics::DiagnosticCode;
use aodc::ir::{dumps_ir, loads_ir};
use aodc::ns_host::DirPackageProvider;
use aodc::{
    audit_dec_file_with_ns_and_options, audit_dec_source, build_aot_file_with_ns_and_options,
    build_aot_file_with_options, check_file, check_file_with_ns_and_options, check_source,
    exec_ir_str, format_source, jit_file_captured, jit_file_with_ns_and_bootstrap_captured,
    run_file, run_file_with_ns_and_bootstrap_capturing, LoadOptions, OptLevel, RtNamespace,
};

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn fresh_temp_dir(tag: &str) -> PathBuf {
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "aodc_p9_pkg_{tag}_{}_{}",
        std::process::id(),
        seq
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp test dir");
    dir
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, content).expect("write test file");
}

#[test]
fn compiler_modules_core_has_zero_ambient_fs_or_env() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let modules_rs = fs::read_to_string(manifest_dir.join("src/modules.rs"))
        .expect("read crates/aodc/src/modules.rs");
    for line in modules_rs.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }
        assert!(
            !trimmed.contains("std::fs"),
            "ambient std::fs forbidden in modules.rs: {trimmed}"
        );
        assert!(
            !trimmed.contains("std::env"),
            "ambient std::env forbidden in modules.rs: {trimmed}"
        );
    }
}

#[test]
fn pure_wave_namespace_and_slash_use_format_roundtrip() {
    let bootstrap_src = "\
namespace world:
    clear
    fork
    mount ro b\"builtin:aod/sys\" -> /sys
    mount ramfs -> /scratch
    mount ramfs before -> /tmp/work
    bind /aod/core -> /core
    unmount /tmp/work
";
    let formatted_boot = format_source(bootstrap_src).expect("format bootstrap namespace");
    assert_eq!(formatted_boot, bootstrap_src);
    let formatted_twice = format_source(&formatted_boot).expect("idempotent format");
    assert_eq!(formatted_twice, bootstrap_src);

    // Also verify alternate modifier order (`mount before ramfs`, `bind replace`) normalizes cleanly
    let alt_boot = "\
namespace world:
  clear
  fork
  mount b\"builtin:aod/sys\" -> /sys
  mount ramfs -> /scratch
  mount before ramfs -> /tmp/work
  bind replace /aod/core -> /core
  unmount /tmp/work
";
    assert_eq!(
        format_source(alt_boot).expect("format alternate modifier order"),
        bootstrap_src
    );

    let boot_audit = audit_dec_source(bootstrap_src);
    assert!(
        boot_audit.passed(),
        "standalone bootstrap namespace file must pass 6/6 audit hooks: {:?}",
        boot_audit.errors
    );
    assert_eq!(boot_audit.hooks_passed(), 6);

    // Verify `/` paths containing adverb stems (`min`, `max`, `superpose`) and keyword `in`
    let dir = fresh_temp_dir("slash_keywords");
    write_file(
        &dir.join("pkg_kw/min/in.dec"),
        "\
dec add_kw(a: int, b: int) -> int:
    a + b
",
    );
    write_file(
        &dir.join("pkg_kw/superpose/max.dec"),
        "\
dec mul_kw(a: int, b: int) -> int:
    a * b
",
    );
    let main_src = "\
namespace kw_app:
    clear
    mount ro b\"./pkg_kw\" -> /math

use /aod/core:
    int

use /math/min/in:
    add_kw

use /math/superpose/max:
    mul_kw

dec main():
    emit add_kw(int(3), mul_kw(int(4), int(5)))
";
    let formatted_main = format_source(main_src).expect("format slash-keyword main");
    assert_eq!(formatted_main, main_src);

    let main_path = dir.join("main.dec");
    write_file(&main_path, main_src);
    let vm_out = run_file(&main_path).expect("run slash-keyword main");
    assert_eq!(vm_out.to_stdout_string(), "23\n");

    let (jit_stdout, jit_res) =
        jit_file_captured(&main_path, OptLevel::O2, 100_000, 64).expect("jit slash-keyword main");
    jit_res.expect("jit run ok");
    assert_eq!(jit_stdout, "23\n");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn same_provider_id_shares_nominal_types_across_mount_aliases() {
    let dir = fresh_temp_dir("same_provider_id");
    let calc_src = "\
struct CalcPair:
    lhs: int
    rhs: int

dec make_pair(a: int, b: int) -> CalcPair:
    CalcPair(a, b)

dec sum_pair(p: CalcPair) -> int:
    p.lhs + p.rhs
";
    // Two distinct physical directories with byte-identical package contents -> identical Provider.id!
    write_file(&dir.join("mirror_one/calc/lib.dec"), calc_src);
    write_file(&dir.join("mirror_two/calc/lib.dec"), calc_src);

    let prov1 = DirPackageProvider::load_package_dir(
        &dir.join("mirror_one/calc"),
        None,
        aod_core::ns::MountMode::Static,
        aod_core::ns::MountPerm::ReadOnly,
    )
    .expect("load mirror_one");
    let prov2 = DirPackageProvider::load_package_dir(
        &dir.join("mirror_two/calc"),
        None,
        aod_core::ns::MountMode::Static,
        aod_core::ns::MountPerm::ReadOnly,
    )
    .expect("load mirror_two");
    assert_eq!(
        prov1.id, prov2.id,
        "byte-identical package trees in different directories must produce identical Merkle Provider.id"
    );

    let main_path = dir.join("main.dec");
    write_file(
        &main_path,
        "\
namespace app:
    clear
    mount ro b\"./mirror_one/calc\" -> /calc_left
    mount ro b\"./mirror_two/calc\" -> /calc_right

use /calc_left:
    make_pair
use /calc_right:
    sum_pair

dec main():
    let p = make_pair(19, 23)
    emit sum_pair(p)
",
    );

    let checked = check_file(&main_path).expect("check nominal sharing");
    assert_eq!(
        checked.unit.bundled_units.len(),
        1,
        "identical Provider.id + subpath must deduplicate to a single UnitId in CheckedUnit"
    );

    let ir_v4 = dumps_ir(&checked);
    assert!(ir_v4.contains("\"aod.dec.ir.v4\""));

    let vm_out = run_file(&main_path).expect("vm run");
    assert_eq!(vm_out.to_stdout_string(), "42\n");

    let (jit_stdout, jit_res) =
        jit_file_captured(&main_path, OptLevel::O2, 100_000, 64).expect("jit compile");
    jit_res.expect("jit run");
    assert_eq!(jit_stdout, "42\n");

    // Delete both mirror directories and prove DecIR v4 executes 100% hermetically!
    fs::remove_dir_all(dir.join("mirror_one")).expect("remove mirror_one");
    fs::remove_dir_all(dir.join("mirror_two")).expect("remove mirror_two");
    let ir_out = exec_ir_str(&ir_v4).expect("hermetic DecIR v4 exec");
    assert_eq!(ir_out.to_stdout_string(), "42\n");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn diamond_multi_version_coexistence_and_type_confusion_rejection() {
    let dir = fresh_temp_dir("diamond_versions");

    // calc 1.0.0: 2D Vec2
    write_file(
        &dir.join("vendor/calc_1_0_0/vec.dec"),
        "\
struct Vec2:
    x: int
    y: int

dec build_vec(a: int) -> Vec2:
    Vec2(a, a + 1)

dec norm_vec(v: Vec2) -> int:
    v.x + v.y
",
    );

    // calc 2.0.0: 3-component Vec2 with different layout and semantics, mounted at the SAME `/calc`!
    write_file(
        &dir.join("vendor/calc_2_0_0/vec.dec"),
        "\
struct Vec2:
    x: int
    y: int
    w: int

dec build_vec(a: int) -> Vec2:
    Vec2(a * 10, a * 20, 7)

dec norm_vec(v: Vec2) -> int:
    v.x + v.y + v.w
",
    );

    // Sub-package alpha mounts calc 1.0.0 at `/calc` via its isolated `ns.dec`
    write_file(
        &dir.join("pkgs/alpha/ns.dec"),
        "\
namespace alpha_ns:
    clear
    mount ro b\"../../vendor/calc_1_0_0\" -> /calc
",
    );
    write_file(
        &dir.join("pkgs/alpha/api.dec"),
        "\
use /calc:
    Vec2, build_vec, norm_vec

dec run_alpha(seed: int) -> int:
    norm_vec(build_vec(seed))

dec export_alpha_vec(seed: int) -> Vec2:
    build_vec(seed)
",
    );

    // Sub-package beta mounts calc 2.0.0 at `/calc` via its isolated `ns.dec`
    write_file(
        &dir.join("pkgs/beta/ns.dec"),
        "\
namespace beta_ns:
    clear
    mount ro b\"../../vendor/calc_2_0_0\" -> /calc
",
    );
    write_file(
        &dir.join("pkgs/beta/api.dec"),
        "\
use /calc:
    Vec2, build_vec, norm_vec

dec run_beta(seed: int) -> int:
    norm_vec(build_vec(seed))

dec consume_beta_vec(v: Vec2) -> int:
    norm_vec(v)
",
    );

    // 1. Coexistence: main calls BOTH alpha (calc 1.0.0 at /calc) and beta (calc 2.0.0 at /calc)
    let main_ok = dir.join("main_ok.dec");
    write_file(
        &main_ok,
        "\
namespace app:
    clear
    mount ro b\"./pkgs/alpha\" -> /alpha
    mount ro b\"./pkgs/beta\" -> /beta

use /alpha/api:
    run_alpha
use /beta/api:
    run_beta

dec main():
    emit run_alpha(5)
    emit run_beta(5)
",
    );

    let checked = check_file(&main_ok).expect("diamond coexistence must typecheck");
    let ir_v4 = dumps_ir(&checked);

    let vm_out = run_file(&main_ok).expect("vm diamond run");
    // alpha: 5 + 6 = 11; beta: 50 + 100 + 7 = 157
    assert_eq!(vm_out.to_stdout_string(), "11\n157\n");

    let (jit_stdout, jit_res) =
        jit_file_captured(&main_ok, OptLevel::O2, 100_000, 64).expect("jit diamond compile");
    jit_res.expect("jit diamond run");
    assert_eq!(jit_stdout, "11\n157\n");

    let aot_exe = dir.join("diamond_exe");
    build_aot_file_with_options(&main_ok, OptLevel::O2, &aot_exe, &LoadOptions::default())
        .expect("build diamond AOT");
    let aot_run = Command::new(&aot_exe).output().expect("exec diamond AOT");
    assert!(aot_run.status.success());
    assert_eq!(String::from_utf8_lossy(&aot_run.stdout), "11\n157\n");

    // 2. Cross-version nominal type firewall: passing calc 1.0.0 `Vec2` into calc 2.0.0 `Vec2` consumer MUST fail!
    let main_bad = dir.join("main_bad.dec");
    write_file(
        &main_bad,
        "\
namespace app_bad:
    clear
    mount ro b\"./pkgs/alpha\" -> /alpha
    mount ro b\"./pkgs/beta\" -> /beta

use /alpha/api:
    export_alpha_vec
use /beta/api:
    consume_beta_vec

dec main():
    let v1 = export_alpha_vec(5)
    emit consume_beta_vec(v1)
",
    );
    let err =
        check_file(&main_bad).expect_err("mixing calc 1.0.0 Vec2 and calc 2.0.0 Vec2 must fail");
    assert_eq!(err.code, DiagnosticCode::TypeError);

    // 3. Hermetic DecIR v4 replay after wiping the entire vendor + pkgs tree!
    fs::remove_dir_all(dir.join("vendor")).expect("remove vendor");
    fs::remove_dir_all(dir.join("pkgs")).expect("remove pkgs");
    let reloaded = loads_ir(&ir_v4).expect("loads_ir v4 hermetically without vendor/pkgs on disk");
    let ir_exec = exec_ir_str(&dumps_ir(&reloaded)).expect("exec reloaded v4 IR");
    assert_eq!(ir_exec.to_stdout_string(), "11\n157\n");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn plan9_bind_before_union_hotfix_and_cli_bootstrap_namespace() {
    let dir = fresh_temp_dir("union_hotfix");

    // Base package `/calc` exports `CalcPair`, `make_pair`, `add`, and `mul`
    write_file(
        &dir.join("vendor/calc/lib.dec"),
        "\
struct CalcPair:
    a: int
    b: int

dec make_pair(a: int, b: int) -> CalcPair:
    CalcPair(a, b)

dec add(a: int, b: int) -> int:
    a + b

dec mul(p: CalcPair) -> int:
    p.a * p.b
",
    );

    // Hotfix overrides ONLY `add` (+1000 sentinel); `CalcPair`, `make_pair`, and `mul` fall through to base!
    write_file(
        &dir.join("patches/fast_add.dec"),
        "\
dec add(a: int, b: int) -> int:
    a + b + 1000
",
    );

    // Runtime data directory mounted via `bootstrap.dec` into BOTH compile & runtime namespace!
    write_file(&dir.join("runtime_cfg/greeting.txt"), "ok");

    let bootstrap_path = dir.join("bootstrap.dec");
    write_file(
        &bootstrap_path,
        "\
namespace prod_world:
    clear
    mount ro b\"builtin:aod/sys\" -> /sys
    mount ro b\"./vendor/calc\" -> /calc
    mount ro b\"./patches/fast_add.dec\" -> /dev/fast_add
    bind before /dev/fast_add -> /calc
    mount ro b\"./runtime_cfg\" -> /cfg
",
    );

    // Notice `main.dec` has ZERO external paths, hashes, or version coordinates!
    let main_path = dir.join("main.dec");
    write_file(
        &main_path,
        "\
use /calc:
    CalcPair, make_pair, add, mul
use /sys:
    read_file_bytes

dec main() -> ():
    let cfg_bytes = read_file_bytes(b\"/cfg/greeting.txt\")
    let p = make_pair(6, 7)
    emit add(2, 3)
    emit mul(p)
    emit len(cfg_bytes)
",
    );

    let opts = LoadOptions::default();
    let audit = audit_dec_file_with_ns_and_options(&main_path, Some(&bootstrap_path), &opts);
    assert!(
        audit.passed(),
        "audit-dec with --namespace must pass 6/6 hooks: {:?}",
        audit.errors
    );

    let checked =
        check_file_with_ns_and_options(&main_path, Some(&bootstrap_path), &opts).expect("check");
    assert_eq!(checked.unit.declared_rt_mounts.len(), 2);

    // 1. Exact VM via `--namespace bootstrap.dec` (auto-mounting `/cfg` from `bootstrap.dec`!)
    let (emits, vm_res) = run_file_with_ns_and_bootstrap_capturing(
        &main_path,
        Some(&bootstrap_path),
        100_000,
        64,
        &opts,
        false,
        RtNamespace::empty(),
        Vec::new(),
    )
    .expect("vm run with bootstrap");
    vm_res.expect("vm outcome ok");
    assert_eq!(emits, vec!["1005", "42", "2"]);

    // 2. ORC LLJIT inside Linux Plan 9 container (auto-mounting `/cfg` read-only into container!)
    let (jit_stdout, jit_res, _plan, _note) = jit_file_with_ns_and_bootstrap_captured(
        &main_path,
        Some(&bootstrap_path),
        OptLevel::O2,
        100_000,
        64,
        None,
        &opts,
        false,
        RtNamespace::empty(),
        Vec::new(),
    )
    .expect("jit compile with bootstrap");
    jit_res.expect("jit run ok");
    assert_eq!(jit_stdout, "1005\n42\n2\n");

    // 3. Standalone AOT ELF built with `--namespace bootstrap.dec`
    let aot_exe = dir.join("union_hotfix_exe");
    build_aot_file_with_ns_and_options(
        &main_path,
        Some(&bootstrap_path),
        OptLevel::O2,
        &aot_exe,
        &opts,
    )
    .expect("build AOT with bootstrap");
    let cfg_mount = format!("ro:{}:/cfg", dir.join("runtime_cfg").display());
    let aot_out = Command::new(&aot_exe)
        .args(["--mount", &cfg_mount])
        .output()
        .expect("run AOT");
    assert!(aot_out.status.success());
    assert_eq!(String::from_utf8_lossy(&aot_out.stdout), "1005\n42\n2\n");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn content_addressed_sha256_lock_rejects_1byte_tamper_without_union_fallthrough() {
    let dir = fresh_temp_dir("sha256_lock");
    let pkg_dir = dir.join("locked_calc");
    write_file(
        &pkg_dir.join("lib.dec"),
        "\
dec compute() -> int:
    77
",
    );
    write_file(
        &dir.join("fallback_calc/lib.dec"),
        "\
dec compute() -> int:
    999
",
    );

    let locked_prov = DirPackageProvider::load_package_dir(
        &pkg_dir,
        None,
        aod_core::ns::MountMode::Static,
        aod_core::ns::MountPerm::ReadOnly,
    )
    .expect("compute Merkle hash of locked_calc");
    let expected_hex = locked_prov.id.to_hex();

    let main_path = dir.join("main.dec");
    write_file(
        &main_path,
        &format!(
            "\
namespace locked_world:
    clear
    mount ro b\"./fallback_calc\" -> /calc
    mount ro before b\"sha256:{expected_hex}@./locked_calc\" -> /calc

use /calc/lib:
    compute

dec main():
    emit compute()
"
        ),
    );

    // Untampered: resolves to `locked_calc` (77)
    let out = run_file(&main_path).expect("untampered locked package must run");
    assert_eq!(out.to_stdout_string(), "77\n");

    // Flip 1 byte inside `locked_calc/lib.dec`: MUST fail immediately and NEVER fall through to 999!
    write_file(
        &pkg_dir.join("lib.dec"),
        "\
dec compute() -> int:
    78
",
    );
    let err = check_file(&main_path).expect_err("tampered package must fail SHA-256 lock check");
    assert_eq!(err.code, DiagnosticCode::ModuleNotFound);
    assert!(
        err.message.contains("SHA-256 mismatch"),
        "expected SHA-256 mismatch error, got: {}",
        err.message
    );

    // Also verify single-file `sha256:<hex>@<file.dec>` lock
    let file_path = dir.join("single.dec");
    let single_src = "dec val() -> int:\n    12\n";
    write_file(&file_path, single_src);
    let wrong_hex = ContentHash::digest_bytes(b"wrong").to_hex();
    let bad_single = dir.join("bad_single_main.dec");
    write_file(
        &bad_single,
        &format!(
            "\
namespace single_lock:
    clear
    mount ro b\"sha256:{wrong_hex}@./single.dec\" -> /single

use /single:
    val

dec main():
    emit val()
"
        ),
    );
    let err2 = check_file(&bad_single).expect_err("wrong single-file SHA-256 must fail");
    assert_eq!(err2.code, DiagnosticCode::ModuleNotFound);
    assert!(err2.message.contains("SHA-256 mismatch"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn clear_capability_namespace_denies_sys_and_qext_unless_mounted() {
    let denied_sys = "\
namespace untrusted_plugin:
    clear

use /sys:
    read_file_bytes

dec main():
    let bs = read_file_bytes(b\"/etc/passwd\")
    emit len(bs)
";
    let err = check_source(denied_sys).expect_err("clear must strip /sys capability");
    assert_eq!(err.code, DiagnosticCode::ModuleNotFound);

    let denied_legacy_sys = "\
namespace untrusted_plugin:
    clear

use aod::sys::read_file_bytes

dec main():
    let bs = read_file_bytes(b\"/etc/passwd\")
    emit len(bs)
";
    let err_leg =
        check_source(denied_legacy_sys).expect_err("clear must strip /aod/sys capability");
    assert_eq!(err_leg.code, DiagnosticCode::ModuleNotFound);

    let denied_qext = "\
namespace untrusted_plugin:
    clear

use /aod/qext:
    ext_decimal

quarantine Q_ext QDemo:
    dec demo:
        () -> QExt<ratio>:
            ext_decimal(b\"1.5\")

dec main():
    pass
";
    let err_q = check_source(denied_qext).expect_err("clear must strip /aod/qext capability");
    assert_eq!(err_q.code, DiagnosticCode::ModuleNotFound);
}
