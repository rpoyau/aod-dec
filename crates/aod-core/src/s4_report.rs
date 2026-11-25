//! Stage 4 (`S4@e013401`) canonical convergence report builder (`aod.s4.rust.report.v1`),
//! host-path/timestamp error sanitizer, portable-report validator, strict 90-tick
//! `wavelet_shedding_simulation.csv` exact verifier, and `aod-core` crate policy auditor.
//!
//! Strictly `std`-only, zero floats, `#![forbid(unsafe_code)]` (inherited from crate root).

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;

use crate::error::{CoreError, CoreResult};
use crate::json::{parse_strict_json, JsonValue};
use crate::ratio::Ratio;
use crate::s2_kernel::{
    canonical_rational_pair, cycle_shedding_trace, parse_printed_rational, CycleSheddingTickInput,
};
use crate::s2_vectors::{read_regular_no_symlink, S2VerifyReport};
use crate::sha256::Sha256;

pub const S4_REPORT_SCHEMA: &str = "aod.s4.rust.report.v1";
pub const S4_BINDING_ID: &str = "S4@e013401";
pub const S4_BASE_COMMIT_BINDING: &str = "3a2418defb4fa92e6fbfc8951b81990fc57ce0e6";
pub const S4_CYCLE_SHEDDING_CSV_REL: &str = "bindings/aod/s4_fixtures/wavelet_shedding_simulation.csv";
pub const S4_CRATE_NAME: &str = "aod-core";
pub const S4_CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const MAX_S4_INPUT_BYTES: usize = 16 * 1024 * 1024;

pub const FORBIDDEN_REPORT_KEYS: [&str; 20] = [
    "timestamp",
    "created_at",
    "started_at",
    "finished_at",
    "duration",
    "duration_s",
    "elapsed",
    "elapsed_s",
    "wall_clock",
    "pid",
    "hostname",
    "executable",
    "stdout",
    "stderr",
    "temp_dir",
    "rustc_version",
    "target_triple",
    "profile",
    "debug",
    "build_mode",
];

const UNIX_ABS_PREFIXES: [&str; 19] = [
    "home",
    "tmp",
    "var",
    "Users",
    "root",
    "opt",
    "usr",
    "etc",
    "private",
    "mnt",
    "srv",
    "proc",
    "sys",
    "workspace",
    "github",
    "app",
    "runner",
    "builds",
    "dev",
];

const TEMP_MARKERS: [&str; 4] = [
    "pytest-of-",
    "aod-anchor-scientific-replay-",
    "/tmp/",
    "\\Temp\\",
];

const EXPECTED_CSV_HEADER: &str =
    "tick,lead,lag,span,compatibility,padar_burden,shed_excess,outward_shedding,local_reclosure,state";

// ============================================================================
// Portable String Validator & Error Sanitizer (matches upstream S4 report.rs)
// ============================================================================

fn is_path_delim(b: u8) -> bool {
    matches!(
        b,
        b' ' | b'\t' | b'\r' | b'\n' | b'\'' | b'"' | b'(' | b'=' | b':' | b'[' | b'{' | b','
    )
}

fn is_path_stop(b: u8) -> bool {
    matches!(
        b,
        b' ' | b'\t' | b'\r' | b'\n' | b'\'' | b'"' | b',' | b')' | b']' | b'}'
    )
}

fn is_iso_timestamp_start(bytes: &[u8], i: usize) -> bool {
    // YYYY-MM-DDTHH:MM:SS (19 bytes)
    if i + 19 > bytes.len() {
        return false;
    }
    if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
        return false;
    }
    bytes[i..i + 4].iter().all(|b| b.is_ascii_digit())
        && bytes[i + 4] == b'-'
        && bytes[i + 5..i + 7].iter().all(|b| b.is_ascii_digit())
        && bytes[i + 7] == b'-'
        && bytes[i + 8..i + 10].iter().all(|b| b.is_ascii_digit())
        && bytes[i + 10] == b'T'
        && bytes[i + 11..i + 13].iter().all(|b| b.is_ascii_digit())
        && bytes[i + 13] == b':'
        && bytes[i + 14..i + 16].iter().all(|b| b.is_ascii_digit())
        && bytes[i + 16] == b':'
        && bytes[i + 17..i + 19].iter().all(|b| b.is_ascii_digit())
}

fn is_win_abs_path_at(bytes: &[u8], i: usize) -> bool {
    if i + 3 > bytes.len() {
        return false;
    }
    if i > 0 && !is_path_delim(bytes[i - 1]) {
        return false;
    }
    bytes[i].is_ascii_alphabetic()
        && bytes[i + 1] == b':'
        && (bytes[i + 2] == b'\\' || bytes[i + 2] == b'/')
}

