//! Stage 2 (`S2`) / Stage 4 (`S4`) cross-language golden-vector loader, schema validator,
//! 30-vector dispatcher, SHA-256 provenance checker, and portable zero-float verifier report.
//!
//! Strictly `std`-only, zero floats, `#![forbid(unsafe_code)]`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::bigint::BigInt;
use crate::error::{CoreError, CoreResult};
use crate::json::{parse_strict_json, JsonValue};
use crate::ratio::Ratio;
use crate::s2_kernel::{
    awareness_current_update, awareness_mindfulness_step, brownian_3d_delta3_msb_x,
    canonical_rational_pair, closure_wave_rd_rcd, cycle_shedding_step, cycle_shedding_trace,
    cyclotomic_z12_overtone, dec_field_tunnelling_window_clip, dec_hinge_slide_normalize,
    dec_normalize_one_node, finite_stokes_cut_balance, higgs_lhc_rounded_display,
    higgs_trace_score_and_freeze, lensing_swells_delta3_and_tau_discrepancy,
    map21_nonlinear_finite_law_variance, parse_canonical_rational_pair, parse_exact_int_str,
    parse_printed_rational, printed_token_half_ulp_enclosure, q4_hamming_and_neighbors,
    relational_cadence_and_lag, run_tumble_3d_delta3_lsb_x, sadar_afc_flux_antisymmetry,
    sparc_exact_bin_eval, temporal_metrology_hellwig_mean, temporal_metrology_phase_si,
    temporal_metrology_signed_advance, z4_dft_gaussian, CycleSheddingOutput,
    CycleSheddingTickInput, DecOneNodeRow, HiggsCandidateInput, HingeSlideBranch,
    Map21SupportPoint, OrientedChannel, PhaseInterval, MAX_CYCLE_SHEDDING_TICKS,
    MAX_ROUNDING_DECIMALS,
};
use crate::sha256::Sha256;

pub const VECTOR_SCHEMA: &str = "aod.s4.vector.v1";
pub const S2_REPORT_SCHEMA: &str = "aod.dec.s2.verify.v1";
pub const S2_BINDING_ID: &str = "S2@3122c94";
pub const RATIONAL_CORE_FILENAME: &str = "rational_core.jsonl";
pub const SYMBOLIC_DISPLAY_FILENAME: &str = "symbolic_and_display.jsonl";
pub const RATIONAL_CORE_REL: &str = "bindings/aod/s2_vectors/rational_core.jsonl";
pub const SYMBOLIC_DISPLAY_REL: &str = "bindings/aod/s2_vectors/symbolic_and_display.jsonl";

pub const VALID_SCIENTIFIC_STATUSES: [&str; 4] = [
    "AUTHENTICATED_EXACT",
    "CONFORMANT_DECLARED_FIXTURE",
    "RETAINED_DATA_CONFORMANT",
    "UNRESOLVED_PROVENANCE_QUARANTINED",
];

pub const VALID_RUST_INT_TYPES: [&str; 4] = ["i32", "i64", "i128", "BigInt"];

const VECTOR_REQUIRED_KEYS: [&str; 11] = [
    "schema_version",
    "case_id",
    "source_binding",
    "operation",
    "domain",
    "inputs",
    "expected_outcome",
    "evidence_role",
    "scientific_status",
    "oracle_basis",
    "integer_encoding",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S2VectorFailure {
    pub case_id: String,
    pub error: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S2VerifyReport {
    pub schema: String,
    pub status: String,
    pub binding_id: String,
    pub binding_status: String,
    pub upstream_commit: String,
    pub upstream_tree: String,
    pub base_commit_binding: String,
    pub rational_core_count: usize,
    pub symbolic_and_display_count: usize,
    pub total_vectors: usize,
    pub passed_vectors: usize,
    pub failed_vectors: usize,
    pub file_digests: BTreeMap<String, String>,
    pub scientific_status_summary: BTreeMap<String, usize>,
    pub failures: Vec<S2VectorFailure>,
}

impl S2VerifyReport {
    pub fn is_ok(&self) -> bool {
        self.status == "PASS"
            && self.failed_vectors == 0
            && self.failures.is_empty()
            && self.total_vectors == 30
            && self.passed_vectors == 30
    }

    /// Serialize to deterministic, zero-float, portable JSON (`BTreeMap` sorted keys, zero host paths,
    /// zero timestamps).
    pub fn to_json(&self) -> JsonValue {
        let mut root = BTreeMap::new();
        root.insert(
            "base_commit_binding".to_string(),
            JsonValue::from_str_val(self.base_commit_binding.clone()),
        );
        root.insert(
            "binding_id".to_string(),
            JsonValue::from_str_val(self.binding_id.clone()),
        );
        root.insert(
            "binding_status".to_string(),
            JsonValue::from_str_val(self.binding_status.clone()),
        );
        root.insert(
            "failed_vectors".to_string(),
            JsonValue::from_usize(self.failed_vectors),
        );
        root.insert(
            "failures".to_string(),
            JsonValue::Array(
                self.failures
                    .iter()
                    .map(|f| {
                        let mut fm = BTreeMap::new();
                        fm.insert("case_id".to_string(), JsonValue::from_str_val(f.case_id.clone()));
                        fm.insert("error".to_string(), JsonValue::from_str_val(f.error.clone()));
                        JsonValue::Object(fm)
                    })
                    .collect(),
            ),
        );
        let mut fdig = BTreeMap::new();
        for (k, v) in &self.file_digests {
            fdig.insert(k.clone(), JsonValue::from_str_val(v.clone()));
        }
        root.insert("file_digests".to_string(), JsonValue::Object(fdig));
        root.insert(
            "passed_vectors".to_string(),
            JsonValue::from_usize(self.passed_vectors),
        );
        root.insert(
            "rational_core_count".to_string(),
            JsonValue::from_usize(self.rational_core_count),
        );
        root.insert(
            "schema".to_string(),
            JsonValue::from_str_val(self.schema.clone()),
        );
        let mut ssum = BTreeMap::new();
        for (k, &v) in &self.scientific_status_summary {
            ssum.insert(k.clone(), JsonValue::from_usize(v));
        }
        root.insert("scientific_status_summary".to_string(), JsonValue::Object(ssum));
        root.insert(
            "status".to_string(),
            JsonValue::from_str_val(self.status.clone()),
        );
        root.insert(
            "symbolic_and_display_count".to_string(),
            JsonValue::from_usize(self.symbolic_and_display_count),
        );
        root.insert(
            "total_vectors".to_string(),
            JsonValue::from_usize(self.total_vectors),
        );
        root.insert(
            "upstream_commit".to_string(),
            JsonValue::from_str_val(self.upstream_commit.clone()),
        );
        root.insert(
            "upstream_tree".to_string(),
            JsonValue::from_str_val(self.upstream_tree.clone()),
        );
        JsonValue::Object(root)
    }
}

// ============================================================================
// Safe Symlink-Rejecting File Reader (`P1-2`)
// ============================================================================

/// Read a regular file (bounded to 16 MiB) while strictly rejecting symlinks via `symlink_metadata`.
/// Error messages NEVER embed host paths — only the portable `label`.
pub(crate) fn read_regular_no_symlink(path: &Path, label: &str) -> CoreResult<Vec<u8>> {
    const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
    let meta = std::fs::symlink_metadata(path)
        .map_err(|_| CoreError::ValidationError(format!("missing or unreadable file: {label}")))?;
    if meta.file_type().is_symlink() {
        return Err(CoreError::ValidationError(format!(
            "symlink forbidden for file: {label}"
        )));
    }
    if !meta.is_file() {
        return Err(CoreError::ValidationError(format!(
            "path is not a regular file: {label}"
        )));
    }
    if meta.len() > MAX_FILE_BYTES as u64 {
        return Err(CoreError::ValidationError(format!(
            "file exceeds 16 MiB limit: {label}"
        )));
    }
    let file = File::open(path)
        .map_err(|_| CoreError::ValidationError(format!("failed opening regular file: {label}")))?;
    let mut buf = Vec::new();
    file.take((MAX_FILE_BYTES + 1) as u64)
        .read_to_end(&mut buf)
        .map_err(|_| CoreError::ValidationError(format!("failed reading regular file: {label}")))?;
    if buf.len() > MAX_FILE_BYTES {
        return Err(CoreError::ValidationError(format!(
            "file exceeds 16 MiB limit: {label}"
        )));
    }
    Ok(buf)
}

fn check_dir_no_symlink(path: &Path, label: &str) -> CoreResult<()> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| {
        CoreError::ValidationError(format!("missing or unreadable directory: {label}"))
    })?;
    if meta.file_type().is_symlink() {
        return Err(CoreError::ValidationError(format!(
            "symlink forbidden for directory: {label}"
        )));
    }
    if !meta.is_dir() {
        return Err(CoreError::ValidationError(format!(
            "path is not a directory: {label}"
        )));
    }
    Ok(())
}

// ============================================================================
// Strict JSON Typed Extractors (`P1-2`, `P1-3`)
// ============================================================================

fn pair_json(q: &Ratio) -> JsonValue {
    let (n, d) = canonical_rational_pair(q);
    JsonValue::Array(vec![JsonValue::Str(n), JsonValue::Str(d)])
}

