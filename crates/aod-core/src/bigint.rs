use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, Mul, Neg, Sub};
use std::str::FromStr;

use crate::error::{CoreError, CoreResult};

/// Arbitrary-precision exact signed integer in canonical sign-magnitude form.
///
/// Representation invariant:
/// - `limbs` are stored little-endian in base 2^32.
/// - No trailing zero limbs (`limbs.last() != Some(&0)`).
/// - Zero is uniquely represented as `neg == false` and `limbs.is_empty()`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct BigInt {
    neg: bool,
    limbs: Vec<u32>,
}

impl BigInt {
    #[inline]
    pub fn zero() -> Self {
        Self {
            neg: false,
            limbs: Vec::new(),
        }
    }

    #[inline]
    pub fn one() -> Self {
        Self {
            neg: false,
            limbs: vec![1],
        }
    }

    #[inline]
    pub fn from_u64(v: u64) -> Self {
        if v == 0 {
            return Self::zero();
        }
        let lo = v as u32;
        let hi = (v >> 32) as u32;
        if hi == 0 {
            Self {
                neg: false,
                limbs: vec![lo],
            }
        } else {
            Self {
                neg: false,
                limbs: vec![lo, hi],
            }
        }
    }

    #[inline]
    pub fn from_i64(v: i64) -> Self {
        if v == 0 {
            return Self::zero();
        }
        let neg = v < 0;
        let mag = v.unsigned_abs();
        let mut out = Self::from_u64(mag);
        out.neg = neg;
        out
    }

    #[inline]
    pub fn from_u128(v: u128) -> Self {
        if v == 0 {
            return Self::zero();
        }
        let mut limbs = Vec::with_capacity(4);
        let mut cur = v;
        while cur != 0 {
            limbs.push(cur as u32);
            cur >>= 32;
        }
        Self { neg: false, limbs }
    }

    #[inline]
    pub fn from_i128(v: i128) -> Self {
        if v == 0 {
            return Self::zero();
        }
        let neg = v < 0;
        let mut out = Self::from_u128(v.unsigned_abs());
        out.neg = neg;
        out
    }

    fn normalize(&mut self) {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
        if self.limbs.is_empty() {
            self.neg = false;
        }
    }

    #[inline]
    pub fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    #[inline]
    pub fn is_negative(&self) -> bool {
        self.neg && !self.limbs.is_empty()
    }

    #[inline]
    pub fn is_positive(&self) -> bool {
        !self.neg && !self.limbs.is_empty()
    }

    #[inline]
    pub fn signum(&self) -> i8 {
        if self.limbs.is_empty() {
            0
        } else if self.neg {
            -1
        } else {
            1
        }
    }

    #[inline]
    pub fn is_even(&self) -> bool {
        self.limbs.first().is_none_or(|&w| (w & 1) == 0)
    }

    #[inline]
    pub fn abs(&self) -> Self {
        Self {
            neg: false,
            limbs: self.limbs.clone(),
        }
    }

    pub fn to_u64(&self) -> Option<u64> {
        if self.neg || self.limbs.len() > 2 {
            return None;
        }
        let lo = *self.limbs.first().unwrap_or(&0) as u64;
        let hi = *self.limbs.get(1).unwrap_or(&0) as u64;
        Some(lo | (hi << 32))
    }

    pub fn to_i64(&self) -> Option<i64> {
        if self.limbs.len() > 2 {
            return None;
        }
        let lo = *self.limbs.first().unwrap_or(&0) as u64;
        let hi = *self.limbs.get(1).unwrap_or(&0) as u64;
        let mag = lo | (hi << 32);
        if self.neg {
            if mag == (1u64 << 63) {
                Some(i64::MIN)
            } else if mag < (1u64 << 63) {
                Some(-(mag as i64))
            } else {
                None
            }
        } else if mag <= i64::MAX as u64 {
            Some(mag as i64)
        } else {
            None
        }
    }

    pub fn to_u128(&self) -> Option<u128> {
        if self.neg || self.limbs.len() > 4 {
            return None;
        }
        let mut acc = 0u128;
        for (i, &w) in self.limbs.iter().enumerate() {
            acc |= (w as u128) << (32 * i);
        }
        Some(acc)
    }

