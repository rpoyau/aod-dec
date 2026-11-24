use std::collections::BTreeMap;

use aod_core::bigint::BigInt;
use aod_core::error::CoreResult;
use aod_core::json::JsonValue;
use aod_core::ledger::{HingeSlide, Kernel, LedgerRow, RouteStatus};
use aod_core::ratio::Ratio;
use aod_core::support::SupportForm;
use aod_core::ternary::{Q4, T1};
use aod_core::wavelet::{
    native_cadence_ratio, ShedRoute, ShedWitness, SingleContactStageAssociation,
};
use aod_core::witness::Trace;

/// Explicit provenance status tag required on every sandbox simulation record (`P0-2`).
pub const MODEL_STATUS: &str = "DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum AxisDir {
    PosX,
    NegX,
    PosY,
    NegY,
    PosZ,
    NegZ,
}

impl AxisDir {
    pub fn delta(self) -> [i64; 3] {
        match self {
            AxisDir::PosX => [1, 0, 0],
            AxisDir::NegX => [-1, 0, 0],
            AxisDir::PosY => [0, 1, 0],
            AxisDir::NegY => [0, -1, 0],
            AxisDir::PosZ => [0, 0, 1],
            AxisDir::NegZ => [0, 0, -1],
        }
    }

    pub fn reflect(self) -> Self {
        match self {
            AxisDir::PosX => AxisDir::NegX,
            AxisDir::NegX => AxisDir::PosX,
            AxisDir::PosY => AxisDir::NegY,
            AxisDir::NegY => AxisDir::PosY,
            AxisDir::PosZ => AxisDir::NegZ,
            AxisDir::NegZ => AxisDir::PosZ,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AxisDir::PosX => "+X",
            AxisDir::NegX => "-X",
            AxisDir::PosY => "+Y",
            AxisDir::NegY => "-Y",
            AxisDir::PosZ => "+Z",
            AxisDir::NegZ => "-Z",
        }
    }

