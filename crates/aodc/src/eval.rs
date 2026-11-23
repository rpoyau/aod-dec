use std::collections::{BTreeMap, BTreeSet};

use aod_core::bigint::BigInt;
use aod_core::ledger::{probability, SadarPromotion};
use aod_core::qext::{parse_decimal_exact, QExtRatio};
use aod_core::ratio::Ratio;
use aod_core::support::SupportForm;
use aod_core::ternary::{Addr8, Q4, T1, T32};
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
    Int(BigInt),
    Bool(bool),
    Str(String),
    T1(T1),
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
}

impl MapKey {
    pub fn from_value(v: &Value, line: usize) -> DecResult<Self> {
        match v {
            Value::Int(n) => Ok(MapKey::Int(n.clone())),
            Value::Bool(b) => Ok(MapKey::Bool(*b)),
            Value::Str(s) => Ok(MapKey::Str(s.clone())),
            Value::T1(t) => Ok(MapKey::T1(*t)),
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
            other => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!("unsupported map key value of type {}", other.type_of()),
            )),
        }
    }

    pub fn to_value(&self) -> Value {
        match self {
            MapKey::Int(n) => Value::Int(n.clone()),
            MapKey::Bool(b) => Value::Bool(*b),
            MapKey::Str(s) => Value::Str(s.clone()),
            MapKey::T1(t) => Value::T1(*t),
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
    Int(BigInt),
    Bool(bool),
    Str(String),
    T1(T1),
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
    Map(Box<BTreeMap<MapKey, Value>>),
    Tuple(Vec<Value>),
    QExtRatio(QExtRatio),
    QExtBool(bool),
    QExtStr(String),
}

impl Value {
    pub fn type_of(&self) -> Type {
        match self {
            Value::Unit => Type::Unit,
            Value::Int(_) => Type::Int,
            Value::Bool(_) => Type::Bool,
            Value::Str(_) => Type::Str,
            Value::T1(_) => Type::T1,
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
            Value::Map(m) => match m.iter().next() {
                Some((k, v)) => Type::Map(Box::new(k.to_value().type_of()), Box::new(v.type_of())),
                None => Type::Map(Box::new(Type::Unknown), Box::new(Type::Unknown)),
            },
            Value::Tuple(elems) => Type::Tuple(elems.iter().map(Value::type_of).collect()),
            Value::QExtRatio(_) => Type::QExt(Box::new(Type::Ratio)),
            Value::QExtBool(_) => Type::QExt(Box::new(Type::Bool)),
            Value::QExtStr(_) => Type::QExt(Box::new(Type::Str)),
        }
    }

    /// Exact v1/v2 deterministic rendering matching `dec_core.py:486-494` byte-for-byte.
    pub fn render(&self) -> String {
        match self {
            Value::Unit => "()".to_string(),
            Value::Int(n) => n.to_decimal_string(),
            Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            Value::Str(s) => s.clone(),
            Value::T1(v) => v.render_v1(),
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
            Value::List(items) => {
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

pub struct Evaluator<'a> {
    checked: &'a CheckedUnit,
    fuel: usize,
    max_depth: usize,
    depth: usize,
    emits: Vec<String>,
    structs: BTreeMap<(String, String), StructDecl>,
    enums: BTreeMap<(String, String), EnumDecl>,
    functions: BTreeMap<(String, String), DecFnDecl>,
    consts: BTreeMap<(String, String), Value>,
    chambers: BTreeMap<(String, String), ChamberDecl>,
    prepares: BTreeMap<(String, String), PrepareDecl>,
    observes: BTreeMap<(String, String), ObserveDecl>,
    qext_modules: BTreeSet<(String, String)>,
}

impl<'a> Evaluator<'a> {
    pub fn new(checked: &'a CheckedUnit, fuel: usize, max_depth: usize) -> DecResult<Self> {
        let mut ev = Self {
            checked,
            fuel,
            max_depth,
            depth: 0,
            emits: Vec::new(),
            structs: BTreeMap::new(),
            enums: BTreeMap::new(),
            functions: BTreeMap::new(),
            consts: BTreeMap::new(),
            chambers: BTreeMap::new(),
            prepares: BTreeMap::new(),
            observes: BTreeMap::new(),
            qext_modules: BTreeSet::new(),
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

    fn base_module_of<'b>(&self, mod_key: &'b str) -> &'b str {
        if let Some((parent, leaf)) = mod_key.rsplit_once("::") {
            if self
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
            .qext_modules
            .contains(&(mod_key.to_string(), ns.to_string()))
        {
            return Some(format!("{mod_key}::{ns}"));
        }
        let base = self.base_module_of(mod_key);
        if base != mod_key
            && self
                .qext_modules
                .contains(&(base.to_string(), ns.to_string()))
        {
            return Some(format!("{base}::{ns}"));
        }
        if let Some(ImportedSymbol::ModuleItem { module, item }) =
            self.imports_for(mod_key).get(ns)
        {
            if self.qext_modules.contains(&(module.clone(), item.clone())) {
                return Some(format!("{module}::{item}"));
            }
        }
        None
    }

    fn resolve_qext_submodule_3seg(&self, mod_key: &str, ns: &str, qmod: &str) -> Option<String> {
        if let Some(ImportedSymbol::ModuleNamespace(m)) = self.imports_for(mod_key).get(ns) {
            if self.qext_modules.contains(&(m.clone(), qmod.to_string())) {
                return Some(format!("{m}::{qmod}"));
            }
        }
        None
    }

    fn index_program(&mut self, mod_key: &str, prog: &Program) -> DecResult<()> {
        for item in &prog.items {
            match item {
                Item::Struct(s) => {
                    self.structs
                        .insert((mod_key.to_string(), s.name.clone()), s.clone());
                }
                Item::Enum(e) => {
                    self.enums
                        .insert((mod_key.to_string(), e.name.clone()), e.clone());
                }
                Item::DecFn(f) => {
                    self.functions
                        .insert((mod_key.to_string(), f.name.clone()), f.clone());
                }
                Item::Chamber(c) => {
                    self.chambers
                        .insert((mod_key.to_string(), c.name.clone()), c.clone());
                }
                Item::Prepare(p) => {
                    self.prepares
                        .insert((mod_key.to_string(), p.name.clone()), p.clone());
                }
                Item::Observe(o) => {
                    self.observes
                        .insert((mod_key.to_string(), o.name.clone()), o.clone());
                }
                Item::QuarantineMod(qm) => {
                    self.qext_modules
                        .insert((mod_key.to_string(), qm.name.clone()));
                    let sub_key = format!("{mod_key}::{}", qm.name);
                    for sub in &qm.items {
                        if let Item::DecFn(f) = sub {
                            self.functions
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
                    self.consts.insert((mod_key.to_string(), c.name.clone()), v);
                }
                Item::QuarantineMod(qm) => {
                    let sub_key = format!("{mod_key}::{}", qm.name);
                    for sub in &qm.items {
                        if let Item::Const(c) = sub {
                            let v = self.eval_expr(&c.value, &sub_key, &scopes)?;
                            self.consts.insert((sub_key.clone(), c.name.clone()), v);
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
            &self.checked.unit.entry_imports
        } else {
            self.checked
                .unit
                .module_imports
                .get(base)
                .unwrap_or(&self.checked.unit.entry_imports)
        }
    }

    fn lookup_user_type(&self, mod_key: &str, name: &str) -> Option<Type> {
        let base = self.base_module_of(mod_key);
        if self.structs.contains_key(&(base.to_string(), name.to_string())) {
            return Some(Type::Struct {
                module: base.to_string(),
                name: name.to_string(),
            });
        }
        if self.enums.contains_key(&(base.to_string(), name.to_string())) {
            return Some(Type::Enum {
                module: base.to_string(),
                name: name.to_string(),
            });
        }
        if let Some(ImportedSymbol::ModuleItem { module, item }) =
            self.imports_for(mod_key).get(name)
        {
            if self.structs.contains_key(&(module.clone(), item.clone())) {
                return Some(Type::Struct {
                    module: module.clone(),
                    name: item.clone(),
                });
            }
            if self.enums.contains_key(&(module.clone(), item.clone())) {
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
            if self.enums.contains_key(&(base.to_string(), path[0].clone())) {
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
        let ed = self.enums.get(&(emod.clone(), ename.clone()))?;
        if ed.variants.iter().any(|v| v.name == vname) {
            Some((emod, ename, vname))
        } else {
            None
        }
    }

    fn resolve_struct_decl(&self, mod_key: &str, path: &[String]) -> Option<(String, StructDecl)> {
        if path.len() == 1 {
            let base = self.base_module_of(mod_key);
            if let Some(s) = self.structs.get(&(base.to_string(), path[0].clone())) {
                return Some((base.to_string(), s.clone()));
            }
            if let Some(ImportedSymbol::ModuleItem { module, item }) =
                self.imports_for(mod_key).get(&path[0])
            {
                if let Some(s) = self.structs.get(&(module.clone(), item.clone())) {
                    return Some((module.clone(), s.clone()));
                }
            }
        } else if path.len() == 2 {
            if let Some(ImportedSymbol::ModuleNamespace(m)) =
                self.imports_for(mod_key).get(&path[0])
            {
                if let Some(s) = self.structs.get(&(m.clone(), path[1].clone())) {
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
            Pattern::IntLit(n) => matches!(val, Value::Int(v) if v == n),
            Pattern::BoolLit(b) => matches!(val, Value::Bool(v) if v == b),
            Pattern::StrLit(s) => matches!(val, Value::Str(v) if v == s),
            Pattern::TritLit(t) => matches!(val, Value::T1(v) if v == t),
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
        let decl = match self.functions.get(&(mod_key.to_string(), name.to_string())) {
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
            DecFnKind::MultiArm { arms, .. } => {
                let pos_vals: Vec<Value> = args.into_iter().map(|(_, v)| v).collect();
                let mut matched_arm = None;
                let mut matched_scope = BTreeMap::new();

                // Deterministic top-to-bottom type + pattern + guard dispatch!
                for arm in arms {
                    if arm.params.len() != pos_vals.len() {
                        continue;
                    }
                    let mut scope = BTreeMap::new();
                    let mut all_match = true;
                    for (param, val) in arm.params.iter().zip(&pos_vals) {
                        match param {
                            DecArmParam::TypedVar { ty, name } => {
                                let expected = Type::resolve_with(
                                    ty,
                                    self.checked.unit.compat,
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
                                    self.checked.unit.compat,
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
            ExprKind::IntLit(n) => Ok(Value::Int(n.clone())),
            ExprKind::BoolLit(b) => Ok(Value::Bool(*b)),
            ExprKind::StrLit(s) => Ok(Value::Str(s.clone())),
            ExprKind::RatioLit(r) => Ok(Value::Ratio(r.clone())),
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
                if let Some(v) = self.consts.get(&(mod_key.to_string(), name.clone())) {
                    return Ok(v.clone());
                }
                let base = self.base_module_of(mod_key).to_string();
                if base != mod_key {
                    if let Some(v) = self.consts.get(&(base, name.clone())) {
                        return Ok(v.clone());
                    }
                }
                if let Some(ImportedSymbol::ModuleItem { module, item }) =
                    self.imports_for(mod_key).get(name).cloned()
                {
                    if let Some(v) = self.consts.get(&(module, item)) {
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
                        if let Some(v) = self.consts.get(&(qsub, segs[1].clone())) {
                            return Ok(v.clone());
                        }
                    }
                    if let Some(ImportedSymbol::ModuleNamespace(target_mod)) =
                        self.imports_for(mod_key).get(&segs[0]).cloned()
                    {
                        if let Some(v) = self.consts.get(&(target_mod, segs[1].clone())) {
                            return Ok(v.clone());
                        }
                    }
                } else if segs.len() == 3 {
                    if let Some(qsub) =
                        self.resolve_qext_submodule_3seg(mod_key, &segs[0], &segs[1])
                    {
                        if let Some(v) = self.consts.get(&(qsub, segs[2].clone())) {
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
                        "indexing requires list or map",
                    )),
                }
            }
            ExprKind::Call { callee, args } => {
                self.eval_call(callee, args, mod_key, scopes, expr.line)
            }
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
        let mut vals = Vec::with_capacity(args.len());
        for a in args {
            vals.push((a.name.clone(), self.eval_expr(&a.value, mod_key, scopes)?));
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

        // Struct constructor call `Foo(1, 2)` or `Foo(x: 1, y: 2)`
        let struct_path_opt: Option<Vec<String>> = match &callee.kind {
            ExprKind::Ident(n) => Some(vec![n.clone()]),
            ExprKind::Path(segs) => Some(segs.clone()),
            _ => None,
        };
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

        let base_mod = self.base_module_of(mod_key).to_string();

        match &callee.kind {
            ExprKind::Ident(name) => {
                if self
                    .functions
                    .contains_key(&(mod_key.to_string(), name.clone()))
                {
                    return self.call_user_fn(mod_key, name, vals, line);
                }
                if base_mod != mod_key
                    && self
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
                        if self.functions.contains_key(&(qsub.clone(), segs[1].clone())) {
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
                        if self.functions.contains_key(&(qsub.clone(), segs[2].clone())) {
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

        for _ in 0..ticks {
            self.tick()?;
            state
                .step()
                .map_err(|e| Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string()))?;
            let snap = state.snapshot();
            for obs in &mut decl_observers {
                obs.observe_snapshot(&snap);
            }
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
    if &actual == expected {
        return true;
    }
    match (&actual, expected) {
        (Type::List(a), Type::List(_)) if **a == Type::Unknown => true,
        (Type::Map(ak, av), Type::Map(_, _))
            if **ak == Type::Unknown && **av == Type::Unknown =>
        {
            true
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
            | Value::T1(_)
            | Value::T32(_)
            | Value::Ratio(_)
            | Value::QExtRatio(_)),
        ) => Ok(v),
        (UnaryOp::Neg, Value::Int(n)) => Ok(Value::Int(-n)),
        (UnaryOp::Neg, Value::T1(t)) => Ok(Value::T1(t.checked_neg())),
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
            _ => Ok(Value::Bool(left == right)),
        };
    }
    if op == BinaryOp::NotEq {
        return match (&left, &right) {
            (Value::QExtRatio(a), Value::QExtRatio(b)) => Ok(Value::QExtBool(a != b)),
            (Value::QExtBool(a), Value::QExtBool(b)) => Ok(Value::QExtBool(a != b)),
            (Value::QExtStr(a), Value::QExtStr(b)) => Ok(Value::QExtBool(a != b)),
            _ => Ok(Value::Bool(left != right)),
        };
    }

    match (left, right) {
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
        (Value::SimResult(s), "chamber_name") => Ok(Value::Str(s.chamber_name)),
        (Value::SimResult(s), "final_state_hash") => Ok(Value::Str(s.final_state_hash)),
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
        (Value::ChamberView(snap), "chamber_name") => Ok(Value::Str(snap.chamber_name)),
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
            match o.stage {
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
            .to_string(),
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
        (Value::CadenceAssociation(a), "reference_id") => Ok(Value::Str(a.reference_id)),

        (Value::Support(s), "is_yro") => Ok(Value::Bool(s.is_yro())),
        (Value::Support(s), "is_closure_certificate") => {
            Ok(Value::Bool(s.is_closure_certificate()))
        }
        (Value::Addr8(a), "is_first_anchor") => Ok(Value::Bool(a.is_first_anchor())),
        (Value::Window(u, _), "start") => Ok(Value::Int(BigInt::from_i64(u))),
        (Value::Window(_, v), "end") => Ok(Value::Int(BigInt::from_i64(v))),
        (Value::Window(u, v), "len") => Ok(Value::Int(BigInt::from_i64(v - u))),

        _ => Err(Diagnostic::at_line(
            DiagnosticCode::RuntimeError,
            line,
            format!("unsupported field .{field}"),
        )),
    }
}

fn eval_builtin(name: &str, args: Vec<Value>, line: usize) -> DecResult<Value> {
    let err = |e: aod_core::error::CoreError| {
        Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())
    };
    match (name, args.as_slice()) {
        ("t1", [Value::Int(n)]) => T1::from_bigint(n).map(Value::T1).map_err(err),
        ("t32", [Value::Int(n)]) => T32::from_bigint(n).map(Value::T32).map_err(err),
        ("q4", [Value::Int(n)]) => Q4::from_bigint(n).map(Value::Q4).map_err(err),
        ("int", [Value::Int(n)]) => Ok(Value::Int(n.clone())),
        ("int", [Value::T1(v)]) => Ok(Value::Int(BigInt::from_i64(v.value() as i64))),
        ("int", [Value::T32(v)]) => Ok(Value::Int(BigInt::from_i128(v.value()))),
        ("int", [Value::Q4(v)]) => Ok(Value::Int(BigInt::from_u64(v.value() as u64))),
        ("int", [Value::Ratio(r)]) => r.to_int_exact().map(Value::Int).map_err(err),
        ("ratio", [Value::Int(a), Value::Int(b)]) => {
            Ratio::new(a.clone(), b.clone()).map(Value::Ratio).map_err(err)
        }
        ("ratio", [Value::Int(a)]) => Ok(Value::Ratio(Ratio::from_int(a.clone()))),
        ("pack", [Value::T32(v)]) => Ok(Value::Int(BigInt::from_u64(v.pack()))),
        ("unpack", [Value::Int(bits)]) => T32::unpack_bigint(bits).map(Value::T32).map_err(err),
        ("neg", [v]) => eval_unary(UnaryOp::Neg, v.clone(), line),

        ("trace", [Value::Str(sc), Value::Str(lin), Value::Q4(st)]) => {
            Trace::new(sc.clone(), lin.clone(), *st)
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
        ("support", [Value::Str(s)]) => SupportForm::parse(s).map(Value::Support).map_err(err),
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
        ("len", [Value::List(xs)]) => Ok(Value::Int(BigInt::from_u64(xs.len() as u64))),
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

        // Quarantined aod::qext builtins
        ("aod::qext::ext_decimal", [Value::Str(s)]) => {
            parse_decimal_exact(s).map(Value::QExtRatio).map_err(err)
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

        _ => Err(Diagnostic::at_line(
            DiagnosticCode::RuntimeError,
            line,
            format!("unlisted builtin call {name}"),
        )),
    }
}
