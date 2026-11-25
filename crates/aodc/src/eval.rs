use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use aod_core::bigint::BigInt;
use aod_core::ledger::{probability, SadarPromotion};
use aod_core::qext::{parse_decimal_exact, QExtRatio};
use aod_core::ratio::Ratio;
use aod_core::sha256::Sha256;
use aod_core::support::SupportForm;
use aod_core::ternary::{render_bytes_literal, Addr8, Q4, T1, T32, T9};
use aod_core::wavelet::{
    declare_synthetic_lineage_registry, deduplicated_return_count, native_cadence_ratio,
    AdmittedClosureRecord, M3ClosureTrace, RecipientContext, RecipientLattice, RecipientOutcome,
    RecipientOutcomeStage, RecipientStage, ShedRoute, ShedWitness, SingleContactStageAssociation,
    StageEventWitness, Wavelet, WaveletRole, WaveletSpec,
};
use aod_core::witness::{Coupling, Fusion, SeatState, Trace};

use crate::ast::{
    AssignTarget, BinaryOp, CallArg, ChamberDecl, DecArmParam, DecFnDecl, DecFnKind, EnumDecl,
    Expr, ExprKind, ForTarget, Item, ObserveDecl, Pattern, PrepareDecl, ProbeDecl, Program, Stmt,
    StmtKind, StructDecl, TritLiteral, UnaryOp,
};
use crate::checker::CheckedUnit;
use crate::diagnostics::{DecResult, Diagnostic, DiagnosticCode};
use crate::modules::{builtin_namespace_items, canonical_builtin_name, ImportedSymbol};
use crate::sim::{
    BlindMotifSpectrum, BoxRegion, CarrierState, ChamberSnapshot, ChamberState,
    DeclarativeObserver, ReferenceClock, SensorCurrentSummary, SimRunResult,
};
use crate::types::Type;

pub const DEFAULT_FUEL: usize = 100_000;
pub const DEFAULT_MAX_DEPTH: usize = 128;

/// Deterministic totally-ordered map key backing `Value::Map(BTreeMap<MapKey, Value>)`.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum MapKey {
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
        payloads: Vec<MapKey>,
    },
    Tuple(Vec<MapKey>),
    Array(Vec<MapKey>),
}

