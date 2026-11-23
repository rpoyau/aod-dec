use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, Mul, Neg, Sub};

use crate::bigint::BigInt;
use crate::error::{CoreError, CoreResult};

/// Native exact rational number in canonical lowest terms (`den > 0`, `gcd(|num|, den) == 1`).
///
/// Decimal string parsing is deliberately excluded from native `Ratio` (`P0-1`);
/// external decimal strings enter strictly through `crate::qext::parse_decimal_exact`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Ratio {
    num: BigInt,
    den: BigInt,
}

impl Ratio {
    pub fn new(mut num: BigInt, mut den: BigInt) -> CoreResult<Self> {
        if den.is_zero() {
            return Err(CoreError::ZeroDivision);
        }
        if num.is_zero() {
            return Ok(Self {
                num: BigInt::zero(),
                den: BigInt::one(),
            });
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

    #[inline]
    pub fn zero() -> Self {
        Self {
            num: BigInt::zero(),
            den: BigInt::one(),
        }
    }

    #[inline]
    pub fn one() -> Self {
        Self {
            num: BigInt::one(),
            den: BigInt::one(),
        }
    }

    #[inline]
    pub fn from_int(num: BigInt) -> Self {
        Self {
            num,
            den: BigInt::one(),
        }
    }

    #[inline]
    pub fn from_i64(num: i64, den: i64) -> CoreResult<Self> {
        Self::new(BigInt::from_i64(num), BigInt::from_i64(den))
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

    pub fn recip(&self) -> CoreResult<Self> {
        Self::new(self.den.clone(), self.num.clone())
    }

    pub fn checked_div(&self, rhs: &Self) -> CoreResult<Self> {
        if rhs.is_zero() {
            return Err(CoreError::ZeroDivision);
        }
        Self::new(&self.num * &rhs.den, &self.den * &rhs.num)
    }

    /// Exact conversion to `BigInt`, rejecting any non-unit denominator.
    pub fn to_int_exact(&self) -> CoreResult<BigInt> {
        if self.den != BigInt::one() {
            return Err(CoreError::DomainError(format!(
                "ratio-to-int would lose a remainder ({self})"
            )));
        }
        Ok(self.num.clone())
    }

    /// Canonical v1/v2 rendering: ALWAYS `"{num}/{den}"` (e.g. `4/1`, `0/1`, `-4/1`).
    pub fn render_v1(&self) -> String {
        format!("{}/{}", self.num, self.den)
    }
}

impl Ord for Ratio {
    fn cmp(&self, other: &Self) -> Ordering {
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
    fn neg(self) -> Ratio {
        Ratio {
            num: -&self.num,
            den: self.den.clone(),
        }
    }
}

impl Neg for Ratio {
    type Output = Ratio;
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
