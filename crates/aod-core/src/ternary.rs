use std::fmt;

use crate::bigint::BigInt;
use crate::error::{CoreError, CoreResult};

/// Balanced single trit in `{-1, 0, +1}`.
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
    pub fn value(self) -> i8 {
        self.0
    }

    pub fn checked_neg(self) -> Self {
        Self(-self.0)
    }

    pub fn checked_add(self, rhs: Self) -> CoreResult<Self> {
        Self::new(self.0 + rhs.0)
    }

    pub fn checked_sub(self, rhs: Self) -> CoreResult<Self> {
        Self::new(self.0 - rhs.0)
    }

    pub fn checked_mul(self, rhs: Self) -> CoreResult<Self> {
        Self::new(self.0 * rhs.0)
    }

    pub fn render_v1(self) -> String {
        format!("t1({})", self.0)
    }
}

impl fmt::Display for T1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "t1({})", self.0)
    }
}

/// Checked 32-trit balanced ternary integer in `[-926_510_094_425_920, +926_510_094_425_920]`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct T32(i128);

impl T32 {
    /// `(3^32 - 1) / 2 = 926_510_094_425_920`.
    pub const LIMIT: i128 = 926_510_094_425_920;

    pub fn new(v: i128) -> CoreResult<Self> {
        if (-Self::LIMIT..=Self::LIMIT).contains(&v) {
            Ok(Self(v))
        } else {
            Err(CoreError::Overflow(format!("t32 overflow: {v}")))
        }
    }

    pub fn from_bigint(v: &BigInt) -> CoreResult<Self> {
        let n = v
            .to_i128()
            .ok_or_else(|| CoreError::Overflow(format!("t32 overflow: {v}")))?;
        Self::new(n)
    }

    #[inline]
    pub fn value(self) -> i128 {
        self.0
    }

    pub fn checked_neg(self) -> CoreResult<Self> {
        Self::new(-self.0)
    }

    pub fn checked_add(self, rhs: Self) -> CoreResult<Self> {
        let sum = self
            .0
            .checked_add(rhs.0)
            .ok_or_else(|| CoreError::Overflow("t32 addition overflow".to_string()))?;
        Self::new(sum)
    }

    pub fn checked_sub(self, rhs: Self) -> CoreResult<Self> {
        let diff = self
            .0
            .checked_sub(rhs.0)
            .ok_or_else(|| CoreError::Overflow("t32 subtraction overflow".to_string()))?;
        Self::new(diff)
    }

    pub fn checked_mul(self, rhs: Self) -> CoreResult<Self> {
        let prod = self
            .0
            .checked_mul(rhs.0)
            .ok_or_else(|| CoreError::Overflow("t32 multiplication overflow".to_string()))?;
        Self::new(prod)
    }

    /// Floor quotient matching Python / Dec v1 `//`.
    pub fn checked_div_floor(self, rhs: Self) -> CoreResult<Self> {
        if rhs.0 == 0 {
            return Err(CoreError::ZeroDivision);
        }
        let mut q = self.0 / rhs.0;
        let r = self.0 % rhs.0;
        if r != 0 && ((r < 0) != (rhs.0 < 0)) {
            q -= 1;
        }
        Self::new(q)
    }

    /// Floor remainder matching Python / Dec v1 `%`.
    pub fn checked_rem_floor(self, rhs: Self) -> CoreResult<Self> {
        if rhs.0 == 0 {
            return Err(CoreError::ZeroDivision);
        }
        let mut r = self.0 % rhs.0;
        if r != 0 && ((r < 0) != (rhs.0 < 0)) {
            r += rhs.0;
        }
        Self::new(r)
    }

    /// Pack 32 balanced trits into a 64-bit word (`00 = 0`, `01 = +1`, `10 = -1`; `11` unused).
    pub fn pack(self) -> u64 {
        let mut n = self.0;
        let mut result = 0u64;
        for lane in 0..32u32 {
            let rem = n.rem_euclid(3);
            let digit: i128 = if rem == 2 { -1 } else { rem };
            n = (n - digit) / 3;
            let code: u64 = match digit {
                0 => 0,
                1 => 1,
                -1 => 2,
                _ => unreachable!(),
            };
            result |= code << (2 * lane);
        }
        debug_assert_eq!(n, 0);
        result
    }

    /// Unpack a 64-bit 2-bit-per-trit word, rejecting any `0b11` lane with `InvalidTritEncoding(3)`.
    pub fn unpack(bits: u64) -> CoreResult<Self> {
        let mut n = 0i128;
        let mut pow3 = 1i128;
        for lane in 0..32u32 {
            let code = ((bits >> (2 * lane)) & 3) as u8;
            let digit: i128 = match code {
                0 => 0,
                1 => 1,
                2 => -1,
                3 => return Err(CoreError::InvalidTritEncoding(3)),
                _ => unreachable!(),
            };
            n += digit * pow3;
            if lane < 31 {
                pow3 *= 3;
            }
        }
        Self::new(n)
    }

    /// Unpack a `BigInt` packed word, rejecting `< 0`, `>= 2^64`, or any `0b11` lane.
    pub fn unpack_bigint(bits: &BigInt) -> CoreResult<Self> {
        let raw = bits
            .to_u64()
            .ok_or_else(|| CoreError::DomainError(format!("packed t32 outside u64: {bits}")))?;
        Self::unpack(raw)
    }

    pub fn render_v1(self) -> String {
        format!("t32({})", self.0)
    }
}

impl fmt::Display for T32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "t32({})", self.0)
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
    pub fn value(self) -> u8 {
        self.0
    }

    #[inline]
    pub fn hamming_distance(self, other: Self) -> u8 {
        (self.0 ^ other.0).count_ones() as u8
    }

    #[inline]
    pub fn is_hamming1(self, other: Self) -> bool {
        self.hamming_distance(other) == 1
    }

    #[inline]
    pub fn neighbors(self) -> [Q4; 4] {
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

    pub fn render_v1(self) -> String {
        format!("q4({})", self.0)
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
        };

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
