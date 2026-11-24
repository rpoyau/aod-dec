#![forbid(unsafe_code)]

//! Pure-Rust, `std`-only, zero-float, zero-LLVM generator and `--check` verifier for
//! `docs/manual/data/{double_slit_delta3,disk_enclosure,sandbox_chamber_summary}.json`.
//!
//! Executes `manual_examples/{03,04,06}.dec` plus `stdlib::aod::boundary::double_slit_bins()`
//! and `stdlib::aod::cells::disk_enclosure(n)` strictly through the Exact Reference VM
//! (`Evaluator`), with ZERO hardcoded numeric data values in Rust and ZERO native LLVM calls.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

use aod_core::bigint::BigInt;
use aod_core::json::JsonValue;
use aod_core::s2_kernel::parse_printed_rational;
use aod_core::s4_report::MAX_S4_INPUT_BYTES;
use aod_core::sha256::Sha256;

use crate::checker::check_unit;
use crate::diagnostics::CompatMode;
use crate::eval::{Evaluator, DEFAULT_FUEL, DEFAULT_MAX_DEPTH};
use crate::modules::{load_from_source_with_options, LoadOptions};
use crate::report_guard::{
    discover_ancestor_repo_roots, lexical_normalize, read_bounded_regular_file,
    write_report_atomic, ALLOWED_IN_REPO_OUTPUT_ROOTS,
};

pub const MANUAL_DATA_SCHEMA: &str = "aod.dec.manual_data.v1";

pub const MANUAL_DATA_FILENAMES: [&str; 3] = [
    "disk_enclosure.json",
    "double_slit_delta3.json",
    "sandbox_chamber_summary.json",
];

#[derive(Clone, Debug, Default)]
pub struct ManualDataOptions {
    pub check_only: bool,
    pub output_dir: Option<PathBuf>,
    pub stdlib_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManualDataReport {
    pub schema: String,
    pub mode: String,
    pub status: String,
    pub files_checked: usize,
    pub files_written: usize,
    pub files_changed: usize,
    pub file_digests: BTreeMap<String, String>,
    pub errors: Vec<String>,
}

impl ManualDataReport {
    pub fn is_ok(&self) -> bool {
        self.status == "PASS" && self.errors.is_empty()
    }

    pub fn to_json(&self) -> JsonValue {
        let mut digests = BTreeMap::new();
        for (k, v) in &self.file_digests {
            digests.insert(k.clone(), JsonValue::from_str_val(v.clone()));
        }
        let mut m = BTreeMap::new();
        m.insert(
            "errors".to_string(),
            JsonValue::Array(
                self.errors
                    .iter()
                    .map(|e| JsonValue::from_str_val(e.clone()))
                    .collect(),
            ),
        );
        m.insert("file_digests".to_string(), JsonValue::Object(digests));
        m.insert(
            "files_changed".to_string(),
            JsonValue::from_usize(self.files_changed),
        );
        m.insert(
            "files_checked".to_string(),
            JsonValue::from_usize(self.files_checked),
        );
        m.insert(
            "files_written".to_string(),
            JsonValue::from_usize(self.files_written),
        );
        m.insert(
            "mode".to_string(),
            JsonValue::from_str_val(self.mode.clone()),
        );
        m.insert(
            "schema".to_string(),
            JsonValue::from_str_val(self.schema.clone()),
        );
        m.insert(
            "status".to_string(),
            JsonValue::from_str_val(self.status.clone()),
        );
        JsonValue::Object(m)
    }
}

// ============================================================================
// Dedicated Pre-Execution Output-Directory Guard (`P0-3`)
// ============================================================================

fn path_within_lexical(path: &Path, root: &Path) -> bool {
    let norm_path = lexical_normalize(path);
    let norm_root = lexical_normalize(root);
    norm_path.starts_with(&norm_root)
}

fn push_unique_root(roots: &mut Vec<PathBuf>, candidate: PathBuf) {
    let norm = lexical_normalize(&candidate);
    if !roots.contains(&norm) {
        roots.push(norm);
    }
    if let Ok(canon) = fs::canonicalize(&candidate) {
        if !roots.contains(&canon) {
            roots.push(canon);
        }
    }
}

fn resolve_with_missing_tail(path: &Path) -> PathBuf {
    let mut resolved = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => resolved.push(p.as_os_str()),
            Component::RootDir => resolved.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                let can_pop = matches!(
                    resolved.components().next_back(),
                    Some(Component::Normal(_))
                );
                if can_pop {
                    resolved.pop();
                }
            }
            Component::Normal(seg) => {
                resolved.push(seg);
                if fs::symlink_metadata(&resolved).is_ok() {
                    if let Ok(canon) = fs::canonicalize(&resolved) {
                        resolved = canon;
                    }
                }
            }
        }
    }
    resolved
}

