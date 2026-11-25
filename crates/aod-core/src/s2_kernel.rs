//! Stage 2 (`S2`) shared exact mathematical kernel for `aod-core`.
//!
//! Strictly `std`-only, zero external crates, zero JSON dependencies, zero floating-point
//! types or literals, and `#![forbid(unsafe_code)]` (inherited from crate root).
//!
//! Implements all exact numeric parsers/formatters, Gaussian rationals `Q(i)`, cyclotomic
//! `Q[x]/(x^4 - x^2 + 1)` powers, and all 24 positive S2/S4 cross-language reference
//! operations bound to `rpoyau/aod` (`S2@3122c94`).

use std::collections::{BTreeMap, BTreeSet};

use crate::bigint::BigInt;
use crate::error::{CoreError, CoreResult};
use crate::ratio::Ratio;

/// Hard upper bound on fixed-decimal rounding places to prevent unbounded allocation (`P0-1`, `P1-2`).
pub const MAX_ROUNDING_DECIMALS: usize = 1024;

/// Hard upper bound on `|exp|` in scientific notation tokens to prevent unbounded allocation (`P1-2`).
pub const MAX_SCIENTIFIC_EXPONENT: i32 = 1024;

/// Hard upper bound on integer exponentiation in closure/Higgs operations.
const MAX_SMALL_POW_EXP: u32 = 256;

/// Hard upper bound on iterative step counts in mindfulness recurrence.
const MAX_RECURRENCE_STEPS: usize = 10_000;

/// Hard upper bound on phase modulus `beta_a` in 3D Brownian delta3 histograms.
const MAX_PHASE_MODULUS: usize = 10_000;

/// Hard upper bound on multi-tick cycle-shedding trace lengths (matches upstream S4 `MAX_STEPS_GUARD`).
pub const MAX_CYCLE_SHEDDING_TICKS: usize = 100_000;

// ============================================================================
// Exact Numeric Parsers, Canonical Encodings, & Banker's Rounding (`P1-2`)
// ============================================================================

/// Strictly parse a canonical decimal integer string matching `^(?:0|-?[1-9][0-9]*)$`.
///
/// Rejects empty strings, whitespace, leading `+`, `"-0"`, leading zeros (`"01"`, `"-01"`),
/// and any non-ASCII-digit characters.
pub fn parse_exact_int_str(token: &str) -> CoreResult<BigInt> {
    let bytes = token.as_bytes();
    if bytes.is_empty() {
        return Err(CoreError::DomainError(
            "canonical integer token must be non-empty".to_string(),
        ));
    }
    if token == "0" {
        return Ok(BigInt::zero());
    }
    let digits = if bytes[0] == b'-' {
        if bytes.len() < 2 {
            return Err(CoreError::DomainError(format!(
                "invalid canonical integer token: {token:?}"
            )));
        }
        &bytes[1..]
    } else {
        bytes
    };
    if digits[0] == b'0' || !digits.iter().all(|b| b.is_ascii_digit()) {
        return Err(CoreError::DomainError(format!(
            "not a canonical integer token: {token:?}"
        )));
    }
    BigInt::parse_decimal(token)
}

/// Losslessly parse a printed decimal or rational string into exact canonical [`Ratio`].
///
/// Strictly rejects empty strings, any whitespace, non-finite spellings (`NaN`, `Inf`,
/// `Infinity`), zero denominators, multiple decimal points, leading/trailing dots,
/// and excessive scientific exponents (`|exp| > 1024`).
pub fn parse_printed_rational(
    token: &str,
    allow_fraction_slash: bool,
    allow_exponent: bool,
) -> CoreResult<Ratio> {
    if token.is_empty() {
        return Err(CoreError::DomainError(
            "printed rational token must be non-empty".to_string(),
        ));
    }
    if token.chars().any(char::is_whitespace) {
        return Err(CoreError::DomainError(format!(
            "printed rational token must not contain whitespace: {token:?}"
        )));
    }

    if let Some((lhs, rhs)) = token.split_once('/') {
        if !allow_fraction_slash {
            return Err(CoreError::DomainError(format!(
                "fraction slash syntax forbidden for token: {token:?}"
            )));
        }
        if rhs.contains('/') || lhs.is_empty() || rhs.is_empty() {
            return Err(CoreError::DomainError(format!(
                "invalid rational token: {token:?}"
            )));
        }
        let lhs_bytes = lhs.as_bytes();
        let (neg, num_digits) = match lhs_bytes[0] {
            b'-' => (true, &lhs[1..]),
            b'+' => (false, &lhs[1..]),
            _ => (false, lhs),
        };
        if num_digits.is_empty()
            || !num_digits.bytes().all(|b| b.is_ascii_digit())
            || !rhs.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(CoreError::DomainError(format!(
                "invalid rational digits in token: {token:?}"
            )));
        }
        let den = BigInt::parse_decimal(rhs)?;
        if !den.is_positive() {
            return Err(CoreError::ZeroDivision);
        }
        let mut num = BigInt::parse_decimal(num_digits)?;
        if neg {
            num = -num;
        }
        return Ratio::new(num, den);
    }

    // Decimal (optionally with scientific exponent e/E)
    let bytes = token.as_bytes();
    let (neg, body) = match bytes[0] {
        b'-' => (true, &token[1..]),
        b'+' => (false, &token[1..]),
        _ => (false, token),
    };
    if body.is_empty() {
        return Err(CoreError::DomainError(format!(
            "missing digits in decimal token: {token:?}"
        )));
    }

    let (mantissa, exp_val): (&str, i32) = if let Some(e_pos) = body.find(['e', 'E']) {
        if !allow_exponent {
            return Err(CoreError::DomainError(format!(
                "scientific exponent forbidden in decimal token: {token:?}"
            )));
        }
        let exp_str = &body[e_pos + 1..];
        if exp_str.is_empty() {
            return Err(CoreError::DomainError(format!(
                "empty exponent in decimal token: {token:?}"
            )));
        }
        let exp_bytes = exp_str.as_bytes();
        let exp_digits = match exp_bytes[0] {
            b'+' | b'-' => &exp_str[1..],
            _ => exp_str,
        };
        if exp_digits.is_empty() || !exp_digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(CoreError::DomainError(format!(
                "invalid exponent in decimal token: {token:?}"
            )));
        }
        let parsed_exp = exp_str.parse::<i32>().map_err(|_| {
            CoreError::Overflow(format!("exponent out of range in decimal token: {token:?}"))
        })?;
        if parsed_exp.unsigned_abs() > MAX_SCIENTIFIC_EXPONENT as u32 {
            return Err(CoreError::Overflow(format!(
                "exponent {parsed_exp} exceeds safety bound +/-{MAX_SCIENTIFIC_EXPONENT}"
            )));
        }
        (&body[..e_pos], parsed_exp)
    } else {
        (body, 0)
    };

    let (int_part, frac_part): (&str, &str) = match mantissa.split_once('.') {
        None => (mantissa, ""),
        Some((ip, fp)) => {
            if fp.contains('.') || fp.is_empty() {
                return Err(CoreError::DomainError(format!(
                    "invalid decimal point in token: {token:?}"
                )));
            }
            (ip, fp)
        }
    };

    if int_part.is_empty()
        || !int_part.bytes().all(|b| b.is_ascii_digit())
        || !frac_part.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(CoreError::DomainError(format!(
            "invalid decimal token: {token:?}"
        )));
    }
    if frac_part.len() > MAX_ROUNDING_DECIMALS {
        return Err(CoreError::Overflow(format!(
            "fractional length {} exceeds safety limit {MAX_ROUNDING_DECIMALS}",
            frac_part.len()
        )));
    }

    let mut combined = String::with_capacity(1 + int_part.len() + frac_part.len());
    if neg {
        combined.push('-');
    }
    combined.push_str(int_part);
    combined.push_str(frac_part);
    let digits = BigInt::parse_decimal(&combined)?;

    let scale: i64 = (frac_part.len() as i64) - (exp_val as i64);
    let max_scale = (MAX_ROUNDING_DECIMALS as i64) + (MAX_SCIENTIFIC_EXPONENT as i64);
    if scale.abs() > max_scale {
        return Err(CoreError::Overflow(format!(
            "effective decimal scale {scale} exceeds safety limit {max_scale}"
        )));
    }

    let ten = BigInt::from_u64(10);
    if scale >= 0 {
        let den = ten.pow(scale as u32);
        Ratio::new(digits, den)
    } else {
        let factor = ten.pow((-scale) as u32);
        Ok(Ratio::from_int(&digits * &factor))
    }
}

