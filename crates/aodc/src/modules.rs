use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use aod_core::sha256::Sha256;

use crate::ast::{Item, Program, UseDecl};
use crate::diagnostics::{DecResult, Diagnostic, DiagnosticCode};
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

/// Complete hermetic compilation unit containing the entry module plus all transitively
/// resolved filesystem/stdlib modules (with exact sources, SHA-256 digests, and
/// post-order topological `module_load_order` for `const` initialization, `P0-1`).
#[derive(Clone, Debug)]
pub struct LoadedUnit {
    pub entry_source: String,
    pub entry_sha256: String,
    pub entry_program: Program,
    pub bundled_modules: BTreeMap<String, BundledModule>,
    pub parsed_modules: BTreeMap<String, Program>,
    /// Post-order DFS module dependency order (dependencies strictly precede dependents, `P0-1`).
    pub module_load_order: Vec<String>,
    /// Mapping in entry (and per module) from imported local alias -> canonical target `(module_or_builtin, symbol)`.
    pub entry_imports: BTreeMap<String, ImportedSymbol>,
    pub module_imports: BTreeMap<String, BTreeMap<String, ImportedSymbol>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportedSymbol {
    Builtin(String),
    ModuleItem { module: String, item: String },
    ModuleNamespace(String),
}

pub fn builtin_namespace_items(namespace: &[String]) -> Option<&'static [&'static str]> {
    if namespace.len() != 2 || namespace[0] != "aod" {
        return None;
    }
    match namespace[1].as_str() {
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
            "str_bytes",
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
            "neg",
            "abs",
            "sgn",
            "min",
            "max",
            "pow",
            "gcd",
            "lcm",
            "trit_shl",
            "trit_shr",
            "superpose_cancel",
            "popcnt",
            "is_hamming1",
            "mux",
            "select",
            "gate",
            "argmin",
            "argmax",
            "walk",
            "settle",
            "fold",
            "scan",
            "each",
            "range_seq",
            "push",
            "concat",
            "slice",
            "set_at",
            "map_put",
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
        ]),
        "witness" => Some(&[
            "trace", "step", "hinge", "dwell", "close", "phases", "beats", "rate", "fold",
            "outer", "fuse", "coupling", "contact", "cadence", "lag", "drift",
        ]),
        "ledger" => Some(&["probability", "sadar_promote", "shedding_excess"]),
        "wavelet" => Some(&[
            "window_dedup_count",
            "native_cadence_ratio",
            "single_contact_association",
            "recipient_eval_demo",
        ]),
        "sim" => Some(&["boundary_of"]),
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
        ]),
        "sys" => Some(&["cli_args", "read_file_bytes", "write_executable_elf"]),
        _ => None,
    }
}

pub fn canonical_builtin_name(namespace: &[String], item: &str) -> String {
    if namespace.len() == 2 && namespace[0] == "aod" {
        match namespace[1].as_str() {
            "qext" => format!("aod::qext::{item}"),
            "sys" => format!("aod::sys::{item}"),
            _ => item.to_string(),
        }
    } else {
        item.to_string()
    }
}

