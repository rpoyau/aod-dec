use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use aod_core::bigint::BigInt;
use aod_core::ledger::{probability, SadarPromotion};
use aod_core::qext::{parse_decimal_exact, QExtRatio};
use aod_core::ratio::Ratio;
use aod_core::sha256::Sha256;
use aod_core::support::SupportForm;
use aod_core::ternary::{Addr8, B64, B8, Q4, T1};
use aod_core::trit_int::{pack_compact_ratio_u64, unpack_compact_ratio_u64};
use aod_core::wavelet::{
    declare_synthetic_lineage_registry, deduplicated_return_count, native_cadence_ratio,
    AdmittedClosureRecord, M3ClosureTrace, RecipientContext, RecipientLattice, RecipientOutcome,
    RecipientOutcomeStage, RecipientStage, ShedRoute, ShedWitness, SingleContactStageAssociation,
    StageEventWitness, Wavelet, WaveletRole, WaveletSpec,
};
use aod_core::witness::{Coupling, Fusion, SeatState, Trace};
use aod_rt::ops::{
    checked_trit_pow, decode_hex_32, exact_unpack_t9_bits, exact_verify_frg_cross_return,
    exact_verify_sheddic_route, exact_verify_sheddic_route_int, MAX_SYS_IO_BYTES,
};
use aod_rt::value::{
    model_status_variant_and_tag, recipient_stage_variant_and_tag, render_bytes_literal,
};

use crate::ast::{
    AdverbOp, BinaryOp, ChamberDecl, DecArmParam, DecFnDecl, EnumDecl, EnumVariantDecl, Expr,
    ExprKind, Item, LetPattern, ObserveDecl, Pattern, PipeStage, PrepareDecl, ProbeDecl, Program,
    Stmt, StmtKind, StructDecl, UnaryOp,
};
use crate::checker::CheckedUnit;
use crate::diagnostics::{DecResult, Diagnostic, DiagnosticCode};
use crate::modules::{builtin_namespace_items, canonical_builtin_name, ImportedSymbol};
use crate::sim::{
    BlindMotifSpectrum, BoxRegion, CarrierState, ChamberSnapshot, ChamberState,
    DeclarativeObserver, ReferenceClock, SensorCurrentSummary, SimRunResult,
};
use crate::types::{
    builtin_enum_variants, builtin_ns_exports_enum, Type, BUILTIN_MOD, MODEL_STATUS_VARIANTS,
    RECIPIENT_STAGE_VARIANTS,
};

pub const DEFAULT_FUEL: usize = 100_000;
pub const DEFAULT_MAX_DEPTH: usize = 128;

/// Deterministic totally-ordered map key matching `aod_rt::value::RtMapKey` byte-for-byte.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum MapKey {
    Null,
    Int(BigInt),
    Bool(bool),
    T1(T1),
    B8(B8),
    B64(B64),
    Bytes(Vec<u8>),
    Q4(Q4),
    Ratio(Ratio),
    Addr8(Addr8),
    Support(SupportForm),
    Array(Vec<MapKey>),
    Enum {
        module: String,
        name: String,
        variant: String,
        tag: i32,
        payloads: Vec<MapKey>,
    },
    Tuple(Vec<MapKey>),
}

