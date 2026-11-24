use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use aod_core::json::{parse_strict_json, JsonValue};
use aod_core::sha256::Sha256;

use crate::hw_probe::HwProbeReport;
use crate::jit::OrcJitSession;
use crate::plan::{recommend_candidates, CodegenPlan, POLICY_SCOPE};

pub const PROFILE_VERSION: u32 = 1;
pub const MAX_PROFILE_BYTES: u64 = 1_048_576; // 1 MiB strict pre-read limit (`P1-1`)
pub const MAX_SAMPLES: usize = 32;
pub const MIN_PAIRS: usize = 5;
pub const BOOTSTRAP_RESAMPLES: usize = 2000;
pub const BOOTSTRAP_SEED: u64 = 20261113;
pub const MIN_LOWER_95_GAIN_BPS: i64 = 500; // > +5.00% paired bootstrap lower 95% bound

static PROFILE_TMP_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticSnapshot {
    pub emits: Vec<String>,
    pub failed: bool,
    pub error: Option<String>,
}

/// Zero-float exact integer paired percentile bootstrap confidence interval in basis points (`10_000 = 100%`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootstrapCi {
    pub point_gain_bps: i64,
    pub lower_95_bps: i64,
    pub upper_95_bps: i64,
    pub resamples: usize,
    pub seed: u64,
}

impl BootstrapCi {
    pub fn to_json(&self) -> JsonValue {
        let mut m = BTreeMap::new();
        m.insert(
            "lower_95_bps".to_string(),
            JsonValue::from_i64(self.lower_95_bps),
        );
        m.insert(
            "point_gain_bps".to_string(),
            JsonValue::from_i64(self.point_gain_bps),
        );
        m.insert(
            "resamples".to_string(),
            JsonValue::from_usize(self.resamples),
        );
        m.insert("seed".to_string(), JsonValue::from_i64(self.seed as i64));
        m.insert(
            "upper_95_bps".to_string(),
            JsonValue::from_i64(self.upper_95_bps),
        );
        JsonValue::Object(m)
    }

    pub fn from_json(v: &JsonValue) -> Result<Self, String> {
        let JsonValue::Object(map) = v else {
            return Err("bootstrap_ci must be a JSON object".to_string());
        };
        let get_i64 = |k: &str| -> Result<i64, String> {
            match map.get(k) {
                Some(JsonValue::Int(n)) => n
                    .to_i64()
                    .ok_or_else(|| format!("bootstrap_ci.{k} out of i64")),
                _ => Err(format!("missing or invalid integer bootstrap_ci.{k}")),
            }
        };
        let resamples_i = get_i64("resamples")?;
        let resamples = usize::try_from(resamples_i)
            .map_err(|_| format!("bootstrap_ci.resamples out of range: {resamples_i}"))?;
        let seed_i = get_i64("seed")?;
        let seed = u64::try_from(seed_i)
            .map_err(|_| format!("bootstrap_ci.seed out of range: {seed_i}"))?;
        Ok(Self {
            point_gain_bps: get_i64("point_gain_bps")?,
            lower_95_bps: get_i64("lower_95_bps")?,
            upper_95_bps: get_i64("upper_95_bps")?,
            resamples,
            seed,
        })
    }
}

fn gain_bps_i128(sum_base: u128, sum_cand: u128) -> i64 {
    if sum_base == 0 {
        return 0;
    }
    let diff = (sum_base as i128) - (sum_cand as i128);
    ((diff.saturating_mul(10_000)) / (sum_base as i128)) as i64
}

