use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use aod_core::bigint::BigInt;
use aod_llvm::{CodegenPlan, UnrollHint};

use crate::ast::{
    AssignTarget, BinaryOp, CallArg, ChamberDecl, DecArm, DecArmParam, DecFnDecl, DecFnKind,
    EnumDecl, Expr, ExprKind, ForTarget, Item, ObserveDecl, Pattern, PrepareDecl, ProbeDecl,
    Program, Stmt, StmtKind, StructDecl, TritLiteral, TypeExpr, UnaryOp,
};
use crate::checker::CheckedUnit;
use crate::diagnostics::{CompatMode, DecResult, Diagnostic, DiagnosticCode};
use crate::modules::{builtin_namespace_items, canonical_builtin_name, ImportedSymbol};
use crate::types::Type;

/// Lower a verified `CheckedUnit` using the default baseline `CodegenPlan`.
pub fn lower_to_llvm_ir(checked: &CheckedUnit) -> DecResult<String> {
    lower_to_llvm_ir_with_plan(checked, &CodegenPlan::baseline_o2())
}

/// Lower a verified `CheckedUnit` into safe textual LLVM 18 IR parameterized by `CodegenPlan`
/// (`#![forbid(unsafe_code)]`, `P0-3`).
pub fn lower_to_llvm_ir_with_plan(
    checked: &CheckedUnit,
    plan: &CodegenPlan,
) -> DecResult<String> {
    let mut cg = Codegen::new(checked, *plan);
    cg.index_all()?;
    cg.emit_all()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LlvmTy {
    Void,
    I1,
    I8,
    I64,
    DecInt,
    DecRatio,
    Handle,
}

impl LlvmTy {
    fn from_dec(ty: &Type) -> Self {
        match ty.unwrap_read_only() {
            Type::Unit => LlvmTy::Void,
            Type::Bool => LlvmTy::I1,
            Type::QExt(inner) if **inner == Type::Bool => LlvmTy::I1,
            Type::T1 | Type::Q4 => LlvmTy::I8,
            Type::T32 => LlvmTy::I64,
            Type::Int => LlvmTy::DecInt,
            Type::Ratio => LlvmTy::DecRatio,
            _ => LlvmTy::Handle,
        }
    }

    fn ir(self) -> &'static str {
        match self {
            LlvmTy::Void => "void",
            LlvmTy::I1 => "i1",
            LlvmTy::I8 => "i8",
            LlvmTy::I64 | LlvmTy::Handle => "i64",
            LlvmTy::DecInt => "%DecInt",
            LlvmTy::DecRatio => "%DecRatio",
        }
    }

    fn zero(self) -> &'static str {
        match self {
            LlvmTy::Void => "",
            LlvmTy::I1 => "false",
            LlvmTy::I8 => "0",
            LlvmTy::I64 | LlvmTy::Handle => "0",
            LlvmTy::DecInt => "zeroinitializer",
            LlvmTy::DecRatio => "{ i8 0, i64 0, i64 1 }",
        }
    }
}

#[derive(Clone, Debug)]
struct Val {
    reg: String,
    dec_ty: Type,
}

impl Val {
    fn llvm_ty(&self) -> LlvmTy {
        LlvmTy::from_dec(&self.dec_ty)
    }
}

#[derive(Clone, Debug)]
struct LocalSlot {
    alloca: String,
    dec_ty: Type,
}

#[derive(Clone, Debug)]
struct ConstMeta {
    global_name: String,
    dec_ty: Type,
    mod_key: String,
    value_expr: Expr,
}

#[derive(Clone, Debug)]
struct StructMeta {
    module: String,
    name: String,
    fields: Vec<(String, Type)>,
}

#[derive(Clone, Debug)]
struct EnumMeta {
    module: String,
    name: String,
    variants: Vec<(String, i32, Vec<Type>)>,
}

#[derive(Clone, Debug)]
struct FnOverload {
    llvm_name: String,
    mod_key: String,
    param_types: Vec<Type>,
    ret_type: Type,
    is_single: bool,
    single_params: Vec<String>,
    single_body: Vec<Stmt>,
    multi_arms: Vec<DecArm>,
}

struct Codegen<'a> {
    checked: &'a CheckedUnit,
    plan: CodegenPlan,
    structs: BTreeMap<(String, String), StructMeta>,
    enums: BTreeMap<(String, String), EnumMeta>,
    chambers: BTreeMap<(String, String), ChamberDecl>,
    prepares: BTreeMap<(String, String), PrepareDecl>,
    observes: BTreeMap<(String, String), ObserveDecl>,
    qext_modules: BTreeSet<(String, String)>,
    consts: BTreeMap<(String, String), ConstMeta>,
    const_order: Vec<(String, String)>,
    /// Keyed by `(mod_key, fn_name)` -> list of distinct-signature compiled LLVM functions.
    fn_overloads: BTreeMap<(String, String), Vec<FnOverload>>,
    /// Procedural observer callbacks `(mod_key, obs_name, probe_name) -> llvm_cb_name`.
    obs_callbacks: BTreeMap<(String, String, String), String>,
    /// Deduplicated UTF-8 byte constants: `bytes -> (global_name, byte_len)`.
    str_pool: BTreeMap<Vec<u8>, (String, usize)>,
    /// Unique self-referential `!llvm.loop` metadata nodes emitted at module tail (`P0-3`).
    loop_md_defs: Vec<String>,
    next_md_id: usize,
    fn_defs: String,
    next_id: usize,
}

impl<'a> Codegen<'a> {
    fn new(checked: &'a CheckedUnit, plan: CodegenPlan) -> Self {
        Self {
            checked,
            plan,
            structs: BTreeMap::new(),
            enums: BTreeMap::new(),
            chambers: BTreeMap::new(),
            prepares: BTreeMap::new(),
            observes: BTreeMap::new(),
            qext_modules: BTreeSet::new(),
            consts: BTreeMap::new(),
            const_order: Vec::new(),
            fn_overloads: BTreeMap::new(),
            obs_callbacks: BTreeMap::new(),
            str_pool: BTreeMap::new(),
            loop_md_defs: Vec::new(),
            next_md_id: 0,
            fn_defs: String::new(),
            next_id: 0,
        }
    }

    /// Allocate a unique self-referential `!N = distinct !{!N, !M}` loop metadata attachment (`P0-3`).
    fn alloc_loop_unroll_md(&mut self) -> Option<String> {
        let hint_body = match self.plan.unroll_hint {
            UnrollHint::Default => return None,
            UnrollHint::Unroll4 => "!{!\"llvm.loop.unroll.count\", i32 4}",
            UnrollHint::Disable => "!{!\"llvm.loop.unroll.disable\"}",
        };
        let loop_id = self.next_md_id;
        let hint_id = self.next_md_id + 1;
        self.next_md_id += 2;
        self.loop_md_defs
            .push(format!("!{loop_id} = distinct !{{!{loop_id}, !{hint_id}}}"));
        self.loop_md_defs.push(format!("!{hint_id} = {hint_body}"));
        Some(format!(", !llvm.loop !{loop_id}"))
    }

    fn compat(&self) -> CompatMode {
        self.checked.unit.compat
    }

    fn fresh_id(&mut self) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        id
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

    fn resolve_type(&self, mod_key: &str, expr: &TypeExpr, line: usize) -> DecResult<Type> {
        Type::resolve_with(expr, self.compat(), line, &|n| {
            self.lookup_user_type(mod_key, n)
        })
    }

    fn intern_bytes(&mut self, bytes: &[u8]) -> (String, usize) {
        if let Some(existing) = self.str_pool.get(bytes) {
            return existing.clone();
        }
        let id = self.str_pool.len();
        let name = format!("@.str.{id}");
        let len = bytes.len();
        self.str_pool.insert(bytes.to_vec(), (name.clone(), len));
        (name, len)
    }

    fn index_all(&mut self) -> DecResult<()> {
        // Pass 1: register struct/enum/qext names
        self.collect_names("<entry>", &self.checked.unit.entry_program);
        for (mk, prog) in &self.checked.unit.parsed_modules {
            self.collect_names(mk, prog);
        }
        // Pass 2: resolve struct fields & enum variants
        self.resolve_types_in_program("<entry>", &self.checked.unit.entry_program)?;
        for (mk, prog) in &self.checked.unit.parsed_modules {
            self.resolve_types_in_program(mk, prog)?;
        }
        // Pass 3: index chambers, prepares, observes, functions, consts in strict post-order
        // topological dependency order (`P0-1`) so `@aod_init_consts` always initializes dependencies first.
        for mk in &self.checked.unit.module_load_order {
            if let Some(prog) = self.checked.unit.parsed_modules.get(mk) {
                self.index_items(mk, prog)?;
            }
        }
        self.index_items("<entry>", &self.checked.unit.entry_program)?;
        Ok(())
    }

    fn collect_names(&mut self, mod_key: &str, prog: &Program) {
        for item in &prog.items {
            match item {
                Item::Struct(s) => {
                    self.structs.insert(
                        (mod_key.to_string(), s.name.clone()),
                        StructMeta {
                            module: mod_key.to_string(),
                            name: s.name.clone(),
                            fields: Vec::new(),
                        },
                    );
                }
                Item::Enum(e) => {
                    self.enums.insert(
                        (mod_key.to_string(), e.name.clone()),
                        EnumMeta {
                            module: mod_key.to_string(),
                            name: e.name.clone(),
                            variants: Vec::new(),
                        },
                    );
                }
                Item::QuarantineMod(qm) => {
                    self.qext_modules
                        .insert((mod_key.to_string(), qm.name.clone()));
                }
                _ => {}
            }
        }
    }