/// Resolve the `stdlib/` directory root strictly according to `P0-2` precedence:
/// 1. Explicit `options.stdlib_dir` (`--stdlib <dir>`) -> use ONLY that path (zero silent fallback).
/// 2. Explicit non-empty `AOD_STDLIB_DIR` env var -> use ONLY that path (zero silent fallback).
/// 3. Upward ancestor walk (up to 6 ancestors from `base_dir`, then `current_dir`) for `<dir>/stdlib/aod/ledger.dec`.
/// 4. Compile-time workspace fallback `<CARGO_MANIFEST_DIR>/../../stdlib` if `aod/ledger.dec` exists.
pub fn resolve_stdlib_root(options: &LoadOptions, base_dir: Option<&Path>) -> Option<PathBuf> {
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

/// Load a source file (or in-memory source with optional base directory) with default `LoadOptions`.
pub fn load_from_source(entry_source: &str, base_dir: Option<&Path>) -> DecResult<LoadedUnit> {
    load_from_source_with_options(entry_source, base_dir, &LoadOptions::default())
}

/// Load a source file (or in-memory source with optional base directory and explicit `LoadOptions`)
/// resolving all `use` declarations transitively, recording post-order DFS `module_load_order`.
pub fn load_from_source_with_options(
    entry_source: &str,
    base_dir: Option<&Path>,
    options: &LoadOptions,
) -> DecResult<LoadedUnit> {
    let entry_program = parse_source(entry_source)?;
    let entry_sha256 = Sha256::digest_hex(entry_source.as_bytes());
    let stdlib_root = resolve_stdlib_root(options, base_dir);

    let mut loader = ModuleLoader {
        base_dir: base_dir.map(Path::to_path_buf),
        stdlib_root,
        embedded: None,
        bundled_modules: BTreeMap::new(),
        parsed_modules: BTreeMap::new(),
        module_load_order: Vec::new(),
        module_imports: BTreeMap::new(),
        visiting: Vec::new(),
        visited: BTreeSet::new(),
    };

    let entry_imports = loader.resolve_program_uses(&entry_program, "<entry>", base_dir)?;

    Ok(LoadedUnit {
        entry_source: entry_source.to_string(),
        entry_sha256,
        entry_program,
        bundled_modules: loader.bundled_modules,
        parsed_modules: loader.parsed_modules,
        module_load_order: loader.module_load_order,
        entry_imports,
        module_imports: loader.module_imports,
    })
}

/// Load a `LoadedUnit` strictly hermetically from embedded `entry_source` + `bundled_modules`
/// (ZERO ambient filesystem reads, ZERO `AOD_STDLIB_DIR` inspection).
pub fn load_hermetic(
    entry_source: &str,
    bundled_modules: &BTreeMap<String, BundledModule>,
) -> DecResult<LoadedUnit> {
    for (name, m) in bundled_modules {
        let actual = Sha256::digest_hex(m.source.as_bytes());
        if actual != m.sha256 {
            return Err(Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                format!("bundled module {name:?} SHA-256 mismatch"),
            ));
        }
    }
    let entry_program = parse_source(entry_source)?;
    let entry_sha256 = Sha256::digest_hex(entry_source.as_bytes());

    let mut loader = ModuleLoader {
        base_dir: None,
        stdlib_root: None,
        embedded: Some(bundled_modules.clone()),
        bundled_modules: BTreeMap::new(),
        parsed_modules: BTreeMap::new(),
        module_load_order: Vec::new(),
        module_imports: BTreeMap::new(),
        visiting: Vec::new(),
        visited: BTreeSet::new(),
    };

    let entry_imports = loader.resolve_program_uses(&entry_program, "<entry>", None)?;

    Ok(LoadedUnit {
        entry_source: entry_source.to_string(),
        entry_sha256,
        entry_program,
        bundled_modules: loader.bundled_modules,
        parsed_modules: loader.parsed_modules,
        module_load_order: loader.module_load_order,
        entry_imports,
        module_imports: loader.module_imports,
    })
}

struct ModuleLoader {
    base_dir: Option<PathBuf>,
    stdlib_root: Option<PathBuf>,
    embedded: Option<BTreeMap<String, BundledModule>>,
    bundled_modules: BTreeMap<String, BundledModule>,
    parsed_modules: BTreeMap<String, Program>,
    module_load_order: Vec<String>,
    module_imports: BTreeMap<String, BTreeMap<String, ImportedSymbol>>,
    visiting: Vec<String>,
    visited: BTreeSet<String>,
}

impl ModuleLoader {
    fn resolve_program_uses(
        &mut self,
        program: &Program,
        current_mod: &str,
        current_dir: Option<&Path>,
    ) -> DecResult<BTreeMap<String, ImportedSymbol>> {
        let mut imports = BTreeMap::new();
        for item in &program.items {
            if let Item::Use(u) = item {
                self.resolve_one_use(u, current_mod, current_dir, &mut imports)?;
            }
        }
        Ok(imports)
    }

