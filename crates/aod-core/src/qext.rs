use std::fmt;

use crate::bigint::BigInt;
use crate::error::{CoreError, CoreResult};
use crate::ratio::Ratio;

/// One-way quarantined external exact rational (`Q_ext` boundary, `P0-1`).
///
/// Native frozen `Ratio` values may enter `QExtRatio` via [`QExtRatio::from_native_frozen`],
/// and external decimal literals enter strictly via [`parse_decimal_exact`].
/// There is intentionally NO unwrap method from `QExtRatio` back to native `Ratio`.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct QExtRatio(Ratio);

impl QExtRatio {
    /// One-way lift from a frozen native `Ratio` into the `Q_ext` quarantine sink.
    #[inline]
    pub fn from_native_frozen(r: Ratio) -> Self {
        Self(r)
    }

    pub fn checked_add(&self, rhs: &Self) -> Self {
        Self(&self.0 + &rhs.0)
    }

    pub fn checked_sub(&self, rhs: &Self) -> Self {
        Self(&self.0 - &rhs.0)
    }

    pub fn checked_mul(&self, rhs: &Self) -> Self {
        Self(&self.0 * &rhs.0)
    }

    pub fn checked_div(&self, rhs: &Self) -> CoreResult<Self> {
        Ok(Self(self.0.checked_div(&rhs.0)?))
    }

    pub fn neg(&self) -> Self {
        Self(-&self.0)
    }

    pub fn abs(&self) -> Self {
        Self(self.0.abs())
    }

    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    pub fn render_exact(&self) -> String {
        self.0.render_v1()
    }

    /// Format a quarantined ratio as an exact terminating decimal string truncated/rounded
    /// only for display reporting inside the quarantine boundary.
    pub fn render_quarantined(&self) -> String {
        format!("Q_ext({})", self.0.render_v1())
    }
}

impl fmt::Display for QExtRatio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Q_ext({})", self.0)
    }
}

/// Strictly parse an external decimal string matching `^[+-]?[0-9]+(\.[0-9]+)?$`
/// directly into a quarantined [`QExtRatio`].
///
/// Rejects empty strings, whitespace, leading/trailing dots (`.5`, `5.`),
/// scientific notation (`1e3`, `1E-3`), underscores, `NaN`, and `Inf`.
pub fn parse_decimal_exact(s: &str) -> CoreResult<QExtRatio> {
    if s.is_empty() {
        return Err(CoreError::InvalidDecimalLiteral(
            "empty decimal string".to_string(),
        ));
    }
    let bytes = s.as_bytes();
    let (neg, body) = match bytes[0] {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    if body.is_empty() {
        return Err(CoreError::InvalidDecimalLiteral(format!(
            "missing digits in {s:?}"
        )));
    }
    let mut parts = body.split('.');
    let int_part = parts.next().unwrap_or("");
    let frac_part = parts.next();
    if parts.next().is_some() {
        return Err(CoreError::InvalidDecimalLiteral(format!(
            "multiple decimal points in {s:?}"
        )));
    }
    if int_part.is_empty() || !int_part.bytes().all(|b| b.is_ascii_digit()) {
        return Err(CoreError::InvalidDecimalLiteral(format!(
            "invalid integer part in {s:?}"
        )));
    }
    let frac_digits = match frac_part {
        None => "",
        Some(f) => {
            if f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit()) {
                return Err(CoreError::InvalidDecimalLiteral(format!(
                    "invalid fractional part in {s:?}"
                )));
            }
            f
        }
    };

    let mut combined = String::with_capacity(1 + int_part.len() + frac_digits.len());
    if neg {
        combined.push('-');
    }
    combined.push_str(int_part);
    combined.push_str(frac_digits);

    let num = BigInt::parse_decimal(&combined)?;
    let scale_u32 = u32::try_from(frac_digits.len()).map_err(|_| {
        CoreError::Overflow("decimal fractional length exceeds u32".to_string())
    })?;
    let den = BigInt::from_u64(10).pow(scale_u32);
    let ratio = Ratio::new(num, den)?;
    Ok(QExtRatio(ratio))
}