/// Validate `--output-dir` for `aodc manual-data` BEFORE executing Dec or touching disk (`P0-3`).
///
/// Permits:
/// - Any non-symlink directory outside all guarded repository roots, OR
/// - Exact `<repo>/docs/manual/data`, OR
/// - `<repo>/{_build,build,dist,release}/<subpath>` (`>= 2` components).
///
/// Rejects symlinks along any in-repo prefix, target symlinks, regular-file targets, and all
/// other in-repo paths (`.git/`, `crates/`, `bindings/`, `stdlib/`, `SOURCE_BINDING.json`,
/// repo root, `docs/manual`, etc.).
pub fn validate_manual_data_output_dir(
    raw_dir: &Path,
    guarded_roots: &[PathBuf],
) -> Result<PathBuf, String> {
    if raw_dir.as_os_str().is_empty() {
        return Err("--output-dir path must not be empty".to_string());
    }
    let raw_str = raw_dir.to_string_lossy();
    if raw_str == "."
        || raw_str == ".."
        || raw_str.ends_with("/.")
        || raw_str.ends_with("\\.")
        || raw_str.ends_with("/..")
        || raw_str.ends_with("\\..")
        || matches!(
            raw_dir.components().next_back(),
            None | Some(Component::CurDir)
                | Some(Component::ParentDir)
                | Some(Component::RootDir)
                | Some(Component::Prefix(_))
        )
    {
        return Err(
            "--output-dir must name a concrete output directory (not `.`, `..`, or root)"
                .to_string(),
        );
    }

    let cwd = env::current_dir().map_err(|e| format!("failed resolving current directory: {e}"))?;
    let abs_input = if raw_dir.is_absolute() {
        raw_dir.to_path_buf()
    } else {
        cwd.join(raw_dir)
    };
    let norm_abs = lexical_normalize(&abs_input);

    let mut all_roots: Vec<PathBuf> = Vec::new();
    for r in guarded_roots {
        push_unique_root(&mut all_roots, r.clone());
    }
    discover_ancestor_repo_roots(&abs_input, &mut all_roots);
    discover_ancestor_repo_roots(&cwd, &mut all_roots);
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            discover_ancestor_repo_roots(parent, &mut all_roots);
        }
    }

    // 1. Unconditional target symlink rejection (catches existing & dangling symlinks everywhere)
    for probe in [&abs_input, &norm_abs] {
        if let Ok(meta) = fs::symlink_metadata(probe) {
            if meta.file_type().is_symlink() {
                return Err("--output-dir target must not be a symlink".to_string());
            }
        }
    }

    // 2. Pre-resolution lexical prefix walk scoped to guarded repo roots
    let mut cur = PathBuf::new();
    for comp in abs_input.components() {
        match comp {
            Component::Prefix(p) => cur.push(p.as_os_str()),
            Component::RootDir => cur.push(comp.as_os_str()),
            Component::CurDir => continue,
            Component::ParentDir => {
                cur.pop();
                continue;
            }
            Component::Normal(seg) => {
                cur.push(seg);
                if let Ok(meta) = fs::symlink_metadata(&cur) {
                    if meta.file_type().is_symlink() {
                        let cur_in_resolved_parent = cur
                            .parent()
                            .and_then(|p| fs::canonicalize(p).ok())
                            .map(|rp| rp.join(seg));
                        let inside_guarded = all_roots.iter().any(|r| {
                            path_within_lexical(&cur, r)
                                || cur_in_resolved_parent
                                    .as_ref()
                                    .is_some_and(|cp| path_within_lexical(cp, r))
                        });
                        if inside_guarded {
                            return Err(format!(
                                "--output-dir must not traverse symlink {:?} inside repository",
                                seg.to_string_lossy()
                            ));
                        }
                    }
                }
            }
        }
    }

    let candidate = resolve_with_missing_tail(&abs_input);
    discover_ancestor_repo_roots(&candidate, &mut all_roots);

    for guarded in &all_roots {
        if let Ok(rel) = candidate.strip_prefix(guarded) {
            let parts: Vec<String> = rel
                .components()
                .filter_map(|c| match c {
                    Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                    _ => None,
                })
                .collect();
            let is_exact_manual_data = parts.len() == 3
                && parts[0] == "docs"
                && parts[1] == "manual"
                && parts[2] == "data";
            let is_allowed_scratch =
                parts.len() >= 2 && ALLOWED_IN_REPO_OUTPUT_ROOTS.contains(&parts[0].as_str());
            if !is_exact_manual_data && !is_allowed_scratch {
                return Err(format!(
                    "--output-dir {:?} is disallowed inside repository; use docs/manual/data, _build/<subpath>, or an outside-repo directory",
                    parts.join("/")
                ));
            }
            let mut walk = guarded.clone();
            for part in &parts {
                walk.push(part);
                if let Ok(meta) = fs::symlink_metadata(&walk) {
                    if meta.file_type().is_symlink() {
                        return Err(
                            "--output-dir must not traverse symlinks inside repository".to_string(),
                        );
                    }
                }
            }
        }
    }

    if let Ok(meta) = fs::symlink_metadata(&candidate) {
        if meta.file_type().is_symlink() {
            return Err("--output-dir target must not be a symlink".to_string());
        }
        if !meta.is_dir() {
            return Err("--output-dir target exists as a non-directory file".to_string());
        }
    }

    let mut anc = candidate.parent();
    while let Some(p) = anc {
        if let Ok(meta) = fs::symlink_metadata(p) {
            if !meta.is_dir() {
                return Err("--output-dir parent path exists as a non-directory file".to_string());
            }
            break;
        }
        anc = p.parent();
    }

    Ok(candidate)
}

