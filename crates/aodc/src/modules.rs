//! Layer 1 & Layer 2 Compiler Module System: Plan 9 `CompileNamespace`, Content-Addressed
//! `UnitId` Tagging, Per-Symbol `BEFORE`/`AFTER` Union Resolution, and Zero Ambient I/O.
//!
//! Zero `std::fs` and zero `std::env` calls exist in this file; all host directory,
//! bundle archive, and environment wiring lives strictly at the boundary in `ns_host.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aod_core::ns::{
    BindMode, CleanPath, ContentHash, MountMode, MountPerm, Namespace, NsError, Provider,
    ProviderVTable, UnitId,
};
use aod_core::sha256::Sha256;
use aod_rt::ns_fs::RtMountSpec;

use crate::ast::{Item, NamespaceDecl, NsMountSource, NsOp, Program, UseDecl};
use crate::diagnostics::{DecResult, Diagnostic, DiagnosticCode};
use crate::ns_host::{
    build_hermetic_v3_namespace, build_hermetic_v4_namespace, build_standard_cli_namespace,
    resolve_stdlib_root_host, HostUriResolver,
};
use crate::parser::parse_source;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LoadOptions {
    /// Explicit `--stdlib <dir>` override pointing to the `stdlib/` directory itself
    /// (containing `aod/ledger.dec`). When set, NO silent fallback is permitted (`P0-2`).
    pub stdlib_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BundledModule {
    pub source: String,
    pub sha256: String,
}

/// Content-addressed unit record serialized into `DecIR v4`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BundledUnitRecord {
    pub unit_key: String,
    pub provider_id: ContentHash,
    pub subpath: String,
    pub source: String,
    pub sha256: String,
}

/// Canonical resolved Plan 9 mount entry serialized into `DecIR v4` for hermetic replay.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NsMountRecord {
    pub target: String,
    pub mode: MountMode,
    pub perm: MountPerm,
    pub provider_id: ContentHash,
    pub subpath_prefix: String,
}

/// Recorded `mount dyn` import binding lowered to indirect Capability VTable dispatch in Phase N4.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DynamicImportRef {
    pub requester_mod: String,
    pub mount_path: String,
    pub unit_key: String,
    pub item: String,
}