impl MapKey {
    pub fn from_value(v: &Value, line: usize) -> DecResult<Self> {
        match v {
            Value::Null => Ok(MapKey::Null),
            Value::Int(n) => Ok(MapKey::Int(n.clone())),
            Value::Bool(b) => Ok(MapKey::Bool(*b)),
            Value::T1(t) => Ok(MapKey::T1(*t)),
            Value::B8(b) => Ok(MapKey::B8(*b)),
            Value::B64(b) => Ok(MapKey::B64(*b)),
            Value::Bytes(bs) => Ok(MapKey::Bytes(bs.clone())),
            Value::Q4(q) => Ok(MapKey::Q4(*q)),
            Value::Ratio(r) => Ok(MapKey::Ratio(r.clone())),
            Value::Addr8(a) => Ok(MapKey::Addr8(a.clone())),
            Value::Support(s) => Ok(MapKey::Support(s.clone())),
            Value::Array(elems) => {
                let mut ak = Vec::with_capacity(elems.len());
                for e in elems {
                    ak.push(Self::from_value(e, line)?);
                }
                Ok(MapKey::Array(ak))
            }
            Value::Enum(ev) => {
                let mut pk = Vec::with_capacity(ev.payloads.len());
                for p in &ev.payloads {
                    pk.push(Self::from_value(p, line)?);
                }
                Ok(MapKey::Enum {
                    module: ev.module.clone(),
                    name: ev.name.clone(),
                    variant: ev.variant.clone(),
                    tag: ev.tag,
                    payloads: pk,
                })
            }
            Value::Tuple(elems) => {
                let mut tk = Vec::with_capacity(elems.len());
                for e in elems {
                    tk.push(Self::from_value(e, line)?);
                }
                Ok(MapKey::Tuple(tk))
            }
            other => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!("unsupported map key value of type {}", other.type_of()),
            )),
        }
    }

    pub fn to_value(&self) -> Value {
        match self {
            MapKey::Null => Value::Null,
            MapKey::Int(n) => Value::Int(n.clone()),
            MapKey::Bool(b) => Value::Bool(*b),
            MapKey::T1(t) => Value::T1(*t),
            MapKey::B8(b) => Value::B8(*b),
            MapKey::B64(b) => Value::B64(*b),
            MapKey::Bytes(bs) => Value::Bytes(bs.clone()),
            MapKey::Q4(q) => Value::Q4(*q),
            MapKey::Ratio(r) => Value::Ratio(r.clone()),
            MapKey::Addr8(a) => Value::Addr8(a.clone()),
            MapKey::Support(s) => Value::Support(s.clone()),
            MapKey::Array(elems) => Value::Array(elems.iter().map(MapKey::to_value).collect()),
            MapKey::Enum {
                module,
                name,
                variant,
                tag,
                payloads,
            } => Value::Enum(Box::new(EnumValue {
                module: module.clone(),
                name: name.clone(),
                variant: variant.clone(),
                tag: *tag,
                payloads: payloads.iter().map(MapKey::to_value).collect(),
            })),
            MapKey::Tuple(elems) => Value::Tuple(elems.iter().map(MapKey::to_value).collect()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructValue {
    pub module: String,
    pub name: String,
    pub fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnumValue {
    pub module: String,
    pub name: String,
    pub variant: String,
    pub tag: i32,
    pub payloads: Vec<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Value {
    Unit,
    Null,
    Int(BigInt),
    Bool(bool),
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
    Struct(Box<StructValue>),
    Enum(Box<EnumValue>),
    Array(Vec<Value>),
    List(Vec<Value>),
    Map(Box<BTreeMap<MapKey, Value>>),
    Tuple(Vec<Value>),
    QExtRatio(QExtRatio),
    QExtBool(bool),
    QExtBytes(Vec<u8>),
}

impl Value {
    pub fn type_of(&self) -> Type {
        match self {
            Value::Unit => Type::Unit,
            Value::Null => Type::Null,
            Value::Int(_) => Type::Int,
            Value::Bool(_) => Type::Bool,
            Value::T1(_) => Type::T1,
            Value::B8(_) => Type::B8,
            Value::B64(_) => Type::B64,
            Value::Bytes(_) => Type::Bytes,
            Value::Q4(_) => Type::Q4,
            Value::Ratio(_) => Type::Ratio,
            Value::Addr8(_) => Type::Addr8,
            Value::Support(_) => Type::Support,
            Value::Window(_, _) => Type::Window,
            Value::Range(_, _) => Type::Range,
            Value::Box3(_) => Type::Box3,
            Value::Trace(_) => Type::Trace,
            Value::Fusion(_) => Type::Fusion,
            Value::Coupling(_) => Type::Coupling,
            Value::RecipientOutcome(_) => Type::RecipientOutcome,
            Value::CadenceAssociation(_) => Type::CadenceAssociation,
            Value::ChamberView(_) => Type::ChamberView,
            Value::SimResult(_) => Type::SimResult,
            Value::Struct(sv) => Type::Struct {
                module: sv.module.clone(),
                name: sv.name.clone(),
            },
            Value::Enum(ev) => Type::Enum {
                module: ev.module.clone(),
                name: ev.name.clone(),
            },
            Value::Array(items) => {
                let elem = items.first().map(Value::type_of).unwrap_or(Type::Unknown);
                Type::Array(Box::new(elem), items.len())
            }
            Value::List(items) => {
                let elem = items.first().map(Value::type_of).unwrap_or(Type::Unknown);
                Type::List(Box::new(elem))
            }
            Value::Map(m) => match m.iter().next() {
                Some((k, v)) => Type::Map(Box::new(k.to_value().type_of()), Box::new(v.type_of())),
                None => Type::Map(Box::new(Type::Unknown), Box::new(Type::Unknown)),
            },
            Value::Tuple(elems) => Type::Tuple(elems.iter().map(Value::type_of).collect()),
            Value::QExtRatio(_) => Type::QExt(Box::new(Type::Ratio)),
            Value::QExtBool(_) => Type::QExt(Box::new(Type::Bool)),
            Value::QExtBytes(_) => Type::QExt(Box::new(Type::Bytes)),
        }
    }

    /// Deterministic rendering matching `aod_rt::value::RtValue::render` byte-for-byte.
    pub fn render(&self) -> String {
        match self {
            Value::Unit => "()".to_string(),
            Value::Null => "null".to_string(),
            Value::Int(n) => n.to_decimal_string(),
            Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            Value::T1(v) => v.render(),
            Value::B8(v) => v.render(),
            Value::B64(v) => v.render(),
            Value::Bytes(bs) => render_bytes_literal(bs),
            Value::Q4(v) => v.render(),
            Value::Ratio(r) => r.render(),
            Value::Addr8(a) => a.render(),
            Value::Support(s) => s.render(),
            Value::Window(u, v) => format!("({u}, {v}]"),
            Value::Range(a, b) => format!("{a}..{b}"),
            Value::Box3(b) => format!(
                "box([{}..{}, {}..{}, {}..{}])",
                b.x.0, b.x.1, b.y.0, b.y.1, b.z.0, b.z.1
            ),
            Value::Trace(t) => t.render_v1(),
            Value::Fusion(f) => f.render_v1(),
            Value::Coupling(c) => c.render_v1(),
            Value::RecipientOutcome(o) => format!(
                "recipient({:?}, phase_delta={}, bip_delta={})",
                o.stage, o.closure_phase_delta, o.reclosure_bip_delta
            ),
            Value::CadenceAssociation(a) => format!(
                "single_contact({}, ref={}, is_cadence_ratio={})",
                a.wavelet_id, a.reference_id, a.is_cadence_ratio
            ),
            Value::ChamberView(snap) => format!(
                "ChamberView(chamber={}, tick={}, carriers={})",
                snap.chamber_name,
                snap.tick,
                snap.carriers.len()
            ),
            Value::SimResult(res) => format!(
                "SimResult(model_status={}, chamber={}, ticks={}, hash={})",
                res.model_status, res.chamber_name, res.ticks, res.final_state_hash
            ),
            Value::Struct(sv) => {
                let inner: Vec<String> = sv
                    .fields
                    .iter()
                    .map(|(k, v)| format!("{k}: {}", v.render()))
                    .collect();
                format!("{} {{ {} }}", sv.name, inner.join(", "))
            }
            Value::Enum(ev) => {
                if ev.payloads.is_empty() {
                    format!("{}::{}", ev.name, ev.variant)
                } else {
                    let inner: Vec<String> = ev.payloads.iter().map(Value::render).collect();
                    format!("{}::{}({})", ev.name, ev.variant, inner.join(", "))
                }
            }
            Value::Array(items) | Value::List(items) => {
                let inner: Vec<String> = items.iter().map(Value::render).collect();
                format!("[{}]", inner.join(", "))
            }
            Value::Map(m) => {
                let inner: Vec<String> = m
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.to_value().render(), v.render()))
                    .collect();
                format!("{{{}}}", inner.join(", "))
            }
            Value::Tuple(elems) => {
                let inner: Vec<String> = elems.iter().map(Value::render).collect();
                format!("({})", inner.join(", "))
            }
            Value::QExtRatio(q) => q.render_quarantined(),
            Value::QExtBool(b) => format!("Q_ext({})", if *b { "true" } else { "false" }),
            Value::QExtBytes(bs) => format!("Q_ext({})", render_bytes_literal(bs)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct EvalOutput {
    pub emits: Vec<String>,
    pub result: Value,
}

impl EvalOutput {
    pub fn to_stdout_string(&self) -> String {
        let mut out = String::new();
        for line in &self.emits {
            out.push_str(line);
            out.push('\n');
        }
        if self.result != Value::Unit {
            out.push_str(&self.result.render());
            out.push('\n');
        }
        out
    }
}

enum ControlFlow {
    Normal,
    Return(Value),
}

struct EvalProgram<'a> {
    checked: &'a CheckedUnit,
    structs: BTreeMap<(String, String), StructDecl>,
    enums: BTreeMap<(String, String), EnumDecl>,
    functions: BTreeMap<(String, String), DecFnDecl>,
    consts: BTreeMap<(String, String), Value>,
    chambers: BTreeMap<(String, String), ChamberDecl>,
    prepares: BTreeMap<(String, String), PrepareDecl>,
    observes: BTreeMap<(String, String), ObserveDecl>,
    qext_modules: BTreeSet<(String, String)>,
}

enum EvalTarget {
    Builtin(String),
    UserFn { mod_key: String, name: String },
}

pub struct Evaluator<'a> {
    program: Arc<EvalProgram<'a>>,
    fuel: usize,
    max_depth: usize,
    depth: usize,
    emits: Vec<String>,
    cli_args: Vec<Vec<u8>>,
    parallel_observers: bool,
}

impl<'a> Evaluator<'a> {
    pub fn new(checked: &'a CheckedUnit, fuel: usize, max_depth: usize) -> DecResult<Self> {
        let mut ev = Self {
            program: Arc::new(EvalProgram {
                checked,
                structs: BTreeMap::new(),
                enums: BTreeMap::new(),
                functions: BTreeMap::new(),
                consts: BTreeMap::new(),
                chambers: BTreeMap::new(),
                prepares: BTreeMap::new(),
                observes: BTreeMap::new(),
                qext_modules: BTreeSet::new(),
            }),
            fuel,
            max_depth,
            depth: 0,
            emits: Vec::new(),
            cli_args: Vec::new(),
            parallel_observers: false,
        };
        ev.register_builtin_enums();
        ev.index_program("<entry>", &checked.unit.entry_program)?;
        for (mod_key, prog) in &checked.unit.parsed_modules {
            ev.index_program(mod_key, prog)?;
        }
        for mod_key in &checked.unit.module_load_order {
            if let Some(prog) = checked.unit.parsed_modules.get(mod_key) {
                ev.eval_program_consts(mod_key, prog)?;
            }
        }
        ev.eval_program_consts("<entry>", &checked.unit.entry_program)?;
        Ok(ev)
    }

    pub fn with_parallel_observers(mut self, enabled: bool) -> Self {
        self.parallel_observers = enabled;
        self
    }

    pub fn with_cli_args(mut self, cli_args: Vec<Vec<u8>>) -> Self {
        self.cli_args = cli_args;
        self
    }

    fn register_builtin_enums(&mut self) {
        let p = Arc::get_mut(&mut self.program)
            .expect("Evaluator::register_builtin_enums runs exclusively during initialization");
        for (ename, variants) in [
            ("ModelStatus", MODEL_STATUS_VARIANTS),
            ("RecipientStage", RECIPIENT_STAGE_VARIANTS),
        ] {
            p.enums.insert(
                (BUILTIN_MOD.to_string(), ename.to_string()),
                EnumDecl {
                    name: ename.to_string(),
                    variants: variants
                        .iter()
                        .map(|(vn, _)| EnumVariantDecl {
                            name: (*vn).to_string(),
                            payloads: Vec::new(),
                            line: 0,
                        })
                        .collect(),
                    line: 0,
                },
            );
        }
    }

    fn base_module_of<'b>(&self, mod_key: &'b str) -> &'b str {
        if let Some((parent, leaf)) = mod_key.rsplit_once("::") {
            if self
                .program
                .qext_modules
                .contains(&(parent.to_string(), leaf.to_string()))
            {
                return parent;
            }
        }
        mod_key
    }

    fn resolve_qext_submodule(&self, mod_key: &str, ns: &str) -> Option<String> {
        if self
            .program
            .qext_modules
            .contains(&(mod_key.to_string(), ns.to_string()))
        {
            return Some(format!("{mod_key}::{ns}"));
        }
        let base = self.base_module_of(mod_key);
        if base != mod_key
            && self
                .program
                .qext_modules
                .contains(&(base.to_string(), ns.to_string()))
        {
            return Some(format!("{base}::{ns}"));
        }
        if let Some(ImportedSymbol::ModuleItem { module, item }) =
            self.imports_for(mod_key).get(ns)
        {
            if self
                .program
                .qext_modules
                .contains(&(module.clone(), item.clone()))
            {
                return Some(format!("{module}::{item}"));
            }
        }
        None
    }

    fn resolve_qext_submodule_3seg(&self, mod_key: &str, ns: &str, qmod: &str) -> Option<String> {
        if let Some(ImportedSymbol::ModuleNamespace(m)) = self.imports_for(mod_key).get(ns) {
            if self
                .program
                .qext_modules
                .contains(&(m.clone(), qmod.to_string()))
            {
                return Some(format!("{m}::{qmod}"));
            }
        }
        None
    }

    fn index_program(&mut self, mod_key: &str, prog: &Program) -> DecResult<()> {
        let p = Arc::get_mut(&mut self.program)
            .expect("Evaluator::index_program runs exclusively during initialization");
        for item in &prog.items {
            match item {
                Item::Struct(s) => {
                    p.structs
                        .insert((mod_key.to_string(), s.name.clone()), s.clone());
                }
                Item::Enum(e) => {
                    p.enums
                        .insert((mod_key.to_string(), e.name.clone()), e.clone());
                }
                Item::DecFn(f) => {
                    p.functions
                        .insert((mod_key.to_string(), f.name.clone()), f.clone());
                }
                Item::Chamber(c) => {
                    p.chambers
                        .insert((mod_key.to_string(), c.name.clone()), c.clone());
                }
                Item::Prepare(pr) => {
                    p.prepares
                        .insert((mod_key.to_string(), pr.name.clone()), pr.clone());
                }
                Item::Observe(o) => {
                    p.observes
                        .insert((mod_key.to_string(), o.name.clone()), o.clone());
                }
                Item::QuarantineMod(qm) => {
                    p.qext_modules
                        .insert((mod_key.to_string(), qm.name.clone()));
                    let sub_key = format!("{mod_key}::{}", qm.name);
                    for sub in &qm.items {
                        if let Item::DecFn(f) = sub {
                            p.functions
                                .insert((sub_key.clone(), f.name.clone()), f.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn eval_program_consts(&mut self, mod_key: &str, prog: &Program) -> DecResult<()> {
        let scopes: Vec<BTreeMap<String, Value>> = Vec::new();
        for item in &prog.items {
            match item {
                Item::Const(c) => {
                    let v = self.eval_expr(&c.value, mod_key, &scopes)?;
                    Arc::get_mut(&mut self.program)
                        .expect("const initialization runs exclusively")
                        .consts
                        .insert((mod_key.to_string(), c.name.clone()), v);
                }
                Item::QuarantineMod(qm) => {
                    let sub_key = format!("{mod_key}::{}", qm.name);
                    for sub in &qm.items {
                        if let Item::Const(c) = sub {
                            let v = self.eval_expr(&c.value, &sub_key, &scopes)?;
                            Arc::get_mut(&mut self.program)
                                .expect("const initialization runs exclusively")
                                .consts
                                .insert((sub_key.clone(), c.name.clone()), v);
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub fn run_main_capturing(mut self) -> (Vec<String>, DecResult<Value>) {
        let result = self.call_user_fn("<entry>", "main", Vec::new(), 0);
        (self.emits, result)
    }

    pub fn run_main(self) -> DecResult<EvalOutput> {
        let (emits, res) = self.run_main_capturing();
        let result = res?;
        Ok(EvalOutput { emits, result })
    }

    pub fn run_entry_fn(mut self, fn_name: &str) -> DecResult<EvalOutput> {
        let result = self.call_user_fn("<entry>", fn_name, Vec::new(), 0)?;
        Ok(EvalOutput {
            emits: self.emits,
            result,
        })
    }

    fn tick(&mut self) -> DecResult<()> {
        if self.fuel == 0 {
            return Err(Diagnostic::new(
                DiagnosticCode::BudgetExhausted,
                "execution step budget exhausted",
            ));
        }
        self.fuel -= 1;
        Ok(())
    }

    fn imports_for(&self, mod_key: &str) -> &BTreeMap<String, ImportedSymbol> {
        let base = self.base_module_of(mod_key);
        if base == "<entry>" {
            &self.program.checked.unit.entry_imports
        } else {
            self.program
                .checked
                .unit
                .module_imports
                .get(base)
                .unwrap_or(&self.program.checked.unit.entry_imports)
        }
    }

    fn lookup_user_type(&self, mod_key: &str, name: &str) -> Option<Type> {
        let base = self.base_module_of(mod_key);
        if name.contains("::") {
            let segs: Vec<&str> = name.split("::").collect();
            if segs.len() == 2 {
                if let Some(ImportedSymbol::ModuleNamespace(m)) =
                    self.imports_for(mod_key).get(segs[0])
                {
                    if let Some(rest) = m.strip_prefix("aod::") {
                        if builtin_ns_exports_enum(rest, segs[1]) {
                            return Some(Type::Enum {
                                module: BUILTIN_MOD.to_string(),
                                name: segs[1].to_string(),
                            });
                        }
                    }
                    if self
                        .program
                        .structs
                        .contains_key(&(m.clone(), segs[1].to_string()))
                    {
                        return Some(Type::Struct {
                            module: m.clone(),
                            name: segs[1].to_string(),
                        });
                    }
                    if self
                        .program
                        .enums
                        .contains_key(&(m.clone(), segs[1].to_string()))
                    {
                        return Some(Type::Enum {
                            module: m.clone(),
                            name: segs[1].to_string(),
                        });
                    }
                }
            } else if segs.len() == 3
                && segs[0] == "aod"
                && builtin_ns_exports_enum(segs[1], segs[2])
            {
                return Some(Type::Enum {
                    module: BUILTIN_MOD.to_string(),
                    name: segs[2].to_string(),
                });
            }
            return None;
        }
        if self
            .program
            .structs
            .contains_key(&(base.to_string(), name.to_string()))
        {
            return Some(Type::Struct {
                module: base.to_string(),
                name: name.to_string(),
            });
        }
        if self
            .program
            .enums
            .contains_key(&(base.to_string(), name.to_string()))
        {
            return Some(Type::Enum {
                module: base.to_string(),
                name: name.to_string(),
            });
        }
        if let Some(ImportedSymbol::ModuleItem { module, item }) =
            self.imports_for(mod_key).get(name)
        {
            if self
                .program
                .structs
                .contains_key(&(module.clone(), item.clone()))
            {
                return Some(Type::Struct {
                    module: module.clone(),
                    name: item.clone(),
                });
            }
            if self
                .program
                .enums
                .contains_key(&(module.clone(), item.clone()))
            {
                return Some(Type::Enum {
                    module: module.clone(),
                    name: item.clone(),
                });
            }
        }
        if builtin_enum_variants(name).is_some() {
            return Some(Type::Enum {
                module: BUILTIN_MOD.to_string(),
                name: name.to_string(),
            });
        }
        None
    }

    fn resolve_enum_variant_decl(
        &self,
        mod_key: &str,
        path: &[String],
    ) -> Option<(String, String, String, i32)> {
        let (emod, ename, vname) = if path.len() == 2 {
            let base = self.base_module_of(mod_key);
            if self
                .program
                .enums
                .contains_key(&(base.to_string(), path[0].clone()))
            {
                (base.to_string(), path[0].clone(), path[1].clone())
            } else if let Some(ImportedSymbol::ModuleItem { module, item }) =
                self.imports_for(mod_key).get(&path[0])
            {
                (module.clone(), item.clone(), path[1].clone())
            } else if builtin_enum_variants(&path[0]).is_some() {
                (BUILTIN_MOD.to_string(), path[0].clone(), path[1].clone())
            } else {
                return None;
            }
        } else if path.len() == 3 {
            if let Some(ImportedSymbol::ModuleNamespace(m)) =
                self.imports_for(mod_key).get(&path[0])
            {
                if let Some(rest) = m.strip_prefix("aod::") {
                    if builtin_ns_exports_enum(rest, &path[1]) {
                        (BUILTIN_MOD.to_string(), path[1].clone(), path[2].clone())
                    } else {
                        return None;
                    }
                } else {
                    (m.clone(), path[1].clone(), path[2].clone())
                }
            } else {
                return None;
            }
        } else if path.len() == 4
            && path[0] == "aod"
            && builtin_ns_exports_enum(&path[1], &path[2])
        {
            (BUILTIN_MOD.to_string(), path[2].clone(), path[3].clone())
        } else {
            return None;
        };
        let ed = self.program.enums.get(&(emod.clone(), ename.clone()))?;
        let tag = ed.variants.iter().position(|v| v.name == vname)?;
        Some((emod, ename, vname, tag as i32))
    }

    fn resolve_struct_decl(&self, mod_key: &str, path: &[String]) -> Option<(String, StructDecl)> {
        if path.len() == 1 {
            let base = self.base_module_of(mod_key);
            if let Some(s) = self
                .program
                .structs
                .get(&(base.to_string(), path[0].clone()))
            {
                return Some((base.to_string(), s.clone()));
            }
            if let Some(ImportedSymbol::ModuleItem { module, item }) =
                self.imports_for(mod_key).get(&path[0])
            {
                if let Some(s) = self.program.structs.get(&(module.clone(), item.clone())) {
                    return Some((module.clone(), s.clone()));
                }
            }
        } else if path.len() == 2 {
            if let Some(ImportedSymbol::ModuleNamespace(m)) =
                self.imports_for(mod_key).get(&path[0])
            {
                if let Some(s) = self.program.structs.get(&(m.clone(), path[1].clone())) {
                    return Some((m.clone(), s.clone()));
                }
            }
        }
        None
    }

    fn infer_eval_pattern_type(
        &self,
        mod_key: &str,
        pat: &Pattern,
        slot_hint: Option<&Type>,
    ) -> Option<Type> {
        match pat {
            Pattern::Wildcard | Pattern::Binding(_) => None,
            Pattern::NullLit => Some(Type::Null),
            Pattern::IntLit(_) => {
                if let Some(h) = slot_hint {
                    if *h.unwrap_read_only() == Type::T1 {
                        return Some(Type::T1);
                    }
                }
                Some(Type::Int)
            }
            Pattern::BoolLit(_) => Some(Type::Bool),
            Pattern::BytesLit(_) => Some(Type::Bytes),
            Pattern::B8Lit(_) => Some(Type::B8),
            Pattern::B64Lit(_) => Some(Type::B64),
            Pattern::Q4Lit(_) => Some(Type::Q4),
            Pattern::TritLit(_) => Some(Type::T1),
            Pattern::RatioLit(_) => Some(Type::Ratio),
            Pattern::Addr8Lit(_) => Some(Type::Addr8),
            Pattern::SupportLit(_) => Some(Type::Support),
            Pattern::EnumVariant { path, .. } => {
                let (emod, ename, _, _) = self.resolve_enum_variant_decl(mod_key, path)?;
                Some(Type::Enum {
                    module: emod,
                    name: ename,
                })
            }
            Pattern::Tuple(elems) => {
                let sub_hints: Option<&[Type]> = match slot_hint.map(Type::unwrap_read_only) {
                    Some(Type::Tuple(ts)) if ts.len() == elems.len() => Some(ts.as_slice()),
                    _ => None,
                };
                let mut ts = Vec::with_capacity(elems.len());
                for (i, e) in elems.iter().enumerate() {
                    let sh = sub_hints.map(|s| &s[i]);
                    ts.push(self.infer_eval_pattern_type(mod_key, e, sh)?);
                }
                Some(Type::Tuple(ts))
            }
        }
    }

    fn match_pattern(
        &self,
        mod_key: &str,
        pat: &Pattern,
        val: &Value,
        bindings: &mut BTreeMap<String, Value>,
    ) -> bool {
        match pat {
            Pattern::Wildcard => true,
            Pattern::Binding(name) => {
                if name != "_" {
                    bindings.insert(name.clone(), val.clone());
                }
                true
            }
            Pattern::NullLit => matches!(val, Value::Null),
            Pattern::IntLit(n) => match val {
                Value::Int(v) => v == n,
                Value::T1(v) => T1::from_bigint(n).ok().as_ref() == Some(v),
                _ => false,
            },
            Pattern::BoolLit(b) => matches!(val, Value::Bool(v) if v == b),
            Pattern::BytesLit(bs) => matches!(val, Value::Bytes(v) if v == bs),
            Pattern::B8Lit(b) => matches!(val, Value::B8(v) if v == b),
            Pattern::B64Lit(b) => matches!(val, Value::B64(v) if v == b),
            Pattern::Q4Lit(q) => matches!(val, Value::Q4(v) if v == q),
            Pattern::TritLit(t) => matches!(val, Value::T1(v) if v == t),
            Pattern::RatioLit(r) => matches!(val, Value::Ratio(v) if v == r),
            Pattern::Addr8Lit(a) => matches!(val, Value::Addr8(v) if v == a),
            Pattern::SupportLit(s) => matches!(val, Value::Support(v) if v == s),
            Pattern::EnumVariant {
                path,
                bindings: sub_pats,
            } => {
                let Some((emod, ename, vname, _)) = self.resolve_enum_variant_decl(mod_key, path)
                else {
                    return false;
                };
                match val {
                    Value::Enum(ev)
                        if ev.module == emod
                            && ev.name == ename
                            && ev.variant == vname
                            && ev.payloads.len() == sub_pats.len() =>
                    {
                        for (sp, pv) in sub_pats.iter().zip(&ev.payloads) {
                            if !self.match_pattern(mod_key, sp, pv, bindings) {
                                return false;
                            }
                        }
                        true
                    }
                    _ => false,
                }
            }
            Pattern::Tuple(sub_pats) => match val {
                Value::Tuple(elems) if elems.len() == sub_pats.len() => {
                    for (sp, ev) in sub_pats.iter().zip(elems) {
                        if !self.match_pattern(mod_key, sp, ev, bindings) {
                            return false;
                        }
                    }
                    true
                }
                _ => false,
            },
        }
    }

    fn user_fn_accepts_arity(&self, mod_key: &str, name: &str, arity: usize) -> bool {
        self.program
            .functions
            .get(&(mod_key.to_string(), name.to_string()))
            .is_some_and(|d| d.arms.iter().any(|a| a.params.len() == arity))
    }

    fn call_user_fn(
        &mut self,
        mod_key: &str,
        name: &str,
        pos_vals: Vec<Value>,
        call_line: usize,
    ) -> DecResult<Value> {
        self.tick()?;
        self.depth += 1;
        if self.depth > self.max_depth {
            self.depth -= 1;
            return Err(Diagnostic::new(
                DiagnosticCode::BudgetExhausted,
                "call depth limit exceeded",
            ));
        }
        let decl = match self
            .program
            .functions
            .get(&(mod_key.to_string(), name.to_string()))
        {
            Some(d) => d.clone(),
            None => {
                self.depth -= 1;
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    call_line,
                    format!("undefined function {mod_key}::{name}"),
                ));
            }
        };

        let res = (|| {
            let arity = pos_vals.len();
            let mut slot_hints: Vec<Option<Type>> = vec![None; arity];
            for arm in &decl.arms {
                if arm.params.len() != arity {
                    continue;
                }
                for (i, p) in arm.params.iter().enumerate() {
                    if slot_hints[i].is_some() {
                        continue;
                    }
                    match p {
                        DecArmParam::TypedVar { ty, .. } | DecArmParam::TypedPattern { ty, .. } => {
                            let resolved = Type::resolve_with(ty, arm.line, &|n| {
                                self.lookup_user_type(mod_key, n)
                            })?;
                            if resolved != Type::Unknown {
                                slot_hints[i] = Some(resolved);
                            }
                        }
                        DecArmParam::Pattern(pat) if !matches!(pat, Pattern::IntLit(_)) => {
                            if let Some(t) = self.infer_eval_pattern_type(mod_key, pat, None) {
                                if t != Type::Unknown {
                                    slot_hints[i] = Some(t);
                                }
                            }
                        }
                        DecArmParam::Pattern(_) => {}
                    }
                }
            }
            for arm in &decl.arms {
                if arm.params.len() != arity {
                    continue;
                }
                for (i, p) in arm.params.iter().enumerate() {
                    if slot_hints[i].is_none() {
                        if let DecArmParam::Pattern(Pattern::IntLit(_)) = p {
                            slot_hints[i] = Some(Type::Int);
                        }
                    }
                }
            }

            let mut matched_arm = None;
            let mut matched_scope = BTreeMap::new();

            for arm in &decl.arms {
                if arm.params.len() != pos_vals.len() {
                    continue;
                }
                let mut scope = BTreeMap::new();
                let mut all_match = true;
                for (idx, (param, val)) in arm.params.iter().zip(&pos_vals).enumerate() {
                    match param {
                        DecArmParam::TypedVar { ty, name: pname } => {
                            let expected = Type::resolve_with(ty, arm.line, &|n| {
                                self.lookup_user_type(mod_key, n)
                            })?;
                            if !runtime_val_matches_type(val, &expected) {
                                all_match = false;
                                break;
                            }
                            if pname != "_" {
                                scope.insert(pname.clone(), val.clone());
                            }
                        }
                        DecArmParam::TypedPattern { ty, pattern } => {
                            let expected = Type::resolve_with(ty, arm.line, &|n| {
                                self.lookup_user_type(mod_key, n)
                            })?;
                            if !runtime_val_matches_type(val, &expected)
                                || !self.match_pattern(mod_key, pattern, val, &mut scope)
                            {
                                all_match = false;
                                break;
                            }
                        }
                        DecArmParam::Pattern(pattern) => {
                            if let Some(expected) = self.infer_eval_pattern_type(
                                mod_key,
                                pattern,
                                slot_hints[idx].as_ref(),
                            ) {
                                if !runtime_val_matches_type(val, &expected) {
                                    all_match = false;
                                    break;
                                }
                            }
                            if !self.match_pattern(mod_key, pattern, val, &mut scope) {
                                all_match = false;
                                break;
                            }
                        }
                    }
                }
                if !all_match {
                    continue;
                }
                if let Some(ref guard) = arm.guard {
                    let g_scopes = vec![scope.clone()];
                    let gv = self.eval_expr(guard, mod_key, &g_scopes)?;
                    if !matches!(gv, Value::Bool(true)) {
                        continue;
                    }
                }
                matched_arm = Some(arm.clone());
                matched_scope = scope;
                break;
            }

            if let Some(arm) = matched_arm {
                let mut scopes = vec![matched_scope];
                return match self.eval_block(&arm.body, mod_key, &mut scopes)? {
                    ControlFlow::Return(v) => Ok(v),
                    ControlFlow::Normal => Ok(Value::Unit),
                };
            }

            // Rank-lifted user scalar function fallback!
            if pos_vals.len() == 1 {
                match &pos_vals[0] {
                    Value::Array(items) => {
                        let mut out = Vec::with_capacity(items.len());
                        for it in items {
                            out.push(self.call_user_fn(mod_key, name, vec![it.clone()], call_line)?);
                        }
                        return Ok(Value::Array(out));
                    }
                    Value::List(items) => {
                        let mut out = Vec::with_capacity(items.len());
                        for it in items {
                            out.push(self.call_user_fn(mod_key, name, vec![it.clone()], call_line)?);
                        }
                        return Ok(Value::List(out));
                    }
                    Value::Range(a, b) => {
                        let mut out = Vec::new();
                        for k in *a..*b {
                            out.push(self.call_user_fn(
                                mod_key,
                                name,
                                vec![Value::Int(BigInt::from_i64(k))],
                                call_line,
                            )?);
                        }
                        return Ok(Value::List(out));
                    }
                    _ => {}
                }
            } else if pos_vals.len() == 2
                && (val_seq_rank(&pos_vals[0]) > 0 || val_seq_rank(&pos_vals[1]) > 0)
            {
                let target = EvalTarget::UserFn {
                    mod_key: mod_key.to_string(),
                    name: name.to_string(),
                };
                return self.eval_rank_lifted_binary_target(
                    &target,
                    pos_vals[0].clone(),
                    pos_vals[1].clone(),
                    call_line,
                );
            }

            Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                call_line,
                format!("no matching arm for call to {name}"),
            ))
        })();

        self.depth -= 1;
        res
    }

    fn eval_block(
        &mut self,
        stmts: &[Stmt],
        mod_key: &str,
        scopes: &mut Vec<BTreeMap<String, Value>>,
    ) -> DecResult<ControlFlow> {
        for stmt in stmts {
            self.tick()?;
            match &stmt.kind {
                StmtKind::Let { pattern, value, .. } => {
                    let v = self.eval_expr(value, mod_key, scopes)?;
                    match pattern {
                        LetPattern::Single(name) => {
                            if name != "_" {
                                scopes.last_mut().unwrap().insert(name.clone(), v);
                            }
                        }
                        LetPattern::Tuple(elems) => match v {
                            Value::Tuple(vals) if vals.len() == elems.len() => {
                                let cur = scopes.last_mut().unwrap();
                                for ((n, _), ev) in elems.iter().zip(vals) {
                                    if n != "_" {
                                        cur.insert(n.clone(), ev);
                                    }
                                }
                            }
                            _ => {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::RuntimeError,
                                    stmt.line,
                                    "let tuple destructuring mismatch at runtime",
                                ));
                            }
                        },
                    }
                }
                StmtKind::Assert(expr) => {
                    let v = self.eval_expr(expr, mod_key, scopes)?;
                    let ok = match v {
                        Value::Bool(b) | Value::QExtBool(b) => b,
                        _ => false,
                    };
                    if !ok {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::AssertionFailed,
                            stmt.line,
                            "assertion failed",
                        ));
                    }
                }
                StmtKind::Emit(expr) => {
                    let v = self.eval_expr(expr, mod_key, scopes)?;
                    self.emits.push(v.render());
                }
                StmtKind::Return(opt) => {
                    let v = match opt {
                        Some(e) => self.eval_expr(e, mod_key, scopes)?,
                        None => Value::Unit,
                    };
                    return Ok(ControlFlow::Return(v));
                }
                StmtKind::Pass => {}
                StmtKind::QuarantineBlock { body } => {
                    scopes.push(BTreeMap::new());
                    let cf = self.eval_block(body, mod_key, scopes)?;
                    scopes.pop();
                    if !matches!(cf, ControlFlow::Normal) {
                        return Ok(cf);
                    }
                }
                StmtKind::ExprStmt(expr) => {
                    let _ = self.eval_expr(expr, mod_key, scopes)?;
                }
            }
        }
        Ok(ControlFlow::Normal)
    }

    fn eval_expr(
        &mut self,
        expr: &Expr,
        mod_key: &str,
        scopes: &[BTreeMap<String, Value>],
    ) -> DecResult<Value> {
        self.tick()?;
        match &expr.kind {
            ExprKind::NullLit => Ok(Value::Null),
            ExprKind::IntLit(n) => Ok(Value::Int(n.clone())),
            ExprKind::BoolLit(b) => Ok(Value::Bool(*b)),
            ExprKind::BytesLit(bs) => Ok(Value::Bytes(bs.clone())),
            ExprKind::RatioLit(r) => Ok(Value::Ratio(r.clone())),
            ExprKind::Addr8Lit(a) => Ok(Value::Addr8(a.clone())),
            ExprKind::SupportLit(s) => Ok(Value::Support(s.clone())),
            ExprKind::TritLit(tl) => Ok(Value::T1(tl.to_t1())),
            ExprKind::Ident(name) => {
                for scope in scopes.iter().rev() {
                    if let Some(v) = scope.get(name) {
                        return Ok(v.clone());
                    }
                }
                if let Some(v) = self
                    .program
                    .consts
                    .get(&(mod_key.to_string(), name.clone()))
                {
                    return Ok(v.clone());
                }
                let base = self.base_module_of(mod_key).to_string();
                if base != mod_key {
                    if let Some(v) = self.program.consts.get(&(base, name.clone())) {
                        return Ok(v.clone());
                    }
                }
                if let Some(ImportedSymbol::ModuleItem { module, item }) =
                    self.imports_for(mod_key).get(name).cloned()
                {
                    if let Some(v) = self.program.consts.get(&(module, item)) {
                        return Ok(v.clone());
                    }
                }
                Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    expr.line,
                    format!("undefined variable {name}"),
                ))
            }
            ExprKind::Path(segs) => {
                if let Some((emod, ename, vname, tag)) =
                    self.resolve_enum_variant_decl(mod_key, segs)
                {
                    return Ok(Value::Enum(Box::new(EnumValue {
                        module: emod,
                        name: ename,
                        variant: vname,
                        tag,
                        payloads: Vec::new(),
                    })));
                }
                if segs.len() == 2 {
                    if let Some(qsub) = self.resolve_qext_submodule(mod_key, &segs[0]) {
                        if let Some(v) = self.program.consts.get(&(qsub, segs[1].clone())) {
                            return Ok(v.clone());
                        }
                    }
                    if let Some(ImportedSymbol::ModuleNamespace(target_mod)) =
                        self.imports_for(mod_key).get(&segs[0]).cloned()
                    {
                        if let Some(v) = self.program.consts.get(&(target_mod, segs[1].clone())) {
                            return Ok(v.clone());
                        }
                    }
                } else if segs.len() == 3 {
                    if let Some(qsub) =
                        self.resolve_qext_submodule_3seg(mod_key, &segs[0], &segs[1])
                    {
                        if let Some(v) = self.program.consts.get(&(qsub, segs[2].clone())) {
                            return Ok(v.clone());
                        }
                    }
                }
                Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    expr.line,
                    format!("undefined path {}", segs.join("::")),
                ))
            }
            ExprKind::MapLit(entries) => {
                let mut m = BTreeMap::new();
                for (ke, ve) in entries {
                    let kv = self.eval_expr(ke, mod_key, scopes)?;
                    let vv = self.eval_expr(ve, mod_key, scopes)?;
                    let key = MapKey::from_value(&kv, ke.line)?;
                    m.insert(key, vv);
                }
                Ok(Value::Map(Box::new(m)))
            }
            ExprKind::SeqLit(items) => {
                if items.is_empty() {
                    return Ok(Value::List(Vec::new()));
                }
                let mut vals = Vec::with_capacity(items.len());
                for it in items {
                    vals.push(self.eval_expr(it, mod_key, scopes)?);
                }
                Ok(Value::Array(vals))
            }
            ExprKind::Tuple(elems) => {
                if elems.is_empty() {
                    return Ok(Value::Unit);
                }
                let mut vals = Vec::with_capacity(elems.len());
                for e in elems {
                    vals.push(self.eval_expr(e, mod_key, scopes)?);
                }
                Ok(Value::Tuple(vals))
            }
            ExprKind::WindowLit { start, end } => {
                let u = extract_i64(self.eval_expr(start, mod_key, scopes)?, expr.line)?;
                let v = extract_i64(self.eval_expr(end, mod_key, scopes)?, expr.line)?;
                if u >= v {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        expr.line,
                        format!("window ({u}, {v}] requires u < v"),
                    ));
                }
                Ok(Value::Window(u, v))
            }
            ExprKind::Range { start, end } => {
                let a = extract_i64(self.eval_expr(start, mod_key, scopes)?, expr.line)?;
                let b = extract_i64(self.eval_expr(end, mod_key, scopes)?, expr.line)?;
                Ok(Value::Range(a, b))
            }
            ExprKind::BoxExpr(dims) => {
                let mut ranges = Vec::with_capacity(3);
                for d in dims {
                    match self.eval_expr(d, mod_key, scopes)? {
                        Value::Range(a, b) => ranges.push((a, b)),
                        _ => {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::RuntimeError,
                                expr.line,
                                "box expects 3 range axes",
                            ));
                        }
                    }
                }
                Ok(Value::Box3(BoxRegion {
                    x: ranges[0],
                    y: ranges[1],
                    z: ranges[2],
                }))
            }
            ExprKind::Simulate {
                prepare,
                ticks,
                observers,
            } => {
                let n = extract_i64(self.eval_expr(ticks, mod_key, scopes)?, expr.line)?;
                if n < 0 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        expr.line,
                        "simulate tick count must be non-negative",
                    ));
                }
                self.run_simulation(mod_key, prepare, n as u64, observers, expr.line)
            }
            ExprKind::Unary { op, operand } => {
                let val = self.eval_expr(operand, mod_key, scopes)?;
                eval_unary(*op, val, expr.line)
            }
            ExprKind::Adverb { op, operand } => {
                let val = self.eval_expr(operand, mod_key, scopes)?;
                eval_adverb(*op, val, expr.line)
            }
            ExprKind::Binary { op, left, right } => {
                // Short-circuit scalar boolean `and` / `or`
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    let lv = self.eval_expr(left, mod_key, scopes)?;
                    if *op == BinaryOp::And {
                        match &lv {
                            Value::Bool(false) => return Ok(Value::Bool(false)),
                            Value::Bool(true) => return self.eval_expr(right, mod_key, scopes),
                            Value::QExtBool(false) => return Ok(Value::QExtBool(false)),
                            Value::QExtBool(true) => return self.eval_expr(right, mod_key, scopes),
                            _ => {}
                        }
                    } else {
                        match &lv {
                            Value::Bool(true) => return Ok(Value::Bool(true)),
                            Value::Bool(false) => return self.eval_expr(right, mod_key, scopes),
                            Value::QExtBool(true) => return Ok(Value::QExtBool(true)),
                            Value::QExtBool(false) => return self.eval_expr(right, mod_key, scopes),
                            _ => {}
                        }
                    }
                    let rv = self.eval_expr(right, mod_key, scopes)?;
                    return eval_binary(*op, lv, rv, expr.line);
                }
                let lv = self.eval_expr(left, mod_key, scopes)?;
                let rv = self.eval_expr(right, mod_key, scopes)?;
                eval_binary(*op, lv, rv, expr.line)
            }
            ExprKind::Pipe { input, stage } => {
                let in_val = self.eval_expr(input, mod_key, scopes)?;
                self.eval_pipe_stage(in_val, stage, mod_key, scopes, expr.line)
            }
            ExprKind::Match { scrutinee, arms } => {
                let sval = self.eval_expr(scrutinee, mod_key, scopes)?;
                for arm in arms {
                    let mut arm_scope = BTreeMap::new();
                    if !self.match_pattern(mod_key, &arm.pattern, &sval, &mut arm_scope) {
                        continue;
                    }
                    let mut arm_scopes = scopes.to_vec();
                    arm_scopes.push(arm_scope);
                    if let Some(ref g) = arm.guard {
                        let gv = self.eval_expr(g, mod_key, &arm_scopes)?;
                        if !matches!(gv, Value::Bool(true)) {
                            continue;
                        }
                    }
                    return self.eval_expr(&arm.body, mod_key, &arm_scopes);
                }
                Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    expr.line,
                    "non-exhaustive match at runtime",
                ))
            }
            ExprKind::Field { base, name } => {
                let bv = self.eval_expr(base, mod_key, scopes)?;
                eval_field(bv, name, expr.line)
            }
            ExprKind::Index { base, index } => {
                let bv = self.eval_expr(base, mod_key, scopes)?;
                let iv = self.eval_expr(index, mod_key, scopes)?;
                match bv {
                    Value::Array(items) | Value::List(items) => {
                        let idx = extract_i64(iv, expr.line)?;
                        if idx < 0 || (idx as usize) >= items.len() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::RuntimeError,
                                expr.line,
                                format!("sequence index {idx} out of bounds"),
                            ));
                        }
                        Ok(items[idx as usize].clone())
                    }
                    Value::Bytes(bs) => {
                        let idx = extract_i64(iv, expr.line)?;
                        if idx < 0 || (idx as usize) >= bs.len() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::RuntimeError,
                                expr.line,
                                format!("bytes index {idx} out of bounds"),
                            ));
                        }
                        Ok(Value::B8(B8(bs[idx as usize])))
                    }
                    Value::Map(m) => {
                        let k = MapKey::from_value(&iv, expr.line)?;
                        m.get(&k).cloned().ok_or_else(|| {
                            Diagnostic::at_line(
                                DiagnosticCode::RuntimeError,
                                expr.line,
                                format!("map key {} not found", iv.render()),
                            )
                        })
                    }
                    _ => Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        expr.line,
                        "indexing requires sequence, bytes, or map",
                    )),
                }
            }
            ExprKind::Call { callee, args } => {
                self.eval_call_with_prepended(None, callee, args, mod_key, scopes, expr.line)
            }
        }
    }

    fn eval_pipe_stage(
        &mut self,
        in_val: Value,
        stage: &PipeStage,
        mod_key: &str,
        scopes: &[BTreeMap<String, Value>],
        line: usize,
    ) -> DecResult<Value> {
        match stage {
            PipeStage::Adverb(op) => eval_adverb(*op, in_val, line),
            PipeStage::Boundary => eval_unary(UnaryOp::Boundary, in_val, line),
            PipeStage::Section { op, rhs } => {
                let rv = self.eval_expr(rhs, mod_key, scopes)?;
                let lv = match in_val {
                    Value::Range(a, b) => materialize_range_list(a, b, line)?,
                    other => other,
                };
                eval_binary(*op, lv, rv, line)
            }
            PipeStage::Call { callee, extra_args } => {
                self.eval_call_with_prepended(Some(in_val), callee, extra_args, mod_key, scopes, line)
            }
        }
    }

    fn resolve_core_op_name_eval(&self, callee: &Expr, mod_key: &str) -> Option<String> {
        let base_mod = self.base_module_of(mod_key).to_string();
        match &callee.kind {
            ExprKind::Ident(n) => {
                if self
                    .program
                    .functions
                    .contains_key(&(mod_key.to_string(), n.clone()))
                    || self.program.functions.contains_key(&(base_mod, n.clone()))
                {
                    return None;
                }
                if let Some(sym) = self.imports_for(mod_key).get(n) {
                    return match sym {
                        ImportedSymbol::Builtin(c) => Some(c.clone()),
                        _ => None,
                    };
                }
                Some(n.clone())
            }
            ExprKind::Path(segs) if segs.len() == 3 && segs[0] == "aod" && segs[1] == "core" => {
                Some(segs[2].clone())
            }
            ExprKind::Path(segs) if segs.len() == 2 => {
                if let Some(ImportedSymbol::ModuleNamespace(target_mod)) =
                    self.imports_for(mod_key).get(&segs[0])
                {
                    if target_mod == "aod::core" {
                        return Some(segs[1].clone());
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn invoke_ho_with_env(
        &mut self,
        target: &EvalTarget,
        mut call_args: Vec<Value>,
        env_val: Option<&Value>,
        line: usize,
    ) -> DecResult<Value> {
        if let Some(ev) = env_val {
            call_args.push(ev.clone());
        }
        self.invoke_eval_target(target, call_args, line)
    }

    fn eval_sum_or_dot(
        &self,
        op: &str,
        callee: &Expr,
        raw_args: &[Expr],
        vals: Vec<Value>,
        mod_key: &str,
        line: usize,
    ) -> DecResult<Value> {
        let inferred_ty = self
            .program
            .checked
            .poly_call_types
            .get(&(
                mod_key.to_string(),
                line,
                format!("{op}:{callee:?}:{raw_args:?}"),
            ))
            .cloned();
        let zero_for = |ty_opt: Option<Type>, sample: Option<&Value>| -> Value {
            match sample.map(Value::type_of).or(ty_opt) {
                Some(Type::B8) => Value::B8(B8(0)),
                Some(Type::T1) => Value::T1(T1::ZERO),
                Some(Type::Ratio) => Value::Ratio(Ratio::zero()),
                Some(t) if t == Type::QExt(Box::new(Type::Ratio)) => {
                    Value::QExtRatio(QExtRatio::from_native_frozen(Ratio::zero()))
                }
                _ => Value::Int(BigInt::zero()),
            }
        };
        let err = |e: aod_core::error::CoreError| {
            Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())
        };
        match op {
            "sum" => {
                let items = eval_afc_seq_values(vals.into_iter().next().unwrap(), "sum", line)?;
                let mut acc = zero_for(inferred_ty, items.first());
                for item in items {
                    acc = match (acc, item) {
                        (Value::Int(a), Value::Int(b)) => Value::Int(&a + &b),
                        (Value::B8(a), Value::B8(b)) => {
                            let s = a.value().checked_add(b.value()).ok_or_else(|| {
                                Diagnostic::at_line(
                                    DiagnosticCode::RuntimeError,
                                    line,
                                    "sum(b8): b8 addition overflow outside 0..=255",
                                )
                            })?;
                            Value::B8(B8(s))
                        }
                        (Value::T1(a), Value::T1(b)) => {
                            Value::T1(T1::new(a.value() + b.value()).map_err(err)?)
                        }
                        (Value::Ratio(a), Value::Ratio(b)) => Value::Ratio(&a + &b),
                        (Value::QExtRatio(a), Value::QExtRatio(b)) => {
                            Value::QExtRatio(a.checked_add(&b))
                        }
                        _ => {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::RuntimeError,
                                line,
                                "sum element type mismatch",
                            ));
                        }
                    };
                }
                Ok(acc)
            }
            "dot" => {
                let mut it = vals.into_iter();
                let xs = eval_afc_seq_values(it.next().unwrap(), "dot", line)?;
                let ys = eval_afc_seq_values(it.next().unwrap(), "dot", line)?;
                if xs.len() != ys.len() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!(
                            "dot(a, b) sequence length mismatch: {} != {}",
                            xs.len(),
                            ys.len()
                        ),
                    ));
                }
                let mut acc = zero_for(inferred_ty, xs.first().or_else(|| ys.first()));
                for (x, y) in xs.into_iter().zip(ys.into_iter()) {
                    acc = match (acc, x, y) {
                        (Value::Int(s), Value::Int(a), Value::Int(b)) => {
                            Value::Int(&s + &(&a * &b))
                        }
                        (Value::B8(s), Value::B8(a), Value::B8(b)) => {
                            let p = a.value().checked_mul(b.value()).ok_or_else(|| {
                                Diagnostic::at_line(
                                    DiagnosticCode::RuntimeError,
                                    line,
                                    "dot(b8): b8 multiplication overflow outside 0..=255",
                                )
                            })?;
                            let sum_u8 = s.value().checked_add(p).ok_or_else(|| {
                                Diagnostic::at_line(
                                    DiagnosticCode::RuntimeError,
                                    line,
                                    "dot(b8): b8 addition overflow outside 0..=255",
                                )
                            })?;
                            Value::B8(B8(sum_u8))
                        }
                        (Value::T1(s), Value::T1(a), Value::T1(b)) => {
                            let p = a.value() * b.value();
                            Value::T1(T1::new(s.value() + p).map_err(err)?)
                        }
                        (Value::Ratio(s), Value::Ratio(a), Value::Ratio(b)) => {
                            Value::Ratio(&s + &(&a * &b))
                        }
                        (Value::QExtRatio(s), Value::QExtRatio(a), Value::QExtRatio(b)) => {
                            Value::QExtRatio(s.checked_add(&a.checked_mul(&b)))
                        }
                        _ => {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::RuntimeError,
                                line,
                                "dot element type mismatch",
                            ));
                        }
                    };
                }
                Ok(acc)
            }
            _ => unreachable!(),
        }
    }

    fn eval_call_with_prepended(
        &mut self,
        prepended: Option<Value>,
        callee: &Expr,
        args: &[Expr],
        mod_key: &str,
        scopes: &[BTreeMap<String, Value>],
        line: usize,
    ) -> DecResult<Value> {
        let core_op = self.resolve_core_op_name_eval(callee, mod_key);
        let simple_name = core_op.as_deref();
        let total_arity = usize::from(prepended.is_some()) + args.len();

        // 1. Higher-order pure combinators & AFC scope operators
        if let Some(cname) = simple_name {
            match (cname, total_arity) {
                ("walk", 3) | ("settle", 3) => {
                    let is_settle = cname == "settle";
                    let (mut cur, count_expr, fn_expr) = if let Some(in_v) = prepended {
                        (in_v, &args[0], &args[1])
                    } else {
                        let s0 = self.eval_expr(&args[0], mod_key, scopes)?;
                        (s0, &args[1], &args[2])
                    };
                    let n = extract_i64(self.eval_expr(count_expr, mod_key, scopes)?, line)?;
                    if n < 0 {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::RuntimeError,
                            line,
                            format!("{cname} count/fuel must be >= 0, got {n}"),
                        ));
                    }
                    let fn_target = self.resolve_eval_target(fn_expr, mod_key, line)?;
                    let takes_two = match &fn_target {
                        EvalTarget::UserFn {
                            mod_key: fmod,
                            name: fname,
                        } => self.user_fn_accepts_arity(fmod, fname, 2),
                        EvalTarget::Builtin(_) => true,
                    };
                    if !is_settle {
                        for step_i in 0..n {
                            self.tick()?;
                            let call_args = if takes_two {
                                vec![cur, Value::Int(BigInt::from_i64(step_i))]
                            } else {
                                vec![cur]
                            };
                            cur = self.invoke_eval_target(&fn_target, call_args, line)?;
                        }
                        return Ok(cur);
                    } else {
                        for step_i in 0..n {
                            self.tick()?;
                            let call_args = if takes_two {
                                vec![cur.clone(), Value::Int(BigInt::from_i64(step_i))]
                            } else {
                                vec![cur.clone()]
                            };
                            let next = self.invoke_eval_target(&fn_target, call_args, line)?;
                            if values_equal(&next, &cur) {
                                return Ok(next);
                            }
                            cur = next;
                        }
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::BudgetExhausted,
                            line,
                            format!("settle did not converge within fuel={n}"),
                        ));
                    }
                }
                ("fold", 3) => {
                    let (seq_val, init_expr, fn_expr) = if let Some(in_v) = prepended {
                        (in_v, &args[0], &args[1])
                    } else {
                        let sv = self.eval_expr(&args[0], mod_key, scopes)?;
                        (sv, &args[1], &args[2])
                    };
                    let (items, _) = extract_seq_items(seq_val, "fold", line)?;
                    let mut acc = self.eval_expr(init_expr, mod_key, scopes)?;
                    let fn_target = self.resolve_eval_target(fn_expr, mod_key, line)?;
                    for item in items {
                        self.tick()?;
                        acc = self.invoke_eval_target(&fn_target, vec![acc, item], line)?;
                    }
                    return Ok(acc);
                }
                ("each", 2) => {
                    let (seq_val, fn_expr) = if let Some(in_v) = prepended {
                        (in_v, &args[0])
                    } else {
                        let sv = self.eval_expr(&args[0], mod_key, scopes)?;
                        (sv, &args[1])
                    };
                    let (items, is_fixed_array) = extract_seq_items(seq_val, "each", line)?;
                    let fn_target = self.resolve_eval_target(fn_expr, mod_key, line)?;
                    let mut out = Vec::with_capacity(items.len());
                    for item in items {
                        self.tick()?;
                        out.push(self.invoke_eval_target(&fn_target, vec![item], line)?);
                    }
                    return Ok(if is_fixed_array {
                        Value::Array(out)
                    } else {
                        Value::List(out)
                    });
                }
                ("evolve", 3 | 4) => {
                    let (mut state, dom_expr, fn_expr, env_expr) = if let Some(in_v) = prepended {
                        (in_v, &args[0], &args[1], args.get(2))
                    } else {
                        let s0 = self.eval_expr(&args[0], mod_key, scopes)?;
                        (s0, &args[1], &args[2], args.get(3))
                    };
                    let dom_val = self.eval_expr(dom_expr, mod_key, scopes)?;
                    let target = self.resolve_eval_target(fn_expr, mod_key, line)?;
                    let env_val = match env_expr {
                        Some(ee) => Some(self.eval_expr(ee, mod_key, scopes)?),
                        None => None,
                    };
                    match dom_val {
                        Value::Range(a, b) => {
                            let mut k = a;
                            while k < b {
                                self.tick()?;
                                state = self.invoke_ho_with_env(
                                    &target,
                                    vec![state, Value::Int(BigInt::from_i64(k))],
                                    env_val.as_ref(),
                                    line,
                                )?;
                                k += 1;
                            }
                            return Ok(state);
                        }
                        Value::Window(u, v) => {
                            let mut prev = u;
                            while prev < v {
                                self.tick()?;
                                let k = prev + 1;
                                state = self.invoke_ho_with_env(
                                    &target,
                                    vec![state, Value::Int(BigInt::from_i64(k))],
                                    env_val.as_ref(),
                                    line,
                                )?;
                                prev = k;
                            }
                            return Ok(state);
                        }
                        other => {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::RuntimeError,
                                line,
                                format!(
                                    "evolve domain must be range or window, got {}",
                                    other.type_of()
                                ),
                            ));
                        }
                    }
                }
                ("scan", 3 | 4) => {
                    let (mut state, src_expr, fn_expr, env_expr) = if let Some(in_v) = prepended {
                        (in_v, &args[0], &args[1], args.get(2))
                    } else {
                        let s0 = self.eval_expr(&args[0], mod_key, scopes)?;
                        (s0, &args[1], &args[2], args.get(3))
                    };
                    let src_val = self.eval_expr(src_expr, mod_key, scopes)?;
                    let target = self.resolve_eval_target(fn_expr, mod_key, line)?;
                    let env_val = match env_expr {
                        Some(ee) => Some(self.eval_expr(ee, mod_key, scopes)?),
                        None => None,
                    };
                    match src_val {
                        Value::Range(a, b) => {
                            let cap = if b > a {
                                ((b as i128 - a as i128).min(65_536)) as usize
                            } else {
                                0
                            };
                            let mut out = Vec::with_capacity(cap);
                            let mut k = a;
                            while k < b {
                                self.tick()?;
                                state = self.invoke_ho_with_env(
                                    &target,
                                    vec![state, Value::Int(BigInt::from_i64(k))],
                                    env_val.as_ref(),
                                    line,
                                )?;
                                out.push(state.clone());
                                k += 1;
                            }
                            return Ok(Value::List(out));
                        }
                        Value::Window(u, v) => {
                            let cap = if v > u {
                                ((v as i128 - u as i128).min(65_536)) as usize
                            } else {
                                0
                            };
                            let mut out = Vec::with_capacity(cap);
                            let mut prev = u;
                            while prev < v {
                                self.tick()?;
                                let k = prev + 1;
                                state = self.invoke_ho_with_env(
                                    &target,
                                    vec![state, Value::Int(BigInt::from_i64(k))],
                                    env_val.as_ref(),
                                    line,
                                )?;
                                out.push(state.clone());
                                prev = k;
                            }
                            return Ok(Value::List(out));
                        }
                        other => {
                            let items = eval_afc_seq_values(other, "scan", line)?;
                            let mut out = Vec::with_capacity(items.len().min(65_536));
                            for x in items {
                                self.tick()?;
                                state = self.invoke_ho_with_env(
                                    &target,
                                    vec![state, x],
                                    env_val.as_ref(),
                                    line,
                                )?;
                                out.push(state.clone());
                            }
                            return Ok(Value::List(out));
                        }
                    }
                }
                ("fold_seq", 3 | 4) => {
                    let (mut state, seq_expr, fn_expr, env_expr) = if let Some(in_v) = prepended {
                        (in_v, &args[0], &args[1], args.get(2))
                    } else {
                        let s0 = self.eval_expr(&args[0], mod_key, scopes)?;
                        (s0, &args[1], &args[2], args.get(3))
                    };
                    let seq_val = self.eval_expr(seq_expr, mod_key, scopes)?;
                    let target = self.resolve_eval_target(fn_expr, mod_key, line)?;
                    let env_val = match env_expr {
                        Some(ee) => Some(self.eval_expr(ee, mod_key, scopes)?),
                        None => None,
                    };
                    let items = eval_afc_seq_values(seq_val, "fold_seq", line)?;
                    for x in items {
                        self.tick()?;
                        state =
                            self.invoke_ho_with_env(&target, vec![state, x], env_val.as_ref(), line)?;
                    }
                    return Ok(state);
                }
                ("map_seq", 2 | 3) => {
                    let (seq_val, fn_expr, env_expr) = if let Some(in_v) = prepended {
                        (in_v, &args[0], args.get(1))
                    } else {
                        let sv = self.eval_expr(&args[0], mod_key, scopes)?;
                        (sv, &args[1], args.get(2))
                    };
                    let target = self.resolve_eval_target(fn_expr, mod_key, line)?;
                    let env_val = match env_expr {
                        Some(ee) => Some(self.eval_expr(ee, mod_key, scopes)?),
                        None => None,
                    };
                    let items = eval_afc_seq_values(seq_val, "map_seq", line)?;
                    let mut out = Vec::with_capacity(items.len().min(65_536));
                    for x in items {
                        self.tick()?;
                        out.push(self.invoke_ho_with_env(
                            &target,
                            vec![x],
                            env_val.as_ref(),
                            line,
                        )?);
                    }
                    return Ok(Value::List(out));
                }
                ("filter_seq", 2 | 3) => {
                    let (seq_val, fn_expr, env_expr) = if let Some(in_v) = prepended {
                        (in_v, &args[0], args.get(1))
                    } else {
                        let sv = self.eval_expr(&args[0], mod_key, scopes)?;
                        (sv, &args[1], args.get(2))
                    };
                    let target = self.resolve_eval_target(fn_expr, mod_key, line)?;
                    let env_val = match env_expr {
                        Some(ee) => Some(self.eval_expr(ee, mod_key, scopes)?),
                        None => None,
                    };
                    let items = eval_afc_seq_values(seq_val, "filter_seq", line)?;
                    let mut out = Vec::new();
                    for x in items {
                        self.tick()?;
                        let keep = self.invoke_ho_with_env(
                            &target,
                            vec![x.clone()],
                            env_val.as_ref(),
                            line,
                        )?;
                        if matches!(keep, Value::Bool(true)) {
                            out.push(x);
                        }
                    }
                    return Ok(Value::List(out));
                }
                ("tabulate", 2 | 3) => {
                    let (dom_val, fn_expr, env_expr) = if let Some(in_v) = prepended {
                        (in_v, &args[0], args.get(1))
                    } else {
                        let dv = self.eval_expr(&args[0], mod_key, scopes)?;
                        (dv, &args[1], args.get(2))
                    };
                    let target = self.resolve_eval_target(fn_expr, mod_key, line)?;
                    let env_val = match env_expr {
                        Some(ee) => Some(self.eval_expr(ee, mod_key, scopes)?),
                        None => None,
                    };
                    match dom_val {
                        Value::Range(a, b) => {
                            let cap = if b > a {
                                ((b as i128 - a as i128).min(65_536)) as usize
                            } else {
                                0
                            };
                            let mut out = Vec::with_capacity(cap);
                            let mut k = a;
                            while k < b {
                                self.tick()?;
                                out.push(self.invoke_ho_with_env(
                                    &target,
                                    vec![Value::Int(BigInt::from_i64(k))],
                                    env_val.as_ref(),
                                    line,
                                )?);
                                k += 1;
                            }
                            return Ok(Value::List(out));
                        }
                        Value::Window(u, v) => {
                            let cap = if v > u {
                                ((v as i128 - u as i128).min(65_536)) as usize
                            } else {
                                0
                            };
                            let mut out = Vec::with_capacity(cap);
                            let mut prev = u;
                            while prev < v {
                                self.tick()?;
                                let k = prev + 1;
                                out.push(self.invoke_ho_with_env(
                                    &target,
                                    vec![Value::Int(BigInt::from_i64(k))],
                                    env_val.as_ref(),
                                    line,
                                )?);
                                prev = k;
                            }
                            return Ok(Value::List(out));
                        }
                        other => {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::RuntimeError,
                                line,
                                format!(
                                    "tabulate domain must be range or window, got {}",
                                    other.type_of()
                                ),
                            ));
                        }
                    }
                }
                ("count", 2 | 3) => {
                    let (seq_val, fn_expr, env_expr) = if let Some(in_v) = prepended {
                        (in_v, &args[0], args.get(1))
                    } else {
                        let sv = self.eval_expr(&args[0], mod_key, scopes)?;
                        (sv, &args[1], args.get(2))
                    };
                    let target = self.resolve_eval_target(fn_expr, mod_key, line)?;
                    let env_val = match env_expr {
                        Some(ee) => Some(self.eval_expr(ee, mod_key, scopes)?),
                        None => None,
                    };
                    let items = eval_afc_seq_values(seq_val, "count", line)?;
                    let mut cnt: u64 = 0;
                    for x in items {
                        self.tick()?;
                        let pv = self.invoke_ho_with_env(
                            &target,
                            vec![x],
                            env_val.as_ref(),
                            line,
                        )?;
                        if matches!(pv, Value::Bool(true)) {
                            cnt += 1;
                        }
                    }
                    return Ok(Value::Int(BigInt::from_u64(cnt)));
                }
                (op @ ("any" | "all"), 2 | 3) => {
                    let is_any = op == "any";
                    let is_qext_bool = self
                        .program
                        .checked
                        .poly_call_types
                        .get(&(
                            mod_key.to_string(),
                            line,
                            format!("{op}:{callee:?}:{args:?}"),
                        ))
                        .is_some_and(|t| *t == Type::QExt(Box::new(Type::Bool)));
                    let (seq_val, fn_expr, env_expr) = if let Some(in_v) = prepended {
                        (in_v, &args[0], args.get(1))
                    } else {
                        let sv = self.eval_expr(&args[0], mod_key, scopes)?;
                        (sv, &args[1], args.get(2))
                    };
                    let target = self.resolve_eval_target(fn_expr, mod_key, line)?;
                    let env_val = match env_expr {
                        Some(ee) => Some(self.eval_expr(ee, mod_key, scopes)?),
                        None => None,
                    };
                    let items = eval_afc_seq_values(seq_val, op, line)?;
                    let mut saw_qext = is_qext_bool;
                    for x in items {
                        self.tick()?;
                        let pv = self.invoke_ho_with_env(
                            &target,
                            vec![x],
                            env_val.as_ref(),
                            line,
                        )?;
                        let b = match pv {
                            Value::Bool(b) => b,
                            Value::QExtBool(b) => {
                                saw_qext = true;
                                b
                            }
                            _ => false,
                        };
                        if is_any && b {
                            return Ok(if saw_qext {
                                Value::QExtBool(true)
                            } else {
                                Value::Bool(true)
                            });
                        }
                        if !is_any && !b {
                            return Ok(if saw_qext {
                                Value::QExtBool(false)
                            } else {
                                Value::Bool(false)
                            });
                        }
                    }
                    let default_b = !is_any;
                    return Ok(if saw_qext {
                        Value::QExtBool(default_b)
                    } else {
                        Value::Bool(default_b)
                    });
                }
                ("select", 3) => {
                    let (cv, t_expr, f_expr) = if let Some(in_v) = prepended {
                        (in_v, &args[0], &args[1])
                    } else {
                        (self.eval_expr(&args[0], mod_key, scopes)?, &args[1], &args[2])
                    };
                    match cv {
                        Value::Bool(b) | Value::QExtBool(b) => {
                            return if b {
                                self.eval_expr(t_expr, mod_key, scopes)
                            } else {
                                self.eval_expr(f_expr, mod_key, scopes)
                            };
                        }
                        other => {
                            let tv = self.eval_expr(t_expr, mod_key, scopes)?;
                            let fv = self.eval_expr(f_expr, mod_key, scopes)?;
                            return eval_select(other, tv, fv, line);
                        }
                    }
                }
                ("mux", 4) => {
                    let (rv, n_expr, z_expr, p_expr) = if let Some(in_v) = prepended {
                        (in_v, &args[0], &args[1], &args[2])
                    } else {
                        (
                            self.eval_expr(&args[0], mod_key, scopes)?,
                            &args[1],
                            &args[2],
                            &args[3],
                        )
                    };
                    match rv {
                        Value::T1(t) => {
                            return match t.value() {
                                -1 => self.eval_expr(n_expr, mod_key, scopes),
                                0 => self.eval_expr(z_expr, mod_key, scopes),
                                1 => self.eval_expr(p_expr, mod_key, scopes),
                                _ => unreachable!(),
                            };
                        }
                        other => {
                            let nv = self.eval_expr(n_expr, mod_key, scopes)?;
                            let zv = self.eval_expr(z_expr, mod_key, scopes)?;
                            let pv = self.eval_expr(p_expr, mod_key, scopes)?;
                            return eval_mux(other, nv, zv, pv, line);
                        }
                    }
                }
                _ => {}
            }
        }

        // Evaluate regular arguments
        let mut vals = Vec::with_capacity(total_arity);
        if let Some(in_v) = prepended {
            vals.push(in_v);
        }
        for a in args {
            vals.push(self.eval_expr(a, mod_key, scopes)?);
        }

        if let Some(cname @ ("sum" | "dot")) = simple_name {
            return self.eval_sum_or_dot(cname, callee, args, vals, mod_key, line);
        }

        // 2. Enum payload constructor `Enum::Variant(args...)`
        if let ExprKind::Path(ref segs) = callee.kind {
            if let Some((emod, ename, vname, tag)) = self.resolve_enum_variant_decl(mod_key, segs) {
                return Ok(Value::Enum(Box::new(EnumValue {
                    module: emod,
                    name: ename,
                    variant: vname,
                    tag,
                    payloads: vals,
                })));
            }
        }

        // 3. Positional struct constructor `Foo(e1, e2, ...)` (falling through to `dec Foo:` if recorded in `poly_call_types`)
        let struct_path_opt: Option<Vec<String>> = match &callee.kind {
            ExprKind::Ident(n) => Some(vec![n.clone()]),
            ExprKind::Path(segs) => Some(segs.clone()),
            _ => None,
        };
        let is_struct_overload = self.program.checked.poly_call_types.contains_key(&(
            mod_key.to_string(),
            line,
            format!("struct_ov:{callee:?}:{args:?}"),
        ));
        if !is_struct_overload {
            if let Some(ref sp) = struct_path_opt {
                if let Some((smod, sdecl)) = self.resolve_struct_decl(mod_key, sp) {
                    let mut fields = BTreeMap::new();
                    for ((fname, _), v) in sdecl.fields.iter().zip(vals) {
                        fields.insert(fname.clone(), v);
                    }
                    return Ok(Value::Struct(Box::new(StructValue {
                        module: smod,
                        name: sdecl.name,
                        fields,
                    })));
                }
            }
        }

        let target = self.resolve_eval_target(callee, mod_key, line)?;
        self.invoke_eval_target(&target, vals, line)
    }

    fn resolve_eval_target(
        &self,
        callee: &Expr,
        mod_key: &str,
        line: usize,
    ) -> DecResult<EvalTarget> {
        let base_mod = self.base_module_of(mod_key).to_string();
        match &callee.kind {
            ExprKind::Ident(name) => {
                if self
                    .program
                    .functions
                    .contains_key(&(mod_key.to_string(), name.clone()))
                {
                    return Ok(EvalTarget::UserFn {
                        mod_key: mod_key.to_string(),
                        name: name.clone(),
                    });
                }
                if base_mod != mod_key
                    && self
                        .program
                        .functions
                        .contains_key(&(base_mod.clone(), name.clone()))
                {
                    return Ok(EvalTarget::UserFn {
                        mod_key: base_mod,
                        name: name.clone(),
                    });
                }
                if let Some(sym) = self.imports_for(mod_key).get(name).cloned() {
                    match sym {
                        ImportedSymbol::ModuleItem { module, item } => {
                            return Ok(EvalTarget::UserFn {
                                mod_key: module,
                                name: item,
                            });
                        }
                        ImportedSymbol::Builtin(canon) => return Ok(EvalTarget::Builtin(canon)),
                        ImportedSymbol::ModuleNamespace(_) => {}
                    }
                }
                for (m, qm) in &self.program.qext_modules {
                    if m == &base_mod {
                        let sub = format!("{m}::{qm}");
                        if self.program.functions.contains_key(&(sub.clone(), name.clone())) {
                            return Ok(EvalTarget::UserFn {
                                mod_key: sub,
                                name: name.clone(),
                            });
                        }
                    }
                }
                Ok(EvalTarget::Builtin(name.clone()))
            }
            ExprKind::Path(segs) => {
                if segs.len() == 3 && segs[0] == "aod" {
                    let ns = &segs[0..2];
                    let sym = &segs[2];
                    if builtin_namespace_items(ns).is_some() {
                        return Ok(EvalTarget::Builtin(canonical_builtin_name(ns, sym)));
                    }
                } else if segs.len() == 2 {
                    if let Some(qsub) = self.resolve_qext_submodule(mod_key, &segs[0]) {
                        if self
                            .program
                            .functions
                            .contains_key(&(qsub.clone(), segs[1].clone()))
                        {
                            return Ok(EvalTarget::UserFn {
                                mod_key: qsub,
                                name: segs[1].clone(),
                            });
                        }
                    }
                    if let Some(ImportedSymbol::ModuleNamespace(target_mod)) =
                        self.imports_for(mod_key).get(&segs[0]).cloned()
                    {
                        if let Some(rest) = target_mod.strip_prefix("aod::") {
                            let ns_vec = vec!["aod".to_string(), rest.to_string()];
                            return Ok(EvalTarget::Builtin(canonical_builtin_name(
                                &ns_vec, &segs[1],
                            )));
                        }
                        return Ok(EvalTarget::UserFn {
                            mod_key: target_mod,
                            name: segs[1].clone(),
                        });
                    }
                } else if segs.len() == 3 {
                    if let Some(qsub) =
                        self.resolve_qext_submodule_3seg(mod_key, &segs[0], &segs[1])
                    {
                        if self
                            .program
                            .functions
                            .contains_key(&(qsub.clone(), segs[2].clone()))
                        {
                            return Ok(EvalTarget::UserFn {
                                mod_key: qsub,
                                name: segs[2].clone(),
                            });
                        }
                    }
                }
                Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("undefined function path {}", segs.join("::")),
                ))
            }
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "invalid call target",
            )),
        }
    }

    fn invoke_eval_target(
        &mut self,
        target: &EvalTarget,
        args: Vec<Value>,
        line: usize,
    ) -> DecResult<Value> {
        match target {
            EvalTarget::UserFn { mod_key, name } => {
                self.call_user_fn(mod_key, name, args, line)
            }
            EvalTarget::Builtin(canon) => {
                if canon.starts_with("aod::sys::") {
                    return self.eval_sys_builtin(canon, args, line);
                }
                match eval_builtin(canon, args.clone(), line) {
                    Ok(v) => Ok(v),
                    Err(e) => {
                        // Rank-lifted scalar builtin fallback!
                        if args.len() == 1 {
                            match &args[0] {
                                Value::Array(items) => {
                                    let mut out = Vec::with_capacity(items.len());
                                    for it in items {
                                        out.push(self.invoke_eval_target(
                                            target,
                                            vec![it.clone()],
                                            line,
                                        )?);
                                    }
                                    return Ok(Value::Array(out));
                                }
                                Value::List(items) => {
                                    let mut out = Vec::with_capacity(items.len());
                                    for it in items {
                                        out.push(self.invoke_eval_target(
                                            target,
                                            vec![it.clone()],
                                            line,
                                        )?);
                                    }
                                    return Ok(Value::List(out));
                                }
                                Value::Range(a, b) => {
                                    let mut out = Vec::new();
                                    for k in *a..*b {
                                        out.push(self.invoke_eval_target(
                                            target,
                                            vec![Value::Int(BigInt::from_i64(k))],
                                            line,
                                        )?);
                                    }
                                    return Ok(Value::List(out));
                                }
                                _ => {}
                            }
                        } else if args.len() == 2
                            && (val_seq_rank(&args[0]) > 0 || val_seq_rank(&args[1]) > 0)
                        {
                            return self.eval_rank_lifted_binary_target(
                                target,
                                args[0].clone(),
                                args[1].clone(),
                                line,
                            );
                        }
                        Err(e)
                    }
                }
            }
        }
    }

    fn eval_rank_lifted_binary_target(
        &mut self,
        target: &EvalTarget,
        lv: Value,
        rv: Value,
        line: usize,
    ) -> DecResult<Value> {
        let lr = val_seq_rank(&lv);
        let rr = val_seq_rank(&rv);
        if lr > rr {
            return match lv {
                Value::Array(xs) => {
                    let mut out = Vec::with_capacity(xs.len());
                    for x in xs {
                        out.push(self.invoke_eval_target(target, vec![x, rv.clone()], line)?);
                    }
                    Ok(Value::Array(out))
                }
                Value::List(xs) => {
                    let mut out = Vec::with_capacity(xs.len());
                    for x in xs {
                        out.push(self.invoke_eval_target(target, vec![x, rv.clone()], line)?);
                    }
                    Ok(Value::List(out))
                }
                _ => unreachable!(),
            };
        }
        if rr > lr {
            return match rv {
                Value::Array(ys) => {
                    let mut out = Vec::with_capacity(ys.len());
                    for y in ys {
                        out.push(self.invoke_eval_target(target, vec![lv.clone(), y], line)?);
                    }
                    Ok(Value::Array(out))
                }
                Value::List(ys) => {
                    let mut out = Vec::with_capacity(ys.len());
                    for y in ys {
                        out.push(self.invoke_eval_target(target, vec![lv.clone(), y], line)?);
                    }
                    Ok(Value::List(out))
                }
                _ => unreachable!(),
            };
        }
        let is_fixed = matches!((&lv, &rv), (Value::Array(_), Value::Array(_)));
        let (xs, _) = extract_seq_items(lv, "rank-lift", line)?;
        let (ys, _) = extract_seq_items(rv, "rank-lift", line)?;
        if xs.len() != ys.len() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!("sequence length mismatch: {} vs {}", xs.len(), ys.len()),
            ));
        }
        let mut out = Vec::with_capacity(xs.len());
        for (x, y) in xs.into_iter().zip(ys) {
            out.push(self.invoke_eval_target(target, vec![x, y], line)?);
        }
        Ok(if is_fixed {
            Value::Array(out)
        } else {
            Value::List(out)
        })
    }

    fn eval_sys_builtin(&self, name: &str, args: Vec<Value>, line: usize) -> DecResult<Value> {
        match (name, args.as_slice()) {
            ("aod::sys::cli_args", []) => Ok(Value::List(
                self.cli_args.iter().cloned().map(Value::Bytes).collect(),
            )),
            ("aod::sys::read_file_bytes", [Value::Bytes(path_bytes)]) => {
                if path_bytes.is_empty() || path_bytes.contains(&0) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        "invalid empty or NUL-containing path in read_file_bytes",
                    ));
                }
                let path_str = std::str::from_utf8(path_bytes).map_err(|_| {
                    Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        "file path must be valid UTF-8",
                    )
                })?;
                let meta = std::fs::metadata(path_str).map_err(|e| {
                    Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("failed to stat {path_str:?}: {e}"),
                    )
                })?;
                if meta.len() > (MAX_SYS_IO_BYTES as u64) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("file {path_str:?} exceeds {MAX_SYS_IO_BYTES}-byte bootstrap limit"),
                    ));
                }
                let data = std::fs::read(path_str).map_err(|e| {
                    Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("failed to read {path_str:?}: {e}"),
                    )
                })?;
                if data.len() > MAX_SYS_IO_BYTES {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("file {path_str:?} exceeds {MAX_SYS_IO_BYTES}-byte bootstrap limit"),
                    ));
                }
                Ok(Value::Bytes(data))
            }
            ("aod::sys::write_executable_elf", [Value::Bytes(path_bytes), Value::Bytes(img)]) => {
                if path_bytes.is_empty() || path_bytes.contains(&0) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        "invalid empty or NUL-containing output ELF path",
                    ));
                }
                if img.len() < 64 || &img[0..4] != b"\x7FELF" {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        "write_executable_elf requires a valid ELF64 header (\\x7FELF)",
                    ));
                }
                if img.len() > MAX_SYS_IO_BYTES {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("ELF image exceeds {MAX_SYS_IO_BYTES}-byte bootstrap limit"),
                    ));
                }
                let path_str = std::str::from_utf8(path_bytes).map_err(|_| {
                    Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        "output ELF path must be valid UTF-8",
                    )
                })?;
                let target = Path::new(path_str);
                if let Ok(meta) = std::fs::symlink_metadata(target) {
                    if meta.file_type().is_symlink() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::RuntimeError,
                            line,
                            format!("refusing to overwrite symlink {path_str:?}"),
                        ));
                    }
                }
                std::fs::write(target, img).map_err(|e| {
                    Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("failed to write ELF {path_str:?}: {e}"),
                    )
                })?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o755))
                        .map_err(|e| {
                            Diagnostic::at_line(
                                DiagnosticCode::RuntimeError,
                                line,
                                format!("failed to chmod +x {path_str:?}: {e}"),
                            )
                        })?;
                }
                Ok(Value::Int(BigInt::from_u64(img.len() as u64)))
            }
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!("unlisted sys builtin {name}"),
            )),
        }
    }

    fn run_simulation(
        &mut self,
        mod_key: &str,
        prepare_name: &str,
        ticks: u64,
        observer_names: &[String],
        line: usize,
    ) -> DecResult<Value> {
        let prep = self
            .program
            .prepares
            .get(&(mod_key.to_string(), prepare_name.to_string()))
            .cloned()
            .ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("undefined prepare {prepare_name}"),
                )
            })?;
        let chamber = self
            .program
            .chambers
            .get(&(mod_key.to_string(), prep.chamber.clone()))
            .cloned()
            .ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("undefined chamber {}", prep.chamber),
                )
            })?;

        let empty_scopes: Vec<BTreeMap<String, Value>> = Vec::new();
        let space = match self.eval_expr(&chamber.space, mod_key, &empty_scopes)? {
            Value::Box3(b) => b,
            _ => {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    chamber.line,
                    "chamber space must evaluate to box([...])",
                ));
            }
        };

        let mut prep_scopes = vec![BTreeMap::new()];
        let _ = self.eval_block(&prep.lets, mod_key, &mut prep_scopes)?;

        let mut clocks = Vec::with_capacity(prep.clocks.len());
        for clk in &prep.clocks {
            let p = extract_i64(self.eval_expr(&clk.period, mod_key, &prep_scopes)?, clk.line)?;
            if p <= 0 {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    clk.line,
                    format!("clock {} period must be >= 1, got {p}", clk.name),
                ));
            }
            let rc = ReferenceClock::new(clk.name.clone(), p as u64).map_err(|e| {
                Diagnostic::at_line(DiagnosticCode::RuntimeError, clk.line, e.to_string())
            })?;
            clocks.push(rc);
        }

        let mut carriers = Vec::with_capacity(prep.injections.len());
        for (idx, inj) in prep.injections.iter().enumerate() {
            let support = match self.eval_expr(&inj.support, mod_key, &prep_scopes)? {
                Value::Support(sf) => sf,
                _ => {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        inj.line,
                        "inject support must be SupportForm",
                    ));
                }
            };
            let pos = match self.eval_expr(&inj.pos, mod_key, &prep_scopes)? {
                Value::Tuple(elems) if elems.len() == 3 => [
                    extract_i64(elems[0].clone(), inj.line)?,
                    extract_i64(elems[1].clone(), inj.line)?,
                    extract_i64(elems[2].clone(), inj.line)?,
                ],
                _ => {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        inj.line,
                        "inject pos must be (int, int, int)",
                    ));
                }
            };
            let orient = match self.eval_expr(&inj.orient, mod_key, &prep_scopes)? {
                Value::T1(t) => t,
                Value::Int(n) => T1::from_bigint(&n).map_err(|e| {
                    Diagnostic::at_line(DiagnosticCode::RuntimeError, inj.line, e.to_string())
                })?,
                _ => {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        inj.line,
                        "inject orient must be t1 or int in {-1, 1}",
                    ));
                }
            };
            if orient == T1::ZERO {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    inj.line,
                    "inject orient must be nonzero (-1t or +1t)",
                ));
            }
            let shells_i =
                extract_i64(self.eval_expr(&inj.shells, mod_key, &prep_scopes)?, inj.line)?;
            if shells_i <= 0 {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    inj.line,
                    format!("inject shells must be >= 1, got {shells_i}"),
                ));
            }
            let shells = u32::try_from(shells_i).map_err(|_| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    inj.line,
                    format!("inject shells {shells_i} exceeds u32"),
                )
            })?;
            let carrier = CarrierState::new(
                (idx + 1) as u64,
                &chamber.name,
                support,
                shells,
                pos,
                inj.dir,
                orient,
            )
            .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, inj.line, e.to_string()))?;
            carriers.push(carrier);
        }

        let mut decl_observers: Vec<DeclarativeObserver> = Vec::new();
        let mut proc_probes: Vec<(String, String, Vec<Stmt>)> = Vec::new();

        for obs_name in observer_names {
            let obs = self
                .program
                .observes
                .get(&(mod_key.to_string(), obs_name.clone()))
                .cloned()
                .ok_or_else(|| {
                    Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("undefined observe {obs_name}"),
                    )
                })?;
            for probe in obs.probes {
                match probe {
                    ProbeDecl::Declarative {
                        kind,
                        name,
                        region,
                        capture,
                        line: pline,
                    } => {
                        let box_r = match self.eval_expr(&region, mod_key, &empty_scopes)? {
                            Value::Box3(b) => b,
                            _ => {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::RuntimeError,
                                    pline,
                                    "probe region must evaluate to box([...])",
                                ));
                            }
                        };
                        decl_observers.push(DeclarativeObserver::new(kind, name, box_r, capture));
                    }
                    ProbeDecl::Procedural {
                        name,
                        param_name,
                        body,
                        ..
                    } => {
                        proc_probes.push((name, param_name, body));
                    }
                }
            }
        }

        let mut state = ChamberState::new(&chamber.name, space, chamber.walls, carriers, clocks);
        let mut procedural_outputs: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let use_parallel = self.parallel_observers && proc_probes.len() > 1;

        for _ in 0..ticks {
            self.tick()?;
            state
                .step()
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string()))?;
            let snap = state.snapshot();
            for obs in &mut decl_observers {
                obs.observe_snapshot(&snap);
            }
            if proc_probes.is_empty() {
                continue;
            }
            if !use_parallel {
                for (pname, param_name, body) in &proc_probes {
                    let mut pscope = BTreeMap::new();
                    pscope.insert(param_name.clone(), Value::ChamberView(Box::new(snap.clone())));
                    let mut pscopes = vec![pscope];
                    if let ControlFlow::Return(v) = self.eval_block(body, mod_key, &mut pscopes)? {
                        if v != Value::Unit {
                            procedural_outputs
                                .entry(pname.clone())
                                .or_default()
                                .push(v.render());
                        }
                    }
                }
            } else {
                let start_fuel = self.fuel;
                let parent_depth = self.depth;
                let parent_max_depth = self.max_depth;
                let prog_ref = &self.program;
                let snap_ref = &snap;

                let outcomes: Vec<(usize, Vec<String>, DecResult<ControlFlow>)> =
                    std::thread::scope(|scope| {
                        let mut handles = Vec::with_capacity(proc_probes.len());
                        for (_, param_name, body) in &proc_probes {
                            let w_prog = Arc::clone(prog_ref);
                            let p_name = param_name.as_str();
                            let body_slice = body.as_slice();
                            handles.push(scope.spawn(move || {
                                let mut worker = Evaluator {
                                    program: w_prog,
                                    fuel: start_fuel,
                                    max_depth: parent_max_depth,
                                    depth: parent_depth,
                                    emits: Vec::new(),
                                    cli_args: Vec::new(),
                                    parallel_observers: false,
                                };
                                let mut pscope = BTreeMap::new();
                                pscope.insert(
                                    p_name.to_string(),
                                    Value::ChamberView(Box::new(snap_ref.clone())),
                                );
                                let mut pscopes = vec![pscope];
                                let res = worker.eval_block(body_slice, mod_key, &mut pscopes);
                                (worker.fuel, worker.emits, res)
                            }));
                        }
                        handles
                            .into_iter()
                            .map(|h| {
                                h.join().unwrap_or_else(|_| {
                                    (
                                        start_fuel,
                                        Vec::new(),
                                        Err(Diagnostic::at_line(
                                            DiagnosticCode::RuntimeError,
                                            line,
                                            "parallel observer worker panicked",
                                        )),
                                    )
                                })
                            })
                            .collect()
                    });

                let mut remaining_fuel = start_fuel;
                for (idx, (fuel_after, w_emits, w_res)) in outcomes.into_iter().enumerate() {
                    let used_i = start_fuel.saturating_sub(fuel_after);
                    let (pname, param_name, body) = &proc_probes[idx];
                    if used_i <= remaining_fuel {
                        remaining_fuel = remaining_fuel.saturating_sub(used_i);
                        self.emits.extend(w_emits);
                        match w_res {
                            Ok(ControlFlow::Return(v)) if v != Value::Unit => {
                                procedural_outputs
                                    .entry(pname.clone())
                                    .or_default()
                                    .push(v.render());
                            }
                            Ok(_) => {}
                            Err(err) => {
                                self.fuel = remaining_fuel;
                                return Err(err);
                            }
                        }
                    } else {
                        self.fuel = remaining_fuel;
                        let mut pscope = BTreeMap::new();
                        pscope.insert(
                            param_name.clone(),
                            Value::ChamberView(Box::new(snap.clone())),
                        );
                        let mut pscopes = vec![pscope];
                        if let ControlFlow::Return(v) =
                            self.eval_block(body, mod_key, &mut pscopes)?
                        {
                            if v != Value::Unit {
                                procedural_outputs
                                    .entry(pname.clone())
                                    .or_default()
                                    .push(v.render());
                            }
                        }
                        remaining_fuel = self.fuel;
                    }
                }
                self.fuel = remaining_fuel;
            }
        }

        let final_hash = state.state_hash();
        let final_snapshot = state.snapshot();

        let mut support_counts = BTreeMap::new();
        for c in &state.carriers {
            *support_counts.entry(c.support.render()).or_insert(0) += 1;
        }
        let total_returns: u64 = state.carriers.iter().map(|c| c.return_count).sum();
        let total_hinges: u64 = state.carriers.iter().map(|c| c.hinge_count).sum();
        let total_shedding_events = state.shedding_events.len() as u64;

        let sensor_currents: Vec<SensorCurrentSummary> = decl_observers
            .iter()
            .map(|obs| {
                let (cadence_ratio, single_contact_association) = obs.cadence_readout();
                SensorCurrentSummary {
                    sensor_name: obs.name.clone(),
                    boundary_crossings: obs.boundary_crossings,
                    bound_contacts: obs.observed_contacts,
                    cadence_ratio,
                    single_contact_association,
                }
            })
            .collect();

        Ok(Value::SimResult(Box::new(SimRunResult {
            model_status: state.model_status,
            chamber_name: state.chamber_name,
            ticks: state.tick,
            final_state_hash: final_hash,
            carrier_count: state.carriers.len(),
            shedding_event_count: state.shedding_events.len(),
            wavelet_event_count: state.wavelet_events.len(),
            boundary_crossings: state.boundary_crossings,
            blind_motif_spectrum: BlindMotifSpectrum {
                support_counts,
                total_returns,
                total_hinges,
                total_shedding_events,
            },
            sensor_currents,
            procedural_outputs,
            final_snapshot,
        })))
    }
}