/// Deterministic zero-float paired percentile bootstrap CI in integer basis points (`i128` exact math).
pub fn paired_bootstrap_ci_bps(pairs: &[(u64, u64)], seed: u64, resamples: usize) -> BootstrapCi {
    let n = pairs.len();
    if n == 0 || resamples == 0 {
        return BootstrapCi {
            point_gain_bps: 0,
            lower_95_bps: 0,
            upper_95_bps: 0,
            resamples,
            seed,
        };
    }

    let mut total_base = 0u128;
    let mut total_cand = 0u128;
    for &(b, c) in pairs {
        total_base += b as u128;
        total_cand += c as u128;
    }
    let point_gain_bps = gain_bps_i128(total_base, total_cand);

    let mut rng = if seed == 0 { 1 } else { seed };
    let mut dist = Vec::with_capacity(resamples);
    for _ in 0..resamples {
        let mut sb = 0u128;
        let mut sc = 0u128;
        for _ in 0..n {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let idx = (rng as usize) % n;
            let (b, c) = pairs[idx];
            sb += b as u128;
            sc += c as u128;
        }
        dist.push(gain_bps_i128(sb, sc));
    }
    dist.sort_unstable();

    // 2.5th and 97.5th percentiles (for 2000 resamples -> indices 50 and 1949)
    let lo_idx = (resamples * 25) / 1000;
    let hi_idx = ((resamples * 975) / 1000).min(resamples - 1);
    BootstrapCi {
        point_gain_bps,
        lower_95_bps: dist[lo_idx],
        upper_95_bps: dist[hi_idx],
        resamples,
        seed,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LearnDecision {
    Retain,
    Rollback,
}

impl LearnDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            LearnDecision::Retain => "retain",
            LearnDecision::Rollback => "rollback",
        }
    }

    pub fn from_name(s: &str) -> Result<Self, String> {
        match s {
            "retain" => Ok(LearnDecision::Retain),
            "rollback" => Ok(LearnDecision::Rollback),
            other => Err(format!("invalid decision {other:?}")),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateEvalSummary {
    pub plan: CodegenPlan,
    pub compile_ns: u64,
    pub training_pairs: Vec<(u64, u64)>,
    pub training_ci: BootstrapCi,
    pub semantic_mismatches: usize,
}

/// Honest, cost-inclusive, rollback-safe retained compiler execution policy profile (`P0-4`, `P1-1`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetainedProfile {
    pub profile_version: u32,
    pub policy_scope: String,
    pub cpu_sig_sha256: String,
    pub kernel_ir_sha256: String,
    pub workload_sha256: String,
    pub decision: LearnDecision,
    pub reason: String,
    pub baseline_plan: CodegenPlan,
    pub active_plan: CodegenPlan,
    pub candidate_plan: CodegenPlan,
    pub generation: usize,
    pub training_pairs: Vec<(u64, u64)>,
    pub heldout_pairs: Vec<(u64, u64)>,
    pub training_ci: BootstrapCi,
    pub heldout_ci: BootstrapCi,
    pub semantic_mismatches: usize,
    /// Honest total overhead = `cand_compile_ns + dispatch_overhead_ns` (NEVER excludes compilation!).
    pub cand_compile_ns: u64,
    pub dispatch_overhead_ns: u64,
    pub measured_overhead_ns: u64,
    pub heldout_base_sum_ns: u64,
    pub heldout_cand_sum_ns: u64,
    pub heldout_net_savings_ns: i64,
    pub break_even_calls: Option<u64>,
    pub profile_sha256: String,
}

impl RetainedProfile {
    fn payload_json(&self) -> JsonValue {
        let pairs_to_json = |ps: &[(u64, u64)]| -> JsonValue {
            JsonValue::Array(
                ps.iter()
                    .map(|&(b, c)| {
                        JsonValue::Array(vec![
                            JsonValue::from_i64(b as i64),
                            JsonValue::from_i64(c as i64),
                        ])
                    })
                    .collect(),
            )
        };
        let mut m = BTreeMap::new();
        m.insert("active_plan".to_string(), self.active_plan.to_json());
        m.insert("baseline_plan".to_string(), self.baseline_plan.to_json());
        m.insert(
            "break_even_calls".to_string(),
            match self.break_even_calls {
                Some(v) => JsonValue::from_i64(v as i64),
                None => JsonValue::Null,
            },
        );
        m.insert(
            "cand_compile_ns".to_string(),
            JsonValue::from_i64(self.cand_compile_ns as i64),
        );
        m.insert("candidate_plan".to_string(), self.candidate_plan.to_json());
        m.insert(
            "cpu_sig_sha256".to_string(),
            JsonValue::from_str_val(self.cpu_sig_sha256.clone()),
        );
        m.insert(
            "decision".to_string(),
            JsonValue::from_str_val(self.decision.as_str()),
        );
        m.insert(
            "dispatch_overhead_ns".to_string(),
            JsonValue::from_i64(self.dispatch_overhead_ns as i64),
        );
        m.insert(
            "generation".to_string(),
            JsonValue::from_usize(self.generation),
        );
        m.insert(
            "heldout_base_sum_ns".to_string(),
            JsonValue::from_i64(self.heldout_base_sum_ns as i64),
        );
        m.insert(
            "heldout_cand_sum_ns".to_string(),
            JsonValue::from_i64(self.heldout_cand_sum_ns as i64),
        );
        m.insert("heldout_ci".to_string(), self.heldout_ci.to_json());
        m.insert(
            "heldout_net_savings_ns".to_string(),
            JsonValue::from_i64(self.heldout_net_savings_ns),
        );
        m.insert(
            "heldout_pairs".to_string(),
            pairs_to_json(&self.heldout_pairs),
        );
        m.insert(
            "kernel_ir_sha256".to_string(),
            JsonValue::from_str_val(self.kernel_ir_sha256.clone()),
        );
        m.insert(
            "measured_overhead_ns".to_string(),
            JsonValue::from_i64(self.measured_overhead_ns as i64),
        );
        m.insert(
            "policy_scope".to_string(),
            JsonValue::from_str_val(self.policy_scope.clone()),
        );
        m.insert(
            "profile_version".to_string(),
            JsonValue::from_i64(self.profile_version as i64),
        );
        m.insert("reason".to_string(), JsonValue::from_str_val(self.reason.clone()));
        m.insert(
            "semantic_mismatches".to_string(),
            JsonValue::from_usize(self.semantic_mismatches),
        );
        m.insert("training_ci".to_string(), self.training_ci.to_json());
        m.insert(
            "training_pairs".to_string(),
            pairs_to_json(&self.training_pairs),
        );
        m.insert(
            "workload_sha256".to_string(),
            JsonValue::from_str_val(self.workload_sha256.clone()),
        );
        JsonValue::Object(m)
    }

    pub fn recompute_sha256(&self) -> String {
        let canon = self.payload_json().to_compact_ascii_json();
        Sha256::digest_hex(canon.as_bytes())
    }

    pub fn to_json(&self) -> JsonValue {
        let JsonValue::Object(mut m) = self.payload_json() else {
            unreachable!()
        };
        m.insert(
            "profile_sha256".to_string(),
            JsonValue::from_str_val(self.profile_sha256.clone()),
        );
        JsonValue::Object(m)
    }

    /// Atomic durable profile persistence: sibling temp -> `sync_all()` -> `rename()` -> parent dir `sync_all()` (`P1-1`).
    pub fn save_atomic(&self, path: &Path) -> Result<(), String> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed creating profile directory {}: {e}", parent.display()))?;

        let seq = PROFILE_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("profile.json");
        let tmp_path = parent.join(format!(".{file_name}.tmp.{}.{seq}", std::process::id()));

        let text = format!("{}\n", self.to_json().to_compact_ascii_json());
        let write_and_rename = (|| -> Result<(), String> {
            {
                let mut f = std::fs::File::create(&tmp_path).map_err(|e| {
                    format!("failed creating temp profile {}: {e}", tmp_path.display())
                })?;
                f.write_all(text.as_bytes())
                    .map_err(|e| format!("failed writing temp profile: {e}"))?;
                f.sync_all()
                    .map_err(|e| format!("failed fsyncing temp profile: {e}"))?;
            }
            std::fs::rename(&tmp_path, path)
                .map_err(|e| format!("failed renaming temp profile to {}: {e}", path.display()))
        })();
        if let Err(e) = write_and_rename {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(e);
        }
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
        Ok(())
    }

    /// Strictly validate & load a retained profile (`P1-1`: pre-read 1 MiB size limit, strict JSON,
    /// SHA-256 digest, bindings, sample counts, and positive timings; any violation -> `FallbackToBaseline`).
    pub fn load_validated(
        path: &Path,
        expected_cpu_sig: &str,
        expected_kernel_ir_sha: &str,
        expected_workload_sha: &str,
    ) -> ProfileLoadOutcome {
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) => {
                return ProfileLoadOutcome::FallbackToBaseline {
                    reason: format!("profile unreadable ({}): {e}", path.display()),
                };
            }
        };
        if meta.len() > MAX_PROFILE_BYTES {
            return ProfileLoadOutcome::FallbackToBaseline {
                reason: format!(
                    "profile exceeds {} byte limit (got {} bytes)",
                    MAX_PROFILE_BYTES,
                    meta.len()
                ),
            };
        }
        let text = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                return ProfileLoadOutcome::FallbackToBaseline {
                    reason: format!("failed reading profile: {e}"),
                };
            }
        };
        let parsed = match parse_strict_json(&text) {
            Ok(j) => j,
            Err(e) => {
                return ProfileLoadOutcome::FallbackToBaseline {
                    reason: format!("strict JSON parse error: {e}"),
                };
            }
        };
        match Self::validate_json(
            &parsed,
            expected_cpu_sig,
            expected_kernel_ir_sha,
            expected_workload_sha,
        ) {
            Ok(prof) => ProfileLoadOutcome::Loaded(prof),
            Err(reason) => ProfileLoadOutcome::FallbackToBaseline { reason },
        }
    }

    fn validate_json(
        v: &JsonValue,
        expected_cpu_sig: &str,
        expected_kernel_ir_sha: &str,
        expected_workload_sha: &str,
    ) -> Result<Self, String> {
        let JsonValue::Object(map) = v else {
            return Err("profile root must be a JSON object".to_string());
        };
        let get_str = |k: &str| -> Result<String, String> {
            match map.get(k) {
                Some(JsonValue::Str(s)) => Ok(s.clone()),
                _ => Err(format!("missing or invalid string field {k}")),
            }
        };
        let get_i64 = |k: &str| -> Result<i64, String> {
            match map.get(k) {
                Some(JsonValue::Int(n)) => {
                    n.to_i64().ok_or_else(|| format!("field {k} out of i64"))
                }
                _ => Err(format!("missing or invalid integer field {k}")),
            }
        };
        let parse_pairs = |k: &str| -> Result<Vec<(u64, u64)>, String> {
            let Some(JsonValue::Array(arr)) = map.get(k) else {
                return Err(format!("missing or invalid array field {k}"));
            };
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                let JsonValue::Array(pair) = item else {
                    return Err(format!("entry in {k} must be [base_ns, cand_ns]"));
                };
                if pair.len() != 2 {
                    return Err(format!("pair in {k} must have length 2"));
                }
                let b = match &pair[0] {
                    JsonValue::Int(n) => n.to_i64().ok_or_else(|| format!("bad base_ns in {k}"))?,
                    _ => return Err(format!("non-integer base_ns in {k}")),
                };
                let c = match &pair[1] {
                    JsonValue::Int(n) => n.to_i64().ok_or_else(|| format!("bad cand_ns in {k}"))?,
                    _ => return Err(format!("non-integer cand_ns in {k}")),
                };
                if b <= 0 || c <= 0 {
                    return Err(format!("timings in {k} must be strictly positive (> 0)"));
                }
                out.push((b as u64, c as u64));
            }
            Ok(out)
        };

        let profile_version_i = get_i64("profile_version")?;
        let profile_version = u32::try_from(profile_version_i)
            .map_err(|_| format!("profile_version out of u32 range: {profile_version_i}"))?;
        if profile_version != PROFILE_VERSION {
            return Err(format!(
                "unsupported profile_version {profile_version} (expected {PROFILE_VERSION})"
            ));
        }
        let policy_scope = get_str("policy_scope")?;
        if policy_scope != POLICY_SCOPE {
            return Err(format!(
                "invalid policy_scope {policy_scope:?} (expected {POLICY_SCOPE:?})"
            ));
        }

        let cpu_sig_sha256 = get_str("cpu_sig_sha256")?;
        if cpu_sig_sha256 != expected_cpu_sig {
            return Err("CPU signature mismatch (profile bound to different CPU)".to_string());
        }
        let kernel_ir_sha256 = get_str("kernel_ir_sha256")?;
        if kernel_ir_sha256 != expected_kernel_ir_sha {
            return Err("kernel IR SHA-256 mismatch (source/IR changed)".to_string());
        }
        let workload_sha256 = get_str("workload_sha256")?;
        if workload_sha256 != expected_workload_sha {
            return Err("workload SHA-256 mismatch (budget/mode changed)".to_string());
        }

        let decision = LearnDecision::from_name(&get_str("decision")?)?;
        let reason = get_str("reason")?;
        let baseline_plan = CodegenPlan::from_json(
            map.get("baseline_plan")
                .ok_or_else(|| "missing baseline_plan".to_string())?,
        )?;
        let active_plan = CodegenPlan::from_json(
            map.get("active_plan")
                .ok_or_else(|| "missing active_plan".to_string())?,
        )?;
        let candidate_plan = CodegenPlan::from_json(
            map.get("candidate_plan")
                .ok_or_else(|| "missing candidate_plan".to_string())?,
        )?;
        let generation_i = get_i64("generation")?;
        if generation_i < 1 {
            return Err("generation must be >= 1".to_string());
        }
        let generation = generation_i as usize;
        let training_pairs = parse_pairs("training_pairs")?;
        let heldout_pairs = parse_pairs("heldout_pairs")?;
        if training_pairs.len() != generation.min(MAX_SAMPLES) {
            return Err(format!(
                "training_pairs.len()={} does not match min(generation={}, {MAX_SAMPLES})",
                training_pairs.len(),
                generation
            ));
        }
        if heldout_pairs.len() < MIN_PAIRS {
            return Err(format!(
                "heldout_pairs.len()={} < MIN_PAIRS ({MIN_PAIRS})",
                heldout_pairs.len()
            ));
        }

        let training_ci = BootstrapCi::from_json(
            map.get("training_ci")
                .ok_or_else(|| "missing training_ci".to_string())?,
        )?;
        let heldout_ci = BootstrapCi::from_json(
            map.get("heldout_ci")
                .ok_or_else(|| "missing heldout_ci".to_string())?,
        )?;
        let semantic_mismatches_i = get_i64("semantic_mismatches")?;
        let semantic_mismatches = usize::try_from(semantic_mismatches_i)
            .map_err(|_| format!("semantic_mismatches out of range: {semantic_mismatches_i}"))?;
        let cand_compile_ns = get_i64("cand_compile_ns")?;
        let dispatch_overhead_ns = get_i64("dispatch_overhead_ns")?;
        let measured_overhead_ns = get_i64("measured_overhead_ns")?;
        if cand_compile_ns <= 0 || dispatch_overhead_ns <= 0 || measured_overhead_ns <= 0 {
            return Err("overhead fields must be strictly positive".to_string());
        }
        if measured_overhead_ns != cand_compile_ns.saturating_add(dispatch_overhead_ns) {
            return Err("measured_overhead_ns != cand_compile_ns + dispatch_overhead_ns".to_string());
        }
        let heldout_base_sum_i = get_i64("heldout_base_sum_ns")?;
        let heldout_base_sum_ns = u64::try_from(heldout_base_sum_i)
            .map_err(|_| format!("heldout_base_sum_ns must be non-negative: {heldout_base_sum_i}"))?;
        let heldout_cand_sum_i = get_i64("heldout_cand_sum_ns")?;
        let heldout_cand_sum_ns = u64::try_from(heldout_cand_sum_i)
            .map_err(|_| format!("heldout_cand_sum_ns must be non-negative: {heldout_cand_sum_i}"))?;
        let heldout_net_savings_ns = get_i64("heldout_net_savings_ns")?;
        let break_even_calls = match map.get("break_even_calls") {
            Some(JsonValue::Null) => None,
            Some(JsonValue::Int(n)) => Some(
                n.to_i64()
                    .filter(|&v| v > 0)
                    .ok_or_else(|| "invalid break_even_calls".to_string())? as u64,
            ),
            _ => return Err("missing or invalid break_even_calls".to_string()),
        };
        let stored_sha = get_str("profile_sha256")?;

        let prof = Self {
            profile_version,
            policy_scope,
            cpu_sig_sha256,
            kernel_ir_sha256,
            workload_sha256,
            decision,
            reason,
            baseline_plan,
            active_plan,
            candidate_plan,
            generation,
            training_pairs,
            heldout_pairs,
            training_ci,
            heldout_ci,
            semantic_mismatches,
            cand_compile_ns: cand_compile_ns as u64,
            dispatch_overhead_ns: dispatch_overhead_ns as u64,
            measured_overhead_ns: measured_overhead_ns as u64,
            heldout_base_sum_ns,
            heldout_cand_sum_ns,
            heldout_net_savings_ns,
            break_even_calls,
            profile_sha256: stored_sha.clone(),
        };

        let recomputed = prof.recompute_sha256();
        if recomputed != stored_sha {
            return Err(format!(
                "profile_sha256 mismatch (stored={stored_sha}, recomputed={recomputed})"
            ));
        }

        // Consistency check on Retain decision
        if prof.decision == LearnDecision::Retain {
            if prof.semantic_mismatches > 0
                || prof.heldout_ci.lower_95_bps <= MIN_LOWER_95_GAIN_BPS
                || prof.heldout_base_sum_ns
                    <= prof
                        .heldout_cand_sum_ns
                        .saturating_add(prof.measured_overhead_ns)
            {
                return Err(
                    "profile claims Retain without satisfying semantic/CI/net-savings invariants"
                        .to_string(),
                );
            }
        } else if prof.active_plan != prof.baseline_plan {
            return Err("Rollback profile must set active_plan == baseline_plan".to_string());
        }

        Ok(prof)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProfileLoadOutcome {
    Loaded(RetainedProfile),
    FallbackToBaseline { reason: String },
}

