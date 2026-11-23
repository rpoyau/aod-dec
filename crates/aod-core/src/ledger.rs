use crate::bigint::BigInt;
use crate::error::{CoreError, CoreResult};
use crate::ratio::Ratio;
use crate::ternary::{Q4, T1};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum RouteStatus {
    Outgoing,
    Returned,
    Hinge,
    Blocked,
    Slide,
    Pending,
}

/// Manual §4 D.E.C. Ledger Row (`manual:dec-ledger:row-format`).
///
/// Always enforces `node_v.is_hamming1(neighbor_v_e)` (`eq:atomic-h1`) and `weight >= 0`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct LedgerRow {
    pub scope_b: String,
    pub node_v: Q4,
    pub edge_slot: String,
    pub neighbor_v_e: Q4,
    pub sigma: T1,
    pub admissible: bool,
    pub weight: BigInt,
    pub probability: Ratio,
    pub route: RouteStatus,
}

impl LedgerRow {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        scope_b: impl Into<String>,
        node_v: Q4,
        edge_slot: impl Into<String>,
        neighbor_v_e: Q4,
        sigma: T1,
        admissible: bool,
        weight: BigInt,
        route: RouteStatus,
    ) -> CoreResult<Self> {
        let scope_b = scope_b.into();
        let edge_slot = edge_slot.into();
        if scope_b.trim().is_empty() {
            return Err(CoreError::ValidationError(
                "ledger scope B must be nonempty".to_string(),
            ));
        }
        if edge_slot.trim().is_empty() {
            return Err(CoreError::ValidationError(
                "ledger edge slot must be nonempty".to_string(),
            ));
        }
        if !node_v.is_hamming1(neighbor_v_e) {
            return Err(CoreError::HammingViolation(format!(
                "ledger neighbor {} is not Hamming-1 adjacent to node {}",
                neighbor_v_e, node_v
            )));
        }
        if weight.is_negative() {
            return Err(CoreError::DomainError(format!(
                "ledger weight must be non-negative, got {weight}"
            )));
        }
        Ok(Self {
            scope_b,
            node_v,
            edge_slot,
            neighbor_v_e,
            sigma,
            admissible,
            weight,
            probability: Ratio::zero(),
            route,
        })
    }
}

/// Normalize a non-empty sequence of non-negative exact `BigInt` weights with positive total.
pub fn normalize_weights(weights: &[BigInt]) -> CoreResult<Vec<Ratio>> {
    if weights.is_empty() {
        return Err(CoreError::DomainError(
            "weights must be a nonempty sequence".to_string(),
        ));
    }
    let mut total = BigInt::zero();
    for w in weights {
        if w.is_negative() {
            return Err(CoreError::DomainError(
                "weights must be exact and nonnegative".to_string(),
            ));
        }
        total = &total + w;
    }
    if !total.is_positive() {
        return Err(CoreError::DomainError(
            "normalization requires positive total weight".to_string(),
        ));
    }
    let mut out = Vec::with_capacity(weights.len());
    for w in weights {
        out.push(Ratio::new(w.clone(), total.clone())?);
    }
    Ok(out)
}

/// Exact ternary continuation probability matching v1 `dec_core.probability(neg, hinge, pos, role)`.
pub fn probability(
    negative: &BigInt,
    hinge_weight: &BigInt,
    positive: &BigInt,
    role: T1,
) -> CoreResult<Ratio> {
    let weights = normalize_weights(&[negative.clone(), hinge_weight.clone(), positive.clone()])?;
    let idx = (role.value() + 1) as usize;
    Ok(weights[idx].clone())
}

/// Exact kernel normalization over D.E.C. ledger rows (`manual:dec-ledger:kernel`).
pub struct Kernel;