    pub fn to_i128(&self) -> Option<i128> {
        if self.limbs.len() > 4 {
            return None;
        }
        let mut mag = 0u128;
        for (i, &w) in self.limbs.iter().enumerate() {
            mag |= (w as u128) << (32 * i);
        }
        if self.neg {
            if mag == (1u128 << 127) {
                Some(i128::MIN)
            } else if mag < (1u128 << 127) {
                Some(-(mag as i128))
            } else {
                None
            }
        } else if mag <= i128::MAX as u128 {
            Some(mag as i128)
        } else {
            None
        }
    }

    /// Strict base-10 integer parser accepting `^[+-]?[0-9]+$`.
    pub fn parse_decimal(s: &str) -> CoreResult<Self> {
        if s.is_empty() {
            return Err(CoreError::DomainError("empty integer literal".to_string()));
        }
        let bytes = s.as_bytes();
        let (neg, digits) = match bytes[0] {
            b'-' => (true, &s[1..]),
            b'+' => (false, &s[1..]),
            _ => (false, s),
        };
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(CoreError::DomainError(format!(
                "invalid exact integer string: {s}"
            )));
        }
        let mut limbs: Vec<u32> = Vec::new();
        let mut idx = 0usize;
        while idx < digits.len() {
            let chunk_len = (digits.len() - idx).min(9);
            let chunk_str = &digits[idx..idx + chunk_len];
            let chunk_val: u32 = chunk_str
                .parse()
                .map_err(|_| CoreError::DomainError(format!("invalid integer chunk: {s}")))?;
            let mul_base = 10u32.pow(chunk_len as u32);
            mul_add_small(&mut limbs, mul_base, chunk_val);
            idx += chunk_len;
        }
        let mut out = Self { neg, limbs };
        out.normalize();
        Ok(out)
    }

    /// Parse integer literal allowing internal `_` separators (as in Dec source).
    pub fn parse_decimal_underscored(s: &str) -> CoreResult<Self> {
        if s.is_empty() {
            return Err(CoreError::DomainError("empty integer literal".to_string()));
        }
        let cleaned: String = s.chars().filter(|&c| c != '_').collect();
        Self::parse_decimal(&cleaned)
    }

    pub fn to_decimal_string(&self) -> String {
        if self.is_zero() {
            return "0".to_string();
        }
        let mut tmp = self.limbs.clone();
        let mut chunks: Vec<u32> = Vec::new();
        while !tmp.is_empty() {
            let rem = div_rem_small_in_place(&mut tmp, 1_000_000_000u32);
            chunks.push(rem);
        }
        let mut out = String::with_capacity(chunks.len() * 9 + 1);
        if self.neg {
            out.push('-');
        }
        if let Some(&top) = chunks.last() {
            out.push_str(&top.to_string());
        }
        for &c in chunks[..chunks.len().saturating_sub(1)].iter().rev() {
            use std::fmt::Write;
            let _ = write!(&mut out, "{c:09}");
        }
        out
    }

    /// Raise to a non-negative `u32` power using exact binary exponentiation.
    pub fn pow(&self, mut exp: u32) -> Self {
        let mut result = Self::one();
        let mut base = self.clone();
        while exp > 0 {
            if (exp & 1) == 1 {
                result = &result * &base;
            }
            exp >>= 1;
            if exp > 0 {
                base = &base * &base;
            }
        }
        result
    }

    /// Truncating division returning `(quotient, remainder)` with `self == q * rhs + r`,
    /// `|r| < |rhs|`, and `r` having the sign of `self` (or zero).
    pub fn div_rem_trunc(&self, rhs: &Self) -> CoreResult<(Self, Self)> {
        if rhs.is_zero() {
            return Err(CoreError::ZeroDivision);
        }
        if self.is_zero() {
            return Ok((Self::zero(), Self::zero()));
        }
        let (q_limbs, r_limbs) = div_rem_mag(&self.limbs, &rhs.limbs);
        let mut q = Self {
            neg: self.neg != rhs.neg,
            limbs: q_limbs,
        };
        let mut r = Self {
            neg: self.neg,
            limbs: r_limbs,
        };
        q.normalize();
        r.normalize();
        Ok((q, r))
    }

    /// Exact Python / Dec v1 floor division: `self == q * rhs + r` with `|r| < |rhs|`
    /// and `r == 0` or `r.signum() == rhs.signum()`.
    pub fn div_rem_floor(&self, rhs: &Self) -> CoreResult<(Self, Self)> {
        let (mut q, mut r) = self.div_rem_trunc(rhs)?;
        if !r.is_zero() && (r.is_negative() != rhs.is_negative()) {
            q = &q - &Self::one();
            r = &r + rhs;
        }
        Ok((q, r))
    }

    pub fn div_floor(&self, rhs: &Self) -> CoreResult<Self> {
        Ok(self.div_rem_floor(rhs)?.0)
    }

    pub fn rem_floor(&self, rhs: &Self) -> CoreResult<Self> {
        Ok(self.div_rem_floor(rhs)?.1)
    }

    /// Exact non-negative greatest common divisor (`gcd(0, 0) == 0`).
    pub fn gcd(&self, other: &Self) -> Self {
        let mut a = self.limbs.clone();
        let mut b = other.limbs.clone();
        trim_limbs(&mut a);
        trim_limbs(&mut b);
        while !b.is_empty() {
            let (_, r) = div_rem_mag(&a, &b);
            a = b;
            b = r;
        }
        let mut out = Self { neg: false, limbs: a };
        out.normalize();
        out
    }

    /// Exact non-negative least common multiple (`lcm(0, x) == 0`).
    pub fn lcm(&self, other: &Self) -> Self {
        if self.is_zero() || other.is_zero() {
            return Self::zero();
        }
        let g = self.gcd(other);
        let (q, _) = self.abs().div_rem_trunc(&g).expect("gcd of nonzeros is nonzero");
        &q * &other.abs()
    }

    /// Non-negative bitwise AND (`P2-2`: explicitly rejects negative operands).
    pub fn bitand(&self, rhs: &Self) -> CoreResult<Self> {
        if self.is_negative() || rhs.is_negative() {
            return Err(CoreError::DomainError(
                "bitwise AND requires non-negative BigInt operands".to_string(),
            ));
        }
        let len = self.limbs.len().min(rhs.limbs.len());
        let mut limbs = Vec::with_capacity(len);
        for i in 0..len {
            limbs.push(self.limbs[i] & rhs.limbs[i]);
        }
        let mut out = Self { neg: false, limbs };
        out.normalize();
        Ok(out)
    }

    /// Non-negative bitwise OR (`P2-2`: explicitly rejects negative operands).
    pub fn bitor(&self, rhs: &Self) -> CoreResult<Self> {
        if self.is_negative() || rhs.is_negative() {
            return Err(CoreError::DomainError(
                "bitwise OR requires non-negative BigInt operands".to_string(),
            ));
        }
        let len = self.limbs.len().max(rhs.limbs.len());
        let mut limbs = Vec::with_capacity(len);
        for i in 0..len {
            let a = *self.limbs.get(i).unwrap_or(&0);
            let b = *rhs.limbs.get(i).unwrap_or(&0);
            limbs.push(a | b);
        }
        let mut out = Self { neg: false, limbs };
        out.normalize();
        Ok(out)
    }

    /// Non-negative bitwise XOR (`P2-2`: explicitly rejects negative operands).
    pub fn bitxor(&self, rhs: &Self) -> CoreResult<Self> {
        if self.is_negative() || rhs.is_negative() {
            return Err(CoreError::DomainError(
                "bitwise XOR requires non-negative BigInt operands".to_string(),
            ));
        }
        let len = self.limbs.len().max(rhs.limbs.len());
        let mut limbs = Vec::with_capacity(len);
        for i in 0..len {
            let a = *self.limbs.get(i).unwrap_or(&0);
            let b = *rhs.limbs.get(i).unwrap_or(&0);
            limbs.push(a ^ b);
        }
        let mut out = Self { neg: false, limbs };
        out.normalize();
        Ok(out)
    }

    /// Non-negative left shift (`P2-2`: explicitly rejects negative operands).
    pub fn shl(&self, bits: u32) -> CoreResult<Self> {
        if self.is_negative() {
            return Err(CoreError::DomainError(
                "bitwise left shift requires non-negative BigInt operand".to_string(),
            ));
        }
        if self.is_zero() || bits == 0 {
            return Ok(self.clone());
        }
        if bits > 1_048_576 {
            return Err(CoreError::Overflow(format!(
                "shift amount {bits} exceeds safety limit"
            )));
        }
        let limbs = shl_mag(&self.limbs, bits as usize);
        let mut out = Self { neg: false, limbs };
        out.normalize();
        Ok(out)
    }

    /// Non-negative right shift (`P2-2`: explicitly rejects negative operands).
    pub fn shr(&self, bits: u32) -> CoreResult<Self> {
        if self.is_negative() {
            return Err(CoreError::DomainError(
                "bitwise right shift requires non-negative BigInt operand".to_string(),
            ));
        }
        if self.is_zero() || bits == 0 {
            return Ok(self.clone());
        }
        let limbs = shr_mag(&self.limbs, bits as usize);
        let mut out = Self { neg: false, limbs };
        out.normalize();
        Ok(out)
    }
}

