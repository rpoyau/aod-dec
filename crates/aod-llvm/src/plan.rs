use std::collections::BTreeMap;

use aod_core::json::JsonValue;

use crate::{LlvmError, LlvmResult};

/// Explicit provenance scope tag required on every hardware probe report and retained profile.
pub const POLICY_SCOPE: &str = "COMPILER_EXECUTION_POLICY_NOT_PHYSICAL_AOD_LAW";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum OptLevel {
    O0,
    O1,
    O2,
    O3,
}

impl OptLevel {
    pub fn from_u8(level: u8) -> LlvmResult<Self> {
        match level {
            0 => Ok(OptLevel::O0),
            1 => Ok(OptLevel::O1),
            2 => Ok(OptLevel::O2),
            3 => Ok(OptLevel::O3),
            other => Err(LlvmError::new(format!(
                "invalid optimization level -O{other}; expected 0, 1, 2, or 3"
            ))),
        }
    }

    pub fn from_name(s: &str) -> LlvmResult<Self> {
        match s {
            "O0" | "0" => Ok(OptLevel::O0),
            "O1" | "1" => Ok(OptLevel::O1),
            "O2" | "2" => Ok(OptLevel::O2),
            "O3" | "3" => Ok(OptLevel::O3),
            other => Err(LlvmError::new(format!(
                "invalid optimization level {other:?}; expected O0, O1, O2, or O3"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            OptLevel::O0 => "O0",
            OptLevel::O1 => "O1",
            OptLevel::O2 => "O2",
            OptLevel::O3 => "O3",
        }
    }

    pub fn pass_pipeline(self) -> &'static str {
        match self {
            OptLevel::O0 => "default<O0>",
            OptLevel::O1 => "default<O1>",
            OptLevel::O2 => "default<O2>",
            OptLevel::O3 => "default<O3>",
        }
    }

    pub fn codegen_opt_level(self) -> u32 {
        match self {
            OptLevel::O0 => 0,
            OptLevel::O1 => 1,
            OptLevel::O2 => 2,
            OptLevel::O3 => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum UnrollHint {
    Default,
    Unroll4,
    Disable,
}

impl UnrollHint {
    pub fn as_str(self) -> &'static str {
        match self {
            UnrollHint::Default => "default",
            UnrollHint::Unroll4 => "unroll4",
            UnrollHint::Disable => "disable",
        }
    }

    pub fn from_name(s: &str) -> Result<Self, String> {
        match s {
            "default" => Ok(UnrollHint::Default),
            "unroll4" => Ok(UnrollHint::Unroll4),
            "disable" => Ok(UnrollHint::Disable),
            other => Err(format!("invalid unroll_hint {other:?}")),
        }
    }
}

/// Parameterized LLVM 18 lowering & backend compilation plan learned per CPU & workload.
/// Strictly an execution policy knob — NEVER alters exact AOD/Dec semantics.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct CodegenPlan {
    pub opt_level: OptLevel,
    pub branchless_select: bool,
    pub unroll_hint: UnrollHint,
    pub target_cpu_native: bool,
    pub inline_ratio_fastpath: bool,
    pub trit_ssa_promotion: bool,
    pub vectorize_loops: bool,
    /// LLVM loop interleave count hint: strictly `0` (default/off), `2`, or `4`.
    pub interleave_count: u8,
    pub layout_prefer_soa: bool,
    pub parallel_observers: bool,
}

/// Hardware-discovered policy hints feeding `recommend_candidates` (strictly non-physical policy).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HwPlanHints {
    pub prefer_branchless: bool,
    pub recommended_unroll: UnrollHint,
    pub prefer_native_cpu: bool,
    pub has_avx2: bool,
    pub prefer_soa: bool,
    pub recommended_workers: usize,
}

impl CodegenPlan {
    pub fn baseline_o0() -> Self {
        Self {
            opt_level: OptLevel::O0,
            branchless_select: false,
            unroll_hint: UnrollHint::Default,
            target_cpu_native: false,
            inline_ratio_fastpath: true,
            trit_ssa_promotion: false,
            vectorize_loops: false,
            interleave_count: 0,
            layout_prefer_soa: false,
            parallel_observers: false,
        }
    }

    pub fn baseline_o2() -> Self {
        Self {
            opt_level: OptLevel::O2,
            branchless_select: false,
            unroll_hint: UnrollHint::Default,
            target_cpu_native: true,
            inline_ratio_fastpath: true,
            trit_ssa_promotion: true,
            vectorize_loops: false,
            interleave_count: 0,
            layout_prefer_soa: false,
            parallel_observers: false,
        }
    }

    pub fn for_opt_level(opt_level: OptLevel) -> Self {
        Self {
            opt_level,
            branchless_select: false,
            unroll_hint: UnrollHint::Default,
            target_cpu_native: opt_level != OptLevel::O0,
            inline_ratio_fastpath: true,
            trit_ssa_promotion: opt_level != OptLevel::O0,
            vectorize_loops: false,
            interleave_count: 0,
            layout_prefer_soa: false,
            parallel_observers: false,
        }
    }

    /// Strictly validate `CodegenPlan` invariants (`P0-4`: `interleave_count` in `{0, 2, 4}`).
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.interleave_count, 0 | 2 | 4) {
            return Err(format!(
                "invalid CodegenPlan.interleave_count {} (expected 0, 2, or 4)",
                self.interleave_count
            ));
        }
        Ok(())
    }