fn runtime_val_matches_type(val: &Value, expected: &Type) -> bool {
    let target = expected.unwrap_read_only();
    let actual = val.type_of();
    if &actual == target {
        return true;
    }
    match (val, target) {
        (Value::List(items), Type::List(_)) if items.is_empty() => true,
        (Value::Array(items) | Value::List(items), Type::List(elem_ty)) => {
            items.iter().all(|it| runtime_val_matches_type(it, elem_ty))
        }
        (Value::Array(items), Type::Array(elem_ty, n)) => {
            items.len() == *n && items.iter().all(|it| runtime_val_matches_type(it, elem_ty))
        }
        (Value::Map(m), Type::Map(_, _)) if m.is_empty() => true,
        (Value::Tuple(vs), Type::Tuple(ts)) if vs.len() == ts.len() => vs
            .iter()
            .zip(ts)
            .all(|(v, t)| runtime_val_matches_type(v, t)),
        _ => false,
    }
}

fn val_seq_rank(v: &Value) -> usize {
    match v {
        Value::Array(xs) | Value::List(xs) => 1 + xs.first().map_or(0, val_seq_rank),
        _ => 0,
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Array(xs) | Value::List(xs), Value::Array(ys) | Value::List(ys)) => {
            xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| values_equal(x, y))
        }
        (Value::Tuple(xs), Value::Tuple(ys)) => {
            xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| values_equal(x, y))
        }
        _ => a == b,
    }
}

