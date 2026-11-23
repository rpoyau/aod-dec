#![deny(unsafe_code)]

//! `aod-rt`: shared exact runtime, deterministic sandbox chamber engine, safe 1-based arena,
//! and hardened flat C ABI (`libaod_rt.a`) for AOD/Dec v2 AOT & ORC LLJIT backends.
//!
//! `unsafe` is strictly quarantined to `src/abi.rs`. Zero IEEE-754 floats.

pub mod abi;
pub mod arena;
pub mod ops;
pub mod sim;
pub mod value;