fn is_unix_abs_path_at(bytes: &[u8], i: usize) -> bool {
    if bytes.get(i) != Some(&b'/') {
        return false;
    }
    if i == 0 {
        return bytes.len() == 1 || !is_path_stop(bytes[1]);
    }
    if !is_path_delim(bytes[i - 1]) {
        return false;
    }
    let rest = &bytes[i + 1..];
    for prefix in UNIX_ABS_PREFIXES {
        let pb = prefix.as_bytes();
        if rest.starts_with(pb) {
            let after = rest.get(pb.len()).copied();
            if after.is_none() || after == Some(b'/') || after.is_some_and(is_path_stop) {
                return true;
            }
        }
    }
    let mut j = 0usize;
    while j < rest.len()
        && (rest[j].is_ascii_alphanumeric() || matches!(rest[j], b'.' | b'_' | b'-'))
    {
        j += 1;
    }
    if j > 0 && rest.get(j) == Some(&b'/') {
        let k = j + 1;
        if k < rest.len()
            && (rest[k].is_ascii_alphanumeric() || matches!(rest[k], b'.' | b'_' | b'-'))
        {
            return true;
        }
    }
    false
}

fn should_replace_raw_root(raw: &str) -> bool {
    let trimmed = raw.trim_end_matches(['/', '\\']);
    trimmed.len() >= 3 && (trimmed.contains('/') || trimmed.contains('\\'))
}

/// Scrub host absolute paths, executable paths, temp markers, and ISO timestamps from error text.
pub fn sanitize_error(message: &str, extra_roots: &[(&Path, &str)]) -> String {
    let mut replacements: Vec<(String, String)> = Vec::new();
    for &(path, label) in extra_roots {
        if let Ok(canon) = fs::canonicalize(path) {
            replacements.push((canon.to_string_lossy().into_owned(), label.to_string()));
        }
        let raw = path.to_string_lossy().into_owned();
        if should_replace_raw_root(&raw) {
            replacements.push((raw, label.to_string()));
        }
    }
    if let Ok(cwd) = env::current_dir() {
        if let Ok(canon) = fs::canonicalize(&cwd) {
            replacements.push((canon.to_string_lossy().into_owned(), "<CWD>".to_string()));
        }
        let raw = cwd.to_string_lossy().into_owned();
        if should_replace_raw_root(&raw) {
            replacements.push((raw, "<CWD>".to_string()));
        }
    }
    let tmp = env::temp_dir();
    if let Ok(canon) = fs::canonicalize(&tmp) {
        replacements.push((canon.to_string_lossy().into_owned(), "<TEMP_DIR>".to_string()));
    }
    let raw_tmp = tmp.to_string_lossy().into_owned();
    if should_replace_raw_root(&raw_tmp) {
        replacements.push((raw_tmp, "<TEMP_DIR>".to_string()));
    }
    if let Ok(exe) = env::current_exe() {
        if let Ok(canon) = fs::canonicalize(&exe) {
            replacements.push((canon.to_string_lossy().into_owned(), "<EXE>".to_string()));
        }
        let raw_exe = exe.to_string_lossy().into_owned();
        if should_replace_raw_root(&raw_exe) {
            replacements.push((raw_exe, "<EXE>".to_string()));
        }
    }

    replacements.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(&b.0)));
    let mut sanitized = message.to_string();
    for (src, dst) in &replacements {
        let trimmed = src.trim_end_matches(['/', '\\']);
        if trimmed.len() < 3 || (!trimmed.contains('/') && !trimmed.contains('\\')) {
            continue;
        }
        if sanitized.contains(trimmed) {
            sanitized = sanitized.replace(trimmed, dst);
        }
    }

    sanitized = scrub_patterns(&sanitized);
    let trimmed = sanitized.trim();
    if trimmed.is_empty() {
        "verification error".to_string()
    } else {
        trimmed.to_string()
    }
}