impl Kernel {
    /// Normalize admissible rows so `sum_e P(e; B) == 1/1` and `adm(e; B) == 0 => P(e; B) == 0/1`.
    pub fn normalize(rows: &[LedgerRow]) -> CoreResult<Vec<LedgerRow>> {
        if rows.is_empty() {
            return Err(CoreError::ValidationError(
                "kernel normalization requires nonempty ledger rows".to_string(),
            ));
        }
        let mut total = BigInt::zero();
        for row in rows {
            if !row.node_v.is_hamming1(row.neighbor_v_e) {
                return Err(CoreError::HammingViolation(format!(
                    "row {} violates Hamming-1 adjacency",
                    row.edge_slot
                )));
            }
            if row.weight.is_negative() {
                return Err(CoreError::DomainError(format!(
                    "negative weight on edge {}",
                    row.edge_slot
                )));
            }
            if row.admissible {
                total = &total + &row.weight;
            }
        }
        if !total.is_positive() {
            return Err(CoreError::DomainError(
                "kernel normalization requires positive total admissible weight".to_string(),
            ));
        }

        let mut normalized = Vec::with_capacity(rows.len());
        let mut prob_sum = Ratio::zero();
        for row in rows {
            let mut copy = row.clone();
            if copy.admissible {
                copy.probability = Ratio::new(copy.weight.clone(), total.clone())?;
                prob_sum = &prob_sum + &copy.probability;
            } else {
                copy.probability = Ratio::zero();
            }
            normalized.push(copy);
        }
        if prob_sum != Ratio::one() {
            return Err(CoreError::ValidationError(
                "normalized kernel probability sum did not equal 1/1".to_string(),
            ));
        }
        Ok(normalized)
    }
}

/// Result of blocking same-resolution dwell `(e_slot, 0_D)` and sliding onto the branch pool.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HingeSlideResult {
    pub blocked_dwell_slot: String,
    pub structural_hinge_0h_retained: bool,
    pub blocked_prior_rows: Vec<LedgerRow>,
    pub slide_rows: Vec<LedgerRow>,
    pub p_returned: Ratio,
    pub p_outgoing: Ratio,
}

/// Manual §4 blocked dwell & hinge slide (`manual:dec-ledger:hinge-slide`, `eq:hinge-saturation`).
pub struct HingeSlide;

impl HingeSlide {
    /// Block `(blocked_dwell_slot, 0_D)` only (retaining structural `0_H`), validate that the
    /// post-block slide pool contains NO `sigma == 0` entries, and renormalize the slide pool.
    pub fn block_dwell_and_slide(
        prior_rows: &[LedgerRow],
        blocked_dwell_slot: &str,
        slide_pool: &[LedgerRow],
    ) -> CoreResult<HingeSlideResult> {
        if slide_pool.is_empty() {
            return Err(CoreError::ValidationError(
                "post-block slide pool must be nonempty".to_string(),
            ));
        }
        let mut found_dwell = false;
        let mut blocked_prior_rows = Vec::with_capacity(prior_rows.len());
        for row in prior_rows {
            let mut updated = row.clone();
            if updated.edge_slot == blocked_dwell_slot && updated.sigma == T1::ZERO {
                updated.admissible = false;
                updated.probability = Ratio::zero();
                updated.route = RouteStatus::Blocked;
                found_dwell = true;
            }
            blocked_prior_rows.push(updated);
        }
        if !found_dwell {
            return Err(CoreError::ValidationError(format!(
                "no dwell row ({blocked_dwell_slot}, 0_D) found in prior ledger"
            )));
        }

        for row in slide_pool {
            if row.sigma == T1::ZERO {
                return Err(CoreError::ValidationError(format!(
                    "post-block slide pool rejects sigma == 0 on edge {}",
                    row.edge_slot
                )));
            }
        }

        let slide_rows = Kernel::normalize(slide_pool)?;
        let mut p_returned = Ratio::zero();
        let mut p_outgoing = Ratio::zero();
        for row in &slide_rows {
            if !row.admissible {
                continue;
            }
            match row.sigma.value() {
                -1 => p_returned = &p_returned + &row.probability,
                1 => p_outgoing = &p_outgoing + &row.probability,
                _ => unreachable!("sigma == 0 rejected above"),
            }
        }

        Ok(HingeSlideResult {
            blocked_dwell_slot: blocked_dwell_slot.to_string(),
            structural_hinge_0h_retained: true,
            blocked_prior_rows,
            slide_rows,
            p_returned,
            p_outgoing,
        })
    }
}