fn materialize_range_list(a: i64, b: i64, line: usize) -> DecResult<Value> {
    if b > a && (b - a) > 1_000_000 {
        return Err(Diagnostic::at_line(
            DiagnosticCode::RuntimeError,
            line,
            format!("range {a}..{b} exceeds 1_000_000 element materialization limit"),
        ));
    }
    Ok(Value::List(
        (a..b).map(|k| Value::Int(BigInt::from_i64(k))).collect(),
    ))
}

fn eval_afc_seq_values(v: Value, ctx_name: &str, line: usize) -> DecResult<Vec<Value>> {
    match v {
        Value::Array(xs) | Value::List(xs) => Ok(xs),
        Value::Bytes(bs) => Ok(bs.into_iter().map(|b| Value::B8(B8(b))).collect()),
        other => Err(Diagnostic::at_line(
            DiagnosticCode::RuntimeError,
            line,
            format!(
                "{ctx_name} expects [T], [T; N], or bytes, got {}",
                other.type_of()
            ),
        )),
    }
}

fn extract_seq_items(v: Value, ctx_name: &str, line: usize) -> DecResult<(Vec<Value>, bool)> {
    match v {
        Value::Array(xs) => Ok((xs, true)),
        Value::List(xs) => Ok((xs, false)),
        Value::Range(a, b) => {
            let Value::List(xs) = materialize_range_list(a, b, line)? else {
                unreachable!()
            };
            Ok((xs, false))
        }
        other => Err(Diagnostic::at_line(
            DiagnosticCode::RuntimeError,
            line,
            format!("{ctx_name} expects sequence or range, got {}", other.type_of()),
        )),
    }
}