fn scrub_patterns(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if is_iso_timestamp_start(bytes, i) {
            out.push_str("<TIMESTAMP>");
            i += 19;
            if i < bytes.len() && bytes[i] == b'.' {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if i < bytes.len() && bytes[i] == b'Z' {
                i += 1;
            } else if i + 3 <= bytes.len()
                && matches!(bytes[i], b'+' | b'-')
                && bytes[i + 1].is_ascii_digit()
                && bytes[i + 2].is_ascii_digit()
            {
                i += 3;
                if i < bytes.len() && bytes[i] == b':' {
                    i += 1;
                }
                if i + 2 <= bytes.len()
                    && bytes[i].is_ascii_digit()
                    && bytes[i + 1].is_ascii_digit()
                {
                    i += 2;
                }
            }
            continue;
        }

        let mut matched_temp = false;
        for marker in TEMP_MARKERS {
            if input[i..].starts_with(marker) {
                out.push_str("<TEMP_PATH>");
                i += marker.len();
                while i < bytes.len() && !is_path_stop(bytes[i]) {
                    i += 1;
                }
                matched_temp = true;
                break;
            }
        }
        if matched_temp {
            continue;
        }

        if is_win_abs_path_at(bytes, i) || is_unix_abs_path_at(bytes, i) {
            out.push_str("<HOST_PATH>");
            while i < bytes.len() && !is_path_stop(bytes[i]) {
                i += 1;
            }
            continue;
        }

        let ch = input[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn check_portable_string(s: &str, path: &str) -> CoreResult<()> {
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        if is_iso_timestamp_start(bytes, i) {
            return Err(CoreError::ValidationError(format!(
                "timestamp leaked in canonical report at {path}"
            )));
        }
        if is_win_abs_path_at(bytes, i) {
            return Err(CoreError::ValidationError(format!(
                "absolute Windows path leaked in canonical report at {path}"
            )));
        }
        if is_unix_abs_path_at(bytes, i) {
            return Err(CoreError::ValidationError(format!(
                "absolute Unix path leaked in canonical report at {path}"
            )));
        }
    }
    for marker in TEMP_MARKERS {
        if s.contains(marker) {
            return Err(CoreError::ValidationError(format!(
                "temporary directory marker leaked in canonical report at {path}"
            )));
        }
    }
    Ok(())
}

/// Recursively verify that a canonical report contains zero volatile keys,
/// zero absolute host paths, zero temp markers, and zero ISO timestamps.
pub fn validate_portable_report(value: &JsonValue, path: &str) -> CoreResult<()> {
    match value {
        JsonValue::Object(map) => {
            for (k, v) in map {
                if FORBIDDEN_REPORT_KEYS.contains(&k.as_str()) {
                    return Err(CoreError::ValidationError(format!(
                        "forbidden volatile report key {k:?} at {path}"
                    )));
                }
                check_portable_string(k, &format!("{path}.<key:{k}>"))?;
                validate_portable_report(v, &format!("{path}.{k}"))?;
            }
        }
        JsonValue::Array(items) => {
            for (idx, item) in items.iter().enumerate() {
                validate_portable_report(item, &format!("{path}[{idx}]"))?;
            }
        }
        JsonValue::Str(s) => {
            check_portable_string(s, path)?;
        }
        JsonValue::Null | JsonValue::Bool(_) | JsonValue::Int(_) => {}
    }
    Ok(())
}

// ============================================================================
// 90-Tick `wavelet_shedding_simulation.csv` Exact Verifier (`S4@e013401`)
// ============================================================================

struct S4BindingInfo {
    binding_id: String,
    binding_status: String,
    base_commit_binding: String,
    expected_csv_sha256: String,
}

fn load_s4_binding(binding_path: &Path) -> CoreResult<S4BindingInfo> {
    let raw = read_regular_no_symlink(binding_path, "SOURCE_BINDING.json")?;
    let text = String::from_utf8(raw).map_err(|_| {
        CoreError::ValidationError("SOURCE_BINDING.json is not valid UTF-8".to_string())
    })?;
    let doc = parse_strict_json(&text)
        .map_err(|e| CoreError::ValidationError(format!("SOURCE_BINDING.json parse error: {e}")))?;
    let upstream = doc
        .get("upstream_bindings")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            CoreError::ValidationError(
                "SOURCE_BINDING.json missing upstream_bindings array".to_string(),
            )
        })?;
    let s4 = upstream
        .iter()
        .find(|entry| entry.get("id").and_then(JsonValue::as_str) == Some(S4_BINDING_ID))
        .ok_or_else(|| {
            CoreError::ValidationError(format!(
                "SOURCE_BINDING.json missing upstream binding {S4_BINDING_ID:?}"
            ))
        })?;
    let s4_obj = s4.as_object().ok_or_else(|| {
        CoreError::ValidationError("S4 binding entry must be a JSON object".to_string())
    })?;
    let binding_status = s4_obj
        .get("status")
        .and_then(JsonValue::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| CoreError::ValidationError("S4 binding missing status".to_string()))?
        .to_string();
    let base_commit_binding = s4_obj
        .get("base_commit_binding")
        .and_then(JsonValue::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            CoreError::ValidationError("S4 binding missing base_commit_binding".to_string())
        })?
        .to_string();
    let vfiles = s4_obj
        .get("vendored_files")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            CoreError::ValidationError("S4 binding missing vendored_files".to_string())
        })?;
    let mut expected_csv_sha256: Option<String> = None;
    for vf in vfiles {
        let local_path = vf
            .get("local_path")
            .and_then(JsonValue::as_str)
            .unwrap_or("");
        if local_path == S4_CYCLE_SHEDDING_CSV_REL {
            let sha = vf
                .get("sha256")
                .and_then(JsonValue::as_str)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    CoreError::ValidationError(
                        "S4 vendored CSV entry missing sha256 digest".to_string(),
                    )
                })?;
            expected_csv_sha256 = Some(sha.to_string());
        }
    }
    let expected_csv_sha256 = expected_csv_sha256.ok_or_else(|| {
        CoreError::ValidationError(format!(
            "S4 binding missing vendored_files entry for {S4_CYCLE_SHEDDING_CSV_REL}"
        ))
    })?;
    Ok(S4BindingInfo {
        binding_id: S4_BINDING_ID.to_string(),
        binding_status,
        base_commit_binding,
        expected_csv_sha256,
    })
}

