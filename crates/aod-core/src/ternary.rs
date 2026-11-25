use std::fmt;

use crate::bigint::BigInt;
use crate::error::{CoreError, CoreResult};
use crate::trit_int::{swar_unpack_t32, TritInt};

/// Balanced single trit in `{-1, 0, +1}` (`00 = 0`, `01 = +1`, `10 = -1`; `11` rejected).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct T1(i8);

impl T1 {
    pub const NEG: Self = Self(-1);
    pub const ZERO: Self = Self(0);
    pub const POS: Self = Self(1);

    pub fn new(v: i8) -> CoreResult<Self> {
        match v {
            -1 | 0 | 1 => Ok(Self(v)),
            _ => Err(CoreError::DomainError(format!("t1 outside {{-1,0,+1}}: {v}"))),
        }
    }

    pub fn from_bigint(v: &BigInt) -> CoreResult<Self> {
        let n = v
            .to_i64()
            .ok_or_else(|| CoreError::DomainError(format!("t1 outside {{-1,0,+1}}: {v}")))?;
        match n {
            -1 => Ok(Self::NEG),
            0 => Ok(Self::ZERO),
            1 => Ok(Self::POS),
            _ => Err(CoreError::DomainError(format!("t1 outside {{-1,0,+1}}: {v}"))),
        }
    }

    #[inline]
    pub const fn value(self) -> i8 {
        self.0
    }

    /// 2-bit encoding: `0 -> 0b00`, `+1 -> 0b01`, `-1 -> 0b10`.
    #[inline]
    pub const fn pack2(self) -> u8 {
        match self.0 {
            0 => 0,
            1 => 1,
            -1 => 2,
            _ => unreachable!(),
        }
    }

    /// Decode a 2-bit lane, rejecting `0b11` (`3`) or `>= 4`.
    #[inline]
    pub fn unpack2(code: u8) -> CoreResult<Self> {
        match code {
            0 => Ok(Self::ZERO),
            1 => Ok(Self::POS),
            2 => Ok(Self::NEG),
            3 => Err(CoreError::InvalidTritEncoding(3)),
            other => Err(CoreError::InvalidTritEncoding(other)),
        }
    }

    #[inline]
    pub const fn checked_neg(self) -> Self {
        Self(-self.0)
    }

    pub fn checked_add(self, rhs: Self) -> CoreResult<Self> {
        Self::new(self.0 + rhs.0)
    }

    pub fn checked_sub(self, rhs: Self) -> CoreResult<Self> {
        Self::new(self.0 - rhs.0)
    }

    #[inline]
    pub const fn checked_mul(self, rhs: Self) -> CoreResult<Self> {
        Ok(Self(self.0 * rhs.0))
    }

    /// Exact 9-pair idempotent superposition & opposing cancellation (`+1` meets `-1` -> `0`).
    #[inline]
    pub const fn superpose_cancel(self, rhs: Self) -> Self {
        let sum = self.0 + rhs.0;
        if sum > 0 {
            Self::POS
        } else if sum < 0 {
            Self::NEG
        } else {
            Self::ZERO
        }
    }

    pub fn render(self) -> String {
        format!("t1({})", self.0)
    }

    pub fn render_v1(self) -> String {
        self.render()
    }
}

impl fmt::Display for T1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "t1({})", self.0)
    }
}

/// Raw non-arithmetic 8-bit octet (`0x00..=0xFF`) for `bytes`, syscalls, and binary I/O.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct B8(pub u8);

impl B8 {
    pub fn from_int(v: &TritInt) -> CoreResult<Self> {
        let n = v
            .to_u64()
            .ok_or_else(|| CoreError::DomainError(format!("b8 outside 0..=255: {v}")))?;
        if n <= 255 {
            Ok(Self(n as u8))
        } else {
            Err(CoreError::DomainError(format!("b8 outside 0..=255: {v}")))
        }
    }

    #[inline]
    pub const fn value(self) -> u8 {
        self.0
    }

    #[inline]
    pub const fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }

    #[inline]
    pub const fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }

    #[inline]
    pub const fn bitxor(self, rhs: Self) -> Self {
        Self(self.0 ^ rhs.0)
    }

    #[inline]
    pub const fn bitnot(self) -> Self {
        Self(!self.0)
    }

    pub fn checked_shl(self, shift: u32) -> CoreResult<Self> {
        if shift < 8 {
            Ok(Self(self.0 << shift))
        } else {
            Err(CoreError::DomainError(format!(
                "b8 shift count must be in 0..8, got {shift}"
            )))
        }
    }

    pub fn checked_shr(self, shift: u32) -> CoreResult<Self> {
        if shift < 8 {
            Ok(Self(self.0 >> shift))
        } else {
            Err(CoreError::DomainError(format!(
                "b8 shift count must be in 0..8, got {shift}"
            )))
        }
    }

    pub fn render(self) -> String {
        format!("b8(0x{:02x})", self.0)
    }
}

