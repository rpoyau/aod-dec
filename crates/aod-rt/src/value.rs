use std::collections::BTreeMap;

use aod_core::bigint::BigInt;
use aod_core::qext::QExtRatio;
use aod_core::ratio::Ratio;
use aod_core::support::SupportForm;
use aod_core::ternary::{render_bytes_literal, Addr8, Q4, T1, T32, T9};
use aod_core::wavelet::{RecipientOutcome, RecipientOutcomeStage, SingleContactStageAssociation};
use aod_core::witness::{Coupling, Fusion, Trace};

use crate::sim::{BoxRegion, ChamberSnapshot, SimRunResult};

/// Deterministic totally-ordered map key matching `aodc::eval::MapKey`.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum RtMapKey {
    Null,
    Int(BigInt),
    U8(u8),
    B64(u64),
    Bool(bool),
    Str(String),
    Bytes(Vec<u8>),
    T1(T1),
    T9(T9),
    T32(T32),
    Q4(Q4),
    Ratio(Ratio),
    Addr8(Addr8),
    Support(SupportForm),
    Enum {
        module: String,
        name: String,
        variant: String,
        tag: i32,
        payloads: Vec<RtMapKey>,
    },
    Tuple(Vec<RtMapKey>),
    Array(Vec<RtMapKey>),
}

impl RtMapKey {
    pub fn from_value(v: &RtValue) -> Result<Self, String> {
        match v {
            RtValue::Null => Ok(RtMapKey::Null),
            RtValue::Int(n) => Ok(RtMapKey::Int(n.clone())),
            RtValue::U8(b) => Ok(RtMapKey::U8(*b)),
            RtValue::B64(w) => Ok(RtMapKey::B64(*w)),
            RtValue::Bool(b) => Ok(RtMapKey::Bool(*b)),
            RtValue::Str(s) => Ok(RtMapKey::Str(s.clone())),
            RtValue::Bytes(bs) => Ok(RtMapKey::Bytes(bs.clone())),
            RtValue::T1(t) => Ok(RtMapKey::T1(*t)),
            RtValue::T9(t) => Ok(RtMapKey::T9(*t)),
            RtValue::T32(t) => Ok(RtMapKey::T32(*t)),
            RtValue::Q4(q) => Ok(RtMapKey::Q4(*q)),
            RtValue::Ratio(r) => Ok(RtMapKey::Ratio(r.clone())),
            RtValue::Addr8(a) => Ok(RtMapKey::Addr8(a.clone())),
            RtValue::Support(s) => Ok(RtMapKey::Support(s.clone())),
            RtValue::Enum(ev) => {
                let mut pk = Vec::with_capacity(ev.payloads.len());
                for p in &ev.payloads {
                    pk.push(Self::from_value(p)?);
                }
                Ok(RtMapKey::Enum {
                    module: ev.module.clone(),
                    name: ev.name.clone(),
                    variant: ev.variant.clone(),
                    tag: ev.tag,
                    payloads: pk,
                })
            }
            RtValue::Tuple(elems) => {
                let mut tk = Vec::with_capacity(elems.len());
                for e in elems {
                    tk.push(Self::from_value(e)?);
                }
                Ok(RtMapKey::Tuple(tk))
            }
            RtValue::Array(elems) => {
                let mut ak = Vec::with_capacity(elems.len());
                for e in elems {
                    ak.push(Self::from_value(e)?);
                }
                Ok(RtMapKey::Array(ak))
            }
            other => Err(format!("unsupported map key value: {}", other.render())),
        }
    }