fn pair_json(q: &Ratio) -> JsonValue {
    let (n, d) = canonical_rational_pair(q);
    JsonValue::Array(vec![JsonValue::Str(n), JsonValue::Str(d)])
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CycleSheddingFixtureReport {
    pub status: String,
    pub binding_id: String,
    pub binding_status: String,
    pub base_commit_binding: String,
    pub fixture_path: String,
    pub fixture_sha256: String,
    pub ticks_verified: usize,
    pub shedding_ticks: usize,
    pub shedding_tick_first: Option<usize>,
    pub shedding_tick_last: Option<usize>,
    pub zero_residual_all_ticks: bool,
    pub within_csv_tolerance_all_ticks: bool,
    pub max_denominator_digits: usize,
    pub final_burden: Option<Ratio>,
    pub total_outward: Option<Ratio>,
    pub total_local: Option<Ratio>,
    pub errors: Vec<String>,
}

impl CycleSheddingFixtureReport {
    pub fn is_ok(&self) -> bool {
        self.status == "PASS" && self.errors.is_empty()
    }

    pub fn to_json(&self) -> JsonValue {
        let mut m = BTreeMap::new();
        m.insert(
            "binding_id".to_string(),
            JsonValue::from_str_val(self.binding_id.clone()),
        );
        m.insert(
            "binding_status".to_string(),
            JsonValue::from_str_val(self.binding_status.clone()),
        );
        m.insert(
            "errors".to_string(),
            JsonValue::Array(
                self.errors
                    .iter()
                    .map(|e| JsonValue::from_str_val(e.clone()))
                    .collect(),
            ),
        );
        m.insert(
            "final_burden".to_string(),
            self.final_burden
                .as_ref()
                .map_or(JsonValue::Null, pair_json),
        );
        m.insert(
            "fixture_path".to_string(),
            JsonValue::from_str_val(self.fixture_path.clone()),
        );
        m.insert(
            "fixture_sha256".to_string(),
            JsonValue::from_str_val(self.fixture_sha256.clone()),
        );
        m.insert(
            "max_denominator_digits".to_string(),
            JsonValue::from_usize(self.max_denominator_digits),
        );
        m.insert(
            "shedding_tick_first".to_string(),
            self.shedding_tick_first
                .map_or(JsonValue::Null, JsonValue::from_usize),
        );
        m.insert(
            "shedding_tick_last".to_string(),
            self.shedding_tick_last
                .map_or(JsonValue::Null, JsonValue::from_usize),
        );
        m.insert(
            "shedding_ticks".to_string(),
            JsonValue::from_usize(self.shedding_ticks),
        );
        m.insert(
            "status".to_string(),
            JsonValue::from_str_val(self.status.clone()),
        );
        m.insert(
            "ticks_verified".to_string(),
            JsonValue::from_usize(self.ticks_verified),
        );
        m.insert(
            "total_local".to_string(),
            self.total_local
                .as_ref()
                .map_or(JsonValue::Null, pair_json),
        );
        m.insert(
            "total_outward".to_string(),
            self.total_outward
                .as_ref()
                .map_or(JsonValue::Null, pair_json),
        );
        m.insert(
            "within_csv_tolerance_all_ticks".to_string(),
            JsonValue::Bool(self.within_csv_tolerance_all_ticks),
        );
        m.insert(
            "zero_residual_all_ticks".to_string(),
            JsonValue::Bool(self.zero_residual_all_ticks),
        );
        JsonValue::Object(m)
    }
}

/// Strictly verify the vendored 90-tick `wavelet_shedding_simulation.csv` fixture against
/// `SOURCE_BINDING.json` (`S4@e013401`) and exact `cycle_shedding_trace` conservation laws.
pub fn verify_s4_cycle_shedding_csv(
    csv_path: &Path,
    binding_path: &Path,
) -> CycleSheddingFixtureReport {
    let mut rep = CycleSheddingFixtureReport {
        status: "FAIL".to_string(),
        binding_id: S4_BINDING_ID.to_string(),
        binding_status: "UNVERIFIED".to_string(),
        base_commit_binding: S4_BASE_COMMIT_BINDING.to_string(),
        fixture_path: S4_CYCLE_SHEDDING_CSV_REL.to_string(),
        fixture_sha256: String::new(),
        ticks_verified: 0,
        shedding_ticks: 0,
        shedding_tick_first: None,
        shedding_tick_last: None,
        zero_residual_all_ticks: false,
        within_csv_tolerance_all_ticks: false,
        max_denominator_digits: 0,
        final_burden: None,
        total_outward: None,
        total_local: None,
        errors: Vec::new(),
    };

    let binding = match load_s4_binding(binding_path) {
        Ok(b) => b,
        Err(e) => {
            rep.errors.push(e.to_string());
            return rep;
        }
    };
    rep.binding_id = binding.binding_id;
    rep.binding_status = binding.binding_status;
    rep.base_commit_binding = binding.base_commit_binding;

    let raw_bytes = match read_regular_no_symlink(csv_path, S4_CYCLE_SHEDDING_CSV_REL) {
        Ok(b) => b,
        Err(e) => {
            rep.errors.push(e.to_string());
            return rep;
        }
    };

    let actual_sha = Sha256::digest_hex(&raw_bytes);
    rep.fixture_sha256 = actual_sha.clone();
    if actual_sha != binding.expected_csv_sha256 {
        rep.errors.push(format!(
            "SHA-256 mismatch for {S4_CYCLE_SHEDDING_CSV_REL}: expected {}, got {actual_sha}",
            binding.expected_csv_sha256
        ));
    }

    let text = match String::from_utf8(raw_bytes) {
        Ok(s) => s,
        Err(_) => {
            rep.errors.push(format!(
                "non-UTF-8 bytes in {S4_CYCLE_SHEDDING_CSV_REL}"
            ));
            return rep;
        }
    };

    let mut lines = text.lines();
    let header = lines
        .next()
        .unwrap_or("")
        .trim_end_matches('\r');
    if header != EXPECTED_CSV_HEADER {
        rep.errors.push(format!(
            "unexpected CSV header: expected {EXPECTED_CSV_HEADER:?}, got {header:?}"
        ));
        return rep;
    }

    struct CsvRow {
        input: CycleSheddingTickInput,
        csv_span: Ratio,
        csv_padar_burden: Ratio,
        csv_shed_excess: Ratio,
        csv_outward_shedding: Ratio,
        csv_local_reclosure: Ratio,
    }

    let mut rows: Vec<CsvRow> = Vec::with_capacity(90);
    for (row_idx, raw_line) in lines.enumerate() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() != 10 {
            rep.errors.push(format!(
                "CSV row {row_idx}: expected 10 columns, got {}",
                cols.len()
            ));
            return rep;
        }
        let tick_num = match cols[0].parse::<usize>() {
            Ok(t) => t,
            Err(_) => {
                rep.errors.push(format!(
                    "CSV row {row_idx}: invalid tick index {:?}",
                    cols[0]
                ));
                return rep;
            }
        };
        if tick_num != rows.len() {
            rep.errors.push(format!(
                "CSV row {}: expected tick {}, got {tick_num}",
                rows.len(),
                rows.len()
            ));
            return rep;
        }
        if cols[9] != "open" {
            rep.errors.push(format!(
                "CSV tick {tick_num}: expected state \"open\", got {:?}",
                cols[9]
            ));
            return rep;
        }
        let parse_col = |col_idx: usize, name: &str| -> CoreResult<Ratio> {
            parse_printed_rational(cols[col_idx], false, false).map_err(|e| {
                CoreError::ValidationError(format!(
                    "CSV tick {tick_num} column {name}: {e}"
                ))
            })
        };
        let parsed_row = (|| -> CoreResult<CsvRow> {
            Ok(CsvRow {
                input: CycleSheddingTickInput {
                    lead: parse_col(1, "lead")?,
                    lag: parse_col(2, "lag")?,
                    compatibility: parse_col(4, "compatibility")?,
                },
                csv_span: parse_col(3, "span")?,
                csv_padar_burden: parse_col(5, "padar_burden")?,
                csv_shed_excess: parse_col(6, "shed_excess")?,
                csv_outward_shedding: parse_col(7, "outward_shedding")?,
                csv_local_reclosure: parse_col(8, "local_reclosure")?,
            })
        })();
        match parsed_row {
            Ok(r) => rows.push(r),
            Err(e) => {
                rep.errors.push(e.to_string());
                return rep;
            }
        }
    }

    if rows.len() != 90 {
        rep.errors.push(format!(
            "expected exactly 90 CSV data rows (ticks 0..89), got {}",
            rows.len()
        ));
        return rep;
    }

    let eta = Ratio::from_i64(41, 50).expect("41/50 valid");
    let gamma = Ratio::from_i64(11, 20).expect("11/20 valid");
    let lambda_reclose = Ratio::from_i64(19, 50).expect("19/50 valid");
    let tol = Ratio::from_i64(1, 100_000_000_000_000).expect("1/10^14 valid");

    let inputs: Vec<CycleSheddingTickInput> = rows.iter().map(|r| r.input.clone()).collect();
    let trace = match cycle_shedding_trace(&Ratio::zero(), &inputs, &eta, &gamma, &lambda_reclose) {
        Ok(t) => t,
        Err(e) => {
            rep.errors.push(e.to_string());
            return rep;
        }
    };

    let mut zero_residual_all = true;
    let mut within_tol_all = true;
    let mut shedding_ticks = 0usize;
    let mut shedding_first: Option<usize> = None;
    let mut shedding_last: Option<usize> = None;
    let mut max_den_digits = 0usize;
    let mut total_outward = Ratio::zero();
    let mut total_local = Ratio::zero();

    for (tick, (step, row)) in trace.iter().zip(rows.iter()).enumerate() {
        // 1. Exact zero-residual conservation identities at every tick
        let res_ok = &step.outward_shedding + &step.local_reclosure == step.remainder_sum
            && step.remainder_sum == step.shed_excess
            && &step.padar_burden + &step.shed_excess == step.before_clipping
            && step.padar_burden <= row.input.compatibility;
        if !res_ok {
            zero_residual_all = false;
            rep.errors.push(format!(
                "tick {tick}: exact cycle-shedding conservation identity failed"
            ));
        }

        // 2. Exact shedding support 29..=39 (11 ticks) vs strict 0/1 outside
        let in_shed_window = (29..=39).contains(&tick);
        if in_shed_window {
            if !step.shed_excess.is_positive()
                || !step.outward_shedding.is_positive()
                || !step.local_reclosure.is_positive()
                || step.padar_burden != row.input.compatibility
            {
                zero_residual_all = false;
                rep.errors.push(format!(
                    "tick {tick}: expected active shedding with padar_burden == compatibility"
                ));
            } else {
                shedding_ticks += 1;
                if shedding_first.is_none() {
                    shedding_first = Some(tick);
                }
                shedding_last = Some(tick);
            }
        } else if !step.shed_excess.is_zero()
            || !step.outward_shedding.is_zero()
            || !step.local_reclosure.is_zero()
            || !step.remainder_sum.is_zero()
        {
            zero_residual_all = false;
            rep.errors.push(format!(
                "tick {tick}: expected strict 0/1 shedding outside 29..=39"
            ));
        }

        // 3. Exact rational vs CSV decimal tolerance |exact - csv| <= 1/10^14 on all 5 derived columns
        let checks = [
            ("span", &step.span, &row.csv_span),
            ("padar_burden", &step.padar_burden, &row.csv_padar_burden),
            ("shed_excess", &step.shed_excess, &row.csv_shed_excess),
            (
                "outward_shedding",
                &step.outward_shedding,
                &row.csv_outward_shedding,
            ),
            (
                "local_reclosure",
                &step.local_reclosure,
                &row.csv_local_reclosure,
            ),
        ];
        for (col_name, exact_val, csv_val) in checks {
            if (exact_val - csv_val).abs() > tol {
                within_tol_all = false;
                rep.errors.push(format!(
                    "tick {tick} column {col_name}: |exact - csv| exceeds 1/10^14 tolerance"
                ));
            }
        }

        for r in [
            &step.span,
            &step.before_clipping,
            &step.shed_excess,
            &step.padar_burden,
            &step.outward_shedding,
            &step.local_reclosure,
            &step.remainder_sum,
        ] {
            let digits = r.den().to_decimal_string().len();
            if digits > max_den_digits {
                max_den_digits = digits;
            }
        }

        total_outward = &total_outward + &step.outward_shedding;
        total_local = &total_local + &step.local_reclosure;
    }

    if shedding_ticks != 11 || shedding_first != Some(29) || shedding_last != Some(39) {
        rep.errors.push(format!(
            "expected exact 11 shedding ticks over 29..=39, got count={shedding_ticks}, first={shedding_first:?}, last={shedding_last:?}"
        ));
    }
    if max_den_digits <= 18 {
        rep.errors.push(format!(
            "expected max_denominator_digits > 18 (genuine BigInt growth), got {max_den_digits}"
        ));
    }

    rep.ticks_verified = trace.len();
    rep.shedding_ticks = shedding_ticks;
    rep.shedding_tick_first = shedding_first;
    rep.shedding_tick_last = shedding_last;
    rep.zero_residual_all_ticks = zero_residual_all;
    rep.within_csv_tolerance_all_ticks = within_tol_all;
    rep.max_denominator_digits = max_den_digits;
    rep.final_burden = trace.last().map(|s| s.padar_burden.clone());
    rep.total_outward = Some(total_outward);
    rep.total_local = Some(total_local);

    if rep.errors.is_empty() {
        rep.status = "PASS".to_string();
    }
    rep
}

