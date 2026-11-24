#![forbid(unsafe_code)]

//! `aod-core`: 100% `std`-only, float-free exact mathematical core for Alpha-Omega Dynamics (AOD)
//! and Dec v2.
//!
//! Zero external crates.io dependencies. Zero IEEE-754 floating-point types or literals.

pub mod bigint;
pub mod error;
pub mod json;
pub mod ledger;
pub mod qext;
pub mod ratio;
pub mod s2_kernel;
pub mod s2_vectors;
pub mod s4_report;
pub mod sha256;
pub mod support;
pub mod ternary;
pub mod wavelet;
pub mod witness;