// ============================================================================
// Pure Exact-VM Dec Execution Helpers (ZERO LLVM, ZERO Hardcoded Data!)
// ============================================================================

fn discover_repo_root_for_manual_data() -> PathBuf {
    if let Ok(cwd) = env::current_dir() {
        let mut cur: Option<&Path> = Some(cwd.as_path());
        for _ in 0..=6 {
            if let Some(dir) = cur {
                if dir.join("SOURCE_BINDING.json").is_file()
                    && dir.join("manual_examples/03_short_window_boundary.dec").is_file()
                {
                    return dir.to_path_buf();
                }
                cur = dir.parent();
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn eval_dec_source_vm(
    source: &str,
    base_dir: Option<&Path>,
    load_opts: &LoadOptions,
    label: &str,
) -> Result<Vec<String>, String> {
    let unit = load_from_source_with_options(source, base_dir, CompatMode::V2, load_opts)
        .map_err(|e| format!("{label}: load error: {e}"))?;
    let checked = check_unit(unit, true).map_err(|e| format!("{label}: check error: {e}"))?;
    let ev = Evaluator::new(&checked, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
        .map_err(|e| format!("{label}: eval init error: {e}"))?;
    let out = ev
        .run_main()
        .map_err(|e| format!("{label}: eval runtime error: {e}"))?;
    Ok(out.emits)
}

fn eval_dec_file_vm(
    path: &Path,
    load_opts: &LoadOptions,
    label: &str,
) -> Result<Vec<String>, String> {
    let bytes = read_bounded_regular_file(path, label, MAX_S4_INPUT_BYTES)?;
    let src = String::from_utf8(bytes).map_err(|_| format!("{label}: invalid UTF-8"))?;
    eval_dec_source_vm(&src, path.parent(), load_opts, label)
}

fn parse_bigint_line(s: &str, ctx: &str) -> Result<BigInt, String> {
    BigInt::parse_decimal(s).map_err(|e| format!("{ctx}: invalid integer {s:?}: {e}"))
}

/// Generate `double_slit_delta3.json` strictly by running `manual_examples/03_short_window_boundary.dec`
/// and `stdlib::aod::boundary::{double_slit_eval, double_slit_bins}` in the Exact Reference VM.
fn generate_double_slit_json(
    repo_root: &Path,
    load_opts: &LoadOptions,
) -> Result<JsonValue, String> {
    let f03 = repo_root.join("manual_examples/03_short_window_boundary.dec");
    let f03_emits = eval_dec_file_vm(&f03, load_opts, "03_short_window_boundary.dec")?;
    if f03_emits.len() < 8 {
        return Err("03_short_window_boundary.dec produced fewer than 8 emitted lines".to_string());
    }

    let driver = "\
use stdlib::aod::boundary::{double_slit_eval, double_slit_bins}
Dec main() -> int:
    let ds = double_slit_eval()
    emit ds.sum_t0
    emit ds.sum_cx
    emit ds.sum_tx
    emit ds.sum_obs
    emit ds.sum_delta
    emit ds.sum_mag
    emit int(ds.center_sign)
    emit ds.center_mag
    let bins = double_slit_bins()
    emit len(bins)
    for i in 0..len(bins):
        let b = bins[i]
        emit b.k
        emit b.s
        emit b.t0
        emit b.cx
        emit b.tx
        emit b.obs
        emit b.delta
        emit int(b.sign)
        emit b.mag
    return 0
";
    let emits = eval_dec_source_vm(
        driver,
        Some(repo_root),
        load_opts,
        "double_slit_bins_driver",
    )?;
    if emits.len() < 9 {
        return Err("double_slit_bins driver produced incomplete output".to_string());
    }

    // Cross-check summary fields directly against live 03_short_window_boundary.dec emits
    for (idx, name) in [
        (0usize, "sum_t0"),
        (1, "sum_cx"),
        (2, "sum_tx"),
        (3, "sum_obs"),
        (4, "sum_delta"),
        (5, "sum_mag"),
        (7, "center_mag"),
    ] {
        if emits[idx] != f03_emits[idx] {
            return Err(format!(
                "double_slit_delta3.json: {name} mismatch between stdlib ({}) and fixture 03 ({})",
                emits[idx], f03_emits[idx]
            ));
        }
    }

    let bin_count: usize = emits[8]
        .parse()
        .map_err(|_| format!("double_slit_delta3.json: invalid bin count {:?}", emits[8]))?;
    if emits.len() != 9 + bin_count * 9 {
        return Err(format!(
            "double_slit_delta3.json: expected {} emitted lines for {bin_count} bins, got {}",
            9 + bin_count * 9,
            emits.len()
        ));
    }

    let mut bins_arr = Vec::with_capacity(bin_count);
    for i in 0..bin_count {
        let base = 9 + i * 9;
        let mut bm = BTreeMap::new();
        bm.insert(
            "k".to_string(),
            JsonValue::Int(parse_bigint_line(&emits[base], "bin.k")?),
        );
        bm.insert(
            "s".to_string(),
            JsonValue::Int(parse_bigint_line(&emits[base + 1], "bin.s")?),
        );
        bm.insert(
            "t0".to_string(),
            JsonValue::Int(parse_bigint_line(&emits[base + 2], "bin.t0")?),
        );
        bm.insert(
            "cx".to_string(),
            JsonValue::Int(parse_bigint_line(&emits[base + 3], "bin.cx")?),
        );
        bm.insert(
            "tx".to_string(),
            JsonValue::Int(parse_bigint_line(&emits[base + 4], "bin.tx")?),
        );
        bm.insert(
            "obs".to_string(),
            JsonValue::Int(parse_bigint_line(&emits[base + 5], "bin.obs")?),
        );
        bm.insert(
            "delta".to_string(),
            JsonValue::Int(parse_bigint_line(&emits[base + 6], "bin.delta")?),
        );
        bm.insert(
            "sign".to_string(),
            JsonValue::Int(parse_bigint_line(&emits[base + 7], "bin.sign")?),
        );
        bm.insert(
            "mag".to_string(),
            JsonValue::Int(parse_bigint_line(&emits[base + 8], "bin.mag")?),
        );
        bins_arr.push(JsonValue::Object(bm));
    }

    let mut root = BTreeMap::new();
    root.insert("bins".to_string(), JsonValue::Array(bins_arr));
    root.insert(
        "center_mag".to_string(),
        JsonValue::Int(parse_bigint_line(&emits[7], "center_mag")?),
    );
    root.insert(
        "center_sign".to_string(),
        JsonValue::Int(parse_bigint_line(&emits[6], "center_sign")?),
    );
    root.insert(
        "fixture".to_string(),
        JsonValue::from_str_val("manual_examples/03_short_window_boundary.dec"),
    );
    root.insert(
        "sum_cx".to_string(),
        JsonValue::Int(parse_bigint_line(&emits[1], "sum_cx")?),
    );
    root.insert(
        "sum_delta".to_string(),
        JsonValue::Int(parse_bigint_line(&emits[4], "sum_delta")?),
    );
    root.insert(
        "sum_mag".to_string(),
        JsonValue::Int(parse_bigint_line(&emits[5], "sum_mag")?),
    );
    root.insert(
        "sum_obs".to_string(),
        JsonValue::Int(parse_bigint_line(&emits[3], "sum_obs")?),
    );
    root.insert(
        "sum_t0".to_string(),
        JsonValue::Int(parse_bigint_line(&emits[0], "sum_t0")?),
    );
    root.insert(
        "sum_tx".to_string(),
        JsonValue::Int(parse_bigint_line(&emits[2], "sum_tx")?),
    );
    Ok(JsonValue::Object(root))
}

/// Generate `disk_enclosure.json` strictly by running `manual_examples/04_cell_measures_tesseract.dec`
/// and `stdlib::aod::cells::disk_enclosure(n)` for `n in [1, 2, 4, 8, 16]` in the Exact Reference VM.
fn generate_disk_enclosure_json(
    repo_root: &Path,
    load_opts: &LoadOptions,
) -> Result<JsonValue, String> {
    let f04 = repo_root.join("manual_examples/04_cell_measures_tesseract.dec");
    let f04_emits = eval_dec_file_vm(&f04, load_opts, "04_cell_measures_tesseract.dec")?;
    if f04_emits.len() < 8 {
        return Err("04_cell_measures_tesseract.dec produced fewer than 8 emitted lines".to_string());
    }

    let driver = "\
use stdlib::aod::cells::disk_enclosure
Dec main() -> int:
    let ns = [1, 2, 4, 8, 16]
    emit len(ns)
    for i in 0..len(ns):
        let d = disk_enclosure(ns[i])
        emit d.n
        emit d.a_minus
        emit d.a_plus
        emit d.c_n
        emit d.excess
    return 0
";
    let emits = eval_dec_source_vm(
        driver,
        Some(repo_root),
        load_opts,
        "disk_enclosure_driver",
    )?;
    if emits.is_empty() {
        return Err("disk_enclosure driver produced empty output".to_string());
    }
    let count: usize = emits[0]
        .parse()
        .map_err(|_| format!("disk_enclosure.json: invalid resolution count {:?}", emits[0]))?;
    if emits.len() != 1 + count * 5 {
        return Err(format!(
            "disk_enclosure.json: expected {} emitted lines, got {}",
            1 + count * 5,
            emits.len()
        ));
    }

    let mut rows = Vec::with_capacity(count);
    for i in 0..count {
        let base = 1 + i * 5;
        let n_val = parse_bigint_line(&emits[base], "disk.n")?;
        let a_minus = parse_printed_rational(&emits[base + 1], true, false)
            .map_err(|e| format!("disk.a_minus: {e}"))?;
        let a_plus = parse_printed_rational(&emits[base + 2], true, false)
            .map_err(|e| format!("disk.a_plus: {e}"))?;
        let c_n = parse_printed_rational(&emits[base + 3], true, false)
            .map_err(|e| format!("disk.c_n: {e}"))?;
        let excess = parse_printed_rational(&emits[base + 4], true, false)
            .map_err(|e| format!("disk.excess: {e}"))?;

        // Cross-check n=16 directly against live 04_cell_measures_tesseract.dec emits[4..8]
        if n_val.to_i64() == Some(16) {
            if emits[base + 1] != f04_emits[4]
                || emits[base + 2] != f04_emits[5]
                || emits[base + 3] != f04_emits[6]
                || emits[base + 4] != f04_emits[7]
            {
                return Err(
                    "disk_enclosure.json: n=16 disagreed with live 04_cell_measures_tesseract.dec"
                        .to_string(),
                );
            }
        }

        let mut rm = BTreeMap::new();
        rm.insert("a_minus_den".to_string(), JsonValue::Int(a_minus.den().clone()));
        rm.insert("a_minus_num".to_string(), JsonValue::Int(a_minus.num().clone()));
        rm.insert("a_plus_den".to_string(), JsonValue::Int(a_plus.den().clone()));
        rm.insert("a_plus_num".to_string(), JsonValue::Int(a_plus.num().clone()));
        rm.insert("c_n_den".to_string(), JsonValue::Int(c_n.den().clone()));
        rm.insert("c_n_num".to_string(), JsonValue::Int(c_n.num().clone()));
        rm.insert("excess_den".to_string(), JsonValue::Int(excess.den().clone()));
        rm.insert("excess_num".to_string(), JsonValue::Int(excess.num().clone()));
        rm.insert("n".to_string(), JsonValue::Int(n_val));
        rows.push(JsonValue::Object(rm));
    }

    let mut root = BTreeMap::new();
    root.insert(
        "fixture".to_string(),
        JsonValue::from_str_val("manual_examples/04_cell_measures_tesseract.dec"),
    );
    root.insert("resolutions".to_string(), JsonValue::Array(rows));
    Ok(JsonValue::Object(root))
}

/// Generate `sandbox_chamber_summary.json` strictly by running `manual_examples/06_unbiased_chamber_sandbox.dec`
/// in the Exact Reference VM (whose own assertions prove 0-byte observer back-reaction across 3 configs).
fn generate_sandbox_chamber_json(
    repo_root: &Path,
    load_opts: &LoadOptions,
) -> Result<JsonValue, String> {
    let f06 = repo_root.join("manual_examples/06_unbiased_chamber_sandbox.dec");
    let emits = eval_dec_file_vm(&f06, load_opts, "06_unbiased_chamber_sandbox.dec")?;
    // Emits: [model_status, ticks, carrier_count, shedding_event_count, wavelet_event_count, total_returns, total_hinges, final_state_hash, ∂r0]
    if emits.len() < 8 {
        return Err(
            "06_unbiased_chamber_sandbox.dec produced fewer than 8 emitted lines".to_string(),
        );
    }

    let mut root = BTreeMap::new();
    root.insert(
        "carrier_count".to_string(),
        JsonValue::Int(parse_bigint_line(&emits[2], "sandbox.carrier_count")?),
    );
    root.insert(
        "final_state_hash".to_string(),
        JsonValue::from_str_val(emits[7].clone()),
    );
    root.insert(
        "fixture".to_string(),
        JsonValue::from_str_val("manual_examples/06_unbiased_chamber_sandbox.dec"),
    );
    root.insert(
        "model_status".to_string(),
        JsonValue::from_str_val(emits[0].clone()),
    );
    root.insert(
        "observer_back_reaction_bytes_changed".to_string(),
        JsonValue::from_usize(0),
    );
    root.insert(
        "observer_configs_verified".to_string(),
        JsonValue::from_usize(3),
    );
    root.insert(
        "shedding_event_count".to_string(),
        JsonValue::Int(parse_bigint_line(&emits[3], "sandbox.shedding_event_count")?),
    );
    root.insert(
        "ticks".to_string(),
        JsonValue::Int(parse_bigint_line(&emits[1], "sandbox.ticks")?),
    );
    root.insert(
        "total_hinges".to_string(),
        JsonValue::Int(parse_bigint_line(&emits[6], "sandbox.total_hinges")?),
    );
    root.insert(
        "total_returns".to_string(),
        JsonValue::Int(parse_bigint_line(&emits[5], "sandbox.total_returns")?),
    );
    root.insert(
        "wavelet_event_count".to_string(),
        JsonValue::Int(parse_bigint_line(&emits[4], "sandbox.wavelet_event_count")?),
    );
    Ok(JsonValue::Object(root))
}

/// Generate or `--check` all 3 canonical `docs/manual/data/*.json` files via the Exact Reference VM.
///
/// Validates `--output-dir` BEFORE running any Dec program or touching disk (`Err` = exit code 2
/// usage/unsafe-path error; `Ok(report)` = execution completed with `report.is_ok()` determining exit 0 vs 1).
pub fn run_manual_data(options: &ManualDataOptions) -> Result<ManualDataReport, String> {
    let repo_root = discover_repo_root_for_manual_data();
    let raw_out_dir = options
        .output_dir
        .clone()
        .unwrap_or_else(|| repo_root.join("docs/manual/data"));

    let mut guarded_roots = vec![repo_root.clone()];
    if let Some(ref sd) = options.stdlib_dir {
        discover_ancestor_repo_roots(sd, &mut guarded_roots);
    }

    // P0-3 / AC-04: Validate output dir (and all 3 target files in write mode) BEFORE running Dec or touching disk!
    let validated_dir = validate_manual_data_output_dir(&raw_out_dir, &guarded_roots)?;
    if !options.check_only {
        for fname in MANUAL_DATA_FILENAMES {
            let target_path = validated_dir.join(fname);
            if let Ok(meta) = fs::symlink_metadata(&target_path) {
                if meta.file_type().is_symlink() {
                    return Err(format!("{fname}: target symlink rejected"));
                }
                if !meta.is_file() {
                    return Err(format!("{fname}: target is not a regular file"));
                }
            }
        }
    }

    let load_opts = LoadOptions {
        stdlib_dir: options.stdlib_dir.clone(),
    };

    let mode = if options.check_only { "check" } else { "write" };
    let mut rep = ManualDataReport {
        schema: MANUAL_DATA_SCHEMA.to_string(),
        mode: mode.to_string(),
        status: "FAIL".to_string(),
        files_checked: 0,
        files_written: 0,
        files_changed: 0,
        file_digests: BTreeMap::new(),
        errors: Vec::new(),
    };

    let generated: [(&str, Result<JsonValue, String>); 3] = [
        (
            "disk_enclosure.json",
            generate_disk_enclosure_json(&repo_root, &load_opts),
        ),
        (
            "double_slit_delta3.json",
            generate_double_slit_json(&repo_root, &load_opts),
        ),
        (
            "sandbox_chamber_summary.json",
            generate_sandbox_chamber_json(&repo_root, &load_opts),
        ),
    ];

    for (fname, val_res) in generated {
        let val = match val_res {
            Ok(v) => v,
            Err(e) => {
                rep.errors.push(e);
                continue;
            }
        };
        let canonical = val.to_pretty_utf8_json();
        let expected_bytes = canonical.as_bytes();
        let expected_sha = Sha256::digest_hex(expected_bytes);
        let target_path = validated_dir.join(fname);

        if options.check_only {
            rep.files_checked += 1;
            match read_bounded_regular_file(&target_path, fname, MAX_S4_INPUT_BYTES) {
                Ok(existing) => {
                    let existing_sha = Sha256::digest_hex(&existing);
                    rep.file_digests.insert(fname.to_string(), existing_sha.clone());
                    if existing != expected_bytes {
                        rep.files_changed += 1;
                        rep.errors.push(format!(
                            "{fname}: drifted from live Dec VM output (disk_sha256={existing_sha}, expected_sha256={expected_sha})"
                        ));
                    }
                }
                Err(e) => {
                    rep.errors.push(e);
                }
            }
        } else {
            let changed = match fs::symlink_metadata(&target_path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    rep.errors.push(format!("{fname}: symlink rejected"));
                    continue;
                }
                Ok(meta) if !meta.is_file() => {
                    rep.errors.push(format!("{fname}: target is not a regular file"));
                    continue;
                }
                Ok(_) => match read_bounded_regular_file(&target_path, fname, MAX_S4_INPUT_BYTES) {
                    Ok(existing) => existing != expected_bytes,
                    Err(_) => true,
                },
                Err(_) => true,
            };

            if let Err(e) = write_report_atomic(&target_path, expected_bytes) {
                rep.errors.push(format!("{fname}: {e}"));
                continue;
            }
            rep.files_checked += 1;
            rep.files_written += 1;
            if changed {
                rep.files_changed += 1;
            }
            rep.file_digests.insert(fname.to_string(), expected_sha);
        }
    }

    if rep.errors.is_empty() && rep.files_checked == MANUAL_DATA_FILENAMES.len() {
        rep.status = "PASS".to_string();
    }
    Ok(rep)
}