fn get_obj<'a>(val: &'a JsonValue, name: &str) -> CoreResult<&'a BTreeMap<String, JsonValue>> {
    val.as_object()
        .ok_or_else(|| CoreError::ValidationError(format!("{name} must be a JSON object")))
}

fn get_arr<'a>(val: &'a JsonValue, name: &str) -> CoreResult<&'a [JsonValue]> {
    val.as_array()
        .ok_or_else(|| CoreError::ValidationError(format!("{name} must be a JSON array")))
}

fn get_field<'a>(
    obj: &'a BTreeMap<String, JsonValue>,
    key: &str,
    ctx: &str,
) -> CoreResult<&'a JsonValue> {
    obj.get(key)
        .ok_or_else(|| CoreError::ValidationError(format!("{ctx} missing required field {key:?}")))
}

fn get_non_empty_str<'a>(
    obj: &'a BTreeMap<String, JsonValue>,
    key: &str,
    ctx: &str,
) -> CoreResult<&'a str> {
    let v = get_field(obj, key, ctx)?;
    let s = v.as_str().ok_or_else(|| {
        CoreError::ValidationError(format!("{ctx}.{key} must be a string"))
    })?;
    if s.is_empty() {
        return Err(CoreError::ValidationError(format!(
            "{ctx}.{key} must be non-empty"
        )));
    }
    Ok(s)
}

/// Strictly require a JSON exact integer (explicitly rejecting `JsonValue::Bool` and all other variants).
pub fn require_exact_int_json(val: &JsonValue, name: &str) -> CoreResult<BigInt> {
    match val {
        JsonValue::Int(n) => Ok(n.clone()),
        JsonValue::Bool(_) => Err(CoreError::DomainError(format!(
            "{name} must be an exact int, got bool"
        ))),
        _ => Err(CoreError::DomainError(format!(
            "{name} must be an exact int"
        ))),
    }
}

fn require_i64_json(val: &JsonValue, name: &str) -> CoreResult<i64> {
    let n = require_exact_int_json(val, name)?;
    n.to_i64()
        .ok_or_else(|| CoreError::Overflow(format!("{name} out of i64 range: {n}")))
}

fn require_usize_json(val: &JsonValue, name: &str) -> CoreResult<usize> {
    let n = require_exact_int_json(val, name)?;
    let u = n
        .to_u64()
        .ok_or_else(|| CoreError::DomainError(format!("{name} must be non-negative int: {n}")))?;
    usize::try_from(u)
        .map_err(|_| CoreError::Overflow(format!("{name} out of usize range: {u}")))
}

/// Strictly parse a JSON canonical rational pair `["num", "den"]`.
pub fn parse_canonical_rational_pair_json(val: &JsonValue, name: &str) -> CoreResult<Ratio> {
    let arr = get_arr(val, name)?;
    if arr.len() != 2 {
        return Err(CoreError::DomainError(format!(
            "{name} must be a 2-element canonical rational pair"
        )));
    }
    let num_s = arr[0]
        .as_str()
        .ok_or_else(|| CoreError::DomainError(format!("{name}[0] must be a canonical integer string")))?;
    let den_s = arr[1]
        .as_str()
        .ok_or_else(|| CoreError::DomainError(format!("{name}[1] must be a canonical integer string")))?;
    parse_canonical_rational_pair(num_s, den_s)
}

/// Parse either a canonical `["num", "den"]` JSON pair OR a printed decimal string
/// (`parse_printed_rational(s, false, false)`), matching upstream S4 `ops.rs:211-221`.
pub fn parse_pair_or_printed_decimal_json(val: &JsonValue, name: &str) -> CoreResult<Ratio> {
    match val {
        JsonValue::Array(_) => parse_canonical_rational_pair_json(val, name),
        JsonValue::Str(s) => parse_printed_rational(s, false, false),
        _ => Err(CoreError::DomainError(format!(
            "{name} must be a canonical rational pair or decimal string"
        ))),
    }
}

fn cycle_shedding_step_to_json(out: &CycleSheddingOutput) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("span".to_string(), pair_json(&out.span));
    m.insert(
        "before_clipping".to_string(),
        pair_json(&out.before_clipping),
    );
    m.insert("shed_excess".to_string(), pair_json(&out.shed_excess));
    m.insert("padar_burden".to_string(), pair_json(&out.padar_burden));
    m.insert(
        "outward_shedding".to_string(),
        pair_json(&out.outward_shedding),
    );
    m.insert(
        "local_reclosure".to_string(),
        pair_json(&out.local_reclosure),
    );
    m.insert("remainder_sum".to_string(), pair_json(&out.remainder_sum));
    JsonValue::Object(m)
}

// ============================================================================
// Full 11-Key `aod.s4.vector.v1` Schema Validator (`P1-3`)
// ============================================================================

/// Validate one golden vector record strictly against `aod.s4.vector.v1`.
pub fn validate_vector_record(record: &JsonValue) -> CoreResult<()> {
    let obj = get_obj(record, "vector record")?;
    let expected_keys: BTreeSet<&str> = VECTOR_REQUIRED_KEYS.iter().copied().collect();
    let actual_keys: BTreeSet<&str> = obj.keys().map(String::as_str).collect();
    let missing: Vec<&str> = expected_keys.difference(&actual_keys).copied().collect();
    if !missing.is_empty() {
        return Err(CoreError::ValidationError(format!(
            "vector record missing required keys: {missing:?}"
        )));
    }
    let extra: Vec<&str> = actual_keys.difference(&expected_keys).copied().collect();
    if !extra.is_empty() {
        return Err(CoreError::ValidationError(format!(
            "vector record contains unexpected keys: {extra:?}"
        )));
    }

    let sv = get_non_empty_str(obj, "schema_version", "vector")?;
    if sv != VECTOR_SCHEMA {
        return Err(CoreError::ValidationError(format!(
            "unexpected vector schema_version: {sv:?}"
        )));
    }

    let case_id = get_non_empty_str(obj, "case_id", "vector")?;
    let _ = get_non_empty_str(obj, "source_binding", "vector")?;
    let operation = get_non_empty_str(obj, "operation", "vector")?;
    let _ = get_non_empty_str(obj, "domain", "vector")?;
    let _ = get_non_empty_str(obj, "evidence_role", "vector")?;
    let oracle_basis = get_non_empty_str(obj, "oracle_basis", "vector")?;
    if oracle_basis == "candidate_snapshot" {
        return Err(CoreError::ValidationError(
            "circular candidate_snapshot oracle_basis is forbidden".to_string(),
        ));
    }

    let sci_status = get_non_empty_str(obj, "scientific_status", "vector")?;
    if !VALID_SCIENTIFIC_STATUSES.contains(&sci_status) {
        return Err(CoreError::ValidationError(format!(
            "invalid scientific_status for {case_id}: {sci_status:?}"
        )));
    }
    if (case_id == "RC-020" || operation == "lensing_swells_delta3_and_tau_discrepancy")
        && sci_status != "UNRESOLVED_PROVENANCE_QUARANTINED"
    {
        return Err(CoreError::ValidationError(format!(
            "{case_id} ({operation}) must preserve scientific_status UNRESOLVED_PROVENANCE_QUARANTINED, got {sci_status:?}"
        )));
    }

    let _ = get_obj(get_field(obj, "inputs", "vector")?, "vector.inputs")?;

    let exp = get_obj(
        get_field(obj, "expected_outcome", "vector")?,
        "vector.expected_outcome",
    )?;
    let exp_status = get_non_empty_str(exp, "status", "expected_outcome")?;
    match exp_status {
        "OK" => {
            let _ = get_field(exp, "exact_value", "expected_outcome")?;
        }
        "REJECT" => {
            let _ = get_non_empty_str(exp, "expected_error_class", "expected_outcome")?;
            let _ = get_non_empty_str(exp, "expected_status", "expected_outcome")?;
        }
        other => {
            return Err(CoreError::ValidationError(format!(
                "invalid expected_outcome.status: {other:?}"
            )));
        }
    }

    let enc = get_obj(
        get_field(obj, "integer_encoding", "vector")?,
        "vector.integer_encoding",
    )?;
    let _ = get_non_empty_str(enc, "format", "integer_encoding")?;
    let rust_min = get_non_empty_str(enc, "rust_min_int_type", "integer_encoding")?;
    if !VALID_RUST_INT_TYPES.contains(&rust_min) {
        return Err(CoreError::ValidationError(format!(
            "invalid rust_min_int_type: {rust_min:?}"
        )));
    }
    let max_digits = require_exact_int_json(
        get_field(enc, "max_abs_integer_digits", "integer_encoding")?,
        "integer_encoding.max_abs_integer_digits",
    )?;
    if max_digits < BigInt::one() {
        return Err(CoreError::ValidationError(format!(
            "integer_encoding.max_abs_integer_digits must be >= 1, got {max_digits}"
        )));
    }
    let _ = get_non_empty_str(enc, "overflow_note", "integer_encoding")?;

    Ok(())
}

// ============================================================================
// Operation Dispatch (24 Positive + 3 Typed-Reject = 27 Distinct Operations)
// ============================================================================