fn trim_limbs(limbs: &mut Vec<u32>) {
    while limbs.last() == Some(&0) {
        limbs.pop();
    }
}

fn cmp_mag(a: &[u32], b: &[u32]) -> Ordering {
    match a.len().cmp(&b.len()) {
        Ordering::Equal => {
            for (&x, &y) in a.iter().rev().zip(b.iter().rev()) {
                match x.cmp(&y) {
                    Ordering::Equal => continue,
                    non_eq => return non_eq,
                }
            }
            Ordering::Equal
        }
        non_eq => non_eq,
    }
}

fn add_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    let max_len = a.len().max(b.len());
    let mut out = Vec::with_capacity(max_len + 1);
    let mut carry = 0u64;
    for i in 0..max_len {
        let av = *a.get(i).unwrap_or(&0) as u64;
        let bv = *b.get(i).unwrap_or(&0) as u64;
        let sum = av + bv + carry;
        out.push(sum as u32);
        carry = sum >> 32;
    }
    if carry != 0 {
        out.push(carry as u32);
    }
    trim_limbs(&mut out);
    out
}

/// Requires `cmp_mag(a, b) != Ordering::Less`.
fn sub_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(a.len());
    let mut borrow = 0i64;
    for i in 0..a.len() {
        let av = a[i] as i64;
        let bv = *b.get(i).unwrap_or(&0) as i64;
        let mut diff = av - bv - borrow;
        if diff < 0 {
            diff += 1i64 << 32;
            borrow = 1;
        } else {
            borrow = 0;
        }
        out.push(diff as u32);
    }
    debug_assert_eq!(borrow, 0);
    trim_limbs(&mut out);
    out
}