fn extract_i64(val: Value, line: usize) -> DecResult<i64> {
    match val {
        Value::Int(n) => n.to_i64().ok_or_else(|| {
            Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!("integer {n} outside i64 domain"),
            )
        }),
        other => Err(Diagnostic::at_line(
            DiagnosticCode::RuntimeError,
            line,
            format!("expected int, got {}", other.type_of()),
        )),
    }
}

fn eval_unary(op: UnaryOp, val: Value, line: usize) -> DecResult<Value> {
    if op == UnaryOp::Boundary {
        return match val {
            Value::Box3(b) => Ok(Value::Bytes(b.boundary_descriptor().into_bytes())),
            Value::SimResult(s) => Ok(Value::Bytes(
                s.final_snapshot.space.boundary_descriptor().into_bytes(),
            )),
            Value::Window(u, v) | Value::Range(u, v) => Ok(Value::Tuple(vec![
                Value::Int(BigInt::from_i64(u)),
                Value::Int(BigInt::from_i64(v)),
            ])),
            Value::Array(xs) => {
                if xs.len() < 2 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        "discrete boundary ∂ requires sequence length >= 2",
                    ));
                }
                let mut out = Vec::with_capacity(xs.len() - 1);
                for w in xs.windows(2) {
                    out.push(eval_binary(BinaryOp::Sub, w[1].clone(), w[0].clone(), line)?);
                }
                Ok(Value::Array(out))
            }
            Value::List(xs) => {
                if xs.len() < 2 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        "discrete boundary ∂ requires sequence length >= 2",
                    ));
                }
                let mut out = Vec::with_capacity(xs.len() - 1);
                for w in xs.windows(2) {
                    out.push(eval_binary(BinaryOp::Sub, w[1].clone(), w[0].clone(), line)?);
                }
                Ok(Value::List(out))
            }
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "invalid boundary ∂ operand",
            )),
        };
    }

    match val {
        Value::Array(xs) => {
            let mut out = Vec::with_capacity(xs.len());
            for x in xs {
                out.push(eval_unary(op, x, line)?);
            }
            Ok(Value::Array(out))
        }
        Value::List(xs) => {
            let mut out = Vec::with_capacity(xs.len());
            for x in xs {
                out.push(eval_unary(op, x, line)?);
            }
            Ok(Value::List(out))
        }
        scalar => match (op, scalar) {
            (UnaryOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
            (UnaryOp::Not, Value::QExtBool(b)) => Ok(Value::QExtBool(!b)),
            (
                UnaryOp::Pos,
                v @ (Value::Int(_) | Value::T1(_) | Value::Ratio(_) | Value::QExtRatio(_)),
            ) => Ok(v),
            (UnaryOp::Neg, Value::Int(n)) => Ok(Value::Int(-n)),
            (UnaryOp::Neg, Value::T1(t)) => Ok(Value::T1(t.checked_neg())),
            (UnaryOp::Neg, Value::Ratio(r)) => Ok(Value::Ratio(-r)),
            (UnaryOp::Neg, Value::QExtRatio(q)) => Ok(Value::QExtRatio(q.neg())),
            (UnaryOp::BitNot, Value::B8(b)) => Ok(Value::B8(b.bitnot())),
            (UnaryOp::BitNot, Value::B64(b)) => Ok(Value::B64(b.bitnot())),
            (UnaryOp::BitNot, Value::Q4(q)) => Q4::new((!q.value()) & 0x0F)
                .map(Value::Q4)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "invalid unary operation",
            )),
        },
    }
}

fn eval_adverb(op: AdverbOp, val: Value, line: usize) -> DecResult<Value> {
    let (items, is_fixed) = extract_seq_items(val, "adverb", line)?;
    let is_scan = matches!(
        op,
        AdverbOp::ScanAdd | AdverbOp::ScanMul | AdverbOp::ScanMin | AdverbOp::ScanMax
    );
    if is_scan {
        if items.is_empty() {
            return Ok(Value::List(Vec::new()));
        }
        let mut out = Vec::with_capacity(items.len());
        let mut acc = items[0].clone();
        out.push(acc.clone());
        for it in items.into_iter().skip(1) {
            acc = match op {
                AdverbOp::ScanAdd => eval_binary(BinaryOp::Add, acc, it, line)?,
                AdverbOp::ScanMul => eval_binary(BinaryOp::Mul, acc, it, line)?,
                AdverbOp::ScanMin => eval_binary(BinaryOp::Min, acc, it, line)?,
                AdverbOp::ScanMax => eval_binary(BinaryOp::Max, acc, it, line)?,
                _ => unreachable!(),
            };
            out.push(acc.clone());
        }
        return Ok(if is_fixed {
            Value::Array(out)
        } else {
            Value::List(out)
        });
    }

    if items.is_empty() {
        return match op {
            AdverbOp::FoldAdd => Ok(Value::Int(BigInt::zero())),
            AdverbOp::FoldMul => Ok(Value::Int(BigInt::one())),
            AdverbOp::FoldAnd => Ok(Value::Bool(true)),
            AdverbOp::FoldOr => Ok(Value::Bool(false)),
            AdverbOp::FoldSuperpose => Ok(Value::T1(T1::ZERO)),
            AdverbOp::FoldMin | AdverbOp::FoldMax => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "min/ and max/ require non-empty sequence",
            )),
            _ => unreachable!(),
        };
    }

    let mut iter = items.into_iter();
    let mut acc = iter.next().unwrap();
    for it in iter {
        acc = match op {
            AdverbOp::FoldAdd => eval_binary(BinaryOp::Add, acc, it, line)?,
            AdverbOp::FoldMul => eval_binary(BinaryOp::Mul, acc, it, line)?,
            AdverbOp::FoldMin => eval_binary(BinaryOp::Min, acc, it, line)?,
            AdverbOp::FoldMax => eval_binary(BinaryOp::Max, acc, it, line)?,
            AdverbOp::FoldAnd => match (&acc, &it) {
                (Value::Bool(a), Value::Bool(b)) => Value::Bool(*a && *b),
                _ => eval_binary(BinaryOp::BitAnd, acc, it, line)?,
            },
            AdverbOp::FoldOr => match (&acc, &it) {
                (Value::Bool(a), Value::Bool(b)) => Value::Bool(*a || *b),
                _ => eval_binary(BinaryOp::BitOr, acc, it, line)?,
            },
            AdverbOp::FoldSuperpose => match (acc, it) {
                (Value::T1(a), Value::T1(b)) => Value::T1(a.superpose_cancel(b)),
                (Value::Int(a), Value::Int(b)) => Value::Int(a.superpose_cancel(&b)),
                _ => {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        "superpose/ requires t1 or int elements",
                    ));
                }
            },
            _ => unreachable!(),
        };
    }
    Ok(acc)
}