/// Raw module payload returned by a `CompileProvider` (`ProviderVTable<RawModuleUnit>`).
#[derive(Clone, Debug)]
pub enum RawUnitKind {
    Builtin(&'static str),
    DecModule {
        source: Arc<str>,
        sha256: ContentHash,
        program: Arc<Program>,
        isolated_ns: Option<CompileNamespace>,
        per_unit_provider_id: Option<ContentHash>,
    },
}

#[derive(Clone, Debug)]
pub struct RawModuleUnit {
    pub kind: RawUnitKind,
}

pub type CompileProvider = Arc<dyn ProviderVTable<RawModuleUnit>>;
pub type CompileNamespace = Namespace<CompileProvider>;

/// Boundary resolver hook invoked ONLY when executing a `mount ... b"<uri>" -> /target` directive
/// inside a `namespace <name>:` block.
pub trait MountUriResolver: Send + Sync {
    fn resolve_mount_uri(
        &self,
        uri: &[u8],
        mode: MountMode,
        perm: MountPerm,
        bind_mode: BindMode,
        target: &CleanPath,
    ) -> Result<(Provider<CompileProvider>, Option<RtMountSpec>), NsError>;
}

pub struct NoopUriResolver;

impl MountUriResolver for NoopUriResolver {
    fn resolve_mount_uri(
        &self,
        uri: &[u8],
        _mode: MountMode,
        _perm: MountPerm,
        _bind_mode: BindMode,
        target: &CleanPath,
    ) -> Result<(Provider<CompileProvider>, Option<RtMountSpec>), NsError> {
        // Builtin capability URI `b"builtin:aod/sys"` or `b"builtin:aod/qext"` works hermetically!
        if let Some(prov) = resolve_builtin_uri_provider(uri) {
            return Ok((prov, None));
        }
        Err(NsError::CapabilityDenied(format!(
            "external URI mount {:?} -> {} is unavailable in pure hermetic mode",
            String::from_utf8_lossy(uri),
            target
        )))
    }
}

/// Complete hermetic compilation unit containing the entry module plus all transitively
/// resolved units keyed internally by cryptographic `UnitId` (`"u_<64hex>"`), decoupling
/// nominal type and LLVM symbol identity from lexical mount strings (`/calc`).
#[derive(Clone, Debug)]
pub struct LoadedUnit {
    pub entry_source: String,
    pub entry_sha256: String,
    pub entry_program: Program,
    /// Legacy path-keyed map preserved for byte-identical `DecIR v3` emission on v3 sources.
    pub bundled_modules: BTreeMap<String, BundledModule>,
    /// Content-addressed units keyed by `"u_<unit_id_hex>"`.
    pub bundled_units: BTreeMap<String, BundledUnitRecord>,
    /// Parsed ASTs keyed by `"u_<unit_id_hex>"`.
    pub parsed_modules: BTreeMap<String, Program>,
    /// Post-order DFS dependency load order of `"u_<unit_id_hex>"` keys (`P0-1`).
    pub module_load_order: Vec<String>,
    pub entry_imports: BTreeMap<String, ImportedSymbol>,
    pub module_imports: BTreeMap<String, BTreeMap<String, ImportedSymbol>>,
    /// `true` when the program uses `namespace <name>:`, rooted `/path` imports, `--namespace`,
    /// or `mount dyn` capabilities (selecting `DecIR v4`).
    pub uses_plan9_ns: bool,
    /// Per-context resolved Plan 9 mount tables (`"<entry>"` and any isolated bundle `unit_key`)
    /// enabling 100% zero-filesystem hermetic rebuild of `DecIR v4`.
    pub unit_ns_tables: BTreeMap<String, Vec<NsMountRecord>>,
    /// Imported items originating from `MountMode::Dynamic` (`mount dyn`) capabilities.
    pub dynamic_imports: Vec<DynamicImportRef>,
    /// Runtime `RtNamespace` mounts declared in the entry's `namespace` block.
    pub declared_rt_mounts: Vec<RtMountSpec>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportedSymbol {
    Builtin(String),
    ModuleItem { module: String, item: String },
    ModuleNamespace(String),
}

struct BuiltinNsProvider {
    tail: &'static str,
}

impl ProviderVTable<RawModuleUnit> for BuiltinNsProvider {
    fn get_unit(&self, subpath: &[u8]) -> Result<Option<RawModuleUnit>, NsError> {
        let clean = CleanPath::clean_subpath(subpath)?;
        if clean.is_empty() {
            Ok(Some(RawModuleUnit {
                kind: RawUnitKind::Builtin(self.tail),
            }))
        } else {
            Ok(None)
        }
    }
}

pub fn make_builtin_provider(tail: &'static str) -> Provider<CompileProvider> {
    let id = ContentHash::digest_domain(b"aod.builtin.provider.v1", &[tail.as_bytes()]);
    let backend: CompileProvider = Arc::new(BuiltinNsProvider { tail });
    Provider::new(id, MountMode::Static, MountPerm::ReadOnly, backend)
}

pub fn resolve_builtin_uri_provider(uri: &[u8]) -> Option<Provider<CompileProvider>> {
    match uri {
        b"builtin:aod/core" => Some(make_builtin_provider("core")),
        b"builtin:aod/witness" => Some(make_builtin_provider("witness")),
        b"builtin:aod/ledger" => Some(make_builtin_provider("ledger")),
        b"builtin:aod/wavelet" => Some(make_builtin_provider("wavelet")),
        b"builtin:aod/sim" => Some(make_builtin_provider("sim")),
        b"builtin:aod/qext" => Some(make_builtin_provider("qext")),
        b"builtin:aod/sys" | b"builtin:sys" => Some(make_builtin_provider("sys")),
        _ => None,
    }
}

/// Mount pure deterministic AOD kernel built-ins (`/aod/{core,witness,ledger,wavelet,sim}`),
/// deliberately omitting `/aod/qext` and `/aod/sys` for isolated `clear` / package namespaces.
pub fn mount_pure_aod_builtins(ns: &mut CompileNamespace) {
    for (path_s, tail) in [
        ("/aod/core", "core"),
        ("/aod/witness", "witness"),
        ("/aod/ledger", "ledger"),
        ("/aod/wavelet", "wavelet"),
        ("/aod/sim", "sim"),
    ] {
        let target = CleanPath::parse_str(path_s).expect("valid static builtin path");
        ns.mount(make_builtin_provider(tail), &target, BindMode::Replace);
    }
}

/// Mount all standard CLI built-ins (pure AOD built-ins + `/aod/qext` + `/aod/sys` + `/sys`).
pub fn mount_all_cli_builtins(ns: &mut CompileNamespace) {
    mount_pure_aod_builtins(ns);
    for (path_s, tail) in [
        ("/aod/qext", "qext"),
        ("/aod/sys", "sys"),
        ("/sys", "sys"),
    ] {
        let target = CleanPath::parse_str(path_s).expect("valid static builtin path");
        ns.mount(make_builtin_provider(tail), &target, BindMode::Replace);
    }
}

pub fn builtin_tail_items(tail: &str) -> Option<&'static [&'static str]> {
    match tail {
        "core" => Some(&[
            "t1",
            "q4",
            "b8",
            "b64",
            "b64_bits",
            "u64_int",
            "int",
            "ratio",
            "bytes",
            "u16_le_bytes",
            "u32_le_bytes",
            "i32_le_bytes",
            "i64_le_bytes",
            "b64_le_bytes",
            "push_byte",
            "concat_bytes",
            "slice_bytes",
            "sha256_bytes",
            "hex_bytes",
            "pack_ratio",
            "unpack_ratio",
            "pack",
            "unpack",
            "unpack_t9",
            "num",
            "den",
            "neg",
            "abs",
            "sgn",
            "signum",
            "min",
            "max",
            "clamp",
            "pow",
            "gcd",
            "lcm",
            "trit_shl",
            "trit_shr",
            "superpose_cancel",
            "bitnot",
            "popcnt",
            "popcnt_xor",
            "is_hamming1",
            "verify_sheddic_route",
            "audit_sheddic_route",
            "verify_frg_cross_return",
            "mux",
            "select",
            "gate",
            "argmin",
            "argmax",
            "walk",
            "settle",
            "fold",
            "evolve",
            "scan",
            "fold_seq",
            "map_seq",
            "filter_seq",
            "tabulate",
            "zip_seq",
            "count",
            "any",
            "all",
            "sum",
            "dot",
            "each",
            "range_seq",
            "array",
            "array_set",
            "push",
            "concat",
            "slice",
            "set_at",
            "list_set",
            "list_push",
            "concat_seq",
            "slice_seq",
            "sort_int_pairs",
            "map_put",
            "map_remove",
            "map_get_or",
            "map_keys",
            "map_entries",
            "addr8",
            "support",
            "support_pi",
            "support_rd",
            "support_pressure",
            "support_overhang",
            "boundary_of",
            "len",
            "contains",
            "ModelStatus",
            "RecipientStage",
        ]),
        "witness" => Some(&[
            "trace", "step", "hinge", "dwell", "close", "phases", "beats", "rate", "fold",
            "outer", "fuse", "coupling", "contact", "cadence", "lag", "drift",
        ]),
        "ledger" => Some(&[
            "probability",
            "sadar_promote",
            "shedding_excess",
            "verify_sheddic_route",
            "audit_sheddic_route",
            "verify_frg_cross_return",
        ]),
        "wavelet" => Some(&[
            "window_dedup_count",
            "native_cadence_ratio",
            "single_contact_association",
            "recipient_eval_demo",
            "RecipientStage",
        ]),
        "sim" => Some(&["boundary_of", "ModelStatus"]),
        "qext" => Some(&[
            "ext_decimal",
            "qext_from_ratio",
            "qext_add",
            "qext_sub",
            "qext_mul",
            "qext_div",
            "qext_render",
            "qext_round_half_even",
            "qext_format_fixed",
            "qext_to_bytes_exact",
            "qext_format_bytes_fixed",
            "qext_render_bytes",
            "qext_format_fixed_bytes",
        ]),
        "sys" => Some(&[
            "cli_args",
            "read_file_bytes",
            "write_executable_elf",
            "accept_stream",
            "stream_local_addr",
            "read_stream_line",
            "read_stream_frame",
            "write_stream_bytes",
            "write_stream_frame",
        ]),
        _ => None,
    }
}

pub fn builtin_namespace_items(namespace: &[String]) -> Option<&'static [&'static str]> {
    if namespace.len() == 2 && namespace[0] == "aod" {
        builtin_tail_items(&namespace[1])
    } else if namespace.len() == 1 && namespace[0] == "sys" {
        builtin_tail_items("sys")
    } else {
        None
    }
}

