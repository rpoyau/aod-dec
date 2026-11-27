//! Layer 4 Host Boundary Providers (`ns_host`): the ONLY module in `aodc` permitted to
//! map host directories, `AOD_STDLIB_DIR`, `sha256:<hex>@<path>` package bundles, or
//! serialized `DecIR` bundles into opaque `CompileProvider` handles.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use aod_core::ns::{
    BindMode, CleanPath, ContentHash, MountMode, MountPerm, NsError, Provider, ProviderVTable,
};
use aod_rt::ns_fs::RtMountSpec;

use crate::ast::Item;
use crate::diagnostics::{DecResult, Diagnostic, DiagnosticCode};
use crate::modules::{
    apply_namespace_decl, make_builtin_provider, mount_all_cli_builtins, mount_pure_aod_builtins,
    resolve_builtin_uri_provider, BundledModule, BundledUnitRecord, CompileNamespace,
    CompileProvider, LoadOptions, MountUriResolver, NsMountRecord, RawModuleUnit, RawUnitKind,
};
use crate::parser::parse_source;

const MAX_PACKAGE_FILES: usize = 512;
const MAX_PACKAGE_DEPTH: usize = 8;

/// In-memory `CompileProvider` mapping clean relative subpaths (`""` or `"a/b"`) to `.dec` sources.
pub struct MemoryCompileProvider {
    units: BTreeMap<String, (Arc<str>, ContentHash, Arc<crate::ast::Program>)>,
    isolated_ns: Option<CompileNamespace>,
}

impl MemoryCompileProvider {
    pub fn from_sources(
        files: &[(&str, &str)],
        isolated_ns: Option<CompileNamespace>,
    ) -> DecResult<Provider<CompileProvider>> {
        let mut units = BTreeMap::new();
        let mut merkle_parts: Vec<Vec<u8>> = Vec::new();

        for (raw_sub, src) in files {
            let clean = CleanPath::clean_subpath_str(raw_sub)
                .map_err(|e| Diagnostic::new(DiagnosticCode::ModuleNotFound, e.to_string()))?;
            let prog = parse_source(src)?;
            let sha = ContentHash::digest_bytes(src.as_bytes());
            units.insert(
                clean.clone(),
                (Arc::from(*src), sha, Arc::new(prog)),
            );
        }
        for (sub, (_, sha, _)) in &units {
            let mut rec = Vec::with_capacity(sub.len() + 33);
            rec.extend_from_slice(sub.as_bytes());
            rec.push(0);
            rec.extend_from_slice(sha.as_bytes());
            merkle_parts.push(rec);
        }
        let refs: Vec<&[u8]> = merkle_parts.iter().map(Vec::as_slice).collect();
        let bundle_id = ContentHash::digest_domain(b"aod.pkg.bundle.v1", &refs);

        let backend: CompileProvider = Arc::new(Self { units, isolated_ns });
        Ok(Provider::new(
            bundle_id,
            MountMode::Static,
            MountPerm::ReadOnly,
            backend,
        ))
    }
}

impl ProviderVTable<RawModuleUnit> for MemoryCompileProvider {
    fn get_unit(&self, subpath: &[u8]) -> Result<Option<RawModuleUnit>, NsError> {
        let clean = CleanPath::clean_subpath(subpath)?;
        let Some((src, sha, prog)) = self.units.get(&clean) else {
            return Ok(None);
        };
        Ok(Some(RawModuleUnit {
            kind: RawUnitKind::DecModule {
                source: Arc::clone(src),
                sha256: *sha,
                program: Arc::clone(prog),
                isolated_ns: self.isolated_ns.clone(),
                per_unit_provider_id: None,
            },
        }))
    }
}

/// Content-addressed directory package provider (`mount ro b"[sha256:<hex>@]<dir>" -> /target`).
/// Computes its `Provider.id` strictly from `(relative_subpath, sha256(file_bytes))` with ZERO
/// host path bytes, so identical package trees anywhere on disk share nominal types and JIT code!
pub struct DirPackageProvider {
    units: BTreeMap<String, (Arc<str>, ContentHash, Arc<crate::ast::Program>)>,
    isolated_ns: Option<CompileNamespace>,
}

