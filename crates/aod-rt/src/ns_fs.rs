//! Safe Rust Plan 9 Runtime Namespace (`RtNamespace`) for `aod-rt` and `aodc`.
//!
//! Zero `unsafe` code (`#![deny(unsafe_code)]` inherited from crate root), zero floats,
//! 100% `std`-only. Routes all runtime file/capability operations (`aod::sys`) through
//! an explicit per-context Plan 9 `Namespace` (`empty` by default!) and provides the exact
//! mount specification projected into the Linux `unshare` + `pivot_root` container.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use aod_core::ns::{
    BindMode, CleanPath, ContentHash, MountMode, MountPerm, Namespace, NsError, Provider,
};

use crate::ops::MAX_SYS_IO_BYTES;

const GUARDED_REPO_RELS: &[&str] = &[
    "tests/fixtures/v1_examples",
    "bindings/aod",
    "reference/aod-dec-dev",
    "SOURCE_BINDING.json",
];

#[derive(Clone, Debug)]
struct MemFile {
    bytes: Arc<Vec<u8>>,
    executable: bool,
}

/// Thread-safe in-memory synthetic filesystem (`ramfs`) bounded by `MAX_SYS_IO_BYTES` per file
/// and `max_files` total entries.
#[derive(Clone, Debug)]
pub struct MemFsProvider {
    files: Arc<Mutex<BTreeMap<String, MemFile>>>,
    max_files: usize,
}

impl Default for MemFsProvider {
    fn default() -> Self {
        Self::new(4096)
    }
}

impl MemFsProvider {
    pub fn new(max_files: usize) -> Self {
        Self {
            files: Arc::new(Mutex::new(BTreeMap::new())),
            max_files: max_files.max(1),
        }
    }

    pub fn insert_ro_file(&self, subpath: &str, data: Vec<u8>) -> Result<(), NsError> {
        let clean = CleanPath::clean_subpath_str(subpath)?;
        let mut guard = self
            .files
            .lock()
            .map_err(|_| NsError::ProviderFault {
                path: clean.clone(),
                detail: "poisoned ramfs mutex".to_string(),
            })?;
        if !guard.contains_key(&clean) && guard.len() >= self.max_files {
            return Err(NsError::CapabilityDenied(
                "ramfs file count limit exceeded".to_string(),
            ));
        }
        guard.insert(
            clean,
            MemFile {
                bytes: Arc::new(data),
                executable: false,
            },
        );
        Ok(())
    }

    pub fn read_file(&self, subpath: &str) -> Result<Option<(Vec<u8>, bool)>, NsError> {
        let clean = CleanPath::clean_subpath_str(subpath)?;
        let guard = self
            .files
            .lock()
            .map_err(|_| NsError::ProviderFault {
                path: clean.clone(),
                detail: "poisoned ramfs mutex".to_string(),
            })?;
        Ok(guard
            .get(&clean)
            .map(|mf| ((*mf.bytes).clone(), mf.executable)))
    }

    fn write_file(&self, subpath: &str, data: &[u8], executable: bool) -> Result<(), NsError> {
        let clean = CleanPath::clean_subpath_str(subpath)?;
        if clean.is_empty() {
            return Err(NsError::ProviderFault {
                path: "/".to_string(),
                detail: "cannot write file bytes directly to mount root directory".to_string(),
            });
        }
        if data.len() > MAX_SYS_IO_BYTES {
            return Err(NsError::CapabilityDenied(format!(
                "file exceeds {MAX_SYS_IO_BYTES}-byte limit"
            )));
        }
        let mut guard = self
            .files
            .lock()
            .map_err(|_| NsError::ProviderFault {
                path: clean.clone(),
                detail: "poisoned ramfs mutex".to_string(),
            })?;
        if !guard.contains_key(&clean) && guard.len() >= self.max_files {
            return Err(NsError::CapabilityDenied(
                "ramfs file count limit exceeded".to_string(),
            ));
        }
        guard.insert(
            clean,
            MemFile {
                bytes: Arc::new(data.to_vec()),
                executable,
            },
        );
        Ok(())
    }
}

/// Explicitly mounted host directory chrooted to `active_root`, rejecting symlinks at every
/// path component and switching seamlessly to its in-container mount path after `pivot_root`.
#[derive(Clone, Debug)]
pub struct ScopedHostDirProvider {
    pub host_root: PathBuf,
    pub active_root: PathBuf,
}