pub fn canonical_tail_builtin_name(tail: &str, item: &str) -> String {
    match tail {
        "qext" => format!("aod::qext::{item}"),
        "sys" => format!("aod::sys::{item}"),
        _ => item.to_string(),
    }
}

pub fn canonical_builtin_name(namespace: &[String], item: &str) -> String {
    if namespace.len() == 2 && namespace[0] == "aod" {
        canonical_tail_builtin_name(&namespace[1], item)
    } else if namespace.len() == 1 && namespace[0] == "sys" {
        canonical_tail_builtin_name("sys", item)
    } else {
        item.to_string()
    }
}

pub fn resolve_stdlib_root(options: &LoadOptions, base_dir: Option<&Path>) -> Option<PathBuf> {
    resolve_stdlib_root_host(options, base_dir)
}

pub fn load_from_source(entry_source: &str, base_dir: Option<&Path>) -> DecResult<LoadedUnit> {
    load_from_source_with_options(entry_source, base_dir, &LoadOptions::default())
}

pub fn load_from_source_with_options(
    entry_source: &str,
    base_dir: Option<&Path>,
    options: &LoadOptions,
) -> DecResult<LoadedUnit> {
    let base_ns = build_standard_cli_namespace(base_dir, options);
    let uri_resolver = HostUriResolver::new(base_dir);
    load_with_namespace_and_resolver(entry_source, &base_ns, &uri_resolver, false)
}

