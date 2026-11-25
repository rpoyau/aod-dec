use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, Mul, Neg, Sub};
use std::str::FromStr;

use crate::error::{CoreError, CoreResult};

/// Even-bit lane mask (`01` per 2-bit trit lane across 32 lanes).
pub const EVEN_BITS: u64 = 0x5555_5555_5555_5555;
/// Odd-bit lane mask (`10` per 2-bit trit lane across 32 lanes).
pub const ODD_BITS: u64 = 0xAAAA_AAAA_AAAA_AAAA;

/// Disjoint top-lane `0b11` threshold: every valid inline 32-trit word satisfies `w < TOP_TAG_MASK`
/// (in fact `w <= ODD_BITS = 0xAAAA_AAAA_AAAA_AAAA`).
pub const TOP_TAG_MASK: u64 = 0xC000_0000_0000_0000;
/// Top-nibble sub-tag mask (`bits 63..60`) when `w >= TOP_TAG_MASK`.
pub const SUBTAG_MASK: u64 = 0xF000_0000_0000_0000;
/// 60-bit payload mask (`bits 59..0`) inside `0b11`-tagged words.
pub const TAG_PAYLOAD_MASK: u64 = 0x0FFF_FFFF_FFFF_FFFF;

/// `11_00_0...0`: Runtime sentinel for AOD `Null` (`Main §1.1`, Eq. 1–2).
pub const TAG_NULL_SENTINEL: u64 = 0xC000_0000_0000_0000;
/// `11_01_<60-bit handle>`: Cold-spill arena handle for multi-limb `BigTrit` / `Ratio`.
pub const TAG_SPILL_PREFIX: u64 = 0xD000_0000_0000_0000;
/// `11_10_<60-bit id>`: Structural source-absence witness `S_k^{+,-e}` (`Main §6.2`).
pub const TAG_ABSENT_PREFIX: u64 = 0xE000_0000_0000_0000;
/// `11_11_<60-bit code>`: Blocked hinge / collision trap diagnostic word.
pub const TAG_TRAP_PREFIX: u64 = 0xF000_0000_0000_0000;

/// Maximum magnitude of an inline 32-trit balanced ternary scalar: `(3^32 - 1) / 2`.
pub const T32_LIMIT: i64 = 926_510_094_425_920;
/// Exact limb radix `3^32 = 2 * T32_LIMIT + 1`.
pub const RADIX_3_32: i128 = 1_853_020_188_851_841;

const fn build_pack4_lut() -> [u8; 81] {
    let mut lut = [0u8; 81];
    let mut idx = 0usize;
    while idx < 81 {
        let mut n = (idx as i32) - 40;
        let mut byte = 0u8;
        let mut lane = 0u32;
        while lane < 4 {
            let rem = n.rem_euclid(3);
            let digit = if rem == 2 { -1 } else { rem };
            n = (n - digit) / 3;
            let code: u8 = match digit {
                0 => 0,
                1 => 1,
                -1 => 2,
                _ => unreachable!(),
            };
            byte |= code << (2 * lane);
            lane += 1;
        }
        lut[idx] = byte;
        idx += 1;
    }
    lut
}

/// 81-byte lookup table mapping biased 4-trit integer `0..=80` (`[-40..=+40] + 40`) to 8-bit 2-bit/trit code.
pub const PACK4_LUT: [u8; 81] = build_pack4_lut();

/// True iff `w` contains zero `0b11` lanes across all 32 2-bit positions.
#[inline]
pub const fn is_valid_trit_word(w: u64) -> bool {
    ((w & (w >> 1)) & EVEN_BITS) == 0
}

/// 1-cycle carry-free 32-trit negation (`00 -> 00`, `01 <-> 10`).
#[inline]
pub const fn swar_neg(w: u64) -> u64 {
    ((w & EVEN_BITS) << 1) | ((w & ODD_BITS) >> 1)
}

/// 2-instruction sign test (`-1`, `0`, `+1`) on a valid 32-trit word without unpacking.
#[inline]
pub const fn swar_signum(w: u64) -> i8 {
    if w == 0 {
        0
    } else if (w.leading_zeros() & 1) == 1 {
        1
    } else {
        -1
    }
}

/// 4-op monotonic trit bias mapping `10 (-1) -> 00`, `00 (0) -> 01`, `01 (+1) -> 10`
/// so unsigned `u64` `<` matches mathematical integer `<` across all `3^32` values.
#[inline]
pub const fn swar_bias(w: u64) -> u64 {
    ((w & EVEN_BITS) << 1) | ((!(w | (w >> 1))) & EVEN_BITS)
}

/// Branchless comparison of two valid inline 32-trit words without unpacking.
#[inline]
pub fn swar_cmp(a: u64, b: u64) -> Ordering {
    if a == b {
        Ordering::Equal
    } else {
        swar_bias(a).cmp(&swar_bias(b))
    }
}

/// Complete-State 32-trit wave superposition (`Main §6.1`, `S_k^+ = (X_k, M_k^C, R_k)`).
///
/// Returns `(x_cancelled_word, r_collision_even_mask)` where `r_collision_even_mask` has bit `2k`
/// set iff lane `k` experienced opposing `+1` (`01`) and `-1` (`10`) collision (`01 | 10 = 11`).
#[inline]
pub const fn swar_superpose_complete(a: u64, b: u64) -> (u64, u64) {
    let raw = a | b;
    let cancel_even = (raw & (raw >> 1)) & EVEN_BITS;
    let clear_mask = !(cancel_even | (cancel_even << 1));
    (raw & clear_mask, cancel_even)
}