impl ScopedHostDirProvider {
    pub fn new(host_dir: &Path, perm: MountPerm) -> Result<(ContentHash, Self), String> {
        let meta = std::fs::symlink_metadata(host_dir)
            .map_err(|e| format!("failed to stat mount source {}: {e}", host_dir.display()))?;
        if meta.file_type().is_symlink() {
            return Err(format!(
                "refusing to mount symlink directory {}",
                host_dir.display()
            ));
        }
        if !meta.is_dir() {
            return Err(format!(
                "mount source {} must be a directory",
                host_dir.display()
            ));
        }
        let canon = std::fs::canonicalize(host_dir)
            .map_err(|e| format!("failed to canonicalize {}: {e}", host_dir.display()))?;

        if perm == MountPerm::ReadWrite {
            reject_guarded_repo_overlap(&canon)?;
        }

        let perm_tag = match perm {
            MountPerm::ReadOnly => b"ro".as_slice(),
            MountPerm::ReadWrite => b"rw".as_slice(),
        };
        let id = ContentHash::digest_domain(
            b"aod.rt.host_dir.v1",
            &[canon.to_string_lossy().as_bytes(), perm_tag],
        );
        Ok((
            id,
            Self {
                host_root: canon.clone(),
                active_root: canon,
            },
        ))
    }

    fn resolve_checked_path(&self, clean_subpath: &str, create_parents: bool) -> Result<PathBuf, NsError> {
        if clean_subpath.is_empty() {
            return Err(NsError::ProviderFault {
                path: self.active_root.display().to_string(),
                detail: "expected file subpath beneath mounted directory, got mount root".to_string(),
            });
        }
        let parts: Vec<&str> = clean_subpath.split('/').collect();
        let mut cur = self.active_root.clone();
        for (idx, seg) in parts.iter().enumerate() {
            if seg.is_empty() || *seg == "." || *seg == ".." {
                return Err(NsError::SubpathEscape(clean_subpath.to_string()));
            }
            cur.push(seg);
            let is_last = idx + 1 == parts.len();
            match std::fs::symlink_metadata(&cur) {
                Ok(meta) => {
                    if meta.file_type().is_symlink() {
                        return Err(NsError::CapabilityDenied(format!(
                            "symlink traversal forbidden inside mounted directory: {clean_subpath:?}"
                        )));
                    }
                    if !is_last && !meta.is_dir() {
                        return Err(NsError::ProviderFault {
                            path: clean_subpath.to_string(),
                            detail: "intermediate path component is not a directory".to_string(),
                        });
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    if !is_last && create_parents {
                        std::fs::create_dir(&cur).map_err(|ce| NsError::ProviderFault {
                            path: clean_subpath.to_string(),
                            detail: format!("failed creating parent directory: {ce}"),
                        })?;
                    }
                }
                Err(e) => {
                    return Err(NsError::ProviderFault {
                        path: clean_subpath.to_string(),
                        detail: format!("stat error: {e}"),
                    });
                }
            }
        }
        Ok(cur)
    }

    fn read_bytes(&self, clean_subpath: &str) -> Result<Option<Vec<u8>>, NsError> {
        let full = self.resolve_checked_path(clean_subpath, false)?;
        let meta = match std::fs::symlink_metadata(&full) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(NsError::ProviderFault {
                    path: clean_subpath.to_string(),
                    detail: format!("failed to stat: {e}"),
                });
            }
        };
        if meta.file_type().is_symlink() {
            return Err(NsError::CapabilityDenied(format!(
                "refusing to read symlink {clean_subpath:?}"
            )));
        }
        if meta.is_dir() {
            return Err(NsError::ProviderFault {
                path: clean_subpath.to_string(),
                detail: "path refers to a directory, not a regular file".to_string(),
            });
        }
        if meta.len() > (MAX_SYS_IO_BYTES as u64) {
            return Err(NsError::CapabilityDenied(format!(
                "file {clean_subpath:?} exceeds {MAX_SYS_IO_BYTES}-byte bootstrap limit"
            )));
        }
        let data = std::fs::read(&full).map_err(|e| NsError::ProviderFault {
            path: clean_subpath.to_string(),
            detail: format!("failed to read: {e}"),
        })?;
        if data.len() > MAX_SYS_IO_BYTES {
            return Err(NsError::CapabilityDenied(format!(
                "file {clean_subpath:?} exceeds {MAX_SYS_IO_BYTES}-byte bootstrap limit"
            )));
        }
        Ok(Some(data))
    }

    fn write_bytes(&self, clean_subpath: &str, data: &[u8], executable: bool) -> Result<(), NsError> {
        if data.len() > MAX_SYS_IO_BYTES {
            return Err(NsError::CapabilityDenied(format!(
                "payload exceeds {MAX_SYS_IO_BYTES}-byte bootstrap limit"
            )));
        }
        let full = self.resolve_checked_path(clean_subpath, true)?;
        std::fs::write(&full, data).map_err(|e| NsError::ProviderFault {
            path: clean_subpath.to_string(),
            detail: format!("failed to write: {e}"),
        })?;
        #[cfg(unix)]
        if executable {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&full, std::fs::Permissions::from_mode(0o755)).map_err(
                |e| NsError::ProviderFault {
                    path: clean_subpath.to_string(),
                    detail: format!("failed to chmod 0755: {e}"),
                },
            )?;
        }
        Ok(())
    }
}