fn parse_phase_interval(val: &JsonValue, name: &str) -> CoreResult<PhaseInterval> {
    let obj = get_obj(val, name)?;
    Ok(PhaseInterval {
        winding: require_exact_int_json(get_field(obj, "winding", name)?, &format!("{name}.winding"))?,
        phase_start: parse_canonical_rational_pair_json(
            get_field(obj, "phase_start", name)?,
            &format!("{name}.phase_start"),
        )?,
        phase_end: parse_canonical_rational_pair_json(
            get_field(obj, "phase_end", name)?,
            &format!("{name}.phase_end"),
        )?,
    })
}

/// Evaluate one positive S2 operation from JSON inputs into canonical exact JSON output.
pub fn eval_positive_operation(
    operation: &str,
    inputs: &BTreeMap<String, JsonValue>,
) -> CoreResult<JsonValue> {
    match operation {
        "dec_normalize_one_node" => {
            let raw_rows = get_arr(get_field(inputs, "rows", operation)?, "rows")?;
            let mut rows = Vec::with_capacity(raw_rows.len());
            for (idx, rv) in raw_rows.iter().enumerate() {
                let ro = get_obj(rv, &format!("rows[{idx}]"))?;
                let nbr_arr = get_arr(get_field(ro, "neighbor", "row")?, "neighbor")?;
                if nbr_arr.len() != 4 {
                    return Err(CoreError::DomainError("non-H1 Q4 neighbor length".to_string()));
                }
                let neighbor = [
                    require_i64_json(&nbr_arr[0], "neighbor[0]")?,
                    require_i64_json(&nbr_arr[1], "neighbor[1]")?,
                    require_i64_json(&nbr_arr[2], "neighbor[2]")?,
                    require_i64_json(&nbr_arr[3], "neighbor[3]")?,
                ];
                rows.push(DecOneNodeRow {
                    admissible: require_exact_int_json(
                        get_field(ro, "admissible", "row")?,
                        "admissible",
                    )?,
                    weight: require_exact_int_json(get_field(ro, "weight", "row")?, "weight")?,
                    state: require_i64_json(get_field(ro, "state", "row")?, "state")?,
                    neighbor,
                });
            }
            let out = dec_normalize_one_node(&rows)?;
            let mut m = BTreeMap::new();
            m.insert("Z".to_string(), JsonValue::Int(out.z_total));
            m.insert(
                "probabilities".to_string(),
                JsonValue::Array(out.probabilities.iter().map(pair_json).collect()),
            );
            m.insert("sum".to_string(), pair_json(&out.sum));
            m.insert(
                "routes".to_string(),
                JsonValue::Array(
                    out.routes
                        .into_iter()
                        .map(|r| JsonValue::Str(r.to_string()))
                        .collect(),
                ),
            );
            Ok(JsonValue::Object(m))
        }

        "dec_hinge_slide_normalize" => {
            let raw = get_arr(get_field(inputs, "branches", operation)?, "branches")?;
            let mut branches = Vec::with_capacity(raw.len());
            for (idx, bv) in raw.iter().enumerate() {
                let bo = get_obj(bv, &format!("branches[{idx}]"))?;
                branches.push(HingeSlideBranch {
                    weight: parse_canonical_rational_pair_json(
                        get_field(bo, "weight", "branch")?,
                        "weight",
                    )?,
                    route: get_non_empty_str(bo, "route", "branch")?.to_string(),
                });
            }
            let out = dec_hinge_slide_normalize(&branches)?;
            let mut m = BTreeMap::new();
            m.insert("Z".to_string(), pair_json(&out.z_total));
            m.insert(
                "probabilities".to_string(),
                JsonValue::Array(out.probabilities.iter().map(pair_json).collect()),
            );
            m.insert("sum_returned".to_string(), pair_json(&out.sum_returned));
            m.insert("sum_outgoing".to_string(), pair_json(&out.sum_outgoing));
            Ok(JsonValue::Object(m))
        }

        "dec_field_tunnelling_window_clip" => {
            let direct_adm =
                require_exact_int_json(get_field(inputs, "direct_adm", operation)?, "direct_adm")?;
            let slide_weight = parse_canonical_rational_pair_json(
                get_field(inputs, "slide_weight", operation)?,
                "slide_weight",
            )?;
            let z_val =
                parse_canonical_rational_pair_json(get_field(inputs, "Z", operation)?, "Z")?;
            let rd_val = require_exact_int_json(get_field(inputs, "RD", operation)?, "RD")?;
            let omega = require_exact_int_json(get_field(inputs, "omega", operation)?, "omega")?;
            let c3 = require_exact_int_json(get_field(inputs, "C3", operation)?, "C3")?;
            let out = dec_field_tunnelling_window_clip(
                &direct_adm,
                &slide_weight,
                &z_val,
                &rd_val,
                &omega,
                &c3,
            )?;
            let mut m = BTreeMap::new();
            m.insert("P".to_string(), pair_json(&out.p_val));
            m.insert("rhoD_omega".to_string(), JsonValue::Int(out.rho_d_omega));
            m.insert("pD".to_string(), JsonValue::Int(out.p_d));
            m.insert(
                "tunnel_contribution".to_string(),
                pair_json(&out.tunnel_contribution),
            );
            Ok(JsonValue::Object(m))
        }

        "sadar_afc_flux_antisymmetry" => {
            let raw_mem = get_arr(get_field(inputs, "memory", operation)?, "memory")?;
            let mut mem = Vec::with_capacity(raw_mem.len());
            for (idx, v) in raw_mem.iter().enumerate() {
                mem.push(parse_canonical_rational_pair_json(
                    v,
                    &format!("memory[{idx}]"),
                )?);
            }
            let raw_mat = get_arr(get_field(inputs, "matrix", operation)?, "matrix")?;
            let mut mat = Vec::with_capacity(raw_mat.len());
            for (i, rv) in raw_mat.iter().enumerate() {
                let row_arr = get_arr(rv, &format!("matrix[{i}]"))?;
                let mut row = Vec::with_capacity(row_arr.len());
                for (j, cell) in row_arr.iter().enumerate() {
                    row.push(parse_canonical_rational_pair_json(
                        cell,
                        &format!("matrix[{i}][{j}]"),
                    )?);
                }
                mat.push(row);
            }
            let out = sadar_afc_flux_antisymmetry(&mem, &mat)?;
            let mut m = BTreeMap::new();
            m.insert(
                "flux".to_string(),
                JsonValue::Array(
                    out.flux
                        .iter()
                        .map(|row| JsonValue::Array(row.iter().map(pair_json).collect()))
                        .collect(),
                ),
            );
            m.insert("antisymmetric".to_string(), JsonValue::Bool(out.antisymmetric));
            Ok(JsonValue::Object(m))
        }

        "q4_hamming_and_neighbors" => {
            let v = require_exact_int_json(get_field(inputs, "vertex", operation)?, "vertex")?;
            let t = require_exact_int_json(get_field(inputs, "target", operation)?, "target")?;
            let w = require_exact_int_json(get_field(inputs, "width", operation)?, "width")?;
            let out = q4_hamming_and_neighbors(&v, &t, &w)?;
            let mut m = BTreeMap::new();
            m.insert(
                "hamming_distance".to_string(),
                JsonValue::from_usize(out.hamming_distance),
            );
            m.insert("is_h1_edge".to_string(), JsonValue::Bool(out.is_h1_edge));
            m.insert(
                "neighbors".to_string(),
                JsonValue::Array(out.neighbors.into_iter().map(JsonValue::Int).collect()),
            );
            Ok(JsonValue::Object(m))
        }

        "closure_wave_rd_rcd" => {
            let p = require_exact_int_json(get_field(inputs, "p", operation)?, "p")?;
            let q = require_exact_int_json(get_field(inputs, "q", operation)?, "q")?;
            let hinges = require_exact_int_json(get_field(inputs, "hinges", operation)?, "hinges")?;
            let raw_refl = get_arr(
                get_field(inputs, "reflection_counts", operation)?,
                "reflection_counts",
            )?;
            let mut refl = Vec::with_capacity(raw_refl.len());
            for (idx, v) in raw_refl.iter().enumerate() {
                refl.push(require_exact_int_json(
                    v,
                    &format!("reflection_counts[{idx}]"),
                )?);
            }
            let window = require_exact_int_json(get_field(inputs, "window", operation)?, "window")?;
            let out = closure_wave_rd_rcd(&p, &q, &hinges, &refl, &window)?;
            let mut m = BTreeMap::new();
            m.insert("Pi".to_string(), JsonValue::Int(out.pi_val));
            m.insert("RD".to_string(), JsonValue::Int(out.rd_val));
            m.insert("rcd_duration".to_string(), JsonValue::Int(out.rcd_duration));
            m.insert("participation".to_string(), JsonValue::Int(out.participation));
            Ok(JsonValue::Object(m))
        }

        "relational_cadence_and_lag" => {
            let na = require_exact_int_json(get_field(inputs, "na", operation)?, "na")?;
            let nb = require_exact_int_json(get_field(inputs, "nb", operation)?, "nb")?;
            let bias_arr = get_arr(get_field(inputs, "bias", operation)?, "bias")?;
            if bias_arr.len() != 2 {
                return Err(CoreError::DomainError("bias must have 2 elements".to_string()));
            }
            let b0 = parse_canonical_rational_pair_json(&bias_arr[0], "bias[0]")?;
            let b1 = parse_canonical_rational_pair_json(&bias_arr[1], "bias[1]")?;
            let prior_lag = parse_canonical_rational_pair_json(
                get_field(inputs, "prior_lag", operation)?,
                "prior_lag",
            )?;
            let out = relational_cadence_and_lag(&na, &nb, &b0, &b1, &prior_lag)?;
            let mut fwd = BTreeMap::new();
            fwd.insert("ratio".to_string(), pair_json(&out.forward.ratio));
            fwd.insert("bias_diff".to_string(), pair_json(&out.forward.bias_diff));
            fwd.insert("lag".to_string(), pair_json(&out.forward.lag));
            fwd.insert("drift".to_string(), pair_json(&out.forward.drift));
            let mut rev = BTreeMap::new();
            rev.insert("ratio".to_string(), pair_json(&out.reverse.ratio));
            rev.insert("bias_diff".to_string(), pair_json(&out.reverse.bias_diff));
            rev.insert("lag".to_string(), pair_json(&out.reverse.lag));
            rev.insert("drift".to_string(), pair_json(&out.reverse.drift));
            let mut m = BTreeMap::new();
            m.insert("forward".to_string(), JsonValue::Object(fwd));
            m.insert("reverse".to_string(), JsonValue::Object(rev));
            Ok(JsonValue::Object(m))
        }

        "finite_stokes_cut_balance" => {
            let raw_reg = get_arr(get_field(inputs, "region", operation)?, "region")?;
            let mut region = Vec::with_capacity(raw_reg.len());
            for (idx, v) in raw_reg.iter().enumerate() {
                region.push(require_exact_int_json(v, &format!("region[{idx}]"))?);
            }
            let raw_ch = get_arr(get_field(inputs, "channels", operation)?, "channels")?;
            let mut channels = Vec::with_capacity(raw_ch.len());
            for (idx, cv) in raw_ch.iter().enumerate() {
                let co = get_obj(cv, &format!("channels[{idx}]"))?;
                channels.push(OrientedChannel {
                    i: require_exact_int_json(get_field(co, "i", "channel")?, "channel.i")?,
                    j: require_exact_int_json(get_field(co, "j", "channel")?, "channel.j")?,
                    p: parse_canonical_rational_pair_json(
                        get_field(co, "p", "channel")?,
                        "channel.p",
                    )?,
                    a: parse_canonical_rational_pair_json(
                        get_field(co, "a", "channel")?,
                        "channel.a",
                    )?,
                    lineage: get_non_empty_str(co, "lineage", "channel")?.to_string(),
                });
            }
            let out = finite_stokes_cut_balance(&channels, &region)?;
            let mut m = BTreeMap::new();
            m.insert("internal".to_string(), pair_json(&out.internal));
            m.insert("boundary".to_string(), pair_json(&out.boundary));
            m.insert("divergence".to_string(), pair_json(&out.divergence));
            m.insert(
                "divergence_equals_boundary".to_string(),
                JsonValue::Bool(out.divergence_equals_boundary),
            );
            Ok(JsonValue::Object(m))
        }

        "awareness_current_update" => {
            let cur = parse_canonical_rational_pair_json(
                get_field(inputs, "current", operation)?,
                "current",
            )?;
            let pres = parse_canonical_rational_pair_json(
                get_field(inputs, "pressure", operation)?,
                "pressure",
            )?;
            let att = parse_canonical_rational_pair_json(
                get_field(inputs, "attention", operation)?,
                "attention",
            )?;
            let qry =
                parse_canonical_rational_pair_json(get_field(inputs, "query", operation)?, "query")?;
            let gain =
                parse_canonical_rational_pair_json(get_field(inputs, "gain", operation)?, "gain")?;
            let raw_ret = get_arr(
                get_field(inputs, "available_retained_values", operation)?,
                "available_retained_values",
            )?;
            let mut ret_vals = Vec::with_capacity(raw_ret.len());
            for (idx, v) in raw_ret.iter().enumerate() {
                ret_vals.push(parse_canonical_rational_pair_json(
                    v,
                    &format!("available_retained_values[{idx}]"),
                )?);
            }
            let thresh = parse_canonical_rational_pair_json(
                get_field(inputs, "selector_threshold", operation)?,
                "selector_threshold",
            )?;
            let out = awareness_current_update(&cur, &pres, &att, &qry, &gain, &ret_vals, &thresh)?;
            let mut m = BTreeMap::new();
            m.insert("carried".to_string(), pair_json(&out.carried));
            m.insert("native_current".to_string(), pair_json(&out.native_current));
            m.insert("next_current".to_string(), pair_json(&out.next_current));
            m.insert(
                "selected_slot".to_string(),
                JsonValue::from_i64(out.selected_slot),
            );
            m.insert(
                "removal_current".to_string(),
                pair_json(&out.removal_current),
            );
            m.insert(
                "awareness_effect".to_string(),
                JsonValue::Bool(out.awareness_effect),
            );
            Ok(JsonValue::Object(m))
        }

        "awareness_mindfulness_step" => {
            let pres = parse_canonical_rational_pair_json(
                get_field(inputs, "pressure", operation)?,
                "pressure",
            )?;
            let j0 = parse_canonical_rational_pair_json(
                get_field(inputs, "initial_current", operation)?,
                "initial_current",
            )?;
            let forcing = parse_canonical_rational_pair_json(
                get_field(inputs, "constant_forcing", operation)?,
                "constant_forcing",
            )?;
            let delay_init = parse_canonical_rational_pair_json(
                get_field(inputs, "delay_initial_history", operation)?,
                "delay_initial_history",
            )?;
            let steps = require_usize_json(get_field(inputs, "steps", operation)?, "steps")?;
            let out = awareness_mindfulness_step(&pres, &j0, &forcing, &delay_init, steps)?;
            let mut m = BTreeMap::new();
            m.insert(
                "no_delay_currents".to_string(),
                JsonValue::Array(out.no_delay_currents.iter().map(pair_json).collect()),
            );
            m.insert(
                "no_delay_stable_at_one".to_string(),
                JsonValue::Bool(out.no_delay_stable_at_one),
            );
            m.insert(
                "extra_delay_currents".to_string(),
                JsonValue::Array(out.extra_delay_currents.iter().map(pair_json).collect()),
            );
            m.insert(
                "extra_delay_stable_at_one".to_string(),
                JsonValue::Bool(out.extra_delay_stable_at_one),
            );
            Ok(JsonValue::Object(m))
        }

        "higgs_trace_score_and_freeze" => {
            let raw_cands = get_arr(get_field(inputs, "candidates", operation)?, "candidates")?;
            let mut cands = Vec::with_capacity(raw_cands.len());
            for (idx, cv) in raw_cands.iter().enumerate() {
                let co = get_obj(cv, &format!("candidates[{idx}]"))?;
                cands.push(HiggsCandidateInput {
                    p: require_exact_int_json(get_field(co, "p", "candidate")?, "p")?,
                    q: require_exact_int_json(get_field(co, "q", "candidate")?, "q")?,
                    win_l: require_exact_int_json(get_field(co, "L", "candidate")?, "L")?,
                    c3: require_exact_int_json(get_field(co, "C3", "candidate")?, "C3")?,
                    ca: require_exact_int_json(get_field(co, "Ca", "candidate")?, "Ca")?,
                    cb: require_exact_int_json(get_field(co, "Cb", "candidate")?, "Cb")?,
                });
            }
            let out = higgs_trace_score_and_freeze(&cands)?;
            let mut m = BTreeMap::new();
            m.insert(
                "scores".to_string(),
                JsonValue::Array(out.scores.iter().map(pair_json).collect()),
            );
            m.insert(
                "winner_index".to_string(),
                JsonValue::from_usize(out.winner_index),
            );
            m.insert("winner_Pi".to_string(), JsonValue::Int(out.winner_pi));
            m.insert("winner_RD".to_string(), JsonValue::Int(out.winner_rd));
            m.insert("winner_PD".to_string(), JsonValue::Int(out.winner_pd));
            m.insert("winner_QD".to_string(), JsonValue::Int(out.winner_qd));
            m.insert("E_H_0_MeV".to_string(), JsonValue::Int(out.e_h_0_mev));
            m.insert("E_H_0_GeV".to_string(), pair_json(&out.e_h_0_gev));
            Ok(JsonValue::Object(m))
        }

        "brownian_3d_delta3_msb_x" => {
            let raw_disps =
                get_arr(get_field(inputs, "displacements", operation)?, "displacements")?;
            let mut disps = Vec::with_capacity(raw_disps.len());
            for (idx, rv) in raw_disps.iter().enumerate() {
                let arr = get_arr(rv, &format!("displacements[{idx}]"))?;
                if arr.len() != 3 {
                    return Err(CoreError::DomainError(
                        "displacement must have 3 coordinates".to_string(),
                    ));
                }
                disps.push([
                    require_exact_int_json(&arr[0], "disp[0]")?,
                    require_exact_int_json(&arr[1], "disp[1]")?,
                    require_exact_int_json(&arr[2], "disp[2]")?,
                ]);
            }
            let beta_a = require_usize_json(get_field(inputs, "beta_a", operation)?, "beta_a")?;
            let out = brownian_3d_delta3_msb_x(&disps, beta_a)?;
            let mut m = BTreeMap::new();
            m.insert(
                "Q".to_string(),
                JsonValue::Array(out.q_vals.into_iter().map(JsonValue::Int).collect()),
            );
            m.insert(
                "octants_msb_x".to_string(),
                JsonValue::Array(
                    out.octants_msb_x
                        .into_iter()
                        .map(JsonValue::from_i64)
                        .collect(),
                ),
            );
            m.insert(
                "phase_DeltaZ".to_string(),
                JsonValue::Array(out.phase_delta_z.into_iter().map(JsonValue::Int).collect()),
            );
            m.insert(
                "octant_DeltaZ".to_string(),
                JsonValue::Array(out.octant_delta_z.into_iter().map(JsonValue::Int).collect()),
            );
            m.insert("sum_phase_m".to_string(), JsonValue::Int(out.sum_phase_m));
            m.insert("sum_octant_m".to_string(), JsonValue::Int(out.sum_octant_m));
            m.insert("R_Q".to_string(), pair_json(&out.r_q));
            m.insert("R_x".to_string(), pair_json(&out.r_x));
            m.insert("R_y".to_string(), pair_json(&out.r_y));
            m.insert("R_z".to_string(), pair_json(&out.r_z));
            Ok(JsonValue::Object(m))
        }

        "run_tumble_3d_delta3_lsb_x" => {
            let raw_vecs = get_arr(get_field(inputs, "z_vectors", operation)?, "z_vectors")?;
            let mut vecs = Vec::with_capacity(raw_vecs.len());
            for (idx, rv) in raw_vecs.iter().enumerate() {
                let arr = get_arr(rv, &format!("z_vectors[{idx}]"))?;
                if arr.len() != 3 {
                    return Err(CoreError::DomainError(
                        "z_vector must have 3 coordinates".to_string(),
                    ));
                }
                vecs.push([
                    require_exact_int_json(&arr[0], "z_vec[0]")?,
                    require_exact_int_json(&arr[1], "z_vec[1]")?,
                    require_exact_int_json(&arr[2], "z_vec[2]")?,
                ]);
            }
            let beta_a = require_exact_int_json(get_field(inputs, "beta_a", operation)?, "beta_a")?;
            let out = run_tumble_3d_delta3_lsb_x(&vecs, &beta_a)?;
            let mut m = BTreeMap::new();
            m.insert(
                "octants_lsb_x".to_string(),
                JsonValue::Array(
                    out.octants_lsb_x
                        .into_iter()
                        .map(JsonValue::from_i64)
                        .collect(),
                ),
            );
            m.insert(
                "octants_msb_x_contrast".to_string(),
                JsonValue::Array(
                    out.octants_msb_x_contrast
                        .into_iter()
                        .map(JsonValue::from_i64)
                        .collect(),
                ),
            );
            m.insert(
                "Q3".to_string(),
                JsonValue::Array(out.q3.into_iter().map(JsonValue::Int).collect()),
            );
            m.insert(
                "theta".to_string(),
                JsonValue::Array(out.theta.into_iter().map(JsonValue::Int).collect()),
            );
            Ok(JsonValue::Object(m))
        }

        "cycle_shedding_step" => {
            let prev = parse_canonical_rational_pair_json(
                get_field(inputs, "prev_burden", operation)?,
                "prev_burden",
            )?;
            let lead =
                parse_canonical_rational_pair_json(get_field(inputs, "lead", operation)?, "lead")?;
            let lag =
                parse_canonical_rational_pair_json(get_field(inputs, "lag", operation)?, "lag")?;
            let comp = parse_canonical_rational_pair_json(
                get_field(inputs, "compatibility", operation)?,
                "compatibility",
            )?;
            let eta =
                parse_canonical_rational_pair_json(get_field(inputs, "eta", operation)?, "eta")?;
            let gamma =
                parse_canonical_rational_pair_json(get_field(inputs, "gamma", operation)?, "gamma")?;
            let lam = parse_canonical_rational_pair_json(
                get_field(inputs, "lambda_reclose", operation)?,
                "lambda_reclose",
            )?;
            let out = cycle_shedding_step(&prev, &lead, &lag, &comp, &eta, &gamma, &lam);
            Ok(cycle_shedding_step_to_json(&out))
        }

        "cycle_shedding_trace" => {
            let init = parse_pair_or_printed_decimal_json(
                get_field(inputs, "initial_burden", operation)?,
                "initial_burden",
            )?;
            let eta = parse_pair_or_printed_decimal_json(
                get_field(inputs, "eta", operation)?,
                "eta",
            )?;
            let gamma = parse_pair_or_printed_decimal_json(
                get_field(inputs, "gamma", operation)?,
                "gamma",
            )?;
            let lam = parse_pair_or_printed_decimal_json(
                get_field(inputs, "lambda_reclose", operation)?,
                "lambda_reclose",
            )?;
            let raw_ticks = get_arr(get_field(inputs, "ticks", operation)?, "ticks")?;
            if raw_ticks.len() > MAX_CYCLE_SHEDDING_TICKS {
                return Err(CoreError::DomainError(format!(
                    "ticks length {} exceeds guard {}",
                    raw_ticks.len(),
                    MAX_CYCLE_SHEDDING_TICKS
                )));
            }
            let mut ticks = Vec::with_capacity(raw_ticks.len());
            for (idx, tv) in raw_ticks.iter().enumerate() {
                let obj = get_obj(tv, &format!("ticks[{idx}]"))?;
                let lead = parse_pair_or_printed_decimal_json(
                    get_field(obj, "lead", "tick")?,
                    "lead",
                )?;
                let lag = parse_pair_or_printed_decimal_json(
                    get_field(obj, "lag", "tick")?,
                    "lag",
                )?;
                let compatibility = parse_pair_or_printed_decimal_json(
                    get_field(obj, "compatibility", "tick")?,
                    "compatibility",
                )?;
                ticks.push(CycleSheddingTickInput {
                    lead,
                    lag,
                    compatibility,
                });
            }
            let trace = cycle_shedding_trace(&init, &ticks, &eta, &gamma, &lam)?;
            let mut max_span = Ratio::zero();
            let mut max_burden = Ratio::zero();
            let mut max_excess = Ratio::zero();
            let mut total_outward = Ratio::zero();
            let mut total_local = Ratio::zero();
            for step in &trace {
                if step.span > max_span {
                    max_span = step.span.clone();
                }
                if step.padar_burden > max_burden {
                    max_burden = step.padar_burden.clone();
                }
                if step.shed_excess > max_excess {
                    max_excess = step.shed_excess.clone();
                }
                total_outward = &total_outward + &step.outward_shedding;
                total_local = &total_local + &step.local_reclosure;
            }
            let final_burden = trace
                .last()
                .map(|s| s.padar_burden.clone())
                .unwrap_or(init);
            let mut m = BTreeMap::new();
            m.insert(
                "ticks".to_string(),
                JsonValue::Array(trace.iter().map(cycle_shedding_step_to_json).collect()),
            );
            m.insert("final_burden".to_string(), pair_json(&final_burden));
            m.insert("max_span".to_string(), pair_json(&max_span));
            m.insert("max_burden".to_string(), pair_json(&max_burden));
            m.insert("max_excess".to_string(), pair_json(&max_excess));
            m.insert("total_outward".to_string(), pair_json(&total_outward));
            m.insert("total_local".to_string(), pair_json(&total_local));
            Ok(JsonValue::Object(m))
        }

        "sparc_exact_bin_eval" => {
            let r_tok = get_non_empty_str(inputs, "R", operation)?;
            let v_obs_tok = get_non_empty_str(inputs, "Vobs", operation)?;
            let err_v_tok = get_non_empty_str(inputs, "errV", operation)?;
            let v_gas_tok = get_non_empty_str(inputs, "Vgas", operation)?;
            let v_disk_tok = get_non_empty_str(inputs, "Vdisk", operation)?;
            let v_bul_tok = get_non_empty_str(inputs, "Vbul", operation)?;
            let out = sparc_exact_bin_eval(
                r_tok,
                v_obs_tok,
                err_v_tok,
                v_gas_tok,
                v_disk_tok,
                v_bul_tok,
            )?;
            let mut m = BTreeMap::new();
            m.insert("U_obs".to_string(), pair_json(&out.u_obs));
            m.insert("sigma_U".to_string(), pair_json(&out.sigma_u));
            m.insert("U_gas".to_string(), pair_json(&out.u_gas));
            m.insert("U_disk".to_string(), pair_json(&out.u_disk));
            m.insert("U_bulge".to_string(), pair_json(&out.u_bulge));
            m.insert("U_bar".to_string(), pair_json(&out.u_bar));
            m.insert("C_cap".to_string(), pair_json(&out.c_cap));
            m.insert("D_BF".to_string(), pair_json(&out.d_bf));
            m.insert("U_AO".to_string(), pair_json(&out.u_ao));
            m.insert("Delta_U".to_string(), pair_json(&out.delta_u));
            m.insert("z_U".to_string(), pair_json(&out.z_u));
            Ok(JsonValue::Object(m))
        }

        "temporal_metrology_phase_si" => {
            let compared = parse_phase_interval(get_field(inputs, "compared", operation)?, "compared")?;
            let reference =
                parse_phase_interval(get_field(inputs, "reference", operation)?, "reference")?;
            let split_left =
                parse_phase_interval(get_field(inputs, "split_left", operation)?, "split_left")?;
            let split_right =
                parse_phase_interval(get_field(inputs, "split_right", operation)?, "split_right")?;
            let cs_hz = require_exact_int_json(get_field(inputs, "cs133_hz", operation)?, "cs133_hz")?;
            let out = temporal_metrology_phase_si(
                &compared,
                &reference,
                &split_left,
                &split_right,
                &cs_hz,
            )?;
            let mut m = BTreeMap::new();
            m.insert(
                "compared_advance".to_string(),
                pair_json(&out.compared_advance),
            );
            m.insert(
                "reference_advance".to_string(),
                pair_json(&out.reference_advance),
            );
            m.insert("ratio".to_string(), pair_json(&out.ratio));
            m.insert(
                "duration_seconds".to_string(),
                pair_json(&out.duration_seconds),
            );
            m.insert(
                "mean_frequency_hz".to_string(),
                pair_json(&out.mean_frequency_hz),
            );
            m.insert(
                "split_left_advance".to_string(),
                pair_json(&out.split_left_advance),
            );
            m.insert(
                "split_right_advance".to_string(),
                pair_json(&out.split_right_advance),
            );
            m.insert(
                "split_additivity_exact".to_string(),
                JsonValue::Bool(out.split_additivity_exact),
            );
            Ok(JsonValue::Object(m))
        }

        "temporal_metrology_signed_advance" => {
            let compared = parse_phase_interval(get_field(inputs, "compared", operation)?, "compared")?;
            let reference =
                parse_phase_interval(get_field(inputs, "reference", operation)?, "reference")?;
            let cs_hz = require_exact_int_json(get_field(inputs, "cs133_hz", operation)?, "cs133_hz")?;
            let out = temporal_metrology_signed_advance(&compared, &reference, &cs_hz)?;
            let mut m = BTreeMap::new();
            m.insert(
                "compared_advance".to_string(),
                pair_json(&out.compared_advance),
            );
            m.insert(
                "reference_advance".to_string(),
                pair_json(&out.reference_advance),
            );
            m.insert(
                "duration_seconds".to_string(),
                pair_json(&out.duration_seconds),
            );
            m.insert(
                "signed_mean_frequency_hz".to_string(),
                pair_json(&out.signed_mean_frequency_hz),
            );
            Ok(JsonValue::Object(m))
        }

        "temporal_metrology_hellwig_mean" => {
            let t4 = parse_canonical_rational_pair_json(
                get_field(inputs, "table_iv_hz", operation)?,
                "table_iv_hz",
            )?;
            let t6 = parse_canonical_rational_pair_json(
                get_field(inputs, "table_vi_hz", operation)?,
                "table_vi_hz",
            )?;
            let out = temporal_metrology_hellwig_mean(&t4, &t6)?;
            let mut m = BTreeMap::new();
            m.insert(
                "published_mean_hz".to_string(),
                pair_json(&out.published_mean_hz),
            );
            m.insert("difference_hz".to_string(), pair_json(&out.difference_hz));
            Ok(JsonValue::Object(m))
        }

        "lensing_swells_delta3_and_tau_discrepancy" => {
            let raw_pred = get_arr(
                get_field(inputs, "swells_predicted", operation)?,
                "swells_predicted",
            )?;
            let raw_obs = get_arr(
                get_field(inputs, "swells_observed", operation)?,
                "swells_observed",
            )?;
            let mut pred = Vec::with_capacity(raw_pred.len());
            for (idx, v) in raw_pred.iter().enumerate() {
                pred.push(require_exact_int_json(
                    v,
                    &format!("swells_predicted[{idx}]"),
                )?);
            }
            let mut obs = Vec::with_capacity(raw_obs.len());
            for (idx, v) in raw_obs.iter().enumerate() {
                obs.push(require_exact_int_json(
                    v,
                    &format!("swells_observed[{idx}]"),
                )?);
            }
            let tau_d = parse_canonical_rational_pair_json(
                get_field(inputs, "tau_diagnostic_1210", operation)?,
                "tau_diagnostic_1210",
            )?;
            let tau_s = parse_canonical_rational_pair_json(
                get_field(inputs, "tau_summary_nominal_1210", operation)?,
                "tau_summary_nominal_1210",
            )?;
            let out = lensing_swells_delta3_and_tau_discrepancy(&pred, &obs, &tau_d, &tau_s)?;
            let mut m = BTreeMap::new();
            m.insert(
                "deltaZ".to_string(),
                JsonValue::Array(out.delta_z.into_iter().map(JsonValue::Int).collect()),
            );
            m.insert(
                "s3".to_string(),
                JsonValue::Array(out.s3.into_iter().map(JsonValue::from_i64).collect()),
            );
            m.insert(
                "m".to_string(),
                JsonValue::Array(out.m.into_iter().map(JsonValue::Int).collect()),
            );
            m.insert("sum_deltaZ".to_string(), JsonValue::Int(out.sum_delta_z));
            m.insert("sum_m".to_string(), JsonValue::Int(out.sum_m));
            m.insert("tau_difference".to_string(), pair_json(&out.tau_difference));
            m.insert("tau_distinct".to_string(), JsonValue::Bool(out.tau_distinct));
            Ok(JsonValue::Object(m))
        }

        "map21_nonlinear_finite_law_variance" => {
            let raw_sup = get_arr(get_field(inputs, "support", operation)?, "support")?;
            let mut support = Vec::with_capacity(raw_sup.len());
            for (idx, pv) in raw_sup.iter().enumerate() {
                let po = get_obj(pv, &format!("support[{idx}]"))?;
                support.push(Map21SupportPoint {
                    p: parse_canonical_rational_pair_json(get_field(po, "p", "point")?, "point.p")?,
                    y: parse_canonical_rational_pair_json(get_field(po, "y", "point")?, "point.y")?,
                    d: parse_canonical_rational_pair_json(get_field(po, "d", "point")?, "point.d")?,
                });
            }
            let out = map21_nonlinear_finite_law_variance(&support)?;
            let mut m = BTreeMap::new();
            m.insert(
                "outputs".to_string(),
                JsonValue::Array(out.outputs.iter().map(pair_json).collect()),
            );
            m.insert("output_mean".to_string(), pair_json(&out.output_mean));
            m.insert("mean_shift".to_string(), pair_json(&out.mean_shift));
            m.insert(
                "output_variance".to_string(),
                pair_json(&out.output_variance),
            );
            m.insert(
                "linearized_variance_diagnostic".to_string(),
                pair_json(&out.linearized_variance_diagnostic),
            );
            Ok(JsonValue::Object(m))
        }

        "sympy_z4_dft" => {
            let raw_vals = get_arr(get_field(inputs, "values", operation)?, "values")?;
            let mut tokens = Vec::with_capacity(raw_vals.len());
            for (idx, v) in raw_vals.iter().enumerate() {
                let s = v.as_str().ok_or_else(|| {
                    CoreError::DomainError(format!("values[{idx}] must be a printed rational string"))
                })?;
                tokens.push(s.to_string());
            }
            let out = z4_dft_gaussian(&tokens)?;
            let mut m = BTreeMap::new();
            m.insert(
                "coefficients".to_string(),
                JsonValue::Array(out.coefficients.into_iter().map(JsonValue::Str).collect()),
            );
            m.insert(
                "reconstruction".to_string(),
                JsonValue::Array(out.reconstruction.into_iter().map(JsonValue::Str).collect()),
            );
            m.insert("parseval_time".to_string(), pair_json(&out.parseval_time));
            m.insert("parseval_freq".to_string(), pair_json(&out.parseval_freq));
            Ok(JsonValue::Object(m))
        }

        "cyclotomic_z12_overtone" => {
            let group_order =
                require_exact_int_json(get_field(inputs, "group_order", operation)?, "group_order")?;
            let raw_pulses = get_arr(get_field(inputs, "pulses", operation)?, "pulses")?;
            let mut pulses = Vec::with_capacity(raw_pulses.len());
            for (idx, v) in raw_pulses.iter().enumerate() {
                pulses.push(require_exact_int_json(v, &format!("pulses[{idx}]"))?);
            }
            let l_min = require_exact_int_json(get_field(inputs, "l_min", operation)?, "l_min")?;
            let overtone_mode = require_exact_int_json(
                get_field(inputs, "overtone_mode", operation)?,
                "overtone_mode",
            )?;
            let out = cyclotomic_z12_overtone(&group_order, &pulses, &l_min, &overtone_mode)?;
            let mut m = BTreeMap::new();
            m.insert("v_hat_3".to_string(), pair_json(&out.v_hat_3));
            m.insert(
                "nonzero_modes".to_string(),
                JsonValue::Array(
                    out.nonzero_modes
                        .into_iter()
                        .map(JsonValue::from_i64)
                        .collect(),
                ),
            );
            m.insert("parseval_exact".to_string(), pair_json(&out.parseval_exact));
            m.insert(
                "overtone_exceeds_nyquist_bound".to_string(),
                JsonValue::Bool(out.overtone_exceeds_nyquist_bound),
            );
            Ok(JsonValue::Object(m))
        }

        "higgs_lhc_rounded_display" => {
            let mass_tok = get_non_empty_str(inputs, "mass_GeV", operation)?;
            let sigma_tok = get_non_empty_str(inputs, "sigma_GeV", operation)?;
            let pred_tok = get_non_empty_str(inputs, "prediction_GeV", operation)?;
            let decimals = require_usize_json(get_field(inputs, "decimals", operation)?, "decimals")?;
            if decimals > MAX_ROUNDING_DECIMALS {
                return Err(CoreError::DomainError(format!(
                    "decimals {decimals} exceeds {MAX_ROUNDING_DECIMALS}"
                )));
            }
            let out = higgs_lhc_rounded_display(mass_tok, sigma_tok, pred_tok, decimals)?;
            let mut m = BTreeMap::new();
            m.insert("delta_GeV".to_string(), pair_json(&out.delta_gev));
            m.insert("z_score_exact".to_string(), pair_json(&out.z_score_exact));
            m.insert(
                "z_score_rounded_6dp".to_string(),
                JsonValue::Str(out.z_score_rounded_6dp),
            );
            m.insert(
                "z_score_rounding_residual".to_string(),
                pair_json(&out.z_score_rounding_residual),
            );
            m.insert(
                "abs_percent_error_rounded_6dp".to_string(),
                JsonValue::Str(out.abs_percent_error_rounded_6dp),
            );
            m.insert(
                "abs_percent_error_rounding_residual".to_string(),
                pair_json(&out.abs_percent_error_rounding_residual),
            );
            Ok(JsonValue::Object(m))
        }

        "printed_token_half_ulp_enclosure" => {
            let tok = get_non_empty_str(inputs, "printed_token", operation)?;
            let out = printed_token_half_ulp_enclosure(tok)?;
            let mut m = BTreeMap::new();
            m.insert(
                "decimal_places".to_string(),
                JsonValue::from_usize(out.decimal_places),
            );
            m.insert("value".to_string(), pair_json(&out.value));
            m.insert("half_ulp_bound".to_string(), pair_json(&out.half_ulp_bound));
            m.insert("enclosure_lower".to_string(), pair_json(&out.enclosure_lower));
            m.insert("enclosure_upper".to_string(), pair_json(&out.enclosure_upper));
            Ok(JsonValue::Object(m))
        }

        other => Err(CoreError::ValidationError(format!(
            "unsupported vector operation: {other:?}"
        ))),
    }
}