/// Loop-free `O(1)` 5-stage SWAR tree unpack of a 32-trit word into signed `i64`.
/// Rejects any `0b11` lane with `CoreError::InvalidTritEncoding(3)`.
#[inline]
pub fn swar_unpack_t32(w: u64) -> CoreResult<i64> {
    if !is_valid_trit_word(w) {
        return Err(CoreError::InvalidTritEncoding(3));
    }
    let u1 = swar_bias(w);
    let u2 = (u1 & 0x3333_3333_3333_3333) + ((u1 >> 2) & 0x3333_3333_3333_3333) * 3;
    let u4 = (u2 & 0x0F0F_0F0F_0F0F_0F0F) + ((u2 >> 4) & 0x0F0F_0F0F_0F0F_0F0F) * 9;
    let u8_l = (u4 & 0x00FF_00FF_00FF_00FF) + ((u4 >> 8) & 0x00FF_00FF_00FF_00FF) * 81;
    let u16_l =
        (u8_l & 0x0000_FFFF_0000_FFFF) + ((u8_l >> 16) & 0x0000_FFFF_0000_FFFF) * 6561;
    let biased_total = (u16_l & 0xFFFF_FFFF) + (u16_l >> 32) * 43_046_721;
    Ok((biased_total as i64) - T32_LIMIT)
}

/// Loop-free `O(1)` divide-and-conquer pack of signed `n` in `[-T32_LIMIT, +T32_LIMIT]` into 32 2-bit trits.
#[inline]
pub fn fast_pack_t32(n: i64) -> u64 {
    debug_assert!((-T32_LIMIT..=T32_LIMIT).contains(&n));
    let u = (n + T32_LIMIT) as u64;
    let lo16 = u % 43_046_721;
    let hi16 = u / 43_046_721;

    let b01 = lo16 % 6561;
    let b23 = lo16 / 6561;
    let b45 = hi16 % 6561;
    let b67 = hi16 / 6561;

    let c0 = PACK4_LUT[(b01 % 81) as usize] as u64;
    let c1 = PACK4_LUT[(b01 / 81) as usize] as u64;
    let c2 = PACK4_LUT[(b23 % 81) as usize] as u64;
    let c3 = PACK4_LUT[(b23 / 81) as usize] as u64;
    let c4 = PACK4_LUT[(b45 % 81) as usize] as u64;
    let c5 = PACK4_LUT[(b45 / 81) as usize] as u64;
    let c6 = PACK4_LUT[(b67 % 81) as usize] as u64;
    let c7 = PACK4_LUT[(b67 / 81) as usize] as u64;

    c0 | (c1 << 8)
        | (c2 << 16)
        | (c3 << 24)
        | (c4 << 32)
        | (c5 << 40)
        | (c6 << 48)
        | (c7 << 56)
}

/// Pack a canonical `(num, den)` (`den > 0`) into a single 64-bit word `[num] [11] [den]`
/// whenever their combined trit width is `<= 31` (`w < TOP_TAG_MASK`, strictly disjoint from inline `int`s).
pub fn pack_compact_ratio_u64(num: i64, den: i64) -> Option<u64> {
    if den <= 0 || !(-T32_LIMIT..=T32_LIMIT).contains(&num) || den > T32_LIMIT {
        return None;
    }
    let den_bits = fast_pack_t32(den);
    let den_trits = ((64 - den_bits.leading_zeros() as usize + 1) / 2).max(1);
    if den_trits > 30 {
        return None;
    }
    let num_bits = fast_pack_t32(num);
    let num_trits = (64 - num_bits.leading_zeros() as usize + 1) / 2;
    if den_trits + 1 + num_trits > 32 {
        return None;
    }
    let mask_den = (1u64 << (2 * den_trits)) - 1;
    let sep = 0b11u64 << (2 * den_trits);
    let shifted_num = num_bits << (2 * (den_trits + 1));
    let packed = (den_bits & mask_den) | sep | shifted_num;
    debug_assert!(packed < TOP_TAG_MASK);
    Some(packed)
}

/// Unpack a compact 1-word `[num] [11] [den]` ratio (`w < TOP_TAG_MASK` with exactly one `0b11` lane in `1..=30`).
pub fn unpack_compact_ratio_u64(w: u64) -> Option<(i64, i64)> {
    if w >= TOP_TAG_MASK {
        return None;
    }
    let coll = (w & (w >> 1)) & EVEN_BITS;
    if coll.count_ones() != 1 {
        return None;
    }
    let k = (coll.trailing_zeros() as usize) / 2;
    if !(1..=30).contains(&k) {
        return None;
    }
    let den_bits = w & ((1u64 << (2 * k)) - 1);
    let num_bits = w >> (2 * (k + 1));
    let num = swar_unpack_t32(num_bits).ok()?;
    let den = swar_unpack_t32(den_bits).ok()?;
    if den <= 0 {
        return None;
    }
    Some((num, den))
}

/// Arbitrary-precision balanced ternary integer in canonical base `3^32` limbs (little-endian).
///
/// Representation invariants:
/// - Every limb is a valid 32-trit packed `u64` (`is_valid_trit_word(w)` and `w < TOP_TAG_MASK`).
/// - No trailing zero limbs (`limbs.last() != Some(&0)`).
/// - Zero is uniquely `limbs.is_empty()` — no separate `bool` sign field exists!
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct BigTrit {
    limbs: Vec<u64>,
}

fn normalize_i128_base_3_32(raw: &[i128]) -> Vec<u64> {
    let mut out = Vec::with_capacity(raw.len() + 2);
    let mut carry: i128 = 0;
    for &c in raw {
        let total = c + carry;
        let mut rem = total.rem_euclid(RADIX_3_32);
        if rem > (T32_LIMIT as i128) {
            rem -= RADIX_3_32;
        }
        carry = (total - rem) / RADIX_3_32;
        out.push(fast_pack_t32(rem as i64));
    }
    while carry != 0 {
        let mut rem = carry.rem_euclid(RADIX_3_32);
        if rem > (T32_LIMIT as i128) {
            rem -= RADIX_3_32;
        }
        carry = (carry - rem) / RADIX_3_32;
        out.push(fast_pack_t32(rem as i64));
    }
    while out.last() == Some(&0) {
        out.pop();
    }
    out
}

