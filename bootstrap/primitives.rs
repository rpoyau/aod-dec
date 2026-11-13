//! Checked finite encodings for the optional native demonstrations.
//! These types are representations, not AOD closure certificates.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Q4(u8);
impl Q4 {
    pub fn new(value: u8) -> Result<Self, &'static str> {
        if value < 16 { Ok(Self(value)) } else { Err("Q4 vertex must be in 0..16") }
    }
    pub fn value(self) -> u8 { self.0 }
    pub fn is_hamming1(self, next: Self) -> bool { (self.0 ^ next.0).count_ones() == 1 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct T32(u64);
impl T32 {
    pub const MAX: i64 = (3_i64.pow(32) - 1) / 2;
    pub fn from_bits(bits: u64) -> Result<Self, &'static str> {
        if bits & (bits >> 1) & 0x5555_5555_5555_5555 != 0 {
            Err("11 is not a balanced trit encoding")
        } else { Ok(Self(bits)) }
    }
    pub fn from_i64(mut value: i64) -> Result<Self, &'static str> {
        if !(-Self::MAX..=Self::MAX).contains(&value) { return Err("T32 overflow"); }
        let mut bits = 0;
        for lane in 0..32 {
            let rem = value.rem_euclid(3);
            let trit = if rem == 2 { -1 } else { rem };
            bits |= match trit { 1 => 1, -1 => 2, _ => 0 } << (lane * 2);
            value = (value - trit) / 3;
        }
        Ok(Self(bits))
    }
    pub fn bits(self) -> u64 { self.0 }
    pub fn value(self) -> i64 {
        let mut result = 0;
        let mut place = 1;
        for lane in 0..32 {
            result += match (self.0 >> (lane * 2)) & 3 { 1 => place, 2 => -place, _ => 0 };
            place *= 3;
        }
        result
    }
    pub fn neg(self) -> Self {
        Self(((self.0 & 0x5555_5555_5555_5555) << 1) | ((self.0 & 0xAAAA_AAAA_AAAA_AAAA) >> 1))
    }
    /// Numeric balanced-ternary addition, including carry and checked range.
    /// Event lineage is retained by the AOD runtime, not by this aggregate value.
    pub fn add(self, other: Self) -> Result<Self, &'static str> {
        Self::from_i64(self.value() + other.value())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ratio { num: i64, den: i64 }
impl Ratio {
    pub fn reduced(num: i64, den: i64) -> Result<Self, &'static str> {
        Self::from_wide(num as i128, den as i128)
    }
    fn from_wide(mut num: i128, mut den: i128) -> Result<Self, &'static str> {
        if den == 0 { return Err("zero denominator"); }
        let mut a = num.unsigned_abs(); let mut b = den.unsigned_abs();
        while b != 0 { let rem = a % b; a = b; b = rem; }
        let g = a as i128;
        num /= g; den /= g;
        if den < 0 { num = -num; den = -den; }
        if num < i64::MIN as i128 || num > i64::MAX as i128 || den > i64::MAX as i128 {
            return Err("reduced ratio exceeds i64 storage");
        }
        Ok(Self { num: num as i64, den: den as i64 })
    }
    pub fn numerator(self) -> i64 { self.num }
    pub fn denominator(self) -> i64 { self.den }
    pub fn add(self, other: Self) -> Result<Self, &'static str> {
        Self::from_wide(self.num as i128 * other.den as i128 + other.num as i128 * self.den as i128,
                        self.den as i128 * other.den as i128)
    }
}
