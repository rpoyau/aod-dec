use std::collections::BTreeMap;
use std::fmt::Write as _;

use aod_core::bigint::BigInt;
use aod_core::qext::QExtRatio;
use aod_core::ratio::Ratio;
use aod_core::support::SupportForm;
use aod_core::ternary::{Addr8, B64, B8, Q4, T1};
use aod_core::wavelet::{RecipientOutcome, RecipientOutcomeStage, SingleContactStageAssociation};
use aod_core::witness::{Coupling, Fusion, Trace};

use crate::sim::{BoxRegion, ChamberSnapshot, SimRunResult};

/// Canonical ASCII escaping for `bytes` values (`b"..."`), shared with `aodc::eval`.
pub fn render_bytes_literal(bs: &[u8]) -> String {
    let mut out = String::with_capacity(bs.len() + 3);
    out.push_str("b\"");
    for &b in bs {
        match b {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0 => out.push_str("\\0"),
            0x20..=0x7E => out.push(b as char),
            other => {
                let _ = write!(&mut out, "\\x{other:02x}");
            }
        }
    }
    out.push('"');
    out
}

/// Deterministic totally-ordered map key matching `aodc::eval::MapKey`.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum RtMapKey {
    Null,
    Int(BigInt),
    Bool(bool),
    Str(String),
    T1(T1),
    B8(B8),
    B64(B64),
    Bytes(Vec<u8>),
    Q4(Q4),
    Ratio(Ratio),
    Addr8(Addr8),
    Support(SupportForm),
    Array(Vec<RtMapKey>),
    Enum {
        module: String,
        name: String,
        variant: String,
        tag: i32,
        payloads: Vec<RtMapKey>,
    },
    Tuple(Vec<RtMapKey>),
}

impl RtMapKey {
    pub fn from_value(v: &RtValue) -> Result<Self, String> {
        match v {
            RtValue::Null => Ok(RtMapKey::Null),
            RtValue::Int(n) => Ok(RtMapKey::Int(n.clone())),
            RtValue::Bool(b) => Ok(RtMapKey::Bool(*b)),
            RtValue::Str(s) => Ok(RtMapKey::Str(s.clone())),
            RtValue::T1(t) => Ok(RtMapKey::T1(*t)),
            RtValue::B8(b) => Ok(RtMapKey::B8(*b)),
            RtValue::B64(b) => Ok(RtMapKey::B64(*b)),
            RtValue::Bytes(bs) => Ok(RtMapKey::Bytes(bs.clone())),
            RtValue::Q4(q) => Ok(RtMapKey::Q4(*q)),
            RtValue::Ratio(r) => Ok(RtMapKey::Ratio(r.clone())),
            RtValue::Addr8(a) => Ok(RtMapKey::Addr8(a.clone())),
            RtValue::Support(s) => Ok(RtMapKey::Support(s.clone())),
            RtValue::Array(elems) => {
                let mut ak = Vec::with_capacity(elems.len());
                for e in elems {
                    ak.push(Self::from_value(e)?);
                }
                Ok(RtMapKey::Array(ak))
            }
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
            other => Err(format!("unsupported map key value: {}", other.render())),
        }
    }

    pub fn to_value(&self) -> RtValue {
        match self {
            RtMapKey::Null => RtValue::Null,
            RtMapKey::Int(n) => RtValue::Int(n.clone()),
            RtMapKey::Bool(b) => RtValue::Bool(*b),
            RtMapKey::Str(s) => RtValue::Str(s.clone()),
            RtMapKey::T1(t) => RtValue::T1(*t),
            RtMapKey::B8(b) => RtValue::B8(*b),
            RtMapKey::B64(b) => RtValue::B64(*b),
            RtMapKey::Bytes(bs) => RtValue::Bytes(bs.clone()),
            RtMapKey::Q4(q) => RtValue::Q4(*q),
            RtMapKey::Ratio(r) => RtValue::Ratio(r.clone()),
            RtMapKey::Addr8(a) => RtValue::Addr8(a.clone()),
            RtMapKey::Support(s) => RtValue::Support(s.clone()),
            RtMapKey::Array(elems) => RtValue::Array(elems.iter().map(RtMapKey::to_value).collect()),
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
    Bool(bool),
    Str(String),
    T1(T1),
    B8(B8),
    B64(B64),
    Bytes(Vec<u8>),
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
    Array(Vec<RtValue>),
    List(Vec<RtValue>),
    Map(Box<BTreeMap<RtMapKey, RtValue>>),
    Tuple(Vec<RtValue>),
    QExtRatio(QExtRatio),
    QExtBool(bool),
    QExtStr(String),
}

impl RtValue {
    /// Byte-for-byte identical rendering to `aodc::eval::Value::render`.
    pub fn render(&self) -> String {
        match self {
            RtValue::Unit => "()".to_string(),
            RtValue::Null => "null".to_string(),
            RtValue::Int(n) => n.to_decimal_string(),
            RtValue::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            RtValue::Str(s) => s.clone(),
            RtValue::T1(v) => v.render(),
            RtValue::B8(v) => v.render(),
            RtValue::B64(v) => v.render(),
            RtValue::Bytes(bs) => render_bytes_literal(bs),
            RtValue::Q4(v) => v.render(),
            RtValue::Ratio(r) => r.render(),
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
            RtValue::Array(items) | RtValue::List(items) => {
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
        }
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