// ============================================================================
// Lightweight `aod-core` Source Policy Auditor (for `policy_audit` section)
// ============================================================================

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyAuditViolation {
    pub file: String,
    pub line: usize,
    pub rule: String,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorePolicyAudit {
    pub status: String,
    pub audited_rust_files: usize,
    pub forbid_unsafe_checked_files: Vec<String>,
    pub violation_count: usize,
    pub violations: Vec<PolicyAuditViolation>,
}

impl CorePolicyAudit {
    pub fn is_ok(&self) -> bool {
        self.status == "PASS" && self.violation_count == 0 && self.violations.is_empty()
    }

    pub fn to_json(&self) -> JsonValue {
        let mut m = BTreeMap::new();
        m.insert(
            "audited_rust_files".to_string(),
            JsonValue::from_usize(self.audited_rust_files),
        );
        m.insert(
            "forbid_unsafe_checked_files".to_string(),
            JsonValue::Array(
                self.forbid_unsafe_checked_files
                    .iter()
                    .map(|s| JsonValue::from_str_val(s.clone()))
                    .collect(),
            ),
        );
        m.insert(
            "status".to_string(),
            JsonValue::from_str_val(self.status.clone()),
        );
        m.insert(
            "violation_count".to_string(),
            JsonValue::from_usize(self.violation_count),
        );
        m.insert(
            "violations".to_string(),
            JsonValue::Array(
                self.violations
                    .iter()
                    .map(|v| {
                        let mut vm = BTreeMap::new();
                        vm.insert("detail".to_string(), JsonValue::from_str_val(v.detail.clone()));
                        vm.insert("file".to_string(), JsonValue::from_str_val(v.file.clone()));
                        vm.insert("line".to_string(), JsonValue::from_usize(v.line));
                        vm.insert("rule".to_string(), JsonValue::from_str_val(v.rule.clone()));
                        JsonValue::Object(vm)
                    })
                    .collect(),
            ),
        );
        JsonValue::Object(m)
    }
}

fn collect_sorted_rs_files(base: &Path, dir: &Path, out: &mut Vec<String>) -> CoreResult<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|_| CoreError::ValidationError("failed reading aod-core source dir".to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CoreError::ValidationError("failed iterating aod-core source dir".to_string()))?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let ft = entry
            .file_type()
            .map_err(|_| CoreError::ValidationError("failed inspecting file type".to_string()))?;
        if ft.is_symlink() {
            return Err(CoreError::ValidationError(
                "symlink forbidden inside aod-core source tree".to_string(),
            ));
        }
        let path = entry.path();
        if ft.is_dir() {
            collect_sorted_rs_files(base, &path, out)?;
        } else if ft.is_file() && path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let rel = path
                .strip_prefix(base)
                .map_err(|_| CoreError::ValidationError("invalid relative source path".to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            out.push(rel);
        }
    }
    Ok(())
}

