use std::collections::{BTreeMap, BTreeSet};

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
use aod_core::witness::{Coupling, EventKind, Fusion, Trace};

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

    pub fn bounds_array(&self) -> [[i64; 2]; 3] {
        [
            [self.x.0, self.x.1],
            [self.y.0, self.y.1],
            [self.z.0, self.z.1],
        ]
    }

    pub fn clamp_pos(&self, pos: [i64; 3]) -> [i64; 3] {
        fn clamp_axis(v: i64, bounds: (i64, i64)) -> i64 {
            if bounds.1 <= bounds.0 {
                bounds.0
            } else {
                v.clamp(bounds.0, bounds.1 - 1)
            }
        }
        [
            clamp_axis(pos[0], self.x),
            clamp_axis(pos[1], self.y),
            clamp_axis(pos[2], self.z),
        ]
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChamberDynamicsConfig {
    pub boundary_step: [i64; 3],
    pub contact_ttl: Option<u64>,
    pub fuse_on_resonance: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundaryMotionRecord {
    pub tick: u64,
    pub prev_bounds: [[i64; 2]; 3],
    pub next_bounds: [[i64; 2]; 3],
    pub cross_flux: BigInt,
    pub sweep_flux: BigInt,
    pub shedding_flux: BigInt,
    pub net_moving_flux: BigInt,
}

impl BoundaryMotionRecord {
    pub fn to_json(&self) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert("cross_flux".to_string(), JsonValue::Int(self.cross_flux.clone()));
        map.insert(
            "net_moving_flux".to_string(),
            JsonValue::Int(self.net_moving_flux.clone()),
        );
        map.insert(
            "next_bounds".to_string(),
            JsonValue::Array(
                self.next_bounds
                    .iter()
                    .map(|&[lo, hi]| {
                        JsonValue::Array(vec![JsonValue::from_i64(lo), JsonValue::from_i64(hi)])
                    })
                    .collect(),
            ),
        );
        map.insert(
            "prev_bounds".to_string(),
            JsonValue::Array(
                self.prev_bounds
                    .iter()
                    .map(|&[lo, hi]| {
                        JsonValue::Array(vec![JsonValue::from_i64(lo), JsonValue::from_i64(hi)])
                    })
                    .collect(),
            ),
        );
        map.insert(
            "shedding_flux".to_string(),
            JsonValue::Int(self.shedding_flux.clone()),
        );
        map.insert("sweep_flux".to_string(), JsonValue::Int(self.sweep_flux.clone()));
        map.insert(
            "tick".to_string(),
            JsonValue::Int(BigInt::from_u64(self.tick)),
        );
        JsonValue::Object(map)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveContactWindow {
    pub left_id: u64,
    pub right_id: u64,
    pub open_tick: u64,
    pub last_contact_tick: u64,
    pub last_bound: (usize, usize),
    pub coupling: Coupling,
}

impl ActiveContactWindow {
    pub fn to_json(&self) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert("coupling".to_string(), self.coupling.to_render_json());
        map.insert(
            "last_bound".to_string(),
            JsonValue::Array(vec![
                JsonValue::from_usize(self.last_bound.0),
                JsonValue::from_usize(self.last_bound.1),
            ]),
        );
        map.insert(
            "last_contact_tick".to_string(),
            JsonValue::Int(BigInt::from_u64(self.last_contact_tick)),
        );
        map.insert(
            "left_id".to_string(),
            JsonValue::Int(BigInt::from_u64(self.left_id)),
        );
        map.insert(
            "open_tick".to_string(),
            JsonValue::Int(BigInt::from_u64(self.open_tick)),
        );
        map.insert(
            "right_id".to_string(),
            JsonValue::Int(BigInt::from_u64(self.right_id)),
        );
        JsonValue::Object(map)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpiredContactRecord {
    pub left_id: u64,
    pub right_id: u64,
    pub window: (i64, i64),
    pub bound_contacts: usize,
    pub stage_only: bool,
    pub expired_at_tick: u64,
}

impl ExpiredContactRecord {
    pub fn to_json(&self) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert(
            "bound_contacts".to_string(),
            JsonValue::from_usize(self.bound_contacts),
        );
        map.insert(
            "expired_at_tick".to_string(),
            JsonValue::Int(BigInt::from_u64(self.expired_at_tick)),
        );
        map.insert(
            "left_id".to_string(),
            JsonValue::Int(BigInt::from_u64(self.left_id)),
        );
        map.insert(
            "right_id".to_string(),
            JsonValue::Int(BigInt::from_u64(self.right_id)),
        );
        map.insert("stage_only".to_string(), JsonValue::Bool(self.stage_only));
        map.insert(
            "window".to_string(),
            JsonValue::Array(vec![
                JsonValue::from_i64(self.window.0),
                JsonValue::from_i64(self.window.1),
            ]),
        );
        JsonValue::Object(map)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResonanceRecord {
    pub tick: u64,
    pub left_id: u64,
    pub right_id: u64,
    pub bound_contacts: usize,
    pub cadence: Ratio,
    pub lag: BigInt,
    pub drift: BigInt,
    pub left_digest: String,
    pub right_digest: String,
}

impl ResonanceRecord {
    pub fn to_json(&self) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert(
            "bound_contacts".to_string(),
            JsonValue::from_usize(self.bound_contacts),
        );
        map.insert(
            "cadence".to_string(),
            JsonValue::Array(vec![
                JsonValue::from_str_val(self.cadence.num().to_decimal_string()),
                JsonValue::from_str_val(self.cadence.den().to_decimal_string()),
            ]),
        );
        map.insert("drift".to_string(), JsonValue::Int(self.drift.clone()));
        map.insert("lag".to_string(), JsonValue::Int(self.lag.clone()));
        map.insert(
            "left_digest".to_string(),
            JsonValue::from_str_val(self.left_digest.clone()),
        );
        map.insert(
            "left_id".to_string(),
            JsonValue::Int(BigInt::from_u64(self.left_id)),
        );
        map.insert(
            "right_digest".to_string(),
            JsonValue::from_str_val(self.right_digest.clone()),
        );
        map.insert(
            "right_id".to_string(),
            JsonValue::Int(BigInt::from_u64(self.right_id)),
        );
        map.insert(
            "tick".to_string(),
            JsonValue::Int(BigInt::from_u64(self.tick)),
        );
        JsonValue::Object(map)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FusionEventRecord {
    pub tick: u64,
    pub outer_carrier_id: u64,
    pub left_parent_id: u64,
    pub right_parent_id: u64,
    pub outer_scope: String,
    pub proposal_digest: String,
    pub outer_closure_digest: String,
}

impl FusionEventRecord {
    pub fn to_json(&self) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert(
            "left_parent_id".to_string(),
            JsonValue::Int(BigInt::from_u64(self.left_parent_id)),
        );
        map.insert(
            "outer_carrier_id".to_string(),
            JsonValue::Int(BigInt::from_u64(self.outer_carrier_id)),
        );
        map.insert(
            "outer_closure_digest".to_string(),
            JsonValue::from_str_val(self.outer_closure_digest.clone()),
        );
        map.insert(
            "outer_scope".to_string(),
            JsonValue::from_str_val(self.outer_scope.clone()),
        );
        map.insert(
            "proposal_digest".to_string(),
            JsonValue::from_str_val(self.proposal_digest.clone()),
        );
        map.insert(
            "right_parent_id".to_string(),
            JsonValue::Int(BigInt::from_u64(self.right_parent_id)),
        );
        map.insert(
            "tick".to_string(),
            JsonValue::Int(BigInt::from_u64(self.tick)),
        );
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
    pub active_carrier_count: usize,
    pub resonance_count: usize,
    pub expired_contact_count: usize,
    pub fusion_count: usize,
    pub shedding_witnesses_json: String,
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
    pub dynamics: ChamberDynamicsConfig,
    pub tick: u64,
    pub carriers: Vec<CarrierState>,
    pub clocks: Vec<ReferenceClock>,
    pub shedding_events: Vec<ShedWitness>,
    pub wavelet_events: Vec<WaveletEventRecord>,
    pub boundary_crossings: u64,
    pub boundary_motions: Vec<BoundaryMotionRecord>,
    pub active_contacts: BTreeMap<(u64, u64), ActiveContactWindow>,
    pub expired_contacts: Vec<ExpiredContactRecord>,
    pub resonances: Vec<ResonanceRecord>,
    pub fusions: Vec<FusionEventRecord>,
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
            dynamics: ChamberDynamicsConfig::default(),
            tick: 0,
            carriers,
            clocks,
            shedding_events: Vec::new(),
            wavelet_events: Vec::new(),
            boundary_crossings: 0,
            boundary_motions: Vec::new(),
            active_contacts: BTreeMap::new(),
            expired_contacts: Vec::new(),
            resonances: Vec::new(),
            fusions: Vec::new(),
            last_tick_shedding: Vec::new(),
            last_tick_wavelets: Vec::new(),
        }
    }

    pub fn with_dynamics(mut self, dynamics: ChamberDynamicsConfig) -> Self {
        self.dynamics = dynamics;
        self
    }

    pub fn active_carrier_count(&self) -> usize {
        self.carriers.iter().filter(|c| c.active).count()
    }

    pub fn shedding_witnesses_json(&self) -> String {
        JsonValue::Array(
            self.shedding_events
                .iter()
                .map(ShedWitness::to_json)
                .collect(),
        )
        .to_compact_ascii_json()
    }

    pub fn moving_flux_total(&self) -> BigInt {
        let mut acc = BigInt::zero();
        for rec in &self.boundary_motions {
            acc = &acc + &rec.net_moving_flux;
        }
        acc
    }

    /// Execute one deterministic exact sandbox tick with ZERO RNG and ZERO observer inputs.
    pub fn step(&mut self) -> CoreResult<()> {
        self.tick += 1;
        self.last_tick_shedding.clear();
        self.last_tick_wavelets.clear();

        let prev_space = self.space;
        let mut cross_flux = BigInt::zero();
        let mut sweep_flux = BigInt::zero();
        let mut shedding_flux = BigInt::zero();

        // 0. Moving Boundary (`eq:moving-boundary` eq. 94) when `boundary_step != [0, 0, 0]`.
        if self.dynamics.boundary_step != [0, 0, 0] {
            let [dx, dy, dz] = self.dynamics.boundary_step;
            let next_space = BoxRegion {
                x: (prev_space.x.0.saturating_add(dx), prev_space.x.1.saturating_add(dx)),
                y: (prev_space.y.0.saturating_add(dy), prev_space.y.1.saturating_add(dy)),
                z: (prev_space.z.0.saturating_add(dz), prev_space.z.1.saturating_add(dz)),
            };
            self.space = next_space;
            let emit_boundary = self.space.boundary_descriptor();
            for carrier in &mut self.carriers {
                if !carrier.active {
                    continue;
                }
                if prev_space.contains(carrier.pos) && !next_space.contains(carrier.pos) {
                    let pi = carrier.support.pi()?;
                    sweep_flux = &sweep_flux + &pi;
                    self.boundary_crossings += 1;
                    match self.walls {
                        WallPolicy::Open => {
                            carrier.active = false;
                            let post_state = carrier
                                .trace
                                .trace_prefix_json(carrier.trace.events.len())
                                .sha256_hex();
                            let witness = ShedWitness::new(
                                format!("carrier:{}", carrier.id),
                                emit_boundary.clone(),
                                format!(
                                    "sweep_wall_tick_{}@[{},{},{}]",
                                    self.tick, carrier.pos[0], carrier.pos[1], carrier.pos[2]
                                ),
                                self.tick as i64,
                                ShedRoute::Open,
                                Ratio::from_int(pi),
                                post_state,
                            )?;
                            self.shedding_events.push(witness.clone());
                            self.last_tick_shedding.push(witness);
                            carrier.shed_count += 1;
                        }
                        WallPolicy::Closed => {
                            carrier.pos = next_space.clamp_pos(carrier.pos);
                            carrier.dir = carrier.dir.reflect();
                            carrier.orient = carrier.orient.checked_neg();
                            carrier.hinge_count += 1;
                        }
                    }
                }
            }
        }

        // 1. Advance active carriers by `dir`; apply Open/Closed wall policy.
        let emit_boundary = self.space.boundary_descriptor();
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
                let pi = carrier.support.pi()?;
                cross_flux = &cross_flux + &pi;
                self.boundary_crossings += 1;
                match self.walls {
                    WallPolicy::Open => {
                        carrier.active = false;
                        let post_state = carrier
                            .trace
                            .trace_prefix_json(carrier.trace.events.len())
                            .sha256_hex();
                        let witness = ShedWitness::new(
                            format!("carrier:{}", carrier.id),
                            emit_boundary.clone(),
                            format!(
                                "open_wall_tick_{}@[{},{},{}]",
                                self.tick, next[0], next[1], next[2]
                            ),
                            self.tick as i64,
                            ShedRoute::Open,
                            Ratio::from_int(pi),
                            post_state,
                        )?;
                        self.shedding_events.push(witness.clone());
                        self.last_tick_shedding.push(witness);
                        carrier.shed_count += 1;
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
                    let seg_begin = carrier.trace.closures.last().map_or(0, |c| c.end);
                    let return_orient = carrier.trace.events[seg_begin..]
                        .iter()
                        .find(|ev| ev.kind == EventKind::Step)
                        .and_then(|ev| T1::new(-ev.orientation).ok())
                        .unwrap_or_else(|| carrier.orient.checked_neg());
                    carrier.trace = carrier.trace.step(start, return_orient)?.close()?;
                    carrier.q4 = start;
                    carrier.return_count += 1;
                    carrier.dwell_blocked = false;
                    carrier.phase_step = 0;
                }
            }
        }

        // Refresh active contact windows that remain within `contact_ttl` so carriers that
        // coincided earlier and close at `phase_step == 2` bind certified return contacts cleanly.
        let id_to_idx: BTreeMap<u64, usize> = self
            .carriers
            .iter()
            .enumerate()
            .map(|(idx, c)| (c.id, idx))
            .collect();
        let ttl_limit = self.dynamics.contact_ttl.unwrap_or(u64::MAX);
        for (&(left_id, right_id), win) in &mut self.active_contacts {
            if self.tick.saturating_sub(win.last_contact_tick) <= ttl_limit {
                if let (Some(&i0), Some(&i1)) =
                    (id_to_idx.get(&left_id), id_to_idx.get(&right_id))
                {
                    refresh_active_contact_window(win, &self.carriers[i0], &self.carriers[i1])?;
                }
            }
        }

        // 3. Cell coincidence detection -> coupling windows, resonance, real parent trace digests
        //    on ShedWitness, dwell blocking, and optional composite Fusion.
        let mut by_cell: BTreeMap<[i64; 3], Vec<usize>> = BTreeMap::new();
        for (idx, c) in self.carriers.iter().enumerate() {
            if c.active {
                by_cell.entry(c.pos).or_default().push(idx);
            }
        }

        let mut coincident_pairs_this_tick: BTreeSet<(u64, u64)> = BTreeSet::new();
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

            let mut cell_resonant = false;
            for p in 0..indices.len() {
                for q in (p + 1)..indices.len() {
                    let (idx_l, idx_r) =
                        if self.carriers[indices[p]].id <= self.carriers[indices[q]].id {
                            (indices[p], indices[q])
                        } else {
                            (indices[q], indices[p])
                        };
                    let c_left = &self.carriers[idx_l];
                    let c_right = &self.carriers[idx_r];
                    let pair_key = (c_left.id, c_right.id);
                    coincident_pairs_this_tick.insert(pair_key);

                    if !c_left.trace.events.is_empty()
                        && c_left.trace.events.len() == c_right.trace.events.len()
                        && c_left.trace.scope == c_right.trace.scope
                        && c_left.trace.lineage != c_right.trace.lineage
                    {
                        if let Some(win) = self.active_contacts.get_mut(&pair_key) {
                            win.last_contact_tick = self.tick;
                            refresh_active_contact_window(win, c_left, c_right)?;
                        } else {
                            let window_label = format!(
                                "contact:{}:{}:open_{}",
                                pair_key.0, pair_key.1, self.tick
                            );
                            let cpl = Coupling::new(&c_left.trace, &c_right.trace, window_label)?;
                            let mut win = ActiveContactWindow {
                                left_id: pair_key.0,
                                right_id: pair_key.1,
                                open_tick: self.tick,
                                last_contact_tick: self.tick,
                                last_bound: (0, 0),
                                coupling: cpl,
                            };
                            refresh_active_contact_window(&mut win, c_left, c_right)?;
                            self.active_contacts.insert(pair_key, win);
                        }
                    }

                    if let Some(win) = self.active_contacts.get(&pair_key) {
                        if win.coupling.contacts.len() >= 2 {
                            let cadence = win.coupling.cadence()?;
                            let lag = win.coupling.lag()?;
                            let drift = win.coupling.drift()?;
                            if drift.is_zero() {
                                cell_resonant = true;
                                let left_digest = win
                                    .coupling
                                    .left
                                    .closures
                                    .last()
                                    .map(|c| c.digest.clone())
                                    .unwrap_or_else(|| {
                                        win.coupling
                                            .left
                                            .trace_prefix_json(win.coupling.left.events.len())
                                            .sha256_hex()
                                    });
                                let right_digest = win
                                    .coupling
                                    .right
                                    .closures
                                    .last()
                                    .map(|c| c.digest.clone())
                                    .unwrap_or_else(|| {
                                        win.coupling
                                            .right
                                            .trace_prefix_json(win.coupling.right.events.len())
                                            .sha256_hex()
                                    });
                                self.resonances.push(ResonanceRecord {
                                    tick: self.tick,
                                    left_id: pair_key.0,
                                    right_id: pair_key.1,
                                    bound_contacts: win.coupling.contacts.len(),
                                    cadence,
                                    lag,
                                    drift,
                                    left_digest,
                                    right_digest,
                                });
                            }
                        }
                    }
                }
            }

            let audit = SupportForm::shedding_audit(&combined_p_d, &baseline_c_close)?;
            if audit.x_shedding.is_positive() {
                let first_id = self.carriers[indices[0]].id;
                let parent_id = format!("carrier:{first_id}");
                let route = if cell_resonant {
                    ShedRoute::Endo
                } else {
                    ShedRoute::Exo
                };
                let post_shed_state = indices
                    .iter()
                    .map(|&idx| {
                        let c = &self.carriers[idx];
                        c.trace.trace_prefix_json(c.trace.events.len()).sha256_hex()
                    })
                    .collect::<Vec<_>>()
                    .join(":");
                let cut_coordinate = format!(
                    "cell_[{},{},{}]_tick_{}",
                    pos[0], pos[1], pos[2], self.tick
                );
                let witness = ShedWitness::new(
                    parent_id.clone(),
                    self.space.boundary_descriptor(),
                    cut_coordinate,
                    self.tick as i64,
                    route,
                    Ratio::from_int(audit.x_shedding.clone()),
                    post_shed_state,
                )?;
                shedding_flux = &shedding_flux + &audit.x_shedding;
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

            let mut fused_this_cell = false;
            if self.dynamics.fuse_on_resonance && indices.len() == 2 {
                let i0 = indices[0];
                let i1 = indices[1];
                let c0_closed = !self.carriers[i0].trace.events.is_empty()
                    && self.carriers[i0].trace.closures.last().map(|c| c.end)
                        == Some(self.carriers[i0].trace.events.len());
                let c1_closed = !self.carriers[i1].trace.events.is_empty()
                    && self.carriers[i1].trace.closures.last().map(|c| c.end)
                        == Some(self.carriers[i1].trace.events.len());
                if c0_closed
                    && c1_closed
                    && self.carriers[i0].orient == self.carriers[i1].orient.checked_neg()
                    && self.carriers[i0].trace.lineage != self.carriers[i1].trace.lineage
                    && self.carriers[i0].trace.scope == self.carriers[i1].trace.scope
                {
                    let c0_id = self.carriers[i0].id;
                    let c1_id = self.carriers[i1].id;
                    let c0_q4 = self.carriers[i0].q4;
                    let c0_dir = self.carriers[i0].dir;
                    let c0_support = self.carriers[i0].support.clone();
                    let max_shells = self.carriers[i0].shells.max(self.carriers[i1].shells);
                    let outer_scope = if self.carriers[i0].trace.scope == self.chamber_name {
                        format!("{}::outer", self.chamber_name)
                    } else {
                        format!("{}::outer", self.carriers[i0].trace.scope)
                    };
                    let outer_lineage = format!("fused:{}_{}_t{}", c0_id, c1_id, self.tick);
                    let fus = Fusion::fold(
                        &self.carriers[i0].trace,
                        &self.carriers[i1].trace,
                        outer_scope.clone(),
                        outer_lineage,
                    )?;
                    let mut outer_t = fus.outer(c0_q4)?;
                    let n0 = c0_q4.neighbors()[0];
                    outer_t = outer_t
                        .step(n0, T1::POS)?
                        .hinge()?
                        .step(c0_q4, T1::NEG)?
                        .close()?;
                    let fused_trace = fus.fuse(&outer_t)?;
                    let outer_closure_digest = fused_trace
                        .closures
                        .last()
                        .expect("closed outer trace")
                        .digest
                        .clone();
                    let proposal_digest = fus.digest.clone();

                    self.carriers[i0].active = false;
                    self.carriers[i1].active = false;

                    let outer_carrier_id = self
                        .carriers
                        .iter()
                        .map(|c| c.id)
                        .max()
                        .map_or(0, |m| m + 1)
                        .max(self.carriers.len() as u64);
                    self.carriers.push(CarrierState {
                        id: outer_carrier_id,
                        support: c0_support,
                        shells: max_shells,
                        pos,
                        dir: c0_dir,
                        orient: T1::POS,
                        q4: c0_q4,
                        trace: fused_trace,
                        dwell_blocked: false,
                        active: true,
                        phase_step: 0,
                        return_count: 1,
                        hinge_count: 1,
                        shed_count: 0,
                    });
                    self.fusions.push(FusionEventRecord {
                        tick: self.tick,
                        outer_carrier_id,
                        left_parent_id: c0_id,
                        right_parent_id: c1_id,
                        outer_scope,
                        proposal_digest,
                        outer_closure_digest,
                    });
                    fused_this_cell = true;
                }
            }

            if !fused_this_cell && indices.len() == 2 {
                let i0 = indices[0];
                let i1 = indices[1];
                if self.carriers[i0].dir == self.carriers[i1].dir.reflect() {
                    self.carriers[i0].dir = self.carriers[i0].dir.reflect();
                    self.carriers[i1].dir = self.carriers[i1].dir.reflect();
                }
            }
        }
        let _ = coincident_pairs_this_tick;

        // Contact expiry (`contact_ttl`)
        if let Some(ttl) = self.dynamics.contact_ttl {
            let cur_tick = self.tick;
            let mut still_active = BTreeMap::new();
            let old_contacts = std::mem::take(&mut self.active_contacts);
            for (pair_key, win) in old_contacts {
                if cur_tick.saturating_sub(win.last_contact_tick) <= ttl {
                    still_active.insert(pair_key, win);
                } else {
                    let bound_contacts = win.coupling.contacts.len();
                    self.expired_contacts.push(ExpiredContactRecord {
                        left_id: win.left_id,
                        right_id: win.right_id,
                        window: (
                            win.open_tick as i64,
                            win.last_contact_tick.saturating_add(ttl) as i64,
                        ),
                        bound_contacts,
                        stage_only: bound_contacts < 2,
                        expired_at_tick: cur_tick,
                    });
                }
            }
            self.active_contacts = still_active;
        }

        // Moving-boundary flux audit record (`eq:moving-boundary` eq. 94)
        if self.dynamics.boundary_step != [0, 0, 0] {
            let net_moving_flux = &(&cross_flux - &sweep_flux) + &shedding_flux;
            self.boundary_motions.push(BoundaryMotionRecord {
                tick: self.tick,
                prev_bounds: prev_space.bounds_array(),
                next_bounds: self.space.bounds_array(),
                cross_flux,
                sweep_flux,
                shedding_flux,
                net_moving_flux,
            });
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
            active_carrier_count: self.active_carrier_count(),
            resonance_count: self.resonances.len(),
            expired_contact_count: self.expired_contacts.len(),
            fusion_count: self.fusions.len(),
            shedding_witnesses_json: self.shedding_witnesses_json(),
        }
    }

    pub fn to_json(&self) -> JsonValue {
        let mut map = BTreeMap::new();
        map.insert(
            "active_carrier_count".to_string(),
            JsonValue::from_usize(self.active_carrier_count()),
        );
        map.insert(
            "boundary_crossings".to_string(),
            JsonValue::from_i64(self.boundary_crossings as i64),
        );
        map.insert(
            "boundary_motions".to_string(),
            JsonValue::Array(
                self.boundary_motions
                    .iter()
                    .map(BoundaryMotionRecord::to_json)
                    .collect(),
            ),
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
            "couplings".to_string(),
            JsonValue::Array(
                self.active_contacts
                    .values()
                    .map(|w| w.coupling.to_render_json())
                    .collect(),
            ),
        );
        map.insert(
            "expired_contacts".to_string(),
            JsonValue::Array(
                self.expired_contacts
                    .iter()
                    .map(ExpiredContactRecord::to_json)
                    .collect(),
            ),
        );
        map.insert(
            "fusions".to_string(),
            JsonValue::Array(
                self.fusions
                    .iter()
                    .map(FusionEventRecord::to_json)
                    .collect(),
            ),
        );
        map.insert(
            "model_status".to_string(),
            JsonValue::from_str_val(self.model_status.clone()),
        );
        map.insert(
            "resonances".to_string(),
            JsonValue::Array(
                self.resonances
                    .iter()
                    .map(ResonanceRecord::to_json)
                    .collect(),
            ),
        );
        map.insert(
            "shedding_event_count".to_string(),
            JsonValue::from_usize(self.shedding_events.len()),
        );
        map.insert(
            "shedding_events".to_string(),
            JsonValue::Array(
                self.shedding_events
                    .iter()
                    .map(ShedWitness::to_json)
                    .collect(),
            ),
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

fn refresh_active_contact_window(
    win: &mut ActiveContactWindow,
    c0: &CarrierState,
    c1: &CarrierState,
) -> CoreResult<()> {
    if !c0.active
        || !c1.active
        || c0.trace.events.is_empty()
        || c0.trace.events.len() != c1.trace.events.len()
        || c0.trace.scope != c1.trace.scope
        || c0.trace.lineage == c1.trace.lineage
    {
        return Ok(());
    }
    let prior_contacts = win.coupling.contacts.clone();
    let mut cpl = Coupling::new(&c0.trace, &c1.trace, win.coupling.window.clone())?;
    for (a, b) in prior_contacts {
        cpl = cpl.contact(a, b)?;
    }
    if let (Some(cl0), Some(cl1)) = (c0.trace.closures.last(), c1.trace.closures.last()) {
        if cl0.end > win.last_bound.0 && cl1.end > win.last_bound.1 {
            cpl = cpl.contact(cl0.end, cl1.end)?;
            win.last_bound = (cl0.end, cl1.end);
        }
    }
    win.coupling = cpl;
    Ok(())
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
    pub state_json: String,
    pub carrier_count: usize,
    pub active_carrier_count: usize,
    pub shedding_event_count: usize,
    pub wavelet_event_count: usize,
    pub boundary_crossings: u64,
    pub shedding_witnesses_json: String,
    pub resonance_count: usize,
    pub expired_contact_count: usize,
    pub fusion_count: usize,
    pub moving_flux_total: BigInt,
    pub blind_motif_spectrum: BlindMotifSpectrum,
    pub sensor_currents: Vec<SensorCurrentSummary>,
    pub procedural_outputs: BTreeMap<String, Vec<String>>,
    pub final_snapshot: ChamberSnapshot,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::runtime_symbol_table;
    use crate::ops::RtState;
    use crate::value::RtValue;
    use aod_core::json::parse_strict_json;

    fn sup_336() -> SupportForm {
        SupportForm::parse("3:3:6").unwrap()
    }

    #[test]
    fn fixture_06_counts_and_full_shed_witness_serialization_preserved() {
        let space = BoxRegion {
            x: (0, 64),
            y: (0, 64),
            z: (0, 64),
        };
        let c1 = CarrierState::new(1, "SandboxLab", sup_336(), 2, [26, 32, 32], AxisDir::PosX, T1::POS).unwrap();
        let c2 = CarrierState::new(2, "SandboxLab", sup_336(), 2, [38, 32, 32], AxisDir::NegX, T1::NEG).unwrap();
        let clk = ReferenceClock::new("ref_clk", 3).unwrap();
        let mut chamber = ChamberState::new("SandboxLab", space, WallPolicy::Open, vec![c1, c2], vec![clk]);
        for _ in 0..12 {
            chamber.step().unwrap();
        }
        assert_eq!(chamber.carriers.len(), 2);
        assert_eq!(chamber.active_carrier_count(), 2);
        assert_eq!(chamber.shedding_events.len(), 1);
        assert_eq!(chamber.wavelet_events.len(), 1);
        let total_returns: u64 = chamber.carriers.iter().map(|c| c.return_count).sum();
        let total_hinges: u64 = chamber.carriers.iter().map(|c| c.hinge_count).sum();
        assert_eq!(total_returns, 8);
        assert_eq!(total_hinges, 8);
        assert_eq!(chamber.resonances.len(), 0);

        let w = &chamber.shedding_events[0];
        assert_eq!(w.route, ShedRoute::Exo);
        assert_eq!(w.cut_coordinate, "cell_[32,32,32]_tick_6");
        let parts: Vec<&str> = w.post_shed_parent_state.split(':').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].len(), 64);
        assert_eq!(parts[1].len(), 64);

        let wj = w.to_json();
        let obj = wj.as_object().unwrap();
        assert_eq!(obj.len(), 7);
        assert_eq!(obj.get("route").and_then(JsonValue::as_str), Some("exo"));

        let parsed_state = parse_strict_json(&chamber.to_json().to_compact_ascii_json()).unwrap();
        assert_eq!(parsed_state.as_object().unwrap().len(), 16);
    }

    #[test]
    fn moving_boundary_open_wall_and_contact_ttl_expiry() {
        let space = BoxRegion {
            x: (0, 8),
            y: (0, 4),
            z: (0, 4),
        };
        let c1 = CarrierState::new(1, "DynLab", sup_336(), 2, [0, 1, 1], AxisDir::PosX, T1::POS).unwrap();
        let c2 = CarrierState::new(2, "DynLab", sup_336(), 2, [7, 2, 2], AxisDir::PosX, T1::NEG).unwrap();
        let mut chamber = ChamberState::new("DynLab", space, WallPolicy::Open, vec![c1, c2], Vec::new())
            .with_dynamics(ChamberDynamicsConfig {
                boundary_step: [1, 0, 0],
                contact_ttl: Some(1),
                fuse_on_resonance: false,
            });
        // Tick 1: space shifts [0,8) -> [1,9); c1 at x=0 is swept (sweep_flux = 30, ShedRoute::Open);
        // c2 steps 7 -> 8 (still inside [1,9)).
        chamber.step().unwrap();
        assert_eq!(chamber.active_carrier_count(), 1);
        assert_eq!(chamber.boundary_motions.len(), 1);
        assert_eq!(chamber.boundary_motions[0].sweep_flux, BigInt::from_i64(30));
        assert_eq!(chamber.boundary_motions[0].net_moving_flux, BigInt::from_i64(-30));
        assert_eq!(chamber.shedding_events[0].route, ShedRoute::Open);

        // Contact TTL expiry test
        let space2 = BoxRegion {
            x: (0, 32),
            y: (0, 4),
            z: (0, 4),
        };
        let a = CarrierState::new(1, "TtlLab", sup_336(), 2, [10, 1, 1], AxisDir::PosX, T1::POS).unwrap();
        let b = CarrierState::new(2, "TtlLab", sup_336(), 2, [16, 1, 1], AxisDir::NegX, T1::NEG).unwrap();
        let mut ttl_chamber = ChamberState::new("TtlLab", space2, WallPolicy::Open, vec![a, b], Vec::new())
            .with_dynamics(ChamberDynamicsConfig {
                boundary_step: [0, 0, 0],
                contact_ttl: Some(1),
                fuse_on_resonance: false,
            });
        // Coincide at tick 3 (x=13), bounce apart on tick 4, expire on tick 5 (5 - 3 = 2 > 1)
        for _ in 0..6 {
            ttl_chamber.step().unwrap();
        }
        assert_eq!(ttl_chamber.expired_contacts.len(), 1);
        assert_eq!(ttl_chamber.expired_contacts[0].bound_contacts, 1);
        assert!(ttl_chamber.expired_contacts[0].stage_only);
    }

    #[test]
    fn closed_resonance_endo_shedding_and_composite_fusion_plus_ops_fields() {
        // Multi-bounce closed box resonance: space [0..5, 0..1, 0..1], carriers at x=1 (+X) & x=3 (-X)
        let space = BoxRegion {
            x: (0, 5),
            y: (0, 1),
            z: (0, 1),
        };
        let c1 = CarrierState::new(1, "ResLab", sup_336(), 2, [1, 0, 0], AxisDir::PosX, T1::POS).unwrap();
        let c2 = CarrierState::new(2, "ResLab", sup_336(), 2, [3, 0, 0], AxisDir::NegX, T1::NEG).unwrap();
        let mut chamber = ChamberState::new("ResLab", space, WallPolicy::Closed, vec![c1, c2], Vec::new())
            .with_dynamics(ChamberDynamicsConfig {
                boundary_step: [0, 0, 0],
                contact_ttl: None,
                fuse_on_resonance: true,
            });
        // Tick 1: coincide at x=2 (Exo, not closed yet). Tick 3: close #1 at walls.
        // Tick 6: coincide at x=2 again with 2 bound contacts -> ResonanceRecord + ShedRoute::Endo + Fusion!
        // Tick 7..9: fused outer carrier continues cleanly for another closed Q4 cycle!
        for _ in 0..9 {
            chamber.step().unwrap();
        }
        assert_eq!(chamber.resonances.len(), 1);
        assert!(chamber.resonances[0].drift.is_zero());
        assert_eq!(chamber.shedding_events.len(), 2);
        assert_eq!(chamber.shedding_events[0].route, ShedRoute::Exo);
        assert_eq!(chamber.shedding_events[1].route, ShedRoute::Endo);
        assert_eq!(chamber.fusions.len(), 1);
        assert_eq!(chamber.carriers.len(), 3);
        assert_eq!(chamber.active_carrier_count(), 1);
        assert_eq!(chamber.carriers[2].id, 3);
        assert_eq!(chamber.carriers[2].return_count, 2);

        // Verify RtState op_sim_builder_set_dynamics, finalize_sim_result, op_field_get, & JIT symbol
        let syms = runtime_symbol_table();
        assert!(syms.iter().any(|&(name, _)| name == "aod_rt_sim_builder_set_dynamics"));

        let mut rt = RtState::with_default_arena(true);
        let name_h = rt.arena.insert(RtValue::Str("ResLab".to_string())).unwrap();
        let box_h = rt.arena.insert(RtValue::Box3(space)).unwrap();
        let b_h = rt.op_sim_builder_new(name_h, box_h, WallPolicy::Closed.to_code()).unwrap();
        rt.op_sim_builder_set_dynamics(b_h, &BigInt::zero(), &BigInt::zero(), &BigInt::zero(), -1, 1)
            .unwrap();
        let res_h = rt.finalize_sim_result(chamber, Vec::new()).unwrap();

        assert_eq!(
            rt.op_field_get(res_h, "active_carrier_count").unwrap(),
            RtValue::Int(BigInt::from_i64(1))
        );
        assert_eq!(
            rt.op_field_get(res_h, "resonance_count").unwrap(),
            RtValue::Int(BigInt::from_i64(1))
        );
        assert_eq!(
            rt.op_field_get(res_h, "fusion_count").unwrap(),
            RtValue::Int(BigInt::from_i64(1))
        );
        assert_eq!(
            rt.op_field_get(res_h, "expired_contact_count").unwrap(),
            RtValue::Int(BigInt::from_i64(0))
        );
        assert_eq!(
            rt.op_field_get(res_h, "moving_flux_total").unwrap(),
            RtValue::Int(BigInt::zero())
        );
        if let RtValue::Bytes(bs) = rt.op_field_get(res_h, "state_json").unwrap() {
            let parsed = parse_strict_json(std::str::from_utf8(&bs).unwrap()).unwrap();
            assert_eq!(
                parsed.get("active_carrier_count").and_then(JsonValue::as_int),
                Some(&BigInt::from_i64(1))
            );
        } else {
            panic!("expected Bytes for state_json");
        }
        let snap_v = rt.op_field_get(res_h, "final_snapshot").unwrap();
        let snap_h = rt.arena.insert(snap_v).unwrap();
        assert_eq!(
            rt.op_field_get(snap_h, "fusion_count").unwrap(),
            RtValue::Int(BigInt::from_i64(1))
        );
        assert!(matches!(
            rt.op_field_get(snap_h, "shedding_witnesses_json").unwrap(),
            RtValue::Bytes(_)
        ));
    }
}