/// Public single-operation exact evaluator (`aodc eval-op`): dispatches all 24 positive S2/S4
/// operations plus multi-tick `cycle_shedding_trace`.
pub fn eval_operation(
    operation: &str,
    inputs: &BTreeMap<String, JsonValue>,
) -> CoreResult<JsonValue> {
    eval_positive_operation(operation, inputs)
}

/// Verify one typed-boundary REJECT negative-control vector (`RC-021`..`RC-023`).
pub fn verify_reject_operation(
    operation: &str,
    inputs: &BTreeMap<String, JsonValue>,
    expected_error_class: &str,
    expected_status: &str,
) -> CoreResult<()> {
    match operation {
        "typed_reject_float_operand" => {
            if expected_error_class != "TypeError" || expected_status != "REJECTED_FLOAT_OPERAND" {
                return Err(CoreError::ValidationError(
                    "typed_reject_float_operand expects TypeError / REJECTED_FLOAT_OPERAND"
                        .to_string(),
                ));
            }
            let json_text = get_non_empty_str(inputs, "json_float_text", operation)?;
            match parse_strict_json(json_text) {
                Err(CoreError::FloatForbidden(_)) => {}
                other => {
                    return Err(CoreError::ValidationError(format!(
                        "parse_strict_json failed to reject JSON floating-point literal with FloatForbidden: {other:?}"
                    )));
                }
            }
            let dec_tok = get_non_empty_str(inputs, "decimal_token_for_runtime_float", operation)?;
            match parse_strict_json(dec_tok) {
                Err(CoreError::FloatForbidden(_)) => {}
                other => {
                    return Err(CoreError::ValidationError(format!(
                        "parse_strict_json failed to reject decimal token as non-integer JSON number: {other:?}"
                    )));
                }
            }
            if parse_exact_int_str(dec_tok).is_ok() {
                return Err(CoreError::ValidationError(format!(
                    "parse_exact_int_str unexpectedly accepted non-integer token {dec_tok:?}"
                )));
            }
            Ok(())
        }

        "typed_reject_bool_as_int" => {
            if expected_error_class != "TypeError" || expected_status != "REJECTED_BOOL_AS_INT" {
                return Err(CoreError::ValidationError(
                    "typed_reject_bool_as_int expects TypeError / REJECTED_BOOL_AS_INT".to_string(),
                ));
            }
            let operand = get_field(inputs, "bool_operand", operation)?;
            if operand.as_bool().is_none() {
                return Err(CoreError::ValidationError(
                    "typed_reject_bool_as_int requires JSON bool operand".to_string(),
                ));
            }
            if require_exact_int_json(operand, "bool_operand").is_ok() {
                return Err(CoreError::ValidationError(
                    "require_exact_int_json failed to reject bool operand".to_string(),
                ));
            }
            let bad_pair = JsonValue::Array(vec![operand.clone(), JsonValue::Str("1".to_string())]);
            if parse_canonical_rational_pair_json(&bad_pair, "bool_pair").is_ok() {
                return Err(CoreError::ValidationError(
                    "parse_canonical_rational_pair_json failed to reject bool element".to_string(),
                ));
            }
            Ok(())
        }

        "typed_reject_nonfinite_and_malformed_tokens" => {
            if expected_error_class != "ValueError" || expected_status != "REJECTED_MALFORMED_TOKEN"
            {
                return Err(CoreError::ValidationError(
                    "typed_reject_nonfinite_and_malformed_tokens expects ValueError / REJECTED_MALFORMED_TOKEN"
                        .to_string(),
                ));
            }
            let tokens = get_arr(get_field(inputs, "bad_tokens", operation)?, "bad_tokens")?;
            if tokens.is_empty() {
                return Err(CoreError::ValidationError(
                    "typed_reject_nonfinite_and_malformed_tokens requires non-empty bad_tokens"
                        .to_string(),
                ));
            }
            for (idx, tv) in tokens.iter().enumerate() {
                let tok = tv.as_str().ok_or_else(|| {
                    CoreError::ValidationError(format!("bad_tokens[{idx}] must be a string"))
                })?;
                if parse_printed_rational(tok, true, false).is_ok() {
                    return Err(CoreError::ValidationError(format!(
                        "parse_printed_rational(exp=false) failed to reject malformed token {tok:?}"
                    )));
                }
                if parse_printed_rational(tok, true, true).is_ok() {
                    return Err(CoreError::ValidationError(format!(
                        "parse_printed_rational(exp=true) failed to reject malformed token {tok:?}"
                    )));
                }
            }
            Ok(())
        }

        other => Err(CoreError::ValidationError(format!(
            "unsupported REJECT vector operation: {other:?}"
        ))),
    }
}