fn reject_guarded_repo_overlap(canon_dir: &Path) -> Result<(), String> {
    let mut cur: Option<&Path> = Some(canon_dir);
    while let Some(dir) = cur {
        if dir.join("SOURCE_BINDING.json").exists() || dir.join("bindings/aod").exists() {
            for rel in GUARDED_REPO_RELS {
                let guarded = dir.join(rel);
                if let Ok(cg) = std::fs::canonicalize(&guarded) {
                    if canon_dir == cg || canon_dir.starts_with(&cg) || cg.starts_with(canon_dir) {
                        return Err(format!(
                            "refusing read-write mount {} overlapping frozen provenance path {rel}",
                            canon_dir.display()
                        ));
                    }
                }
            }
        }
        cur = dir.parent();
    }
    Ok(())
}

/// Runtime VFS backend attached to a Plan 9 `Provider<RtFsBackend>`.
#[derive(Clone, Debug)]
pub enum RtFsBackend {
    RamFs(MemFsProvider),
    HostDir(ScopedHostDirProvider),
}

impl RtFsBackend {
    fn read_bytes(&self, clean_subpath: &str) -> Result<Option<Vec<u8>>, NsError> {
        match self {
            RtFsBackend::RamFs(m) => Ok(m.read_file(clean_subpath)?.map(|(b, _)| b)),
            RtFsBackend::HostDir(h) => h.read_bytes(clean_subpath),
        }
    }

    fn write_bytes(&self, clean_subpath: &str, data: &[u8], executable: bool) -> Result<(), NsError> {
        match self {
            RtFsBackend::RamFs(m) => m.write_file(clean_subpath, data, executable),
            RtFsBackend::HostDir(h) => h.write_bytes(clean_subpath, data, executable),
        }
    }
}

/// Parsed CLI / bootstrap runtime mount directive (`--mount <spec>`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RtMountSpec {
    HostDir {
        perm: MountPerm,
        bind_mode: BindMode,
        host_dir: PathBuf,
        target: CleanPath,
    },
    RamFs {
        bind_mode: BindMode,
        target: CleanPath,
    },
}

