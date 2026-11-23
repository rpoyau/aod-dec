use std::fmt;

use crate::bigint::BigInt;
use crate::error::{CoreError, CoreResult};

const MAX_SUPPORT_EXPONENT: u32 = 10_000;

/// Proposed AOD curling-support specification `p:q:len` or `p:q:len_yro`
/// (`eq:curling-support-model`, `eq:key-proposed-admitted-trace`, `eq:odd-support-index`).
///
/// Explicitly NOT a closure certificate: `SupportForm` records a proposed support-model
/// specification and finite exact `BigInt` accessors (`Pi`, `RD`, `rho_window`, `P^D`, `Q^D`,
/// `X_shedding`). Admission always requires an independently witnessed closure trace.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SupportForm {
    p: BigInt,
    q: BigInt,
    len: BigInt,
    is_yro: bool,
}

/// Exact shedding residual & surplus audit (`eq:closure-residual`, `eq:shedding-excess`).
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SheddingAudit {
    pub p_d: BigInt,
    pub c_close: BigInt,
    pub delta_close: BigInt,
    pub x_shedding: BigInt,
}

impl SupportForm {
    pub fn new(p: BigInt, q: BigInt, len: BigInt, is_yro: bool) -> CoreResult<Self> {
        let one = BigInt::one();
        if p < one {
            return Err(CoreError::InvalidSupportForm(format!(
                "support exponent p must be >= 1, got {p}"
            )));
        }
        if q < one {
            return Err(CoreError::InvalidSupportForm(format!(
                "support base q must be >= 1, got {q}"
            )));
        }
        if len < one {
            return Err(CoreError::InvalidSupportForm(format!(
                "support length len must be >= 1, got {len}"
            )));
        }
        let even = len.is_even();
        if !even && !is_yro {
            return Err(CoreError::OddSupportRequiresYro(format!("{p}:{q}:{len}")));
        }
        if even && is_yro {
            return Err(CoreError::EvenSupportForbidsYro(format!(
                "{p}:{q}:{len}_yro"
            )));
        }
        Ok(Self { p, q, len, is_yro })
    }

    pub fn from_u64(p: u64, q: u64, len: u64, is_yro: bool) -> CoreResult<Self> {
        Self::new(
            BigInt::from_u64(p),
            BigInt::from_u64(q),
            BigInt::from_u64(len),
            is_yro,
        )
    }