    fn resolve_types_in_program(&mut self, mod_key: &str, prog: &Program) -> DecResult<()> {
        for item in &prog.items {
            match item {
                Item::Struct(s) => self.register_struct_decl(mod_key, s)?,
                Item::Enum(e) => self.register_enum_decl(mod_key, e)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn register_struct_decl(&mut self, mod_key: &str, s: &StructDecl) -> DecResult<()> {
        let mut fields = Vec::with_capacity(s.fields.len());
        for (fname, fty) in &s.fields {
            fields.push((fname.clone(), self.resolve_type(mod_key, fty, s.line)?));
        }
        self.structs.insert(
            (mod_key.to_string(), s.name.clone()),
            StructMeta {
                module: mod_key.to_string(),
                name: s.name.clone(),
                fields,
            },
        );
        Ok(())
    }

    fn register_enum_decl(&mut self, mod_key: &str, e: &EnumDecl) -> DecResult<()> {
        let mut variants = Vec::with_capacity(e.variants.len());
        for (idx, v) in e.variants.iter().enumerate() {
            let mut payloads = Vec::with_capacity(v.payloads.len());
            for pt in &v.payloads {
                payloads.push(self.resolve_type(mod_key, pt, v.line)?);
            }
            variants.push((v.name.clone(), idx as i32, payloads));
        }
        self.enums.insert(
            (mod_key.to_string(), e.name.clone()),
            EnumMeta {
                module: mod_key.to_string(),
                name: e.name.clone(),
                variants,
            },
        );
        Ok(())
    }

    fn index_items(&mut self, mod_key: &str, prog: &Program) -> DecResult<()> {
        for item in &prog.items {
            match item {
                Item::Chamber(c) => {
                    self.chambers
                        .insert((mod_key.to_string(), c.name.clone()), c.clone());
                }
                Item::Prepare(p) => {
                    self.prepares
                        .insert((mod_key.to_string(), p.name.clone()), p.clone());
                }
                Item::Observe(o) => {
                    for probe in &o.probes {
                        if let ProbeDecl::Procedural { name, .. } = probe {
                            let id = self.fresh_id();
                            let cb_name = format!("@__obs_cb_{id}");
                            self.obs_callbacks.insert(
                                (mod_key.to_string(), o.name.clone(), name.clone()),
                                cb_name,
                            );
                        }
                    }
                    self.observes
                        .insert((mod_key.to_string(), o.name.clone()), o.clone());
                }
                Item::DecFn(f) => self.index_fn(mod_key, f)?,
                Item::QuarantineMod(qm) => {
                    let sub_key = format!("{mod_key}::{}", qm.name);
                    for sub in &qm.items {
                        if let Item::DecFn(f) = sub {
                            self.index_fn(&sub_key, f)?;
                        }
                    }
                }
                _ => {}
            }
        }
        // Index consts after functions so const expressions can infer return types cleanly
        for item in &prog.items {
            match item {
                Item::Const(c) => self.index_const(mod_key, c)?,
                Item::QuarantineMod(qm) => {
                    let sub_key = format!("{mod_key}::{}", qm.name);
                    for sub in &qm.items {
                        if let Item::Const(c) = sub {
                            self.index_const(&sub_key, c)?;
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn index_const(&mut self, mod_key: &str, c: &crate::ast::ConstDecl) -> DecResult<()> {
        let key = (mod_key.to_string(), c.name.clone());
        let dec_ty = self
            .checked
            .const_types
            .get(&key)
            .cloned()
            .ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    c.line,
                    format!("codegen: missing verified type for const {mod_key}::{}", c.name),
                )
            })?;
        let id = self.fresh_id();
        let global_name = format!("@__const_{id}");
        self.consts.insert(
            key.clone(),
            ConstMeta {
                global_name,
                dec_ty,
                mod_key: mod_key.to_string(),
                value_expr: c.value.clone(),
            },
        );
        self.const_order.push(key);
        Ok(())
    }

    fn index_fn(&mut self, mod_key: &str, f: &DecFnDecl) -> DecResult<()> {
        let is_entry_main = mod_key == "<entry>" && f.name == "main";
        match &f.kind {
            DecFnKind::SingleArm {
                params,
                return_type,
                body,
            } => {
                let mut param_types = Vec::with_capacity(params.len());
                let mut param_names = Vec::with_capacity(params.len());
                for p in params {
                    param_types.push(self.resolve_type(mod_key, &p.ty, f.line)?);
                    param_names.push(p.name.clone());
                }
                let ret_type = self.resolve_type(mod_key, return_type, f.line)?;
                let id = self.fresh_id();
                let llvm_name = if is_entry_main && params.is_empty() {
                    "@dec_entry_main".to_string()
                } else {
                    format!("@dec_fn_{}_{}", sanitize_ident(&f.name), id)
                };
                self.fn_overloads
                    .entry((mod_key.to_string(), f.name.clone()))
                    .or_default()
                    .push(FnOverload {
                        llvm_name,
                        mod_key: mod_key.to_string(),
                        param_types,
                        ret_type,
                        is_single: true,
                        single_params: param_names,
                        single_body: body.clone(),
                        multi_arms: Vec::new(),
                    });
            }
            DecFnKind::MultiArm {
                header_typed_params,
                arms,
                ..
            } => {
                // Group multi-arm rows by concrete resolved parameter signature so distinct
                // typed overloads compile to separate typed LLVM functions while overlapping
                // pattern/guard rows compile into one top-to-bottom dispatching function!
                let mut slot_hints: Vec<Option<Type>> =
                    vec![None; arms.first().map_or(0, |a| a.params.len())];
                if let Some(htp) = header_typed_params {
                    for (i, p) in htp.iter().enumerate() {
                        slot_hints[i] = Some(self.resolve_type(mod_key, &p.ty, f.line)?);
                    }
                }
                for arm in arms {
                    let tys =
                        self.infer_arm_param_types(mod_key, arm, header_typed_params.as_deref())?;
                    for (i, t) in tys.into_iter().enumerate() {
                        if t != Type::Unknown && slot_hints[i].is_none() {
                            slot_hints[i] = Some(t);
                        }
                    }
                }

                let mut groups: Vec<(Vec<Type>, Type, Vec<DecArm>)> = Vec::new();
                for arm in arms {
                    let raw_tys =
                        self.infer_arm_param_types(mod_key, arm, header_typed_params.as_deref())?;
                    let concrete_tys: Vec<Type> = raw_tys
                        .into_iter()
                        .enumerate()
                        .map(|(i, t)| {
                            if t == Type::Unknown {
                                slot_hints[i].clone().unwrap_or(Type::Int)
                            } else {
                                t
                            }
                        })
                        .collect();
                    let rty = self.resolve_type(mod_key, &arm.return_type, arm.line)?;
                    if let Some(existing) = groups.iter_mut().find(|(sig, _, _)| sig == &concrete_tys)
                    {
                        existing.2.push(arm.clone());
                    } else {
                        groups.push((concrete_tys, rty, vec![arm.clone()]));
                    }
                }

                for (param_types, ret_type, group_arms) in groups {
                    let id = self.fresh_id();
                    let llvm_name = if is_entry_main && param_types.is_empty() {
                        "@dec_entry_main".to_string()
                    } else {
                        format!("@dec_fn_{}_{}", sanitize_ident(&f.name), id)
                    };
                    self.fn_overloads
                        .entry((mod_key.to_string(), f.name.clone()))
                        .or_default()
                        .push(FnOverload {
                            llvm_name,
                            mod_key: mod_key.to_string(),
                            param_types,
                            ret_type,
                            is_single: false,
                            single_params: Vec::new(),
                            single_body: Vec::new(),
                            multi_arms: group_arms,
                        });
                }
            }
        }
        Ok(())
    }

    fn infer_arm_param_types(
        &self,
        mod_key: &str,
        arm: &DecArm,
        header_typed: Option<&[crate::ast::TypedParam]>,
    ) -> DecResult<Vec<Type>> {
        let mut out = Vec::with_capacity(arm.params.len());
        for (idx, p) in arm.params.iter().enumerate() {
            let inferred = match p {
                DecArmParam::TypedVar { ty, .. } | DecArmParam::TypedPattern { ty, .. } => {
                    self.resolve_type(mod_key, ty, arm.line)?
                }
                DecArmParam::Pattern(pat) => {
                    if let Some(t) = self.infer_pattern_type(mod_key, pat) {
                        t
                    } else if let Some(ht) = header_typed {
                        self.resolve_type(mod_key, &ht[idx].ty, arm.line)?
                    } else {
                        Type::Unknown
                    }
                }
            };
            out.push(inferred);
        }
        Ok(out)
    }

    fn infer_pattern_type(&self, mod_key: &str, pat: &Pattern) -> Option<Type> {
        match pat {
            Pattern::Wildcard | Pattern::Binding(_) => None,
            Pattern::IntLit(_) => Some(Type::Int),
            Pattern::BoolLit(_) => Some(Type::Bool),
            Pattern::StrLit(_) => Some(Type::Str),
            Pattern::TritLit(_) => Some(Type::T1),
            Pattern::RatioLit(_) => Some(Type::Ratio),
            Pattern::Addr8Lit(_) => Some(Type::Addr8),
            Pattern::SupportLit(_) => Some(Type::Support),
            Pattern::EnumVariant { path, .. } => {
                let (em, _) = self.resolve_enum_variant(mod_key, path)?;
                Some(Type::Enum {
                    module: em.module.clone(),
                    name: em.name.clone(),
                })
            }
            Pattern::Tuple(ps) => {
                let mut ts = Vec::with_capacity(ps.len());
                for p in ps {
                    ts.push(self.infer_pattern_type(mod_key, p)?);
                }
                Some(Type::Tuple(ts))
            }
        }
    }

    fn resolve_enum_variant(&self, mod_key: &str, path: &[String]) -> Option<(&EnumMeta, usize)> {
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
        let em = self.enums.get(&(emod, ename))?;
        let idx = em.variants.iter().position(|(vn, _, _)| vn == &vname)?;
        Some((em, idx))
    }

    fn resolve_struct(&self, mod_key: &str, path: &[String]) -> Option<&StructMeta> {
        if path.len() == 1 {
            let base = self.base_module_of(mod_key);
            if let Some(sm) = self.structs.get(&(base.to_string(), path[0].clone())) {
                return Some(sm);
            }
            if let Some(ImportedSymbol::ModuleItem { module, item }) =
                self.imports_for(mod_key).get(&path[0])
            {
                return self.structs.get(&(module.clone(), item.clone()));
            }
        } else if path.len() == 2 {
            if let Some(ImportedSymbol::ModuleNamespace(m)) =
                self.imports_for(mod_key).get(&path[0])
            {
                return self.structs.get(&(m.clone(), path[1].clone()));
            }
        }
        None
    }

    fn emit_all(&mut self) -> DecResult<String> {
        // 1. Emit procedural observer callbacks
        let obs_list: Vec<(String, ObserveDecl)> = self
            .observes
            .iter()
            .map(|((m, _), o)| (m.clone(), o.clone()))
            .collect();
        for (mk, obs) in obs_list {
            for probe in &obs.probes {
                if let ProbeDecl::Procedural {
                    name,
                    param_name,
                    return_type,
                    body,
                    line,
                    ..
                } = probe
                {
                    let cb_name = self.obs_callbacks
                        [&(mk.clone(), obs.name.clone(), name.clone())]
                        .clone();
                    let ret_ty = self.resolve_type(&mk, return_type, *line)?;
                    self.emit_obs_callback(&cb_name, &mk, param_name, &ret_ty, body)?;
                }
            }
        }

        // 2. Emit every user function & overload (including unreachable ones!)
        let all_overloads: Vec<FnOverload> = self
            .fn_overloads
            .values()
            .flat_map(|v| v.iter().cloned())
            .collect();
        for ov in all_overloads {
            self.emit_user_fn(&ov)?;
        }

        // 3. Emit `@aod_init_consts(ptr %ctx)`
        self.emit_init_consts()?;

        // 4. Emit `@aod_jit_entry(ptr %ctx)` and AOT `@main(i32 %argc, ptr %argv)`
        let main_ret = self
            .checked
            .main_return_type
            .clone()
            .unwrap_or(Type::Unit);
        self.emit_entrypoints(&main_ret);

        // Assemble final LLVM 18 module
        let mut ir = String::with_capacity(self.fn_defs.len() + 24_000);
        ir.push_str(LLVM_MODULE_PREAMBLE);
        ir.push('\n');

        // Emit module const globals
        for key in &self.const_order {
            let cm = &self.consts[key];
            let lt = LlvmTy::from_dec(&cm.dec_ty);
            if lt != LlvmTy::Void {
                let _ = writeln!(
                    ir,
                    "{} = internal global {} {}",
                    cm.global_name,
                    lt.ir(),
                    lt.zero()
                );
            }
        }

        // Emit interned byte-string globals
        for (bytes, (gname, len)) in &self.str_pool {
            let escaped = escape_llvm_bytes(bytes);
            let _ = writeln!(
                ir,
                "{gname} = private unnamed_addr constant [{len} x i8] c\"{escaped}\", align 1"
            );
        }

        ir.push('\n');
        ir.push_str(&self.fn_defs);
        if !self.loop_md_defs.is_empty() {
            ir.push('\n');
            for md in &self.loop_md_defs {
                ir.push_str(md);
                ir.push('\n');
            }
        }
        Ok(ir)
    }

    fn emit_init_consts(&mut self) -> DecResult<()> {
        let order = self.const_order.clone();
        let mut fb = FnBuilder::new("@aod_init_consts", Type::Unit, "<entry>");
        for key in order {
            let cm = self.consts[&key].clone();
            fb.mod_key = cm.mod_key.clone();
            let v = fb.emit_expr(self, &cm.value_expr)?;
            fb.emit_fail_return_check();
            let lt = LlvmTy::from_dec(&cm.dec_ty);
            if lt != LlvmTy::Void {
                fb.line(&format!(
                    "  store {} {}, ptr {}",
                    lt.ir(),
                    v.reg,
                    cm.global_name
                ));
            }
        }
        fb.finish_void();
        self.fn_defs.push_str(&fb.render_fn("define internal void @aod_init_consts(ptr %ctx)"));
        Ok(())
    }

    fn emit_obs_callback(
        &mut self,
        cb_name: &str,
        mod_key: &str,
        param_name: &str,
        ret_ty: &Type,
        body: &[Stmt],
    ) -> DecResult<()> {
        let mut fb = FnBuilder::new(cb_name, ret_ty.clone(), mod_key);
        fb.is_obs_cb = true;
        fb.emit_entry_depth_and_fuel();
        let view_slot = fb.alloc_local(param_name, Type::ReadOnly(Box::new(Type::ChamberView)));
        fb.line(&format!("  store i64 %view_h, ptr {}", view_slot.alloca));
        fb.scopes.last_mut().unwrap().insert(param_name.to_string(), view_slot);
        fb.emit_stmts(self, body)?;
        if !fb.terminated {
            fb.emit_depth_dec();
            fb.line("  ret void");
            fb.terminated = true;
        }
        self.fn_defs.push_str(&fb.render_fn(&format!(
            "define internal void {cb_name}(ptr %ctx, i64 %view_h)"
        )));
        Ok(())
    }

    fn emit_user_fn(&mut self, ov: &FnOverload) -> DecResult<()> {
        let mut sig_params = String::from("ptr %ctx");
        for (i, pt) in ov.param_types.iter().enumerate() {
            let lt = LlvmTy::from_dec(pt);
            let _ = write!(sig_params, ", {} %arg{i}", lt.ir());
        }
        let ret_lt = LlvmTy::from_dec(&ov.ret_type);
        let header = format!(
            "define internal {} {}({sig_params})",
            ret_lt.ir(),
            ov.llvm_name
        );

        let mut fb = FnBuilder::new(&ov.llvm_name, ov.ret_type.clone(), &ov.mod_key);
        fb.emit_entry_depth_and_fuel();

        if ov.is_single {
            for (i, (pname, pty)) in ov
                .single_params
                .iter()
                .zip(&ov.param_types)
                .enumerate()
            {
                let lt = LlvmTy::from_dec(pty);
                let slot = fb.alloc_local(pname, pty.clone());
                if lt != LlvmTy::Void {
                    fb.line(&format!("  store {} %arg{i}, ptr {}", lt.ir(), slot.alloca));
                }
                fb.scopes.last_mut().unwrap().insert(pname.clone(), slot);
            }
            fb.emit_stmts(self, &ov.single_body)?;
            if !fb.terminated {
                fb.emit_depth_dec();
                fb.emit_default_ret();
            }
        } else {
            // Multi-arm dispatch (1 arm for plain v1/v2 overload, or N arms top-to-bottom for patterns/guards!)
            for (arm_idx, arm) in ov.multi_arms.iter().enumerate() {
                let next_arm_lbl = format!("arm_next_{arm_idx}");
                fb.push_scope();
                for (i, (param, pty)) in arm.params.iter().zip(&ov.param_types).enumerate() {
                    let arg_val = Val {
                        reg: format!("%arg{i}"),
                        dec_ty: pty.clone(),
                    };
                    match param {
                        DecArmParam::TypedVar { name, .. } => {
                            let lt = LlvmTy::from_dec(pty);
                            let slot = fb.alloc_local(name, pty.clone());
                            if lt != LlvmTy::Void {
                                fb.line(&format!(
                                    "  store {} %arg{i}, ptr {}",
                                    lt.ir(),
                                    slot.alloca
                                ));
                            }
                            fb.scopes.last_mut().unwrap().insert(name.clone(), slot);
                        }
                        DecArmParam::TypedPattern { pattern, .. }
                        | DecArmParam::Pattern(pattern) => {
                            let cond = fb.emit_pattern_match(self, pattern, &arg_val)?;
                            let ok_lbl = fb.fresh_label("pat_ok");
                            fb.line(&format!(
                                "  br i1 {cond}, label %{ok_lbl}, label %{next_arm_lbl}"
                            ));
                            fb.start_block(&ok_lbl);
                        }
                    }
                }
                if let Some(ref guard) = arm.guard {
                    let gv = fb.emit_expr(self, guard)?;
                    fb.emit_fail_return_check();
                    let gok = fb.fresh_label("guard_ok");
                    fb.line(&format!(
                        "  br i1 {}, label %{gok}, label %{next_arm_lbl}",
                        gv.reg
                    ));
                    fb.start_block(&gok);
                }

                // Allocate optional output binding for v1/v2 multi-arm `-> Type out:`
                let out_slot = if let Some(ref out_name) = arm.output_name {
                    let slot = fb.alloc_local(out_name, ov.ret_type.clone());
                    fb.scopes.last_mut().unwrap().insert(out_name.clone(), slot.clone());
                    Some(slot)
                } else {
                    None
                };

                fb.emit_stmts(self, &arm.body)?;
                if !fb.terminated {
                    if let Some(slot) = out_slot {
                        if ret_lt != LlvmTy::Void {
                            let r = fb.fresh_reg();
                            fb.line(&format!(
                                "  {r} = load {}, ptr {}",
                                ret_lt.ir(),
                                slot.alloca
                            ));
                            fb.emit_depth_dec();
                            fb.line(&format!("  ret {} {r}", ret_lt.ir()));
                            fb.terminated = true;
                        } else {
                            fb.emit_depth_dec();
                            fb.line("  ret void");
                            fb.terminated = true;
                        }
                    } else {
                        fb.emit_depth_dec();
                        fb.emit_default_ret();
                    }
                }
                fb.pop_scope();
                fb.start_block(&next_arm_lbl);
            }
            // Fallthrough if no pattern arm matched at runtime
            let (msg_g, msg_len) = self.intern_bytes(b"no matching arm for Dec call");
            fb.line(&format!(
                "  call void @aod_rt_fail(ptr %ctx, ptr {msg_g}, i64 {msg_len})"
            ));
            fb.emit_depth_dec();
            fb.emit_default_ret();
        }

        self.fn_defs.push_str(&fb.render_fn(&header));
        Ok(())
    }

    fn emit_entrypoints(&mut self, main_ret: &Type) {
        let ret_lt = LlvmTy::from_dec(main_ret);
        let mut s = String::new();
        s.push_str("define void @aod_jit_entry(ptr %ctx) {\nentry:\n");
        s.push_str("  call void @aod_init_consts(ptr %ctx)\n");
        s.push_str("  %f0p = getelementptr inbounds %AodRtContext, ptr %ctx, i32 0, i32 3\n");
        s.push_str("  %f0 = load i8, ptr %f0p\n");
        s.push_str("  %bad0 = icmp ne i8 %f0, 0\n");
        s.push_str("  br i1 %bad0, label %done, label %run_main\nrun_main:\n");
        if ret_lt == LlvmTy::Void {
            s.push_str("  call void @dec_entry_main(ptr %ctx)\n");
            s.push_str("  br label %done\n");
        } else {
            let _ = writeln!(s, "  %res = call {} @dec_entry_main(ptr %ctx)", ret_lt.ir());
            s.push_str("  %f1 = load i8, ptr %f0p\n");
            s.push_str("  %bad1 = icmp ne i8 %f1, 0\n");
            s.push_str("  br i1 %bad1, label %done, label %emit_ret\nemit_ret:\n");
            match main_ret.unwrap_read_only() {
                Type::Int => {
                    s.push_str("  %tmp_i = alloca %DecInt, align 8\n");
                    s.push_str("  store %DecInt %res, ptr %tmp_i\n");
                    s.push_str("  call void @aod_rt_emit_int(ptr %ctx, ptr %tmp_i)\n");
                }
                Type::Bool => {
                    s.push_str("  %b8 = zext i1 %res to i8\n");
                    s.push_str("  call void @aod_rt_emit_bool(ptr %ctx, i8 %b8)\n");
                }
                Type::QExt(inner) if **inner == Type::Bool => {
                    s.push_str("  %b8 = zext i1 %res to i8\n");
                    s.push_str("  call void @aod_rt_emit_qext_bool(ptr %ctx, i8 %b8)\n");
                }
                Type::T1 => s.push_str("  call void @aod_rt_emit_t1(ptr %ctx, i8 %res)\n"),
                Type::T32 => s.push_str("  call void @aod_rt_emit_t32(ptr %ctx, i64 %res)\n"),
                Type::Q4 => s.push_str("  call void @aod_rt_emit_q4(ptr %ctx, i8 %res)\n"),
                Type::Ratio => {
                    s.push_str("  %tmp_r = alloca %DecRatio, align 8\n");
                    s.push_str("  store %DecRatio %res, ptr %tmp_r\n");
                    s.push_str("  call void @aod_rt_emit_ratio(ptr %ctx, ptr %tmp_r)\n");
                }
                _ => s.push_str("  call void @aod_rt_emit_handle(ptr %ctx, i64 %res)\n"),
            }
            s.push_str("  br label %done\n");
        }
        s.push_str("done:\n  ret void\n}\n\n");

        // Standalone AOT ELF `main(argc, argv) -> i32`
        s.push_str("define i32 @main(i32 %argc, ptr %argv) {\nentry:\n");
        s.push_str("  %ctx = alloca %AodRtContext, align 8\n");
        s.push_str("  %parse_rc = call i32 @aod_rt_cli_parse_args(i32 %argc, ptr %argv, ptr %ctx)\n");
        s.push_str("  %parse_err = icmp ne i32 %parse_rc, 0\n");
        s.push_str("  br i1 %parse_err, label %bad_cli, label %ok_cli\nbad_cli:\n  ret i32 1\nok_cli:\n");
        s.push_str("  call void @aod_jit_entry(ptr %ctx)\n");
        s.push_str("  %exit_code = call i32 @aod_rt_ctx_finish(ptr %ctx)\n");
        s.push_str("  ret i32 %exit_code\n}\n");

        self.fn_defs.push_str(&s);
    }
}

struct FnBuilder {
    _name: String,
    ret_ty: Type,
    mod_key: String,
    is_obs_cb: bool,
    allocas: Vec<String>,
    body: String,
    reg_counter: usize,
    lbl_counter: usize,
    terminated: bool,
    scopes: Vec<BTreeMap<String, LocalSlot>>,
    loop_stack: Vec<(String, String)>, // (continue_lbl, break_lbl)
}

impl FnBuilder {
    fn new(name: &str, ret_ty: Type, mod_key: &str) -> Self {
        Self {
            _name: name.to_string(),
            ret_ty,
            mod_key: mod_key.to_string(),
            is_obs_cb: false,
            allocas: Vec::new(),
            body: String::new(),
            reg_counter: 0,
            lbl_counter: 0,
            terminated: false,
            scopes: vec![BTreeMap::new()],
            loop_stack: Vec::new(),
        }
    }

    fn fresh_reg(&mut self) -> String {
        let id = self.reg_counter;
        self.reg_counter += 1;
        format!("%r{id}")
    }

    fn fresh_label(&mut self, prefix: &str) -> String {
        let id = self.lbl_counter;
        self.lbl_counter += 1;
        format!("{prefix}_{id}")
    }

    fn line(&mut self, s: &str) {
        if !self.terminated {
            self.body.push_str(s);
            self.body.push('\n');
        }
    }

    fn start_block(&mut self, label: &str) {
        let _ = writeln!(self.body, "{label}:");
        self.terminated = false;
    }

    fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn lookup_var(&self, name: &str) -> Option<LocalSlot> {
        for sc in self.scopes.iter().rev() {
            if let Some(s) = sc.get(name) {
                return Some(s.clone());
            }
        }
        None
    }

    fn alloc_local(&mut self, hint: &str, dec_ty: Type) -> LocalSlot {
        let id = self.reg_counter;
        self.reg_counter += 1;
        let alloca = format!("%slot_{}_{id}", sanitize_ident(hint));
        let lt = LlvmTy::from_dec(&dec_ty);
        if lt != LlvmTy::Void {
            self.allocas
                .push(format!("  {alloca} = alloca {}, align 8", lt.ir()));
        }
        LocalSlot { alloca, dec_ty }
    }

    fn alloc_temp(&mut self, llvm_ir_ty: &str) -> String {
        let id = self.reg_counter;
        self.reg_counter += 1;
        let name = format!("%tmp_{id}");
        self.allocas
            .push(format!("  {name} = alloca {llvm_ir_ty}, align 8"));
        name
    }

    fn store_temp_int(&mut self, reg: &str) -> String {
        let ptr = self.alloc_temp("%DecInt");
        self.line(&format!("  store %DecInt {reg}, ptr {ptr}"));
        ptr
    }

    fn store_temp_ratio(&mut self, reg: &str) -> String {
        let ptr = self.alloc_temp("%DecRatio");
        self.line(&format!("  store %DecRatio {reg}, ptr {ptr}"));
        ptr
    }

    fn emit_entry_depth_and_fuel(&mut self) {
        // Inline call-depth guard + coarsened fuel check at function entry
        let dp = self.fresh_reg();
        let d = self.fresh_reg();
        let mp = self.fresh_reg();
        let m = self.fresh_reg();
        let over = self.fresh_reg();
        let ok_d = self.fresh_label("depth_ok");
        let fail_d = self.fresh_label("depth_fail");
        self.line(&format!(
            "  {dp} = getelementptr inbounds %AodRtContext, ptr %ctx, i32 0, i32 1"
        ));
        self.line(&format!("  {d} = load i32, ptr {dp}"));
        self.line(&format!(
            "  {mp} = getelementptr inbounds %AodRtContext, ptr %ctx, i32 0, i32 2"
        ));
        self.line(&format!("  {m} = load i32, ptr {mp}"));
        self.line(&format!("  {over} = icmp sge i32 {d}, {m}"));
        self.line(&format!(
            "  br i1 {over}, label %{fail_d}, label %{ok_d}"
        ));
        self.start_block(&fail_d);
        self.line("  call void @aod_rt_budget_exhausted(ptr %ctx, i8 1)");
        self.emit_default_ret();
        self.start_block(&ok_d);
        let d1 = self.fresh_reg();
        self.line(&format!("  {d1} = add i32 {d}, 1"));
        self.line(&format!("  store i32 {d1}, ptr {dp}"));
        self.emit_fuel_tick();
    }

    fn emit_depth_dec(&mut self) {
        if self.terminated {
            return;
        }
        let dp = self.fresh_reg();
        let d = self.fresh_reg();
        let d1 = self.fresh_reg();
        self.line(&format!(
            "  {dp} = getelementptr inbounds %AodRtContext, ptr %ctx, i32 0, i32 1"
        ));
        self.line(&format!("  {d} = load i32, ptr {dp}"));
        self.line(&format!("  {d1} = sub i32 {d}, 1"));
        self.line(&format!("  store i32 {d1}, ptr {dp}"));
    }

    fn emit_fuel_tick(&mut self) {
        if self.terminated {
            return;
        }
        let fp = self.fresh_reg();
        let f = self.fresh_reg();
        let exhaust = self.fresh_reg();
        let ok = self.fresh_label("fuel_ok");
        let fail = self.fresh_label("fuel_fail");
        self.line(&format!(
            "  {fp} = getelementptr inbounds %AodRtContext, ptr %ctx, i32 0, i32 0"
        ));
        self.line(&format!("  {f} = load i64, ptr {fp}"));
        self.line(&format!("  {exhaust} = icmp sle i64 {f}, 0"));
        self.line(&format!("  br i1 {exhaust}, label %{fail}, label %{ok}"));
        self.start_block(&fail);
        self.line("  call void @aod_rt_budget_exhausted(ptr %ctx, i8 0)");
        self.emit_depth_dec();
        self.emit_default_ret();
        self.start_block(&ok);
        let f1 = self.fresh_reg();
        self.line(&format!("  {f1} = sub i64 {f}, 1"));
        self.line(&format!("  store i64 {f1}, ptr {fp}"));
    }

    fn emit_fail_return_check(&mut self) {
        if self.terminated {
            return;
        }
        let fp = self.fresh_reg();
        let fv = self.fresh_reg();
        let bad = self.fresh_reg();
        let ok = self.fresh_label("ok");
        let abort = self.fresh_label("abort");
        self.line(&format!(
            "  {fp} = getelementptr inbounds %AodRtContext, ptr %ctx, i32 0, i32 3"
        ));
        self.line(&format!("  {fv} = load i8, ptr {fp}"));
        self.line(&format!("  {bad} = icmp ne i8 {fv}, 0"));
        self.line(&format!("  br i1 {bad}, label %{abort}, label %{ok}"));
        self.start_block(&abort);
        self.emit_depth_dec();
        self.emit_default_ret();
        self.start_block(&ok);
    }

    fn emit_default_ret(&mut self) {
        if self.terminated {
            return;
        }
        if self.is_obs_cb {
            self.line("  ret void");
        } else {
            let lt = LlvmTy::from_dec(&self.ret_ty);
            if lt == LlvmTy::Void {
                self.line("  ret void");
            } else {
                self.line(&format!("  ret {} {}", lt.ir(), lt.zero()));
            }
        }
        self.terminated = true;
    }

    fn finish_void(&mut self) {
        if !self.terminated {
            self.line("  ret void");
            self.terminated = true;
        }
    }

    fn render_fn(&self, header: &str) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "{header} {{");
        out.push_str("entry:\n");
        for a in &self.allocas {
            out.push_str(a);
            out.push('\n');
        }
        out.push_str(&self.body);
        out.push_str("}\n\n");
        out
    }

    // ==================== Boxing / Unboxing to RtValue Arena Handles ====================

    fn box_val_to_handle(&mut self, v: &Val) -> String {
        match v.dec_ty.unwrap_read_only() {
            Type::Unit => {
                let r = self.fresh_reg();
                self.line(&format!("  {r} = call i64 @aod_rt_val_unit(ptr %ctx)"));
                r
            }
            Type::Int => {
                let p = self.store_temp_int(&v.reg);
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_val_from_int(ptr %ctx, ptr {p})"
                ));
                r
            }
            Type::Bool => {
                let b8 = self.fresh_reg();
                let r = self.fresh_reg();
                self.line(&format!("  {b8} = zext i1 {} to i8", v.reg));
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_val_from_bool(ptr %ctx, i8 {b8})"
                ));
                r
            }
            Type::QExt(inner) if **inner == Type::Bool => {
                let b8 = self.fresh_reg();
                let r = self.fresh_reg();
                self.line(&format!("  {b8} = zext i1 {} to i8", v.reg));
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_val_from_qext_bool(ptr %ctx, i8 {b8})"
                ));
                r
            }
            Type::T1 => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_val_from_t1(ptr %ctx, i8 {})",
                    v.reg
                ));
                r
            }
            Type::T32 => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_val_from_t32(ptr %ctx, i64 {})",
                    v.reg
                ));
                r
            }
            Type::Q4 => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_val_from_q4(ptr %ctx, i8 {})",
                    v.reg
                ));
                r
            }
            Type::Ratio => {
                let p = self.store_temp_ratio(&v.reg);
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_val_from_ratio(ptr %ctx, ptr {p})"
                ));
                r
            }
            _ => v.reg.clone(),
        }
    }

    fn unbox_handle_to_val(&mut self, h_reg: &str, target_ty: &Type) -> Val {
        let reg = match target_ty.unwrap_read_only() {
            Type::Unit => String::new(),
            Type::Int => {
                let out = self.alloc_temp("%DecInt");
                self.line(&format!(
                    "  call void @aod_rt_val_to_int(ptr %ctx, i64 {h_reg}, ptr {out})"
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecInt, ptr {out}"));
                r
            }
            Type::Bool => {
                let b8 = self.fresh_reg();
                self.line(&format!(
                    "  {b8} = call i8 @aod_rt_val_to_bool(ptr %ctx, i64 {h_reg})"
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = icmp ne i8 {b8}, 0"));
                r
            }
            Type::QExt(inner) if **inner == Type::Bool => {
                let b8 = self.fresh_reg();
                self.line(&format!(
                    "  {b8} = call i8 @aod_rt_val_to_qext_bool(ptr %ctx, i64 {h_reg})"
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = icmp ne i8 {b8}, 0"));
                r
            }
            Type::T1 => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i8 @aod_rt_val_to_t1(ptr %ctx, i64 {h_reg})"
                ));
                self.emit_fail_return_check();
                r
            }
            Type::T32 => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_val_to_t32(ptr %ctx, i64 {h_reg})"
                ));
                self.emit_fail_return_check();
                r
            }
            Type::Q4 => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i8 @aod_rt_val_to_q4(ptr %ctx, i64 {h_reg})"
                ));
                self.emit_fail_return_check();
                r
            }
            Type::Ratio => {
                let out = self.alloc_temp("%DecRatio");
                self.line(&format!(
                    "  call void @aod_rt_val_to_ratio(ptr %ctx, i64 {h_reg}, ptr {out})"
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecRatio, ptr {out}"));
                r
            }
            _ => h_reg.to_string(),
        };
        Val {
            reg,
            dec_ty: target_ty.clone(),
        }
    }

    fn emit_str_handle(&mut self, cg: &mut Codegen<'_>, text: &str) -> String {
        let (g, len) = cg.intern_bytes(text.as_bytes());
        let r = self.fresh_reg();
        self.line(&format!(
            "  {r} = call i64 @aod_rt_str_new(ptr %ctx, ptr {g}, i64 {len})"
        ));
        self.emit_fail_return_check();
        r
    }

    // ==================== Statements ====================

    fn emit_stmts(&mut self, cg: &mut Codegen<'_>, stmts: &[Stmt]) -> DecResult<()> {
        for s in stmts {
            if self.terminated {
                break;
            }
            self.emit_stmt(cg, s)?;
        }
        Ok(())
    }

    fn emit_stmt(&mut self, cg: &mut Codegen<'_>, stmt: &Stmt) -> DecResult<()> {
        match &stmt.kind {
            StmtKind::Let { name, ty, value }
            | StmtKind::Mut { name, ty, value } => {
                let mut val = self.emit_expr(cg, value)?;
                if let Some(te) = ty {
                    val.dec_ty = cg.resolve_type(&self.mod_key, te, stmt.line)?;
                }
                let lt = val.llvm_ty();
                let slot = self.alloc_local(name, val.dec_ty.clone());
                if lt != LlvmTy::Void {
                    self.line(&format!("  store {} {}, ptr {}", lt.ir(), val.reg, slot.alloca));
                }
                self.scopes.last_mut().unwrap().insert(name.clone(), slot);
                Ok(())
            }
            StmtKind::TypedLocal { ty, name, value } => {
                let mut val = self.emit_expr(cg, value)?;
                val.dec_ty = cg.resolve_type(&self.mod_key, ty, stmt.line)?;
                let lt = val.llvm_ty();
                let slot = self.alloc_local(name, val.dec_ty.clone());
                if lt != LlvmTy::Void {
                    self.line(&format!("  store {} {}, ptr {}", lt.ir(), val.reg, slot.alloca));
                }
                self.scopes.last_mut().unwrap().insert(name.clone(), slot);
                Ok(())
            }
            StmtKind::Assign { target, value } => {
                let val = self.emit_expr(cg, value)?;
                match target {
                    AssignTarget::Ident(name) => {
                        let slot = self.lookup_var(name).ok_or_else(|| {
                            Diagnostic::at_line(
                                DiagnosticCode::RuntimeError,
                                stmt.line,
                                format!("codegen: undefined local {name}"),
                            )
                        })?;
                        let lt = LlvmTy::from_dec(&slot.dec_ty);
                        if lt != LlvmTy::Void {
                            self.line(&format!(
                                "  store {} {}, ptr {}",
                                lt.ir(),
                                val.reg,
                                slot.alloca
                            ));
                        }
                    }
                    AssignTarget::Field { base, name } => {
                        // Deep-copy / copy-on-write struct field update (`P0-3`)!
                        if let ExprKind::Ident(ref vname) = base.kind {
                            let slot = self.lookup_var(vname).ok_or_else(|| {
                                Diagnostic::at_line(
                                    DiagnosticCode::RuntimeError,
                                    stmt.line,
                                    format!("codegen: undefined struct {vname}"),
                                )
                            })?;
                            let old_h = self.fresh_reg();
                            self.line(&format!("  {old_h} = load i64, ptr {}", slot.alloca));
                            let vh = self.box_val_to_handle(&val);
                            let (fg, flen) = cg.intern_bytes(name.as_bytes());
                            let new_h = self.fresh_reg();
                            self.line(&format!(
                                "  {new_h} = call i64 @aod_rt_struct_set(ptr %ctx, i64 {old_h}, ptr {fg}, i64 {flen}, i64 {vh})"
                            ));
                            self.emit_fail_return_check();
                            self.line(&format!("  store i64 {new_h}, ptr {}", slot.alloca));
                        }
                    }
                    AssignTarget::Index { base, index } => {
                        // Deep-copy / copy-on-write list/map index update (`P0-3`)!
                        let idx_val = self.emit_expr(cg, index)?;
                        if let ExprKind::Ident(ref vname) = base.kind {
                            let slot = self.lookup_var(vname).ok_or_else(|| {
                                Diagnostic::at_line(
                                    DiagnosticCode::RuntimeError,
                                    stmt.line,
                                    format!("codegen: undefined container {vname}"),
                                )
                            })?;
                            let old_h = self.fresh_reg();
                            self.line(&format!("  {old_h} = load i64, ptr {}", slot.alloca));
                            let vh = self.box_val_to_handle(&val);
                            let new_h = match slot.dec_ty.unwrap_read_only() {
                                Type::List(_) => {
                                    let ip = self.store_temp_int(&idx_val.reg);
                                    let r = self.fresh_reg();
                                    self.line(&format!(
                                        "  {r} = call i64 @aod_rt_list_set(ptr %ctx, i64 {old_h}, ptr {ip}, i64 {vh})"
                                    ));
                                    r
                                }
                                Type::Map(_, _) => {
                                    let kh = self.box_val_to_handle(&idx_val);
                                    let r = self.fresh_reg();
                                    self.line(&format!(
                                        "  {r} = call i64 @aod_rt_map_set(ptr %ctx, i64 {old_h}, i64 {kh}, i64 {vh})"
                                    ));
                                    r
                                }
                                _ => {
                                    return Err(Diagnostic::at_line(
                                        DiagnosticCode::RuntimeError,
                                        stmt.line,
                                        "codegen: unsupported index assign container",
                                    ));
                                }
                            };
                            self.emit_fail_return_check();
                            self.line(&format!("  store i64 {new_h}, ptr {}", slot.alloca));
                        }
                    }
                }
                Ok(())
            }
            StmtKind::Assert(expr) => {
                let cv = self.emit_expr(cg, expr)?;
                let ok_lbl = self.fresh_label("assert_ok");
                let fail_lbl = self.fresh_label("assert_fail");
                self.line(&format!(
                    "  br i1 {}, label %{ok_lbl}, label %{fail_lbl}",
                    cv.reg
                ));
                self.start_block(&fail_lbl);
                self.line(&format!(
                    "  call void @aod_rt_assert_failed(ptr %ctx, i64 {})",
                    stmt.line
                ));
                self.emit_depth_dec();
                self.emit_default_ret();
                self.start_block(&ok_lbl);
                Ok(())
            }
            StmtKind::Emit(expr) => {
                let v = self.emit_expr(cg, expr)?;
                match v.dec_ty.unwrap_read_only() {
                    Type::Int => {
                        let p = self.store_temp_int(&v.reg);
                        self.line(&format!("  call void @aod_rt_emit_int(ptr %ctx, ptr {p})"));
                    }
                    Type::Bool => {
                        let b8 = self.fresh_reg();
                        self.line(&format!("  {b8} = zext i1 {} to i8", v.reg));
                        self.line(&format!("  call void @aod_rt_emit_bool(ptr %ctx, i8 {b8})"));
                    }
                    Type::QExt(inner) if **inner == Type::Bool => {
                        let b8 = self.fresh_reg();
                        self.line(&format!("  {b8} = zext i1 {} to i8", v.reg));
                        self.line(&format!(
                            "  call void @aod_rt_emit_qext_bool(ptr %ctx, i8 {b8})"
                        ));
                    }
                    Type::T1 => {
                        self.line(&format!("  call void @aod_rt_emit_t1(ptr %ctx, i8 {})", v.reg));
                    }
                    Type::T32 => {
                        self.line(&format!(
                            "  call void @aod_rt_emit_t32(ptr %ctx, i64 {})",
                            v.reg
                        ));
                    }
                    Type::Q4 => {
                        self.line(&format!("  call void @aod_rt_emit_q4(ptr %ctx, i8 {})", v.reg));
                    }
                    Type::Ratio => {
                        let p = self.store_temp_ratio(&v.reg);
                        self.line(&format!("  call void @aod_rt_emit_ratio(ptr %ctx, ptr {p})"));
                    }
                    _ => {
                        self.line(&format!(
                            "  call void @aod_rt_emit_handle(ptr %ctx, i64 {})",
                            v.reg
                        ));
                    }
                }
                self.emit_fail_return_check();
                Ok(())
            }
            StmtKind::Return(opt) => {
                if let Some(e) = opt {
                    let v = self.emit_expr(cg, e)?;
                    self.emit_depth_dec();
                    if self.is_obs_cb || v.llvm_ty() == LlvmTy::Void {
                        self.line("  ret void");
                    } else {
                        self.line(&format!("  ret {} {}", v.llvm_ty().ir(), v.reg));
                    }
                    self.terminated = true;
                } else {
                    self.emit_depth_dec();
                    self.line("  ret void");
                    self.terminated = true;
                }
                Ok(())
            }
            StmtKind::If {
                cond,
                then_body,
                elifs,
                else_body,
            } => {
                // Sound P0-3 branchless `select` lowering for pure single-statement `if c: v = a else: v = b`
                if cg.plan.branchless_select
                    && elifs.is_empty()
                    && then_body.len() == 1
                    && else_body.len() == 1
                {
                    if let (
                        StmtKind::Assign {
                            target: AssignTarget::Ident(tv),
                            value: te,
                        },
                        StmtKind::Assign {
                            target: AssignTarget::Ident(ev),
                            value: ee,
                        },
                    ) = (&then_body[0].kind, &else_body[0].kind)
                    {
                        if tv == ev
                            && self.is_pure_select_safe(cg, te)
                            && self.is_pure_select_safe(cg, ee)
                        {
                            if let Some(slot) = self.lookup_var(tv) {
                                let lt = LlvmTy::from_dec(&slot.dec_ty);
                                if lt != LlvmTy::Void {
                                    let cv = self.emit_expr(cg, cond)?;
                                    let tv_val = self.emit_expr(cg, te)?;
                                    let ev_val = self.emit_expr(cg, ee)?;
                                    let sel = self.fresh_reg();
                                    self.line(&format!(
                                        "  {sel} = select i1 {}, {} {}, {} {}",
                                        cv.reg,
                                        lt.ir(),
                                        tv_val.reg,
                                        lt.ir(),
                                        ev_val.reg
                                    ));
                                    self.line(&format!(
                                        "  store {} {sel}, ptr {}",
                                        lt.ir(),
                                        slot.alloca
                                    ));
                                    return Ok(());
                                }
                            }
                        }
                    }
                }

                let merge_lbl = self.fresh_label("if_merge");
                let mut cur_cond = cond;
                let mut cur_then = then_body;
                let mut remaining_elifs = elifs.iter();

                loop {
                    let cv = self.emit_expr(cg, cur_cond)?;
                    let then_lbl = self.fresh_label("if_then");
                    let next_lbl = self.fresh_label("if_else");
                    self.line(&format!(
                        "  br i1 {}, label %{then_lbl}, label %{next_lbl}",
                        cv.reg
                    ));

                    self.start_block(&then_lbl);
                    self.push_scope();
                    self.emit_stmts(cg, cur_then)?;
                    self.pop_scope();
                    if !self.terminated {
                        self.line(&format!("  br label %{merge_lbl}"));
                        self.terminated = true;
                    }

                    self.start_block(&next_lbl);
                    if let Some((ec, eb)) = remaining_elifs.next() {
                        cur_cond = ec;
                        cur_then = eb;
                    } else {
                        break;
                    }
                }

                if !else_body.is_empty() {
                    self.push_scope();
                    self.emit_stmts(cg, else_body)?;
                    self.pop_scope();
                }
                if !self.terminated {
                    self.line(&format!("  br label %{merge_lbl}"));
                    self.terminated = true;
                }
                self.start_block(&merge_lbl);
                Ok(())
            }
            StmtKind::Match { scrutinee, arms } => {
                let sval = self.emit_expr(cg, scrutinee)?;
                let merge_lbl = self.fresh_label("match_merge");

                // Use LLVM `switch i32` when matching directly on an Enum scrutinee!
                if matches!(sval.dec_ty.unwrap_read_only(), Type::Enum { .. })
                    && arms.iter().all(|a| {
                        a.guard.is_none()
                            && matches!(
                                a.pattern,
                                Pattern::EnumVariant { .. } | Pattern::Wildcard | Pattern::Binding(_)
                            )
                    })
                {
                    let tag = self.fresh_reg();
                    self.line(&format!(
                        "  {tag} = call i32 @aod_rt_enum_tag(ptr %ctx, i64 {})",
                        sval.reg
                    ));
                    self.emit_fail_return_check();

                    let mut cases: Vec<(i32, String, usize)> = Vec::new();
                    let mut default_arm: Option<(String, usize)> = None;
                    for (idx, arm) in arms.iter().enumerate() {
                        match &arm.pattern {
                            Pattern::EnumVariant { path, .. } => {
                                if let Some((em, vidx)) =
                                    cg.resolve_enum_variant(&self.mod_key, path)
                                {
                                    let vtag = em.variants[vidx].1;
                                    if !cases.iter().any(|(t, _, _)| *t == vtag) {
                                        let lbl = self.fresh_label("match_case");
                                        cases.push((vtag, lbl, idx));
                                    }
                                }
                            }
                            Pattern::Wildcard | Pattern::Binding(_) => {
                                if default_arm.is_none() {
                                    default_arm = Some((self.fresh_label("match_def"), idx));
                                }
                            }
                            _ => {}
                        }
                    }
                    let def_lbl = default_arm
                        .as_ref()
                        .map(|(l, _)| l.clone())
                        .unwrap_or_else(|| merge_lbl.clone());

                    let mut sw = format!("  switch i32 {tag}, label %{def_lbl} [");
                    for (vtag, lbl, _) in &cases {
                        let _ = write!(sw, " i32 {vtag}, label %{lbl}");
                    }
                    sw.push_str(" ]");
                    self.line(&sw);
                    self.terminated = true;

                    for (_, lbl, idx) in cases {
                        let arm = &arms[idx];
                        self.start_block(&lbl);
                        self.push_scope();
                        let cond = self.emit_pattern_match(cg, &arm.pattern, &sval)?;
                        let body_lbl = self.fresh_label("match_body");
                        self.line(&format!(
                            "  br i1 {cond}, label %{body_lbl}, label %{merge_lbl}"
                        ));
                        self.start_block(&body_lbl);
                        self.emit_stmts(cg, &arm.body)?;
                        self.pop_scope();
                        if !self.terminated {
                            self.line(&format!("  br label %{merge_lbl}"));
                            self.terminated = true;
                        }
                    }
                    if let Some((dlbl, idx)) = default_arm {
                        let arm = &arms[idx];
                        self.start_block(&dlbl);
                        self.push_scope();
                        let _ = self.emit_pattern_match(cg, &arm.pattern, &sval)?;
                        self.emit_stmts(cg, &arm.body)?;
                        self.pop_scope();
                        if !self.terminated {
                            self.line(&format!("  br label %{merge_lbl}"));
                            self.terminated = true;
                        }
                    }
                    self.start_block(&merge_lbl);
                    return Ok(());
                }

                // General sequential match (guards, nested patterns, literal matches)
                for arm in arms {
                    let next_lbl = self.fresh_label("match_next");
                    self.push_scope();
                    let cond = self.emit_pattern_match(cg, &arm.pattern, &sval)?;
                    let ok_lbl = self.fresh_label("match_ok");
                    self.line(&format!(
                        "  br i1 {cond}, label %{ok_lbl}, label %{next_lbl}"
                    ));
                    self.start_block(&ok_lbl);
                    if let Some(ref g) = arm.guard {
                        let gv = self.emit_expr(cg, g)?;
                        let gok = self.fresh_label("match_gok");
                        self.line(&format!(
                            "  br i1 {}, label %{gok}, label %{next_lbl}",
                            gv.reg
                        ));
                        self.start_block(&gok);
                    }
                    self.emit_stmts(cg, &arm.body)?;
                    self.pop_scope();
                    if !self.terminated {
                        self.line(&format!("  br label %{merge_lbl}"));
                        self.terminated = true;
                    }
                    self.start_block(&next_lbl);
                }
                if !self.terminated {
                    self.line(&format!("  br label %{merge_lbl}"));
                    self.terminated = true;
                }
                self.start_block(&merge_lbl);
                Ok(())
            }
            StmtKind::For { target, iter, body } => {
                // UNBOXED FAST PATH (`AC-05`, `AC-15`): `for i in start..end:` uses pure `i64` counter!
                if let (ForTarget::Single(var_name), ExprKind::Range { start, end }) =
                    (target, &iter.kind)
                {
                    let sv = self.emit_expr(cg, start)?;
                    let ev = self.emit_expr(cg, end)?;
                    let st = self.fresh_reg();
                    let et = self.fresh_reg();
                    let both_fast = self.fresh_reg();
                    let s_i64 = self.fresh_reg();
                    let e_i64 = self.fresh_reg();
                    self.line(&format!("  {st} = extractvalue %DecInt {}, 0", sv.reg));
                    self.line(&format!("  {et} = extractvalue %DecInt {}, 0", ev.reg));
                    let or_t = self.fresh_reg();
                    self.line(&format!("  {or_t} = or i8 {st}, {et}"));
                    self.line(&format!("  {both_fast} = icmp eq i8 {or_t}, 0"));
                    let ok_r = self.fresh_label("range_fast");
                    let bad_r = self.fresh_label("range_oob");
                    self.line(&format!(
                        "  br i1 {both_fast}, label %{ok_r}, label %{bad_r}"
                    ));
                    self.start_block(&bad_r);
                    let (mg, ml) = cg.intern_bytes(b"range endpoints outside i64 domain");
                    self.line(&format!(
                        "  call void @aod_rt_fail(ptr %ctx, ptr {mg}, i64 {ml})"
                    ));
                    self.emit_depth_dec();
                    self.emit_default_ret();

                    self.start_block(&ok_r);
                    self.line(&format!("  {s_i64} = extractvalue %DecInt {}, 1", sv.reg));
                    self.line(&format!("  {e_i64} = extractvalue %DecInt {}, 1", ev.reg));

                    let idx_slot = self.alloc_temp("i64");
                    self.line(&format!("  store i64 {s_i64}, ptr {idx_slot}"));
                    let var_slot = self.alloc_local(var_name, Type::Int);

                    let cond_lbl = self.fresh_label("for_cond");
                    let body_lbl = self.fresh_label("for_body");
                    let step_lbl = self.fresh_label("for_step");
                    let end_lbl = self.fresh_label("for_end");
                    self.line(&format!("  br label %{cond_lbl}"));

                    self.start_block(&cond_lbl);
                    let cur = self.fresh_reg();
                    let cmp = self.fresh_reg();
                    self.line(&format!("  {cur} = load i64, ptr {idx_slot}"));
                    self.line(&format!("  {cmp} = icmp slt i64 {cur}, {e_i64}"));
                    self.line(&format!(
                        "  br i1 {cmp}, label %{body_lbl}, label %{end_lbl}"
                    ));

                    self.start_block(&body_lbl);
                    self.emit_fuel_tick();
                    let packed = self.fresh_reg();
                    self.line(&format!(
                        "  {packed} = insertvalue %DecInt {{ i8 0, i64 0 }}, i64 {cur}, 1"
                    ));
                    self.line(&format!(
                        "  store %DecInt {packed}, ptr {}",
                        var_slot.alloca
                    ));
                    self.push_scope();
                    self.scopes
                        .last_mut()
                        .unwrap()
                        .insert(var_name.clone(), var_slot);
                    self.loop_stack.push((step_lbl.clone(), end_lbl.clone()));
                    self.emit_stmts(cg, body)?;
                    self.loop_stack.pop();
                    self.pop_scope();
                    if !self.terminated {
                        self.line(&format!("  br label %{step_lbl}"));
                        self.terminated = true;
                    }

                    self.start_block(&step_lbl);
                    let cur2 = self.fresh_reg();
                    let next = self.fresh_reg();
                    self.line(&format!("  {cur2} = load i64, ptr {idx_slot}"));
                    self.line(&format!("  {next} = add nsw i64 {cur2}, 1"));
                    self.line(&format!("  store i64 {next}, ptr {idx_slot}"));
                    let md_suffix = cg.alloc_loop_unroll_md().unwrap_or_default();
                    self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
                    self.terminated = true;

                    self.start_block(&end_lbl);
                    return Ok(());
                }

                // General `for` over Range variable, List, or deterministic BTreeMap Map!
                let it = self.emit_expr(cg, iter)?;
                match it.dec_ty.unwrap_read_only().clone() {
                    Type::Range => {
                        let s_ptr = self.alloc_temp("i64");
                        let e_ptr = self.alloc_temp("i64");
                        self.line(&format!(
                            "  call void @aod_rt_range_bounds(ptr %ctx, i64 {}, ptr {s_ptr}, ptr {e_ptr})",
                            it.reg
                        ));
                        self.emit_fail_return_check();
                        let s_i64 = self.fresh_reg();
                        let e_i64 = self.fresh_reg();
                        self.line(&format!("  {s_i64} = load i64, ptr {s_ptr}"));
                        self.line(&format!("  {e_i64} = load i64, ptr {e_ptr}"));
                        let var_name = match target {
                            ForTarget::Single(v) => v.clone(),
                            _ => "_".to_string(),
                        };
                        let idx_slot = self.alloc_temp("i64");
                        self.line(&format!("  store i64 {s_i64}, ptr {idx_slot}"));
                        let var_slot = self.alloc_local(&var_name, Type::Int);
                        let cond_lbl = self.fresh_label("for_cond");
                        let body_lbl = self.fresh_label("for_body");
                        let step_lbl = self.fresh_label("for_step");
                        let end_lbl = self.fresh_label("for_end");
                        self.line(&format!("  br label %{cond_lbl}"));

                        self.start_block(&cond_lbl);
                        let cur = self.fresh_reg();
                        let cmp = self.fresh_reg();
                        self.line(&format!("  {cur} = load i64, ptr {idx_slot}"));
                        self.line(&format!("  {cmp} = icmp slt i64 {cur}, {e_i64}"));
                        self.line(&format!(
                            "  br i1 {cmp}, label %{body_lbl}, label %{end_lbl}"
                        ));
                        self.start_block(&body_lbl);
                        self.emit_fuel_tick();
                        let packed = self.fresh_reg();
                        self.line(&format!(
                            "  {packed} = insertvalue %DecInt {{ i8 0, i64 0 }}, i64 {cur}, 1"
                        ));
                        self.line(&format!("  store %DecInt {packed}, ptr {}", var_slot.alloca));
                        self.push_scope();
                        self.scopes.last_mut().unwrap().insert(var_name, var_slot);
                        self.loop_stack.push((step_lbl.clone(), end_lbl.clone()));
                        self.emit_stmts(cg, body)?;
                        self.loop_stack.pop();
                        self.pop_scope();
                        if !self.terminated {
                            self.line(&format!("  br label %{step_lbl}"));
                            self.terminated = true;
                        }
                        self.start_block(&step_lbl);
                        let cur2 = self.fresh_reg();
                        let next = self.fresh_reg();
                        self.line(&format!("  {cur2} = load i64, ptr {idx_slot}"));
                        self.line(&format!("  {next} = add i64 {cur2}, 1"));
                        self.line(&format!("  store i64 {next}, ptr {idx_slot}"));
                        self.line(&format!("  br label %{cond_lbl}"));
                        self.terminated = true;
                        self.start_block(&end_lbl);
                    }
                    Type::List(elem_ty) => {
                        self.emit_for_over_list_handle(cg, &it.reg, &elem_ty, target, body)?;
                    }
                    Type::Map(k_ty, v_ty) => {
                        let (list_h, elem_ty) = match target {
                            ForTarget::Single(_) => {
                                let lh = self.fresh_reg();
                                self.line(&format!(
                                    "  {lh} = call i64 @aod_rt_map_keys_list(ptr %ctx, i64 {})",
                                    it.reg
                                ));
                                self.emit_fail_return_check();
                                (lh, (*k_ty).clone())
                            }
                            ForTarget::Tuple(_) => {
                                let lh = self.fresh_reg();
                                self.line(&format!(
                                    "  {lh} = call i64 @aod_rt_map_entries_list(ptr %ctx, i64 {})",
                                    it.reg
                                ));
                                self.emit_fail_return_check();
                                (lh, Type::Tuple(vec![(*k_ty).clone(), (*v_ty).clone()]))
                            }
                        };
                        self.emit_for_over_list_handle(cg, &list_h, &elem_ty, target, body)?;
                    }
                    _ => {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::RuntimeError,
                            stmt.line,
                            "codegen: unsupported for iterable",
                        ));
                    }
                }
                Ok(())
            }
            StmtKind::While { cond, body } => {
                let cond_lbl = self.fresh_label("while_cond");
                let body_lbl = self.fresh_label("while_body");
                let end_lbl = self.fresh_label("while_end");
                self.line(&format!("  br label %{cond_lbl}"));

                self.start_block(&cond_lbl);
                self.emit_fuel_tick();
                let cv = self.emit_expr(cg, cond)?;
                self.line(&format!(
                    "  br i1 {}, label %{body_lbl}, label %{end_lbl}",
                    cv.reg
                ));

                self.start_block(&body_lbl);
                self.push_scope();
                self.loop_stack.push((cond_lbl.clone(), end_lbl.clone()));
                self.emit_stmts(cg, body)?;
                self.loop_stack.pop();
                self.pop_scope();
                if !self.terminated {
                    let md_suffix = cg.alloc_loop_unroll_md().unwrap_or_default();
                    self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
                    self.terminated = true;
                }
                self.start_block(&end_lbl);
                Ok(())
            }
            StmtKind::Break => {
                let (_, brk) = self.loop_stack.last().cloned().ok_or_else(|| {
                    Diagnostic::at_line(DiagnosticCode::RuntimeError, stmt.line, "break outside loop")
                })?;
                self.line(&format!("  br label %{brk}"));
                self.terminated = true;
                Ok(())
            }
            StmtKind::Continue => {
                let (cont, _) = self.loop_stack.last().cloned().ok_or_else(|| {
                    Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        stmt.line,
                        "continue outside loop",
                    )
                })?;
                self.line(&format!("  br label %{cont}"));
                self.terminated = true;
                Ok(())
            }
            StmtKind::Pass => Ok(()),
            StmtKind::QuarantineBlock { body } => {
                self.push_scope();
                self.emit_stmts(cg, body)?;
                self.pop_scope();
                Ok(())
            }
            StmtKind::ExprStmt(expr) => {
                let _ = self.emit_expr(cg, expr)?;
                Ok(())
            }
        }
    }

    fn emit_for_over_list_handle(
        &mut self,
        cg: &mut Codegen<'_>,
        list_h: &str,
        elem_ty: &Type,
        target: &ForTarget,
        body: &[Stmt],
    ) -> DecResult<()> {
        let len_r = self.fresh_reg();
        self.line(&format!(
            "  {len_r} = call i64 @aod_rt_list_len_raw(ptr %ctx, i64 {list_h})"
        ));
        self.emit_fail_return_check();
        let idx_slot = self.alloc_temp("i64");
        self.line(&format!("  store i64 0, ptr {idx_slot}"));

        let cond_lbl = self.fresh_label("fl_cond");
        let body_lbl = self.fresh_label("fl_body");
        let step_lbl = self.fresh_label("fl_step");
        let end_lbl = self.fresh_label("fl_end");
        self.line(&format!("  br label %{cond_lbl}"));

        self.start_block(&cond_lbl);
        let cur = self.fresh_reg();
        let cmp = self.fresh_reg();
        self.line(&format!("  {cur} = load i64, ptr {idx_slot}"));
        self.line(&format!("  {cmp} = icmp slt i64 {cur}, {len_r}"));
        self.line(&format!(
            "  br i1 {cmp}, label %{body_lbl}, label %{end_lbl}"
        ));

        self.start_block(&body_lbl);
        self.emit_fuel_tick();
        let idx_dec = self.fresh_reg();
        self.line(&format!(
            "  {idx_dec} = insertvalue %DecInt {{ i8 0, i64 0 }}, i64 {cur}, 1"
        ));
        let ip = self.store_temp_int(&idx_dec);
        let elem_h = self.fresh_reg();
        self.line(&format!(
            "  {elem_h} = call i64 @aod_rt_list_get(ptr %ctx, i64 {list_h}, ptr {ip})"
        ));
        self.emit_fail_return_check();

        self.push_scope();
        match target {
            ForTarget::Single(vname) => {
                let ev = self.unbox_handle_to_val(&elem_h, elem_ty);
                let slot = self.alloc_local(vname, elem_ty.clone());
                let lt = LlvmTy::from_dec(elem_ty);
                if lt != LlvmTy::Void {
                    self.line(&format!("  store {} {}, ptr {}", lt.ir(), ev.reg, slot.alloca));
                }
                self.scopes.last_mut().unwrap().insert(vname.clone(), slot);
            }
            ForTarget::Tuple(vars) => {
                if let Type::Tuple(etys) = elem_ty {
                    for (i, (vn, et)) in vars.iter().zip(etys).enumerate() {
                        let fh = self.fresh_reg();
                        self.line(&format!(
                            "  {fh} = call i64 @aod_rt_tuple_get(ptr %ctx, i64 {elem_h}, i64 {i})"
                        ));
                        self.emit_fail_return_check();
                        let fv = self.unbox_handle_to_val(&fh, et);
                        let slot = self.alloc_local(vn, et.clone());
                        let lt = LlvmTy::from_dec(et);
                        if lt != LlvmTy::Void {
                            self.line(&format!(
                                "  store {} {}, ptr {}",
                                lt.ir(),
                                fv.reg,
                                slot.alloca
                            ));
                        }
                        self.scopes.last_mut().unwrap().insert(vn.clone(), slot);
                    }
                }
            }
        }
        self.loop_stack.push((step_lbl.clone(), end_lbl.clone()));
        self.emit_stmts(cg, body)?;
        self.loop_stack.pop();
        self.pop_scope();
        if !self.terminated {
            self.line(&format!("  br label %{step_lbl}"));
            self.terminated = true;
        }

        self.start_block(&step_lbl);
        let c2 = self.fresh_reg();
        let nxt = self.fresh_reg();
        self.line(&format!("  {c2} = load i64, ptr {idx_slot}"));
        self.line(&format!("  {nxt} = add nsw i64 {c2}, 1"));
        self.line(&format!("  store i64 {nxt}, ptr {idx_slot}"));
        self.line(&format!("  br label %{cond_lbl}"));
        self.terminated = true;
        self.start_block(&end_lbl);
        Ok(())
    }

    // ==================== Pattern Matching ====================

    fn emit_pattern_match(
        &mut self,
        cg: &mut Codegen<'_>,
        pat: &Pattern,
        val: &Val,
    ) -> DecResult<String> {
        match pat {
            Pattern::Wildcard => Ok("true".to_string()),
            Pattern::Binding(name) => {
                let slot = self.alloc_local(name, val.dec_ty.clone());
                let lt = val.llvm_ty();
                if lt != LlvmTy::Void {
                    self.line(&format!("  store {} {}, ptr {}", lt.ir(), val.reg, slot.alloca));
                }
                self.scopes.last_mut().unwrap().insert(name.clone(), slot);
                Ok("true".to_string())
            }
            Pattern::IntLit(n) => {
                let lit = self.emit_bigint_lit(cg, n);
                let cmp = self.fresh_reg();
                self.line(&format!(
                    "  {cmp} = call i32 @__aod_int_cmp(ptr %ctx, %DecInt {}, %DecInt {})",
                    val.reg, lit.reg
                ));
                self.emit_fail_return_check();
                let eq = self.fresh_reg();
                self.line(&format!("  {eq} = icmp eq i32 {cmp}, 0"));
                Ok(eq)
            }
            Pattern::BoolLit(b) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = icmp eq i1 {}, {}",
                    val.reg,
                    if *b { "true" } else { "false" }
                ));
                Ok(r)
            }
            Pattern::TritLit(t) => {
                let r = self.fresh_reg();
                self.line(&format!("  {r} = icmp eq i8 {}, {}", val.reg, t.value()));
                Ok(r)
            }
            Pattern::StrLit(s) => {
                let sh = self.emit_str_handle(cg, s);
                let eq8 = self.fresh_reg();
                self.line(&format!(
                    "  {eq8} = call i8 @aod_rt_handle_eq(ptr %ctx, i64 {}, i64 {sh})",
                    val.reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = icmp ne i8 {eq8}, 0"));
                Ok(r)
            }
            Pattern::RatioLit(rat) => {
                let lit = self.emit_ratio_lit(cg, rat);
                let cmp = self.fresh_reg();
                self.line(&format!(
                    "  {cmp} = call i32 @__aod_ratio_cmp(ptr %ctx, %DecRatio {}, %DecRatio {})",
                    val.reg, lit.reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = icmp eq i32 {cmp}, 0"));
                Ok(r)
            }
            Pattern::Addr8Lit(a) => {
                let (g, len) = cg.intern_bytes(a.render().as_bytes());
                let ah = self.fresh_reg();
                self.line(&format!(
                    "  {ah} = call i64 @aod_rt_addr8_parse(ptr %ctx, ptr {g}, i64 {len})"
                ));
                let eq8 = self.fresh_reg();
                self.line(&format!(
                    "  {eq8} = call i8 @aod_rt_handle_eq(ptr %ctx, i64 {}, i64 {ah})",
                    val.reg
                ));
                let r = self.fresh_reg();
                self.line(&format!("  {r} = icmp ne i8 {eq8}, 0"));
                Ok(r)
            }
            Pattern::SupportLit(sf) => {
                let (g, len) = cg.intern_bytes(sf.render().as_bytes());
                let sh = self.fresh_reg();
                self.line(&format!(
                    "  {sh} = call i64 @aod_rt_support_parse(ptr %ctx, ptr {g}, i64 {len})"
                ));
                let eq8 = self.fresh_reg();
                self.line(&format!(
                    "  {eq8} = call i8 @aod_rt_handle_eq(ptr %ctx, i64 {}, i64 {sh})",
                    val.reg
                ));
                let r = self.fresh_reg();
                self.line(&format!("  {r} = icmp ne i8 {eq8}, 0"));
                Ok(r)
            }
            Pattern::EnumVariant { path, bindings } => {
                let (em_tag, payload_tys) = {
                    let (em, vidx) = cg
                        .resolve_enum_variant(&self.mod_key, path)
                        .ok_or_else(|| {
                            Diagnostic::new(
                                DiagnosticCode::RuntimeError,
                                format!("codegen: unknown enum pattern {}", path.join("::")),
                            )
                        })?;
                    (em.variants[vidx].1, em.variants[vidx].2.clone())
                };
                let tag = self.fresh_reg();
                self.line(&format!(
                    "  {tag} = call i32 @aod_rt_enum_tag(ptr %ctx, i64 {})",
                    val.reg
                ));
                self.emit_fail_return_check();
                let tag_eq = self.fresh_reg();
                self.line(&format!("  {tag_eq} = icmp eq i32 {tag}, {em_tag}"));
                if bindings.is_empty() {
                    return Ok(tag_eq);
                }
                let ok_lbl = self.fresh_label("ev_ok");
                let fail_lbl = self.fresh_label("ev_fail");
                let join_lbl = self.fresh_label("ev_join");
                let res_slot = self.alloc_temp("i1");
                self.line(&format!("  store i1 false, ptr {res_slot}"));
                self.line(&format!(
                    "  br i1 {tag_eq}, label %{ok_lbl}, label %{fail_lbl}"
                ));
                self.start_block(&ok_lbl);
                let mut acc = "true".to_string();
                for (i, (bp, pty)) in bindings.iter().zip(&payload_tys).enumerate() {
                    let ph = self.fresh_reg();
                    self.line(&format!(
                        "  {ph} = call i64 @aod_rt_enum_payload(ptr %ctx, i64 {}, i64 {i})",
                        val.reg
                    ));
                    self.emit_fail_return_check();
                    let pv = self.unbox_handle_to_val(&ph, pty);
                    let sub_ok = self.emit_pattern_match(cg, bp, &pv)?;
                    let next_acc = self.fresh_reg();
                    self.line(&format!("  {next_acc} = and i1 {acc}, {sub_ok}"));
                    acc = next_acc;
                }
                self.line(&format!("  store i1 {acc}, ptr {res_slot}"));
                self.line(&format!("  br label %{join_lbl}"));
                self.start_block(&fail_lbl);
                self.line(&format!("  br label %{join_lbl}"));
                self.start_block(&join_lbl);
                let final_r = self.fresh_reg();
                self.line(&format!("  {final_r} = load i1, ptr {res_slot}"));
                Ok(final_r)
            }
            Pattern::Tuple(sub_pats) => {
                let elem_tys = match val.dec_ty.unwrap_read_only() {
                    Type::Tuple(ts) => ts.clone(),
                    _ => Vec::new(),
                };
                let mut acc = "true".to_string();
                for (i, (sp, et)) in sub_pats.iter().zip(&elem_tys).enumerate() {
                    let fh = self.fresh_reg();
                    self.line(&format!(
                        "  {fh} = call i64 @aod_rt_tuple_get(ptr %ctx, i64 {}, i64 {i})",
                        val.reg
                    ));
                    self.emit_fail_return_check();
                    let fv = self.unbox_handle_to_val(&fh, et);
                    let sub_ok = self.emit_pattern_match(cg, sp, &fv)?;
                    let next_acc = self.fresh_reg();
                    self.line(&format!("  {next_acc} = and i1 {acc}, {sub_ok}"));
                    acc = next_acc;
                }
                Ok(acc)
            }
        }
    }

    // ==================== Expressions ====================

    fn emit_bigint_lit(&mut self, cg: &mut Codegen<'_>, n: &BigInt) -> Val {
        if let Some(i) = n.to_i64() {
            let r = self.fresh_reg();
            self.line(&format!(
                "  {r} = insertvalue %DecInt {{ i8 0, i64 0 }}, i64 {i}, 1"
            ));
            Val {
                reg: r,
                dec_ty: Type::Int,
            }
        } else {
            let dec_s = n.to_decimal_string();
            let (g, len) = cg.intern_bytes(dec_s.as_bytes());
            let out = self.alloc_temp("%DecInt");
            self.line(&format!(
                "  call void @aod_rt_int_from_str(ptr %ctx, ptr {g}, i64 {len}, ptr {out})"
            ));
            self.emit_fail_return_check();
            let r = self.fresh_reg();
            self.line(&format!("  {r} = load %DecInt, ptr {out}"));
            Val {
                reg: r,
                dec_ty: Type::Int,
            }
        }
    }

    fn emit_ratio_lit(&mut self, cg: &mut Codegen<'_>, rat: &aod_core::ratio::Ratio) -> Val {
        if let (Some(n), Some(d)) = (rat.num().to_i64(), rat.den().to_i64()) {
            let r1 = self.fresh_reg();
            let r2 = self.fresh_reg();
            self.line(&format!(
                "  {r1} = insertvalue %DecRatio {{ i8 0, i64 0, i64 1 }}, i64 {n}, 1"
            ));
            self.line(&format!("  {r2} = insertvalue %DecRatio {r1}, i64 {d}, 2"));
            Val {
                reg: r2,
                dec_ty: Type::Ratio,
            }
        } else {
            let ns = rat.num().to_decimal_string();
            let ds = rat.den().to_decimal_string();
            let (ng, nlen) = cg.intern_bytes(ns.as_bytes());
            let (dg, dlen) = cg.intern_bytes(ds.as_bytes());
            let out = self.alloc_temp("%DecRatio");
            self.line(&format!(
                "  call void @aod_rt_ratio_from_str(ptr %ctx, ptr {ng}, i64 {nlen}, ptr {dg}, i64 {dlen}, ptr {out})"
            ));
            self.emit_fail_return_check();
            let r = self.fresh_reg();
            self.line(&format!("  {r} = load %DecRatio, ptr {out}"));
            Val {
                reg: r,
                dec_ty: Type::Ratio,
            }
        }
    }

    fn emit_expr(&mut self, cg: &mut Codegen<'_>, expr: &Expr) -> DecResult<Val> {
        match &expr.kind {
            ExprKind::IntLit(n) => Ok(self.emit_bigint_lit(cg, n)),
            ExprKind::BoolLit(b) => Ok(Val {
                reg: if *b { "true" } else { "false" }.to_string(),
                dec_ty: Type::Bool,
            }),
            ExprKind::StrLit(s) => {
                let h = self.emit_str_handle(cg, s);
                Ok(Val {
                    reg: h,
                    dec_ty: Type::Str,
                })
            }
            ExprKind::RatioLit(rat) => Ok(self.emit_ratio_lit(cg, rat)),
            ExprKind::Addr8Lit(a) => {
                let (g, len) = cg.intern_bytes(a.render().as_bytes());
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_addr8_parse(ptr %ctx, ptr {g}, i64 {len})"
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Addr8,
                })
            }
            ExprKind::SupportLit(sf) => {
                let (g, len) = cg.intern_bytes(sf.render().as_bytes());
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_support_parse(ptr %ctx, ptr {g}, i64 {len})"
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Support,
                })
            }
            ExprKind::TritLit(tl) => {
                let v = match tl {
                    TritLiteral::One => "1",
                    TritLiteral::Zero | TritLiteral::Hinge0H | TritLiteral::Dwell0D => "0",
                };
                Ok(Val {
                    reg: v.to_string(),
                    dec_ty: Type::T1,
                })
            }
            ExprKind::Ident(name) => {
                if let Some(slot) = self.lookup_var(name) {
                    let lt = LlvmTy::from_dec(&slot.dec_ty);
                    if lt == LlvmTy::Void {
                        return Ok(Val {
                            reg: String::new(),
                            dec_ty: slot.dec_ty,
                        });
                    }
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = load {}, ptr {}", lt.ir(), slot.alloca));
                    return Ok(Val {
                        reg: r,
                        dec_ty: slot.dec_ty,
                    });
                }
                if let Some(cm) = cg.consts.get(&(self.mod_key.clone(), name.clone())).cloned() {
                    return Ok(self.load_const(&cm));
                }
                let base = cg.base_module_of(&self.mod_key).to_string();
                if let Some(cm) = cg.consts.get(&(base, name.clone())).cloned() {
                    return Ok(self.load_const(&cm));
                }
                if let Some(ImportedSymbol::ModuleItem { module, item }) =
                    cg.imports_for(&self.mod_key).get(name).cloned()
                {
                    if let Some(cm) = cg.consts.get(&(module, item)).cloned() {
                        return Ok(self.load_const(&cm));
                    }
                }
                Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    expr.line,
                    format!("codegen: undefined ident {name}"),
                ))
            }
            ExprKind::Path(segs) => {
                if let Some((em, vidx)) = cg.resolve_enum_variant(&self.mod_key, segs) {
                    let em_mod = em.module.clone();
                    let em_name = em.name.clone();
                    let (vname, vtag, _) = em.variants[vidx].clone();
                    let mh = self.emit_str_handle(cg, &em_mod);
                    let nh = self.emit_str_handle(cg, &em_name);
                    let vh = self.emit_str_handle(cg, &vname);
                    let r = self.fresh_reg();
                    self.line(&format!(
                        "  {r} = call i64 @aod_rt_enum_new(ptr %ctx, i64 {mh}, i64 {nh}, i64 {vh}, i32 {vtag}, ptr null, i64 0)"
                    ));
                    self.emit_fail_return_check();
                    return Ok(Val {
                        reg: r,
                        dec_ty: Type::Enum {
                            module: em_mod,
                            name: em_name,
                        },
                    });
                }
                if segs.len() == 2 {
                    if let Some(qsub) = cg.resolve_qext_submodule(&self.mod_key, &segs[0]) {
                        if let Some(cm) = cg.consts.get(&(qsub, segs[1].clone())).cloned() {
                            return Ok(self.load_const(&cm));
                        }
                    }
                    if let Some(ImportedSymbol::ModuleNamespace(m)) =
                        cg.imports_for(&self.mod_key).get(&segs[0]).cloned()
                    {
                        if let Some(cm) = cg.consts.get(&(m, segs[1].clone())).cloned() {
                            return Ok(self.load_const(&cm));
                        }
                    }
                } else if segs.len() == 3 {
                    if let Some(qsub) =
                        cg.resolve_qext_submodule_3seg(&self.mod_key, &segs[0], &segs[1])
                    {
                        if let Some(cm) = cg.consts.get(&(qsub, segs[2].clone())).cloned() {
                            return Ok(self.load_const(&cm));
                        }
                    }
                }
                Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    expr.line,
                    format!("codegen: undefined path {}", segs.join("::")),
                ))
            }
            ExprKind::List(items) => {
                let mut elem_ty = Type::Unknown;
                let mut handles = Vec::with_capacity(items.len());
                for it in items {
                    let v = self.emit_expr(cg, it)?;
                    if elem_ty == Type::Unknown {
                        elem_ty = v.dec_ty.clone();
                    }
                    handles.push(self.box_val_to_handle(&v));
                }
                let h = self.emit_handle_array_call("aod_rt_list_new", &handles);
                Ok(Val {
                    reg: h,
                    dec_ty: Type::List(Box::new(elem_ty)),
                })
            }
            ExprKind::Tuple(elems) => {
                if elems.is_empty() {
                    return Ok(Val {
                        reg: String::new(),
                        dec_ty: Type::Unit,
                    });
                }
                let mut tys = Vec::with_capacity(elems.len());
                let mut handles = Vec::with_capacity(elems.len());
                for e in elems {
                    let v = self.emit_expr(cg, e)?;
                    tys.push(v.dec_ty.clone());
                    handles.push(self.box_val_to_handle(&v));
                }
                let h = self.emit_handle_array_call("aod_rt_tuple_new", &handles);
                Ok(Val {
                    reg: h,
                    dec_ty: Type::Tuple(tys),
                })
            }
            ExprKind::MapLit(entries) => {
                let mut kt = Type::Unknown;
                let mut vt = Type::Unknown;
                let mut khs = Vec::with_capacity(entries.len());
                let mut vhs = Vec::with_capacity(entries.len());
                for (ke, ve) in entries {
                    let kv = self.emit_expr(cg, ke)?;
                    let vv = self.emit_expr(cg, ve)?;
                    if kt == Type::Unknown {
                        kt = kv.dec_ty.clone();
                        vt = vv.dec_ty.clone();
                    }
                    khs.push(self.box_val_to_handle(&kv));
                    vhs.push(self.box_val_to_handle(&vv));
                }
                let n = khs.len();
                let r = if n == 0 {
                    let r = self.fresh_reg();
                    self.line(&format!(
                        "  {r} = call i64 @aod_rt_map_new(ptr %ctx, ptr null, ptr null, i64 0)"
                    ));
                    r
                } else {
                    let karr = self.alloc_temp(&format!("[{n} x i64]"));
                    let varr = self.alloc_temp(&format!("[{n} x i64]"));
                    for (i, (kh, vh)) in khs.iter().zip(&vhs).enumerate() {
                        let kp = self.fresh_reg();
                        let vp = self.fresh_reg();
                        self.line(&format!(
                            "  {kp} = getelementptr inbounds [{n} x i64], ptr {karr}, i64 0, i64 {i}"
                        ));
                        self.line(&format!("  store i64 {kh}, ptr {kp}"));
                        self.line(&format!(
                            "  {vp} = getelementptr inbounds [{n} x i64], ptr {varr}, i64 0, i64 {i}"
                        ));
                        self.line(&format!("  store i64 {vh}, ptr {vp}"));
                    }
                    let r = self.fresh_reg();
                    self.line(&format!(
                        "  {r} = call i64 @aod_rt_map_new(ptr %ctx, ptr {karr}, ptr {varr}, i64 {n})"
                    ));
                    r
                };
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Map(Box::new(kt), Box::new(vt)),
                })
            }
            ExprKind::StructLit { path, fields } => {
                let sm = cg
                    .resolve_struct(&self.mod_key, path)
                    .cloned()
                    .ok_or_else(|| {
                        Diagnostic::at_line(
                            DiagnosticCode::RuntimeError,
                            expr.line,
                            format!("codegen: unknown struct {}", path.join("::")),
                        )
                    })?;
                let mut pairs: Vec<(String, Val)> = Vec::with_capacity(fields.len());
                for (fname, fexpr) in fields {
                    let v = self.emit_expr(cg, fexpr)?;
                    pairs.push((fname.clone(), v));
                }
                self.emit_struct_construct(cg, &sm, &pairs)
            }
            ExprKind::WindowLit { start, end } => {
                let u = self.emit_expr(cg, start)?;
                let v = self.emit_expr(cg, end)?;
                let up = self.store_temp_int(&u.reg);
                let vp = self.store_temp_int(&v.reg);
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_window_new(ptr %ctx, ptr {up}, ptr {vp})"
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Window,
                })
            }
            ExprKind::Range { start, end } => {
                let a = self.emit_expr(cg, start)?;
                let b = self.emit_expr(cg, end)?;
                let ap = self.store_temp_int(&a.reg);
                let bp = self.store_temp_int(&b.reg);
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_range_new(ptr %ctx, ptr {ap}, ptr {bp})"
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Range,
                })
            }
            ExprKind::BoxExpr(dims) => {
                let d0 = self.emit_expr(cg, &dims[0])?;
                let d1 = self.emit_expr(cg, &dims[1])?;
                let d2 = self.emit_expr(cg, &dims[2])?;
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_box_new(ptr %ctx, i64 {}, i64 {}, i64 {})",
                    d0.reg, d1.reg, d2.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Box3,
                })
            }
            ExprKind::Simulate {
                prepare,
                ticks,
                observers,
            } => self.emit_simulate(cg, prepare, ticks, observers, expr.line),
            ExprKind::Unary { op, operand } => {
                let v = self.emit_expr(cg, operand)?;
                self.emit_unary(*op, v)
            }
            ExprKind::Binary { op, left, right } => {
                // Short-circuit `and` / `or` via PHI blocks (untaken branch has zero effects!)
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    let lv = self.emit_expr(cg, left)?;
                    let lhs_lbl = self.fresh_label("sc_lhs");
                    let rhs_lbl = self.fresh_label("sc_rhs");
                    let join_lbl = self.fresh_label("sc_join");
                    let res_slot = self.alloc_temp("i1");
                    self.line(&format!("  store i1 {}, ptr {res_slot}", lv.reg));
                    if *op == BinaryOp::And {
                        self.line(&format!(
                            "  br i1 {}, label %{rhs_lbl}, label %{lhs_lbl}",
                            lv.reg
                        ));
                    } else {
                        self.line(&format!(
                            "  br i1 {}, label %{lhs_lbl}, label %{rhs_lbl}",
                            lv.reg
                        ));
                    }
                    self.start_block(&lhs_lbl);
                    self.line(&format!("  br label %{join_lbl}"));
                    self.start_block(&rhs_lbl);
                    let rv = self.emit_expr(cg, right)?;
                    self.line(&format!("  store i1 {}, ptr {res_slot}", rv.reg));
                    if !self.terminated {
                        self.line(&format!("  br label %{join_lbl}"));
                        self.terminated = true;
                    }
                    self.start_block(&join_lbl);
                    let out = self.fresh_reg();
                    self.line(&format!("  {out} = load i1, ptr {res_slot}"));
                    return Ok(Val {
                        reg: out,
                        dec_ty: lv.dec_ty,
                    });
                }
                let lv = self.emit_expr(cg, left)?;
                let rv = self.emit_expr(cg, right)?;
                self.emit_binary(cg, *op, lv, rv)
            }
            ExprKind::Ternary { cond, yes, no } => {
                // Sound P0-3 branchless `select` lowering when both arms pass strict whitelist
                if cg.plan.branchless_select
                    && self.is_pure_select_safe(cg, yes)
                    && self.is_pure_select_safe(cg, no)
                {
                    let cv = self.emit_expr(cg, cond)?;
                    let yv = self.emit_expr(cg, yes)?;
                    let nv = self.emit_expr(cg, no)?;
                    let lt = yv.llvm_ty();
                    if lt != LlvmTy::Void {
                        let sel = self.fresh_reg();
                        self.line(&format!(
                            "  {sel} = select i1 {}, {} {}, {} {}",
                            cv.reg,
                            lt.ir(),
                            yv.reg,
                            lt.ir(),
                            nv.reg
                        ));
                        return Ok(Val {
                            reg: sel,
                            dec_ty: yv.dec_ty,
                        });
                    }
                    return Ok(Val {
                        reg: String::new(),
                        dec_ty: yv.dec_ty,
                    });
                }

                let cv = self.emit_expr(cg, cond)?;
                let y_lbl = self.fresh_label("tern_yes");
                let n_lbl = self.fresh_label("tern_no");
                let j_lbl = self.fresh_label("tern_join");
                self.line(&format!(
                    "  br i1 {}, label %{y_lbl}, label %{n_lbl}",
                    cv.reg
                ));
                self.start_block(&y_lbl);
                let yv = self.emit_expr(cg, yes)?;
                let lt = yv.llvm_ty();
                let res_slot = if lt != LlvmTy::Void {
                    let s = self.alloc_temp(lt.ir());
                    self.line(&format!("  store {} {}, ptr {s}", lt.ir(), yv.reg));
                    Some(s)
                } else {
                    None
                };
                if !self.terminated {
                    self.line(&format!("  br label %{j_lbl}"));
                    self.terminated = true;
                }
                self.start_block(&n_lbl);
                let nv = self.emit_expr(cg, no)?;
                if let Some(ref s) = res_slot {
                    self.line(&format!("  store {} {}, ptr {s}", lt.ir(), nv.reg));
                }
                if !self.terminated {
                    self.line(&format!("  br label %{j_lbl}"));
                    self.terminated = true;
                }
                self.start_block(&j_lbl);
                let reg = if let Some(s) = res_slot {
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = load {}, ptr {s}", lt.ir()));
                    r
                } else {
                    String::new()
                };
                Ok(Val {
                    reg,
                    dec_ty: yv.dec_ty,
                })
            }
            ExprKind::Field { base, name } => {
                let bv = self.emit_expr(cg, base)?;
                let field_ty = self.resolve_field_type(cg, &bv.dec_ty, name, expr.line)?;
                let (fg, flen) = cg.intern_bytes(name.as_bytes());
                let fh = self.fresh_reg();
                self.line(&format!(
                    "  {fh} = call i64 @aod_rt_field_get(ptr %ctx, i64 {}, ptr {fg}, i64 {flen})",
                    bv.reg
                ));
                self.emit_fail_return_check();
                Ok(self.unbox_handle_to_val(&fh, &field_ty))
            }
            ExprKind::Index { base, index } => {
                let bv = self.emit_expr(cg, base)?;
                let iv = self.emit_expr(cg, index)?;
                match bv.dec_ty.unwrap_read_only().clone() {
                    Type::List(elem_ty) => {
                        let ip = self.store_temp_int(&iv.reg);
                        let eh = self.fresh_reg();
                        self.line(&format!(
                            "  {eh} = call i64 @aod_rt_list_get(ptr %ctx, i64 {}, ptr {ip})",
                            bv.reg
                        ));
                        self.emit_fail_return_check();
                        Ok(self.unbox_handle_to_val(&eh, &elem_ty))
                    }
                    Type::Map(_, v_ty) => {
                        let kh = self.box_val_to_handle(&iv);
                        let vh = self.fresh_reg();
                        self.line(&format!(
                            "  {vh} = call i64 @aod_rt_map_get(ptr %ctx, i64 {}, i64 {kh})",
                            bv.reg
                        ));
                        self.emit_fail_return_check();
                        Ok(self.unbox_handle_to_val(&vh, &v_ty))
                    }
                    other => Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        expr.line,
                        format!("codegen: cannot index {other}"),
                    )),
                }
            }
            ExprKind::Call { callee, args } => self.emit_call(cg, callee, args, expr.line),
        }
    }

    fn load_const(&mut self, cm: &ConstMeta) -> Val {
        let lt = LlvmTy::from_dec(&cm.dec_ty);
        if lt == LlvmTy::Void {
            return Val {
                reg: String::new(),
                dec_ty: cm.dec_ty.clone(),
            };
        }
        let r = self.fresh_reg();
        self.line(&format!("  {r} = load {}, ptr {}", lt.ir(), cm.global_name));
        Val {
            reg: r,
            dec_ty: cm.dec_ty.clone(),
        }
    }

    fn emit_handle_array_call(&mut self, fn_name: &str, handles: &[String]) -> String {
        let n = handles.len();
        let r = if n == 0 {
            let r = self.fresh_reg();
            self.line(&format!("  {r} = call i64 @{fn_name}(ptr %ctx, ptr null, i64 0)"));
            r
        } else {
            let arr = self.alloc_temp(&format!("[{n} x i64]"));
            for (i, h) in handles.iter().enumerate() {
                let p = self.fresh_reg();
                self.line(&format!(
                    "  {p} = getelementptr inbounds [{n} x i64], ptr {arr}, i64 0, i64 {i}"
                ));
                self.line(&format!("  store i64 {h}, ptr {p}"));
            }
            let r = self.fresh_reg();
            self.line(&format!(
                "  {r} = call i64 @{fn_name}(ptr %ctx, ptr {arr}, i64 {n})"
            ));
            r
        };
        self.emit_fail_return_check();
        r
    }

    fn emit_struct_construct(
        &mut self,
        cg: &mut Codegen<'_>,
        sm: &StructMeta,
        pairs: &[(String, Val)],
    ) -> DecResult<Val> {
        let mh = self.emit_str_handle(cg, &sm.module);
        let nh = self.emit_str_handle(cg, &sm.name);
        let n = pairs.len();
        let r = if n == 0 {
            let r = self.fresh_reg();
            self.line(&format!(
                "  {r} = call i64 @aod_rt_struct_new(ptr %ctx, i64 {mh}, i64 {nh}, ptr null, ptr null, i64 0)"
            ));
            r
        } else {
            let narr = self.alloc_temp(&format!("[{n} x i64]"));
            let varr = self.alloc_temp(&format!("[{n} x i64]"));
            for (i, (fname, fval)) in pairs.iter().enumerate() {
                let fnh = self.emit_str_handle(cg, fname);
                let fvh = self.box_val_to_handle(fval);
                let np = self.fresh_reg();
                let vp = self.fresh_reg();
                self.line(&format!(
                    "  {np} = getelementptr inbounds [{n} x i64], ptr {narr}, i64 0, i64 {i}"
                ));
                self.line(&format!("  store i64 {fnh}, ptr {np}"));
                self.line(&format!(
                    "  {vp} = getelementptr inbounds [{n} x i64], ptr {varr}, i64 0, i64 {i}"
                ));
                self.line(&format!("  store i64 {fvh}, ptr {vp}"));
            }
            let r = self.fresh_reg();
            self.line(&format!(
                "  {r} = call i64 @aod_rt_struct_new(ptr %ctx, i64 {mh}, i64 {nh}, ptr {narr}, ptr {varr}, i64 {n})"
            ));
            r
        };
        self.emit_fail_return_check();
        Ok(Val {
            reg: r,
            dec_ty: Type::Struct {
                module: sm.module.clone(),
                name: sm.name.clone(),
            },
        })
    }

    fn resolve_field_type(
        &self,
        cg: &Codegen<'_>,
        base_ty: &Type,
        field: &str,
        line: usize,
    ) -> DecResult<Type> {
        let inner = base_ty.unwrap_read_only();
        if let Type::Struct { module, name } = inner {
            if let Some(sm) = cg.structs.get(&(module.clone(), name.clone())) {
                if let Some((_, ft)) = sm.fields.iter().find(|(n, _)| n == field) {
                    return Ok(ft.clone());
                }
            }
        }
        match (inner, field) {
            (Type::SimResult, "model_status" | "chamber_name" | "final_state_hash") => Ok(Type::Str),
            (
                Type::SimResult,
                "ticks"
                | "carrier_count"
                | "shedding_event_count"
                | "wavelet_event_count"
                | "boundary_crossings"
                | "total_returns"
                | "total_hinges"
                | "total_shedding_events",
            ) => Ok(Type::Int),
            (Type::SimResult, "final_snapshot") => {
                Ok(Type::ReadOnly(Box::new(Type::ChamberView)))
            }
            (Type::ChamberView, "model_status" | "chamber_name") => Ok(Type::Str),
            (
                Type::ChamberView,
                "tick"
                | "carrier_count"
                | "boundary_crossings"
                | "shedding_count"
                | "wavelet_count"
                | "clock_count",
            ) => Ok(Type::Int),
            (Type::ChamberView, "carriers") => {
                Ok(Type::ReadOnly(Box::new(Type::List(Box::new(Type::Int)))))
            }
            (Type::RecipientOutcome, "stage") => Ok(Type::Str),
            (Type::RecipientOutcome, "closure_phase_delta" | "reclosure_bip_delta") => {
                Ok(Type::Int)
            }
            (Type::RecipientOutcome, "blocks_later_seating") => Ok(Type::Bool),
            (Type::CadenceAssociation, "is_cadence_ratio") => Ok(Type::Bool),
            (Type::CadenceAssociation, "wavelet_id" | "reference_id") => Ok(Type::Str),
            (Type::Support, "is_yro" | "is_closure_certificate") => Ok(Type::Bool),
            (Type::Addr8, "is_first_anchor") => Ok(Type::Bool),
            (Type::Window, "start" | "end" | "len") => Ok(Type::Int),
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!("codegen: unknown field .{field} on {inner}"),
            )),
        }
    }

    fn emit_unary(&mut self, op: UnaryOp, v: Val) -> DecResult<Val> {
        match (op, v.dec_ty.unwrap_read_only().clone()) {
            (UnaryOp::Not, ty) => {
                let r = self.fresh_reg();
                self.line(&format!("  {r} = xor i1 {}, true", v.reg));
                Ok(Val {
                    reg: r,
                    dec_ty: ty,
                })
            }
            (UnaryOp::Pos, _) => Ok(v),
            (UnaryOp::Neg, Type::Int) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call %DecInt @__aod_int_neg(ptr %ctx, %DecInt {})",
                    v.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Int,
                })
            }
            (UnaryOp::Neg, Type::T1) => {
                let r = self.fresh_reg();
                self.line(&format!("  {r} = sub i8 0, {}", v.reg));
                Ok(Val {
                    reg: r,
                    dec_ty: Type::T1,
                })
            }
            (UnaryOp::Neg, Type::T32) => {
                let r = self.fresh_reg();
                self.line(&format!("  {r} = sub i64 0, {}", v.reg));
                Ok(Val {
                    reg: r,
                    dec_ty: Type::T32,
                })
            }
            (UnaryOp::Neg, Type::Ratio) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call %DecRatio @__aod_ratio_neg(ptr %ctx, %DecRatio {})",
                    v.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Ratio,
                })
            }
            (UnaryOp::Neg, Type::QExt(inner)) if *inner == Type::Ratio => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_qext_neg(ptr %ctx, i64 {})",
                    v.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::QExt(Box::new(Type::Ratio)),
                })
            }
            (UnaryOp::Boundary, ty) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_boundary_of(ptr %ctx, i64 {})",
                    v.reg
                ));
                self.emit_fail_return_check();
                let ret_ty = match ty {
                    Type::Box3 | Type::SimResult => Type::Str,
                    _ => Type::Tuple(vec![Type::Int, Type::Int]),
                };
                Ok(Val {
                    reg: r,
                    dec_ty: ret_ty,
                })
            }
            _ => Err(Diagnostic::new(
                DiagnosticCode::RuntimeError,
                "codegen: unsupported unary op",
            )),
        }
    }

    /// Strict P0-3 pure-expression whitelist for sound branchless `select` lowering.
    /// NEVER permits trapping, spilling, allocating, or side-effecting expressions to be evaluated eagerly!
    fn peek_pure_type(&self, cg: &Codegen<'_>, expr: &Expr) -> Option<Type> {
        match &expr.kind {
            ExprKind::IntLit(n) if n.to_i64().is_some() => Some(Type::Int),
            ExprKind::BoolLit(_) => Some(Type::Bool),
            ExprKind::TritLit(_) => Some(Type::T1),
            ExprKind::RatioLit(r) if r.num().to_i64().is_some() && r.den().to_i64().is_some() => {
                Some(Type::Ratio)
            }
            ExprKind::Ident(name) => {
                if let Some(slot) = self.lookup_var(name) {
                    return Some(slot.dec_ty.unwrap_read_only().clone());
                }
                if let Some(cm) = cg.consts.get(&(self.mod_key.clone(), name.clone())) {
                    return Some(cm.dec_ty.unwrap_read_only().clone());
                }
                let base = cg.base_module_of(&self.mod_key).to_string();
                if let Some(cm) = cg.consts.get(&(base, name.clone())) {
                    return Some(cm.dec_ty.unwrap_read_only().clone());
                }
                if let Some(ImportedSymbol::ModuleItem { module, item }) =
                    cg.imports_for(&self.mod_key).get(name)
                {
                    if let Some(cm) = cg.consts.get(&(module.clone(), item.clone())) {
                        return Some(cm.dec_ty.unwrap_read_only().clone());
                    }
                }
                None
            }
            ExprKind::Unary { op, operand } => {
                let t = self.peek_pure_type(cg, operand)?;
                match op {
                    UnaryOp::Pos => Some(t),
                    UnaryOp::Not if t == Type::Bool => Some(Type::Bool),
                    UnaryOp::Neg if matches!(t, Type::T1 | Type::T32) => Some(t),
                    _ => None,
                }
            }
            ExprKind::Binary { op, left, right } => {
                let lt = self.peek_pure_type(cg, left)?;
                let rt = self.peek_pure_type(cg, right)?;
                if lt != rt {
                    return None;
                }
                match op {
                    BinaryOp::Eq
                    | BinaryOp::NotEq
                    | BinaryOp::Lt
                    | BinaryOp::LtEq
                    | BinaryOp::Gt
                    | BinaryOp::GtEq
                        if matches!(lt, Type::Bool | Type::T1 | Type::T32 | Type::Q4) =>
                    {
                        Some(Type::Bool)
                    }
                    BinaryOp::Mul if lt == Type::T1 => Some(Type::T1),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn is_pure_select_safe(&self, cg: &Codegen<'_>, expr: &Expr) -> bool {
        self.peek_pure_type(cg, expr).is_some()
    }

    fn emit_binary(&mut self, cg: &Codegen<'_>, op: BinaryOp, lv: Val, rv: Val) -> DecResult<Val> {
        let ty = lv.dec_ty.unwrap_read_only().clone();
        match ty {
            Type::Int => match op {
                BinaryOp::Add => self.emit_int_helper("@__aod_int_add", &lv.reg, &rv.reg),
                BinaryOp::Sub => self.emit_int_helper("@__aod_int_sub", &lv.reg, &rv.reg),
                BinaryOp::Mul => self.emit_int_helper("@__aod_int_mul", &lv.reg, &rv.reg),
                BinaryOp::FloorDiv => self.emit_int_helper("@__aod_int_div_floor", &lv.reg, &rv.reg),
                BinaryOp::Mod => self.emit_int_helper("@__aod_int_rem_floor", &lv.reg, &rv.reg),
                BinaryOp::BitAnd => self.emit_int_helper("@__aod_int_bitand", &lv.reg, &rv.reg),
                BinaryOp::BitOr => self.emit_int_helper("@__aod_int_bitor", &lv.reg, &rv.reg),
                BinaryOp::BitXor => self.emit_int_helper("@__aod_int_bitxor", &lv.reg, &rv.reg),
                BinaryOp::Shl => self.emit_int_helper("@__aod_int_shl", &lv.reg, &rv.reg),
                BinaryOp::Shr => self.emit_int_helper("@__aod_int_shr", &lv.reg, &rv.reg),
                BinaryOp::Div => {
                    let r = self.fresh_reg();
                    self.line(&format!(
                        "  {r} = call %DecRatio @__aod_int_exact_div(ptr %ctx, %DecInt {}, %DecInt {})",
                        lv.reg, rv.reg
                    ));
                    self.emit_fail_return_check();
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::Ratio,
                    })
                }
                BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Lt
                | BinaryOp::LtEq
                | BinaryOp::Gt
                | BinaryOp::GtEq => {
                    let cmp = self.fresh_reg();
                    self.line(&format!(
                        "  {cmp} = call i32 @__aod_int_cmp(ptr %ctx, %DecInt {}, %DecInt {})",
                        lv.reg, rv.reg
                    ));
                    self.emit_fail_return_check();
                    Ok(self.cmp3_to_bool(&cmp, op, Type::Bool))
                }
                _ => unreachable!(),
            },
            Type::T1 => match op {
                BinaryOp::Add => self.emit_t1_helper("@__aod_t1_add", &lv.reg, &rv.reg),
                BinaryOp::Sub => self.emit_t1_helper("@__aod_t1_sub", &lv.reg, &rv.reg),
                BinaryOp::Mul => self.emit_t1_helper("@__aod_t1_mul", &lv.reg, &rv.reg),
                BinaryOp::Div => {
                    let n = self.fresh_reg();
                    let d = self.fresh_reg();
                    self.line(&format!("  {n} = sext i8 {} to i128", lv.reg));
                    self.line(&format!("  {d} = sext i8 {} to i128", rv.reg));
                    let r = self.fresh_reg();
                    self.line(&format!(
                        "  {r} = call %DecRatio @__aod_ratio_norm_i128(ptr %ctx, i128 {n}, i128 {d})"
                    ));
                    self.emit_fail_return_check();
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::Ratio,
                    })
                }
                _ => Ok(self.emit_scalar_cmp("i8", op, &lv.reg, &rv.reg, true)),
            },
            Type::T32 => match op {
                BinaryOp::Add => self.emit_t32_helper("@__aod_t32_add", &lv.reg, &rv.reg),
                BinaryOp::Sub => self.emit_t32_helper("@__aod_t32_sub", &lv.reg, &rv.reg),
                BinaryOp::Mul => self.emit_t32_helper("@__aod_t32_mul", &lv.reg, &rv.reg),
                BinaryOp::FloorDiv => self.emit_t32_helper("@__aod_t32_div_floor", &lv.reg, &rv.reg),
                BinaryOp::Mod => self.emit_t32_helper("@__aod_t32_rem_floor", &lv.reg, &rv.reg),
                BinaryOp::Div => {
                    let n = self.fresh_reg();
                    let d = self.fresh_reg();
                    self.line(&format!("  {n} = sext i64 {} to i128", lv.reg));
                    self.line(&format!("  {d} = sext i64 {} to i128", rv.reg));
                    let r = self.fresh_reg();
                    self.line(&format!(
                        "  {r} = call %DecRatio @__aod_ratio_norm_i128(ptr %ctx, i128 {n}, i128 {d})"
                    ));
                    self.emit_fail_return_check();
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::Ratio,
                    })
                }
                _ => Ok(self.emit_scalar_cmp("i64", op, &lv.reg, &rv.reg, true)),
            },
            Type::Q4 => Ok(self.emit_scalar_cmp("i8", op, &lv.reg, &rv.reg, false)),
            Type::Bool => Ok(self.emit_scalar_cmp("i1", op, &lv.reg, &rv.reg, false)),
            Type::QExt(ref inner) if **inner == Type::Bool => {
                let mut v = self.emit_scalar_cmp("i1", op, &lv.reg, &rv.reg, false);
                v.dec_ty = Type::QExt(Box::new(Type::Bool));
                Ok(v)
            }
            Type::Ratio => match op {
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                    if cg.plan.inline_ratio_fastpath {
                        let helper = match op {
                            BinaryOp::Add => "@__aod_ratio_add",
                            BinaryOp::Sub => "@__aod_ratio_sub",
                            BinaryOp::Mul => "@__aod_ratio_mul",
                            BinaryOp::Div => "@__aod_ratio_div",
                            _ => unreachable!(),
                        };
                        self.emit_ratio_helper(helper, &lv.reg, &rv.reg)
                    } else {
                        let rt_fn = match op {
                            BinaryOp::Add => "@aod_rt_ratio_add",
                            BinaryOp::Sub => "@aod_rt_ratio_sub",
                            BinaryOp::Mul => "@aod_rt_ratio_mul",
                            BinaryOp::Div => "@aod_rt_ratio_div",
                            _ => unreachable!(),
                        };
                        let pa = self.store_temp_ratio(&lv.reg);
                        let pb = self.store_temp_ratio(&rv.reg);
                        let out = self.alloc_temp("%DecRatio");
                        self.line(&format!(
                            "  call void {rt_fn}(ptr %ctx, ptr {pa}, ptr {pb}, ptr {out})"
                        ));
                        self.emit_fail_return_check();
                        let r = self.fresh_reg();
                        self.line(&format!("  {r} = load %DecRatio, ptr {out}"));
                        Ok(Val {
                            reg: r,
                            dec_ty: Type::Ratio,
                        })
                    }
                }
                BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Lt
                | BinaryOp::LtEq
                | BinaryOp::Gt
                | BinaryOp::GtEq => {
                    let cmp = self.fresh_reg();
                    self.line(&format!(
                        "  {cmp} = call i32 @__aod_ratio_cmp(ptr %ctx, %DecRatio {}, %DecRatio {})",
                        lv.reg, rv.reg
                    ));
                    self.emit_fail_return_check();
                    Ok(self.cmp3_to_bool(&cmp, op, Type::Bool))
                }
                _ => unreachable!(),
            },
            Type::QExt(ref inner) if **inner == Type::Ratio => match op {
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                    let code = match op {
                        BinaryOp::Add => 0,
                        BinaryOp::Sub => 1,
                        BinaryOp::Mul => 2,
                        BinaryOp::Div => 3,
                        _ => unreachable!(),
                    };
                    let r = self.fresh_reg();
                    self.line(&format!(
                        "  {r} = call i64 @aod_rt_qext_binop(ptr %ctx, i8 {code}, i64 {}, i64 {})",
                        lv.reg, rv.reg
                    ));
                    self.emit_fail_return_check();
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::QExt(Box::new(Type::Ratio)),
                    })
                }
                BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Lt
                | BinaryOp::LtEq
                | BinaryOp::Gt
                | BinaryOp::GtEq => {
                    let cmp = self.fresh_reg();
                    self.line(&format!(
                        "  {cmp} = call i32 @aod_rt_qext_cmp(ptr %ctx, i64 {}, i64 {})",
                        lv.reg, rv.reg
                    ));
                    self.emit_fail_return_check();
                    Ok(self.cmp3_to_bool(&cmp, op, Type::QExt(Box::new(Type::Bool))))
                }
                _ => unreachable!(),
            },
            _ => {
                // Arena handle equality (`Str`, `Addr8`, `Support`, `Struct`, `Enum`, `List`, `Map`, `Tuple`, `QExt<Str>`, ...)
                let eq8 = self.fresh_reg();
                self.line(&format!(
                    "  {eq8} = call i8 @aod_rt_handle_eq(ptr %ctx, i64 {}, i64 {})",
                    lv.reg, rv.reg
                ));
                self.emit_fail_return_check();
                let pred = if op == BinaryOp::NotEq { "eq" } else { "ne" };
                let r = self.fresh_reg();
                self.line(&format!("  {r} = icmp {pred} i8 {eq8}, 0"));
                let out_ty = if ty.is_qext() {
                    Type::QExt(Box::new(Type::Bool))
                } else {
                    Type::Bool
                };
                Ok(Val {
                    reg: r,
                    dec_ty: out_ty,
                })
            }
        }
    }

    fn emit_int_helper(&mut self, helper: &str, a: &str, b: &str) -> DecResult<Val> {
        let r = self.fresh_reg();
        self.line(&format!(
            "  {r} = call %DecInt {helper}(ptr %ctx, %DecInt {a}, %DecInt {b})"
        ));
        self.emit_fail_return_check();
        Ok(Val {
            reg: r,
            dec_ty: Type::Int,
        })
    }

    fn emit_t1_helper(&mut self, helper: &str, a: &str, b: &str) -> DecResult<Val> {
        let r = self.fresh_reg();
        self.line(&format!("  {r} = call i8 {helper}(ptr %ctx, i8 {a}, i8 {b})"));
        self.emit_fail_return_check();
        Ok(Val {
            reg: r,
            dec_ty: Type::T1,
        })
    }

    fn emit_t32_helper(&mut self, helper: &str, a: &str, b: &str) -> DecResult<Val> {
        let r = self.fresh_reg();
        self.line(&format!(
            "  {r} = call i64 {helper}(ptr %ctx, i64 {a}, i64 {b})"
        ));
        self.emit_fail_return_check();
        Ok(Val {
            reg: r,
            dec_ty: Type::T32,
        })
    }

    fn emit_ratio_helper(&mut self, helper: &str, a: &str, b: &str) -> DecResult<Val> {
        let r = self.fresh_reg();
        self.line(&format!(
            "  {r} = call %DecRatio {helper}(ptr %ctx, %DecRatio {a}, %DecRatio {b})"
        ));
        self.emit_fail_return_check();
        Ok(Val {
            reg: r,
            dec_ty: Type::Ratio,
        })
    }

    fn cmp3_to_bool(&mut self, cmp_reg: &str, op: BinaryOp, out_ty: Type) -> Val {
        let pred = match op {
            BinaryOp::Eq => "eq",
            BinaryOp::NotEq => "ne",
            BinaryOp::Lt => "slt",
            BinaryOp::LtEq => "sle",
            BinaryOp::Gt => "sgt",
            BinaryOp::GtEq => "sge",
            _ => "eq",
        };
        let r = self.fresh_reg();
        self.line(&format!("  {r} = icmp {pred} i32 {cmp_reg}, 0"));
        Val {
            reg: r,
            dec_ty: out_ty,
        }
    }

    fn emit_scalar_cmp(
        &mut self,
        ir_ty: &str,
        op: BinaryOp,
        a: &str,
        b: &str,
        signed: bool,
    ) -> Val {
        let pred = match (op, signed) {
            (BinaryOp::Eq, _) => "eq",
            (BinaryOp::NotEq, _) => "ne",
            (BinaryOp::Lt, true) => "slt",
            (BinaryOp::Lt, false) => "ult",
            (BinaryOp::LtEq, true) => "sle",
            (BinaryOp::LtEq, false) => "ule",
            (BinaryOp::Gt, true) => "sgt",
            (BinaryOp::Gt, false) => "ugt",
            (BinaryOp::GtEq, true) => "sge",
            (BinaryOp::GtEq, false) => "uge",
            _ => "eq",
        };
        let r = self.fresh_reg();
        self.line(&format!("  {r} = icmp {pred} {ir_ty} {a}, {b}"));
        Val {
            reg: r,
            dec_ty: Type::Bool,
        }
    }

    // ==================== Calls & Builtins ====================

    fn emit_call(
        &mut self,
        cg: &mut Codegen<'_>,
        callee: &Expr,
        args: &[CallArg],
        line: usize,
    ) -> DecResult<Val> {
        let mut eval_args: Vec<(Option<String>, Val)> = Vec::with_capacity(args.len());
        for a in args {
            eval_args.push((a.name.clone(), self.emit_expr(cg, &a.value)?));
        }

        // 1. Enum payload constructor `Enum::Variant(args...)`
        if let ExprKind::Path(ref segs) = callee.kind {
            if let Some((em, vidx)) = cg.resolve_enum_variant(&self.mod_key, segs) {
                let em_mod = em.module.clone();
                let em_name = em.name.clone();
                let (vname, vtag, _) = em.variants[vidx].clone();
                let mh = self.emit_str_handle(cg, &em_mod);
                let nh = self.emit_str_handle(cg, &em_name);
                let vh = self.emit_str_handle(cg, &vname);
                let phs: Vec<String> = eval_args
                    .iter()
                    .map(|(_, v)| self.box_val_to_handle(v))
                    .collect();
                let n = phs.len();
                let r = if n == 0 {
                    let r = self.fresh_reg();
                    self.line(&format!(
                        "  {r} = call i64 @aod_rt_enum_new(ptr %ctx, i64 {mh}, i64 {nh}, i64 {vh}, i32 {vtag}, ptr null, i64 0)"
                    ));
                    r
                } else {
                    let arr = self.alloc_temp(&format!("[{n} x i64]"));
                    for (i, ph) in phs.iter().enumerate() {
                        let p = self.fresh_reg();
                        self.line(&format!(
                            "  {p} = getelementptr inbounds [{n} x i64], ptr {arr}, i64 0, i64 {i}"
                        ));
                        self.line(&format!("  store i64 {ph}, ptr {p}"));
                    }
                    let r = self.fresh_reg();
                    self.line(&format!(
                        "  {r} = call i64 @aod_rt_enum_new(ptr %ctx, i64 {mh}, i64 {nh}, i64 {vh}, i32 {vtag}, ptr {arr}, i64 {n})"
                    ));
                    r
                };
                self.emit_fail_return_check();
                return Ok(Val {
                    reg: r,
                    dec_ty: Type::Enum {
                        module: em_mod,
                        name: em_name,
                    },
                });
            }
        }

        // 2. Struct positional/named constructor `Foo(...)`
        let sp_opt: Option<Vec<String>> = match &callee.kind {
            ExprKind::Ident(n) => Some(vec![n.clone()]),
            ExprKind::Path(segs) => Some(segs.clone()),
            _ => None,
        };
        if let Some(ref sp) = sp_opt {
            if let Some(sm) = cg.resolve_struct(&self.mod_key, sp).cloned() {
                let mut pairs = Vec::with_capacity(sm.fields.len());
                let mut pos_idx = 0usize;
                for (n_opt, v) in eval_args {
                    match n_opt {
                        None => {
                            pairs.push((sm.fields[pos_idx].0.clone(), v));
                            pos_idx += 1;
                        }
                        Some(n) => pairs.push((n, v)),
                    }
                }
                return self.emit_struct_construct(cg, &sm, &pairs);
            }
        }

        // 3. Resolve user function vs builtin
        let base_mod = cg.base_module_of(&self.mod_key).to_string();
        match &callee.kind {
            ExprKind::Ident(name) => {
                if let Some(ovs) = cg
                    .fn_overloads
                    .get(&(self.mod_key.clone(), name.clone()))
                    .cloned()
                {
                    return self.emit_user_call(&ovs, eval_args, line);
                }
                if base_mod != self.mod_key {
                    if let Some(ovs) = cg.fn_overloads.get(&(base_mod, name.clone())).cloned() {
                        return self.emit_user_call(&ovs, eval_args, line);
                    }
                }
                if let Some(sym) = cg.imports_for(&self.mod_key).get(name).cloned() {
                    match sym {
                        ImportedSymbol::ModuleItem { module, item } => {
                            if let Some(ovs) = cg.fn_overloads.get(&(module, item)).cloned() {
                                return self.emit_user_call(&ovs, eval_args, line);
                            }
                        }
                        ImportedSymbol::Builtin(canon) => {
                            let pos: Vec<Val> = eval_args.into_iter().map(|(_, v)| v).collect();
                            return self.emit_builtin(&canon, pos, line);
                        }
                        ImportedSymbol::ModuleNamespace(_) => {}
                    }
                }
                let pos: Vec<Val> = eval_args.into_iter().map(|(_, v)| v).collect();
                self.emit_builtin(name, pos, line)
            }
            ExprKind::Path(segs) => {
                if segs.len() == 3 && segs[0] == "aod" {
                    let ns = &segs[0..2];
                    if builtin_namespace_items(ns).is_some() {
                        let canon = canonical_builtin_name(ns, &segs[2]);
                        let pos: Vec<Val> = eval_args.into_iter().map(|(_, v)| v).collect();
                        return self.emit_builtin(&canon, pos, line);
                    }
                } else if segs.len() == 2 {
                    if let Some(qsub) = cg.resolve_qext_submodule(&self.mod_key, &segs[0]) {
                        if let Some(ovs) = cg.fn_overloads.get(&(qsub, segs[1].clone())).cloned() {
                            return self.emit_user_call(&ovs, eval_args, line);
                        }
                    }
                    if let Some(ImportedSymbol::ModuleNamespace(m)) =
                        cg.imports_for(&self.mod_key).get(&segs[0]).cloned()
                    {
                        if let Some(rest) = m.strip_prefix("aod::") {
                            let ns_vec = vec!["aod".to_string(), rest.to_string()];
                            let canon = canonical_builtin_name(&ns_vec, &segs[1]);
                            let pos: Vec<Val> = eval_args.into_iter().map(|(_, v)| v).collect();
                            return self.emit_builtin(&canon, pos, line);
                        }
                        if let Some(ovs) = cg.fn_overloads.get(&(m, segs[1].clone())).cloned() {
                            return self.emit_user_call(&ovs, eval_args, line);
                        }
                    }
                } else if segs.len() == 3 {
                    if let Some(qsub) =
                        cg.resolve_qext_submodule_3seg(&self.mod_key, &segs[0], &segs[1])
                    {
                        if let Some(ovs) = cg.fn_overloads.get(&(qsub, segs[2].clone())).cloned() {
                            return self.emit_user_call(&ovs, eval_args, line);
                        }
                    }
                }
                Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("codegen: undefined function path {}", segs.join("::")),
                ))
            }
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "codegen: invalid call target",
            )),
        }
    }

    fn emit_user_call(
        &mut self,
        overloads: &[FnOverload],
        args: Vec<(Option<String>, Val)>,
        line: usize,
    ) -> DecResult<Val> {
        let ov = if overloads.len() == 1 {
            &overloads[0]
        } else {
            let arg_tys: Vec<Type> = args
                .iter()
                .map(|(_, v)| v.dec_ty.unwrap_read_only().clone())
                .collect();
            overloads
                .iter()
                .find(|o| {
                    o.param_types.len() == arg_tys.len()
                        && o.param_types.iter().zip(&arg_tys).all(|(p, a)| {
                            p.unwrap_read_only() == a
                                || matches!((a, p.unwrap_read_only()), (Type::List(ax), Type::List(_)) if **ax == Type::Unknown)
                                || matches!((a, p.unwrap_read_only()), (Type::Map(ak, av), Type::Map(_, _)) if **ak == Type::Unknown && **av == Type::Unknown)
                        })
                })
                .ok_or_else(|| {
                    Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        "codegen: no matching overload",
                    )
                })?
        };

        let ordered: Vec<Val> = if ov.is_single {
            let mut slots: Vec<Option<Val>> = vec![None; ov.single_params.len()];
            let mut pi = 0usize;
            for (n_opt, v) in args {
                match n_opt {
                    None => {
                        slots[pi] = Some(v);
                        pi += 1;
                    }
                    Some(n) => {
                        let idx = ov.single_params.iter().position(|p| p == &n).unwrap();
                        slots[idx] = Some(v);
                    }
                }
            }
            slots.into_iter().map(Option::unwrap).collect()
        } else {
            args.into_iter().map(|(_, v)| v).collect()
        };

        let mut call_args = String::from("ptr %ctx");
        for (v, pt) in ordered.iter().zip(&ov.param_types) {
            let lt = LlvmTy::from_dec(pt);
            let _ = write!(call_args, ", {} {}", lt.ir(), v.reg);
        }
        let ret_lt = LlvmTy::from_dec(&ov.ret_type);
        let reg = if ret_lt == LlvmTy::Void {
            self.line(&format!("  call void {}({call_args})", ov.llvm_name));
            String::new()
        } else {
            let r = self.fresh_reg();
            self.line(&format!(
                "  {r} = call {} {}({call_args})",
                ret_lt.ir(),
                ov.llvm_name
            ));
            r
        };
        self.emit_fail_return_check();
        Ok(Val {
            reg,
            dec_ty: ov.ret_type.clone(),
        })
    }

    fn emit_builtin(&mut self, name: &str, args: Vec<Val>, line: usize) -> DecResult<Val> {
        match (name, args.as_slice()) {
            ("t1", [a]) => {
                let p = self.store_temp_int(&a.reg);
                let r = self.fresh_reg();
                self.line(&format!("  {r} = call i8 @aod_rt_int_to_t1(ptr %ctx, ptr {p})"));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::T1,
                })
            }
            ("t32", [a]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @__aod_int_to_t32(ptr %ctx, %DecInt {})",
                    a.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::T32,
                })
            }
            ("q4", [a]) => {
                let p = self.store_temp_int(&a.reg);
                let r = self.fresh_reg();
                self.line(&format!("  {r} = call i8 @aod_rt_int_to_q4(ptr %ctx, ptr {p})"));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Q4,
                })
            }
            ("int", [a]) => match a.dec_ty.unwrap_read_only() {
                Type::Int => Ok(a.clone()),
                Type::T1 => {
                    let ext = self.fresh_reg();
                    let r = self.fresh_reg();
                    self.line(&format!("  {ext} = sext i8 {} to i64", a.reg));
                    self.line(&format!(
                        "  {r} = insertvalue %DecInt {{ i8 0, i64 0 }}, i64 {ext}, 1"
                    ));
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::Int,
                    })
                }
                Type::T32 => {
                    let r = self.fresh_reg();
                    self.line(&format!(
                        "  {r} = insertvalue %DecInt {{ i8 0, i64 0 }}, i64 {}, 1",
                        a.reg
                    ));
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::Int,
                    })
                }
                Type::Q4 => {
                    let ext = self.fresh_reg();
                    let r = self.fresh_reg();
                    self.line(&format!("  {ext} = zext i8 {} to i64", a.reg));
                    self.line(&format!(
                        "  {r} = insertvalue %DecInt {{ i8 0, i64 0 }}, i64 {ext}, 1"
                    ));
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::Int,
                    })
                }
                Type::Ratio => {
                    let rp = self.store_temp_ratio(&a.reg);
                    let out = self.alloc_temp("%DecInt");
                    self.line(&format!(
                        "  call void @aod_rt_ratio_to_int_exact(ptr %ctx, ptr {rp}, ptr {out})"
                    ));
                    self.emit_fail_return_check();
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = load %DecInt, ptr {out}"));
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::Int,
                    })
                }
                _ => Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "codegen: unsupported int(...) conversion",
                )),
            },
            ("ratio", [a]) => {
                let one = self.fresh_reg();
                self.line(&format!(
                    "  {one} = insertvalue %DecInt {{ i8 0, i64 0 }}, i64 1, 1"
                ));
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call %DecRatio @__aod_int_exact_div(ptr %ctx, %DecInt {}, %DecInt {one})",
                    a.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Ratio,
                })
            }
            ("ratio", [a, b]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call %DecRatio @__aod_int_exact_div(ptr %ctx, %DecInt {}, %DecInt {})",
                    a.reg, b.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Ratio,
                })
            }
            ("pack", [a]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call %DecInt @__aod_t32_pack(ptr %ctx, i64 {})",
                    a.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Int,
                })
            }
            ("unpack", [a]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @__aod_t32_unpack(ptr %ctx, %DecInt {})",
                    a.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::T32,
                })
            }
            ("neg", [a]) => self.emit_unary(UnaryOp::Neg, a.clone()),
            ("boundary_of", [a]) => self.emit_unary(UnaryOp::Boundary, a.clone()),
            ("len", [a]) => {
                let out = self.alloc_temp("%DecInt");
                self.line(&format!(
                    "  call void @aod_rt_len(ptr %ctx, i64 {}, ptr {out})",
                    a.reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecInt, ptr {out}"));
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Int,
                })
            }
            ("contains", [m, k]) => {
                let kh = self.box_val_to_handle(k);
                let b8 = self.fresh_reg();
                self.line(&format!(
                    "  {b8} = call i8 @aod_rt_map_contains(ptr %ctx, i64 {}, i64 {kh})",
                    m.reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = icmp ne i8 {b8}, 0"));
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Bool,
                })
            }
            ("trace", [sc, lin, q]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_trace_new(ptr %ctx, i64 {}, i64 {}, i8 {})",
                    sc.reg, lin.reg, q.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Trace,
                })
            }
            ("step", [t, q, o]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_trace_step(ptr %ctx, i64 {}, i8 {}, i8 {})",
                    t.reg, q.reg, o.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Trace,
                })
            }
            ("hinge", [t]) => self.emit_handle1("aod_rt_trace_hinge", &t.reg, Type::Trace),
            ("dwell", [t]) => self.emit_handle1("aod_rt_trace_dwell", &t.reg, Type::Trace),
            ("close", [t]) => self.emit_handle1("aod_rt_trace_close", &t.reg, Type::Trace),
            ("phases", [t]) => self.emit_handle_to_int("aod_rt_trace_phases", &t.reg),
            ("beats", [t]) => self.emit_handle_to_int("aod_rt_trace_beats", &t.reg),
            ("rate", [t]) => self.emit_handle_to_ratio("aod_rt_trace_rate", &t.reg),
            ("fold", [a, b, sc, lin]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_fold(ptr %ctx, i64 {}, i64 {}, i64 {}, i64 {})",
                    a.reg, b.reg, sc.reg, lin.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Fusion,
                })
            }
            ("outer", [f, q]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_outer(ptr %ctx, i64 {}, i8 {})",
                    f.reg, q.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Trace,
                })
            }
            ("fuse", [f, t]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_fuse(ptr %ctx, i64 {}, i64 {})",
                    f.reg, t.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Trace,
                })
            }
            ("coupling", [a, b, w]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_coupling_new(ptr %ctx, i64 {}, i64 {}, i64 {})",
                    a.reg, b.reg, w.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Coupling,
                })
            }
            ("contact", [c, a, b]) => {
                let ap = self.store_temp_int(&a.reg);
                let bp = self.store_temp_int(&b.reg);
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_coupling_contact(ptr %ctx, i64 {}, ptr {ap}, ptr {bp})",
                    c.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Coupling,
                })
            }
            ("cadence", [c]) => self.emit_handle_to_ratio("aod_rt_coupling_cadence", &c.reg),
            ("lag", [c]) => self.emit_handle_to_int("aod_rt_coupling_lag", &c.reg),
            ("drift", [c]) => self.emit_handle_to_int("aod_rt_coupling_drift", &c.reg),
            ("probability", [neg, h, pos, role]) => {
                let np = self.store_temp_int(&neg.reg);
                let hp = self.store_temp_int(&h.reg);
                let pp = self.store_temp_int(&pos.reg);
                let out = self.alloc_temp("%DecRatio");
                self.line(&format!(
                    "  call void @aod_rt_probability(ptr %ctx, ptr {np}, ptr {hp}, ptr {pp}, i8 {}, ptr {out})",
                    role.reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecRatio, ptr {out}"));
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Ratio,
                })
            }
            ("addr8", [s]) => self.emit_handle1("aod_rt_addr8_from_str", &s.reg, Type::Addr8),
            ("support", [s]) => self.emit_handle1("aod_rt_support_from_str", &s.reg, Type::Support),
            ("support", [p, q, l, yro]) => {
                let pp = self.store_temp_int(&p.reg);
                let qp = self.store_temp_int(&q.reg);
                let lp = self.store_temp_int(&l.reg);
                let y8 = self.fresh_reg();
                self.line(&format!("  {y8} = zext i1 {} to i8", yro.reg));
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_support_new(ptr %ctx, ptr {pp}, ptr {qp}, ptr {lp}, i8 {y8})"
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Support,
                })
            }
            ("support_pi", [sf]) => self.emit_handle_to_int("aod_rt_support_pi", &sf.reg),
            ("support_rd", [sf, h]) => {
                let hp = self.store_temp_int(&h.reg);
                let out = self.alloc_temp("%DecInt");
                self.line(&format!(
                    "  call void @aod_rt_support_rd(ptr %ctx, i64 {}, ptr {hp}, ptr {out})",
                    sf.reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecInt, ptr {out}"));
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Int,
                })
            }
            ("support_pressure", [sf, c3, h, w]) => {
                let c3p = self.store_temp_int(&c3.reg);
                let hp = self.store_temp_int(&h.reg);
                let wp = self.store_temp_int(&w.reg);
                let out = self.alloc_temp("%DecInt");
                self.line(&format!(
                    "  call void @aod_rt_support_pressure(ptr %ctx, i64 {}, ptr {c3p}, ptr {hp}, ptr {wp}, ptr {out})",
                    sf.reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecInt, ptr {out}"));
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Int,
                })
            }
            ("support_overhang", [sf, c3, h, w]) => {
                let c3p = self.store_temp_int(&c3.reg);
                let hp = self.store_temp_int(&h.reg);
                let wp = self.store_temp_int(&w.reg);
                let out = self.alloc_temp("%DecInt");
                self.line(&format!(
                    "  call void @aod_rt_support_overhang(ptr %ctx, i64 {}, ptr {c3p}, ptr {hp}, ptr {wp}, ptr {out})",
                    sf.reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecInt, ptr {out}"));
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Int,
                })
            }
            ("shedding_excess", [pd, cc]) => {
                let pdp = self.store_temp_int(&pd.reg);
                let ccp = self.store_temp_int(&cc.reg);
                let out = self.alloc_temp("%DecInt");
                self.line(&format!(
                    "  call void @aod_rt_shedding_excess(ptr %ctx, ptr {pdp}, ptr {ccp}, ptr {out})"
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecInt, ptr {out}"));
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Int,
                })
            }
            ("sadar_promote", [c3, rho, att]) => {
                let c3p = self.store_temp_int(&c3.reg);
                let rhop = self.store_temp_int(&rho.reg);
                let attp = self.store_temp_ratio(&att.reg);
                let out = self.alloc_temp("%DecRatio");
                self.line(&format!(
                    "  call void @aod_rt_sadar_promote(ptr %ctx, ptr {c3p}, ptr {rhop}, ptr {attp}, ptr {out})"
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecRatio, ptr {out}"));
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Ratio,
                })
            }
            ("window_dedup_count", [lin, sc, cuts, wins]) => {
                let out = self.alloc_temp("%DecInt");
                self.line(&format!(
                    "  call void @aod_rt_window_dedup_count(ptr %ctx, i64 {}, i64 {}, i64 {}, i64 {}, ptr {out})",
                    lin.reg, sc.reg, cuts.reg, wins.reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecInt, ptr {out}"));
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Int,
                })
            }
            ("native_cadence_ratio", [ida, qa, idr, qr, coupled, bc]) => {
                let qap = self.store_temp_int(&qa.reg);
                let qrp = self.store_temp_int(&qr.reg);
                let bcp = self.store_temp_int(&bc.reg);
                let c8 = self.fresh_reg();
                self.line(&format!("  {c8} = zext i1 {} to i8", coupled.reg));
                let out = self.alloc_temp("%DecRatio");
                self.line(&format!(
                    "  call void @aod_rt_native_cadence_ratio(ptr %ctx, i64 {}, ptr {qap}, i64 {}, ptr {qrp}, i8 {c8}, ptr {bcp}, ptr {out})",
                    ida.reg, idr.reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecRatio, ptr {out}"));
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Ratio,
                })
            }
            ("single_contact_association", [wid, pid, rid, win, bc]) => {
                let bcp = self.store_temp_int(&bc.reg);
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_single_contact_assoc(ptr %ctx, i64 {}, i64 {}, i64 {}, i64 {}, ptr {bcp})",
                    wid.reg, pid.reg, rid.reg, win.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::CadenceAssociation,
                })
            }
            ("recipient_eval_demo", [ek, sk, win]) => {
                let ekp = self.store_temp_int(&ek.reg);
                let skp = self.store_temp_int(&sk.reg);
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_recipient_eval_demo(ptr %ctx, ptr {ekp}, ptr {skp}, i64 {})",
                    win.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::RecipientOutcome,
                })
            }
            ("aod::qext::ext_decimal", [s]) => self.emit_handle1(
                "aod_rt_qext_ext_decimal",
                &s.reg,
                Type::QExt(Box::new(Type::Ratio)),
            ),
            ("aod::qext::qext_from_ratio", [rat]) => {
                let rp = self.store_temp_ratio(&rat.reg);
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_qext_from_ratio(ptr %ctx, ptr {rp})"
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::QExt(Box::new(Type::Ratio)),
                })
            }
            ("aod::qext::qext_add", [a, b]) => self.emit_qext_binop(0, &a.reg, &b.reg),
            ("aod::qext::qext_sub", [a, b]) => self.emit_qext_binop(1, &a.reg, &b.reg),
            ("aod::qext::qext_mul", [a, b]) => self.emit_qext_binop(2, &a.reg, &b.reg),
            ("aod::qext::qext_div", [a, b]) => self.emit_qext_binop(3, &a.reg, &b.reg),
            ("aod::qext::qext_render", [q]) => self.emit_handle1(
                "aod_rt_qext_render",
                &q.reg,
                Type::QExt(Box::new(Type::Str)),
            ),
            ("aod::qext::qext_round_half_even", [q, dec]) => {
                let dp = self.store_temp_int(&dec.reg);
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_qext_round_half_even(ptr %ctx, i64 {}, ptr {dp})",
                    q.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::QExt(Box::new(Type::Ratio)),
                })
            }
            ("aod::qext::qext_format_fixed", [q, dec]) => {
                let dp = self.store_temp_int(&dec.reg);
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_qext_format_fixed(ptr %ctx, i64 {}, ptr {dp})",
                    q.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::QExt(Box::new(Type::Str)),
                })
            }
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!("codegen: unlisted builtin {name}"),
            )),
        }
    }

    fn emit_handle1(&mut self, func: &str, h: &str, out_ty: Type) -> DecResult<Val> {
        let r = self.fresh_reg();
        self.line(&format!("  {r} = call i64 @{func}(ptr %ctx, i64 {h})"));
        self.emit_fail_return_check();
        Ok(Val {
            reg: r,
            dec_ty: out_ty,
        })
    }

    fn emit_handle_to_int(&mut self, func: &str, h: &str) -> DecResult<Val> {
        let out = self.alloc_temp("%DecInt");
        self.line(&format!("  call void @{func}(ptr %ctx, i64 {h}, ptr {out})"));
        self.emit_fail_return_check();
        let r = self.fresh_reg();
        self.line(&format!("  {r} = load %DecInt, ptr {out}"));
        Ok(Val {
            reg: r,
            dec_ty: Type::Int,
        })
    }

    fn emit_handle_to_ratio(&mut self, func: &str, h: &str) -> DecResult<Val> {
        let out = self.alloc_temp("%DecRatio");
        self.line(&format!("  call void @{func}(ptr %ctx, i64 {h}, ptr {out})"));
        self.emit_fail_return_check();
        let r = self.fresh_reg();
        self.line(&format!("  {r} = load %DecRatio, ptr {out}"));
        Ok(Val {
            reg: r,
            dec_ty: Type::Ratio,
        })
    }

    fn emit_qext_binop(&mut self, code: u8, a: &str, b: &str) -> DecResult<Val> {
        let r = self.fresh_reg();
        self.line(&format!(
            "  {r} = call i64 @aod_rt_qext_binop(ptr %ctx, i8 {code}, i64 {a}, i64 {b})"
        ));
        self.emit_fail_return_check();
        Ok(Val {
            reg: r,
            dec_ty: Type::QExt(Box::new(Type::Ratio)),
        })
    }

    fn emit_simulate(
        &mut self,
        cg: &mut Codegen<'_>,
        prepare_name: &str,
        ticks_expr: &Expr,
        observers: &[String],
        line: usize,
    ) -> DecResult<Val> {
        let ticks_val = self.emit_expr(cg, ticks_expr)?;
        let prep = cg
            .prepares
            .get(&(self.mod_key.clone(), prepare_name.to_string()))
            .cloned()
            .ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("codegen: undefined prepare {prepare_name}"),
                )
            })?;
        let chamber = cg
            .chambers
            .get(&(self.mod_key.clone(), prep.chamber.clone()))
            .cloned()
            .ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("codegen: undefined chamber {}", prep.chamber),
                )
            })?;

        let space_v = self.emit_expr(cg, &chamber.space)?;
        let cname_h = self.emit_str_handle(cg, &chamber.name);
        let b_h = self.fresh_reg();
        self.line(&format!(
            "  {b_h} = call i64 @aod_rt_sim_builder_new(ptr %ctx, i64 {cname_h}, i64 {}, i8 {})",
            space_v.reg,
            chamber.walls.to_code()
        ));
        self.emit_fail_return_check();

        // Prepare lets, clocks, injections inside fresh scope
        self.push_scope();
        self.emit_stmts(cg, &prep.lets)?;
        for clk in &prep.clocks {
            let pv = self.emit_expr(cg, &clk.period)?;
            let pp = self.store_temp_int(&pv.reg);
            let nh = self.emit_str_handle(cg, &clk.name);
            self.line(&format!(
                "  call void @aod_rt_sim_builder_add_clock(ptr %ctx, i64 {b_h}, i64 {nh}, ptr {pp})"
            ));
            self.emit_fail_return_check();
        }
        for inj in &prep.injections {
            let sf = self.emit_expr(cg, &inj.support)?;
            let pos = self.emit_expr(cg, &inj.pos)?;
            let xh = self.fresh_reg();
            let yh = self.fresh_reg();
            let zh = self.fresh_reg();
            self.line(&format!(
                "  {xh} = call i64 @aod_rt_tuple_get(ptr %ctx, i64 {}, i64 0)",
                pos.reg
            ));
            self.line(&format!(
                "  {yh} = call i64 @aod_rt_tuple_get(ptr %ctx, i64 {}, i64 1)",
                pos.reg
            ));
            self.line(&format!(
                "  {zh} = call i64 @aod_rt_tuple_get(ptr %ctx, i64 {}, i64 2)",
                pos.reg
            ));
            let xv = self.unbox_handle_to_val(&xh, &Type::Int);
            let yv = self.unbox_handle_to_val(&yh, &Type::Int);
            let zv = self.unbox_handle_to_val(&zh, &Type::Int);
            let xp = self.store_temp_int(&xv.reg);
            let yp = self.store_temp_int(&yv.reg);
            let zp = self.store_temp_int(&zv.reg);

            let ov = self.emit_expr(cg, &inj.orient)?;
            let orient_i8 = if ov.dec_ty == Type::T1 {
                ov.reg
            } else {
                let op = self.store_temp_int(&ov.reg);
                let r = self.fresh_reg();
                self.line(&format!("  {r} = call i8 @aod_rt_int_to_t1(ptr %ctx, ptr {op})"));
                self.emit_fail_return_check();
                r
            };
            let shv = self.emit_expr(cg, &inj.shells)?;
            let shp = self.store_temp_int(&shv.reg);
            self.line(&format!(
                "  call void @aod_rt_sim_builder_add_carrier(ptr %ctx, i64 {b_h}, i64 {}, ptr {xp}, ptr {yp}, ptr {zp}, i8 {}, i8 {orient_i8}, ptr {shp})",
                sf.reg,
                inj.dir.to_code()
            ));
            self.emit_fail_return_check();
        }
        self.pop_scope();

        // Register attached observers
        for obs_name in observers {
            let obs = cg
                .observes
                .get(&(self.mod_key.clone(), obs_name.clone()))
                .cloned()
                .ok_or_else(|| {
                    Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("codegen: undefined observe {obs_name}"),
                    )
                })?;
            for probe in &obs.probes {
                match probe {
                    ProbeDecl::Declarative {
                        kind,
                        name,
                        region,
                        ..
                    } => {
                        let rv = self.emit_expr(cg, region)?;
                        let nh = self.emit_str_handle(cg, name);
                        self.line(&format!(
                            "  call void @aod_rt_sim_builder_add_decl_obs(ptr %ctx, i64 {b_h}, i8 {}, i64 {nh}, i64 {})",
                            kind.to_code(),
                            rv.reg
                        ));
                        self.emit_fail_return_check();
                    }
                    ProbeDecl::Procedural { name, .. } => {
                        let cb_name = cg.obs_callbacks
                            [&(self.mod_key.clone(), obs.name.clone(), name.clone())]
                            .clone();
                        let nh = self.emit_str_handle(cg, name);
                        self.line(&format!(
                            "  call void @aod_rt_sim_builder_add_proc_obs(ptr %ctx, i64 {b_h}, i64 {nh}, ptr {cb_name})"
                        ));
                        self.emit_fail_return_check();
                    }
                }
            }
        }

        let tp = self.store_temp_int(&ticks_val.reg);
        let sim_h = self.fresh_reg();
        self.line(&format!(
            "  {sim_h} = call i64 @aod_rt_sim_run(ptr %ctx, i64 {b_h}, ptr {tp})"
        ));
        self.emit_fail_return_check();
        Ok(Val {
            reg: sim_h,
            dec_ty: Type::SimResult,
        })
    }
}