impl DirPackageProvider {
    pub fn load_single_dec_file(
        file: &Path,
        expected_sha256: Option<ContentHash>,
        mode: MountMode,
        perm: MountPerm,
    ) -> Result<Provider<CompileProvider>, NsError> {
        let meta = std::fs::symlink_metadata(file).map_err(|e| NsError::ProviderFault {
            path: file.display().to_string(),
            detail: format!("failed to stat module file: {e}"),
        })?;
        if meta.file_type().is_symlink() {
            return Err(NsError::CapabilityDenied(format!(
                "refusing to mount symlink module file {}",
                file.display()
            )));
        }
        let text = std::fs::read_to_string(file).map_err(|e| NsError::ProviderFault {
            path: file.display().to_string(),
            detail: format!("failed reading .dec file: {e}"),
        })?;
        let file_sha = ContentHash::digest_bytes(text.as_bytes());
        if let Some(expected) = expected_sha256 {
            if file_sha != expected {
                return Err(NsError::ProviderFault {
                    path: file.display().to_string(),
                    detail: format!(
                        "file SHA-256 mismatch: expected {expected}, computed {file_sha}"
                    ),
                });
            }
        }
        let prog = parse_source(&text).map_err(|e| NsError::ProviderFault {
            path: file.display().to_string(),
            detail: e.to_string(),
        })?;
        let mut isolated_ns = CompileNamespace::empty();
        mount_pure_aod_builtins(&mut isolated_ns);
        let mut units = BTreeMap::new();
        units.insert(
            String::new(),
            (Arc::from(text.as_str()), file_sha, Arc::new(prog)),
        );
        let backend: CompileProvider = Arc::new(Self {
            units,
            isolated_ns: Some(isolated_ns),
        });
        Ok(Provider::new(file_sha, mode, perm, backend))
    }

    pub fn load_package_dir(
        dir: &Path,
        expected_sha256: Option<ContentHash>,
        mode: MountMode,
        perm: MountPerm,
    ) -> Result<Provider<CompileProvider>, NsError> {
        let meta = std::fs::symlink_metadata(dir).map_err(|e| NsError::ProviderFault {
            path: dir.display().to_string(),
            detail: format!("failed to stat package directory: {e}"),
        })?;
        if meta.file_type().is_symlink() {
            return Err(NsError::CapabilityDenied(format!(
                "refusing to mount symlink package directory {}",
                dir.display()
            )));
        }
        if !meta.is_dir() {
            return Err(NsError::ProviderFault {
                path: dir.display().to_string(),
                detail: "mount source must be a directory".to_string(),
            });
        }
        let canon = std::fs::canonicalize(dir).map_err(|e| NsError::ProviderFault {
            path: dir.display().to_string(),
            detail: format!("failed canonicalizing package directory: {e}"),
        })?;

        let mut raw_files: BTreeMap<String, (String, ContentHash)> = BTreeMap::new();
        collect_package_dec_files(&canon, &canon, 0, &mut raw_files)?;

        // Compute deterministic host-path-independent Merkle `Provider.id` over all `.dec` files!
        let mut merkle_parts: Vec<Vec<u8>> = Vec::with_capacity(raw_files.len());
        for (rel_dec, (_, sha)) in &raw_files {
            let mut rec = Vec::with_capacity(rel_dec.len() + 33);
            rec.extend_from_slice(rel_dec.as_bytes());
            rec.push(0);
            rec.extend_from_slice(sha.as_bytes());
            merkle_parts.push(rec);
        }
        let refs: Vec<&[u8]> = merkle_parts.iter().map(Vec::as_slice).collect();
        let bundle_id = ContentHash::digest_domain(b"aod.pkg.bundle.v1", &refs);

        if let Some(expected) = expected_sha256 {
            if bundle_id != expected {
                return Err(NsError::ProviderFault {
                    path: dir.display().to_string(),
                    detail: format!(
                        "package SHA-256 mismatch: expected {expected}, computed {bundle_id}"
                    ),
                });
            }
        }

        // If `ns.dec` exists inside the package root, evaluate its `namespace` block to build
        // this package's isolated child `CompileNamespace` (`clear` + pure `/aod/*` + declared deps)!
        let mut isolated_ns = {
            let mut cns = CompileNamespace::empty();
            mount_pure_aod_builtins(&mut cns);
            cns
        };

        let mut units = BTreeMap::new();
        let mut opt_pkg_ns_prog: Option<crate::ast::Program> = None;

        for (rel_dec, (src, sha)) in &raw_files {
            let prog = parse_source(src).map_err(|e| NsError::ProviderFault {
                path: rel_dec.clone(),
                detail: e.to_string(),
            })?;
            if rel_dec == "ns.dec"
                && prog
                    .items
                    .iter()
                    .all(|it| matches!(it, Item::Namespace(_)))
            {
                opt_pkg_ns_prog = Some(prog);
                continue;
            }
            let prog_arc = Arc::new(prog);
            let src_arc: Arc<str> = Arc::from(src.as_str());

            // Map `<subpath>.dec` -> `<subpath>`, and `<subpath>/mod.dec` (or root `mod.dec`) -> `<subpath>` / `""`!
            if rel_dec == "mod.dec" {
                units.insert(String::new(), (Arc::clone(&src_arc), *sha, Arc::clone(&prog_arc)));
            } else if let Some(prefix) = rel_dec.strip_suffix("/mod.dec") {
                units.insert(
                    prefix.to_string(),
                    (Arc::clone(&src_arc), *sha, Arc::clone(&prog_arc)),
                );
            } else if let Some(stem) = rel_dec.strip_suffix(".dec") {
                units.insert(stem.to_string(), (src_arc, *sha, prog_arc));
            }
        }

        // Also map single-file package `<dir>/<only>.dec` to `""` when `mod.dec` is absent
        // so `mount ro b"./calc_v1" -> /calc` + `use /calc: add` works for both `mod.dec` and single-file dirs!
        if !units.contains_key("") && units.len() == 1 {
            let (_, entry_val) = units.iter().next().unwrap();
            let cloned = entry_val.clone();
            units.insert(String::new(), cloned);
        }

        // Mount self at `/self` inside the package's own isolated namespace so internal submodules work,
        // then apply any `ns.dec` directives relative to `canon`!
        let self_backend_prelim: CompileProvider = Arc::new(Self {
            units: units.clone(),
            isolated_ns: None,
        });
        isolated_ns.mount(
            Provider::new(
                bundle_id,
                MountMode::Static,
                MountPerm::ReadOnly,
                self_backend_prelim,
            ),
            &CleanPath::parse_str("/self").unwrap(),
            BindMode::Replace,
        );

        if let Some(ns_prog) = opt_pkg_ns_prog {
            let pkg_resolver = HostUriResolver::new(Some(&canon));
            let mut unused_rt = Vec::new();
            for item in &ns_prog.items {
                if let Item::Namespace(decl) = item {
                    apply_namespace_decl(&mut isolated_ns, &mut unused_rt, decl, &pkg_resolver)
                        .map_err(|e| NsError::ProviderFault {
                            path: format!("{}/ns.dec", dir.display()),
                            detail: e.to_string(),
                        })?;
                }
            }
        }

        let backend: CompileProvider = Arc::new(Self {
            units,
            isolated_ns: Some(isolated_ns),
        });
        Ok(Provider::new(bundle_id, mode, perm, backend))
    }
}