fn sub_mag_in_place(a: &mut Vec<u32>, b: &[u32]) {
    let mut borrow = 0i64;
    for i in 0..a.len() {
        let av = a[i] as i64;
        let bv = *b.get(i).unwrap_or(&0) as i64;
        let mut diff = av - bv - borrow;
        if diff < 0 {
            diff += 1i64 << 32;
            borrow = 1;
        } else {
            borrow = 0;
        }
        a[i] = diff as u32;
    }
    debug_assert_eq!(borrow, 0);
    trim_limbs(a);
}

fn mul_mag(a: &[u32], b: &[u32]) -> Vec<u32> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out = vec![0u32; a.len() + b.len()];
    for (i, &av) in a.iter().enumerate() {
        let mut carry = 0u64;
        let av64 = av as u64;
        for (j, &bv) in b.iter().enumerate() {
            let prod = av64 * (bv as u64) + (out[i + j] as u64) + carry;
            out[i + j] = prod as u32;
            carry = prod >> 32;
        }
        out[i + b.len()] = ((out[i + b.len()] as u64) + carry) as u32;
    }
    trim_limbs(&mut out);
    out
}

fn mul_add_small(limbs: &mut Vec<u32>, mul: u32, add: u32) {
    let mul64 = mul as u64;
    let mut carry = add as u64;
    for slot in limbs.iter_mut() {
        let prod = (*slot as u64) * mul64 + carry;
        *slot = prod as u32;
        carry = prod >> 32;
    }
    if carry != 0 {
        limbs.push(carry as u32);
    }
    trim_limbs(limbs);
}

