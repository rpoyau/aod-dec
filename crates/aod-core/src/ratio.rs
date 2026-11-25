use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, Mul, Neg, Sub};

use crate::bigint::BigInt;
use crate::error::{CoreError, CoreResult};
use crate::trit_int::{
    fast_pack_t32, pack_compact_ratio_u64, swar_unpack_t32, unpack_compact_ratio_u64, TritInt,
    T32_LIMIT,
};

/// Native exact rational number as 2 coupled 2-bit balanced-ternary integers `(num, den)`
/// in canonical lowest terms (`den > 0`, `gcd(|num|, den) == 1`).
///
/// Decimal string parsing is deliberately excluded from native `Ratio` (`P0-1`);
/// external decimal strings enter strictly through `crate::qext::parse_decimal_exact`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Ratio {
    num: TritInt,
    den: TritInt,
}

#[inline]
fn gcd_u128_fast(mut a: u128, mut b: u128) -> u128 {
    if a == 0 {
        return b;
    }
    if b == 0 {
        return a;
    }
    let shift = (a | b).trailing_zeros();
    a >>= a.trailing_zeros();
    loop {
        b >>= b.trailing_zeros();
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        b -= a;
        if b == 0 {
            return a << shift;
        }
    }
}

impl Ratio {
    pub fn new(mut num: BigInt, mut den: BigInt) -> CoreResult<Self> {
        if den.is_zero() {
            return Err(CoreError::ZeroDivision);
        }
        if num.is_zero() {
            return Ok(Self::zero());
        }
        if let (Some(na), Some(da)) = (num.inline_word(), den.inline_word()) {
            let nv = swar_unpack_t32(na)? as i128;
            let dv = swar_unpack_t32(da)? as i128;
            return Self::from_i128(nv, dv);
        }
        if den.is_negative() {
            num = -num;
            den = -den;
        }
        let g = num.gcd(&den);
        if g != BigInt::one() {
            num = num.div_rem_trunc(&g)?.0;
            den = den.div_rem_trunc(&g)?.0;
        }
        Ok(Self { num, den })
    }

    /// Zero-allocation fast constructor from `i128` numerator and denominator.
    pub fn from_i128(mut num: i128, mut den: i128) -> CoreResult<Self> {
        if den == 0 {
            return Err(CoreError::ZeroDivision);
        }
        if num == 0 {
            return Ok(Self::zero());
        }
        if den < 0 {
            num = -num;
            den = -den;
        }
        let g = gcd_u128_fast(num.unsigned_abs(), den.unsigned_abs()) as i128;
        if g > 1 {
            num /= g;
            den /= g;
        }
        Ok(Self {
            num: TritInt::from_i128(num),
            den: TritInt::from_i128(den),
        })
    }

    /// Construct directly from already-canonical inline 32-trit `(num_word, den_word)`.
    #[inline]
    pub fn from_canonical_inline_words(num_w: u64, den_w: u64) -> CoreResult<Self> {
        let num = TritInt::from_inline_word(num_w)?;
        let den = TritInt::from_inline_word(den_w)?;
        if !den.is_positive() {
            return Err(CoreError::ZeroDivision);
        }
        Ok(Self { num, den })
    }

    /// Construct from arbitrary inline 32-trit words `(num_w, den_w)` and reduce to lowest terms.
    #[inline]
    pub fn from_inline_words_reduced(num_w: u64, den_w: u64) -> CoreResult<Self> {
        let nv = swar_unpack_t32(num_w)? as i128;
        let dv = swar_unpack_t32(den_w)? as i128;
        Self::from_i128(nv, dv)
    }

    /// Return the 2 coupled inline 32-trit `u64` words `(num_w, den_w)` if both fit in 32 trits.
    #[inline]
    pub const fn inline_words(&self) -> Option<(u64, u64)> {
        match (self.num.inline_word(), self.den.inline_word()) {
            (Some(nw), Some(dw)) => Some((nw, dw)),
            _ => None,
        }
    }

    /// Pack into a single 64-bit compact word `[num] [11] [den]` if combined trit width is `<= 31`.
    pub fn pack_compact_u64(&self) -> Option<u64> {
        let (nw, dw) = self.inline_words()?;
        let n = swar_unpack_t32(nw).ok()?;
        let d = swar_unpack_t32(dw).ok()?;
        pack_compact_ratio_u64(n, d)
    }