impl ProviderVTable<RawModuleUnit> for DirPackageProvider {
    fn get_unit(&self, subpath: &[u8]) -> Result<Option<RawModuleUnit>, NsError> {
        let clean = CleanPath::clean_subpath(subpath)?;
        let Some((src, sha, prog)) = self.units.get(&clean) else {
            return Ok(None);
        };
        Ok(Some(RawModuleUnit {
            kind: RawUnitKind::DecModule {
                source: Arc::clone(src),
                sha256: *sha,
                program: Arc::clone(prog),
                isolated_ns: self.isolated_ns.clone(),
                per_unit_provider_id: None,
            },
        }))
    }
}

fn collect_package_dec_files(
    root: &Path,
    cur: &Path,
    depth: usize,
    out: &mut BTreeMap<String, (String, ContentHash)>,
) -> Result<(), NsError> {
    if depth > MAX_PACKAGE_DEPTH {
        return Err(NsError::CapabilityDenied(
            "package directory nesting exceeds maximum depth".to_string(),
        ));
    }
    let rd = std::fs::read_dir(cur).map_err(|e| NsError::ProviderFault {
        path: cur.display().to_string(),
        detail: format!("read_dir failed: {e}"),
    })?;
    let mut entries = Vec::new();
    for res in rd {
        let ent = res.map_err(|e| NsError::ProviderFault {
            path: cur.display().to_string(),
            detail: format!("dir entry error: {e}"),
        })?;
        entries.push(ent);
    }
    entries.sort_by_key(|e| e.file_name());

    for ent in entries {
        let ft = ent.file_type().map_err(|e| NsError::ProviderFault {
            path: ent.path().display().to_string(),
            detail: format!("file_type error: {e}"),
        })?;
        if ft.is_symlink() {
            // Never follow symlinks inside package directories
            continue;
        }
        let p = ent.path();
        let name = ent.file_name();
        let name_s = name.to_string_lossy();
        if name_s.starts_with('.') || name_s == "target" {
            continue;
        }
        if ft.is_dir() {
            collect_package_dec_files(root, &p, depth + 1, out)?;
        } else if ft.is_file() && p.extension().and_then(|x| x.to_str()) == Some("dec") {
            if out.len() >= MAX_PACKAGE_FILES {
                return Err(NsError::CapabilityDenied(format!(
                    "package exceeds {MAX_PACKAGE_FILES} .dec files"
                )));
            }
            let rel = p
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let text = std::fs::read_to_string(&p).map_err(|e| NsError::ProviderFault {
                path: rel.clone(),
                detail: format!("failed reading .dec file: {e}"),
            })?;
            let sha = ContentHash::digest_bytes(text.as_bytes());
            out.insert(rel, (text, sha));
        }
    }
    Ok(())
}