impl RtMountSpec {
    /// Parse `--mount` specifications:
    /// - `ro:<host_dir>:<ns_path>` (or `ro+before:...`, `ro+after:...`)
    /// - `rw:<host_dir>:<ns_path>` (or `rw+before:...`, `rw+after:...`)
    /// - `ramfs:<ns_path>` (or `ramfs+before:...`, `ramfs+after:...`)
    pub fn parse_cli(spec: &str) -> Result<Self, String> {
        let (mode_tok, rest) = spec
            .split_once(':')
            .ok_or_else(|| format!("invalid --mount spec {spec:?}; expected ro:<dir>:/path, rw:<dir>:/path, or ramfs:/path"))?;

        let (kind, bind_mode) = match mode_tok {
            "ro" => ("ro", BindMode::Replace),
            "ro+before" => ("ro", BindMode::Before),
            "ro+after" => ("ro", BindMode::After),
            "rw" => ("rw", BindMode::Replace),
            "rw+before" => ("rw", BindMode::Before),
            "rw+after" => ("rw", BindMode::After),
            "ramfs" => ("ramfs", BindMode::Replace),
            "ramfs+before" => ("ramfs", BindMode::Before),
            "ramfs+after" => ("ramfs", BindMode::After),
            other => {
                return Err(format!(
                    "unknown --mount mode {other:?} in {spec:?}; expected ro, rw, or ramfs"
                ));
            }
        };

        if kind == "ramfs" {
            let target = CleanPath::parse_str(rest)
                .map_err(|e| format!("invalid target path in --mount {spec:?}: {e}"))?;
            return Ok(RtMountSpec::RamFs { bind_mode, target });
        }

        // Split `<host_dir>:/ns/path` at the last `:` preceding a `/` so host paths remain unambiguous.
        let (host_str, ns_str) = rest.rsplit_once(":/").ok_or_else(|| {
            format!(
                "invalid --mount spec {spec:?}; expected {mode_tok}:<host_dir>:/<ns_path>"
            )
        })?;
        if host_str.is_empty() {
            return Err(format!("empty host directory in --mount {spec:?}"));
        }
        let target_raw = format!("/{ns_str}");
        let target = CleanPath::parse_str(&target_raw)
            .map_err(|e| format!("invalid namespace target {target_raw:?} in --mount {spec:?}: {e}"))?;
        let perm = if kind == "rw" {
            MountPerm::ReadWrite
        } else {
            MountPerm::ReadOnly
        };
        Ok(RtMountSpec::HostDir {
            perm,
            bind_mode,
            host_dir: PathBuf::from(host_str),
            target,
        })
    }
}

/// Exact kernel bind-mount instruction for `pivot_root` container initialization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PivotBindMount {
    pub host_dir: PathBuf,
    pub container_path: String,
    pub perm: MountPerm,
}

/// Per-task Plan 9 Runtime Namespace (`empty` by default: zero ambient host disk access).
#[derive(Clone, Debug)]
pub struct RtNamespace {
    ns: Namespace<RtFsBackend>,
    unconfined: bool,
}

impl Default for RtNamespace {
    fn default() -> Self {
        Self::empty()
    }
}

impl RtNamespace {
    /// Completely empty capability namespace (`clear` / `rfork(RFCNAMEG)`): default for all tasks!
    pub fn empty() -> Self {
        Self {
            ns: Namespace::empty(),
            unconfined: false,
        }
    }

    /// Explicit opt-in unconfined mode (`--unconfined` CLI flag ONLY).
    pub fn unconfined_host() -> Self {
        Self {
            ns: Namespace::empty(),
            unconfined: true,
        }
    }

    pub fn is_unconfined(&self) -> bool {
        self.unconfined
    }

    pub fn is_empty(&self) -> bool {
        !self.unconfined && self.ns.is_empty()
    }

    pub fn fork(&self, isolate: bool) -> Self {
        if isolate {
            Self::empty()
        } else {
            self.clone()
        }
    }

    pub fn mount_ramfs(
        &mut self,
        ramfs: MemFsProvider,
        target: &CleanPath,
        bind_mode: BindMode,
    ) {
        let id = ContentHash::digest_domain(
            b"aod.rt.ramfs.v1",
            &[target.as_bytes()],
        );
        let prov = Provider::new(
            id,
            MountMode::Static,
            MountPerm::ReadWrite,
            RtFsBackend::RamFs(ramfs),
        );
        self.ns.mount(prov, target, bind_mode);
    }

    pub fn mount_host_dir(
        &mut self,
        host_dir: &Path,
        target: &CleanPath,
        perm: MountPerm,
        bind_mode: BindMode,
    ) -> Result<(), String> {
        let (id, scoped) = ScopedHostDirProvider::new(host_dir, perm)?;
        let prov = Provider::new(
            id,
            MountMode::Static,
            perm,
            RtFsBackend::HostDir(scoped),
        );
        self.ns.mount(prov, target, bind_mode);
        Ok(())
    }

    pub fn bind(
        &mut self,
        existing: &CleanPath,
        target: &CleanPath,
        mode: BindMode,
    ) -> Result<(), String> {
        self.ns.bind(existing, target, mode).map_err(|e| e.to_string())
    }