fn eval_binary(op: BinaryOp, left: Value, right: Value, line: usize) -> DecResult<Value> {
    // 1. Mask Sieve `mask ?> xs`
    if op == BinaryOp::Sieve {
        let (mask_items, _) = extract_seq_items(left, "sieve mask", line)?;
        let (xs_items, _) = extract_seq_items(right, "sieve sequence", line)?;
        if mask_items.len() != xs_items.len() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!(
                    "sieve `?>` length mismatch: mask {} != sequence {}",
                    mask_items.len(),
                    xs_items.len()
                ),
            ));
        }
        let mut out = Vec::new();
        for (m, x) in mask_items.into_iter().zip(xs_items) {
            match m {
                Value::Bool(true) => out.push(x),
                Value::Bool(false) => {}
                _ => {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        "sieve mask requires bool elements",
                    ));
                }
            }
        }
        return Ok(Value::List(out));
    }

    // 2. Outer products `+:` and `*:`
    if matches!(op, BinaryOp::OuterAdd | BinaryOp::OuterMul) {
        let inner_op = if op == BinaryOp::OuterAdd {
            BinaryOp::Add
        } else {
            BinaryOp::Mul
        };
        let is_fixed = matches!((&left, &right), (Value::Array(_), Value::Array(_)));
        let (xs, _) = extract_seq_items(left, "outer product lhs", line)?;
        let (ys, _) = extract_seq_items(right, "outer product rhs", line)?;
        let mut rows = Vec::with_capacity(xs.len());
        for x in xs {
            let mut row = Vec::with_capacity(ys.len());
            for y in &ys {
                row.push(eval_binary(inner_op, x.clone(), y.clone(), line)?);
            }
            rows.push(if is_fixed {
                Value::Array(row)
            } else {
                Value::List(row)
            });
        }
        return Ok(if is_fixed {
            Value::Array(rows)
        } else {
            Value::List(rows)
        });
    }

    // 3. Whole-sequence `==` / `!=` when both operands have equal positive rank
    let lr = val_seq_rank(&left);
    let rr = val_seq_rank(&right);
    if matches!(op, BinaryOp::Eq | BinaryOp::NotEq) && lr > 0 && lr == rr {
        let eq = values_equal(&left, &right);
        return Ok(Value::Bool(if op == BinaryOp::Eq { eq } else { !eq }));
    }

    // 4. Rank-lifted binary operations + scalar broadcast
    if lr > 0 || rr > 0 {
        if lr > rr {
            return match left {
                Value::Array(xs) => {
                    let mut out = Vec::with_capacity(xs.len());
                    for x in xs {
                        out.push(eval_binary(op, x, right.clone(), line)?);
                    }
                    Ok(Value::Array(out))
                }
                Value::List(xs) => {
                    let mut out = Vec::with_capacity(xs.len());
                    for x in xs {
                        out.push(eval_binary(op, x, right.clone(), line)?);
                    }
                    Ok(Value::List(out))
                }
                _ => unreachable!(),
            };
        }
        if rr > lr {
            return match right {
                Value::Array(ys) => {
                    let mut out = Vec::with_capacity(ys.len());
                    for y in ys {
                        out.push(eval_binary(op, left.clone(), y, line)?);
                    }
                    Ok(Value::Array(out))
                }
                Value::List(ys) => {
                    let mut out = Vec::with_capacity(ys.len());
                    for y in ys {
                        out.push(eval_binary(op, left.clone(), y, line)?);
                    }
                    Ok(Value::List(out))
                }
                _ => unreachable!(),
            };
        }
        let is_fixed = matches!((&left, &right), (Value::Array(_), Value::Array(_)));
        let (xs, _) = extract_seq_items(left, "rank-lift lhs", line)?;
        let (ys, _) = extract_seq_items(right, "rank-lift rhs", line)?;
        if xs.len() != ys.len() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!("sequence length mismatch: {} vs {}", xs.len(), ys.len()),
            ));
        }
        let mut out = Vec::with_capacity(xs.len());
        for (x, y) in xs.into_iter().zip(ys) {
            out.push(eval_binary(op, x, y, line)?);
        }
        return Ok(if is_fixed {
            Value::Array(out)
        } else {
            Value::List(out)
        });
    }

    // 5. Scalar binary operations
    if op == BinaryOp::Eq {
        return match (&left, &right) {
            (Value::QExtRatio(a), Value::QExtRatio(b)) => Ok(Value::QExtBool(a == b)),
            (Value::QExtBool(a), Value::QExtBool(b)) => Ok(Value::QExtBool(a == b)),
            (Value::QExtBytes(a), Value::QExtBytes(b)) => Ok(Value::QExtBool(a == b)),
            _ => Ok(Value::Bool(values_equal(&left, &right))),
        };
    }
    if op == BinaryOp::NotEq {
        return match (&left, &right) {
            (Value::QExtRatio(a), Value::QExtRatio(b)) => Ok(Value::QExtBool(a != b)),
            (Value::QExtBool(a), Value::QExtBool(b)) => Ok(Value::QExtBool(a != b)),
            (Value::QExtBytes(a), Value::QExtBytes(b)) => Ok(Value::QExtBool(a != b)),
            _ => Ok(Value::Bool(!values_equal(&left, &right))),
        };
    }

    // Ternary phase-gate `x * role`
    if op == BinaryOp::Mul {
        match (&left, &right) {
            (Value::Int(n), Value::T1(t)) | (Value::T1(t), Value::Int(n)) => {
                return Ok(Value::Int(match t.value() {
                    -1 => -n.clone(),
                    0 => BigInt::zero(),
                    1 => n.clone(),
                    _ => unreachable!(),
                }));
            }
            (Value::Ratio(r), Value::T1(t)) | (Value::T1(t), Value::Ratio(r)) => {
                return Ok(Value::Ratio(match t.value() {
                    -1 => -r.clone(),
                    0 => Ratio::zero(),
                    1 => r.clone(),
                    _ => unreachable!(),
                }));
            }
            _ => {}
        }
    }

    // Bit shifts `(b8 | b64 | q4) << int`
    if matches!(op, BinaryOp::Shl | BinaryOp::Shr) {
        let shift = match &right {
            Value::Int(n) => n.to_u64().and_then(|u| u32::try_from(u).ok()).ok_or_else(|| {
                Diagnostic::at_line(DiagnosticCode::RuntimeError, line, "invalid bit shift count")
            })?,
            _ => {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "bit shift count must be non-negative int",
                ));
            }
        };
        return match (op, left) {
            (BinaryOp::Shl, Value::B8(b)) => b
                .checked_shl(shift)
                .map(Value::B8)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            (BinaryOp::Shr, Value::B8(b)) => b
                .checked_shr(shift)
                .map(Value::B8)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            (BinaryOp::Shl, Value::B64(b)) => b
                .checked_shl(shift)
                .map(Value::B64)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            (BinaryOp::Shr, Value::B64(b)) => b
                .checked_shr(shift)
                .map(Value::B64)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            (BinaryOp::Shl, Value::Q4(q)) => {
                if shift >= 4 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("q4 shift count must be in 0..4, got {shift}"),
                    ));
                }
                Q4::new((q.value() << shift) & 0x0F)
                    .map(Value::Q4)
                    .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string()))
            }
            (BinaryOp::Shr, Value::Q4(q)) => {
                if shift >= 4 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("q4 shift count must be in 0..4, got {shift}"),
                    ));
                }
                Q4::new(q.value() >> shift)
                    .map(Value::Q4)
                    .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string()))
            }
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "invalid shift operand",
            )),
        };
    }

    match (left, right) {
        (Value::Bool(a), Value::Bool(b)) => match op {
            BinaryOp::And => Ok(Value::Bool(a && b)),
            BinaryOp::Or => Ok(Value::Bool(a || b)),
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "unsupported bool operator",
            )),
        },
        (Value::QExtBool(a), Value::QExtBool(b)) => match op {
            BinaryOp::And => Ok(Value::QExtBool(a && b)),
            BinaryOp::Or => Ok(Value::QExtBool(a || b)),
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "unsupported QExt<bool> operator",
            )),
        },
        (Value::Int(a), Value::Int(b)) => match op {
            BinaryOp::Add => Ok(Value::Int(&a + &b)),
            BinaryOp::Sub => Ok(Value::Int(&a - &b)),
            BinaryOp::Mul => Ok(Value::Int(&a * &b)),
            BinaryOp::Div => Ratio::new(a, b)
                .map(Value::Ratio)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::FloorDiv => a
                .div_floor(&b)
                .map(Value::Int)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::Mod => a
                .rem_floor(&b)
                .map(Value::Int)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::Min => Ok(Value::Int(if a <= b { a } else { b })),
            BinaryOp::Max => Ok(Value::Int(if a >= b { a } else { b })),
            BinaryOp::Lt => Ok(Value::Bool(a < b)),
            BinaryOp::LtEq => Ok(Value::Bool(a <= b)),
            BinaryOp::Gt => Ok(Value::Bool(a > b)),
            BinaryOp::GtEq => Ok(Value::Bool(a >= b)),
            BinaryOp::Cmp3 => Ok(Value::T1(ord_to_t1(a.cmp(&b)))),
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "unsupported int operator",
            )),
        },
        (Value::T1(a), Value::T1(b)) => match op {
            BinaryOp::Add => a
                .checked_add(b)
                .map(Value::T1)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::Sub => a
                .checked_sub(b)
                .map(Value::T1)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::Mul => a
                .checked_mul(b)
                .map(Value::T1)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::Div => Ratio::from_i64(a.value() as i64, b.value() as i64)
                .map(Value::Ratio)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::Min => Ok(Value::T1(if a <= b { a } else { b })),
            BinaryOp::Max => Ok(Value::T1(if a >= b { a } else { b })),
            BinaryOp::Lt => Ok(Value::Bool(a < b)),
            BinaryOp::LtEq => Ok(Value::Bool(a <= b)),
            BinaryOp::Gt => Ok(Value::Bool(a > b)),
            BinaryOp::GtEq => Ok(Value::Bool(a >= b)),
            BinaryOp::Cmp3 => Ok(Value::T1(ord_to_t1(a.cmp(&b)))),
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "unsupported t1 operator",
            )),
        },
        (Value::Ratio(a), Value::Ratio(b)) => match op {
            BinaryOp::Add => Ok(Value::Ratio(&a + &b)),
            BinaryOp::Sub => Ok(Value::Ratio(&a - &b)),
            BinaryOp::Mul => Ok(Value::Ratio(&a * &b)),
            BinaryOp::Div => a
                .checked_div(&b)
                .map(Value::Ratio)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::Min => Ok(Value::Ratio(if a <= b { a } else { b })),
            BinaryOp::Max => Ok(Value::Ratio(if a >= b { a } else { b })),
            BinaryOp::Lt => Ok(Value::Bool(a < b)),
            BinaryOp::LtEq => Ok(Value::Bool(a <= b)),
            BinaryOp::Gt => Ok(Value::Bool(a > b)),
            BinaryOp::GtEq => Ok(Value::Bool(a >= b)),
            BinaryOp::Cmp3 => Ok(Value::T1(ord_to_t1(a.cmp(&b)))),
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "unsupported ratio operator",
            )),
        },
        (Value::B8(a), Value::B8(b)) => match op {
            BinaryOp::BitAnd => Ok(Value::B8(a.bitand(b))),
            BinaryOp::BitOr => Ok(Value::B8(a.bitor(b))),
            BinaryOp::BitXor => Ok(Value::B8(a.bitxor(b))),
            BinaryOp::Min => Ok(Value::B8(if a <= b { a } else { b })),
            BinaryOp::Max => Ok(Value::B8(if a >= b { a } else { b })),
            BinaryOp::Lt => Ok(Value::Bool(a < b)),
            BinaryOp::LtEq => Ok(Value::Bool(a <= b)),
            BinaryOp::Gt => Ok(Value::Bool(a > b)),
            BinaryOp::GtEq => Ok(Value::Bool(a >= b)),
            BinaryOp::Cmp3 => Ok(Value::T1(ord_to_t1(a.cmp(&b)))),
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "unsupported b8 operator",
            )),
        },
        (Value::B64(a), Value::B64(b)) => match op {
            BinaryOp::BitAnd => Ok(Value::B64(a.bitand(b))),
            BinaryOp::BitOr => Ok(Value::B64(a.bitor(b))),
            BinaryOp::BitXor => Ok(Value::B64(a.bitxor(b))),
            BinaryOp::Min => Ok(Value::B64(if a <= b { a } else { b })),
            BinaryOp::Max => Ok(Value::B64(if a >= b { a } else { b })),
            BinaryOp::Lt => Ok(Value::Bool(a < b)),
            BinaryOp::LtEq => Ok(Value::Bool(a <= b)),
            BinaryOp::Gt => Ok(Value::Bool(a > b)),
            BinaryOp::GtEq => Ok(Value::Bool(a >= b)),
            BinaryOp::Cmp3 => Ok(Value::T1(ord_to_t1(a.cmp(&b)))),
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "unsupported b64 operator",
            )),
        },
        (Value::Q4(a), Value::Q4(b)) => match op {
            BinaryOp::BitAnd => Q4::new(a.value() & b.value())
                .map(Value::Q4)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::BitOr => Q4::new(a.value() | b.value())
                .map(Value::Q4)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::BitXor => Q4::new(a.value() ^ b.value())
                .map(Value::Q4)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::Min => Ok(Value::Q4(if a <= b { a } else { b })),
            BinaryOp::Max => Ok(Value::Q4(if a >= b { a } else { b })),
            BinaryOp::Lt => Ok(Value::Bool(a < b)),
            BinaryOp::LtEq => Ok(Value::Bool(a <= b)),
            BinaryOp::Gt => Ok(Value::Bool(a > b)),
            BinaryOp::GtEq => Ok(Value::Bool(a >= b)),
            BinaryOp::Cmp3 => Ok(Value::T1(ord_to_t1(a.cmp(&b)))),
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "unsupported q4 operator",
            )),
        },
        (Value::Bytes(a), Value::Bytes(b)) => eval_ord_only(op, a, b, Value::Bytes, line),
        (Value::Addr8(a), Value::Addr8(b)) => eval_ord_only(op, a, b, Value::Addr8, line),
        (Value::QExtRatio(a), Value::QExtRatio(b)) => match op {
            BinaryOp::Add => Ok(Value::QExtRatio(a.checked_add(&b))),
            BinaryOp::Sub => Ok(Value::QExtRatio(a.checked_sub(&b))),
            BinaryOp::Mul => Ok(Value::QExtRatio(a.checked_mul(&b))),
            BinaryOp::Div => a
                .checked_div(&b)
                .map(Value::QExtRatio)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::Min => Ok(Value::QExtRatio(if a <= b { a } else { b })),
            BinaryOp::Max => Ok(Value::QExtRatio(if a >= b { a } else { b })),
            BinaryOp::Lt => Ok(Value::QExtBool(a < b)),
            BinaryOp::LtEq => Ok(Value::QExtBool(a <= b)),
            BinaryOp::Gt => Ok(Value::QExtBool(a > b)),
            BinaryOp::GtEq => Ok(Value::QExtBool(a >= b)),
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "unsupported QExt<ratio> operator",
            )),
        },
        _ => Err(Diagnostic::at_line(
            DiagnosticCode::RuntimeError,
            line,
            "mismatched operand types at runtime",
        )),
    }
}

fn ord_to_t1(ord: std::cmp::Ordering) -> T1 {
    match ord {
        std::cmp::Ordering::Less => T1::NEG,
        std::cmp::Ordering::Equal => T1::ZERO,
        std::cmp::Ordering::Greater => T1::POS,
    }
}

fn eval_ord_only<T: Ord>(
    op: BinaryOp,
    a: T,
    b: T,
    wrap: fn(T) -> Value,
    line: usize,
) -> DecResult<Value> {
    match op {
        BinaryOp::Min => Ok(wrap(if a <= b { a } else { b })),
        BinaryOp::Max => Ok(wrap(if a >= b { a } else { b })),
        BinaryOp::Lt => Ok(Value::Bool(a < b)),
        BinaryOp::LtEq => Ok(Value::Bool(a <= b)),
        BinaryOp::Gt => Ok(Value::Bool(a > b)),
        BinaryOp::GtEq => Ok(Value::Bool(a >= b)),
        BinaryOp::Cmp3 => Ok(Value::T1(ord_to_t1(a.cmp(&b)))),
        _ => Err(Diagnostic::at_line(
            DiagnosticCode::RuntimeError,
            line,
            "unsupported operator on ordered non-numeric value",
        )),
    }
}

fn eval_recipient_stage_name(stage: RecipientOutcomeStage) -> &'static str {
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

fn eval_field(base: Value, field: &str, line: usize) -> DecResult<Value> {
    match base {
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                out.push(eval_field(it, field, line)?);
            }
            return Ok(Value::Array(out));
        }
        Value::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                out.push(eval_field(it, field, line)?);
            }
            return Ok(Value::List(out));
        }
        Value::Struct(sv) => {
            return sv.fields.get(field).cloned().ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("no field .{field} on struct {}", sv.name),
                )
            });
        }
        other => match (other, field) {
            (Value::SimResult(s), "model_status") => {
                let (variant, tag) = model_status_variant_and_tag(&s.model_status);
                Ok(Value::Enum(Box::new(EnumValue {
                    module: BUILTIN_MOD.to_string(),
                    name: "ModelStatus".to_string(),
                    variant: variant.to_string(),
                    tag,
                    payloads: Vec::new(),
                })))
            }
            (Value::SimResult(s), "model_status_bytes") => {
                Ok(Value::Bytes(s.model_status.into_bytes()))
            }
            (Value::SimResult(s), "chamber_name" | "chamber_name_bytes") => {
                Ok(Value::Bytes(s.chamber_name.into_bytes()))
            }
            (Value::SimResult(s), "final_state_hash" | "final_state_hash_bytes") => {
                Ok(Value::Bytes(s.final_state_hash.into_bytes()))
            }
            (Value::SimResult(s), "final_state_sha256") => decode_hex_32(&s.final_state_hash)
                .map(Value::Bytes)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e)),
            (Value::SimResult(s), "boundary_bytes") => Ok(Value::Bytes(
                s.final_snapshot.space.boundary_descriptor().into_bytes(),
            )),
            (Value::Box3(b), "boundary_bytes") => {
                Ok(Value::Bytes(b.boundary_descriptor().into_bytes()))
            }
            (Value::SimResult(s), "ticks") => Ok(Value::Int(BigInt::from_u64(s.ticks))),
            (Value::SimResult(s), "carrier_count") => {
                Ok(Value::Int(BigInt::from_u64(s.carrier_count as u64)))
            }
            (Value::SimResult(s), "shedding_event_count") => {
                Ok(Value::Int(BigInt::from_u64(s.shedding_event_count as u64)))
            }
            (Value::SimResult(s), "wavelet_event_count") => {
                Ok(Value::Int(BigInt::from_u64(s.wavelet_event_count as u64)))
            }
            (Value::SimResult(s), "boundary_crossings") => {
                Ok(Value::Int(BigInt::from_u64(s.boundary_crossings)))
            }
            (Value::SimResult(s), "total_returns") => {
                Ok(Value::Int(BigInt::from_u64(s.blind_motif_spectrum.total_returns)))
            }
            (Value::SimResult(s), "total_hinges") => {
                Ok(Value::Int(BigInt::from_u64(s.blind_motif_spectrum.total_hinges)))
            }
            (Value::SimResult(s), "total_shedding_events") => Ok(Value::Int(BigInt::from_u64(
                s.blind_motif_spectrum.total_shedding_events,
            ))),
            (Value::SimResult(s), "final_snapshot") => {
                Ok(Value::ChamberView(Box::new(s.final_snapshot)))
            }

            (Value::ChamberView(snap), "model_status") => {
                let (variant, tag) = model_status_variant_and_tag(&snap.model_status);
                Ok(Value::Enum(Box::new(EnumValue {
                    module: BUILTIN_MOD.to_string(),
                    name: "ModelStatus".to_string(),
                    variant: variant.to_string(),
                    tag,
                    payloads: Vec::new(),
                })))
            }
            (Value::ChamberView(snap), "model_status_bytes") => {
                Ok(Value::Bytes(snap.model_status.into_bytes()))
            }
            (Value::ChamberView(snap), "chamber_name" | "chamber_name_bytes") => {
                Ok(Value::Bytes(snap.chamber_name.into_bytes()))
            }
            (Value::ChamberView(snap), "tick") => Ok(Value::Int(BigInt::from_u64(snap.tick))),
            (Value::ChamberView(snap), "carrier_count") => {
                Ok(Value::Int(BigInt::from_u64(snap.carriers.len() as u64)))
            }
            (Value::ChamberView(snap), "boundary_crossings") => {
                Ok(Value::Int(BigInt::from_u64(snap.boundary_crossings)))
            }
            (Value::ChamberView(snap), "shedding_count") => Ok(Value::Int(BigInt::from_u64(
                snap.shedding_events_this_tick.len() as u64,
            ))),
            (Value::ChamberView(snap), "wavelet_count") => Ok(Value::Int(BigInt::from_u64(
                snap.wavelet_events_this_tick.len() as u64,
            ))),
            (Value::ChamberView(snap), "clock_count") => {
                Ok(Value::Int(BigInt::from_u64(snap.clocks.len() as u64)))
            }
            (Value::ChamberView(snap), "carriers") => Ok(Value::List(
                snap.carriers
                    .iter()
                    .map(|c| Value::Int(BigInt::from_u64(c.id)))
                    .collect(),
            )),

            (Value::RecipientOutcome(o), "stage") => {
                let (variant, tag) = recipient_stage_variant_and_tag(o.stage);
                Ok(Value::Enum(Box::new(EnumValue {
                    module: BUILTIN_MOD.to_string(),
                    name: "RecipientStage".to_string(),
                    variant: variant.to_string(),
                    tag,
                    payloads: Vec::new(),
                })))
            }
            (Value::RecipientOutcome(o), "stage_bytes") => Ok(Value::Bytes(
                eval_recipient_stage_name(o.stage).as_bytes().to_vec(),
            )),
            (Value::RecipientOutcome(o), "closure_phase_delta") => {
                Ok(Value::Int(BigInt::from_u64(o.closure_phase_delta as u64)))
            }
            (Value::RecipientOutcome(o), "reclosure_bip_delta") => {
                Ok(Value::Int(BigInt::from_u64(o.reclosure_bip_delta as u64)))
            }
            (Value::RecipientOutcome(o), "blocks_later_seating") => {
                Ok(Value::Bool(o.blocks_later_seating))
            }

            (Value::CadenceAssociation(a), "is_cadence_ratio") => Ok(Value::Bool(a.is_cadence_ratio)),
            (Value::CadenceAssociation(a), "wavelet_id" | "wavelet_id_bytes") => {
                Ok(Value::Bytes(a.wavelet_id.into_bytes()))
            }
            (Value::CadenceAssociation(a), "reference_id" | "reference_id_bytes") => {
                Ok(Value::Bytes(a.reference_id.into_bytes()))
            }

            (Value::Support(s), "is_yro") => Ok(Value::Bool(s.is_yro())),
            (Value::Support(s), "is_closure_certificate") => {
                Ok(Value::Bool(s.is_closure_certificate()))
            }
            (Value::Addr8(a), "is_first_anchor") => Ok(Value::Bool(a.is_first_anchor())),
            (Value::Window(u, _), "start") => Ok(Value::Int(BigInt::from_i64(u))),
            (Value::Window(_, v), "end") => Ok(Value::Int(BigInt::from_i64(v))),
            (Value::Window(u, v), "len") => {
                Ok(Value::Int(BigInt::from_i128((v as i128) - (u as i128))))
            }
            (Value::Ratio(r), "num") => Ok(Value::Int(r.num().clone())),
            (Value::Ratio(r), "den") => Ok(Value::Int(r.den().clone())),

            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!("unsupported field .{field}"),
            )),
        },
    }
}