/// Strictly parse a canonical reduced `[num_str, den_str]` pair into [`Ratio`].
///
/// Enforces canonical integer spelling on both entries, `den > 0`, and canonical
/// reducedness (rejecting e.g. `("2", "4")`, `("0", "2")`, `("-0", "1")`, `("+1", "2")`).
pub fn parse_canonical_rational_pair(num_s: &str, den_s: &str) -> CoreResult<Ratio> {
    let num = parse_exact_int_str(num_s)?;
    let den = parse_exact_int_str(den_s)?;
    if !den.is_positive() {
        return Err(CoreError::DomainError(format!(
            "canonical rational denominator must be positive, got {den_s:?}"
        )));
    }
    let q = Ratio::new(num, den)?;
    let (canon_num, canon_den) = canonical_rational_pair(&q);
    if canon_num != num_s || canon_den != den_s {
        return Err(CoreError::DomainError(format!(
            "rational pair ({num_s:?}, {den_s:?}) is not in canonical reduced form ({canon_num:?}, {canon_den:?})"
        )));
    }
    Ok(q)
}

/// Return canonical `(num_str, den_str)` with `den > 0`, reduced, and zero as `("0", "1")`.
pub fn canonical_rational_pair(q: &Ratio) -> (String, String) {
    (q.num().to_decimal_string(), q.den().to_decimal_string())
}

/// Return canonical exact rational string (`"n"` if denominator is 1, else `"n/d"`).
pub fn canonical_rational_str(q: &Ratio) -> String {
    if q.den() == &BigInt::one() {
        q.num().to_decimal_string()
    } else {
        format!("{}/{}", q.num(), q.den())
    }
}

/// Exact banker's rounding (round half to even) using pure integer arithmetic.
///
/// Rejects `decimals > MAX_ROUNDING_DECIMALS` (1024).
pub fn round_rational_half_even(value: &Ratio, decimals: usize) -> CoreResult<Ratio> {
    if decimals > MAX_ROUNDING_DECIMALS {
        return Err(CoreError::DomainError(format!(
            "rounding decimals {decimals} exceeds safety limit {MAX_ROUNDING_DECIMALS}"
        )));
    }
    let scale = BigInt::from_u64(10).pow(decimals as u32);
    let scaled = value * &Ratio::from_int(scale.clone());
    let num = scaled.num();
    let den = scaled.den();
    let neg = num.is_negative();
    let abs_num = num.abs();
    let (mut quot, rem) = abs_num.div_rem_trunc(den)?;
    let twice_rem = &rem + &rem;
    if &twice_rem > den || (&twice_rem == den && !quot.is_even()) {
        quot = &quot + &BigInt::one();
    }
    let signed_quot = if neg && !quot.is_zero() { -quot } else { quot };
    Ratio::new(signed_quot, scale)
}

/// Format an exact rational to a fixed-decimal string using exact half-to-even rounding.
///
/// Rejects `decimals > MAX_ROUNDING_DECIMALS` (1024) and NEVER emits `"-0.000000"` when a
/// small negative rational rounds to zero.
pub fn format_rational_fixed(value: &Ratio, decimals: usize) -> CoreResult<String> {
    let rounded = round_rational_half_even(value, decimals)?;
    let scale = BigInt::from_u64(10).pow(decimals as u32);
    let scaled_q = &rounded * &Ratio::from_int(scale.clone());
    let scaled_int = scaled_q.to_int_exact()?;
    // Because BigInt::zero().is_negative() is always false, rounded zero never gets a '-' sign.
    let neg = scaled_int.is_negative();
    let mag = scaled_int.abs();
    if decimals == 0 {
        return Ok(if neg {
            format!("-{}", mag.to_decimal_string())
        } else {
            mag.to_decimal_string()
        });
    }
    let (int_part, frac_part) = mag.div_rem_trunc(&scale)?;
    let frac_raw = frac_part.to_decimal_string();
    let pad = decimals.saturating_sub(frac_raw.len());
    let mut out = String::with_capacity(
        usize::from(neg) + int_part.to_decimal_string().len() + 1 + decimals,
    );
    if neg {
        out.push('-');
    }
    out.push_str(&int_part.to_decimal_string());
    out.push('.');
    for _ in 0..pad {
        out.push('0');
    }
    out.push_str(&frac_raw);
    Ok(out)
}

// ============================================================================
// Gaussian Rationals Q(i) & Cyclotomic Q[x]/(x^4 - x^2 + 1) (`SD-001..003`)
// ============================================================================

/// Exact element of the Gaussian rational field `Q(i) = { re + im * I }`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GaussianRatio {
    pub re: Ratio,
    pub im: Ratio,
}

impl GaussianRatio {
    pub fn zero() -> Self {
        Self {
            re: Ratio::zero(),
            im: Ratio::zero(),
        }
    }

    pub fn from_ratio(re: Ratio) -> Self {
        Self {
            re,
            im: Ratio::zero(),
        }
    }

    pub fn new(re: Ratio, im: Ratio) -> Self {
        Self { re, im }
    }

    pub fn add(&self, rhs: &Self) -> Self {
        Self {
            re: &self.re + &rhs.re,
            im: &self.im + &rhs.im,
        }
    }

    pub fn mul(&self, rhs: &Self) -> Self {
        Self {
            re: &(&self.re * &rhs.re) - &(&self.im * &rhs.im),
            im: &(&self.re * &rhs.im) + &(&self.im * &rhs.re),
        }
    }

    pub fn div_by_int(&self, divisor: i64) -> CoreResult<Self> {
        let d = Ratio::from_i64(divisor, 1)?;
        Ok(Self {
            re: self.re.checked_div(&d)?,
            im: self.im.checked_div(&d)?,
        })
    }

    pub fn norm_sq(&self) -> Ratio {
        &(&self.re * &self.re) + &(&self.im * &self.im)
    }

    /// Format identically to SymPy `str(re + im * I)` across all `Q(i)` values (`P1-3`).
    ///
    /// Examples: `"0"`, `"1/2"`, `"-I/2"`, `"I"`, `"-I"`, `"3*I"`, `"3*I/2"`,
    /// `"1/4 - I/2"`, `"-1/2 + 3*I/4"`.
    pub fn format_sympy(&self) -> String {
        fn format_pos_im(im_abs: &Ratio) -> String {
            let n = im_abs.num();
            let d = im_abs.den();
            let one = BigInt::one();
            if n == &one && d == &one {
                "I".to_string()
            } else if n == &one {
                format!("I/{d}")
            } else if d == &one {
                format!("{n}*I")
            } else {
                format!("{n}*I/{d}")
            }
        }

        if self.re.is_zero() && self.im.is_zero() {
            return "0".to_string();
        }
        if self.im.is_zero() {
            return canonical_rational_str(&self.re);
        }
        if self.re.is_zero() {
            return if self.im.is_negative() {
                format!("-{}", format_pos_im(&self.im.abs()))
            } else {
                format_pos_im(&self.im)
            };
        }
        let re_s = canonical_rational_str(&self.re);
        if self.im.is_negative() {
            format!("{re_s} - {}", format_pos_im(&self.im.abs()))
        } else {
            format!("{re_s} + {}", format_pos_im(&self.im))
        }
    }
}

/// Exact power of `i = exp(2*pi*i/4)` for any integer exponent `k`.
fn unit_i_power(exp: i64) -> GaussianRatio {
    match exp.rem_euclid(4) {
        0 => GaussianRatio::new(Ratio::one(), Ratio::zero()),
        1 => GaussianRatio::new(Ratio::zero(), Ratio::one()),
        2 => GaussianRatio::new(-Ratio::one(), Ratio::zero()),
        _ => GaussianRatio::new(Ratio::zero(), -Ratio::one()),
    }
}

/// Exact power of `zeta_12` in `Q[x] / (x^4 - x^2 + 1)` using pure `Ratio` arithmetic.
pub fn zeta12_power(exponent: &BigInt) -> [Ratio; 4] {
    let twelve = BigInt::from_i64(12);
    let deg = exponent
        .rem_floor(&twelve)
        .expect("12 is nonzero")
        .to_u64()
        .expect("rem_floor mod 12 fits u64") as usize;
    let mut coeffs = vec![Ratio::zero(); deg.max(3) + 1];
    coeffs[deg] = Ratio::one();
    for power in (4..coeffs.len()).rev() {
        let lead = coeffs[power].clone();
        coeffs[power] = Ratio::zero();
        coeffs[power - 2] = &coeffs[power - 2] + &lead;
        coeffs[power - 4] = &coeffs[power - 4] - &lead;
    }
    [
        coeffs[0].clone(),
        coeffs[1].clone(),
        coeffs[2].clone(),
        coeffs[3].clone(),
    ]
}

// ============================================================================
// Helper Primitives
// ============================================================================

fn sign3(x: &BigInt) -> i64 {
    i64::from(x.signum())
}