/// On-demand local sibling file provider used strictly for the default CLI `base_dir` fallback
/// so compiling a standalone file inside `/tmp` never scans unrelated `/tmp` subdirectories.
struct LazyLocalDirProvider {
    root: PathBuf,
    cache: Mutex<BTreeMap<String, Option<(Arc<str>, ContentHash, Arc<crate::ast::Program>)>>>,
}

impl LazyLocalDirProvider {
    fn new(root: PathBuf) -> Provider<CompileProvider> {
        let id = ContentHash::digest_domain(b"aod.local.lazy.v1", &[b"local"]);
        let backend: CompileProvider = Arc::new(Self {
            root,
            cache: Mutex::new(BTreeMap::new()),
        });
        Provider::new(id, MountMode::Static, MountPerm::ReadOnly, backend)
    }

    fn find_candidate_file(&self, clean_subpath: &str) -> Option<PathBuf> {
        if clean_subpath.is_empty() {
            return None;
        }
        let mut cur = self.root.clone();
        for seg in clean_subpath.split('/') {
            if seg.is_empty() || seg == "." || seg == ".." {
                return None;
            }
            cur.push(seg);
        }
        let as_file = cur.with_extension("dec");
        if is_regular_non_symlink(&as_file) {
            return Some(as_file);
        }
        let as_mod = cur.join("mod.dec");
        if is_regular_non_symlink(&as_mod) {
            return Some(as_mod);
        }
        None
    }
}

fn is_regular_non_symlink(p: &Path) -> bool {
    match std::fs::symlink_metadata(p) {
        Ok(m) => m.file_type().is_file() && !m.file_type().is_symlink(),
        Err(_) => false,
    }
}

impl ProviderVTable<RawModuleUnit> for LazyLocalDirProvider {
    fn get_unit(&self, subpath: &[u8]) -> Result<Option<RawModuleUnit>, NsError> {
        let clean = CleanPath::clean_subpath(subpath)?;
        if let Ok(guard) = self.cache.lock() {
            if let Some(cached) = guard.get(&clean) {
                return Ok(cached.as_ref().map(|(src, sha, prog)| RawModuleUnit {
                    kind: RawUnitKind::DecModule {
                        source: Arc::clone(src),
                        sha256: *sha,
                        program: Arc::clone(prog),
                        isolated_ns: None,
                        per_unit_provider_id: Some(*sha),
                    },
                }));
            }
        }

        let Some(file_path) = self.find_candidate_file(&clean) else {
            if let Ok(mut guard) = self.cache.lock() {
                guard.insert(clean, None);
            }
            return Ok(None);
        };
        let text = std::fs::read_to_string(&file_path).map_err(|e| NsError::ProviderFault {
            path: clean.clone(),
            detail: format!("failed reading module: {e}"),
        })?;
        let sha = ContentHash::digest_bytes(text.as_bytes());
        let prog = parse_source(&text).map_err(|e| NsError::ProviderFault {
            path: clean.clone(),
            detail: e.to_string(),
        })?;
        let src_arc: Arc<str> = Arc::from(text.as_str());
        let prog_arc = Arc::new(prog);
        if let Ok(mut guard) = self.cache.lock() {
            guard.insert(
                clean,
                Some((Arc::clone(&src_arc), sha, Arc::clone(&prog_arc))),
            );
        }
        Ok(Some(RawModuleUnit {
            kind: RawUnitKind::DecModule {
                source: src_arc,
                sha256: sha,
                program: prog_arc,
                isolated_ns: None,
                per_unit_provider_id: Some(sha),
            },
        }))
    }
}

