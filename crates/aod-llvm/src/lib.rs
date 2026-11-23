#![deny(unsafe_code)]

//! `aod-llvm`: `std`-only LLVM 18 C API dynamic loader (`dlopen`), parameterized `CodegenPlan`,
//! CPU discovery (`hw_info`), 5 live self-verifying JIT micro-probes (`hw_probe`), zero-float
//! paired bootstrap hardware learner (`learner`), standalone AOT ELF emitter/linker, and ORC LLJIT.
//!
//! `unsafe` is strictly quarantined to `src/{ffi.rs, aot.rs, jit.rs, hw_probe.rs}`. Zero IEEE-754 floats.

pub mod aot;
pub mod ffi;
pub mod hw_info;
pub mod hw_probe;
pub mod jit;
pub mod learner;
pub mod plan;

use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LlvmError {
    pub message: String,
}

impl LlvmError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for LlvmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LLVM error: {}", self.message)
    }
}

impl std::error::Error for LlvmError {}

pub type LlvmResult<T> = Result<T, LlvmError>;

pub use aot::{
    emit_object_file, emit_object_file_with_plan, find_aod_rt_lib, link_aot_executable,
    link_aot_executable_with_plan, verify_and_optimize_ir, verify_and_optimize_ir_with_plan,
};
pub use hw_info::HostHardwareInfo;
pub use hw_probe::{probe_perf_event_open, read_rdtscp, run_all_hw_probes, HwProbeReport, PerfCapability};
pub use jit::OrcJitSession;
pub use learner::{
    paired_bootstrap_ci_bps, run_learn_session, BootstrapCi, LearnDecision, LearnSessionReport,
    ProfileLoadOutcome, RetainedProfile,
};
pub use plan::{recommend_candidates, CodegenPlan, OptLevel, UnrollHint, POLICY_SCOPE};
