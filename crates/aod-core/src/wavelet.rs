use std::collections::{BTreeMap, BTreeSet};

use crate::bigint::BigInt;
use crate::error::{CoreError, CoreResult};
use crate::json::JsonValue;
use crate::ratio::Ratio;
use crate::ternary::{Q4, T1};
use crate::witness::SeatState;

fn require_nonempty(s: &str, field: &str) -> CoreResult<()> {
    if s.trim().is_empty() {
        return Err(CoreError::ValidationError(format!(
            "{field} must be a non-empty string"
        )));
    }
    Ok(())
}

fn digest_json(v: &JsonValue) -> String {
    v.sha256_hex()
}

fn digest_str_tuple(items: &[String]) -> String {
    let arr = JsonValue::Array(
        items
            .iter()
            .map(|s| JsonValue::from_str_val(s.clone()))
            .collect(),
    );
    digest_json(&arr)
}

// ============================================================================
// M3 Native Closure Trace, Certificate, AdmittedClosureRecord, and M3Wave
// (Cleanly separate from v1 `crate::witness::Trace`.)
// ============================================================================

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ReflectionWitness {
    pub scope: String,
    pub lineage: String,
    pub turnaround_id: String,
    pub outbound_ids: Vec<String>,
    pub return_ids: Vec<String>,
    pub target_start_id: String,
    pub committed_at: i64,
    pub turnaround_committed_at: i64,
}

impl ReflectionWitness {
    pub fn to_json(&self) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert(
            "committed_at".to_string(),
            JsonValue::from_i64(self.committed_at),
        );
        map.insert(
            "lineage".to_string(),
            JsonValue::from_str_val(self.lineage.clone()),
        );
        map.insert(
            "outbound_ids".to_string(),
            JsonValue::Array(
                self.outbound_ids
                    .iter()
                    .map(|s| JsonValue::from_str_val(s.clone()))
                    .collect(),
            ),
        );
        map.insert(
            "return_ids".to_string(),
            JsonValue::Array(
                self.return_ids
                    .iter()
                    .map(|s| JsonValue::from_str_val(s.clone()))
                    .collect(),
            ),
        );
        map.insert(
            "scope".to_string(),
            JsonValue::from_str_val(self.scope.clone()),
        );
        map.insert(
            "target_start_id".to_string(),
            JsonValue::from_str_val(self.target_start_id.clone()),
        );
        map.insert(
            "turnaround_committed_at".to_string(),
            JsonValue::from_i64(self.turnaround_committed_at),
        );
        map.insert(
            "turnaround_id".to_string(),
            JsonValue::from_str_val(self.turnaround_id.clone()),
        );
        JsonValue::Object(map)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct M3ClosureTrace {
    pub scope: String,
    pub scale: u32,
    pub lineage: String,
    pub event_ids: Vec<String>,
    pub vertices: Vec<Q4>,
    pub indices: Vec<i64>,
    pub roles: Vec<T1>,
    pub turn: usize,
    pub ancestor: String,
    pub reflection: Option<ReflectionWitness>,
    pub reflected_comparison: bool,
}

impl M3ClosureTrace {
    /// Construct a valid M3 closed trace mirroring `aod_native_closure_wave.trace(...)`.
    #[allow(clippy::too_many_arguments)]
    pub fn build_closed(
        lineage: impl Into<String>,
        occurrence: i64,
        vertices: &[u8],
        dwell: i64,
        scope: impl Into<String>,
        scale: u32,
        turn: usize,
        start_index: Option<i64>,
        chirality: T1,
    ) -> CoreResult<Self> {
        if dwell < 0 {
            return Err(CoreError::DomainError("dwell must be >= 0".to_string()));
        }
        let lineage = lineage.into();
        let scope = scope.into();
        let start = start_index.unwrap_or(occurrence * 10);
        let q_verts: CoreResult<Vec<Q4>> = vertices.iter().map(|&v| Q4::new(v)).collect();
        let q_verts = q_verts?;
        if q_verts.len() < 3 || turn == 0 || turn >= q_verts.len() - 1 {
            return Err(CoreError::ValidationError(
                "trace requires at least 3 vertices and interior turn".to_string(),
            ));
        }
        let event_ids: Vec<String> = (0..q_verts.len())
            .map(|n| format!("{lineage}:{occurrence}:e{n}"))
            .collect();
        let indices: Vec<i64> = (0..q_verts.len())
            .map(|n| start + (n as i64) + if n > turn { dwell } else { 0 })
            .collect();
        let mut roles = Vec::with_capacity(3 + dwell as usize);
        roles.push(chirality);
        for _ in 0..(1 + dwell as usize) {
            roles.push(T1::ZERO);
        }
        roles.push(chirality.checked_neg());

        let witness = ReflectionWitness {
            scope: scope.clone(),
            lineage: lineage.clone(),
            turnaround_id: event_ids[turn].clone(),
            outbound_ids: event_ids[1..=turn].to_vec(),
            return_ids: event_ids[turn + 1..].to_vec(),
            target_start_id: event_ids[0].clone(),
            committed_at: start + (q_verts.len() as i64) - 1 + dwell,
            turnaround_committed_at: start + (turn as i64),
        };
        let trace = Self {
            scope,
            scale,
            lineage: lineage.clone(),
            event_ids,
            vertices: q_verts,
            indices,
            roles,
            turn,
            ancestor: format!("{lineage}:ancestor"),
            reflection: Some(witness),
            reflected_comparison: true,
        };
        trace.validate(true)?;
        Ok(trace)
    }

    /// Standard minimal 3-vertex H1 return `(0 -> 1 -> 0)`.
    pub fn minimal_closed(
        lineage: impl Into<String>,
        occurrence: i64,
        scope: impl Into<String>,
        scale: u32,
        start_index: Option<i64>,
    ) -> CoreResult<Self> {
        Self::build_closed(
            lineage,
            occurrence,
            &[0, 1, 0],
            0,
            scope,
            scale,
            1,
            start_index,
            T1::POS,
        )
    }

    pub fn validate(&self, require_closed: bool) -> CoreResult<()> {
        require_nonempty(&self.scope, "trace.scope")?;
        require_nonempty(&self.lineage, "trace.lineage")?;
        require_nonempty(&self.ancestor, "trace.ancestor")?;
        if self.vertices.len() < 3 {
            return Err(CoreError::ValidationError(
                "two nonempty legs required".to_string(),
            ));
        }
        if self.vertices.len() != self.event_ids.len() || self.vertices.len() != self.indices.len() {
            return Err(CoreError::ValidationError(
                "trace coordinate length mismatch".to_string(),
            ));
        }
        let unique_events: BTreeSet<&str> = self.event_ids.iter().map(String::as_str).collect();
        if unique_events.len() != self.event_ids.len() {
            return Err(CoreError::ValidationError(
                "event replay inside trace".to_string(),
            ));
        }
        if !self.indices.windows(2).all(|w| w[0] < w[1]) {
            return Err(CoreError::ValidationError(
                "uncommitted or non-increasing event order".to_string(),
            ));
        }
        if !self.vertices.windows(2).all(|w| w[0].is_hamming1(w[1])) {
            return Err(CoreError::HammingViolation(
                "non-H1 atomic traversal".to_string(),
            ));
        }
        if self.roles.len() < 3
            || self.roles[0] == T1::ZERO
            || self.roles[self.roles.len() - 1] != self.roles[0].checked_neg()
            || !self.roles[1..self.roles.len() - 1]
                .iter()
                .all(|&r| r == T1::ZERO)
        {
            return Err(CoreError::ValidationError(
                "mandatory hinge and counterbranch required".to_string(),
            ));
        }
        if self.turn == 0 || self.turn >= self.vertices.len() - 1 {
            return Err(CoreError::ValidationError(
                "empty directed leg".to_string(),
            ));
        }
        let w = self.reflection.as_ref().ok_or_else(|| {
            CoreError::ValidationError("missing bound reflection witness".to_string())
        })?;
        if w.scope != self.scope
            || w.lineage != self.lineage
            || w.turnaround_id != self.event_ids[self.turn]
            || w.target_start_id != self.event_ids[0]
        {
            return Err(CoreError::ValidationError(
                "wrong reflection lineage or hinge".to_string(),
            ));
        }
        if w.outbound_ids != self.event_ids[1..=self.turn]
            || w.return_ids != self.event_ids[self.turn + 1..]
        {
            return Err(CoreError::ValidationError(
                "reflection legs not bound to committed events".to_string(),
            ));
        }
        let last_idx = *self.indices.last().expect("len >= 3");
        if w.turnaround_committed_at != self.indices[self.turn]
            || w.turnaround_committed_at >= last_idx
            || w.committed_at != last_idx
        {
            return Err(CoreError::ValidationError(
                "reflection witness violates causal order".to_string(),
            ));
        }
        if require_closed && self.vertices[0] != *self.vertices.last().expect("len >= 3") {
            return Err(CoreError::ValidationError(
                "end differs from retained start".to_string(),
            ));
        }
        Ok(())
    }

    pub fn start_cut(&self) -> i64 {
        self.indices[0]
    }

    pub fn completion_cut(&self) -> i64 {
        *self.indices.last().unwrap_or(&0)
    }

    pub fn to_json(&self) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert(
            "ancestor".to_string(),
            JsonValue::from_str_val(self.ancestor.clone()),
        );
        map.insert(
            "event_ids".to_string(),
            JsonValue::Array(
                self.event_ids
                    .iter()
                    .map(|s| JsonValue::from_str_val(s.clone()))
                    .collect(),
            ),
        );
        map.insert(
            "indices".to_string(),
            JsonValue::Array(self.indices.iter().map(|&i| JsonValue::from_i64(i)).collect()),
        );
        map.insert(
            "lineage".to_string(),
            JsonValue::from_str_val(self.lineage.clone()),
        );
        map.insert(
            "reflected_comparison".to_string(),
            JsonValue::Bool(self.reflected_comparison),
        );
        map.insert(
            "reflection".to_string(),
            match &self.reflection {
                Some(r) => r.to_json(),
                None => JsonValue::Null,
            },
        );
        map.insert(
            "roles".to_string(),
            JsonValue::Array(
                self.roles
                    .iter()
                    .map(|r| JsonValue::from_i64(r.value() as i64))
                    .collect(),
            ),
        );
        map.insert("scale".to_string(), JsonValue::from_i64(self.scale as i64));
        map.insert(
            "scope".to_string(),
            JsonValue::from_str_val(self.scope.clone()),
        );
        map.insert("turn".to_string(), JsonValue::from_usize(self.turn));
        map.insert(
            "vertices".to_string(),
            JsonValue::Array(
                self.vertices
                    .iter()
                    .map(|v| JsonValue::from_i64(v.value() as i64))
                    .collect(),
            ),
        );
        JsonValue::Object(map)
    }

    pub fn trace_digest(&self) -> String {
        digest_json(&self.to_json())
    }

    pub fn return_id(&self) -> String {
        let arr = JsonValue::Array(vec![
            JsonValue::from_str_val(self.scope.clone()),
            JsonValue::from_i64(self.scale as i64),
            JsonValue::from_str_val(self.lineage.clone()),
            JsonValue::from_str_val(self.event_ids.first().cloned().unwrap_or_default()),
            JsonValue::from_str_val(self.event_ids.last().cloned().unwrap_or_default()),
            JsonValue::from_str_val(self.trace_digest()),
        ]);
        digest_json(&arr)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct M3Certificate {
    pub motif: String,
    pub scope: String,
    pub scale: u32,
    pub lineage: String,
    pub trace_digest: String,
    pub return_id: String,
    pub witness_digest: String,
    pub status: String,
    pub certificate_digest: String,
    pub inner_wave_origins: Vec<(String, String, u32)>,
}

impl M3Certificate {
    pub fn certify(
        trace: &M3ClosureTrace,
        motif: impl Into<String>,
        inner_wave_origins: Vec<(String, String, u32)>,
    ) -> CoreResult<Self> {
        trace.validate(true)?;
        let motif = motif.into();
        if motif != "Monon" && motif != "outer" {
            return Err(CoreError::ValidationError(format!(
                "unsupported certificate motif {motif:?}; expected Monon or outer"
            )));
        }
        if motif != "outer" && !inner_wave_origins.is_empty() {
            return Err(CoreError::ValidationError(
                "primitive certificate has no folded inner construction".to_string(),
            ));
        }
        let refl = trace.reflection.as_ref().expect("validated above");
        let witness_digest = digest_json(&refl.to_json());
        let trace_digest = trace.trace_digest();
        let return_id = trace.return_id();
        let status = "closed".to_string();
        let fields = JsonValue::Array(vec![
            JsonValue::from_str_val(motif.clone()),
            JsonValue::from_str_val(trace.scope.clone()),
            JsonValue::from_i64(trace.scale as i64),
            JsonValue::from_str_val(trace.lineage.clone()),
            JsonValue::from_str_val(trace_digest.clone()),
            JsonValue::from_str_val(return_id.clone()),
            JsonValue::from_str_val(witness_digest.clone()),
            JsonValue::from_str_val(status.clone()),
        ]);
        let certificate_digest = digest_json(&fields);
        Ok(Self {
            motif,
            scope: trace.scope.clone(),
            scale: trace.scale,
            lineage: trace.lineage.clone(),
            trace_digest,
            return_id,
            witness_digest,
            status,
            certificate_digest,
            inner_wave_origins,
        })
    }
}

/// Strongly typed admitted M3 closure record (`(Trace, Certificate)` bound to verified digests).
/// Bare booleans cannot construct or substitute for `AdmittedClosureRecord`.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AdmittedClosureRecord {
    trace: M3ClosureTrace,
    certificate: M3Certificate,
}

impl AdmittedClosureRecord {
    pub fn admit_monon(trace: M3ClosureTrace) -> CoreResult<Self> {
        let cert = M3Certificate::certify(&trace, "Monon", Vec::new())?;
        Ok(Self {
            trace,
            certificate: cert,
        })
    }

    pub fn admit(
        trace: M3ClosureTrace,
        motif: impl Into<String>,
        inner_wave_origins: Vec<(String, String, u32)>,
    ) -> CoreResult<Self> {
        let cert = M3Certificate::certify(&trace, motif, inner_wave_origins)?;
        Ok(Self {
            trace,
            certificate: cert,
        })
    }