    /// Parse `p:q:len` or `p:q:len_yro` with strict odd-length `_yro` enforcement.
    pub fn parse(s: &str) -> CoreResult<Self> {
        let (core, is_yro) = match s.strip_suffix("_yro") {
            Some(stripped) => (stripped, true),
            None => (s, false),
        };
        let parts: Vec<&str> = core.split(':').collect();
        if parts.len() != 3 || parts.iter().any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit())) {
            return Err(CoreError::InvalidSupportForm(format!(
                "expected p:q:len[_yro], got {s:?}"
            )));
        }
        let p = BigInt::parse_decimal(parts[0])?;
        let q = BigInt::parse_decimal(parts[1])?;
        let len = BigInt::parse_decimal(parts[2])?;
        Self::new(p, q, len, is_yro)
    }

    #[inline]
    pub fn p(&self) -> &BigInt {
        &self.p
    }

    #[inline]
    pub fn q(&self) -> &BigInt {
        &self.q
    }

    #[inline]
    pub fn len_index(&self) -> &BigInt {
        &self.len
    }

    #[inline]
    pub fn is_yro(&self) -> bool {
        self.is_yro
    }

    /// Explicit ontology guard (`eq:key-proposed-admitted-trace`): a `SupportForm`
    /// is a proposed support specification, NEVER a closure certificate.
    #[inline]
    pub fn is_closure_certificate(&self) -> bool {
        false
    }

    /// Exact branch support `Pi = q^p + q` (`eq:curling-support-model`).
    ///
    /// Reference checks: `1:2 -> 4`, `3:2 -> 10`, `3:3 -> 30`, `3:4 -> 68`.
    pub fn pi(&self) -> CoreResult<BigInt> {
        let exp = self
            .p
            .to_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| {
                CoreError::Overflow(format!("support exponent p={} exceeds u32", self.p))
            })?;
        if exp > MAX_SUPPORT_EXPONENT {
            return Err(CoreError::Overflow(format!(
                "support exponent p={exp} exceeds safety guard {MAX_SUPPORT_EXPONENT}"
            )));
        }
        Ok(&self.q.pow(exp) + &self.q)
    }

    /// Exact symmetric branch-hinge duration `RD = 2 * Pi + H_mu` (`eq:branch-hinge-rd-symmetric`).
    ///
    /// Requires `h_mu >= 1`. Reference checks with `h_mu = 1`:
    /// `1:2 -> 9`, `3:2 -> 21`, `3:3 -> 61`, `3:4 -> 137`.
    pub fn rd(&self, h_mu: &BigInt) -> CoreResult<BigInt> {
        if *h_mu < BigInt::one() {
            return Err(CoreError::DomainError(format!(
                "hinge multiplicity h_mu must be >= 1, got {h_mu}"
            )));
        }
        let pi = self.pi()?;
        Ok(&(&BigInt::from_u64(2) * &pi) + h_mu)
    }

    /// Window-clipped duration participation `rho^D_omega = min(RD, |omega|)` (`eq:window-clipped-rho`).
    pub fn rho_window(&self, h_mu: &BigInt, window_len: &BigInt) -> CoreResult<BigInt> {
        if window_len.is_negative() {
            return Err(CoreError::DomainError(format!(
                "window length must be >= 0, got {window_len}"
            )));
        }
        let rd = self.rd(h_mu)?;
        Ok(if rd < *window_len { rd } else { window_len.clone() })
    }

    /// Pending duration overhang `Q^{D,dur} = max(0, RD - |omega|)` (`eq:duration-overhang`).
    pub fn duration_overhang(&self, h_mu: &BigInt, window_len: &BigInt) -> CoreResult<BigInt> {
        if window_len.is_negative() {
            return Err(CoreError::DomainError(format!(
                "window length must be >= 0, got {window_len}"
            )));
        }
        let rd = self.rd(h_mu)?;
        if rd > *window_len {
            Ok(&rd - window_len)
        } else {
            Ok(BigInt::zero())
        }
    }

    /// Exact channel/duonic pressure `P^D = C_3 * rho^D_omega` (`eq:b-scoped-channel-pressure`, `eq:duonic-pressure`).
    pub fn pressure(
        &self,
        c3: &BigInt,
        h_mu: &BigInt,
        window_len: &BigInt,
    ) -> CoreResult<BigInt> {
        if c3.is_negative() {
            return Err(CoreError::DomainError(format!(
                "channel capacity C_3 must be >= 0, got {c3}"
            )));
        }
        let rho = self.rho_window(h_mu, window_len)?;
        Ok(c3 * &rho)
    }

    /// Exact pending burden overhang `Q^D = C_3 * max(0, RD - |omega|)` (`eq:burden-overhang`).
    pub fn overhang(
        &self,
        c3: &BigInt,
        h_mu: &BigInt,
        window_len: &BigInt,
    ) -> CoreResult<BigInt> {
        if c3.is_negative() {
            return Err(CoreError::DomainError(format!(
                "channel capacity C_3 must be >= 0, got {c3}"
            )));
        }
        let dur_over = self.duration_overhang(h_mu, window_len)?;
        Ok(c3 * &dur_over)
    }

    /// Exact closure residual & shedding excess audit (`eq:closure-residual`, `eq:shedding-excess`).
    pub fn shedding_audit(p_d: &BigInt, c_close: &BigInt) -> CoreResult<SheddingAudit> {
        if p_d.is_negative() {
            return Err(CoreError::DomainError(format!(
                "pressure P^D must be >= 0, got {p_d}"
            )));
        }
        if c_close.is_negative() {
            return Err(CoreError::DomainError(format!(
                "closure capacity C_close must be >= 0, got {c_close}"
            )));
        }
        let delta_close = p_d - c_close;
        let x_shedding = if delta_close.is_positive() {
            delta_close.clone()
        } else {
            BigInt::zero()
        };
        Ok(SheddingAudit {
            p_d: p_d.clone(),
            c_close: c_close.clone(),
            delta_close,
            x_shedding,
        })
    }

    pub fn render(&self) -> String {
        if self.is_yro {
            format!("{}:{}:{}_yro", self.p, self.q, self.len)
        } else {
            format!("{}:{}:{}", self.p, self.q, self.len)
        }
    }
}

impl fmt::Display for SupportForm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render())
    }
}