fn sanitize_ident(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn escape_llvm_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        if (0x20..=0x7E).contains(&b) && b != b'"' && b != b'\\' {
            out.push(b as char);
        } else {
            let _ = write!(out, "\\{:02X}", b);
        }
    }
    out
}

/// LLVM 18 module preamble: exact C ABI types, LLVM overflow/cttz/ctpop intrinsics,
/// `aod_rt_*` declarations, and unboxed-first inline arithmetic helpers (`P0-1`, `P0-3`).
const LLVM_MODULE_PREAMBLE: &str = r#"; ModuleID = 'aod_dec_v2'
%AodRtContext = type { i64, i32, i32, i8, i8, [6 x i8], ptr }
%DecInt = type { i8, i64 }
%DecRatio = type { i8, i64, i64 }

declare { i64, i1 } @llvm.sadd.with.overflow.i64(i64, i64)
declare { i64, i1 } @llvm.ssub.with.overflow.i64(i64, i64)
declare { i64, i1 } @llvm.smul.with.overflow.i64(i64, i64)
declare { i128, i1 } @llvm.sadd.with.overflow.i128(i128, i128)
declare { i128, i1 } @llvm.ssub.with.overflow.i128(i128, i128)
declare i128 @llvm.cttz.i128(i128, i1 immarg)