/// Host URI resolver supporting `[sha256:<64hex>@]<relative_or_abs_dir>` and `builtin:*`.
pub struct HostUriResolver {
    base_dir: Option<PathBuf>,
}

impl HostUriResolver {
    pub fn new(base_dir: Option<&Path>) -> Self {
        Self {
            base_dir: base_dir.map(Path::to_path_buf),
        }
    }
}

fn parse_stream_uri_spec(
    uri_str: &str,
    mode: MountMode,
    perm: MountPerm,
    bind_mode: BindMode,
    target: &CleanPath,
) -> Result<Option<RtMountSpec>, NsError> {
    let check_static = |scheme: &str| -> Result<(), NsError> {
        if mode == MountMode::Dynamic {
            return Err(NsError::ProviderFault {
                path: uri_str.to_string(),
                detail: format!("{scheme} stream endpoint cannot be mounted as a dynamic module"),
            });
        }
        Ok(())
    };

    if uri_str == "stdio" {
        check_static("stdio")?;
        return Ok(Some(RtMountSpec::Stdio {
            perm,
            bind_mode,
            target: target.clone(),
        }));
    }
    if let Some(addr) = uri_str.strip_prefix("tcp:") {
        check_static("tcp")?;
        if addr.is_empty() {
            return Err(NsError::ProviderFault {
                path: uri_str.to_string(),
                detail: "empty address in tcp:<host>:<port> mount URI".to_string(),
            });
        }
        return Ok(Some(RtMountSpec::TcpConnect {
            perm,
            bind_mode,
            addr: addr.to_string(),
            target: target.clone(),
        }));
    }
    if let Some(addr) = uri_str.strip_prefix("tcp-listen:") {
        check_static("tcp-listen")?;
        if addr.is_empty() {
            return Err(NsError::ProviderFault {
                path: uri_str.to_string(),
                detail: "empty address in tcp-listen:<host>:<port> mount URI".to_string(),
            });
        }
        return Ok(Some(RtMountSpec::TcpListen {
            perm,
            bind_mode,
            addr: addr.to_string(),
            target: target.clone(),
        }));
    }
    if let Some(sock) = uri_str.strip_prefix("unix:") {
        check_static("unix")?;
        if sock.is_empty() {
            return Err(NsError::ProviderFault {
                path: uri_str.to_string(),
                detail: "empty socket path in unix:<path> mount URI".to_string(),
            });
        }
        return Ok(Some(RtMountSpec::UnixConnect {
            perm,
            bind_mode,
            sock_path: PathBuf::from(sock),
            target: target.clone(),
        }));
    }
    if let Some(sock) = uri_str.strip_prefix("unix-listen:") {
        check_static("unix-listen")?;
        if sock.is_empty() {
            return Err(NsError::ProviderFault {
                path: uri_str.to_string(),
                detail: "empty socket path in unix-listen:<path> mount URI".to_string(),
            });
        }
        return Ok(Some(RtMountSpec::UnixListen {
            perm,
            bind_mode,
            sock_path: PathBuf::from(sock),
            target: target.clone(),
        }));
    }
    Ok(None)
}