impl fmt::Display for B8 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render())
    }
}

/// Raw non-arithmetic 64-bit hardware register view (`b64`).
///
/// Inspecting an inline 32-trit `int` via `B64::from_trit_int` is a 0-cycle bitcast returning
/// its exact 2-bit-per-trit 64-bit hardware word (`w < 0xC000_0000_0000_0000`), while
/// `B64::to_trit_int` unpacks/validates 32 trits and rejects any `0b11` lane with `InvalidTritEncoding(3)`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct B64(pub u64);

impl B64 {
    #[inline]
    pub const fn new(bits: u64) -> Self {
        Self(bits)
    }

    /// Zero-cost register view of an inline 32-trit `TritInt`.
    /// Rejects spilled `> 32`-trit integers with `Overflow`.
    pub fn from_trit_int(v: &TritInt) -> CoreResult<Self> {
        match v.inline_word() {
            Some(w) => Ok(Self(w)),
            None => Err(CoreError::Overflow(format!(
                "int exceeds 32-trit single-register view for b64: {v}"
            ))),
        }
    }

    /// Construct from an unsigned bit-pattern integer in `0..=u64::MAX` (for `0x`/`0b` literals).
    pub fn from_unsigned_int(v: &TritInt) -> CoreResult<Self> {
        let bits = v
            .to_u64()
            .ok_or_else(|| CoreError::Overflow(format!("b64 unsigned literal out of range: {v}")))?;
        Ok(Self(bits))
    }

    /// Reinterpret a 64-bit word as a 32-trit `TritInt`, rejecting any `0b11` lane!
    pub fn to_trit_int(self) -> CoreResult<TritInt> {
        let _ = swar_unpack_t32(self.0)?;
        TritInt::from_inline_word(self.0)
    }

    #[inline]
    pub const fn value(self) -> u64 {
        self.0
    }

    #[inline]
    pub const fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }

    #[inline]
    pub const fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }

    #[inline]
    pub const fn bitxor(self, rhs: Self) -> Self {
        Self(self.0 ^ rhs.0)
    }

    #[inline]
    pub const fn bitnot(self) -> Self {
        Self(!self.0)
    }

    #[inline]
    pub const fn popcnt(self) -> u32 {
        self.0.count_ones()
    }

    pub fn checked_shl(self, shift: u32) -> CoreResult<Self> {
        if shift < 64 {
            Ok(Self(self.0 << shift))
        } else {
            Err(CoreError::DomainError(format!(
                "b64 shift count must be in 0..64, got {shift}"
            )))
        }
    }

    pub fn checked_shr(self, shift: u32) -> CoreResult<Self> {
        if shift < 64 {
            Ok(Self(self.0 >> shift))
        } else {
            Err(CoreError::DomainError(format!(
                "b64 shift count must be in 0..64, got {shift}"
            )))
        }
    }

    pub fn render(self) -> String {
        format!("b64(0x{:016x})", self.0)
    }
}

impl fmt::Display for B64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render())
    }
}

/// Finite 4-cube vertex in `Q_4 = {0, ..., 15}`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Q4(u8);

impl Q4 {
    pub fn new(v: u8) -> CoreResult<Self> {
        if v < 16 {
            Ok(Self(v))
        } else {
            Err(CoreError::DomainError(format!("q4 outside 0..15: {v}")))
        }
    }

    pub fn from_bigint(v: &BigInt) -> CoreResult<Self> {
        let n = v
            .to_u64()
            .ok_or_else(|| CoreError::DomainError(format!("q4 outside 0..15: {v}")))?;
        if n < 16 {
            Ok(Self(n as u8))
        } else {
            Err(CoreError::DomainError(format!("q4 outside 0..15: {v}")))
        }
    }

    #[inline]
    pub const fn value(self) -> u8 {
        self.0
    }