pub fn load_with_external_namespace_source(
    entry_source: &str,
    bootstrap_ns_source: &str,
    entry_base_dir: Option<&Path>,
    ns_base_dir: Option<&Path>,
    options: &LoadOptions,
) -> DecResult<LoadedUnit> {
    let mut base_ns = build_standard_cli_namespace(entry_base_dir, options);
    let ns_prog = parse_source(bootstrap_ns_source)?;
    let ns_resolver = HostUriResolver::new(ns_base_dir);
    let mut rt_mounts = Vec::new();
    let mut saw_ns = false;
    for item in &ns_prog.items {
        match item {
            Item::Namespace(decl) => {
                saw_ns = true;
                apply_namespace_decl(&mut base_ns, &mut rt_mounts, decl, &ns_resolver)?;
            }
            _ => {
                return Err(Diagnostic::new(
                    DiagnosticCode::ParseError,
                    "bootstrap --namespace file may only contain `namespace <name>:` declarations",
                ));
            }
        }
    }
    if !saw_ns {
        return Err(Diagnostic::new(
            DiagnosticCode::ParseError,
            "bootstrap --namespace file must declare at least one `namespace <name>:` block",
        ));
    }
    let entry_resolver = HostUriResolver::new(entry_base_dir);
    let mut loaded = load_with_namespace_and_resolver(entry_source, &base_ns, &entry_resolver, true)?;
    if loaded.declared_rt_mounts.is_empty() {
        loaded.declared_rt_mounts = rt_mounts;
    }
    Ok(loaded)
}

pub fn load_with_namespace(entry_source: &str, ns: &CompileNamespace) -> DecResult<LoadedUnit> {
    load_with_namespace_and_resolver(entry_source, ns, &NoopUriResolver, true)
}

pub fn load_hermetic(
    entry_source: &str,
    bundled_modules: &BTreeMap<String, BundledModule>,
) -> DecResult<LoadedUnit> {
    let ns = build_hermetic_v3_namespace(bundled_modules)?;
    load_with_namespace_and_resolver(entry_source, &ns, &NoopUriResolver, false)
}

pub fn load_hermetic_v4(
    entry_source: &str,
    bundled_units: &BTreeMap<String, BundledUnitRecord>,
    unit_ns_tables: &BTreeMap<String, Vec<NsMountRecord>>,
) -> DecResult<LoadedUnit> {
    let entry_ns = build_hermetic_v4_namespace("<entry>", bundled_units, unit_ns_tables)?;
    load_with_namespace_and_v4_tables(entry_source, &entry_ns, Some((bundled_units, unit_ns_tables)), &NoopUriResolver, true)
}

pub fn snapshot_ns_table(ns: &CompileNamespace) -> Vec<NsMountRecord> {
    let mut out = Vec::new();
    for (target, stack) in ns.entries() {
        for prov in stack {
            out.push(NsMountRecord {
                target: target.as_str().to_string(),
                mode: prov.mode,
                perm: prov.perm,
                provider_id: prov.id,
                subpath_prefix: prov.subpath_prefix.clone(),
            });
        }
    }
    out
}