impl MountUriResolver for HostUriResolver {
    fn resolve_mount_uri(
        &self,
        uri: &[u8],
        mode: MountMode,
        perm: MountPerm,
        bind_mode: BindMode,
        target: &CleanPath,
    ) -> Result<(Provider<CompileProvider>, Option<RtMountSpec>), NsError> {
        if let Some(prov) = resolve_builtin_uri_provider(uri) {
            return Ok((prov, None));
        }
        let uri_str = std::str::from_utf8(uri).map_err(|_| NsError::InvalidUtf8Path)?;
        if let Some(rt_spec) = parse_stream_uri_spec(uri_str, mode, perm, bind_mode, target)? {
            let perm_tag = match perm {
                MountPerm::ReadOnly => b"ro".as_slice(),
                MountPerm::ReadWrite => b"rw".as_slice(),
            };
            let stream_id =
                ContentHash::digest_domain(b"aod.ns.stream_uri.v1", &[uri, perm_tag]);
            let empty_backend: CompileProvider = Arc::new(MemoryCompileProvider {
                units: BTreeMap::new(),
                isolated_ns: None,
            });
            let prov = Provider::new(stream_id, mode, perm, empty_backend);
            return Ok((prov, Some(rt_spec)));
        }
        let (expected_hash, raw_path_s) = if let Some(rest) = uri_str.strip_prefix("sha256:") {
            let (hex_s, p_s) = rest.split_once('@').ok_or_else(|| NsError::ProviderFault {
                path: uri_str.to_string(),
                detail: "expected 'sha256:<64hex>@<path>' in content-addressed mount URI".to_string(),
            })?;
            (Some(ContentHash::from_hex(hex_s)?), p_s)
        } else {
            (None, uri_str)
        };

        let p = Path::new(raw_path_s);
        let resolved_dir = if p.is_absolute() {
            p.to_path_buf()
        } else if let Some(ref b) = self.base_dir {
            b.join(p)
        } else {
            std::env::current_dir()
                .map_err(|e| NsError::ProviderFault {
                    path: raw_path_s.to_string(),
                    detail: format!("cwd error: {e}"),
                })?
                .join(p)
        };

        let meta = std::fs::symlink_metadata(&resolved_dir).map_err(|e| NsError::ProviderFault {
            path: resolved_dir.display().to_string(),
            detail: format!("failed to stat mount source: {e}"),
        })?;
        if meta.file_type().is_symlink() {
            return Err(NsError::CapabilityDenied(format!(
                "refusing to mount symlink {}",
                resolved_dir.display()
            )));
        }
        if meta.is_file() {
            let prov = DirPackageProvider::load_single_dec_file(
                &resolved_dir,
                expected_hash,
                mode,
                perm,
            )?;
            return Ok((prov, None));
        }

        let prov = DirPackageProvider::load_package_dir(&resolved_dir, expected_hash, mode, perm)?;
        let rt_spec = RtMountSpec::HostDir {
            perm,
            bind_mode,
            host_dir: resolved_dir,
            target: target.clone(),
        };
        Ok((prov, Some(rt_spec)))
    }
}

/// Resolve the `stdlib/` directory root strictly according to `P0-2` precedence:
/// 1. Explicit `options.stdlib_dir` (`--stdlib <dir>`) -> use ONLY that path (zero silent fallback).
/// 2. Explicit non-empty `AOD_STDLIB_DIR` env var -> use ONLY that path (zero silent fallback).
/// 3. Upward ancestor walk (up to 6 ancestors from `base_dir`, then `current_dir`) for `<dir>/stdlib/aod/ledger.dec`.
/// 4. Compile-time workspace fallback `<CARGO_MANIFEST_DIR>/../../stdlib` if `aod/ledger.dec` exists.
pub fn resolve_stdlib_root_host(options: &LoadOptions, base_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(ref explicit) = options.stdlib_dir {
        return Some(explicit.clone());
    }
    if let Ok(env_dir) = std::env::var("AOD_STDLIB_DIR") {
        if !env_dir.trim().is_empty() {
            return Some(PathBuf::from(env_dir));
        }
    }

    let mut search_starts = Vec::new();
    if let Some(d) = base_dir {
        search_starts.push(d.to_path_buf());
    }
    if let Ok(cwd) = std::env::current_dir() {
        if !search_starts.contains(&cwd) {
            search_starts.push(cwd);
        }
    }

    for start in search_starts {
        let mut cur: Option<&Path> = Some(start.as_path());
        for _ in 0..=6 {
            let Some(dir) = cur else { break };
            let cand = dir.join("stdlib");
            if cand.join("aod").join("ledger.dec").is_file() {
                return Some(cand);
            }
            cur = dir.parent();
        }
    }

    let manifest_fallback = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(|root| root.join("stdlib"));
    if let Some(ref cand) = manifest_fallback {
        if cand.join("aod").join("ledger.dec").is_file() {
            return Some(cand.clone());
        }
    }
    None
}

/// Construct the default CLI `CompileNamespace` at the host boundary:
/// - Built-ins at `/aod/{core,witness,ledger,wavelet,sim,qext,sys}` and `/sys`.
/// - `stdlib/` (if valid) mounted at `/stdlib` and bound at `/lib/aod` (`bind /stdlib/aod -> /lib/aod`).
/// - `base_dir` (if `Some`) mounted lazily at `"/"` (`After`).
pub fn build_standard_cli_namespace(
    base_dir: Option<&Path>,
    options: &LoadOptions,
) -> CompileNamespace {
    let mut ns = CompileNamespace::empty();
    mount_all_cli_builtins(&mut ns);

    if let Some(stdlib_dir) = resolve_stdlib_root_host(options, base_dir) {
        // stdlib modules themselves use `stdlib::aod::...` (`use stdlib::aod::ledger` inside `sectors.dec`),
        // so load stdlib without an isolated child namespace so stdlib modules share `/stdlib` + `/aod/*`.
        if let Ok(std_prov) = load_unisolated_stdlib_provider(&stdlib_dir) {
            let stdlib_target = CleanPath::parse_str("/stdlib").unwrap();
            ns.mount(std_prov, &stdlib_target, BindMode::Replace);
            let _ = ns.bind(
                &CleanPath::parse_str("/stdlib/aod").unwrap(),
                &CleanPath::parse_str("/lib/aod").unwrap(),
                BindMode::Replace,
            );
        }
    }

    if let Some(dir) = base_dir {
        let local_prov = LazyLocalDirProvider::new(dir.to_path_buf());
        ns.mount(local_prov, &CleanPath::root(), BindMode::After);
    }
    ns
}

