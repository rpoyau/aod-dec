#![deny(unsafe_code)]

//! `aod-llvm`: `std`-only LLVM 18 C API dynamic loader (`dlopen`), IR verifier/optimizer,
//! standalone AOT ELF emitter/linker, and in-process ORC LLJIT session.
//!
//! `unsafe` is strictly quarantined to `src/{ffi.rs, aot.rs, jit.rs}`. Zero IEEE-754 floats.

pub mod aot;
pub mod ffi;
pub mod jit;

use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum OptLevel {
    O0,
    O1,
    O2,
    O3,
}

impl OptLevel {
    pub fn from_u8(level: u8) -> Result<Self, LlvmError> {
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

pub use aot::{emit_object_file, find_aod_rt_lib, link_aot_executable, verify_and_optimize_ir};
pub use jit::OrcJitSession;