#[derive(Clone, Debug)]
pub struct LearnSessionReport {
    pub profile: RetainedProfile,
    pub candidates_evaluated: Vec<CandidateEvalSummary>,
    pub baseline_stdout: String,
}

fn time_semantic_exec(sess: &OrcJitSession, fuel: i64, max_depth: i32) -> (SemanticSnapshot, u64) {
    let t0 = Instant::now();
    let (emits, failed, error) = sess.run_semantic_captured(fuel, max_depth);
    let ns = t0.elapsed().as_nanos().max(1) as u64;
    (SemanticSnapshot { emits, failed, error }, ns)
}

/// Run the full hardware-informed, semantically-guarded, honest cost-inclusive learning loop (`P0-4`).
#[allow(clippy::too_many_arguments)]
pub fn run_learn_session<F>(
    hw_report: &HwProbeReport,
    baseline_plan: CodegenPlan,
    kernel_ir_sha256: String,
    workload_sha256: String,
    fuel: i64,
    max_depth: i32,
    training_pairs_count: usize,
    heldout_pairs_count: usize,
    compile_plan: F,
) -> Result<LearnSessionReport, String>
where
    F: Fn(&CodegenPlan) -> Result<(OrcJitSession, u64), String>,
{
    let train_n = training_pairs_count.clamp(MIN_PAIRS, MAX_SAMPLES);
    let held_n = heldout_pairs_count.clamp(MIN_PAIRS, MAX_SAMPLES);

    // 1. Compile baseline plan & capture exact reference semantic snapshot
    let (base_sess, _base_compile_ns) = compile_plan(&baseline_plan)?;
    let (base_sem, _) = time_semantic_exec(&base_sess, fuel, max_depth);
    let mut baseline_stdout = String::new();
    for line in &base_sem.emits {
        baseline_stdout.push_str(line);
        baseline_stdout.push('\n');
    }

    // 2. Hardware-guided candidate list
    let candidates = recommend_candidates(
        hw_report.branch_predictor.prefer_branchless_irregular,
        hw_report.unroll_layout.recommended_unroll,
        hw_report.unroll_layout.prefer_native_cpu,
        &baseline_plan,
    );

    let mut evaluated = Vec::new();
    let mut best_cand: Option<(CodegenPlan, OrcJitSession, u64, Vec<(u64, u64)>, BootstrapCi)> =
        None;

    for cand_plan in candidates {
        let (cand_sess, cand_compile_ns) = compile_plan(&cand_plan)?;
        // Warmup + exact semantic guard check
        let (warm_sem, _) = time_semantic_exec(&cand_sess, fuel, max_depth);
        let mut mismatches = usize::from(warm_sem != base_sem);
        let mut pairs = Vec::with_capacity(train_n);

        if mismatches == 0 {
            for i in 0..train_n {
                // Interleaved alternating order (B,C then C,B) to remove ordering bias
                let (b_ns, c_ns) = if i % 2 == 0 {
                    let (bs, bn) = time_semantic_exec(&base_sess, fuel, max_depth);
                    let (cs, cn) = time_semantic_exec(&cand_sess, fuel, max_depth);
                    if bs != base_sem || cs != base_sem {
                        mismatches += 1;
                    }
                    (bn, cn)
                } else {
                    let (cs, cn) = time_semantic_exec(&cand_sess, fuel, max_depth);
                    let (bs, bn) = time_semantic_exec(&base_sess, fuel, max_depth);
                    if bs != base_sem || cs != base_sem {
                        mismatches += 1;
                    }
                    (bn, cn)
                };
                if mismatches > 0 {
                    break;
                }
                pairs.push((b_ns, c_ns));
            }
        }

        let train_ci = paired_bootstrap_ci_bps(&pairs, BOOTSTRAP_SEED, BOOTSTRAP_RESAMPLES);
        evaluated.push(CandidateEvalSummary {
            plan: cand_plan,
            compile_ns: cand_compile_ns.max(1),
            training_pairs: pairs.clone(),
            training_ci: train_ci,
            semantic_mismatches: mismatches,
        });

        if mismatches == 0 {
            let sum_b: u128 = pairs.iter().map(|&(b, _)| b as u128).sum();
            let sum_c: u128 = pairs.iter().map(|&(_, c)| c as u128).sum();
            let est_net = (held_n as i128)
                .saturating_mul((sum_b as i128) - (sum_c as i128))
                / (pairs.len().max(1) as i128)
                - (cand_compile_ns.max(1) as i128);
            let better = match &best_cand {
                None => true,
                Some((_, _, prev_compile, prev_pairs, prev_ci)) => {
                    let pb: u128 = prev_pairs.iter().map(|&(b, _)| b as u128).sum();
                    let pc: u128 = prev_pairs.iter().map(|&(_, c)| c as u128).sum();
                    let prev_est_net = (held_n as i128)
                        .saturating_mul((pb as i128) - (pc as i128))
                        / (prev_pairs.len().max(1) as i128)
                        - (*prev_compile as i128);
                    let cur_sig = train_ci.lower_95_bps > MIN_LOWER_95_GAIN_BPS;
                    let prev_sig = prev_ci.lower_95_bps > MIN_LOWER_95_GAIN_BPS;
                    (cur_sig && !prev_sig)
                        || (cur_sig == prev_sig && est_net > prev_est_net)
                }
            };
            if better {
                best_cand = Some((cand_plan, cand_sess, cand_compile_ns.max(1), pairs, train_ci));
            }
        }
    }

    // 3. Held-out interleaved validation + honest compile-inclusive net-gain gate (`P0-4`)
    let (cand_plan, cand_sess, cand_compile_ns, train_pairs, train_ci) = match best_cand {
        Some(b) => b,
        None => {
            // Every candidate failed semantic guard -> strict rollback to baseline
            let mut held_pairs = Vec::with_capacity(held_n);
            for _ in 0..held_n {
                let (_, bn) = time_semantic_exec(&base_sess, fuel, max_depth);
                held_pairs.push((bn, bn));
            }
            let sum_b: u64 = held_pairs.iter().map(|(b, _)| *b).sum();
            let mut prof = RetainedProfile {
                profile_version: PROFILE_VERSION,
                policy_scope: POLICY_SCOPE.to_string(),
                cpu_sig_sha256: hw_report.hardware.cpu_sig_sha256.clone(),
                kernel_ir_sha256,
                workload_sha256,
                decision: LearnDecision::Rollback,
                reason: "all candidates failed Exact Semantic Guard".to_string(),
                baseline_plan,
                active_plan: baseline_plan,
                candidate_plan: baseline_plan,
                generation: 1,
                training_pairs: vec![held_pairs[0]],
                heldout_pairs: held_pairs,
                training_ci: BootstrapCi {
                    point_gain_bps: 0,
                    lower_95_bps: 0,
                    upper_95_bps: 0,
                    resamples: BOOTSTRAP_RESAMPLES,
                    seed: BOOTSTRAP_SEED,
                },
                heldout_ci: BootstrapCi {
                    point_gain_bps: 0,
                    lower_95_bps: 0,
                    upper_95_bps: 0,
                    resamples: BOOTSTRAP_RESAMPLES,
                    seed: BOOTSTRAP_SEED,
                },
                semantic_mismatches: 1,
                cand_compile_ns: 1,
                dispatch_overhead_ns: 1,
                measured_overhead_ns: 2,
                heldout_base_sum_ns: sum_b,
                heldout_cand_sum_ns: sum_b,
                heldout_net_savings_ns: -2,
                break_even_calls: None,
                profile_sha256: String::new(),
            };
            prof.profile_sha256 = prof.recompute_sha256();
            return Ok(LearnSessionReport {
                profile: prof,
                candidates_evaluated: evaluated,
                baseline_stdout,
            });
        }
    };

    let t_disp = Instant::now();
    let _plan_check = cand_plan.plan_id();
    let dispatch_overhead_ns = t_disp.elapsed().as_nanos().max(1) as u64;
    let measured_overhead_ns = cand_compile_ns.saturating_add(dispatch_overhead_ns);

    let mut held_mismatches = 0usize;
    let mut heldout_pairs = Vec::with_capacity(held_n);
    for i in 0..held_n {
        let (b_ns, c_ns) = if i % 2 == 0 {
            let (bs, bn) = time_semantic_exec(&base_sess, fuel, max_depth);
            let (cs, cn) = time_semantic_exec(&cand_sess, fuel, max_depth);
            if bs != base_sem || cs != base_sem {
                held_mismatches += 1;
            }
            (bn, cn)
        } else {
            let (cs, cn) = time_semantic_exec(&cand_sess, fuel, max_depth);
            let (bs, bn) = time_semantic_exec(&base_sess, fuel, max_depth);
            if bs != base_sem || cs != base_sem {
                held_mismatches += 1;
            }
            (bn, cn)
        };
        heldout_pairs.push((b_ns, c_ns));
    }

    let heldout_ci = paired_bootstrap_ci_bps(&heldout_pairs, BOOTSTRAP_SEED, BOOTSTRAP_RESAMPLES);
    let heldout_base_sum_ns: u64 = heldout_pairs.iter().map(|(b, _)| *b).sum();
    let heldout_cand_sum_ns: u64 = heldout_pairs.iter().map(|(_, c)| *c).sum();
    let net_savings_i128 = (heldout_base_sum_ns as i128)
        - (heldout_cand_sum_ns as i128)
        - (measured_overhead_ns as i128);
    let heldout_net_savings_ns = net_savings_i128.clamp(i64::MIN as i128, i64::MAX as i128) as i64;

    let avg_base = heldout_base_sum_ns / (held_n as u64);
    let avg_cand = heldout_cand_sum_ns / (held_n as u64);
    let break_even_calls = if avg_base > avg_cand {
        Some(measured_overhead_ns.div_ceil(avg_base - avg_cand))
    } else {
        None
    };

    let (decision, active_plan, reason) = if held_mismatches > 0 {
        (
            LearnDecision::Rollback,
            baseline_plan,
            format!("held-out Exact Semantic Guard failed ({held_mismatches} mismatches)"),
        )
    } else if heldout_ci.lower_95_bps <= MIN_LOWER_95_GAIN_BPS {
        (
            LearnDecision::Rollback,
            baseline_plan,
            format!(
                "held-out lower 95% CI {} bps <= {} bps threshold",
                heldout_ci.lower_95_bps, MIN_LOWER_95_GAIN_BPS
            ),
        )
    } else if heldout_base_sum_ns <= heldout_cand_sum_ns.saturating_add(measured_overhead_ns) {
        (
            LearnDecision::Rollback,
            baseline_plan,
            format!(
                "honest compile-inclusive overhead ({} ns) not amortized over {} held-out runs (net {} ns)",
                measured_overhead_ns, held_n, heldout_net_savings_ns
            ),
        )
    } else {
        (
            LearnDecision::Retain,
            cand_plan,
            format!(
                "retained {}: held-out lower 95% CI +{} bps, honest net savings +{} ns (compile+dispatch overhead {} ns amortized)",
                cand_plan.plan_id(),
                heldout_ci.lower_95_bps,
                heldout_net_savings_ns,
                measured_overhead_ns
            ),
        )
    };

    let generation = train_pairs.len();
    let mut profile = RetainedProfile {
        profile_version: PROFILE_VERSION,
        policy_scope: POLICY_SCOPE.to_string(),
        cpu_sig_sha256: hw_report.hardware.cpu_sig_sha256.clone(),
        kernel_ir_sha256,
        workload_sha256,
        decision,
        reason,
        baseline_plan,
        active_plan,
        candidate_plan: cand_plan,
        generation,
        training_pairs: train_pairs,
        heldout_pairs,
        training_ci: train_ci,
        heldout_ci,
        semantic_mismatches: held_mismatches,
        cand_compile_ns,
        dispatch_overhead_ns,
        measured_overhead_ns,
        heldout_base_sum_ns,
        heldout_cand_sum_ns,
        heldout_net_savings_ns,
        break_even_calls,
        profile_sha256: String::new(),
    };
    profile.profile_sha256 = profile.recompute_sha256();

    Ok(LearnSessionReport {
        profile,
        candidates_evaluated: evaluated,
        baseline_stdout,
    })
}
