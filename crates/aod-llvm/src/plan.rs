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
}

impl CodegenPlan {
    pub fn baseline_o0() -> Self {
        Self {
            opt_level: OptLevel::O0,
            branchless_select: false,
            unroll_hint: UnrollHint::Default,
            target_cpu_native: false,
            inline_ratio_fastpath: true,
        }
    }

    pub fn baseline_o2() -> Self {
        Self {
            opt_level: OptLevel::O2,
            branchless_select: false,
            unroll_hint: UnrollHint::Default,
            target_cpu_native: true,
            inline_ratio_fastpath: true,
        }
    }

    pub fn for_opt_level(opt_level: OptLevel) -> Self {
        Self {
            opt_level,
            branchless_select: false,
            unroll_hint: UnrollHint::Default,
            target_cpu_native: opt_level != OptLevel::O0,
            inline_ratio_fastpath: true,
        }
    }

    pub fn plan_id(&self) -> String {
        format!(
            "{}_sel{}_u{}_cpu{}_rat{}",
            self.opt_level.as_str(),
            if self.branchless_select { 1 } else { 0 },
            self.unroll_hint.as_str(),
            if self.target_cpu_native { "native" } else { "generic" },
            if self.inline_ratio_fastpath { 1 } else { 0 }
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
            "opt_level".to_string(),
            JsonValue::from_str_val(self.opt_level.as_str()),
        );
        m.insert("plan_id".to_string(), JsonValue::from_str_val(self.plan_id()));
        m.insert(
            "target_cpu_native".to_string(),
            JsonValue::Bool(self.target_cpu_native),
        );
        m.insert(
            "unroll_hint".to_string(),
            JsonValue::from_str_val(self.unroll_hint.as_str()),
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
        let opt_level = OptLevel::from_name(get_str("opt_level")?).map_err(|e| e.message)?;
        let unroll_hint = UnrollHint::from_name(get_str("unroll_hint")?)?;
        let plan = Self {
            opt_level,
            branchless_select: get_bool("branchless_select")?,
            unroll_hint,
            target_cpu_native: get_bool("target_cpu_native")?,
            inline_ratio_fastpath: get_bool("inline_ratio_fastpath")?,
        };
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

/// Deterministically recommend hardware-informed candidate plans distinct from `baseline`.
pub fn recommend_candidates(
    prefer_branchless: bool,
    recommended_unroll: UnrollHint,
    prefer_native_cpu: bool,
    baseline: &CodegenPlan,
) -> Vec<CodegenPlan> {
    let raw = [
        // 1. Hardware-probe-guided O2 plan
        CodegenPlan {
            opt_level: OptLevel::O2,
            branchless_select: prefer_branchless,
            unroll_hint: recommended_unroll,
            target_cpu_native: prefer_native_cpu,
            inline_ratio_fastpath: true,
        },
        // 2. Clean O2 native default unroll
        CodegenPlan {
            opt_level: OptLevel::O2,
            branchless_select: false,
            unroll_hint: UnrollHint::Default,
            target_cpu_native: true,
            inline_ratio_fastpath: true,
        },
        // 3. O2 with branchless select + Unroll4
        CodegenPlan {
            opt_level: OptLevel::O2,
            branchless_select: true,
            unroll_hint: UnrollHint::Unroll4,
            target_cpu_native: true,
            inline_ratio_fastpath: true,
        },
        // 4. O3 aggressive native plan
        CodegenPlan {
            opt_level: OptLevel::O3,
            branchless_select: prefer_branchless,
            unroll_hint: UnrollHint::Unroll4,
            target_cpu_native: true,
            inline_ratio_fastpath: true,
        },
    ];
    let mut out = Vec::new();
    for c in raw {
        if &c != baseline && !out.contains(&c) {
            out.push(c);
        }
    }
    out
}