/// Validate and execute one parsed `aod.s4.vector.v1` record against the exact Rust kernel.
pub fn verify_single_vector(record: &JsonValue) -> CoreResult<String> {
    validate_vector_record(record)?;
    let obj = get_obj(record, "vector")?;
    let case_id = get_non_empty_str(obj, "case_id", "vector")?;
    let operation = get_non_empty_str(obj, "operation", "vector")?;
    let sci_status = get_non_empty_str(obj, "scientific_status", "vector")?;
    let inputs = get_obj(get_field(obj, "inputs", "vector")?, "inputs")?;
    let expected = get_obj(
        get_field(obj, "expected_outcome", "vector")?,
        "expected_outcome",
    )?;
    let exp_status = get_non_empty_str(expected, "status", "expected_outcome")?;

    if exp_status == "REJECT" {
        let err_cls = get_non_empty_str(expected, "expected_error_class", "expected_outcome")?;
        let exp_st = get_non_empty_str(expected, "expected_status", "expected_outcome")?;
        verify_reject_operation(operation, inputs, err_cls, exp_st)?;
        return Ok(sci_status.to_string());
    }

    let actual = eval_positive_operation(operation, inputs)?;
    let expected_val = get_field(expected, "exact_value", "expected_outcome")?;
    if &actual != expected_val {
        return Err(CoreError::ValidationError(format!(
            "vector {case_id} ({operation}) mismatch: actual={}, expected={}",
            actual.to_compact_ascii_json(),
            expected_val.to_compact_ascii_json()
        )));
    }
    Ok(sci_status.to_string())
}