/// Per-branch input for Manual §4 field tunnelling hinge-slide window clip.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnelBranchInput {
    pub edge_slot: String,
    pub sigma: T1,
    pub rd: BigInt,
    pub window_len: BigInt,
    pub c3: BigInt,
    pub attention: Option<Ratio>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TunnelClipResult {
    pub t_tunnel: Ratio,
    pub phi_tunnel: Option<Ratio>,
    pub per_branch_pressure: Vec<(String, BigInt)>,
}

/// Manual §4 field tunnelling: hinge-slide window clip (`manual:dec-ledger:field-tunnelling`).
pub struct TunnelClip;

impl TunnelClip {
    /// Evaluate scalar tunnel contribution `T^D_tunnel = sum P_slide(s) * p^D_s` (and optional
    /// oriented tunnel flux `Phi^D_tunnel = sum P_slide(s) * p^D_s * A_s`) over either a single
    /// selected tunnel branch (e.g. `(e_3, +1)` -> `6/7`) or a multi-branch subset of `slide_rows`.
    pub fn evaluate(
        slide_rows: &[LedgerRow],
        selected_branches: &[TunnelBranchInput],
    ) -> CoreResult<TunnelClipResult> {
        if selected_branches.is_empty() {
            return Err(CoreError::ValidationError(
                "tunnel clip requires at least one declared branch".to_string(),
            ));
        }
        let mut t_tunnel = Ratio::zero();
        let mut all_have_attention = true;
        let mut phi_acc = Ratio::zero();
        let mut per_branch_pressure = Vec::with_capacity(selected_branches.len());

        for branch in selected_branches {
            if branch.rd.is_negative() || branch.window_len.is_negative() || branch.c3.is_negative() {
                return Err(CoreError::DomainError(
                    "tunnel clip requires non-negative RD, window_len, and C_3".to_string(),
                ));
            }
            let matched = slide_rows
                .iter()
                .find(|r| r.edge_slot == branch.edge_slot && r.sigma == branch.sigma && r.admissible)
                .ok_or_else(|| {
                    CoreError::ValidationError(format!(
                        "selected tunnel branch ({}, {}) not found as admissible in slide_rows",
                        branch.edge_slot, branch.sigma
                    ))
                })?;

            let rho = if branch.rd < branch.window_len {
                branch.rd.clone()
            } else {
                branch.window_len.clone()
            };
            let p_d = &branch.c3 * &rho;
            let contrib = &matched.probability * &Ratio::from_int(p_d.clone());
            t_tunnel = &t_tunnel + &contrib;
            if let Some(att) = &branch.attention {
                phi_acc = &phi_acc + &(&contrib * att);
            } else {
                all_have_attention = false;
            }
            per_branch_pressure.push((branch.edge_slot.clone(), p_d));
        }

        Ok(TunnelClipResult {
            t_tunnel,
            phi_tunnel: if all_have_attention {
                Some(phi_acc)
            } else {
                None
            },
            per_branch_pressure,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SadarResult {
    pub p_d: BigInt,
    pub attention: Ratio,
    pub sadar: Ratio,
    pub reverse_attention: Ratio,
    pub reverse_sadar: Ratio,
}

/// Manual §4 D.E.C. row-pair promotion to oriented ADAR & SADAR (`manual:dec-ledger:adar-sadar`,
/// `eq:sadar-orientation-antisymmetry`).
pub struct SadarPromotion;

impl SadarPromotion {
    pub fn promote(c3: &BigInt, rho_omega: &BigInt, attention: Ratio) -> CoreResult<SadarResult> {
        if c3.is_negative() || rho_omega.is_negative() {
            return Err(CoreError::DomainError(
                "SADAR promotion requires non-negative C_3 and rho_omega".to_string(),
            ));
        }
        let p_d = c3 * rho_omega;
        let p_ratio = Ratio::from_int(p_d.clone());
        let sadar = &p_ratio * &attention;
        let reverse_attention = -&attention;
        let reverse_sadar = &p_ratio * &reverse_attention;
        debug_assert_eq!(&sadar + &reverse_sadar, Ratio::zero());
        Ok(SadarResult {
            p_d,
            attention,
            sadar,
            reverse_attention,
            reverse_sadar,
        })
    }
}