fn eval_mux(role: Value, on_neg: Value, on_zero: Value, on_pos: Value, line: usize) -> DecResult<Value> {
    match role {
        Value::T1(t) => Ok(match t.value() {
            -1 => on_neg,
            0 => on_zero,
            1 => on_pos,
            _ => unreachable!(),
        }),
        Value::Array(rs) => {
            let n = rs.len();
            let mut out = Vec::with_capacity(n);
            for (i, r) in rs.into_iter().enumerate() {
                let bn = index_or_scalar(&on_neg, i);
                let bz = index_or_scalar(&on_zero, i);
                let bp = index_or_scalar(&on_pos, i);
                out.push(eval_mux(r, bn, bz, bp, line)?);
            }
            Ok(Value::Array(out))
        }
        Value::List(rs) => {
            let n = rs.len();
            let mut out = Vec::with_capacity(n);
            for (i, r) in rs.into_iter().enumerate() {
                let bn = index_or_scalar(&on_neg, i);
                let bz = index_or_scalar(&on_zero, i);
                let bp = index_or_scalar(&on_pos, i);
                out.push(eval_mux(r, bn, bz, bp, line)?);
            }
            Ok(Value::List(out))
        }
        _ => Err(Diagnostic::at_line(
            DiagnosticCode::RuntimeError,
            line,
            "mux expects t1 selector",
        )),
    }
}

fn eval_select(cond: Value, on_true: Value, on_false: Value, line: usize) -> DecResult<Value> {
    match cond {
        Value::Bool(b) | Value::QExtBool(b) => Ok(if b { on_true } else { on_false }),
        Value::Array(cs) => {
            let n = cs.len();
            let mut out = Vec::with_capacity(n);
            for (i, c) in cs.into_iter().enumerate() {
                let bt = index_or_scalar(&on_true, i);
                let bf = index_or_scalar(&on_false, i);
                out.push(eval_select(c, bt, bf, line)?);
            }
            Ok(Value::Array(out))
        }
        Value::List(cs) => {
            let n = cs.len();
            let mut out = Vec::with_capacity(n);
            for (i, c) in cs.into_iter().enumerate() {
                let bt = index_or_scalar(&on_true, i);
                let bf = index_or_scalar(&on_false, i);
                out.push(eval_select(c, bt, bf, line)?);
            }
            Ok(Value::List(out))
        }
        _ => Err(Diagnostic::at_line(
            DiagnosticCode::RuntimeError,
            line,
            "select expects bool condition",
        )),
    }
}

fn eval_gate(cond: Value, val: Value, line: usize) -> DecResult<Value> {
    match (cond, val) {
        (Value::Bool(b), v) => {
            if b {
                Ok(v)
            } else {
                zero_like(&v, line)
            }
        }
        (Value::Array(cs), v) => {
            let mut out = Vec::with_capacity(cs.len());
            for (i, c) in cs.into_iter().enumerate() {
                let vi = index_or_scalar(&v, i);
                out.push(eval_gate(c, vi, line)?);
            }
            Ok(Value::Array(out))
        }
        (Value::List(cs), v) => {
            let mut out = Vec::with_capacity(cs.len());
            for (i, c) in cs.into_iter().enumerate() {
                let vi = index_or_scalar(&v, i);
                out.push(eval_gate(c, vi, line)?);
            }
            Ok(Value::List(out))
        }
        _ => Err(Diagnostic::at_line(
            DiagnosticCode::RuntimeError,
            line,
            "gate expects bool condition",
        )),
    }
}

fn zero_like(v: &Value, line: usize) -> DecResult<Value> {
    match v {
        Value::Int(_) => Ok(Value::Int(BigInt::zero())),
        Value::T1(_) => Ok(Value::T1(T1::ZERO)),
        Value::Ratio(_) => Ok(Value::Ratio(Ratio::zero())),
        Value::Array(xs) => {
            let mut out = Vec::with_capacity(xs.len());
            for x in xs {
                out.push(zero_like(x, line)?);
            }
            Ok(Value::Array(out))
        }
        Value::List(xs) => {
            let mut out = Vec::with_capacity(xs.len());
            for x in xs {
                out.push(zero_like(x, line)?);
            }
            Ok(Value::List(out))
        }
        _ => Err(Diagnostic::at_line(
            DiagnosticCode::RuntimeError,
            line,
            "gate requires numeric value",
        )),
    }
}

fn index_or_scalar(v: &Value, idx: usize) -> Value {
    match v {
        Value::Array(xs) | Value::List(xs) => xs[idx].clone(),
        other => other.clone(),
    }
}

fn bytes_to_utf8_eval(bs: &[u8], ctx_name: &str, line: usize) -> DecResult<String> {
    std::str::from_utf8(bs).map(str::to_string).map_err(|_| {
        Diagnostic::at_line(
            DiagnosticCode::RuntimeError,
            line,
            format!("{ctx_name} input is not valid UTF-8"),
        )
    })
}

fn cmp_clamp_ord(a: &Value, b: &Value, line: usize) -> DecResult<std::cmp::Ordering> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),
        (Value::B8(x), Value::B8(y)) => Ok(x.cmp(y)),
        (Value::B64(x), Value::B64(y)) => Ok(x.cmp(y)),
        (Value::T1(x), Value::T1(y)) => Ok(x.cmp(y)),
        (Value::Ratio(x), Value::Ratio(y)) => Ok(x.cmp(y)),
        _ => Err(Diagnostic::at_line(
            DiagnosticCode::RuntimeError,
            line,
            "clamp requires matching ordered scalar operands",
        )),
    }
}