/// Load a strict zero-float JSONL golden-vector file (rejecting symlinks and floats).
pub fn load_vector_file(path: &Path, label: &str) -> CoreResult<(String, Vec<JsonValue>)> {
    let raw = read_regular_no_symlink(path, label)?;
    let digest = Sha256::digest_hex(&raw);
    let text = String::from_utf8(raw)
        .map_err(|_| CoreError::ValidationError(format!("invalid UTF-8 in {label}")))?;
    let mut records = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let parsed = parse_strict_json(line).map_err(|e| {
            CoreError::ValidationError(format!("{label}:L{}: {e}", idx + 1))
        })?;
        validate_vector_record(&parsed).map_err(|e| {
            CoreError::ValidationError(format!("{label}:L{}: {e}", idx + 1))
        })?;
        records.push(parsed);
    }
    Ok((digest, records))
}

struct ExpectedS2Binding {
    binding_id: String,
    binding_status: String,
    upstream_commit: String,
    upstream_tree: String,
    base_commit_binding: String,
    expected_digests: BTreeMap<String, String>,
}

fn load_s2_binding(binding_path: &Path) -> CoreResult<ExpectedS2Binding> {
    let raw = read_regular_no_symlink(binding_path, "SOURCE_BINDING.json")?;
    let text = String::from_utf8(raw)
        .map_err(|_| CoreError::ValidationError("SOURCE_BINDING.json is not valid UTF-8".to_string()))?;
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

    let s2 = upstream
        .iter()
        .find(|b| {
            b.get("id").and_then(JsonValue::as_str) == Some(S2_BINDING_ID)
                || b.get("stage").and_then(JsonValue::as_str) == Some("S2")
        })
        .ok_or_else(|| {
            CoreError::ValidationError(format!(
                "SOURCE_BINDING.json missing S2 binding ({S2_BINDING_ID})"
            ))
        })?;

    let s2_obj = get_obj(s2, "S2 binding")?;
    let binding_id = get_non_empty_str(s2_obj, "id", "S2 binding")?.to_string();
    let binding_status = get_non_empty_str(s2_obj, "status", "S2 binding")?.to_string();
    let upstream_commit = get_non_empty_str(s2_obj, "commit", "S2 binding")?.to_string();
    let upstream_tree = get_non_empty_str(s2_obj, "tree", "S2 binding")?.to_string();
    let base_commit_binding =
        get_non_empty_str(s2_obj, "base_commit_binding", "S2 binding")?.to_string();

    let vfiles = get_arr(
        get_field(s2_obj, "vendored_files", "S2 binding")?,
        "S2 binding.vendored_files",
    )?;
    let mut expected_digests = BTreeMap::new();
    for (idx, vf) in vfiles.iter().enumerate() {
        let vo = get_obj(vf, &format!("S2 vendored_files[{idx}]"))?;
        let local_path = get_non_empty_str(vo, "local_path", "vendored_file")?;
        let sha256 = get_non_empty_str(vo, "sha256", "vendored_file")?;
        expected_digests.insert(local_path.to_string(), sha256.to_string());
    }
    for req in [RATIONAL_CORE_REL, SYMBOLIC_DISPLAY_REL] {
        if !expected_digests.contains_key(req) {
            return Err(CoreError::ValidationError(format!(
                "S2 binding missing pinned SHA-256 for {req}"
            )));
        }
    }

    Ok(ExpectedS2Binding {
        binding_id,
        binding_status,
        upstream_commit,
        upstream_tree,
        base_commit_binding,
        expected_digests,
    })
}