fn int_balance(n: usize, k: usize) -> CoreResult<Vec<BigInt>> {
    if k == 0 {
        return Err(CoreError::ZeroDivision);
    }
    let base = (n / k) as u64;
    let rem = n % k;
    Ok((0..k)
        .map(|idx| BigInt::from_u64(base + u64::from(idx < rem)))
        .collect())
}

fn checked_pow_u32(base: &BigInt, exp: &BigInt, label: &str) -> CoreResult<BigInt> {
    let e = exp
        .to_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| CoreError::DomainError(format!("{label} exponent must be non-negative: {exp}")))?;
    if e > MAX_SMALL_POW_EXP {
        return Err(CoreError::Overflow(format!(
            "{label} exponent {e} exceeds safety bound {MAX_SMALL_POW_EXP}"
        )));
    }
    Ok(base.pow(e))
}

// ============================================================================
// 1..20: Rational Core Operations (`RC-001`..`RC-020`, `RC-024`)
// ============================================================================

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecOneNodeRow {
    pub admissible: BigInt,
    pub weight: BigInt,
    pub state: i64,
    pub neighbor: [i64; 4],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecOneNodeOutput {
    pub z_total: BigInt,
    pub probabilities: Vec<Ratio>,
    pub sum: Ratio,
    pub routes: Vec<&'static str>,
}

/// `RC-001` (`dec_normalize_one_node`).
pub fn dec_normalize_one_node(rows: &[DecOneNodeRow]) -> CoreResult<DecOneNodeOutput> {
    if rows.len() != 4 {
        return Err(CoreError::DomainError(format!(
            "dec_normalize_one_node expects 4 rows, got {}",
            rows.len()
        )));
    }
    let mut aw_list = Vec::with_capacity(4);
    let mut routes = Vec::with_capacity(4);
    let mut seen_neighbors: BTreeSet<[i64; 4]> = BTreeSet::new();
    for row in rows {
        if row.admissible.is_negative() || row.weight.is_negative() {
            return Err(CoreError::DomainError(
                "admissible and weight must be non-negative".to_string(),
            ));
        }
        let valid_basis = row.neighbor.iter().all(|&c| c == 0 || c == 1)
            && row.neighbor.iter().sum::<i64>() == 1;
        if !valid_basis {
            return Err(CoreError::DomainError(
                "neighbor must be a Q4 basis vector ({0,1} with Hamming weight 1)".to_string(),
            ));
        }
        seen_neighbors.insert(row.neighbor);
        let route = match row.state {
            1 => "outgoing",
            -1 => "returned",
            0 => "hinge",
            other => {
                return Err(CoreError::DomainError(format!(
                    "invalid state {other}; expected -1, 0, or 1"
                )));
            }
        };
        aw_list.push(&row.admissible * &row.weight);
        routes.push(route);
    }
    if seen_neighbors.len() != 4 {
        return Err(CoreError::DomainError(
            "dec_normalize_one_node requires 4 distinct Q4 neighbors".to_string(),
        ));
    }
    let z_total = aw_list.iter().fold(BigInt::zero(), |acc, x| &acc + x);
    if !z_total.is_positive() {
        return Err(CoreError::ZeroDivision);
    }
    let mut probabilities = Vec::with_capacity(4);
    let mut sum = Ratio::zero();
    for aw in aw_list {
        let p = Ratio::new(aw, z_total.clone())?;
        sum = &sum + &p;
        probabilities.push(p);
    }
    Ok(DecOneNodeOutput {
        z_total,
        probabilities,
        sum,
        routes,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HingeSlideBranch {
    pub weight: Ratio,
    pub route: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HingeSlideOutput {
    pub z_total: Ratio,
    pub probabilities: Vec<Ratio>,
    pub sum_returned: Ratio,
    pub sum_outgoing: Ratio,
}

/// `RC-002` (`dec_hinge_slide_normalize`).
pub fn dec_hinge_slide_normalize(branches: &[HingeSlideBranch]) -> CoreResult<HingeSlideOutput> {
    if branches.is_empty() {
        return Err(CoreError::DomainError(
            "dec_hinge_slide_normalize requires non-empty branches".to_string(),
        ));
    }
    let mut z_total = Ratio::zero();
    for b in branches {
        if b.weight.is_negative() {
            return Err(CoreError::DomainError(
                "branch weight must be non-negative".to_string(),
            ));
        }
        z_total = &z_total + &b.weight;
    }
    if !z_total.is_positive() {
        return Err(CoreError::ZeroDivision);
    }
    let mut probabilities = Vec::with_capacity(branches.len());
    let mut sum_returned = Ratio::zero();
    let mut sum_outgoing = Ratio::zero();
    for b in branches {
        let p = b.weight.checked_div(&z_total)?;
        if b.route == "returned" {
            sum_returned = &sum_returned + &p;
        } else if b.route == "outgoing" {
            sum_outgoing = &sum_outgoing + &p;
        }
        probabilities.push(p);
    }
    Ok(HingeSlideOutput {
        z_total,
        probabilities,
        sum_returned,
        sum_outgoing,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnellingWindowClipOutput {
    pub p_val: Ratio,
    pub rho_d_omega: BigInt,
    pub p_d: BigInt,
    pub tunnel_contribution: Ratio,
}

/// `RC-003` (`dec_field_tunnelling_window_clip`).
pub fn dec_field_tunnelling_window_clip(
    direct_adm: &BigInt,
    slide_weight: &Ratio,
    z_val: &Ratio,
    rd_val: &BigInt,
    omega: &BigInt,
    c3: &BigInt,
) -> CoreResult<TunnellingWindowClipOutput> {
    if !direct_adm.is_zero() {
        return Err(CoreError::DomainError(
            "direct_adm must be 0 for hinge-slide tunnelling".to_string(),
        ));
    }
    if !z_val.is_positive() {
        return Err(CoreError::ZeroDivision);
    }
    let p_val = slide_weight.checked_div(z_val)?;
    let rho_d_omega = if rd_val <= omega {
        rd_val.clone()
    } else {
        omega.clone()
    };
    let p_d = c3 * &rho_d_omega;
    let tunnel_contribution = &p_val * &Ratio::from_int(p_d.clone());
    Ok(TunnellingWindowClipOutput {
        p_val,
        rho_d_omega,
        p_d,
        tunnel_contribution,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SadarFluxOutput {
    pub flux: Vec<Vec<Ratio>>,
    pub antisymmetric: bool,
}

/// `RC-004` (`sadar_afc_flux_antisymmetry`).
///
/// Enforces non-empty square stochastic matrix and unit-sum memory (`P1-3`).
pub fn sadar_afc_flux_antisymmetry(
    memory: &[Ratio],
    matrix: &[Vec<Ratio>],
) -> CoreResult<SadarFluxOutput> {
    let n = memory.len();
    if n == 0 {
        return Err(CoreError::DomainError(
            "memory distribution must be non-empty".to_string(),
        ));
    }
    if matrix.len() != n || matrix.iter().any(|row| row.len() != n) {
        return Err(CoreError::DomainError(format!(
            "transition matrix must be square {n}x{n}"
        )));
    }
    let mem_sum = memory.iter().fold(Ratio::zero(), |acc, x| &acc + x);
    if mem_sum != Ratio::one() {
        return Err(CoreError::DomainError(format!(
            "memory distribution not normalized (sum={mem_sum})"
        )));
    }
    for (i, row) in matrix.iter().enumerate() {
        let row_sum = row.iter().fold(Ratio::zero(), |acc, x| &acc + x);
        if row_sum != Ratio::one() {
            return Err(CoreError::DomainError(format!(
                "matrix row {i} not stochastic (sum={row_sum})"
            )));
        }
    }
    let mut flux = vec![vec![Ratio::zero(); n]; n];
    for i in 0..n {
        for j in 0..n {
            flux[i][j] = &(&memory[i] * &matrix[i][j]) - &(&memory[j] * &matrix[j][i]);
        }
    }
    let mut antisymmetric = true;
    for i in 0..n {
        for j in 0..n {
            if flux[i][j] != -&flux[j][i] {
                antisymmetric = false;
            }
        }
    }
    Ok(SadarFluxOutput {
        flux,
        antisymmetric,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Q4HammingOutput {
    pub hamming_distance: usize,
    pub is_h1_edge: bool,
    pub neighbors: Vec<BigInt>,
}

/// `RC-005` (`q4_hamming_and_neighbors`).
pub fn q4_hamming_and_neighbors(
    vertex: &BigInt,
    target: &BigInt,
    width: &BigInt,
) -> CoreResult<Q4HammingOutput> {
    let w = width
        .to_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| CoreError::DomainError(format!("invalid hypercube width: {width}")))?;
    if w == 0 || w > MAX_SMALL_POW_EXP {
        return Err(CoreError::DomainError(format!(
            "hypercube width {w} out of range 1..={MAX_SMALL_POW_EXP}"
        )));
    }
    let bound = BigInt::one().shl(w)?;
    if vertex.is_negative() || vertex >= &bound || target.is_negative() || target >= &bound {
        return Err(CoreError::DomainError(format!(
            "vertex/target out of Q_{w} range [0, {bound})"
        )));
    }
    let diff = vertex.bitxor(target)?;
    let one = BigInt::one();
    let mut hamming_distance = 0usize;
    let mut neighbors = Vec::with_capacity(w as usize);
    for slot in 0..w {
        let bit = diff.shr(slot)?.bitand(&one)?;
        if bit == one {
            hamming_distance += 1;
        }
        let mask = one.shl(slot)?;
        neighbors.push(vertex.bitxor(&mask)?);
    }
    Ok(Q4HammingOutput {
        hamming_distance,
        is_h1_edge: hamming_distance == 1,
        neighbors,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClosureWaveRdRcdOutput {
    pub pi_val: BigInt,
    pub rd_val: BigInt,
    pub rcd_duration: BigInt,
    pub participation: BigInt,
}

/// `RC-006` (`closure_wave_rd_rcd`).
pub fn closure_wave_rd_rcd(
    p: &BigInt,
    q: &BigInt,
    hinges: &BigInt,
    reflection_counts: &[BigInt],
    window: &BigInt,
) -> CoreResult<ClosureWaveRdRcdOutput> {
    let q_pow_p = checked_pow_u32(q, p, "closure_wave_rd_rcd q^p")?;
    let pi_val = &q_pow_p + q;
    let rd_val = &(&BigInt::from_i64(2) * &pi_val) + hinges;
    let refl_sum = reflection_counts
        .iter()
        .fold(BigInt::zero(), |acc, x| &acc + x);
    let rcd_duration = &rd_val + &refl_sum;
    let participation = if &rcd_duration <= window {
        rcd_duration.clone()
    } else {
        window.clone()
    };
    Ok(ClosureWaveRdRcdOutput {
        pi_val,
        rd_val,
        rcd_duration,
        participation,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectionalCadence {
    pub ratio: Ratio,
    pub bias_diff: Ratio,
    pub lag: Ratio,
    pub drift: Ratio,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationalCadenceOutput {
    pub forward: DirectionalCadence,
    pub reverse: DirectionalCadence,
}

/// `RC-007` (`relational_cadence_and_lag`).
pub fn relational_cadence_and_lag(
    na: &BigInt,
    nb: &BigInt,
    bias0: &Ratio,
    bias1: &Ratio,
    prior_lag: &Ratio,
) -> CoreResult<RelationalCadenceOutput> {
    if na.is_zero() || nb.is_zero() {
        return Err(CoreError::ZeroDivision);
    }
    let bias_diff = bias0 - bias1;
    let fwd_ratio = Ratio::new(na.clone(), nb.clone())?;
    let fwd_lag = &Ratio::from_int(na - nb) + &bias_diff;
    let fwd_drift = &fwd_lag - prior_lag;

    let rev_ratio = Ratio::new(nb.clone(), na.clone())?;
    let rev_bias = -&bias_diff;
    let rev_lag = -&fwd_lag;
    let rev_drift = &rev_lag - &(-prior_lag);

    Ok(RelationalCadenceOutput {
        forward: DirectionalCadence {
            ratio: fwd_ratio,
            bias_diff,
            lag: fwd_lag,
            drift: fwd_drift,
        },
        reverse: DirectionalCadence {
            ratio: rev_ratio,
            bias_diff: rev_bias,
            lag: rev_lag,
            drift: rev_drift,
        },
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrientedChannel {
    pub i: BigInt,
    pub j: BigInt,
    pub p: Ratio,
    pub a: Ratio,
    pub lineage: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FiniteStokesOutput {
    pub internal: Ratio,
    pub boundary: Ratio,
    pub divergence: Ratio,
    pub divergence_equals_boundary: bool,
}

/// `RC-008` (`finite_stokes_cut_balance`).
pub fn finite_stokes_cut_balance(
    channels: &[OrientedChannel],
    region: &[BigInt],
) -> CoreResult<FiniteStokesOutput> {
    let region_set: BTreeSet<BigInt> = region.iter().cloned().collect();
    let mut ledger: BTreeMap<(BigInt, BigInt, String), Ratio> = BTreeMap::new();
    for ch in channels {
        if ch.i == ch.j || ch.p.is_negative() || ch.lineage.is_empty() {
            return Err(CoreError::DomainError(
                "invalid oriented channel in finite_stokes_cut_balance".to_string(),
            ));
        }
        let val = &ch.p * &ch.a;
        ledger.insert((ch.i.clone(), ch.j.clone(), ch.lineage.clone()), val.clone());
        ledger.insert((ch.j.clone(), ch.i.clone(), ch.lineage.clone()), -val);
    }
    let mut internal = Ratio::zero();
    let mut divergence = Ratio::zero();
    let mut boundary = Ratio::zero();
    for ((i, j, _), v) in &ledger {
        let i_in = region_set.contains(i);
        let j_in = region_set.contains(j);
        if i_in && j_in {
            internal = &internal + v;
        }
        if i_in {
            divergence = &divergence + v;
        }
        if i_in && !j_in {
            boundary = &boundary + v;
        }
    }
    let divergence_equals_boundary = divergence == boundary && internal.is_zero();
    Ok(FiniteStokesOutput {
        internal,
        boundary,
        divergence,
        divergence_equals_boundary,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AwarenessCurrentOutput {
    pub carried: Ratio,
    pub native_current: Ratio,
    pub next_current: Ratio,
    pub selected_slot: i64,
    pub removal_current: Ratio,
    pub awareness_effect: bool,
}

/// `RC-009` (`awareness_current_update`).
#[allow(clippy::too_many_arguments)]
pub fn awareness_current_update(
    current: &Ratio,
    pressure: &Ratio,
    attention: &Ratio,
    query: &Ratio,
    gain: &Ratio,
    available_retained_values: &[Ratio],
    selector_threshold: &Ratio,
) -> CoreResult<AwarenessCurrentOutput> {
    if !pressure.is_positive() {
        return Err(CoreError::DomainError(
            "positive pressure required in awareness_current_update".to_string(),
        ));
    }
    let ret_sum = available_retained_values
        .iter()
        .fold(Ratio::zero(), |acc, x| &acc + x);
    let carried = gain * &ret_sum;
    let native_current = current + &attention.checked_div(pressure)?;
    let next_current = &native_current + &(query + &carried).checked_div(pressure)?;
    let removal_current = &native_current + &query.checked_div(pressure)?;
    let selected_slot = if &next_current > selector_threshold {
        0
    } else {
        1
    };
    let awareness_effect = next_current != removal_current;
    Ok(AwarenessCurrentOutput {
        carried,
        native_current,
        next_current,
        selected_slot,
        removal_current,
        awareness_effect,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AwarenessMindfulnessOutput {
    pub no_delay_currents: Vec<Ratio>,
    pub no_delay_stable_at_one: bool,
    pub extra_delay_currents: Vec<Ratio>,
    pub extra_delay_stable_at_one: bool,
}

/// `RC-010` (`awareness_mindfulness_step`).
pub fn awareness_mindfulness_step(
    pressure: &Ratio,
    initial_current: &Ratio,
    constant_forcing: &Ratio,
    delay_initial_history: &Ratio,
    steps: usize,
) -> CoreResult<AwarenessMindfulnessOutput> {
    if !pressure.is_positive() {
        return Err(CoreError::DomainError(
            "positive pressure required in awareness_mindfulness_step".to_string(),
        ));
    }
    if steps > MAX_RECURRENCE_STEPS {
        return Err(CoreError::Overflow(format!(
            "steps {steps} exceeds safety bound {MAX_RECURRENCE_STEPS}"
        )));
    }
    let mut no_delay_currents = Vec::with_capacity(steps);
    let mut j_cur = initial_current.clone();
    for _ in 0..steps {
        let delta = (constant_forcing - &j_cur).checked_div(pressure)?;
        j_cur = &j_cur + &delta;
        no_delay_currents.push(j_cur.clone());
    }

    let mut extra_delay_currents = Vec::with_capacity(steps);
    let mut j_cur_d = initial_current.clone();
    let mut prev_obs = delay_initial_history.clone();
    for _ in 0..steps {
        let obs = j_cur_d.clone();
        let delta = (constant_forcing - &prev_obs).checked_div(pressure)?;
        j_cur_d = &j_cur_d + &delta;
        prev_obs = obs;
        extra_delay_currents.push(j_cur_d.clone());
    }

    let one = Ratio::one();
    let no_delay_stable_at_one = no_delay_currents.iter().all(|x| x == &one);
    let extra_delay_stable_at_one = extra_delay_currents.iter().all(|x| x == &one);
    Ok(AwarenessMindfulnessOutput {
        no_delay_currents,
        no_delay_stable_at_one,
        extra_delay_currents,
        extra_delay_stable_at_one,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HiggsCandidateInput {
    pub p: BigInt,
    pub q: BigInt,
    pub win_l: BigInt,
    pub c3: BigInt,
    pub ca: BigInt,
    pub cb: BigInt,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HiggsInternalSelection {
    pub scores: Vec<Ratio>,
    pub winner_index: usize,
    pub winner_pi: BigInt,
    pub winner_rd: BigInt,
    pub winner_pd: BigInt,
    pub winner_qd: BigInt,
    pub winner_l: BigInt,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HiggsTraceFreezeOutput {
    pub scores: Vec<Ratio>,
    pub winner_index: usize,
    pub winner_pi: BigInt,
    pub winner_rd: BigInt,
    pub winner_pd: BigInt,
    pub winner_qd: BigInt,
    pub e_h_0_mev: BigInt,
    pub e_h_0_gev: Ratio,
}

/// Phase 1 of `RC-011` (`P0-2`): purely internal dimensionless candidate scoring and freeze.
/// Zero `MeV`/`GeV` constants and zero external experimental targets touch winner selection.
pub fn higgs_internal_score_and_select(
    candidates: &[HiggsCandidateInput],
) -> CoreResult<HiggsInternalSelection> {
    if candidates.is_empty() {
        return Err(CoreError::DomainError(
            "higgs_trace_score_and_freeze requires at least one candidate".to_string(),
        ));
    }
    let mut scores = Vec::with_capacity(candidates.len());
    let mut details = Vec::with_capacity(candidates.len());
    let two = BigInt::from_i64(2);
    let three = BigInt::from_i64(3);
    let seven = BigInt::from_i64(7);
    let hundred = BigInt::from_i64(100);
    let two_fifty = BigInt::from_i64(250);
    let twelve = BigInt::from_i64(12);

    for c in candidates {
        let q_pow_p = checked_pow_u32(&c.q, &c.p, "higgs q^p")?;
        let pi_v = &q_pow_p + &c.q;
        let rd_v = &(&two * &pi_v) + &BigInt::one();
        let rho = if rd_v <= c.win_l {
            rd_v.clone()
        } else {
            c.win_l.clone()
        };
        let pd_v = &c.c3 * &rho;
        let rem_rd = &rd_v - &c.win_l;
        let qd_v = if rem_rd.is_positive() {
            &c.c3 * &rem_rd
        } else {
            BigInt::zero()
        };
        let dl = &pd_v - &c.ca;
        let du = &pd_v - &c.cb;
        let d_sad = &(&dl - &three).abs() + &(&du + &three).abs();
        let in_qd_band = qd_v >= hundred && qd_v <= two_fifty;
        let d_q = if in_qd_band {
            BigInt::zero()
        } else if qd_v < hundred {
            &hundred - &qd_v
        } else {
            &qd_v - &two_fifty
        };
        let d_route = if c.win_l == seven && dl == three && du == -&three && in_qd_band {
            BigInt::zero()
        } else {
            BigInt::one()
        };
        let s_h = &(&(&Ratio::new(d_sad, twelve.clone())?
            + &Ratio::new(d_q, hundred.clone())?)
            + &Ratio::new((&c.win_l - &seven).abs(), two.clone())?)
            + &Ratio::from_int(d_route);
        scores.push(s_h);
        details.push((pi_v, rd_v, pd_v, qd_v, c.win_l.clone()));
    }

    let mut winner_index = 0usize;
    for i in 1..scores.len() {
        if scores[i] < scores[winner_index] {
            winner_index = i;
        }
    }
    let (winner_pi, winner_rd, winner_pd, winner_qd, winner_l) = details[winner_index].clone();
    Ok(HiggsInternalSelection {
        scores,
        winner_index,
        winner_pi,
        winner_rd,
        winner_pd,
        winner_qd,
        winner_l,
    })
}

/// Phase 2 of `RC-011` (`P0-2`): post-freeze external energy map strictly from the frozen winner.
pub fn higgs_freeze_energy_from_winner(
    frozen_rd: &BigInt,
    frozen_l: &BigInt,
) -> CoreResult<(BigInt, Ratio)> {
    let shift_exp = frozen_l + &BigInt::from_i64(4);
    let pow2 = checked_pow_u32(&BigInt::from_i64(2), &shift_exp, "higgs 2^(L+4)")?;
    let e_mev = frozen_rd * &pow2;
    let e_gev = Ratio::new(e_mev.clone(), BigInt::from_i64(1000))?;
    Ok((e_mev, e_gev))
}

/// `RC-011` (`higgs_trace_score_and_freeze`): strictly two-phase (freeze winner internally first,
/// then compute `E_H_0_MeV` / `E_H_0_GeV` strictly from the frozen winner only).
pub fn higgs_trace_score_and_freeze(
    candidates: &[HiggsCandidateInput],
) -> CoreResult<HiggsTraceFreezeOutput> {
    let frozen = higgs_internal_score_and_select(candidates)?;
    let (e_h_0_mev, e_h_0_gev) =
        higgs_freeze_energy_from_winner(&frozen.winner_rd, &frozen.winner_l)?;
    Ok(HiggsTraceFreezeOutput {
        scores: frozen.scores,
        winner_index: frozen.winner_index,
        winner_pi: frozen.winner_pi,
        winner_rd: frozen.winner_rd,
        winner_pd: frozen.winner_pd,
        winner_qd: frozen.winner_qd,
        e_h_0_mev,
        e_h_0_gev,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Brownian3dOutput {
    pub q_vals: Vec<BigInt>,
    pub octants_msb_x: Vec<i64>,
    pub phase_delta_z: Vec<BigInt>,
    pub octant_delta_z: Vec<BigInt>,
    pub sum_phase_m: BigInt,
    pub sum_octant_m: BigInt,
    pub r_q: Ratio,
    pub r_x: Ratio,
    pub r_y: Ratio,
    pub r_z: Ratio,
}

/// `RC-012` (`brownian_3d_delta3_msb_x`).
pub fn brownian_3d_delta3_msb_x(
    displacements: &[[BigInt; 3]],
    beta_a: usize,
) -> CoreResult<Brownian3dOutput> {
    if displacements.is_empty() {
        return Err(CoreError::DomainError(
            "displacements must be non-empty".to_string(),
        ));
    }
    if beta_a == 0 || beta_a > MAX_PHASE_MODULUS {
        return Err(CoreError::DomainError(format!(
            "beta_a {beta_a} out of valid range 1..={MAX_PHASE_MODULUS}"
        )));
    }
    let beta_bi = BigInt::from_u64(beta_a as u64);
    let mut q_vals = Vec::with_capacity(displacements.len());
    let mut octants_msb_x = Vec::with_capacity(displacements.len());
    let mut phase_cnt = vec![BigInt::zero(); beta_a];
    let mut oct_cnt = vec![BigInt::zero(); 8];
    let mut sx = BigInt::zero();
    let mut sy = BigInt::zero();
    let mut sz = BigInt::zero();

    for [a, b, c] in displacements {
        let a2 = a * a;
        let b2 = b * b;
        let c2 = c * c;
        let qv = &(&a2 + &b2) + &c2;
        let oc = (if a.is_negative() { 4 } else { 0 })
            + (if b.is_negative() { 2 } else { 0 })
            + (if c.is_negative() { 1 } else { 0 });
        let slot = qv
            .rem_floor(&beta_bi)?
            .to_u64()
            .ok_or_else(|| CoreError::Overflow("phase slot out of range".to_string()))?
            as usize;
        phase_cnt[slot] = &phase_cnt[slot] + &BigInt::one();
        oct_cnt[oc as usize] = &oct_cnt[oc as usize] + &BigInt::one();
        sx = &sx + &a2;
        sy = &sy + &b2;
        sz = &sz + &c2;
        q_vals.push(qv);
        octants_msb_x.push(oc);
    }

    let n = displacements.len();
    let p_comp = int_balance(n, beta_a)?;
    let o_comp = int_balance(n, 8)?;
    let phase_delta_z: Vec<BigInt> = p_comp
        .iter()
        .zip(&phase_cnt)
        .map(|(c, o)| c - o)
        .collect();
    let octant_delta_z: Vec<BigInt> = o_comp
        .iter()
        .zip(&oct_cnt)
        .map(|(c, o)| c - o)
        .collect();
    let sum_phase_m = phase_delta_z
        .iter()
        .fold(BigInt::zero(), |acc, x| &acc + &x.abs());
    let sum_octant_m = octant_delta_z
        .iter()
        .fold(BigInt::zero(), |acc, x| &acc + &x.abs());
    let sum_q = q_vals.iter().fold(BigInt::zero(), |acc, x| &acc + x);
    if !sum_q.is_positive() {
        return Err(CoreError::ZeroDivision);
    }
    let r_q = Ratio::new(sum_q.clone(), BigInt::from_u64(n as u64))?;
    let r_x = Ratio::new(sx, sum_q.clone())?;
    let r_y = Ratio::new(sy, sum_q.clone())?;
    let r_z = Ratio::new(sz, sum_q)?;

    Ok(Brownian3dOutput {
        q_vals,
        octants_msb_x,
        phase_delta_z,
        octant_delta_z,
        sum_phase_m,
        sum_octant_m,
        r_q,
        r_x,
        r_y,
        r_z,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunTumble3dOutput {
    pub octants_lsb_x: Vec<i64>,
    pub octants_msb_x_contrast: Vec<i64>,
    pub q3: Vec<BigInt>,
    pub theta: Vec<BigInt>,
}

/// `RC-013` (`run_tumble_3d_delta3_lsb_x`).
pub fn run_tumble_3d_delta3_lsb_x(
    z_vectors: &[[BigInt; 3]],
    beta_a: &BigInt,
) -> CoreResult<RunTumble3dOutput> {
    if !beta_a.is_positive() {
        return Err(CoreError::DomainError(
            "beta_a must be positive in run_tumble_3d_delta3_lsb_x".to_string(),
        ));
    }
    let mut octants_lsb_x = Vec::with_capacity(z_vectors.len());
    let mut octants_msb_x_contrast = Vec::with_capacity(z_vectors.len());
    let mut q3 = Vec::with_capacity(z_vectors.len());
    let mut theta = Vec::with_capacity(z_vectors.len());
    for [x, y, z] in z_vectors {
        let lsb = (if x.is_negative() { 1 } else { 0 })
            + (if y.is_negative() { 2 } else { 0 })
            + (if z.is_negative() { 4 } else { 0 });
        let msb = (if x.is_negative() { 4 } else { 0 })
            + (if y.is_negative() { 2 } else { 0 })
            + (if z.is_negative() { 1 } else { 0 });
        let qv = &(&(x * x) + &(y * y)) + &(z * z);
        let th = qv.rem_floor(beta_a)?;
        octants_lsb_x.push(lsb);
        octants_msb_x_contrast.push(msb);
        q3.push(qv);
        theta.push(th);
    }
    Ok(RunTumble3dOutput {
        octants_lsb_x,
        octants_msb_x_contrast,
        q3,
        theta,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CycleSheddingOutput {
    pub span: Ratio,
    pub before_clipping: Ratio,
    pub shed_excess: Ratio,
    pub padar_burden: Ratio,
    pub outward_shedding: Ratio,
    pub local_reclosure: Ratio,
    pub remainder_sum: Ratio,
}

/// `RC-014` (`cycle_shedding_step`).
pub fn cycle_shedding_step(
    prev_burden: &Ratio,
    lead: &Ratio,
    lag: &Ratio,
    compatibility: &Ratio,
    eta: &Ratio,
    gamma: &Ratio,
    lambda_reclose: &Ratio,
) -> CycleSheddingOutput {
    let span = (lead - lag).abs();
    let before_clipping = &(eta * prev_burden) + &(gamma * &span);
    let raw_excess = &before_clipping - compatibility;
    let shed_excess = if raw_excess.is_positive() {
        raw_excess
    } else {
        Ratio::zero()
    };
    let padar_burden = &before_clipping - &shed_excess;
    let outward_shedding = &(&Ratio::one() - lambda_reclose) * &shed_excess;
    let local_reclosure = lambda_reclose * &shed_excess;
    let remainder_sum = &outward_shedding + &local_reclosure;
    CycleSheddingOutput {
        span,
        before_clipping,
        shed_excess,
        padar_burden,
        outward_shedding,
        local_reclosure,
        remainder_sum,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CycleSheddingTickInput {
    pub lead: Ratio,
    pub lag: Ratio,
    pub compatibility: Ratio,
}

/// Exact multi-tick chained cycle-shedding trace (`S4@e013401`), enforcing `MAX_CYCLE_SHEDDING_TICKS`.
pub fn cycle_shedding_trace(
    initial_burden: &Ratio,
    ticks: &[CycleSheddingTickInput],
    eta: &Ratio,
    gamma: &Ratio,
    lambda_reclose: &Ratio,
) -> CoreResult<Vec<CycleSheddingOutput>> {
    if ticks.len() > MAX_CYCLE_SHEDDING_TICKS {
        return Err(CoreError::DomainError(format!(
            "cycle_shedding_trace ticks length {} exceeds guard {}",
            ticks.len(),
            MAX_CYCLE_SHEDDING_TICKS
        )));
    }
    let mut prev = initial_burden.clone();
    let mut out = Vec::with_capacity(ticks.len());
    for tick in ticks {
        let step = cycle_shedding_step(
            &prev,
            &tick.lead,
            &tick.lag,
            &tick.compatibility,
            eta,
            gamma,
            lambda_reclose,
        );
        prev = step.padar_burden.clone();
        out.push(step);
    }
    Ok(out)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SparcBinOutput {
    pub u_obs: Ratio,
    pub sigma_u: Ratio,
    pub u_gas: Ratio,
    pub u_disk: Ratio,
    pub u_bulge: Ratio,
    pub u_bar: Ratio,
    pub c_cap: Ratio,
    pub d_bf: Ratio,
    pub u_ao: Ratio,
    pub delta_u: Ratio,
    pub z_u: Ratio,
}

/// `RC-015` / `RC-016` (`sparc_exact_bin_eval`).
pub fn sparc_exact_bin_eval(
    r_tok: &str,
    v_obs_tok: &str,
    err_v_tok: &str,
    v_gas_tok: &str,
    v_disk_tok: &str,
    v_bul_tok: &str,
) -> CoreResult<SparcBinOutput> {
    let r_v = parse_printed_rational(r_tok, false, false)?;
    let v_obs = parse_printed_rational(v_obs_tok, false, false)?;
    let err_v = parse_printed_rational(err_v_tok, false, false)?;
    let v_gas = parse_printed_rational(v_gas_tok, false, false)?;
    let v_disk = parse_printed_rational(v_disk_tok, false, false)?;
    let v_bul = parse_printed_rational(v_bul_tok, false, false)?;

    let u_obs = &v_obs * &v_obs;
    let sigma_u = &(&Ratio::from_i64(2, 1)? * &v_obs) * &err_v;
    if sigma_u.is_zero() {
        return Err(CoreError::ZeroDivision);
    }
    let u_gas = &v_gas * &v_gas;
    let u_disk = &v_disk * &v_disk;
    let u_bulge = &v_bul * &v_bul;
    let half = Ratio::from_i64(1, 2)?;
    let seven_tenths = Ratio::from_i64(7, 10)?;
    let u_bar = &(&u_gas + &(&half * &u_disk)) + &(&seven_tenths * &u_bulge);
    let c_cap = &r_v * &(&(&v_gas.abs() + &v_disk.abs()) + &v_bul.abs());
    let d_bf = c_cap.clone();
    let u_ao = &u_bar + &(&Ratio::from_i64(8, 1)? * &d_bf);
    let delta_u = &u_ao - &u_obs;
    let z_u = delta_u.checked_div(&sigma_u)?;
    Ok(SparcBinOutput {
        u_obs,
        sigma_u,
        u_gas,
        u_disk,
        u_bulge,
        u_bar,
        c_cap,
        d_bf,
        u_ao,
        delta_u,
        z_u,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhaseInterval {
    pub winding: BigInt,
    pub phase_start: Ratio,
    pub phase_end: Ratio,
}

impl PhaseInterval {
    /// Compute `winding + phase_end - phase_start` after strictly enforcing `0 <= phase < 1` (`P1-3`).
    pub fn advance(&self) -> CoreResult<Ratio> {
        let zero = Ratio::zero();
        let one = Ratio::one();
        if self.phase_start < zero
            || self.phase_start >= one
            || self.phase_end < zero
            || self.phase_end >= one
        {
            return Err(CoreError::DomainError(format!(
                "phase out of [0, 1): start={}, end={}",
                self.phase_start, self.phase_end
            )));
        }
        Ok(&(&Ratio::from_int(self.winding.clone()) + &self.phase_end) - &self.phase_start)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TemporalMetrologyPhaseSiOutput {
    pub compared_advance: Ratio,
    pub reference_advance: Ratio,
    pub ratio: Ratio,
    pub duration_seconds: Ratio,
    pub mean_frequency_hz: Ratio,
    pub split_left_advance: Ratio,
    pub split_right_advance: Ratio,
    pub split_additivity_exact: bool,
}

/// `RC-017` (`temporal_metrology_phase_si`).
pub fn temporal_metrology_phase_si(
    compared: &PhaseInterval,
    reference: &PhaseInterval,
    split_left: &PhaseInterval,
    split_right: &PhaseInterval,
    cs133_hz: &BigInt,
) -> CoreResult<TemporalMetrologyPhaseSiOutput> {
    if !cs133_hz.is_positive() {
        return Err(CoreError::DomainError("cs133_hz must be positive".to_string()));
    }
    let compared_advance = compared.advance()?;
    let reference_advance = reference.advance()?;
    let split_left_advance = split_left.advance()?;
    let split_right_advance = split_right.advance()?;
    if !reference_advance.is_positive() {
        return Err(CoreError::DomainError(
            "positive reference advance required".to_string(),
        ));
    }
    let cs_q = Ratio::from_int(cs133_hz.clone());
    let ratio = compared_advance.checked_div(&reference_advance)?;
    let duration_seconds = reference_advance.checked_div(&cs_q)?;
    let mean_frequency_hz = (&cs_q * &compared_advance).checked_div(&reference_advance)?;
    let split_additivity_exact = (&split_left_advance + &split_right_advance) == compared_advance;
    Ok(TemporalMetrologyPhaseSiOutput {
        compared_advance,
        reference_advance,
        ratio,
        duration_seconds,
        mean_frequency_hz,
        split_left_advance,
        split_right_advance,
        split_additivity_exact,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TemporalMetrologySignedOutput {
    pub compared_advance: Ratio,
    pub reference_advance: Ratio,
    pub duration_seconds: Ratio,
    pub signed_mean_frequency_hz: Ratio,
}

/// `RC-018` (`temporal_metrology_signed_advance`).
pub fn temporal_metrology_signed_advance(
    compared: &PhaseInterval,
    reference: &PhaseInterval,
    cs133_hz: &BigInt,
) -> CoreResult<TemporalMetrologySignedOutput> {
    if !cs133_hz.is_positive() {
        return Err(CoreError::DomainError("cs133_hz must be positive".to_string()));
    }
    let compared_advance = compared.advance()?;
    let reference_advance = reference.advance()?;
    if !reference_advance.is_positive() {
        return Err(CoreError::DomainError(
            "positive reference advance required".to_string(),
        ));
    }
    let cs_q = Ratio::from_int(cs133_hz.clone());
    let duration_seconds = reference_advance.checked_div(&cs_q)?;
    let signed_mean_frequency_hz = (&cs_q * &compared_advance).checked_div(&reference_advance)?;
    Ok(TemporalMetrologySignedOutput {
        compared_advance,
        reference_advance,
        duration_seconds,
        signed_mean_frequency_hz,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TemporalMetrologyHellwigOutput {
    pub published_mean_hz: Ratio,
    pub difference_hz: Ratio,
}

/// `RC-019` (`temporal_metrology_hellwig_mean`).
pub fn temporal_metrology_hellwig_mean(
    table_iv_hz: &Ratio,
    table_vi_hz: &Ratio,
) -> CoreResult<TemporalMetrologyHellwigOutput> {
    let two = Ratio::from_i64(2, 1)?;
    let published_mean_hz = (table_iv_hz + table_vi_hz).checked_div(&two)?;
    let difference_hz = table_iv_hz - table_vi_hz;
    Ok(TemporalMetrologyHellwigOutput {
        published_mean_hz,
        difference_hz,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LensingSwellsTauOutput {
    pub delta_z: Vec<BigInt>,
    pub s3: Vec<i64>,
    pub m: Vec<BigInt>,
    pub sum_delta_z: BigInt,
    pub sum_m: BigInt,
    pub tau_difference: Ratio,
    pub tau_distinct: bool,
}

/// `RC-020` (`lensing_swells_delta3_and_tau_discrepancy`).
pub fn lensing_swells_delta3_and_tau_discrepancy(
    swells_predicted: &[BigInt],
    swells_observed: &[BigInt],
    tau_diagnostic_1210: &Ratio,
    tau_summary_nominal_1210: &Ratio,
) -> CoreResult<LensingSwellsTauOutput> {
    if swells_predicted.len() != swells_observed.len() {
        return Err(CoreError::DomainError(
            "swells_predicted and swells_observed lengths must match".to_string(),
        ));
    }
    let mut delta_z = Vec::with_capacity(swells_predicted.len());
    let mut s3 = Vec::with_capacity(swells_predicted.len());
    let mut m = Vec::with_capacity(swells_predicted.len());
    let mut sum_delta_z = BigInt::zero();
    let mut sum_m = BigInt::zero();
    for (p, o) in swells_predicted.iter().zip(swells_observed) {
        let d = p - o;
        let s = sign3(&d);
        let mag = d.abs();
        sum_delta_z = &sum_delta_z + &d;
        sum_m = &sum_m + &mag;
        delta_z.push(d);
        s3.push(s);
        m.push(mag);
    }
    let tau_difference = tau_summary_nominal_1210 - tau_diagnostic_1210;
    let tau_distinct = tau_diagnostic_1210 != tau_summary_nominal_1210;
    Ok(LensingSwellsTauOutput {
        delta_z,
        s3,
        m,
        sum_delta_z,
        sum_m,
        tau_difference,
        tau_distinct,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Map21SupportPoint {
    pub p: Ratio,
    pub y: Ratio,
    pub d: Ratio,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Map21Output {
    pub outputs: Vec<Ratio>,
    pub output_mean: Ratio,
    pub mean_shift: Ratio,
    pub output_variance: Ratio,
    pub linearized_variance_diagnostic: Ratio,
}

/// `RC-024` (`map21_nonlinear_finite_law_variance`).
pub fn map21_nonlinear_finite_law_variance(
    support: &[Map21SupportPoint],
) -> CoreResult<Map21Output> {
    if support.is_empty() {
        return Err(CoreError::DomainError(
            "MAP-21 support must be non-empty".to_string(),
        ));
    }
    let one = Ratio::one();
    let mut p_sum = Ratio::zero();
    let mut outputs = Vec::with_capacity(support.len());
    for pt in support {
        let denom = &one + &pt.y;
        if denom.is_zero() {
            return Err(CoreError::ZeroDivision);
        }
        p_sum = &p_sum + &pt.p;
        outputs.push(&one.checked_div(&denom)? + &pt.d);
    }
    if p_sum != one {
        return Err(CoreError::DomainError(format!(
            "support weights must sum to 1, got {p_sum}"
        )));
    }

    let mut output_mean = Ratio::zero();
    for (pt, out) in support.iter().zip(&outputs) {
        output_mean = &output_mean + &(&pt.p * out);
    }
    let mean_shift = &output_mean - &one;

    let mut output_variance = Ratio::zero();
    for (pt, out) in support.iter().zip(&outputs) {
        let diff = out - &output_mean;
        output_variance = &output_variance + &(&pt.p * &(&diff * &diff));
    }

    let mut my = Ratio::zero();
    let mut md = Ratio::zero();
    for pt in support {
        my = &my + &(&pt.p * &pt.y);
        md = &md + &(&pt.p * &pt.d);
    }
    let mut cyy = Ratio::zero();
    let mut cdd = Ratio::zero();
    let mut cyd = Ratio::zero();
    for pt in support {
        let dy = &pt.y - &my;
        let dd = &pt.d - &md;
        cyy = &cyy + &(&pt.p * &(&dy * &dy));
        cdd = &cdd + &(&pt.p * &(&dd * &dd));
        cyd = &cyd + &(&pt.p * &(&dy * &dd));
    }
    let two = Ratio::from_i64(2, 1)?;
    let linearized_variance_diagnostic = &(&cyy - &(&two * &cyd)) + &cdd;

    Ok(Map21Output {
        outputs,
        output_mean,
        mean_shift,
        output_variance,
        linearized_variance_diagnostic,
    })
}

// ============================================================================
// 21..24: Symbolic & Quarantined Display Operations (`SD-001`..`SD-006`)
// ============================================================================

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Z4DftOutput {
    pub coefficients: Vec<String>,
    pub reconstruction: Vec<String>,
    pub parseval_time: Ratio,
    pub parseval_freq: Ratio,
}

/// `SD-001` / `SD-002` (`sympy_z4_dft`): exact Gaussian rational `Q(i)` Z4 DFT without floats or SymPy.
pub fn z4_dft_gaussian(value_tokens: &[String]) -> CoreResult<Z4DftOutput> {
    if value_tokens.len() != 4 {
        return Err(CoreError::DomainError(format!(
            "Z4 DFT requires 4 values, got {}",
            value_tokens.len()
        )));
    }
    let mut vals = Vec::with_capacity(4);
    for tok in value_tokens {
        vals.push(parse_printed_rational(tok, true, false)?);
    }

    let mut coeffs = Vec::with_capacity(4);
    for k in 0..4i64 {
        let mut sum = GaussianRatio::zero();
        for (g, v) in vals.iter().enumerate() {
            let term = GaussianRatio::from_ratio(v.clone()).mul(&unit_i_power(-k * (g as i64)));
            sum = sum.add(&term);
        }
        coeffs.push(sum.div_by_int(4)?);
    }

    let mut recon = Vec::with_capacity(4);
    for g in 0..4i64 {
        let mut sum = GaussianRatio::zero();
        for (k, c) in coeffs.iter().enumerate() {
            let term = c.mul(&unit_i_power((k as i64) * g));
            sum = sum.add(&term);
        }
        recon.push(sum);
    }

    let time_sum = vals
        .iter()
        .fold(Ratio::zero(), |acc, v| &acc + &(v * v));
    let parseval_time = time_sum.checked_div(&Ratio::from_i64(4, 1)?)?;
    let parseval_freq = coeffs
        .iter()
        .fold(Ratio::zero(), |acc, c| &acc + &c.norm_sq());
    if parseval_time != parseval_freq {
        return Err(CoreError::ValidationError(
            "Parseval identity failed on Z4 DFT".to_string(),
        ));
    }

    Ok(Z4DftOutput {
        coefficients: coeffs.iter().map(GaussianRatio::format_sympy).collect(),
        reconstruction: recon.iter().map(GaussianRatio::format_sympy).collect(),
        parseval_time,
        parseval_freq,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CyclotomicZ12Output {
    pub v_hat_3: Ratio,
    pub nonzero_modes: Vec<i64>,
    pub parseval_exact: Ratio,
    pub overtone_exceeds_nyquist_bound: bool,
}

/// `SD-003` (`cyclotomic_z12_overtone`): strictly requires `group_order == 12` (`P1-3`).
pub fn cyclotomic_z12_overtone(
    group_order: &BigInt,
    pulses: &[BigInt],
    l_min: &BigInt,
    overtone_mode: &BigInt,
) -> CoreResult<CyclotomicZ12Output> {
    if group_order != &BigInt::from_i64(12) {
        return Err(CoreError::DomainError(format!(
            "cyclotomic_z12_overtone requires group_order == 12, got {group_order}"
        )));
    }
    if !l_min.is_positive() {
        return Err(CoreError::DomainError(
            "l_min must be positive in cyclotomic_z12_overtone".to_string(),
        ));
    }
    let m_mode = overtone_mode
        .to_i64()
        .filter(|&m| (0..12).contains(&m))
        .ok_or_else(|| {
            CoreError::DomainError(format!("overtone_mode out of 0..12: {overtone_mode}"))
        })? as usize;

    let twelve_q = Ratio::from_i64(12, 1)?;
    let mut coeffs_by_mode: Vec<[Ratio; 4]> = Vec::with_capacity(12);
    for mode in 0..12i64 {
        let mut acc = [Ratio::zero(), Ratio::zero(), Ratio::zero(), Ratio::zero()];
        let neg_mode = BigInt::from_i64(-mode);
        for p in pulses {
            let exp = &neg_mode * p;
            let z = zeta12_power(&exp);
            for j in 0..4 {
                acc[j] = &acc[j] + &z[j];
            }
        }
        for c in &mut acc {
            *c = c.checked_div(&twelve_q)?;
        }
        coeffs_by_mode.push(acc);
    }

    for (mode, coeff) in coeffs_by_mode.iter().enumerate() {
        if !coeff[1].is_zero() || !coeff[2].is_zero() || !coeff[3].is_zero() {
            return Err(CoreError::ValidationError(format!(
                "unexpected non-rational cyclotomic part at mode {mode}"
            )));
        }
    }

    let v_hat_3 = coeffs_by_mode[m_mode][0].clone();
    let nonzero_modes: Vec<i64> = (0..12)
        .filter(|&m| !coeffs_by_mode[m][0].is_zero())
        .map(|m| m as i64)
        .collect();

    let freq_e = coeffs_by_mode
        .iter()
        .fold(Ratio::zero(), |acc, c| &acc + &(&c[0] * &c[0]));
    let time_e = Ratio::new(BigInt::from_u64(pulses.len() as u64), BigInt::from_i64(12))?;
    if freq_e != time_e {
        return Err(CoreError::ValidationError(format!(
            "Z12 Parseval mismatch: freq={freq_e} != time={time_e}"
        )));
    }

    let nyquist = Ratio::new(group_order.clone(), &BigInt::from_i64(2) * l_min)?;
    let overtone_exceeds_nyquist_bound = Ratio::from_int(overtone_mode.clone()) > nyquist;

    Ok(CyclotomicZ12Output {
        v_hat_3,
        nonzero_modes,
        parseval_exact: freq_e,
        overtone_exceeds_nyquist_bound,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HiggsLhcRoundedDisplayOutput {
    pub delta_gev: Ratio,
    pub z_score_exact: Ratio,
    pub z_score_rounded_6dp: String,
    pub z_score_rounding_residual: Ratio,
    pub abs_percent_error_rounded_6dp: String,
    pub abs_percent_error_rounding_residual: Ratio,
}

/// `SD-004` / `SD-005` (`higgs_lhc_rounded_display`).
pub fn higgs_lhc_rounded_display(
    mass_gev_tok: &str,
    sigma_gev_tok: &str,
    prediction_gev_tok: &str,
    decimals: usize,
) -> CoreResult<HiggsLhcRoundedDisplayOutput> {
    if decimals > MAX_ROUNDING_DECIMALS {
        return Err(CoreError::DomainError(format!(
            "decimals {decimals} exceeds safety limit {MAX_ROUNDING_DECIMALS}"
        )));
    }
    let mass = parse_printed_rational(mass_gev_tok, false, false)?;
    let sigma = parse_printed_rational(sigma_gev_tok, false, false)?;
    let pred = parse_printed_rational(prediction_gev_tok, false, false)?;
    if !mass.is_positive() || !sigma.is_positive() {
        return Err(CoreError::DomainError(
            "mass_GeV and sigma_GeV must be positive".to_string(),
        ));
    }

    let delta_gev = &pred - &mass;
    let z_score_exact = delta_gev.checked_div(&sigma)?;
    let z_rnd = round_rational_half_even(&z_score_exact, decimals)?;
    let z_score_rounded_6dp = format_rational_fixed(&z_score_exact, decimals)?;
    let z_score_rounding_residual = (&z_rnd - &z_score_exact).abs();

    let hundred = Ratio::from_i64(100, 1)?;
    let ape_exact = (&delta_gev.abs() * &hundred).checked_div(&mass)?;
    let ape_rnd = round_rational_half_even(&ape_exact, decimals)?;
    let abs_percent_error_rounded_6dp = format_rational_fixed(&ape_exact, decimals)?;
    let abs_percent_error_rounding_residual = (&ape_rnd - &ape_exact).abs();

    Ok(HiggsLhcRoundedDisplayOutput {
        delta_gev,
        z_score_exact,
        z_score_rounded_6dp,
        z_score_rounding_residual,
        abs_percent_error_rounded_6dp,
        abs_percent_error_rounding_residual,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HalfUlpEnclosureOutput {
    pub decimal_places: usize,
    pub value: Ratio,
    pub half_ulp_bound: Ratio,
    pub enclosure_lower: Ratio,
    pub enclosure_upper: Ratio,
}

/// `SD-006` (`printed_token_half_ulp_enclosure`).
pub fn printed_token_half_ulp_enclosure(printed_token: &str) -> CoreResult<HalfUlpEnclosureOutput> {
    let value = parse_printed_rational(printed_token, false, false)?;
    let Some((_, frac)) = printed_token.split_once('.') else {
        return Err(CoreError::DomainError(format!(
            "decimal printed_token with '.' required, got {printed_token:?}"
        )));
    };
    let decimal_places = frac.len();
    if decimal_places > MAX_ROUNDING_DECIMALS {
        return Err(CoreError::Overflow(format!(
            "decimal_places {decimal_places} exceeds safety limit {MAX_ROUNDING_DECIMALS}"
        )));
    }
    let scale = BigInt::from_u64(10).pow(decimal_places as u32);
    let half_ulp_bound = Ratio::new(BigInt::one(), &BigInt::from_i64(2) * &scale)?;
    let enclosure_lower = &value - &half_ulp_bound;
    let enclosure_upper = &value + &half_ulp_bound;
    Ok(HalfUlpEnclosureOutput {
        decimal_places,
        value,
        half_ulp_bound,
        enclosure_lower,
        enclosure_upper,
    })
}