    fn resolve_one_use(
        &mut self,
        u: &UseDecl,
        current_mod: &str,
        current_dir: Option<&Path>,
        imports: &mut BTreeMap<String, ImportedSymbol>,
    ) -> DecResult<()> {
        if u.path.is_empty() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::ParseError,
                u.line,
                "empty use path",
            ));
        }

        // Built-in `aod::*` namespace
        if u.path[0] == "aod" {
            if let Some(items) = &u.items {
                let allowed = builtin_namespace_items(&u.path).ok_or_else(|| {
                    Diagnostic::at_line(
                        DiagnosticCode::ModuleNotFound,
                        u.line,
                        format!("unknown built-in namespace {}", u.path.join("::")),
                    )
                })?;
                for sym in items {
                    if !allowed.contains(&sym.as_str()) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ModuleNotFound,
                            u.line,
                            format!("symbol {sym} not found in {}", u.path.join("::")),
                        ));
                    }
                    imports.insert(
                        sym.clone(),
                        ImportedSymbol::Builtin(canonical_builtin_name(&u.path, sym)),
                    );
                }
                return Ok(());
            } else if u.path.len() == 3 {
                let ns = &u.path[0..2];
                let sym = &u.path[2];
                let allowed = builtin_namespace_items(ns).ok_or_else(|| {
                    Diagnostic::at_line(
                        DiagnosticCode::ModuleNotFound,
                        u.line,
                        format!("unknown built-in namespace {}", ns.join("::")),
                    )
                })?;
                if !allowed.contains(&sym.as_str()) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ModuleNotFound,
                        u.line,
                        format!("symbol {sym} not found in {}", ns.join("::")),
                    ));
                }
                imports.insert(
                    sym.clone(),
                    ImportedSymbol::Builtin(canonical_builtin_name(ns, sym)),
                );
                return Ok(());
            } else if u.path.len() == 2 {
                if builtin_namespace_items(&u.path).is_none() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ModuleNotFound,
                        u.line,
                        format!("unknown built-in namespace {}", u.path.join("::")),
                    ));
                }
                imports.insert(
                    u.path[1].clone(),
                    ImportedSymbol::ModuleNamespace(u.path.join("::")),
                );
                return Ok(());
            } else {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ModuleNotFound,
                    u.line,
                    format!("invalid built-in use path {}", u.path.join("::")),
                ));
            }
        }

        // Reject bare `use stdlib` without at least one submodule segment (`P0-2`)
        if u.path[0] == "stdlib" && u.path.len() < 2 {
            return Err(Diagnostic::at_line(
                DiagnosticCode::ModuleNotFound,
                u.line,
                "stdlib import requires at least one submodule segment (e.g. `use stdlib::aod::ledger`)",
            ));
        }

        // Filesystem, stdlib, or hermetic embedded module
        if let Some(items) = &u.items {
            let mod_key = u.path.join("::");
            self.load_user_module(&mod_key, &u.path, current_mod, current_dir, u.line)?;
            for sym in items {
                imports.insert(
                    sym.clone(),
                    ImportedSymbol::ModuleItem {
                        module: mod_key.clone(),
                        item: sym.clone(),
                    },
                );
            }
        } else if u.path.len() == 1 {
            let mod_key = u.path[0].clone();
            self.load_user_module(&mod_key, &u.path, current_mod, current_dir, u.line)?;
            imports.insert(
                mod_key.clone(),
                ImportedSymbol::ModuleNamespace(mod_key),
            );
        } else {
            let full_key = u.path.join("::");
            if self.module_exists(&full_key, &u.path, current_dir) {
                self.load_user_module(&full_key, &u.path, current_mod, current_dir, u.line)?;
                let alias = u.path.last().cloned().unwrap();
                imports.insert(alias, ImportedSymbol::ModuleNamespace(full_key));
            } else {
                let parent_path = &u.path[..u.path.len() - 1];
                if parent_path.first().map(String::as_str) == Some("stdlib") && parent_path.len() < 2
                {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ModuleNotFound,
                        u.line,
                        format!("stdlib module {full_key:?} not found"),
                    ));
                }
                let mod_key = parent_path.join("::");
                let sym = u.path.last().cloned().unwrap();
                self.load_user_module(&mod_key, parent_path, current_mod, current_dir, u.line)?;
                imports.insert(
                    sym.clone(),
                    ImportedSymbol::ModuleItem {
                        module: mod_key,
                        item: sym,
                    },
                );
            }
        }
        Ok(())
    }

    fn module_exists(&self, mod_key: &str, segs: &[String], current_dir: Option<&Path>) -> bool {
        if let Some(emb) = &self.embedded {
            return emb.contains_key(mod_key);
        }
        self.find_module_file(segs, current_dir).is_some()
    }

    fn find_module_file(&self, segs: &[String], current_dir: Option<&Path>) -> Option<PathBuf> {
        if segs.is_empty()
            || segs
                .iter()
                .any(|s| s.is_empty() || s == "." || s == ".." || s.contains('/') || s.contains('\\'))
        {
            return None;
        }

        if segs[0] == "stdlib" {
            if segs.len() < 2 {
                return None;
            }
            let root = self.stdlib_root.as_ref()?;
            let mut direct = root.clone();
            for s in &segs[1..] {
                direct.push(s);
            }
            let as_file = direct.with_extension("dec");
            if as_file.is_file() {
                return Some(as_file);
            }
            let as_mod = direct.join("mod.dec");
            if as_mod.is_file() {
                return Some(as_mod);
            }
            return None;
        }

        let mut search_roots = Vec::new();
        if let Some(d) = current_dir {
            search_roots.push(d.to_path_buf());
        }
        if let Some(d) = &self.base_dir {
            if !search_roots.contains(d) {
                search_roots.push(d.clone());
            }
        }
        for root in search_roots {
            let mut direct = root.clone();
            for s in segs {
                direct.push(s);
            }
            let as_file = direct.with_extension("dec");
            if as_file.is_file() {
                return Some(as_file);
            }
            let as_mod = direct.join("mod.dec");
            if as_mod.is_file() {
                return Some(as_mod);
            }
        }
        None
    }

    fn load_user_module(
        &mut self,
        mod_key: &str,
        segs: &[String],
        requester: &str,
        current_dir: Option<&Path>,
        line: usize,
    ) -> DecResult<()> {
        if self.visiting.iter().any(|m| m == mod_key) {
            let mut chain = self.visiting.clone();
            chain.push(mod_key.to_string());
            return Err(Diagnostic::at_line(
                DiagnosticCode::ModuleCycleError,
                line,
                format!("cyclic module import detected: {}", chain.join(" -> ")),
            ));
        }
        if self.visited.contains(mod_key) {
            return Ok(());
        }

        let (source, next_dir) = if let Some(emb) = &self.embedded {
            let m = emb.get(mod_key).ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::ModuleNotFound,
                    line,
                    format!(
                        "module {mod_key:?} (imported by {requester}) not found in hermetic IR bundle"
                    ),
                )
            })?;
            (m.source.clone(), None)
        } else {
            let file_path = self.find_module_file(segs, current_dir).ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::ModuleNotFound,
                    line,
                    format!("module {mod_key:?} (imported by {requester}) not found on filesystem"),
                )
            })?;
            let text = std::fs::read_to_string(&file_path).map_err(|e| {
                Diagnostic::at_line(
                    DiagnosticCode::ModuleNotFound,
                    line,
                    format!("failed reading module {mod_key:?}: {e}"),
                )
            })?;
            let parent_dir = file_path.parent().map(Path::to_path_buf);
            (text, parent_dir)
        };

        let sha = Sha256::digest_hex(source.as_bytes());
        let program = parse_source(&source)?;

        self.visiting.push(mod_key.to_string());
        let mod_imports = self.resolve_program_uses(&program, mod_key, next_dir.as_deref())?;
        self.visiting.pop();

        self.visited.insert(mod_key.to_string());
        self.module_load_order.push(mod_key.to_string());
        self.bundled_modules.insert(
            mod_key.to_string(),
            BundledModule {
                source,
                sha256: sha,
            },
        );
        self.parsed_modules.insert(mod_key.to_string(), program);
        self.module_imports.insert(mod_key.to_string(), mod_imports);
        Ok(())
    }
}