declare i32 @aod_rt_cli_parse_args(i32, ptr, ptr)
declare i32 @aod_rt_ctx_finish(ptr)
declare void @aod_rt_fail(ptr, ptr, i64)
declare void @aod_rt_assert_failed(ptr, i64)
declare void @aod_rt_budget_exhausted(ptr, i8)

declare void @aod_rt_int_from_str(ptr, ptr, i64, ptr)
declare void @aod_rt_int_from_u64(ptr, i64, ptr)
declare void @aod_rt_int_neg(ptr, ptr, ptr)
declare void @aod_rt_int_add(ptr, ptr, ptr, ptr)
declare void @aod_rt_int_sub(ptr, ptr, ptr, ptr)
declare void @aod_rt_int_mul(ptr, ptr, ptr, ptr)
declare void @aod_rt_int_div_floor(ptr, ptr, ptr, ptr)
declare void @aod_rt_int_rem_floor(ptr, ptr, ptr, ptr)
declare void @aod_rt_int_exact_div(ptr, ptr, ptr, ptr)
declare void @aod_rt_int_bitand(ptr, ptr, ptr, ptr)
declare void @aod_rt_int_bitor(ptr, ptr, ptr, ptr)
declare void @aod_rt_int_bitxor(ptr, ptr, ptr, ptr)
declare void @aod_rt_int_shl(ptr, ptr, ptr, ptr)
declare void @aod_rt_int_shr(ptr, ptr, ptr, ptr)
declare i32 @aod_rt_int_cmp(ptr, ptr, ptr)
declare i8 @aod_rt_int_to_t1(ptr, ptr)
declare i64 @aod_rt_int_to_t32(ptr, ptr)
declare i8 @aod_rt_int_to_q4(ptr, ptr)
declare i64 @aod_rt_t32_unpack_bigint(ptr, ptr)