    pub fn validate(&self) -> CoreResult<()> {
        if self.certificate.certificate_digest.is_empty() {
            return Err(CoreError::ValidationError("blank certificate".to_string()));
        }
        let recomputed = M3Certificate::certify(
            &self.trace,
            self.certificate.motif.clone(),
            self.certificate.inner_wave_origins.clone(),
        )?;
        if recomputed != self.certificate {
            return Err(CoreError::ValidationError(
                "scope, lineage, trace, witness, or status mismatch in admitted closure record"
                    .to_string(),
            ));
        }
        Ok(())
    }

    pub fn trace(&self) -> &M3ClosureTrace {
        &self.trace
    }

    pub fn certificate(&self) -> &M3Certificate {
        &self.certificate
    }
}

/// Count complete distinct certified M3 closure cycles (`closure_cycle_count`).
pub fn closure_cycle_count(
    records: &[AdmittedClosureRecord],
    cut: Option<i64>,
) -> CoreResult<usize> {
    let mut seen = BTreeSet::new();
    let mut terminal_bindings: BTreeMap<(String, u32, String, String), String> = BTreeMap::new();
    for rec in records {
        rec.validate()?;
        let t = rec.trace();
        let term_ev = t.event_ids.last().cloned().unwrap_or_default();
        let key = (t.scope.clone(), t.scale, t.lineage.clone(), term_ev);
        let t_dig = t.trace_digest();
        if let Some(prev) = terminal_bindings.get(&key) {
            if prev != &t_dig {
                return Err(CoreError::CollisionError(
                    "conflicting trace for immutable terminal return event".to_string(),
                ));
            }
        } else {
            terminal_bindings.insert(key, t_dig);
        }
        if cut.is_none_or(|k| t.completion_cut() <= k) {
            seen.insert(t.return_id());
        }
    }
    Ok(seen.len())
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct M3Wave {
    pub scope: String,
    pub scale: u32,
    pub lineage: String,
    pub motif: String,
    pub identity: String,
    pub records: Vec<AdmittedClosureRecord>,
    pub motif_core: Option<String>,
}

impl M3Wave {
    pub fn new(
        records: Vec<AdmittedClosureRecord>,
        motif_core: Option<String>,
    ) -> CoreResult<Self> {
        if records.len() < 2 {
            return Err(CoreError::ValidationError(
                "one closure supplies no recurrence (M3Wave requires >= 2 admitted returns)"
                    .to_string(),
            ));
        }
        let _ = closure_cycle_count(&records, None)?;
        let rids: BTreeSet<String> = records.iter().map(|r| r.trace().return_id()).collect();
        if rids.len() != records.len() {
            return Err(CoreError::ValidationError(
                "replayed return is not a beat".to_string(),
            ));
        }
        let first_t = records[0].trace();
        let first_c = records[0].certificate();
        for rec in &records[1..] {
            let t = rec.trace();
            let c = rec.certificate();
            if (t.scope.as_str(), t.scale, t.lineage.as_str(), c.motif.as_str())
                != (
                    first_t.scope.as_str(),
                    first_t.scale,
                    first_t.lineage.as_str(),
                    first_c.motif.as_str(),
                )
            {
                return Err(CoreError::ValidationError(
                    "incompatible recurrence lineage, motif or scope".to_string(),
                ));
            }
            if first_c.motif == "outer" {
                if c.inner_wave_origins != first_c.inner_wave_origins {
                    return Err(CoreError::ValidationError(
                        "recurrence changed retained outer inner_wave_origins".to_string(),
                    ));
                }
            } else if t.ancestor != first_t.ancestor {
                return Err(CoreError::ValidationError(
                    "recurrence changed retained ancestor identity".to_string(),
                ));
            }
        }
        for pair in records.windows(2) {
            if pair[0].trace().completion_cut() >= pair[1].trace().start_cut() {
                return Err(CoreError::ChronologyError(
                    "recurrence must follow previous completion".to_string(),
                ));
            }
        }
        let eff_motif = match &motif_core {
            Some(core) => {
                require_nonempty(core, "motif_core")?;
                core.clone()
            }
            None => first_c.motif.clone(),
        };
        let identity = compute_wave_identity(
            &first_t.scope,
            &first_t.lineage,
            &eff_motif,
            &first_t.return_id(),
        );
        Ok(Self {
            scope: first_t.scope.clone(),
            scale: first_t.scale,
            lineage: first_t.lineage.clone(),
            motif: eff_motif,
            identity,
            records,
            motif_core,
        })
    }

    /// Build a standard recurrent wave matching `native.recurrent(lineage, n, spacing, scope, scale)`.
    pub fn recurrent(
        lineage: impl Into<String>,
        n: usize,
        spacing: i64,
        scope: impl Into<String>,
        scale: u32,
    ) -> CoreResult<Self> {
        let lineage = lineage.into();
        let scope = scope.into();
        let mut records = Vec::with_capacity(n);
        for k in 0..n {
            let t = M3ClosureTrace::minimal_closed(
                lineage.clone(),
                k as i64,
                scope.clone(),
                scale,
                Some((k as i64) * spacing),
            )?;
            records.push(AdmittedClosureRecord::admit_monon(t)?);
        }
        Self::new(records, None)
    }

    pub fn validate(&self) -> CoreResult<()> {
        let rebuilt = Self::new(self.records.clone(), self.motif_core.clone())?;
        if rebuilt != *self {
            return Err(CoreError::ValidationError(
                "wave identity or certificate history mismatch".to_string(),
            ));
        }
        Ok(())
    }

    pub fn bip(&self) -> usize {
        self.records.len().saturating_sub(1)
    }
}

pub fn compute_wave_identity(
    scope: &str,
    lineage: &str,
    motif: &str,
    origin_return_id: &str,
) -> String {
    let arr = JsonValue::Array(vec![
        JsonValue::from_str_val(scope.to_string()),
        JsonValue::from_str_val(lineage.to_string()),
        JsonValue::from_str_val(motif.to_string()),
        JsonValue::from_str_val(origin_return_id.to_string()),
    ]);
    digest_json(&arr)
}

// ============================================================================
// F04: ParentDescriptor & Canonical ParentRootKey
// ============================================================================

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ParentDescriptorKind {
    AdmittedNativeParent,
    AdmittedOriginClosure,
    DeclaredSyntheticParent,
    MappedConditionalParent,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ParentRootKey {
    pub family: String,
    pub scope_id: String,
    pub scale: u32,
    pub lineage_id: String,
    pub motif: String,
    pub origin_return_id: Option<String>,
    pub origin_ancestor: String,
    pub inner_wave_origins: Vec<(String, String, u32)>,
    pub motif_core: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct WaveletParentKey {
    pub kind: ParentDescriptorKind,
    pub wave_identity: Option<String>,
    pub root_key: ParentRootKey,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ParentDescriptor {
    pub kind: ParentDescriptorKind,
    pub admitted_recurring_wave: bool,
    pub wave_identity: Option<String>,
    pub scope_id: String,
    pub scale: u32,
    pub lineage_id: String,
    pub motif: String,
    pub origin_return_id: Option<String>,
    pub origin_ancestor: String,
    pub inner_wave_origins: Vec<(String, String, u32)>,
    pub motif_core: Option<String>,
    pub source_scope: Option<String>,
    pub scope_map_id: Option<String>,
}

impl ParentDescriptor {
    pub fn describe_admitted_parent(wave: &M3Wave) -> CoreResult<Self> {
        wave.validate()?;
        let t0 = wave.records[0].trace();
        let c0 = wave.records[0].certificate();
        let desc = Self {
            kind: ParentDescriptorKind::AdmittedNativeParent,
            admitted_recurring_wave: true,
            wave_identity: Some(wave.identity.clone()),
            scope_id: wave.scope.clone(),
            scale: wave.scale,
            lineage_id: wave.lineage.clone(),
            motif: wave.motif.clone(),
            origin_return_id: Some(t0.return_id()),
            origin_ancestor: t0.ancestor.clone(),
            inner_wave_origins: c0.inner_wave_origins.clone(),
            motif_core: wave.motif_core.clone(),
            source_scope: None,
            scope_map_id: None,
        };
        desc.validate()?;
        Ok(desc)
    }

    pub fn describe_admitted_origin_closure(
        rec: &AdmittedClosureRecord,
        motif_core: Option<String>,
    ) -> CoreResult<Self> {
        rec.validate()?;
        let t0 = rec.trace();
        let c0 = rec.certificate();
        let eff_motif = motif_core.clone().unwrap_or_else(|| c0.motif.clone());
        let desc = Self {
            kind: ParentDescriptorKind::AdmittedOriginClosure,
            admitted_recurring_wave: false,
            wave_identity: None,
            scope_id: t0.scope.clone(),
            scale: t0.scale,
            lineage_id: t0.lineage.clone(),
            motif: eff_motif,
            origin_return_id: Some(t0.return_id()),
            origin_ancestor: t0.ancestor.clone(),
            inner_wave_origins: c0.inner_wave_origins.clone(),
            motif_core,
            source_scope: None,
            scope_map_id: None,
        };
        desc.validate()?;
        Ok(desc)
    }

    pub fn declare_synthetic_parent(
        lineage_id: impl Into<String>,
        scope_id: impl Into<String>,
        scale: u32,
        motif: impl Into<String>,
        origin_return_id: Option<String>,
        origin_ancestor: Option<String>,
    ) -> CoreResult<Self> {
        let lin = lineage_id.into();
        let scp = scope_id.into();
        let mot = motif.into();
        require_nonempty(&lin, "lineage_id")?;
        require_nonempty(&scp, "scope_id")?;
        require_nonempty(&mot, "motif")?;
        if let Some(ref rid) = origin_return_id {
            require_nonempty(rid, "origin_return_id")?;
        }
        let orig_anc = match origin_ancestor {
            Some(a) => {
                require_nonempty(&a, "origin_ancestor")?;
                a
            }
            None => format!("syn-ancestor:{lin}"),
        };
        let wid = format!(
            "syn-parent:{scp}:s{scale}:{lin}:{mot}:{}",
            origin_return_id.as_deref().unwrap_or("unanchored")
        );
        let desc = Self {
            kind: ParentDescriptorKind::DeclaredSyntheticParent,
            admitted_recurring_wave: false,
            wave_identity: Some(wid),
            scope_id: scp,
            scale,
            lineage_id: lin,
            motif: mot,
            origin_return_id,
            origin_ancestor: orig_anc,
            inner_wave_origins: Vec::new(),
            motif_core: None,
            source_scope: None,
            scope_map_id: None,
        };
        desc.validate()?;
        Ok(desc)
    }

    pub fn validate(&self) -> CoreResult<()> {
        require_nonempty(&self.scope_id, "parent_descriptor.scope_id")?;
        require_nonempty(&self.lineage_id, "parent_descriptor.lineage_id")?;
        require_nonempty(&self.motif, "parent_descriptor.motif")?;
        require_nonempty(&self.origin_ancestor, "parent_descriptor.origin_ancestor")?;
        if let Some(ref mc) = self.motif_core {
            require_nonempty(mc, "parent_descriptor.motif_core")?;
        }

        match self.kind {
            ParentDescriptorKind::AdmittedNativeParent => {
                if !self.admitted_recurring_wave {
                    return Err(CoreError::ValidationError(
                        "admitted_native_parent requires admitted_recurring_wave=true".to_string(),
                    ));
                }
                let orig_rid = self.origin_return_id.as_deref().ok_or_else(|| {
                    CoreError::ValidationError(
                        "admitted_native_parent requires origin_return_id".to_string(),
                    )
                })?;
                require_nonempty(orig_rid, "parent_descriptor.origin_return_id")?;
                let wid = self.wave_identity.as_deref().ok_or_else(|| {
                    CoreError::ValidationError(
                        "admitted_native_parent requires wave_identity".to_string(),
                    )
                })?;
                let expected_wid =
                    compute_wave_identity(&self.scope_id, &self.lineage_id, &self.motif, orig_rid);
                if wid != expected_wid {
                    return Err(CoreError::ValidationError(
                        "admitted_native_parent wave_identity mismatch".to_string(),
                    ));
                }
            }
            ParentDescriptorKind::AdmittedOriginClosure => {
                if self.admitted_recurring_wave || self.wave_identity.is_some() {
                    return Err(CoreError::PromotionForbidden(
                        "admitted_origin_closure cannot claim a recurring wave_identity"
                            .to_string(),
                    ));
                }
                let orig_rid = self.origin_return_id.as_deref().ok_or_else(|| {
                    CoreError::ValidationError(
                        "admitted_origin_closure requires origin_return_id".to_string(),
                    )
                })?;
                require_nonempty(orig_rid, "parent_descriptor.origin_return_id")?;
            }
            ParentDescriptorKind::DeclaredSyntheticParent => {
                if self.admitted_recurring_wave {
                    return Err(CoreError::PromotionForbidden(
                        "declared_synthetic_parent cannot claim authenticated native recurrence"
                            .to_string(),
                    ));
                }
                let wid = self.wave_identity.as_deref().ok_or_else(|| {
                    CoreError::ValidationError(
                        "declared_synthetic_parent requires wave_identity".to_string(),
                    )
                })?;
                let expected = format!(
                    "syn-parent:{}:s{}:{}:{}:{}",
                    self.scope_id,
                    self.scale,
                    self.lineage_id,
                    self.motif,
                    self.origin_return_id.as_deref().unwrap_or("unanchored")
                );
                if wid != expected {
                    return Err(CoreError::ValidationError(
                        "declared_synthetic_parent wave_identity mismatch".to_string(),
                    ));
                }
            }
            ParentDescriptorKind::MappedConditionalParent => {
                let src = self.source_scope.as_deref().ok_or_else(|| {
                    CoreError::ValidationError(
                        "mapped_conditional_parent requires source_scope".to_string(),
                    )
                })?;
                let mid = self.scope_map_id.as_deref().ok_or_else(|| {
                    CoreError::ValidationError(
                        "mapped_conditional_parent requires scope_map_id".to_string(),
                    )
                })?;
                require_nonempty(src, "parent_descriptor.source_scope")?;
                require_nonempty(mid, "parent_descriptor.scope_map_id")?;
                if src == self.scope_id {
                    return Err(CoreError::ScopeMapForbidden(
                        "mapped_conditional_parent requires distinct source/target scopes"
                            .to_string(),
                    ));
                }
                let wid = self.wave_identity.as_deref().ok_or_else(|| {
                    CoreError::ValidationError(
                        "mapped_conditional_parent requires wave_identity".to_string(),
                    )
                })?;
                let expected_prefix = format!("mapped:{mid}:{}:", self.scope_id);
                if !wid.starts_with(&expected_prefix) {
                    return Err(CoreError::ValidationError(
                        "mapped_conditional_parent wave_identity prefix mismatch".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn root_key(&self) -> CoreResult<ParentRootKey> {
        self.validate()?;
        let family = match self.kind {
            ParentDescriptorKind::AdmittedNativeParent
            | ParentDescriptorKind::AdmittedOriginClosure => "native".to_string(),
            ParentDescriptorKind::DeclaredSyntheticParent => "synthetic".to_string(),
            ParentDescriptorKind::MappedConditionalParent => format!(
                "mapped:{}:{}",
                self.scope_map_id.as_deref().unwrap_or_default(),
                self.source_scope.as_deref().unwrap_or_default()
            ),
        };
        Ok(ParentRootKey {
            family,
            scope_id: self.scope_id.clone(),
            scale: self.scale,
            lineage_id: self.lineage_id.clone(),
            motif: self.motif.clone(),
            origin_return_id: self.origin_return_id.clone(),
            origin_ancestor: self.origin_ancestor.clone(),
            inner_wave_origins: self.inner_wave_origins.clone(),
            motif_core: self.motif_core.clone(),
        })
    }

    pub fn wavelet_parent_key(&self) -> CoreResult<WaveletParentKey> {
        let root_key = self.root_key()?;
        Ok(WaveletParentKey {
            kind: self.kind,
            wave_identity: self.wave_identity.clone(),
            root_key,
        })
    }

    pub fn matches_wavelet(&self, wavelet_pdesc: &ParentDescriptor) -> CoreResult<bool> {
        self.validate()?;
        wavelet_pdesc.validate()?;
        if self.kind == ParentDescriptorKind::AdmittedOriginClosure
            && wavelet_pdesc.kind == ParentDescriptorKind::AdmittedNativeParent
        {
            Ok(self.root_key()? == wavelet_pdesc.root_key()?)
        } else {
            Ok(self.wavelet_parent_key()? == wavelet_pdesc.wavelet_parent_key()?)
        }
    }
}

// ============================================================================
// ReturnWitness, F01/F05 Authoritative OccurrenceRegistry, & F02/F07 ScopeMap
// ============================================================================

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum OccurrenceRecordKind {
    AdmittedNativeTrace,
    DeclaredSyntheticOccurrence,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ImmutableOccurrencePayload {
    pub record_kind: OccurrenceRecordKind,
    pub return_id: String,
    pub parent_root_key: ParentRootKey,
    pub cut_coordinate: String,
    pub occurrence_j: usize,
    pub start_cut: Option<i64>,
    pub completion_cut: i64,
    pub compatible: bool,
    pub ancestor: Option<String>,
    pub lineage_prefix_ids: Option<Vec<String>>,
    pub lineage_prefix_digest: Option<String>,
    pub trace_digest: Option<String>,
    pub certificate_digest: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ReturnWitness {
    pub record_kind: OccurrenceRecordKind,
    pub authenticated_trace: bool,
    pub return_id: String,
    pub display_label: String,
    pub occurrence_j: usize,
    pub start_cut: Option<i64>,
    pub completion_cut: i64,
    pub lineage_id: String,
    pub scope_id: String,
    pub scale: u32,
    pub cut_coordinate: String,
    pub compatible: bool,
    pub ancestor: Option<String>,
    pub origin_return_id: Option<String>,
    pub parent_descriptor: ParentDescriptor,
    pub lineage_prefix_ids: Option<Vec<String>>,
    pub lineage_prefix_digest: Option<String>,
    pub trace_digest: Option<String>,
    pub certificate_digest: Option<String>,
    pub admitted_record: Option<AdmittedClosureRecord>,
}

impl ReturnWitness {
    #[allow(clippy::too_many_arguments)]
    pub fn synthetic(
        return_id: impl Into<String>,
        occurrence_j: usize,
        completion_cut: i64,
        lineage_id: impl Into<String>,
        scope_id: impl Into<String>,
        scale: u32,
        motif: impl Into<String>,
        parent_descriptor: Option<ParentDescriptor>,
        lineage_prefix_ids: Option<Vec<String>>,
        lineage_prefix_digest: Option<String>,
        cut_coordinate: impl Into<String>,
        compatible: bool,
        display_label: Option<String>,
    ) -> CoreResult<Self> {
        let rid = return_id.into();
        let lin = lineage_id.into();
        let scp = scope_id.into();
        let coord = cut_coordinate.into();
        let mot = motif.into();
        require_nonempty(&rid, "return_id")?;
        require_nonempty(&lin, "lineage_id")?;
        require_nonempty(&scp, "scope_id")?;
        require_nonempty(&coord, "cut_coordinate")?;
        let pdesc = match parent_descriptor {
            Some(p) => {
                p.validate()?;
                p
            }
            None => ParentDescriptor::declare_synthetic_parent(&lin, &scp, scale, &mot, None, None)?,
        };
        if pdesc.kind != ParentDescriptorKind::DeclaredSyntheticParent
            || pdesc.lineage_id != lin
            || pdesc.scope_id != scp
            || pdesc.scale != scale
        {
            return Err(CoreError::ValidationError(
                "synthetic witness parent_descriptor mismatch".to_string(),
            ));
        }
        let (pref_ids, pref_dig) = if lineage_prefix_ids.is_some() || lineage_prefix_digest.is_some()
        {
            let ids = lineage_prefix_ids.ok_or_else(|| {
                CoreError::ValidationError("synthetic lineage_prefix_ids required".to_string())
            })?;
            if ids.len() != occurrence_j + 1 {
                return Err(CoreError::ValidationError(
                    "synthetic lineage_prefix_ids length must equal occurrence_j + 1".to_string(),
                ));
            }
            let dig = lineage_prefix_digest.unwrap_or_else(|| digest_str_tuple(&ids));
            (Some(ids), Some(dig))
        } else {
            (None, None)
        };
        let label = match display_label {
            Some(l) => {
                require_nonempty(&l, "display_label")?;
                l
            }
            None => rid.clone(),
        };
        let wit = Self {
            record_kind: OccurrenceRecordKind::DeclaredSyntheticOccurrence,
            authenticated_trace: false,
            return_id: rid,
            display_label: label,
            occurrence_j,
            start_cut: None,
            completion_cut,
            lineage_id: lin,
            scope_id: scp,
            scale,
            cut_coordinate: coord,
            compatible,
            ancestor: None,
            origin_return_id: pdesc.origin_return_id.clone(),
            parent_descriptor: pdesc,
            lineage_prefix_ids: pref_ids,
            lineage_prefix_digest: pref_dig,
            trace_digest: None,
            certificate_digest: None,
            admitted_record: None,
        };
        let _ = wit.immutable_payload()?;
        Ok(wit)
    }

    pub fn immutable_payload(&self) -> CoreResult<ImmutableOccurrencePayload> {
        require_nonempty(&self.return_id, "return_id")?;
        require_nonempty(&self.lineage_id, "lineage_id")?;
        require_nonempty(&self.scope_id, "scope_id")?;
        require_nonempty(&self.cut_coordinate, "cut_coordinate")?;
        self.parent_descriptor.validate()?;
        let root_key = self.parent_descriptor.root_key()?;

        match self.record_kind {
            OccurrenceRecordKind::AdmittedNativeTrace => {
                if !self.authenticated_trace {
                    return Err(CoreError::ValidationError(
                        "admitted trace flag mismatch".to_string(),
                    ));
                }
                let rec = self.admitted_record.as_ref().ok_or_else(|| {
                    CoreError::ValidationError(
                        "admitted_native_trace missing bound AdmittedClosureRecord".to_string(),
                    )
                })?;
                rec.validate()?;
                let t = rec.trace();
                let c = rec.certificate();
                if !matches!(
                    self.parent_descriptor.kind,
                    ParentDescriptorKind::AdmittedNativeParent
                        | ParentDescriptorKind::AdmittedOriginClosure
                ) {
                    return Err(CoreError::ValidationError(
                        "admitted_native_trace requires admitted parent_descriptor".to_string(),
                    ));
                }
                if self.return_id != t.return_id() || self.return_id != c.return_id {
                    return Err(CoreError::ValidationError(
                        "admitted occurrence return_id does not match Trace.return_id".to_string(),
                    ));
                }
                if self.lineage_id != t.lineage
                    || self.scope_id != t.scope
                    || self.scale != t.scale
                    || self.completion_cut != t.completion_cut()
                    || self.start_cut != Some(t.start_cut())
                    || self.ancestor.as_deref() != Some(t.ancestor.as_str())
                {
                    return Err(CoreError::ValidationError(
                        "admitted occurrence metadata diverges from bound Trace".to_string(),
                    ));
                }
                if self.parent_descriptor.lineage_id != t.lineage
                    || self.parent_descriptor.scope_id != t.scope
                    || self.parent_descriptor.scale != t.scale
                {
                    return Err(CoreError::ValidationError(
                        "admitted occurrence parent_descriptor diverges from bound Trace"
                            .to_string(),
                    ));
                }
                if c.motif == "outer" {
                    if self.parent_descriptor.inner_wave_origins != c.inner_wave_origins {
                        return Err(CoreError::ValidationError(
                            "outer occurrence inner_wave_origins mismatch".to_string(),
                        ));
                    }
                    if self.occurrence_j == 0 && self.parent_descriptor.origin_ancestor != t.ancestor {
                        return Err(CoreError::ValidationError(
                            "outer origin_ancestor mismatch".to_string(),
                        ));
                    }
                } else if self.parent_descriptor.origin_ancestor != t.ancestor
                    || !self.parent_descriptor.inner_wave_origins.is_empty()
                {
                    return Err(CoreError::ValidationError(
                        "admitted occurrence origin_ancestor mismatch".to_string(),
                    ));
                }
                if self.origin_return_id != self.parent_descriptor.origin_return_id {
                    return Err(CoreError::ValidationError(
                        "admitted occurrence origin_return_id diverges from parent_descriptor"
                            .to_string(),
                    ));
                }
                let prefix_ids = self.lineage_prefix_ids.as_ref().ok_or_else(|| {
                    CoreError::ValidationError(
                        "admitted occurrence missing lineage_prefix_ids".to_string(),
                    )
                })?;
                if prefix_ids.len() != self.occurrence_j + 1 {
                    return Err(CoreError::ValidationError(
                        "admitted occurrence lineage_prefix_ids length must equal occurrence_j + 1"
                            .to_string(),
                    ));
                }
                let unique: BTreeSet<&str> = prefix_ids.iter().map(String::as_str).collect();
                let orig_rid = self
                    .parent_descriptor
                    .origin_return_id
                    .as_deref()
                    .unwrap_or_default();
                if unique.len() != prefix_ids.len()
                    || prefix_ids[0] != orig_rid
                    || prefix_ids[prefix_ids.len() - 1] != self.return_id
                    || ((self.occurrence_j == 0) != (self.return_id == orig_rid))
                {
                    return Err(CoreError::ValidationError(
                        "admitted occurrence lineage_prefix_ids or origin index binding invalid"
                            .to_string(),
                    ));
                }
                let pref_dig = self.lineage_prefix_digest.as_deref().ok_or_else(|| {
                    CoreError::ValidationError(
                        "admitted occurrence missing lineage_prefix_digest".to_string(),
                    )
                })?;
                if pref_dig != digest_str_tuple(prefix_ids) {
                    return Err(CoreError::ValidationError(
                        "admitted occurrence lineage_prefix_digest does not match bound lineage_prefix_ids"
                            .to_string(),
                    ));
                }
                if self.trace_digest.as_deref() != Some(t.trace_digest().as_str())
                    || self.certificate_digest.as_deref()
                        != Some(c.certificate_digest.as_str())
                {
                    return Err(CoreError::ValidationError(
                        "admitted occurrence digests diverge from bound Trace/Certificate"
                            .to_string(),
                    ));
                }
                Ok(ImmutableOccurrencePayload {
                    record_kind: self.record_kind,
                    return_id: self.return_id.clone(),
                    parent_root_key: root_key,
                    cut_coordinate: self.cut_coordinate.clone(),
                    occurrence_j: self.occurrence_j,
                    start_cut: Some(t.start_cut()),
                    completion_cut: self.completion_cut,
                    compatible: self.compatible,
                    ancestor: Some(t.ancestor.clone()),
                    lineage_prefix_ids: Some(prefix_ids.clone()),
                    lineage_prefix_digest: Some(pref_dig.to_string()),
                    trace_digest: Some(t.trace_digest()),
                    certificate_digest: Some(c.certificate_digest.clone()),
                })
            }
            OccurrenceRecordKind::DeclaredSyntheticOccurrence => {
                if self.authenticated_trace
                    || self.trace_digest.is_some()
                    || self.certificate_digest.is_some()
                    || self.admitted_record.is_some()
                {
                    return Err(CoreError::ValidationError(
                        "declared_synthetic_occurrence must not claim authenticated trace/certificate digests"
                            .to_string(),
                    ));
                }
                if self.parent_descriptor.kind != ParentDescriptorKind::DeclaredSyntheticParent
                    || self.parent_descriptor.lineage_id != self.lineage_id
                    || self.parent_descriptor.scope_id != self.scope_id
                    || self.parent_descriptor.scale != self.scale
                    || self.origin_return_id != self.parent_descriptor.origin_return_id
                {
                    return Err(CoreError::ValidationError(
                        "declared_synthetic_occurrence parent_descriptor mismatch".to_string(),
                    ));
                }
                if self.lineage_prefix_ids.is_some() || self.lineage_prefix_digest.is_some() {
                    let prefix_ids = self.lineage_prefix_ids.as_ref().ok_or_else(|| {
                        CoreError::ValidationError(
                            "synthetic lineage_prefix_ids required with digest".to_string(),
                        )
                    })?;
                    if prefix_ids.len() != self.occurrence_j + 1 {
                        return Err(CoreError::ValidationError(
                            "synthetic lineage_prefix_ids length must equal occurrence_j + 1"
                                .to_string(),
                        ));
                    }
                    let unique: BTreeSet<&str> = prefix_ids.iter().map(String::as_str).collect();
                    if unique.len() != prefix_ids.len()
                        || prefix_ids[prefix_ids.len() - 1] != self.return_id
                    {
                        return Err(CoreError::ValidationError(
                            "synthetic lineage_prefix_ids invalid".to_string(),
                        ));
                    }
                    if let Some(ref orig_rid) = self.parent_descriptor.origin_return_id {
                        if prefix_ids[0] != *orig_rid
                            || ((self.occurrence_j == 0) != (self.return_id == *orig_rid))
                        {
                            return Err(CoreError::ValidationError(
                                "synthetic origin_return_id/occurrence_j binding invalid"
                                    .to_string(),
                            ));
                        }
                    }
                    let pref_dig = self.lineage_prefix_digest.as_deref().ok_or_else(|| {
                        CoreError::ValidationError(
                            "synthetic lineage_prefix_digest missing".to_string(),
                        )
                    })?;
                    if pref_dig != digest_str_tuple(prefix_ids) {
                        return Err(CoreError::ValidationError(
                            "synthetic lineage_prefix_digest does not match lineage_prefix_ids"
                                .to_string(),
                        ));
                    }
                }
                Ok(ImmutableOccurrencePayload {
                    record_kind: self.record_kind,
                    return_id: self.return_id.clone(),
                    parent_root_key: root_key,
                    cut_coordinate: self.cut_coordinate.clone(),
                    occurrence_j: self.occurrence_j,
                    start_cut: None,
                    completion_cut: self.completion_cut,
                    compatible: self.compatible,
                    ancestor: None,
                    lineage_prefix_ids: self.lineage_prefix_ids.clone(),
                    lineage_prefix_digest: self.lineage_prefix_digest.clone(),
                    trace_digest: None,
                    certificate_digest: None,
                })
            }
        }
    }
}

/// Derive canonical occurrences (`origin j = 0`) from validated ordered native closure records.
pub fn project_admitted_lineage_occurrences(
    records: &[AdmittedClosureRecord],
    cut_coordinate: &str,
    display_prefix: &str,
    motif_core: Option<String>,
) -> CoreResult<Vec<ReturnWitness>> {
    if records.is_empty() {
        return Err(CoreError::ValidationError(
            "certified_records must be non-empty".to_string(),
        ));
    }
    require_nonempty(cut_coordinate, "cut_coordinate")?;
    require_nonempty(display_prefix, "display_prefix")?;

    if records.len() == 1 {
        let rec = &records[0];
        rec.validate()?;
        if closure_cycle_count(records, None)? != 1 {
            return Err(CoreError::ValidationError(
                "origin closure failed closure_cycle_count == 1 check".to_string(),
            ));
        }
        let pdesc = ParentDescriptor::describe_admitted_origin_closure(rec, motif_core)?;
        let t = rec.trace();
        let c = rec.certificate();
        let prefix_ids = vec![t.return_id()];
        let pref_dig = digest_str_tuple(&prefix_ids);
        let wit = ReturnWitness {
            record_kind: OccurrenceRecordKind::AdmittedNativeTrace,
            authenticated_trace: true,
            return_id: t.return_id(),
            display_label: format!("{display_prefix}0"),
            occurrence_j: 0,
            start_cut: Some(t.start_cut()),
            completion_cut: t.completion_cut(),
            lineage_id: t.lineage.clone(),
            scope_id: t.scope.clone(),
            scale: t.scale,
            cut_coordinate: cut_coordinate.to_string(),
            compatible: true,
            ancestor: Some(t.ancestor.clone()),
            origin_return_id: pdesc.origin_return_id.clone(),
            parent_descriptor: pdesc,
            lineage_prefix_ids: Some(prefix_ids),
            lineage_prefix_digest: Some(pref_dig),
            trace_digest: Some(t.trace_digest()),
            certificate_digest: Some(c.certificate_digest.clone()),
            admitted_record: Some(rec.clone()),
        };
        let _ = wit.immutable_payload()?;
        return Ok(vec![wit]);
    }

    let wave = M3Wave::new(records.to_vec(), motif_core)?;
    project_admitted_wave_occurrences(&wave, cut_coordinate, display_prefix)
}

pub fn project_admitted_wave_occurrences(
    wave: &M3Wave,
    cut_coordinate: &str,
    display_prefix: &str,
) -> CoreResult<Vec<ReturnWitness>> {
    wave.validate()?;
    require_nonempty(cut_coordinate, "cut_coordinate")?;
    require_nonempty(display_prefix, "display_prefix")?;
    let pdesc = ParentDescriptor::describe_admitted_parent(wave)?;
    if closure_cycle_count(&wave.records, None)? != wave.records.len() {
        return Err(CoreError::ValidationError(
            "closure_cycle_count disagrees with admitted wave return count".to_string(),
        ));
    }
    let mut out = Vec::with_capacity(wave.records.len());
    for (idx, rec) in wave.records.iter().enumerate() {
        let t = rec.trace();
        let c = rec.certificate();
        let prefix_ids: Vec<String> = wave.records[..=idx]
            .iter()
            .map(|r| r.trace().return_id())
            .collect();
        let pref_dig = digest_str_tuple(&prefix_ids);
        let wit = ReturnWitness {
            record_kind: OccurrenceRecordKind::AdmittedNativeTrace,
            authenticated_trace: true,
            return_id: t.return_id(),
            display_label: format!("{display_prefix}{idx}"),
            occurrence_j: idx,
            start_cut: Some(t.start_cut()),
            completion_cut: t.completion_cut(),
            lineage_id: t.lineage.clone(),
            scope_id: t.scope.clone(),
            scale: t.scale,
            cut_coordinate: cut_coordinate.to_string(),
            compatible: true,
            ancestor: Some(t.ancestor.clone()),
            origin_return_id: pdesc.origin_return_id.clone(),
            parent_descriptor: pdesc.clone(),
            lineage_prefix_ids: Some(prefix_ids),
            lineage_prefix_digest: Some(pref_dig),
            trace_digest: Some(t.trace_digest()),
            certificate_digest: Some(c.certificate_digest.clone()),
            admitted_record: Some(rec.clone()),
        };
        let _ = wit.immutable_payload()?;
        out.push(wit);
    }
    Ok(out)
}

/// Derive ordered finite-model synthetic occurrences (`origin j = 0`) with display labels kept
/// separate from canonical synthetic `return_id`s.
pub fn declare_synthetic_lineage_registry(
    lineage_id: &str,
    scope_id: &str,
    completion_cuts: &[i64],
    scale: u32,
    cut_coordinate: &str,
    display_labels: Option<&[&str]>,
) -> CoreResult<(Vec<ReturnWitness>, OccurrenceRegistry)> {
    require_nonempty(lineage_id, "lineage_id")?;
    require_nonempty(scope_id, "scope_id")?;
    require_nonempty(cut_coordinate, "cut_coordinate")?;
    if completion_cuts.is_empty() {
        return Err(CoreError::ValidationError(
            "completion_cuts must be non-empty".to_string(),
        ));
    }
    if !completion_cuts.windows(2).all(|w| w[0] < w[1]) {
        return Err(CoreError::ChronologyError(
            "synthetic lineage completion_cuts must be strictly increasing".to_string(),
        ));
    }
    let labels: Vec<String> = match display_labels {
        Some(lbls) => {
            if lbls.len() != completion_cuts.len() {
                return Err(CoreError::ValidationError(
                    "display_labels length must match completion_cuts length".to_string(),
                ));
            }
            for &l in lbls {
                require_nonempty(l, "display_label")?;
            }
            lbls.iter().map(|s| (*s).to_string()).collect()
        }
        None => (0..completion_cuts.len()).map(|i| format!("C{i}")).collect(),
    };
    let rids: Vec<String> = completion_cuts
        .iter()
        .enumerate()
        .map(|(idx, &cut)| {
            format!("syn:{lineage_id}:{scope_id}:s{scale}:{cut_coordinate}:j{idx}:k{cut}")
        })
        .collect();
    let shared_pdesc = ParentDescriptor::declare_synthetic_parent(
        lineage_id,
        scope_id,
        scale,
        "declared_synthetic",
        Some(rids[0].clone()),
        None,
    )?;
    let mut witnesses = Vec::with_capacity(completion_cuts.len());
    for (idx, &cut) in completion_cuts.iter().enumerate() {
        let prefix = rids[..=idx].to_vec();
        let dig = digest_str_tuple(&prefix);
        witnesses.push(ReturnWitness::synthetic(
            rids[idx].clone(),
            idx,
            cut,
            lineage_id,
            scope_id,
            scale,
            "declared_synthetic",
            Some(shared_pdesc.clone()),
            Some(prefix),
            Some(dig),
            cut_coordinate,
            true,
            Some(labels[idx].clone()),
        )?);
    }
    let reg = OccurrenceRegistry::build(&witnesses)?;
    Ok((witnesses, reg))
}

/// Authoritative, injective, anti-aliased canonical occurrence registry (`F01`, `F05`).
///
/// Immutable after construction; extension happens strictly through the pure
/// [`OccurrenceRegistry::extend_occurrence_registry`] method returning a new registry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OccurrenceRegistry {
    by_id: BTreeMap<String, ReturnWitness>,
    payload_by_id: BTreeMap<String, ImmutableOccurrencePayload>,
}

impl OccurrenceRegistry {
    pub fn build(records: &[ReturnWitness]) -> CoreResult<Self> {
        let mut by_id: BTreeMap<String, ReturnWitness> = BTreeMap::new();
        let mut payload_by_id: BTreeMap<String, ImmutableOccurrencePayload> = BTreeMap::new();
        let mut by_occ_slot: BTreeMap<(ParentRootKey, String, usize), String> = BTreeMap::new();
        let mut by_cut_slot: BTreeMap<(ParentRootKey, String, i64), String> = BTreeMap::new();
        let mut stream_kinds: BTreeMap<(ParentRootKey, String), OccurrenceRecordKind> =
            BTreeMap::new();
        let mut grouped: BTreeMap<(ParentRootKey, String), Vec<ReturnWitness>> = BTreeMap::new();

        for raw in records {
            let payload = raw.immutable_payload()?;
            let rid = raw.return_id.clone();
            if let Some(existing_payload) = payload_by_id.get(&rid) {
                if existing_payload != &payload {
                    return Err(CoreError::CollisionError(format!(
                        "return_id collision for {rid:?}: conflicting immutable occurrence payloads"
                    )));
                }
                continue;
            }
            let stream_key = (payload.parent_root_key.clone(), raw.cut_coordinate.clone());
            if let Some(&prev_kind) = stream_kinds.get(&stream_key) {
                if prev_kind != raw.record_kind {
                    return Err(CoreError::ValidationError(
                        "mixed native and synthetic occurrence kinds on same stream".to_string(),
                    ));
                }
            } else {
                stream_kinds.insert(stream_key.clone(), raw.record_kind);
            }

            let occ_key = (
                payload.parent_root_key.clone(),
                raw.cut_coordinate.clone(),
                raw.occurrence_j,
            );
            if let Some(other_rid) = by_occ_slot.get(&occ_key) {
                return Err(CoreError::AliasingError(format!(
                    "canonical occurrence j={} aliased under multiple return_ids ({other_rid:?}, {rid:?})",
                    raw.occurrence_j
                )));
            }
            by_occ_slot.insert(occ_key, rid.clone());

            let cut_key = (
                payload.parent_root_key.clone(),
                raw.cut_coordinate.clone(),
                raw.completion_cut,
            );
            if let Some(other_rid) = by_cut_slot.get(&cut_key) {
                return Err(CoreError::AliasingError(format!(
                    "canonical completion cut {} aliased under multiple return_ids ({other_rid:?}, {rid:?})",
                    raw.completion_cut
                )));
            }
            by_cut_slot.insert(cut_key, rid.clone());

            by_id.insert(rid.clone(), raw.clone());
            payload_by_id.insert(rid, payload);
            grouped.entry(stream_key).or_default().push(raw.clone());
        }

        for members in grouped.values() {
            let mut ordered = members.clone();
            ordered.sort_by_key(|r| r.occurrence_j);
            for pair in ordered.windows(2) {
                let left = &pair[0];
                let right = &pair[1];
                if left.completion_cut >= right.completion_cut {
                    return Err(CoreError::ChronologyError(format!(
                        "non-monotonic occurrence cuts: j={}@{} vs j={}@{}",
                        left.occurrence_j,
                        left.completion_cut,
                        right.occurrence_j,
                        right.completion_cut
                    )));
                }
                if let (Some(lp), Some(rp)) =
                    (&left.lineage_prefix_ids, &right.lineage_prefix_ids)
                {
                    if rp.len() <= left.occurrence_j || rp[..=left.occurrence_j] != lp[..] {
                        return Err(CoreError::ValidationError(
                            "retained prefix history diverged on occurrence stream".to_string(),
                        ));
                    }
                }
                if left.record_kind == OccurrenceRecordKind::AdmittedNativeTrace {
                    let lt = left.admitted_record.as_ref().expect("validated").trace();
                    let rt = right.admitted_record.as_ref().expect("validated").trace();
                    if lt.completion_cut() >= rt.start_cut() {
                        return Err(CoreError::ChronologyError(
                            "admitted native traces overlap or invert causal order".to_string(),
                        ));
                    }
                    let lc = left.admitted_record.as_ref().expect("validated").certificate();
                    if lc.motif != "outer" && lt.ancestor != rt.ancestor {
                        return Err(CoreError::ValidationError(
                            "admitted native traces changed retained ancestor".to_string(),
                        ));
                    }
                }
            }
            if ordered
                .first()
                .is_some_and(|r| r.record_kind == OccurrenceRecordKind::AdmittedNativeTrace)
            {
                let native_recs: Vec<AdmittedClosureRecord> = ordered
                    .iter()
                    .map(|r| r.admitted_record.clone().expect("validated"))
                    .collect();
                if closure_cycle_count(&native_recs, None)? != ordered.len() {
                    return Err(CoreError::ValidationError(
                        "native closure_cycle_count mismatch on registry stream".to_string(),
                    ));
                }
            }
        }

        Ok(Self {
            by_id,
            payload_by_id,
        })
    }

    /// Pure non-mutating registry extension (`F05`).
    pub fn extend_occurrence_registry(&self, new_records: &[ReturnWitness]) -> CoreResult<Self> {
        if new_records.is_empty() {
            return Err(CoreError::ValidationError(
                "new_records for registry extension must be non-empty".to_string(),
            ));
        }
        let mut combined: Vec<ReturnWitness> = self.by_id.values().cloned().collect();
        combined.extend(new_records.iter().cloned());
        Self::build(&combined)
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    pub fn keys(&self) -> Vec<String> {
        self.by_id.keys().cloned().collect()
    }

    pub fn get(&self, return_id: &str) -> Option<&ReturnWitness> {
        self.by_id.get(return_id)
    }

    pub fn payload(&self, return_id: &str) -> Option<&ImmutableOccurrencePayload> {
        self.payload_by_id.get(return_id)
    }

    pub fn values(&self) -> impl Iterator<Item = &ReturnWitness> {
        self.by_id.values()
    }
}

/// Explicit conditional identity-preserving cross-scope map for a single parent (`F02`, `F07`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopeMap {
    pub map_id: String,
    pub source_scope: String,
    pub target_scope: String,
    pub lineage_id: String,
    pub scale: u32,
    pub authenticated_native_isomorphism: bool,
    pub source_parent_descriptor: ParentDescriptor,
    pub source_parent_root_key: ParentRootKey,
    pub bound_occurrences: BTreeMap<String, ImmutableOccurrencePayload>,
}

impl ScopeMap {
    pub fn declare_conditional(
        map_id: impl Into<String>,
        source_scope: impl Into<String>,
        target_scope: impl Into<String>,
        lineage_id: impl Into<String>,
        scale: u32,
        occurrence_records: &[ReturnWitness],
    ) -> CoreResult<Self> {
        let mid = map_id.into();
        let src = source_scope.into();
        let dst = target_scope.into();
        let lin = lineage_id.into();
        require_nonempty(&mid, "map_id")?;
        require_nonempty(&src, "source_scope")?;
        require_nonempty(&dst, "target_scope")?;
        require_nonempty(&lin, "lineage_id")?;
        if src == dst {
            return Err(CoreError::ScopeMapForbidden(
                "conditional scope map requires distinct source and target scopes".to_string(),
            ));
        }
        let canon = OccurrenceRegistry::build(occurrence_records)?;
        if canon.is_empty() {
            return Err(CoreError::ValidationError(
                "conditional scope map requires at least one bound occurrence".to_string(),
            ));
        }
        let mut bound_occurrences = BTreeMap::new();
        let mut root_keys: BTreeSet<ParentRootKey> = BTreeSet::new();
        let mut rep_pdesc: Option<ParentDescriptor> = None;
        for (rid, rec) in &canon.by_id {
            if rec.lineage_id != lin || rec.scope_id != src || rec.scale != scale {
                return Err(CoreError::ValidationError(
                    "occurrence in conditional scope map does not match declared source scope/lineage/scale"
                        .to_string(),
                ));
            }
            rec.parent_descriptor.validate()?;
            root_keys.insert(rec.parent_descriptor.root_key()?);
            if rep_pdesc.is_none()
                || rec.parent_descriptor.kind == ParentDescriptorKind::AdmittedNativeParent
            {
                rep_pdesc = Some(rec.parent_descriptor.clone());
            }
            bound_occurrences.insert(rid.clone(), rec.immutable_payload()?);
        }
        if root_keys.len() != 1 {
            return Err(CoreError::ValidationError(
                "conditional scope map must bind occurrences of a single parent identity"
                    .to_string(),
            ));
        }
        let source_parent_descriptor = rep_pdesc.expect("nonempty");
        let source_parent_root_key = root_keys.into_iter().next().expect("len == 1");
        Ok(Self {
            map_id: mid,
            source_scope: src,
            target_scope: dst,
            lineage_id: lin,
            scale,
            authenticated_native_isomorphism: false,
            source_parent_descriptor,
            source_parent_root_key,
            bound_occurrences,
        })
    }

    pub fn validate_for_witness(
        &self,
        witness: &ReturnWitness,
        target_scope: &str,
        target_lineage: &str,
    ) -> CoreResult<()> {
        require_nonempty(&self.map_id, "scope_map.map_id")?;
        if self.source_scope != witness.scope_id
            || self.target_scope != target_scope
            || self.source_scope == self.target_scope
        {
            return Err(CoreError::ValidationError(
                "scope_map source/target scope mismatch".to_string(),
            ));
        }
        if self.lineage_id != target_lineage
            || self.lineage_id != witness.lineage_id
            || self.scale != witness.scale
        {
            return Err(CoreError::ValidationError(
                "scope_map lineage/scale mismatch".to_string(),
            ));
        }
        let payload = witness.immutable_payload()?;
        if self.source_parent_root_key != witness.parent_descriptor.root_key()? {
            return Err(CoreError::ValidationError(
                "scope_map source parent identity mismatch".to_string(),
            ));
        }
        if self.source_parent_root_key != self.source_parent_descriptor.root_key()? {
            return Err(CoreError::ValidationError(
                "scope_map source_parent_root_key conflicts with source_parent_descriptor"
                    .to_string(),
            ));
        }
        let bound_payload = self.bound_occurrences.get(&witness.return_id).ok_or_else(|| {
            CoreError::ValidationError(format!(
                "return_id {:?} is not bound in supplied scope_map",
                witness.return_id
            ))
        })?;
        if bound_payload != &payload {
            return Err(CoreError::CollisionError(format!(
                "return_id {:?} payload conflicts with bound occurrence in scope_map",
                witness.return_id
            )));
        }
        Ok(())
    }

    pub fn map_parent_to_scope(
        &self,
        src_pdesc: &ParentDescriptor,
        target_scope: &str,
    ) -> CoreResult<ParentDescriptor> {
        src_pdesc.validate()?;
        if self.source_scope != src_pdesc.scope_id
            || self.target_scope != target_scope
            || self.lineage_id != src_pdesc.lineage_id
            || self.scale != src_pdesc.scale
            || self.source_parent_root_key != src_pdesc.root_key()?
        {
            return Err(CoreError::ValidationError(
                "scope_map does not match parent_descriptor".to_string(),
            ));
        }
        let base_wid = src_pdesc.wave_identity.clone().unwrap_or_else(|| {
            format!(
                "origin:{}",
                src_pdesc.origin_return_id.as_deref().unwrap_or_default()
            )
        });
        let mapped = ParentDescriptor {
            kind: ParentDescriptorKind::MappedConditionalParent,
            admitted_recurring_wave: src_pdesc.admitted_recurring_wave,
            wave_identity: Some(format!("mapped:{}:{target_scope}:{base_wid}", self.map_id)),
            scope_id: target_scope.to_string(),
            scale: src_pdesc.scale,
            lineage_id: src_pdesc.lineage_id.clone(),
            motif: src_pdesc.motif.clone(),
            origin_return_id: src_pdesc.origin_return_id.clone(),
            origin_ancestor: src_pdesc.origin_ancestor.clone(),
            inner_wave_origins: src_pdesc.inner_wave_origins.clone(),
            motif_core: src_pdesc.motif_core.clone(),
            source_scope: Some(src_pdesc.scope_id.clone()),
            scope_map_id: Some(self.map_id.clone()),
        };
        mapped.validate()?;
        Ok(mapped)
    }

    /// `F07`: classify whether a canonical record belongs to this single-parent scope map's
    /// source domain using full `source_parent_root_key`.
    pub fn canonical_record_in_domain(
        &self,
        record: &ReturnWitness,
        target_scope: &str,
        lineage_filter: Option<&str>,
    ) -> CoreResult<bool> {
        if self.source_scope == self.target_scope {
            return Ok(false);
        }
        if self.source_scope != record.scope_id
            || self.target_scope != target_scope
            || self.lineage_id != record.lineage_id
            || self.scale != record.scale
        {
            return Ok(false);
        }
        if let Some(lin) = lineage_filter {
            if record.lineage_id != lin {
                return Ok(false);
            }
        }
        if self.source_parent_root_key != self.source_parent_descriptor.root_key()? {
            return Err(CoreError::ValidationError(
                "scope_map source_parent_root_key conflicts with source_parent_descriptor"
                    .to_string(),
            ));
        }
        Ok(record.parent_descriptor.root_key()? == self.source_parent_root_key)
    }
}

fn effective_witness_parent(
    witness: &ReturnWitness,
    target_scope: &str,
    scope_map: Option<&ScopeMap>,
) -> CoreResult<ParentDescriptor> {
    let _ = witness.immutable_payload()?;
    witness.parent_descriptor.validate()?;
    if witness.scope_id == target_scope {
        return Ok(witness.parent_descriptor.clone());
    }
    let smap = scope_map.ok_or_else(|| {
        CoreError::ScopeMapRequired(format!(
            "cross-scope witness ({:?} != {:?}) requires an explicit validated scope_map",
            witness.scope_id, target_scope
        ))
    })?;
    smap.validate_for_witness(witness, target_scope, &witness.lineage_id)?;
    smap.map_parent_to_scope(&smap.source_parent_descriptor, target_scope)
}

fn resolve_target_parent_on_scope(
    explicit_pdesc: &ParentDescriptor,
    target_scope: &str,
    scope_map: Option<&ScopeMap>,
) -> CoreResult<ParentDescriptor> {
    explicit_pdesc.validate()?;
    if explicit_pdesc.scope_id == target_scope {
        return Ok(explicit_pdesc.clone());
    }
    let smap = scope_map.ok_or_else(|| {
        CoreError::ScopeMapRequired(
            "cross-scope parent_descriptor requires an explicit validated scope_map".to_string(),
        )
    })?;
    smap.map_parent_to_scope(explicit_pdesc, target_scope)
}

// ============================================================================
// Wavelet, Window Completeness (F07), Overlap Dedup, & Cancellation Retention
// ============================================================================

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum WaveletRole {
    LocalSegment,
    CoupledSegment,
    ShedContinuation,
}

impl WaveletRole {
    pub fn as_str(self) -> &'static str {
        match self {
            WaveletRole::LocalSegment => "local_segment",
            WaveletRole::CoupledSegment => "coupled_segment",
            WaveletRole::ShedContinuation => "shed_continuation",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Wavelet {
    pub id_w: String,
    pub parent_id: String,
    pub scale: u32,
    pub parent_bound: bool,
    pub parent_descriptor: ParentDescriptor,
    pub ancestry: Vec<String>,
    pub scope_id: String,
    pub cut_coordinate: String,
    pub window: (i64, i64),
    pub sigma_seat: SeatState,
    pub sigma_role: WaveletRole,
    pub records: Vec<String>,
    pub return_witnesses: Vec<ReturnWitness>,
    pub witnesses_complete: bool,
    pub signed_current: BigInt,
    pub is_admitted_wave: bool,
}

#[derive(Clone, Debug)]
pub struct WaveletSpec<'a> {
    pub wavelet_id: &'a str,
    pub parent_lineage: &'a str,
    pub explicit_parent: Option<&'a ParentDescriptor>,
    pub ancestry: Vec<String>,
    pub window: (i64, i64),
    pub sigma_seat: SeatState,
    pub sigma_role: WaveletRole,
    pub records: Vec<String>,
    pub return_witnesses: Vec<ReturnWitness>,
    pub signed_current: BigInt,
    pub is_admitted_wave: bool,
    pub scope_id: &'a str,
    pub witnesses_complete: bool,
    pub scale: Option<u32>,
    pub cut_coordinate: &'a str,
    pub scope_map: Option<&'a ScopeMap>,
    pub registry: Option<&'a OccurrenceRegistry>,
}

impl Wavelet {
    pub fn new(spec: WaveletSpec<'_>) -> CoreResult<Self> {
        require_nonempty(spec.wavelet_id, "wavelet_id")?;
        require_nonempty(spec.parent_lineage, "parent_id")?;
        require_nonempty(spec.scope_id, "scope_id")?;
        require_nonempty(spec.cut_coordinate, "cut_coordinate")?;
        if spec.ancestry.is_empty() {
            return Err(CoreError::ValidationError(
                "wavelet must retain non-empty ancestry".to_string(),
            ));
        }
        for anc in &spec.ancestry {
            require_nonempty(anc, "ancestry entry")?;
        }
        let (u, v) = spec.window;
        if u >= v {
            return Err(CoreError::ValidationError(
                "ordered half-open window u < v required".to_string(),
            ));
        }
        if spec.records.is_empty() {
            return Err(CoreError::ValidationError(
                "wavelet must retain committed construction records".to_string(),
            ));
        }
        for rec in &spec.records {
            require_nonempty(rec, "committed record")?;
        }

        // Reject attaching a ScopeMap when scopes already match!
        if let Some(smap) = spec.scope_map {
            let any_cross_scope_witness = spec
                .return_witnesses
                .iter()
                .any(|w| w.scope_id != spec.scope_id);
            let cross_scope_parent = spec
                .explicit_parent
                .is_some_and(|p| p.scope_id != spec.scope_id);
            if !any_cross_scope_witness
                && !cross_scope_parent
                && (!spec.return_witnesses.is_empty() || spec.explicit_parent.is_some())
            {
                return Err(CoreError::ScopeMapForbidden(format!(
                    "same-scope wavelet {:?} must not attach cross-scope map {:?}",
                    spec.scope_id, smap.map_id
                )));
            }
        }

        let local_reg = OccurrenceRegistry::build(&spec.return_witnesses)?;
        let mut eff_pdescs = Vec::with_capacity(local_reg.len());
        for item in local_reg.values() {
            let ep = effective_witness_parent(item, spec.scope_id, spec.scope_map)?;
            if ep.lineage_id != spec.parent_lineage {
                return Err(CoreError::ValidationError(
                    "return witness lineage mismatch".to_string(),
                ));
            }
            eff_pdescs.push(ep);
        }

        let mut parent_bound = true;
        let bound_pdesc = if let Some(explicit) = spec.explicit_parent {
            if explicit.lineage_id != spec.parent_lineage {
                return Err(CoreError::ValidationError(
                    "explicit parent_descriptor lineage_id mismatches parent_lineage".to_string(),
                ));
            }
            let resolved = resolve_target_parent_on_scope(explicit, spec.scope_id, spec.scope_map)?;
            if let Some(scl) = spec.scale {
                if scl != resolved.scale {
                    return Err(CoreError::ValidationError(
                        "wavelet scale mismatches parent_descriptor scale".to_string(),
                    ));
                }
            }
            for ep in &eff_pdescs {
                if !ep.matches_wavelet(&resolved)? {
                    return Err(CoreError::ValidationError(
                        "single-parent wavelet cannot combine witnesses from incompatible parent identities"
                            .to_string(),
                    ));
                }
            }
            resolved
        } else if !eff_pdescs.is_empty() {
            let preferred = eff_pdescs
                .iter()
                .find(|ep| ep.kind == ParentDescriptorKind::AdmittedNativeParent)
                .unwrap_or(&eff_pdescs[0])
                .clone();
            for ep in &eff_pdescs {
                if !ep.matches_wavelet(&preferred)? {
                    return Err(CoreError::ValidationError(
                        "single-parent wavelet cannot combine distinct parent identities or scales"
                            .to_string(),
                    ));
                }
            }
            if let Some(scl) = spec.scale {
                if scl != preferred.scale {
                    return Err(CoreError::ValidationError(
                        "wavelet scale mismatches return witness scale".to_string(),
                    ));
                }
            }
            preferred
        } else {
            let mut matched_canon: BTreeMap<WaveletParentKey, ParentDescriptor> = BTreeMap::new();
            if let Some(canon_reg) = spec.registry {
                for r in canon_reg.values() {
                    if r.lineage_id != spec.parent_lineage {
                        continue;
                    }
                    let ep = if r.scope_id == spec.scope_id {
                        r.parent_descriptor.validate()?;
                        r.parent_descriptor.clone()
                    } else if let Some(smap) = spec.scope_map {
                        if smap.canonical_record_in_domain(
                            r,
                            spec.scope_id,
                            Some(spec.parent_lineage),
                        )? {
                            smap.map_parent_to_scope(&smap.source_parent_descriptor, spec.scope_id)?
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    };
                    if let Some(scl) = spec.scale {
                        if ep.scale != scl {
                            continue;
                        }
                    }
                    matched_canon.insert(ep.wavelet_parent_key()?, ep);
                }
            }
            if matched_canon.len() > 1 {
                return Err(CoreError::ValidationError(
                    "ambiguous parent lineage string across multiple canonical parents; supply explicit ParentDescriptor"
                        .to_string(),
                ));
            }
            if let Some((_, p)) = matched_canon.into_iter().next() {
                p
            } else {
                parent_bound = false;
                ParentDescriptor::declare_synthetic_parent(
                    spec.parent_lineage,
                    spec.scope_id,
                    spec.scale.unwrap_or(0),
                    "declared_synthetic",
                    None,
                    None,
                )?
            }
        };

        if matches!(
            bound_pdesc.kind,
            ParentDescriptorKind::AdmittedNativeParent
                | ParentDescriptorKind::AdmittedOriginClosure
        ) && !spec
            .ancestry
            .iter()
            .any(|a| a == &bound_pdesc.origin_ancestor)
        {
            return Err(CoreError::ValidationError(
                "admitted native wavelet must retain parent origin_ancestor in ancestry"
                    .to_string(),
            ));
        }

        let w = Self {
            id_w: spec.wavelet_id.to_string(),
            parent_id: spec.parent_lineage.to_string(),
            scale: bound_pdesc.scale,
            parent_bound,
            parent_descriptor: bound_pdesc.clone(),
            ancestry: spec.ancestry,
            scope_id: spec.scope_id.to_string(),
            cut_coordinate: spec.cut_coordinate.to_string(),
            window: (u, v),
            sigma_seat: spec.sigma_seat,
            sigma_role: spec.sigma_role,
            records: spec.records,
            return_witnesses: spec.return_witnesses,
            witnesses_complete: spec.witnesses_complete,
            signed_current: spec.signed_current,
            is_admitted_wave: spec.is_admitted_wave,
        };

        let eligible = eligible_return_ids(
            &w,
            Some(&bound_pdesc),
            Some(spec.scope_id),
            spec.scope_map,
            spec.registry,
        )?;
        if spec.is_admitted_wave
            && (spec.sigma_role == WaveletRole::ShedContinuation
                || spec.sigma_seat == SeatState::OpenDuon
                || eligible.is_empty()
                || bound_pdesc.kind == ParentDescriptorKind::AdmittedOriginClosure)
        {
            return Err(CoreError::PromotionForbidden(
                "open/shed wavelet, origin-only closure, or zero-eligible-return fragment cannot be promoted to admitted wave"
                    .to_string(),
            ));
        }
        Ok(w)
    }

    /// Passively restrict an admitted `M3Wave` to a finite window preserving full parent identity.
    #[allow(clippy::too_many_arguments)]
    pub fn restrict_admitted(
        wave: &M3Wave,
        wavelet_id: &str,
        window: (i64, i64),
        sigma_seat: SeatState,
        sigma_role: WaveletRole,
        cut_coordinate: &str,
        retained_occurrences: Option<Vec<ReturnWitness>>,
        signed_current: BigInt,
        is_admitted_wave: bool,
        registry: Option<&OccurrenceRegistry>,
    ) -> CoreResult<Self> {
        let pdesc = ParentDescriptor::describe_admitted_parent(wave)?;
        let all_occ = project_admitted_wave_occurrences(wave, cut_coordinate, "C")?;
        let default_reg = OccurrenceRegistry::build(&all_occ)?;
        let active_reg = registry.unwrap_or(&default_reg);
        let (u, v) = window;
        let selected: Vec<ReturnWitness> = match retained_occurrences {
            Some(sel) => sel,
            None => all_occ
                .iter()
                .filter(|r| r.completion_cut <= v)
                .cloned()
                .collect(),
        };
        let mut rec_digests: Vec<String> = selected
            .iter()
            .filter_map(|r| r.certificate_digest.clone())
            .collect();
        if rec_digests.is_empty() {
            rec_digests.push(wave.records[0].certificate().certificate_digest.clone());
        }
        Self::new(WaveletSpec {
            wavelet_id,
            parent_lineage: &wave.lineage,
            explicit_parent: Some(&pdesc),
            ancestry: vec![pdesc.origin_ancestor.clone(), format!("window:{u}_{v}")],
            window,
            sigma_seat,
            sigma_role,
            records: rec_digests,
            return_witnesses: selected,
            signed_current,
            is_admitted_wave,
            scope_id: &wave.scope,
            witnesses_complete: true,
            scale: Some(wave.scale),
            cut_coordinate,
            scope_map: None,
            registry: Some(active_reg),
        })
    }
}

pub fn eligible_return_ids(
    wavelet: &Wavelet,
    target_parent: Option<&ParentDescriptor>,
    target_scope: Option<&str>,
    scope_map: Option<&ScopeMap>,
    registry: Option<&OccurrenceRegistry>,
) -> CoreResult<BTreeSet<String>> {
    if !wavelet.witnesses_complete {
        return Err(CoreError::ValidationError(
            "complete eligible return records required".to_string(),
        ));
    }
    wavelet.parent_descriptor.validate()?;
    let scope = match target_scope {
        Some(s) => {
            require_nonempty(s, "target_scope")?;
            s
        }
        None => &wavelet.scope_id,
    };
    if wavelet.scope_id != scope {
        return Err(CoreError::ValidationError(
            "incompatible wavelet target_scope".to_string(),
        ));
    }
    if wavelet.parent_descriptor.scope_id != scope
        || wavelet.parent_descriptor.lineage_id != wavelet.parent_id
    {
        return Err(CoreError::ValidationError(
            "wavelet parent_descriptor mismatches wavelet scope_id or parent_id".to_string(),
        ));
    }

    if let Some(t_pdesc) = target_parent {
        if wavelet.parent_id != t_pdesc.lineage_id {
            return Err(CoreError::ValidationError(
                "incompatible parent lineage".to_string(),
            ));
        }
        let eff_t = resolve_target_parent_on_scope(t_pdesc, scope, scope_map)?;
        if wavelet.parent_descriptor.wavelet_parent_key()? != eff_t.wavelet_parent_key()? {
            return Err(CoreError::ValidationError(
                "wavelet parent_descriptor mismatches target parent lineage/identity".to_string(),
            ));
        }
    }

    let local_reg = OccurrenceRegistry::build(&wavelet.return_witnesses)?;
    let (u, v) = wavelet.window;
    let mut eligible = BTreeSet::new();

    for (rid, item) in &local_reg.by_id {
        if let Some(canon_reg) = registry {
            let canon_payload = canon_reg.payload(rid).ok_or_else(|| {
                CoreError::ValidationError(format!(
                    "selected return_id {rid:?} is absent from canonical occurrence registry"
                ))
            })?;
            if &item.immutable_payload()? != canon_payload {
                return Err(CoreError::CollisionError(format!(
                    "selected return witness {rid:?} conflicts with canonical occurrence record"
                )));
            }
        }
        let eff_item_pdesc = effective_witness_parent(item, scope, scope_map)?;
        if !eff_item_pdesc.matches_wavelet(&wavelet.parent_descriptor)? {
            return Err(CoreError::ValidationError(
                "return witness parent_descriptor mismatches wavelet parent lineage/identity"
                    .to_string(),
            ));
        }
        if item.cut_coordinate != wavelet.cut_coordinate {
            return Err(CoreError::ValidationError(
                "return witness cut_coordinate differs from wavelet window coordinate".to_string(),
            ));
        }
        if item.compatible && item.occurrence_j >= 1 && (u < item.completion_cut && item.completion_cut <= v) {
            eligible.insert(rid.clone());
        }
    }

    if wavelet.parent_descriptor.kind == ParentDescriptorKind::MappedConditionalParent {
        let smap = scope_map.ok_or_else(|| {
            CoreError::ScopeMapRequired(
                "mapped_conditional_parent wavelet requires an explicit validated scope_map"
                    .to_string(),
            )
        })?;
        let mapped_from_smap = smap.map_parent_to_scope(&smap.source_parent_descriptor, scope)?;
        if mapped_from_smap.wavelet_parent_key()? != wavelet.parent_descriptor.wavelet_parent_key()? {
            return Err(CoreError::ValidationError(
                "supplied scope_map does not match wavelet mapped_conditional_parent identity"
                    .to_string(),
            ));
        }
    }
    Ok(eligible)
}

/// Validate window completeness against an authoritative `OccurrenceRegistry` (`F01`, `F05`, `F07`).
///
/// When `scope_map` is supplied, canonical cross-scope records are filtered strictly by full
/// `source_parent_root_key` (`F07`) so unrelated parents sharing `(scope, lineage, scale)`
/// do not trigger false failures while omitted/unbound own-parent returns fail strictly.
pub fn validate_complete_windows(
    wavelets: &[Wavelet],
    canon: &OccurrenceRegistry,
    scope_map: Option<&ScopeMap>,
) -> CoreResult<OccurrenceRegistry> {
    if wavelets.is_empty() {
        return Err(CoreError::ValidationError(
            "non-empty wavelets required".to_string(),
        ));
    }
    for w in wavelets {
        w.parent_descriptor.validate()?;
        let actual = eligible_return_ids(
            w,
            Some(&w.parent_descriptor),
            Some(&w.scope_id),
            scope_map,
            Some(canon),
        )?;
        let mut eff_w_pdesc = w.parent_descriptor.clone();
        if !w.parent_bound {
            let mut matched_canon: BTreeMap<WaveletParentKey, ParentDescriptor> = BTreeMap::new();
            for r in canon.values() {
                if r.lineage_id != w.parent_id || r.cut_coordinate != w.cut_coordinate {
                    continue;
                }
                let ep = if r.scope_id == w.scope_id {
                    r.parent_descriptor.validate()?;
                    r.parent_descriptor.clone()
                } else if let Some(smap) = scope_map {
                    if smap.canonical_record_in_domain(r, &w.scope_id, Some(&w.parent_id))? {
                        smap.map_parent_to_scope(&smap.source_parent_descriptor, &w.scope_id)?
                    } else {
                        continue;
                    }
                } else {
                    continue;
                };
                matched_canon.insert(ep.wavelet_parent_key()?, ep);
            }
            if matched_canon.len() > 1 {
                return Err(CoreError::ValidationError(
                    "ambiguous parent lineage string across multiple canonical parents".to_string(),
                ));
            }
            if let Some((_, ep)) = matched_canon.into_iter().next() {
                eff_w_pdesc = ep;
                if matches!(
                    eff_w_pdesc.kind,
                    ParentDescriptorKind::AdmittedNativeParent
                        | ParentDescriptorKind::AdmittedOriginClosure
                ) && !w.ancestry.iter().any(|a| a == &eff_w_pdesc.origin_ancestor)
                {
                    return Err(CoreError::ValidationError(
                        "admitted native wavelet must retain parent origin_ancestor in ancestry"
                            .to_string(),
                    ));
                }
            }
        }

        let (u, v) = w.window;
        let mut expected = BTreeSet::new();
        for (rid, r) in &canon.by_id {
            if r.cut_coordinate != w.cut_coordinate {
                continue;
            }
            if !(r.compatible && r.occurrence_j >= 1 && (u < r.completion_cut && r.completion_cut <= v))
            {
                continue;
            }
            let eff_r_pdesc = if r.scope_id != w.scope_id {
                if eff_w_pdesc.kind != ParentDescriptorKind::MappedConditionalParent {
                    continue;
                }
                let smap = match scope_map {
                    Some(m) => m,
                    None => continue,
                };
                if !smap.canonical_record_in_domain(r, &w.scope_id, None)? {
                    continue;
                }
                effective_witness_parent(r, &w.scope_id, Some(smap))?
            } else {
                r.parent_descriptor.validate()?;
                r.parent_descriptor.clone()
            };
            if !eff_r_pdesc.matches_wavelet(&eff_w_pdesc)? {
                continue;
            }
            expected.insert(rid.clone());
        }

        if actual != expected {
            return Err(CoreError::ValidationError(format!(
                "complete window {:?} omitted or mismatched eligible returns: actual={actual:?} expected={expected:?}",
                w.window
            )));
        }
    }
    Ok(canon.clone())
}

/// Union-deduplicate eligible returns across overlapping wavelets of one bound parent
/// (`eq:wavelet-overlap-dedup`). Never silently expands a supplied canonical registry (`F05`).
pub fn window_union_dedup(
    wavelets: &[Wavelet],
    target_parent: Option<&ParentDescriptor>,
    target_lineage: Option<&str>,
    target_scope: &str,
    scope_map: Option<&ScopeMap>,
    registry: Option<&OccurrenceRegistry>,
) -> CoreResult<BTreeSet<String>> {
    if wavelets.is_empty() {
        return Err(CoreError::ValidationError(
            "non-empty wavelets required".to_string(),
        ));
    }
    require_nonempty(target_scope, "target_scope")?;

    let selected_witnesses: Vec<ReturnWitness> = wavelets
        .iter()
        .flat_map(|w| w.return_witnesses.iter().cloned())
        .collect();
    let conditional_selected = OccurrenceRegistry::build(&selected_witnesses)?;
    let active_registry = registry.unwrap_or(&conditional_selected);

    for w in wavelets {
        if w.scope_id != target_scope {
            return Err(CoreError::ValidationError(
                "wavelet scope_id mismatches target_scope".to_string(),
            ));
        }
    }
    let first_pdesc = &wavelets[0].parent_descriptor;
    let first_key = first_pdesc.wavelet_parent_key()?;
    for other in &wavelets[1..] {
        if other.parent_descriptor.wavelet_parent_key()? != first_key {
            return Err(CoreError::ValidationError(
                "cannot aggregate wavelets from incompatible parent lineages/identities as one current"
                    .to_string(),
            ));
        }
    }

    if let Some(lin) = target_lineage {
        require_nonempty(lin, "target_lineage")?;
        if first_pdesc.lineage_id != lin {
            return Err(CoreError::ValidationError(
                "incompatible target parent lineage".to_string(),
            ));
        }
    }
    if let Some(t_pdesc) = target_parent {
        if first_pdesc.lineage_id != t_pdesc.lineage_id {
            return Err(CoreError::ValidationError(
                "incompatible target parent lineage".to_string(),
            ));
        }
        let eff_t = resolve_target_parent_on_scope(t_pdesc, target_scope, scope_map)?;
        if first_key != eff_t.wavelet_parent_key()? {
            return Err(CoreError::ValidationError(
                "wavelet parent_descriptor mismatches target parent lineage/identity".to_string(),
            ));
        }
    }

    let mut seen = BTreeSet::new();
    for w in wavelets {
        let ids = eligible_return_ids(
            w,
            Some(first_pdesc),
            Some(target_scope),
            scope_map,
            Some(active_registry),
        )?;
        seen.extend(ids);
    }
    Ok(seen)
}

pub fn deduplicated_return_count(
    wavelets: &[Wavelet],
    target_parent: Option<&ParentDescriptor>,
    target_lineage: Option<&str>,
    target_scope: &str,
    scope_map: Option<&ScopeMap>,
    registry: Option<&OccurrenceRegistry>,
) -> CoreResult<usize> {
    Ok(window_union_dedup(
        wavelets,
        target_parent,
        target_lineage,
        target_scope,
        scope_map,
        registry,
    )?
    .len())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CancellationOutcome {
    pub net_current: BigInt,
    pub retained_histories: Vec<(Vec<String>, Vec<String>)>,
}

/// Opposing signed window currents cancel in scalar aggregate while retaining every distinct
/// source history (`eq:wavelet-cancellation-retention`).
pub fn cancel_retain(wavelets: &[Wavelet]) -> CancellationOutcome {
    let mut net = BigInt::zero();
    let mut histories = Vec::with_capacity(wavelets.len());
    for w in wavelets {
        net = &net + &w.signed_current;
        histories.push((w.ancestry.clone(), w.records.clone()));
    }
    CancellationOutcome {
        net_current: net,
        retained_histories: histories,
    }
}

/// Outer-return disjointness guard (`eq:fold-outer-closure`): certified inner returns cannot
/// be reused as outer returns.
pub fn register_outer_returns(inner_ids: &[&str], outer_ids: &[&str]) -> CoreResult<usize> {
    let inner_set: BTreeSet<&str> = inner_ids.iter().copied().collect();
    let mut outer_set = BTreeSet::new();
    for &oid in outer_ids {
        if inner_set.contains(oid) {
            return Err(CoreError::ValidationError(format!(
                "inner return {oid:?} reused as outer return"
            )));
        }
        outer_set.insert(oid);
    }
    Ok(outer_set.len())
}

// ============================================================================
// F03 & F06: ShedWitness, CoordinateRelation, StageEventWitness, RecipientLattice
// ============================================================================

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ShedRoute {
    Endo,
    Exo,
    Redir,
    Open,
}

impl ShedRoute {
    pub fn as_str(self) -> &'static str {
        match self {
            ShedRoute::Endo => "endo",
            ShedRoute::Exo => "exo",
            ShedRoute::Redir => "redir",
            ShedRoute::Open => "open",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ShedWitness {
    pub parent_id: String,
    pub emit_boundary: String,
    pub cut_coordinate: String,
    pub emit_cut: i64,
    pub route: ShedRoute,
    pub x_route: Ratio,
    pub post_shed_parent_state: String,
}

impl ShedWitness {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        parent_id: impl Into<String>,
        emit_boundary: impl Into<String>,
        cut_coordinate: impl Into<String>,
        emit_cut: i64,
        route: ShedRoute,
        x_route: Ratio,
        post_shed_parent_state: impl Into<String>,
    ) -> CoreResult<Self> {
        let w = Self {
            parent_id: parent_id.into(),
            emit_boundary: emit_boundary.into(),
            cut_coordinate: cut_coordinate.into(),
            emit_cut,
            route,
            x_route,
            post_shed_parent_state: post_shed_parent_state.into(),
        };
        w.validate()?;
        Ok(w)
    }

    pub fn validate(&self) -> CoreResult<()> {
        require_nonempty(&self.parent_id, "shed_witness.parent_id")?;
        require_nonempty(&self.emit_boundary, "shed_witness.emit_boundary")?;
        require_nonempty(&self.cut_coordinate, "shed_witness.cut_coordinate")?;
        require_nonempty(
            &self.post_shed_parent_state,
            "shed_witness.post_shed_parent_state",
        )?;
        if !self.x_route.is_positive() {
            return Err(CoreError::DomainError(
                "shed continuation witness requires strictly positive X_route > 0".to_string(),
            ));
        }
        Ok(())
    }

    pub fn to_json(&self) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert(
            "cut_coordinate".to_string(),
            JsonValue::from_str_val(self.cut_coordinate.clone()),
        );
        map.insert(
            "emit_boundary".to_string(),
            JsonValue::from_str_val(self.emit_boundary.clone()),
        );
        map.insert(
            "emit_cut".to_string(),
            JsonValue::from_i64(self.emit_cut),
        );
        map.insert(
            "parent_id".to_string(),
            JsonValue::from_str_val(self.parent_id.clone()),
        );
        map.insert(
            "post_shed_parent_state".to_string(),
            JsonValue::from_str_val(self.post_shed_parent_state.clone()),
        );
        map.insert(
            "route".to_string(),
            JsonValue::from_str_val(self.route.as_str()),
        );
        map.insert(
            "x_route".to_string(),
            JsonValue::Array(vec![
                JsonValue::from_str_val(self.x_route.num().to_decimal_string()),
                JsonValue::from_str_val(self.x_route.den().to_decimal_string()),
            ]),
        );
        JsonValue::Object(map)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct CoordinateRelation {
    pub relation_id: String,
    pub source_coordinate: String,
    pub target_coordinate: String,
    pub parent_id: String,
    pub emit_cut: i64,
    pub mapped_recipient_cut: i64,
}

impl CoordinateRelation {
    pub fn declare(
        relation_id: impl Into<String>,
        source_coordinate: impl Into<String>,
        target_coordinate: impl Into<String>,
        parent_id: impl Into<String>,
        emit_cut: i64,
        mapped_recipient_cut: i64,
    ) -> CoreResult<Self> {
        let rel = Self {
            relation_id: relation_id.into(),
            source_coordinate: source_coordinate.into(),
            target_coordinate: target_coordinate.into(),
            parent_id: parent_id.into(),
            emit_cut,
            mapped_recipient_cut,
        };
        require_nonempty(&rel.relation_id, "relation_id")?;
        require_nonempty(&rel.source_coordinate, "source_coordinate")?;
        require_nonempty(&rel.target_coordinate, "target_coordinate")?;
        require_nonempty(&rel.parent_id, "parent_id")?;
        if rel.source_coordinate == rel.target_coordinate {
            return Err(CoreError::CoordinateRelationForbidden(
                "coordinate_relation applies only to distinct source and target coordinates"
                    .to_string(),
            ));
        }
        Ok(rel)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum RecipientStage {
    Emitted,
    Delivered,
    Accepted,
    UnacceptedPass,
    Seated,
    PendingUnseated,
    ClosedOrigin,
    Reclosed,
}

impl RecipientStage {
    pub fn is_non_closure_stage(self) -> bool {
        matches!(
            self,
            RecipientStage::Delivered
                | RecipientStage::Accepted
                | RecipientStage::UnacceptedPass
                | RecipientStage::Seated
                | RecipientStage::PendingUnseated
        )
    }

    pub fn allows_transition(self, next: RecipientStage) -> bool {
        matches!(
            (self, next),
            (RecipientStage::Emitted, RecipientStage::Delivered)
                | (RecipientStage::Delivered, RecipientStage::Accepted)
                | (RecipientStage::Delivered, RecipientStage::UnacceptedPass)
                | (RecipientStage::Accepted, RecipientStage::Seated)
                | (RecipientStage::Accepted, RecipientStage::PendingUnseated)
                | (RecipientStage::Seated, RecipientStage::ClosedOrigin)
                | (RecipientStage::Seated, RecipientStage::Reclosed)
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct StageEventWitness {
    pub event_id: String,
    pub stage: RecipientStage,
    pub scope_id: String,
    pub cut_coordinate: String,
    pub cut: i64,
}

impl StageEventWitness {
    pub fn new(
        event_id: impl Into<String>,
        stage: RecipientStage,
        scope_id: impl Into<String>,
        cut_coordinate: impl Into<String>,
        cut: i64,
    ) -> CoreResult<Self> {
        let ev = Self {
            event_id: event_id.into(),
            stage,
            scope_id: scope_id.into(),
            cut_coordinate: cut_coordinate.into(),
            cut,
        };
        require_nonempty(&ev.event_id, "event_id")?;
        if !ev.stage.is_non_closure_stage() {
            return Err(CoreError::ValidationError(format!(
                "unsupported non-closure stage event kind: {:?}",
                ev.stage
            )));
        }
        require_nonempty(&ev.scope_id, "scope_id")?;
        require_nonempty(&ev.cut_coordinate, "cut_coordinate")?;
        Ok(ev)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecipientContext {
    pub recipient_lineage: String,
    pub recipient_scope: String,
    pub cut_coordinate: String,
    pub closure_cut_coordinate: Option<String>,
    pub window: (i64, i64),
    pub coordinate_relation: Option<CoordinateRelation>,
    pub prior_certified_returns: Vec<AdmittedClosureRecord>,
    pub seen_return_ids: BTreeSet<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum RecipientOutcomeStage {
    Emitted,
    Delivered,
    Accepted,
    UnacceptedPass,
    Seated,
    PendingUnseated,
    ClosedOrigin,
    Reclosed,
    ReclosedReplay,
    ReclosedOutOfWindow,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecipientOutcome {
    pub stage: RecipientOutcomeStage,
    pub closure_phase_delta: u8,
    pub reclosure_bip_delta: u8,
    pub blocks_later_seating: bool,
    pub occurrence_j: Option<usize>,
    pub return_id: Option<String>,
    pub last_verified_cut: Option<i64>,
}

/// Passive recipient event lattice evaluator (`eq:recipient-event-lattice`, `F03`, `F06`).
pub struct RecipientLattice;

impl RecipientLattice {
    pub fn evaluate(
        witness: &ShedWitness,
        ctx: &RecipientContext,
        stages: &[RecipientStage],
        closure_record: Option<&AdmittedClosureRecord>,
        stage_events: Option<&BTreeMap<RecipientStage, StageEventWitness>>,
        recipient_scope_map: Option<&ScopeMap>,
    ) -> CoreResult<RecipientOutcome> {
        witness.validate()?;
        require_nonempty(&ctx.recipient_lineage, "recipient_lineage")?;
        require_nonempty(&ctx.recipient_scope, "recipient_scope")?;
        require_nonempty(&ctx.cut_coordinate, "cut_coordinate")?;
        let (u, v) = ctx.window;
        if u >= v {
            return Err(CoreError::ValidationError(
                "ordered recipient window u < v required".to_string(),
            ));
        }
        if stages.is_empty() || stages[0] != RecipientStage::Emitted {
            return Err(CoreError::ValidationError(
                "recipient stage path must start at Emitted".to_string(),
            ));
        }
        for pair in stages.windows(2) {
            if !pair[0].allows_transition(pair[1]) {
                return Err(CoreError::ValidationError(format!(
                    "illegal stage transition {:?} -> {:?}",
                    pair[0], pair[1]
                )));
            }
        }

        // F03: resolve effective emission cut via CoordinateRelation contract.
        let eff_emit_cut = if witness.cut_coordinate == ctx.cut_coordinate {
            if ctx.coordinate_relation.is_some() {
                return Err(CoreError::CoordinateRelationForbidden(
                    "shared cut_coordinate must not attach a cross-coordinate relation".to_string(),
                ));
            }
            witness.emit_cut
        } else {
            let rel = ctx.coordinate_relation.as_ref().ok_or_else(|| {
                CoreError::CoordinateRelationRequired(format!(
                    "incomparable cut coordinates ({:?} != {:?}) require an explicit CoordinateRelation",
                    witness.cut_coordinate, ctx.cut_coordinate
                ))
            })?;
            if rel.source_coordinate != witness.cut_coordinate
                || rel.target_coordinate != ctx.cut_coordinate
                || rel.parent_id != witness.parent_id
                || rel.emit_cut != witness.emit_cut
            {
                return Err(CoreError::ValidationError(
                    "coordinate_relation does not match shed emission witness and recipient coordinate"
                        .to_string(),
                ));
            }
            rel.mapped_recipient_cut
        };

        let events_map = stage_events.ok_or_else(|| {
            CoreError::ValidationError(
                "explicit stage_events mapping required; delivery/acceptance/seating cannot be inferred"
                    .to_string(),
            )
        })?;
        let mut prev_cut = eff_emit_cut;
        let mut prev_stage = RecipientStage::Emitted;
        for &st in &stages[1..] {
            if !st.is_non_closure_stage() {
                continue;
            }
            let ev = events_map.get(&st).ok_or_else(|| {
                CoreError::ValidationError(format!(
                    "missing explicit stage-event witness for traversed recipient stage {st:?}"
                ))
            })?;
            require_nonempty(&ev.event_id, "stage_event.event_id")?;
            if ev.stage != st {
                return Err(CoreError::ValidationError(format!(
                    "stage_event tag mismatch for {st:?}"
                )));
            }
            if ev.scope_id != ctx.recipient_scope {
                return Err(CoreError::ValidationError(format!(
                    "stage_event {st:?} scope_id does not match recipient_scope"
                )));
            }
            if ev.cut_coordinate != ctx.cut_coordinate {
                return Err(CoreError::ValidationError(format!(
                    "stage_event {st:?} cut_coordinate does not match recipient cut_coordinate"
                )));
            }
            if prev_cut > ev.cut {
                return Err(CoreError::ChronologyError(format!(
                    "recipient stage {st:?} at cut {} precedes {prev_stage:?} at cut {prev_cut}",
                    ev.cut
                )));
            }
            prev_cut = ev.cut;
            prev_stage = st;
        }

        let terminal = *stages.last().expect("nonempty");
        if !matches!(terminal, RecipientStage::ClosedOrigin | RecipientStage::Reclosed) {
            if closure_record.is_some() {
                return Err(CoreError::ValidationError(format!(
                    "non-closure recipient stage {terminal:?} must not attach a closure_record"
                )));
            }
            let out_stage = match terminal {
                RecipientStage::Emitted => RecipientOutcomeStage::Emitted,
                RecipientStage::Delivered => RecipientOutcomeStage::Delivered,
                RecipientStage::Accepted => RecipientOutcomeStage::Accepted,
                RecipientStage::UnacceptedPass => RecipientOutcomeStage::UnacceptedPass,
                RecipientStage::Seated => RecipientOutcomeStage::Seated,
                RecipientStage::PendingUnseated => RecipientOutcomeStage::PendingUnseated,
                _ => unreachable!(),
            };
            return Ok(RecipientOutcome {
                stage: out_stage,
                closure_phase_delta: 0,
                reclosure_bip_delta: 0,
                blocks_later_seating: false,
                occurrence_j: None,
                return_id: None,
                last_verified_cut: Some(prev_cut),
            });
        }

        let rec = closure_record.ok_or_else(|| {
            CoreError::ValidationError(
                "recipient origin/reclosure stage requires an AdmittedClosureRecord".to_string(),
            )
        })?;
        rec.validate()?;
        let t_rec = rec.trace();
        let closure_coord = ctx
            .closure_cut_coordinate
            .as_deref()
            .unwrap_or(&ctx.cut_coordinate);
        if closure_coord != ctx.cut_coordinate {
            return Err(CoreError::ValidationError(
                "closure_cut_coordinate differs from recipient cut_coordinate".to_string(),
            ));
        }
        if t_rec.lineage != ctx.recipient_lineage {
            return Err(CoreError::ValidationError(
                "recipient closure lineage mismatch".to_string(),
            ));
        }

        if t_rec.scope != ctx.recipient_scope && recipient_scope_map.is_none() {
            return Err(CoreError::ScopeMapRequired(format!(
                "cross-scope recipient closure ({:?} != {:?}) requires an explicit validated ScopeMap",
                t_rec.scope, ctx.recipient_scope
            )));
        }
        if t_rec.scope == ctx.recipient_scope && recipient_scope_map.is_some() {
            return Err(CoreError::ScopeMapForbidden(
                "same-scope recipient closure must not attach a cross-scope map".to_string(),
            ));
        }

        for (idx, prior) in ctx.prior_certified_returns.iter().enumerate() {
            prior.validate()?;
            let pt = prior.trace();
            if (pt.lineage.as_str(), pt.scope.as_str(), pt.scale)
                != (t_rec.lineage.as_str(), t_rec.scope.as_str(), t_rec.scale)
            {
                return Err(CoreError::ValidationError(format!(
                    "prior recipient record {idx} mismatches closure lineage/scope/scale"
                )));
            }
        }

        let completion_cut = t_rec.completion_cut();
        if terminal == RecipientStage::ClosedOrigin {
            if !ctx.prior_certified_returns.is_empty() {
                return Err(CoreError::ValidationError(
                    "closed_origin applies only to initial recipient closure (no prior origin)"
                        .to_string(),
                ));
            }
            let wit_origin = project_admitted_lineage_occurrences(
                std::slice::from_ref(rec),
                &ctx.cut_coordinate,
                "C",
                None,
            )?
            .into_iter()
            .next()
            .expect("single record");
            if t_rec.scope != ctx.recipient_scope {
                recipient_scope_map
                    .expect("checked above")
                    .validate_for_witness(&wit_origin, &ctx.recipient_scope, &ctx.recipient_lineage)?;
            }
            if !(eff_emit_cut <= prev_cut && prev_cut <= completion_cut) {
                return Err(CoreError::ChronologyError(format!(
                    "recipient completion at cut {completion_cut} precedes {prev_stage:?} at cut {prev_cut}"
                )));
            }
            let in_win = u < completion_cut && completion_cut <= v;
            return Ok(RecipientOutcome {
                stage: RecipientOutcomeStage::ClosedOrigin,
                closure_phase_delta: u8::from(in_win),
                reclosure_bip_delta: 0,
                blocks_later_seating: false,
                occurrence_j: Some(0),
                return_id: Some(t_rec.return_id()),
                last_verified_cut: Some(completion_cut),
            });
        }

        // Terminal == Reclosed
        if ctx.prior_certified_returns.is_empty() {
            return Err(CoreError::ValidationError(
                "reclosure requires a pre-existing admitted recipient origin closure (j=0)"
                    .to_string(),
            ));
        }
        let projected_prior = project_admitted_lineage_occurrences(
            &ctx.prior_certified_returns,
            &ctx.cut_coordinate,
            "C",
            None,
        )?;
        let prior_occ_by_id: BTreeMap<String, ReturnWitness> = projected_prior
            .into_iter()
            .map(|r| (r.return_id.clone(), r))
            .collect();
        let prior_by_id: BTreeMap<String, &AdmittedClosureRecord> = ctx
            .prior_certified_returns
            .iter()
            .map(|r| (r.trace().return_id(), r))
            .collect();

        // F06: replay of an already-certified entry (e.g. j=1 or j=2) including cross-scope mapped replay!
        let rec_rid = t_rec.return_id();
        if let Some(&prior_rec) = prior_by_id.get(&rec_rid) {
            if prior_rec != rec {
                return Err(CoreError::CollisionError(
                    "replayed return_id payload conflicts with prior admitted recipient record"
                        .to_string(),
                ));
            }
            let wit_rec = prior_occ_by_id.get(&rec_rid).expect("projected");
            if t_rec.scope != ctx.recipient_scope {
                recipient_scope_map
                    .expect("checked above")
                    .validate_for_witness(wit_rec, &ctx.recipient_scope, &ctx.recipient_lineage)?;
            }
            if !(eff_emit_cut <= prev_cut && prev_cut <= completion_cut) {
                return Err(CoreError::ChronologyError(format!(
                    "recipient completion at cut {completion_cut} precedes {prev_stage:?} at cut {prev_cut}"
                )));
            }
            return Ok(RecipientOutcome {
                stage: RecipientOutcomeStage::ReclosedReplay,
                closure_phase_delta: 0,
                reclosure_bip_delta: 0,
                blocks_later_seating: false,
                occurrence_j: Some(wit_rec.occurrence_j),
                return_id: Some(rec_rid),
                last_verified_cut: Some(completion_cut),
            });
        }

        let mut full_records = ctx.prior_certified_returns.clone();
        full_records.push(rec.clone());
        let projected_full =
            project_admitted_lineage_occurrences(&full_records, &ctx.cut_coordinate, "C", None)?;
        let wit_rec = projected_full.last().expect("nonempty");
        if t_rec.scope != ctx.recipient_scope {
            recipient_scope_map
                .expect("checked above")
                .validate_for_witness(wit_rec, &ctx.recipient_scope, &ctx.recipient_lineage)?;
        }
        if !(eff_emit_cut <= prev_cut && prev_cut <= completion_cut) {
            return Err(CoreError::ChronologyError(format!(
                "recipient completion at cut {completion_cut} precedes {prev_stage:?} at cut {prev_cut}"
            )));
        }
        let occ_j = wit_rec.occurrence_j;
        if ctx.seen_return_ids.contains(&rec_rid) {
            return Ok(RecipientOutcome {
                stage: RecipientOutcomeStage::ReclosedReplay,
                closure_phase_delta: 0,
                reclosure_bip_delta: 0,
                blocks_later_seating: false,
                occurrence_j: Some(occ_j),
                return_id: Some(rec_rid),
                last_verified_cut: Some(completion_cut),
            });
        }
        if !(u < completion_cut && completion_cut <= v) {
            return Ok(RecipientOutcome {
                stage: RecipientOutcomeStage::ReclosedOutOfWindow,
                closure_phase_delta: 0,
                reclosure_bip_delta: 0,
                blocks_later_seating: false,
                occurrence_j: Some(occ_j),
                return_id: Some(rec_rid),
                last_verified_cut: Some(completion_cut),
            });
        }
        Ok(RecipientOutcome {
            stage: RecipientOutcomeStage::Reclosed,
            closure_phase_delta: 1,
            reclosure_bip_delta: 1,
            blocks_later_seating: false,
            occurrence_j: Some(occ_j),
            return_id: Some(rec_rid),
            last_verified_cut: Some(completion_cut),
        })
    }
}

// ============================================================================
// Native Cadence Ratio vs Single-Contact Stage Association
// (`eq:cadence-ratio`, `eq:wavelet-reference-timing`)
// ============================================================================

/// Exact native cadence ratio `r_{a|r}(omega) = q_a / q_r`.
///
/// Requires distinct non-empty identities (`id_a != id_r`), `coupled == true`,
/// `q_a >= 0`, `q_r > 0`, and `bound_contacts >= 2`.
pub fn native_cadence_ratio(
    id_a: &str,
    q_a: &BigInt,
    id_r: &str,
    q_r: &BigInt,
    coupled: bool,
    bound_contacts: usize,
) -> CoreResult<Ratio> {
    require_nonempty(id_a, "id_a")?;
    require_nonempty(id_r, "id_r")?;
    if id_a == id_r {
        return Err(CoreError::AliasingError(
            "aliased wave identities rejected in native_cadence_ratio".to_string(),
        ));
    }
    if !coupled {
        return Err(CoreError::ValidationError(
            "uncoupled comparison rejected in native_cadence_ratio".to_string(),
        ));
    }
    if q_a.is_negative() {
        return Err(CoreError::DomainError(
            "q_a must be non-negative".to_string(),
        ));
    }
    if !q_r.is_positive() {
        return Err(CoreError::ZeroDivision);
    }
    if bound_contacts < 2 {
        return Err(CoreError::ValidationError(
            "repeated bound contacts (>= 2) required for biz^TF cadence ratio".to_string(),
        ));
    }
    Ratio::new(q_a.clone(), q_r.clone())
}

/// Separately typed passive window/stage association for a single delivered contact
/// (`bound_contacts == 1`, `is_cadence_ratio == false`, `biz_tf == None`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SingleContactStageAssociation {
    pub wavelet_id: String,
    pub parent_id: String,
    pub reference_id: String,
    pub window: (i64, i64),
    pub is_cadence_ratio: bool,
    pub biz_tf: Option<Ratio>,
}

impl SingleContactStageAssociation {
    pub fn new(
        wavelet_id: impl Into<String>,
        parent_id: impl Into<String>,
        reference_id: impl Into<String>,
        window: (i64, i64),
        bound_contacts: usize,
    ) -> CoreResult<Self> {
        let wid = wavelet_id.into();
        let pid = parent_id.into();
        let rid = reference_id.into();
        require_nonempty(&wid, "wavelet_id")?;
        require_nonempty(&pid, "parent_id")?;
        require_nonempty(&rid, "reference_id")?;
        if wid == rid || pid == rid {
            return Err(CoreError::AliasingError(
                "aliased identities rejected in SingleContactStageAssociation".to_string(),
            ));
        }
        if window.0 >= window.1 {
            return Err(CoreError::ValidationError(
                "ordered window u < v required".to_string(),
            ));
        }
        if bound_contacts != 1 {
            return Err(CoreError::ValidationError(
                "single-contact stage association requires bound_contacts == 1".to_string(),
            ));
        }
        Ok(Self {
            wavelet_id: wid,
            parent_id: pid,
            reference_id: rid,
            window,
            is_cadence_ratio: false,
            biz_tf: None,
        })
    }
}