    pub fn plan_id(&self) -> String {
        format!(
            "{}_sel{}_u{}_cpu{}_rat{}_trit{}_vec{}_ic{}_soa{}_par{}",
            self.opt_level.as_str(),
            u8::from(self.branchless_select),
            self.unroll_hint.as_str(),
            if self.target_cpu_native {
                "native"
            } else {
                "generic"
            },
            u8::from(self.inline_ratio_fastpath),
            u8::from(self.trit_ssa_promotion),
            u8::from(self.vectorize_loops),
            self.interleave_count,
            u8::from(self.layout_prefer_soa),
            u8::from(self.parallel_observers),
        )
    }

    pub fn to_json(&self) -> JsonValue {
        let mut m = BTreeMap::new();
        m.insert(
            "branchless_select".to_string(),
            JsonValue::Bool(self.branchless_select),
        );
        m.insert(
            "inline_ratio_fastpath".to_string(),
            JsonValue::Bool(self.inline_ratio_fastpath),
        );
        m.insert(
            "interleave_count".to_string(),
            JsonValue::from_i64(i64::from(self.interleave_count)),
        );
        m.insert(
            "layout_prefer_soa".to_string(),
            JsonValue::Bool(self.layout_prefer_soa),
        );
        m.insert(
            "opt_level".to_string(),
            JsonValue::from_str_val(self.opt_level.as_str()),
        );
        m.insert(
            "parallel_observers".to_string(),
            JsonValue::Bool(self.parallel_observers),
        );
        m.insert("plan_id".to_string(), JsonValue::from_str_val(self.plan_id()));
        m.insert(
            "target_cpu_native".to_string(),
            JsonValue::Bool(self.target_cpu_native),
        );
        m.insert(
            "trit_ssa_promotion".to_string(),
            JsonValue::Bool(self.trit_ssa_promotion),
        );
        m.insert(
            "unroll_hint".to_string(),
            JsonValue::from_str_val(self.unroll_hint.as_str()),
        );
        m.insert(
            "vectorize_loops".to_string(),
            JsonValue::Bool(self.vectorize_loops),
        );
        JsonValue::Object(m)
    }

    pub fn from_json(v: &JsonValue) -> Result<Self, String> {
        let JsonValue::Object(map) = v else {
            return Err("plan must be a JSON object".to_string());
        };
        let get_str = |k: &str| -> Result<&str, String> {
            match map.get(k) {
                Some(JsonValue::Str(s)) => Ok(s.as_str()),
                _ => Err(format!("missing or invalid string field plan.{k}")),
            }
        };
        let get_bool = |k: &str| -> Result<bool, String> {
            match map.get(k) {
                Some(JsonValue::Bool(b)) => Ok(*b),
                _ => Err(format!("missing or invalid bool field plan.{k}")),
            }
        };
        let get_u8 = |k: &str| -> Result<u8, String> {
            match map.get(k) {
                Some(JsonValue::Int(n)) => n
                    .to_i64()
                    .and_then(|v| u8::try_from(v).ok())
                    .ok_or_else(|| format!("plan.{k} out of u8 range")),
                _ => Err(format!("missing or invalid integer field plan.{k}")),
            }
        };
        let opt_level = OptLevel::from_name(get_str("opt_level")?).map_err(|e| e.message)?;
        let unroll_hint = UnrollHint::from_name(get_str("unroll_hint")?)?;
        let trit_ssa_promotion = match map.get("trit_ssa_promotion") {
            Some(JsonValue::Bool(b)) => *b,
            None => opt_level != OptLevel::O0,
            _ => return Err("invalid bool field plan.trit_ssa_promotion".to_string()),
        };
        let plan = Self {
            opt_level,
            branchless_select: get_bool("branchless_select")?,
            unroll_hint,
            target_cpu_native: get_bool("target_cpu_native")?,
            inline_ratio_fastpath: get_bool("inline_ratio_fastpath")?,
            trit_ssa_promotion,
            vectorize_loops: get_bool("vectorize_loops")?,
            interleave_count: get_u8("interleave_count")?,
            layout_prefer_soa: get_bool("layout_prefer_soa")?,
            parallel_observers: get_bool("parallel_observers")?,
        };
        plan.validate()?;
        if let Some(JsonValue::Str(pid)) = map.get("plan_id") {
            if pid != &plan.plan_id() {
                return Err(format!(
                    "plan_id mismatch: expected {}, got {pid}",
                    plan.plan_id()
                ));
            }
        }
        Ok(plan)
    }
}

