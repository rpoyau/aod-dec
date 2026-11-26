use std::path::PathBuf;

use aod_core::ns::{BindMode, CleanPath, MountPerm};
use aod_rt::ns_fs::{MemFsProvider, RtMountSpec, RtNamespace};

fn valid_dummy_elf64() -> Vec<u8> {
    let mut buf = vec![0u8; 128];
    buf[0..4].copy_from_slice(b"\x7FELF");
    buf[4] = 2; // ELFCLASS64
    buf[5] = 1; // ELFDATA2LSB
    buf
}

#[test]
fn default_empty_rt_namespace_denies_all_ambient_host_reads_and_writes() {
    let ns = RtNamespace::empty();
    assert!(ns.is_empty());
    assert!(!ns.is_unconfined());

    let err_etc = ns.read_file_bytes(b"/etc/passwd").unwrap_err();
    assert!(
        err_etc.contains("not mounted"),
        "expected not mounted error, got {err_etc}"
    );

    let err_rel = ns.read_file_bytes(b"Cargo.toml").unwrap_err();
    assert!(
        err_rel.contains("must start with '/'"),
        "expected leading slash error, got {err_rel}"
    );

    let elf = valid_dummy_elf64();
    let err_write = ns.write_executable_elf(b"/tmp/unmounted_out", &elf).unwrap_err();
    assert!(err_write.contains("not mounted"), "got {err_write}");
}

#[test]
fn ramfs_and_ro_host_union_overlay_with_symlink_and_dot_dot_defenses() {
    let tmp = std::env::temp_dir().join(format!("aod_rt_ns_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let ro_dir = tmp.join("ro_src");
    let outside_secret = tmp.join("outside_secret.txt");
    std::fs::create_dir_all(&ro_dir).unwrap();
    std::fs::write(&outside_secret, b"TOP_SECRET_OUTSIDE").unwrap();
    std::fs::write(ro_dir.join("hello.dec"), b"const X: int = 42\n").unwrap();

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside_secret, ro_dir.join("escape_link")).unwrap();
    }

    let mut ns = RtNamespace::empty();
    let spec_ro = RtMountSpec::parse_cli(&format!("ro:{}:/work", ro_dir.display())).unwrap();
    let spec_ram_before = RtMountSpec::parse_cli("ramfs+before:/work").unwrap();
    ns.apply_spec(&spec_ro).unwrap();
    ns.apply_spec(&spec_ram_before).unwrap();

    // 1. Reading `/work/hello.dec` misses empty `ramfs` front layer and falls through to `ro`!
    let data = ns.read_file_bytes(b"/work/hello.dec").unwrap();
    assert_eq!(data, b"const X: int = 42\n");

    // 2. Lexical `..` escape `/work/../outside_secret.txt` normalizes to `/outside_secret.txt` -> NotMounted!
    let dotdot_err = ns
        .read_file_bytes(b"/work/../outside_secret.txt")
        .unwrap_err();
    assert!(dotdot_err.contains("not mounted"), "got {dotdot_err}");

    // 3. Symlink inside mounted directory is strictly rejected!
    #[cfg(unix)]
    {
        let sym_err = ns.read_file_bytes(b"/work/escape_link").unwrap_err();
        assert!(
            sym_err.contains("symlink"),
            "expected symlink refusal, got {sym_err}"
        );
    }

    // 4. Writing an ELF to `/work/built_elf` lands in the writable `ramfs` front union layer
    //    WITHOUT touching the underlying read-only host directory!
    let elf = valid_dummy_elf64();
    let written = ns.write_executable_elf(b"/work/built_elf", &elf).unwrap();
    assert_eq!(written, elf.len());
    assert!(!ro_dir.join("built_elf").exists());
    assert_eq!(ns.read_file_bytes(b"/work/built_elf").unwrap(), elf);

    // 5. Read-only-only mount rejects writes with `read-only`!
    let mut ro_only_ns = RtNamespace::empty();
    ro_only_ns.apply_spec(&spec_ro).unwrap();
    let ro_write_err = ro_only_ns
        .write_executable_elf(b"/work/denied_elf", &elf)
        .unwrap_err();
    assert!(ro_write_err.contains("read-only"), "got {ro_write_err}");

    // 6. Pure in-memory `MemFsProvider` works completely disk-free!
    let mem = MemFsProvider::default();
    mem.insert_ro_file("in.dec", b"dec main() -> int:\n    0\n".to_vec())
        .unwrap();
    let mut diskless = RtNamespace::empty();
    diskless.mount_ramfs(
        mem,
        &CleanPath::parse_str("/mem").unwrap(),
        BindMode::Replace,
    );
    assert_eq!(
        diskless.read_file_bytes(b"/mem/in.dec").unwrap(),
        b"dec main() -> int:\n    0\n"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn guarded_repo_provenance_paths_cannot_be_mounted_read_write() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();

    let mut ns = RtNamespace::empty();
    // Mounting `bindings/aod` read-only is permitted, but read-write MUST fail!
    let bindings_dir = repo_root.join("bindings/aod");
    let target = CleanPath::parse_str("/bindings").unwrap();
    ns.mount_host_dir(&bindings_dir, &target, MountPerm::ReadOnly, BindMode::Replace)
        .expect("read-only mount of bindings/aod should succeed");

    let rw_err = ns
        .mount_host_dir(&bindings_dir, &target, MountPerm::ReadWrite, BindMode::Replace)
        .unwrap_err();
    assert!(
        rw_err.contains("frozen provenance"),
        "expected frozen provenance guard error, got {rw_err}"
    );

    // Mounting repo_root itself read-write would encompass SOURCE_BINDING.json -> MUST fail!
    let root_rw_err = ns
        .mount_host_dir(&repo_root, &target, MountPerm::ReadWrite, BindMode::Replace)
        .unwrap_err();
    assert!(
        root_rw_err.contains("frozen provenance"),
        "expected frozen provenance guard error on repo root, got {root_rw_err}"
    );
}