pub fn apply_namespace_decl(
    ns: &mut CompileNamespace,
    rt_mounts: &mut Vec<RtMountSpec>,
    decl: &NamespaceDecl,
    resolver: &dyn MountUriResolver,
) -> DecResult<()> {
    for op in &decl.ops {
        match op {
            NsOp::Clear { .. } => {
                ns.clear();
                mount_pure_aod_builtins(ns);
                rt_mounts.clear();
            }
            NsOp::Fork { .. } => {
                *ns = ns.fork(false);
            }
            NsOp::Mount {
                mode,
                perm,
                bind_mode,
                source,
                target,
                line,
            } => {
                let target_clean = clean_path_from_segs(target, *line)?;
                match source {
                    NsMountSource::RamFs => {
                        rt_mounts.push(RtMountSpec::RamFs {
                            bind_mode: *bind_mode,
                            target: target_clean,
                        });
                    }
                    NsMountSource::ExistingPath(existing_segs) => {
                        let existing_clean = clean_path_from_segs(existing_segs, *line)?;
                        if *mode == MountMode::Dynamic {
                            let resolved = ns.resolve(&existing_clean);
                            if resolved.is_empty() {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::ModuleNotFound,
                                    *line,
                                    format!(
                                        "cannot mount dyn from unmounted path {}",
                                        existing_clean
                                    ),
                                ));
                            }
                            let top = &resolved[0];
                            let dyn_prov = Provider::new(
                                top.provider.id,
                                MountMode::Dynamic,
                                MountPerm::ReadOnly,
                                top.provider.backend.clone(),
                            )
                            .with_subpath_prefix(top.subpath.clone());
                            ns.mount(dyn_prov, &target_clean, *bind_mode);
                        } else {
                            ns.bind(&existing_clean, &target_clean, *bind_mode)
                                .map_err(|e| {
                                    Diagnostic::at_line(
                                        DiagnosticCode::ModuleNotFound,
                                        *line,
                                        e.to_string(),
                                    )
                                })?;
                        }
                    }
                    NsMountSource::BytesUri(uri) => {
                        let (prov, opt_rt) = resolver
                            .resolve_mount_uri(uri, *mode, *perm, *bind_mode, &target_clean)
                            .map_err(|e| {
                                Diagnostic::at_line(
                                    DiagnosticCode::ModuleNotFound,
                                    *line,
                                    e.to_string(),
                                )
                            })?;
                        ns.mount(prov, &target_clean, *bind_mode);
                        if let Some(rt_spec) = opt_rt {
                            rt_mounts.push(rt_spec);
                        }
                    }
                }
            }
            NsOp::Bind {
                mode,
                existing,
                target,
                line,
            } => {
                let existing_clean = clean_path_from_segs(existing, *line)?;
                let target_clean = clean_path_from_segs(target, *line)?;
                ns.bind(&existing_clean, &target_clean, *mode).map_err(|e| {
                    Diagnostic::at_line(DiagnosticCode::ModuleNotFound, *line, e.to_string())
                })?;
            }
            NsOp::Unmount { target, line } => {
                let target_clean = clean_path_from_segs(target, *line)?;
                let _ = ns.unmount(&target_clean, None);
            }
        }
    }
    Ok(())
}

pub fn load_with_namespace_and_resolver(
    entry_source: &str,
    initial_ns: &CompileNamespace,
    resolver: &dyn MountUriResolver,
    force_plan9_ns: bool,
) -> DecResult<LoadedUnit> {
    load_with_namespace_and_v4_tables(entry_source, initial_ns, None, resolver, force_plan9_ns)
}

fn load_with_namespace_and_v4_tables(
    entry_source: &str,
    initial_ns: &CompileNamespace,
    hermetic_v4: Option<(
        &BTreeMap<String, BundledUnitRecord>,
        &BTreeMap<String, Vec<NsMountRecord>>,
    )>,
    resolver: &dyn MountUriResolver,
    force_plan9_ns: bool,
) -> DecResult<LoadedUnit> {
    let entry_program = parse_source(entry_source)?;
    let entry_sha256 = Sha256::digest_hex(entry_source.as_bytes());

    let mut entry_ns = initial_ns.fork(false);
    let mut declared_rt_mounts = Vec::new();
    let mut uses_plan9_ns = force_plan9_ns;

    for item in &entry_program.items {
        match item {
            Item::Namespace(decl) => {
                uses_plan9_ns = true;
                // In pure hermetic v4 replay, `initial_ns` already encodes the exact resolved mounts!
                if hermetic_v4.is_none() {
                    apply_namespace_decl(&mut entry_ns, &mut declared_rt_mounts, decl, resolver)?;
                }
            }
            Item::Use(u) if u.rooted_slash => {
                uses_plan9_ns = true;
            }
            _ => {}
        }
    }

    let mut unit_ns_tables = BTreeMap::new();
    unit_ns_tables.insert("<entry>".to_string(), snapshot_ns_table(&entry_ns));

    let mut loader = ModuleLoader {
        uses_plan9_ns,
        hermetic_v4,
        bundled_modules: BTreeMap::new(),
        bundled_units: BTreeMap::new(),
        parsed_modules: BTreeMap::new(),
        module_load_order: Vec::new(),
        module_imports: BTreeMap::new(),
        unit_ns_tables,
        dynamic_imports: Vec::new(),
        visiting: Vec::new(),
        visited: BTreeSet::new(),
    };

    let entry_imports = loader.resolve_program_uses(&entry_program, "<entry>", &entry_ns)?;
    if !loader.dynamic_imports.is_empty() {
        loader.uses_plan9_ns = true;
    }

    Ok(LoadedUnit {
        entry_source: entry_source.to_string(),
        entry_sha256,
        entry_program,
        bundled_modules: loader.bundled_modules,
        bundled_units: loader.bundled_units,
        parsed_modules: loader.parsed_modules,
        module_load_order: loader.module_load_order,
        entry_imports,
        module_imports: loader.module_imports,
        uses_plan9_ns: loader.uses_plan9_ns,
        unit_ns_tables: loader.unit_ns_tables,
        dynamic_imports: loader.dynamic_imports,
        declared_rt_mounts,
    })
}