    /// Unpack a compact 1-word `[num] [11] [den]` ratio and verify canonical reduction.
    pub fn unpack_compact_u64(w: u64) -> CoreResult<Self> {
        let (n, d) = unpack_compact_ratio_u64(w)
            .ok_or_else(|| CoreError::DomainError(format!("invalid compact ratio word: 0x{w:016x}")))?;
        Self::from_i64(n, d)
    }

    #[inline]
    pub const fn zero() -> Self {
        Self {
            num: TritInt::ZERO,
            den: TritInt::ONE,
        }
    }

    #[inline]
    pub const fn one() -> Self {
        Self {
            num: TritInt::ONE,
            den: TritInt::ONE,
        }
    }

    #[inline]
    pub fn from_int(num: BigInt) -> Self {
        Self {
            num,
            den: TritInt::ONE,
        }
    }

    #[inline]
    pub fn from_i64(num: i64, den: i64) -> CoreResult<Self> {
        if den == 0 {
            return Err(CoreError::ZeroDivision);
        }
        if num == 0 {
            return Ok(Self::zero());
        }
        Self::from_i128(num as i128, den as i128)
    }

    #[inline]
    pub fn num(&self) -> &BigInt {
        &self.num
    }

    #[inline]
    pub fn den(&self) -> &BigInt {
        &self.den
    }

    #[inline]
    pub fn is_zero(&self) -> bool {
        self.num.is_zero()
    }

    #[inline]
    pub fn is_negative(&self) -> bool {
        self.num.is_negative()
    }

    #[inline]
    pub fn is_positive(&self) -> bool {
        self.num.is_positive()
    }

    pub fn abs(&self) -> Self {
        Self {
            num: self.num.abs(),
            den: self.den.clone(),
        }
    }

    /// O(1) reciprocal: swaps `(den, num)` without recomputing GCD (canonical coprimality is symmetric!).
    pub fn recip(&self) -> CoreResult<Self> {
        if self.num.is_zero() {
            return Err(CoreError::ZeroDivision);
        }
        if self.num.is_negative() {
            Ok(Self {
                num: -&self.den,
                den: -&self.num,
            })
        } else {
            Ok(Self {
                num: self.den.clone(),
                den: self.num.clone(),
            })
        }
    }

    pub fn checked_div(&self, rhs: &Self) -> CoreResult<Self> {
        if rhs.is_zero() {
            return Err(CoreError::ZeroDivision);
        }
        if let (Some((na, da)), Some((nb, db))) = (self.inline_words(), rhs.inline_words()) {
            let n_a = swar_unpack_t32(na).expect("canonical") as i128;
            let d_a = swar_unpack_t32(da).expect("canonical") as i128;
            let n_b = swar_unpack_t32(nb).expect("canonical") as i128;
            let d_b = swar_unpack_t32(db).expect("canonical") as i128;
            // `|T32_LIMIT * T32_LIMIT| < 8.59e29 << i128::MAX`: guaranteed zero `i128` overflow!
            return Self::from_i128(n_a * d_b, d_a * n_b);
        }
        Self::new(&self.num * &rhs.den, &self.den * &rhs.num)
    }

    /// Exact conversion to `BigInt`, rejecting any non-unit denominator.
    pub fn to_int_exact(&self) -> CoreResult<BigInt> {
        if self.den != TritInt::ONE {
            return Err(CoreError::DomainError(format!(
                "ratio-to-int would lose a remainder ({self})"
            )));
        }
        Ok(self.num.clone())
    }

    /// Canonical rendering: ALWAYS `"{num}/{den}"` (e.g. `4/1`, `0/1`, `-4/1`).
    pub fn render(&self) -> String {
        format!("{}/{}", self.num, self.den)
    }

    pub fn render_v1(&self) -> String {
        self.render()
    }

    /// Helper constructing unit-denominator inline ratio when `|n| <= T32_LIMIT`.
    #[inline]
    pub fn from_small_int(n: i64) -> Self {
        if (-T32_LIMIT..=T32_LIMIT).contains(&n) {
            Self {
                num: TritInt::Inline(fast_pack_t32(n)),
                den: TritInt::ONE,
            }
        } else {
            Self::from_int(TritInt::from_i64(n))
        }
    }
}