impl MapKey {
    pub fn from_value(v: &Value, line: usize) -> DecResult<Self> {
        match v {
            Value::Null => Ok(MapKey::Null),
            Value::Int(n) => Ok(MapKey::Int(n.clone())),
            Value::U8(b) => Ok(MapKey::U8(*b)),
            Value::B64(w) => Ok(MapKey::B64(*w)),
            Value::Bool(b) => Ok(MapKey::Bool(*b)),
            Value::Str(s) => Ok(MapKey::Str(s.clone())),
            Value::Bytes(bs) => Ok(MapKey::Bytes(bs.clone())),
            Value::T1(t) => Ok(MapKey::T1(*t)),
            Value::T9(t) => Ok(MapKey::T9(*t)),
            Value::T32(t) => Ok(MapKey::T32(*t)),
            Value::Q4(q) => Ok(MapKey::Q4(*q)),
            Value::Ratio(r) => Ok(MapKey::Ratio(r.clone())),
            Value::Addr8(a) => Ok(MapKey::Addr8(a.clone())),
            Value::Support(s) => Ok(MapKey::Support(s.clone())),
            Value::Enum(ev) => {
                let mut pk = Vec::with_capacity(ev.payloads.len());
                for p in &ev.payloads {
                    pk.push(Self::from_value(p, line)?);
                }
                Ok(MapKey::Enum {
                    module: ev.module.clone(),
                    name: ev.name.clone(),
                    variant: ev.variant.clone(),
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
            Value::Array(elems) => {
                let mut ak = Vec::with_capacity(elems.len());
                for e in elems {
                    ak.push(Self::from_value(e, line)?);
                }
                Ok(MapKey::Array(ak))
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
            MapKey::U8(b) => Value::U8(*b),
            MapKey::B64(w) => Value::B64(*w),
            MapKey::Bool(b) => Value::Bool(*b),
            MapKey::Str(s) => Value::Str(s.clone()),
            MapKey::Bytes(bs) => Value::Bytes(bs.clone()),
            MapKey::T1(t) => Value::T1(*t),
            MapKey::T9(t) => Value::T9(*t),
            MapKey::T32(t) => Value::T32(*t),
            MapKey::Q4(q) => Value::Q4(*q),
            MapKey::Ratio(r) => Value::Ratio(r.clone()),
            MapKey::Addr8(a) => Value::Addr8(a.clone()),
            MapKey::Support(s) => Value::Support(s.clone()),
            MapKey::Enum {
                module,
                name,
                variant,
                payloads,
            } => Value::Enum(Box::new(EnumValue {
                module: module.clone(),
                name: name.clone(),
                variant: variant.clone(),
                payloads: payloads.iter().map(MapKey::to_value).collect(),
            })),
            MapKey::Tuple(elems) => Value::Tuple(elems.iter().map(MapKey::to_value).collect()),
            MapKey::Array(elems) => Value::Array(elems.iter().map(MapKey::to_value).collect()),
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
    pub payloads: Vec<Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Value {
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
    Struct(Box<StructValue>),
    Enum(Box<EnumValue>),
    List(Vec<Value>),
    Array(Vec<Value>),
    Map(Box<BTreeMap<MapKey, Value>>),
    Tuple(Vec<Value>),
    QExtRatio(QExtRatio),
    QExtBool(bool),
    QExtStr(String),
    QExtBytes(Vec<u8>),
}

impl Value {
    pub fn type_of(&self) -> Type {
        match self {
            Value::Unit => Type::Unit,
            Value::Null => Type::Null,
            Value::Int(_) => Type::Int,
            Value::U8(_) => Type::U8,
            Value::B64(_) => Type::B64,
            Value::Bool(_) => Type::Bool,
            Value::Str(_) => Type::Str,
            Value::Bytes(_) => Type::Bytes,
            Value::T1(_) => Type::T1,
            Value::T9(_) => Type::T9,
            Value::T32(_) => Type::T32,
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
            Value::List(items) => {
                let elem = items.first().map(Value::type_of).unwrap_or(Type::Unknown);
                Type::List(Box::new(elem))
            }
            Value::Array(items) => {
                let elem = items.first().map(Value::type_of).unwrap_or(Type::Unknown);
                Type::Array(Box::new(elem), items.len())
            }
            Value::Map(m) => match m.iter().next() {
                Some((k, v)) => Type::Map(Box::new(k.to_value().type_of()), Box::new(v.type_of())),
                None => Type::Map(Box::new(Type::Unknown), Box::new(Type::Unknown)),
            },
            Value::Tuple(elems) => Type::Tuple(elems.iter().map(Value::type_of).collect()),
            Value::QExtRatio(_) => Type::QExt(Box::new(Type::Ratio)),
            Value::QExtBool(_) => Type::QExt(Box::new(Type::Bool)),
            Value::QExtStr(_) => Type::QExt(Box::new(Type::Str)),
            Value::QExtBytes(_) => Type::QExt(Box::new(Type::Bytes)),
        }
    }

    /// Exact v1/v2 deterministic rendering matching `dec_core.py:486-494` and `RtValue::render`.
    pub fn render(&self) -> String {
        match self {
            Value::Unit => "()".to_string(),
            Value::Null => "null".to_string(),
            Value::Int(n) => n.to_decimal_string(),
            Value::U8(b) => format!("u8({b})"),
            Value::B64(w) => format!("b64(0x{w:016x})"),
            Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            Value::Str(s) => s.clone(),
            Value::Bytes(bs) => render_bytes_literal(bs),
            Value::T1(v) => v.render_v1(),
            Value::T9(v) => v.render(),
            Value::T32(v) => v.render_v1(),
            Value::Q4(v) => v.render_v1(),
            Value::Ratio(r) => r.render_v1(),
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
            Value::List(items) | Value::Array(items) => {
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
            Value::QExtStr(s) => format!("Q_ext({s})"),
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
    Break,
    Continue,
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

pub struct Evaluator<'a> {
    program: Arc<EvalProgram<'a>>,
    fuel: usize,
    max_depth: usize,
    depth: usize,
    emits: Vec<String>,
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
            parallel_observers: false,
        };
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

    /// Enable or disable parallel execution of multiple read-only procedural observers (`P0-1`/`P0-2`).
    pub fn with_parallel_observers(mut self, enabled: bool) -> Self {
        self.parallel_observers = enabled;
        self
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
        None
    }

    fn resolve_enum_variant_decl(
        &self,
        mod_key: &str,
        path: &[String],
    ) -> Option<(String, String, String)> {
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
            } else {
                return None;
            }
        } else if path.len() == 3 {
            if let Some(ImportedSymbol::ModuleNamespace(m)) =
                self.imports_for(mod_key).get(&path[0])
            {
                (m.clone(), path[1].clone(), path[2].clone())
            } else {
                return None;
            }
        } else {
            return None;
        };
        let ed = self.program.enums.get(&(emod.clone(), ename.clone()))?;
        if ed.variants.iter().any(|v| v.name == vname) {
            Some((emod, ename, vname))
        } else {
            None
        }
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
                bindings.insert(name.clone(), val.clone());
                true
            }
            Pattern::NullLit => matches!(val, Value::Null),
            Pattern::IntLit(n) => matches!(val, Value::Int(v) if v == n),
            Pattern::U8Lit(b) => matches!(val, Value::U8(v) if v == b),
            Pattern::B64Lit(w) => matches!(val, Value::B64(v) if v == w),
            Pattern::BoolLit(b) => matches!(val, Value::Bool(v) if v == b),
            Pattern::StrLit(s) => matches!(val, Value::Str(v) if v == s),
            Pattern::BytesLit(bs) => matches!(val, Value::Bytes(v) if v == bs),
            Pattern::TritLit(t) => matches!(val, Value::T1(v) if v == t),
            Pattern::T9Lit(t) => matches!(val, Value::T9(v) if v == t),
            Pattern::T32Lit(t) => matches!(val, Value::T32(v) if v == t),
            Pattern::Q4Lit(q) => matches!(val, Value::Q4(v) if v == q),
            Pattern::RatioLit(r) => matches!(val, Value::Ratio(v) if v == r),
            Pattern::Addr8Lit(a) => matches!(val, Value::Addr8(v) if v == a),
            Pattern::SupportLit(s) => matches!(val, Value::Support(v) if v == s),
            Pattern::EnumVariant {
                path,
                bindings: sub_pats,
            } => {
                let Some((emod, ename, vname)) = self.resolve_enum_variant_decl(mod_key, path)
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

    fn call_user_fn(
        &mut self,
        mod_key: &str,
        name: &str,
        args: Vec<(Option<String>, Value)>,
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

        let res = match &decl.kind {
            DecFnKind::MultiArm {
                header_typed_params,
                arms,
                ..
            } => {
                let pos_vals: Vec<Value> = args.into_iter().map(|(_, v)| v).collect();
                let arity = pos_vals.len();
                let mut slot_hints: Vec<Option<Type>> = vec![None; arity];
                if let Some(htp) = header_typed_params.as_ref().filter(|ht| ht.len() == arity) {
                    for (i, p) in htp.iter().enumerate() {
                        slot_hints[i] = Some(Type::resolve_with(
                            &p.ty,
                            self.program.checked.unit.compat,
                            decl.line,
                            &|n| self.lookup_user_type(mod_key, n),
                        )?);
                    }
                }
                for arm in arms.iter().filter(|a| a.params.len() == arity) {
                    for (i, p) in arm.params.iter().enumerate() {
                        if slot_hints[i].is_some() {
                            continue;
                        }
                        let inferred = match p {
                            DecArmParam::TypedVar { ty, .. }
                            | DecArmParam::TypedPattern { ty, .. } => Some(Type::resolve_with(
                                ty,
                                self.program.checked.unit.compat,
                                arm.line,
                                &|n| self.lookup_user_type(mod_key, n),
                            )?),
                            DecArmParam::Pattern(pat) => self.infer_eval_pattern_type(mod_key, pat),
                        };
                        if let Some(t) = inferred {
                            if t != Type::Unknown {
                                slot_hints[i] = Some(t);
                            }
                        }
                    }
                }

                let mut matched_arm = None;
                let mut matched_scope = BTreeMap::new();

                // Deterministic top-to-bottom type + pattern + guard dispatch!
                for arm in arms {
                    if arm.params.len() != arity {
                        continue;
                    }
                    let mut scope = BTreeMap::new();
                    let mut all_match = true;
                    for (idx, (param, val)) in arm.params.iter().zip(&pos_vals).enumerate() {
                        match param {
                            DecArmParam::TypedVar { ty, name } => {
                                let expected = Type::resolve_with(
                                    ty,
                                    self.program.checked.unit.compat,
                                    arm.line,
                                    &|n| self.lookup_user_type(mod_key, n),
                                )?;
                                if !runtime_val_matches_type(val, &expected) {
                                    all_match = false;
                                    break;
                                }
                                scope.insert(name.clone(), val.clone());
                            }
                            DecArmParam::TypedPattern { ty, pattern } => {
                                let expected = Type::resolve_with(
                                    ty,
                                    self.program.checked.unit.compat,
                                    arm.line,
                                    &|n| self.lookup_user_type(mod_key, n),
                                )?;
                                if !runtime_val_matches_type(val, &expected)
                                    || !self.match_pattern(mod_key, pattern, val, &mut scope)
                                {
                                    all_match = false;
                                    break;
                                }
                            }
                            DecArmParam::Pattern(pattern) => {
                                let expected = self
                                    .infer_eval_pattern_type(mod_key, pattern)
                                    .or_else(|| slot_hints[idx].clone())
                                    .unwrap_or(Type::Int);
                                if !runtime_val_matches_type(val, &expected)
                                    || !self.match_pattern(mod_key, pattern, val, &mut scope)
                                {
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

                let arm = matched_arm.ok_or_else(|| {
                    Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        call_line,
                        format!("no matching arm for call to {name}"),
                    )
                })?;
                let mut scopes = vec![matched_scope];
                match self.eval_block(&arm.body, mod_key, &mut scopes)? {
                    ControlFlow::Return(v) => Ok(v),
                    ControlFlow::Normal => match &arm.output_name {
                        Some(out_n) => scopes[0].get(out_n).cloned().ok_or_else(|| {
                            Diagnostic::at_line(
                                DiagnosticCode::RuntimeError,
                                arm.line,
                                "result binding was not assigned",
                            )
                        }),
                        None => Ok(Value::Unit),
                    },
                    _ => Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        arm.line,
                        "unexpected break/continue",
                    )),
                }
            }
            DecFnKind::SingleArm { params, body, .. } => {
                let mut slots: Vec<Option<Value>> = vec![None; params.len()];
                let mut pos_idx = 0usize;
                for (n_opt, val) in args {
                    match n_opt {
                        None => {
                            slots[pos_idx] = Some(val);
                            pos_idx += 1;
                        }
                        Some(n) => {
                            let idx = params.iter().position(|p| p.name == n).ok_or_else(|| {
                                Diagnostic::at_line(
                                    DiagnosticCode::RuntimeError,
                                    call_line,
                                    format!("unknown named argument {n}"),
                                )
                            })?;
                            slots[idx] = Some(val);
                        }
                    }
                }
                let mut scope = BTreeMap::new();
                for (p, slot) in params.iter().zip(slots) {
                    scope.insert(p.name.clone(), slot.expect("typechecked"));
                }
                let mut scopes = vec![scope];
                match self.eval_block(body, mod_key, &mut scopes)? {
                    ControlFlow::Return(v) => Ok(v),
                    ControlFlow::Normal => Ok(Value::Unit),
                    _ => Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        decl.line,
                        "unexpected break/continue",
                    )),
                }
            }
        };

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
                StmtKind::Let { name, value, .. }
                | StmtKind::Mut { name, value, .. }
                | StmtKind::TypedLocal { name, value, .. } => {
                    let v = self.eval_expr(value, mod_key, scopes)?;
                    scopes.last_mut().unwrap().insert(name.clone(), v);
                }
                StmtKind::LetTuple { elems, value, .. } => {
                    let v = self.eval_expr(value, mod_key, scopes)?;
                    let Value::Tuple(items) = v else {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::RuntimeError,
                            stmt.line,
                            "expected tuple value in let (...) destructuring",
                        ));
                    };
                    if items.len() != elems.len() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::RuntimeError,
                            stmt.line,
                            "tuple destructuring arity mismatch",
                        ));
                    }
                    let cur = scopes.last_mut().unwrap();
                    for (el, item) in elems.iter().zip(items) {
                        if el.name != "_" {
                            cur.insert(el.name.clone(), item);
                        }
                    }
                }
                StmtKind::Assign { target, value } => {
                    let v = self.eval_expr(value, mod_key, scopes)?;
                    match target {
                        AssignTarget::Ident(name) => {
                            let mut assigned = false;
                            for scope in scopes.iter_mut().rev() {
                                if scope.contains_key(name) {
                                    scope.insert(name.clone(), v.clone());
                                    assigned = true;
                                    break;
                                }
                            }
                            if !assigned {
                                scopes[0].insert(name.clone(), v);
                            }
                        }
                        AssignTarget::Field { base, name } => {
                            if let ExprKind::Ident(ref vname) = base.kind {
                                let mut updated = false;
                                for scope in scopes.iter_mut().rev() {
                                    if let Some(Value::Struct(sv)) = scope.get_mut(vname) {
                                        sv.fields.insert(name.clone(), v.clone());
                                        updated = true;
                                        break;
                                    }
                                }
                                if !updated {
                                    return Err(Diagnostic::at_line(
                                        DiagnosticCode::RuntimeError,
                                        stmt.line,
                                        format!("field mutation on {vname}.{name} failed"),
                                    ));
                                }
                            } else {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::RuntimeError,
                                    stmt.line,
                                    "field mutation is forbidden",
                                ));
                            }
                        }
                        AssignTarget::Index { base, index } => {
                            let idx_val = self.eval_expr(index, mod_key, scopes)?;
                            if let ExprKind::Ident(ref vname) = base.kind {
                                for scope in scopes.iter_mut().rev() {
                                    if let Some(container) = scope.get_mut(vname) {
                                        match container {
                                            Value::List(items) => {
                                                let idx_i = match idx_val {
                                                    Value::Int(n) => n.to_i64().unwrap_or(-1),
                                                    _ => -1,
                                                };
                                                if idx_i < 0 || (idx_i as usize) >= items.len() {
                                                    return Err(Diagnostic::at_line(
                                                        DiagnosticCode::RuntimeError,
                                                        stmt.line,
                                                        format!("index {idx_i} out of bounds"),
                                                    ));
                                                }
                                                items[idx_i as usize] = v;
                                            }
                                            Value::Map(m) => {
                                                let k = MapKey::from_value(&idx_val, stmt.line)?;
                                                m.insert(k, v);
                                            }
                                            _ => {
                                                return Err(Diagnostic::at_line(
                                                    DiagnosticCode::RuntimeError,
                                                    stmt.line,
                                                    "index assignment requires list or map",
                                                ));
                                            }
                                        }
                                        break;
                                    }
                                }
                            }
                        }
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
                StmtKind::If {
                    cond,
                    then_body,
                    elifs,
                    else_body,
                } => {
                    let cv = self.eval_expr(cond, mod_key, scopes)?;
                    let take_then = matches!(cv, Value::Bool(true) | Value::QExtBool(true));
                    if take_then {
                        scopes.push(BTreeMap::new());
                        let cf = self.eval_block(then_body, mod_key, scopes)?;
                        scopes.pop();
                        if !matches!(cf, ControlFlow::Normal) {
                            return Ok(cf);
                        }
                    } else {
                        let mut took_elif = false;
                        for (ec, eb) in elifs {
                            let ev = self.eval_expr(ec, mod_key, scopes)?;
                            if matches!(ev, Value::Bool(true) | Value::QExtBool(true)) {
                                took_elif = true;
                                scopes.push(BTreeMap::new());
                                let cf = self.eval_block(eb, mod_key, scopes)?;
                                scopes.pop();
                                if !matches!(cf, ControlFlow::Normal) {
                                    return Ok(cf);
                                }
                                break;
                            }
                        }
                        if !took_elif && !else_body.is_empty() {
                            scopes.push(BTreeMap::new());
                            let cf = self.eval_block(else_body, mod_key, scopes)?;
                            scopes.pop();
                            if !matches!(cf, ControlFlow::Normal) {
                                return Ok(cf);
                            }
                        }
                    }
                }
                StmtKind::Match { scrutinee, arms } => {
                    let sval = self.eval_expr(scrutinee, mod_key, scopes)?;
                    for arm in arms {
                        let mut arm_scope = BTreeMap::new();
                        if !self.match_pattern(mod_key, &arm.pattern, &sval, &mut arm_scope) {
                            continue;
                        }
                        scopes.push(arm_scope);
                        let guard_ok = if let Some(ref g) = arm.guard {
                            let gv = self.eval_expr(g, mod_key, scopes)?;
                            matches!(gv, Value::Bool(true))
                        } else {
                            true
                        };
                        if !guard_ok {
                            scopes.pop();
                            continue;
                        }
                        let cf = self.eval_block(&arm.body, mod_key, scopes)?;
                        scopes.pop();
                        if !matches!(cf, ControlFlow::Normal) {
                            return Ok(cf);
                        }
                        break;
                    }
                }
                StmtKind::For { target, iter, body } => {
                    let it = self.eval_expr(iter, mod_key, scopes)?;
                    let items: Vec<Value> = match (target, it) {
                        (ForTarget::Single(_), Value::Range(a, b)) => {
                            (a..b).map(|k| Value::Int(BigInt::from_i64(k))).collect()
                        }
                        (_, Value::List(vs)) => vs,
                        (ForTarget::Single(_), Value::Map(m)) => {
                            m.keys().map(MapKey::to_value).collect()
                        }
                        (ForTarget::Tuple(_), Value::Map(m)) => m
                            .into_iter()
                            .map(|(k, v)| Value::Tuple(vec![k.to_value(), v]))
                            .collect(),
                        _ => {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::RuntimeError,
                                stmt.line,
                                "for loop requires range, list, or map",
                            ));
                        }
                    };
                    for item in items {
                        self.tick()?;
                        let mut lscope = BTreeMap::new();
                        match (target, item) {
                            (ForTarget::Single(var), val) => {
                                lscope.insert(var.clone(), val);
                            }
                            (ForTarget::Tuple(vars), Value::Tuple(elems))
                                if vars.len() == elems.len() =>
                            {
                                for (vn, ev) in vars.iter().zip(elems) {
                                    lscope.insert(vn.clone(), ev);
                                }
                            }
                            _ => {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::RuntimeError,
                                    stmt.line,
                                    "for loop tuple destructure mismatch",
                                ));
                            }
                        }
                        scopes.push(lscope);
                        let cf = self.eval_block(body, mod_key, scopes)?;
                        scopes.pop();
                        match cf {
                            ControlFlow::Normal | ControlFlow::Continue => {}
                            ControlFlow::Break => break,
                            ControlFlow::Return(v) => return Ok(ControlFlow::Return(v)),
                        }
                    }
                }
                StmtKind::While { cond, body } => loop {
                    self.tick()?;
                    let cv = self.eval_expr(cond, mod_key, scopes)?;
                    if !matches!(cv, Value::Bool(true)) {
                        break;
                    }
                    scopes.push(BTreeMap::new());
                    let cf = self.eval_block(body, mod_key, scopes)?;
                    scopes.pop();
                    match cf {
                        ControlFlow::Normal | ControlFlow::Continue => {}
                        ControlFlow::Break => break,
                        ControlFlow::Return(v) => return Ok(ControlFlow::Return(v)),
                    }
                },
                StmtKind::Break => return Ok(ControlFlow::Break),
                StmtKind::Continue => return Ok(ControlFlow::Continue),
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
            ExprKind::StrLit(s) => Ok(Value::Str(s.clone())),
            ExprKind::BytesLit(bs) => Ok(Value::Bytes(bs.clone())),
            ExprKind::RatioLit(rat) => Ok(Value::Ratio(rat.clone())),
            ExprKind::Addr8Lit(a) => Ok(Value::Addr8(a.clone())),
            ExprKind::SupportLit(s) => Ok(Value::Support(s.clone())),
            ExprKind::TritLit(TritLiteral::Zero | TritLiteral::Hinge0H | TritLiteral::Dwell0D) => {
                Ok(Value::T1(T1::ZERO))
            }
            ExprKind::TritLit(TritLiteral::One) => Ok(Value::T1(T1::POS)),
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
                if let Some((emod, ename, vname)) = self.resolve_enum_variant_decl(mod_key, segs) {
                    return Ok(Value::Enum(Box::new(EnumValue {
                        module: emod,
                        name: ename,
                        variant: vname,
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
            ExprKind::StructLit { path, fields } => {
                let (smod, sdecl) =
                    self.resolve_struct_decl(mod_key, path).ok_or_else(|| {
                        Diagnostic::at_line(
                            DiagnosticCode::RuntimeError,
                            expr.line,
                            format!("unknown struct {}", path.join("::")),
                        )
                    })?;
                let mut fvals = BTreeMap::new();
                for (fname, fexpr) in fields {
                    let v = self.eval_expr(fexpr, mod_key, scopes)?;
                    fvals.insert(fname.clone(), v);
                }
                Ok(Value::Struct(Box::new(StructValue {
                    module: smod,
                    name: sdecl.name,
                    fields: fvals,
                })))
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
            ExprKind::List(items) => {
                let mut vals = Vec::with_capacity(items.len());
                for it in items {
                    vals.push(self.eval_expr(it, mod_key, scopes)?);
                }
                Ok(Value::List(vals))
            }
            ExprKind::ArrayLit(items) => {
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
            ExprKind::Binary { op, left, right } => {
                match op {
                    BinaryOp::And => {
                        let lv = self.eval_expr(left, mod_key, scopes)?;
                        return match lv {
                            Value::Bool(false) => Ok(Value::Bool(false)),
                            Value::Bool(true) => self.eval_expr(right, mod_key, scopes),
                            Value::QExtBool(false) => Ok(Value::QExtBool(false)),
                            Value::QExtBool(true) => self.eval_expr(right, mod_key, scopes),
                            _ => Err(Diagnostic::at_line(
                                DiagnosticCode::RuntimeError,
                                expr.line,
                                "and requires bool",
                            )),
                        };
                    }
                    BinaryOp::Or => {
                        let lv = self.eval_expr(left, mod_key, scopes)?;
                        return match lv {
                            Value::Bool(true) => Ok(Value::Bool(true)),
                            Value::Bool(false) => self.eval_expr(right, mod_key, scopes),
                            Value::QExtBool(true) => Ok(Value::QExtBool(true)),
                            Value::QExtBool(false) => self.eval_expr(right, mod_key, scopes),
                            _ => Err(Diagnostic::at_line(
                                DiagnosticCode::RuntimeError,
                                expr.line,
                                "or requires bool",
                            )),
                        };
                    }
                    _ => {}
                }
                let lv = self.eval_expr(left, mod_key, scopes)?;
                let rv = self.eval_expr(right, mod_key, scopes)?;
                eval_binary(*op, lv, rv, expr.line)
            }
            ExprKind::Ternary { cond, yes, no } => {
                let cv = self.eval_expr(cond, mod_key, scopes)?;
                if matches!(cv, Value::Bool(true)) {
                    self.eval_expr(yes, mod_key, scopes)
                } else {
                    self.eval_expr(no, mod_key, scopes)
                }
            }
            ExprKind::Field { base, name } => {
                let bv = self.eval_expr(base, mod_key, scopes)?;
                eval_field(bv, name, expr.line)
            }
            ExprKind::Index { base, index } => {
                let bv = self.eval_expr(base, mod_key, scopes)?;
                let iv = self.eval_expr(index, mod_key, scopes)?;
                match bv {
                    Value::Bytes(bs) => {
                        let idx = extract_i64(iv, expr.line)?;
                        if idx < 0 || (idx as usize) >= bs.len() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::RuntimeError,
                                expr.line,
                                format!("bytes index {idx} out of bounds"),
                            ));
                        }
                        Ok(Value::U8(bs[idx as usize]))
                    }
                    Value::Array(items) => {
                        let idx = extract_i64(iv, expr.line)?;
                        if idx < 0 || (idx as usize) >= items.len() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::RuntimeError,
                                expr.line,
                                format!("array index {idx} out of bounds"),
                            ));
                        }
                        Ok(items[idx as usize].clone())
                    }
                    Value::List(items) => {
                        let idx = extract_i64(iv, expr.line)?;
                        if idx < 0 || (idx as usize) >= items.len() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::RuntimeError,
                                expr.line,
                                format!("list index {idx} out of bounds"),
                            ));
                        }
                        Ok(items[idx as usize].clone())
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
                        "indexing requires bytes, array, list, or map",
                    )),
                }
            }
            ExprKind::Call { callee, args } => {
                self.eval_call(callee, args, mod_key, scopes, expr.line)
            }
        }
    }

    fn infer_eval_pattern_type(&self, mod_key: &str, pat: &Pattern) -> Option<Type> {
        match pat {
            Pattern::Wildcard | Pattern::Binding(_) => None,
            Pattern::NullLit => Some(Type::Null),
            Pattern::IntLit(_) => Some(Type::Int),
            Pattern::U8Lit(_) => Some(Type::U8),
            Pattern::B64Lit(_) => Some(Type::B64),
            Pattern::BoolLit(_) => Some(Type::Bool),
            Pattern::StrLit(_) => Some(Type::Str),
            Pattern::BytesLit(_) => Some(Type::Bytes),
            Pattern::TritLit(_) => Some(Type::T1),
            Pattern::T9Lit(_) => Some(Type::T9),
            Pattern::T32Lit(_) => Some(Type::T32),
            Pattern::Q4Lit(_) => Some(Type::Q4),
            Pattern::RatioLit(_) => Some(Type::Ratio),
            Pattern::Addr8Lit(_) => Some(Type::Addr8),
            Pattern::SupportLit(_) => Some(Type::Support),
            Pattern::EnumVariant { path, .. } => {
                let (emod, ename, _) = self.resolve_enum_variant_decl(mod_key, path)?;
                Some(Type::Enum {
                    module: emod,
                    name: ename,
                })
            }
            Pattern::Tuple(elems) => {
                let mut ts = Vec::with_capacity(elems.len());
                for e in elems {
                    ts.push(self.infer_eval_pattern_type(mod_key, e)?);
                }
                Some(Type::Tuple(ts))
            }
        }
    }

    fn peek_eval_builtin_canon(&self, callee: &Expr, mod_key: &str) -> Option<String> {
        let base_mod = self.base_module_of(mod_key).to_string();
        match &callee.kind {
            ExprKind::Ident(name) => {
                if self
                    .program
                    .functions
                    .contains_key(&(mod_key.to_string(), name.clone()))
                    || self.program.functions.contains_key(&(base_mod, name.clone()))
                {
                    return None;
                }
                if let Some(sym) = self.imports_for(mod_key).get(name) {
                    return match sym {
                        ImportedSymbol::Builtin(c) => Some(c.clone()),
                        _ => None,
                    };
                }
                Some(name.clone())
            }
            ExprKind::Path(segs) if segs.len() == 3 && segs[0] == "aod" => {
                let ns = &segs[0..2];
                if builtin_namespace_items(ns).is_some() {
                    Some(canonical_builtin_name(ns, &segs[2]))
                } else {
                    None
                }
            }
            ExprKind::Path(segs) if segs.len() == 2 => {
                if let Some(ImportedSymbol::ModuleNamespace(target_mod)) =
                    self.imports_for(mod_key).get(&segs[0])
                {
                    if let Some(rest) = target_mod.strip_prefix("aod::") {
                        let ns_vec = vec!["aod".to_string(), rest.to_string()];
                        return Some(canonical_builtin_name(&ns_vec, &segs[1]));
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn resolve_eval_ho_target(
        &self,
        mod_key: &str,
        fn_expr: &Expr,
        line: usize,
    ) -> DecResult<EvalHoTarget> {
        let base_mod = self.base_module_of(mod_key).to_string();
        match &fn_expr.kind {
            ExprKind::Ident(name) => {
                if self
                    .program
                    .functions
                    .contains_key(&(mod_key.to_string(), name.clone()))
                {
                    return Ok(EvalHoTarget::UserFn(mod_key.to_string(), name.clone()));
                }
                if base_mod != mod_key
                    && self
                        .program
                        .functions
                        .contains_key(&(base_mod.clone(), name.clone()))
                {
                    return Ok(EvalHoTarget::UserFn(base_mod, name.clone()));
                }
                if let Some(sym) = self.imports_for(mod_key).get(name).cloned() {
                    match sym {
                        ImportedSymbol::ModuleItem { module, item } => {
                            return Ok(EvalHoTarget::UserFn(module, item));
                        }
                        ImportedSymbol::Builtin(canon) => {
                            return Ok(EvalHoTarget::Builtin(canon));
                        }
                        ImportedSymbol::ModuleNamespace(_) => {}
                    }
                }
                for (m, qm) in &self.program.qext_modules {
                    if m == &base_mod {
                        let sub = format!("{m}::{qm}");
                        if self.program.functions.contains_key(&(sub.clone(), name.clone())) {
                            return Ok(EvalHoTarget::UserFn(sub, name.clone()));
                        }
                    }
                }
                Ok(EvalHoTarget::Builtin(name.clone()))
            }
            ExprKind::Path(segs) if segs.len() == 3 && segs[0] == "aod" => {
                let ns = &segs[0..2];
                Ok(EvalHoTarget::Builtin(canonical_builtin_name(ns, &segs[2])))
            }
            ExprKind::Path(segs) if segs.len() == 2 => {
                if let Some(qsub) = self.resolve_qext_submodule(mod_key, &segs[0]) {
                    if self
                        .program
                        .functions
                        .contains_key(&(qsub.clone(), segs[1].clone()))
                    {
                        return Ok(EvalHoTarget::UserFn(qsub, segs[1].clone()));
                    }
                }
                if let Some(ImportedSymbol::ModuleNamespace(target_mod)) =
                    self.imports_for(mod_key).get(&segs[0]).cloned()
                {
                    if let Some(rest) = target_mod.strip_prefix("aod::") {
                        let ns_vec = vec!["aod".to_string(), rest.to_string()];
                        return Ok(EvalHoTarget::Builtin(canonical_builtin_name(
                            &ns_vec, &segs[1],
                        )));
                    }
                    return Ok(EvalHoTarget::UserFn(target_mod, segs[1].clone()));
                }
                Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("undefined higher-order callback path {}", segs.join("::")),
                ))
            }
            ExprKind::Path(segs) if segs.len() == 3 => {
                if let Some(qsub) = self.resolve_qext_submodule_3seg(mod_key, &segs[0], &segs[1]) {
                    if self
                        .program
                        .functions
                        .contains_key(&(qsub.clone(), segs[2].clone()))
                    {
                        return Ok(EvalHoTarget::UserFn(qsub, segs[2].clone()));
                    }
                }
                Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("undefined higher-order callback path {}", segs.join("::")),
                ))
            }
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "invalid higher-order callback reference",
            )),
        }
    }

    fn call_eval_ho(
        &mut self,
        target: &EvalHoTarget,
        mut pos_args: Vec<Value>,
        env_opt: Option<&Value>,
        line: usize,
    ) -> DecResult<Value> {
        if let Some(ev) = env_opt {
            pos_args.push(ev.clone());
        }
        match target {
            EvalHoTarget::UserFn(fmod, fname) => {
                let named: Vec<(Option<String>, Value)> =
                    pos_args.into_iter().map(|v| (None, v)).collect();
                self.call_user_fn(fmod, fname, named, line)
            }
            EvalHoTarget::Builtin(canon) => eval_builtin(canon, pos_args, line),
        }
    }

    fn eval_seq_values(val: Value, op: &str, line: usize) -> DecResult<Vec<Value>> {
        match val {
            Value::List(xs) | Value::Array(xs) => Ok(xs),
            Value::Bytes(bs) => Ok(bs.into_iter().map(Value::U8).collect()),
            other => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!("{op} expects [T], array[T, N], or bytes, got {}", other.type_of()),
            )),
        }
    }

    fn eval_higher_order_scope_op(
        &mut self,
        op: &str,
        args: &[CallArg],
        mod_key: &str,
        scopes: &[BTreeMap<String, Value>],
        line: usize,
    ) -> DecResult<Value> {
        match op {
            "evolve" => {
                let mut state = self.eval_expr(&args[0].value, mod_key, scopes)?;
                let dom_val = self.eval_expr(&args[1].value, mod_key, scopes)?;
                let target = self.resolve_eval_ho_target(mod_key, &args[2].value, line)?;
                let env_val = if args.len() == 4 {
                    Some(self.eval_expr(&args[3].value, mod_key, scopes)?)
                } else {
                    None
                };
                match dom_val {
                    Value::Range(a, b) => {
                        let mut k = a;
                        while k < b {
                            self.tick()?;
                            state = self.call_eval_ho(
                                &target,
                                vec![state, Value::Int(BigInt::from_i64(k))],
                                env_val.as_ref(),
                                line,
                            )?;
                            k += 1;
                        }
                    }
                    Value::Window(u, v) => {
                        let mut prev = u;
                        while prev < v {
                            self.tick()?;
                            let k = prev + 1;
                            state = self.call_eval_ho(
                                &target,
                                vec![state, Value::Int(BigInt::from_i64(k))],
                                env_val.as_ref(),
                                line,
                            )?;
                            prev = k;
                        }
                    }
                    other => {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::RuntimeError,
                            line,
                            format!("evolve domain must be range or window, got {}", other.type_of()),
                        ));
                    }
                }
                Ok(state)
            }
            "scan" => {
                let mut state = self.eval_expr(&args[0].value, mod_key, scopes)?;
                let src_val = self.eval_expr(&args[1].value, mod_key, scopes)?;
                let target = self.resolve_eval_ho_target(mod_key, &args[2].value, line)?;
                let env_val = if args.len() == 4 {
                    Some(self.eval_expr(&args[3].value, mod_key, scopes)?)
                } else {
                    None
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
                            state = self.call_eval_ho(
                                &target,
                                vec![state, Value::Int(BigInt::from_i64(k))],
                                env_val.as_ref(),
                                line,
                            )?;
                            out.push(state.clone());
                            k += 1;
                        }
                        Ok(Value::List(out))
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
                            state = self.call_eval_ho(
                                &target,
                                vec![state, Value::Int(BigInt::from_i64(k))],
                                env_val.as_ref(),
                                line,
                            )?;
                            out.push(state.clone());
                            prev = k;
                        }
                        Ok(Value::List(out))
                    }
                    other => {
                        let items = Self::eval_seq_values(other, "scan", line)?;
                        let mut out = Vec::with_capacity(items.len().min(65_536));
                        for x in items {
                            self.tick()?;
                            state = self.call_eval_ho(
                                &target,
                                vec![state, x],
                                env_val.as_ref(),
                                line,
                            )?;
                            out.push(state.clone());
                        }
                        Ok(Value::List(out))
                    }
                }
            }
            "fold_seq" => {
                let mut state = self.eval_expr(&args[0].value, mod_key, scopes)?;
                let seq_val = self.eval_expr(&args[1].value, mod_key, scopes)?;
                let target = self.resolve_eval_ho_target(mod_key, &args[2].value, line)?;
                let env_val = if args.len() == 4 {
                    Some(self.eval_expr(&args[3].value, mod_key, scopes)?)
                } else {
                    None
                };
                let items = Self::eval_seq_values(seq_val, "fold_seq", line)?;
                for x in items {
                    self.tick()?;
                    state = self.call_eval_ho(&target, vec![state, x], env_val.as_ref(), line)?;
                }
                Ok(state)
            }
            "map_seq" => {
                let seq_val = self.eval_expr(&args[0].value, mod_key, scopes)?;
                let target = self.resolve_eval_ho_target(mod_key, &args[1].value, line)?;
                let env_val = if args.len() == 3 {
                    Some(self.eval_expr(&args[2].value, mod_key, scopes)?)
                } else {
                    None
                };
                let items = Self::eval_seq_values(seq_val, "map_seq", line)?;
                let mut out = Vec::with_capacity(items.len().min(65_536));
                for x in items {
                    self.tick()?;
                    let y = self.call_eval_ho(&target, vec![x], env_val.as_ref(), line)?;
                    out.push(y);
                }
                Ok(Value::List(out))
            }
            "filter_seq" => {
                let seq_val = self.eval_expr(&args[0].value, mod_key, scopes)?;
                let target = self.resolve_eval_ho_target(mod_key, &args[1].value, line)?;
                let env_val = if args.len() == 3 {
                    Some(self.eval_expr(&args[2].value, mod_key, scopes)?)
                } else {
                    None
                };
                let items = Self::eval_seq_values(seq_val, "filter_seq", line)?;
                let mut out = Vec::new();
                for x in items {
                    self.tick()?;
                    let keep = self.call_eval_ho(&target, vec![x.clone()], env_val.as_ref(), line)?;
                    if matches!(keep, Value::Bool(true)) {
                        out.push(x);
                    }
                }
                Ok(Value::List(out))
            }
            "tabulate" => {
                let dom_val = self.eval_expr(&args[0].value, mod_key, scopes)?;
                let target = self.resolve_eval_ho_target(mod_key, &args[1].value, line)?;
                let env_val = if args.len() == 3 {
                    Some(self.eval_expr(&args[2].value, mod_key, scopes)?)
                } else {
                    None
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
                            let y = self.call_eval_ho(
                                &target,
                                vec![Value::Int(BigInt::from_i64(k))],
                                env_val.as_ref(),
                                line,
                            )?;
                            out.push(y);
                            k += 1;
                        }
                        Ok(Value::List(out))
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
                            let y = self.call_eval_ho(
                                &target,
                                vec![Value::Int(BigInt::from_i64(k))],
                                env_val.as_ref(),
                                line,
                            )?;
                            out.push(y);
                            prev = k;
                        }
                        Ok(Value::List(out))
                    }
                    other => Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("tabulate domain must be range or window, got {}", other.type_of()),
                    )),
                }
            }
            "zip_seq" => {
                let a_val = self.eval_expr(&args[0].value, mod_key, scopes)?;
                let b_val = self.eval_expr(&args[1].value, mod_key, scopes)?;
                let xs = Self::eval_seq_values(a_val, "zip_seq", line)?;
                let ys = Self::eval_seq_values(b_val, "zip_seq", line)?;
                if xs.len() != ys.len() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!(
                            "zip_seq(a, b) sequence length mismatch: {} != {}",
                            xs.len(),
                            ys.len()
                        ),
                    ));
                }
                let out: Vec<Value> = xs
                    .into_iter()
                    .zip(ys)
                    .map(|(a, b)| Value::Tuple(vec![a, b]))
                    .collect();
                Ok(Value::List(out))
            }
            "count" => {
                let seq_val = self.eval_expr(&args[0].value, mod_key, scopes)?;
                let target = self.resolve_eval_ho_target(mod_key, &args[1].value, line)?;
                let env_val = if args.len() == 3 {
                    Some(self.eval_expr(&args[2].value, mod_key, scopes)?)
                } else {
                    None
                };
                let items = Self::eval_seq_values(seq_val, "count", line)?;
                let mut n = 0u64;
                for x in items {
                    self.tick()?;
                    if matches!(
                        self.call_eval_ho(&target, vec![x], env_val.as_ref(), line)?,
                        Value::Bool(true)
                    ) {
                        n += 1;
                    }
                }
                Ok(Value::Int(BigInt::from_u64(n)))
            }
            "any" | "all" => {
                let is_any = op == "any";
                let is_qext_bool = self
                    .program
                    .checked
                    .poly_call_types
                    .get(&(mod_key.to_string(), line, format!("{op}:{args:?}")))
                    == Some(&Type::QExt(Box::new(Type::Bool)));
                let seq_val = self.eval_expr(&args[0].value, mod_key, scopes)?;
                let target = self.resolve_eval_ho_target(mod_key, &args[1].value, line)?;
                let env_val = if args.len() == 3 {
                    Some(self.eval_expr(&args[2].value, mod_key, scopes)?)
                } else {
                    None
                };
                let items = Self::eval_seq_values(seq_val, op, line)?;
                for x in items {
                    self.tick()?;
                    let pv = self.call_eval_ho(&target, vec![x], env_val.as_ref(), line)?;
                    let b = match pv {
                        Value::Bool(b) | Value::QExtBool(b) => b,
                        _ => false,
                    };
                    if is_any && b {
                        return Ok(if is_qext_bool {
                            Value::QExtBool(true)
                        } else {
                            Value::Bool(true)
                        });
                    }
                    if !is_any && !b {
                        return Ok(if is_qext_bool {
                            Value::QExtBool(false)
                        } else {
                            Value::Bool(false)
                        });
                    }
                }
                let default_b = !is_any;
                Ok(if is_qext_bool {
                    Value::QExtBool(default_b)
                } else {
                    Value::Bool(default_b)
                })
            }
            _ => unreachable!(),
        }
    }

    fn eval_sum_or_dot(
        &self,
        op: &str,
        raw_args: &[CallArg],
        vals: Vec<Value>,
        mod_key: &str,
        line: usize,
    ) -> DecResult<Value> {
        let inferred_ty = self
            .program
            .checked
            .poly_call_types
            .get(&(mod_key.to_string(), line, format!("{op}:{raw_args:?}")))
            .cloned();
        let zero_for = |ty_opt: Option<Type>, sample: Option<&Value>| -> Value {
            match sample.map(Value::type_of).or(ty_opt) {
                Some(Type::U8) => Value::U8(0),
                Some(Type::T1) => Value::T1(T1::ZERO),
                Some(Type::T9) => Value::T9(T9::new(0).expect("0 in t9")),
                Some(Type::T32) => Value::T32(T32::new(0).expect("0 in t32")),
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
                let items = Self::eval_seq_values(
                    vals.into_iter().next().unwrap(),
                    "sum",
                    line,
                )?;
                let mut acc = zero_for(inferred_ty, items.first());
                for item in items {
                    acc = match (acc, item) {
                        (Value::Int(a), Value::Int(b)) => Value::Int(&a + &b),
                        (Value::U8(a), Value::U8(b)) => Value::U8(a.checked_add(b).ok_or_else(|| {
                            Diagnostic::at_line(
                                DiagnosticCode::RuntimeError,
                                line,
                                "sum(u8): u8 addition overflow",
                            )
                        })?),
                        (Value::T1(a), Value::T1(b)) => {
                            Value::T1(T1::new(a.value() + b.value()).map_err(err)?)
                        }
                        (Value::T9(a), Value::T9(b)) => Value::T9(a.checked_add(b).map_err(err)?),
                        (Value::T32(a), Value::T32(b)) => Value::T32(a.checked_add(b).map_err(err)?),
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
                let xs = Self::eval_seq_values(it.next().unwrap(), "dot", line)?;
                let ys = Self::eval_seq_values(it.next().unwrap(), "dot", line)?;
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
                        (Value::Int(s), Value::Int(a), Value::Int(b)) => Value::Int(&s + &(&a * &b)),
                        (Value::U8(s), Value::U8(a), Value::U8(b)) => {
                            let p = a.checked_mul(b).ok_or_else(|| {
                                Diagnostic::at_line(
                                    DiagnosticCode::RuntimeError,
                                    line,
                                    "dot(u8): u8 multiplication overflow",
                                )
                            })?;
                            Value::U8(s.checked_add(p).ok_or_else(|| {
                                Diagnostic::at_line(
                                    DiagnosticCode::RuntimeError,
                                    line,
                                    "dot(u8): u8 addition overflow",
                                )
                            })?)
                        }
                        (Value::T1(s), Value::T1(a), Value::T1(b)) => {
                            let p = a.value() * b.value();
                            Value::T1(T1::new(s.value() + p).map_err(err)?)
                        }
                        (Value::T9(s), Value::T9(a), Value::T9(b)) => {
                            let p = a.checked_mul(b).map_err(err)?;
                            Value::T9(s.checked_add(p).map_err(err)?)
                        }
                        (Value::T32(s), Value::T32(a), Value::T32(b)) => {
                            let p = a.checked_mul(b).map_err(err)?;
                            Value::T32(s.checked_add(p).map_err(err)?)
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

    fn eval_call(
        &mut self,
        callee: &Expr,
        args: &[CallArg],
        mod_key: &str,
        scopes: &[BTreeMap<String, Value>],
        line: usize,
    ) -> DecResult<Value> {
        if let Some(
            op @ ("evolve" | "scan" | "fold_seq" | "map_seq" | "filter_seq" | "tabulate"
            | "zip_seq" | "count" | "any" | "all"),
        ) = self.peek_eval_builtin_canon(callee, mod_key).as_deref()
        {
            return self.eval_higher_order_scope_op(op, args, mod_key, scopes, line);
        }

        let mut vals = Vec::with_capacity(args.len());
        for a in args {
            vals.push((a.name.clone(), self.eval_expr(&a.value, mod_key, scopes)?));
        }

        if let Some(op @ ("sum" | "dot")) = self.peek_eval_builtin_canon(callee, mod_key).as_deref()
        {
            let pos: Vec<Value> = vals.into_iter().map(|(_, v)| v).collect();
            return self.eval_sum_or_dot(op, args, pos, mod_key, line);
        }

        // Enum payload constructor `Enum::Variant(args...)`
        if let ExprKind::Path(ref segs) = callee.kind {
            if let Some((emod, ename, vname)) = self.resolve_enum_variant_decl(mod_key, segs) {
                let payloads: Vec<Value> = vals.into_iter().map(|(_, v)| v).collect();
                return Ok(Value::Enum(Box::new(EnumValue {
                    module: emod,
                    name: ename,
                    variant: vname,
                    payloads,
                })));
            }
        }

        // Struct constructor call `Foo(1, 2)` or `Foo(x: 1, y: 2)` (falling through to `dec Foo:` if recorded in `poly_call_types`!)
        let struct_path_opt: Option<Vec<String>> = match &callee.kind {
            ExprKind::Ident(n) => Some(vec![n.clone()]),
            ExprKind::Path(segs) => Some(segs.clone()),
            _ => None,
        };
        let is_struct_overload = self
            .program
            .checked
            .poly_call_types
            .contains_key(&(
                mod_key.to_string(),
                line,
                format!("struct_ov:{callee:?}:{args:?}"),
            ));
        if !is_struct_overload {
            if let Some(ref sp) = struct_path_opt {
                if let Some((smod, sdecl)) = self.resolve_struct_decl(mod_key, sp) {
                    let mut fields = BTreeMap::new();
                    let mut pos_idx = 0usize;
                    for (n_opt, v) in vals {
                        match n_opt {
                            None => {
                                let fname = sdecl.fields[pos_idx].0.clone();
                                fields.insert(fname, v);
                                pos_idx += 1;
                            }
                            Some(n) => {
                                fields.insert(n, v);
                            }
                        }
                    }
                    return Ok(Value::Struct(Box::new(StructValue {
                        module: smod,
                        name: sdecl.name,
                        fields,
                    })));
                }
            }
        }

        let base_mod = self.base_module_of(mod_key).to_string();

        match &callee.kind {
            ExprKind::Ident(name) => {
                if self
                    .program
                    .functions
                    .contains_key(&(mod_key.to_string(), name.clone()))
                {
                    return self.call_user_fn(mod_key, name, vals, line);
                }
                if base_mod != mod_key
                    && self
                        .program
                        .functions
                        .contains_key(&(base_mod.clone(), name.clone()))
                {
                    return self.call_user_fn(&base_mod, name, vals, line);
                }
                if let Some(sym) = self.imports_for(mod_key).get(name).cloned() {
                    match sym {
                        ImportedSymbol::ModuleItem { module, item } => {
                            return self.call_user_fn(&module, &item, vals, line);
                        }
                        ImportedSymbol::Builtin(canon) => {
                            let pos: Vec<Value> = vals.into_iter().map(|(_, v)| v).collect();
                            return eval_builtin(&canon, pos, line);
                        }
                        ImportedSymbol::ModuleNamespace(_) => {}
                    }
                }
                let pos: Vec<Value> = vals.into_iter().map(|(_, v)| v).collect();
                eval_builtin(name, pos, line)
            }
            ExprKind::Path(segs) => {
                if segs.len() == 3 && segs[0] == "aod" {
                    let ns = &segs[0..2];
                    let sym = &segs[2];
                    if builtin_namespace_items(ns).is_some() {
                        let canon = canonical_builtin_name(ns, sym);
                        let pos: Vec<Value> = vals.into_iter().map(|(_, v)| v).collect();
                        return eval_builtin(&canon, pos, line);
                    }
                } else if segs.len() == 2 {
                    if let Some(qsub) = self.resolve_qext_submodule(mod_key, &segs[0]) {
                        if self
                            .program
                            .functions
                            .contains_key(&(qsub.clone(), segs[1].clone()))
                        {
                            return self.call_user_fn(&qsub, &segs[1], vals, line);
                        }
                    }
                    if let Some(ImportedSymbol::ModuleNamespace(target_mod)) =
                        self.imports_for(mod_key).get(&segs[0]).cloned()
                    {
                        if let Some(rest) = target_mod.strip_prefix("aod::") {
                            let ns_vec = vec!["aod".to_string(), rest.to_string()];
                            let canon = canonical_builtin_name(&ns_vec, &segs[1]);
                            let pos: Vec<Value> = vals.into_iter().map(|(_, v)| v).collect();
                            return eval_builtin(&canon, pos, line);
                        }
                        return self.call_user_fn(&target_mod, &segs[1], vals, line);
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
                            return self.call_user_fn(&qsub, &segs[2], vals, line);
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

        // P2-1: Reject clock period <= 0 explicitly without silent clamping!
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
            let rc = ReferenceClock::new(clk.name.clone(), p as u64)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, clk.line, e.to_string()))?;
            clocks.push(rc);
        }

        // P2-1: Reject orient == 0t and shells <= 0 explicitly without silent clamping!
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
                Value::Int(n) => T1::from_bigint(&n)
                    .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, inj.line, e.to_string()))?,
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
                    match self.eval_block(body, mod_key, &mut pscopes)? {
                        ControlFlow::Return(v) if v != Value::Unit => {
                            procedural_outputs
                                .entry(pname.clone())
                                .or_default()
                                .push(v.render());
                        }
                        _ => {}
                    }
                }
            } else {
                // P0-1 & P0-2: Lightweight scoped parallel observer workers sharing `&Arc<EvalProgram>`
                // (zero per-tick AST cloning) with exact sequential-equivalent fuel & error merge.
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
                        // Tight-budget boundary (`P0-2`): re-run ONLY observer `idx` sequentially
                        // with exact `remaining_fuel` so emits, fuel, and BudgetExhausted match!
                        self.fuel = remaining_fuel;
                        let mut pscope = BTreeMap::new();
                        pscope.insert(
                            param_name.clone(),
                            Value::ChamberView(Box::new(snap.clone())),
                        );
                        let mut pscopes = vec![pscope];
                        match self.eval_block(body, mod_key, &mut pscopes)? {
                            ControlFlow::Return(v) if v != Value::Unit => {
                                procedural_outputs
                                    .entry(pname.clone())
                                    .or_default()
                                    .push(v.render());
                            }
                            _ => {}
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
    let actual = val.type_of();
    runtime_type_matches(&actual, expected)
}

fn runtime_type_matches(actual: &Type, expected: &Type) -> bool {
    let expected = expected.unwrap_read_only();
    if actual == expected {
        return true;
    }
    match (actual, expected) {
        (Type::List(a), Type::List(e)) => **a == Type::Unknown || runtime_type_matches(a, e),
        (Type::Array(a, an), Type::Array(e, en)) if an == en => runtime_type_matches(a, e),
        (Type::Map(ak, av), Type::Map(ek, ev)) => {
            (**ak == Type::Unknown && **av == Type::Unknown)
                || (ak == ek && runtime_type_matches(av, ev))
        }
        (Type::Tuple(as_), Type::Tuple(es)) if as_.len() == es.len() => {
            as_.iter().zip(es).all(|(a, e)| runtime_type_matches(a, e))
        }
        _ => false,
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
    match (op, val) {
        (UnaryOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
        (UnaryOp::Not, Value::QExtBool(b)) => Ok(Value::QExtBool(!b)),
        (
            UnaryOp::Pos,
            v @ (Value::Int(_)
            | Value::U8(_)
            | Value::T1(_)
            | Value::T9(_)
            | Value::T32(_)
            | Value::Ratio(_)
            | Value::QExtRatio(_)),
        ) => Ok(v),
        (UnaryOp::Neg, Value::Int(n)) => Ok(Value::Int(-n)),
        (UnaryOp::Neg, Value::T1(t)) => Ok(Value::T1(t.checked_neg())),
        (UnaryOp::Neg, Value::T9(t)) => Ok(Value::T9(t.checked_neg())),
        (UnaryOp::Neg, Value::T32(t)) => t
            .checked_neg()
            .map(Value::T32)
            .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
        (UnaryOp::Neg, Value::Ratio(r)) => Ok(Value::Ratio(-r)),
        (UnaryOp::Neg, Value::QExtRatio(q)) => Ok(Value::QExtRatio(q.neg())),
        (UnaryOp::Boundary, Value::Box3(b)) => Ok(Value::Str(b.boundary_descriptor())),
        (UnaryOp::Boundary, Value::SimResult(s)) => {
            Ok(Value::Str(s.final_snapshot.space.boundary_descriptor()))
        }
        (UnaryOp::Boundary, Value::Window(u, v)) | (UnaryOp::Boundary, Value::Range(u, v)) => {
            Ok(Value::Tuple(vec![
                Value::Int(BigInt::from_i64(u)),
                Value::Int(BigInt::from_i64(v)),
            ]))
        }
        _ => Err(Diagnostic::at_line(
            DiagnosticCode::RuntimeError,
            line,
            "invalid unary operation",
        )),
    }
}

fn eval_binary(op: BinaryOp, left: Value, right: Value, line: usize) -> DecResult<Value> {
    if op == BinaryOp::Eq {
        return match (&left, &right) {
            (Value::QExtRatio(a), Value::QExtRatio(b)) => Ok(Value::QExtBool(a == b)),
            (Value::QExtBool(a), Value::QExtBool(b)) => Ok(Value::QExtBool(a == b)),
            (Value::QExtStr(a), Value::QExtStr(b)) => Ok(Value::QExtBool(a == b)),
            (Value::QExtBytes(a), Value::QExtBytes(b)) => Ok(Value::QExtBool(a == b)),
            _ => Ok(Value::Bool(left == right)),
        };
    }
    if op == BinaryOp::NotEq {
        return match (&left, &right) {
            (Value::QExtRatio(a), Value::QExtRatio(b)) => Ok(Value::QExtBool(a != b)),
            (Value::QExtBool(a), Value::QExtBool(b)) => Ok(Value::QExtBool(a != b)),
            (Value::QExtStr(a), Value::QExtStr(b)) => Ok(Value::QExtBool(a != b)),
            (Value::QExtBytes(a), Value::QExtBytes(b)) => Ok(Value::QExtBool(a != b)),
            _ => Ok(Value::Bool(left != right)),
        };
    }

    match (left, right) {
        (Value::U8(a), Value::Int(b)) => {
            let s = b
                .to_u64()
                .filter(|&v| v < 8)
                .ok_or_else(|| {
                    Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("u8 shift count {b} out of range 0..8"),
                    )
                })? as u32;
            match op {
                BinaryOp::Shl => Ok(Value::U8(a << s)),
                BinaryOp::Shr => Ok(Value::U8(a >> s)),
                _ => Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "unsupported (u8, int) operator",
                )),
            }
        }
        (Value::B64(a), Value::Int(b)) => {
            let s = b
                .to_u64()
                .filter(|&v| v < 64)
                .ok_or_else(|| {
                    Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("b64 shift count {b} out of range 0..64"),
                    )
                })? as u32;
            match op {
                BinaryOp::Shl => Ok(Value::B64(a << s)),
                BinaryOp::Shr => Ok(Value::B64(a >> s)),
                _ => Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "unsupported (b64, int) operator",
                )),
            }
        }
        (Value::U8(a), Value::U8(b)) => match op {
            BinaryOp::Add => a.checked_add(b).map(Value::U8).ok_or_else(|| {
                Diagnostic::at_line(DiagnosticCode::RuntimeError, line, "u8 addition overflow")
            }),
            BinaryOp::Sub => a.checked_sub(b).map(Value::U8).ok_or_else(|| {
                Diagnostic::at_line(DiagnosticCode::RuntimeError, line, "u8 subtraction underflow")
            }),
            BinaryOp::Mul => a.checked_mul(b).map(Value::U8).ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "u8 multiplication overflow",
                )
            }),
            BinaryOp::Div => Ratio::from_i64(a as i64, b as i64)
                .map(Value::Ratio)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::FloorDiv => {
                if b == 0 {
                    Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        "division by zero",
                    ))
                } else {
                    Ok(Value::U8(a / b))
                }
            }
            BinaryOp::Mod => {
                if b == 0 {
                    Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        "division by zero",
                    ))
                } else {
                    Ok(Value::U8(a % b))
                }
            }
            BinaryOp::BitAnd => Ok(Value::U8(a & b)),
            BinaryOp::BitOr => Ok(Value::U8(a | b)),
            BinaryOp::BitXor => Ok(Value::U8(a ^ b)),
            BinaryOp::Shl => {
                if b >= 8 {
                    Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("u8 shift count {b} out of range 0..8"),
                    ))
                } else {
                    Ok(Value::U8(a << (b as u32)))
                }
            }
            BinaryOp::Shr => {
                if b >= 8 {
                    Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("u8 shift count {b} out of range 0..8"),
                    ))
                } else {
                    Ok(Value::U8(a >> (b as u32)))
                }
            }
            BinaryOp::Lt => Ok(Value::Bool(a < b)),
            BinaryOp::LtEq => Ok(Value::Bool(a <= b)),
            BinaryOp::Gt => Ok(Value::Bool(a > b)),
            BinaryOp::GtEq => Ok(Value::Bool(a >= b)),
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "unsupported u8 operator",
            )),
        },
        (Value::B64(a), Value::B64(b)) => match op {
            BinaryOp::BitAnd => Ok(Value::B64(a & b)),
            BinaryOp::BitOr => Ok(Value::B64(a | b)),
            BinaryOp::BitXor => Ok(Value::B64(a ^ b)),
            BinaryOp::Shl => {
                if b >= 64 {
                    Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("b64 shift count {b} out of range 0..64"),
                    ))
                } else {
                    Ok(Value::B64(a << (b as u32)))
                }
            }
            BinaryOp::Shr => {
                if b >= 64 {
                    Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("b64 shift count {b} out of range 0..64"),
                    ))
                } else {
                    Ok(Value::B64(a >> (b as u32)))
                }
            }
            BinaryOp::Lt => Ok(Value::Bool(a < b)),
            BinaryOp::LtEq => Ok(Value::Bool(a <= b)),
            BinaryOp::Gt => Ok(Value::Bool(a > b)),
            BinaryOp::GtEq => Ok(Value::Bool(a >= b)),
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "unsupported b64 operator",
            )),
        },
        (Value::T9(a), Value::T9(b)) => match op {
            BinaryOp::Add => a
                .checked_add(b)
                .map(Value::T9)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::Sub => a
                .checked_sub(b)
                .map(Value::T9)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::Mul => a
                .checked_mul(b)
                .map(Value::T9)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::Div => Ratio::from_i64(a.value() as i64, b.value() as i64)
                .map(Value::Ratio)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::FloorDiv => a
                .checked_div_floor(b)
                .map(Value::T9)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::Mod => a
                .checked_rem_floor(b)
                .map(Value::T9)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::Lt => Ok(Value::Bool(a < b)),
            BinaryOp::LtEq => Ok(Value::Bool(a <= b)),
            BinaryOp::Gt => Ok(Value::Bool(a > b)),
            BinaryOp::GtEq => Ok(Value::Bool(a >= b)),
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "unsupported t9 operator",
            )),
        },
        (Value::Q4(a), Value::Q4(b)) => match op {
            BinaryOp::BitAnd => Ok(Value::Q4(a.bitand(b))),
            BinaryOp::BitOr => Ok(Value::Q4(a.bitor(b))),
            BinaryOp::BitXor => Ok(Value::Q4(a.bitxor(b))),
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "unsupported q4 operator",
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
            BinaryOp::BitAnd => a
                .bitand(&b)
                .map(Value::Int)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::BitOr => a
                .bitor(&b)
                .map(Value::Int)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::BitXor => a
                .bitxor(&b)
                .map(Value::Int)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::Shl => {
                let s = b.to_u64().and_then(|v| u32::try_from(v).ok()).ok_or_else(|| {
                    Diagnostic::at_line(DiagnosticCode::RuntimeError, line, "invalid shift count")
                })?;
                a.shl(s)
                    .map(Value::Int)
                    .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string()))
            }
            BinaryOp::Shr => {
                let s = b.to_u64().and_then(|v| u32::try_from(v).ok()).ok_or_else(|| {
                    Diagnostic::at_line(DiagnosticCode::RuntimeError, line, "invalid shift count")
                })?;
                a.shr(s)
                    .map(Value::Int)
                    .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string()))
            }
            BinaryOp::Lt => Ok(Value::Bool(a < b)),
            BinaryOp::LtEq => Ok(Value::Bool(a <= b)),
            BinaryOp::Gt => Ok(Value::Bool(a > b)),
            BinaryOp::GtEq => Ok(Value::Bool(a >= b)),
            _ => unreachable!(),
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
            BinaryOp::Lt => Ok(Value::Bool(a < b)),
            BinaryOp::LtEq => Ok(Value::Bool(a <= b)),
            BinaryOp::Gt => Ok(Value::Bool(a > b)),
            BinaryOp::GtEq => Ok(Value::Bool(a >= b)),
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "unsupported t1 operator",
            )),
        },
        (Value::T32(a), Value::T32(b)) => match op {
            BinaryOp::Add => a
                .checked_add(b)
                .map(Value::T32)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::Sub => a
                .checked_sub(b)
                .map(Value::T32)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::Mul => a
                .checked_mul(b)
                .map(Value::T32)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::Div => Ratio::new(BigInt::from_i128(a.value()), BigInt::from_i128(b.value()))
                .map(Value::Ratio)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::FloorDiv => a
                .checked_div_floor(b)
                .map(Value::T32)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::Mod => a
                .checked_rem_floor(b)
                .map(Value::T32)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
            BinaryOp::Lt => Ok(Value::Bool(a < b)),
            BinaryOp::LtEq => Ok(Value::Bool(a <= b)),
            BinaryOp::Gt => Ok(Value::Bool(a > b)),
            BinaryOp::GtEq => Ok(Value::Bool(a >= b)),
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "unsupported t32 operator",
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
            BinaryOp::Lt => Ok(Value::Bool(a < b)),
            BinaryOp::LtEq => Ok(Value::Bool(a <= b)),
            BinaryOp::Gt => Ok(Value::Bool(a > b)),
            BinaryOp::GtEq => Ok(Value::Bool(a >= b)),
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "unsupported ratio operator",
            )),
        },
        (Value::QExtRatio(a), Value::QExtRatio(b)) => match op {
            BinaryOp::Add => Ok(Value::QExtRatio(a.checked_add(&b))),
            BinaryOp::Sub => Ok(Value::QExtRatio(a.checked_sub(&b))),
            BinaryOp::Mul => Ok(Value::QExtRatio(a.checked_mul(&b))),
            BinaryOp::Div => a
                .checked_div(&b)
                .map(Value::QExtRatio)
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())),
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