/// Audit `crates/aod-core/` for zero floats, `#![forbid(unsafe_code)]`, zero `unsafe` tokens,
/// zero symlinks, and strictly empty `[dependencies]` in `Cargo.toml`.
pub fn audit_aod_core_policy(crate_root: &Path) -> CorePolicyAudit {
    let mut violations: Vec<PolicyAuditViolation> = Vec::new();
    let mut forbid_checked: Vec<String> = Vec::new();
    let mut rs_rel_files: Vec<String> = Vec::new();

    let src_dir = crate_root.join("src");
    if let Err(e) = collect_sorted_rs_files(crate_root, &src_dir, &mut rs_rel_files) {
        violations.push(PolicyAuditViolation {
            file: "src".to_string(),
            line: 0,
            rule: "crate_tree".to_string(),
            detail: e.to_string(),
        });
    }
    rs_rel_files.sort();

    // Build forbidden tokens dynamically so this file remains clean under static source audits.
    let f16_tok = ["f", "16"].concat();
    let f32_tok = ["f", "32"].concat();
    let f64_tok = ["f", "64"].concat();
    let f128_tok = ["f", "128"].concat();
    let unsafe_tok = ["un", "safe"].concat();
    let forbid_attr = ["#![forbid(", "un", "safe_code)]"].concat();
    let unsafe_code_tok = ["un", "safe_code"].concat();

    // Check Cargo.toml has zero external dependencies
    let cargo_toml_path = crate_root.join("Cargo.toml");
    match read_regular_no_symlink(&cargo_toml_path, "Cargo.toml") {
        Ok(bytes) => {
            if let Ok(text) = String::from_utf8(bytes) {
                if let Some(deps_sec) = text.split("[dependencies]").nth(1) {
                    let before_next = deps_sec.split('[').next().unwrap_or("");
                    for (idx, line) in before_next.lines().enumerate() {
                        let t = line.trim();
                        if !t.is_empty() && !t.starts_with('#') {
                            violations.push(PolicyAuditViolation {
                                file: "Cargo.toml".to_string(),
                                line: idx + 1,
                                rule: "zero_external_deps".to_string(),
                                detail: format!("forbidden dependency entry in aod-core: {t}"),
                            });
                        }
                    }
                }
            } else {
                violations.push(PolicyAuditViolation {
                    file: "Cargo.toml".to_string(),
                    line: 0,
                    rule: "utf8".to_string(),
                    detail: "Cargo.toml is not valid UTF-8".to_string(),
                });
            }
        }
        Err(e) => {
            violations.push(PolicyAuditViolation {
                file: "Cargo.toml".to_string(),
                line: 0,
                rule: "manifest".to_string(),
                detail: e.to_string(),
            });
        }
    }

    for rel in &rs_rel_files {
        let full = crate_root.join(rel);
        let content = match read_regular_no_symlink(&full, rel) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(s) => s,
                Err(_) => {
                    violations.push(PolicyAuditViolation {
                        file: rel.clone(),
                        line: 0,
                        rule: "utf8".to_string(),
                        detail: "Rust source file is not valid UTF-8".to_string(),
                    });
                    continue;
                }
            },
            Err(e) => {
                violations.push(PolicyAuditViolation {
                    file: rel.clone(),
                    line: 0,
                    rule: "io".to_string(),
                    detail: e.to_string(),
                });
                continue;
            }
        };

        if rel == "src/lib.rs" {
            forbid_checked.push(rel.clone());
            if !content.contains(&forbid_attr) {
                violations.push(PolicyAuditViolation {
                    file: rel.clone(),
                    line: 1,
                    rule: "forbid_unsafe".to_string(),
                    detail: "src/lib.rs missing active forbid(unsafe_code) attribute".to_string(),
                });
            }
        }

        for (line_idx, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            let mut ident = String::new();
            for ch in trimmed.chars().chain(std::iter::once(' ')) {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    ident.push(ch);
                } else {
                    if ident == f16_tok
                        || ident == f32_tok
                        || ident == f64_tok
                        || ident == f128_tok
                    {
                        violations.push(PolicyAuditViolation {
                            file: rel.clone(),
                            line: line_idx + 1,
                            rule: "zero_float".to_string(),
                            detail: format!("forbidden float identifier `{ident}`"),
                        });
                    }
                    if ident == unsafe_tok && !trimmed.contains(&unsafe_code_tok) {
                        violations.push(PolicyAuditViolation {
                            file: rel.clone(),
                            line: line_idx + 1,
                            rule: "zero_unsafe".to_string(),
                            detail: format!("forbidden `{unsafe_tok}` keyword in aod-core"),
                        });
                    }
                    ident.clear();
                }
            }
        }
    }

    let violation_count = violations.len();
    CorePolicyAudit {
        status: if violation_count == 0 && !rs_rel_files.is_empty() {
            "PASS".to_string()
        } else {
            "FAIL".to_string()
        },
        audited_rust_files: rs_rel_files.len(),
        forbid_unsafe_checked_files: forbid_checked,
        violation_count,
        violations,
    }
}