declare void @aod_rt_ratio_reduce_i128(ptr, i64, i64, i64, i64, ptr)
declare void @aod_rt_ratio_from_str(ptr, ptr, i64, ptr, i64, ptr)
declare void @aod_rt_ratio_neg(ptr, ptr, ptr)
declare void @aod_rt_ratio_add(ptr, ptr, ptr, ptr)
declare void @aod_rt_ratio_sub(ptr, ptr, ptr, ptr)
declare void @aod_rt_ratio_mul(ptr, ptr, ptr, ptr)
declare void @aod_rt_ratio_div(ptr, ptr, ptr, ptr)
declare i32 @aod_rt_ratio_cmp(ptr, ptr, ptr)
declare void @aod_rt_ratio_to_int_exact(ptr, ptr, ptr)

declare void @aod_rt_emit_int(ptr, ptr)
declare void @aod_rt_emit_bool(ptr, i8)
declare void @aod_rt_emit_qext_bool(ptr, i8)
declare void @aod_rt_emit_t1(ptr, i8)
declare void @aod_rt_emit_t32(ptr, i64)
declare void @aod_rt_emit_q4(ptr, i8)
declare void @aod_rt_emit_ratio(ptr, ptr)
declare void @aod_rt_emit_handle(ptr, i64)

declare i64 @aod_rt_str_new(ptr, ptr, i64)
declare i8 @aod_rt_handle_eq(ptr, i64, i64)
declare i64 @aod_rt_val_unit(ptr)
declare i64 @aod_rt_val_from_int(ptr, ptr)
declare void @aod_rt_val_to_int(ptr, i64, ptr)
declare i64 @aod_rt_val_from_bool(ptr, i8)
declare i8 @aod_rt_val_to_bool(ptr, i64)
declare i64 @aod_rt_val_from_qext_bool(ptr, i8)
declare i8 @aod_rt_val_to_qext_bool(ptr, i64)
declare i64 @aod_rt_val_from_t1(ptr, i8)
declare i8 @aod_rt_val_to_t1(ptr, i64)
declare i64 @aod_rt_val_from_t32(ptr, i64)
declare i64 @aod_rt_val_to_t32(ptr, i64)
declare i64 @aod_rt_val_from_q4(ptr, i8)
declare i8 @aod_rt_val_to_q4(ptr, i64)
declare i64 @aod_rt_val_from_ratio(ptr, ptr)
declare void @aod_rt_val_to_ratio(ptr, i64, ptr)