    pub fn apply_spec(&mut self, spec: &RtMountSpec) -> Result<(), String> {
        match spec {
            RtMountSpec::RamFs { bind_mode, target } => {
                self.mount_ramfs(MemFsProvider::default(), target, *bind_mode);
                Ok(())
            }
            RtMountSpec::HostDir {
                perm,
                bind_mode,
                host_dir,
                target,
            } => self.mount_host_dir(host_dir, target, *perm, *bind_mode),
        }
    }

    /// Build the exact kernel bind-mount list and corresponding post-`pivot_root` `RtNamespace`
    /// where every `ScopedHostDirProvider` points to its in-container mount path (`/src`, `/out`,
    /// or `/.ns_layers/<idx>` for multi-layer union stacks).
    pub fn prepare_pivot_root_plan(&self) -> (Vec<PivotBindMount>, Self) {
        let mut mounts = Vec::new();
        let mut layer_seq = 0usize;

        // Pre-scan which mount points are single-layer non-root HostDir mounts without subpath prefixes.
        let mut direct_mounts = std::collections::BTreeSet::new();
        for (target, stack) in self.ns.entries() {
            if !target.is_root()
                && stack.len() == 1
                && stack[0].subpath_prefix.is_empty()
                && matches!(stack[0].backend, RtFsBackend::HostDir(_))
            {
                direct_mounts.insert(target);
            }
        }
        // Also guard against nested mount points colliding with a direct parent's internal `.ns_layers`
        let mapped_ns = self.ns.map_backends(|mount_point, prov| match &prov.backend {
            RtFsBackend::RamFs(m) => RtFsBackend::RamFs(m.clone()),
            RtFsBackend::HostDir(h) => {
                let container_path = if direct_mounts.contains(mount_point) {
                    mount_point.as_str().to_string()
                } else {
                    let p = format!("/.ns_layers/{layer_seq}");
                    layer_seq += 1;
                    p
                };
                mounts.push(PivotBindMount {
                    host_dir: h.host_root.clone(),
                    container_path: container_path.clone(),
                    perm: prov.perm,
                });
                RtFsBackend::HostDir(ScopedHostDirProvider {
                    host_root: h.host_root.clone(),
                    active_root: PathBuf::from(container_path),
                })
            }
        });

        (
            mounts,
            Self {
                ns: mapped_ns,
                unconfined: self.unconfined,
            },
        )
    }

    /// Return all host directories and their required permissions for Landlock LSM fallback rules.
    pub fn all_host_bind_dirs(&self) -> Vec<(PathBuf, MountPerm)> {
        let mut by_path: BTreeMap<PathBuf, MountPerm> = BTreeMap::new();
        for (_, stack) in self.ns.entries() {
            for prov in stack {
                if let RtFsBackend::HostDir(ref h) = prov.backend {
                    let entry = by_path
                        .entry(h.host_root.clone())
                        .or_insert(MountPerm::ReadOnly);
                    if prov.perm == MountPerm::ReadWrite {
                        *entry = MountPerm::ReadWrite;
                    }
                }
            }
        }
        by_path.into_iter().collect()
    }

    /// Read a file through the task's Plan 9 namespace (enforcing `MAX_SYS_IO_BYTES` and
    /// refusing ambient host paths unless `unconfined` was explicitly set).
    pub fn read_file_bytes(&self, raw_path: &[u8]) -> Result<Vec<u8>, String> {
        if raw_path.is_empty() || raw_path.contains(&0) {
            return Err("invalid empty or NUL-containing file path".to_string());
        }
        if self.unconfined {
            let path_str = std::str::from_utf8(raw_path)
                .map_err(|_| "file path must be valid UTF-8".to_string())?;
            let meta = std::fs::symlink_metadata(path_str)
                .map_err(|e| format!("failed to stat {path_str:?}: {e}"))?;
            if meta.file_type().is_symlink() {
                return Err(format!("refusing to read symlink {path_str:?}"));
            }
            if meta.len() > (MAX_SYS_IO_BYTES as u64) {
                return Err(format!(
                    "file {path_str:?} exceeds {MAX_SYS_IO_BYTES}-byte bootstrap limit"
                ));
            }
            let data = std::fs::read(path_str)
                .map_err(|e| format!("failed to read {path_str:?}: {e}"))?;
            if data.len() > MAX_SYS_IO_BYTES {
                return Err(format!(
                    "file {path_str:?} exceeds {MAX_SYS_IO_BYTES}-byte bootstrap limit"
                ));
            }
            return Ok(data);
        }

        let clean_path = CleanPath::parse(raw_path)
            .map_err(|e| format!("sandbox namespace error: {e}"))?;
        let candidates = self.ns.resolve(&clean_path);
        if candidates.is_empty() {
            return Err(format!(
                "sandbox namespace error: path {:?} is not mounted",
                clean_path.as_str()
            ));
        }

        for cand in candidates {
            if let Some(bytes) = cand
                .provider
                .backend
                .read_bytes(&cand.subpath)
                .map_err(|e| format!("sandbox namespace error: {e}"))?
            {
                return Ok(bytes);
            }
        }
        Err(format!(
            "file {:?} not found in mounted namespace",
            clean_path.as_str()
        ))
    }