    pub fn to_code(self) -> u8 {
        match self {
            AxisDir::PosX => 0,
            AxisDir::NegX => 1,
            AxisDir::PosY => 2,
            AxisDir::NegY => 3,
            AxisDir::PosZ => 4,
            AxisDir::NegZ => 5,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(AxisDir::PosX),
            1 => Some(AxisDir::NegX),
            2 => Some(AxisDir::PosY),
            3 => Some(AxisDir::NegY),
            4 => Some(AxisDir::PosZ),
            5 => Some(AxisDir::NegZ),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum WallPolicy {
    Open,
    Closed,
}

impl WallPolicy {
    pub fn to_code(self) -> u8 {
        match self {
            WallPolicy::Open => 0,
            WallPolicy::Closed => 1,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(WallPolicy::Open),
            1 => Some(WallPolicy::Closed),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ProbeKind {
    Tracer,
    Sensor,
}

impl ProbeKind {
    pub fn to_code(self) -> u8 {
        match self {
            ProbeKind::Tracer => 0,
            ProbeKind::Sensor => 1,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(ProbeKind::Tracer),
            1 => Some(ProbeKind::Sensor),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct BoxRegion {
    pub x: (i64, i64),
    pub y: (i64, i64),
    pub z: (i64, i64),
}

impl BoxRegion {
    pub fn contains(&self, pos: [i64; 3]) -> bool {
        pos[0] >= self.x.0
            && pos[0] < self.x.1
            && pos[1] >= self.y.0
            && pos[1] < self.y.1
            && pos[2] >= self.z.0
            && pos[2] < self.z.1
    }

    pub fn boundary_descriptor(&self) -> String {
        format!(
            "∂box([{}..{}, {}..{}, {}..{}])",
            self.x.0, self.x.1, self.y.0, self.y.1, self.z.0, self.z.1
        )
    }

    pub fn to_json(&self) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert(
            "x".to_string(),
            JsonValue::Array(vec![
                JsonValue::from_i64(self.x.0),
                JsonValue::from_i64(self.x.1),
            ]),
        );
        map.insert(
            "y".to_string(),
            JsonValue::Array(vec![
                JsonValue::from_i64(self.y.0),
                JsonValue::from_i64(self.y.1),
            ]),
        );
        map.insert(
            "z".to_string(),
            JsonValue::Array(vec![
                JsonValue::from_i64(self.z.0),
                JsonValue::from_i64(self.z.1),
            ]),
        );
        JsonValue::Object(map)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CarrierState {
    pub id: u64,
    pub support: SupportForm,
    pub shells: u32,
    pub pos: [i64; 3],
    pub dir: AxisDir,
    pub orient: T1,
    pub q4: Q4,
    pub trace: Trace,
    pub dwell_blocked: bool,
    pub active: bool,
    pub phase_step: u8,
    pub return_count: u64,
    pub hinge_count: u64,
    pub shed_count: u64,
}

impl CarrierState {
    pub fn new(
        id: u64,
        scope: &str,
        support: SupportForm,
        shells: u32,
        pos: [i64; 3],
        dir: AxisDir,
        orient: T1,
    ) -> CoreResult<Self> {
        if shells == 0 {
            return Err(aod_core::error::CoreError::DomainError(
                "carrier shells must be >= 1".to_string(),
            ));
        }
        if orient == T1::ZERO {
            return Err(aod_core::error::CoreError::DomainError(
                "carrier orient must be nonzero (-1t or +1t)".to_string(),
            ));
        }
        let start = Q4::new(0)?;
        let trace = Trace::new(scope, format!("carrier:{id}"), start)?;
        Ok(Self {
            id,
            support,
            shells,
            pos,
            dir,
            orient,
            q4: start,
            trace,
            dwell_blocked: false,
            active: true,
            phase_step: 0,
            return_count: 0,
            hinge_count: 0,
            shed_count: 0,
        })
    }

    pub fn to_json(&self) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert("active".to_string(), JsonValue::Bool(self.active));
        map.insert(
            "dir".to_string(),
            JsonValue::from_str_val(self.dir.as_str().to_string()),
        );
        map.insert("dwell_blocked".to_string(), JsonValue::Bool(self.dwell_blocked));
        map.insert(
            "hinge_count".to_string(),
            JsonValue::from_i64(self.hinge_count as i64),
        );
        map.insert("id".to_string(), JsonValue::from_i64(self.id as i64));
        map.insert(
            "orient".to_string(),
            JsonValue::from_i64(self.orient.value() as i64),
        );
        map.insert(
            "pos".to_string(),
            JsonValue::Array(self.pos.iter().map(|&p| JsonValue::from_i64(p)).collect()),
        );
        map.insert("q4".to_string(), JsonValue::from_i64(self.q4.value() as i64));
        map.insert(
            "return_count".to_string(),
            JsonValue::from_i64(self.return_count as i64),
        );
        map.insert(
            "shed_count".to_string(),
            JsonValue::from_i64(self.shed_count as i64),
        );
        map.insert("shells".to_string(), JsonValue::from_i64(self.shells as i64));
        map.insert(
            "support".to_string(),
            JsonValue::from_str_val(self.support.render()),
        );
        map.insert("trace".to_string(), self.trace.to_render_json());
        JsonValue::Object(map)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceClock {
    pub name: String,
    pub period: u64,
    pub tick: u64,
    pub phase_count: u64,
}

impl ReferenceClock {
    pub fn new(name: impl Into<String>, period: u64) -> CoreResult<Self> {
        if period == 0 {
            return Err(aod_core::error::CoreError::DomainError(
                "reference clock period must be >= 1".to_string(),
            ));
        }
        Ok(Self {
            name: name.into(),
            period,
            tick: 0,
            phase_count: 0,
        })
    }

    pub fn step(&mut self) {
        self.tick += 1;
        if self.tick % self.period == 0 {
            self.phase_count += 1;
        }
    }

    pub fn to_json(&self) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert("name".to_string(), JsonValue::from_str_val(self.name.clone()));
        map.insert("period".to_string(), JsonValue::from_i64(self.period as i64));
        map.insert(
            "phase_count".to_string(),
            JsonValue::from_i64(self.phase_count as i64),
        );
        map.insert("tick".to_string(), JsonValue::from_i64(self.tick as i64));
        JsonValue::Object(map)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WaveletEventRecord {
    pub tick: u64,
    pub parent_id: String,
    pub pos: [i64; 3],
    pub x_shedding: BigInt,
    pub window: (i64, i64),
}

impl WaveletEventRecord {
    pub fn to_json(&self) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert(
            "parent_id".to_string(),
            JsonValue::from_str_val(self.parent_id.clone()),
        );
        map.insert(
            "pos".to_string(),
            JsonValue::Array(self.pos.iter().map(|&p| JsonValue::from_i64(p)).collect()),
        );
        map.insert("tick".to_string(), JsonValue::from_i64(self.tick as i64));
        map.insert(
            "window".to_string(),
            JsonValue::Array(vec![
                JsonValue::from_i64(self.window.0),
                JsonValue::from_i64(self.window.1),
            ]),
        );
        map.insert("x_shedding".to_string(), JsonValue::Int(self.x_shedding.clone()));
        JsonValue::Object(map)
    }
}

/// Immutable post-tick snapshot passed strictly read-only (`&ChamberSnapshot`) to observers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChamberSnapshot {
    pub model_status: String,
    pub chamber_name: String,
    pub space: BoxRegion,
    pub tick: u64,
    pub carriers: Vec<CarrierState>,
    pub clocks: Vec<ReferenceClock>,
    pub shedding_events_this_tick: Vec<ShedWitness>,
    pub wavelet_events_this_tick: Vec<WaveletEventRecord>,
    pub boundary_crossings: u64,
}

/// Explicit deterministic zero-RNG exact sandbox chamber (`P0-2`).
///
/// Structural zero-back-reaction guarantee: `ChamberState::step(&mut self)` accepts
/// NO observer references whatsoever.
#[derive(Clone, Debug)]
pub struct ChamberState {
    pub model_status: String,
    pub chamber_name: String,
    pub space: BoxRegion,
    pub walls: WallPolicy,
    pub tick: u64,
    pub carriers: Vec<CarrierState>,
    pub clocks: Vec<ReferenceClock>,
    pub shedding_events: Vec<ShedWitness>,
    pub wavelet_events: Vec<WaveletEventRecord>,
    pub boundary_crossings: u64,
    last_tick_shedding: Vec<ShedWitness>,
    last_tick_wavelets: Vec<WaveletEventRecord>,
}

impl ChamberState {
    pub fn new(
        chamber_name: impl Into<String>,
        space: BoxRegion,
        walls: WallPolicy,
        carriers: Vec<CarrierState>,
        clocks: Vec<ReferenceClock>,
    ) -> Self {
        Self {
            model_status: MODEL_STATUS.to_string(),
            chamber_name: chamber_name.into(),
            space,
            walls,
            tick: 0,
            carriers,
            clocks,
            shedding_events: Vec::new(),
            wavelet_events: Vec::new(),
            boundary_crossings: 0,
            last_tick_shedding: Vec::new(),
            last_tick_wavelets: Vec::new(),
        }
    }

    /// Execute one deterministic exact sandbox tick with ZERO RNG and ZERO observer inputs.
    pub fn step(&mut self) -> CoreResult<()> {
        self.tick += 1;
        self.last_tick_shedding.clear();
        self.last_tick_wavelets.clear();

        // 1. Advance active carriers by `dir`; apply Open/Closed wall policy.
        for carrier in &mut self.carriers {
            if !carrier.active {
                continue;
            }
            let d = carrier.dir.delta();
            let next = [
                carrier.pos[0] + d[0],
                carrier.pos[1] + d[1],
                carrier.pos[2] + d[2],
            ];
            if self.space.contains(next) {
                carrier.pos = next;
            } else {
                self.boundary_crossings += 1;
                match self.walls {
                    WallPolicy::Open => {
                        carrier.active = false;
                    }
                    WallPolicy::Closed => {
                        carrier.dir = carrier.dir.reflect();
                        carrier.orient = carrier.orient.checked_neg();
                        carrier.hinge_count += 1;
                    }
                }
            }
        }

        // 2. Deterministic Manual §4 Q4 kernel ([1,1,4,2]) / HingeSlide ([3,3,1]) trace update.
        for carrier in &mut self.carriers {
            if !carrier.active {
                continue;
            }
            let neighbors = carrier.q4.neighbors();
            let chosen_slot = select_deterministic_slot(
                &self.chamber_name,
                carrier.q4,
                neighbors,
                carrier.dwell_blocked,
                self.tick,
                carrier.id,
            )?;

            match carrier.phase_step {
                0 => {
                    let target = neighbors[chosen_slot];
                    carrier.trace = carrier.trace.step(target, carrier.orient)?;
                    carrier.q4 = target;
                    carrier.phase_step = 1;
                }
                1 => {
                    carrier.trace = carrier.trace.hinge()?;
                    carrier.hinge_count += 1;
                    carrier.phase_step = 2;
                }
                _ => {
                    let start = carrier.trace.start;
                    carrier.trace = carrier
                        .trace
                        .step(start, carrier.orient.checked_neg())?
                        .close()?;
                    carrier.q4 = start;
                    carrier.return_count += 1;
                    carrier.dwell_blocked = false;
                    carrier.phase_step = 0;
                }
            }
        }

        // 3. Cell coincidence detection -> dwell blocking + exact SupportForm::shedding_audit.
        let mut by_cell: BTreeMap<[i64; 3], Vec<usize>> = BTreeMap::new();
        for (idx, c) in self.carriers.iter().enumerate() {
            if c.active {
                by_cell.entry(c.pos).or_default().push(idx);
            }
        }

        let one = BigInt::one();
        let c3 = BigInt::from_i64(3);
        for (pos, indices) in by_cell {
            if indices.len() < 2 {
                continue;
            }
            let mut combined_p_d = BigInt::zero();
            let mut baseline_c_close = BigInt::zero();
            for (k, &idx) in indices.iter().enumerate() {
                let carrier = &mut self.carriers[idx];
                carrier.dwell_blocked = true;
                let w_len = BigInt::from_u64(carrier.shells as u64);
                let p_d = carrier.support.pressure(&c3, &one, &w_len)?;
                if k == 0 {
                    baseline_c_close = p_d.clone();
                }
                combined_p_d = &combined_p_d + &p_d;
            }
            let audit = SupportForm::shedding_audit(&combined_p_d, &baseline_c_close)?;
            if audit.x_shedding.is_positive() {
                let first_id = self.carriers[indices[0]].id;
                let parent_id = format!("carrier:{first_id}");
                let witness = ShedWitness::new(
                    parent_id.clone(),
                    self.space.boundary_descriptor(),
                    "sandbox_tick",
                    self.tick as i64,
                    ShedRoute::Exo,
                    Ratio::from_int(audit.x_shedding.clone()),
                    format!("post_shed_tick_{}", self.tick),
                )?;
                let wave_ev = WaveletEventRecord {
                    tick: self.tick,
                    parent_id,
                    pos,
                    x_shedding: audit.x_shedding,
                    window: ((self.tick as i64).saturating_sub(1), self.tick as i64),
                };
                self.shedding_events.push(witness.clone());
                self.wavelet_events.push(wave_ev.clone());
                self.last_tick_shedding.push(witness);
                self.last_tick_wavelets.push(wave_ev);

                for &idx in &indices {
                    self.carriers[idx].shed_count += 1;
                }
            }

            if indices.len() == 2 {
                let i0 = indices[0];
                let i1 = indices[1];
                if self.carriers[i0].dir == self.carriers[i1].dir.reflect() {
                    self.carriers[i0].dir = self.carriers[i0].dir.reflect();
                    self.carriers[i1].dir = self.carriers[i1].dir.reflect();
                }
            }
        }

        // 4. Step decoupled reference clocks independently.
        for clock in &mut self.clocks {
            clock.step();
        }
        Ok(())
    }

    pub fn snapshot(&self) -> ChamberSnapshot {
        ChamberSnapshot {
            model_status: self.model_status.clone(),
            chamber_name: self.chamber_name.clone(),
            space: self.space,
            tick: self.tick,
            carriers: self.carriers.clone(),
            clocks: self.clocks.clone(),
            shedding_events_this_tick: self.last_tick_shedding.clone(),
            wavelet_events_this_tick: self.last_tick_wavelets.clone(),
            boundary_crossings: self.boundary_crossings,
        }
    }

    pub fn to_json(&self) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert(
            "boundary_crossings".to_string(),
            JsonValue::from_i64(self.boundary_crossings as i64),
        );
        map.insert(
            "carriers".to_string(),
            JsonValue::Array(self.carriers.iter().map(CarrierState::to_json).collect()),
        );
        map.insert(
            "chamber_name".to_string(),
            JsonValue::from_str_val(self.chamber_name.clone()),
        );
        map.insert(
            "clocks".to_string(),
            JsonValue::Array(self.clocks.iter().map(ReferenceClock::to_json).collect()),
        );
        map.insert(
            "model_status".to_string(),
            JsonValue::from_str_val(self.model_status.clone()),
        );
        map.insert(
            "shedding_event_count".to_string(),
            JsonValue::from_usize(self.shedding_events.len()),
        );
        map.insert("space".to_string(), self.space.to_json());
        map.insert("tick".to_string(), JsonValue::from_i64(self.tick as i64));
        map.insert(
            "wavelet_events".to_string(),
            JsonValue::Array(
                self.wavelet_events
                    .iter()
                    .map(WaveletEventRecord::to_json)
                    .collect(),
            ),
        );
        JsonValue::Object(map)
    }

    pub fn state_hash(&self) -> String {
        self.to_json().sha256_hex()
    }
}

fn select_deterministic_slot(
    scope: &str,
    node: Q4,
    neighbors: [Q4; 4],
    dwell_blocked: bool,
    tick: u64,
    carrier_id: u64,
) -> CoreResult<usize> {
    let base_rows = vec![
        LedgerRow::new(scope, node, "e_1", neighbors[0], T1::POS, true, BigInt::from_i64(1), RouteStatus::Outgoing)?,
        LedgerRow::new(scope, node, "e_2", neighbors[1], T1::NEG, true, BigInt::from_i64(1), RouteStatus::Returned)?,
        LedgerRow::new(scope, node, "e_3", neighbors[2], T1::ZERO, true, BigInt::from_i64(4), RouteStatus::Hinge)?,
        LedgerRow::new(scope, node, "e_4", neighbors[3], T1::POS, true, BigInt::from_i64(2), RouteStatus::Outgoing)?,
    ];
    let norm = Kernel::normalize(&base_rows)?;
    if dwell_blocked {
        let slide_pool = vec![
            LedgerRow::new(scope, node, "e_1", neighbors[0], T1::NEG, true, BigInt::from_i64(3), RouteStatus::Slide)?,
            LedgerRow::new(scope, node, "e_2", neighbors[1], T1::NEG, true, BigInt::from_i64(3), RouteStatus::Slide)?,
            LedgerRow::new(scope, node, "e_3", neighbors[2], T1::POS, true, BigInt::from_i64(1), RouteStatus::Slide)?,
        ];
        let slide = HingeSlide::block_dwell_and_slide(&norm, "e_3", &slide_pool)?;
        let ticket = ((tick.wrapping_add(carrier_id)) % 7) as i64;
        let slot = if ticket < 3 {
            0
        } else if ticket < 6 {
            1
        } else {
            2
        };
        debug_assert!(slide.slide_rows[slot].probability.is_positive());
        Ok(slot)
    } else {
        let ticket = ((tick.wrapping_add(carrier_id)) % 8) as i64;
        let slot = if ticket < 1 {
            0
        } else if ticket < 2 {
            1
        } else if ticket < 6 {
            2
        } else {
            3
        };
        debug_assert!(norm[slot].probability.is_positive());
        Ok(slot)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclarativeObserver {
    pub kind: ProbeKind,
    pub name: String,
    pub region: BoxRegion,
    pub capture: Vec<String>,
    pub observed_support_counts: BTreeMap<String, u64>,
    pub observed_returns: u64,
    pub observed_hinges: u64,
    pub observed_shedding: u64,
    pub observed_contacts: usize,
    pub boundary_crossings: u64,
    pub last_ref_phases: u64,
}

impl DeclarativeObserver {
    pub fn new(kind: ProbeKind, name: String, region: BoxRegion, capture: Vec<String>) -> Self {
        Self {
            kind,
            name,
            region,
            capture,
            observed_support_counts: BTreeMap::new(),
            observed_returns: 0,
            observed_hinges: 0,
            observed_shedding: 0,
            observed_contacts: 0,
            boundary_crossings: 0,
            last_ref_phases: 0,
        }
    }

    pub fn observe_snapshot(&mut self, snap: &ChamberSnapshot) {
        let mut saw_in_region = false;
        for c in &snap.carriers {
            if c.active && self.region.contains(c.pos) {
                saw_in_region = true;
                *self
                    .observed_support_counts
                    .entry(c.support.render())
                    .or_insert(0) += 1;
                self.observed_returns = self.observed_returns.max(c.return_count);
                self.observed_hinges = self.observed_hinges.max(c.hinge_count);
            }
        }
        if saw_in_region {
            self.observed_contacts += 1;
        }
        for w in &snap.wavelet_events_this_tick {
            if self.region.contains(w.pos) {
                self.observed_shedding += 1;
            }
        }
        self.boundary_crossings = snap.boundary_crossings;
        if let Some(clk) = snap.clocks.first() {
            self.last_ref_phases = clk.phase_count;
        }
    }

    pub fn cadence_readout(&self) -> (Option<Ratio>, Option<SingleContactStageAssociation>) {
        if self.observed_contacts >= 2 && self.last_ref_phases > 0 {
            let ratio = native_cadence_ratio(
                &format!("probe:{}", self.name),
                &BigInt::from_u64(self.observed_returns),
                "clock:ref",
                &BigInt::from_u64(self.last_ref_phases),
                true,
                self.observed_contacts,
            )
            .ok();
            (ratio, None)
        } else if self.observed_contacts == 1 {
            let assoc = SingleContactStageAssociation::new(
                format!("probe:{}", self.name),
                format!("chamber:{}", self.name),
                "clock:ref",
                (0, 1),
                1,
            )
            .ok();
            (None, assoc)
        } else {
            (None, None)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlindMotifSpectrum {
    pub support_counts: BTreeMap<String, u64>,
    pub total_returns: u64,
    pub total_hinges: u64,
    pub total_shedding_events: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SensorCurrentSummary {
    pub sensor_name: String,
    pub boundary_crossings: u64,
    pub bound_contacts: usize,
    pub cadence_ratio: Option<Ratio>,
    pub single_contact_association: Option<SingleContactStageAssociation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimRunResult {
    pub model_status: String,
    pub chamber_name: String,
    pub ticks: u64,
    pub final_state_hash: String,
    pub carrier_count: usize,
    pub shedding_event_count: usize,
    pub wavelet_event_count: usize,
    pub boundary_crossings: u64,
    pub blind_motif_spectrum: BlindMotifSpectrum,
    pub sensor_currents: Vec<SensorCurrentSummary>,
    pub procedural_outputs: BTreeMap<String, Vec<String>>,
    pub final_snapshot: ChamberSnapshot,
}