impl Ord for Ratio {
    fn cmp(&self, other: &Self) -> Ordering {
        if let (Some((na, da)), Some((nb, db))) = (self.inline_words(), other.inline_words()) {
            let n_a = swar_unpack_t32(na).expect("canonical") as i128;
            let d_a = swar_unpack_t32(da).expect("canonical") as i128;
            let n_b = swar_unpack_t32(nb).expect("canonical") as i128;
            let d_b = swar_unpack_t32(db).expect("canonical") as i128;
            return (n_a * d_b).cmp(&(n_b * d_a));
        }
        (&self.num * &other.den).cmp(&(&other.num * &self.den))
    }
}

impl PartialOrd for Ratio {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Neg for &Ratio {
    type Output = Ratio;
    #[inline]
    fn neg(self) -> Ratio {
        Ratio {
            num: -&self.num,
            den: self.den.clone(),
        }
    }
}

impl Neg for Ratio {
    type Output = Ratio;
    #[inline]
    fn neg(self) -> Ratio {
        Ratio {
            num: -self.num,
            den: self.den,
        }
    }
}

impl Add for &Ratio {
    type Output = Ratio;
    fn add(self, rhs: &Ratio) -> Ratio {
        if let (Some((na, da)), Some((nb, db))) = (self.inline_words(), rhs.inline_words()) {
            let n_a = swar_unpack_t32(na).expect("canonical") as i128;
            let d_a = swar_unpack_t32(da).expect("canonical") as i128;
            let n_b = swar_unpack_t32(nb).expect("canonical") as i128;
            let d_b = swar_unpack_t32(db).expect("canonical") as i128;
            // `2 * T32_LIMIT^2 < 1.72e30 << i128::MAX`: guaranteed zero `i128` overflow!
            return Ratio::from_i128(n_a * d_b + n_b * d_a, d_a * d_b)
                .expect("product of positive denominators is positive");
        }
        let num = &(&self.num * &rhs.den) + &(&rhs.num * &self.den);
        let den = &self.den * &rhs.den;
        Ratio::new(num, den).expect("product of positive denominators is positive")
    }
}

impl Add for Ratio {
    type Output = Ratio;
    fn add(self, rhs: Ratio) -> Ratio {
        &self + &rhs
    }
}

impl Sub for &Ratio {
    type Output = Ratio;
    fn sub(self, rhs: &Ratio) -> Ratio {
        if let (Some((na, da)), Some((nb, db))) = (self.inline_words(), rhs.inline_words()) {
            let n_a = swar_unpack_t32(na).expect("canonical") as i128;
            let d_a = swar_unpack_t32(da).expect("canonical") as i128;
            let n_b = swar_unpack_t32(nb).expect("canonical") as i128;
            let d_b = swar_unpack_t32(db).expect("canonical") as i128;
            return Ratio::from_i128(n_a * d_b - n_b * d_a, d_a * d_b)
                .expect("product of positive denominators is positive");
        }
        let num = &(&self.num * &rhs.den) - &(&rhs.num * &self.den);
        let den = &self.den * &rhs.den;
        Ratio::new(num, den).expect("product of positive denominators is positive")
    }
}

impl Sub for Ratio {
    type Output = Ratio;
    fn sub(self, rhs: Ratio) -> Ratio {
        &self - &rhs
    }
}

impl Mul for &Ratio {
    type Output = Ratio;
    fn mul(self, rhs: &Ratio) -> Ratio {
        if let (Some((na, da)), Some((nb, db))) = (self.inline_words(), rhs.inline_words()) {
            let n_a = swar_unpack_t32(na).expect("canonical") as i128;
            let d_a = swar_unpack_t32(da).expect("canonical") as i128;
            let n_b = swar_unpack_t32(nb).expect("canonical") as i128;
            let d_b = swar_unpack_t32(db).expect("canonical") as i128;
            return Ratio::from_i128(n_a * n_b, d_a * d_b)
                .expect("product of positive denominators is positive");
        }
        let num = &self.num * &rhs.num;
        let den = &self.den * &rhs.den;
        Ratio::new(num, den).expect("product of positive denominators is positive")
    }
}

impl Mul for Ratio {
    type Output = Ratio;
    fn mul(self, rhs: Ratio) -> Ratio {
        &self * &rhs
    }
}

impl fmt::Display for Ratio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}