/// Deterministically recommend up to 6 hardware-informed candidate plans distinct from `baseline` (`P2-1`).
pub fn recommend_candidates(hints: &HwPlanHints, baseline: &CodegenPlan) -> Vec<CodegenPlan> {
    let par_ok = hints.recommended_workers > 1;
    let raw = [
        // 1. Hardware-probe-guided O2 plan with SSA shadow-ALU promotion
        CodegenPlan {
            opt_level: OptLevel::O2,
            branchless_select: hints.prefer_branchless,
            unroll_hint: hints.recommended_unroll,
            target_cpu_native: hints.prefer_native_cpu,
            inline_ratio_fastpath: true,
            trit_ssa_promotion: true,
            vectorize_loops: hints.has_avx2,
            interleave_count: if hints.has_avx2 { 2 } else { 0 },
            layout_prefer_soa: hints.prefer_soa,
            parallel_observers: par_ok,
        },
        // 2. Clean O2 native default unroll
        CodegenPlan {
            opt_level: OptLevel::O2,
            branchless_select: false,
            unroll_hint: UnrollHint::Default,
            target_cpu_native: true,
            inline_ratio_fastpath: true,
            trit_ssa_promotion: true,
            vectorize_loops: false,
            interleave_count: 0,
            layout_prefer_soa: false,
            parallel_observers: false,
        },
        // 3. O2 branchless + Unroll4 + vectorized interleave-2
        CodegenPlan {
            opt_level: OptLevel::O2,
            branchless_select: true,
            unroll_hint: UnrollHint::Unroll4,
            target_cpu_native: true,
            inline_ratio_fastpath: true,
            trit_ssa_promotion: true,
            vectorize_loops: true,
            interleave_count: 2,
            layout_prefer_soa: hints.prefer_soa,
            parallel_observers: false,
        },
        // 4. O2 vectorized interleave-4 + SoA layout hint
        CodegenPlan {
            opt_level: OptLevel::O2,
            branchless_select: hints.prefer_branchless,
            unroll_hint: UnrollHint::Default,
            target_cpu_native: true,
            inline_ratio_fastpath: true,
            trit_ssa_promotion: true,
            vectorize_loops: true,
            interleave_count: 4,
            layout_prefer_soa: true,
            parallel_observers: false,
        },
        // 5. O2 parallel read-only observer plan
        CodegenPlan {
            opt_level: OptLevel::O2,
            branchless_select: false,
            unroll_hint: UnrollHint::Default,
            target_cpu_native: true,
            inline_ratio_fastpath: true,
            trit_ssa_promotion: true,
            vectorize_loops: false,
            interleave_count: 0,
            layout_prefer_soa: false,
            parallel_observers: par_ok,
        },
        // 6. O3 aggressive native vectorized plan
        CodegenPlan {
            opt_level: OptLevel::O3,
            branchless_select: hints.prefer_branchless,
            unroll_hint: UnrollHint::Unroll4,
            target_cpu_native: true,
            inline_ratio_fastpath: true,
            trit_ssa_promotion: true,
            vectorize_loops: true,
            interleave_count: if hints.has_avx2 { 4 } else { 2 },
            layout_prefer_soa: hints.prefer_soa,
            parallel_observers: par_ok,
        },
    ];
    let mut out = Vec::new();
    for c in raw {
        if &c != baseline && !out.contains(&c) {
            out.push(c);
            if out.len() == 6 {
                break;
            }
        }
    }
    out
}