    /// Write an executable ELF64 image through the task's Plan 9 namespace (selecting the first
    /// `ReadWrite` layer in the union stack at `raw_path`, or failing with `ReadOnlyMount` / `NotMounted`).
    pub fn write_executable_elf(&self, raw_path: &[u8], img: &[u8]) -> Result<usize, String> {
        if raw_path.is_empty() || raw_path.contains(&0) {
            return Err("invalid empty or NUL-containing output ELF path".to_string());
        }
        if img.len() < 64 || &img[0..4] != b"\x7FELF" {
            return Err("write_executable_elf requires a valid ELF64 header (\\x7FELF)".to_string());
        }
        if img.len() > MAX_SYS_IO_BYTES {
            return Err(format!(
                "ELF image exceeds {MAX_SYS_IO_BYTES}-byte bootstrap limit"
            ));
        }

        if self.unconfined {
            let path_str = std::str::from_utf8(raw_path)
                .map_err(|_| "output ELF path must be valid UTF-8".to_string())?;
            let target = Path::new(path_str);
            if let Ok(meta) = std::fs::symlink_metadata(target) {
                if meta.file_type().is_symlink() {
                    return Err(format!("refusing to overwrite symlink {path_str:?}"));
                }
            }
            reject_guarded_repo_file_target(target)?;
            std::fs::write(target, img)
                .map_err(|e| format!("failed to write ELF {path_str:?}: {e}"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| format!("failed to chmod +x {path_str:?}: {e}"))?;
            }
            return Ok(img.len());
        }

        let clean_path = CleanPath::parse(raw_path)
            .map_err(|e| format!("sandbox namespace error: {e}"))?;
        let candidates = self.ns.resolve(&clean_path);
        if candidates.is_empty() {
            return Err(format!(
                "sandbox namespace error: path {:?} is not mounted",
                clean_path.as_str()
            ));
        }

        for cand in &candidates {
            if cand.provider.perm == MountPerm::ReadWrite {
                cand.provider
                    .backend
                    .write_bytes(&cand.subpath, img, true)
                    .map_err(|e| format!("sandbox namespace error: {e}"))?;
                return Ok(img.len());
            }
        }
        Err(format!(
            "sandbox namespace error: mount covering {:?} is read-only",
            clean_path.as_str()
        ))
    }
}

fn reject_guarded_repo_file_target(target: &Path) -> Result<(), String> {
    let abs = if target.is_absolute() {
        target.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("cwd error: {e}"))?
            .join(target)
    };
    let Some(parent) = abs.parent() else {
        return Ok(());
    };
    if let Ok(canon_parent) = std::fs::canonicalize(parent) {
        let full = canon_parent.join(abs.file_name().unwrap_or_default());
        let mut cur: Option<&Path> = Some(&canon_parent);
        while let Some(dir) = cur {
            if dir.join("SOURCE_BINDING.json").exists() || dir.join("bindings/aod").exists() {
                for rel in GUARDED_REPO_RELS {
                    let guarded = dir.join(rel);
                    if let Ok(cg) = std::fs::canonicalize(&guarded) {
                        if full == cg || full.starts_with(&cg) {
                            return Err(format!(
                                "refusing to overwrite frozen provenance artifact under {rel}"
                            ));
                        }
                    }
                }
            }
            cur = dir.parent();
        }
    }
    Ok(())
}