fn load_unisolated_stdlib_provider(stdlib_dir: &Path) -> Result<Provider<CompileProvider>, NsError> {
    let canon = std::fs::canonicalize(stdlib_dir).map_err(|e| NsError::ProviderFault {
        path: stdlib_dir.display().to_string(),
        detail: e.to_string(),
    })?;
    let mut raw_files: BTreeMap<String, (String, ContentHash)> = BTreeMap::new();
    collect_package_dec_files(&canon, &canon, 0, &mut raw_files)?;

    let mut merkle_parts: Vec<Vec<u8>> = Vec::with_capacity(raw_files.len());
    let mut units = BTreeMap::new();
    for (rel_dec, (src, sha)) in &raw_files {
        let mut rec = Vec::with_capacity(rel_dec.len() + 33);
        rec.extend_from_slice(rel_dec.as_bytes());
        rec.push(0);
        rec.extend_from_slice(sha.as_bytes());
        merkle_parts.push(rec);

        let prog = parse_source(src).map_err(|e| NsError::ProviderFault {
            path: rel_dec.clone(),
            detail: e.to_string(),
        })?;
        if let Some(stem) = rel_dec.strip_suffix(".dec") {
            units.insert(
                stem.to_string(),
                (Arc::from(src.as_str()), *sha, Arc::new(prog)),
            );
        }
    }
    let refs: Vec<&[u8]> = merkle_parts.iter().map(Vec::as_slice).collect();
    let bundle_id = ContentHash::digest_domain(b"aod.pkg.bundle.v1", &refs);
    let backend: CompileProvider = Arc::new(DirPackageProvider {
        units,
        isolated_ns: None,
    });
    Ok(Provider::new(
        bundle_id,
        MountMode::Static,
        MountPerm::ReadOnly,
        backend,
    ))
}

/// Reconstruct a `CompileNamespace` strictly from embedded `DecIR v3` `bundled_modules`
/// with ZERO ambient filesystem or environment access.
pub fn build_hermetic_v3_namespace(
    bundled_modules: &BTreeMap<String, BundledModule>,
) -> DecResult<CompileNamespace> {
    let mut ns = CompileNamespace::empty();
    mount_all_cli_builtins(&mut ns);

    let mut units = BTreeMap::new();
    for (mod_name, m) in bundled_modules {
        let actual_hex = aod_core::sha256::Sha256::digest_hex(m.source.as_bytes());
        if actual_hex != m.sha256 {
            return Err(Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                format!("bundled module {mod_name:?} SHA-256 mismatch"),
            ));
        }
        let subpath = mod_name.replace("::", "/");
        let sha = ContentHash::digest_bytes(m.source.as_bytes());
        let prog = parse_source(&m.source)?;
        units.insert(
            subpath,
            (Arc::from(m.source.as_str()), sha, Arc::new(prog)),
        );
    }

    struct HermeticV3Provider {
        units: BTreeMap<String, (Arc<str>, ContentHash, Arc<crate::ast::Program>)>,
    }
    impl ProviderVTable<RawModuleUnit> for HermeticV3Provider {
        fn get_unit(&self, subpath: &[u8]) -> Result<Option<RawModuleUnit>, NsError> {
            let clean = CleanPath::clean_subpath(subpath)?;
            let Some((src, sha, prog)) = self.units.get(&clean) else {
                return Ok(None);
            };
            Ok(Some(RawModuleUnit {
                kind: RawUnitKind::DecModule {
                    source: Arc::clone(src),
                    sha256: *sha,
                    program: Arc::clone(prog),
                    isolated_ns: None,
                    per_unit_provider_id: Some(*sha),
                },
            }))
        }
    }

    let id = ContentHash::digest_domain(b"aod.hermetic.v3", &[b"v3"]);
    let backend: CompileProvider = Arc::new(HermeticV3Provider { units });
    ns.mount(
        Provider::new(id, MountMode::Static, MountPerm::ReadOnly, backend),
        &CleanPath::root(),
        BindMode::After,
    );
    Ok(ns)
}