fn clean_path_from_segs(segs: &[String], line: usize) -> DecResult<CleanPath> {
    if segs.is_empty() {
        return Ok(CleanPath::root());
    }
    let raw = format!("/{}", segs.join("/"));
    CleanPath::parse_str(&raw)
        .map_err(|e| Diagnostic::at_line(DiagnosticCode::ParseError, line, e.to_string()))
}

fn program_exports_symbol(prog: &Program, sym: &str) -> bool {
    prog.items.iter().any(|it| match it {
        Item::Const(c) => c.name == sym,
        Item::Struct(s) => s.name == sym,
        Item::Enum(e) => e.name == sym,
        Item::DecFn(f) => f.name == sym,
        Item::Chamber(c) => c.name == sym,
        Item::Prepare(p) => p.name == sym,
        Item::Observe(o) => o.name == sym,
        Item::QuarantineMod(q) => q.name == sym,
        Item::Use(_) | Item::Namespace(_) => false,
    })
}

struct ResolvedDecLayer {
    mount_mode: MountMode,
    provider_id: ContentHash,
    subpath: String,
    source: Arc<str>,
    sha256: ContentHash,
    program: Arc<Program>,
    isolated_ns: Option<CompileNamespace>,
}

impl ResolvedDecLayer {
    fn unit_id(&self) -> UnitId {
        UnitId::for_provider_unit(self.provider_id, &self.subpath)
    }

    fn unit_key(&self) -> String {
        format!("u_{}", self.unit_id().to_hex())
    }
}

struct ModuleLoader<'h> {
    uses_plan9_ns: bool,
    hermetic_v4: Option<(
        &'h BTreeMap<String, BundledUnitRecord>,
        &'h BTreeMap<String, Vec<NsMountRecord>>,
    )>,
    bundled_modules: BTreeMap<String, BundledModule>,
    bundled_units: BTreeMap<String, BundledUnitRecord>,
    parsed_modules: BTreeMap<String, Program>,
    module_load_order: Vec<String>,
    module_imports: BTreeMap<String, BTreeMap<String, ImportedSymbol>>,
    unit_ns_tables: BTreeMap<String, Vec<NsMountRecord>>,
    dynamic_imports: Vec<DynamicImportRef>,
    visiting: Vec<String>,
    visited: BTreeSet<String>,
}

impl<'h> ModuleLoader<'h> {
    fn resolve_program_uses(
        &mut self,
        program: &Program,
        current_mod: &str,
        active_ns: &CompileNamespace,
    ) -> DecResult<BTreeMap<String, ImportedSymbol>> {
        let mut imports = BTreeMap::new();
        for item in &program.items {
            if let Item::Use(u) = item {
                if u.rooted_slash {
                    self.uses_plan9_ns = true;
                }
                self.resolve_one_use(u, current_mod, active_ns, &mut imports)?;
            }
        }
        Ok(imports)
    }