fn div_rem_small_in_place(limbs: &mut Vec<u32>, d: u32) -> u32 {
    let d64 = d as u64;
    let mut rem = 0u64;
    for slot in limbs.iter_mut().rev() {
        let cur = (rem << 32) | (*slot as u64);
        *slot = (cur / d64) as u32;
        rem = cur % d64;
    }
    trim_limbs(limbs);
    rem as u32
}

fn bit_len_mag(a: &[u32]) -> usize {
    match a.last() {
        None => 0,
        Some(&top) => (a.len() - 1) * 32 + (32 - top.leading_zeros() as usize),
    }
}

fn shl_mag(a: &[u32], bits: usize) -> Vec<u32> {
    if a.is_empty() || bits == 0 {
        return a.to_vec();
    }
    let word_shift = bits / 32;
    let bit_shift = bits % 32;
    let mut out = vec![0u32; word_shift + a.len() + usize::from(bit_shift != 0)];
    if bit_shift == 0 {
        out[word_shift..word_shift + a.len()].copy_from_slice(a);
    } else {
        let mut carry = 0u32;
        for (i, &w) in a.iter().enumerate() {
            out[word_shift + i] = (w << bit_shift) | carry;
            carry = w >> (32 - bit_shift);
        }
        if carry != 0 {
            out[word_shift + a.len()] = carry;
        }
    }
    trim_limbs(&mut out);
    out
}

fn shr_mag(a: &[u32], bits: usize) -> Vec<u32> {
    if a.is_empty() || bits == 0 {
        return a.to_vec();
    }
    let word_shift = bits / 32;
    if word_shift >= a.len() {
        return Vec::new();
    }
    let bit_shift = bits % 32;
    let new_len = a.len() - word_shift;
    let mut out = vec![0u32; new_len];
    if bit_shift == 0 {
        out.copy_from_slice(&a[word_shift..]);
    } else {
        let mut carry = 0u32;
        for i in (0..new_len).rev() {
            let w = a[word_shift + i];
            out[i] = (w >> bit_shift) | carry;
            carry = w << (32 - bit_shift);
        }
    }
    trim_limbs(&mut out);
    out
}

fn shr1_in_place(a: &mut Vec<u32>) {
    let mut carry = 0u32;
    for slot in a.iter_mut().rev() {
        let next_carry = *slot << 31;
        *slot = (*slot >> 1) | carry;
        carry = next_carry;
    }
    trim_limbs(a);
}

fn div_rem_mag(u: &[u32], v: &[u32]) -> (Vec<u32>, Vec<u32>) {
    debug_assert!(!v.is_empty());
    match cmp_mag(u, v) {
        Ordering::Less => return (Vec::new(), u.to_vec()),
        Ordering::Equal => return (vec![1], Vec::new()),
        Ordering::Greater => {}
    }
    if v.len() == 1 {
        let mut q = u.to_vec();
        let rem = div_rem_small_in_place(&mut q, v[0]);
        let r = if rem == 0 { Vec::new() } else { vec![rem] };
        return (q, r);
    }

    let u_bits = bit_len_mag(u);
    let v_bits = bit_len_mag(v);
    let shift = u_bits - v_bits;
    let mut rem = u.to_vec();
    let mut divisor = shl_mag(v, shift);
    let mut q = vec![0u32; (shift / 32) + 1];

    for bit in (0..=shift).rev() {
        if cmp_mag(&rem, &divisor) != Ordering::Less {
            sub_mag_in_place(&mut rem, &divisor);
            q[bit / 32] |= 1u32 << (bit % 32);
        }
        if bit > 0 {
            shr1_in_place(&mut divisor);
        }
    }
    trim_limbs(&mut q);
    trim_limbs(&mut rem);
    (q, rem)
}