/// Reconstruct an exact `CompileNamespace` for `owner_key` (`"<entry>"` or isolated `unit_key`)
/// strictly from `DecIR v4` `bundled_units` + `unit_ns_tables` with ZERO ambient filesystem access!
pub fn build_hermetic_v4_namespace(
    owner_key: &str,
    bundled_units: &BTreeMap<String, BundledUnitRecord>,
    unit_ns_tables: &BTreeMap<String, Vec<NsMountRecord>>,
) -> DecResult<CompileNamespace> {
    struct HermeticV4Provider {
        units: BTreeMap<String, (Arc<str>, ContentHash, Arc<crate::ast::Program>)>,
    }
    impl ProviderVTable<RawModuleUnit> for HermeticV4Provider {
        fn get_unit(&self, subpath: &[u8]) -> Result<Option<RawModuleUnit>, NsError> {
            let clean = CleanPath::clean_subpath(subpath)?;
            let Some((src, sha, prog)) = self.units.get(&clean) else {
                return Ok(None);
            };
            Ok(Some(RawModuleUnit {
                kind: RawUnitKind::DecModule {
                    source: Arc::clone(src),
                    sha256: *sha,
                    program: Arc::clone(prog),
                    isolated_ns: None,
                    per_unit_provider_id: None,
                },
            }))
        }
    }

    // Group verified units by `provider_id`
    let mut by_provider: BTreeMap<
        ContentHash,
        BTreeMap<String, (Arc<str>, ContentHash, Arc<crate::ast::Program>)>,
    > = BTreeMap::new();

    for (ukey, rec) in bundled_units {
        let actual_hex = aod_core::sha256::Sha256::digest_hex(rec.source.as_bytes());
        if actual_hex != rec.sha256 {
            return Err(Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                format!("unit {ukey:?} SHA-256 mismatch"),
            ));
        }
        let expected_uid = aod_core::ns::UnitId::for_provider_unit(rec.provider_id, &rec.subpath);
        let expected_ukey = format!("u_{}", expected_uid.to_hex());
        if *ukey != expected_ukey || rec.unit_key != expected_ukey {
            return Err(Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                format!("unit {ukey:?} does not match cryptographic UnitId {expected_ukey}"),
            ));
        }
        let sha = ContentHash::digest_bytes(rec.source.as_bytes());
        let prog = parse_source(&rec.source)?;
        by_provider.entry(rec.provider_id).or_default().insert(
            rec.subpath.clone(),
            (Arc::from(rec.source.as_str()), sha, Arc::new(prog)),
        );
    }

    let builtin_by_id: BTreeMap<ContentHash, &'static str> = [
        "core", "witness", "ledger", "wavelet", "sim", "qext", "sys",
    ]
    .into_iter()
    .map(|t| (make_builtin_provider(t).id, t))
    .collect();

    let table = unit_ns_tables.get(owner_key).ok_or_else(|| {
        Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            format!("missing namespace_table for {owner_key:?} in DecIR v4"),
        )
    })?;

    let mut ns = CompileNamespace::empty();
    let mut seen_targets = std::collections::BTreeSet::new();
    for entry in table {
        let target = CleanPath::parse_str(&entry.target).map_err(|e| {
            Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                format!("invalid mount target {:?} in DecIR v4: {e}", entry.target),
            )
        })?;
        let prov = if let Some(&tail) = builtin_by_id.get(&entry.provider_id) {
            let mut p = make_builtin_provider(tail);
            p.mode = entry.mode;
            p.perm = entry.perm;
            p.subpath_prefix = entry.subpath_prefix.clone();
            p
        } else {
            let units = by_provider
                .get(&entry.provider_id)
                .cloned()
                .unwrap_or_default();
            let backend: CompileProvider = Arc::new(HermeticV4Provider { units });
            Provider::new(entry.provider_id, entry.mode, entry.perm, backend)
                .with_subpath_prefix(entry.subpath_prefix.clone())
        };
        let bmode = if seen_targets.insert(entry.target.clone()) {
            BindMode::Replace
        } else {
            BindMode::After
        };
        ns.mount(prov, &target, bmode);
    }

    Ok(ns)
}