fn eval_field(base: Value, field: &str, line: usize) -> DecResult<Value> {
    if let Value::Struct(sv) = base {
        return sv.fields.get(field).cloned().ok_or_else(|| {
            Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!("no field .{field} on struct {}", sv.name),
            )
        });
    }
    match (base, field) {
        (Value::SimResult(s), "model_status") => Ok(Value::Str(s.model_status)),
        (Value::SimResult(s), "model_status_bytes") => {
            Ok(Value::Bytes(s.model_status.into_bytes()))
        }
        (Value::SimResult(s), "chamber_name") => Ok(Value::Str(s.chamber_name)),
        (Value::SimResult(s), "chamber_name_bytes") => {
            Ok(Value::Bytes(s.chamber_name.into_bytes()))
        }
        (Value::SimResult(s), "final_state_hash") => Ok(Value::Str(s.final_state_hash)),
        (Value::SimResult(s), "final_state_hash_bytes") => {
            Ok(Value::Bytes(s.final_state_hash.into_bytes()))
        }
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

        (Value::ChamberView(snap), "model_status") => Ok(Value::Str(snap.model_status)),
        (Value::ChamberView(snap), "model_status_bytes") => {
            Ok(Value::Bytes(snap.model_status.into_bytes()))
        }
        (Value::ChamberView(snap), "chamber_name") => Ok(Value::Str(snap.chamber_name)),
        (Value::ChamberView(snap), "chamber_name_bytes") => {
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

        (Value::RecipientOutcome(o), "stage") => Ok(Value::Str(
            eval_recipient_stage_name(o.stage).to_string(),
        )),
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
        (Value::CadenceAssociation(a), "wavelet_id") => Ok(Value::Str(a.wavelet_id)),
        (Value::CadenceAssociation(a), "wavelet_id_bytes") => {
            Ok(Value::Bytes(a.wavelet_id.into_bytes()))
        }
        (Value::CadenceAssociation(a), "reference_id") => Ok(Value::Str(a.reference_id)),
        (Value::CadenceAssociation(a), "reference_id_bytes") => {
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
    }
}

enum EvalHoTarget {
    UserFn(String, String),
    Builtin(String),
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

fn eval_builtin(name: &str, args: Vec<Value>, line: usize) -> DecResult<Value> {
    let err = |e: aod_core::error::CoreError| {
        Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())
    };
    if name == "array" {
        if args.is_empty() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "array(...) requires at least 1 argument",
            ));
        }
        return Ok(Value::Array(args));
    }

    match (name, args.as_slice()) {
        ("t1", [Value::Int(n)]) => T1::from_bigint(n).map(Value::T1).map_err(err),
        ("t1", [Value::T1(v)]) => Ok(Value::T1(*v)),

        ("t9", [Value::Int(n)]) => T9::from_bigint(n).map(Value::T9).map_err(err),
        ("t9", [Value::T1(v)]) => T9::new(v.value() as i64).map(Value::T9).map_err(err),
        ("t9", [Value::U8(v)]) => T9::new(*v as i64).map(Value::T9).map_err(err),
        ("t9", [Value::T9(v)]) => Ok(Value::T9(*v)),
        ("t9", [Value::T32(v)]) => {
            let n = i64::try_from(v.value()).map_err(|_| {
                Diagnostic::at_line(DiagnosticCode::RuntimeError, line, "t9 overflow")
            })?;
            T9::new(n).map(Value::T9).map_err(err)
        }

        ("t32", [Value::Int(n)]) => T32::from_bigint(n).map(Value::T32).map_err(err),
        ("t32", [Value::T1(v)]) => T32::new(v.value() as i128).map(Value::T32).map_err(err),
        ("t32", [Value::U8(v)]) => T32::new(*v as i128).map(Value::T32).map_err(err),
        ("t32", [Value::T9(v)]) => T32::new(v.value() as i128).map(Value::T32).map_err(err),
        ("t32", [Value::T32(v)]) => Ok(Value::T32(*v)),
        ("t32", [Value::B64(bits)]) => T32::unpack(*bits).map(Value::T32).map_err(err),

        ("q4", [Value::Int(n)]) => Q4::from_bigint(n).map(Value::Q4).map_err(err),
        ("q4", [Value::U8(v)]) => Q4::new(*v).map(Value::Q4).map_err(err),
        ("q4", [Value::Q4(v)]) => Ok(Value::Q4(*v)),

        ("u8", [Value::Int(n)]) => n
            .to_u64()
            .and_then(|v| u8::try_from(v).ok())
            .map(Value::U8)
            .ok_or_else(|| {
                Diagnostic::at_line(DiagnosticCode::RuntimeError, line, format!("u8 overflow: {n}"))
            }),
        ("u8", [Value::U8(v)]) => Ok(Value::U8(*v)),
        ("u8", [Value::Bool(b)]) => Ok(Value::U8(u8::from(*b))),
        ("u8", [Value::Q4(q)]) => Ok(Value::U8(q.value())),
        ("u8", [Value::T9(t)]) => u8::try_from(t.value()).map(Value::U8).map_err(|_| {
            Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!("u8 overflow: {}", t.value()),
            )
        }),
        ("u8", [Value::T32(t)]) => u8::try_from(t.value()).map(Value::U8).map_err(|_| {
            Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!("u8 overflow: {}", t.value()),
            )
        }),
        ("u8", [Value::B64(w)]) => u8::try_from(*w).map(Value::U8).map_err(|_| {
            Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!("u8 overflow: {w}"),
            )
        }),

        ("b64", [Value::Int(n)]) => n.to_u64().map(Value::B64).ok_or_else(|| {
            Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!("b64 overflow: {n}"),
            )
        }),
        ("b64", [Value::U8(v)]) => Ok(Value::B64(*v as u64)),
        ("b64", [Value::Q4(q)]) => Ok(Value::B64(q.value() as u64)),
        ("b64", [Value::B64(w)]) => Ok(Value::B64(*w)),
        ("b64", [Value::T32(t)]) => Ok(Value::B64(t.pack())),
        ("b64", [Value::T9(t)]) => Ok(Value::B64(t.pack() as u64)),

        ("int", [Value::Int(n)]) => Ok(Value::Int(n.clone())),
        ("int", [Value::U8(v)]) => Ok(Value::Int(BigInt::from_u64(*v as u64))),
        ("int", [Value::B64(w)]) => Ok(Value::Int(BigInt::from_u64(*w))),
        ("int", [Value::T1(v)]) => Ok(Value::Int(BigInt::from_i64(v.value() as i64))),
        ("int", [Value::T9(v)]) => Ok(Value::Int(BigInt::from_i64(v.value() as i64))),
        ("int", [Value::T32(v)]) => Ok(Value::Int(BigInt::from_i128(v.value()))),
        ("int", [Value::Q4(v)]) => Ok(Value::Int(BigInt::from_u64(v.value() as u64))),
        ("int", [Value::Ratio(r)]) => r.to_int_exact().map(Value::Int).map_err(err),
        ("ratio", [Value::Int(a), Value::Int(b)]) => {
            Ratio::new(a.clone(), b.clone()).map(Value::Ratio).map_err(err)
        }
        ("ratio", [Value::Int(a)]) => Ok(Value::Ratio(Ratio::from_int(a.clone()))),
        ("pack", [Value::T32(v)]) => Ok(Value::Int(BigInt::from_u64(v.pack()))),
        ("pack", [Value::T9(v)]) => Ok(Value::Int(BigInt::from_u64(v.pack() as u64))),
        ("unpack", [Value::Int(bits)]) => T32::unpack_bigint(bits).map(Value::T32).map_err(err),
        ("unpack_t9", [Value::Int(bits)]) => T9::unpack_bigint(bits).map(Value::T9).map_err(err),
        ("unpack_t9", [Value::B64(bits)]) => T9::unpack(*bits).map(Value::T9).map_err(err),
        ("neg", [v]) => eval_unary(UnaryOp::Neg, v.clone(), line),
        ("bitnot", [Value::B64(w)]) => Ok(Value::B64(!*w)),
        ("popcnt", [Value::B64(w)]) => Ok(Value::Int(BigInt::from_u64(w.count_ones() as u64))),
        ("is_hamming1", [Value::Q4(a), Value::Q4(b)]) => Ok(Value::Bool(a.is_hamming1(*b))),
        ("popcnt_xor", [Value::Q4(a), Value::Q4(b)]) => {
            Ok(Value::Int(BigInt::from_u64(a.popcnt_xor(*b) as u64)))
        }
        ("superpose_cancel", [Value::T1(a), Value::T1(b)]) => Ok(Value::T1(a.superpose_cancel(*b))),
        ("superpose_cancel", [Value::T32(a), Value::T32(b)]) => {
            Ok(Value::T32(a.superpose_cancel(*b)))
        }

        ("bytes", [Value::List(items)]) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                match it {
                    Value::U8(b) => out.push(*b),
                    other => {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::RuntimeError,
                            line,
                            format!("bytes([u8]) expected u8 element, got {}", other.type_of()),
                        ));
                    }
                }
            }
            Ok(Value::Bytes(out))
        }
        ("bytes", [Value::Str(s)]) => Ok(Value::Bytes(s.as_bytes().to_vec())),
        ("bytes", [Value::Bytes(bs)]) => Ok(Value::Bytes(bs.clone())),
        ("bytes", [Value::B64(w)]) => Ok(Value::Bytes(w.to_le_bytes().to_vec())),
        ("bytes", [Value::Addr8(a)]) => Ok(Value::Bytes(a.render().into_bytes())),
        ("slice_bytes", [Value::Bytes(bs), Value::Int(s), Value::Int(e)]) => {
            let si = s.to_i64().ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("slice_bytes start {s} out of bounds"),
                )
            })?;
            let ei = e.to_i64().ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("slice_bytes end {e} out of bounds"),
                )
            })?;
            if si < 0 || ei < si || (ei as usize) > bs.len() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("slice_bytes({si}, {ei}) out of bounds for length {}", bs.len()),
                ));
            }
            Ok(Value::Bytes(bs[(si as usize)..(ei as usize)].to_vec()))
        }
        ("concat_bytes", [Value::Bytes(a), Value::Bytes(b)]) => {
            let mut out = Vec::with_capacity(a.len() + b.len());
            out.extend_from_slice(a);
            out.extend_from_slice(b);
            Ok(Value::Bytes(out))
        }
        ("sha256_bytes", [Value::Bytes(bs)]) => Ok(Value::Bytes(Sha256::digest(bs).to_vec())),
        ("hex_bytes", [Value::Bytes(bs)]) => {
            const HEX: &[u8; 16] = b"0123456789abcdef";
            let mut out = Vec::with_capacity(bs.len() * 2);
            for &b in bs {
                out.push(HEX[(b >> 4) as usize]);
                out.push(HEX[(b & 0x0f) as usize]);
            }
            Ok(Value::Bytes(out))
        }
        ("array_set", [Value::Array(items), Value::Int(idx), val]) => {
            let i = idx.to_i64().ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("array index {idx} out of bounds"),
                )
            })?;
            if i < 0 || (i as usize) >= items.len() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("array index {i} out of bounds"),
                ));
            }
            let mut cloned = items.clone();
            cloned[i as usize] = val.clone();
            Ok(Value::Array(cloned))
        }
        ("select", [Value::Bool(c), a, b]) | ("select", [Value::QExtBool(c), a, b]) => {
            if *c {
                Ok(a.clone())
            } else {
                Ok(b.clone())
            }
        }
        ("verify_sheddic_route" | "audit_sheddic_route", [Value::Int(xs), Value::U8(enc)]) => {
            Ok(Value::Bool(
                !xs.is_positive() || (*enc >= 2 && enc.is_multiple_of(2)),
            ))
        }
        ("verify_frg_cross_return", [Value::Int(t0), Value::Int(tx), Value::Int(cx)]) => {
            Ok(Value::Bool(&(tx - t0) == cx))
        }

        ("trace", [Value::Str(sc), Value::Str(lin), Value::Q4(st)]) => {
            Trace::new(sc.clone(), lin.clone(), *st)
                .map(|t| Value::Trace(Box::new(t)))
                .map_err(err)
        }
        ("trace", [Value::Bytes(sc_b), Value::Bytes(lin_b), Value::Q4(st)]) => {
            let sc = bytes_to_utf8(sc_b, "trace(bytes, bytes, q4)", line)?;
            let lin = bytes_to_utf8(lin_b, "trace(bytes, bytes, q4)", line)?;
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
        ("fold", [Value::Trace(a), Value::Trace(b), Value::Str(sc), Value::Str(lin)]) => {
            Fusion::fold(a, b, sc.clone(), lin.clone())
                .map(|f| Value::Fusion(Box::new(f)))
                .map_err(err)
        }
        ("fold", [Value::Trace(a), Value::Trace(b), Value::Bytes(sc_b), Value::Bytes(lin_b)]) => {
            let sc = bytes_to_utf8(sc_b, "fold(..., bytes, bytes)", line)?;
            let lin = bytes_to_utf8(lin_b, "fold(..., bytes, bytes)", line)?;
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
        ("coupling", [Value::Trace(a), Value::Trace(b), Value::Str(w)]) => {
            Coupling::new(a, b, w.clone())
                .map(|c| Value::Coupling(Box::new(c)))
                .map_err(err)
        }
        ("coupling", [Value::Trace(a), Value::Trace(b), Value::Bytes(wb)]) => {
            let w = bytes_to_utf8(wb, "coupling(..., bytes)", line)?;
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

        // v2 builtins
        ("addr8", [Value::Str(s)]) => Addr8::parse(s).map(Value::Addr8).map_err(err),
        ("addr8", [Value::Bytes(bs)]) => {
            let s = bytes_to_utf8(bs, "addr8(bytes)", line)?;
            Addr8::parse(&s).map(Value::Addr8).map_err(err)
        }
        ("support", [Value::Str(s)]) => SupportForm::parse(s).map(Value::Support).map_err(err),
        ("support", [Value::Bytes(bs)]) => {
            let s = bytes_to_utf8(bs, "support(bytes)", line)?;
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
        ("len", [Value::Str(s)]) => Ok(Value::Int(BigInt::from_u64(s.chars().count() as u64))),
        ("len", [Value::Bytes(bs)]) => Ok(Value::Int(BigInt::from_u64(bs.len() as u64))),
        ("len", [Value::List(xs)]) | ("len", [Value::Array(xs)]) => {
            Ok(Value::Int(BigInt::from_u64(xs.len() as u64)))
        }
        ("len", [Value::Map(m)]) => Ok(Value::Int(BigInt::from_u64(m.len() as u64))),
        ("contains", [Value::Map(m), key_val]) => {
            let k = MapKey::from_value(key_val, line)?;
            Ok(Value::Bool(m.contains_key(&k)))
        }
        (
            "window_dedup_count",
            [Value::Str(lin), Value::Str(scope), Value::List(cut_vals), Value::List(win_vals)],
        ) => {
            let mut cuts = Vec::with_capacity(cut_vals.len());
            for cv in cut_vals {
                cuts.push(extract_i64(cv.clone(), line)?);
            }
            let (witnesses, reg) =
                declare_synthetic_lineage_registry(lin, scope, &cuts, 0, "native_cut", None)
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
                    parent_lineage: lin,
                    explicit_parent: Some(pdesc),
                    ancestry: vec![format!("anc:{lin}"), format!("w{idx}")],
                    window: (u, v),
                    sigma_seat: SeatState::SeatedDuad,
                    sigma_role: WaveletRole::CoupledSegment,
                    records: vec![format!("rec:w{idx}")],
                    return_witnesses: selected,
                    signed_current: BigInt::zero(),
                    is_admitted_wave: false,
                    scope_id: scope,
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
                deduplicated_return_count(&wavelets, Some(pdesc), None, scope, None, Some(&reg))
                    .map_err(err)?;
            Ok(Value::Int(BigInt::from_u64(count as u64)))
        }
        (
            "native_cadence_ratio",
            [
                Value::Str(id_a),
                Value::Int(q_a),
                Value::Str(id_r),
                Value::Int(q_r),
                Value::Bool(coupled),
                Value::Int(contacts),
            ],
        ) => {
            let bc = contacts.to_u64().ok_or_else(|| {
                Diagnostic::at_line(DiagnosticCode::RuntimeError, line, "invalid bound_contacts")
            })? as usize;
            native_cadence_ratio(id_a, q_a, id_r, q_r, *coupled, bc)
                .map(Value::Ratio)
                .map_err(err)
        }
        (
            "single_contact_association",
            [
                Value::Str(wid),
                Value::Str(pid),
                Value::Str(rid),
                Value::Window(u, v),
                Value::Int(contacts),
            ],
        ) => {
            let bc = contacts.to_u64().ok_or_else(|| {
                Diagnostic::at_line(DiagnosticCode::RuntimeError, line, "invalid bound_contacts")
            })? as usize;
            SingleContactStageAssociation::new(wid.clone(), pid.clone(), rid.clone(), (*u, *v), bc)
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

        // Pure persistent container helpers (`P0-2`, `AC-05`)
        ("map_put", [Value::Map(m), key_val, val]) => {
            let k = MapKey::from_value(key_val, line)?;
            let mut cloned = (**m).clone();
            cloned.insert(k, val.clone());
            Ok(Value::Map(Box::new(cloned)))
        }
        ("map_remove", [Value::Map(m), key_val]) => {
            let k = MapKey::from_value(key_val, line)?;
            let mut cloned = (**m).clone();
            cloned.remove(&k);
            Ok(Value::Map(Box::new(cloned)))
        }
        ("map_get_or", [Value::Map(m), key_val, default_val]) => {
            let k = MapKey::from_value(key_val, line)?;
            Ok(m.get(&k).cloned().unwrap_or_else(|| default_val.clone()))
        }
        ("map_keys", [Value::Map(m)]) => {
            Ok(Value::List(m.keys().map(MapKey::to_value).collect()))
        }
        ("map_entries", [Value::Map(m)]) => Ok(Value::List(
            m.iter()
                .map(|(k, v)| Value::Tuple(vec![k.to_value(), v.clone()]))
                .collect(),
        )),
        ("list_set", [Value::List(xs), Value::Int(idx), val]) => {
            let i = extract_i64(Value::Int(idx.clone()), line)?;
            if i < 0 || (i as usize) >= xs.len() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("list index {i} out of bounds"),
                ));
            }
            let mut cloned = xs.clone();
            cloned[i as usize] = val.clone();
            Ok(Value::List(cloned))
        }
        ("list_push", [Value::List(xs), val]) => {
            let mut cloned = xs.clone();
            cloned.push(val.clone());
            Ok(Value::List(cloned))
        }
        (
            "concat_seq",
            [Value::List(a) | Value::Array(a), Value::List(b) | Value::Array(b)],
        ) => {
            let mut out = Vec::with_capacity(a.len() + b.len());
            out.extend_from_slice(a);
            out.extend_from_slice(b);
            Ok(Value::List(out))
        }
        ("slice_seq", [Value::List(xs) | Value::Array(xs), Value::Int(start), Value::Int(end)]) => {
            let s = extract_i64(Value::Int(start.clone()), line)?;
            let e = extract_i64(Value::Int(end.clone()), line)?;
            if s < 0 || e < s || (e as usize) > xs.len() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!(
                        "slice_seq({s}, {e}) out of bounds for length {}",
                        xs.len()
                    ),
                ));
            }
            Ok(Value::List(xs[(s as usize)..(e as usize)].to_vec()))
        }

        // M3 wavelet bytes overloads (`P0-4`, `AC-03`)
        (
            "window_dedup_count",
            [
                Value::Bytes(lin_b),
                Value::Bytes(scope_b),
                Value::List(cut_vals),
                Value::List(win_vals),
            ],
        ) => {
            let lin = bytes_to_utf8(lin_b, "window_dedup_count(bytes, ...)", line)?;
            let scope = bytes_to_utf8(scope_b, "window_dedup_count(bytes, ...)", line)?;
            eval_builtin(
                "window_dedup_count",
                vec![
                    Value::Str(lin),
                    Value::Str(scope),
                    Value::List(cut_vals.clone()),
                    Value::List(win_vals.clone()),
                ],
                line,
            )
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
            let ida = bytes_to_utf8(ida_b, "native_cadence_ratio(bytes, ...)", line)?;
            let idr = bytes_to_utf8(idr_b, "native_cadence_ratio(bytes, ...)", line)?;
            eval_builtin(
                "native_cadence_ratio",
                vec![
                    Value::Str(ida),
                    Value::Int(q_a.clone()),
                    Value::Str(idr),
                    Value::Int(q_r.clone()),
                    Value::Bool(*coupled),
                    Value::Int(contacts.clone()),
                ],
                line,
            )
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
            let wid = bytes_to_utf8(wid_b, "single_contact_association(bytes, ...)", line)?;
            let pid = bytes_to_utf8(pid_b, "single_contact_association(bytes, ...)", line)?;
            let rid = bytes_to_utf8(rid_b, "single_contact_association(bytes, ...)", line)?;
            eval_builtin(
                "single_contact_association",
                vec![
                    Value::Str(wid),
                    Value::Str(pid),
                    Value::Str(rid),
                    Value::Window(*u, *v),
                    Value::Int(contacts.clone()),
                ],
                line,
            )
        }

        // Phase D2 exact scalar & rational intrinsics
        ("num", [Value::Ratio(r)]) => Ok(Value::Int(r.num().clone())),
        ("den", [Value::Ratio(r)]) => Ok(Value::Int(r.den().clone())),
        ("abs", [Value::Int(n)]) => Ok(Value::Int(n.abs())),
        ("abs", [Value::T1(v)]) => Ok(Value::T1(v.abs())),
        ("abs", [Value::T9(v)]) => Ok(Value::T9(v.abs())),
        ("abs", [Value::T32(v)]) => Ok(Value::T32(v.abs())),
        ("abs", [Value::Ratio(r)]) => Ok(Value::Ratio(r.abs())),
        ("abs", [Value::QExtRatio(q)]) => Ok(Value::QExtRatio(q.abs())),
        ("signum", [Value::Int(n)]) => Ok(Value::T1(
            T1::new(n.signum()).expect("BigInt::signum in {-1,0,1}"),
        )),
        ("signum", [Value::T1(v)]) => Ok(Value::T1(v.signum())),
        ("signum", [Value::T9(v)]) => Ok(Value::T1(v.signum())),
        ("signum", [Value::T32(v)]) => Ok(Value::T1(v.signum())),
        ("signum", [Value::Ratio(r)]) => Ok(Value::T1(r.signum())),
        ("min", [a, b]) => {
            if cmp_values_ord(a, b, line)?.is_le() {
                Ok(a.clone())
            } else {
                Ok(b.clone())
            }
        }
        ("max", [a, b]) => {
            if cmp_values_ord(a, b, line)?.is_ge() {
                Ok(a.clone())
            } else {
                Ok(b.clone())
            }
        }
        ("clamp", [x, lo, hi]) => {
            if cmp_values_ord(lo, hi, line)?.is_gt() {
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
            if cmp_values_ord(x, lo, line)?.is_lt() {
                Ok(lo.clone())
            } else if cmp_values_ord(x, hi, line)?.is_gt() {
                Ok(hi.clone())
            } else {
                Ok(x.clone())
            }
        }
        ("gcd", [Value::Int(a), Value::Int(b)]) => Ok(Value::Int(a.gcd(b))),
        ("gcd", [Value::T9(a), Value::T9(b)]) => Ok(Value::T9(a.gcd(*b))),
        ("gcd", [Value::T32(a), Value::T32(b)]) => Ok(Value::T32(a.gcd(*b))),
        ("lcm", [Value::Int(a), Value::Int(b)]) => Ok(Value::Int(a.lcm(b))),
        ("lcm", [Value::T9(a), Value::T9(b)]) => a.checked_lcm(*b).map(Value::T9).map_err(err),
        ("lcm", [Value::T32(a), Value::T32(b)]) => a.checked_lcm(*b).map(Value::T32).map_err(err),
        ("pow", [Value::Int(base), Value::Int(exp)]) => {
            base.checked_pow_bigint(exp).map(Value::Int).map_err(err)
        }
        ("pow", [Value::T9(base), Value::Int(exp)]) => {
            let e = checked_eval_pow_exp(exp, "t9", line)?;
            base.checked_pow(e).map(Value::T9).map_err(err)
        }
        ("pow", [Value::T9(base), Value::T9(exp)]) => {
            let e = checked_eval_pow_exp(&BigInt::from_i64(exp.value() as i64), "t9", line)?;
            base.checked_pow(e).map(Value::T9).map_err(err)
        }
        ("pow", [Value::T32(base), Value::Int(exp)]) => {
            let e = checked_eval_pow_exp(exp, "t32", line)?;
            base.checked_pow(e).map(Value::T32).map_err(err)
        }
        ("pow", [Value::T32(base), Value::T32(exp)]) => {
            let e = checked_eval_pow_exp(&BigInt::from_i128(exp.value()), "t32", line)?;
            base.checked_pow(e).map(Value::T32).map_err(err)
        }
        ("pow", [Value::Ratio(base), Value::Int(exp)]) => {
            base.checked_pow_bigint(exp).map(Value::Ratio).map_err(err)
        }

        // Quarantined aod::qext builtins
        ("aod::qext::ext_decimal", [Value::Str(s)]) => {
            parse_decimal_exact(s).map(Value::QExtRatio).map_err(err)
        }
        ("aod::qext::ext_decimal", [Value::Bytes(bs)]) => {
            let s = bytes_to_utf8(bs, "ext_decimal(bytes)", line)?;
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
        ("aod::qext::qext_render", [Value::QExtRatio(q)]) => Ok(Value::QExtStr(q.render_exact())),
        ("aod::qext::qext_round_half_even", [Value::QExtRatio(q), Value::Int(d)]) => {
            let dec = checked_eval_qext_decimals(d, line)?;
            q.round_half_even(dec).map(Value::QExtRatio).map_err(err)
        }
        ("aod::qext::qext_format_fixed", [Value::QExtRatio(q), Value::Int(d)]) => {
            let dec = checked_eval_qext_decimals(d, line)?;
            q.format_fixed(dec).map(Value::QExtStr).map_err(err)
        }
        (
            "aod::qext::qext_to_bytes_exact" | "aod::qext::qext_render_bytes",
            [Value::QExtRatio(q)],
        ) => Ok(Value::QExtBytes(q.render_exact().into_bytes())),
        (
            "aod::qext::qext_format_bytes_fixed" | "aod::qext::qext_format_fixed_bytes",
            [Value::QExtRatio(q), Value::Int(d)],
        ) => {
            let dec = checked_eval_qext_decimals(d, line)?;
            q.format_fixed(dec)
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

fn checked_eval_pow_exp(exp: &BigInt, label: &str, line: usize) -> DecResult<u32> {
    if exp.is_negative() {
        return Err(Diagnostic::at_line(
            DiagnosticCode::RuntimeError,
            line,
            format!("pow({label}, exp) requires non-negative exponent, got {exp}"),
        ));
    }
    let e = exp
        .to_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| {
            Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!(
                    "pow({label}, exp) exponent {exp} exceeds safety limit {}",
                    BigInt::MAX_POW_EXP
                ),
            )
        })?;
    if e > BigInt::MAX_POW_EXP {
        return Err(Diagnostic::at_line(
            DiagnosticCode::RuntimeError,
            line,
            format!(
                "pow({label}, exp) exponent {e} exceeds safety limit {}",
                BigInt::MAX_POW_EXP
            ),
        ));
    }
    Ok(e)
}

fn bytes_to_utf8(bs: &[u8], ctx_name: &str, line: usize) -> DecResult<String> {
    std::str::from_utf8(bs)
        .map(str::to_string)
        .map_err(|_| {
            Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!("{ctx_name}: byte sequence is not valid UTF-8"),
            )
        })
}

fn checked_eval_qext_decimals(d: &BigInt, line: usize) -> DecResult<usize> {
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
    Ok(dec_i as usize)
}

fn cmp_values_ord(a: &Value, b: &Value, line: usize) -> DecResult<std::cmp::Ordering> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),
        (Value::U8(x), Value::U8(y)) => Ok(x.cmp(y)),
        (Value::B64(x), Value::B64(y)) => Ok(x.cmp(y)),
        (Value::T1(x), Value::T1(y)) => Ok(x.cmp(y)),
        (Value::T9(x), Value::T9(y)) => Ok(x.cmp(y)),
        (Value::T32(x), Value::T32(y)) => Ok(x.cmp(y)),
        (Value::Ratio(x), Value::Ratio(y)) => Ok(x.cmp(y)),
        (Value::QExtRatio(x), Value::QExtRatio(y)) => Ok(x.cmp(y)),
        _ => Err(Diagnostic::at_line(
            DiagnosticCode::RuntimeError,
            line,
            "incomparable values in min/max/clamp",
        )),
    }
}
