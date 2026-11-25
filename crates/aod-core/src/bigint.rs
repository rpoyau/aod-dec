//! Unified exact integer type backed by 2-bit balanced ternary `TritInt`
//! (inline 32-trit `u64` fast path + base-`3^32` balanced `BigTrit` limbs).

pub use crate::trit_int::{BigTrit, TritInt, MAX_POW_BITS, MAX_POW_EXP};

/// Canonical exact integer alias across `aod-core`.
pub type BigInt = TritInt;