// ============================================================================
// Canonical `aod.s4.rust.report.v1` Report Assembly & Pretty Serialization
// ============================================================================

fn not_requested_section() -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert(
        "status".to_string(),
        JsonValue::Str("NOT_REQUESTED".to_string()),
    );
    JsonValue::Object(m)
}

fn vectors_section_sanitized(
    vectors: &S2VerifyReport,
    extra_roots: &[(&Path, &str)],
) -> JsonValue {
    let mut digests = BTreeMap::new();
    for (k, v) in &vectors.file_digests {
        digests.insert(k.clone(), JsonValue::from_str_val(v.clone()));
    }
    let mut sci_summary = BTreeMap::new();
    for (k, &v) in &vectors.scientific_status_summary {
        sci_summary.insert(k.clone(), JsonValue::from_usize(v));
    }
    let failures = vectors
        .failures
        .iter()
        .map(|f| {
            let mut fm = BTreeMap::new();
            fm.insert(
                "case_id".to_string(),
                JsonValue::from_str_val(sanitize_error(&f.case_id, extra_roots)),
            );
            fm.insert(
                "error".to_string(),
                JsonValue::from_str_val(sanitize_error(&f.error, extra_roots)),
            );
            JsonValue::Object(fm)
        })
        .collect();

    let mut m = BTreeMap::new();
    m.insert("failed_vectors".to_string(), JsonValue::from_usize(vectors.failed_vectors));
    m.insert("failures".to_string(), JsonValue::Array(failures));
    m.insert("file_digests".to_string(), JsonValue::Object(digests));
    m.insert("passed_vectors".to_string(), JsonValue::from_usize(vectors.passed_vectors));
    m.insert(
        "rational_core_count".to_string(),
        JsonValue::from_usize(vectors.rational_core_count),
    );
    m.insert(
        "scientific_status_summary".to_string(),
        JsonValue::Object(sci_summary),
    );
    m.insert("status".to_string(), JsonValue::from_str_val(vectors.status.clone()));
    m.insert(
        "symbolic_and_display_count".to_string(),
        JsonValue::from_usize(vectors.symbolic_and_display_count),
    );
    m.insert("total_vectors".to_string(), JsonValue::from_usize(vectors.total_vectors));
    JsonValue::Object(m)
}