fn eval_builtin(name: &str, args: Vec<Value>, line: usize) -> DecResult<Value> {
    let err = |e: aod_core::error::CoreError| {
        Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())
    };
    if name == "array" {
        if args.is_empty() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "array(...) requires at least 1 element",
            ));
        }
        return Ok(Value::Array(args));
    }

    match (name, args.as_slice()) {
        // --- Core constructors & exact conversions ---
        ("t1", [Value::Int(n)]) => T1::from_bigint(n).map(Value::T1).map_err(err),
        ("t1", [Value::T1(t)]) => Ok(Value::T1(*t)),
        ("q4", [Value::Int(n)]) => Q4::from_bigint(n).map(Value::Q4).map_err(err),
        ("q4", [Value::B8(b)]) => Q4::new(b.value()).map(Value::Q4).map_err(err),
        ("q4", [Value::Q4(q)]) => Ok(Value::Q4(*q)),
        ("b8", [Value::Int(n)]) => B8::from_int(n).map(Value::B8).map_err(err),
        ("b8", [Value::B8(b)]) => Ok(Value::B8(*b)),
        ("b8", [Value::Bool(b)]) => Ok(Value::B8(B8(u8::from(*b)))),
        ("b8", [Value::Q4(q)]) => Ok(Value::B8(B8(q.value()))),
        ("b8", [Value::B64(w)]) => u8::try_from(w.value())
            .map(|u| Value::B8(B8(u)))
            .map_err(|_| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("b8 overflow: {}", w.value()),
                )
            }),
        ("b64", [Value::Int(n)]) => B64::from_trit_int(n).map(Value::B64).map_err(err),
        ("b64", [Value::B64(b)]) => Ok(Value::B64(*b)),
        ("b64", [Value::B8(b)]) => Ok(Value::B64(B64::new(b.value() as u64))),
        ("b64", [Value::Q4(q)]) => Ok(Value::B64(B64::new(q.value() as u64))),
        ("b64_bits", [Value::Int(n)]) => B64::from_unsigned_int(n).map(Value::B64).map_err(err),
        ("u64_int", [Value::B64(b)]) => Ok(Value::Int(BigInt::from_u64(b.value()))),
        ("int", [Value::Int(n)]) => Ok(Value::Int(n.clone())),
        ("int", [Value::T1(v)]) => Ok(Value::Int(BigInt::from_i64(v.value() as i64))),
        ("int", [Value::Q4(v)]) => Ok(Value::Int(BigInt::from_u64(v.value() as u64))),
        ("int", [Value::B8(v)]) => Ok(Value::Int(BigInt::from_u64(v.value() as u64))),
        ("int", [Value::B64(v)]) => v.to_trit_int().map(Value::Int).map_err(err),
        ("int", [Value::Ratio(r)]) => r.to_int_exact().map(Value::Int).map_err(err),
        ("ratio", [Value::Int(a), Value::Int(b)]) => {
            Ratio::new(a.clone(), b.clone()).map(Value::Ratio).map_err(err)
        }
        ("ratio", [Value::Int(a)]) => Ok(Value::Ratio(Ratio::from_int(a.clone()))),
        ("ratio", [Value::T1(t)]) => Ok(Value::Ratio(Ratio::from_int(BigInt::from_i64(
            t.value() as i64,
        )))),
        ("ratio", [Value::Ratio(r)]) => Ok(Value::Ratio(r.clone())),
        ("pack", [Value::Int(n)]) => B64::from_trit_int(n)
            .map(|b| Value::Int(BigInt::from_u64(b.value())))
            .map_err(err),
        ("unpack", [Value::Int(n)]) => {
            let bits = n.to_u64().ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("unpack(int): bit pattern {n} outside u64 domain"),
                )
            })?;
            B64::new(bits).to_trit_int().map(Value::Int).map_err(err)
        }
        ("unpack_t9", [Value::Int(n)]) => {
            let bits = n.to_u64().ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("unpack_t9(int): bit pattern {n} outside u64 domain"),
                )
            })?;
            exact_unpack_t9_bits(bits)
                .map(Value::Int)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e))
        }
        ("unpack_t9", [Value::B64(b)]) => exact_unpack_t9_bits(b.value())
            .map(Value::Int)
            .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e)),
        ("bytes", [Value::Bytes(bs)]) => Ok(Value::Bytes(bs.clone())),
        ("bytes", [Value::Addr8(a)]) => Ok(Value::Bytes(a.render().into_bytes())),
        ("bytes", [Value::B64(b)]) => Ok(Value::Bytes(b.value().to_le_bytes().to_vec())),
        ("bytes", [Value::Array(items) | Value::List(items)]) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                match it {
                    Value::B8(b) => out.push(b.value()),
                    _ => {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::RuntimeError,
                            line,
                            "bytes(...) sequence expects b8 elements",
                        ));
                    }
                }
            }
            Ok(Value::Bytes(out))
        }
        ("u16_le_bytes", [Value::Int(n)]) => {
            let u = n.to_u64().filter(|&v| v <= u16::MAX as u64).ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("u16_le_bytes out of 0..=65535 range: {n}"),
                )
            })? as u16;
            Ok(Value::Bytes(u.to_le_bytes().to_vec()))
        }
        ("u32_le_bytes", [Value::Int(n)]) => {
            let u = n.to_u64().filter(|&v| v <= u32::MAX as u64).ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("u32_le_bytes out of 0..=4294967295 range: {n}"),
                )
            })? as u32;
            Ok(Value::Bytes(u.to_le_bytes().to_vec()))
        }
        ("i32_le_bytes", [Value::Int(n)]) => {
            let v = n
                .to_i64()
                .and_then(|x| i32::try_from(x).ok())
                .ok_or_else(|| {
                    Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("i32_le_bytes out of i32 range: {n}"),
                    )
                })?;
            Ok(Value::Bytes(v.to_le_bytes().to_vec()))
        }
        ("i64_le_bytes", [Value::Int(n)]) => {
            let v = n.to_i64().ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("i64_le_bytes out of i64 range: {n}"),
                )
            })?;
            Ok(Value::Bytes(v.to_le_bytes().to_vec()))
        }
        ("b64_le_bytes", [Value::B64(b)]) => Ok(Value::Bytes(b.value().to_le_bytes().to_vec())),
        ("push_byte", [Value::Bytes(bs), Value::B8(b)]) => {
            let mut out = bs.clone();
            out.push(b.value());
            Ok(Value::Bytes(out))
        }
        ("concat_bytes", [Value::Bytes(a), Value::Bytes(b)]) => {
            let mut out = Vec::with_capacity(a.len() + b.len());
            out.extend_from_slice(a);
            out.extend_from_slice(b);
            Ok(Value::Bytes(out))
        }
        ("slice_bytes", [Value::Bytes(bs), Value::Int(s), Value::Int(e)]) => {
            let si = extract_i64(Value::Int(s.clone()), line)?;
            let ei = extract_i64(Value::Int(e.clone()), line)?;
            if si < 0 || ei < si || (ei as usize) > bs.len() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("slice_bytes({si}..{ei}) out of bounds for len {}", bs.len()),
                ));
            }
            Ok(Value::Bytes(bs[si as usize..ei as usize].to_vec()))
        }
        ("sha256_bytes", [Value::Bytes(bs)]) => {
            Ok(Value::Bytes(Sha256::digest(bs).to_vec()))
        }
        ("hex_bytes", [Value::Bytes(bs)]) => {
            const HEX: &[u8; 16] = b"0123456789abcdef";
            let mut out = Vec::with_capacity(bs.len() * 2);
            for &b in bs {
                out.push(HEX[(b >> 4) as usize]);
                out.push(HEX[(b & 0x0F) as usize]);
            }
            Ok(Value::Bytes(out))
        }
        ("pack_ratio", [Value::Ratio(r)]) => {
            let ni = r.num().to_i64().ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "ratio exceeds compact 1-word 31-trit budget",
                )
            })?;
            let di = r.den().to_i64().ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "ratio exceeds compact 1-word 31-trit budget",
                )
            })?;
            let packed = pack_compact_ratio_u64(ni, di).ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "ratio exceeds compact 1-word [num][11][den] 31-trit capacity",
                )
            })?;
            Ok(Value::B64(B64::new(packed)))
        }
        ("unpack_ratio", [Value::B64(b)]) => {
            let (n, d) = unpack_compact_ratio_u64(b.value()).ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "invalid compact 1-word [num][11][den] ratio encoding",
                )
            })?;
            Ratio::from_i64(n, d).map(Value::Ratio).map_err(err)
        }

        // --- Algebra, ternary, & bit queries ---
        ("num", [Value::Ratio(r)]) => Ok(Value::Int(r.num().clone())),
        ("den", [Value::Ratio(r)]) => Ok(Value::Int(r.den().clone())),
        ("neg", [v]) => eval_unary(UnaryOp::Neg, v.clone(), line),
        ("abs", [Value::Int(n)]) => Ok(Value::Int(n.abs())),
        ("abs", [Value::T1(t)]) => Ok(Value::T1(if t.value() < 0 {
            T1::POS
        } else {
            *t
        })),
        ("abs", [Value::Ratio(r)]) => Ok(Value::Ratio(r.abs())),
        ("abs", [Value::QExtRatio(q)]) => Ok(Value::QExtRatio(
            if *q < QExtRatio::from_native_frozen(Ratio::zero()) {
                q.neg()
            } else {
                q.clone()
            },
        )),
        ("sgn" | "signum", [Value::Int(n)]) => T1::new(n.signum()).map(Value::T1).map_err(err),
        ("sgn" | "signum", [Value::Ratio(r)]) => {
            T1::new(r.num().signum()).map(Value::T1).map_err(err)
        }
        ("sgn" | "signum", [Value::T1(t)]) => Ok(Value::T1(*t)),
        ("min", [a, b]) => eval_binary(BinaryOp::Min, a.clone(), b.clone(), line),
        ("max", [a, b]) => eval_binary(BinaryOp::Max, a.clone(), b.clone(), line),
        ("clamp", [x, lo, hi]) => {
            if cmp_clamp_ord(lo, hi, line)?.is_gt() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!(
                        "clamp(x, lo, hi) requires lo <= hi, got {} > {}",
                        lo.render(),
                        hi.render()
                    ),
                ));
            }
            if cmp_clamp_ord(x, lo, line)?.is_lt() {
                Ok(lo.clone())
            } else if cmp_clamp_ord(x, hi, line)?.is_gt() {
                Ok(hi.clone())
            } else {
                Ok(x.clone())
            }
        }
        ("pow", [Value::Int(base), Value::Int(exp)]) => checked_trit_pow(base, exp)
            .map(Value::Int)
            .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e)),
        ("pow", [Value::Ratio(base), Value::Int(exp)]) => {
            if !exp.is_negative() {
                let num = checked_trit_pow(base.num(), exp)
                    .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e))?;
                let den = checked_trit_pow(base.den(), exp)
                    .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e))?;
                Ratio::new(num, den).map(Value::Ratio).map_err(err)
            } else {
                let neg_exp = -exp.clone();
                let num = checked_trit_pow(base.den(), &neg_exp)
                    .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e))?;
                let den = checked_trit_pow(base.num(), &neg_exp)
                    .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e))?;
                Ratio::new(num, den).map(Value::Ratio).map_err(err)
            }
        }
        ("gcd", [Value::Int(a), Value::Int(b)]) => Ok(Value::Int(a.gcd(b))),
        ("lcm", [Value::Int(a), Value::Int(b)]) => {
            if a.is_zero() || b.is_zero() {
                Ok(Value::Int(BigInt::zero()))
            } else {
                let g = a.gcd(b);
                let q = a.abs().div_floor(&g).map_err(err)?;
                Ok(Value::Int(&q * &b.abs()))
            }
        }
        ("trit_shl", [Value::Int(x), Value::Int(k)]) => {
            let ku = k.to_u64().and_then(|u| u32::try_from(u).ok()).ok_or_else(|| {
                Diagnostic::at_line(DiagnosticCode::RuntimeError, line, "trit_shl requires k >= 0")
            })?;
            let p3 = BigInt::from_i64(3).pow(ku);
            Ok(Value::Int(x * &p3))
        }
        ("trit_shr", [Value::Int(x), Value::Int(k)]) => {
            let ku = k.to_u64().and_then(|u| u32::try_from(u).ok()).ok_or_else(|| {
                Diagnostic::at_line(DiagnosticCode::RuntimeError, line, "trit_shr requires k >= 0")
            })?;
            let p3 = BigInt::from_i64(3).pow(ku);
            x.div_floor(&p3).map(Value::Int).map_err(err)
        }
        ("superpose_cancel", [Value::T1(a), Value::T1(b)]) => {
            Ok(Value::T1(a.superpose_cancel(*b)))
        }
        ("superpose_cancel", [Value::Int(a), Value::Int(b)]) => {
            Ok(Value::Int(a.superpose_cancel(b)))
        }
        ("bitnot", [v]) => eval_unary(UnaryOp::BitNot, v.clone(), line),
        ("popcnt", [Value::B64(b)]) => Ok(Value::Int(BigInt::from_u64(b.popcnt() as u64))),
        ("popcnt", [Value::B8(b)]) => {
            Ok(Value::Int(BigInt::from_u64(b.value().count_ones() as u64)))
        }
        ("popcnt", [Value::Q4(q)]) => {
            Ok(Value::Int(BigInt::from_u64(q.value().count_ones() as u64)))
        }
        ("popcnt_xor", [Value::Q4(a), Value::Q4(b)]) => Ok(Value::Int(BigInt::from_u64(
            (a.value() ^ b.value()).count_ones() as u64,
        ))),
        ("is_hamming1", [Value::Q4(a), Value::Q4(b)]) => {
            Ok(Value::Bool((a.value() ^ b.value()).count_ones() == 1))
        }
        ("verify_sheddic_route" | "audit_sheddic_route", [Value::Int(xs), Value::B8(enc)]) => {
            Ok(Value::Bool(exact_verify_sheddic_route(xs, enc.value())))
        }
        ("verify_sheddic_route" | "audit_sheddic_route", [Value::Int(xs), Value::Int(enc)]) => {
            Ok(Value::Bool(exact_verify_sheddic_route_int(xs, enc)))
        }
        ("verify_frg_cross_return", [Value::Int(t0), Value::Int(tx), Value::Int(cx)]) => {
            Ok(Value::Bool(exact_verify_frg_cross_return(t0, tx, cx)))
        }

        // --- Branchless selection & sequence combinators ---
        ("mux", [role, on_neg, on_zero, on_pos]) => {
            eval_mux(role.clone(), on_neg.clone(), on_zero.clone(), on_pos.clone(), line)
        }
        ("select", [cond, on_true, on_false]) => {
            eval_select(cond.clone(), on_true.clone(), on_false.clone(), line)
        }
        ("gate", [cond, val]) => eval_gate(cond.clone(), val.clone(), line),
        ("argmin" | "argmax", [Value::Array(items) | Value::List(items)]) => {
            if items.is_empty() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("{name} requires non-empty sequence"),
                ));
            }
            let want_max = name == "argmax";
            let mut best_idx = 0usize;
            let mut best_key = MapKey::from_value(&items[0], line)?;
            for (idx, it) in items.iter().enumerate().skip(1) {
                let k = MapKey::from_value(it, line)?;
                let take = if want_max {
                    k > best_key
                } else {
                    k < best_key
                };
                if take {
                    best_key = k;
                    best_idx = idx;
                }
            }
            Ok(Value::Int(BigInt::from_u64(best_idx as u64)))
        }
        ("range_seq", [Value::Int(a), Value::Int(b)]) => {
            let ai = extract_i64(Value::Int(a.clone()), line)?;
            let bi = extract_i64(Value::Int(b.clone()), line)?;
            materialize_range_list(ai, bi, line)
        }
        ("push" | "list_push", [Value::Array(xs) | Value::List(xs), elem]) => {
            let mut out = xs.clone();
            out.push(elem.clone());
            Ok(Value::List(out))
        }
        (
            "concat" | "concat_seq",
            [Value::Array(a) | Value::List(a), Value::Array(b) | Value::List(b)],
        ) => {
            let mut out = Vec::with_capacity(a.len() + b.len());
            out.extend_from_slice(a);
            out.extend_from_slice(b);
            Ok(Value::List(out))
        }
        (
            "slice" | "slice_seq",
            [Value::Array(xs) | Value::List(xs), Value::Int(s), Value::Int(e)],
        ) => {
            let si = extract_i64(Value::Int(s.clone()), line)?;
            let ei = extract_i64(Value::Int(e.clone()), line)?;
            if si < 0 || ei < si || (ei as usize) > xs.len() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("{name}({si}..{ei}) out of bounds for len {}", xs.len()),
                ));
            }
            Ok(Value::List(xs[si as usize..ei as usize].to_vec()))
        }
        ("array_set", [Value::Array(xs), Value::Int(idx), val]) => {
            let ii = extract_i64(Value::Int(idx.clone()), line)?;
            if ii < 0 || (ii as usize) >= xs.len() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("array index {ii} out of bounds"),
                ));
            }
            let mut out = xs.clone();
            out[ii as usize] = val.clone();
            Ok(Value::Array(out))
        }
        ("list_set", [Value::Array(xs) | Value::List(xs), Value::Int(idx), val]) => {
            let ii = extract_i64(Value::Int(idx.clone()), line)?;
            if ii < 0 || (ii as usize) >= xs.len() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("list index {ii} out of bounds"),
                ));
            }
            let mut out = xs.clone();
            out[ii as usize] = val.clone();
            Ok(Value::List(out))
        }
        ("set_at", [seq, Value::Int(idx), val]) => {
            let ii = extract_i64(Value::Int(idx.clone()), line)?;
            match seq {
                Value::Array(xs) => {
                    if ii < 0 || (ii as usize) >= xs.len() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::RuntimeError,
                            line,
                            format!("set_at index {ii} out of bounds for len {}", xs.len()),
                        ));
                    }
                    let mut out = xs.clone();
                    out[ii as usize] = val.clone();
                    Ok(Value::Array(out))
                }
                Value::List(xs) => {
                    if ii < 0 || (ii as usize) >= xs.len() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::RuntimeError,
                            line,
                            format!("set_at index {ii} out of bounds for len {}", xs.len()),
                        ));
                    }
                    let mut out = xs.clone();
                    out[ii as usize] = val.clone();
                    Ok(Value::List(out))
                }
                _ => Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "set_at expects sequence",
                )),
            }
        }
        ("zip_seq", [a, b]) => {
            let xs = eval_afc_seq_values(a.clone(), "zip_seq", line)?;
            let ys = eval_afc_seq_values(b.clone(), "zip_seq", line)?;
            if xs.len() != ys.len() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("zip_seq length mismatch: {} != {}", xs.len(), ys.len()),
                ));
            }
            let out: Vec<Value> = xs
                .into_iter()
                .zip(ys)
                .map(|(x, y)| Value::Tuple(vec![x, y]))
                .collect();
            Ok(Value::List(out))
        }
        ("map_put", [Value::Map(m), k, v]) => {
            let mk = MapKey::from_value(k, line)?;
            let mut out = (**m).clone();
            out.insert(mk, v.clone());
            Ok(Value::Map(Box::new(out)))
        }
        ("map_remove", [Value::Map(m), k]) => {
            let mk = MapKey::from_value(k, line)?;
            let mut out = (**m).clone();
            out.remove(&mk);
            Ok(Value::Map(Box::new(out)))
        }
        ("map_get_or", [Value::Map(m), k, default_val]) => {
            let mk = MapKey::from_value(k, line)?;
            Ok(m.get(&mk).cloned().unwrap_or_else(|| default_val.clone()))
        }
        ("map_keys", [Value::Map(m)]) => {
            let keys: Vec<Value> = m.keys().map(MapKey::to_value).collect();
            Ok(Value::List(keys))
        }
        ("map_entries", [Value::Map(m)]) => {
            let entries: Vec<Value> = m
                .iter()
                .map(|(k, v)| Value::Tuple(vec![k.to_value(), v.clone()]))
                .collect();
            Ok(Value::List(entries))
        }

        // --- Witness primitives ---
        ("trace", [Value::Bytes(sc_b), Value::Bytes(lin_b), Value::Q4(st)]) => {
            let sc = bytes_to_utf8_eval(sc_b, "trace(bytes, bytes, q4)", line)?;
            let lin = bytes_to_utf8_eval(lin_b, "trace(bytes, bytes, q4)", line)?;
            Trace::new(sc, lin, *st)
                .map(|t| Value::Trace(Box::new(t)))
                .map_err(err)
        }
        ("step", [Value::Trace(t), Value::Q4(q), Value::T1(o)]) => {
            t.step(*q, *o).map(|t| Value::Trace(Box::new(t))).map_err(err)
        }
        ("hinge", [Value::Trace(t)]) => t.hinge().map(|t| Value::Trace(Box::new(t))).map_err(err),
        ("dwell", [Value::Trace(t)]) => t.dwell().map(|t| Value::Trace(Box::new(t))).map_err(err),
        ("close", [Value::Trace(t)]) => t.close().map(|t| Value::Trace(Box::new(t))).map_err(err),
        ("phases", [Value::Trace(t)]) => t
            .phases()
            .map(|p| Value::Int(BigInt::from_u64(p as u64)))
            .map_err(err),
        ("beats", [Value::Trace(t)]) => t
            .beats()
            .map(|b| Value::Int(BigInt::from_u64(b as u64)))
            .map_err(err),
        ("rate", [Value::Trace(t)]) => t.rate().map(Value::Ratio).map_err(err),
        ("fold", [Value::Trace(a), Value::Trace(b), Value::Bytes(sc_b), Value::Bytes(lin_b)]) => {
            let sc = bytes_to_utf8_eval(sc_b, "fold(..., bytes, bytes)", line)?;
            let lin = bytes_to_utf8_eval(lin_b, "fold(..., bytes, bytes)", line)?;
            Fusion::fold(a, b, sc, lin)
                .map(|f| Value::Fusion(Box::new(f)))
                .map_err(err)
        }
        ("outer", [Value::Fusion(f), Value::Q4(st)]) => {
            f.outer(*st).map(|t| Value::Trace(Box::new(t))).map_err(err)
        }
        ("fuse", [Value::Fusion(f), Value::Trace(t)]) => {
            f.fuse(t).map(|t| Value::Trace(Box::new(t))).map_err(err)
        }
        ("coupling", [Value::Trace(a), Value::Trace(b), Value::Bytes(wb)]) => {
            let w = bytes_to_utf8_eval(wb, "coupling(..., bytes)", line)?;
            Coupling::new(a, b, w)
                .map(|c| Value::Coupling(Box::new(c)))
                .map_err(err)
        }
        ("contact", [Value::Coupling(c), Value::Int(a), Value::Int(b)]) => {
            let au = a.to_u64().ok_or_else(|| {
                Diagnostic::at_line(DiagnosticCode::RuntimeError, line, "invalid contact ordinal")
            })? as usize;
            let bu = b.to_u64().ok_or_else(|| {
                Diagnostic::at_line(DiagnosticCode::RuntimeError, line, "invalid contact ordinal")
            })? as usize;
            c.contact(au, bu)
                .map(|c| Value::Coupling(Box::new(c)))
                .map_err(err)
        }
        ("cadence", [Value::Coupling(c)]) => c.cadence().map(Value::Ratio).map_err(err),
        ("lag", [Value::Coupling(c)]) => c.lag().map(Value::Int).map_err(err),
        ("drift", [Value::Coupling(c)]) => c.drift().map(Value::Int).map_err(err),
        ("probability", [Value::Int(neg), Value::Int(h), Value::Int(pos), Value::T1(role)]) => {
            probability(neg, h, pos, *role).map(Value::Ratio).map_err(err)
        }

        // --- Exact AOD / support / ledger / M3 wavelet helpers ---
        ("addr8", [Value::Bytes(bs)]) => {
            let s = bytes_to_utf8_eval(bs, "addr8(bytes)", line)?;
            Addr8::parse(&s).map(Value::Addr8).map_err(err)
        }
        ("support", [Value::Bytes(bs)]) => {
            let s = bytes_to_utf8_eval(bs, "support(bytes)", line)?;
            SupportForm::parse(&s).map(Value::Support).map_err(err)
        }
        ("support", [Value::Int(p), Value::Int(q), Value::Int(len), Value::Bool(yro)]) => {
            SupportForm::new(p.clone(), q.clone(), len.clone(), *yro)
                .map(Value::Support)
                .map_err(err)
        }
        ("support_pi", [Value::Support(sf)]) => sf.pi().map(Value::Int).map_err(err),
        ("support_rd", [Value::Support(sf), Value::Int(h)]) => {
            sf.rd(h).map(Value::Int).map_err(err)
        }
        (
            "support_pressure",
            [Value::Support(sf), Value::Int(c3), Value::Int(h), Value::Int(w)],
        ) => sf.pressure(c3, h, w).map(Value::Int).map_err(err),
        (
            "support_overhang",
            [Value::Support(sf), Value::Int(c3), Value::Int(h), Value::Int(w)],
        ) => sf.overhang(c3, h, w).map(Value::Int).map_err(err),
        ("shedding_excess", [Value::Int(p_d), Value::Int(c_close)]) => {
            SupportForm::shedding_audit(p_d, c_close)
                .map(|a| Value::Int(a.x_shedding))
                .map_err(err)
        }
        ("sadar_promote", [Value::Int(c3), Value::Int(rho), Value::Ratio(att)]) => {
            SadarPromotion::promote(c3, rho, att.clone())
                .map(|r| Value::Ratio(r.sadar))
                .map_err(err)
        }
        ("boundary_of", [v]) => eval_unary(UnaryOp::Boundary, v.clone(), line),
        ("len", [Value::Bytes(bs)]) => Ok(Value::Int(BigInt::from_u64(bs.len() as u64))),
        ("len", [Value::Array(xs) | Value::List(xs)]) => {
            Ok(Value::Int(BigInt::from_u64(xs.len() as u64)))
        }
        ("len", [Value::Map(m)]) => Ok(Value::Int(BigInt::from_u64(m.len() as u64))),
        ("contains", [Value::Map(m), key_val]) => {
            let k = MapKey::from_value(key_val, line)?;
            Ok(Value::Bool(m.contains_key(&k)))
        }
        ("contains", [Value::Array(xs) | Value::List(xs), target]) => {
            Ok(Value::Bool(xs.iter().any(|x| values_equal(x, target))))
        }
        (
            "window_dedup_count",
            [
                Value::Bytes(lin_b),
                Value::Bytes(scope_b),
                Value::Array(cut_vals) | Value::List(cut_vals),
                Value::Array(win_vals) | Value::List(win_vals),
            ],
        ) => {
            let lin = bytes_to_utf8_eval(lin_b, "window_dedup_count(bytes, bytes, ...)", line)?;
            let scope =
                bytes_to_utf8_eval(scope_b, "window_dedup_count(bytes, bytes, ...)", line)?;
            let mut cuts = Vec::with_capacity(cut_vals.len());
            for cv in cut_vals {
                cuts.push(extract_i64(cv.clone(), line)?);
            }
            let (witnesses, reg) =
                declare_synthetic_lineage_registry(&lin, &scope, &cuts, 0, "native_cut", None)
                    .map_err(err)?;
            let pdesc = &witnesses[0].parent_descriptor;
            let mut wavelets = Vec::with_capacity(win_vals.len());
            for (idx, wv) in win_vals.iter().enumerate() {
                let (u, v) = match wv {
                    Value::Window(u, v) => (*u, *v),
                    _ => {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::RuntimeError,
                            line,
                            "window_dedup_count expects list of windows",
                        ));
                    }
                };
                let selected: Vec<_> = witnesses
                    .iter()
                    .filter(|w| w.completion_cut <= v)
                    .cloned()
                    .collect();
                let wl = Wavelet::new(WaveletSpec {
                    wavelet_id: &format!("w{idx}"),
                    parent_lineage: &lin,
                    explicit_parent: Some(pdesc),
                    ancestry: vec![format!("anc:{lin}"), format!("w{idx}")],
                    window: (u, v),
                    sigma_seat: SeatState::SeatedDuad,
                    sigma_role: WaveletRole::CoupledSegment,
                    records: vec![format!("rec:w{idx}")],
                    return_witnesses: selected,
                    signed_current: BigInt::zero(),
                    is_admitted_wave: false,
                    scope_id: &scope,
                    witnesses_complete: true,
                    scale: Some(0),
                    cut_coordinate: "native_cut",
                    scope_map: None,
                    registry: Some(&reg),
                })
                .map_err(err)?;
                wavelets.push(wl);
            }
            let count =
                deduplicated_return_count(&wavelets, Some(pdesc), None, &scope, None, Some(&reg))
                    .map_err(err)?;
            Ok(Value::Int(BigInt::from_u64(count as u64)))
        }
        (
            "native_cadence_ratio",
            [
                Value::Bytes(ida_b),
                Value::Int(q_a),
                Value::Bytes(idr_b),
                Value::Int(q_r),
                Value::Bool(coupled),
                Value::Int(contacts),
            ],
        ) => {
            let id_a = bytes_to_utf8_eval(ida_b, "native_cadence_ratio(bytes, ...)", line)?;
            let id_r = bytes_to_utf8_eval(idr_b, "native_cadence_ratio(bytes, ...)", line)?;
            let bc = contacts.to_u64().ok_or_else(|| {
                Diagnostic::at_line(DiagnosticCode::RuntimeError, line, "invalid bound_contacts")
            })? as usize;
            native_cadence_ratio(&id_a, q_a, &id_r, q_r, *coupled, bc)
                .map(Value::Ratio)
                .map_err(err)
        }
        (
            "single_contact_association",
            [
                Value::Bytes(wid_b),
                Value::Bytes(pid_b),
                Value::Bytes(rid_b),
                Value::Window(u, v),
                Value::Int(contacts),
            ],
        ) => {
            let wid = bytes_to_utf8_eval(wid_b, "single_contact_association(bytes, ...)", line)?;
            let pid = bytes_to_utf8_eval(pid_b, "single_contact_association(bytes, ...)", line)?;
            let rid = bytes_to_utf8_eval(rid_b, "single_contact_association(bytes, ...)", line)?;
            let bc = contacts.to_u64().ok_or_else(|| {
                Diagnostic::at_line(DiagnosticCode::RuntimeError, line, "invalid bound_contacts")
            })? as usize;
            SingleContactStageAssociation::new(wid, pid, rid, (*u, *v), bc)
                .map(|a| Value::CadenceAssociation(Box::new(a)))
                .map_err(err)
        }
        ("recipient_eval_demo", [Value::Int(emit_cut), Value::Int(start_idx), Value::Window(u, v)]) => {
            let ek = extract_i64(Value::Int(emit_cut.clone()), line)?;
            let sk = extract_i64(Value::Int(start_idx.clone()), line)?;
            let shed = ShedWitness::new(
                "W_parent",
                "dT_R",
                "shared_k",
                ek,
                ShedRoute::Exo,
                Ratio::from_i64(1, 2).map_err(err)?,
                "post_shed",
            )
            .map_err(err)?;
            let prior0 = AdmittedClosureRecord::admit_monon(
                M3ClosureTrace::minimal_closed("R_rec", 0, "B_rec", 0, Some(1)).map_err(err)?,
            )
            .map_err(err)?;
            let fresh = AdmittedClosureRecord::admit_monon(
                M3ClosureTrace::minimal_closed("R_rec", 1, "B_rec", 0, Some(sk)).map_err(err)?,
            )
            .map_err(err)?;
            let ctx = RecipientContext {
                recipient_lineage: "R_rec".to_string(),
                recipient_scope: "B_rec".to_string(),
                cut_coordinate: "shared_k".to_string(),
                closure_cut_coordinate: None,
                window: (*u, *v),
                coordinate_relation: None,
                prior_certified_returns: vec![prior0],
                seen_return_ids: BTreeSet::new(),
            };
            let mut events = BTreeMap::new();
            events.insert(
                RecipientStage::Delivered,
                StageEventWitness::new("d1", RecipientStage::Delivered, "B_rec", "shared_k", ek)
                    .map_err(err)?,
            );
            events.insert(
                RecipientStage::Accepted,
                StageEventWitness::new("a1", RecipientStage::Accepted, "B_rec", "shared_k", ek)
                    .map_err(err)?,
            );
            events.insert(
                RecipientStage::Seated,
                StageEventWitness::new("s1", RecipientStage::Seated, "B_rec", "shared_k", ek)
                    .map_err(err)?,
            );
            RecipientLattice::evaluate(
                &shed,
                &ctx,
                &[
                    RecipientStage::Emitted,
                    RecipientStage::Delivered,
                    RecipientStage::Accepted,
                    RecipientStage::Seated,
                    RecipientStage::Reclosed,
                ],
                Some(&fresh),
                Some(&events),
                None,
            )
            .map(|o| Value::RecipientOutcome(Box::new(o)))
            .map_err(err)
        }

        // --- Quarantined aod::qext builtins ---
        ("aod::qext::ext_decimal", [Value::Bytes(bs)]) => {
            let s = bytes_to_utf8_eval(bs, "ext_decimal(bytes)", line)?;
            parse_decimal_exact(&s).map(Value::QExtRatio).map_err(err)
        }
        ("aod::qext::qext_from_ratio", [Value::Ratio(r)]) => {
            Ok(Value::QExtRatio(QExtRatio::from_native_frozen(r.clone())))
        }
        ("aod::qext::qext_add", [Value::QExtRatio(a), Value::QExtRatio(b)]) => {
            Ok(Value::QExtRatio(a.checked_add(b)))
        }
        ("aod::qext::qext_sub", [Value::QExtRatio(a), Value::QExtRatio(b)]) => {
            Ok(Value::QExtRatio(a.checked_sub(b)))
        }
        ("aod::qext::qext_mul", [Value::QExtRatio(a), Value::QExtRatio(b)]) => {
            Ok(Value::QExtRatio(a.checked_mul(b)))
        }
        ("aod::qext::qext_div", [Value::QExtRatio(a), Value::QExtRatio(b)]) => {
            a.checked_div(b).map(Value::QExtRatio).map_err(err)
        }
        (
            "aod::qext::qext_render"
            | "aod::qext::qext_to_bytes_exact"
            | "aod::qext::qext_render_bytes",
            [Value::QExtRatio(q)],
        ) => Ok(Value::QExtBytes(q.render_exact().into_bytes())),
        ("aod::qext::qext_round_half_even", [Value::QExtRatio(q), Value::Int(d)]) => {
            let max_d = aod_core::s2_kernel::MAX_ROUNDING_DECIMALS;
            let dec_i = d.to_i64().ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("qext decimals {d} out of bounds (expected 0..={max_d})"),
                )
            })?;
            if dec_i < 0 || (dec_i as usize) > max_d {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("qext decimals {dec_i} out of bounds (expected 0..={max_d})"),
                ));
            }
            q.round_half_even(dec_i as usize)
                .map(Value::QExtRatio)
                .map_err(err)
        }
        (
            "aod::qext::qext_format_fixed"
            | "aod::qext::qext_format_bytes_fixed"
            | "aod::qext::qext_format_fixed_bytes",
            [Value::QExtRatio(q), Value::Int(d)],
        ) => {
            let max_d = aod_core::s2_kernel::MAX_ROUNDING_DECIMALS;
            let dec_i = d.to_i64().ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("qext decimals {d} out of bounds (expected 0..={max_d})"),
                )
            })?;
            if dec_i < 0 || (dec_i as usize) > max_d {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("qext decimals {dec_i} out of bounds (expected 0..={max_d})"),
                ));
            }
            q.format_fixed(dec_i as usize)
                .map(|s| Value::QExtBytes(s.into_bytes()))
                .map_err(err)
        }

        _ => Err(Diagnostic::at_line(
            DiagnosticCode::RuntimeError,
            line,
            format!("unlisted builtin call {name}"),
        )),
    }
}