declare i64 @aod_rt_addr8_parse(ptr, ptr, i64)
declare i64 @aod_rt_addr8_from_str(ptr, i64)
declare i64 @aod_rt_support_parse(ptr, ptr, i64)
declare i64 @aod_rt_support_from_str(ptr, i64)
declare i64 @aod_rt_support_new(ptr, ptr, ptr, ptr, i8)
declare void @aod_rt_support_pi(ptr, i64, ptr)
declare void @aod_rt_support_rd(ptr, i64, ptr, ptr)
declare void @aod_rt_support_pressure(ptr, i64, ptr, ptr, ptr, ptr)
declare void @aod_rt_support_overhang(ptr, i64, ptr, ptr, ptr, ptr)
declare void @aod_rt_shedding_excess(ptr, ptr, ptr, ptr)
declare void @aod_rt_sadar_promote(ptr, ptr, ptr, ptr, ptr)
declare void @aod_rt_probability(ptr, ptr, ptr, ptr, i8, ptr)

declare i64 @aod_rt_window_new(ptr, ptr, ptr)
declare i64 @aod_rt_range_new(ptr, ptr, ptr)
declare void @aod_rt_range_bounds(ptr, i64, ptr, ptr)
declare i64 @aod_rt_box_new(ptr, i64, i64, i64)
declare i64 @aod_rt_boundary_of(ptr, i64)