/// Assemble and validate a deterministic canonical `aod.s4.rust.report.v1` JSON document.
///
/// All error and violation strings are sanitized via [`sanitize_error`] BEFORE running
/// [`validate_portable_report`], ensuring failing runs still emit clean portable `FAIL` JSON.
pub fn build_s4_report(
    vectors: &S2VerifyReport,
    fixture: &CycleSheddingFixtureReport,
    policy: Option<&CorePolicyAudit>,
    extra_roots: &[(&Path, &str)],
) -> CoreResult<(JsonValue, bool)> {
    let mut any_fail = false;

    if !vectors.is_ok() {
        any_fail = true;
    }
    let vectors_json = vectors_section_sanitized(vectors, extra_roots);

    let mut clean_fixture = fixture.clone();
    clean_fixture.errors = clean_fixture
        .errors
        .iter()
        .map(|e| sanitize_error(e, extra_roots))
        .collect();
    if !clean_fixture.is_ok() {
        any_fail = true;
    }
    let fixture_json = clean_fixture.to_json();

    let policy_json = if let Some(p) = policy {
        let mut clean_policy = p.clone();
        for v in &mut clean_policy.violations {
            v.detail = sanitize_error(&v.detail, extra_roots);
            v.file = sanitize_error(&v.file, extra_roots);
        }
        if !clean_policy.is_ok() {
            any_fail = true;
        }
        clean_policy.to_json()
    } else {
        not_requested_section()
    };

    let overall_status = if any_fail { "FAIL" } else { "PASS" };

    let mut report = BTreeMap::new();
    report.insert(
        "base_commit_binding".to_string(),
        JsonValue::from_str_val(clean_fixture.base_commit_binding.clone()),
    );
    report.insert(
        "crate_name".to_string(),
        JsonValue::from_str_val(S4_CRATE_NAME),
    );
    report.insert(
        "crate_version".to_string(),
        JsonValue::from_str_val(S4_CRATE_VERSION),
    );
    report.insert("cycle_shedding_fixture".to_string(), fixture_json);
    report.insert(
        "overall_status".to_string(),
        JsonValue::from_str_val(overall_status),
    );
    report.insert("policy_audit".to_string(), policy_json);
    report.insert(
        "schema".to_string(),
        JsonValue::from_str_val(S4_REPORT_SCHEMA),
    );
    report.insert("vectors_verification".to_string(), vectors_json);

    let root = JsonValue::Object(report);
    validate_portable_report(&root, "$")?;
    Ok((root, !any_fail))
}

/// Validate portability and serialize a canonical `aod.s4.rust.report.v1` JSON document
/// using `JsonValue::to_pretty_utf8_json()` (`sort_keys=True`, indent 2, trailing `\n`).
pub fn serialize_s4_report(report: &JsonValue) -> CoreResult<String> {
    validate_portable_report(report, "$")?;
    Ok(report.to_pretty_utf8_json())
}