impl BigTrit {
    #[inline]
    pub fn zero() -> Self {
        Self { limbs: Vec::new() }
    }

    #[inline]
    pub fn limbs(&self) -> &[u64] {
        &self.limbs
    }

    pub fn from_limbs_canonical(mut limbs: Vec<u64>) -> CoreResult<Self> {
        for &w in &limbs {
            if !is_valid_trit_word(w) {
                return Err(CoreError::InvalidTritEncoding(3));
            }
        }
        while limbs.last() == Some(&0) {
            limbs.pop();
        }
        Ok(Self { limbs })
    }

    pub fn from_i128(mut v: i128) -> Self {
        if v == 0 {
            return Self::zero();
        }
        let mut limbs = Vec::with_capacity(4);
        while v != 0 {
            let mut rem = v.rem_euclid(RADIX_3_32);
            if rem > (T32_LIMIT as i128) {
                rem -= RADIX_3_32;
            }
            v = (v - rem) / RADIX_3_32;
            limbs.push(fast_pack_t32(rem as i64));
        }
        while limbs.last() == Some(&0) {
            limbs.pop();
        }
        Self { limbs }
    }

    pub fn from_u128(mut v: u128) -> Self {
        if v == 0 {
            return Self::zero();
        }
        let radix_u = RADIX_3_32 as u128;
        let limit_u = T32_LIMIT as u128;
        let mut limbs = Vec::with_capacity(4);
        let mut carry = 0u128;
        while v != 0 || carry != 0 {
            let rem_v = v % radix_u;
            let q_v = v / radix_u;
            let total = rem_v + carry;
            let rem = total % radix_u;
            let extra = total / radix_u;
            if rem > limit_u {
                let signed_rem = (rem as i128) - RADIX_3_32;
                limbs.push(fast_pack_t32(signed_rem as i64));
                carry = extra + 1;
            } else {
                limbs.push(fast_pack_t32(rem as i64));
                carry = extra;
            }
            v = q_v;
        }
        while limbs.last() == Some(&0) {
            limbs.pop();
        }
        Self { limbs }
    }

    fn trim(&mut self) {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
    }

    #[inline]
    pub fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    #[inline]
    pub fn signum(&self) -> i8 {
        match self.limbs.last() {
            None => 0,
            Some(&top) => swar_signum(top),
        }
    }

    /// Because `3 == 1 (mod 2)` and both `+1` (`01`) and `-1` (`10`) have `popcnt == 1`,
    /// a balanced ternary integer is even iff its total `count_ones()` parity is even!
    #[inline]
    pub fn is_even(&self) -> bool {
        let mut parity = 0u32;
        for &w in &self.limbs {
            parity ^= w.count_ones();
        }
        (parity & 1) == 0
    }

    pub fn neg(&self) -> Self {
        Self {
            limbs: self.limbs.iter().map(|&w| swar_neg(w)).collect(),
        }
    }

    pub fn abs(&self) -> Self {
        if self.signum() < 0 {
            self.neg()
        } else {
            self.clone()
        }
    }