declare i64 @aod_rt_trace_new(ptr, i64, i64, i8)
declare i64 @aod_rt_trace_step(ptr, i64, i8, i8)
declare i64 @aod_rt_trace_hinge(ptr, i64)
declare i64 @aod_rt_trace_dwell(ptr, i64)
declare i64 @aod_rt_trace_close(ptr, i64)
declare void @aod_rt_trace_phases(ptr, i64, ptr)
declare void @aod_rt_trace_beats(ptr, i64, ptr)
declare void @aod_rt_trace_rate(ptr, i64, ptr)
declare i64 @aod_rt_fold(ptr, i64, i64, i64, i64)
declare i64 @aod_rt_outer(ptr, i64, i8)
declare i64 @aod_rt_fuse(ptr, i64, i64)
declare i64 @aod_rt_coupling_new(ptr, i64, i64, i64)
declare i64 @aod_rt_coupling_contact(ptr, i64, ptr, ptr)
declare void @aod_rt_coupling_cadence(ptr, i64, ptr)
declare void @aod_rt_coupling_lag(ptr, i64, ptr)
declare void @aod_rt_coupling_drift(ptr, i64, ptr)
declare void @aod_rt_window_dedup_count(ptr, i64, i64, i64, i64, ptr)
declare void @aod_rt_native_cadence_ratio(ptr, i64, ptr, i64, ptr, i8, ptr, ptr)
declare i64 @aod_rt_single_contact_assoc(ptr, i64, i64, i64, i64, ptr)
declare i64 @aod_rt_recipient_eval_demo(ptr, ptr, ptr, i64)

declare i64 @aod_rt_qext_ext_decimal(ptr, i64)
declare i64 @aod_rt_qext_from_ratio(ptr, ptr)
declare i64 @aod_rt_qext_binop(ptr, i8, i64, i64)
declare i64 @aod_rt_qext_neg(ptr, i64)
declare i32 @aod_rt_qext_cmp(ptr, i64, i64)
declare i64 @aod_rt_qext_render(ptr, i64)
declare i64 @aod_rt_qext_round_half_even(ptr, i64, ptr)
declare i64 @aod_rt_qext_format_fixed(ptr, i64, ptr)

declare i64 @aod_rt_list_new(ptr, ptr, i64)
declare i64 @aod_rt_list_get(ptr, i64, ptr)
declare i64 @aod_rt_list_set(ptr, i64, ptr, i64)
declare i64 @aod_rt_list_len_raw(ptr, i64)
declare void @aod_rt_len(ptr, i64, ptr)
declare i64 @aod_rt_tuple_new(ptr, ptr, i64)
declare i64 @aod_rt_tuple_get(ptr, i64, i64)
declare i64 @aod_rt_map_new(ptr, ptr, ptr, i64)
declare i64 @aod_rt_map_get(ptr, i64, i64)
declare i64 @aod_rt_map_set(ptr, i64, i64, i64)
declare i8 @aod_rt_map_contains(ptr, i64, i64)
declare i64 @aod_rt_map_keys_list(ptr, i64)
declare i64 @aod_rt_map_entries_list(ptr, i64)
declare i64 @aod_rt_struct_new(ptr, i64, i64, ptr, ptr, i64)
declare i64 @aod_rt_struct_set(ptr, i64, ptr, i64, i64)
declare i64 @aod_rt_enum_new(ptr, i64, i64, i64, i32, ptr, i64)
declare i32 @aod_rt_enum_tag(ptr, i64)
declare i64 @aod_rt_enum_payload(ptr, i64, i64)
declare i64 @aod_rt_field_get(ptr, i64, ptr, i64)

declare i64 @aod_rt_sim_builder_new(ptr, i64, i64, i8)
declare void @aod_rt_sim_builder_add_clock(ptr, i64, i64, ptr)
declare void @aod_rt_sim_builder_add_carrier(ptr, i64, i64, ptr, ptr, ptr, i8, i8, ptr)
declare void @aod_rt_sim_builder_add_decl_obs(ptr, i64, i8, i64, i64)
declare void @aod_rt_sim_builder_add_proc_obs(ptr, i64, i64, ptr)
declare i64 @aod_rt_sim_run(ptr, i64, ptr)

@.err_t1 = private unnamed_addr constant [17 x i8] c"t1 out of bounds\00", align 1
@.err_t32 = private unnamed_addr constant [18 x i8] c"t32 out of bounds\00", align 1
@.err_div0 = private unnamed_addr constant [17 x i8] c"division by zero\00", align 1
@.err_neg_pack = private unnamed_addr constant [30 x i8] c"packed t32 cannot be negative\00", align 1
@.err_bad_lane = private unnamed_addr constant [30 x i8] c"invalid balanced trit code 11\00", align 1

; ==================== Unboxed-First DecInt Helpers ====================

define internal %DecInt @__aod_int_neg(ptr %ctx, %DecInt %a) alwaysinline {
entry:
  %pa = alloca %DecInt, align 8
  %pout = alloca %DecInt, align 8
  %ta = extractvalue %DecInt %a, 0
  %va = extractvalue %DecInt %a, 1
  %is_inline = icmp eq i8 %ta, 0
  %not_min = icmp ne i64 %va, -9223372036854775808
  %fast = and i1 %is_inline, %not_min
  br i1 %fast, label %ok, label %cold
ok:
  %nv = sub nsw i64 0, %va
  %r = insertvalue %DecInt { i8 0, i64 0 }, i64 %nv, 1
  ret %DecInt %r
cold:
  store %DecInt %a, ptr %pa
  call void @aod_rt_int_neg(ptr %ctx, ptr %pa, ptr %pout)
  %res = load %DecInt, ptr %pout
  ret %DecInt %res
}

define internal %DecInt @__aod_int_add(ptr %ctx, %DecInt %a, %DecInt %b) alwaysinline {
entry:
  %pa = alloca %DecInt, align 8
  %pb = alloca %DecInt, align 8
  %pout = alloca %DecInt, align 8
  %ta = extractvalue %DecInt %a, 0
  %tb = extractvalue %DecInt %b, 0
  %or_t = or i8 %ta, %tb
  %both_inline = icmp eq i8 %or_t, 0
  br i1 %both_inline, label %fast, label %cold
fast:
  %va = extractvalue %DecInt %a, 1
  %vb = extractvalue %DecInt %b, 1
  %ov_res = call { i64, i1 } @llvm.sadd.with.overflow.i64(i64 %va, i64 %vb)
  %sum = extractvalue { i64, i1 } %ov_res, 0
  %ov = extractvalue { i64, i1 } %ov_res, 1
  br i1 %ov, label %cold, label %ok
ok:
  %r = insertvalue %DecInt { i8 0, i64 0 }, i64 %sum, 1
  ret %DecInt %r
cold:
  store %DecInt %a, ptr %pa
  store %DecInt %b, ptr %pb
  call void @aod_rt_int_add(ptr %ctx, ptr %pa, ptr %pb, ptr %pout)
  %res = load %DecInt, ptr %pout
  ret %DecInt %res
}

define internal %DecInt @__aod_int_sub(ptr %ctx, %DecInt %a, %DecInt %b) alwaysinline {
entry:
  %pa = alloca %DecInt, align 8
  %pb = alloca %DecInt, align 8
  %pout = alloca %DecInt, align 8
  %ta = extractvalue %DecInt %a, 0
  %tb = extractvalue %DecInt %b, 0
  %or_t = or i8 %ta, %tb
  %both_inline = icmp eq i8 %or_t, 0
  br i1 %both_inline, label %fast, label %cold
fast:
  %va = extractvalue %DecInt %a, 1
  %vb = extractvalue %DecInt %b, 1
  %ov_res = call { i64, i1 } @llvm.ssub.with.overflow.i64(i64 %va, i64 %vb)
  %diff = extractvalue { i64, i1 } %ov_res, 0
  %ov = extractvalue { i64, i1 } %ov_res, 1
  br i1 %ov, label %cold, label %ok
ok:
  %r = insertvalue %DecInt { i8 0, i64 0 }, i64 %diff, 1
  ret %DecInt %r
cold:
  store %DecInt %a, ptr %pa
  store %DecInt %b, ptr %pb
  call void @aod_rt_int_sub(ptr %ctx, ptr %pa, ptr %pb, ptr %pout)
  %res = load %DecInt, ptr %pout
  ret %DecInt %res
}

define internal %DecInt @__aod_int_mul(ptr %ctx, %DecInt %a, %DecInt %b) alwaysinline {
entry:
  %pa = alloca %DecInt, align 8
  %pb = alloca %DecInt, align 8
  %pout = alloca %DecInt, align 8
  %ta = extractvalue %DecInt %a, 0
  %tb = extractvalue %DecInt %b, 0
  %or_t = or i8 %ta, %tb
  %both_inline = icmp eq i8 %or_t, 0
  br i1 %both_inline, label %fast, label %cold
fast:
  %va = extractvalue %DecInt %a, 1
  %vb = extractvalue %DecInt %b, 1
  %ov_res = call { i64, i1 } @llvm.smul.with.overflow.i64(i64 %va, i64 %vb)
  %prod = extractvalue { i64, i1 } %ov_res, 0
  %ov = extractvalue { i64, i1 } %ov_res, 1
  br i1 %ov, label %cold, label %ok
ok:
  %r = insertvalue %DecInt { i8 0, i64 0 }, i64 %prod, 1
  ret %DecInt %r
cold:
  store %DecInt %a, ptr %pa
  store %DecInt %b, ptr %pb
  call void @aod_rt_int_mul(ptr %ctx, ptr %pa, ptr %pb, ptr %pout)
  %res = load %DecInt, ptr %pout
  ret %DecInt %res
}

define internal %DecInt @__aod_int_div_floor(ptr %ctx, %DecInt %a, %DecInt %b) alwaysinline {
entry:
  %pa = alloca %DecInt, align 8
  %pb = alloca %DecInt, align 8
  %pout = alloca %DecInt, align 8
  %ta = extractvalue %DecInt %a, 0
  %tb = extractvalue %DecInt %b, 0
  %or_t = or i8 %ta, %tb
  %both_inline = icmp eq i8 %or_t, 0
  br i1 %both_inline, label %chk, label %cold
chk:
  %va = extractvalue %DecInt %a, 1
  %vb = extractvalue %DecInt %b, 1
  %nz = icmp ne i64 %vb, 0
  %is_min = icmp eq i64 %va, -9223372036854775808
  %is_m1 = icmp eq i64 %vb, -1
  %min_div = and i1 %is_min, %is_m1
  %not_min_div = xor i1 %min_div, true
  %safe = and i1 %nz, %not_min_div
  br i1 %safe, label %fast, label %cold
fast:
  %q = sdiv i64 %va, %vb
  %rem = srem i64 %va, %vb
  %rem_nz = icmp ne i64 %rem, 0
  %x = xor i64 %va, %vb
  %diff_sign = icmp slt i64 %x, 0
  %adj = and i1 %rem_nz, %diff_sign
  %qm1 = sub i64 %q, 1
  %qf = select i1 %adj, i64 %qm1, i64 %q
  %r = insertvalue %DecInt { i8 0, i64 0 }, i64 %qf, 1
  ret %DecInt %r
cold:
  store %DecInt %a, ptr %pa
  store %DecInt %b, ptr %pb
  call void @aod_rt_int_div_floor(ptr %ctx, ptr %pa, ptr %pb, ptr %pout)
  %res = load %DecInt, ptr %pout
  ret %DecInt %res
}

define internal %DecInt @__aod_int_rem_floor(ptr %ctx, %DecInt %a, %DecInt %b) alwaysinline {
entry:
  %pa = alloca %DecInt, align 8
  %pb = alloca %DecInt, align 8
  %pout = alloca %DecInt, align 8
  %ta = extractvalue %DecInt %a, 0
  %tb = extractvalue %DecInt %b, 0
  %or_t = or i8 %ta, %tb
  %both_inline = icmp eq i8 %or_t, 0
  br i1 %both_inline, label %chk, label %cold
chk:
  %va = extractvalue %DecInt %a, 1
  %vb = extractvalue %DecInt %b, 1
  %nz = icmp ne i64 %vb, 0
  %is_min = icmp eq i64 %va, -9223372036854775808
  %is_m1 = icmp eq i64 %vb, -1
  %min_div = and i1 %is_min, %is_m1
  %not_min_div = xor i1 %min_div, true
  %safe = and i1 %nz, %not_min_div
  br i1 %safe, label %fast, label %cold
fast:
  %rem = srem i64 %va, %vb
  %rem_nz = icmp ne i64 %rem, 0
  %x = xor i64 %rem, %vb
  %diff_sign = icmp slt i64 %x, 0
  %adj = and i1 %rem_nz, %diff_sign
  %r_plus_b = add i64 %rem, %vb
  %rf = select i1 %adj, i64 %r_plus_b, i64 %rem
  %r = insertvalue %DecInt { i8 0, i64 0 }, i64 %rf, 1
  ret %DecInt %r
cold:
  store %DecInt %a, ptr %pa
  store %DecInt %b, ptr %pb
  call void @aod_rt_int_rem_floor(ptr %ctx, ptr %pa, ptr %pb, ptr %pout)
  %res = load %DecInt, ptr %pout
  ret %DecInt %res
}

define internal %DecInt @__aod_int_bitand(ptr %ctx, %DecInt %a, %DecInt %b) alwaysinline {
entry:
  %pa = alloca %DecInt, align 8
  %pb = alloca %DecInt, align 8
  %pout = alloca %DecInt, align 8
  %ta = extractvalue %DecInt %a, 0
  %tb = extractvalue %DecInt %b, 0
  %or_t = or i8 %ta, %tb
  %both_inline = icmp eq i8 %or_t, 0
  br i1 %both_inline, label %chk, label %cold
chk:
  %va = extractvalue %DecInt %a, 1
  %vb = extractvalue %DecInt %b, 1
  %or_v = or i64 %va, %vb
  %nonneg = icmp sge i64 %or_v, 0
  br i1 %nonneg, label %fast, label %cold
fast:
  %v = and i64 %va, %vb
  %r = insertvalue %DecInt { i8 0, i64 0 }, i64 %v, 1
  ret %DecInt %r
cold:
  store %DecInt %a, ptr %pa
  store %DecInt %b, ptr %pb
  call void @aod_rt_int_bitand(ptr %ctx, ptr %pa, ptr %pb, ptr %pout)
  %res = load %DecInt, ptr %pout
  ret %DecInt %res
}

define internal %DecInt @__aod_int_bitor(ptr %ctx, %DecInt %a, %DecInt %b) alwaysinline {
entry:
  %pa = alloca %DecInt, align 8
  %pb = alloca %DecInt, align 8
  %pout = alloca %DecInt, align 8
  %ta = extractvalue %DecInt %a, 0
  %tb = extractvalue %DecInt %b, 0
  %or_t = or i8 %ta, %tb
  %both_inline = icmp eq i8 %or_t, 0
  br i1 %both_inline, label %chk, label %cold
chk:
  %va = extractvalue %DecInt %a, 1
  %vb = extractvalue %DecInt %b, 1
  %or_v = or i64 %va, %vb
  %nonneg = icmp sge i64 %or_v, 0
  br i1 %nonneg, label %fast, label %cold
fast:
  %r = insertvalue %DecInt { i8 0, i64 0 }, i64 %or_v, 1
  ret %DecInt %r
cold:
  store %DecInt %a, ptr %pa
  store %DecInt %b, ptr %pb
  call void @aod_rt_int_bitor(ptr %ctx, ptr %pa, ptr %pb, ptr %pout)
  %res = load %DecInt, ptr %pout
  ret %DecInt %res
}

define internal %DecInt @__aod_int_bitxor(ptr %ctx, %DecInt %a, %DecInt %b) alwaysinline {
entry:
  %pa = alloca %DecInt, align 8
  %pb = alloca %DecInt, align 8
  %pout = alloca %DecInt, align 8
  %ta = extractvalue %DecInt %a, 0
  %tb = extractvalue %DecInt %b, 0
  %or_t = or i8 %ta, %tb
  %both_inline = icmp eq i8 %or_t, 0
  br i1 %both_inline, label %chk, label %cold
chk:
  %va = extractvalue %DecInt %a, 1
  %vb = extractvalue %DecInt %b, 1
  %or_v = or i64 %va, %vb
  %nonneg = icmp sge i64 %or_v, 0
  br i1 %nonneg, label %fast, label %cold
fast:
  %v = xor i64 %va, %vb
  %r = insertvalue %DecInt { i8 0, i64 0 }, i64 %v, 1
  ret %DecInt %r
cold:
  store %DecInt %a, ptr %pa
  store %DecInt %b, ptr %pb
  call void @aod_rt_int_bitxor(ptr %ctx, ptr %pa, ptr %pb, ptr %pout)
  %res = load %DecInt, ptr %pout
  ret %DecInt %res
}

define internal %DecInt @__aod_int_shl(ptr %ctx, %DecInt %a, %DecInt %b) alwaysinline {
entry:
  %pa = alloca %DecInt, align 8
  %pb = alloca %DecInt, align 8
  %pout = alloca %DecInt, align 8
  %ta = extractvalue %DecInt %a, 0
  %tb = extractvalue %DecInt %b, 0
  %or_t = or i8 %ta, %tb
  %both_inline = icmp eq i8 %or_t, 0
  br i1 %both_inline, label %chk, label %cold
chk:
  %va = extractvalue %DecInt %a, 1
  %vb = extractvalue %DecInt %b, 1
  %a_nn = icmp sge i64 %va, 0
  %b_nn = icmp sge i64 %vb, 0
  %b_le62 = icmp sle i64 %vb, 62
  %ok1 = and i1 %a_nn, %b_nn
  %ok2 = and i1 %ok1, %b_le62
  br i1 %ok2, label %chk2, label %cold
chk2:
  %lim = lshr i64 9223372036854775807, %vb
  %fits = icmp sle i64 %va, %lim
  br i1 %fits, label %fast, label %cold
fast:
  %v = shl i64 %va, %vb
  %r = insertvalue %DecInt { i8 0, i64 0 }, i64 %v, 1
  ret %DecInt %r
cold:
  store %DecInt %a, ptr %pa
  store %DecInt %b, ptr %pb
  call void @aod_rt_int_shl(ptr %ctx, ptr %pa, ptr %pb, ptr %pout)
  %res = load %DecInt, ptr %pout
  ret %DecInt %res
}

define internal %DecInt @__aod_int_shr(ptr %ctx, %DecInt %a, %DecInt %b) alwaysinline {
entry:
  %pa = alloca %DecInt, align 8
  %pb = alloca %DecInt, align 8
  %pout = alloca %DecInt, align 8
  %ta = extractvalue %DecInt %a, 0
  %tb = extractvalue %DecInt %b, 0
  %or_t = or i8 %ta, %tb
  %both_inline = icmp eq i8 %or_t, 0
  br i1 %both_inline, label %chk, label %cold
chk:
  %va = extractvalue %DecInt %a, 1
  %vb = extractvalue %DecInt %b, 1
  %a_nn = icmp sge i64 %va, 0
  %b_nn = icmp sge i64 %vb, 0
  %nn = and i1 %a_nn, %b_nn
  br i1 %nn, label %chk64, label %cold
chk64:
  %ge64 = icmp sge i64 %vb, 64
  br i1 %ge64, label %ret0, label %fast
ret0:
  ret %DecInt zeroinitializer
fast:
  %v = lshr i64 %va, %vb
  %r = insertvalue %DecInt { i8 0, i64 0 }, i64 %v, 1
  ret %DecInt %r
cold:
  store %DecInt %a, ptr %pa
  store %DecInt %b, ptr %pb
  call void @aod_rt_int_shr(ptr %ctx, ptr %pa, ptr %pb, ptr %pout)
  %res = load %DecInt, ptr %pout
  ret %DecInt %res
}

define internal i32 @__aod_int_cmp(ptr %ctx, %DecInt %a, %DecInt %b) alwaysinline {
entry:
  %pa = alloca %DecInt, align 8
  %pb = alloca %DecInt, align 8
  %ta = extractvalue %DecInt %a, 0
  %tb = extractvalue %DecInt %b, 0
  %or_t = or i8 %ta, %tb
  %both_inline = icmp eq i8 %or_t, 0
  br i1 %both_inline, label %fast, label %cold
fast:
  %va = extractvalue %DecInt %a, 1
  %vb = extractvalue %DecInt %b, 1
  %lt = icmp slt i64 %va, %vb
  %gt = icmp sgt i64 %va, %vb
  %lt32 = zext i1 %lt to i32
  %gt32 = zext i1 %gt to i32
  %diff = sub i32 %gt32, %lt32
  ret i32 %diff
cold:
  store %DecInt %a, ptr %pa
  store %DecInt %b, ptr %pb
  %res = call i32 @aod_rt_int_cmp(ptr %ctx, ptr %pa, ptr %pb)
  ret i32 %res
}