    pub fn to_value(&self) -> RtValue {
        match self {
            RtMapKey::Null => RtValue::Null,
            RtMapKey::Int(n) => RtValue::Int(n.clone()),
            RtMapKey::U8(b) => RtValue::U8(*b),
            RtMapKey::B64(w) => RtValue::B64(*w),
            RtMapKey::Bool(b) => RtValue::Bool(*b),
            RtMapKey::Str(s) => RtValue::Str(s.clone()),
            RtMapKey::Bytes(bs) => RtValue::Bytes(bs.clone()),
            RtMapKey::T1(t) => RtValue::T1(*t),
            RtMapKey::T9(t) => RtValue::T9(*t),
            RtMapKey::T32(t) => RtValue::T32(*t),
            RtMapKey::Q4(q) => RtValue::Q4(*q),
            RtMapKey::Ratio(r) => RtValue::Ratio(r.clone()),
            RtMapKey::Addr8(a) => RtValue::Addr8(a.clone()),
            RtMapKey::Support(s) => RtValue::Support(s.clone()),
            RtMapKey::Enum {
                module,
                name,
                variant,
                tag,
                payloads,
            } => RtValue::Enum(Box::new(RtEnumValue {
                module: module.clone(),
                name: name.clone(),
                variant: variant.clone(),
                tag: *tag,
                payloads: payloads.iter().map(RtMapKey::to_value).collect(),
            })),
            RtMapKey::Tuple(elems) => RtValue::Tuple(elems.iter().map(RtMapKey::to_value).collect()),
            RtMapKey::Array(elems) => RtValue::Array(elems.iter().map(RtMapKey::to_value).collect()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RtStructValue {
    pub module: String,
    pub name: String,
    pub fields: BTreeMap<String, RtValue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RtEnumValue {
    pub module: String,
    pub name: String,
    pub variant: String,
    pub tag: i32,
    pub payloads: Vec<RtValue>,
}

/// Arena-resident exact runtime value shared by AOT and ORC LLJIT native code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RtValue {
    Unit,
    Null,
    Int(BigInt),
    U8(u8),
    B64(u64),
    Bool(bool),
    Str(String),
    Bytes(Vec<u8>),
    T1(T1),
    T9(T9),
    T32(T32),
    Q4(Q4),
    Ratio(Ratio),
    Addr8(Addr8),
    Support(SupportForm),
    Window(i64, i64),
    Range(i64, i64),
    Box3(BoxRegion),
    Trace(Box<Trace>),
    Fusion(Box<Fusion>),
    Coupling(Box<Coupling>),
    RecipientOutcome(Box<RecipientOutcome>),
    CadenceAssociation(Box<SingleContactStageAssociation>),
    ChamberView(Box<ChamberSnapshot>),
    SimResult(Box<SimRunResult>),
    Struct(Box<RtStructValue>),
    Enum(Box<RtEnumValue>),
    List(Vec<RtValue>),
    Array(Vec<RtValue>),
    Map(Box<BTreeMap<RtMapKey, RtValue>>),
    Tuple(Vec<RtValue>),
    QExtRatio(QExtRatio),
    QExtBool(bool),
    QExtStr(String),
    QExtBytes(Vec<u8>),
}

impl RtValue {
    /// Byte-for-byte identical rendering to `aodc::eval::Value::render` & v1 `dec_core.py`.
    pub fn render(&self) -> String {
        match self {
            RtValue::Unit => "()".to_string(),
            RtValue::Null => "null".to_string(),
            RtValue::Int(n) => n.to_decimal_string(),
            RtValue::U8(b) => format!("u8({b})"),
            RtValue::B64(w) => format!("b64(0x{w:016x})"),
            RtValue::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            RtValue::Str(s) => s.clone(),
            RtValue::Bytes(bs) => render_bytes_literal(bs),
            RtValue::T1(v) => v.render_v1(),
            RtValue::T9(v) => v.render(),
            RtValue::T32(v) => v.render_v1(),
            RtValue::Q4(v) => v.render_v1(),
            RtValue::Ratio(r) => r.render_v1(),
            RtValue::Addr8(a) => a.render(),
            RtValue::Support(s) => s.render(),
            RtValue::Window(u, v) => format!("({u}, {v}]"),
            RtValue::Range(a, b) => format!("{a}..{b}"),
            RtValue::Box3(b) => format!(
                "box([{}..{}, {}..{}, {}..{}])",
                b.x.0, b.x.1, b.y.0, b.y.1, b.z.0, b.z.1
            ),
            RtValue::Trace(t) => t.render_v1(),
            RtValue::Fusion(f) => f.render_v1(),
            RtValue::Coupling(c) => c.render_v1(),
            RtValue::RecipientOutcome(o) => format!(
                "recipient({:?}, phase_delta={}, bip_delta={})",
                o.stage, o.closure_phase_delta, o.reclosure_bip_delta
            ),
            RtValue::CadenceAssociation(a) => format!(
                "single_contact({}, ref={}, is_cadence_ratio={})",
                a.wavelet_id, a.reference_id, a.is_cadence_ratio
            ),
            RtValue::ChamberView(snap) => format!(
                "ChamberView(chamber={}, tick={}, carriers={})",
                snap.chamber_name,
                snap.tick,
                snap.carriers.len()
            ),
            RtValue::SimResult(res) => format!(
                "SimResult(model_status={}, chamber={}, ticks={}, hash={})",
                res.model_status, res.chamber_name, res.ticks, res.final_state_hash
            ),
            RtValue::Struct(sv) => {
                let inner: Vec<String> = sv
                    .fields
                    .iter()
                    .map(|(k, v)| format!("{k}: {}", v.render()))
                    .collect();
                format!("{} {{ {} }}", sv.name, inner.join(", "))
            }
            RtValue::Enum(ev) => {
                if ev.payloads.is_empty() {
                    format!("{}::{}", ev.name, ev.variant)
                } else {
                    let inner: Vec<String> = ev.payloads.iter().map(RtValue::render).collect();
                    format!("{}::{}({})", ev.name, ev.variant, inner.join(", "))
                }
            }
            RtValue::List(items) | RtValue::Array(items) => {
                let inner: Vec<String> = items.iter().map(RtValue::render).collect();
                format!("[{}]", inner.join(", "))
            }
            RtValue::Map(m) => {
                let inner: Vec<String> = m
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.to_value().render(), v.render()))
                    .collect();
                format!("{{{}}}", inner.join(", "))
            }
            RtValue::Tuple(elems) => {
                let inner: Vec<String> = elems.iter().map(RtValue::render).collect();
                format!("({})", inner.join(", "))
            }
            RtValue::QExtRatio(q) => q.render_quarantined(),
            RtValue::QExtBool(b) => format!("Q_ext({})", if *b { "true" } else { "false" }),
            RtValue::QExtStr(s) => format!("Q_ext({s})"),
            RtValue::QExtBytes(bs) => format!("Q_ext({})", render_bytes_literal(bs)),
        }
    }
}

pub fn model_status_variant_and_tag(status: &str) -> (&'static str, i32) {
    match status {
        "DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED" => {
            ("DeclaredSandboxNotS3Authenticated", 0)
        }
        _ => ("DeclaredSandboxNotS3Authenticated", 0),
    }
}

pub fn recipient_stage_variant_and_tag(stage: RecipientOutcomeStage) -> (&'static str, i32) {
    match stage {
        RecipientOutcomeStage::Emitted => ("Emitted", 0),
        RecipientOutcomeStage::Delivered => ("Delivered", 1),
        RecipientOutcomeStage::Accepted => ("Accepted", 2),
        RecipientOutcomeStage::UnacceptedPass => ("UnacceptedPass", 3),
        RecipientOutcomeStage::Seated => ("Seated", 4),
        RecipientOutcomeStage::PendingUnseated => ("PendingUnseated", 5),
        RecipientOutcomeStage::ClosedOrigin => ("ClosedOrigin", 6),
        RecipientOutcomeStage::Reclosed => ("Reclosed", 7),
        RecipientOutcomeStage::ReclosedReplay => ("ReclosedReplay", 8),
        RecipientOutcomeStage::ReclosedOutOfWindow => ("ReclosedOutOfWindow", 9),
    }
}

pub fn recipient_stage_name(stage: RecipientOutcomeStage) -> &'static str {
    match stage {
        RecipientOutcomeStage::Emitted => "emitted",
        RecipientOutcomeStage::Delivered => "delivered",
        RecipientOutcomeStage::Accepted => "accepted",
        RecipientOutcomeStage::UnacceptedPass => "unaccepted_pass",
        RecipientOutcomeStage::Seated => "seated",
        RecipientOutcomeStage::PendingUnseated => "pending_unseated",
        RecipientOutcomeStage::ClosedOrigin => "closed_origin",
        RecipientOutcomeStage::Reclosed => "reclosed",
        RecipientOutcomeStage::ReclosedReplay => "reclosed_replay",
        RecipientOutcomeStage::ReclosedOutOfWindow => "reclosed_out_of_window",
    }
}