fn empty_status_summary() -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for st in VALID_SCIENTIFIC_STATUSES {
        m.insert(st.to_string(), 0);
    }
    m
}

/// Verify all 30 S2 golden vectors in `vectors_dir` against `binding_path` (`SOURCE_BINDING.json`).
///
/// Always returns a complete, portable, zero-float [`S2VerifyReport`] (even on `FAIL`), containing
/// zero host absolute paths and zero timestamps.
pub fn verify_s2_vectors_dir(vectors_dir: &Path, binding_path: &Path) -> S2VerifyReport {
    let mut report = S2VerifyReport {
        schema: S2_REPORT_SCHEMA.to_string(),
        status: "FAIL".to_string(),
        binding_id: S2_BINDING_ID.to_string(),
        binding_status: "UNVERIFIED".to_string(),
        upstream_commit: String::new(),
        upstream_tree: String::new(),
        base_commit_binding: String::new(),
        rational_core_count: 0,
        symbolic_and_display_count: 0,
        total_vectors: 0,
        passed_vectors: 0,
        failed_vectors: 0,
        file_digests: BTreeMap::new(),
        scientific_status_summary: empty_status_summary(),
        failures: Vec::new(),
    };

    if let Err(e) = check_dir_no_symlink(vectors_dir, "s2_vectors") {
        report.failures.push(S2VectorFailure {
            case_id: "VECTORS_DIR".to_string(),
            error: e.to_string(),
        });
        return report;
    }

    let binding = match load_s2_binding(binding_path) {
        Ok(b) => b,
        Err(e) => {
            report.failures.push(S2VectorFailure {
                case_id: "SOURCE_BINDING".to_string(),
                error: e.to_string(),
            });
            return report;
        }
    };

    report.binding_id = binding.binding_id;
    report.binding_status = binding.binding_status;
    report.upstream_commit = binding.upstream_commit;
    report.upstream_tree = binding.upstream_tree;
    report.base_commit_binding = binding.base_commit_binding;

    let rc_path = vectors_dir.join(RATIONAL_CORE_FILENAME);
    let sd_path = vectors_dir.join(SYMBOLIC_DISPLAY_FILENAME);

    let rc_loaded = load_vector_file(&rc_path, RATIONAL_CORE_FILENAME);
    let sd_loaded = load_vector_file(&sd_path, SYMBOLIC_DISPLAY_FILENAME);

    let mut all_records: Vec<JsonValue> = Vec::new();
    let mut digest_ok = true;

    match rc_loaded {
        Ok((sha, rows)) => {
            report
                .file_digests
                .insert(RATIONAL_CORE_REL.to_string(), sha.clone());
            if let Some(expected_sha) = binding.expected_digests.get(RATIONAL_CORE_REL) {
                if &sha != expected_sha {
                    digest_ok = false;
                    report.failures.push(S2VectorFailure {
                        case_id: "SHA256_RATIONAL_CORE".to_string(),
                        error: format!(
                            "SHA-256 mismatch for {RATIONAL_CORE_REL}: expected {expected_sha}, got {sha}"
                        ),
                    });
                }
            }
            report.rational_core_count = rows.len();
            let expected_ids: Vec<String> = (1..=24).map(|i| format!("RC-{i:03}")).collect();
            let actual_ids: Vec<String> = rows
                .iter()
                .map(|r| {
                    r.get("case_id")
                        .and_then(JsonValue::as_str)
                        .unwrap_or("")
                        .to_string()
                })
                .collect();
            if actual_ids != expected_ids {
                digest_ok = false;
                report.failures.push(S2VectorFailure {
                    case_id: "ORDER_RATIONAL_CORE".to_string(),
                    error: format!(
                        "expected exact 24 ordered IDs RC-001..RC-024, got {} record(s)",
                        rows.len()
                    ),
                });
            }
            all_records.extend(rows);
        }
        Err(e) => {
            digest_ok = false;
            report.failures.push(S2VectorFailure {
                case_id: "LOAD_RATIONAL_CORE".to_string(),
                error: e.to_string(),
            });
        }
    }

    match sd_loaded {
        Ok((sha, rows)) => {
            report
                .file_digests
                .insert(SYMBOLIC_DISPLAY_REL.to_string(), sha.clone());
            if let Some(expected_sha) = binding.expected_digests.get(SYMBOLIC_DISPLAY_REL) {
                if &sha != expected_sha {
                    digest_ok = false;
                    report.failures.push(S2VectorFailure {
                        case_id: "SHA256_SYMBOLIC_DISPLAY".to_string(),
                        error: format!(
                            "SHA-256 mismatch for {SYMBOLIC_DISPLAY_REL}: expected {expected_sha}, got {sha}"
                        ),
                    });
                }
            }
            report.symbolic_and_display_count = rows.len();
            let expected_ids: Vec<String> = (1..=6).map(|i| format!("SD-{i:03}")).collect();
            let actual_ids: Vec<String> = rows
                .iter()
                .map(|r| {
                    r.get("case_id")
                        .and_then(JsonValue::as_str)
                        .unwrap_or("")
                        .to_string()
                })
                .collect();
            if actual_ids != expected_ids {
                digest_ok = false;
                report.failures.push(S2VectorFailure {
                    case_id: "ORDER_SYMBOLIC_DISPLAY".to_string(),
                    error: format!(
                        "expected exact 6 ordered IDs SD-001..SD-006, got {} record(s)",
                        rows.len()
                    ),
                });
            }
            all_records.extend(rows);
        }
        Err(e) => {
            digest_ok = false;
            report.failures.push(S2VectorFailure {
                case_id: "LOAD_SYMBOLIC_DISPLAY".to_string(),
                error: e.to_string(),
            });
        }
    }

    report.total_vectors = all_records.len();

    for rec in &all_records {
        let cid = rec
            .get("case_id")
            .and_then(JsonValue::as_str)
            .unwrap_or("UNKNOWN")
            .to_string();
        match verify_single_vector(rec) {
            Ok(sci_status) => {
                report.passed_vectors += 1;
                if let Some(cnt) = report.scientific_status_summary.get_mut(&sci_status) {
                    *cnt += 1;
                }
            }
            Err(e) => {
                report.failed_vectors += 1;
                report.failures.push(S2VectorFailure {
                    case_id: cid,
                    error: e.to_string(),
                });
            }
        }
    }

    if digest_ok
        && report.rational_core_count == 24
        && report.symbolic_and_display_count == 6
        && report.total_vectors == 30
        && report.passed_vectors == 30
        && report.failed_vectors == 0
        && report.failures.is_empty()
    {
        report.status = "PASS".to_string();
    } else {
        report.status = "FAIL".to_string();
    }

    report
}