define internal i64 @__aod_int_to_t32(ptr %ctx, %DecInt %a) alwaysinline {
entry:
  %pa = alloca %DecInt, align 8
  %ta = extractvalue %DecInt %a, 0
  %is_inline = icmp eq i8 %ta, 0
  br i1 %is_inline, label %fast, label %cold
fast:
  %va = extractvalue %DecInt %a, 1
  %lo = icmp sge i64 %va, -926510094425920
  %hi = icmp sle i64 %va, 926510094425920
  %ok = and i1 %lo, %hi
  br i1 %ok, label %ret_ok, label %cold
ret_ok:
  ret i64 %va
cold:
  store %DecInt %a, ptr %pa
  %res = call i64 @aod_rt_int_to_t32(ptr %ctx, ptr %pa)
  ret i64 %res
}

; ==================== Binary Stein's GCD (ZERO urem i128 / __umodti3!) ====================

define internal i128 @__aod_gcd_u128(i128 %u0, i128 %v0) alwaysinline {
entry:
  %u_z = icmp eq i128 %u0, 0
  br i1 %u_z, label %ret_v0, label %chk_v0
ret_v0:
  ret i128 %v0
chk_v0:
  %v_z = icmp eq i128 %v0, 0
  br i1 %v_z, label %ret_u0, label %init
ret_u0:
  ret i128 %u0
init:
  %tzu = call i128 @llvm.cttz.i128(i128 %u0, i1 true)
  %tzv = call i128 @llvm.cttz.i128(i128 %v0, i1 true)
  %lt = icmp ult i128 %tzu, %tzv
  %shift = select i1 %lt, i128 %tzu, i128 %tzv
  %u1 = lshr i128 %u0, %tzu
  br label %loop
loop:
  %u = phi i128 [ %u1, %init ], [ %u_next, %loop ]
  %v = phi i128 [ %v0, %init ], [ %v_next, %loop ]
  %tzv2 = call i128 @llvm.cttz.i128(i128 %v, i1 true)
  %v1 = lshr i128 %v, %tzv2
  %swap = icmp ugt i128 %u, %v1
  %u_next = select i1 %swap, i128 %v1, i128 %u
  %v_hi = select i1 %swap, i128 %u, i128 %v1
  %v_next = sub i128 %v_hi, %u_next
  %done = icmp eq i128 %v_next, 0
  br i1 %done, label %exit, label %loop
exit:
  %res = shl i128 %u_next, %shift
  ret i128 %res
}

; ==================== Unboxed-First DecRatio Helpers (ZERO __divti3/__udivti3!) ====================

define internal %DecRatio @__aod_ratio_norm_i128(ptr %ctx, i128 %num0, i128 %den0) alwaysinline {
entry:
  %pout = alloca %DecRatio, align 8
  %den_z = icmp eq i128 %den0, 0
  br i1 %den_z, label %cold, label %chk_sign
chk_sign:
  %den_neg = icmp slt i128 %den0, 0
  %num_is_min = icmp eq i128 %num0, -170141183460469231731687303715884105728
  %den_is_min = icmp eq i128 %den0, -170141183460469231731687303715884105728
  %any_min = or i1 %num_is_min, %den_is_min
  br i1 %any_min, label %cold, label %sign_ok
sign_ok:
  %neg_n = sub i128 0, %num0
  %neg_d = sub i128 0, %den0
  %num = select i1 %den_neg, i128 %neg_n, i128 %num0
  %den = select i1 %den_neg, i128 %neg_d, i128 %den0
  %num_z = icmp eq i128 %num, 0
  br i1 %num_z, label %ret_zero, label %nonzero
ret_zero:
  ret %DecRatio { i8 0, i64 0, i64 1 }
nonzero:
  %n_neg = icmp slt i128 %num, 0
  %n_abs_neg = sub i128 0, %num
  %abs_n = select i1 %n_neg, i128 %n_abs_neg, i128 %num
  %g = call i128 @__aod_gcd_u128(i128 %abs_n, i128 %den)
  %g_is_1 = icmp eq i128 %g, 1
  br i1 %g_is_1, label %chk_i64, label %chk_pow2
chk_pow2:
  %gm1 = sub i128 %g, 1
  %and_g = and i128 %g, %gm1
  %is_pow2 = icmp eq i128 %and_g, 0
  br i1 %is_pow2, label %reduce_pow2, label %chk_div64
reduce_pow2:
  %sh = call i128 @llvm.cttz.i128(i128 %g, i1 true)
  %num_p2 = ashr i128 %num, %sh
  %den_p2 = lshr i128 %den, %sh
  br label %chk_i64
chk_div64:
  %n_fits64 = icmp ule i128 %abs_n, 9223372036854775807
  %d_fits64 = icmp ule i128 %den, 9223372036854775807
  %both64 = and i1 %n_fits64, %d_fits64
  br i1 %both64, label %reduce_div64, label %cold
reduce_div64:
  %n64 = trunc i128 %num to i64
  %d64 = trunc i128 %den to i64
  %g64 = trunc i128 %g to i64
  %rn64 = sdiv i64 %n64, %g64
  %rd64 = sdiv i64 %d64, %g64
  %r1_64 = insertvalue %DecRatio { i8 0, i64 0, i64 1 }, i64 %rn64, 1
  %r2_64 = insertvalue %DecRatio %r1_64, i64 %rd64, 2
  ret %DecRatio %r2_64
chk_i64:
  %fn = phi i128 [ %num, %nonzero ], [ %num_p2, %reduce_pow2 ]
  %fd = phi i128 [ %den, %nonzero ], [ %den_p2, %reduce_pow2 ]
  %n_lo = icmp sge i128 %fn, -9223372036854775808
  %n_hi = icmp sle i128 %fn, 9223372036854775807
  %d_hi = icmp sle i128 %fd, 9223372036854775807
  %ok_n = and i1 %n_lo, %n_hi
  %ok_all = and i1 %ok_n, %d_hi
  br i1 %ok_all, label %pack_ok, label %cold
pack_ok:
  %tn = trunc i128 %fn to i64
  %td = trunc i128 %fd to i64
  %r1 = insertvalue %DecRatio { i8 0, i64 0, i64 1 }, i64 %tn, 1
  %r2 = insertvalue %DecRatio %r1, i64 %td, 2
  ret %DecRatio %r2
cold:
  %n_lo64 = trunc i128 %num0 to i64
  %n_shr = lshr i128 %num0, 64
  %n_hi64 = trunc i128 %n_shr to i64
  %d_lo64 = trunc i128 %den0 to i64
  %d_shr = lshr i128 %den0, 64
  %d_hi64 = trunc i128 %d_shr to i64
  call void @aod_rt_ratio_reduce_i128(ptr %ctx, i64 %n_lo64, i64 %n_hi64, i64 %d_lo64, i64 %d_hi64, ptr %pout)
  %res = load %DecRatio, ptr %pout
  ret %DecRatio %res
}

define internal %DecRatio @__aod_int_exact_div(ptr %ctx, %DecInt %a, %DecInt %b) alwaysinline {
entry:
  %pa = alloca %DecInt, align 8
  %pb = alloca %DecInt, align 8
  %pout = alloca %DecRatio, align 8
  %ta = extractvalue %DecInt %a, 0
  %tb = extractvalue %DecInt %b, 0
  %or_t = or i8 %ta, %tb
  %both_inline = icmp eq i8 %or_t, 0
  br i1 %both_inline, label %fast, label %cold
fast:
  %va = extractvalue %DecInt %a, 1
  %vb = extractvalue %DecInt %b, 1
  %n128 = sext i64 %va to i128
  %d128 = sext i64 %vb to i128
  %res_f = call %DecRatio @__aod_ratio_norm_i128(ptr %ctx, i128 %n128, i128 %d128)
  ret %DecRatio %res_f
cold:
  store %DecInt %a, ptr %pa
  store %DecInt %b, ptr %pb
  call void @aod_rt_int_exact_div(ptr %ctx, ptr %pa, ptr %pb, ptr %pout)
  %res = load %DecRatio, ptr %pout
  ret %DecRatio %res
}

define internal %DecRatio @__aod_ratio_neg(ptr %ctx, %DecRatio %a) alwaysinline {
entry:
  %pa = alloca %DecRatio, align 8
  %pout = alloca %DecRatio, align 8
  %ta = extractvalue %DecRatio %a, 0
  %na = extractvalue %DecRatio %a, 1
  %is_inline = icmp eq i8 %ta, 0
  %not_min = icmp ne i64 %na, -9223372036854775808
  %fast = and i1 %is_inline, %not_min
  br i1 %fast, label %ok, label %cold
ok:
  %nn = sub nsw i64 0, %na
  %r = insertvalue %DecRatio %a, i64 %nn, 1
  ret %DecRatio %r
cold:
  store %DecRatio %a, ptr %pa
  call void @aod_rt_ratio_neg(ptr %ctx, ptr %pa, ptr %pout)
  %res = load %DecRatio, ptr %pout
  ret %DecRatio %res
}

define internal %DecRatio @__aod_ratio_add(ptr %ctx, %DecRatio %a, %DecRatio %b) alwaysinline {
entry:
  %pa = alloca %DecRatio, align 8
  %pb = alloca %DecRatio, align 8
  %pout = alloca %DecRatio, align 8
  %ta = extractvalue %DecRatio %a, 0
  %tb = extractvalue %DecRatio %b, 0
  %or_t = or i8 %ta, %tb
  %both_inline = icmp eq i8 %or_t, 0
  br i1 %both_inline, label %fast, label %cold
fast:
  %na = extractvalue %DecRatio %a, 1
  %da = extractvalue %DecRatio %a, 2
  %nb = extractvalue %DecRatio %b, 1
  %db = extractvalue %DecRatio %b, 2
  %na128 = sext i64 %na to i128
  %da128 = sext i64 %da to i128
  %nb128 = sext i64 %nb to i128
  %db128 = sext i64 %db to i128
  %t1 = mul nsw i128 %na128, %db128
  %t2 = mul nsw i128 %nb128, %da128
  %ov_res = call { i128, i1 } @llvm.sadd.with.overflow.i128(i128 %t1, i128 %t2)
  %num = extractvalue { i128, i1 } %ov_res, 0
  %ov = extractvalue { i128, i1 } %ov_res, 1
  br i1 %ov, label %cold, label %norm
norm:
  %den = mul nsw i128 %da128, %db128
  %res_f = call %DecRatio @__aod_ratio_norm_i128(ptr %ctx, i128 %num, i128 %den)
  ret %DecRatio %res_f
cold:
  store %DecRatio %a, ptr %pa
  store %DecRatio %b, ptr %pb
  call void @aod_rt_ratio_add(ptr %ctx, ptr %pa, ptr %pb, ptr %pout)
  %res = load %DecRatio, ptr %pout
  ret %DecRatio %res
}

define internal %DecRatio @__aod_ratio_sub(ptr %ctx, %DecRatio %a, %DecRatio %b) alwaysinline {
entry:
  %pa = alloca %DecRatio, align 8
  %pb = alloca %DecRatio, align 8
  %pout = alloca %DecRatio, align 8
  %ta = extractvalue %DecRatio %a, 0
  %tb = extractvalue %DecRatio %b, 0
  %or_t = or i8 %ta, %tb
  %both_inline = icmp eq i8 %or_t, 0
  br i1 %both_inline, label %fast, label %cold
fast:
  %na = extractvalue %DecRatio %a, 1
  %da = extractvalue %DecRatio %a, 2
  %nb = extractvalue %DecRatio %b, 1
  %db = extractvalue %DecRatio %b, 2
  %na128 = sext i64 %na to i128
  %da128 = sext i64 %da to i128
  %nb128 = sext i64 %nb to i128
  %db128 = sext i64 %db to i128
  %t1 = mul nsw i128 %na128, %db128
  %t2 = mul nsw i128 %nb128, %da128
  %ov_res = call { i128, i1 } @llvm.ssub.with.overflow.i128(i128 %t1, i128 %t2)
  %num = extractvalue { i128, i1 } %ov_res, 0
  %ov = extractvalue { i128, i1 } %ov_res, 1
  br i1 %ov, label %cold, label %norm
norm:
  %den = mul nsw i128 %da128, %db128
  %res_f = call %DecRatio @__aod_ratio_norm_i128(ptr %ctx, i128 %num, i128 %den)
  ret %DecRatio %res_f
cold:
  store %DecRatio %a, ptr %pa
  store %DecRatio %b, ptr %pb
  call void @aod_rt_ratio_sub(ptr %ctx, ptr %pa, ptr %pb, ptr %pout)
  %res = load %DecRatio, ptr %pout
  ret %DecRatio %res
}

define internal %DecRatio @__aod_ratio_mul(ptr %ctx, %DecRatio %a, %DecRatio %b) alwaysinline {
entry:
  %pa = alloca %DecRatio, align 8
  %pb = alloca %DecRatio, align 8
  %pout = alloca %DecRatio, align 8
  %ta = extractvalue %DecRatio %a, 0
  %tb = extractvalue %DecRatio %b, 0
  %or_t = or i8 %ta, %tb
  %both_inline = icmp eq i8 %or_t, 0
  br i1 %both_inline, label %fast, label %cold
fast:
  %na = extractvalue %DecRatio %a, 1
  %da = extractvalue %DecRatio %a, 2
  %nb = extractvalue %DecRatio %b, 1
  %db = extractvalue %DecRatio %b, 2
  %na128 = sext i64 %na to i128
  %da128 = sext i64 %da to i128
  %nb128 = sext i64 %nb to i128
  %db128 = sext i64 %db to i128
  %num = mul nsw i128 %na128, %nb128
  %den = mul nsw i128 %da128, %db128
  %res_f = call %DecRatio @__aod_ratio_norm_i128(ptr %ctx, i128 %num, i128 %den)
  ret %DecRatio %res_f
cold:
  store %DecRatio %a, ptr %pa
  store %DecRatio %b, ptr %pb
  call void @aod_rt_ratio_mul(ptr %ctx, ptr %pa, ptr %pb, ptr %pout)
  %res = load %DecRatio, ptr %pout
  ret %DecRatio %res
}

define internal %DecRatio @__aod_ratio_div(ptr %ctx, %DecRatio %a, %DecRatio %b) alwaysinline {
entry:
  %pa = alloca %DecRatio, align 8
  %pb = alloca %DecRatio, align 8
  %pout = alloca %DecRatio, align 8
  %ta = extractvalue %DecRatio %a, 0
  %tb = extractvalue %DecRatio %b, 0
  %or_t = or i8 %ta, %tb
  %both_inline = icmp eq i8 %or_t, 0
  br i1 %both_inline, label %fast, label %cold
fast:
  %na = extractvalue %DecRatio %a, 1
  %da = extractvalue %DecRatio %a, 2
  %nb = extractvalue %DecRatio %b, 1
  %db = extractvalue %DecRatio %b, 2
  %na128 = sext i64 %na to i128
  %da128 = sext i64 %da to i128
  %nb128 = sext i64 %nb to i128
  %db128 = sext i64 %db to i128
  %num = mul nsw i128 %na128, %db128
  %den = mul nsw i128 %da128, %nb128
  %res_f = call %DecRatio @__aod_ratio_norm_i128(ptr %ctx, i128 %num, i128 %den)
  ret %DecRatio %res_f
cold:
  store %DecRatio %a, ptr %pa
  store %DecRatio %b, ptr %pb
  call void @aod_rt_ratio_div(ptr %ctx, ptr %pa, ptr %pb, ptr %pout)
  %res = load %DecRatio, ptr %pout
  ret %DecRatio %res
}

define internal i32 @__aod_ratio_cmp(ptr %ctx, %DecRatio %a, %DecRatio %b) alwaysinline {
entry:
  %pa = alloca %DecRatio, align 8
  %pb = alloca %DecRatio, align 8
  %ta = extractvalue %DecRatio %a, 0
  %tb = extractvalue %DecRatio %b, 0
  %or_t = or i8 %ta, %tb
  %both_inline = icmp eq i8 %or_t, 0
  br i1 %both_inline, label %fast, label %cold
fast:
  %na = extractvalue %DecRatio %a, 1
  %da = extractvalue %DecRatio %a, 2
  %nb = extractvalue %DecRatio %b, 1
  %db = extractvalue %DecRatio %b, 2
  %na128 = sext i64 %na to i128
  %da128 = sext i64 %da to i128
  %nb128 = sext i64 %nb to i128
  %db128 = sext i64 %db to i128
  %lhs = mul nsw i128 %na128, %db128
  %rhs = mul nsw i128 %nb128, %da128
  %lt = icmp slt i128 %lhs, %rhs
  %gt = icmp sgt i128 %lhs, %rhs
  %lt32 = zext i1 %lt to i32
  %gt32 = zext i1 %gt to i32
  %diff = sub i32 %gt32, %lt32
  ret i32 %diff
cold:
  store %DecRatio %a, ptr %pa
  store %DecRatio %b, ptr %pb
  %res = call i32 @aod_rt_ratio_cmp(ptr %ctx, ptr %pa, ptr %pb)
  ret i32 %res
}

; ==================== Unboxed T1 & T32 + pack/unpack (`P0-3`) ====================

define internal i8 @__aod_t1_add(ptr %ctx, i8 %a, i8 %b) alwaysinline {
entry:
  %s = add i8 %a, %b
  %lo = icmp sge i8 %s, -1
  %hi = icmp sle i8 %s, 1
  %ok = and i1 %lo, %hi
  br i1 %ok, label %ret_ok, label %fail
ret_ok:
  ret i8 %s
fail:
  call void @aod_rt_fail(ptr %ctx, ptr @.err_t1, i64 16)
  ret i8 0
}

define internal i8 @__aod_t1_sub(ptr %ctx, i8 %a, i8 %b) alwaysinline {
entry:
  %s = sub i8 %a, %b
  %lo = icmp sge i8 %s, -1
  %hi = icmp sle i8 %s, 1
  %ok = and i1 %lo, %hi
  br i1 %ok, label %ret_ok, label %fail
ret_ok:
  ret i8 %s
fail:
  call void @aod_rt_fail(ptr %ctx, ptr @.err_t1, i64 16)
  ret i8 0
}

define internal i8 @__aod_t1_mul(ptr %ctx, i8 %a, i8 %b) alwaysinline {
entry:
  %p = mul i8 %a, %b
  ret i8 %p
}

define internal i64 @__aod_t32_add(ptr %ctx, i64 %a, i64 %b) alwaysinline {
entry:
  %ov_res = call { i64, i1 } @llvm.sadd.with.overflow.i64(i64 %a, i64 %b)
  %v = extractvalue { i64, i1 } %ov_res, 0
  %ov = extractvalue { i64, i1 } %ov_res, 1
  br i1 %ov, label %fail, label %chk
chk:
  %lo = icmp sge i64 %v, -926510094425920
  %hi = icmp sle i64 %v, 926510094425920
  %ok = and i1 %lo, %hi
  br i1 %ok, label %ret_ok, label %fail
ret_ok:
  ret i64 %v
fail:
  call void @aod_rt_fail(ptr %ctx, ptr @.err_t32, i64 17)
  ret i64 0
}

define internal i64 @__aod_t32_sub(ptr %ctx, i64 %a, i64 %b) alwaysinline {
entry:
  %ov_res = call { i64, i1 } @llvm.ssub.with.overflow.i64(i64 %a, i64 %b)
  %v = extractvalue { i64, i1 } %ov_res, 0
  %ov = extractvalue { i64, i1 } %ov_res, 1
  br i1 %ov, label %fail, label %chk
chk:
  %lo = icmp sge i64 %v, -926510094425920
  %hi = icmp sle i64 %v, 926510094425920
  %ok = and i1 %lo, %hi
  br i1 %ok, label %ret_ok, label %fail
ret_ok:
  ret i64 %v
fail:
  call void @aod_rt_fail(ptr %ctx, ptr @.err_t32, i64 17)
  ret i64 0
}

define internal i64 @__aod_t32_mul(ptr %ctx, i64 %a, i64 %b) alwaysinline {
entry:
  %ov_res = call { i64, i1 } @llvm.smul.with.overflow.i64(i64 %a, i64 %b)
  %v = extractvalue { i64, i1 } %ov_res, 0
  %ov = extractvalue { i64, i1 } %ov_res, 1
  br i1 %ov, label %fail, label %chk
chk:
  %lo = icmp sge i64 %v, -926510094425920
  %hi = icmp sle i64 %v, 926510094425920
  %ok = and i1 %lo, %hi
  br i1 %ok, label %ret_ok, label %fail
ret_ok:
  ret i64 %v
fail:
  call void @aod_rt_fail(ptr %ctx, ptr @.err_t32, i64 17)
  ret i64 0
}

define internal i64 @__aod_t32_div_floor(ptr %ctx, i64 %a, i64 %b) alwaysinline {
entry:
  %z = icmp eq i64 %b, 0
  br i1 %z, label %div0, label %fast
div0:
  call void @aod_rt_fail(ptr %ctx, ptr @.err_div0, i64 16)
  ret i64 0
fast:
  %q = sdiv i64 %a, %b
  %rem = srem i64 %a, %b
  %rnz = icmp ne i64 %rem, 0
  %x = xor i64 %a, %b
  %ds = icmp slt i64 %x, 0
  %adj = and i1 %rnz, %ds
  %qm1 = sub i64 %q, 1
  %qf = select i1 %adj, i64 %qm1, i64 %q
  %lo = icmp sge i64 %qf, -926510094425920
  %hi = icmp sle i64 %qf, 926510094425920
  %ok = and i1 %lo, %hi
  br i1 %ok, label %ret_ok, label %oob
ret_ok:
  ret i64 %qf
oob:
  call void @aod_rt_fail(ptr %ctx, ptr @.err_t32, i64 17)
  ret i64 0
}

define internal i64 @__aod_t32_rem_floor(ptr %ctx, i64 %a, i64 %b) alwaysinline {
entry:
  %z = icmp eq i64 %b, 0
  br i1 %z, label %div0, label %fast
div0:
  call void @aod_rt_fail(ptr %ctx, ptr @.err_div0, i64 16)
  ret i64 0
fast:
  %rem = srem i64 %a, %b
  %rnz = icmp ne i64 %rem, 0
  %x = xor i64 %rem, %b
  %ds = icmp slt i64 %x, 0
  %adj = and i1 %rnz, %ds
  %rp = add i64 %rem, %b
  %rf = select i1 %adj, i64 %rp, i64 %rem
  ret i64 %rf
}

define internal %DecInt @__aod_t32_pack(ptr %ctx, i64 %v0) {
entry:
  %pout = alloca %DecInt, align 8
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i_next, %loop ]
  %n = phi i64 [ %v0, %entry ], [ %n_next, %loop ]
  %bits = phi i64 [ 0, %entry ], [ %bits_next, %loop ]
  %r0 = srem i64 %n, 3
  %r_neg = icmp slt i64 %r0, 0
  %r_plus = add i64 %r0, 3
  %rem = select i1 %r_neg, i64 %r_plus, i64 %r0
  %is2 = icmp eq i64 %rem, 2
  %digit = select i1 %is2, i64 -1, i64 %rem
  ; Exact aod_core::T32::pack encoding: 0 -> 00 (0), +1 -> 01 (1), -1 -> 10 (2)
  %sh = shl i64 %i, 1
  %shifted = shl i64 %rem, %sh
  %bits_next = or i64 %bits, %shifted
  %n_sub = sub i64 %n, %digit
  %n_next = sdiv i64 %n_sub, 3
  %i_next = add i64 %i, 1
  %done = icmp eq i64 %i_next, 32
  br i1 %done, label %after, label %loop
after:
  %bit63_set = icmp slt i64 %bits_next, 0
  br i1 %bit63_set, label %spill_u64, label %inline_ok
inline_ok:
  %res_in = insertvalue %DecInt { i8 0, i64 0 }, i64 %bits_next, 1
  ret %DecInt %res_in
spill_u64:
  call void @aod_rt_int_from_u64(ptr %ctx, i64 %bits_next, ptr %pout)
  %res_sp = load %DecInt, ptr %pout
  ret %DecInt %res_sp
}

define internal i64 @__aod_t32_unpack(ptr %ctx, %DecInt %a) {
entry:
  %pa = alloca %DecInt, align 8
  %ta = extractvalue %DecInt %a, 0
  %is_big = icmp ne i8 %ta, 0
  br i1 %is_big, label %big_path, label %chk_neg
big_path:
  store %DecInt %a, ptr %pa
  %rb = call i64 @aod_rt_t32_unpack_bigint(ptr %ctx, ptr %pa)
  ret i64 %rb
chk_neg:
  %bits = extractvalue %DecInt %a, 1
  %neg = icmp slt i64 %bits, 0
  br i1 %neg, label %fail_neg, label %loop
fail_neg:
  call void @aod_rt_fail(ptr %ctx, ptr @.err_neg_pack, i64 29)
  ret i64 0
loop:
  %i = phi i64 [ 0, %chk_neg ], [ %i_next, %step_ok ]
  %acc = phi i64 [ 0, %chk_neg ], [ %acc_next, %step_ok ]
  %pow = phi i64 [ 1, %chk_neg ], [ %pow_next, %step_ok ]
  %sh = shl i64 %i, 1
  %shifted = lshr i64 %bits, %sh
  %lane = and i64 %shifted, 3
  %bad = icmp eq i64 %lane, 3
  br i1 %bad, label %fail_lane, label %step_ok
fail_lane:
  call void @aod_rt_fail(ptr %ctx, ptr @.err_bad_lane, i64 29)
  ret i64 0
step_ok:
  ; Exact aod_core::T32::unpack decoding: 0 -> 0, 1 -> +1, 2 -> -1
  %is2 = icmp eq i64 %lane, 2
  %digit = select i1 %is2, i64 -1, i64 %lane
  %term = mul i64 %digit, %pow
  %acc_next = add i64 %acc, %term
  %pow_next = mul i64 %pow, 3
  %i_next = add i64 %i, 1
  %done = icmp eq i64 %i_next, 32
  br i1 %done, label %ret_ok, label %loop
ret_ok:
  ret i64 %acc_next
}
"#;