impl Ord for BigInt {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.is_negative(), other.is_negative()) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => cmp_mag(&self.limbs, &other.limbs),
            (true, true) => cmp_mag(&other.limbs, &self.limbs),
        }
    }
}

impl PartialOrd for BigInt {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Neg for &BigInt {
    type Output = BigInt;
    fn neg(self) -> BigInt {
        if self.is_zero() {
            BigInt::zero()
        } else {
            BigInt {
                neg: !self.neg,
                limbs: self.limbs.clone(),
            }
        }
    }
}

impl Neg for BigInt {
    type Output = BigInt;
    fn neg(mut self) -> BigInt {
        if !self.is_zero() {
            self.neg = !self.neg;
        }
        self
    }
}

impl Add for &BigInt {
    type Output = BigInt;
    fn add(self, rhs: &BigInt) -> BigInt {
        if self.is_zero() {
            return rhs.clone();
        }
        if rhs.is_zero() {
            return self.clone();
        }
        if self.neg == rhs.neg {
            let mut out = BigInt {
                neg: self.neg,
                limbs: add_mag(&self.limbs, &rhs.limbs),
            };
            out.normalize();
            out
        } else {
            match cmp_mag(&self.limbs, &rhs.limbs) {
                Ordering::Equal => BigInt::zero(),
                Ordering::Greater => {
                    let mut out = BigInt {
                        neg: self.neg,
                        limbs: sub_mag(&self.limbs, &rhs.limbs),
                    };
                    out.normalize();
                    out
                }
                Ordering::Less => {
                    let mut out = BigInt {
                        neg: rhs.neg,
                        limbs: sub_mag(&rhs.limbs, &self.limbs),
                    };
                    out.normalize();
                    out
                }
            }
        }
    }
}

impl Add for BigInt {
    type Output = BigInt;
    fn add(self, rhs: BigInt) -> BigInt {
        &self + &rhs
    }
}

impl Sub for &BigInt {
    type Output = BigInt;
    fn sub(self, rhs: &BigInt) -> BigInt {
        self + &(-rhs)
    }
}

impl Sub for BigInt {
    type Output = BigInt;
    fn sub(self, rhs: BigInt) -> BigInt {
        &self - &rhs
    }
}

impl Mul for &BigInt {
    type Output = BigInt;
    fn mul(self, rhs: &BigInt) -> BigInt {
        if self.is_zero() || rhs.is_zero() {
            return BigInt::zero();
        }
        let mut out = BigInt {
            neg: self.neg != rhs.neg,
            limbs: mul_mag(&self.limbs, &rhs.limbs),
        };
        out.normalize();
        out
    }
}

impl Mul for BigInt {
    type Output = BigInt;
    fn mul(self, rhs: BigInt) -> BigInt {
        &self * &rhs
    }
}

impl From<i64> for BigInt {
    fn from(v: i64) -> Self {
        Self::from_i64(v)
    }
}

impl From<u64> for BigInt {
    fn from(v: u64) -> Self {
        Self::from_u64(v)
    }
}

impl From<i128> for BigInt {
    fn from(v: i128) -> Self {
        Self::from_i128(v)
    }
}

impl From<u128> for BigInt {
    fn from(v: u128) -> Self {
        Self::from_u128(v)
    }
}

impl From<i32> for BigInt {
    fn from(v: i32) -> Self {
        Self::from_i64(v as i64)
    }
}

impl From<u32> for BigInt {
    fn from(v: u32) -> Self {
        Self::from_u64(v as u64)
    }
}

impl From<usize> for BigInt {
    fn from(v: usize) -> Self {
        Self::from_u64(v as u64)
    }
}

impl FromStr for BigInt {
    type Err = CoreError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_decimal(s)
    }
}

impl fmt::Display for BigInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_decimal_string())
    }
}