    pub fn cmp_val(&self, other: &Self) -> Ordering {
        if self.limbs.len() != other.limbs.len() {
            return if self.limbs.len() > other.limbs.len() {
                if self.signum() > 0 {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            } else if other.signum() > 0 {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        for (&a, &b) in self.limbs.iter().rev().zip(other.limbs.iter().rev()) {
            if a != b {
                return swar_bias(a).cmp(&swar_bias(b));
            }
        }
        Ordering::Equal
    }

    pub fn add(&self, other: &Self) -> Self {
        let max_len = self.limbs.len().max(other.limbs.len());
        let mut out = Vec::with_capacity(max_len + 1);
        let mut carry: i64 = 0;
        for i in 0..max_len {
            let a = self
                .limbs
                .get(i)
                .copied()
                .map(|w| swar_unpack_t32(w).expect("canonical BigTrit limb"))
                .unwrap_or(0);
            let b = other
                .limbs
                .get(i)
                .copied()
                .map(|w| swar_unpack_t32(w).expect("canonical BigTrit limb"))
                .unwrap_or(0);
            let mut sum = (a as i128) + (b as i128) + (carry as i128);
            if sum > (T32_LIMIT as i128) {
                sum -= RADIX_3_32;
                carry = 1;
            } else if sum < -(T32_LIMIT as i128) {
                sum += RADIX_3_32;
                carry = -1;
            } else {
                carry = 0;
            }
            out.push(fast_pack_t32(sum as i64));
        }
        if carry != 0 {
            out.push(fast_pack_t32(carry));
        }
        let mut res = Self { limbs: out };
        res.trim();
        res
    }

    pub fn sub(&self, other: &Self) -> Self {
        self.add(&other.neg())
    }

    pub fn mul(&self, other: &Self) -> Self {
        if self.is_zero() || other.is_zero() {
            return Self::zero();
        }
        let va: Vec<i128> = self
            .limbs
            .iter()
            .map(|&w| swar_unpack_t32(w).expect("canonical BigTrit limb") as i128)
            .collect();
        let vb: Vec<i128> = other
            .limbs
            .iter()
            .map(|&w| swar_unpack_t32(w).expect("canonical BigTrit limb") as i128)
            .collect();
        let mut raw = vec![0i128; va.len() + vb.len()];
        for (i, &ai) in va.iter().enumerate() {
            let mut carry: i128 = 0;
            for (j, &bj) in vb.iter().enumerate() {
                let prod = ai * bj + raw[i + j] + carry;
                let mut rem = prod.rem_euclid(RADIX_3_32);
                if rem > (T32_LIMIT as i128) {
                    rem -= RADIX_3_32;
                }
                carry = (prod - rem) / RADIX_3_32;
                raw[i + j] = rem;
            }
            raw[i + vb.len()] += carry;
        }
        Self {
            limbs: normalize_i128_base_3_32(&raw),
        }
    }

    /// Multiply non-negative or signed `BigTrit` by small positive `mul <= T32_LIMIT` and add `add`.
    pub fn mul_add_small(&self, mul: i64, add: i64) -> Self {
        let mut raw: Vec<i128> = Vec::with_capacity(self.limbs.len() + 2);
        let mut carry = add as i128;
        let m = mul as i128;
        for &w in &self.limbs {
            let d = swar_unpack_t32(w).expect("canonical BigTrit limb") as i128;
            let prod = d * m + carry;
            let mut rem = prod.rem_euclid(RADIX_3_32);
            if rem > (T32_LIMIT as i128) {
                rem -= RADIX_3_32;
            }
            carry = (prod - rem) / RADIX_3_32;
            raw.push(rem);
        }
        if carry != 0 {
            raw.push(carry);
        }
        Self {
            limbs: normalize_i128_base_3_32(&raw),
        }
    }

    /// Exact Euclidean division of `self >= 0` by `0 < d <= T32_LIMIT` via telescoping balanced-limb reduction.
    pub fn div_rem_small_pos(&self, d: i64) -> (Self, i64) {
        debug_assert!(d > 0 && d <= T32_LIMIT);
        debug_assert!(self.signum() >= 0);
        if self.is_zero() {
            return (Self::zero(), 0);
        }
        let d128 = d as i128;
        let mut q_raw = vec![0i128; self.limbs.len()];
        let mut rem: i128 = 0;
        for i in (0..self.limbs.len()).rev() {
            let digit = swar_unpack_t32(self.limbs[i]).expect("canonical BigTrit limb") as i128;
            let cur = rem * RADIX_3_32 + digit;
            q_raw[i] = cur.div_euclid(d128);
            rem = cur.rem_euclid(d128);
        }
        (
            Self {
                limbs: normalize_i128_base_3_32(&q_raw),
            },
            rem as i64,
        )
    }

    /// 1-based trit length up to the highest non-zero trit (`0` returns `0`).
    pub fn trit_len(&self) -> usize {
        match self.limbs.last() {
            None => 0,
            Some(&top) => {
                let top_trits = (64 - top.leading_zeros() as usize + 1) / 2;
                (self.limbs.len() - 1) * 32 + top_trits
            }
        }
    }

    /// Count trailing `00` trits (`k` such that `3^k | self`).
    pub fn trailing_zero_trits(&self) -> usize {
        for (idx, &w) in self.limbs.iter().enumerate() {
            if w != 0 {
                return idx * 32 + ((w.trailing_zeros() as usize) >> 1);
            }
        }
        0
    }

    /// Exact division by `3^k` when `3^k | self` (pure bit shifts across 2-bit-trit limbs).
    pub fn shr_trits_exact(&self, k: usize) -> Self {
        if self.is_zero() || k == 0 {
            return self.clone();
        }
        let word_shift = k / 32;
        let trit_shift = k % 32;
        if word_shift >= self.limbs.len() {
            return Self::zero();
        }
        if trit_shift == 0 {
            return Self {
                limbs: self.limbs[word_shift..].to_vec(),
            };
        }
        let bit_shift = 2 * trit_shift;
        let mut out = Vec::with_capacity(self.limbs.len() - word_shift);
        for i in word_shift..self.limbs.len() {
            let lo = self.limbs[i] >> bit_shift;
            let hi = if i + 1 < self.limbs.len() {
                self.limbs[i + 1] << (64 - bit_shift)
            } else {
                0
            };
            out.push(lo | hi);
        }
        let mut res = Self { limbs: out };
        res.trim();
        res
    }

    /// Exact multiplication by `3^k` (pure left bit shifts across 2-bit-trit limbs).
    pub fn shl_trits(&self, k: usize) -> Self {
        if self.is_zero() || k == 0 {
            return self.clone();
        }
        let word_shift = k / 32;
        let trit_shift = k % 32;
        let mut out = vec![0u64; word_shift];
        if trit_shift == 0 {
            out.extend_from_slice(&self.limbs);
            return Self { limbs: out };
        }
        let bit_shift = 2 * trit_shift;
        let mut carry = 0u64;
        for &w in &self.limbs {
            out.push((w << bit_shift) | carry);
            carry = w >> (64 - bit_shift);
        }
        if carry != 0 {
            out.push(carry);
        }
        let mut res = Self { limbs: out };
        res.trim();
        res
    }

    #[inline]
    fn lowest_trit_code(&self) -> u64 {
        self.limbs.first().map(|&w| w & 3).unwrap_or(0)
    }

    /// Division-Free Balanced-Ternary Stein GCD (`gcd(0, x) == |x|`).
    pub fn gcd_ternary_stein(&self, other: &Self) -> Self {
        let mut a = self.abs();
        let mut b = other.abs();
        if a.is_zero() {
            return b;
        }
        if b.is_zero() {
            return a;
        }
        let ka = a.trailing_zero_trits();
        let kb = b.trailing_zero_trits();
        let k_common = ka.min(kb);
        a = a.shr_trits_exact(ka);
        b = b.shr_trits_exact(kb);

        loop {
            match a.cmp_val(&b) {
                Ordering::Equal => return a.shl_trits(k_common),
                Ordering::Less => std::mem::swap(&mut a, &mut b),
                Ordering::Greater => {}
            }
            let next = if a.lowest_trit_code() == b.lowest_trit_code() {
                a.sub(&b)
            } else {
                a.add(&b)
            };
            let tz = next.trailing_zero_trits();
            debug_assert!(tz >= 1);
            a = next.shr_trits_exact(tz).abs();
        }
    }

    /// Exact non-negative division `u = q * v + r` (`0 <= r < v`) for `u >= 0, v > 0`.
    pub fn div_rem_pos(u: &Self, v: &Self) -> (Self, Self) {
        debug_assert!(u.signum() >= 0 && v.signum() > 0);
        match u.cmp_val(v) {
            Ordering::Less => return (Self::zero(), u.clone()),
            Ordering::Equal => return (Self::from_i128(1), Self::zero()),
            Ordering::Greater => {}
        }
        if v.limbs.len() == 1 {
            let d = swar_unpack_t32(v.limbs[0]).expect("canonical BigTrit limb");
            let (q, r) = u.div_rem_small_pos(d);
            return (q, Self::from_i128(r as i128));
        }
        let ku = u.trit_len();
        let kv = v.trit_len();
        let shift = ku.saturating_sub(kv) + 1;
        let mut rem = u.clone();
        let mut divisor = v.shl_trits(shift);
        // `q_trits` stores unsigned base-3 digits `{0, 1, 2}` from `k = 0 ..= shift` (little-endian).
        let mut q_trits = vec![0u8; shift + 1];
        for k in (0..=shift).rev() {
            let mut d = 0u8;
            while rem.cmp_val(&divisor) != Ordering::Less {
                rem = rem.sub(&divisor);
                d += 1;
            }
            q_trits[k] = d;
            if k > 0 {
                divisor = divisor.shr_trits_exact(1);
            }
        }
        let num_limbs = q_trits.len().div_ceil(32);
        let mut raw_q = Vec::with_capacity(num_limbs);
        for chunk in q_trits.chunks(32) {
            let mut acc = 0i128;
            let mut pow3 = 1i128;
            for (idx, &d) in chunk.iter().enumerate() {
                acc += (d as i128) * pow3;
                if idx < 31 {
                    pow3 *= 3;
                }
            }
            raw_q.push(acc);
        }
        (
            Self {
                limbs: normalize_i128_base_3_32(&raw_q),
            },
            rem,
        )
    }

    pub fn to_i128(&self) -> Option<i128> {
        if self.limbs.len() > 3 {
            return None;
        }
        let mut acc: i128 = 0;
        for &w in self.limbs.iter().rev() {
            let d = swar_unpack_t32(w).expect("canonical BigTrit limb") as i128;
            acc = acc.checked_mul(RADIX_3_32)?.checked_add(d)?;
        }
        Some(acc)
    }

    pub fn to_u128(&self) -> Option<u128> {
        if self.signum() < 0 || self.limbs.len() > 3 {
            return None;
        }
        if self.is_zero() {
            return Some(0);
        }
        // Accumulate high-to-low in signed `i128` when small, or exact `u128` via `i128` split:
        // `3^64 = 3_433_683_820_292_512_484_657_849_089_281 < 2^102 < u128::MAX`!
        // Note: `u128::MAX` (~3.40e38) has `3` limbs (`3 * 32 = 96` trits, `3^64 * d2 + 3^32 * d1 + d0`).
        // Since `d2` for `<= u128::MAX` is at most `u128::MAX / 3^64 < 99_100_000`, `acc` fits cleanly!
        let r_u = RADIX_3_32 as u128;
        let mut acc: u128 = 0;
        for &w in self.limbs.iter().rev() {
            let d = swar_unpack_t32(w).expect("canonical BigTrit limb");
            acc = acc.checked_mul(r_u)?;
            if d >= 0 {
                acc = acc.checked_add(d as u128)?;
            } else {
                acc = acc.checked_sub(d.unsigned_abs() as u128)?;
            }
        }
        Some(acc)
    }
}

/// Unified exact 2-bit balanced ternary integer (`int` in clean-break Dec).
///
/// Canonical invariant:
/// - `Inline(w)` iff `|value| <= T32_LIMIT` (`w < TOP_TAG_MASK`, zero heap allocation).
/// - `Spilled(Box<BigTrit>)` iff `|value| > T32_LIMIT` (`b.limbs().len() >= 2`).
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum TritInt {
    Inline(u64),
    Spilled(Box<BigTrit>),
}

impl TritInt {
    pub const ZERO: Self = Self::Inline(0);
    /// `+1` encoded as `0b01 = 1`.
    pub const ONE: Self = Self::Inline(1);
    /// `-1` encoded as `0b10 = 2`.
    pub const NEG_ONE: Self = Self::Inline(2);

    #[inline]
    pub const fn zero() -> Self {
        Self::ZERO
    }

    #[inline]
    pub const fn one() -> Self {
        Self::ONE
    }

    fn from_big_trit(b: BigTrit) -> Self {
        match b.limbs.as_slice() {
            [] => Self::ZERO,
            [w] => Self::Inline(*w),
            _ => Self::Spilled(Box::new(b)),
        }
    }

    fn to_big_trit(&self) -> BigTrit {
        match self {
            Self::Inline(0) => BigTrit::zero(),
            Self::Inline(w) => BigTrit { limbs: vec![*w] },
            Self::Spilled(b) => (**b).clone(),
        }
    }

    /// Construct directly from a verified inline 32-trit packed word (`w < TOP_TAG_MASK`, no `0b11` lanes).
    #[inline]
    pub fn from_inline_word(w: u64) -> CoreResult<Self> {
        if !is_valid_trit_word(w) {
            return Err(CoreError::InvalidTritEncoding(3));
        }
        Ok(Self::Inline(w))
    }

    /// Return the raw 64-bit 2-bit-per-trit word if this `int` fits in 32 trits (`|x| <= T32_LIMIT`).
    #[inline]
    pub const fn inline_word(&self) -> Option<u64> {
        match self {
            Self::Inline(w) => Some(*w),
            Self::Spilled(_) => None,
        }
    }

    #[inline]
    pub fn from_i64(v: i64) -> Self {
        if (-T32_LIMIT..=T32_LIMIT).contains(&v) {
            Self::Inline(fast_pack_t32(v))
        } else {
            Self::from_big_trit(BigTrit::from_i128(v as i128))
        }
    }

    #[inline]
    pub fn from_u64(v: u64) -> Self {
        if v <= (T32_LIMIT as u64) {
            Self::Inline(fast_pack_t32(v as i64))
        } else {
            Self::from_big_trit(BigTrit::from_u128(v as u128))
        }
    }

    #[inline]
    pub fn from_i128(v: i128) -> Self {
        if (-(T32_LIMIT as i128)..=(T32_LIMIT as i128)).contains(&v) {
            Self::Inline(fast_pack_t32(v as i64))
        } else {
            Self::from_big_trit(BigTrit::from_i128(v))
        }
    }

    #[inline]
    pub fn from_u128(v: u128) -> Self {
        if v <= (T32_LIMIT as u128) {
            Self::Inline(fast_pack_t32(v as i64))
        } else {
            Self::from_big_trit(BigTrit::from_u128(v))
        }
    }

    #[inline]
    pub fn is_zero(&self) -> bool {
        matches!(self, Self::Inline(0))
    }

    #[inline]
    pub fn signum(&self) -> i8 {
        match self {
            Self::Inline(w) => swar_signum(*w),
            Self::Spilled(b) => b.signum(),
        }
    }

    #[inline]
    pub fn is_negative(&self) -> bool {
        self.signum() < 0
    }

    #[inline]
    pub fn is_positive(&self) -> bool {
        self.signum() > 0
    }

    /// 1-instruction `POPCNT` parity check (`3 == 1 mod 2`, `01` and `10` each have 1 bit set).
    #[inline]
    pub fn is_even(&self) -> bool {
        match self {
            Self::Inline(w) => (w.count_ones() & 1) == 0,
            Self::Spilled(b) => b.is_even(),
        }
    }

    #[inline]
    pub fn abs(&self) -> Self {
        if self.is_negative() {
            -self
        } else {
            self.clone()
        }
    }

    pub fn to_i64(&self) -> Option<i64> {
        match self {
            Self::Inline(w) => Some(swar_unpack_t32(*w).expect("canonical Inline TritInt")),
            Self::Spilled(b) => {
                let v = b.to_i128()?;
                i64::try_from(v).ok()
            }
        }
    }

    pub fn to_u64(&self) -> Option<u64> {
        match self {
            Self::Inline(w) => {
                let v = swar_unpack_t32(*w).expect("canonical Inline TritInt");
                u64::try_from(v).ok()
            }
            Self::Spilled(b) => {
                let v = b.to_u128()?;
                u64::try_from(v).ok()
            }
        }
    }

    pub fn to_i128(&self) -> Option<i128> {
        match self {
            Self::Inline(w) => Some(swar_unpack_t32(*w).expect("canonical Inline TritInt") as i128),
            Self::Spilled(b) => b.to_i128(),
        }
    }

    pub fn to_u128(&self) -> Option<u128> {
        match self {
            Self::Inline(w) => {
                let v = swar_unpack_t32(*w).expect("canonical Inline TritInt");
                u128::try_from(v).ok()
            }
            Self::Spilled(b) => b.to_u128(),
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
        // Fast path for `<= 14` digits (always fits inside `T32_LIMIT = 926_510_094_425_920`):
        let trimmed = digits.trim_start_matches('0');
        if trimmed.is_empty() {
            return Ok(Self::ZERO);
        }
        if trimmed.len() <= 14 {
            let mag: i64 = trimmed
                .parse()
                .map_err(|_| CoreError::DomainError(format!("invalid integer literal: {s}")))?;
            let signed = if neg { -mag } else { mag };
            return Ok(Self::Inline(fast_pack_t32(signed)));
        }
        let mut acc = BigTrit::zero();
        let mut idx = 0usize;
        while idx < trimmed.len() {
            let chunk_len = (trimmed.len() - idx).min(9);
            let chunk_val: i64 = trimmed[idx..idx + chunk_len]
                .parse()
                .map_err(|_| CoreError::DomainError(format!("invalid integer chunk: {s}")))?;
            let mul_base = 10i64.pow(chunk_len as u32);
            acc = acc.mul_add_small(mul_base, chunk_val);
            idx += chunk_len;
        }
        if neg {
            acc = acc.neg();
        }
        Ok(Self::from_big_trit(acc))
    }

    /// Parse integer literal allowing internal `_` separators.
    pub fn parse_decimal_underscored(s: &str) -> CoreResult<Self> {
        if s.is_empty() {
            return Err(CoreError::DomainError("empty integer literal".to_string()));
        }
        let cleaned: String = s.chars().filter(|&c| c != '_').collect();
        Self::parse_decimal(&cleaned)
    }

    pub fn to_decimal_string(&self) -> String {
        match self {
            Self::Inline(0) => "0".to_string(),
            Self::Inline(w) => swar_unpack_t32(*w)
                .expect("canonical Inline TritInt")
                .to_string(),
            Self::Spilled(b) => {
                let neg = b.signum() < 0;
                let mut cur = b.abs();
                let mut chunks: Vec<i64> = Vec::new();
                while !cur.is_zero() {
                    let (q, rem) = cur.div_rem_small_pos(1_000_000_000);
                    chunks.push(rem);
                    cur = q;
                }
                let mut out = String::with_capacity(chunks.len() * 9 + 1);
                if neg {
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
        }
    }

    pub fn pow(&self, mut exp: u32) -> Self {
        let mut result = Self::ONE;
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
    /// `|r| < |rhs|`, and `r` carrying the sign of `self` (or zero).
    pub fn div_rem_trunc(&self, rhs: &Self) -> CoreResult<(Self, Self)> {
        if rhs.is_zero() {
            return Err(CoreError::ZeroDivision);
        }
        if self.is_zero() {
            return Ok((Self::ZERO, Self::ZERO));
        }
        if let (Self::Inline(wa), Self::Inline(wb)) = (self, rhs) {
            let a = swar_unpack_t32(*wa).expect("canonical Inline TritInt");
            let b = swar_unpack_t32(*wb).expect("canonical Inline TritInt");
            let q = a / b;
            let r = a % b;
            return Ok((Self::Inline(fast_pack_t32(q)), Self::Inline(fast_pack_t32(r))));
        }
        let ua = self.to_big_trit().abs();
        let vb = rhs.to_big_trit().abs();
        let (mut q, mut r) = BigTrit::div_rem_pos(&ua, &vb);
        if self.is_negative() != rhs.is_negative() {
            q = q.neg();
        }
        if self.is_negative() {
            r = r.neg();
        }
        Ok((Self::from_big_trit(q), Self::from_big_trit(r)))
    }

    /// Exact floor division (`self == q * rhs + r`, `r == 0` or `r.signum() == rhs.signum()`).
    pub fn div_rem_floor(&self, rhs: &Self) -> CoreResult<(Self, Self)> {
        let (mut q, mut r) = self.div_rem_trunc(rhs)?;
        if !r.is_zero() && (r.is_negative() != rhs.is_negative()) {
            q = &q - &Self::ONE;
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

    /// Exact non-negative GCD using Division-Free Balanced-Ternary Stein GCD!
    pub fn gcd(&self, other: &Self) -> Self {
        if self.is_zero() {
            return other.abs();
        }
        if other.is_zero() {
            return self.abs();
        }
        if let (Self::Inline(wa), Self::Inline(wb)) = (self, other) {
            // Inline fast ternary Stein GCD directly on `u64` 2-bit-trit words!
            let mut a = if swar_signum(*wa) < 0 { swar_neg(*wa) } else { *wa };
            let mut b = if swar_signum(*wb) < 0 { swar_neg(*wb) } else { *wb };
            let ka = (a.trailing_zeros() >> 1).min(32);
            let kb = (b.trailing_zeros() >> 1).min(32);
            let k_common = ka.min(kb);
            a >>= 2 * ka;
            b >>= 2 * kb;
            let mut va = swar_unpack_t32(a).expect("canonical Inline TritInt");
            let mut vb = swar_unpack_t32(b).expect("canonical Inline TritInt");
            loop {
                if va == vb {
                    let g_shifted = fast_pack_t32(va) << (2 * k_common);
                    return Self::Inline(g_shifted);
                }
                if va < vb {
                    std::mem::swap(&mut va, &mut vb);
                    std::mem::swap(&mut a, &mut b);
                }
                let next_val = if (a & 3) == (b & 3) { va - vb } else { va + vb };
                let mut next_w = fast_pack_t32(next_val.abs());
                let tz = next_w.trailing_zeros() >> 1;
                next_w >>= 2 * tz;
                a = next_w;
                va = swar_unpack_t32(a).expect("canonical Inline TritInt");
            }
        }
        Self::from_big_trit(self.to_big_trit().gcd_ternary_stein(&other.to_big_trit()))
    }

    pub fn lcm(&self, other: &Self) -> Self {
        if self.is_zero() || other.is_zero() {
            return Self::ZERO;
        }
        let g = self.gcd(other);
        let (q, _) = self.abs().div_rem_trunc(&g).expect("gcd of nonzeros is nonzero");
        &q * &other.abs()
    }

    /// Exact power-of-2 scale `self * 2^shift` for non-negative hypercube index math (`s2_kernel`).
    pub fn shl(&self, shift: u32) -> CoreResult<Self> {
        if self.is_negative() {
            return Err(CoreError::DomainError(
                "left shift requires non-negative integer operand".to_string(),
            ));
        }
        if self.is_zero() || shift == 0 {
            return Ok(self.clone());
        }
        let factor = Self::from_u64(2).pow(shift);
        Ok(self * &factor)
    }

    /// Exact floor `self // 2^shift` for non-negative hypercube index math (`s2_kernel`).
    pub fn shr(&self, shift: u32) -> CoreResult<Self> {
        if self.is_negative() {
            return Err(CoreError::DomainError(
                "right shift requires non-negative integer operand".to_string(),
            ));
        }
        if self.is_zero() || shift == 0 {
            return Ok(self.clone());
        }
        let divisor = Self::from_u64(2).pow(shift);
        self.div_floor(&divisor)
    }

    fn to_little_endian_u32_bits(&self) -> CoreResult<Vec<u32>> {
        if self.is_negative() {
            return Err(CoreError::DomainError(
                "binary bit extraction requires non-negative integer operand".to_string(),
            ));
        }
        if let Some(v) = self.to_u128() {
            let mut out = Vec::with_capacity(4);
            let mut cur = v;
            while cur != 0 {
                out.push(cur as u32);
                cur >>= 32;
            }
            return Ok(out);
        }
        let mut cur = self.to_big_trit();
        let mut out = Vec::new();
        let base_2_32 = 1i64 << 32;
        while !cur.is_zero() {
            let (q, r) = cur.div_rem_small_pos(base_2_32);
            out.push(r as u32);
            cur = q;
        }
        Ok(out)
    }

    fn from_little_endian_u32_bits(words: &[u32]) -> Self {
        let mut acc = BigTrit::zero();
        let base_2_32 = 1i64 << 32;
        for &w in words.iter().rev() {
            acc = acc.mul_add_small(base_2_32, w as i64);
        }
        Self::from_big_trit(acc)
    }

    pub fn bitand(&self, rhs: &Self) -> CoreResult<Self> {
        let a = self.to_little_endian_u32_bits()?;
        let b = rhs.to_little_endian_u32_bits()?;
        let len = a.len().min(b.len());
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            out.push(a[i] & b[i]);
        }
        Ok(Self::from_little_endian_u32_bits(&out))
    }

    pub fn bitor(&self, rhs: &Self) -> CoreResult<Self> {
        let a = self.to_little_endian_u32_bits()?;
        let b = rhs.to_little_endian_u32_bits()?;
        let len = a.len().max(b.len());
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            out.push(*a.get(i).unwrap_or(&0) | *b.get(i).unwrap_or(&0));
        }
        Ok(Self::from_little_endian_u32_bits(&out))
    }

    pub fn bitxor(&self, rhs: &Self) -> CoreResult<Self> {
        let a = self.to_little_endian_u32_bits()?;
        let b = rhs.to_little_endian_u32_bits()?;
        let len = a.len().max(b.len());
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            out.push(*a.get(i).unwrap_or(&0) ^ *b.get(i).unwrap_or(&0));
        }
        Ok(Self::from_little_endian_u32_bits(&out))
    }

    /// Lane-wise Complete-State balanced-ternary superposition (`+1` and `-1` cancel to `0`).
    pub fn superpose_cancel(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Inline(wa), Self::Inline(wb)) => {
                Self::Inline(swar_superpose_complete(*wa, *wb).0)
            }
            _ => {
                let a = self.to_big_trit();
                let b = other.to_big_trit();
                let len = a.limbs.len().max(b.limbs.len());
                let mut limbs = Vec::with_capacity(len);
                for i in 0..len {
                    let la = a.limbs.get(i).copied().unwrap_or(0);
                    let lb = b.limbs.get(i).copied().unwrap_or(0);
                    limbs.push(swar_superpose_complete(la, lb).0);
                }
                while limbs.last() == Some(&0) {
                    limbs.pop();
                }
                Self::from_big_trit(BigTrit { limbs })
            }
        }
    }
}

impl Ord for TritInt {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Inline(a), Self::Inline(b)) => swar_cmp(*a, *b),
            (Self::Inline(_), Self::Spilled(b)) => {
                // `|self| <= T32_LIMIT < |other|`, so `other`'s sign strictly determines order!
                if b.signum() > 0 {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            }
            (Self::Spilled(a), Self::Inline(_)) => {
                if a.signum() > 0 {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            }
            (Self::Spilled(a), Self::Spilled(b)) => a.cmp_val(b),
        }
    }
}

impl PartialOrd for TritInt {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Neg for &TritInt {
    type Output = TritInt;
    #[inline]
    fn neg(self) -> TritInt {
        match self {
            TritInt::Inline(w) => TritInt::Inline(swar_neg(*w)),
            TritInt::Spilled(b) => TritInt::Spilled(Box::new(b.neg())),
        }
    }
}

impl Neg for TritInt {
    type Output = TritInt;
    #[inline]
    fn neg(self) -> TritInt {
        -&self
    }
}

impl Add for &TritInt {
    type Output = TritInt;
    fn add(self, rhs: &TritInt) -> TritInt {
        if let (TritInt::Inline(wa), TritInt::Inline(wb)) = (self, rhs) {
            if *wa == 0 {
                return rhs.clone();
            }
            if *wb == 0 {
                return self.clone();
            }
            let sum = swar_unpack_t32(*wa).expect("canonical Inline TritInt")
                + swar_unpack_t32(*wb).expect("canonical Inline TritInt");
            return TritInt::from_i64(sum);
        }
        TritInt::from_big_trit(self.to_big_trit().add(&rhs.to_big_trit()))
    }
}

impl Add for TritInt {
    type Output = TritInt;
    fn add(self, rhs: TritInt) -> TritInt {
        &self + &rhs
    }
}

impl Sub for &TritInt {
    type Output = TritInt;
    fn sub(self, rhs: &TritInt) -> TritInt {
        if let (TritInt::Inline(wa), TritInt::Inline(wb)) = (self, rhs) {
            if *wb == 0 {
                return self.clone();
            }
            let diff = swar_unpack_t32(*wa).expect("canonical Inline TritInt")
                - swar_unpack_t32(*wb).expect("canonical Inline TritInt");
            return TritInt::from_i64(diff);
        }
        TritInt::from_big_trit(self.to_big_trit().sub(&rhs.to_big_trit()))
    }
}

impl Sub for TritInt {
    type Output = TritInt;
    fn sub(self, rhs: TritInt) -> TritInt {
        &self - &rhs
    }
}

impl Mul for &TritInt {
    type Output = TritInt;
    fn mul(self, rhs: &TritInt) -> TritInt {
        if self.is_zero() || rhs.is_zero() {
            return TritInt::ZERO;
        }
        if let (TritInt::Inline(wa), TritInt::Inline(wb)) = (self, rhs) {
            let a = swar_unpack_t32(*wa).expect("canonical Inline TritInt") as i128;
            let b = swar_unpack_t32(*wb).expect("canonical Inline TritInt") as i128;
            return TritInt::from_i128(a * b);
        }
        TritInt::from_big_trit(self.to_big_trit().mul(&rhs.to_big_trit()))
    }
}

impl Mul for TritInt {
    type Output = TritInt;
    fn mul(self, rhs: TritInt) -> TritInt {
        &self * &rhs
    }
}

impl From<i64> for TritInt {
    fn from(v: i64) -> Self {
        Self::from_i64(v)
    }
}

impl From<u64> for TritInt {
    fn from(v: u64) -> Self {
        Self::from_u64(v)
    }
}

impl From<i128> for TritInt {
    fn from(v: i128) -> Self {
        Self::from_i128(v)
    }
}

impl From<u128> for TritInt {
    fn from(v: u128) -> Self {
        Self::from_u128(v)
    }
}

impl From<i32> for TritInt {
    fn from(v: i32) -> Self {
        Self::from_i64(v as i64)
    }
}

impl From<u32> for TritInt {
    fn from(v: u32) -> Self {
        Self::from_u64(v as u64)
    }
}

impl From<usize> for TritInt {
    fn from(v: usize) -> Self {
        Self::from_u64(v as u64)
    }
}

impl FromStr for TritInt {
    type Err = CoreError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_decimal(s)
    }
}

impl fmt::Display for TritInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_decimal_string())
    }
}