    fn lookup_layers_at(
        &self,
        ns: &CompileNamespace,
        path: &CleanPath,
        line: usize,
    ) -> DecResult<(Option<&'static str>, Vec<ResolvedDecLayer>)> {
        let candidates = ns.resolve(path);
        let mut layers = Vec::new();
        for cand in candidates {
            let opt_unit = cand
                .provider
                .backend
                .get_unit(cand.subpath.as_bytes())
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::ModuleNotFound, line, e.to_string()))?;
            if let Some(raw) = opt_unit {
                match raw.kind {
                    RawUnitKind::Builtin(tail) => {
                        if layers.is_empty() {
                            return Ok((Some(tail), Vec::new()));
                        }
                    }
                    RawUnitKind::DecModule {
                        source,
                        sha256,
                        program,
                        isolated_ns,
                        per_unit_provider_id,
                    } => {
                        layers.push(ResolvedDecLayer {
                            mount_mode: cand.provider.mode,
                            provider_id: per_unit_provider_id.unwrap_or(cand.provider.id),
                            subpath: cand.subpath,
                            source,
                            sha256,
                            program,
                            isolated_ns,
                        });
                    }
                }
            }
        }
        Ok((None, layers))
    }

    fn resolve_one_use(
        &mut self,
        u: &UseDecl,
        current_mod: &str,
        active_ns: &CompileNamespace,
        imports: &mut BTreeMap<String, ImportedSymbol>,
    ) -> DecResult<()> {
        if u.path.is_empty() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::ParseError,
                u.line,
                "empty use path",
            ));
        }

        // Preserve legacy `use aod::...` exact diagnostics when `!u.rooted_slash`
        if !u.rooted_slash && u.path[0] == "aod" && u.path.len() > 3 {
            return Err(Diagnostic::at_line(
                DiagnosticCode::ModuleNotFound,
                u.line,
                format!("invalid built-in use path {}", u.path.join("::")),
            ));
        }
        if !u.rooted_slash && u.path[0] == "stdlib" && u.path.len() < 2 {
            return Err(Diagnostic::at_line(
                DiagnosticCode::ModuleNotFound,
                u.line,
                "stdlib import requires at least one submodule segment (e.g. `use stdlib::aod::ledger`)",
            ));
        }

        let full_clean = clean_path_from_segs(&u.path, u.line)?;
        let display_path = if u.rooted_slash {
            full_clean.as_str().to_string()
        } else {
            u.path.join("::")
        };

        if let Some(items) = &u.items {
            let (opt_builtin, layers) = self.lookup_layers_at(active_ns, &full_clean, u.line)?;
            if let Some(tail) = opt_builtin {
                let allowed = builtin_tail_items(tail).ok_or_else(|| {
                    Diagnostic::at_line(
                        DiagnosticCode::ModuleNotFound,
                        u.line,
                        format!("unknown built-in namespace {display_path}"),
                    )
                })?;
                for sym in items {
                    if !allowed.contains(&sym.as_str()) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ModuleNotFound,
                            u.line,
                            format!("symbol {sym} not found in {display_path}"),
                        ));
                    }
                    imports.insert(
                        sym.clone(),
                        ImportedSymbol::Builtin(canonical_tail_builtin_name(tail, sym)),
                    );
                }
                return Ok(());
            }

            if layers.is_empty() {
                let msg = if !u.rooted_slash && u.path[0] == "aod" {
                    format!("unknown built-in namespace {display_path}")
                } else {
                    format!(
                        "module {display_path:?} (imported by {current_mod}) not found in namespace"
                    )
                };
                return Err(Diagnostic::at_line(DiagnosticCode::ModuleNotFound, u.line, msg));
            }

            let legacy_key = if !u.rooted_slash && layers.len() == 1 {
                Some(u.path.join("::"))
            } else {
                None
            };

            for sym in items {
                let chosen = layers
                    .iter()
                    .find(|l| program_exports_symbol(&l.program, sym))
                    .unwrap_or(&layers[0]);
                let unit_key =
                    self.ensure_unit_loaded(chosen, legacy_key.as_deref(), active_ns, u.line)?;
                if chosen.mount_mode == MountMode::Dynamic {
                    self.dynamic_imports.push(DynamicImportRef {
                        requester_mod: current_mod.to_string(),
                        mount_path: full_clean.as_str().to_string(),
                        unit_key: unit_key.clone(),
                        item: sym.clone(),
                    });
                }
                imports.insert(
                    sym.clone(),
                    ImportedSymbol::ModuleItem {
                        module: unit_key,
                        item: sym.clone(),
                    },
                );
            }
            return Ok(());
        }

        // `u.items == None`: either namespace import (`use /calc`, `use aod::core`, `use stdlib::aod::ledger`)
        // or single-symbol path import (`use /calc/add`, `use aod::core::int`, `use stdlib::aod::ledger::RoleSums`).
        let (opt_builtin, layers) = self.lookup_layers_at(active_ns, &full_clean, u.line)?;
        if let Some(tail) = opt_builtin {
            let alias = u.path.last().cloned().unwrap();
            imports.insert(
                alias,
                ImportedSymbol::ModuleNamespace(format!("aod::{tail}")),
            );
            return Ok(());
        }
        if !layers.is_empty() {
            let legacy_key = if !u.rooted_slash && layers.len() == 1 {
                Some(u.path.join("::"))
            } else {
                None
            };
            let top_key =
                self.ensure_unit_loaded(&layers[0], legacy_key.as_deref(), active_ns, u.line)?;
            let alias = u.path.last().cloned().unwrap();
            imports.insert(alias, ImportedSymbol::ModuleNamespace(top_key));
            return Ok(());
        }

        if u.path.len() >= 2 {
            let parent_segs = &u.path[..u.path.len() - 1];
            let sym = u.path.last().cloned().unwrap();
            if !u.rooted_slash
                && parent_segs.first().map(String::as_str) == Some("stdlib")
                && parent_segs.len() < 2
            {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ModuleNotFound,
                    u.line,
                    format!("stdlib module {display_path:?} not found"),
                ));
            }
            let parent_clean = clean_path_from_segs(parent_segs, u.line)?;
            let (p_builtin, p_layers) = self.lookup_layers_at(active_ns, &parent_clean, u.line)?;
            if let Some(tail) = p_builtin {
                let allowed = builtin_tail_items(tail).ok_or_else(|| {
                    Diagnostic::at_line(
                        DiagnosticCode::ModuleNotFound,
                        u.line,
                        format!("unknown built-in namespace {}", parent_segs.join("::")),
                    )
                })?;
                if !allowed.contains(&sym.as_str()) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ModuleNotFound,
                        u.line,
                        format!("symbol {sym} not found in {}", parent_segs.join("::")),
                    ));
                }
                imports.insert(
                    sym.clone(),
                    ImportedSymbol::Builtin(canonical_tail_builtin_name(tail, &sym)),
                );
                return Ok(());
            }
            if !p_layers.is_empty() {
                let legacy_key = if !u.rooted_slash && p_layers.len() == 1 {
                    Some(parent_segs.join("::"))
                } else {
                    None
                };
                let chosen = p_layers
                    .iter()
                    .find(|l| program_exports_symbol(&l.program, &sym))
                    .unwrap_or(&p_layers[0]);
                let unit_key =
                    self.ensure_unit_loaded(chosen, legacy_key.as_deref(), active_ns, u.line)?;
                if chosen.mount_mode == MountMode::Dynamic {
                    self.dynamic_imports.push(DynamicImportRef {
                        requester_mod: current_mod.to_string(),
                        mount_path: parent_clean.as_str().to_string(),
                        unit_key: unit_key.clone(),
                        item: sym.clone(),
                    });
                }
                imports.insert(
                    sym.clone(),
                    ImportedSymbol::ModuleItem {
                        module: unit_key,
                        item: sym,
                    },
                );
                return Ok(());
            }
        }

        let msg = if !u.rooted_slash && u.path[0] == "aod" {
            if u.path.len() == 3 {
                format!("unknown built-in namespace {}", u.path[..2].join("::"))
            } else {
                format!("unknown built-in namespace {display_path}")
            }
        } else {
            format!("module {display_path:?} (imported by {current_mod}) not found in namespace")
        };
        Err(Diagnostic::at_line(DiagnosticCode::ModuleNotFound, u.line, msg))
    }

    fn ensure_unit_loaded(
        &mut self,
        layer: &ResolvedDecLayer,
        legacy_mod_key: Option<&str>,
        requester_ns: &CompileNamespace,
        line: usize,
    ) -> DecResult<String> {
        let unit_key = layer.unit_key();

        if let Some(lkey) = legacy_mod_key {
            self.bundled_modules.entry(lkey.to_string()).or_insert_with(|| BundledModule {
                source: layer.source.to_string(),
                sha256: layer.sha256.to_hex(),
            });
        }

        if self.visiting.iter().any(|m| m == &unit_key) {
            let mut chain = self.visiting.clone();
            chain.push(unit_key);
            return Err(Diagnostic::at_line(
                DiagnosticCode::ModuleCycleError,
                line,
                format!("cyclic module import detected: {}", chain.join(" -> ")),
            ));
        }
        if self.visited.contains(&unit_key) {
            return Ok(unit_key);
        }

        let child_ns = if let Some((v4_units, v4_tables)) = self.hermetic_v4 {
            if v4_tables.contains_key(&unit_key) {
                let rebuilt_cns = build_hermetic_v4_namespace(&unit_key, v4_units, v4_tables)?;
                self.unit_ns_tables
                    .insert(unit_key.clone(), snapshot_ns_table(&rebuilt_cns));
                rebuilt_cns
            } else {
                requester_ns.fork(false)
            }
        } else if let Some(ref iso) = layer.isolated_ns {
            self.unit_ns_tables
                .insert(unit_key.clone(), snapshot_ns_table(iso));
            iso.fork(false)
        } else {
            requester_ns.fork(false)
        };

        self.visiting.push(unit_key.clone());
        let mod_imports = self.resolve_program_uses(&layer.program, &unit_key, &child_ns)?;
        self.visiting.pop();

        self.visited.insert(unit_key.clone());
        self.module_load_order.push(unit_key.clone());
        self.bundled_units.insert(
            unit_key.clone(),
            BundledUnitRecord {
                unit_key: unit_key.clone(),
                provider_id: layer.provider_id,
                subpath: layer.subpath.clone(),
                source: layer.source.to_string(),
                sha256: layer.sha256.to_hex(),
            },
        );
        self.parsed_modules
            .insert(unit_key.clone(), (*layer.program).clone());
        self.module_imports.insert(unit_key.clone(), mod_imports);

        Ok(unit_key)
    }
}
