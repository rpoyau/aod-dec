use std::collections::{BTreeMap, BTreeSet};

use crate::bigint::BigInt;
use crate::error::{CoreError, CoreResult};
use crate::json::JsonValue;
use crate::ratio::Ratio;
use crate::ternary::{Q4, T1};

const MAX_TRACE_DEPTH: usize = 64;

fn require_label(s: &str, field: &str) -> CoreResult<()> {
    if s.trim().is_empty() {
        return Err(CoreError::ValidationError(format!(
            "{field} must be nonempty"
        )));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EventKind {
    Step,
    Hinge,
    Dwell,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::Step => "step",
            EventKind::Hinge => "hinge",
            EventKind::Dwell => "dwell",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Event {
    pub ordinal: usize,
    pub kind: EventKind,
    pub source: u8,
    pub target: u8,
    pub orientation: i8,
}

impl Event {
    pub fn to_json(&self) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert("kind".to_string(), JsonValue::from_str_val(self.kind.as_str()));
        map.insert("ordinal".to_string(), JsonValue::from_usize(self.ordinal));
        map.insert(
            "orientation".to_string(),
            JsonValue::from_i64(self.orientation as i64),
        );
        map.insert("source".to_string(), JsonValue::from_i64(self.source as i64));
        map.insert("target".to_string(), JsonValue::from_i64(self.target as i64));
        JsonValue::Object(map)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Closure {
    pub begin: usize,
    pub end: usize,
    pub digest: String,
}

impl Closure {
    pub fn to_json(&self) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert("begin".to_string(), JsonValue::from_usize(self.begin));
        map.insert(
            "digest".to_string(),
            JsonValue::from_str_val(self.digest.clone()),
        );
        map.insert("end".to_string(), JsonValue::from_usize(self.end));
        JsonValue::Object(map)
    }
}

/// Finite retained Q4 closure trace preserving v1 dual JSON serialization (`P0-3`):
/// - `trace_prefix_json(end)` for SHA-256 closure/fusion digest (`"start": <int>`,
///   `"parents": ["<last_closure_digest>", ...]`, NO `"closures"` field, matching `dec_core.py:213`).
/// - `to_render_json()` / `render_v1()` for `emit` (`"start": {"value": <int>}`,
///   `"parents": [<full recursive Trace JSON>]`, WITH `"closures"`, matching `dec_core.py:494`).
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Trace {
    pub scope: String,
    pub lineage: String,
    pub start: Q4,
    pub events: Vec<Event>,
    pub closures: Vec<Closure>,
    pub parents: Vec<Trace>,
    pub proposal: String,
}

impl Trace {
    pub fn new(scope: impl Into<String>, lineage: impl Into<String>, start: Q4) -> CoreResult<Self> {
        let t = Self {
            scope: scope.into(),
            lineage: lineage.into(),
            start,
            events: Vec::new(),
            closures: Vec::new(),
            parents: Vec::new(),
            proposal: String::new(),
        };
        t.validate()?;
        Ok(t)
    }

    /// Exact v1 `trace_prefix(t, end)` JSON object used for SHA-256 closure certification.
    pub fn trace_prefix_json(&self, end: usize) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert(
            "events".to_string(),
            JsonValue::Array(self.events[..end].iter().map(Event::to_json).collect()),
        );
        map.insert(
            "lineage".to_string(),
            JsonValue::from_str_val(self.lineage.clone()),
        );
        let parent_digests = self
            .parents
            .iter()
            .map(|p| {
                let dig = p
                    .closures
                    .last()
                    .map(|c| c.digest.clone())
                    .unwrap_or_default();
                JsonValue::from_str_val(dig)
            })
            .collect();
        map.insert("parents".to_string(), JsonValue::Array(parent_digests));
        map.insert(
            "proposal".to_string(),
            JsonValue::from_str_val(self.proposal.clone()),
        );
        map.insert(
            "scope".to_string(),
            JsonValue::from_str_val(self.scope.clone()),
        );
        map.insert(
            "start".to_string(),
            JsonValue::from_i64(self.start.value() as i64),
        );
        JsonValue::Object(map)
    }

    /// Exact v1 `asdict(trace)` JSON object used by `emit` / `render`.
    pub fn to_render_json(&self) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert(
            "closures".to_string(),
            JsonValue::Array(self.closures.iter().map(Closure::to_json).collect()),
        );
        map.insert(
            "events".to_string(),
            JsonValue::Array(self.events.iter().map(Event::to_json).collect()),
        );
        map.insert(
            "lineage".to_string(),
            JsonValue::from_str_val(self.lineage.clone()),
        );
        map.insert(
            "parents".to_string(),
            JsonValue::Array(self.parents.iter().map(Trace::to_render_json).collect()),
        );
        map.insert(
            "proposal".to_string(),
            JsonValue::from_str_val(self.proposal.clone()),
        );
        map.insert(
            "scope".to_string(),
            JsonValue::from_str_val(self.scope.clone()),
        );
        let mut start_obj = BTreeMap::new();
        start_obj.insert(
            "value".to_string(),
            JsonValue::from_i64(self.start.value() as i64),
        );
        map.insert("start".to_string(), JsonValue::Object(start_obj));
        JsonValue::Object(map)
    }

    pub fn render_v1(&self) -> String {
        self.to_render_json().to_compact_ascii_json()
    }

    pub fn validate(&self) -> CoreResult<u8> {
        self.validate_depth(0)
    }

    fn validate_depth(&self, depth: usize) -> CoreResult<u8> {
        if depth >= MAX_TRACE_DEPTH {
            return Err(CoreError::ValidationError(
                "excessively nested trace".to_string(),
            ));
        }
        require_label(&self.scope, "scope")?;
        require_label(&self.lineage, "lineage")?;

        let mut inner_digests = BTreeSet::new();
        if !self.parents.is_empty() {
            if self.parents.len() != 2 {
                return Err(CoreError::ValidationError(
                    "fusion requires two retained parents in this subset".to_string(),
                ));
            }
            for p in &self.parents {
                p.validate_depth(depth + 1)?;
                let last_end = p.closures.last().map(|c| c.end);
                if last_end != Some(p.events.len()) {
                    return Err(CoreError::ValidationError(
                        "fusion parent is not closed".to_string(),
                    ));
                }
                for c in &p.closures {
                    inner_digests.insert(c.digest.clone());
                }
            }
            if (self.parents[0].scope.as_str(), self.parents[0].lineage.as_str())
                == (self.parents[1].scope.as_str(), self.parents[1].lineage.as_str())
            {
                return Err(CoreError::ValidationError(
                    "fusion parents must have distinct lineages".to_string(),
                ));
            }
            if self.scope == self.parents[0].scope || self.scope == self.parents[1].scope {
                return Err(CoreError::ValidationError(
                    "outer scope must differ from inner scopes".to_string(),
                ));
            }
            let expected = fusion_digest(&self.parents[0], &self.parents[1], &self.scope, &self.lineage)?;
            if self.proposal != expected {
                return Err(CoreError::ValidationError(
                    "outer trace does not match retained fusion proposal".to_string(),
                ));
            }
        } else if !self.proposal.is_empty() {
            return Err(CoreError::ValidationError(
                "orphan fusion proposal".to_string(),
            ));
        }

        let mut begin = 0usize;
        for cert in &self.closures {
            if cert.begin != begin || !(begin < cert.end && cert.end <= self.events.len()) {
                return Err(CoreError::ValidationError(
                    "invalid/replayed closure range".to_string(),
                ));
            }
            self.validate_segment(begin, cert.end, true)?;
            let expected_dig = self.trace_prefix_json(cert.end).sha256_hex();
            if cert.digest != expected_dig {
                return Err(CoreError::ValidationError(
                    "closure certificate does not match retained history".to_string(),
                ));
            }
            // M3 eq:fold-outer-closure: outer closure digests must stay disjoint from inner digests.
            if inner_digests.contains(&cert.digest) {
                return Err(CoreError::ValidationError(
                    "outer closure digest must be disjoint from inner closure digests".to_string(),
                ));
            }
            begin = cert.end;
        }
        self.validate_segment(begin, self.events.len(), false)
    }

    fn validate_segment(&self, begin: usize, end: usize, complete: bool) -> CoreResult<u8> {
        if begin > end || end > self.events.len() {
            return Err(CoreError::ValidationError(
                "invalid trace segment".to_string(),
            ));
        }
        let mut position = self.start.value();
        let mut orientation: Option<i8> = None;
        let mut hinged = false;
        let mut moves = 0usize;

        for (offset, event) in self.events[begin..end].iter().enumerate() {
            let expected_ordinal = begin + offset + 1;
            if event.ordinal != expected_ordinal {
                return Err(CoreError::ValidationError(
                    "invalid or repeated event identity".to_string(),
                ));
            }
            if event.source != position {
                return Err(CoreError::ValidationError(
                    "discontinuous retained trace".to_string(),
                ));
            }
            let src_q4 = Q4::new(position)?;
            let dst_q4 = Q4::new(event.target)?;

            match event.kind {
                EventKind::Step => {
                    if event.orientation != -1 && event.orientation != 1 {
                        return Err(CoreError::ValidationError(
                            "step requires nonzero orientation".to_string(),
                        ));
                    }
                    if !src_q4.is_hamming1(dst_q4) {
                        return Err(CoreError::HammingViolation(
                            "atomic move must be Hamming-1".to_string(),
                        ));
                    }
                    if orientation.is_none() {
                        if hinged {
                            return Err(CoreError::ValidationError(
                                "hinge cannot precede outgoing branch".to_string(),
                            ));
                        }
                        orientation = Some(event.orientation);
                    }
                    let expected_orient = if hinged {
                        -orientation.unwrap()
                    } else {
                        orientation.unwrap()
                    };
                    if event.orientation != expected_orient {
                        return Err(CoreError::ValidationError(
                            "counterbranch orientation requires structural hinge".to_string(),
                        ));
                    }
                    moves += 1;
                    position = event.target;
                }
                EventKind::Hinge => {
                    if orientation.is_none() || hinged {
                        return Err(CoreError::ValidationError(
                            "one structural hinge must follow outgoing branch".to_string(),
                        ));
                    }
                    if event.target != position || event.orientation != 0 {
                        return Err(CoreError::ValidationError(
                            "invalid structural hinge".to_string(),
                        ));
                    }
                    hinged = true;
                }
                EventKind::Dwell => {
                    if event.target != position || event.orientation != 0 {
                        return Err(CoreError::ValidationError("invalid dwell".to_string()));
                    }
                }
            }
        }

        if complete {
            if moves < 2 || !hinged {
                return Err(CoreError::ValidationError(
                    "closure needs outgoing branch, hinge and counterbranch".to_string(),
                ));
            }
            if position != self.start.value() {
                return Err(CoreError::ValidationError(
                    "open trace does not return to retained start".to_string(),
                ));
            }
            let last = &self.events[end - 1];
            if last.kind != EventKind::Step {
                return Err(CoreError::ValidationError(
                    "closure must identify a return event, not dwell".to_string(),
                ));
            }
            if last.orientation != -orientation.unwrap() {
                return Err(CoreError::ValidationError(
                    "return event is not counterbranch".to_string(),
                ));
            }
        }
        Ok(position)
    }

    pub fn step(&self, target: Q4, orientation: T1) -> CoreResult<Self> {
        let pos = self.validate()?;
        let mut out = self.clone();
        out.events.push(Event {
            ordinal: out.events.len() + 1,
            kind: EventKind::Step,
            source: pos,
            target: target.value(),
            orientation: orientation.value(),
        });
        out.validate()?;
        Ok(out)
    }

    pub fn hinge(&self) -> CoreResult<Self> {
        let pos = self.validate()?;
        let mut out = self.clone();
        out.events.push(Event {
            ordinal: out.events.len() + 1,
            kind: EventKind::Hinge,
            source: pos,
            target: pos,
            orientation: 0,
        });
        out.validate()?;
        Ok(out)
    }

    pub fn dwell(&self) -> CoreResult<Self> {
        let pos = self.validate()?;
        let mut out = self.clone();
        out.events.push(Event {
            ordinal: out.events.len() + 1,
            kind: EventKind::Dwell,
            source: pos,
            target: pos,
            orientation: 0,
        });
        out.validate()?;
        Ok(out)
    }

    pub fn close(&self) -> CoreResult<Self> {
        self.validate()?;
        let begin = self.closures.last().map_or(0, |c| c.end);
        if begin == self.events.len() && !self.closures.is_empty() {
            // Persistent certification/replay does not create a return event.
            return Ok(self.clone());
        }
        self.validate_segment(begin, self.events.len(), true)?;
        let digest = self.trace_prefix_json(self.events.len()).sha256_hex();
        let mut out = self.clone();
        out.closures.push(Closure {
            begin,
            end: out.events.len(),
            digest,
        });
        out.validate()?;
        Ok(out)
    }

    pub fn phases(&self) -> CoreResult<usize> {
        self.validate()?;
        Ok(self.closures.len())
    }

    pub fn beats(&self) -> CoreResult<usize> {
        Ok(self.phases()?.saturating_sub(1))
    }

    pub fn rate(&self) -> CoreResult<Ratio> {
        self.validate()?;
        if self.events.is_empty() {
            return Err(CoreError::ValidationError(
                "rate requires a positive committed event span".to_string(),
            ));
        }
        Ratio::new(
            BigInt::from_u64(self.closures.len() as u64),
            BigInt::from_u64(self.events.len() as u64),
        )
    }

    /// Expose retained inner parent traces (`eq:expose-inner-lineage`).
    pub fn expose_inner(&self) -> CoreResult<&[Trace]> {
        self.validate()?;
        Ok(&self.parents)
    }
}

pub fn fusion_digest(a: &Trace, b: &Trace, scope: &str, lineage: &str) -> CoreResult<String> {
    let a_dig = a
        .closures
        .last()
        .ok_or_else(|| CoreError::ValidationError("fusion left parent is not closed".to_string()))?
        .digest
        .clone();
    let b_dig = b
        .closures
        .last()
        .ok_or_else(|| CoreError::ValidationError("fusion right parent is not closed".to_string()))?
        .digest
        .clone();
    let mut map = BTreeMap::new();
    map.insert(
        "lineage".to_string(),
        JsonValue::from_str_val(lineage.to_string()),
    );
    map.insert(
        "parents".to_string(),
        JsonValue::Array(vec![
            JsonValue::from_str_val(a_dig),
            JsonValue::from_str_val(b_dig),
        ]),
    );
    map.insert(
        "scope".to_string(),
        JsonValue::from_str_val(scope.to_string()),
    );
    Ok(JsonValue::Object(map).sha256_hex())
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Fusion {
    pub left: Trace,
    pub right: Trace,
    pub scope: String,
    pub lineage: String,
    pub digest: String,
}

impl Fusion {
    pub fn fold(
        a: &Trace,
        b: &Trace,
        scope: impl Into<String>,
        lineage: impl Into<String>,
    ) -> CoreResult<Self> {
        let scope = scope.into();
        let lineage = lineage.into();
        for t in [a, b] {
            t.validate()?;
            if t.closures.last().map(|c| c.end) != Some(t.events.len()) {
                return Err(CoreError::ValidationError(
                    "fold requires closed retained inner inputs".to_string(),
                ));
            }
        }
        if (a.scope.as_str(), a.lineage.as_str()) == (b.scope.as_str(), b.lineage.as_str()) {
            return Err(CoreError::ValidationError(
                "fold requires distinct inner lineages".to_string(),
            ));
        }
        require_label(&scope, "scope")?;
        require_label(&lineage, "lineage")?;
        if scope == a.scope || scope == b.scope {
            return Err(CoreError::ValidationError(
                "outer scope must differ from inner scopes".to_string(),
            ));
        }
        let digest = fusion_digest(a, b, &scope, &lineage)?;
        Ok(Self {
            left: a.clone(),
            right: b.clone(),
            scope,
            lineage,
            digest,
        })
    }

    pub fn validate(&self) -> CoreResult<()> {
        let expected = Self::fold(&self.left, &self.right, self.scope.clone(), self.lineage.clone())?;
        if expected != *self {
            return Err(CoreError::ValidationError(
                "modified fusion proposal".to_string(),
            ));
        }
        Ok(())
    }

    pub fn outer(&self, start: Q4) -> CoreResult<Trace> {
        self.validate()?;
        let t = Trace {
            scope: self.scope.clone(),
            lineage: self.lineage.clone(),
            start,
            events: Vec::new(),
            closures: Vec::new(),
            parents: vec![self.left.clone(), self.right.clone()],
            proposal: self.digest.clone(),
        };
        t.validate()?;
        Ok(t)
    }

    pub fn fuse(&self, outer_trace: &Trace) -> CoreResult<Trace> {
        self.validate()?;
        outer_trace.validate()?;
        let expected_parents = vec![self.left.clone(), self.right.clone()];
        if outer_trace.scope != self.scope
            || outer_trace.lineage != self.lineage
            || outer_trace.parents != expected_parents
            || outer_trace.proposal != self.digest
        {
            return Err(CoreError::ValidationError(
                "outer trace belongs to a different proposal".to_string(),
            ));
        }
        if outer_trace.closures.last().map(|c| c.end) != Some(outer_trace.events.len()) {
            return Err(CoreError::ValidationError(
                "fusion requires independently witnessed outer closure".to_string(),
            ));
        }
        // Explicitly enforce eq:fold-outer-closure outer/inner closure digest disjointness.
        let inner_digests: BTreeSet<&str> = self
            .left
            .closures
            .iter()
            .chain(self.right.closures.iter())
            .map(|c| c.digest.as_str())
            .collect();
        for c in &outer_trace.closures {
            if inner_digests.contains(c.digest.as_str()) {
                return Err(CoreError::ValidationError(
                    "outer closure reused an inner closure digest".to_string(),
                ));
            }
        }
        Ok(outer_trace.clone())
    }

    pub fn to_render_json(&self) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert(
            "digest".to_string(),
            JsonValue::from_str_val(self.digest.clone()),
        );
        map.insert("left".to_string(), self.left.to_render_json());
        map.insert(
            "lineage".to_string(),
            JsonValue::from_str_val(self.lineage.clone()),
        );
        map.insert("right".to_string(), self.right.to_render_json());
        map.insert(
            "scope".to_string(),
            JsonValue::from_str_val(self.scope.clone()),
        );
        JsonValue::Object(map)
    }

    pub fn render_v1(&self) -> String {
        self.to_render_json().to_compact_ascii_json()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Coupling {
    pub left: Trace,
    pub right: Trace,
    pub window: String,
    pub contacts: Vec<(usize, usize)>,
}

impl Coupling {
    pub fn new(left: &Trace, right: &Trace, window: impl Into<String>) -> CoreResult<Self> {
        let window = window.into();
        left.validate()?;
        right.validate()?;
        require_label(&window, "window")?;
        if left.scope != right.scope || left.lineage == right.lineage {
            return Err(CoreError::ValidationError(
                "coupling needs distinct lineages in common scope".to_string(),
            ));
        }
        if left.events.len() != right.events.len() || left.events.is_empty() {
            return Err(CoreError::ValidationError(
                "coupling needs equal positive committed event spans".to_string(),
            ));
        }
        Ok(Self {
            left: left.clone(),
            right: right.clone(),
            window,
            contacts: Vec::new(),
        })
    }

    pub fn validate(&self) -> CoreResult<()> {
        let _ = Self::new(&self.left, &self.right, self.window.clone())?;
        let a_ids: BTreeSet<usize> = self.left.closures.iter().map(|c| c.end).collect();
        let b_ids: BTreeSet<usize> = self.right.closures.iter().map(|c| c.end).collect();
        let mut prev_a = 0usize;
        let mut prev_b = 0usize;
        for &(a, b) in &self.contacts {
            if !a_ids.contains(&a) || !b_ids.contains(&b) {
                return Err(CoreError::ValidationError(
                    "contact must bind certified return events".to_string(),
                ));
            }
            if a <= prev_a || b <= prev_b {
                return Err(CoreError::ValidationError(
                    "contacts must be distinct, ordered and unreplayed".to_string(),
                ));
            }
            prev_a = a;
            prev_b = b;
        }
        Ok(())
    }

    pub fn contact(&self, a: usize, b: usize) -> CoreResult<Self> {
        self.validate()?;
        let mut out = self.clone();
        out.contacts.push((a, b));
        out.validate()?;
        Ok(out)
    }

    fn require_comparison(&self) -> CoreResult<()> {
        self.validate()?;
        if self.contacts.len() < 2 || self.left.phases()? < 2 || self.right.phases()? < 2 {
            return Err(CoreError::ValidationError(
                "comparison needs recurrent lineages and repeated bound contacts".to_string(),
            ));
        }
        Ok(())
    }

    pub fn cadence(&self) -> CoreResult<Ratio> {
        self.require_comparison()?;
        Ratio::new(
            BigInt::from_u64(self.left.phases()? as u64),
            BigInt::from_u64(self.right.phases()? as u64),
        )
    }

    pub fn lag(&self) -> CoreResult<BigInt> {
        self.require_comparison()?;
        let (a, b) = *self.contacts.last().expect("at least 2 contacts");
        Ok(&BigInt::from_u64(a as u64) - &BigInt::from_u64(b as u64))
    }

    pub fn drift(&self) -> CoreResult<BigInt> {
        let end_lag = self.lag()?;
        let (a0, b0) = self.contacts[0];
        let start_lag = &BigInt::from_u64(a0 as u64) - &BigInt::from_u64(b0 as u64);
        Ok(&end_lag - &start_lag)
    }

    pub fn to_render_json(&self) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert(
            "contacts".to_string(),
            JsonValue::Array(
                self.contacts
                    .iter()
                    .map(|&(a, b)| {
                        JsonValue::Array(vec![JsonValue::from_usize(a), JsonValue::from_usize(b)])
                    })
                    .collect(),
            ),
        );
        map.insert("left".to_string(), self.left.to_render_json());
        map.insert("right".to_string(), self.right.to_render_json());
        map.insert(
            "window".to_string(),
            JsonValue::from_str_val(self.window.clone()),
        );
        JsonValue::Object(map)
    }

    pub fn render_v1(&self) -> String {
        self.to_render_json().to_compact_ascii_json()
    }
}

// --- M3 eq:duon-current & eq:duad-same-current ---

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum DionPolarity {
    Alpha,
    Omega,
}

/// One of the 12 distinct Duon support signatures `(D_1, mu, D_2)`
/// with `D_i in {D_alpha, D_Omega}` and `mu in {-1, 0, +1}`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DuonVariant {
    pub first: DionPolarity,
    pub hinge_role: T1,
    pub second: DionPolarity,
}

impl DuonVariant {
    pub const fn new(first: DionPolarity, hinge_role: T1, second: DionPolarity) -> Self {
        Self {
            first,
            hinge_role,
            second,
        }
    }

    /// Enumerate all 12 distinct Duon variants.
    pub fn all_variants() -> [DuonVariant; 12] {
        use DionPolarity::{Alpha, Omega};
        [
            Self::new(Alpha, T1::NEG, Alpha),
            Self::new(Alpha, T1::NEG, Omega),
            Self::new(Alpha, T1::ZERO, Alpha),
            Self::new(Alpha, T1::ZERO, Omega),
            Self::new(Alpha, T1::POS, Alpha),
            Self::new(Alpha, T1::POS, Omega),
            Self::new(Omega, T1::NEG, Alpha),
            Self::new(Omega, T1::NEG, Omega),
            Self::new(Omega, T1::ZERO, Alpha),
            Self::new(Omega, T1::ZERO, Omega),
            Self::new(Omega, T1::POS, Alpha),
            Self::new(Omega, T1::POS, Omega),
        ]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum SeatState {
    OpenDuon,
    SeatedDuad,
}

impl SeatState {
    pub fn as_str(self) -> &'static str {
        match self {
            SeatState::OpenDuon => "open_duon",
            SeatState::SeatedDuad => "seated_duad",
        }
    }
}

/// Carried Duon/Duad current (`eq:duon-current`, `eq:duad-same-current`).
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct DuonCurrent {
    pub duon_id: String,
    pub lineage_id: String,
    pub prefix_digest: String,
    pub variant: DuonVariant,
    pub hinge_0h: bool,
    pub seat_state: SeatState,
}

impl DuonCurrent {
    pub fn open(
        duon_id: impl Into<String>,
        lineage_id: impl Into<String>,
        prefix_digest: impl Into<String>,
        variant: DuonVariant,
        hinge_0h: bool,
    ) -> CoreResult<Self> {
        let duon_id = duon_id.into();
        let lineage_id = lineage_id.into();
        let prefix_digest = prefix_digest.into();
        require_label(&duon_id, "duon_id")?;
        require_label(&lineage_id, "lineage_id")?;
        require_label(&prefix_digest, "prefix_digest")?;
        Ok(Self {
            duon_id,
            lineage_id,
            prefix_digest,
            variant,
            hinge_0h,
            seat_state: SeatState::OpenDuon,
        })
    }

    /// Seat an open-seat Duon into a `SeatedDuad` preserving identical `duon_id` and `0_H` hinge.
    pub fn seat_duad(&self) -> CoreResult<Self> {
        self.seat_duad_checked(None, true)
    }

    /// Hostile-testable seating transition (`eq:duad-same-current`):
    /// requires `OpenDuon`, `self.hinge_0h && keep_hinge`, and identical `duon_id`.
    pub fn seat_duad_checked(
        &self,
        new_duon_id: Option<&str>,
        keep_hinge: bool,
    ) -> CoreResult<Self> {
        if self.seat_state != SeatState::OpenDuon {
            return Err(CoreError::ValidationError(
                "only an open-seat Duon can seat into seated_duad".to_string(),
            ));
        }
        if !keep_hinge || !self.hinge_0h {
            return Err(CoreError::ValidationError(
                "structural 0_H hinge required to seat Duad".to_string(),
            ));
        }
        let target_id = new_duon_id.unwrap_or(&self.duon_id);
        if target_id != self.duon_id {
            return Err(CoreError::ValidationError(
                "Duad seating must preserve identical duon_id".to_string(),
            ));
        }
        Ok(Self {
            duon_id: self.duon_id.clone(),
            lineage_id: self.lineage_id.clone(),
            prefix_digest: self.prefix_digest.clone(),
            variant: self.variant,
            hinge_0h: true,
            seat_state: SeatState::SeatedDuad,
        })
    }
}