    #[inline]
    pub const fn bitand(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    #[inline]
    pub const fn bitor(self, other: Self) -> Self {
        Self((self.0 | other.0) & 0x0F)
    }

    #[inline]
    pub const fn bitxor(self, other: Self) -> Self {
        Self((self.0 ^ other.0) & 0x0F)
    }

    #[inline]
    pub const fn hamming_distance(self, other: Self) -> u8 {
        (self.0 ^ other.0).count_ones() as u8
    }

    #[inline]
    pub const fn is_hamming1(self, other: Self) -> bool {
        self.hamming_distance(other) == 1
    }

    #[inline]
    pub const fn neighbors(self) -> [Q4; 4] {
        [
            Q4(self.0 ^ 1),
            Q4(self.0 ^ 2),
            Q4(self.0 ^ 4),
            Q4(self.0 ^ 8),
        ]
    }

    pub fn flip_axis(self, axis: u8) -> CoreResult<Q4> {
        if axis < 4 {
            Ok(Q4(self.0 ^ (1u8 << axis)))
        } else {
            Err(CoreError::DomainError(format!(
                "Q4 axis index must be in 0..4, got {axis}"
            )))
        }
    }

    pub fn render(self) -> String {
        format!("q4({})", self.0)
    }

    pub fn render_v1(self) -> String {
        self.render()
    }
}

impl fmt::Display for Q4 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "q4({})", self.0)
    }
}

/// Retained octal fractal address (`Addr8`), preserving leading zeros so `00_8 != 0_8`.
///
/// Parses `00_8`, `00|-`, and `0o00`; rejects any digit `8` or `9` with `InvalidOctalAnchor`.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Addr8 {
    digits: Vec<u8>,
}

impl Addr8 {
    pub fn from_digits(digits: Vec<u8>) -> CoreResult<Self> {
        if digits.is_empty() {
            return Err(CoreError::InvalidOctalAnchor(
                "octal anchor must have at least one digit".to_string(),
            ));
        }
        for &d in &digits {
            if d >= 8 {
                return Err(CoreError::InvalidOctalAnchor(format!(
                    "non-octal digit {d} in Addr8"
                )));
            }
        }
        Ok(Self { digits })
    }

    /// Parse `00_8`, `00|-`, or `0o00` preserving exact leading-zero digits.
    pub fn parse(s: &str) -> CoreResult<Self> {
        let raw_digits = if let Some(prefix) = s.strip_suffix("_8") {
            prefix
        } else if let Some(prefix) = s.strip_suffix("|-") {
            prefix
        } else if let Some(rest) = s.strip_prefix("0o") {
            rest
        } else {
            return Err(CoreError::InvalidOctalAnchor(format!(
                "expected octal anchor suffix (_8 or |-) or 0o prefix, got {s:?}"
            )));
        }
        ;

        if raw_digits.is_empty() {
            return Err(CoreError::InvalidOctalAnchor(format!(
                "missing octal digits in {s:?}"
            )));
        }

        let mut digits = Vec::with_capacity(raw_digits.len());
        for b in raw_digits.bytes() {
            match b {
                b'0'..=b'7' => digits.push(b - b'0'),
                _ => {
                    return Err(CoreError::InvalidOctalAnchor(format!(
                        "invalid octal digit {:?} in {s:?}",
                        b as char
                    )));
                }
            }
        }
        Ok(Self { digits })
    }

    #[inline]
    pub fn digits(&self) -> &[u8] {
        &self.digits
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.digits.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.digits.is_empty()
    }

    /// First anchor check (`eq:first-anchor`): exact retained primitive `00_8`.
    #[inline]
    pub fn is_first_anchor(&self) -> bool {
        self.digits.as_slice() == [0, 0]
    }

    pub fn to_bigint(&self) -> BigInt {
        let mut acc = BigInt::zero();
        let eight = BigInt::from_u64(8);
        for &d in &self.digits {
            acc = &(&acc * &eight) + &BigInt::from_u64(d as u64);
        }
        acc
    }

    pub fn render(&self) -> String {
        let mut out = String::with_capacity(self.digits.len() + 2);
        for &d in &self.digits {
            out.push((b'0' + d) as char);
        }
        out.push_str("_8");
        out
    }
}

impl fmt::Display for Addr8 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render())
    }
}

/// Canonical ASCII escaping for `bytes` values (`b"..."`).
pub fn render_bytes_literal(bs: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bs.len() + 3);
    out.push_str("b\"");
    for &b in bs {
        match b {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0 => out.push_str("\\0"),
            0x20..=0x7E => out.push(b as char),
            other => {
                let _ = write!(&mut out, "\\x{other:02x}");
            }
        }
    }
    out.push('"');
    out
}
