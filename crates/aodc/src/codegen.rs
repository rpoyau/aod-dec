use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use aod_core::bigint::BigInt;
use aod_core::ratio::Ratio;
use aod_core::trit_int::{PACK4_LUT, TAG_NULL_SENTINEL};
use aod_llvm::{CodegenPlan, UnrollHint};

use crate::ast::{
    AdverbOp, BinaryOp, ChamberDecl, ConstDecl, DecArm, DecArmParam, DecFnDecl, EnumDecl, Expr,
    ExprKind, Item, LetPattern, MatchExprArm, ObserveDecl, Pattern, PipeStage, PrepareDecl,
    ProbeDecl, Program, Stmt, StmtKind, StructDecl, TypeExpr, UnaryOp,
};
use crate::checker::CheckedUnit;
use crate::diagnostics::{DecResult, Diagnostic, DiagnosticCode};
use crate::modules::{builtin_namespace_items, canonical_builtin_name, ImportedSymbol};
use crate::types::{
    builtin_enum_variants, builtin_ns_exports_enum, is_reserved_builtin_name, lookup_builtin, Type,
    BUILTIN_MOD, MODEL_STATUS_VARIANTS, RECIPIENT_STAGE_VARIANTS,
};

/// Lower a verified `CheckedUnit` using the default baseline `CodegenPlan`.
pub fn lower_to_llvm_ir(checked: &CheckedUnit) -> DecResult<String> {
    lower_to_llvm_ir_with_plan(checked, &CodegenPlan::baseline_o2())
}

/// Lower a verified `CheckedUnit` into safe textual LLVM 18 IR parameterized by `CodegenPlan`.
///
/// Representation invariants:
/// - `int` lowers to a single 64-bit GPR (`i64`) carrying a Trit-Boxed 2-bit balanced ternary word
///   (`w u< 0xC000_0000_0000_0000` inline 32-trit, `0xD000... | handle` arena spill).
/// - `ratio` lowers to a coupled 2-GPR pair (`%DecRatio = type { i64, i64 }`).
/// - `null` lowers to `i64 -4611686018427387904` (`0xC000_0000_0000_0000`).
pub fn lower_to_llvm_ir_with_plan(
    checked: &CheckedUnit,
    plan: &CodegenPlan,
) -> DecResult<String> {
    plan.validate()
        .map_err(|msg| Diagnostic::new(DiagnosticCode::RuntimeError, msg))?;
    let mut cg = Codegen::new(checked, *plan, None);
    cg.index_all()?;
    cg.emit_all()
}

pub fn dynamic_export_symbol_name(
    export_prefix: &str,
    fn_name: &str,
    overload_index: u32,
) -> String {
    format!(
        "{export_prefix}__{}__ov{overload_index}",
        sanitize_ident(fn_name)
    )
}

/// Lower a verified library/provider `CheckedUnit` (`require_main == false`) into an LLVM IR module
/// exporting `<entry>` functions as `@<export_prefix>__<fn>__ov<k>` for Inferno `load` dispatch.
pub fn lower_dynamic_provider_to_llvm_ir_with_plan(
    checked: &CheckedUnit,
    export_prefix: &str,
    plan: &CodegenPlan,
) -> DecResult<String> {
    plan.validate()
        .map_err(|msg| Diagnostic::new(DiagnosticCode::RuntimeError, msg))?;
    let mut cg = Codegen::new(checked, *plan, Some(export_prefix.to_string()));
    cg.index_all()?;
    cg.emit_all()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LlvmTy {
    Void,
    NullI64,
    I1,
    I8,
    I64,
    DecRatio,
    Handle,
}

impl LlvmTy {
    fn from_dec(ty: &Type) -> Self {
        match ty.unwrap_read_only() {
            Type::Unit => LlvmTy::Void,
            Type::Null => LlvmTy::NullI64,
            Type::Bool => LlvmTy::I1,
            Type::QExt(inner) if **inner == Type::Bool => LlvmTy::I1,
            Type::T1 | Type::Q4 | Type::B8 => LlvmTy::I8,
            Type::Int | Type::B64 => LlvmTy::I64,
            Type::Ratio => LlvmTy::DecRatio,
            _ => LlvmTy::Handle,
        }
    }

    fn ir(self) -> &'static str {
        match self {
            LlvmTy::Void => "void",
            LlvmTy::I1 => "i1",
            LlvmTy::I8 => "i8",
            LlvmTy::NullI64 | LlvmTy::I64 | LlvmTy::Handle => "i64",
            LlvmTy::DecRatio => "%DecRatio",
        }
    }

    fn zero(self) -> &'static str {
        match self {
            LlvmTy::Void => "",
            LlvmTy::NullI64 => "-4611686018427387904",
            LlvmTy::I1 => "false",
            LlvmTy::I8 => "0",
            LlvmTy::I64 | LlvmTy::Handle => "0",
            LlvmTy::DecRatio => "{ i64 0, i64 1 }",
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
    unboxed_evolve_llvm_name: Option<String>,
    dyn_slot: Option<u32>,
    exported_external: bool,
    mod_key: String,
    param_types: Vec<Type>,
    ret_type: Type,
    arms: Vec<DecArm>,
}

#[derive(Clone, Debug)]
enum LoweredCallTarget {
    UserFn(Vec<FnOverload>),
    Builtin(String),
}

struct Codegen<'a> {
    checked: &'a CheckedUnit,
    plan: CodegenPlan,
    export_entry_prefix: Option<String>,
    structs: BTreeMap<(String, String), StructMeta>,
    enums: BTreeMap<(String, String), EnumMeta>,
    chambers: BTreeMap<(String, String), ChamberDecl>,
    prepares: BTreeMap<(String, String), PrepareDecl>,
    observes: BTreeMap<(String, String), ObserveDecl>,
    qext_modules: BTreeSet<(String, String)>,
    consts: BTreeMap<(String, String), ConstMeta>,
    const_order: Vec<(String, String)>,
    fn_overloads: BTreeMap<(String, String), Vec<FnOverload>>,
    obs_callbacks: BTreeMap<(String, String, String), String>,
    str_pool: BTreeMap<Vec<u8>, (String, usize)>,
    loop_md_defs: Vec<String>,
    next_md_id: usize,
    fn_defs: String,
    next_id: usize,
}

impl<'a> Codegen<'a> {
    fn new(
        checked: &'a CheckedUnit,
        plan: CodegenPlan,
        export_entry_prefix: Option<String>,
    ) -> Self {
        Self {
            checked,
            plan,
            export_entry_prefix,
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

    fn alloc_loop_hint_md(&mut self) -> Option<String> {
        let mut bodies: Vec<String> = Vec::new();
        match self.plan.unroll_hint {
            UnrollHint::Default => {}
            UnrollHint::Unroll4 => {
                bodies.push("!{!\"llvm.loop.unroll.count\", i32 4}".to_string());
            }
            UnrollHint::Disable => {
                bodies.push("!{!\"llvm.loop.unroll.disable\"}".to_string());
            }
        }
        if self.plan.vectorize_loops {
            bodies.push("!{!\"llvm.loop.vectorize.enable\", i1 true}".to_string());
        }
        if matches!(self.plan.interleave_count, 2 | 4) {
            bodies.push(format!(
                "!{{!\"llvm.loop.interleave.count\", i32 {}}}",
                self.plan.interleave_count
            ));
        }
        if bodies.is_empty() {
            return None;
        }
        let loop_id = self.next_md_id;
        self.next_md_id += 1;
        let mut refs = vec![format!("!{loop_id}")];
        let mut child_defs = Vec::with_capacity(bodies.len());
        for body in bodies {
            let cid = self.next_md_id;
            self.next_md_id += 1;
            refs.push(format!("!{cid}"));
            child_defs.push(format!("!{cid} = {body}"));
        }
        self.loop_md_defs
            .push(format!("!{loop_id} = distinct !{{{}}}", refs.join(", ")));
        self.loop_md_defs.extend(child_defs);
        Some(format!(", !llvm.loop !{loop_id}"))
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
        if name.contains("::") {
            let segs: Vec<&str> = name.split("::").collect();
            if segs.len() == 2 {
                if let Some(ImportedSymbol::ModuleNamespace(m)) =
                    self.imports_for(mod_key).get(segs[0])
                {
                    if let Some(ns_tail) = m.strip_prefix("aod::") {
                        if builtin_ns_exports_enum(ns_tail, segs[1]) {
                            return Some(Type::Enum {
                                module: BUILTIN_MOD.to_string(),
                                name: segs[1].to_string(),
                            });
                        }
                        return None;
                    }
                    if self.structs.contains_key(&(m.clone(), segs[1].to_string())) {
                        return Some(Type::Struct {
                            module: m.clone(),
                            name: segs[1].to_string(),
                        });
                    }
                    if self.enums.contains_key(&(m.clone(), segs[1].to_string())) {
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
        match self.imports_for(mod_key).get(name) {
            Some(ImportedSymbol::ModuleItem { module, item }) => {
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
            Some(ImportedSymbol::Builtin(canon)) if builtin_enum_variants(canon).is_some() => {
                return Some(Type::Enum {
                    module: BUILTIN_MOD.to_string(),
                    name: canon.clone(),
                });
            }
            _ => {}
        }
        if builtin_enum_variants(name).is_some() {
            return Some(Type::Enum {
                module: BUILTIN_MOD.to_string(),
                name: name.to_string(),
            });
        }
        None
    }

    fn resolve_type(&self, mod_key: &str, expr: &TypeExpr, line: usize) -> DecResult<Type> {
        Type::resolve_with(expr, line, &|n| self.lookup_user_type(mod_key, n))
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

    fn register_builtin_enums(&mut self) {
        for (ename, variants) in [
            ("ModelStatus", MODEL_STATUS_VARIANTS),
            ("RecipientStage", RECIPIENT_STAGE_VARIANTS),
        ] {
            self.enums.insert(
                (BUILTIN_MOD.to_string(), ename.to_string()),
                EnumMeta {
                    module: BUILTIN_MOD.to_string(),
                    name: ename.to_string(),
                    variants: variants
                        .iter()
                        .map(|(vn, tag)| ((*vn).to_string(), *tag, Vec::new()))
                        .collect(),
                },
            );
        }
    }

    fn index_all(&mut self) -> DecResult<()> {
        self.register_builtin_enums();
        self.collect_names("<entry>", &self.checked.unit.entry_program);
        for (mk, prog) in &self.checked.unit.parsed_modules {
            self.collect_names(mk, prog);
        }
        self.resolve_types_in_program("<entry>", &self.checked.unit.entry_program)?;
        for (mk, prog) in &self.checked.unit.parsed_modules {
            self.resolve_types_in_program(mk, prog)?;
        }
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

    fn index_const(&mut self, mod_key: &str, c: &ConstDecl) -> DecResult<()> {
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

        // 2-pass slot hints keyed by `(arity, slot_index)` so contextual bare `Pattern::IntLit`
        // on `t1` arms inherits `Type::T1` instead of prematurely locking `Type::Int`.
        let mut slot_hints: BTreeMap<(usize, usize), Type> = BTreeMap::new();
        for arm in &f.arms {
            let arity = arm.params.len();
            for (i, p) in arm.params.iter().enumerate() {
                if slot_hints.contains_key(&(arity, i)) {
                    continue;
                }
                match p {
                    DecArmParam::TypedVar { ty, .. } | DecArmParam::TypedPattern { ty, .. } => {
                        let resolved = self.resolve_type(mod_key, ty, arm.line)?;
                        if resolved != Type::Unknown {
                            slot_hints.insert((arity, i), resolved);
                        }
                    }
                    DecArmParam::Pattern(pat) if !matches!(pat, Pattern::IntLit(_)) => {
                        if let Some(t) = self.infer_pattern_type(mod_key, pat, None) {
                            if t != Type::Unknown {
                                slot_hints.insert((arity, i), t);
                            }
                        }
                    }
                    DecArmParam::Pattern(_) => {}
                }
            }
        }
        for arm in &f.arms {
            let arity = arm.params.len();
            for (i, p) in arm.params.iter().enumerate() {
                if !slot_hints.contains_key(&(arity, i)) {
                    if let DecArmParam::Pattern(Pattern::IntLit(_)) = p {
                        slot_hints.insert((arity, i), Type::Int);
                    }
                }
            }
        }

        let mut groups: Vec<(Vec<Type>, Type, Vec<DecArm>)> = Vec::new();
        for arm in &f.arms {
            let arity = arm.params.len();
            let raw_tys = self.infer_arm_param_types(mod_key, arm, &slot_hints)?;
            let concrete_tys: Vec<Type> = raw_tys
                .into_iter()
                .enumerate()
                .map(|(i, t)| {
                    if t == Type::Unknown {
                        slot_hints
                            .get(&(arity, i))
                            .cloned()
                            .unwrap_or(Type::Int)
                    } else {
                        t
                    }
                })
                .collect();
            let rty = self.resolve_type(mod_key, &arm.return_type, arm.line)?;
            if let Some(existing) = groups.iter_mut().find(|(sig, _, _)| sig == &concrete_tys) {
                existing.2.push(arm.clone());
            } else {
                groups.push((concrete_tys, rty, vec![arm.clone()]));
            }
        }

        for (ov_idx, (param_types, ret_type, group_arms)) in groups.into_iter().enumerate() {
            let id = self.fresh_id();
            let dyn_slot = self
                .checked
                .dynamic_slots
                .iter()
                .find(|s| {
                    s.unit_key == mod_key
                        && s.fn_name == f.name
                        && s.overload_index == (ov_idx as u32)
                })
                .map(|s| s.slot_index);

            let (llvm_name, exported_external) = if let Some(ref prefix) = self.export_entry_prefix
            {
                if mod_key == "<entry>" && f.name != "main" {
                    (
                        format!(
                            "@{}",
                            dynamic_export_symbol_name(prefix, &f.name, ov_idx as u32)
                        ),
                        true,
                    )
                } else {
                    (format!("@dec_fn_{}_{}", sanitize_ident(&f.name), id), false)
                }
            } else if is_entry_main && param_types.is_empty() {
                ("@dec_entry_main".to_string(), false)
            } else {
                (format!("@dec_fn_{}_{}", sanitize_ident(&f.name), id), false)
            };

            let unboxed_evolve_llvm_name = if dyn_slot.is_none()
                && !exported_external
                && qualifies_for_unboxed_evolve(&f.name, &param_types, &ret_type, &group_arms)
            {
                Some(format!("{llvm_name}_unboxed"))
            } else {
                None
            };
            self.fn_overloads
                .entry((mod_key.to_string(), f.name.clone()))
                .or_default()
                .push(FnOverload {
                    llvm_name,
                    unboxed_evolve_llvm_name,
                    dyn_slot,
                    exported_external,
                    mod_key: mod_key.to_string(),
                    param_types,
                    ret_type,
                    arms: group_arms,
                });
        }
        Ok(())
    }

    fn infer_arm_param_types(
        &self,
        mod_key: &str,
        arm: &DecArm,
        slot_hints: &BTreeMap<(usize, usize), Type>,
    ) -> DecResult<Vec<Type>> {
        let arity = arm.params.len();
        let mut out = Vec::with_capacity(arity);
        for (i, p) in arm.params.iter().enumerate() {
            let sh = slot_hints.get(&(arity, i));
            let inferred = match p {
                DecArmParam::TypedVar { ty, .. } | DecArmParam::TypedPattern { ty, .. } => {
                    self.resolve_type(mod_key, ty, arm.line)?
                }
                DecArmParam::Pattern(pat) => self
                    .infer_pattern_type(mod_key, pat, sh)
                    .unwrap_or(Type::Unknown),
            };
            out.push(inferred);
        }
        Ok(out)
    }

    fn infer_pattern_type(
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
                let (em, _) = self.resolve_enum_variant(mod_key, path)?;
                Some(Type::Enum {
                    module: em.module.clone(),
                    name: em.name.clone(),
                })
            }
            Pattern::Tuple(ps) => {
                let sub_hints: Option<&[Type]> = match slot_hint.map(Type::unwrap_read_only) {
                    Some(Type::Tuple(ts)) if ts.len() == ps.len() => Some(ts.as_slice()),
                    _ => None,
                };
                let mut ts = Vec::with_capacity(ps.len());
                for (i, p) in ps.iter().enumerate() {
                    let sh = sub_hints.map(|s| &s[i]);
                    ts.push(self.infer_pattern_type(mod_key, p, sh)?);
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

        let all_overloads: Vec<FnOverload> = self
            .fn_overloads
            .values()
            .flat_map(|v| v.iter().cloned())
            .collect();
        for ov in all_overloads {
            self.emit_user_fn(&ov)?;
            self.emit_unboxed_evolve_fn(&ov)?;
        }

        self.emit_init_consts()?;

        if self.export_entry_prefix.is_none() {
            let main_ret = self
                .checked
                .main_return_type
                .clone()
                .unwrap_or(Type::Unit);
            self.emit_entrypoints(&main_ret);
        }

        let mut ir = String::with_capacity(self.fn_defs.len() + 28_000);
        let lut_escaped = escape_llvm_bytes(&PACK4_LUT);
        let _ = writeln!(
            ir,
            "@.pack4_lut = private unnamed_addr constant [81 x i8] c\"{lut_escaped}\", align 1"
        );
        ir.push_str(LLVM_MODULE_PREAMBLE);
        ir.push('\n');

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
            let v = fb.emit_expr_with_hint(self, &cm.value_expr, Some(&cm.dec_ty))?;
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
        self.fn_defs
            .push_str(&fb.render_fn("define internal void @aod_init_consts(ptr %ctx)"));
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
        fb.scopes
            .last_mut()
            .unwrap()
            .insert(param_name.to_string(), view_slot);
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
        let linkage = if ov.exported_external {
            "define"
        } else {
            "define internal"
        };
        let header = format!("{linkage} {} {}({sig_params})", ret_lt.ir(), ov.llvm_name);

        let mut fb = FnBuilder::new(&ov.llvm_name, ov.ret_type.clone(), &ov.mod_key);
        fb.emit_entry_depth_and_fuel();
        if ov.exported_external && !self.const_order.is_empty() {
            fb.line("  call void @aod_init_consts(ptr %ctx)");
            fb.emit_fail_return_check();
        }

        if self.try_emit_two_arm_branchless_select(&mut fb, ov, ret_lt)? {
            self.fn_defs.push_str(&fb.render_fn(&header));
            return Ok(());
        }

        for (arm_idx, arm) in ov.arms.iter().enumerate() {
            let next_arm_lbl = format!("arm_next_{arm_idx}");
            fb.push_scope();
            for (i, (param, pty)) in arm.params.iter().zip(&ov.param_types).enumerate() {
                let arg_val = Val {
                    reg: format!("%arg{i}"),
                    dec_ty: pty.clone(),
                };
                match param {
                    DecArmParam::TypedVar { name, .. } => {
                        if name != "_" {
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

            fb.emit_stmts(self, &arm.body)?;
            if !fb.terminated {
                fb.emit_depth_dec();
                fb.emit_default_ret();
            }
            fb.pop_scope();
            fb.start_block(&next_arm_lbl);
        }

        let (msg_g, msg_len) = self.intern_bytes(b"no matching arm for dec call");
        fb.line(&format!(
            "  call void @aod_rt_fail(ptr %ctx, ptr {msg_g}, i64 {msg_len})"
        ));
        fb.emit_depth_dec();
        fb.emit_default_ret();

        self.fn_defs.push_str(&fb.render_fn(&header));
        Ok(())
    }

    fn try_emit_two_arm_branchless_select(
        &mut self,
        fb: &mut FnBuilder,
        ov: &FnOverload,
        ret_lt: LlvmTy,
    ) -> DecResult<bool> {
        if !self.plan.branchless_select
            || ov.arms.len() != 2
            || matches!(ret_lt, LlvmTy::Handle | LlvmTy::Void)
            || ov
                .param_types
                .iter()
                .any(|pt| matches!(LlvmTy::from_dec(pt), LlvmTy::Handle | LlvmTy::Void))
        {
            return Ok(false);
        }
        let arm0 = &ov.arms[0];
        let arm1 = &ov.arms[1];
        if arm1.guard.is_some() || arm0.body.len() != 1 || arm1.body.len() != 1 {
            return Ok(false);
        }
        let StmtKind::Return(Some(ref e_then)) = arm0.body[0].kind else {
            return Ok(false);
        };
        let StmtKind::Return(Some(ref e_else)) = arm1.body[0].kind else {
            return Ok(false);
        };

        let mut arm0_has_refutable_pat = false;
        for p in &arm0.params {
            match p {
                DecArmParam::TypedVar { .. } => {}
                DecArmParam::TypedPattern { pattern, .. } | DecArmParam::Pattern(pattern) => {
                    match pattern {
                        Pattern::Wildcard | Pattern::Binding(_) => {}
                        Pattern::NullLit
                        | Pattern::BoolLit(_)
                        | Pattern::B8Lit(_)
                        | Pattern::B64Lit(_)
                        | Pattern::TritLit(_)
                        | Pattern::Q4Lit(_) => {
                            arm0_has_refutable_pat = true;
                        }
                        Pattern::IntLit(n) if n.inline_word().is_some() => {
                            arm0_has_refutable_pat = true;
                        }
                        Pattern::RatioLit(r) if r.inline_words().is_some() => {
                            arm0_has_refutable_pat = true;
                        }
                        _ => return Ok(false),
                    }
                }
            }
        }
        if !arm0_has_refutable_pat && arm0.guard.is_none() {
            return Ok(false);
        }
        for p in &arm1.params {
            match p {
                DecArmParam::TypedVar { .. } => {}
                DecArmParam::TypedPattern { pattern, .. } | DecArmParam::Pattern(pattern) => {
                    if !matches!(pattern, Pattern::Wildcard | Pattern::Binding(_)) {
                        return Ok(false);
                    }
                }
            }
        }

        // Pre-verify purity of `e_then` (and `guard` if paired with a refutable literal pattern)
        // in Arm 0's isolated parameter scope BEFORE emitting any IR!
        fb.push_scope();
        for (param, pty) in arm0.params.iter().zip(&ov.param_types) {
            match param {
                DecArmParam::TypedVar { name, .. }
                | DecArmParam::TypedPattern {
                    pattern: Pattern::Binding(name),
                    ..
                }
                | DecArmParam::Pattern(Pattern::Binding(name))
                    if name != "_" =>
                {
                    fb.scopes.last_mut().unwrap().insert(
                        name.clone(),
                        LocalSlot {
                            alloca: String::new(),
                            dec_ty: pty.clone(),
                        },
                    );
                }
                _ => {}
            }
        }
        let guard_ok_with_pat = match (arm0_has_refutable_pat, &arm0.guard) {
            (true, Some(g)) => fb.is_pure_select_safe(self, g),
            _ => true,
        };
        let then_pure = guard_ok_with_pat && fb.is_pure_select_safe(self, e_then);
        fb.pop_scope();
        if !then_pure {
            return Ok(false);
        }

        // Pre-verify purity of `e_else` in Arm 1's isolated parameter scope BEFORE emitting any IR!
        fb.push_scope();
        for (param, pty) in arm1.params.iter().zip(&ov.param_types) {
            match param {
                DecArmParam::TypedVar { name, .. }
                | DecArmParam::TypedPattern {
                    pattern: Pattern::Binding(name),
                    ..
                }
                | DecArmParam::Pattern(Pattern::Binding(name))
                    if name != "_" =>
                {
                    fb.scopes.last_mut().unwrap().insert(
                        name.clone(),
                        LocalSlot {
                            alloca: String::new(),
                            dec_ty: pty.clone(),
                        },
                    );
                }
                _ => {}
            }
        }
        let else_pure = fb.is_pure_select_safe(self, e_else);
        fb.pop_scope();
        if !else_pure {
            return Ok(false);
        }

        // Emit Arm 0 in its own isolated scope
        fb.push_scope();
        let mut cond_reg = "true".to_string();
        for (i, (param, pty)) in arm0.params.iter().zip(&ov.param_types).enumerate() {
            let arg_val = Val {
                reg: format!("%arg{i}"),
                dec_ty: pty.clone(),
            };
            match param {
                DecArmParam::TypedVar { name, .. } => {
                    if name != "_" {
                        let lt = LlvmTy::from_dec(pty);
                        let slot = fb.alloc_local(name, pty.clone());
                        fb.line(&format!("  store {} %arg{i}, ptr {}", lt.ir(), slot.alloca));
                        fb.scopes.last_mut().unwrap().insert(name.clone(), slot);
                    }
                }
                DecArmParam::TypedPattern { pattern, .. } | DecArmParam::Pattern(pattern) => {
                    let pcond = fb.emit_pattern_match(self, pattern, &arg_val)?;
                    if cond_reg == "true" {
                        cond_reg = pcond;
                    } else if pcond != "true" {
                        let nxt = fb.fresh_reg();
                        fb.line(&format!("  {nxt} = and i1 {cond_reg}, {pcond}"));
                        cond_reg = nxt;
                    }
                }
            }
        }
        if let Some(ref guard) = arm0.guard {
            let gv = fb.emit_expr(self, guard)?;
            fb.emit_fail_return_check();
            if cond_reg == "true" {
                cond_reg = gv.reg;
            } else {
                let nxt = fb.fresh_reg();
                fb.line(&format!("  {nxt} = and i1 {cond_reg}, {}", gv.reg));
                cond_reg = nxt;
            }
        }
        let v_then = fb.emit_expr_with_hint(self, e_then, Some(&ov.ret_type))?;
        fb.pop_scope();

        // Emit Arm 1 in its own fresh scope (isolated from Arm 0 parameter names!)
        fb.push_scope();
        for (i, (param, pty)) in arm1.params.iter().zip(&ov.param_types).enumerate() {
            match param {
                DecArmParam::TypedVar { name, .. }
                | DecArmParam::TypedPattern {
                    pattern: Pattern::Binding(name),
                    ..
                }
                | DecArmParam::Pattern(Pattern::Binding(name)) => {
                    if name != "_" {
                        let lt = LlvmTy::from_dec(pty);
                        let slot = fb.alloc_local(name, pty.clone());
                        fb.line(&format!("  store {} %arg{i}, ptr {}", lt.ir(), slot.alloca));
                        fb.scopes.last_mut().unwrap().insert(name.clone(), slot);
                    }
                }
                _ => {}
            }
        }
        let v_else = fb.emit_expr_with_hint(self, e_else, Some(&ov.ret_type))?;
        fb.pop_scope();

        let sel = fb.fresh_reg();
        fb.line(&format!(
            "  {sel} = select i1 {cond_reg}, {} {}, {} {}",
            ret_lt.ir(),
            v_then.reg,
            ret_lt.ir(),
            v_else.reg
        ));
        fb.emit_depth_dec();
        fb.line(&format!("  ret {} {sel}", ret_lt.ir()));
        fb.terminated = true;
        Ok(true)
    }

    fn emit_unboxed_evolve_fn(&mut self, ov: &FnOverload) -> DecResult<()> {
        let Some(ref unboxed_name) = ov.unboxed_evolve_llvm_name else {
            return Ok(());
        };
        let Type::Tuple(ref state_elems) = ov.param_types[0] else {
            return Ok(());
        };
        let arm = &ov.arms[0];
        let mut sig_params = String::from("ptr %ctx");
        for j in 0..state_elems.len() {
            let _ = write!(sig_params, ", ptr %s{j}");
        }
        let _ = write!(sig_params, ", i64 %arg_idx");
        if ov.param_types.len() == 3 {
            let env_lt = LlvmTy::from_dec(&ov.param_types[2]);
            if env_lt != LlvmTy::Void {
                let _ = write!(sig_params, ", {} %arg_env", env_lt.ir());
            }
        }
        let header = format!("define internal void {unboxed_name}({sig_params})");

        let mut fb = FnBuilder::new(unboxed_name, Type::Unit, &ov.mod_key);
        fb.emit_entry_depth_and_fuel();

        // 1. Pre-load ALL `%s0..%s_{k-1}` into distinct local slots (skipping `_` wildcards!)
        //    BEFORE evaluating any body statements.
        let sub_pats = match &arm.params[0] {
            DecArmParam::TypedPattern {
                pattern: Pattern::Tuple(ps),
                ..
            }
            | DecArmParam::Pattern(Pattern::Tuple(ps)) => ps,
            _ => return Ok(()),
        };
        for (j, (sp, ety)) in sub_pats.iter().zip(state_elems).enumerate() {
            if let Pattern::Binding(bname) = sp {
                if bname != "_" {
                    let elt = LlvmTy::from_dec(ety);
                    let loaded = fb.fresh_reg();
                    fb.line(&format!("  {loaded} = load {}, ptr %s{j}", elt.ir()));
                    let slot = fb.alloc_local(bname, ety.clone());
                    fb.line(&format!("  store {} {loaded}, ptr {}", elt.ir(), slot.alloca));
                    fb.scopes.last_mut().unwrap().insert(bname.clone(), slot);
                }
            }
        }

        // 2. Bind `idx` and optional `env`
        fb.bind_irrefutable_arm_param(&arm.params[1], "%arg_idx", &Type::Int);
        if ov.param_types.len() == 3 {
            fb.bind_irrefutable_arm_param(&arm.params[2], "%arg_env", &ov.param_types[2]);
        }

        // 3. Emit preceding body statements, then evaluate ALL `k` return tuple elements into SSA
        //    registers BEFORE calling `emit_depth_dec()` and storing back into `%s0..%s_{k-1}`!
        let last_idx = arm.body.len() - 1;
        fb.emit_stmts(self, &arm.body[..last_idx])?;
        let StmtKind::Return(Some(Expr {
            kind: ExprKind::Tuple(ref ret_elems),
            ..
        })) = arm.body[last_idx].kind
        else {
            return Ok(());
        };
        let mut out_regs = Vec::with_capacity(ret_elems.len());
        for (j, re) in ret_elems.iter().enumerate() {
            let rv = fb.emit_expr_with_hint(self, re, state_elems.get(j))?;
            out_regs.push((rv.reg, LlvmTy::from_dec(&rv.dec_ty)));
        }
        fb.emit_depth_dec();
        for (j, (oreg, olt)) in out_regs.iter().enumerate() {
            fb.line(&format!("  store {} {oreg}, ptr %s{j}", olt.ir()));
        }
        fb.line("  ret void");
        fb.terminated = true;

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
                Type::Null => s.push_str("  call void @aod_rt_emit_null(ptr %ctx)\n"),
                Type::Int => s.push_str("  call void @aod_rt_emit_int(ptr %ctx, i64 %res)\n"),
                Type::Bool => {
                    s.push_str("  %b8 = zext i1 %res to i8\n");
                    s.push_str("  call void @aod_rt_emit_bool(ptr %ctx, i8 %b8)\n");
                }
                Type::QExt(inner) if **inner == Type::Bool => {
                    s.push_str("  %b8 = zext i1 %res to i8\n");
                    s.push_str("  call void @aod_rt_emit_qext_bool(ptr %ctx, i8 %b8)\n");
                }
                Type::T1 => s.push_str("  call void @aod_rt_emit_t1(ptr %ctx, i8 %res)\n"),
                Type::B8 => s.push_str("  call void @aod_rt_emit_b8(ptr %ctx, i8 %res)\n"),
                Type::B64 => s.push_str("  call void @aod_rt_emit_b64(ptr %ctx, i64 %res)\n"),
                Type::Q4 => s.push_str("  call void @aod_rt_emit_q4(ptr %ctx, i8 %res)\n"),
                Type::Ratio => {
                    s.push_str("  %rn = extractvalue %DecRatio %res, 0\n");
                    s.push_str("  %rd = extractvalue %DecRatio %res, 1\n");
                    s.push_str("  call void @aod_rt_emit_ratio(ptr %ctx, i64 %rn, i64 %rd)\n");
                }
                _ => s.push_str("  call void @aod_rt_emit_handle(ptr %ctx, i64 %res)\n"),
            }
            s.push_str("  br label %done\n");
        }
        s.push_str("done:\n  ret void\n}\n\n");

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
        }
    }

    fn bind_irrefutable_arm_param(&mut self, param: &DecArmParam, arg_reg: &str, pty: &Type) {
        let lt = LlvmTy::from_dec(pty);
        match param {
            DecArmParam::TypedVar { name, .. } => {
                if name != "_" {
                    let slot = self.alloc_local(name, pty.clone());
                    if lt != LlvmTy::Void {
                        self.line(&format!("  store {} {arg_reg}, ptr {}", lt.ir(), slot.alloca));
                    }
                    self.scopes.last_mut().unwrap().insert(name.clone(), slot);
                }
            }
            DecArmParam::TypedPattern { pattern, .. } | DecArmParam::Pattern(pattern) => {
                self.bind_irrefutable_pat(pattern, arg_reg, pty);
            }
        }
    }

    fn bind_irrefutable_pat(&mut self, pat: &Pattern, val_reg: &str, pty: &Type) {
        match pat {
            Pattern::Wildcard => {}
            Pattern::Binding(name) => {
                if name != "_" {
                    let lt = LlvmTy::from_dec(pty);
                    let slot = self.alloc_local(name, pty.clone());
                    if lt != LlvmTy::Void {
                        self.line(&format!("  store {} {val_reg}, ptr {}", lt.ir(), slot.alloca));
                    }
                    self.scopes.last_mut().unwrap().insert(name.clone(), slot);
                }
            }
            Pattern::Tuple(ps) => {
                if let Type::Tuple(elem_tys) = pty.unwrap_read_only() {
                    for (i, (sp, et)) in ps.iter().zip(elem_tys).enumerate() {
                        if matches!(sp, Pattern::Wildcard) {
                            continue;
                        }
                        if let Pattern::Binding(n) = sp {
                            if n == "_" {
                                continue;
                            }
                        }
                        let fh = self.fresh_reg();
                        self.line(&format!(
                            "  {fh} = call i64 @aod_rt_tuple_get(ptr %ctx, i64 {val_reg}, i64 {i})"
                        ));
                        self.emit_fail_return_check();
                        let fv = self.unbox_handle_to_val(&fh, et);
                        self.bind_irrefutable_pat(sp, &fv.reg, et);
                    }
                }
            }
            _ => {}
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

    fn extract_ratio_words(&mut self, reg: &str) -> (String, String) {
        let na = self.fresh_reg();
        let da = self.fresh_reg();
        self.line(&format!("  {na} = extractvalue %DecRatio {reg}, 0"));
        self.line(&format!("  {da} = extractvalue %DecRatio {reg}, 1"));
        (na, da)
    }

    fn emit_entry_depth_and_fuel(&mut self) {
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
            Type::Null => {
                let r = self.fresh_reg();
                self.line(&format!("  {r} = call i64 @aod_rt_val_null(ptr %ctx)"));
                r
            }
            Type::Int => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_val_from_int(ptr %ctx, i64 {})",
                    v.reg
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
            Type::B8 => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_val_from_b8(ptr %ctx, i8 {})",
                    v.reg
                ));
                r
            }
            Type::B64 => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_val_from_b64(ptr %ctx, i64 {})",
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
                let (na, da) = self.extract_ratio_words(&v.reg);
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_val_from_ratio(ptr %ctx, i64 {na}, i64 {da})"
                ));
                r
            }
            _ => v.reg.clone(),
        }
    }

    fn unbox_handle_to_val(&mut self, h_reg: &str, target_ty: &Type) -> Val {
        let reg = match target_ty.unwrap_read_only() {
            Type::Unit => String::new(),
            Type::Null => (TAG_NULL_SENTINEL as i64).to_string(),
            Type::Int => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_val_to_int(ptr %ctx, i64 {h_reg})"
                ));
                self.emit_fail_return_check();
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
            Type::B8 => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i8 @aod_rt_val_to_b8(ptr %ctx, i64 {h_reg})"
                ));
                self.emit_fail_return_check();
                r
            }
            Type::B64 => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_val_to_b64(ptr %ctx, i64 {h_reg})"
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

    fn emit_bytes_handle(&mut self, cg: &mut Codegen<'_>, bs: &[u8]) -> String {
        let (g, len) = cg.intern_bytes(bs);
        let r = self.fresh_reg();
        self.line(&format!(
            "  {r} = call i64 @aod_rt_bytes_new(ptr %ctx, ptr {g}, i64 {len})"
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
            StmtKind::Let { pattern, ty, value } => {
                let resolved_ann = match ty {
                    Some(te) => Some(cg.resolve_type(&self.mod_key, te, stmt.line)?),
                    None => None,
                };
                let mut val = self.emit_expr_with_hint(cg, value, resolved_ann.as_ref())?;
                if let Some(ann) = resolved_ann {
                    val.dec_ty = ann;
                }
                match pattern {
                    LetPattern::Single(name) => {
                        if name != "_" {
                            let lt = val.llvm_ty();
                            let slot = self.alloc_local(name, val.dec_ty.clone());
                            if lt != LlvmTy::Void {
                                self.line(&format!(
                                    "  store {} {}, ptr {}",
                                    lt.ir(),
                                    val.reg,
                                    slot.alloca
                                ));
                            }
                            self.scopes.last_mut().unwrap().insert(name.clone(), slot);
                        }
                    }
                    LetPattern::Tuple(elems) => {
                        let val_ro = val.dec_ty.is_read_only();
                        let elem_tys = match val.dec_ty.unwrap_read_only() {
                            Type::Tuple(ts) => ts.clone(),
                            other => {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::RuntimeError,
                                    stmt.line,
                                    format!("codegen: expected tuple for destructuring, got {other}"),
                                ));
                            }
                        };
                        for (i, ((n, te_opt), et)) in elems.iter().zip(&elem_tys).enumerate() {
                            if n == "_" {
                                continue;
                            }
                            let mut target_et = match te_opt {
                                Some(te) => cg.resolve_type(&self.mod_key, te, stmt.line)?,
                                None => et.clone(),
                            };
                            if val_ro && target_et.needs_readonly_propagation() {
                                target_et = target_et.wrap_read_only();
                            }
                            let fh = self.fresh_reg();
                            self.line(&format!(
                                "  {fh} = call i64 @aod_rt_tuple_get(ptr %ctx, i64 {}, i64 {i})",
                                val.reg
                            ));
                            self.emit_fail_return_check();
                            let fv = self.unbox_handle_to_val(&fh, &target_et);
                            let lt = fv.llvm_ty();
                            let slot = self.alloc_local(n, target_et);
                            if lt != LlvmTy::Void {
                                self.line(&format!(
                                    "  store {} {}, ptr {}",
                                    lt.ir(),
                                    fv.reg,
                                    slot.alloca
                                ));
                            }
                            self.scopes.last_mut().unwrap().insert(n.clone(), slot);
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
                    Type::Null => {
                        self.line("  call void @aod_rt_emit_null(ptr %ctx)");
                    }
                    Type::Int => {
                        self.line(&format!("  call void @aod_rt_emit_int(ptr %ctx, i64 {})", v.reg));
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
                    Type::B8 => {
                        self.line(&format!("  call void @aod_rt_emit_b8(ptr %ctx, i8 {})", v.reg));
                    }
                    Type::B64 => {
                        self.line(&format!("  call void @aod_rt_emit_b64(ptr %ctx, i64 {})", v.reg));
                    }
                    Type::Q4 => {
                        self.line(&format!("  call void @aod_rt_emit_q4(ptr %ctx, i8 {})", v.reg));
                    }
                    Type::Ratio => {
                        let (na, da) = self.extract_ratio_words(&v.reg);
                        self.line(&format!(
                            "  call void @aod_rt_emit_ratio(ptr %ctx, i64 {na}, i64 {da})"
                        ));
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
                    let ret_hint = self.ret_ty.clone();
                    let v = self.emit_expr_with_hint(cg, e, Some(&ret_hint))?;
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

    // ==================== Pattern Matching ====================

    fn emit_pattern_match(
        &mut self,
        cg: &mut Codegen<'_>,
        pat: &Pattern,
        val: &Val,
    ) -> DecResult<String> {
        match pat {
            Pattern::Wildcard | Pattern::NullLit => Ok("true".to_string()),
            Pattern::Binding(name) => {
                if name != "_" {
                    let slot = self.alloc_local(name, val.dec_ty.clone());
                    let lt = val.llvm_ty();
                    if lt != LlvmTy::Void {
                        self.line(&format!(
                            "  store {} {}, ptr {}",
                            lt.ir(),
                            val.reg,
                            slot.alloca
                        ));
                    }
                    self.scopes.last_mut().unwrap().insert(name.clone(), slot);
                }
                Ok("true".to_string())
            }
            Pattern::IntLit(n) => {
                if *val.dec_ty.unwrap_read_only() == Type::T1 {
                    let Some(v) = n.to_i64() else {
                        return Ok("false".to_string());
                    };
                    if !(-1..=1).contains(&v) {
                        return Ok("false".to_string());
                    }
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = icmp eq i8 {}, {v}", val.reg));
                    return Ok(r);
                }
                let lit = self.emit_bigint_lit(cg, n);
                let cmp = self.fresh_reg();
                self.line(&format!(
                    "  {cmp} = call i8 @__aod_int_cmp(ptr %ctx, i64 {}, i64 {})",
                    val.reg, lit.reg
                ));
                self.emit_fail_return_check();
                let eq = self.fresh_reg();
                self.line(&format!("  {eq} = icmp eq i8 {cmp}, 0"));
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
            Pattern::B8Lit(b) => {
                let r = self.fresh_reg();
                self.line(&format!("  {r} = icmp eq i8 {}, {}", val.reg, b.0 as i8));
                Ok(r)
            }
            Pattern::B64Lit(b) => {
                let r = self.fresh_reg();
                self.line(&format!("  {r} = icmp eq i64 {}, {}", val.reg, b.0 as i64));
                Ok(r)
            }
            Pattern::Q4Lit(q) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = icmp eq i8 {}, {}",
                    val.reg,
                    q.value() as i8
                ));
                Ok(r)
            }
            Pattern::TritLit(t) => {
                let r = self.fresh_reg();
                self.line(&format!("  {r} = icmp eq i8 {}, {}", val.reg, t.value()));
                Ok(r)
            }
            Pattern::BytesLit(bs) => {
                let bh = self.emit_bytes_handle(cg, bs);
                let eq8 = self.fresh_reg();
                self.line(&format!(
                    "  {eq8} = call i8 @aod_rt_handle_eq(ptr %ctx, i64 {}, i64 {bh})",
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
                    "  {cmp} = call i8 @__aod_ratio_cmp(ptr %ctx, %DecRatio {}, %DecRatio {})",
                    val.reg, lit.reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = icmp eq i8 {cmp}, 0"));
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

    fn emit_ratio_num_or_den(&mut self, rat_reg: &str, is_den: bool) -> Val {
        let (na, da) = self.extract_ratio_words(rat_reg);
        let ia = self.fresh_reg();
        let id = self.fresh_reg();
        let dz = self.fresh_reg();
        let c1 = self.fresh_reg();
        let ok = self.fresh_reg();
        let fast_lbl = self.fresh_label("rat_proj_inl");
        let slow_lbl = self.fresh_label("rat_proj_slow");
        let join_lbl = self.fresh_label("rat_proj_join");
        let slot = self.alloc_temp("i64");
        self.line(&format!("  {ia} = icmp ult i64 {na}, -4611686018427387904"));
        self.line(&format!("  {id} = icmp ult i64 {da}, -4611686018427387904"));
        self.line(&format!("  {dz} = icmp ne i64 {da}, 0"));
        self.line(&format!("  {c1} = and i1 {ia}, {id}"));
        self.line(&format!("  {ok} = and i1 {c1}, {dz}"));
        self.line(&format!(
            "  br i1 {ok}, label %{fast_lbl}, label %{slow_lbl}"
        ));
        self.start_block(&fast_lbl);
        let fast_w = if is_den { &da } else { &na };
        self.line(&format!("  store i64 {fast_w}, ptr {slot}"));
        self.line(&format!("  br label %{join_lbl}"));
        self.start_block(&slow_lbl);
        let rt_fn = if is_den {
            "@aod_rt_ratio_den"
        } else {
            "@aod_rt_ratio_num"
        };
        let sw = self.fresh_reg();
        self.line(&format!(
            "  {sw} = call i64 {rt_fn}(ptr %ctx, i64 {na}, i64 {da})"
        ));
        self.emit_fail_return_check();
        self.line(&format!("  store i64 {sw}, ptr {slot}"));
        self.line(&format!("  br label %{join_lbl}"));
        self.start_block(&join_lbl);
        let r = self.fresh_reg();
        self.line(&format!("  {r} = load i64, ptr {slot}"));
        Val {
            reg: r,
            dec_ty: Type::Int,
        }
    }

    fn emit_bigint_lit(&mut self, cg: &mut Codegen<'_>, n: &BigInt) -> Val {
        if let Some(w) = n.inline_word() {
            Val {
                reg: (w as i64).to_string(),
                dec_ty: Type::Int,
            }
        } else {
            let dec_s = n.to_decimal_string();
            let (g, len) = cg.intern_bytes(dec_s.as_bytes());
            let r = self.fresh_reg();
            self.line(&format!(
                "  {r} = call i64 @aod_rt_int_from_str(ptr %ctx, ptr {g}, i64 {len})"
            ));
            self.emit_fail_return_check();
            Val {
                reg: r,
                dec_ty: Type::Int,
            }
        }
    }

    fn emit_ratio_lit(&mut self, cg: &mut Codegen<'_>, rat: &Ratio) -> Val {
        if let Some((nw, dw)) = rat.inline_words() {
            Val {
                reg: format!("{{ i64 {}, i64 {} }}", nw as i64, dw as i64),
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
        self.emit_expr_with_hint(cg, expr, None)
    }

    fn emit_expr_with_hint(
        &mut self,
        cg: &mut Codegen<'_>,
        expr: &Expr,
        hint: Option<&Type>,
    ) -> DecResult<Val> {
        match &expr.kind {
            ExprKind::NullLit => Ok(Val {
                reg: (TAG_NULL_SENTINEL as i64).to_string(),
                dec_ty: Type::Null,
            }),
            ExprKind::IntLit(n) => Ok(self.emit_bigint_lit(cg, n)),
            ExprKind::BoolLit(b) => Ok(Val {
                reg: if *b { "true" } else { "false" }.to_string(),
                dec_ty: Type::Bool,
            }),
            ExprKind::BytesLit(bs) => {
                let h = self.emit_bytes_handle(cg, bs);
                Ok(Val {
                    reg: h,
                    dec_ty: Type::Bytes,
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
            ExprKind::TritLit(tl) => Ok(Val {
                reg: tl.to_t1().value().to_string(),
                dec_ty: Type::T1,
            }),
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
                if let Some(cm) = cg.consts.get(&(base.clone(), name.clone())).cloned() {
                    return Ok(self.load_const(&cm));
                }
                if let Some(ImportedSymbol::ModuleItem { module, item }) =
                    cg.imports_for(&self.mod_key).get(name).cloned()
                {
                    if let Some(cm) = cg.consts.get(&(module, item)).cloned() {
                        return Ok(self.load_const(&cm));
                    }
                }
                for (m, qm) in &cg.qext_modules {
                    if m == &base {
                        let sub = format!("{m}::{qm}");
                        if let Some(cm) = cg.consts.get(&(sub, name.clone())).cloned() {
                            return Ok(self.load_const(&cm));
                        }
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
            ExprKind::SeqLit(items) => {
                let elem_hint: Option<&Type> = match hint.map(Type::unwrap_read_only) {
                    Some(Type::Array(e, _) | Type::List(e)) => Some(e),
                    _ => None,
                };
                if items.is_empty() {
                    let et = elem_hint.cloned().unwrap_or(Type::Unknown);
                    let h = self.emit_handle_array_call("aod_rt_list_new", &[]);
                    return Ok(Val {
                        reg: h,
                        dec_ty: Type::List(Box::new(et)),
                    });
                }
                let mut elem_ty = elem_hint.cloned().unwrap_or(Type::Unknown);
                let mut handles = Vec::with_capacity(items.len());
                for it in items {
                    let v = self.emit_expr_with_hint(cg, it, elem_hint)?;
                    if elem_ty == Type::Unknown {
                        elem_ty = v.dec_ty.clone();
                    }
                    handles.push(self.box_val_to_handle(&v));
                }
                let want_list = matches!(hint.map(Type::unwrap_read_only), Some(Type::List(_)));
                let h = if want_list {
                    self.emit_handle_array_call("aod_rt_list_new", &handles)
                } else {
                    self.emit_handle_array_call("aod_rt_array_new", &handles)
                };
                let out_ty = if want_list {
                    Type::List(Box::new(elem_ty))
                } else {
                    Type::Array(Box::new(elem_ty), items.len())
                };
                Ok(Val {
                    reg: h,
                    dec_ty: out_ty,
                })
            }
            ExprKind::Tuple(elems) => {
                if elems.is_empty() {
                    return Ok(Val {
                        reg: String::new(),
                        dec_ty: Type::Unit,
                    });
                }
                let hint_elems: Option<&[Type]> = match hint.map(Type::unwrap_read_only) {
                    Some(Type::Tuple(ts)) if ts.len() == elems.len() => Some(ts.as_slice()),
                    _ => None,
                };
                let mut tys = Vec::with_capacity(elems.len());
                let mut handles = Vec::with_capacity(elems.len());
                for (i, e) in elems.iter().enumerate() {
                    let eh = hint_elems.map(|ts| &ts[i]);
                    let v = self.emit_expr_with_hint(cg, e, eh)?;
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
                let (k_hint, v_hint) = match hint.map(Type::unwrap_read_only) {
                    Some(Type::Map(k, v)) => (Some(k.as_ref()), Some(v.as_ref())),
                    _ => (None, None),
                };
                let mut kt = k_hint.cloned().unwrap_or(Type::Unknown);
                let mut vt = v_hint.cloned().unwrap_or(Type::Unknown);
                let mut khs = Vec::with_capacity(entries.len());
                let mut vhs = Vec::with_capacity(entries.len());
                for (ke, ve) in entries {
                    let kv = self.emit_expr_with_hint(cg, ke, k_hint)?;
                    let vv = self.emit_expr_with_hint(cg, ve, v_hint)?;
                    if kt == Type::Unknown {
                        kt = kv.dec_ty.clone();
                    }
                    if vt == Type::Unknown {
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
            ExprKind::WindowLit { start, end } => {
                let u = self.emit_expr(cg, start)?;
                let v = self.emit_expr(cg, end)?;
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_window_new(ptr %ctx, i64 {}, i64 {})",
                    u.reg, v.reg
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
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_range_new(ptr %ctx, i64 {}, i64 {})",
                    a.reg, b.reg
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
                self.emit_unary(cg, *op, v, expr.line)
            }
            ExprKind::Adverb { op, operand } => self.emit_adverb_expr(cg, *op, operand, expr.line),
            ExprKind::Binary { op, left, right } => {
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    let lv = self.emit_expr(cg, left)?;
                    if matches!(lv.dec_ty.unwrap_read_only(), Type::Bool)
                        || matches!(lv.dec_ty.unwrap_read_only(), Type::QExt(inner) if **inner == Type::Bool)
                    {
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
                    let rv = self.emit_expr(cg, right)?;
                    return self.emit_binary(cg, *op, lv, rv, expr.line);
                }
                let lv = self.emit_expr(cg, left)?;
                let rv = self.emit_expr(cg, right)?;
                self.emit_binary(cg, *op, lv, rv, expr.line)
            }
            ExprKind::Pipe { input, stage } => {
                self.emit_pipe_expr(cg, input, stage, hint, expr.line)
            }
            ExprKind::Match { scrutinee, arms } => {
                self.emit_match_expr(cg, scrutinee, arms, hint, expr.line)
            }
            ExprKind::Field { base, name } => {
                let bv = self.emit_expr(cg, base)?;
                let base_ro = bv.dec_ty.is_read_only();
                match bv.dec_ty.unwrap_read_only().clone() {
                    Type::Ratio if name == "num" || name == "den" => {
                        Ok(self.emit_ratio_num_or_den(&bv.reg, name == "den"))
                    }
                    Type::Array(elem_ty, n) => {
                        let mut ft = self.resolve_field_type(cg, &elem_ty, name, expr.line)?;
                        if base_ro && ft.needs_readonly_propagation() {
                            ft = ft.wrap_read_only();
                        }
                        let (fg, flen) = cg.intern_bytes(name.as_bytes());
                        let fh = self.fresh_reg();
                        self.line(&format!(
                            "  {fh} = call i64 @aod_rt_field_get(ptr %ctx, i64 {}, ptr {fg}, i64 {flen})",
                            bv.reg
                        ));
                        self.emit_fail_return_check();
                        let out_ty = Type::Array(Box::new(ft), n);
                        Ok(Val {
                            reg: fh,
                            dec_ty: if base_ro { out_ty.wrap_read_only() } else { out_ty },
                        })
                    }
                    Type::List(elem_ty) => {
                        let mut ft = self.resolve_field_type(cg, &elem_ty, name, expr.line)?;
                        if base_ro && ft.needs_readonly_propagation() {
                            ft = ft.wrap_read_only();
                        }
                        let (fg, flen) = cg.intern_bytes(name.as_bytes());
                        let fh = self.fresh_reg();
                        self.line(&format!(
                            "  {fh} = call i64 @aod_rt_field_get(ptr %ctx, i64 {}, ptr {fg}, i64 {flen})",
                            bv.reg
                        ));
                        self.emit_fail_return_check();
                        let out_ty = Type::List(Box::new(ft));
                        Ok(Val {
                            reg: fh,
                            dec_ty: if base_ro { out_ty.wrap_read_only() } else { out_ty },
                        })
                    }
                    _ => {
                        let mut field_ty =
                            self.resolve_field_type(cg, &bv.dec_ty, name, expr.line)?;
                        if base_ro && field_ty.needs_readonly_propagation() {
                            field_ty = field_ty.wrap_read_only();
                        }
                        let (fg, flen) = cg.intern_bytes(name.as_bytes());
                        let fh = self.fresh_reg();
                        self.line(&format!(
                            "  {fh} = call i64 @aod_rt_field_get(ptr %ctx, i64 {}, ptr {fg}, i64 {flen})",
                            bv.reg
                        ));
                        self.emit_fail_return_check();
                        Ok(self.unbox_handle_to_val(&fh, &field_ty))
                    }
                }
            }
            ExprKind::Index { base, index } => {
                let bv = self.emit_expr(cg, base)?;
                let iv = self.emit_expr(cg, index)?;
                match bv.dec_ty.unwrap_read_only().clone() {
                    Type::Array(elem_ty, _) | Type::List(elem_ty) => {
                        let eh = self.fresh_reg();
                        self.line(&format!(
                            "  {eh} = call i64 @aod_rt_seq_get(ptr %ctx, i64 {}, i64 {})",
                            bv.reg, iv.reg
                        ));
                        self.emit_fail_return_check();
                        Ok(self.unbox_handle_to_val(&eh, &elem_ty))
                    }
                    Type::Bytes => {
                        let b = self.fresh_reg();
                        self.line(&format!(
                            "  {b} = call i8 @aod_rt_bytes_get(ptr %ctx, i64 {}, i64 {})",
                            bv.reg, iv.reg
                        ));
                        self.emit_fail_return_check();
                        Ok(Val {
                            reg: b,
                            dec_ty: Type::B8,
                        })
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
            ExprKind::Call { callee, args } => {
                self.emit_call_with_prepended(cg, None, callee, args, hint, expr.line)
            }
        }
    }

    fn emit_match_expr(
        &mut self,
        cg: &mut Codegen<'_>,
        scrutinee: &Expr,
        arms: &[MatchExprArm],
        hint: Option<&Type>,
        line: usize,
    ) -> DecResult<Val> {
        let sval = self.emit_expr(cg, scrutinee)?;
        let merge_lbl = self.fresh_label("match_merge");
        let mut res_slot: Option<(String, Type)> = None;

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
            let effective_hint = res_slot.as_ref().map(|(_, t)| t).or(hint).cloned();
            let body_val = self.emit_expr_with_hint(cg, &arm.body, effective_hint.as_ref())?;
            if res_slot.is_none() {
                let target_ty = if type_contains_unknown(&body_val.dec_ty) {
                    hint.cloned().unwrap_or(body_val.dec_ty.clone())
                } else {
                    body_val.dec_ty.clone()
                };
                let lt = LlvmTy::from_dec(&target_ty);
                let slot = if lt != LlvmTy::Void {
                    self.alloc_temp(lt.ir())
                } else {
                    String::new()
                };
                res_slot = Some((slot, target_ty));
            }
            if let Some((ref slot, ref target_ty)) = res_slot {
                let lt = LlvmTy::from_dec(target_ty);
                if lt != LlvmTy::Void && !self.terminated {
                    self.line(&format!("  store {} {}, ptr {slot}", lt.ir(), body_val.reg));
                }
            }
            self.pop_scope();
            if !self.terminated {
                self.line(&format!("  br label %{merge_lbl}"));
                self.terminated = true;
            }
            self.start_block(&next_lbl);
        }

        if !self.terminated {
            let (mg, ml) = cg.intern_bytes(b"non-exhaustive match at runtime");
            self.line(&format!(
                "  call void @aod_rt_fail(ptr %ctx, ptr {mg}, i64 {ml})"
            ));
            self.emit_depth_dec();
            self.emit_default_ret();
        }
        self.start_block(&merge_lbl);

        let Some((slot, out_ty)) = res_slot else {
            return Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                "codegen: empty match arms",
            ));
        };
        let lt = LlvmTy::from_dec(&out_ty);
        if lt == LlvmTy::Void {
            Ok(Val {
                reg: String::new(),
                dec_ty: out_ty,
            })
        } else {
            let r = self.fresh_reg();
            self.line(&format!("  {r} = load {}, ptr {slot}", lt.ir()));
            Ok(Val {
                reg: r,
                dec_ty: out_ty,
            })
        }
    }

    fn emit_pipe_expr(
        &mut self,
        cg: &mut Codegen<'_>,
        input: &Expr,
        stage: &PipeStage,
        hint: Option<&Type>,
        line: usize,
    ) -> DecResult<Val> {
        // Stream-fusion fast path: `(mask ?> vals) |> fold_adverb` or `(a..b) |> fold_adverb`
        if let PipeStage::Adverb(op) = stage {
            return self.emit_adverb_expr(cg, *op, input, line);
        }
        let in_val = self.emit_expr(cg, input)?;
        match stage {
            PipeStage::Adverb(_) => unreachable!(),
            PipeStage::Boundary => self.emit_unary(cg, UnaryOp::Boundary, in_val, line),
            PipeStage::Section { op, rhs } => {
                let lv = self.materialize_if_range(in_val);
                let rv = self.emit_expr(cg, rhs)?;
                self.emit_binary(cg, *op, lv, rv, line)
            }
            PipeStage::Call { callee, extra_args } => {
                self.emit_call_with_prepended(cg, Some(in_val), callee, extra_args, hint, line)
            }
        }
    }

    fn materialize_if_range(&mut self, v: Val) -> Val {
        if *v.dec_ty.unwrap_read_only() == Type::Range {
            let lh = self.fresh_reg();
            self.line(&format!(
                "  {lh} = call i64 @aod_rt_range_to_list(ptr %ctx, i64 {})",
                v.reg
            ));
            self.emit_fail_return_check();
            Val {
                reg: lh,
                dec_ty: Type::List(Box::new(Type::Int)),
            }
        } else {
            v
        }
    }

    // ==================== Adverbs & Stream Fusion ====================

    fn emit_adverb_expr(
        &mut self,
        cg: &mut Codegen<'_>,
        op: AdverbOp,
        operand: &Expr,
        line: usize,
    ) -> DecResult<Val> {
        let is_scan = matches!(
            op,
            AdverbOp::ScanAdd | AdverbOp::ScanMul | AdverbOp::ScanMin | AdverbOp::ScanMax
        );

        // Stream Fusion 1: `(mask ?> vals) |> fold_adverb` with ZERO intermediate list allocation!
        if !is_scan && !matches!(op, AdverbOp::FoldMin | AdverbOp::FoldMax) {
            if let ExprKind::Binary {
                op: BinaryOp::Sieve,
                left: mask_expr,
                right: vals_expr,
            } = &operand.kind
            {
                let mv = self.emit_expr(cg, mask_expr)?;
                let vv = self.emit_expr(cg, vals_expr)?;
                let mv = self.materialize_if_range(mv);
                let vv = self.materialize_if_range(vv);
                let elem_ty = seq_elem_type(&vv.dec_ty).unwrap_or(Type::Int);
                return self.emit_fused_sieve_fold(cg, op, &mv.reg, &vv.reg, &elem_ty, line);
            }
        }

        // Stream Fusion 2: `(start..end) |> fold_adverb` in pure unboxed i64 registers!
        if matches!(op, AdverbOp::FoldAdd | AdverbOp::FoldMul) {
            if let ExprKind::Range { start, end } = &operand.kind {
                let sv = self.emit_expr(cg, start)?;
                let ev = self.emit_expr(cg, end)?;
                return self.emit_fused_range_fold(cg, op, &sv.reg, &ev.reg);
            }
        }

        let seq_val = self.emit_expr(cg, operand)?;
        let seq_val = self.materialize_if_range(seq_val);
        let (elem_ty, fixed_len) = match seq_val.dec_ty.unwrap_read_only().clone() {
            Type::Array(e, n) => ((*e).clone(), Some(n)),
            Type::List(e) => ((*e).clone(), None),
            other => {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("codegen: adverb expects sequence, got {other}"),
                ));
            }
        };

        if is_scan {
            self.emit_scan_loop(cg, op, &seq_val.reg, &elem_ty, fixed_len, line)
        } else {
            self.emit_fold_loop(cg, op, &seq_val.reg, &elem_ty, line)
        }
    }

    fn emit_fused_range_fold(
        &mut self,
        cg: &mut Codegen<'_>,
        op: AdverbOp,
        start_reg: &str,
        end_reg: &str,
    ) -> DecResult<Val> {
        let rh = self.fresh_reg();
        self.line(&format!(
            "  {rh} = call i64 @aod_rt_range_new(ptr %ctx, i64 {start_reg}, i64 {end_reg})"
        ));
        self.emit_fail_return_check();
        let s_ptr = self.alloc_temp("i64");
        let e_ptr = self.alloc_temp("i64");
        self.line(&format!(
            "  call void @aod_rt_range_bounds(ptr %ctx, i64 {rh}, ptr {s_ptr}, ptr {e_ptr})"
        ));
        self.emit_fail_return_check();
        let s_i64 = self.fresh_reg();
        let e_i64 = self.fresh_reg();
        self.line(&format!("  {s_i64} = load i64, ptr {s_ptr}"));
        self.line(&format!("  {e_i64} = load i64, ptr {e_ptr}"));

        let init_word = if op == AdverbOp::FoldAdd { "0" } else { "1" };
        let acc_slot = self.alloc_temp("i64");
        let idx_slot = self.alloc_temp("i64");
        self.line(&format!("  store i64 {init_word}, ptr {acc_slot}"));
        self.line(&format!("  store i64 {s_i64}, ptr {idx_slot}"));

        let cond_lbl = self.fresh_label("rfold_cond");
        let body_lbl = self.fresh_label("rfold_body");
        let end_lbl = self.fresh_label("rfold_end");
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
        let item_w = self.fresh_reg();
        self.line(&format!(
            "  {item_w} = call i64 @aod_rt_int_from_i64(ptr %ctx, i64 {cur})"
        ));
        let prev_acc = self.fresh_reg();
        self.line(&format!("  {prev_acc} = load i64, ptr {acc_slot}"));
        let helper = if op == AdverbOp::FoldAdd {
            "@__aod_int_add"
        } else {
            "@__aod_int_mul"
        };
        let next_acc = self.fresh_reg();
        self.line(&format!(
            "  {next_acc} = call i64 {helper}(ptr %ctx, i64 {prev_acc}, i64 {item_w})"
        ));
        self.emit_fail_return_check();
        self.line(&format!("  store i64 {next_acc}, ptr {acc_slot}"));
        let nxt = self.fresh_reg();
        self.line(&format!("  {nxt} = add i64 {cur}, 1"));
        self.line(&format!("  store i64 {nxt}, ptr {idx_slot}"));
        let md_suffix = cg.alloc_loop_hint_md().unwrap_or_default();
        self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
        self.terminated = true;

        self.start_block(&end_lbl);
        let res = self.fresh_reg();
        self.line(&format!("  {res} = load i64, ptr {acc_slot}"));
        Ok(Val {
            reg: res,
            dec_ty: Type::Int,
        })
    }

    fn emit_fused_sieve_fold(
        &mut self,
        cg: &mut Codegen<'_>,
        op: AdverbOp,
        mask_h: &str,
        vals_h: &str,
        elem_ty: &Type,
        line: usize,
    ) -> DecResult<Val> {
        let ml = self.fresh_reg();
        let vl = self.fresh_reg();
        self.line(&format!(
            "  {ml} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {mask_h})"
        ));
        self.line(&format!(
            "  {vl} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {vals_h})"
        ));
        self.emit_fail_return_check();
        let len_eq = self.fresh_reg();
        self.line(&format!("  {len_eq} = icmp eq i64 {ml}, {vl}"));
        let ok_len = self.fresh_label("sieve_len_ok");
        let bad_len = self.fresh_label("sieve_len_bad");
        self.line(&format!(
            "  br i1 {len_eq}, label %{ok_len}, label %{bad_len}"
        ));
        self.start_block(&bad_len);
        let (mg, mlen) = cg.intern_bytes(b"sieve length mismatch");
        self.line(&format!(
            "  call void @aod_rt_fail(ptr %ctx, ptr {mg}, i64 {mlen})"
        ));
        self.emit_depth_dec();
        self.emit_default_ret();

        self.start_block(&ok_len);
        let init_val = self.emit_adverb_identity(cg, op, elem_ty, line)?;
        let lt = LlvmTy::from_dec(&init_val.dec_ty);
        let acc_slot = self.alloc_temp(lt.ir());
        let idx_slot = self.alloc_temp("i64");
        self.line(&format!(
            "  store {} {}, ptr {acc_slot}",
            lt.ir(),
            init_val.reg
        ));
        self.line(&format!("  store i64 0, ptr {idx_slot}"));

        let cond_lbl = self.fresh_label("sfold_cond");
        let body_lbl = self.fresh_label("sfold_body");
        let take_lbl = self.fresh_label("sfold_take");
        let step_lbl = self.fresh_label("sfold_step");
        let end_lbl = self.fresh_label("sfold_end");
        self.line(&format!("  br label %{cond_lbl}"));

        self.start_block(&cond_lbl);
        let idx = self.fresh_reg();
        let cmp = self.fresh_reg();
        self.line(&format!("  {idx} = load i64, ptr {idx_slot}"));
        self.line(&format!("  {cmp} = icmp slt i64 {idx}, {vl}"));
        self.line(&format!(
            "  br i1 {cmp}, label %{body_lbl}, label %{end_lbl}"
        ));

        self.start_block(&body_lbl);
        self.emit_fuel_tick();
        let mh_i = self.fresh_reg();
        self.line(&format!(
            "  {mh_i} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {mask_h}, i64 {idx})"
        ));
        let mv_i = self.unbox_handle_to_val(&mh_i, &Type::Bool);
        self.line(&format!(
            "  br i1 {}, label %{take_lbl}, label %{step_lbl}",
            mv_i.reg
        ));

        self.start_block(&take_lbl);
        let vh_i = self.fresh_reg();
        self.line(&format!(
            "  {vh_i} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {vals_h}, i64 {idx})"
        ));
        let item_v = self.unbox_handle_to_val(&vh_i, &init_val.dec_ty);
        let cur_acc = self.fresh_reg();
        self.line(&format!("  {cur_acc} = load {}, ptr {acc_slot}", lt.ir()));
        let acc_v = Val {
            reg: cur_acc,
            dec_ty: init_val.dec_ty.clone(),
        };
        let next_v = self.emit_adverb_step(cg, op, acc_v, item_v, line)?;
        self.line(&format!(
            "  store {} {}, ptr {acc_slot}",
            lt.ir(),
            next_v.reg
        ));
        self.line(&format!("  br label %{step_lbl}"));

        self.start_block(&step_lbl);
        let nxt = self.fresh_reg();
        self.line(&format!("  {nxt} = add nsw i64 {idx}, 1"));
        self.line(&format!("  store i64 {nxt}, ptr {idx_slot}"));
        let md_suffix = cg.alloc_loop_hint_md().unwrap_or_default();
        self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
        self.terminated = true;

        self.start_block(&end_lbl);
        let res = self.fresh_reg();
        self.line(&format!("  {res} = load {}, ptr {acc_slot}", lt.ir()));
        Ok(Val {
            reg: res,
            dec_ty: init_val.dec_ty,
        })
    }

    fn emit_adverb_identity(
        &mut self,
        cg: &mut Codegen<'_>,
        op: AdverbOp,
        elem_ty: &Type,
        line: usize,
    ) -> DecResult<Val> {
        let base_ty = if *elem_ty.unwrap_read_only() == Type::Unknown {
            match op {
                AdverbOp::FoldAdd | AdverbOp::FoldMul => Type::Int,
                AdverbOp::FoldAnd | AdverbOp::FoldOr => Type::Bool,
                AdverbOp::FoldSuperpose => Type::T1,
                _ => Type::Int,
            }
        } else {
            elem_ty.unwrap_read_only().clone()
        };
        match (op, &base_ty) {
            (AdverbOp::FoldAdd, Type::Int) => Ok(Val {
                reg: "0".to_string(),
                dec_ty: Type::Int,
            }),
            (AdverbOp::FoldAdd, Type::Ratio) => Ok(Val {
                reg: "{ i64 0, i64 1 }".to_string(),
                dec_ty: Type::Ratio,
            }),
            (AdverbOp::FoldAdd, Type::T1) => Ok(Val {
                reg: "0".to_string(),
                dec_ty: Type::T1,
            }),
            (AdverbOp::FoldAdd, Type::QExt(inner)) if **inner == Type::Ratio => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_qext_from_ratio(ptr %ctx, i64 0, i64 1)"
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::QExt(Box::new(Type::Ratio)),
                })
            }
            (AdverbOp::FoldMul, Type::Int) => Ok(Val {
                reg: "1".to_string(),
                dec_ty: Type::Int,
            }),
            (AdverbOp::FoldMul, Type::Ratio) => Ok(Val {
                reg: "{ i64 1, i64 1 }".to_string(),
                dec_ty: Type::Ratio,
            }),
            (AdverbOp::FoldMul, Type::T1) => Ok(Val {
                reg: "1".to_string(),
                dec_ty: Type::T1,
            }),
            (AdverbOp::FoldMul, Type::QExt(inner)) if **inner == Type::Ratio => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_qext_from_ratio(ptr %ctx, i64 1, i64 1)"
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::QExt(Box::new(Type::Ratio)),
                })
            }
            (AdverbOp::FoldAnd, Type::Bool) => Ok(Val {
                reg: "true".to_string(),
                dec_ty: Type::Bool,
            }),
            (AdverbOp::FoldAnd, Type::B8) => Ok(Val {
                reg: "-1".to_string(),
                dec_ty: Type::B8,
            }),
            (AdverbOp::FoldAnd, Type::B64) => Ok(Val {
                reg: "-1".to_string(),
                dec_ty: Type::B64,
            }),
            (AdverbOp::FoldAnd, Type::Q4) => Ok(Val {
                reg: "15".to_string(),
                dec_ty: Type::Q4,
            }),
            (AdverbOp::FoldOr, Type::Bool) => Ok(Val {
                reg: "false".to_string(),
                dec_ty: Type::Bool,
            }),
            (AdverbOp::FoldOr, Type::B8) => Ok(Val {
                reg: "0".to_string(),
                dec_ty: Type::B8,
            }),
            (AdverbOp::FoldOr, Type::B64) => Ok(Val {
                reg: "0".to_string(),
                dec_ty: Type::B64,
            }),
            (AdverbOp::FoldOr, Type::Q4) => Ok(Val {
                reg: "0".to_string(),
                dec_ty: Type::Q4,
            }),
            (AdverbOp::FoldSuperpose, Type::T1) => Ok(Val {
                reg: "0".to_string(),
                dec_ty: Type::T1,
            }),
            (AdverbOp::FoldSuperpose, Type::Int) => Ok(Val {
                reg: "0".to_string(),
                dec_ty: Type::Int,
            }),
            _ => {
                let _ = cg;
                Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("codegen: unsupported adverb identity for {base_ty}"),
                ))
            }
        }
    }

    fn emit_adverb_step(
        &mut self,
        cg: &mut Codegen<'_>,
        op: AdverbOp,
        acc: Val,
        item: Val,
        line: usize,
    ) -> DecResult<Val> {
        match op {
            AdverbOp::FoldAdd | AdverbOp::ScanAdd => {
                self.emit_binary(cg, BinaryOp::Add, acc, item, line)
            }
            AdverbOp::FoldMul | AdverbOp::ScanMul => {
                self.emit_binary(cg, BinaryOp::Mul, acc, item, line)
            }
            AdverbOp::FoldMin | AdverbOp::ScanMin => {
                self.emit_binary(cg, BinaryOp::Min, acc, item, line)
            }
            AdverbOp::FoldMax | AdverbOp::ScanMax => {
                self.emit_binary(cg, BinaryOp::Max, acc, item, line)
            }
            AdverbOp::FoldAnd => {
                if *acc.dec_ty.unwrap_read_only() == Type::Bool {
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = and i1 {}, {}", acc.reg, item.reg));
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::Bool,
                    })
                } else {
                    self.emit_binary(cg, BinaryOp::BitAnd, acc, item, line)
                }
            }
            AdverbOp::FoldOr => {
                if *acc.dec_ty.unwrap_read_only() == Type::Bool {
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = or i1 {}, {}", acc.reg, item.reg));
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::Bool,
                    })
                } else {
                    self.emit_binary(cg, BinaryOp::BitOr, acc, item, line)
                }
            }
            AdverbOp::FoldSuperpose => self.emit_builtin("superpose_cancel", vec![acc, item], line),
        }
    }

    fn emit_fold_loop(
        &mut self,
        cg: &mut Codegen<'_>,
        op: AdverbOp,
        seq_h: &str,
        elem_ty: &Type,
        line: usize,
    ) -> DecResult<Val> {
        let len = self.fresh_reg();
        self.line(&format!(
            "  {len} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {seq_h})"
        ));
        self.emit_fail_return_check();

        let out_ty = if *elem_ty.unwrap_read_only() == Type::Unknown {
            match op {
                AdverbOp::FoldAdd | AdverbOp::FoldMul => Type::Int,
                AdverbOp::FoldAnd | AdverbOp::FoldOr => Type::Bool,
                AdverbOp::FoldSuperpose => Type::T1,
                _ => Type::Int,
            }
        } else {
            elem_ty.unwrap_read_only().clone()
        };
        let lt = LlvmTy::from_dec(&out_ty);
        let acc_slot = self.alloc_temp(lt.ir());
        let idx_slot = self.alloc_temp("i64");

        let is_empty = self.fresh_reg();
        let empty_lbl = self.fresh_label("fold_empty");
        let non_empty_lbl = self.fresh_label("fold_nonempty");
        let cond_lbl = self.fresh_label("fold_cond");
        let body_lbl = self.fresh_label("fold_body");
        let done_lbl = self.fresh_label("fold_done");
        self.line(&format!("  {is_empty} = icmp sle i64 {len}, 0"));
        self.line(&format!(
            "  br i1 {is_empty}, label %{empty_lbl}, label %{non_empty_lbl}"
        ));

        self.start_block(&empty_lbl);
        if matches!(op, AdverbOp::FoldMin | AdverbOp::FoldMax) {
            let (mg, ml) = cg.intern_bytes(b"min/ and max/ require non-empty sequence");
            self.line(&format!(
                "  call void @aod_rt_fail(ptr %ctx, ptr {mg}, i64 {ml})"
            ));
            self.emit_depth_dec();
            self.emit_default_ret();
        } else {
            let id_val = self.emit_adverb_identity(cg, op, &out_ty, line)?;
            self.line(&format!(
                "  store {} {}, ptr {acc_slot}",
                lt.ir(),
                id_val.reg
            ));
            self.line(&format!("  br label %{done_lbl}"));
            self.terminated = true;
        }

        self.start_block(&non_empty_lbl);
        let h0 = self.fresh_reg();
        self.line(&format!(
            "  {h0} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {seq_h}, i64 0)"
        ));
        let v0 = self.unbox_handle_to_val(&h0, &out_ty);
        self.line(&format!("  store {} {}, ptr {acc_slot}", lt.ir(), v0.reg));
        self.line(&format!("  store i64 1, ptr {idx_slot}"));
        self.line(&format!("  br label %{cond_lbl}"));

        self.start_block(&cond_lbl);
        let idx = self.fresh_reg();
        let cmp = self.fresh_reg();
        self.line(&format!("  {idx} = load i64, ptr {idx_slot}"));
        self.line(&format!("  {cmp} = icmp slt i64 {idx}, {len}"));
        self.line(&format!(
            "  br i1 {cmp}, label %{body_lbl}, label %{done_lbl}"
        ));

        self.start_block(&body_lbl);
        self.emit_fuel_tick();
        let hi = self.fresh_reg();
        self.line(&format!(
            "  {hi} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {seq_h}, i64 {idx})"
        ));
        let vi = self.unbox_handle_to_val(&hi, &out_ty);
        let cur = self.fresh_reg();
        self.line(&format!("  {cur} = load {}, ptr {acc_slot}", lt.ir()));
        let acc_v = Val {
            reg: cur,
            dec_ty: out_ty.clone(),
        };
        let next_v = self.emit_adverb_step(cg, op, acc_v, vi, line)?;
        self.line(&format!(
            "  store {} {}, ptr {acc_slot}",
            lt.ir(),
            next_v.reg
        ));
        let nxt = self.fresh_reg();
        self.line(&format!("  {nxt} = add nsw i64 {idx}, 1"));
        self.line(&format!("  store i64 {nxt}, ptr {idx_slot}"));
        let md_suffix = cg.alloc_loop_hint_md().unwrap_or_default();
        self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
        self.terminated = true;

        self.start_block(&done_lbl);
        let res = self.fresh_reg();
        self.line(&format!("  {res} = load {}, ptr {acc_slot}", lt.ir()));
        Ok(Val {
            reg: res,
            dec_ty: out_ty,
        })
    }

    fn emit_scan_loop(
        &mut self,
        cg: &mut Codegen<'_>,
        op: AdverbOp,
        seq_h: &str,
        elem_ty: &Type,
        fixed_len: Option<usize>,
        line: usize,
    ) -> DecResult<Val> {
        let len = self.fresh_reg();
        self.line(&format!(
            "  {len} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {seq_h})"
        ));
        self.emit_fail_return_check();

        let out_elem = if *elem_ty.unwrap_read_only() == Type::Unknown {
            Type::Int
        } else {
            elem_ty.unwrap_read_only().clone()
        };
        let lt = LlvmTy::from_dec(&out_elem);
        let acc_slot = self.alloc_temp(lt.ir());
        let idx_slot = self.alloc_temp("i64");
        let res_h_slot = self.alloc_temp("i64");

        let is_empty = self.fresh_reg();
        let empty_lbl = self.fresh_label("scan_empty");
        let non_empty_lbl = self.fresh_label("scan_nonempty");
        let cond_lbl = self.fresh_label("scan_cond");
        let body_lbl = self.fresh_label("scan_body");
        let finish_lbl = self.fresh_label("scan_finish");
        let done_lbl = self.fresh_label("scan_done");
        self.line(&format!("  {is_empty} = icmp sle i64 {len}, 0"));
        self.line(&format!(
            "  br i1 {is_empty}, label %{empty_lbl}, label %{non_empty_lbl}"
        ));

        self.start_block(&empty_lbl);
        let empty_h = self.emit_handle_array_call("aod_rt_list_new", &[]);
        self.line(&format!("  store i64 {empty_h}, ptr {res_h_slot}"));
        self.line(&format!("  br label %{done_lbl}"));

        self.start_block(&non_empty_lbl);
        let buf_id = self.fresh_reg();
        self.line(&format!(
            "  {buf_id} = call i64 @aod_rt_seq_buf_new(ptr %ctx, i64 {len})"
        ));
        let h0 = self.fresh_reg();
        self.line(&format!(
            "  {h0} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {seq_h}, i64 0)"
        ));
        let v0 = self.unbox_handle_to_val(&h0, &out_elem);
        self.line(&format!("  store {} {}, ptr {acc_slot}", lt.ir(), v0.reg));
        let bh0 = self.box_val_to_handle(&v0);
        self.line(&format!(
            "  call void @aod_rt_seq_buf_push(ptr %ctx, i64 {buf_id}, i64 {bh0})"
        ));
        self.line(&format!("  store i64 1, ptr {idx_slot}"));
        self.line(&format!("  br label %{cond_lbl}"));

        self.start_block(&cond_lbl);
        let idx = self.fresh_reg();
        let cmp = self.fresh_reg();
        self.line(&format!("  {idx} = load i64, ptr {idx_slot}"));
        self.line(&format!("  {cmp} = icmp slt i64 {idx}, {len}"));
        self.line(&format!(
            "  br i1 {cmp}, label %{body_lbl}, label %{finish_lbl}"
        ));

        self.start_block(&body_lbl);
        self.emit_fuel_tick();
        let hi = self.fresh_reg();
        self.line(&format!(
            "  {hi} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {seq_h}, i64 {idx})"
        ));
        let vi = self.unbox_handle_to_val(&hi, &out_elem);
        let cur = self.fresh_reg();
        self.line(&format!("  {cur} = load {}, ptr {acc_slot}", lt.ir()));
        let acc_v = Val {
            reg: cur,
            dec_ty: out_elem.clone(),
        };
        let next_v = self.emit_adverb_step(cg, op, acc_v, vi, line)?;
        self.line(&format!(
            "  store {} {}, ptr {acc_slot}",
            lt.ir(),
            next_v.reg
        ));
        let bhi = self.box_val_to_handle(&next_v);
        self.line(&format!(
            "  call void @aod_rt_seq_buf_push(ptr %ctx, i64 {buf_id}, i64 {bhi})"
        ));
        let nxt = self.fresh_reg();
        self.line(&format!("  {nxt} = add nsw i64 {idx}, 1"));
        self.line(&format!("  store i64 {nxt}, ptr {idx_slot}"));
        let md_suffix = cg.alloc_loop_hint_md().unwrap_or_default();
        self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
        self.terminated = true;

        self.start_block(&finish_lbl);
        let is_arr = u8::from(fixed_len.is_some());
        let fin_h = self.fresh_reg();
        self.line(&format!(
            "  {fin_h} = call i64 @aod_rt_seq_buf_finish(ptr %ctx, i64 {buf_id}, i8 {is_arr})"
        ));
        self.emit_fail_return_check();
        self.line(&format!("  store i64 {fin_h}, ptr {res_h_slot}"));
        self.line(&format!("  br label %{done_lbl}"));

        self.start_block(&done_lbl);
        let out_h = self.fresh_reg();
        self.line(&format!("  {out_h} = load i64, ptr {res_h_slot}"));
        let seq_ty = match fixed_len {
            Some(n) => Type::Array(Box::new(out_elem), n),
            None => Type::List(Box::new(out_elem)),
        };
        Ok(Val {
            reg: out_h,
            dec_ty: seq_ty,
        })
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
            (Type::Ratio, "num" | "den") => Ok(Type::Int),
            (Type::SimResult | Type::ChamberView, "model_status") => Ok(Type::Enum {
                module: BUILTIN_MOD.to_string(),
                name: "ModelStatus".to_string(),
            }),
            (
                Type::SimResult,
                "model_status_bytes"
                | "chamber_name"
                | "chamber_name_bytes"
                | "final_state_hash"
                | "final_state_hash_bytes"
                | "final_state_sha256"
                | "boundary_bytes"
                | "state_json"
                | "shedding_witnesses_json",
            ) => Ok(Type::Bytes),
            (
                Type::SimResult,
                "ticks"
                | "carrier_count"
                | "active_carrier_count"
                | "shedding_event_count"
                | "wavelet_event_count"
                | "boundary_crossings"
                | "total_returns"
                | "total_hinges"
                | "total_shedding_events"
                | "resonance_count"
                | "expired_contact_count"
                | "fusion_count"
                | "moving_flux_total",
            ) => Ok(Type::Int),
            (Type::SimResult, "final_snapshot") => {
                Ok(Type::ReadOnly(Box::new(Type::ChamberView)))
            }
            (
                Type::ChamberView,
                "model_status_bytes"
                | "chamber_name"
                | "chamber_name_bytes"
                | "shedding_witnesses_json",
            ) => Ok(Type::Bytes),
            (
                Type::ChamberView,
                "tick"
                | "carrier_count"
                | "active_carrier_count"
                | "boundary_crossings"
                | "shedding_count"
                | "wavelet_count"
                | "clock_count"
                | "resonance_count"
                | "expired_contact_count"
                | "fusion_count",
            ) => Ok(Type::Int),
            (Type::ChamberView, "carriers") => {
                Ok(Type::ReadOnly(Box::new(Type::List(Box::new(Type::Int)))))
            }
            (Type::Box3, "boundary_bytes") => Ok(Type::Bytes),
            (Type::RecipientOutcome, "stage") => Ok(Type::Enum {
                module: BUILTIN_MOD.to_string(),
                name: "RecipientStage".to_string(),
            }),
            (Type::RecipientOutcome, "stage_bytes") => Ok(Type::Bytes),
            (Type::RecipientOutcome, "closure_phase_delta" | "reclosure_bip_delta") => {
                Ok(Type::Int)
            }
            (Type::RecipientOutcome, "blocks_later_seating") => Ok(Type::Bool),
            (Type::CadenceAssociation, "is_cadence_ratio") => Ok(Type::Bool),
            (
                Type::CadenceAssociation,
                "wavelet_id" | "wavelet_id_bytes" | "reference_id" | "reference_id_bytes",
            ) => Ok(Type::Bytes),
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

    // ==================== Unary, Binary, & Rank-Lifted Loops ====================

    fn emit_unary(
        &mut self,
        cg: &mut Codegen<'_>,
        op: UnaryOp,
        v: Val,
        line: usize,
    ) -> DecResult<Val> {
        let base_ty = v.dec_ty.unwrap_read_only().clone();
        if op == UnaryOp::Boundary {
            let r = self.fresh_reg();
            self.line(&format!(
                "  {r} = call i64 @aod_rt_boundary_of(ptr %ctx, i64 {})",
                v.reg
            ));
            self.emit_fail_return_check();
            let ret_ty = match base_ty {
                Type::Box3 | Type::SimResult => Type::Bytes,
                Type::Window | Type::Range => Type::Tuple(vec![Type::Int, Type::Int]),
                Type::Array(elem, n) => Type::Array(elem, n.saturating_sub(1)),
                Type::List(elem) => Type::List(elem),
                other => {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("codegen: invalid boundary operand {other}"),
                    ));
                }
            };
            return Ok(Val {
                reg: r,
                dec_ty: ret_ty,
            });
        }

        match base_ty {
            Type::Array(elem_ty, n) => {
                self.emit_rank_unary_op_loop(cg, op, &v.reg, &elem_ty, Some(n), line)
            }
            Type::List(elem_ty) => {
                self.emit_rank_unary_op_loop(cg, op, &v.reg, &elem_ty, None, line)
            }
            Type::Bool => match op {
                UnaryOp::Not => {
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = xor i1 {}, true", v.reg));
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::Bool,
                    })
                }
                _ => Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "codegen: unsupported bool unary op",
                )),
            },
            Type::QExt(ref inner) if **inner == Type::Bool => match op {
                UnaryOp::Not => {
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = xor i1 {}, true", v.reg));
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::QExt(Box::new(Type::Bool)),
                    })
                }
                _ => Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "codegen: unsupported QExt<bool> unary op",
                )),
            },
            Type::Int => match op {
                UnaryOp::Pos => Ok(Val {
                    reg: v.reg,
                    dec_ty: Type::Int,
                }),
                UnaryOp::Neg => {
                    let r = self.fresh_reg();
                    let helper = if cg.plan.trit_ssa_promotion {
                        "@__aod_int_neg"
                    } else {
                        "@aod_rt_int_neg"
                    };
                    self.line(&format!("  {r} = call i64 {helper}(ptr %ctx, i64 {})", v.reg));
                    self.emit_fail_return_check();
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::Int,
                    })
                }
                _ => Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "codegen: unsupported int unary op",
                )),
            },
            Type::T1 => match op {
                UnaryOp::Pos => Ok(Val {
                    reg: v.reg,
                    dec_ty: Type::T1,
                }),
                UnaryOp::Neg => {
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = sub i8 0, {}", v.reg));
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::T1,
                    })
                }
                _ => Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "codegen: unsupported t1 unary op",
                )),
            },
            Type::Ratio => match op {
                UnaryOp::Pos => Ok(Val {
                    reg: v.reg,
                    dec_ty: Type::Ratio,
                }),
                UnaryOp::Neg => {
                    if cg.plan.inline_ratio_fastpath {
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
                    } else {
                        let (na, da) = self.extract_ratio_words(&v.reg);
                        let out = self.alloc_temp("%DecRatio");
                        self.line(&format!(
                            "  call void @aod_rt_ratio_neg(ptr %ctx, i64 {na}, i64 {da}, ptr {out})"
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
                _ => Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "codegen: unsupported ratio unary op",
                )),
            },
            Type::QExt(ref inner) if **inner == Type::Ratio => match op {
                UnaryOp::Pos => Ok(Val {
                    reg: v.reg,
                    dec_ty: Type::QExt(Box::new(Type::Ratio)),
                }),
                UnaryOp::Neg => {
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
                _ => Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "codegen: unsupported QExt<ratio> unary op",
                )),
            },
            Type::B8 => match op {
                UnaryOp::BitNot => {
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = xor i8 {}, -1", v.reg));
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::B8,
                    })
                }
                _ => Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "codegen: unsupported b8 unary op",
                )),
            },
            Type::B64 => match op {
                UnaryOp::BitNot => {
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = xor i64 {}, -1", v.reg));
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::B64,
                    })
                }
                _ => Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "codegen: unsupported b64 unary op",
                )),
            },
            Type::Q4 => match op {
                UnaryOp::BitNot => {
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = xor i8 {}, 15", v.reg));
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::Q4,
                    })
                }
                _ => Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "codegen: unsupported q4 unary op",
                )),
            },
            other => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!("codegen: unsupported unary op on {other}"),
            )),
        }
    }

    fn emit_rank_unary_op_loop(
        &mut self,
        cg: &mut Codegen<'_>,
        op: UnaryOp,
        seq_h: &str,
        elem_ty: &Type,
        fixed_len: Option<usize>,
        line: usize,
    ) -> DecResult<Val> {
        let len = self.fresh_reg();
        self.line(&format!(
            "  {len} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {seq_h})"
        ));
        self.emit_fail_return_check();
        let buf_id = self.fresh_reg();
        self.line(&format!(
            "  {buf_id} = call i64 @aod_rt_seq_buf_new(ptr %ctx, i64 {len})"
        ));
        let idx_slot = self.alloc_temp("i64");
        self.line(&format!("  store i64 0, ptr {idx_slot}"));

        let cond_lbl = self.fresh_label("uop_cond");
        let body_lbl = self.fresh_label("uop_body");
        let end_lbl = self.fresh_label("uop_end");
        self.line(&format!("  br label %{cond_lbl}"));

        self.start_block(&cond_lbl);
        let idx = self.fresh_reg();
        let cmp = self.fresh_reg();
        self.line(&format!("  {idx} = load i64, ptr {idx_slot}"));
        self.line(&format!("  {cmp} = icmp slt i64 {idx}, {len}"));
        self.line(&format!(
            "  br i1 {cmp}, label %{body_lbl}, label %{end_lbl}"
        ));

        self.start_block(&body_lbl);
        self.emit_fuel_tick();
        let eh = self.fresh_reg();
        self.line(&format!(
            "  {eh} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {seq_h}, i64 {idx})"
        ));
        let ev = self.unbox_handle_to_val(&eh, elem_ty);
        let mapped = self.emit_unary(cg, op, ev, line)?;
        let out_elem_ty = mapped.dec_ty.clone();
        let mh = self.box_val_to_handle(&mapped);
        self.line(&format!(
            "  call void @aod_rt_seq_buf_push(ptr %ctx, i64 {buf_id}, i64 {mh})"
        ));
        let nxt = self.fresh_reg();
        self.line(&format!("  {nxt} = add nsw i64 {idx}, 1"));
        self.line(&format!("  store i64 {nxt}, ptr {idx_slot}"));
        let md_suffix = cg.alloc_loop_hint_md().unwrap_or_default();
        self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
        self.terminated = true;

        self.start_block(&end_lbl);
        let is_arr = u8::from(fixed_len.is_some());
        let out_h = self.fresh_reg();
        self.line(&format!(
            "  {out_h} = call i64 @aod_rt_seq_buf_finish(ptr %ctx, i64 {buf_id}, i8 {is_arr})"
        ));
        self.emit_fail_return_check();
        let out_ty = match fixed_len {
            Some(n) => Type::Array(Box::new(out_elem_ty), n),
            None => Type::List(Box::new(out_elem_ty)),
        };
        Ok(Val {
            reg: out_h,
            dec_ty: out_ty,
        })
    }

    fn emit_binary(
        &mut self,
        cg: &mut Codegen<'_>,
        op: BinaryOp,
        lv: Val,
        rv: Val,
        line: usize,
    ) -> DecResult<Val> {
        let lty = lv.dec_ty.unwrap_read_only().clone();
        let rty = rv.dec_ty.unwrap_read_only().clone();

        // 1. Mask Sieve `mask ?> xs`
        if op == BinaryOp::Sieve {
            let lv = self.materialize_if_range(lv);
            let rv = self.materialize_if_range(rv);
            let out_elem = seq_elem_type(&rv.dec_ty).unwrap_or(Type::Unknown);
            let r = self.fresh_reg();
            self.line(&format!(
                "  {r} = call i64 @aod_rt_seq_sieve(ptr %ctx, i64 {}, i64 {})",
                lv.reg, rv.reg
            ));
            self.emit_fail_return_check();
            return Ok(Val {
                reg: r,
                dec_ty: Type::List(Box::new(out_elem)),
            });
        }

        // 2. Outer products `+:` and `*:`
        if matches!(op, BinaryOp::OuterAdd | BinaryOp::OuterMul) {
            return self.emit_outer_product_loop(cg, op, lv, rv, line);
        }

        // 3. Whole-sequence `==` / `!=` when both sides have equal positive sequence rank
        let lr = type_seq_rank(&lty);
        let rr = type_seq_rank(&rty);
        if matches!(op, BinaryOp::Eq | BinaryOp::NotEq) && lr > 0 && lr == rr {
            let eq8 = self.fresh_reg();
            self.line(&format!(
                "  {eq8} = call i8 @aod_rt_handle_eq(ptr %ctx, i64 {}, i64 {})",
                lv.reg, rv.reg
            ));
            self.emit_fail_return_check();
            let pred = if op == BinaryOp::NotEq { "eq" } else { "ne" };
            let r = self.fresh_reg();
            self.line(&format!("  {r} = icmp {pred} i8 {eq8}, 0"));
            return Ok(Val {
                reg: r,
                dec_ty: Type::Bool,
            });
        }

        // 4. Rank-lifted binary operations + scalar/lower-rank broadcast
        if lr > 0 || rr > 0 {
            return self.emit_rank_binary_op_loop(cg, op, lv, rv, line);
        }

        // 5. Ternary phase-gate `int * t1` / `t1 * int` and `ratio * t1` / `t1 * ratio`
        if op == BinaryOp::Mul {
            if lty == Type::Int && rty == Type::T1 {
                return Ok(self.emit_int_phase_gate(&lv.reg, &rv.reg, cg.plan.trit_ssa_promotion));
            }
            if lty == Type::T1 && rty == Type::Int {
                return Ok(self.emit_int_phase_gate(&rv.reg, &lv.reg, cg.plan.trit_ssa_promotion));
            }
            if lty == Type::Ratio && rty == Type::T1 {
                return Ok(self.emit_ratio_phase_gate(&lv.reg, &rv.reg, cg.plan.inline_ratio_fastpath));
            }
            if lty == Type::T1 && rty == Type::Ratio {
                return Ok(self.emit_ratio_phase_gate(&rv.reg, &lv.reg, cg.plan.inline_ratio_fastpath));
            }
        }

        // 6. Bit shifts `(b8 | b64 | q4) << int` / `>> int`
        if matches!(op, BinaryOp::Shl | BinaryOp::Shr) {
            return self.emit_bit_shift(cg, op, lv, rv, line);
        }

        // 7. Scalar homogeneous operations
        match lty {
            Type::Null => {
                let r = if op == BinaryOp::Eq { "true" } else { "false" };
                Ok(Val {
                    reg: r.to_string(),
                    dec_ty: Type::Bool,
                })
            }
            Type::Int => match op {
                BinaryOp::Add => self.emit_int_bin(cg, "@__aod_int_add", "@aod_rt_int_add", &lv.reg, &rv.reg),
                BinaryOp::Sub => self.emit_int_bin(cg, "@__aod_int_sub", "@aod_rt_int_sub", &lv.reg, &rv.reg),
                BinaryOp::Mul => self.emit_int_bin(cg, "@__aod_int_mul", "@aod_rt_int_mul", &lv.reg, &rv.reg),
                BinaryOp::FloorDiv => {
                    self.emit_int_bin(cg, "@__aod_int_div_floor", "@aod_rt_int_div_floor", &lv.reg, &rv.reg)
                }
                BinaryOp::Mod => {
                    self.emit_int_bin(cg, "@__aod_int_rem_floor", "@aod_rt_int_rem_floor", &lv.reg, &rv.reg)
                }
                BinaryOp::Div => {
                    if cg.plan.inline_ratio_fastpath {
                        let r = self.fresh_reg();
                        self.line(&format!(
                            "  {r} = call %DecRatio @__aod_int_exact_div(ptr %ctx, i64 {}, i64 {})",
                            lv.reg, rv.reg
                        ));
                        self.emit_fail_return_check();
                        Ok(Val {
                            reg: r,
                            dec_ty: Type::Ratio,
                        })
                    } else {
                        let out = self.alloc_temp("%DecRatio");
                        self.line(&format!(
                            "  call void @aod_rt_int_exact_div(ptr %ctx, i64 {}, i64 {}, ptr {out})",
                            lv.reg, rv.reg
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
                BinaryOp::Min | BinaryOp::Max => {
                    let cmp = self.emit_int_cmp8(cg, &lv.reg, &rv.reg);
                    let pred = if op == BinaryOp::Min { "sle" } else { "sge" };
                    let take_l = self.fresh_reg();
                    let sel = self.fresh_reg();
                    self.line(&format!("  {take_l} = icmp {pred} i8 {cmp}, 0"));
                    self.line(&format!(
                        "  {sel} = select i1 {take_l}, i64 {}, i64 {}",
                        lv.reg, rv.reg
                    ));
                    Ok(Val {
                        reg: sel,
                        dec_ty: Type::Int,
                    })
                }
                BinaryOp::Cmp3 => {
                    let cmp = self.emit_int_cmp8(cg, &lv.reg, &rv.reg);
                    Ok(Val {
                        reg: cmp,
                        dec_ty: Type::T1,
                    })
                }
                BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Lt
                | BinaryOp::LtEq
                | BinaryOp::Gt
                | BinaryOp::GtEq => {
                    let cmp = self.emit_int_cmp8(cg, &lv.reg, &rv.reg);
                    Ok(self.cmp8_to_bool(&cmp, op, Type::Bool))
                }
                _ => Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "codegen: unsupported int binary op",
                )),
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
                BinaryOp::Min | BinaryOp::Max => {
                    let pred = if op == BinaryOp::Min { "sle" } else { "sge" };
                    let take_l = self.fresh_reg();
                    let sel = self.fresh_reg();
                    self.line(&format!("  {take_l} = icmp {pred} i8 {}, {}", lv.reg, rv.reg));
                    self.line(&format!(
                        "  {sel} = select i1 {take_l}, i8 {}, i8 {}",
                        lv.reg, rv.reg
                    ));
                    Ok(Val {
                        reg: sel,
                        dec_ty: Type::T1,
                    })
                }
                BinaryOp::Cmp3 => Ok(self.emit_scalar_cmp3("i8", &lv.reg, &rv.reg, true)),
                _ => Ok(self.emit_scalar_cmp("i8", op, &lv.reg, &rv.reg, true)),
            },
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
                        let (na, da) = self.extract_ratio_words(&lv.reg);
                        let (nb, db) = self.extract_ratio_words(&rv.reg);
                        let out = self.alloc_temp("%DecRatio");
                        self.line(&format!(
                            "  call void {rt_fn}(ptr %ctx, i64 {na}, i64 {da}, i64 {nb}, i64 {db}, ptr {out})"
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
                BinaryOp::Min | BinaryOp::Max => {
                    let cmp = self.emit_ratio_cmp8(cg, &lv.reg, &rv.reg);
                    let pred = if op == BinaryOp::Min { "sle" } else { "sge" };
                    let take_l = self.fresh_reg();
                    let sel = self.fresh_reg();
                    self.line(&format!("  {take_l} = icmp {pred} i8 {cmp}, 0"));
                    self.line(&format!(
                        "  {sel} = select i1 {take_l}, %DecRatio {}, %DecRatio {}",
                        lv.reg, rv.reg
                    ));
                    Ok(Val {
                        reg: sel,
                        dec_ty: Type::Ratio,
                    })
                }
                BinaryOp::Cmp3 => {
                    let cmp = self.emit_ratio_cmp8(cg, &lv.reg, &rv.reg);
                    Ok(Val {
                        reg: cmp,
                        dec_ty: Type::T1,
                    })
                }
                BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Lt
                | BinaryOp::LtEq
                | BinaryOp::Gt
                | BinaryOp::GtEq => {
                    let cmp = self.emit_ratio_cmp8(cg, &lv.reg, &rv.reg);
                    Ok(self.cmp8_to_bool(&cmp, op, Type::Bool))
                }
                _ => Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    "codegen: unsupported ratio binary op",
                )),
            },
            Type::B8 | Type::Q4 => match op {
                BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor => {
                    let ir_op = match op {
                        BinaryOp::BitAnd => "and",
                        BinaryOp::BitOr => "or",
                        BinaryOp::BitXor => "xor",
                        _ => unreachable!(),
                    };
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = {ir_op} i8 {}, {}", lv.reg, rv.reg));
                    Ok(Val {
                        reg: r,
                        dec_ty: lty,
                    })
                }
                BinaryOp::Min | BinaryOp::Max => {
                    let pred = if op == BinaryOp::Min { "ule" } else { "uge" };
                    let take_l = self.fresh_reg();
                    let sel = self.fresh_reg();
                    self.line(&format!("  {take_l} = icmp {pred} i8 {}, {}", lv.reg, rv.reg));
                    self.line(&format!(
                        "  {sel} = select i1 {take_l}, i8 {}, i8 {}",
                        lv.reg, rv.reg
                    ));
                    Ok(Val {
                        reg: sel,
                        dec_ty: lty,
                    })
                }
                BinaryOp::Cmp3 => Ok(self.emit_scalar_cmp3("i8", &lv.reg, &rv.reg, false)),
                _ => Ok(self.emit_scalar_cmp("i8", op, &lv.reg, &rv.reg, false)),
            },
            Type::B64 => match op {
                BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor => {
                    let ir_op = match op {
                        BinaryOp::BitAnd => "and",
                        BinaryOp::BitOr => "or",
                        BinaryOp::BitXor => "xor",
                        _ => unreachable!(),
                    };
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = {ir_op} i64 {}, {}", lv.reg, rv.reg));
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::B64,
                    })
                }
                BinaryOp::Min | BinaryOp::Max => {
                    let pred = if op == BinaryOp::Min { "ule" } else { "uge" };
                    let take_l = self.fresh_reg();
                    let sel = self.fresh_reg();
                    self.line(&format!("  {take_l} = icmp {pred} i64 {}, {}", lv.reg, rv.reg));
                    self.line(&format!(
                        "  {sel} = select i1 {take_l}, i64 {}, i64 {}",
                        lv.reg, rv.reg
                    ));
                    Ok(Val {
                        reg: sel,
                        dec_ty: Type::B64,
                    })
                }
                BinaryOp::Cmp3 => Ok(self.emit_scalar_cmp3("i64", &lv.reg, &rv.reg, false)),
                _ => Ok(self.emit_scalar_cmp("i64", op, &lv.reg, &rv.reg, false)),
            },
            Type::Bool => match op {
                BinaryOp::And => {
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = and i1 {}, {}", lv.reg, rv.reg));
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::Bool,
                    })
                }
                BinaryOp::Or => {
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = or i1 {}, {}", lv.reg, rv.reg));
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::Bool,
                    })
                }
                _ => Ok(self.emit_scalar_cmp("i1", op, &lv.reg, &rv.reg, false)),
            },
            Type::QExt(ref inner) if **inner == Type::Bool => match op {
                BinaryOp::And => {
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = and i1 {}, {}", lv.reg, rv.reg));
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::QExt(Box::new(Type::Bool)),
                    })
                }
                BinaryOp::Or => {
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = or i1 {}, {}", lv.reg, rv.reg));
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::QExt(Box::new(Type::Bool)),
                    })
                }
                _ => {
                    let mut v = self.emit_scalar_cmp("i1", op, &lv.reg, &rv.reg, false);
                    v.dec_ty = Type::QExt(Box::new(Type::Bool));
                    Ok(v)
                }
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
                BinaryOp::Min | BinaryOp::Max => {
                    let cmp = self.fresh_reg();
                    self.line(&format!(
                        "  {cmp} = call i8 @aod_rt_qext_cmp(ptr %ctx, i64 {}, i64 {})",
                        lv.reg, rv.reg
                    ));
                    self.emit_fail_return_check();
                    let pred = if op == BinaryOp::Min { "sle" } else { "sge" };
                    let take_l = self.fresh_reg();
                    let sel = self.fresh_reg();
                    self.line(&format!("  {take_l} = icmp {pred} i8 {cmp}, 0"));
                    self.line(&format!(
                        "  {sel} = select i1 {take_l}, i64 {}, i64 {}",
                        lv.reg, rv.reg
                    ));
                    Ok(Val {
                        reg: sel,
                        dec_ty: Type::QExt(Box::new(Type::Ratio)),
                    })
                }
                _ => {
                    let cmp = self.fresh_reg();
                    self.line(&format!(
                        "  {cmp} = call i8 @aod_rt_qext_cmp(ptr %ctx, i64 {}, i64 {})",
                        lv.reg, rv.reg
                    ));
                    self.emit_fail_return_check();
                    Ok(self.cmp8_to_bool(&cmp, op, Type::QExt(Box::new(Type::Bool))))
                }
            },
            Type::Bytes | Type::Addr8
                if !matches!(op, BinaryOp::Eq | BinaryOp::NotEq) =>
            {
                let cmp = self.fresh_reg();
                self.line(&format!(
                    "  {cmp} = call i8 @aod_rt_handle_cmp(ptr %ctx, i64 {}, i64 {})",
                    lv.reg, rv.reg
                ));
                self.emit_fail_return_check();
                match op {
                    BinaryOp::Min | BinaryOp::Max => {
                        let pred = if op == BinaryOp::Min { "sle" } else { "sge" };
                        let take_l = self.fresh_reg();
                        let sel = self.fresh_reg();
                        self.line(&format!("  {take_l} = icmp {pred} i8 {cmp}, 0"));
                        self.line(&format!(
                            "  {sel} = select i1 {take_l}, i64 {}, i64 {}",
                            lv.reg, rv.reg
                        ));
                        Ok(Val {
                            reg: sel,
                            dec_ty: lty,
                        })
                    }
                    BinaryOp::Cmp3 => Ok(Val {
                        reg: cmp,
                        dec_ty: Type::T1,
                    }),
                    _ => Ok(self.cmp8_to_bool(&cmp, op, Type::Bool)),
                }
            }
            _ => {
                let eq8 = self.fresh_reg();
                self.line(&format!(
                    "  {eq8} = call i8 @aod_rt_handle_eq(ptr %ctx, i64 {}, i64 {})",
                    lv.reg, rv.reg
                ));
                self.emit_fail_return_check();
                let pred = if op == BinaryOp::NotEq { "eq" } else { "ne" };
                let r = self.fresh_reg();
                self.line(&format!("  {r} = icmp {pred} i8 {eq8}, 0"));
                let out_ty = if lty.is_qext() {
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

    fn emit_int_phase_gate(&mut self, int_reg: &str, role_reg: &str, ssa_fast: bool) -> Val {
        let neg_fn = if ssa_fast {
            "@__aod_int_neg"
        } else {
            "@aod_rt_int_neg"
        };
        let is_zero = self.fresh_reg();
        let is_neg = self.fresh_reg();
        let neg_v = self.fresh_reg();
        let s1 = self.fresh_reg();
        let s2 = self.fresh_reg();
        self.line(&format!("  {is_zero} = icmp eq i8 {role_reg}, 0"));
        self.line(&format!("  {is_neg} = icmp slt i8 {role_reg}, 0"));
        self.line(&format!(
            "  {neg_v} = call i64 {neg_fn}(ptr %ctx, i64 {int_reg})"
        ));
        self.emit_fail_return_check();
        self.line(&format!(
            "  {s1} = select i1 {is_neg}, i64 {neg_v}, i64 {int_reg}"
        ));
        self.line(&format!("  {s2} = select i1 {is_zero}, i64 0, i64 {s1}"));
        Val {
            reg: s2,
            dec_ty: Type::Int,
        }
    }

    fn emit_ratio_phase_gate(&mut self, rat_reg: &str, role_reg: &str, fast: bool) -> Val {
        let neg_r = if fast {
            let r = self.fresh_reg();
            self.line(&format!(
                "  {r} = call %DecRatio @__aod_ratio_neg(ptr %ctx, %DecRatio {rat_reg})"
            ));
            self.emit_fail_return_check();
            r
        } else {
            let (na, da) = self.extract_ratio_words(rat_reg);
            let out = self.alloc_temp("%DecRatio");
            self.line(&format!(
                "  call void @aod_rt_ratio_neg(ptr %ctx, i64 {na}, i64 {da}, ptr {out})"
            ));
            self.emit_fail_return_check();
            let r = self.fresh_reg();
            self.line(&format!("  {r} = load %DecRatio, ptr {out}"));
            r
        };
        let is_zero = self.fresh_reg();
        let is_neg = self.fresh_reg();
        let s1 = self.fresh_reg();
        let s2 = self.fresh_reg();
        self.line(&format!("  {is_zero} = icmp eq i8 {role_reg}, 0"));
        self.line(&format!("  {is_neg} = icmp slt i8 {role_reg}, 0"));
        self.line(&format!(
            "  {s1} = select i1 {is_neg}, %DecRatio {neg_r}, %DecRatio {rat_reg}"
        ));
        self.line(&format!(
            "  {s2} = select i1 {is_zero}, %DecRatio {{ i64 0, i64 1 }}, %DecRatio {s1}"
        ));
        Val {
            reg: s2,
            dec_ty: Type::Ratio,
        }
    }

    fn emit_bit_shift(
        &mut self,
        cg: &mut Codegen<'_>,
        op: BinaryOp,
        lv: Val,
        rv: Val,
        line: usize,
    ) -> DecResult<Val> {
        let lty = lv.dec_ty.unwrap_read_only().clone();
        let max_shift: u64 = match lty {
            Type::B8 => 7,
            Type::B64 => 63,
            Type::Q4 => 3,
            _ => {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("codegen: invalid shift lhs {lty}"),
                ));
            }
        };
        // Convert `rv: int` (`DecInt`) to unsigned `u64` via `aod_rt_b64_from_unsigned_int`
        // (which rejects negative shifts cleanly!) and check `<= max_shift`.
        let sh64 = self.fresh_reg();
        self.line(&format!(
            "  {sh64} = call i64 @aod_rt_b64_from_unsigned_int(ptr %ctx, i64 {})",
            rv.reg
        ));
        self.emit_fail_return_check();
        let in_bounds = self.fresh_reg();
        let ok_lbl = self.fresh_label("sh_ok");
        let bad_lbl = self.fresh_label("sh_oob");
        self.line(&format!("  {in_bounds} = icmp ule i64 {sh64}, {max_shift}"));
        self.line(&format!(
            "  br i1 {in_bounds}, label %{ok_lbl}, label %{bad_lbl}"
        ));
        self.start_block(&bad_lbl);
        let (mg, ml) = cg.intern_bytes(b"bit shift count out of bounds");
        self.line(&format!(
            "  call void @aod_rt_fail(ptr %ctx, ptr {mg}, i64 {ml})"
        ));
        self.emit_depth_dec();
        self.emit_default_ret();

        self.start_block(&ok_lbl);
        match lty {
            Type::B64 => {
                let ir_op = if op == BinaryOp::Shl { "shl" } else { "lshr" };
                let r = self.fresh_reg();
                self.line(&format!("  {r} = {ir_op} i64 {}, {sh64}", lv.reg));
                Ok(Val {
                    reg: r,
                    dec_ty: Type::B64,
                })
            }
            Type::B8 => {
                let sh8 = self.fresh_reg();
                let r = self.fresh_reg();
                let ir_op = if op == BinaryOp::Shl { "shl" } else { "lshr" };
                self.line(&format!("  {sh8} = trunc i64 {sh64} to i8"));
                self.line(&format!("  {r} = {ir_op} i8 {}, {sh8}", lv.reg));
                Ok(Val {
                    reg: r,
                    dec_ty: Type::B8,
                })
            }
            Type::Q4 => {
                let sh8 = self.fresh_reg();
                let raw = self.fresh_reg();
                let r = self.fresh_reg();
                let ir_op = if op == BinaryOp::Shl { "shl" } else { "lshr" };
                self.line(&format!("  {sh8} = trunc i64 {sh64} to i8"));
                self.line(&format!("  {raw} = {ir_op} i8 {}, {sh8}", lv.reg));
                self.line(&format!("  {r} = and i8 {raw}, 15"));
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Q4,
                })
            }
            _ => unreachable!(),
        }
    }

    fn emit_outer_product_loop(
        &mut self,
        cg: &mut Codegen<'_>,
        op: BinaryOp,
        lv: Val,
        rv: Val,
        line: usize,
    ) -> DecResult<Val> {
        let lv = self.materialize_if_range(lv);
        let rv = self.materialize_if_range(rv);
        let (le, l_fixed) = match lv.dec_ty.unwrap_read_only().clone() {
            Type::Array(e, n) => ((*e).clone(), Some(n)),
            Type::List(e) => ((*e).clone(), None),
            other => {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("codegen: outer product expects sequence, got {other}"),
                ));
            }
        };
        let (re, r_fixed) = match rv.dec_ty.unwrap_read_only().clone() {
            Type::Array(e, n) => ((*e).clone(), Some(n)),
            Type::List(e) => ((*e).clone(), None),
            other => {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("codegen: outer product expects sequence, got {other}"),
                ));
            }
        };
        let both_fixed = l_fixed.is_some() && r_fixed.is_some();
        let inner_op = if op == BinaryOp::OuterAdd {
            BinaryOp::Add
        } else {
            BinaryOp::Mul
        };

        let m_len = self.fresh_reg();
        let n_len = self.fresh_reg();
        self.line(&format!(
            "  {m_len} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {})",
            lv.reg
        ));
        self.line(&format!(
            "  {n_len} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {})",
            rv.reg
        ));
        self.emit_fail_return_check();

        let outer_buf = self.fresh_reg();
        self.line(&format!(
            "  {outer_buf} = call i64 @aod_rt_seq_buf_new(ptr %ctx, i64 {m_len})"
        ));
        let i_slot = self.alloc_temp("i64");
        let j_slot = self.alloc_temp("i64");
        self.line(&format!("  store i64 0, ptr {i_slot}"));

        let o_cond = self.fresh_label("out_i_cond");
        let o_body = self.fresh_label("out_i_body");
        let i_cond = self.fresh_label("out_j_cond");
        let i_body = self.fresh_label("out_j_body");
        let i_end = self.fresh_label("out_j_end");
        let o_end = self.fresh_label("out_i_end");
        self.line(&format!("  br label %{o_cond}"));

        self.start_block(&o_cond);
        let i_cur = self.fresh_reg();
        let i_cmp = self.fresh_reg();
        self.line(&format!("  {i_cur} = load i64, ptr {i_slot}"));
        self.line(&format!("  {i_cmp} = icmp slt i64 {i_cur}, {m_len}"));
        self.line(&format!(
            "  br i1 {i_cmp}, label %{o_body}, label %{o_end}"
        ));

        self.start_block(&o_body);
        let xh = self.fresh_reg();
        self.line(&format!(
            "  {xh} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {i_cur})",
            lv.reg
        ));
        let xv = self.unbox_handle_to_val(&xh, &le);
        let row_buf = self.fresh_reg();
        self.line(&format!(
            "  {row_buf} = call i64 @aod_rt_seq_buf_new(ptr %ctx, i64 {n_len})"
        ));
        self.line(&format!("  store i64 0, ptr {j_slot}"));
        self.line(&format!("  br label %{i_cond}"));

        self.start_block(&i_cond);
        let j_cur = self.fresh_reg();
        let j_cmp = self.fresh_reg();
        self.line(&format!("  {j_cur} = load i64, ptr {j_slot}"));
        self.line(&format!("  {j_cmp} = icmp slt i64 {j_cur}, {n_len}"));
        self.line(&format!(
            "  br i1 {j_cmp}, label %{i_body}, label %{i_end}"
        ));

        self.start_block(&i_body);
        self.emit_fuel_tick();
        let yh = self.fresh_reg();
        self.line(&format!(
            "  {yh} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {j_cur})",
            rv.reg
        ));
        let yv = self.unbox_handle_to_val(&yh, &re);
        let cell = self.emit_binary(cg, inner_op, xv.clone(), yv, line)?;
        let cell_ty = cell.dec_ty.clone();
        let ch = self.box_val_to_handle(&cell);
        self.line(&format!(
            "  call void @aod_rt_seq_buf_push(ptr %ctx, i64 {row_buf}, i64 {ch})"
        ));
        let j_nxt = self.fresh_reg();
        self.line(&format!("  {j_nxt} = add nsw i64 {j_cur}, 1"));
        self.line(&format!("  store i64 {j_nxt}, ptr {j_slot}"));
        let md_suffix = cg.alloc_loop_hint_md().unwrap_or_default();
        self.line(&format!("  br label %{i_cond}{md_suffix}"));
        self.terminated = true;

        self.start_block(&i_end);
        let is_arr = u8::from(both_fixed);
        let row_h = self.fresh_reg();
        self.line(&format!(
            "  {row_h} = call i64 @aod_rt_seq_buf_finish(ptr %ctx, i64 {row_buf}, i8 {is_arr})"
        ));
        self.line(&format!(
            "  call void @aod_rt_seq_buf_push(ptr %ctx, i64 {outer_buf}, i64 {row_h})"
        ));
        let i_nxt = self.fresh_reg();
        self.line(&format!("  {i_nxt} = add nsw i64 {i_cur}, 1"));
        self.line(&format!("  store i64 {i_nxt}, ptr {i_slot}"));
        self.line(&format!("  br label %{o_cond}"));
        self.terminated = true;

        self.start_block(&o_end);
        let mat_h = self.fresh_reg();
        self.line(&format!(
            "  {mat_h} = call i64 @aod_rt_seq_buf_finish(ptr %ctx, i64 {outer_buf}, i8 {is_arr})"
        ));
        self.emit_fail_return_check();

        let out_ty = match (l_fixed, r_fixed) {
            (Some(m), Some(n)) => Type::Array(Box::new(Type::Array(Box::new(cell_ty), n)), m),
            _ => Type::List(Box::new(Type::List(Box::new(cell_ty)))),
        };
        Ok(Val {
            reg: mat_h,
            dec_ty: out_ty,
        })
    }

    fn emit_rank_binary_op_loop(
        &mut self,
        cg: &mut Codegen<'_>,
        op: BinaryOp,
        lv: Val,
        rv: Val,
        line: usize,
    ) -> DecResult<Val> {
        let lv = self.materialize_if_range(lv);
        let rv = self.materialize_if_range(rv);
        let lr = type_seq_rank(&lv.dec_ty);
        let rr = type_seq_rank(&rv.dec_ty);

        let (len_reg, fixed_len, step_l, step_r) = if lr > rr {
            let (le, fl) = extract_seq_elem_and_fixed(&lv.dec_ty);
            let len = self.fresh_reg();
            self.line(&format!(
                "  {len} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {})",
                lv.reg
            ));
            self.emit_fail_return_check();
            (len, fl, Some(le), None)
        } else if rr > lr {
            let (re, fr) = extract_seq_elem_and_fixed(&rv.dec_ty);
            let len = self.fresh_reg();
            self.line(&format!(
                "  {len} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {})",
                rv.reg
            ));
            self.emit_fail_return_check();
            (len, fr, None, Some(re))
        } else {
            let (le, fl) = extract_seq_elem_and_fixed(&lv.dec_ty);
            let (re, fr) = extract_seq_elem_and_fixed(&rv.dec_ty);
            let ll = self.fresh_reg();
            let rl = self.fresh_reg();
            self.line(&format!(
                "  {ll} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {})",
                lv.reg
            ));
            self.line(&format!(
                "  {rl} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {})",
                rv.reg
            ));
            self.emit_fail_return_check();
            let eq = self.fresh_reg();
            let ok_lbl = self.fresh_label("zip_len_ok");
            let bad_lbl = self.fresh_label("zip_len_bad");
            self.line(&format!("  {eq} = icmp eq i64 {ll}, {rl}"));
            self.line(&format!("  br i1 {eq}, label %{ok_lbl}, label %{bad_lbl}"));
            self.start_block(&bad_lbl);
            let (mg, ml) = cg.intern_bytes(b"sequence length mismatch in rank-lifted op");
            self.line(&format!(
                "  call void @aod_rt_fail(ptr %ctx, ptr {mg}, i64 {ml})"
            ));
            self.emit_depth_dec();
            self.emit_default_ret();
            self.start_block(&ok_lbl);
            let fixed = match (fl, fr) {
                (Some(n), Some(_)) => Some(n),
                _ => None,
            };
            (ll, fixed, Some(le), Some(re))
        };

        let buf_id = self.fresh_reg();
        self.line(&format!(
            "  {buf_id} = call i64 @aod_rt_seq_buf_new(ptr %ctx, i64 {len_reg})"
        ));
        let idx_slot = self.alloc_temp("i64");
        self.line(&format!("  store i64 0, ptr {idx_slot}"));

        let cond_lbl = self.fresh_label("bop_cond");
        let body_lbl = self.fresh_label("bop_body");
        let end_lbl = self.fresh_label("bop_end");
        self.line(&format!("  br label %{cond_lbl}"));

        self.start_block(&cond_lbl);
        let idx = self.fresh_reg();
        let cmp = self.fresh_reg();
        self.line(&format!("  {idx} = load i64, ptr {idx_slot}"));
        self.line(&format!("  {cmp} = icmp slt i64 {idx}, {len_reg}"));
        self.line(&format!(
            "  br i1 {cmp}, label %{body_lbl}, label %{end_lbl}"
        ));

        self.start_block(&body_lbl);
        self.emit_fuel_tick();
        let item_l = if let Some(ref le) = step_l {
            let h = self.fresh_reg();
            self.line(&format!(
                "  {h} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {idx})",
                lv.reg
            ));
            self.unbox_handle_to_val(&h, le)
        } else {
            lv.clone()
        };
        let item_r = if let Some(ref re) = step_r {
            let h = self.fresh_reg();
            self.line(&format!(
                "  {h} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {idx})",
                rv.reg
            ));
            self.unbox_handle_to_val(&h, re)
        } else {
            rv.clone()
        };
        let mapped = self.emit_binary(cg, op, item_l, item_r, line)?;
        let out_elem_ty = mapped.dec_ty.clone();
        let mh = self.box_val_to_handle(&mapped);
        self.line(&format!(
            "  call void @aod_rt_seq_buf_push(ptr %ctx, i64 {buf_id}, i64 {mh})"
        ));
        let nxt = self.fresh_reg();
        self.line(&format!("  {nxt} = add nsw i64 {idx}, 1"));
        self.line(&format!("  store i64 {nxt}, ptr {idx_slot}"));
        let md_suffix = cg.alloc_loop_hint_md().unwrap_or_default();
        self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
        self.terminated = true;

        self.start_block(&end_lbl);
        let is_arr = u8::from(fixed_len.is_some());
        let out_h = self.fresh_reg();
        self.line(&format!(
            "  {out_h} = call i64 @aod_rt_seq_buf_finish(ptr %ctx, i64 {buf_id}, i8 {is_arr})"
        ));
        self.emit_fail_return_check();
        let out_ty = match fixed_len {
            Some(n) => Type::Array(Box::new(out_elem_ty), n),
            None => Type::List(Box::new(out_elem_ty)),
        };
        Ok(Val {
            reg: out_h,
            dec_ty: out_ty,
        })
    }

    fn emit_int_bin(
        &mut self,
        cg: &Codegen<'_>,
        ssa_fn: &str,
        rt_fn: &str,
        a: &str,
        b: &str,
    ) -> DecResult<Val> {
        let callee = if cg.plan.trit_ssa_promotion {
            ssa_fn
        } else {
            rt_fn
        };
        let r = self.fresh_reg();
        self.line(&format!("  {r} = call i64 {callee}(ptr %ctx, i64 {a}, i64 {b})"));
        self.emit_fail_return_check();
        Ok(Val {
            reg: r,
            dec_ty: Type::Int,
        })
    }

    fn emit_int_cmp8(&mut self, cg: &Codegen<'_>, a: &str, b: &str) -> String {
        let callee = if cg.plan.trit_ssa_promotion {
            "@__aod_int_cmp"
        } else {
            "@aod_rt_int_cmp"
        };
        let r = self.fresh_reg();
        self.line(&format!("  {r} = call i8 {callee}(ptr %ctx, i64 {a}, i64 {b})"));
        self.emit_fail_return_check();
        r
    }

    fn emit_ratio_cmp8(&mut self, cg: &Codegen<'_>, a: &str, b: &str) -> String {
        if cg.plan.inline_ratio_fastpath {
            let r = self.fresh_reg();
            self.line(&format!(
                "  {r} = call i8 @__aod_ratio_cmp(ptr %ctx, %DecRatio {a}, %DecRatio {b})"
            ));
            self.emit_fail_return_check();
            r
        } else {
            let (na, da) = self.extract_ratio_words(a);
            let (nb, db) = self.extract_ratio_words(b);
            let r = self.fresh_reg();
            self.line(&format!(
                "  {r} = call i8 @aod_rt_ratio_cmp(ptr %ctx, i64 {na}, i64 {da}, i64 {nb}, i64 {db})"
            ));
            self.emit_fail_return_check();
            r
        }
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

    fn cmp8_to_bool(&mut self, cmp_reg: &str, op: BinaryOp, out_ty: Type) -> Val {
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
        self.line(&format!("  {r} = icmp {pred} i8 {cmp_reg}, 0"));
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

    fn emit_scalar_cmp3(&mut self, ir_ty: &str, a: &str, b: &str, signed: bool) -> Val {
        let lt_pred = if signed { "slt" } else { "ult" };
        let gt_pred = if signed { "sgt" } else { "ugt" };
        let lt = self.fresh_reg();
        let gt = self.fresh_reg();
        let lt8 = self.fresh_reg();
        let gt8 = self.fresh_reg();
        let diff = self.fresh_reg();
        self.line(&format!("  {lt} = icmp {lt_pred} {ir_ty} {a}, {b}"));
        self.line(&format!("  {gt} = icmp {gt_pred} {ir_ty} {a}, {b}"));
        self.line(&format!("  {lt8} = zext i1 {lt} to i8"));
        self.line(&format!("  {gt8} = zext i1 {gt} to i8"));
        self.line(&format!("  {diff} = sub i8 {gt8}, {lt8}"));
        Val {
            reg: diff,
            dec_ty: Type::T1,
        }
    }
    // ==================== Calls, Combinators, & Builtins ====================

    fn peek_pure_type(&self, cg: &Codegen<'_>, expr: &Expr) -> Option<Type> {
        match &expr.kind {
            ExprKind::NullLit => Some(Type::Null),
            ExprKind::IntLit(n) if n.inline_word().is_some() => Some(Type::Int),
            ExprKind::BoolLit(_) => Some(Type::Bool),
            ExprKind::TritLit(_) => Some(Type::T1),
            ExprKind::RatioLit(r) if r.inline_words().is_some() => Some(Type::Ratio),
            ExprKind::Ident(name) => {
                if let Some(slot) = self.lookup_var(name) {
                    return Some(slot.dec_ty.unwrap_read_only().clone());
                }
                if let Some(cm) = cg.consts.get(&(self.mod_key.clone(), name.clone())) {
                    return Some(cm.dec_ty.unwrap_read_only().clone());
                }
                let base = cg.base_module_of(&self.mod_key).to_string();
                if let Some(cm) = cg.consts.get(&(base.clone(), name.clone())) {
                    return Some(cm.dec_ty.unwrap_read_only().clone());
                }
                if let Some(ImportedSymbol::ModuleItem { module, item }) =
                    cg.imports_for(&self.mod_key).get(name)
                {
                    if let Some(cm) = cg.consts.get(&(module.clone(), item.clone())) {
                        return Some(cm.dec_ty.unwrap_read_only().clone());
                    }
                }
                for (m, q) in &cg.qext_modules {
                    if m == &base {
                        let qsub = format!("{base}::{q}");
                        if let Some(cm) = cg.consts.get(&(qsub, name.clone())) {
                            return Some(cm.dec_ty.unwrap_read_only().clone());
                        }
                    }
                }
                None
            }
            ExprKind::Unary { op, operand } => {
                let t = self.peek_pure_type(cg, operand)?;
                match op {
                    UnaryOp::Pos => Some(t),
                    UnaryOp::Not if t == Type::Bool => Some(Type::Bool),
                    UnaryOp::Neg if t == Type::T1 => Some(Type::T1),
                    UnaryOp::BitNot if matches!(t, Type::B8 | Type::B64 | Type::Q4) => Some(t),
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
                        if matches!(
                            lt,
                            Type::Null | Type::Bool | Type::B8 | Type::B64 | Type::T1 | Type::Q4
                        ) =>
                    {
                        Some(Type::Bool)
                    }
                    BinaryOp::Cmp3
                        if matches!(lt, Type::B8 | Type::B64 | Type::T1 | Type::Q4) =>
                    {
                        Some(Type::T1)
                    }
                    BinaryOp::Min | BinaryOp::Max
                        if matches!(lt, Type::B8 | Type::B64 | Type::T1 | Type::Q4) =>
                    {
                        Some(lt)
                    }
                    BinaryOp::Mul if lt == Type::T1 => Some(Type::T1),
                    BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor
                        if matches!(lt, Type::B8 | Type::B64 | Type::Q4) =>
                    {
                        Some(lt)
                    }
                    BinaryOp::And | BinaryOp::Or if lt == Type::Bool => Some(Type::Bool),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn is_pure_select_safe(&self, cg: &Codegen<'_>, expr: &Expr) -> bool {
        self.peek_pure_type(cg, expr).is_some()
    }

    fn resolve_core_op_name_cg(&self, cg: &Codegen<'_>, callee: &Expr) -> Option<String> {
        match &callee.kind {
            ExprKind::Ident(n) => {
                if self.lookup_var(n).is_some() {
                    return None;
                }
                if !is_reserved_builtin_name(n) {
                    let base_mod = cg.base_module_of(&self.mod_key).to_string();
                    if cg.fn_overloads.contains_key(&(self.mod_key.clone(), n.clone()))
                        || (base_mod != self.mod_key
                            && cg.fn_overloads.contains_key(&(base_mod, n.clone())))
                        || matches!(
                            cg.imports_for(&self.mod_key).get(n),
                            Some(ImportedSymbol::ModuleItem { .. })
                        )
                    {
                        return None;
                    }
                }
                Some(n.clone())
            }
            ExprKind::Path(segs) => {
                if segs.len() == 3 && segs[0] == "aod" && segs[1] == "core" {
                    Some(segs[2].clone())
                } else if segs.len() == 2
                    && cg.imports_for(&self.mod_key).get(&segs[0])
                        == Some(&ImportedSymbol::ModuleNamespace("aod::core".to_string()))
                {
                    Some(segs[1].clone())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn emit_call_with_prepended(
        &mut self,
        cg: &mut Codegen<'_>,
        prepended: Option<Val>,
        callee: &Expr,
        args: &[Expr],
        hint: Option<&Type>,
        line: usize,
    ) -> DecResult<Val> {
        let core_op = self.resolve_core_op_name_cg(cg, callee);
        let simple_name = core_op.as_deref();
        let total_arity = usize::from(prepended.is_some()) + args.len();

        // 1. Higher-order pure combinators, AFC scope/contraction operators, & scalar `select`/`mux`
        if let Some(cname) = simple_name {
            match (cname, total_arity) {
                ("walk", 3) | ("settle", 3) => {
                    let is_settle = cname == "settle";
                    let (cur0, count_expr, fn_expr) = if let Some(in_v) = prepended {
                        (in_v, &args[0], &args[1])
                    } else {
                        (self.emit_expr(cg, &args[0])?, &args[1], &args[2])
                    };
                    let count_val = self.emit_expr(cg, count_expr)?;
                    let fn_target = self.resolve_lowered_target(cg, fn_expr, line)?;
                    return self.emit_walk_or_settle(
                        cg,
                        is_settle,
                        cur0,
                        &count_val.reg,
                        &fn_target,
                        line,
                    );
                }
                ("fold", 3) => {
                    let (seq_v, init_expr, fn_expr) = if let Some(in_v) = prepended {
                        (in_v, &args[0], &args[1])
                    } else {
                        (self.emit_expr(cg, &args[0])?, &args[1], &args[2])
                    };
                    let seq_v = self.materialize_if_range(seq_v);
                    let init_val = self.emit_expr(cg, init_expr)?;
                    let fn_target = self.resolve_lowered_target(cg, fn_expr, line)?;
                    return self.emit_ho_fold_or_scan(
                        cg, false, seq_v, init_val, &fn_target, line,
                    );
                }
                ("each", 2) => {
                    let (seq_v, fn_expr) = if let Some(in_v) = prepended {
                        (in_v, &args[0])
                    } else {
                        (self.emit_expr(cg, &args[0])?, &args[1])
                    };
                    let seq_v = self.materialize_if_range(seq_v);
                    let fn_target = self.resolve_lowered_target(cg, fn_expr, line)?;
                    return self.emit_ho_each(cg, seq_v, &fn_target, line);
                }
                ("evolve", 3 | 4) => {
                    return self.emit_afc_evolve(cg, prepended, args, line);
                }
                ("scan", 3 | 4) => {
                    return self.emit_afc_scan_or_fold(cg, true, prepended, args, line);
                }
                ("fold_seq", 3 | 4) => {
                    return self.emit_afc_scan_or_fold(cg, false, prepended, args, line);
                }
                ("map_seq", 2 | 3) => {
                    return self.emit_afc_map_seq(cg, prepended, args, line);
                }
                ("filter_seq", 2 | 3) => {
                    return self.emit_afc_filter_seq(cg, prepended, args, line);
                }
                ("tabulate", 2 | 3) => {
                    return self.emit_afc_tabulate(cg, prepended, args, line);
                }
                ("zip_seq", 2) => {
                    return self.emit_afc_zip_seq(cg, prepended, args, hint, line);
                }
                ("count", 2 | 3) => {
                    return self.emit_afc_count(cg, prepended, args, line);
                }
                ("any" | "all", 2 | 3) => {
                    return self.emit_afc_any_or_all(
                        cg,
                        cname == "any",
                        prepended,
                        callee,
                        args,
                        hint,
                        line,
                    );
                }
                ("sum", 1) | ("dot", 2) => {
                    return self.emit_sum_or_dot(cg, cname, prepended, callee, args, hint, line);
                }
                ("array", n) if n >= 1 => {
                    let elem_hint: Option<&Type> = match hint.map(Type::unwrap_read_only) {
                        Some(Type::Array(e, _)) => Some(e),
                        _ => None,
                    };
                    let mut vals = Vec::with_capacity(n);
                    if let Some(in_v) = prepended {
                        vals.push(in_v);
                    }
                    for a in args {
                        vals.push(self.emit_expr_with_hint(cg, a, elem_hint)?);
                    }
                    let elem_ty = vals[0].dec_ty.clone();
                    let handles: Vec<String> =
                        vals.iter().map(|v| self.box_val_to_handle(v)).collect();
                    let h = self.emit_handle_array_call("aod_rt_array_new", &handles);
                    return Ok(Val {
                        reg: h,
                        dec_ty: Type::Array(Box::new(elem_ty), n),
                    });
                }
                ("select", 3) => {
                    let (cv, t_expr, f_expr) = if let Some(in_v) = prepended {
                        (in_v, &args[0], &args[1])
                    } else {
                        (self.emit_expr(cg, &args[0])?, &args[1], &args[2])
                    };
                    let c_base = cv.dec_ty.unwrap_read_only();
                    if *c_base == Type::Bool
                        || matches!(c_base, Type::QExt(inner) if **inner == Type::Bool)
                    {
                        return self.emit_scalar_select(cg, &cv.reg, t_expr, f_expr, hint);
                    }
                    let tv = self.emit_expr_with_hint(cg, t_expr, hint)?;
                    let fv = self.emit_expr_with_hint(cg, f_expr, hint)?;
                    return self.emit_rank_select_loop(cg, cv, tv, fv, line);
                }
                ("mux", 4) => {
                    let (rv, n_expr, z_expr, p_expr) = if let Some(in_v) = prepended {
                        (in_v, &args[0], &args[1], &args[2])
                    } else {
                        (
                            self.emit_expr(cg, &args[0])?,
                            &args[1],
                            &args[2],
                            &args[3],
                        )
                    };
                    if *rv.dec_ty.unwrap_read_only() == Type::T1 {
                        return self.emit_scalar_mux(cg, &rv.reg, n_expr, z_expr, p_expr, hint);
                    }
                    let nv = self.emit_expr_with_hint(cg, n_expr, hint)?;
                    let zv = self.emit_expr_with_hint(cg, z_expr, hint)?;
                    let pv = self.emit_expr_with_hint(cg, p_expr, hint)?;
                    return self.emit_rank_mux_loop(cg, rv, nv, zv, pv, line);
                }
                _ => {}
            }
        }

        // 2. Enum payload constructor `Enum::Variant(args...)`
        if let ExprKind::Path(ref segs) = callee.kind {
            if let Some((em, vidx)) = cg.resolve_enum_variant(&self.mod_key, segs) {
                let em_mod = em.module.clone();
                let em_name = em.name.clone();
                let (vname, vtag, payload_tys) = em.variants[vidx].clone();
                let mut vals = Vec::with_capacity(total_arity);
                if let Some(in_v) = prepended {
                    vals.push(in_v);
                }
                for a in args {
                    let phint = payload_tys.get(vals.len());
                    vals.push(self.emit_expr_with_hint(cg, a, phint)?);
                }
                let mh = self.emit_str_handle(cg, &em_mod);
                let nh = self.emit_str_handle(cg, &em_name);
                let vh = self.emit_str_handle(cg, &vname);
                let phs: Vec<String> = vals.iter().map(|v| self.box_val_to_handle(v)).collect();
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

        // 3. Positional struct constructor `Foo(e1, e2, ...)` (falling through to `dec Foo:` if `struct_ov:` recorded!)
        let sp_opt: Option<Vec<String>> = match &callee.kind {
            ExprKind::Ident(n) => Some(vec![n.clone()]),
            ExprKind::Path(segs) => Some(segs.clone()),
            _ => None,
        };
        let is_struct_overload = cg.checked.poly_call_types.contains_key(&(
            self.mod_key.clone(),
            line,
            format!("struct_ov:{callee:?}:{args:?}"),
        ));
        if !is_struct_overload {
            if let Some(ref sp) = sp_opt {
                if let Some(sm) = cg.resolve_struct(&self.mod_key, sp).cloned() {
                    if total_arity == sm.fields.len() {
                        let mut pairs = Vec::with_capacity(sm.fields.len());
                        let mut f_idx = 0usize;
                        if let Some(in_v) = prepended {
                            pairs.push((sm.fields[0].0.clone(), in_v));
                            f_idx = 1;
                        }
                        for a in args {
                            let fhint = sm.fields.get(f_idx).map(|(_, t)| t);
                            let v = self.emit_expr_with_hint(cg, a, fhint)?;
                            pairs.push((sm.fields[f_idx].0.clone(), v));
                            f_idx += 1;
                        }
                        return self.emit_struct_construct(cg, &sm, &pairs);
                    }
                }
            }
        }

        // 4. Resolve function target & evaluate arguments with parameter hints when unique
        let target = self.resolve_lowered_target(cg, callee, line)?;
        let param_hints: Option<Vec<Type>> = match &target {
            LoweredCallTarget::UserFn(ovs)
                if ovs.len() == 1 && ovs[0].param_types.len() == total_arity =>
            {
                Some(ovs[0].param_types.clone())
            }
            LoweredCallTarget::Builtin(canon) => {
                let sigs = lookup_builtin(canon);
                if sigs.len() == 1 && sigs[0].params.len() == total_arity {
                    Some(sigs[0].params.clone())
                } else {
                    None
                }
            }
            _ => None,
        };

        let mut vals = Vec::with_capacity(total_arity);
        let mut arg_pos = 0usize;
        if let Some(in_v) = prepended {
            vals.push(in_v);
            arg_pos = 1;
        }
        for a in args {
            let ah = param_hints.as_ref().and_then(|ps| ps.get(arg_pos));
            vals.push(self.emit_expr_with_hint(cg, a, ah)?);
            arg_pos += 1;
        }

        self.invoke_lowered_target(cg, &target, vals, hint, line)
    }

    fn emit_scalar_select(
        &mut self,
        cg: &mut Codegen<'_>,
        cond_reg: &str,
        t_expr: &Expr,
        f_expr: &Expr,
        hint: Option<&Type>,
    ) -> DecResult<Val> {
        if cg.plan.branchless_select
            && self.is_pure_select_safe(cg, t_expr)
            && self.is_pure_select_safe(cg, f_expr)
        {
            let tv = self.emit_expr_with_hint(cg, t_expr, hint)?;
            let fv = self.emit_expr_with_hint(cg, f_expr, hint)?;
            let lt = tv.llvm_ty();
            if lt == LlvmTy::Void {
                return Ok(Val {
                    reg: String::new(),
                    dec_ty: tv.dec_ty,
                });
            }
            let sel = self.fresh_reg();
            self.line(&format!(
                "  {sel} = select i1 {cond_reg}, {} {}, {} {}",
                lt.ir(),
                tv.reg,
                lt.ir(),
                fv.reg
            ));
            return Ok(Val {
                reg: sel,
                dec_ty: tv.dec_ty,
            });
        }

        let t_lbl = self.fresh_label("sel_t");
        let f_lbl = self.fresh_label("sel_f");
        let j_lbl = self.fresh_label("sel_j");
        self.line(&format!(
            "  br i1 {cond_reg}, label %{t_lbl}, label %{f_lbl}"
        ));

        self.start_block(&t_lbl);
        let tv = self.emit_expr_with_hint(cg, t_expr, hint)?;
        let out_ty = if type_contains_unknown(&tv.dec_ty) {
            hint.cloned().unwrap_or(tv.dec_ty.clone())
        } else {
            tv.dec_ty.clone()
        };
        let lt = LlvmTy::from_dec(&out_ty);
        let slot = if lt != LlvmTy::Void {
            let s = self.alloc_temp(lt.ir());
            self.line(&format!("  store {} {}, ptr {s}", lt.ir(), tv.reg));
            Some(s)
        } else {
            None
        };
        if !self.terminated {
            self.line(&format!("  br label %{j_lbl}"));
            self.terminated = true;
        }

        self.start_block(&f_lbl);
        let fv = self.emit_expr_with_hint(cg, f_expr, Some(&out_ty))?;
        let final_ty = if type_contains_unknown(&out_ty) {
            fv.dec_ty.clone()
        } else {
            out_ty
        };
        if let Some(ref s) = slot {
            self.line(&format!("  store {} {}, ptr {s}", lt.ir(), fv.reg));
        }
        if !self.terminated {
            self.line(&format!("  br label %{j_lbl}"));
            self.terminated = true;
        }

        self.start_block(&j_lbl);
        let reg = match slot {
            Some(s) => {
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load {}, ptr {s}", lt.ir()));
                r
            }
            None => String::new(),
        };
        Ok(Val {
            reg,
            dec_ty: final_ty,
        })
    }

    fn emit_scalar_mux(
        &mut self,
        cg: &mut Codegen<'_>,
        role_reg: &str,
        n_expr: &Expr,
        z_expr: &Expr,
        p_expr: &Expr,
        hint: Option<&Type>,
    ) -> DecResult<Val> {
        if cg.plan.branchless_select
            && self.is_pure_select_safe(cg, n_expr)
            && self.is_pure_select_safe(cg, z_expr)
            && self.is_pure_select_safe(cg, p_expr)
        {
            let nv = self.emit_expr_with_hint(cg, n_expr, hint)?;
            let zv = self.emit_expr_with_hint(cg, z_expr, hint)?;
            let pv = self.emit_expr_with_hint(cg, p_expr, hint)?;
            let lt = nv.llvm_ty();
            if lt == LlvmTy::Void {
                return Ok(Val {
                    reg: String::new(),
                    dec_ty: nv.dec_ty,
                });
            }
            let is_neg = self.fresh_reg();
            let is_zero = self.fresh_reg();
            let s1 = self.fresh_reg();
            let s2 = self.fresh_reg();
            self.line(&format!("  {is_neg} = icmp slt i8 {role_reg}, 0"));
            self.line(&format!("  {is_zero} = icmp eq i8 {role_reg}, 0"));
            self.line(&format!(
                "  {s1} = select i1 {is_zero}, {} {}, {} {}",
                lt.ir(),
                zv.reg,
                lt.ir(),
                pv.reg
            ));
            self.line(&format!(
                "  {s2} = select i1 {is_neg}, {} {}, {} {s1}",
                lt.ir(),
                nv.reg,
                lt.ir()
            ));
            return Ok(Val {
                reg: s2,
                dec_ty: nv.dec_ty,
            });
        }

        let is_neg = self.fresh_reg();
        let neg_lbl = self.fresh_label("mux_neg");
        let chk0_lbl = self.fresh_label("mux_chk0");
        let zero_lbl = self.fresh_label("mux_zero");
        let pos_lbl = self.fresh_label("mux_pos");
        let join_lbl = self.fresh_label("mux_join");
        self.line(&format!("  {is_neg} = icmp slt i8 {role_reg}, 0"));
        self.line(&format!(
            "  br i1 {is_neg}, label %{neg_lbl}, label %{chk0_lbl}"
        ));

        self.start_block(&chk0_lbl);
        let is_z = self.fresh_reg();
        self.line(&format!("  {is_z} = icmp eq i8 {role_reg}, 0"));
        self.line(&format!(
            "  br i1 {is_z}, label %{zero_lbl}, label %{pos_lbl}"
        ));

        self.start_block(&neg_lbl);
        let nv = self.emit_expr_with_hint(cg, n_expr, hint)?;
        let mut out_ty = if type_contains_unknown(&nv.dec_ty) {
            hint.cloned().unwrap_or(nv.dec_ty.clone())
        } else {
            nv.dec_ty.clone()
        };
        let lt = LlvmTy::from_dec(&out_ty);
        let slot = if lt != LlvmTy::Void {
            let s = self.alloc_temp(lt.ir());
            self.line(&format!("  store {} {}, ptr {s}", lt.ir(), nv.reg));
            Some(s)
        } else {
            None
        };
        if !self.terminated {
            self.line(&format!("  br label %{join_lbl}"));
            self.terminated = true;
        }

        self.start_block(&zero_lbl);
        let zv = self.emit_expr_with_hint(cg, z_expr, Some(&out_ty))?;
        if type_contains_unknown(&out_ty) {
            out_ty = zv.dec_ty.clone();
        }
        if let Some(ref s) = slot {
            self.line(&format!("  store {} {}, ptr {s}", lt.ir(), zv.reg));
        }
        if !self.terminated {
            self.line(&format!("  br label %{join_lbl}"));
            self.terminated = true;
        }

        self.start_block(&pos_lbl);
        let pv = self.emit_expr_with_hint(cg, p_expr, Some(&out_ty))?;
        if type_contains_unknown(&out_ty) {
            out_ty = pv.dec_ty.clone();
        }
        if let Some(ref s) = slot {
            self.line(&format!("  store {} {}, ptr {s}", lt.ir(), pv.reg));
        }
        if !self.terminated {
            self.line(&format!("  br label %{join_lbl}"));
            self.terminated = true;
        }

        self.start_block(&join_lbl);
        let reg = match slot {
            Some(s) => {
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load {}, ptr {s}", lt.ir()));
                r
            }
            None => String::new(),
        };
        Ok(Val {
            reg,
            dec_ty: out_ty,
        })
    }

    fn emit_rank_select_loop(
        &mut self,
        cg: &mut Codegen<'_>,
        cv: Val,
        tv: Val,
        fv: Val,
        line: usize,
    ) -> DecResult<Val> {
        let c_base = cv.dec_ty.unwrap_read_only().clone();
        if c_base == Type::Bool || matches!(c_base, Type::QExt(ref inner) if **inner == Type::Bool)
        {
            let lt = tv.llvm_ty();
            if lt == LlvmTy::Void {
                return Ok(tv);
            }
            let r = self.fresh_reg();
            self.line(&format!(
                "  {r} = select i1 {}, {} {}, {} {}",
                cv.reg,
                lt.ir(),
                tv.reg,
                lt.ir(),
                fv.reg
            ));
            return Ok(Val {
                reg: r,
                dec_ty: tv.dec_ty,
            });
        }

        let (ce, fixed_len) = extract_seq_elem_and_fixed(&cv.dec_ty);
        let cr = type_seq_rank(&cv.dec_ty);
        let tr = type_seq_rank(&tv.dec_ty);
        let fr = type_seq_rank(&fv.dec_ty);

        let len = self.fresh_reg();
        self.line(&format!(
            "  {len} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {})",
            cv.reg
        ));
        self.emit_fail_return_check();

        let buf_id = self.fresh_reg();
        self.line(&format!(
            "  {buf_id} = call i64 @aod_rt_seq_buf_new(ptr %ctx, i64 {len})"
        ));
        let idx_slot = self.alloc_temp("i64");
        self.line(&format!("  store i64 0, ptr {idx_slot}"));
        let cond_lbl = self.fresh_label("rsel_cond");
        let body_lbl = self.fresh_label("rsel_body");
        let end_lbl = self.fresh_label("rsel_end");
        self.line(&format!("  br label %{cond_lbl}"));

        self.start_block(&cond_lbl);
        let idx = self.fresh_reg();
        let cmp = self.fresh_reg();
        self.line(&format!("  {idx} = load i64, ptr {idx_slot}"));
        self.line(&format!("  {cmp} = icmp slt i64 {idx}, {len}"));
        self.line(&format!(
            "  br i1 {cmp}, label %{body_lbl}, label %{end_lbl}"
        ));

        self.start_block(&body_lbl);
        self.emit_fuel_tick();
        let ch = self.fresh_reg();
        self.line(&format!(
            "  {ch} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {idx})",
            cv.reg
        ));
        let ci = self.unbox_handle_to_val(&ch, &ce);
        let ti = if tr >= cr && tr > 0 {
            let (te, _) = extract_seq_elem_and_fixed(&tv.dec_ty);
            let h = self.fresh_reg();
            self.line(&format!(
                "  {h} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {idx})",
                tv.reg
            ));
            self.unbox_handle_to_val(&h, &te)
        } else {
            tv.clone()
        };
        let fi = if fr >= cr && fr > 0 {
            let (fe, _) = extract_seq_elem_and_fixed(&fv.dec_ty);
            let h = self.fresh_reg();
            self.line(&format!(
                "  {h} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {idx})",
                fv.reg
            ));
            self.unbox_handle_to_val(&h, &fe)
        } else {
            fv.clone()
        };
        let elem = self.emit_rank_select_loop(cg, ci, ti, fi, line)?;
        let out_elem_ty = elem.dec_ty.clone();
        let eh = self.box_val_to_handle(&elem);
        self.line(&format!(
            "  call void @aod_rt_seq_buf_push(ptr %ctx, i64 {buf_id}, i64 {eh})"
        ));
        let nxt = self.fresh_reg();
        self.line(&format!("  {nxt} = add nsw i64 {idx}, 1"));
        self.line(&format!("  store i64 {nxt}, ptr {idx_slot}"));
        let md_suffix = cg.alloc_loop_hint_md().unwrap_or_default();
        self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
        self.terminated = true;

        self.start_block(&end_lbl);
        let is_arr = u8::from(fixed_len.is_some());
        let out_h = self.fresh_reg();
        self.line(&format!(
            "  {out_h} = call i64 @aod_rt_seq_buf_finish(ptr %ctx, i64 {buf_id}, i8 {is_arr})"
        ));
        self.emit_fail_return_check();
        let out_ty = match fixed_len {
            Some(n) => Type::Array(Box::new(out_elem_ty), n),
            None => Type::List(Box::new(out_elem_ty)),
        };
        Ok(Val {
            reg: out_h,
            dec_ty: out_ty,
        })
    }

    fn emit_rank_mux_loop(
        &mut self,
        cg: &mut Codegen<'_>,
        rv: Val,
        nv: Val,
        zv: Val,
        pv: Val,
        line: usize,
    ) -> DecResult<Val> {
        if *rv.dec_ty.unwrap_read_only() == Type::T1 {
            let lt = nv.llvm_ty();
            if lt == LlvmTy::Void {
                return Ok(nv);
            }
            let is_neg = self.fresh_reg();
            let is_zero = self.fresh_reg();
            let s1 = self.fresh_reg();
            let s2 = self.fresh_reg();
            self.line(&format!("  {is_neg} = icmp slt i8 {}, 0", rv.reg));
            self.line(&format!("  {is_zero} = icmp eq i8 {}, 0", rv.reg));
            self.line(&format!(
                "  {s1} = select i1 {is_zero}, {} {}, {} {}",
                lt.ir(),
                zv.reg,
                lt.ir(),
                pv.reg
            ));
            self.line(&format!(
                "  {s2} = select i1 {is_neg}, {} {}, {} {s1}",
                lt.ir(),
                nv.reg,
                lt.ir()
            ));
            return Ok(Val {
                reg: s2,
                dec_ty: nv.dec_ty,
            });
        }

        let (re, fixed_len) = extract_seq_elem_and_fixed(&rv.dec_ty);
        let rr = type_seq_rank(&rv.dec_ty);
        let nr = type_seq_rank(&nv.dec_ty);
        let zr = type_seq_rank(&zv.dec_ty);
        let pr = type_seq_rank(&pv.dec_ty);

        let len = self.fresh_reg();
        self.line(&format!(
            "  {len} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {})",
            rv.reg
        ));
        self.emit_fail_return_check();

        let buf_id = self.fresh_reg();
        self.line(&format!(
            "  {buf_id} = call i64 @aod_rt_seq_buf_new(ptr %ctx, i64 {len})"
        ));
        let idx_slot = self.alloc_temp("i64");
        self.line(&format!("  store i64 0, ptr {idx_slot}"));
        let cond_lbl = self.fresh_label("rmux_cond");
        let body_lbl = self.fresh_label("rmux_body");
        let end_lbl = self.fresh_label("rmux_end");
        self.line(&format!("  br label %{cond_lbl}"));

        self.start_block(&cond_lbl);
        let idx = self.fresh_reg();
        let cmp = self.fresh_reg();
        self.line(&format!("  {idx} = load i64, ptr {idx_slot}"));
        self.line(&format!("  {cmp} = icmp slt i64 {idx}, {len}"));
        self.line(&format!(
            "  br i1 {cmp}, label %{body_lbl}, label %{end_lbl}"
        ));

        self.start_block(&body_lbl);
        self.emit_fuel_tick();
        let rh = self.fresh_reg();
        self.line(&format!(
            "  {rh} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {idx})",
            rv.reg
        ));
        let ri = self.unbox_handle_to_val(&rh, &re);
        let ni = if nr >= rr && nr > 0 {
            let (ne, _) = extract_seq_elem_and_fixed(&nv.dec_ty);
            let h = self.fresh_reg();
            self.line(&format!(
                "  {h} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {idx})",
                nv.reg
            ));
            self.unbox_handle_to_val(&h, &ne)
        } else {
            nv.clone()
        };
        let zi = if zr >= rr && zr > 0 {
            let (ze, _) = extract_seq_elem_and_fixed(&zv.dec_ty);
            let h = self.fresh_reg();
            self.line(&format!(
                "  {h} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {idx})",
                zv.reg
            ));
            self.unbox_handle_to_val(&h, &ze)
        } else {
            zv.clone()
        };
        let pi = if pr >= rr && pr > 0 {
            let (pe, _) = extract_seq_elem_and_fixed(&pv.dec_ty);
            let h = self.fresh_reg();
            self.line(&format!(
                "  {h} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {idx})",
                pv.reg
            ));
            self.unbox_handle_to_val(&h, &pe)
        } else {
            pv.clone()
        };
        let elem = self.emit_rank_mux_loop(cg, ri, ni, zi, pi, line)?;
        let out_elem_ty = elem.dec_ty.clone();
        let eh = self.box_val_to_handle(&elem);
        self.line(&format!(
            "  call void @aod_rt_seq_buf_push(ptr %ctx, i64 {buf_id}, i64 {eh})"
        ));
        let nxt = self.fresh_reg();
        self.line(&format!("  {nxt} = add nsw i64 {idx}, 1"));
        self.line(&format!("  store i64 {nxt}, ptr {idx_slot}"));
        let md_suffix = cg.alloc_loop_hint_md().unwrap_or_default();
        self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
        self.terminated = true;

        self.start_block(&end_lbl);
        let is_arr = u8::from(fixed_len.is_some());
        let out_h = self.fresh_reg();
        self.line(&format!(
            "  {out_h} = call i64 @aod_rt_seq_buf_finish(ptr %ctx, i64 {buf_id}, i8 {is_arr})"
        ));
        self.emit_fail_return_check();
        let out_ty = match fixed_len {
            Some(n) => Type::Array(Box::new(out_elem_ty), n),
            None => Type::List(Box::new(out_elem_ty)),
        };
        Ok(Val {
            reg: out_h,
            dec_ty: out_ty,
        })
    }

    fn emit_gate_val(
        &mut self,
        cg: &mut Codegen<'_>,
        cv: Val,
        vv: Val,
        line: usize,
    ) -> DecResult<Val> {
        if *cv.dec_ty.unwrap_read_only() == Type::Bool {
            return match vv.dec_ty.unwrap_read_only().clone() {
                Type::Int => {
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = select i1 {}, i64 {}, i64 0", cv.reg, vv.reg));
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::Int,
                    })
                }
                Type::T1 => {
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = select i1 {}, i8 {}, i8 0", cv.reg, vv.reg));
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::T1,
                    })
                }
                Type::Ratio => {
                    let r = self.fresh_reg();
                    self.line(&format!(
                        "  {r} = select i1 {}, %DecRatio {}, %DecRatio {{ i64 0, i64 1 }}",
                        cv.reg, vv.reg
                    ));
                    Ok(Val {
                        reg: r,
                        dec_ty: Type::Ratio,
                    })
                }
                Type::Array(elem_ty, n) => {
                    self.emit_zero_or_keep_seq_gate(cg, &cv.reg, &vv.reg, &elem_ty, Some(n), line)
                }
                Type::List(elem_ty) => {
                    self.emit_zero_or_keep_seq_gate(cg, &cv.reg, &vv.reg, &elem_ty, None, line)
                }
                other => Err(Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("codegen: gate requires numeric value, got {other}"),
                )),
            };
        }

        let (ce, fixed_len) = extract_seq_elem_and_fixed(&cv.dec_ty);
        let cr = type_seq_rank(&cv.dec_ty);
        let vr = type_seq_rank(&vv.dec_ty);
        let len = self.fresh_reg();
        self.line(&format!(
            "  {len} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {})",
            cv.reg
        ));
        self.emit_fail_return_check();
        let buf_id = self.fresh_reg();
        self.line(&format!(
            "  {buf_id} = call i64 @aod_rt_seq_buf_new(ptr %ctx, i64 {len})"
        ));
        let idx_slot = self.alloc_temp("i64");
        self.line(&format!("  store i64 0, ptr {idx_slot}"));
        let cond_lbl = self.fresh_label("gate_cond");
        let body_lbl = self.fresh_label("gate_body");
        let end_lbl = self.fresh_label("gate_end");
        self.line(&format!("  br label %{cond_lbl}"));

        self.start_block(&cond_lbl);
        let idx = self.fresh_reg();
        let cmp = self.fresh_reg();
        self.line(&format!("  {idx} = load i64, ptr {idx_slot}"));
        self.line(&format!("  {cmp} = icmp slt i64 {idx}, {len}"));
        self.line(&format!(
            "  br i1 {cmp}, label %{body_lbl}, label %{end_lbl}"
        ));

        self.start_block(&body_lbl);
        self.emit_fuel_tick();
        let ch = self.fresh_reg();
        self.line(&format!(
            "  {ch} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {idx})",
            cv.reg
        ));
        let ci = self.unbox_handle_to_val(&ch, &ce);
        let vi = if vr >= cr && vr > 0 {
            let (ve, _) = extract_seq_elem_and_fixed(&vv.dec_ty);
            let h = self.fresh_reg();
            self.line(&format!(
                "  {h} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {idx})",
                vv.reg
            ));
            self.unbox_handle_to_val(&h, &ve)
        } else {
            vv.clone()
        };
        let elem = self.emit_gate_val(cg, ci, vi, line)?;
        let out_elem_ty = elem.dec_ty.clone();
        let eh = self.box_val_to_handle(&elem);
        self.line(&format!(
            "  call void @aod_rt_seq_buf_push(ptr %ctx, i64 {buf_id}, i64 {eh})"
        ));
        let nxt = self.fresh_reg();
        self.line(&format!("  {nxt} = add nsw i64 {idx}, 1"));
        self.line(&format!("  store i64 {nxt}, ptr {idx_slot}"));
        let md_suffix = cg.alloc_loop_hint_md().unwrap_or_default();
        self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
        self.terminated = true;

        self.start_block(&end_lbl);
        let is_arr = u8::from(fixed_len.is_some());
        let out_h = self.fresh_reg();
        self.line(&format!(
            "  {out_h} = call i64 @aod_rt_seq_buf_finish(ptr %ctx, i64 {buf_id}, i8 {is_arr})"
        ));
        self.emit_fail_return_check();
        let out_ty = match fixed_len {
            Some(n) => Type::Array(Box::new(out_elem_ty), n),
            None => Type::List(Box::new(out_elem_ty)),
        };
        Ok(Val {
            reg: out_h,
            dec_ty: out_ty,
        })
    }

    fn emit_zero_or_keep_seq_gate(
        &mut self,
        cg: &mut Codegen<'_>,
        cond_reg: &str,
        seq_h: &str,
        elem_ty: &Type,
        fixed_len: Option<usize>,
        line: usize,
    ) -> DecResult<Val> {
        let len = self.fresh_reg();
        self.line(&format!(
            "  {len} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {seq_h})"
        ));
        self.emit_fail_return_check();
        let buf_id = self.fresh_reg();
        self.line(&format!(
            "  {buf_id} = call i64 @aod_rt_seq_buf_new(ptr %ctx, i64 {len})"
        ));
        let idx_slot = self.alloc_temp("i64");
        self.line(&format!("  store i64 0, ptr {idx_slot}"));
        let cond_lbl = self.fresh_label("zgate_cond");
        let body_lbl = self.fresh_label("zgate_body");
        let end_lbl = self.fresh_label("zgate_end");
        self.line(&format!("  br label %{cond_lbl}"));

        self.start_block(&cond_lbl);
        let idx = self.fresh_reg();
        let cmp = self.fresh_reg();
        self.line(&format!("  {idx} = load i64, ptr {idx_slot}"));
        self.line(&format!("  {cmp} = icmp slt i64 {idx}, {len}"));
        self.line(&format!(
            "  br i1 {cmp}, label %{body_lbl}, label %{end_lbl}"
        ));

        self.start_block(&body_lbl);
        self.emit_fuel_tick();
        let eh = self.fresh_reg();
        self.line(&format!(
            "  {eh} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {seq_h}, i64 {idx})"
        ));
        let ev = self.unbox_handle_to_val(&eh, elem_ty);
        let cval = Val {
            reg: cond_reg.to_string(),
            dec_ty: Type::Bool,
        };
        let gated = self.emit_gate_val(cg, cval, ev, line)?;
        let gh = self.box_val_to_handle(&gated);
        self.line(&format!(
            "  call void @aod_rt_seq_buf_push(ptr %ctx, i64 {buf_id}, i64 {gh})"
        ));
        let nxt = self.fresh_reg();
        self.line(&format!("  {nxt} = add nsw i64 {idx}, 1"));
        self.line(&format!("  store i64 {nxt}, ptr {idx_slot}"));
        self.line(&format!("  br label %{cond_lbl}"));
        self.terminated = true;

        self.start_block(&end_lbl);
        let is_arr = u8::from(fixed_len.is_some());
        let out_h = self.fresh_reg();
        self.line(&format!(
            "  {out_h} = call i64 @aod_rt_seq_buf_finish(ptr %ctx, i64 {buf_id}, i8 {is_arr})"
        ));
        self.emit_fail_return_check();
        let out_ty = match fixed_len {
            Some(n) => Type::Array(Box::new(elem_ty.clone()), n),
            None => Type::List(Box::new(elem_ty.clone())),
        };
        Ok(Val {
            reg: out_h,
            dec_ty: out_ty,
        })
    }

    fn emit_walk_or_settle(
        &mut self,
        cg: &mut Codegen<'_>,
        is_settle: bool,
        cur0: Val,
        count_reg: &str,
        fn_target: &LoweredCallTarget,
        line: usize,
    ) -> DecResult<Val> {
        let state_ty = cur0.dec_ty.unwrap_read_only().clone();
        let lt = LlvmTy::from_dec(&state_ty);
        let state_slot = if lt != LlvmTy::Void {
            let s = self.alloc_temp(lt.ir());
            self.line(&format!("  store {} {}, ptr {s}", lt.ir(), cur0.reg));
            Some(s)
        } else {
            None
        };

        // Unpack non-negative iteration count `n` via `aod_rt_b64_from_unsigned_int`
        let n_u64 = self.fresh_reg();
        self.line(&format!(
            "  {n_u64} = call i64 @aod_rt_b64_from_unsigned_int(ptr %ctx, i64 {count_reg})"
        ));
        self.emit_fail_return_check();
        let fits_i64 = self.fresh_reg();
        let n_ok = self.fresh_label("walk_n_ok");
        let n_bad = self.fresh_label("walk_n_bad");
        self.line(&format!("  {fits_i64} = icmp sge i64 {n_u64}, 0"));
        self.line(&format!("  br i1 {fits_i64}, label %{n_ok}, label %{n_bad}"));
        self.start_block(&n_bad);
        let (mg, ml) = cg.intern_bytes(b"walk/settle count exceeds i64::MAX");
        self.line(&format!(
            "  call void @aod_rt_fail(ptr %ctx, ptr {mg}, i64 {ml})"
        ));
        self.emit_depth_dec();
        self.emit_default_ret();

        self.start_block(&n_ok);
        let takes_two = match fn_target {
            LoweredCallTarget::UserFn(ovs) => ovs.iter().any(|o| {
                o.param_types.len() == 2
                    && types_match_for_call(&state_ty, o.param_types[0].unwrap_read_only())
            }),
            LoweredCallTarget::Builtin(_) => true,
        };

        let idx_slot = self.alloc_temp("i64");
        self.line(&format!("  store i64 0, ptr {idx_slot}"));
        let cond_lbl = self.fresh_label("walk_cond");
        let body_lbl = self.fresh_label("walk_body");
        let step_lbl = self.fresh_label("walk_step");
        let exhaust_lbl = self.fresh_label("walk_exhaust");
        let done_lbl = self.fresh_label("walk_done");
        self.line(&format!("  br label %{cond_lbl}"));

        self.start_block(&cond_lbl);
        let idx = self.fresh_reg();
        let cmp = self.fresh_reg();
        self.line(&format!("  {idx} = load i64, ptr {idx_slot}"));
        self.line(&format!("  {cmp} = icmp slt i64 {idx}, {n_u64}"));
        let false_lbl = if is_settle { &exhaust_lbl } else { &done_lbl };
        self.line(&format!(
            "  br i1 {cmp}, label %{body_lbl}, label %{false_lbl}"
        ));

        self.start_block(&body_lbl);
        self.emit_fuel_tick();
        let cur_val = match state_slot {
            Some(ref s) => {
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load {}, ptr {s}", lt.ir()));
                Val {
                    reg: r,
                    dec_ty: state_ty.clone(),
                }
            }
            None => Val {
                reg: String::new(),
                dec_ty: state_ty.clone(),
            },
        };
        let call_args = if takes_two {
            let step_w = self.fresh_reg();
            self.line(&format!(
                "  {step_w} = call i64 @aod_rt_int_from_i64(ptr %ctx, i64 {idx})"
            ));
            vec![
                cur_val.clone(),
                Val {
                    reg: step_w,
                    dec_ty: Type::Int,
                },
            ]
        } else {
            vec![cur_val.clone()]
        };
        let next_val = self.invoke_lowered_target(cg, fn_target, call_args, Some(&state_ty), line)?;
        if let Some(ref s) = state_slot {
            self.line(&format!("  store {} {}, ptr {s}", lt.ir(), next_val.reg));
        }

        if is_settle {
            let eq_val = self.emit_binary(cg, BinaryOp::Eq, next_val, cur_val, line)?;
            self.line(&format!(
                "  br i1 {}, label %{done_lbl}, label %{step_lbl}",
                eq_val.reg
            ));
        } else {
            self.line(&format!("  br label %{step_lbl}"));
        }

        self.start_block(&step_lbl);
        let nxt = self.fresh_reg();
        self.line(&format!("  {nxt} = add nsw i64 {idx}, 1"));
        self.line(&format!("  store i64 {nxt}, ptr {idx_slot}"));
        let md_suffix = cg.alloc_loop_hint_md().unwrap_or_default();
        self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
        self.terminated = true;

        if is_settle {
            self.start_block(&exhaust_lbl);
            self.line("  call void @aod_rt_budget_exhausted(ptr %ctx, i8 0)");
            self.emit_depth_dec();
            self.emit_default_ret();
        }

        self.start_block(&done_lbl);
        let out_reg = match state_slot {
            Some(s) => {
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load {}, ptr {s}", lt.ir()));
                r
            }
            None => String::new(),
        };
        Ok(Val {
            reg: out_reg,
            dec_ty: state_ty,
        })
    }

    fn emit_ho_fold_or_scan(
        &mut self,
        cg: &mut Codegen<'_>,
        is_scan: bool,
        seq_v: Val,
        init_val: Val,
        fn_target: &LoweredCallTarget,
        line: usize,
    ) -> DecResult<Val> {
        let (elem_ty, fixed_len) = extract_seq_elem_and_fixed(&seq_v.dec_ty);
        let acc_ty = init_val.dec_ty.unwrap_read_only().clone();
        let lt = LlvmTy::from_dec(&acc_ty);
        let acc_slot = if lt != LlvmTy::Void {
            let s = self.alloc_temp(lt.ir());
            self.line(&format!("  store {} {}, ptr {s}", lt.ir(), init_val.reg));
            Some(s)
        } else {
            None
        };

        let len = self.fresh_reg();
        self.line(&format!(
            "  {len} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {})",
            seq_v.reg
        ));
        self.emit_fail_return_check();

        let buf_id = if is_scan {
            let b = self.fresh_reg();
            self.line(&format!(
                "  {b} = call i64 @aod_rt_seq_buf_new(ptr %ctx, i64 {len})"
            ));
            Some(b)
        } else {
            None
        };

        let idx_slot = self.alloc_temp("i64");
        self.line(&format!("  store i64 0, ptr {idx_slot}"));
        let cond_lbl = self.fresh_label("hof_cond");
        let body_lbl = self.fresh_label("hof_body");
        let end_lbl = self.fresh_label("hof_end");
        self.line(&format!("  br label %{cond_lbl}"));

        self.start_block(&cond_lbl);
        let idx = self.fresh_reg();
        let cmp = self.fresh_reg();
        self.line(&format!("  {idx} = load i64, ptr {idx_slot}"));
        self.line(&format!("  {cmp} = icmp slt i64 {idx}, {len}"));
        self.line(&format!(
            "  br i1 {cmp}, label %{body_lbl}, label %{end_lbl}"
        ));

        self.start_block(&body_lbl);
        self.emit_fuel_tick();
        let eh = self.fresh_reg();
        self.line(&format!(
            "  {eh} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {idx})",
            seq_v.reg
        ));
        let ev = self.unbox_handle_to_val(&eh, &elem_ty);
        let cur_acc = match acc_slot {
            Some(ref s) => {
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load {}, ptr {s}", lt.ir()));
                Val {
                    reg: r,
                    dec_ty: acc_ty.clone(),
                }
            }
            None => Val {
                reg: String::new(),
                dec_ty: acc_ty.clone(),
            },
        };
        let next_acc = self.invoke_lowered_target(
            cg,
            fn_target,
            vec![cur_acc, ev],
            Some(&acc_ty),
            line,
        )?;
        if let Some(ref s) = acc_slot {
            self.line(&format!("  store {} {}, ptr {s}", lt.ir(), next_acc.reg));
        }
        if let Some(ref b) = buf_id {
            let nh = self.box_val_to_handle(&next_acc);
            self.line(&format!(
                "  call void @aod_rt_seq_buf_push(ptr %ctx, i64 {b}, i64 {nh})"
            ));
        }
        let nxt = self.fresh_reg();
        self.line(&format!("  {nxt} = add nsw i64 {idx}, 1"));
        self.line(&format!("  store i64 {nxt}, ptr {idx_slot}"));
        let md_suffix = cg.alloc_loop_hint_md().unwrap_or_default();
        self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
        self.terminated = true;

        self.start_block(&end_lbl);
        if let Some(b) = buf_id {
            let is_arr = u8::from(fixed_len.is_some());
            let out_h = self.fresh_reg();
            self.line(&format!(
                "  {out_h} = call i64 @aod_rt_seq_buf_finish(ptr %ctx, i64 {b}, i8 {is_arr})"
            ));
            self.emit_fail_return_check();
            let out_ty = match fixed_len {
                Some(n) => Type::Array(Box::new(acc_ty), n),
                None => Type::List(Box::new(acc_ty)),
            };
            Ok(Val {
                reg: out_h,
                dec_ty: out_ty,
            })
        } else {
            let out_reg = match acc_slot {
                Some(s) => {
                    let r = self.fresh_reg();
                    self.line(&format!("  {r} = load {}, ptr {s}", lt.ir()));
                    r
                }
                None => String::new(),
            };
            Ok(Val {
                reg: out_reg,
                dec_ty: acc_ty,
            })
        }
    }

    fn emit_ho_each(
        &mut self,
        cg: &mut Codegen<'_>,
        seq_v: Val,
        fn_target: &LoweredCallTarget,
        line: usize,
    ) -> DecResult<Val> {
        let (elem_ty, fixed_len) = extract_seq_elem_and_fixed(&seq_v.dec_ty);
        let len = self.fresh_reg();
        self.line(&format!(
            "  {len} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {})",
            seq_v.reg
        ));
        self.emit_fail_return_check();
        let buf_id = self.fresh_reg();
        self.line(&format!(
            "  {buf_id} = call i64 @aod_rt_seq_buf_new(ptr %ctx, i64 {len})"
        ));
        let idx_slot = self.alloc_temp("i64");
        self.line(&format!("  store i64 0, ptr {idx_slot}"));
        let cond_lbl = self.fresh_label("each_cond");
        let body_lbl = self.fresh_label("each_body");
        let end_lbl = self.fresh_label("each_end");
        self.line(&format!("  br label %{cond_lbl}"));

        self.start_block(&cond_lbl);
        let idx = self.fresh_reg();
        let cmp = self.fresh_reg();
        self.line(&format!("  {idx} = load i64, ptr {idx_slot}"));
        self.line(&format!("  {cmp} = icmp slt i64 {idx}, {len}"));
        self.line(&format!(
            "  br i1 {cmp}, label %{body_lbl}, label %{end_lbl}"
        ));

        self.start_block(&body_lbl);
        self.emit_fuel_tick();
        let eh = self.fresh_reg();
        self.line(&format!(
            "  {eh} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {idx})",
            seq_v.reg
        ));
        let ev = self.unbox_handle_to_val(&eh, &elem_ty);
        let mapped = self.invoke_lowered_target(cg, fn_target, vec![ev], None, line)?;
        let out_elem_ty = mapped.dec_ty.clone();
        let mh = self.box_val_to_handle(&mapped);
        self.line(&format!(
            "  call void @aod_rt_seq_buf_push(ptr %ctx, i64 {buf_id}, i64 {mh})"
        ));
        let nxt = self.fresh_reg();
        self.line(&format!("  {nxt} = add nsw i64 {idx}, 1"));
        self.line(&format!("  store i64 {nxt}, ptr {idx_slot}"));
        let md_suffix = cg.alloc_loop_hint_md().unwrap_or_default();
        self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
        self.terminated = true;

        self.start_block(&end_lbl);
        let is_arr = u8::from(fixed_len.is_some());
        let out_h = self.fresh_reg();
        self.line(&format!(
            "  {out_h} = call i64 @aod_rt_seq_buf_finish(ptr %ctx, i64 {buf_id}, i8 {is_arr})"
        ));
        self.emit_fail_return_check();
        let out_ty = match fixed_len {
            Some(n) => Type::Array(Box::new(out_elem_ty), n),
            None => Type::List(Box::new(out_elem_ty)),
        };
        Ok(Val {
            reg: out_h,
            dec_ty: out_ty,
        })
    }

    fn unique_ho_param_hint_cg(
        target: &LoweredCallTarget,
        arity: usize,
        idx: usize,
    ) -> Option<Type> {
        match target {
            LoweredCallTarget::UserFn(ovs) => {
                let mut found: Option<Type> = None;
                for o in ovs {
                    if o.param_types.len() == arity {
                        let t = &o.param_types[idx];
                        if type_contains_unknown(t) {
                            return None;
                        }
                        match &found {
                            None => found = Some(t.clone()),
                            Some(prev) if prev != t => return None,
                            Some(_) => {}
                        }
                    }
                }
                found
            }
            LoweredCallTarget::Builtin(canon) => {
                let sigs = lookup_builtin(canon);
                let mut found: Option<Type> = None;
                for s in sigs {
                    if s.params.len() == arity {
                        let t = &s.params[idx];
                        match &found {
                            None => found = Some(t.clone()),
                            Some(prev) if prev != t => return None,
                            Some(_) => {}
                        }
                    }
                }
                found
            }
        }
    }

    fn extract_afc_seq_elem_cg(
        &mut self,
        seq_v: &Val,
        hint: Option<Type>,
    ) -> (String, Type, Type) {
        let len_r = self.fresh_reg();
        self.line(&format!(
            "  {len_r} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {})",
            seq_v.reg
        ));
        self.emit_fail_return_check();
        let is_ro = seq_v.dec_ty.is_read_only();
        match seq_v.dec_ty.unwrap_read_only() {
            Type::Bytes => (len_r, Type::B8, Type::B8),
            Type::Array(e, _) | Type::List(e) => {
                let raw = if **e == Type::Unknown {
                    hint.map(|h| h.unwrap_read_only().clone())
                        .filter(|h| *h != Type::Unknown)
                        .unwrap_or(Type::Int)
                } else {
                    (**e).clone()
                };
                let passed = if is_ro && raw.needs_readonly_propagation() {
                    raw.clone().wrap_read_only()
                } else {
                    raw.clone()
                };
                (len_r, raw, passed)
            }
            other => (len_r, other.clone(), other.clone()),
        }
    }

    fn emit_ho_invoke(
        &mut self,
        cg: &mut Codegen<'_>,
        target: &LoweredCallTarget,
        mut args: Vec<Val>,
        env_val: Option<&Val>,
        hint: Option<&Type>,
        line: usize,
    ) -> DecResult<Val> {
        if let Some(ev) = env_val {
            args.push(ev.clone());
        }
        match target {
            LoweredCallTarget::UserFn(overloads) => {
                let ov = find_matching_overload(overloads, &args).ok_or_else(|| {
                    Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        "codegen: no matching callback overload",
                    )
                })?;
                self.emit_direct_user_call(ov, &args)
            }
            LoweredCallTarget::Builtin(canon) => self
                .try_emit_builtin(cg, canon, &args, hint, line)?
                .ok_or_else(|| {
                    Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("codegen: no matching builtin callback signature for {canon}"),
                    )
                }),
        }
    }

    fn emit_domain_loop<F>(
        &mut self,
        cg: &mut Codegen<'_>,
        dom_val: &Val,
        prefix: &str,
        mut body_fn: F,
    ) -> DecResult<()>
    where
        F: FnMut(&mut Self, &mut Codegen<'_>, Val) -> DecResult<()>,
    {
        let is_window = *dom_val.dec_ty.unwrap_read_only() == Type::Window;
        let lo_slot = self.alloc_temp("i64");
        let hi_slot = self.alloc_temp("i64");
        if is_window {
            self.line(&format!(
                "  call void @aod_rt_window_bounds(ptr %ctx, i64 {}, ptr {lo_slot}, ptr {hi_slot})",
                dom_val.reg
            ));
        } else {
            self.line(&format!(
                "  call void @aod_rt_range_bounds(ptr %ctx, i64 {}, ptr {lo_slot}, ptr {hi_slot})",
                dom_val.reg
            ));
        }
        self.emit_fail_return_check();
        let lo_r = self.fresh_reg();
        let hi_r = self.fresh_reg();
        self.line(&format!("  {lo_r} = load i64, ptr {lo_slot}"));
        self.line(&format!("  {hi_r} = load i64, ptr {hi_slot}"));

        let cur_slot = self.alloc_temp("i64");
        self.line(&format!("  store i64 {lo_r}, ptr {cur_slot}"));

        let cond_lbl = self.fresh_label(&format!("{prefix}_cond"));
        let body_lbl = self.fresh_label(&format!("{prefix}_body"));
        let end_lbl = self.fresh_label(&format!("{prefix}_end"));
        self.line(&format!("  br label %{cond_lbl}"));

        self.start_block(&cond_lbl);
        let cur = self.fresh_reg();
        let cmp = self.fresh_reg();
        self.line(&format!("  {cur} = load i64, ptr {cur_slot}"));
        self.line(&format!("  {cmp} = icmp slt i64 {cur}, {hi_r}"));
        self.line(&format!(
            "  br i1 {cmp}, label %{body_lbl}, label %{end_lbl}"
        ));

        self.start_block(&body_lbl);
        self.emit_fuel_tick();
        let k_i64 = if is_window {
            // `cur < hi_r <= i64::MAX`, so `cur + 1` never overflows signed `i64`!
            let k = self.fresh_reg();
            self.line(&format!("  {k} = add i64 {cur}, 1"));
            k
        } else {
            cur.clone()
        };
        let k_dec = self.fresh_reg();
        self.line(&format!(
            "  {k_dec} = call i64 @__aod_int_from_i64_fast(ptr %ctx, i64 {k_i64})"
        ));
        self.emit_fail_return_check();
        body_fn(
            self,
            cg,
            Val {
                reg: k_dec,
                dec_ty: Type::Int,
            },
        )?;
        let nxt = if is_window {
            k_i64
        } else {
            let n = self.fresh_reg();
            self.line(&format!("  {n} = add i64 {cur}, 1"));
            n
        };
        self.line(&format!("  store i64 {nxt}, ptr {cur_slot}"));
        let md_suffix = cg.alloc_loop_hint_md().unwrap_or_default();
        self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
        self.terminated = true;

        self.start_block(&end_lbl);
        Ok(())
    }

    fn resolve_state_ty_for_ho(
        init_ty: &Type,
        idx_or_elem_ty: &Type,
        env_val: Option<&Val>,
        target: &LoweredCallTarget,
    ) -> Type {
        if let LoweredCallTarget::UserFn(ovs) = target {
            let mut probe = vec![
                Val {
                    reg: String::new(),
                    dec_ty: init_ty.clone(),
                },
                Val {
                    reg: String::new(),
                    dec_ty: idx_or_elem_ty.clone(),
                },
            ];
            if let Some(ev) = env_val {
                probe.push(Val {
                    reg: String::new(),
                    dec_ty: ev.dec_ty.clone(),
                });
            }
            if let Some(ov) = find_matching_overload(ovs, &probe) {
                if matches!(init_ty, Type::Array(_, _)) && matches!(ov.ret_type, Type::List(_)) {
                    return ov.ret_type.clone();
                }
            }
        }
        init_ty.clone()
    }

    fn emit_afc_evolve(
        &mut self,
        cg: &mut Codegen<'_>,
        prepended: Option<Val>,
        args: &[Expr],
        line: usize,
    ) -> DecResult<Val> {
        let total_arity = usize::from(prepended.is_some()) + args.len();
        let cb_arity = if total_arity == 4 { 3 } else { 2 };
        let (init_opt_v, init_expr_opt, dom_expr, fn_expr, env_expr_opt) =
            if let Some(in_v) = prepended {
                (Some(in_v), None, &args[0], &args[1], args.get(2))
            } else {
                (None, Some(&args[0]), &args[1], &args[2], args.get(3))
            };
        let fn_target = self.resolve_lowered_target(cg, fn_expr, line)?;
        let s_hint = Self::unique_ho_param_hint_cg(&fn_target, cb_arity, 0);
        let env_hint = if cb_arity == 3 {
            Self::unique_ho_param_hint_cg(&fn_target, 3, 2)
        } else {
            None
        };

        let (mut state_ty, init_unboxed_opt, init_boxed_opt) = match init_opt_v {
            Some(iv) => (iv.dec_ty.clone(), None, Some(iv)),
            None => {
                let init_e = init_expr_opt.unwrap();
                if let ExprKind::Tuple(ref tuple_elems) = init_e.kind {
                    if !tuple_elems.is_empty() {
                        let elem_hints: Option<&[Type]> = match s_hint.as_ref() {
                            Some(Type::Tuple(ts)) if ts.len() == tuple_elems.len() => {
                                Some(ts.as_slice())
                            }
                            _ => None,
                        };
                        let mut ivs = Vec::with_capacity(tuple_elems.len());
                        for (j, te) in tuple_elems.iter().enumerate() {
                            ivs.push(self.emit_expr_with_hint(
                                cg,
                                te,
                                elem_hints.and_then(|hs| hs.get(j)),
                            )?);
                        }
                        let sty = Type::Tuple(ivs.iter().map(|v| v.dec_ty.clone()).collect());
                        (sty, Some(ivs), None)
                    } else {
                        let iv = self.emit_expr_with_hint(cg, init_e, s_hint.as_ref())?;
                        (iv.dec_ty.clone(), None, Some(iv))
                    }
                } else {
                    let iv = self.emit_expr_with_hint(cg, init_e, s_hint.as_ref())?;
                    (iv.dec_ty.clone(), None, Some(iv))
                }
            }
        };

        let dom_val = self.emit_expr(cg, dom_expr)?;
        let env_val = match env_expr_opt {
            Some(ee) => Some(self.emit_expr_with_hint(cg, ee, env_hint.as_ref())?),
            None => None,
        };
        state_ty = Self::resolve_state_ty_for_ho(&state_ty, &Type::Int, env_val.as_ref(), &fn_target);

        // Unboxed scalar-tuple `evolve` fast-path (zero arena allocations per tick!)
        if let (LoweredCallTarget::UserFn(ovs), Type::Tuple(elem_tys)) = (&fn_target, &state_ty) {
            let mut probe = vec![
                Val {
                    reg: String::new(),
                    dec_ty: state_ty.clone(),
                },
                Val {
                    reg: String::new(),
                    dec_ty: Type::Int,
                },
            ];
            if let Some(ref ev) = env_val {
                probe.push(Val {
                    reg: String::new(),
                    dec_ty: ev.dec_ty.clone(),
                });
            }
            if let Some(ov) = find_matching_overload(ovs, &probe) {
                if let Some(ref unboxed_fn) = ov.unboxed_evolve_llvm_name {
                    let init_elems: Vec<Val> = if let Some(ivs) = init_unboxed_opt {
                        ivs
                    } else {
                        let boxed_iv = init_boxed_opt.unwrap();
                        let mut extracted = Vec::with_capacity(elem_tys.len());
                        for (j, ety) in elem_tys.iter().enumerate() {
                            let eh = self.fresh_reg();
                            self.line(&format!(
                                "  {eh} = call i64 @aod_rt_tuple_get(ptr %ctx, i64 {}, i64 {j})",
                                boxed_iv.reg
                            ));
                            self.emit_fail_return_check();
                            extracted.push(self.unbox_handle_to_val(&eh, ety));
                        }
                        extracted
                    };
                    let mut s_ptrs = Vec::with_capacity(elem_tys.len());
                    for (iv, ety) in init_elems.iter().zip(elem_tys) {
                        let elt = LlvmTy::from_dec(ety);
                        let ptr = self.alloc_temp(elt.ir());
                        self.line(&format!("  store {} {}, ptr {ptr}", elt.ir(), iv.reg));
                        s_ptrs.push(ptr);
                    }
                    let unboxed_fn_name = unboxed_fn.clone();
                    self.emit_domain_loop(cg, &dom_val, "ev", |this, _cg_inner, k_val| {
                        let mut call_args = String::from("ptr %ctx");
                        for sp in &s_ptrs {
                            let _ = write!(call_args, ", ptr {sp}");
                        }
                        let _ = write!(call_args, ", i64 {}", k_val.reg);
                        if let Some(ref ev) = env_val {
                            let env_lt = LlvmTy::from_dec(&ev.dec_ty);
                            if env_lt != LlvmTy::Void {
                                let _ = write!(call_args, ", {} {}", env_lt.ir(), ev.reg);
                            }
                        }
                        this.line(&format!("  call void {unboxed_fn_name}({call_args})"));
                        this.emit_fail_return_check();
                        Ok(())
                    })?;
                    let mut final_handles = Vec::with_capacity(elem_tys.len());
                    for (sp, ety) in s_ptrs.iter().zip(elem_tys) {
                        let elt = LlvmTy::from_dec(ety);
                        let r = self.fresh_reg();
                        self.line(&format!("  {r} = load {}, ptr {sp}", elt.ir()));
                        final_handles.push(self.box_val_to_handle(&Val {
                            reg: r,
                            dec_ty: ety.clone(),
                        }));
                    }
                    let out_h = self.emit_handle_array_call("aod_rt_tuple_new", &final_handles);
                    return Ok(Val {
                        reg: out_h,
                        dec_ty: state_ty,
                    });
                }
            }
        }

        let init_val = if let Some(iv) = init_boxed_opt {
            iv
        } else {
            let ivs = init_unboxed_opt.unwrap();
            let handles: Vec<String> = ivs.iter().map(|v| self.box_val_to_handle(v)).collect();
            let h = self.emit_handle_array_call("aod_rt_tuple_new", &handles);
            Val {
                reg: h,
                dec_ty: state_ty.clone(),
            }
        };
        let state_lt = LlvmTy::from_dec(&state_ty);
        let state_slot = self.alloc_temp(state_lt.ir());
        self.line(&format!(
            "  store {} {}, ptr {state_slot}",
            state_lt.ir(),
            init_val.reg
        ));

        self.emit_domain_loop(cg, &dom_val, "ev", |this, cg_inner, k_val| {
            let cur_s = this.fresh_reg();
            this.line(&format!(
                "  {cur_s} = load {}, ptr {state_slot}",
                state_lt.ir()
            ));
            let next_s = this.emit_ho_invoke(
                cg_inner,
                &fn_target,
                vec![
                    Val {
                        reg: cur_s,
                        dec_ty: state_ty.clone(),
                    },
                    k_val,
                ],
                env_val.as_ref(),
                Some(&state_ty),
                line,
            )?;
            this.line(&format!(
                "  store {} {}, ptr {state_slot}",
                state_lt.ir(),
                next_s.reg
            ));
            Ok(())
        })?;

        let final_s = self.fresh_reg();
        self.line(&format!(
            "  {final_s} = load {}, ptr {state_slot}",
            state_lt.ir()
        ));
        Ok(Val {
            reg: final_s,
            dec_ty: state_ty,
        })
    }

    fn emit_afc_scan_or_fold(
        &mut self,
        cg: &mut Codegen<'_>,
        is_scan: bool,
        prepended: Option<Val>,
        args: &[Expr],
        line: usize,
    ) -> DecResult<Val> {
        let total_arity = usize::from(prepended.is_some()) + args.len();
        let cb_arity = if total_arity == 4 { 3 } else { 2 };
        let (init_opt_v, init_expr_opt, src_expr, fn_expr, env_expr_opt) =
            if let Some(in_v) = prepended {
                (Some(in_v), None, &args[0], &args[1], args.get(2))
            } else {
                (None, Some(&args[0]), &args[1], &args[2], args.get(3))
            };
        let fn_target = self.resolve_lowered_target(cg, fn_expr, line)?;
        let s_hint = Self::unique_ho_param_hint_cg(&fn_target, cb_arity, 0);
        let env_hint = if cb_arity == 3 {
            Self::unique_ho_param_hint_cg(&fn_target, 3, 2)
        } else {
            None
        };
        let init_val = match init_opt_v {
            Some(v) => v,
            None => self.emit_expr_with_hint(cg, init_expr_opt.unwrap(), s_hint.as_ref())?,
        };
        let src_val = self.emit_expr(cg, src_expr)?;
        let env_val = match env_expr_opt {
            Some(ee) => Some(self.emit_expr_with_hint(cg, ee, env_hint.as_ref())?),
            None => None,
        };

        let is_dom = matches!(
            src_val.dec_ty.unwrap_read_only(),
            Type::Range | Type::Window
        );
        if is_dom {
            let state_ty = Self::resolve_state_ty_for_ho(
                &init_val.dec_ty,
                &Type::Int,
                env_val.as_ref(),
                &fn_target,
            );
            let state_lt = LlvmTy::from_dec(&state_ty);
            let state_slot = self.alloc_temp(state_lt.ir());
            self.line(&format!(
                "  store {} {}, ptr {state_slot}",
                state_lt.ir(),
                init_val.reg
            ));
            let buf_id = self.fresh_reg();
            self.line(&format!(
                "  {buf_id} = call i64 @aod_rt_seq_buf_new(ptr %ctx, i64 16)"
            ));
            self.emit_fail_return_check();

            self.emit_domain_loop(cg, &src_val, "scn", |this, cg_inner, k_val| {
                let cur_s = this.fresh_reg();
                this.line(&format!(
                    "  {cur_s} = load {}, ptr {state_slot}",
                    state_lt.ir()
                ));
                let next_s = this.emit_ho_invoke(
                    cg_inner,
                    &fn_target,
                    vec![
                        Val {
                            reg: cur_s,
                            dec_ty: state_ty.clone(),
                        },
                        k_val,
                    ],
                    env_val.as_ref(),
                    Some(&state_ty),
                    line,
                )?;
                this.line(&format!(
                    "  store {} {}, ptr {state_slot}",
                    state_lt.ir(),
                    next_s.reg
                ));
                let nh = this.box_val_to_handle(&next_s);
                this.line(&format!(
                    "  call void @aod_rt_seq_buf_push(ptr %ctx, i64 {buf_id}, i64 {nh})"
                ));
                this.emit_fail_return_check();
                Ok(())
            })?;

            let out_h = self.fresh_reg();
            self.line(&format!(
                "  {out_h} = call i64 @aod_rt_seq_buf_finish(ptr %ctx, i64 {buf_id}, i8 0)"
            ));
            self.emit_fail_return_check();
            return Ok(Val {
                reg: out_h,
                dec_ty: Type::List(Box::new(state_ty)),
            });
        }

        let elem_hint = Self::unique_ho_param_hint_cg(&fn_target, cb_arity, 1);
        let (len_r, _raw_elem, passed_elem) = self.extract_afc_seq_elem_cg(&src_val, elem_hint);
        let state_ty = Self::resolve_state_ty_for_ho(
            &init_val.dec_ty,
            &passed_elem,
            env_val.as_ref(),
            &fn_target,
        );
        let state_lt = LlvmTy::from_dec(&state_ty);
        let state_slot = self.alloc_temp(state_lt.ir());
        self.line(&format!(
            "  store {} {}, ptr {state_slot}",
            state_lt.ir(),
            init_val.reg
        ));
        let buf_opt = if is_scan {
            let b = self.fresh_reg();
            self.line(&format!(
                "  {b} = call i64 @aod_rt_seq_buf_new(ptr %ctx, i64 {len_r})"
            ));
            self.emit_fail_return_check();
            Some(b)
        } else {
            None
        };

        let idx_slot = self.alloc_temp("i64");
        self.line(&format!("  store i64 0, ptr {idx_slot}"));
        let cond_lbl = self.fresh_label("sq_cond");
        let body_lbl = self.fresh_label("sq_body");
        let end_lbl = self.fresh_label("sq_end");
        self.line(&format!("  br label %{cond_lbl}"));

        self.start_block(&cond_lbl);
        let idx = self.fresh_reg();
        let cmp = self.fresh_reg();
        self.line(&format!("  {idx} = load i64, ptr {idx_slot}"));
        self.line(&format!("  {cmp} = icmp slt i64 {idx}, {len_r}"));
        self.line(&format!(
            "  br i1 {cmp}, label %{body_lbl}, label %{end_lbl}"
        ));

        self.start_block(&body_lbl);
        self.emit_fuel_tick();
        let eh = self.fresh_reg();
        self.line(&format!(
            "  {eh} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {idx})",
            src_val.reg
        ));
        let ev = self.unbox_handle_to_val(&eh, &passed_elem);
        let cur_s = self.fresh_reg();
        self.line(&format!(
            "  {cur_s} = load {}, ptr {state_slot}",
            state_lt.ir()
        ));
        let next_s = self.emit_ho_invoke(
            cg,
            &fn_target,
            vec![
                Val {
                    reg: cur_s,
                    dec_ty: state_ty.clone(),
                },
                ev,
            ],
            env_val.as_ref(),
            Some(&state_ty),
            line,
        )?;
        self.line(&format!(
            "  store {} {}, ptr {state_slot}",
            state_lt.ir(),
            next_s.reg
        ));
        if let Some(ref b) = buf_opt {
            let nh = self.box_val_to_handle(&next_s);
            self.line(&format!(
                "  call void @aod_rt_seq_buf_push(ptr %ctx, i64 {b}, i64 {nh})"
            ));
            self.emit_fail_return_check();
        }
        let nxt = self.fresh_reg();
        self.line(&format!("  {nxt} = add nsw i64 {idx}, 1"));
        self.line(&format!("  store i64 {nxt}, ptr {idx_slot}"));
        let md_suffix = cg.alloc_loop_hint_md().unwrap_or_default();
        self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
        self.terminated = true;

        self.start_block(&end_lbl);
        if let Some(b) = buf_opt {
            let out_h = self.fresh_reg();
            self.line(&format!(
                "  {out_h} = call i64 @aod_rt_seq_buf_finish(ptr %ctx, i64 {b}, i8 0)"
            ));
            self.emit_fail_return_check();
            Ok(Val {
                reg: out_h,
                dec_ty: Type::List(Box::new(state_ty)),
            })
        } else {
            let final_s = self.fresh_reg();
            self.line(&format!(
                "  {final_s} = load {}, ptr {state_slot}",
                state_lt.ir()
            ));
            Ok(Val {
                reg: final_s,
                dec_ty: state_ty,
            })
        }
    }

    fn emit_afc_map_seq(
        &mut self,
        cg: &mut Codegen<'_>,
        prepended: Option<Val>,
        args: &[Expr],
        line: usize,
    ) -> DecResult<Val> {
        let total_arity = usize::from(prepended.is_some()) + args.len();
        let cb_arity = if total_arity == 3 { 2 } else { 1 };
        let (seq_val, fn_expr, env_expr_opt) = if let Some(in_v) = prepended {
            (in_v, &args[0], args.get(1))
        } else {
            (self.emit_expr(cg, &args[0])?, &args[1], args.get(2))
        };
        let fn_target = self.resolve_lowered_target(cg, fn_expr, line)?;
        let elem_hint = Self::unique_ho_param_hint_cg(&fn_target, cb_arity, 0);
        let env_hint = if cb_arity == 2 {
            Self::unique_ho_param_hint_cg(&fn_target, 2, 1)
        } else {
            None
        };
        let env_val = match env_expr_opt {
            Some(ee) => Some(self.emit_expr_with_hint(cg, ee, env_hint.as_ref())?),
            None => None,
        };
        let (len_r, _raw_elem, passed_elem) = self.extract_afc_seq_elem_cg(&seq_val, elem_hint);
        let buf_id = self.fresh_reg();
        self.line(&format!(
            "  {buf_id} = call i64 @aod_rt_seq_buf_new(ptr %ctx, i64 {len_r})"
        ));
        self.emit_fail_return_check();

        let idx_slot = self.alloc_temp("i64");
        self.line(&format!("  store i64 0, ptr {idx_slot}"));
        let cond_lbl = self.fresh_label("ms_cond");
        let body_lbl = self.fresh_label("ms_body");
        let end_lbl = self.fresh_label("ms_end");
        self.line(&format!("  br label %{cond_lbl}"));

        self.start_block(&cond_lbl);
        let idx = self.fresh_reg();
        let cmp = self.fresh_reg();
        self.line(&format!("  {idx} = load i64, ptr {idx_slot}"));
        self.line(&format!("  {cmp} = icmp slt i64 {idx}, {len_r}"));
        self.line(&format!(
            "  br i1 {cmp}, label %{body_lbl}, label %{end_lbl}"
        ));

        self.start_block(&body_lbl);
        self.emit_fuel_tick();
        let eh = self.fresh_reg();
        self.line(&format!(
            "  {eh} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {idx})",
            seq_val.reg
        ));
        let ev = self.unbox_handle_to_val(&eh, &passed_elem);
        let mapped = self.emit_ho_invoke(cg, &fn_target, vec![ev], env_val.as_ref(), None, line)?;
        let y_ty = mapped.dec_ty.clone();
        let mh = self.box_val_to_handle(&mapped);
        self.line(&format!(
            "  call void @aod_rt_seq_buf_push(ptr %ctx, i64 {buf_id}, i64 {mh})"
        ));
        self.emit_fail_return_check();
        let nxt = self.fresh_reg();
        self.line(&format!("  {nxt} = add nsw i64 {idx}, 1"));
        self.line(&format!("  store i64 {nxt}, ptr {idx_slot}"));
        let md_suffix = cg.alloc_loop_hint_md().unwrap_or_default();
        self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
        self.terminated = true;

        self.start_block(&end_lbl);
        let out_h = self.fresh_reg();
        self.line(&format!(
            "  {out_h} = call i64 @aod_rt_seq_buf_finish(ptr %ctx, i64 {buf_id}, i8 0)"
        ));
        self.emit_fail_return_check();
        Ok(Val {
            reg: out_h,
            dec_ty: Type::List(Box::new(y_ty)),
        })
    }

    fn emit_afc_filter_seq(
        &mut self,
        cg: &mut Codegen<'_>,
        prepended: Option<Val>,
        args: &[Expr],
        line: usize,
    ) -> DecResult<Val> {
        let total_arity = usize::from(prepended.is_some()) + args.len();
        let cb_arity = if total_arity == 3 { 2 } else { 1 };
        let (seq_val, fn_expr, env_expr_opt) = if let Some(in_v) = prepended {
            (in_v, &args[0], args.get(1))
        } else {
            (self.emit_expr(cg, &args[0])?, &args[1], args.get(2))
        };
        let seq_ro = seq_val.dec_ty.is_read_only();
        let fn_target = self.resolve_lowered_target(cg, fn_expr, line)?;
        let elem_hint = Self::unique_ho_param_hint_cg(&fn_target, cb_arity, 0);
        let env_hint = if cb_arity == 2 {
            Self::unique_ho_param_hint_cg(&fn_target, 2, 1)
        } else {
            None
        };
        let env_val = match env_expr_opt {
            Some(ee) => Some(self.emit_expr_with_hint(cg, ee, env_hint.as_ref())?),
            None => None,
        };
        let (len_r, raw_elem, passed_elem) = self.extract_afc_seq_elem_cg(&seq_val, elem_hint);
        let buf_id = self.fresh_reg();
        self.line(&format!(
            "  {buf_id} = call i64 @aod_rt_seq_buf_new(ptr %ctx, i64 0)"
        ));
        self.emit_fail_return_check();

        let idx_slot = self.alloc_temp("i64");
        self.line(&format!("  store i64 0, ptr {idx_slot}"));
        let cond_lbl = self.fresh_label("fs_cond");
        let body_lbl = self.fresh_label("fs_body");
        let push_lbl = self.fresh_label("fs_push");
        let step_lbl = self.fresh_label("fs_step");
        let end_lbl = self.fresh_label("fs_end");
        self.line(&format!("  br label %{cond_lbl}"));

        self.start_block(&cond_lbl);
        let idx = self.fresh_reg();
        let cmp = self.fresh_reg();
        self.line(&format!("  {idx} = load i64, ptr {idx_slot}"));
        self.line(&format!("  {cmp} = icmp slt i64 {idx}, {len_r}"));
        self.line(&format!(
            "  br i1 {cmp}, label %{body_lbl}, label %{end_lbl}"
        ));

        self.start_block(&body_lbl);
        self.emit_fuel_tick();
        let eh_raw = self.fresh_reg();
        self.line(&format!(
            "  {eh_raw} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {idx})",
            seq_val.reg
        ));
        let ev = self.unbox_handle_to_val(&eh_raw, &passed_elem);
        let keep_val = self.emit_ho_invoke(
            cg,
            &fn_target,
            vec![ev.clone()],
            env_val.as_ref(),
            Some(&Type::Bool),
            line,
        )?;
        self.line(&format!(
            "  br i1 {}, label %{push_lbl}, label %{step_lbl}",
            keep_val.reg
        ));

        self.start_block(&push_lbl);
        let eh_box = self.box_val_to_handle(&ev);
        self.line(&format!(
            "  call void @aod_rt_seq_buf_push(ptr %ctx, i64 {buf_id}, i64 {eh_box})"
        ));
        self.emit_fail_return_check();
        self.line(&format!("  br label %{step_lbl}"));

        self.start_block(&step_lbl);
        let nxt = self.fresh_reg();
        self.line(&format!("  {nxt} = add nsw i64 {idx}, 1"));
        self.line(&format!("  store i64 {nxt}, ptr {idx_slot}"));
        let md_suffix = cg.alloc_loop_hint_md().unwrap_or_default();
        self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
        self.terminated = true;

        self.start_block(&end_lbl);
        let out_h = self.fresh_reg();
        self.line(&format!(
            "  {out_h} = call i64 @aod_rt_seq_buf_finish(ptr %ctx, i64 {buf_id}, i8 0)"
        ));
        self.emit_fail_return_check();
        let out_ty = if seq_ro {
            Type::List(Box::new(raw_elem)).wrap_read_only()
        } else {
            Type::List(Box::new(raw_elem))
        };
        Ok(Val {
            reg: out_h,
            dec_ty: out_ty,
        })
    }

    fn emit_afc_tabulate(
        &mut self,
        cg: &mut Codegen<'_>,
        prepended: Option<Val>,
        args: &[Expr],
        line: usize,
    ) -> DecResult<Val> {
        let total_arity = usize::from(prepended.is_some()) + args.len();
        let cb_arity = if total_arity == 3 { 2 } else { 1 };
        let (dom_val, fn_expr, env_expr_opt) = if let Some(in_v) = prepended {
            (in_v, &args[0], args.get(1))
        } else {
            (self.emit_expr(cg, &args[0])?, &args[1], args.get(2))
        };
        let fn_target = self.resolve_lowered_target(cg, fn_expr, line)?;
        let env_hint = if cb_arity == 2 {
            Self::unique_ho_param_hint_cg(&fn_target, 2, 1)
        } else {
            None
        };
        let env_val = match env_expr_opt {
            Some(ee) => Some(self.emit_expr_with_hint(cg, ee, env_hint.as_ref())?),
            None => None,
        };
        let buf_id = self.fresh_reg();
        self.line(&format!(
            "  {buf_id} = call i64 @aod_rt_seq_buf_new(ptr %ctx, i64 16)"
        ));
        self.emit_fail_return_check();

        let mut u_ty = Type::Unknown;
        self.emit_domain_loop(cg, &dom_val, "tab", |this, cg_inner, k_val| {
            let y_val =
                this.emit_ho_invoke(cg_inner, &fn_target, vec![k_val], env_val.as_ref(), None, line)?;
            u_ty = y_val.dec_ty.clone();
            let yh = this.box_val_to_handle(&y_val);
            this.line(&format!(
                "  call void @aod_rt_seq_buf_push(ptr %ctx, i64 {buf_id}, i64 {yh})"
            ));
            this.emit_fail_return_check();
            Ok(())
        })?;

        let out_h = self.fresh_reg();
        self.line(&format!(
            "  {out_h} = call i64 @aod_rt_seq_buf_finish(ptr %ctx, i64 {buf_id}, i8 0)"
        ));
        self.emit_fail_return_check();
        Ok(Val {
            reg: out_h,
            dec_ty: Type::List(Box::new(u_ty)),
        })
    }

    fn emit_afc_zip_seq(
        &mut self,
        cg: &mut Codegen<'_>,
        prepended: Option<Val>,
        args: &[Expr],
        hint: Option<&Type>,
        _line: usize,
    ) -> DecResult<Val> {
        let (hint_a, hint_b): (Option<Type>, Option<Type>) = match hint.map(Type::unwrap_read_only)
        {
            Some(Type::List(inner)) => match inner.unwrap_read_only() {
                Type::Tuple(ts) if ts.len() == 2 => (Some(ts[0].clone()), Some(ts[1].clone())),
                _ => (None, None),
            },
            _ => (None, None),
        };
        let (av, bv) = if let Some(in_v) = prepended {
            (in_v, self.emit_expr(cg, &args[0])?)
        } else {
            (
                self.emit_expr(cg, &args[0])?,
                self.emit_expr(cg, &args[1])?,
            )
        };
        let elem_a = match av.dec_ty.unwrap_read_only() {
            Type::Bytes => Type::B8,
            Type::Array(e, _) | Type::List(e) => {
                let raw = if **e == Type::Unknown {
                    hint_a.unwrap_or(Type::Unknown)
                } else {
                    (**e).clone()
                };
                if av.dec_ty.is_read_only() && raw.needs_readonly_propagation() {
                    raw.wrap_read_only()
                } else {
                    raw
                }
            }
            other => other.clone(),
        };
        let elem_b = match bv.dec_ty.unwrap_read_only() {
            Type::Bytes => Type::B8,
            Type::Array(e, _) | Type::List(e) => {
                let raw = if **e == Type::Unknown {
                    hint_b.unwrap_or(Type::Unknown)
                } else {
                    (**e).clone()
                };
                if bv.dec_ty.is_read_only() && raw.needs_readonly_propagation() {
                    raw.wrap_read_only()
                } else {
                    raw
                }
            }
            other => other.clone(),
        };
        let r = self.fresh_reg();
        self.line(&format!(
            "  {r} = call i64 @aod_rt_zip_seq(ptr %ctx, i64 {}, i64 {})",
            av.reg, bv.reg
        ));
        self.emit_fail_return_check();
        Ok(Val {
            reg: r,
            dec_ty: Type::List(Box::new(Type::Tuple(vec![elem_a, elem_b]))),
        })
    }

    fn emit_afc_count(
        &mut self,
        cg: &mut Codegen<'_>,
        prepended: Option<Val>,
        args: &[Expr],
        line: usize,
    ) -> DecResult<Val> {
        let total_arity = usize::from(prepended.is_some()) + args.len();
        let cb_arity = if total_arity == 3 { 2 } else { 1 };
        let (seq_val, fn_expr, env_expr_opt) = if let Some(in_v) = prepended {
            (in_v, &args[0], args.get(1))
        } else {
            (self.emit_expr(cg, &args[0])?, &args[1], args.get(2))
        };
        let fn_target = self.resolve_lowered_target(cg, fn_expr, line)?;
        let elem_hint = Self::unique_ho_param_hint_cg(&fn_target, cb_arity, 0);
        let env_hint = if cb_arity == 2 {
            Self::unique_ho_param_hint_cg(&fn_target, 2, 1)
        } else {
            None
        };
        let env_val = match env_expr_opt {
            Some(ee) => Some(self.emit_expr_with_hint(cg, ee, env_hint.as_ref())?),
            None => None,
        };
        let (len_r, _raw_elem, passed_elem) = self.extract_afc_seq_elem_cg(&seq_val, elem_hint);
        let cnt_slot = self.alloc_temp("i64");
        let idx_slot = self.alloc_temp("i64");
        self.line(&format!("  store i64 0, ptr {cnt_slot}"));
        self.line(&format!("  store i64 0, ptr {idx_slot}"));

        let cond_lbl = self.fresh_label("cnt_cond");
        let body_lbl = self.fresh_label("cnt_body");
        let end_lbl = self.fresh_label("cnt_end");
        self.line(&format!("  br label %{cond_lbl}"));

        self.start_block(&cond_lbl);
        let idx = self.fresh_reg();
        let cmp = self.fresh_reg();
        self.line(&format!("  {idx} = load i64, ptr {idx_slot}"));
        self.line(&format!("  {cmp} = icmp slt i64 {idx}, {len_r}"));
        self.line(&format!(
            "  br i1 {cmp}, label %{body_lbl}, label %{end_lbl}"
        ));

        self.start_block(&body_lbl);
        self.emit_fuel_tick();
        let eh = self.fresh_reg();
        self.line(&format!(
            "  {eh} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {idx})",
            seq_val.reg
        ));
        let ev = self.unbox_handle_to_val(&eh, &passed_elem);
        let pv = self.emit_ho_invoke(
            cg,
            &fn_target,
            vec![ev],
            env_val.as_ref(),
            Some(&Type::Bool),
            line,
        )?;
        let c0 = self.fresh_reg();
        let inc = self.fresh_reg();
        let c1 = self.fresh_reg();
        self.line(&format!("  {c0} = load i64, ptr {cnt_slot}"));
        self.line(&format!("  {inc} = zext i1 {} to i64", pv.reg));
        self.line(&format!("  {c1} = add i64 {c0}, {inc}"));
        self.line(&format!("  store i64 {c1}, ptr {cnt_slot}"));

        let nxt = self.fresh_reg();
        self.line(&format!("  {nxt} = add nsw i64 {idx}, 1"));
        self.line(&format!("  store i64 {nxt}, ptr {idx_slot}"));
        let md_suffix = cg.alloc_loop_hint_md().unwrap_or_default();
        self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
        self.terminated = true;

        self.start_block(&end_lbl);
        let total = self.fresh_reg();
        let out_dec = self.fresh_reg();
        self.line(&format!("  {total} = load i64, ptr {cnt_slot}"));
        self.line(&format!(
            "  {out_dec} = call i64 @__aod_int_from_i64_fast(ptr %ctx, i64 {total})"
        ));
        self.emit_fail_return_check();
        Ok(Val {
            reg: out_dec,
            dec_ty: Type::Int,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_afc_any_or_all(
        &mut self,
        cg: &mut Codegen<'_>,
        is_any: bool,
        prepended: Option<Val>,
        callee: &Expr,
        args: &[Expr],
        hint: Option<&Type>,
        line: usize,
    ) -> DecResult<Val> {
        let op = if is_any { "any" } else { "all" };
        let total_arity = usize::from(prepended.is_some()) + args.len();
        let cb_arity = if total_arity == 3 { 2 } else { 1 };
        let (seq_val, fn_expr, env_expr_opt) = if let Some(in_v) = prepended {
            (in_v, &args[0], args.get(1))
        } else {
            (self.emit_expr(cg, &args[0])?, &args[1], args.get(2))
        };
        let fn_target = self.resolve_lowered_target(cg, fn_expr, line)?;
        let elem_hint = Self::unique_ho_param_hint_cg(&fn_target, cb_arity, 0);
        let env_hint = if cb_arity == 2 {
            Self::unique_ho_param_hint_cg(&fn_target, 2, 1)
        } else {
            None
        };
        let env_val = match env_expr_opt {
            Some(ee) => Some(self.emit_expr_with_hint(cg, ee, env_hint.as_ref())?),
            None => None,
        };
        let (len_r, _raw_elem, passed_elem) = self.extract_afc_seq_elem_cg(&seq_val, elem_hint);

        let res_slot = self.alloc_temp("i1");
        let idx_slot = self.alloc_temp("i64");
        self.line(&format!(
            "  store i1 {}, ptr {res_slot}",
            if is_any { "false" } else { "true" }
        ));
        self.line(&format!("  store i64 0, ptr {idx_slot}"));

        let cond_lbl = self.fresh_label("aa_cond");
        let body_lbl = self.fresh_label("aa_body");
        let early_lbl = self.fresh_label("aa_early");
        let step_lbl = self.fresh_label("aa_step");
        let end_lbl = self.fresh_label("aa_end");
        self.line(&format!("  br label %{cond_lbl}"));

        self.start_block(&cond_lbl);
        let idx = self.fresh_reg();
        let cmp = self.fresh_reg();
        self.line(&format!("  {idx} = load i64, ptr {idx_slot}"));
        self.line(&format!("  {cmp} = icmp slt i64 {idx}, {len_r}"));
        self.line(&format!(
            "  br i1 {cmp}, label %{body_lbl}, label %{end_lbl}"
        ));

        self.start_block(&body_lbl);
        self.emit_fuel_tick();
        let eh = self.fresh_reg();
        self.line(&format!(
            "  {eh} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {idx})",
            seq_val.reg
        ));
        let ev = self.unbox_handle_to_val(&eh, &passed_elem);
        let pv = self.emit_ho_invoke(cg, &fn_target, vec![ev], env_val.as_ref(), hint, line)?;
        let pred_is_qext = matches!(pv.dec_ty.unwrap_read_only(), Type::QExt(inner) if **inner == Type::Bool);
        if is_any {
            self.line(&format!(
                "  br i1 {}, label %{early_lbl}, label %{step_lbl}",
                pv.reg
            ));
        } else {
            self.line(&format!(
                "  br i1 {}, label %{step_lbl}, label %{early_lbl}",
                pv.reg
            ));
        }

        self.start_block(&early_lbl);
        self.line(&format!(
            "  store i1 {}, ptr {res_slot}",
            if is_any { "true" } else { "false" }
        ));
        self.line(&format!("  br label %{end_lbl}"));

        self.start_block(&step_lbl);
        let nxt = self.fresh_reg();
        self.line(&format!("  {nxt} = add nsw i64 {idx}, 1"));
        self.line(&format!("  store i64 {nxt}, ptr {idx_slot}"));
        let md_suffix = cg.alloc_loop_hint_md().unwrap_or_default();
        self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
        self.terminated = true;

        self.start_block(&end_lbl);
        let final_b = self.fresh_reg();
        self.line(&format!("  {final_b} = load i1, ptr {res_slot}"));
        let out_ty = cg
            .checked
            .poly_call_types
            .get(&(
                self.mod_key.clone(),
                line,
                format!("{op}:{callee:?}:{args:?}"),
            ))
            .cloned()
            .unwrap_or_else(|| {
                if pred_is_qext {
                    Type::QExt(Box::new(Type::Bool))
                } else {
                    Type::Bool
                }
            });
        Ok(Val {
            reg: final_b,
            dec_ty: out_ty,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_sum_or_dot(
        &mut self,
        cg: &mut Codegen<'_>,
        op: &str,
        prepended: Option<Val>,
        callee: &Expr,
        args: &[Expr],
        hint: Option<&Type>,
        line: usize,
    ) -> DecResult<Val> {
        let mut pos = Vec::with_capacity(usize::from(prepended.is_some()) + args.len());
        if let Some(in_v) = prepended {
            pos.push(self.materialize_if_range(in_v));
        }
        for a in args {
            let v = self.emit_expr(cg, a)?;
            pos.push(self.materialize_if_range(v));
        }
        let mut elem_ty = pos
            .iter()
            .find_map(|v| match v.dec_ty.unwrap_read_only() {
                Type::Bytes => Some(Type::B8),
                Type::Array(e, _) | Type::List(e) if **e != Type::Unknown => {
                    Some(e.unwrap_read_only().clone())
                }
                _ => None,
            })
            .or_else(|| {
                cg.checked
                    .poly_call_types
                    .get(&(
                        self.mod_key.clone(),
                        line,
                        format!("{op}:{callee:?}:{args:?}"),
                    ))
                    .cloned()
            })
            .or_else(|| hint.map(|h| h.unwrap_read_only().clone()))
            .unwrap_or(Type::Int);
        elem_ty = elem_ty.unwrap_read_only().clone();

        match (op, pos.as_slice(), &elem_ty) {
            ("sum", [seq], Type::Int) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_sum_int(ptr %ctx, i64 {})",
                    seq.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Int,
                })
            }
            ("sum", [seq], Type::B8) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i8 @aod_rt_sum_b8(ptr %ctx, i64 {})",
                    seq.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::B8,
                })
            }
            ("sum", [seq], Type::T1) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i8 @aod_rt_sum_t1(ptr %ctx, i64 {})",
                    seq.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::T1,
                })
            }
            ("sum", [seq], Type::Ratio) => {
                let out = self.alloc_temp("%DecRatio");
                self.line(&format!(
                    "  call void @aod_rt_sum_ratio(ptr %ctx, i64 {}, ptr {out})",
                    seq.reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecRatio, ptr {out}"));
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Ratio,
                })
            }
            ("sum", [seq], Type::QExt(inner)) if **inner == Type::Ratio => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_sum_qext_ratio(ptr %ctx, i64 {})",
                    seq.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::QExt(Box::new(Type::Ratio)),
                })
            }
            ("dot", [a, b], Type::Int) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_dot_int(ptr %ctx, i64 {}, i64 {})",
                    a.reg, b.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Int,
                })
            }
            ("dot", [a, b], Type::B8) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i8 @aod_rt_dot_b8(ptr %ctx, i64 {}, i64 {})",
                    a.reg, b.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::B8,
                })
            }
            ("dot", [a, b], Type::T1) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i8 @aod_rt_dot_t1(ptr %ctx, i64 {}, i64 {})",
                    a.reg, b.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::T1,
                })
            }
            ("dot", [a, b], Type::Ratio) => {
                let out = self.alloc_temp("%DecRatio");
                self.line(&format!(
                    "  call void @aod_rt_dot_ratio(ptr %ctx, i64 {}, i64 {}, ptr {out})",
                    a.reg, b.reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecRatio, ptr {out}"));
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Ratio,
                })
            }
            ("dot", [a, b], Type::QExt(inner)) if **inner == Type::Ratio => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_dot_qext_ratio(ptr %ctx, i64 {}, i64 {})",
                    a.reg, b.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::QExt(Box::new(Type::Ratio)),
                })
            }
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!("codegen: unsupported {op} element type {elem_ty}"),
            )),
        }
    }

    fn resolve_lowered_target(
        &self,
        cg: &Codegen<'_>,
        callee: &Expr,
        line: usize,
    ) -> DecResult<LoweredCallTarget> {
        let base_mod = cg.base_module_of(&self.mod_key).to_string();
        match &callee.kind {
            ExprKind::Ident(name) => {
                if let Some(ovs) = cg
                    .fn_overloads
                    .get(&(self.mod_key.clone(), name.clone()))
                    .cloned()
                {
                    return Ok(LoweredCallTarget::UserFn(ovs));
                }
                if base_mod != self.mod_key {
                    if let Some(ovs) = cg
                        .fn_overloads
                        .get(&(base_mod.clone(), name.clone()))
                        .cloned()
                    {
                        return Ok(LoweredCallTarget::UserFn(ovs));
                    }
                }
                if let Some(sym) = cg.imports_for(&self.mod_key).get(name).cloned() {
                    match sym {
                        ImportedSymbol::ModuleItem { module, item } => {
                            if let Some(ovs) = cg.fn_overloads.get(&(module, item)).cloned() {
                                return Ok(LoweredCallTarget::UserFn(ovs));
                            }
                        }
                        ImportedSymbol::Builtin(canon) => {
                            return Ok(LoweredCallTarget::Builtin(canon));
                        }
                        ImportedSymbol::ModuleNamespace(_) => {}
                    }
                }
                for (m, q) in &cg.qext_modules {
                    if m == &base_mod {
                        let qsub = format!("{base_mod}::{q}");
                        if let Some(ovs) = cg.fn_overloads.get(&(qsub, name.clone())).cloned() {
                            return Ok(LoweredCallTarget::UserFn(ovs));
                        }
                    }
                }
                Ok(LoweredCallTarget::Builtin(name.clone()))
            }
            ExprKind::Path(segs) => {
                if segs.len() == 3 && segs[0] == "aod" {
                    let ns = &segs[0..2];
                    if builtin_namespace_items(ns).is_some() {
                        return Ok(LoweredCallTarget::Builtin(canonical_builtin_name(
                            ns, &segs[2],
                        )));
                    }
                } else if segs.len() == 2 {
                    if let Some(qsub) = cg.resolve_qext_submodule(&self.mod_key, &segs[0]) {
                        if let Some(ovs) = cg.fn_overloads.get(&(qsub, segs[1].clone())).cloned() {
                            return Ok(LoweredCallTarget::UserFn(ovs));
                        }
                    }
                    if let Some(ImportedSymbol::ModuleNamespace(m)) =
                        cg.imports_for(&self.mod_key).get(&segs[0]).cloned()
                    {
                        if let Some(rest) = m.strip_prefix("aod::") {
                            let ns_vec = vec!["aod".to_string(), rest.to_string()];
                            return Ok(LoweredCallTarget::Builtin(canonical_builtin_name(
                                &ns_vec, &segs[1],
                            )));
                        }
                        if let Some(ovs) = cg.fn_overloads.get(&(m, segs[1].clone())).cloned() {
                            return Ok(LoweredCallTarget::UserFn(ovs));
                        }
                    }
                } else if segs.len() == 3 {
                    if let Some(qsub) =
                        cg.resolve_qext_submodule_3seg(&self.mod_key, &segs[0], &segs[1])
                    {
                        if let Some(ovs) = cg.fn_overloads.get(&(qsub, segs[2].clone())).cloned() {
                            return Ok(LoweredCallTarget::UserFn(ovs));
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

    fn invoke_lowered_target(
        &mut self,
        cg: &mut Codegen<'_>,
        target: &LoweredCallTarget,
        vals: Vec<Val>,
        hint: Option<&Type>,
        line: usize,
    ) -> DecResult<Val> {
        // 1. Direct match first
        match target {
            LoweredCallTarget::UserFn(overloads) => {
                if let Some(ov) = find_matching_overload(overloads, &vals) {
                    return self.emit_direct_user_call(ov, &vals);
                }
            }
            LoweredCallTarget::Builtin(canon) => {
                if let Some(res) = self.try_emit_builtin(cg, canon, &vals, hint, line)? {
                    return Ok(res);
                }
            }
        }

        // 2. Automatic rank-lifted unary call fallback
        if vals.len() == 1 {
            let v0 = self.materialize_if_range(vals.into_iter().next().unwrap());
            if type_seq_rank(&v0.dec_ty) > 0 {
                return self.emit_ho_each(cg, v0, target, line);
            }
        } else if vals.len() == 2 {
            // 3. Automatic rank-lifted binary call fallback
            let mut it = vals.into_iter();
            let lv = self.materialize_if_range(it.next().unwrap());
            let rv = self.materialize_if_range(it.next().unwrap());
            if type_seq_rank(&lv.dec_ty) > 0 || type_seq_rank(&rv.dec_ty) > 0 {
                return self.emit_rank_binary_target_loop(cg, target, lv, rv, line);
            }
        }

        Err(Diagnostic::at_line(
            DiagnosticCode::RuntimeError,
            line,
            "codegen: no matching overload or builtin signature",
        ))
    }

    fn emit_rank_binary_target_loop(
        &mut self,
        cg: &mut Codegen<'_>,
        target: &LoweredCallTarget,
        lv: Val,
        rv: Val,
        line: usize,
    ) -> DecResult<Val> {
        let lr = type_seq_rank(&lv.dec_ty);
        let rr = type_seq_rank(&rv.dec_ty);

        let (len_reg, fixed_len, step_l, step_r) = if lr > rr {
            let (le, fl) = extract_seq_elem_and_fixed(&lv.dec_ty);
            let len = self.fresh_reg();
            self.line(&format!(
                "  {len} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {})",
                lv.reg
            ));
            self.emit_fail_return_check();
            (len, fl, Some(le), None)
        } else if rr > lr {
            let (re, fr) = extract_seq_elem_and_fixed(&rv.dec_ty);
            let len = self.fresh_reg();
            self.line(&format!(
                "  {len} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {})",
                rv.reg
            ));
            self.emit_fail_return_check();
            (len, fr, None, Some(re))
        } else {
            let (le, fl) = extract_seq_elem_and_fixed(&lv.dec_ty);
            let (re, fr) = extract_seq_elem_and_fixed(&rv.dec_ty);
            let ll = self.fresh_reg();
            let rl = self.fresh_reg();
            self.line(&format!(
                "  {ll} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {})",
                lv.reg
            ));
            self.line(&format!(
                "  {rl} = call i64 @aod_rt_seq_len_raw(ptr %ctx, i64 {})",
                rv.reg
            ));
            self.emit_fail_return_check();
            let eq = self.fresh_reg();
            let ok_lbl = self.fresh_label("bcall_len_ok");
            let bad_lbl = self.fresh_label("bcall_len_bad");
            self.line(&format!("  {eq} = icmp eq i64 {ll}, {rl}"));
            self.line(&format!("  br i1 {eq}, label %{ok_lbl}, label %{bad_lbl}"));
            self.start_block(&bad_lbl);
            let (mg, ml) = cg.intern_bytes(b"sequence length mismatch in rank-lifted call");
            self.line(&format!(
                "  call void @aod_rt_fail(ptr %ctx, ptr {mg}, i64 {ml})"
            ));
            self.emit_depth_dec();
            self.emit_default_ret();
            self.start_block(&ok_lbl);
            let fixed = match (fl, fr) {
                (Some(n), Some(_)) => Some(n),
                _ => None,
            };
            (ll, fixed, Some(le), Some(re))
        };

        let buf_id = self.fresh_reg();
        self.line(&format!(
            "  {buf_id} = call i64 @aod_rt_seq_buf_new(ptr %ctx, i64 {len_reg})"
        ));
        let idx_slot = self.alloc_temp("i64");
        self.line(&format!("  store i64 0, ptr {idx_slot}"));
        let cond_lbl = self.fresh_label("bcall_cond");
        let body_lbl = self.fresh_label("bcall_body");
        let end_lbl = self.fresh_label("bcall_end");
        self.line(&format!("  br label %{cond_lbl}"));

        self.start_block(&cond_lbl);
        let idx = self.fresh_reg();
        let cmp = self.fresh_reg();
        self.line(&format!("  {idx} = load i64, ptr {idx_slot}"));
        self.line(&format!("  {cmp} = icmp slt i64 {idx}, {len_reg}"));
        self.line(&format!(
            "  br i1 {cmp}, label %{body_lbl}, label %{end_lbl}"
        ));

        self.start_block(&body_lbl);
        self.emit_fuel_tick();
        let item_l = if let Some(ref le) = step_l {
            let h = self.fresh_reg();
            self.line(&format!(
                "  {h} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {idx})",
                lv.reg
            ));
            self.unbox_handle_to_val(&h, le)
        } else {
            lv.clone()
        };
        let item_r = if let Some(ref re) = step_r {
            let h = self.fresh_reg();
            self.line(&format!(
                "  {h} = call i64 @aod_rt_seq_get_raw_idx(ptr %ctx, i64 {}, i64 {idx})",
                rv.reg
            ));
            self.unbox_handle_to_val(&h, re)
        } else {
            rv.clone()
        };
        let mapped = self.invoke_lowered_target(cg, target, vec![item_l, item_r], None, line)?;
        let out_elem_ty = mapped.dec_ty.clone();
        let mh = self.box_val_to_handle(&mapped);
        self.line(&format!(
            "  call void @aod_rt_seq_buf_push(ptr %ctx, i64 {buf_id}, i64 {mh})"
        ));
        let nxt = self.fresh_reg();
        self.line(&format!("  {nxt} = add nsw i64 {idx}, 1"));
        self.line(&format!("  store i64 {nxt}, ptr {idx_slot}"));
        let md_suffix = cg.alloc_loop_hint_md().unwrap_or_default();
        self.line(&format!("  br label %{cond_lbl}{md_suffix}"));
        self.terminated = true;

        self.start_block(&end_lbl);
        let is_arr = u8::from(fixed_len.is_some());
        let out_h = self.fresh_reg();
        self.line(&format!(
            "  {out_h} = call i64 @aod_rt_seq_buf_finish(ptr %ctx, i64 {buf_id}, i8 {is_arr})"
        ));
        self.emit_fail_return_check();
        let out_ty = match fixed_len {
            Some(n) => Type::Array(Box::new(out_elem_ty), n),
            None => Type::List(Box::new(out_elem_ty)),
        };
        Ok(Val {
            reg: out_h,
            dec_ty: out_ty,
        })
    }

    fn emit_direct_user_call(&mut self, ov: &FnOverload, vals: &[Val]) -> DecResult<Val> {
        let mut call_args = String::from("ptr %ctx");
        for (v, pt) in vals.iter().zip(&ov.param_types) {
            let lt = LlvmTy::from_dec(pt);
            let _ = write!(call_args, ", {} {}", lt.ir(), v.reg);
        }
        let callee_target = if let Some(slot_idx) = ov.dyn_slot {
            let fn_ptr = self.fresh_reg();
            self.line(&format!(
                "  {fn_ptr} = call ptr @aod_rt_resolve_dyn_slot(ptr %ctx, i32 {slot_idx}, ptr {})",
                ov.llvm_name
            ));
            fn_ptr
        } else {
            ov.llvm_name.clone()
        };
        let ret_lt = LlvmTy::from_dec(&ov.ret_type);
        let reg = if ret_lt == LlvmTy::Void {
            self.line(&format!("  call void {callee_target}({call_args})"));
            String::new()
        } else {
            let r = self.fresh_reg();
            self.line(&format!(
                "  {r} = call {} {callee_target}({call_args})",
                ret_lt.ir()
            ));
            r
        };
        self.emit_fail_return_check();
        Ok(Val {
            reg,
            dec_ty: ov.ret_type.clone(),
        })
    }
    fn emit_builtin(&mut self, canon: &str, vals: Vec<Val>, line: usize) -> DecResult<Val> {
        match (canon, vals.as_slice()) {
            ("superpose_cancel", [a, b])
                if *a.dec_ty.unwrap_read_only() == Type::T1
                    && *b.dec_ty.unwrap_read_only() == Type::T1 =>
            {
                let sum = self.fresh_reg();
                let is_neg = self.fresh_reg();
                let is_pos = self.fresh_reg();
                let s1 = self.fresh_reg();
                let s2 = self.fresh_reg();
                self.line(&format!("  {sum} = add i8 {}, {}", a.reg, b.reg));
                self.line(&format!("  {is_neg} = icmp slt i8 {sum}, 0"));
                self.line(&format!("  {is_pos} = icmp sgt i8 {sum}, 0"));
                self.line(&format!("  {s1} = select i1 {is_pos}, i8 1, i8 0"));
                self.line(&format!("  {s2} = select i1 {is_neg}, i8 -1, i8 {s1}"));
                Ok(Val {
                    reg: s2,
                    dec_ty: Type::T1,
                })
            }
            ("superpose_cancel", [a, b])
                if *a.dec_ty.unwrap_read_only() == Type::Int
                    && *b.dec_ty.unwrap_read_only() == Type::Int =>
            {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_int_superpose_cancel(ptr %ctx, i64 {}, i64 {})",
                    a.reg, b.reg
                ));
                self.emit_fail_return_check();
                Ok(Val {
                    reg: r,
                    dec_ty: Type::Int,
                })
            }
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::RuntimeError,
                line,
                format!("codegen: internal builtin helper unsupported for {canon}"),
            )),
        }
    }

    fn try_emit_builtin(
        &mut self,
        cg: &mut Codegen<'_>,
        canon: &str,
        vals: &[Val],
        _hint: Option<&Type>,
        line: usize,
    ) -> DecResult<Option<Val>> {
        let arg_tys: Vec<Type> = vals
            .iter()
            .map(|v| v.dec_ty.unwrap_read_only().clone())
            .collect();

        let res: Val = match (canon, arg_tys.as_slice()) {
            // --- Core constructors & exact conversions ---
            ("t1", [Type::T1]) => vals[0].clone(),
            ("t1", [Type::Int]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i8 @aod_rt_int_to_t1(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::T1,
                }
            }
            ("q4", [Type::Q4]) => vals[0].clone(),
            ("q4", [Type::Int]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i8 @aod_rt_int_to_q4(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Q4,
                }
            }
            ("q4", [Type::B8]) => {
                let ok = self.fresh_reg();
                let ok_lbl = self.fresh_label("q4_b8_ok");
                let bad_lbl = self.fresh_label("q4_b8_bad");
                self.line(&format!("  {ok} = icmp ule i8 {}, 15", vals[0].reg));
                self.line(&format!("  br i1 {ok}, label %{ok_lbl}, label %{bad_lbl}"));
                self.start_block(&bad_lbl);
                let (mg, ml) = cg.intern_bytes(b"q4 out of bounds [0, 15]");
                self.line(&format!(
                    "  call void @aod_rt_fail(ptr %ctx, ptr {mg}, i64 {ml})"
                ));
                self.emit_depth_dec();
                self.emit_default_ret();
                self.start_block(&ok_lbl);
                Val {
                    reg: vals[0].reg.clone(),
                    dec_ty: Type::Q4,
                }
            }
            ("b8", [Type::B8]) => vals[0].clone(),
            ("b8", [Type::Int]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i8 @aod_rt_int_to_b8(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::B8,
                }
            }
            ("b8", [Type::Bool]) => {
                let r = self.fresh_reg();
                self.line(&format!("  {r} = zext i1 {} to i8", vals[0].reg));
                Val {
                    reg: r,
                    dec_ty: Type::B8,
                }
            }
            ("b8", [Type::Q4]) => Val {
                reg: vals[0].reg.clone(),
                dec_ty: Type::B8,
            },
            ("b8", [Type::B64]) => {
                let ok = self.fresh_reg();
                let ok_lbl = self.fresh_label("b8_b64_ok");
                let bad_lbl = self.fresh_label("b8_b64_bad");
                self.line(&format!("  {ok} = icmp ule i64 {}, 255", vals[0].reg));
                self.line(&format!("  br i1 {ok}, label %{ok_lbl}, label %{bad_lbl}"));
                self.start_block(&bad_lbl);
                let (mg, ml) = cg.intern_bytes(b"b8 out of bounds [0, 255]");
                self.line(&format!(
                    "  call void @aod_rt_fail(ptr %ctx, ptr {mg}, i64 {ml})"
                ));
                self.emit_depth_dec();
                self.emit_default_ret();
                self.start_block(&ok_lbl);
                let r = self.fresh_reg();
                self.line(&format!("  {r} = trunc i64 {} to i8", vals[0].reg));
                Val {
                    reg: r,
                    dec_ty: Type::B8,
                }
            }
            ("b64", [Type::B64]) => vals[0].clone(),
            ("b64", [Type::Int]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_int_to_b64_view(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::B64,
                }
            }
            ("b64", [Type::B8 | Type::Q4]) => {
                let r = self.fresh_reg();
                self.line(&format!("  {r} = zext i8 {} to i64", vals[0].reg));
                Val {
                    reg: r,
                    dec_ty: Type::B64,
                }
            }
            ("b64_bits", [Type::Int]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_b64_from_unsigned_int(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::B64,
                }
            }
            ("u64_int", [Type::B64]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_int_from_u64(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("int", [Type::Int]) => vals[0].clone(),
            ("int", [Type::T1]) => {
                // In 2-bit Trit-Boxed representation: -1 -> 2 (0b10), 0 -> 0 (0b00), +1 -> 1 (0b01)
                let is_neg = self.fresh_reg();
                let is_pos = self.fresh_reg();
                let s1 = self.fresh_reg();
                let s2 = self.fresh_reg();
                self.line(&format!("  {is_neg} = icmp slt i8 {}, 0", vals[0].reg));
                self.line(&format!("  {is_pos} = icmp sgt i8 {}, 0", vals[0].reg));
                self.line(&format!("  {s1} = select i1 {is_pos}, i64 1, i64 0"));
                self.line(&format!("  {s2} = select i1 {is_neg}, i64 2, i64 {s1}"));
                Val {
                    reg: s2,
                    dec_ty: Type::Int,
                }
            }
            ("int", [Type::Q4]) => {
                let u = self.fresh_reg();
                let r = self.fresh_reg();
                self.line(&format!("  {u} = zext i8 {} to i64", vals[0].reg));
                self.line(&format!("  {r} = call i64 @__aod_fast_pack_t32(i64 {u})"));
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("int", [Type::B8]) => {
                let u = self.fresh_reg();
                let r = self.fresh_reg();
                self.line(&format!("  {u} = zext i8 {} to i64", vals[0].reg));
                self.line(&format!("  {r} = call i64 @__aod_fast_pack_t32(i64 {u})"));
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("int", [Type::B64]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_int_from_b64_view(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("int", [Type::Ratio]) => {
                let (na, da) = self.extract_ratio_words(&vals[0].reg);
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_ratio_to_int_exact(ptr %ctx, i64 {na}, i64 {da})"
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("ratio", [Type::Ratio]) => vals[0].clone(),
            ("ratio", [Type::Int]) => {
                let is_inl = self.fresh_reg();
                let inl_lbl = self.fresh_label("rat_int_inl");
                let slow_lbl = self.fresh_label("rat_int_slow");
                let join_lbl = self.fresh_label("rat_int_join");
                let out = self.alloc_temp("%DecRatio");
                self.line(&format!(
                    "  {is_inl} = icmp ult i64 {}, -4611686018427387904",
                    vals[0].reg
                ));
                self.line(&format!(
                    "  br i1 {is_inl}, label %{inl_lbl}, label %{slow_lbl}"
                ));
                self.start_block(&inl_lbl);
                let p0 = self.fresh_reg();
                let p1 = self.fresh_reg();
                self.line(&format!(
                    "  {p0} = insertvalue %DecRatio undef, i64 {}, 0",
                    vals[0].reg
                ));
                self.line(&format!("  {p1} = insertvalue %DecRatio {p0}, i64 1, 1"));
                self.line(&format!("  store %DecRatio {p1}, ptr {out}"));
                self.line(&format!("  br label %{join_lbl}"));
                self.start_block(&slow_lbl);
                self.line(&format!(
                    "  call void @aod_rt_ratio_from_ints(ptr %ctx, i64 {}, i64 1, ptr {out})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                self.line(&format!("  br label %{join_lbl}"));
                self.start_block(&join_lbl);
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecRatio, ptr {out}"));
                Val {
                    reg: r,
                    dec_ty: Type::Ratio,
                }
            }
            ("ratio", [Type::T1]) => {
                let is_neg = self.fresh_reg();
                let is_pos = self.fresh_reg();
                let s1 = self.fresh_reg();
                let nw = self.fresh_reg();
                let p0 = self.fresh_reg();
                let p1 = self.fresh_reg();
                self.line(&format!("  {is_neg} = icmp slt i8 {}, 0", vals[0].reg));
                self.line(&format!("  {is_pos} = icmp sgt i8 {}, 0", vals[0].reg));
                self.line(&format!("  {s1} = select i1 {is_pos}, i64 1, i64 0"));
                self.line(&format!("  {nw} = select i1 {is_neg}, i64 2, i64 {s1}"));
                self.line(&format!("  {p0} = insertvalue %DecRatio undef, i64 {nw}, 0"));
                self.line(&format!("  {p1} = insertvalue %DecRatio {p0}, i64 1, 1"));
                Val {
                    reg: p1,
                    dec_ty: Type::Ratio,
                }
            }
            ("ratio", [Type::Int, Type::Int]) => {
                let out = self.alloc_temp("%DecRatio");
                self.line(&format!(
                    "  call void @aod_rt_ratio_from_ints(ptr %ctx, i64 {}, i64 {}, ptr {out})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecRatio, ptr {out}"));
                Val {
                    reg: r,
                    dec_ty: Type::Ratio,
                }
            }
            ("num" | "den", [Type::Ratio]) => {
                self.emit_ratio_num_or_den(&vals[0].reg, canon == "den")
            }
            ("bytes", [Type::Bytes]) => vals[0].clone(),
            ("bytes", [Type::B64]) | ("b64_le_bytes", [Type::B64]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_bytes_from_b64(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Bytes,
                }
            }
            ("bytes", [Type::Addr8]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_bytes_from_seq(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Bytes,
                }
            }
            ("bytes", [Type::Array(elem, _) | Type::List(elem)])
                if **elem == Type::B8 || **elem == Type::Unknown =>
            {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_bytes_from_seq(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Bytes,
                }
            }
            (
                "u16_le_bytes" | "u32_le_bytes" | "i32_le_bytes" | "i64_le_bytes",
                [Type::Int],
            ) => {
                let kind: u8 = match canon {
                    "u16_le_bytes" => 0,
                    "u32_le_bytes" => 1,
                    "i32_le_bytes" => 2,
                    _ => 3,
                };
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_le_bytes_from_int(ptr %ctx, i64 {}, i8 {kind})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Bytes,
                }
            }
            ("push_byte", [Type::Bytes, Type::B8]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_bytes_push(ptr %ctx, i64 {}, i8 {})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Bytes,
                }
            }
            ("concat_bytes", [Type::Bytes, Type::Bytes]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_bytes_concat(ptr %ctx, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Bytes,
                }
            }
            ("slice_bytes", [Type::Bytes, Type::Int, Type::Int]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_bytes_slice(ptr %ctx, i64 {}, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg, vals[2].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Bytes,
                }
            }
            ("sha256_bytes", [Type::Bytes]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_bytes_sha256(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Bytes,
                }
            }
            ("hex_bytes", [Type::Bytes]) => {
                let bh = self.fresh_reg();
                self.line(&format!(
                    "  {bh} = call i64 @aod_rt_bytes_hex(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: bh,
                    dec_ty: Type::Bytes,
                }
            }
            ("pack", [Type::Int]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_int_pack_t32(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("unpack", [Type::Int]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_int_unpack_t32(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("unpack_t9", [Type::Int]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_unpack_t9_int(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("unpack_t9", [Type::B64]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_unpack_t9_b64(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("pack_ratio", [Type::Ratio]) => {
                let (na, da) = self.extract_ratio_words(&vals[0].reg);
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_pack_ratio(ptr %ctx, i64 {na}, i64 {da})"
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::B64,
                }
            }
            ("unpack_ratio", [Type::B64]) => {
                let out = self.alloc_temp("%DecRatio");
                self.line(&format!(
                    "  call void @aod_rt_unpack_ratio(ptr %ctx, i64 {}, ptr {out})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecRatio, ptr {out}"));
                Val {
                    reg: r,
                    dec_ty: Type::Ratio,
                }
            }

            // --- Algebra, ternary, & bit queries ---
            ("neg", [Type::Int | Type::T1 | Type::Ratio]) => {
                self.emit_unary(cg, UnaryOp::Neg, vals[0].clone(), line)?
            }
            ("neg", [Type::QExt(inner)]) if **inner == Type::Ratio => {
                self.emit_unary(cg, UnaryOp::Neg, vals[0].clone(), line)?
            }
            ("bitnot", [Type::B8 | Type::B64 | Type::Q4]) => {
                self.emit_unary(cg, UnaryOp::BitNot, vals[0].clone(), line)?
            }
            ("abs", [Type::Int]) => {
                let sgn = self.fresh_reg();
                let sgn_fn = if cg.plan.trit_ssa_promotion {
                    "@__aod_int_sgn"
                } else {
                    "@aod_rt_int_sgn"
                };
                self.line(&format!(
                    "  {sgn} = call i8 {sgn_fn}(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                let is_neg = self.fresh_reg();
                self.line(&format!("  {is_neg} = icmp slt i8 {sgn}, 0"));
                let neg_v = self.emit_unary(cg, UnaryOp::Neg, vals[0].clone(), line)?;
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = select i1 {is_neg}, i64 {}, i64 {}",
                    neg_v.reg, vals[0].reg
                ));
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("abs", [Type::T1]) => {
                let is_neg = self.fresh_reg();
                let r = self.fresh_reg();
                self.line(&format!("  {is_neg} = icmp slt i8 {}, 0", vals[0].reg));
                self.line(&format!("  {r} = select i1 {is_neg}, i8 1, i8 {}", vals[0].reg));
                Val {
                    reg: r,
                    dec_ty: Type::T1,
                }
            }
            ("abs", [Type::Ratio]) => {
                let (na, da) = self.extract_ratio_words(&vals[0].reg);
                let sgn = self.fresh_reg();
                self.line(&format!(
                    "  {sgn} = call i8 @aod_rt_ratio_sgn(ptr %ctx, i64 {na}, i64 {da})"
                ));
                self.emit_fail_return_check();
                let is_neg = self.fresh_reg();
                self.line(&format!("  {is_neg} = icmp slt i8 {sgn}, 0"));
                let neg_v = self.emit_unary(cg, UnaryOp::Neg, vals[0].clone(), line)?;
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = select i1 {is_neg}, %DecRatio {}, %DecRatio {}",
                    neg_v.reg, vals[0].reg
                ));
                Val {
                    reg: r,
                    dec_ty: Type::Ratio,
                }
            }
            ("abs", [Type::QExt(inner)]) if **inner == Type::Ratio => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_qext_abs(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::QExt(Box::new(Type::Ratio)),
                }
            }
            ("sgn" | "signum", [Type::T1]) => Val {
                reg: vals[0].reg.clone(),
                dec_ty: Type::T1,
            },
            ("sgn" | "signum", [Type::Int]) => {
                let callee = if cg.plan.trit_ssa_promotion {
                    "@__aod_int_sgn"
                } else {
                    "@aod_rt_int_sgn"
                };
                let r = self.fresh_reg();
                self.line(&format!("  {r} = call i8 {callee}(ptr %ctx, i64 {})", vals[0].reg));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::T1,
                }
            }
            ("sgn" | "signum", [Type::Ratio]) => {
                let (na, da) = self.extract_ratio_words(&vals[0].reg);
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i8 @aod_rt_ratio_sgn(ptr %ctx, i64 {na}, i64 {da})"
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::T1,
                }
            }
            (
                "min" | "max",
                [Type::Int, Type::Int]
                | [Type::T1, Type::T1]
                | [Type::Ratio, Type::Ratio]
                | [Type::B8, Type::B8]
                | [Type::B64, Type::B64],
            ) => {
                let op = if canon == "min" {
                    BinaryOp::Min
                } else {
                    BinaryOp::Max
                };
                self.emit_binary(cg, op, vals[0].clone(), vals[1].clone(), line)?
            }
            ("min" | "max", [Type::QExt(a), Type::QExt(b)])
                if **a == Type::Ratio && **b == Type::Ratio =>
            {
                let cmp = self.fresh_reg();
                self.line(&format!(
                    "  {cmp} = call i8 @aod_rt_qext_cmp(ptr %ctx, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                let cond = self.fresh_reg();
                let pred = if canon == "min" { "slt" } else { "sgt" };
                self.line(&format!("  {cond} = icmp {pred} i8 {cmp}, 0"));
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = select i1 {cond}, i64 {}, i64 {}",
                    vals[0].reg, vals[1].reg
                ));
                Val {
                    reg: r,
                    dec_ty: Type::QExt(Box::new(Type::Ratio)),
                }
            }
            ("clamp", [t0, t1, t2])
                if t0 == t1
                    && t1 == t2
                    && matches!(t0, Type::Int | Type::B8 | Type::B64 | Type::T1 | Type::Ratio) =>
            {
                let bad = self.emit_binary(cg, BinaryOp::Gt, vals[1].clone(), vals[2].clone(), line)?;
                let bad_lbl = self.fresh_label("clamp_bad");
                let ok_lbl = self.fresh_label("clamp_ok");
                self.line(&format!(
                    "  br i1 {}, label %{bad_lbl}, label %{ok_lbl}",
                    bad.reg
                ));
                self.start_block(&bad_lbl);
                let (mg, mlen) = cg.intern_bytes(b"clamp(x, lo, hi) requires lo <= hi");
                self.line(&format!(
                    "  call void @aod_rt_fail(ptr %ctx, ptr {mg}, i64 {mlen})"
                ));
                self.emit_depth_dec();
                self.emit_default_ret();

                self.start_block(&ok_lbl);
                let lt_lo = self.emit_binary(cg, BinaryOp::Lt, vals[0].clone(), vals[1].clone(), line)?;
                let gt_hi = self.emit_binary(cg, BinaryOp::Gt, vals[0].clone(), vals[2].clone(), line)?;
                let ir_t = vals[0].llvm_ty().ir();
                let s1 = self.fresh_reg();
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {s1} = select i1 {}, {ir_t} {}, {ir_t} {}",
                    gt_hi.reg, vals[2].reg, vals[0].reg
                ));
                self.line(&format!(
                    "  {r} = select i1 {}, {ir_t} {}, {ir_t} {s1}",
                    lt_lo.reg, vals[1].reg
                ));
                Val {
                    reg: r,
                    dec_ty: t0.clone(),
                }
            }
            ("pow", [Type::Int, Type::Int]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_int_pow(ptr %ctx, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("pow", [Type::Ratio, Type::Int]) => {
                let (na, da) = self.extract_ratio_words(&vals[0].reg);
                let out = self.alloc_temp("%DecRatio");
                self.line(&format!(
                    "  call void @aod_rt_ratio_pow(ptr %ctx, i64 {na}, i64 {da}, i64 {}, ptr {out})",
                    vals[1].reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecRatio, ptr {out}"));
                Val {
                    reg: r,
                    dec_ty: Type::Ratio,
                }
            }
            ("gcd" | "lcm" | "trit_shl" | "trit_shr", [Type::Int, Type::Int]) => {
                let callee = match canon {
                    "gcd" => "@aod_rt_int_gcd",
                    "lcm" => "@aod_rt_int_lcm",
                    "trit_shl" => "@aod_rt_int_trit_shl",
                    _ => "@aod_rt_int_trit_shr",
                };
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 {callee}(ptr %ctx, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("superpose_cancel", [Type::T1, Type::T1] | [Type::Int, Type::Int]) => {
                self.emit_builtin("superpose_cancel", vals.to_vec(), line)?
            }
            ("popcnt", [Type::B64]) => {
                let cnt = self.fresh_reg();
                let r = self.fresh_reg();
                self.line(&format!("  {cnt} = call i64 @llvm.ctpop.i64(i64 {})", vals[0].reg));
                self.line(&format!("  {r} = call i64 @__aod_fast_pack_t32(i64 {cnt})"));
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("popcnt", [Type::B8 | Type::Q4]) => {
                let cnt8 = self.fresh_reg();
                let cnt64 = self.fresh_reg();
                let r = self.fresh_reg();
                self.line(&format!("  {cnt8} = call i8 @llvm.ctpop.i8(i8 {})", vals[0].reg));
                self.line(&format!("  {cnt64} = zext i8 {cnt8} to i64"));
                self.line(&format!("  {r} = call i64 @__aod_fast_pack_t32(i64 {cnt64})"));
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("popcnt_xor", [Type::Q4, Type::Q4]) => {
                let x = self.fresh_reg();
                let m = self.fresh_reg();
                let c8 = self.fresh_reg();
                let c64 = self.fresh_reg();
                let r = self.fresh_reg();
                self.line(&format!("  {x} = xor i8 {}, {}", vals[0].reg, vals[1].reg));
                self.line(&format!("  {m} = and i8 {x}, 15"));
                self.line(&format!("  {c8} = call i8 @llvm.ctpop.i8(i8 {m})"));
                self.line(&format!("  {c64} = zext i8 {c8} to i64"));
                self.line(&format!("  {r} = call i64 @__aod_fast_pack_t32(i64 {c64})"));
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("is_hamming1", [Type::Q4, Type::Q4]) => {
                let x = self.fresh_reg();
                let m = self.fresh_reg();
                let c = self.fresh_reg();
                let r = self.fresh_reg();
                self.line(&format!("  {x} = xor i8 {}, {}", vals[0].reg, vals[1].reg));
                self.line(&format!("  {m} = and i8 {x}, 15"));
                self.line(&format!("  {c} = call i8 @llvm.ctpop.i8(i8 {m})"));
                self.line(&format!("  {r} = icmp eq i8 {c}, 1"));
                Val {
                    reg: r,
                    dec_ty: Type::Bool,
                }
            }
            ("verify_sheddic_route" | "audit_sheddic_route", [Type::Int, Type::B8]) => {
                let b8 = self.fresh_reg();
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {b8} = call i8 @aod_rt_verify_sheddic_route(ptr %ctx, i64 {}, i8 {})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                self.line(&format!("  {r} = icmp ne i8 {b8}, 0"));
                Val {
                    reg: r,
                    dec_ty: Type::Bool,
                }
            }
            ("verify_sheddic_route" | "audit_sheddic_route", [Type::Int, Type::Int]) => {
                let b8 = self.fresh_reg();
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {b8} = call i8 @aod_rt_verify_sheddic_route_int(ptr %ctx, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                self.line(&format!("  {r} = icmp ne i8 {b8}, 0"));
                Val {
                    reg: r,
                    dec_ty: Type::Bool,
                }
            }
            ("verify_frg_cross_return", [Type::Int, Type::Int, Type::Int]) => {
                let b8 = self.fresh_reg();
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {b8} = call i8 @aod_rt_verify_frg_cross_return(ptr %ctx, i64 {}, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg, vals[2].reg
                ));
                self.emit_fail_return_check();
                self.line(&format!("  {r} = icmp ne i8 {b8}, 0"));
                Val {
                    reg: r,
                    dec_ty: Type::Bool,
                }
            }
            ("gate", [_, _]) => {
                let cv = self.materialize_if_range(vals[0].clone());
                let vv = self.materialize_if_range(vals[1].clone());
                self.emit_gate_val(cg, cv, vv, line)?
            }
            ("argmin" | "argmax", [Type::Array(_, _) | Type::List(_) | Type::Range]) => {
                let sv = self.materialize_if_range(vals[0].clone());
                let is_max = u8::from(canon == "argmax");
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_seq_argmin_argmax(ptr %ctx, i64 {}, i8 {is_max})",
                    sv.reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("range_seq", [Type::Int, Type::Int]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_range_seq(ptr %ctx, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::List(Box::new(Type::Int)),
                }
            }
            ("push" | "list_push", [Type::Array(elem, _) | Type::List(elem), _]) => {
                let eh = self.box_val_to_handle(&vals[1]);
                let out_elem = if **elem == Type::Unknown {
                    vals[1].dec_ty.clone()
                } else {
                    (**elem).clone()
                };
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_seq_push(ptr %ctx, i64 {}, i64 {eh})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::List(Box::new(out_elem)),
                }
            }
            (
                "concat" | "concat_seq",
                [Type::Array(e1, _) | Type::List(e1), Type::Array(e2, _) | Type::List(e2)],
            ) => {
                let any_ro = vals[0].dec_ty.is_read_only() || vals[1].dec_ty.is_read_only();
                let out_elem = if **e1 == Type::Unknown {
                    e2.unwrap_read_only().clone()
                } else {
                    e1.unwrap_read_only().clone()
                };
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_seq_concat(ptr %ctx, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                let base_ty = Type::List(Box::new(out_elem));
                Val {
                    reg: r,
                    dec_ty: if any_ro {
                        base_ty.wrap_read_only()
                    } else {
                        base_ty
                    },
                }
            }
            (
                "slice" | "slice_seq",
                [Type::Array(elem, _) | Type::List(elem), Type::Int, Type::Int],
            ) => {
                let is_ro = vals[0].dec_ty.is_read_only();
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_seq_slice(ptr %ctx, i64 {}, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg, vals[2].reg
                ));
                self.emit_fail_return_check();
                let base_ty = Type::List(Box::new(elem.unwrap_read_only().clone()));
                Val {
                    reg: r,
                    dec_ty: if is_ro {
                        base_ty.wrap_read_only()
                    } else {
                        base_ty
                    },
                }
            }
            ("set_at" | "array_set", [Type::Array(elem, n), Type::Int, _]) => {
                let eh = self.box_val_to_handle(&vals[2]);
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_array_set(ptr %ctx, i64 {}, i64 {}, i64 {eh})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Array(elem.clone(), *n),
                }
            }
            ("set_at" | "list_set", [Type::List(elem), Type::Int, _]) => {
                let eh = self.box_val_to_handle(&vals[2]);
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_array_set(ptr %ctx, i64 {}, i64 {}, i64 {eh})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::List(elem.clone()),
                }
            }
            ("list_set", [Type::Array(elem, _), Type::Int, _]) => {
                let eh = self.box_val_to_handle(&vals[2]);
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_array_set(ptr %ctx, i64 {}, i64 {}, i64 {eh})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::List(elem.clone()),
                }
            }
            ("map_put", [Type::Map(kt, vt), _, _]) => {
                let kh = self.box_val_to_handle(&vals[1]);
                let vh = self.box_val_to_handle(&vals[2]);
                let out_k = if **kt == Type::Unknown {
                    vals[1].dec_ty.clone()
                } else {
                    (**kt).clone()
                };
                let out_v = if **vt == Type::Unknown {
                    vals[2].dec_ty.clone()
                } else {
                    (**vt).clone()
                };
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_map_set(ptr %ctx, i64 {}, i64 {kh}, i64 {vh})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Map(Box::new(out_k), Box::new(out_v)),
                }
            }
            ("map_remove", [Type::Map(kt, vt), _]) => {
                let kh = self.box_val_to_handle(&vals[1]);
                let out_k = if **kt == Type::Unknown {
                    vals[1].dec_ty.clone()
                } else {
                    (**kt).clone()
                };
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_map_remove(ptr %ctx, i64 {}, i64 {kh})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Map(Box::new(out_k), vt.clone()),
                }
            }
            ("map_get_or", [Type::Map(_, vt), _, _]) => {
                let map_ro = vals[0].dec_ty.is_read_only();
                let raw_vt = if **vt == Type::Unknown {
                    vals[2].dec_ty.unwrap_read_only().clone()
                } else {
                    vt.unwrap_read_only().clone()
                };
                let mut out_vt = raw_vt;
                if (map_ro || vals[2].dec_ty.is_read_only()) && out_vt.needs_readonly_propagation()
                {
                    out_vt = out_vt.wrap_read_only();
                }
                let kh = self.box_val_to_handle(&vals[1]);
                let fb_h = self.box_val_to_handle(&vals[2]);
                let vh = self.fresh_reg();
                self.line(&format!(
                    "  {vh} = call i64 @aod_rt_map_get_or(ptr %ctx, i64 {}, i64 {kh}, i64 {fb_h})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                self.unbox_handle_to_val(&vh, &out_vt)
            }
            ("map_keys", [Type::Map(kt, _)]) => {
                let is_ro = vals[0].dec_ty.is_read_only();
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_map_keys_list(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                let base_ty = Type::List(Box::new(kt.unwrap_read_only().clone()));
                Val {
                    reg: r,
                    dec_ty: if is_ro {
                        base_ty.wrap_read_only()
                    } else {
                        base_ty
                    },
                }
            }
            ("map_entries", [Type::Map(kt, vt)]) => {
                let is_ro = vals[0].dec_ty.is_read_only();
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_map_entries_list(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                let base_ty = Type::List(Box::new(Type::Tuple(vec![
                    kt.unwrap_read_only().clone(),
                    vt.unwrap_read_only().clone(),
                ])));
                Val {
                    reg: r,
                    dec_ty: if is_ro {
                        base_ty.wrap_read_only()
                    } else {
                        base_ty
                    },
                }
            }
            ("contains", [Type::Map(_, _), _]) => {
                let kh = self.box_val_to_handle(&vals[1]);
                let b8 = self.fresh_reg();
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {b8} = call i8 @aod_rt_map_contains(ptr %ctx, i64 {}, i64 {kh})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                self.line(&format!("  {r} = icmp ne i8 {b8}, 0"));
                Val {
                    reg: r,
                    dec_ty: Type::Bool,
                }
            }
            ("contains", [Type::Array(_, _) | Type::List(_), _]) => {
                let eh = self.box_val_to_handle(&vals[1]);
                let b8 = self.fresh_reg();
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {b8} = call i8 @aod_rt_seq_contains(ptr %ctx, i64 {}, i64 {eh})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                self.line(&format!("  {r} = icmp ne i8 {b8}, 0"));
                Val {
                    reg: r,
                    dec_ty: Type::Bool,
                }
            }
            ("len", [Type::Bytes | Type::Array(_, _) | Type::List(_) | Type::Map(_, _)]) => {
                let r = self.fresh_reg();
                self.line(&format!("  {r} = call i64 @aod_rt_len(ptr %ctx, i64 {})", vals[0].reg));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }

            // --- Witness primitives ---
            ("trace", [Type::Bytes, Type::Bytes, Type::Q4]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_trace_new(ptr %ctx, i64 {}, i64 {}, i8 {})",
                    vals[0].reg, vals[1].reg, vals[2].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Trace,
                }
            }
            ("step", [Type::Trace, Type::Q4, Type::T1]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_trace_step(ptr %ctx, i64 {}, i8 {}, i8 {})",
                    vals[0].reg, vals[1].reg, vals[2].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Trace,
                }
            }
            ("hinge" | "dwell" | "close", [Type::Trace]) => {
                let callee = match canon {
                    "hinge" => "@aod_rt_trace_hinge",
                    "dwell" => "@aod_rt_trace_dwell",
                    _ => "@aod_rt_trace_close",
                };
                let r = self.fresh_reg();
                self.line(&format!("  {r} = call i64 {callee}(ptr %ctx, i64 {})", vals[0].reg));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Trace,
                }
            }
            ("phases" | "beats", [Type::Trace]) => {
                let callee = if canon == "phases" {
                    "@aod_rt_trace_phases"
                } else {
                    "@aod_rt_trace_beats"
                };
                let r = self.fresh_reg();
                self.line(&format!("  {r} = call i64 {callee}(ptr %ctx, i64 {})", vals[0].reg));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("rate", [Type::Trace]) => {
                let out = self.alloc_temp("%DecRatio");
                self.line(&format!(
                    "  call void @aod_rt_trace_rate(ptr %ctx, i64 {}, ptr {out})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecRatio, ptr {out}"));
                Val {
                    reg: r,
                    dec_ty: Type::Ratio,
                }
            }
            ("fold", [Type::Trace, Type::Trace, Type::Bytes, Type::Bytes]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_fold(ptr %ctx, i64 {}, i64 {}, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg, vals[2].reg, vals[3].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Fusion,
                }
            }
            ("outer", [Type::Fusion, Type::Q4]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_outer(ptr %ctx, i64 {}, i8 {})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Trace,
                }
            }
            ("fuse", [Type::Fusion, Type::Trace]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_fuse(ptr %ctx, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Trace,
                }
            }
            ("coupling", [Type::Trace, Type::Trace, Type::Bytes]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_coupling_new(ptr %ctx, i64 {}, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg, vals[2].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Coupling,
                }
            }
            ("contact", [Type::Coupling, Type::Int, Type::Int]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_coupling_contact(ptr %ctx, i64 {}, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg, vals[2].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Coupling,
                }
            }
            ("cadence", [Type::Coupling]) => {
                let out = self.alloc_temp("%DecRatio");
                self.line(&format!(
                    "  call void @aod_rt_coupling_cadence(ptr %ctx, i64 {}, ptr {out})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecRatio, ptr {out}"));
                Val {
                    reg: r,
                    dec_ty: Type::Ratio,
                }
            }
            ("lag" | "drift", [Type::Coupling]) => {
                let callee = if canon == "lag" {
                    "@aod_rt_coupling_lag"
                } else {
                    "@aod_rt_coupling_drift"
                };
                let r = self.fresh_reg();
                self.line(&format!("  {r} = call i64 {callee}(ptr %ctx, i64 {})", vals[0].reg));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("probability", [Type::Int, Type::Int, Type::Int, Type::T1]) => {
                let out = self.alloc_temp("%DecRatio");
                self.line(&format!(
                    "  call void @aod_rt_probability(ptr %ctx, i64 {}, i64 {}, i64 {}, i8 {}, ptr {out})",
                    vals[0].reg, vals[1].reg, vals[2].reg, vals[3].reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecRatio, ptr {out}"));
                Val {
                    reg: r,
                    dec_ty: Type::Ratio,
                }
            }

            // --- Support, ledger, boundary, & wavelet primitives ---
            ("addr8", [Type::Bytes]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_addr8_from_bytes(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Addr8,
                }
            }
            ("support", [Type::Bytes]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_support_from_bytes(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Support,
                }
            }
            ("support", [Type::Int, Type::Int, Type::Int, Type::Bool]) => {
                let b8 = self.fresh_reg();
                let r = self.fresh_reg();
                self.line(&format!("  {b8} = zext i1 {} to i8", vals[3].reg));
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_support_new(ptr %ctx, i64 {}, i64 {}, i64 {}, i8 {b8})",
                    vals[0].reg, vals[1].reg, vals[2].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Support,
                }
            }
            ("support_pi", [Type::Support]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_support_pi(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("support_rd", [Type::Support, Type::Int]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_support_rd(ptr %ctx, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("support_pressure" | "support_overhang", [Type::Support, Type::Int, Type::Int, Type::Int]) => {
                let callee = if canon == "support_pressure" {
                    "@aod_rt_support_pressure"
                } else {
                    "@aod_rt_support_overhang"
                };
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 {callee}(ptr %ctx, i64 {}, i64 {}, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg, vals[2].reg, vals[3].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("shedding_excess", [Type::Int, Type::Int]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_shedding_excess(ptr %ctx, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            ("sadar_promote", [Type::Int, Type::Int, Type::Ratio]) => {
                let (na, da) = self.extract_ratio_words(&vals[2].reg);
                let out = self.alloc_temp("%DecRatio");
                self.line(&format!(
                    "  call void @aod_rt_sadar_promote(ptr %ctx, i64 {}, i64 {}, i64 {na}, i64 {da}, ptr {out})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecRatio, ptr {out}"));
                Val {
                    reg: r,
                    dec_ty: Type::Ratio,
                }
            }
            ("boundary_of", [Type::Box3 | Type::Window | Type::Range | Type::SimResult]) => {
                self.emit_unary(cg, UnaryOp::Boundary, vals[0].clone(), line)?
            }
            (
                "window_dedup_count",
                [
                    Type::Bytes,
                    Type::Bytes,
                    Type::Array(_, _) | Type::List(_),
                    Type::Array(_, _) | Type::List(_),
                ],
            ) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_window_dedup_count(ptr %ctx, i64 {}, i64 {}, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg, vals[2].reg, vals[3].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }
            (
                "native_cadence_ratio",
                [Type::Bytes, Type::Int, Type::Bytes, Type::Int, Type::Bool, Type::Int],
            ) => {
                let b8 = self.fresh_reg();
                let out = self.alloc_temp("%DecRatio");
                self.line(&format!("  {b8} = zext i1 {} to i8", vals[4].reg));
                self.line(&format!(
                    "  call void @aod_rt_native_cadence_ratio(ptr %ctx, i64 {}, i64 {}, i64 {}, i64 {}, i8 {b8}, i64 {}, ptr {out})",
                    vals[0].reg, vals[1].reg, vals[2].reg, vals[3].reg, vals[5].reg
                ));
                self.emit_fail_return_check();
                let r = self.fresh_reg();
                self.line(&format!("  {r} = load %DecRatio, ptr {out}"));
                Val {
                    reg: r,
                    dec_ty: Type::Ratio,
                }
            }
            (
                "single_contact_association",
                [Type::Bytes, Type::Bytes, Type::Bytes, Type::Window, Type::Int],
            ) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_single_contact_assoc(ptr %ctx, i64 {}, i64 {}, i64 {}, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg, vals[2].reg, vals[3].reg, vals[4].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::CadenceAssociation,
                }
            }
            ("recipient_eval_demo", [Type::Int, Type::Int, Type::Window]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_recipient_eval_demo(ptr %ctx, i64 {}, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg, vals[2].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::RecipientOutcome,
                }
            }

            // --- Strictly quarantined `aod::qext` builtins ---
            ("aod::qext::ext_decimal", [Type::Bytes]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_ext_decimal(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::QExt(Box::new(Type::Ratio)),
                }
            }
            ("aod::qext::qext_from_ratio", [Type::Ratio]) => {
                let (na, da) = self.extract_ratio_words(&vals[0].reg);
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_qext_from_ratio(ptr %ctx, i64 {na}, i64 {da})"
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::QExt(Box::new(Type::Ratio)),
                }
            }
            (
                "aod::qext::qext_add"
                | "aod::qext::qext_sub"
                | "aod::qext::qext_mul"
                | "aod::qext::qext_div",
                [Type::QExt(a), Type::QExt(b)],
            ) if **a == Type::Ratio && **b == Type::Ratio => {
                let op_code: u8 = match canon {
                    "aod::qext::qext_add" => 0,
                    "aod::qext::qext_sub" => 1,
                    "aod::qext::qext_mul" => 2,
                    _ => 3,
                };
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_qext_binop(ptr %ctx, i8 {op_code}, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::QExt(Box::new(Type::Ratio)),
                }
            }
            (
                "aod::qext::qext_render"
                | "aod::qext::qext_render_bytes"
                | "aod::qext::qext_to_bytes_exact",
                [Type::QExt(a)],
            ) if **a == Type::Ratio => {
                let callee = if canon == "aod::qext::qext_to_bytes_exact" {
                    "@aod_rt_qext_to_bytes_exact"
                } else {
                    "@aod_rt_qext_render_bytes"
                };
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 {callee}(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::QExt(Box::new(Type::Bytes)),
                }
            }
            ("aod::qext::qext_round_half_even", [Type::QExt(a), Type::Int])
                if **a == Type::Ratio =>
            {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_qext_round_half_even(ptr %ctx, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::QExt(Box::new(Type::Ratio)),
                }
            }
            (
                "aod::qext::qext_format_fixed"
                | "aod::qext::qext_format_fixed_bytes"
                | "aod::qext::qext_format_bytes_fixed",
                [Type::QExt(a), Type::Int],
            ) if **a == Type::Ratio => {
                let callee = if canon == "aod::qext::qext_format_bytes_fixed" {
                    "@aod_rt_qext_format_bytes_fixed"
                } else {
                    "@aod_rt_qext_format_fixed_bytes"
                };
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 {callee}(ptr %ctx, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::QExt(Box::new(Type::Bytes)),
                }
            }

            // --- Explicit `aod::sys` bootstrap I/O (`Effect::SysIo`) ---
            ("aod::sys::cli_args", []) => {
                let r = self.fresh_reg();
                self.line(&format!("  {r} = call i64 @aod_rt_sys_cli_args(ptr %ctx)"));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::List(Box::new(Type::Bytes)),
                }
            }
            ("aod::sys::read_file_bytes", [Type::Bytes]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_sys_read_file_bytes(ptr %ctx, i64 {})",
                    vals[0].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Bytes,
                }
            }
            ("aod::sys::write_executable_elf", [Type::Bytes, Type::Bytes]) => {
                let r = self.fresh_reg();
                self.line(&format!(
                    "  {r} = call i64 @aod_rt_sys_write_executable_elf(ptr %ctx, i64 {}, i64 {})",
                    vals[0].reg, vals[1].reg
                ));
                self.emit_fail_return_check();
                Val {
                    reg: r,
                    dec_ty: Type::Int,
                }
            }

            _ => return Ok(None),
        };

        Ok(Some(res))
    }

    fn emit_simulate(
        &mut self,
        cg: &mut Codegen<'_>,
        prepare_name: &str,
        ticks_expr: &Expr,
        observer_names: &[String],
        line: usize,
    ) -> DecResult<Val> {
        let base_mod = cg.base_module_of(&self.mod_key).to_string();
        let (prep_mod, prep) = cg
            .prepares
            .get(&(self.mod_key.clone(), prepare_name.to_string()))
            .map(|p| (self.mod_key.clone(), p.clone()))
            .or_else(|| {
                cg.prepares
                    .get(&(base_mod.clone(), prepare_name.to_string()))
                    .map(|p| (base_mod.clone(), p.clone()))
            })
            .or_else(|| {
                if let Some(ImportedSymbol::ModuleItem { module, item }) =
                    cg.imports_for(&self.mod_key).get(prepare_name)
                {
                    cg.prepares
                        .get(&(module.clone(), item.clone()))
                        .map(|p| (module.clone(), p.clone()))
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("codegen: unknown prepare {prepare_name}"),
                )
            })?;

        let chamber = cg
            .chambers
            .get(&(prep_mod.clone(), prep.chamber.clone()))
            .cloned()
            .or_else(|| {
                if let Some(ImportedSymbol::ModuleItem { module, item }) =
                    cg.imports_for(&prep_mod).get(&prep.chamber)
                {
                    cg.chambers.get(&(module.clone(), item.clone())).cloned()
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::RuntimeError,
                    line,
                    format!("codegen: unknown chamber {}", prep.chamber),
                )
            })?;

        let saved_mod = self.mod_key.clone();
        self.mod_key = prep_mod.clone();
        self.push_scope();

        let cname_h = self.emit_str_handle(cg, &chamber.name);
        let space_v = self.emit_expr(cg, &chamber.space)?;
        let walls_code = chamber.walls.to_code();
        let bh = self.fresh_reg();
        self.line(&format!(
            "  {bh} = call i64 @aod_rt_sim_builder_new(ptr %ctx, i64 {cname_h}, i64 {}, i8 {walls_code})",
            space_v.reg
        ));
        self.emit_fail_return_check();

        if chamber.boundary_step.is_some()
            || chamber.contact_ttl.is_some()
            || chamber.fuse_on_resonance.is_some()
        {
            let (dx_reg, dy_reg, dz_reg) = if let Some(ref bs_expr) = chamber.boundary_step {
                let bs_v = self.emit_expr(cg, bs_expr)?;
                let dxh = self.fresh_reg();
                let dyh = self.fresh_reg();
                let dzh = self.fresh_reg();
                self.line(&format!(
                    "  {dxh} = call i64 @aod_rt_tuple_get(ptr %ctx, i64 {}, i64 0)",
                    bs_v.reg
                ));
                self.line(&format!(
                    "  {dyh} = call i64 @aod_rt_tuple_get(ptr %ctx, i64 {}, i64 1)",
                    bs_v.reg
                ));
                self.line(&format!(
                    "  {dzh} = call i64 @aod_rt_tuple_get(ptr %ctx, i64 {}, i64 2)",
                    bs_v.reg
                ));
                self.emit_fail_return_check();
                let dxv = self.unbox_handle_to_val(&dxh, &Type::Int);
                let dyv = self.unbox_handle_to_val(&dyh, &Type::Int);
                let dzv = self.unbox_handle_to_val(&dzh, &Type::Int);
                (dxv.reg, dyv.reg, dzv.reg)
            } else {
                ("0".to_string(), "0".to_string(), "0".to_string())
            };
            let (has_ttl, ttl_reg) = if let Some(ref ttl_expr) = chamber.contact_ttl {
                let tv = self.emit_expr(cg, ttl_expr)?;
                (1u8, tv.reg)
            } else {
                (0u8, "0".to_string())
            };
            let fuse_reg = if let Some(ref fus_expr) = chamber.fuse_on_resonance {
                let fv = self.emit_expr(cg, fus_expr)?;
                let f8 = self.fresh_reg();
                self.line(&format!("  {f8} = zext i1 {} to i8", fv.reg));
                f8
            } else {
                "0".to_string()
            };
            self.line(&format!(
                "  call void @aod_rt_sim_builder_set_dynamics(ptr %ctx, i64 {bh}, i64 {dx_reg}, i64 {dy_reg}, i64 {dz_reg}, i8 {has_ttl}, i64 {ttl_reg}, i8 {fuse_reg})"
            ));
            self.emit_fail_return_check();
        }

        for st in &prep.lets {
            self.emit_stmt(cg, st)?;
        }

        for clk in &prep.clocks {
            let nh = self.emit_str_handle(cg, &clk.name);
            let pv = self.emit_expr(cg, &clk.period)?;
            self.line(&format!(
                "  call void @aod_rt_sim_builder_add_clock(ptr %ctx, i64 {bh}, i64 {nh}, i64 {})",
                pv.reg
            ));
            self.emit_fail_return_check();
        }

        for inj in &prep.injections {
            let sup_v = self.emit_expr(cg, &inj.support)?;
            let pos_v = self.emit_expr(cg, &inj.pos)?;
            let xh = self.fresh_reg();
            let yh = self.fresh_reg();
            let zh = self.fresh_reg();
            self.line(&format!(
                "  {xh} = call i64 @aod_rt_tuple_get(ptr %ctx, i64 {}, i64 0)",
                pos_v.reg
            ));
            self.line(&format!(
                "  {yh} = call i64 @aod_rt_tuple_get(ptr %ctx, i64 {}, i64 1)",
                pos_v.reg
            ));
            self.line(&format!(
                "  {zh} = call i64 @aod_rt_tuple_get(ptr %ctx, i64 {}, i64 2)",
                pos_v.reg
            ));
            self.emit_fail_return_check();
            let xv = self.unbox_handle_to_val(&xh, &Type::Int);
            let yv = self.unbox_handle_to_val(&yh, &Type::Int);
            let zv = self.unbox_handle_to_val(&zh, &Type::Int);
            let dir_code = inj.dir.to_code();
            let orient_v = self.emit_expr(cg, &inj.orient)?;
            let shells_v = self.emit_expr(cg, &inj.shells)?;
            self.line(&format!(
                "  call void @aod_rt_sim_builder_add_carrier(ptr %ctx, i64 {bh}, i64 {}, i64 {}, i64 {}, i64 {}, i8 {dir_code}, i8 {}, i64 {})",
                sup_v.reg, xv.reg, yv.reg, zv.reg, orient_v.reg, shells_v.reg
            ));
            self.emit_fail_return_check();
        }

        self.pop_scope();
        self.mod_key = saved_mod;

        for obs_name in observer_names {
            let (obs_mod, obs) = cg
                .observes
                .get(&(self.mod_key.clone(), obs_name.clone()))
                .map(|o| (self.mod_key.clone(), o.clone()))
                .or_else(|| {
                    cg.observes
                        .get(&(base_mod.clone(), obs_name.clone()))
                        .map(|o| (base_mod.clone(), o.clone()))
                })
                .or_else(|| {
                    if let Some(ImportedSymbol::ModuleItem { module, item }) =
                        cg.imports_for(&self.mod_key).get(obs_name)
                    {
                        cg.observes
                            .get(&(module.clone(), item.clone()))
                            .map(|o| (module.clone(), o.clone()))
                    } else {
                        None
                    }
                })
                .ok_or_else(|| {
                    Diagnostic::at_line(
                        DiagnosticCode::RuntimeError,
                        line,
                        format!("codegen: unknown observe {obs_name}"),
                    )
                })?;

            let prev_mod = self.mod_key.clone();
            self.mod_key = obs_mod.clone();
            for probe in &obs.probes {
                match probe {
                    ProbeDecl::Declarative {
                        kind, name, region, ..
                    } => {
                        let kcode = kind.to_code();
                        let nh = self.emit_str_handle(cg, name);
                        let reg_v = self.emit_expr(cg, region)?;
                        self.line(&format!(
                            "  call void @aod_rt_sim_builder_add_decl_obs(ptr %ctx, i64 {bh}, i8 {kcode}, i64 {nh}, i64 {})",
                            reg_v.reg
                        ));
                        self.emit_fail_return_check();
                    }
                    ProbeDecl::Procedural { name, .. } => {
                        let nh = self.emit_str_handle(cg, name);
                        let cb_sym = cg.obs_callbacks
                            [&(obs_mod.clone(), obs.name.clone(), name.clone())]
                            .clone();
                        self.line(&format!(
                            "  call void @aod_rt_sim_builder_add_proc_obs(ptr %ctx, i64 {bh}, i64 {nh}, ptr {cb_sym})"
                        ));
                        self.emit_fail_return_check();
                    }
                }
            }
            self.mod_key = prev_mod;
        }

        let ticks_v = self.emit_expr(cg, ticks_expr)?;
        let par_flag = u8::from(cg.plan.parallel_observers);
        let res_h = self.fresh_reg();
        self.line(&format!(
            "  {res_h} = call i64 @aod_rt_sim_run_ex(ptr %ctx, i64 {bh}, i64 {}, i8 {par_flag})",
            ticks_v.reg
        ));
        self.emit_fail_return_check();
        Ok(Val {
            reg: res_h,
            dec_ty: Type::SimResult,
        })
    }
}

// ============================================================================
// Free Codegen Type & Mangling Helpers
// ============================================================================

fn seq_elem_type(ty: &Type) -> Option<Type> {
    match ty.unwrap_read_only() {
        Type::Array(e, _) | Type::List(e) => Some((**e).clone()),
        Type::Range => Some(Type::Int),
        Type::Bytes => Some(Type::B8),
        _ => None,
    }
}

fn extract_seq_elem_and_fixed(ty: &Type) -> (Type, Option<usize>) {
    match ty.unwrap_read_only() {
        Type::Array(e, n) => ((**e).clone(), Some(*n)),
        Type::List(e) => ((**e).clone(), None),
        Type::Range => (Type::Int, None),
        Type::Bytes => (Type::B8, None),
        other => (other.clone(), None),
    }
}

fn type_seq_rank(ty: &Type) -> usize {
    match ty.unwrap_read_only() {
        Type::Array(inner, _) | Type::List(inner) => 1 + type_seq_rank(inner),
        Type::Range => 1,
        _ => 0,
    }
}

fn type_contains_unknown(ty: &Type) -> bool {
    match ty {
        Type::Unknown => true,
        Type::Array(e, _) | Type::List(e) | Type::ReadOnly(e) | Type::QExt(e) => {
            type_contains_unknown(e)
        }
        Type::Map(k, v) => type_contains_unknown(k) || type_contains_unknown(v),
        Type::Tuple(ts) => ts.iter().any(type_contains_unknown),
        _ => false,
    }
}

fn types_match_for_call(actual: &Type, expected: &Type) -> bool {
    let a = actual.unwrap_read_only();
    let e = expected.unwrap_read_only();
    if *a == Type::Unknown || *e == Type::Unknown {
        return true;
    }
    match (a, e) {
        (Type::Array(ae, an), Type::Array(ee, en)) => an == en && types_match_for_call(ae, ee),
        (Type::Array(ae, _) | Type::List(ae), Type::List(ee)) => types_match_for_call(ae, ee),
        (Type::Map(ak, av), Type::Map(ek, ev)) => {
            types_match_for_call(ak, ek) && types_match_for_call(av, ev)
        }
        (Type::Tuple(as_), Type::Tuple(es)) => {
            as_.len() == es.len()
                && as_
                    .iter()
                    .zip(es)
                    .all(|(x, y)| types_match_for_call(x, y))
        }
        (Type::QExt(ai), Type::QExt(ei)) => types_match_for_call(ai, ei),
        _ => a == e,
    }
}

fn find_matching_overload<'a>(overloads: &'a [FnOverload], vals: &[Val]) -> Option<&'a FnOverload> {
    // Prefer exact `[T; N]` array matches before `[T; N] -> [T]` coercion
    for ov in overloads {
        if ov.param_types.len() == vals.len()
            && vals
                .iter()
                .zip(&ov.param_types)
                .all(|(v, p)| v.dec_ty.unwrap_read_only() == p.unwrap_read_only())
        {
            return Some(ov);
        }
    }
    overloads.iter().find(|ov| {
        ov.param_types.len() == vals.len()
            && vals
                .iter()
                .zip(&ov.param_types)
                .all(|(v, p)| types_match_for_call(&v.dec_ty, p))
    })
}

fn is_irrefutable_codegen_pat(pat: &Pattern) -> bool {
    match pat {
        Pattern::Wildcard | Pattern::Binding(_) => true,
        Pattern::Tuple(ps) => ps.iter().all(is_irrefutable_codegen_pat),
        _ => false,
    }
}

fn is_irrefutable_codegen_param(param: &DecArmParam) -> bool {
    match param {
        DecArmParam::TypedVar { .. } => true,
        DecArmParam::TypedPattern { pattern, .. } | DecArmParam::Pattern(pattern) => {
            is_irrefutable_codegen_pat(pattern)
        }
    }
}

fn stmt_contains_return(stmt: &Stmt) -> bool {
    match &stmt.kind {
        StmtKind::Return(_) => true,
        StmtKind::QuarantineBlock { body } => body.iter().any(stmt_contains_return),
        _ => false,
    }
}

fn expr_mentions_ident(expr: &Expr, target: &str) -> bool {
    match &expr.kind {
        ExprKind::Ident(s) => s == target,
        ExprKind::Path(segs) => segs.last().is_some_and(|s| s == target),
        ExprKind::SeqLit(xs) | ExprKind::Tuple(xs) | ExprKind::BoxExpr(xs) => {
            xs.iter().any(|x| expr_mentions_ident(x, target))
        }
        ExprKind::MapLit(es) => es
            .iter()
            .any(|(k, v)| expr_mentions_ident(k, target) || expr_mentions_ident(v, target)),
        ExprKind::WindowLit { start, end } | ExprKind::Range { start, end } => {
            expr_mentions_ident(start, target) || expr_mentions_ident(end, target)
        }
        ExprKind::Unary { operand, .. }
        | ExprKind::Adverb { operand, .. }
        | ExprKind::Field { base: operand, .. } => expr_mentions_ident(operand, target),
        ExprKind::Binary { left, right, .. }
        | ExprKind::Index {
            base: left,
            index: right,
        } => expr_mentions_ident(left, target) || expr_mentions_ident(right, target),
        ExprKind::Call { callee, args } => {
            expr_mentions_ident(callee, target)
                || args.iter().any(|a| expr_mentions_ident(a, target))
        }
        ExprKind::Pipe { input, stage } => {
            expr_mentions_ident(input, target)
                || match stage.as_ref() {
                    PipeStage::Adverb(_) | PipeStage::Boundary => false,
                    PipeStage::Section { rhs, .. } => expr_mentions_ident(rhs, target),
                    PipeStage::Call { callee, extra_args } => {
                        expr_mentions_ident(callee, target)
                            || extra_args.iter().any(|a| expr_mentions_ident(a, target))
                    }
                }
        }
        ExprKind::Match { scrutinee, arms } => {
            expr_mentions_ident(scrutinee, target)
                || arms.iter().any(|a| {
                    a.guard
                        .as_ref()
                        .is_some_and(|g| expr_mentions_ident(g, target))
                        || expr_mentions_ident(&a.body, target)
                })
        }
        ExprKind::Simulate { ticks, .. } => expr_mentions_ident(ticks, target),
        _ => false,
    }
}

fn stmts_mention_ident(stmts: &[Stmt], target: &str) -> bool {
    stmts.iter().any(|s| match &s.kind {
        StmtKind::Let { value, .. }
        | StmtKind::Assert(value)
        | StmtKind::Emit(value)
        | StmtKind::ExprStmt(value)
        | StmtKind::Return(Some(value)) => expr_mentions_ident(value, target),
        StmtKind::QuarantineBlock { body } => stmts_mention_ident(body, target),
        StmtKind::Return(None) | StmtKind::Pass => false,
    })
}

fn qualifies_for_unboxed_evolve(
    fn_name: &str,
    param_types: &[Type],
    ret_type: &Type,
    multi_arms: &[DecArm],
) -> bool {
    if multi_arms.len() != 1 || !matches!(param_types.len(), 2 | 3) {
        return false;
    }
    if param_types[1] != Type::Int || &param_types[0] != ret_type {
        return false;
    }
    let Type::Tuple(ref state_elems) = param_types[0] else {
        return false;
    };
    if !(2..=8).contains(&state_elems.len())
        || state_elems
            .iter()
            .any(|et| matches!(LlvmTy::from_dec(et), LlvmTy::Handle | LlvmTy::Void))
    {
        return false;
    }
    let arm = &multi_arms[0];
    if arm.guard.is_some() || arm.body.is_empty() {
        return false;
    }
    let sub_pats = match &arm.params[0] {
        DecArmParam::TypedPattern {
            pattern: Pattern::Tuple(ps),
            ..
        }
        | DecArmParam::Pattern(Pattern::Tuple(ps)) => ps,
        _ => return false,
    };
    if sub_pats.len() != state_elems.len()
        || !sub_pats
            .iter()
            .all(|sp| matches!(sp, Pattern::Wildcard | Pattern::Binding(_)))
    {
        return false;
    }
    if !matches!(
        &arm.params[1],
        DecArmParam::TypedVar { .. }
            | DecArmParam::TypedPattern {
                pattern: Pattern::Wildcard | Pattern::Binding(_),
                ..
            }
            | DecArmParam::Pattern(Pattern::Wildcard | Pattern::Binding(_))
    ) {
        return false;
    }
    if arm.params.len() == 3 && !is_irrefutable_codegen_param(&arm.params[2]) {
        return false;
    }
    if stmts_mention_ident(&arm.body, fn_name) {
        return false;
    }
    let last_idx = arm.body.len() - 1;
    if arm.body[..last_idx].iter().any(stmt_contains_return) {
        return false;
    }
    matches!(
        &arm.body[last_idx].kind,
        StmtKind::Return(Some(Expr {
            kind: ExprKind::Tuple(ret_elems),
            ..
        })) if ret_elems.len() == state_elems.len()
    )
}

fn sanitize_ident(s: &str) -> String {
    let mut out = String::with_capacity(s.len().max(1));
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push('x');
    }
    out
}

fn escape_llvm_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        if (0x20..=0x7E).contains(&b) && b != b'"' && b != b'\\' {
            out.push(b as char);
        } else {
            let _ = write!(out, "\\{b:02X}");
        }
    }
    out
}

// ============================================================================
// LLVM 18 Module Preamble: Trit-Boxed `DecInt` (i64) & `DecRatio` ({i64, i64})
// ============================================================================

const LLVM_MODULE_PREAMBLE: &str = r#"; AOD Dec Clean-Break LLVM 18 Module (Trit-Boxed 2-Bit Balanced Ternary ABI)
%AodRtContext = type { i64, i32, i32, i8, i8, [6 x i8], ptr }
%DecRatio = type { i64, i64 }

@.t1_ovf_msg = private unnamed_addr constant [20 x i8] c"t1 addition overflow", align 1

declare i64 @llvm.ctlz.i64(i64, i1 immarg)
declare i64 @llvm.cttz.i64(i64, i1 immarg)
declare i64 @llvm.ctpop.i64(i64)
declare i8 @llvm.ctpop.i8(i8)

declare void @aod_rt_ctx_init(ptr, i64, i32, i8)
declare i32 @aod_rt_cli_parse_args(i32, ptr, ptr)
declare i32 @aod_rt_ctx_finish(ptr)
declare void @aod_rt_fail(ptr, ptr, i64)
declare void @aod_rt_assert_failed(ptr, i64)
declare void @aod_rt_budget_exhausted(ptr, i8)

declare i64 @aod_rt_int_from_str(ptr, ptr, i64)
declare i64 @aod_rt_int_from_i64(ptr, i64)
declare i64 @aod_rt_int_from_u64(ptr, i64)
declare i64 @aod_rt_int_neg(ptr, i64)
declare i64 @aod_rt_int_abs(ptr, i64)
declare i64 @aod_rt_int_add(ptr, i64, i64)
declare i64 @aod_rt_int_sub(ptr, i64, i64)
declare i64 @aod_rt_int_mul(ptr, i64, i64)
declare i64 @aod_rt_int_div_floor(ptr, i64, i64)
declare i64 @aod_rt_int_rem_floor(ptr, i64, i64)
declare i64 @aod_rt_int_pow(ptr, i64, i64)
declare i64 @aod_rt_int_gcd(ptr, i64, i64)
declare i64 @aod_rt_int_lcm(ptr, i64, i64)
declare i64 @aod_rt_int_superpose_cancel(ptr, i64, i64)
declare i8 @aod_rt_t1_superpose_cancel(ptr, i8, i8)
declare i64 @aod_rt_int_trit_shl(ptr, i64, i64)
declare i64 @aod_rt_int_trit_shr(ptr, i64, i64)
declare i64 @aod_rt_int_pack_t32(ptr, i64)
declare i64 @aod_rt_int_unpack_t32(ptr, i64)
declare i64 @aod_rt_unpack_t9_b64(ptr, i64)
declare i64 @aod_rt_unpack_t9_int(ptr, i64)
declare i8 @aod_rt_verify_sheddic_route(ptr, i64, i8)
declare i8 @aod_rt_verify_sheddic_route_int(ptr, i64, i64)
declare i8 @aod_rt_verify_frg_cross_return(ptr, i64, i64, i64)
declare i64 @aod_rt_b64_from_unsigned_int(ptr, i64)
declare void @aod_rt_int_exact_div(ptr, i64, i64, ptr)
declare i8 @aod_rt_int_cmp(ptr, i64, i64)
declare i8 @aod_rt_int_sgn(ptr, i64)
declare i8 @aod_rt_int_to_t1(ptr, i64)
declare i8 @aod_rt_int_to_q4(ptr, i64)
declare i8 @aod_rt_int_to_b8(ptr, i64)
declare i64 @aod_rt_int_to_b64_view(ptr, i64)
declare i64 @aod_rt_int_from_b64_view(ptr, i64)

declare void @aod_rt_ratio_reduce_i128(ptr, i64, i64, i64, i64, ptr)
declare void @aod_rt_ratio_from_ints(ptr, i64, i64, ptr)
declare void @aod_rt_ratio_from_str(ptr, ptr, i64, ptr, i64, ptr)
declare void @aod_rt_ratio_neg(ptr, i64, i64, ptr)
declare void @aod_rt_ratio_abs(ptr, i64, i64, ptr)
declare void @aod_rt_ratio_add(ptr, i64, i64, i64, i64, ptr)
declare void @aod_rt_ratio_sub(ptr, i64, i64, i64, i64, ptr)
declare void @aod_rt_ratio_mul(ptr, i64, i64, i64, i64, ptr)
declare void @aod_rt_ratio_div(ptr, i64, i64, i64, i64, ptr)
declare i8 @aod_rt_ratio_cmp(ptr, i64, i64, i64, i64)
declare i8 @aod_rt_ratio_sgn(ptr, i64, i64)
declare i64 @aod_rt_ratio_num(ptr, i64, i64)
declare i64 @aod_rt_ratio_den(ptr, i64, i64)
declare i64 @aod_rt_ratio_to_int_exact(ptr, i64, i64)
declare i64 @aod_rt_pack_ratio(ptr, i64, i64)
declare void @aod_rt_unpack_ratio(ptr, i64, ptr)
declare void @aod_rt_ratio_pow(ptr, i64, i64, i64, ptr)

declare void @aod_rt_emit_null(ptr)
declare void @aod_rt_emit_int(ptr, i64)
declare void @aod_rt_emit_bool(ptr, i8)
declare void @aod_rt_emit_qext_bool(ptr, i8)
declare void @aod_rt_emit_qext_bytes(ptr, i64)
declare void @aod_rt_emit_t1(ptr, i8)
declare void @aod_rt_emit_b8(ptr, i8)
declare void @aod_rt_emit_b64(ptr, i64)
declare void @aod_rt_emit_q4(ptr, i8)
declare void @aod_rt_emit_ratio(ptr, i64, i64)
declare void @aod_rt_emit_handle(ptr, i64)

declare i64 @aod_rt_str_new(ptr, ptr, i64)
declare i64 @aod_rt_bytes_new(ptr, ptr, i64)
declare i64 @aod_rt_bytes_from_seq(ptr, i64)
declare i64 @aod_rt_bytes_from_b64(ptr, i64)
declare i8 @aod_rt_bytes_get(ptr, i64, i64)
declare i64 @aod_rt_bytes_slice(ptr, i64, i64, i64)
declare i64 @aod_rt_bytes_concat(ptr, i64, i64)
declare i64 @aod_rt_bytes_sha256(ptr, i64)
declare i64 @aod_rt_bytes_hex(ptr, i64)
declare i64 @aod_rt_str_from_bytes(ptr, i64)
declare i64 @aod_rt_le_bytes_from_int(ptr, i64, i8)
declare i64 @aod_rt_bytes_push(ptr, i64, i8)

declare i64 @aod_rt_sys_cli_args(ptr)
declare i64 @aod_rt_sys_read_file_bytes(ptr, i64)
declare i64 @aod_rt_sys_write_executable_elf(ptr, i64, i64)

declare i8 @aod_rt_handle_eq(ptr, i64, i64)
declare i8 @aod_rt_handle_cmp(ptr, i64, i64)
declare i64 @aod_rt_val_unit(ptr)
declare i64 @aod_rt_val_null(ptr)
declare i64 @aod_rt_val_from_int(ptr, i64)
declare i64 @aod_rt_val_to_int(ptr, i64)
declare i64 @aod_rt_val_from_bool(ptr, i8)
declare i8 @aod_rt_val_to_bool(ptr, i64)
declare i64 @aod_rt_val_from_qext_bool(ptr, i8)
declare i8 @aod_rt_val_to_qext_bool(ptr, i64)
declare i64 @aod_rt_val_from_t1(ptr, i8)
declare i8 @aod_rt_val_to_t1(ptr, i64)
declare i64 @aod_rt_val_from_b8(ptr, i8)
declare i8 @aod_rt_val_to_b8(ptr, i64)
declare i64 @aod_rt_val_from_b64(ptr, i64)
declare i64 @aod_rt_val_to_b64(ptr, i64)
declare i64 @aod_rt_val_from_q4(ptr, i8)
declare i8 @aod_rt_val_to_q4(ptr, i64)
declare i64 @aod_rt_val_from_ratio(ptr, i64, i64)
declare void @aod_rt_val_to_ratio(ptr, i64, ptr)

declare i64 @aod_rt_addr8_parse(ptr, ptr, i64)
declare i64 @aod_rt_addr8_from_str(ptr, i64)
declare i64 @aod_rt_addr8_from_bytes(ptr, i64)
declare i64 @aod_rt_support_parse(ptr, ptr, i64)
declare i64 @aod_rt_support_from_str(ptr, i64)
declare i64 @aod_rt_support_from_bytes(ptr, i64)
declare i64 @aod_rt_support_new(ptr, i64, i64, i64, i8)
declare i64 @aod_rt_support_pi(ptr, i64)
declare i64 @aod_rt_support_rd(ptr, i64, i64)
declare i64 @aod_rt_support_pressure(ptr, i64, i64, i64, i64)
declare i64 @aod_rt_support_overhang(ptr, i64, i64, i64, i64)
declare i64 @aod_rt_shedding_excess(ptr, i64, i64)
declare void @aod_rt_sadar_promote(ptr, i64, i64, i64, i64, ptr)
declare void @aod_rt_probability(ptr, i64, i64, i64, i8, ptr)
declare i64 @aod_rt_window_new(ptr, i64, i64)
declare void @aod_rt_window_bounds(ptr, i64, ptr, ptr)
declare i64 @aod_rt_range_new(ptr, i64, i64)
declare void @aod_rt_range_bounds(ptr, i64, ptr, ptr)
declare i64 @aod_rt_range_to_list(ptr, i64)
declare i64 @aod_rt_box_new(ptr, i64, i64, i64)
declare i64 @aod_rt_boundary_of(ptr, i64)

declare i64 @aod_rt_trace_new(ptr, i64, i64, i8)
declare i64 @aod_rt_trace_step(ptr, i64, i8, i8)
declare i64 @aod_rt_trace_hinge(ptr, i64)
declare i64 @aod_rt_trace_dwell(ptr, i64)
declare i64 @aod_rt_trace_close(ptr, i64)
declare i64 @aod_rt_trace_phases(ptr, i64)
declare i64 @aod_rt_trace_beats(ptr, i64)
declare void @aod_rt_trace_rate(ptr, i64, ptr)
declare i64 @aod_rt_fold(ptr, i64, i64, i64, i64)
declare i64 @aod_rt_fusion_new(ptr, i64, i64, i64, i64)
declare i64 @aod_rt_outer(ptr, i64, i8)
declare i64 @aod_rt_fuse(ptr, i64, i64)
declare i64 @aod_rt_coupling_new(ptr, i64, i64, i64)
declare i64 @aod_rt_coupling_contact(ptr, i64, i64, i64)
declare void @aod_rt_coupling_cadence(ptr, i64, ptr)
declare i64 @aod_rt_coupling_lag(ptr, i64)
declare i64 @aod_rt_coupling_drift(ptr, i64)
declare i64 @aod_rt_window_dedup_count(ptr, i64, i64, i64, i64)
declare void @aod_rt_native_cadence_ratio(ptr, i64, i64, i64, i64, i8, i64, ptr)
declare i64 @aod_rt_single_contact_assoc(ptr, i64, i64, i64, i64, i64)
declare i64 @aod_rt_recipient_eval_demo(ptr, i64, i64, i64)

declare i64 @aod_rt_qext_ext_decimal(ptr, i64)
declare i64 @aod_rt_ext_decimal(ptr, i64)
declare i64 @aod_rt_qext_from_ratio(ptr, i64, i64)
declare i64 @aod_rt_qext_binop(ptr, i8, i64, i64)
declare i64 @aod_rt_qext_neg(ptr, i64)
declare i64 @aod_rt_qext_abs(ptr, i64)
declare i8 @aod_rt_qext_cmp(ptr, i64, i64)
declare i64 @aod_rt_qext_render(ptr, i64)
declare i64 @aod_rt_qext_render_bytes(ptr, i64)
declare i64 @aod_rt_qext_to_bytes_exact(ptr, i64)
declare i64 @aod_rt_qext_round_half_even(ptr, i64, i64)
declare i64 @aod_rt_qext_format_fixed(ptr, i64, i64)
declare i64 @aod_rt_qext_format_fixed_bytes(ptr, i64, i64)
declare i64 @aod_rt_qext_format_bytes_fixed(ptr, i64, i64)

declare i64 @aod_rt_array_new(ptr, ptr, i64)
declare i64 @aod_rt_list_new(ptr, ptr, i64)
declare i64 @aod_rt_seq_get(ptr, i64, i64)
declare i64 @aod_rt_seq_get_raw_idx(ptr, i64, i64)
declare i64 @aod_rt_array_set(ptr, i64, i64, i64)
declare i64 @aod_rt_seq_len_raw(ptr, i64)
declare i64 @aod_rt_seq_sieve(ptr, i64, i64)
declare i64 @aod_rt_seq_argmin_argmax(ptr, i64, i8)
declare i64 @aod_rt_len(ptr, i64)
declare i64 @aod_rt_seq_push(ptr, i64, i64)
declare i64 @aod_rt_seq_concat(ptr, i64, i64)
declare i64 @aod_rt_seq_slice(ptr, i64, i64, i64)
declare i8 @aod_rt_seq_contains(ptr, i64, i64)
declare i64 @aod_rt_range_seq(ptr, i64, i64)
declare i64 @aod_rt_seq_buf_new(ptr, i64)
declare void @aod_rt_seq_buf_push(ptr, i64, i64)
declare i64 @aod_rt_seq_buf_finish(ptr, i64, i8)
declare i64 @aod_rt_zip_seq(ptr, i64, i64)
declare i64 @aod_rt_sum_int(ptr, i64)
declare i8 @aod_rt_sum_b8(ptr, i64)
declare i8 @aod_rt_sum_t1(ptr, i64)
declare void @aod_rt_sum_ratio(ptr, i64, ptr)
declare i64 @aod_rt_sum_qext_ratio(ptr, i64)
declare i64 @aod_rt_dot_int(ptr, i64, i64)
declare i8 @aod_rt_dot_b8(ptr, i64, i64)
declare i8 @aod_rt_dot_t1(ptr, i64, i64)
declare void @aod_rt_dot_ratio(ptr, i64, i64, ptr)
declare i64 @aod_rt_dot_qext_ratio(ptr, i64, i64)
declare i64 @aod_rt_tuple_new(ptr, ptr, i64)
declare i64 @aod_rt_tuple_get(ptr, i64, i64)
declare i64 @aod_rt_map_new(ptr, ptr, ptr, i64)
declare i64 @aod_rt_map_get(ptr, i64, i64)
declare i64 @aod_rt_map_get_or(ptr, i64, i64, i64)
declare i64 @aod_rt_map_set(ptr, i64, i64, i64)
declare i64 @aod_rt_map_remove(ptr, i64, i64)
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
declare void @aod_rt_sim_builder_set_dynamics(ptr, i64, i64, i64, i64, i8, i64, i8)
declare void @aod_rt_sim_builder_add_clock(ptr, i64, i64, i64)
declare void @aod_rt_sim_builder_add_carrier(ptr, i64, i64, i64, i64, i64, i8, i8, i64)
declare void @aod_rt_sim_builder_add_decl_obs(ptr, i64, i8, i64, i64)
declare void @aod_rt_sim_builder_add_proc_obs(ptr, i64, i64, ptr)
declare i64 @aod_rt_sim_run(ptr, i64, i64)
declare i64 @aod_rt_sim_run_ex(ptr, i64, i64, i8)
declare ptr @aod_rt_resolve_dyn_slot(ptr, i32, ptr)

define internal i64 @__aod_int_from_i64_fast(ptr %ctx, i64 %v) alwaysinline {
entry:
  %ge = icmp sge i64 %v, -926510094425920
  %le = icmp sle i64 %v, 926510094425920
  %fit = and i1 %ge, %le
  br i1 %fit, label %fast, label %slow
fast:
  %pw = call i64 @__aod_fast_pack_t32(i64 %v)
  ret i64 %pw
slow:
  %sw = call i64 @aod_rt_int_from_i64(ptr %ctx, i64 %v)
  ret i64 %sw
}

define internal i64 @__aod_swar_neg(i64 %w) alwaysinline {
entry:
  %even = and i64 %w, 6148914691236517205
  %odd = and i64 %w, -6148914691236517206
  %shl = shl i64 %even, 1
  %shr = lshr i64 %odd, 1
  %res = or i64 %shl, %shr
  ret i64 %res
}

define internal i64 @__aod_swar_bias(i64 %w) alwaysinline {
entry:
  %even = and i64 %w, 6148914691236517205
  %hi = shl i64 %even, 1
  %shr = lshr i64 %w, 1
  %or1 = or i64 %w, %shr
  %not1 = xor i64 %or1, -1
  %lo = and i64 %not1, 6148914691236517205
  %res = or i64 %hi, %lo
  ret i64 %res
}

define internal i64 @__aod_swar_unpack_unchecked(i64 %w) alwaysinline {
entry:
  %u1 = call i64 @__aod_swar_bias(i64 %w)
  %u1_lo = and i64 %u1, 3689348814741910323
  %u1_sh = lshr i64 %u1, 2
  %u1_hi = and i64 %u1_sh, 3689348814741910323
  %u1_hm = mul i64 %u1_hi, 3
  %u2 = add i64 %u1_lo, %u1_hm
  %u2_lo = and i64 %u2, 1085102592571150095
  %u2_sh = lshr i64 %u2, 4
  %u2_hi = and i64 %u2_sh, 1085102592571150095
  %u2_hm = mul i64 %u2_hi, 9
  %u4 = add i64 %u2_lo, %u2_hm
  %u4_lo = and i64 %u4, 71777214294589695
  %u4_sh = lshr i64 %u4, 8
  %u4_hi = and i64 %u4_sh, 71777214294589695
  %u4_hm = mul i64 %u4_hi, 81
  %u8l = add i64 %u4_lo, %u4_hm
  %u8_lo = and i64 %u8l, 281470681808895
  %u8_sh = lshr i64 %u8l, 16
  %u8_hi = and i64 %u8_sh, 281470681808895
  %u8_hm = mul i64 %u8_hi, 6561
  %u16l = add i64 %u8_lo, %u8_hm
  %lo32 = and i64 %u16l, 4294967295
  %hi32 = lshr i64 %u16l, 32
  %hi32_m = mul i64 %hi32, 43046721
  %biased = add i64 %lo32, %hi32_m
  %res = sub i64 %biased, 926510094425920
  ret i64 %res
}

define internal i64 @__aod_fast_pack_t32(i64 %n) alwaysinline {
entry:
  %u = add i64 %n, 926510094425920
  %lo16 = urem i64 %u, 43046721
  %hi16 = udiv i64 %u, 43046721
  %b01 = urem i64 %lo16, 6561
  %b23 = udiv i64 %lo16, 6561
  %b45 = urem i64 %hi16, 6561
  %b67 = udiv i64 %hi16, 6561
  %i0 = urem i64 %b01, 81
  %i1 = udiv i64 %b01, 81
  %i2 = urem i64 %b23, 81
  %i3 = udiv i64 %b23, 81
  %i4 = urem i64 %b45, 81
  %i5 = udiv i64 %b45, 81
  %i6 = urem i64 %b67, 81
  %i7 = udiv i64 %b67, 81
  %p0 = getelementptr inbounds [81 x i8], ptr @.pack4_lut, i64 0, i64 %i0
  %p1 = getelementptr inbounds [81 x i8], ptr @.pack4_lut, i64 0, i64 %i1
  %p2 = getelementptr inbounds [81 x i8], ptr @.pack4_lut, i64 0, i64 %i2
  %p3 = getelementptr inbounds [81 x i8], ptr @.pack4_lut, i64 0, i64 %i3
  %p4 = getelementptr inbounds [81 x i8], ptr @.pack4_lut, i64 0, i64 %i4
  %p5 = getelementptr inbounds [81 x i8], ptr @.pack4_lut, i64 0, i64 %i5
  %p6 = getelementptr inbounds [81 x i8], ptr @.pack4_lut, i64 0, i64 %i6
  %p7 = getelementptr inbounds [81 x i8], ptr @.pack4_lut, i64 0, i64 %i7
  %v0 = load i8, ptr %p0, align 1
  %v1 = load i8, ptr %p1, align 1
  %v2 = load i8, ptr %p2, align 1
  %v3 = load i8, ptr %p3, align 1
  %v4 = load i8, ptr %p4, align 1
  %v5 = load i8, ptr %p5, align 1
  %v6 = load i8, ptr %p6, align 1
  %v7 = load i8, ptr %p7, align 1
  %c0 = zext i8 %v0 to i64
  %c1 = zext i8 %v1 to i64
  %c2 = zext i8 %v2 to i64
  %c3 = zext i8 %v3 to i64
  %c4 = zext i8 %v4 to i64
  %c5 = zext i8 %v5 to i64
  %c6 = zext i8 %v6 to i64
  %c7 = zext i8 %v7 to i64
  %s1 = shl i64 %c1, 8
  %s2 = shl i64 %c2, 16
  %s3 = shl i64 %c3, 24
  %s4 = shl i64 %c4, 32
  %s5 = shl i64 %c5, 40
  %s6 = shl i64 %c6, 48
  %s7 = shl i64 %c7, 56
  %o01 = or i64 %c0, %s1
  %o23 = or i64 %s2, %s3
  %o45 = or i64 %s4, %s5
  %o67 = or i64 %s6, %s7
  %o0123 = or i64 %o01, %o23
  %o4567 = or i64 %o45, %o67
  %res = or i64 %o0123, %o4567
  ret i64 %res
}

define internal i64 @__aod_int_neg(ptr %ctx, i64 %a) alwaysinline {
entry:
  %inl = icmp ult i64 %a, -4611686018427387904
  br i1 %inl, label %fast, label %slow
fast:
  %r = call i64 @__aod_swar_neg(i64 %a)
  ret i64 %r
slow:
  %rs = call i64 @aod_rt_int_neg(ptr %ctx, i64 %a)
  ret i64 %rs
}

define internal i8 @__aod_int_sgn(ptr %ctx, i64 %a) alwaysinline {
entry:
  %isz = icmp eq i64 %a, 0
  br i1 %isz, label %zero, label %nz
zero:
  ret i8 0
nz:
  %inl = icmp ult i64 %a, -4611686018427387904
  br i1 %inl, label %fast, label %slow
fast:
  %lz = call i64 @llvm.ctlz.i64(i64 %a, i1 true)
  %odd = and i64 %lz, 1
  %pos = icmp eq i64 %odd, 1
  %s = select i1 %pos, i8 1, i8 -1
  ret i8 %s
slow:
  %rs = call i8 @aod_rt_int_sgn(ptr %ctx, i64 %a)
  ret i8 %rs
}

define internal i8 @__aod_int_cmp(ptr %ctx, i64 %a, i64 %b) alwaysinline {
entry:
  %ia = icmp ult i64 %a, -4611686018427387904
  %ib = icmp ult i64 %b, -4611686018427387904
  %both = and i1 %ia, %ib
  br i1 %both, label %fast, label %slow
fast:
  %eq = icmp eq i64 %a, %b
  br i1 %eq, label %same, label %diff
same:
  ret i8 0
diff:
  %ba = call i64 @__aod_swar_bias(i64 %a)
  %bb = call i64 @__aod_swar_bias(i64 %b)
  %lt = icmp ult i64 %ba, %bb
  %c = select i1 %lt, i8 -1, i8 1
  ret i8 %c
slow:
  %rs = call i8 @aod_rt_int_cmp(ptr %ctx, i64 %a, i64 %b)
  ret i8 %rs
}

define internal i64 @__aod_int_add(ptr %ctx, i64 %a, i64 %b) alwaysinline {
entry:
  %ia = icmp ult i64 %a, -4611686018427387904
  %ib = icmp ult i64 %b, -4611686018427387904
  %both = and i1 %ia, %ib
  br i1 %both, label %fast, label %slow
fast:
  %va = call i64 @__aod_swar_unpack_unchecked(i64 %a)
  %vb = call i64 @__aod_swar_unpack_unchecked(i64 %b)
  %sum = add nsw i64 %va, %vb
  %ge = icmp sge i64 %sum, -926510094425920
  %le = icmp sle i64 %sum, 926510094425920
  %fit = and i1 %ge, %le
  br i1 %fit, label %pack, label %spill
pack:
  %pw = call i64 @__aod_fast_pack_t32(i64 %sum)
  ret i64 %pw
spill:
  %sw = call i64 @aod_rt_int_from_i64(ptr %ctx, i64 %sum)
  ret i64 %sw
slow:
  %rs = call i64 @aod_rt_int_add(ptr %ctx, i64 %a, i64 %b)
  ret i64 %rs
}

define internal i64 @__aod_int_sub(ptr %ctx, i64 %a, i64 %b) alwaysinline {
entry:
  %ia = icmp ult i64 %a, -4611686018427387904
  %ib = icmp ult i64 %b, -4611686018427387904
  %both = and i1 %ia, %ib
  br i1 %both, label %fast, label %slow
fast:
  %va = call i64 @__aod_swar_unpack_unchecked(i64 %a)
  %vb = call i64 @__aod_swar_unpack_unchecked(i64 %b)
  %diff = sub nsw i64 %va, %vb
  %ge = icmp sge i64 %diff, -926510094425920
  %le = icmp sle i64 %diff, 926510094425920
  %fit = and i1 %ge, %le
  br i1 %fit, label %pack, label %spill
pack:
  %pw = call i64 @__aod_fast_pack_t32(i64 %diff)
  ret i64 %pw
spill:
  %sw = call i64 @aod_rt_int_from_i64(ptr %ctx, i64 %diff)
  ret i64 %sw
slow:
  %rs = call i64 @aod_rt_int_sub(ptr %ctx, i64 %a, i64 %b)
  ret i64 %rs
}

define internal i64 @__aod_int_mul(ptr %ctx, i64 %a, i64 %b) alwaysinline {
entry:
  %ia = icmp ult i64 %a, -4611686018427387904
  %ib = icmp ult i64 %b, -4611686018427387904
  %both = and i1 %ia, %ib
  br i1 %both, label %fast, label %slow
fast:
  %va = call i64 @__aod_swar_unpack_unchecked(i64 %a)
  %vb = call i64 @__aod_swar_unpack_unchecked(i64 %b)
  %va128 = sext i64 %va to i128
  %vb128 = sext i64 %vb to i128
  %prod = mul nsw i128 %va128, %vb128
  %ge = icmp sge i128 %prod, -926510094425920
  %le = icmp sle i128 %prod, 926510094425920
  %fit = and i1 %ge, %le
  br i1 %fit, label %pack, label %slow
pack:
  %p64 = trunc i128 %prod to i64
  %pw = call i64 @__aod_fast_pack_t32(i64 %p64)
  ret i64 %pw
slow:
  %rs = call i64 @aod_rt_int_mul(ptr %ctx, i64 %a, i64 %b)
  ret i64 %rs
}

define internal i64 @__aod_int_div_floor(ptr %ctx, i64 %a, i64 %b) alwaysinline {
entry:
  %ia = icmp ult i64 %a, -4611686018427387904
  %ib = icmp ult i64 %b, -4611686018427387904
  %nz = icmp ne i64 %b, 0
  %c1 = and i1 %ia, %ib
  %ok = and i1 %c1, %nz
  br i1 %ok, label %fast, label %slow
fast:
  %va = call i64 @__aod_swar_unpack_unchecked(i64 %a)
  %vb = call i64 @__aod_swar_unpack_unchecked(i64 %b)
  %qt = sdiv i64 %va, %vb
  %rt = srem i64 %va, %vb
  %rnz = icmp ne i64 %rt, 0
  %xr = xor i64 %rt, %vb
  %opp = icmp slt i64 %xr, 0
  %adj = and i1 %rnz, %opp
  %qm1 = sub nsw i64 %qt, 1
  %qf = select i1 %adj, i64 %qm1, i64 %qt
  %pw = call i64 @__aod_fast_pack_t32(i64 %qf)
  ret i64 %pw
slow:
  %rs = call i64 @aod_rt_int_div_floor(ptr %ctx, i64 %a, i64 %b)
  ret i64 %rs
}

define internal i64 @__aod_int_rem_floor(ptr %ctx, i64 %a, i64 %b) alwaysinline {
entry:
  %ia = icmp ult i64 %a, -4611686018427387904
  %ib = icmp ult i64 %b, -4611686018427387904
  %nz = icmp ne i64 %b, 0
  %c1 = and i1 %ia, %ib
  %ok = and i1 %c1, %nz
  br i1 %ok, label %fast, label %slow
fast:
  %va = call i64 @__aod_swar_unpack_unchecked(i64 %a)
  %vb = call i64 @__aod_swar_unpack_unchecked(i64 %b)
  %rt = srem i64 %va, %vb
  %rnz = icmp ne i64 %rt, 0
  %xr = xor i64 %rt, %vb
  %opp = icmp slt i64 %xr, 0
  %adj = and i1 %rnz, %opp
  %rp = add nsw i64 %rt, %vb
  %rf = select i1 %adj, i64 %rp, i64 %rt
  %pw = call i64 @__aod_fast_pack_t32(i64 %rf)
  ret i64 %pw
slow:
  %rs = call i64 @aod_rt_int_rem_floor(ptr %ctx, i64 %a, i64 %b)
  ret i64 %rs
}

define internal i64 @__aod_gcd_u64(i64 %a0, i64 %b0) {
entry:
  %az = icmp eq i64 %a0, 0
  br i1 %az, label %ret_b, label %chk_b
ret_b:
  ret i64 %b0
chk_b:
  %bz = icmp eq i64 %b0, 0
  br i1 %bz, label %ret_a, label %init
ret_a:
  ret i64 %a0
init:
  %za = call i64 @llvm.cttz.i64(i64 %a0, i1 true)
  %zb = call i64 @llvm.cttz.i64(i64 %b0, i1 true)
  %lt_z = icmp ult i64 %za, %zb
  %k = select i1 %lt_z, i64 %za, i64 %zb
  %u_init = lshr i64 %a0, %za
  br label %loop
loop:
  %u = phi i64 [ %u_init, %init ], [ %u_next, %step ]
  %v_raw = phi i64 [ %b0, %init ], [ %diff, %step ]
  %zv = call i64 @llvm.cttz.i64(i64 %v_raw, i1 true)
  %v = lshr i64 %v_raw, %zv
  %gt = icmp ugt i64 %u, %v
  %u_min = select i1 %gt, i64 %v, i64 %u
  %v_max = select i1 %gt, i64 %u, i64 %v
  %diff = sub i64 %v_max, %u_min
  %done = icmp eq i64 %diff, 0
  %u_next = add i64 %u_min, 0
  br i1 %done, label %exit, label %step
step:
  br label %loop
exit:
  %g = shl i64 %u_min, %k
  ret i64 %g
}

define internal %DecRatio @__aod_ratio_norm_i128(ptr %ctx, i128 %n_in, i128 %d_in) {
entry:
  %dz = icmp eq i128 %d_in, 0
  br i1 %dz, label %slow, label %nz_den
nz_den:
  %nz = icmp eq i128 %n_in, 0
  br i1 %nz, label %ret_zero, label %canon
ret_zero:
  ret %DecRatio { i64 0, i64 1 }
canon:
  %dneg = icmp slt i128 %d_in, 0
  %nn = sub i128 0, %n_in
  %dn = sub i128 0, %d_in
  %n1 = select i1 %dneg, i128 %nn, i128 %n_in
  %d1 = select i1 %dneg, i128 %dn, i128 %d_in
  %nneg = icmp slt i128 %n1, 0
  %nneg_abs = sub i128 0, %n1
  %un = select i1 %nneg, i128 %nneg_abs, i128 %n1
  %or_ud = or i128 %un, %d1
  %hi_ud = lshr i128 %or_ud, 64
  %fit64 = icmp eq i128 %hi_ud, 0
  br i1 %fit64, label %gcd64, label %slow
gcd64:
  %un64 = trunc i128 %un to i64
  %d1_64 = trunc i128 %d1 to i64
  %g = call i64 @__aod_gcd_u64(i64 %un64, i64 %d1_64)
  %rn_u = udiv i64 %un64, %g
  %rd_u = udiv i64 %d1_64, %g
  %u_fit = icmp ule i64 %rn_u, 926510094425920
  %d_fit = icmp ule i64 %rd_u, 926510094425920
  %fit = and i1 %u_fit, %d_fit
  br i1 %fit, label %pack, label %slow
pack:
  %rn_neg = sub i64 0, %rn_u
  %rn64 = select i1 %nneg, i64 %rn_neg, i64 %rn_u
  %nw = call i64 @__aod_fast_pack_t32(i64 %rn64)
  %dw = call i64 @__aod_fast_pack_t32(i64 %rd_u)
  %p0 = insertvalue %DecRatio undef, i64 %nw, 0
  %p1 = insertvalue %DecRatio %p0, i64 %dw, 1
  ret %DecRatio %p1
slow:
  %out = alloca %DecRatio, align 8
  %n_lo = trunc i128 %n_in to i64
  %n_sh = ashr i128 %n_in, 64
  %n_hi = trunc i128 %n_sh to i64
  %d_lo = trunc i128 %d_in to i64
  %d_sh = ashr i128 %d_in, 64
  %d_hi = trunc i128 %d_sh to i64
  call void @aod_rt_ratio_reduce_i128(ptr %ctx, i64 %n_lo, i64 %n_hi, i64 %d_lo, i64 %d_hi, ptr %out)
  %res = load %DecRatio, ptr %out, align 8
  ret %DecRatio %res
}

define internal %DecRatio @__aod_int_exact_div(ptr %ctx, i64 %a, i64 %b) alwaysinline {
entry:
  %ia = icmp ult i64 %a, -4611686018427387904
  %ib = icmp ult i64 %b, -4611686018427387904
  %nz = icmp ne i64 %b, 0
  %c1 = and i1 %ia, %ib
  %ok = and i1 %c1, %nz
  br i1 %ok, label %fast, label %slow
fast:
  %va = call i64 @__aod_swar_unpack_unchecked(i64 %a)
  %vb = call i64 @__aod_swar_unpack_unchecked(i64 %b)
  %va128 = sext i64 %va to i128
  %vb128 = sext i64 %vb to i128
  %r = call %DecRatio @__aod_ratio_norm_i128(ptr %ctx, i128 %va128, i128 %vb128)
  ret %DecRatio %r
slow:
  %out = alloca %DecRatio, align 8
  call void @aod_rt_int_exact_div(ptr %ctx, i64 %a, i64 %b, ptr %out)
  %rs = load %DecRatio, ptr %out, align 8
  ret %DecRatio %rs
}

define internal %DecRatio @__aod_ratio_neg(ptr %ctx, %DecRatio %a) alwaysinline {
entry:
  %na = extractvalue %DecRatio %a, 0
  %da = extractvalue %DecRatio %a, 1
  %ia = icmp ult i64 %na, -4611686018427387904
  %id = icmp ult i64 %da, -4611686018427387904
  %dz = icmp ne i64 %da, 0
  %c1 = and i1 %ia, %id
  %ok = and i1 %c1, %dz
  br i1 %ok, label %fast, label %slow
fast:
  %nneg = call i64 @__aod_swar_neg(i64 %na)
  %p0 = insertvalue %DecRatio undef, i64 %nneg, 0
  %p1 = insertvalue %DecRatio %p0, i64 %da, 1
  ret %DecRatio %p1
slow:
  %out = alloca %DecRatio, align 8
  call void @aod_rt_ratio_neg(ptr %ctx, i64 %na, i64 %da, ptr %out)
  %rs = load %DecRatio, ptr %out, align 8
  ret %DecRatio %rs
}

define internal %DecRatio @__aod_ratio_add(ptr %ctx, %DecRatio %a, %DecRatio %b) alwaysinline {
entry:
  %na = extractvalue %DecRatio %a, 0
  %da = extractvalue %DecRatio %a, 1
  %nb = extractvalue %DecRatio %b, 0
  %db = extractvalue %DecRatio %b, 1
  %c0 = icmp ult i64 %na, -4611686018427387904
  %c1 = icmp ult i64 %da, -4611686018427387904
  %c2 = icmp ult i64 %nb, -4611686018427387904
  %c3 = icmp ult i64 %db, -4611686018427387904
  %d0 = icmp ne i64 %da, 0
  %d1 = icmp ne i64 %db, 0
  %m0 = and i1 %c0, %c1
  %m1 = and i1 %c2, %c3
  %m2 = and i1 %d0, %d1
  %m3 = and i1 %m0, %m1
  %ok = and i1 %m3, %m2
  br i1 %ok, label %fast, label %slow
fast:
  %na_v = call i64 @__aod_swar_unpack_unchecked(i64 %na)
  %da_v = call i64 @__aod_swar_unpack_unchecked(i64 %da)
  %nb_v = call i64 @__aod_swar_unpack_unchecked(i64 %nb)
  %db_v = call i64 @__aod_swar_unpack_unchecked(i64 %db)
  %na128 = sext i64 %na_v to i128
  %da128 = sext i64 %da_v to i128
  %nb128 = sext i64 %nb_v to i128
  %db128 = sext i64 %db_v to i128
  %t1 = mul nsw i128 %na128, %db128
  %t2 = mul nsw i128 %nb128, %da128
  %num = add nsw i128 %t1, %t2
  %den = mul nsw i128 %da128, %db128
  %r = call %DecRatio @__aod_ratio_norm_i128(ptr %ctx, i128 %num, i128 %den)
  ret %DecRatio %r
slow:
  %out = alloca %DecRatio, align 8
  call void @aod_rt_ratio_add(ptr %ctx, i64 %na, i64 %da, i64 %nb, i64 %db, ptr %out)
  %rs = load %DecRatio, ptr %out, align 8
  ret %DecRatio %rs
}

define internal %DecRatio @__aod_ratio_sub(ptr %ctx, %DecRatio %a, %DecRatio %b) alwaysinline {
entry:
  %na = extractvalue %DecRatio %a, 0
  %da = extractvalue %DecRatio %a, 1
  %nb = extractvalue %DecRatio %b, 0
  %db = extractvalue %DecRatio %b, 1
  %c0 = icmp ult i64 %na, -4611686018427387904
  %c1 = icmp ult i64 %da, -4611686018427387904
  %c2 = icmp ult i64 %nb, -4611686018427387904
  %c3 = icmp ult i64 %db, -4611686018427387904
  %d0 = icmp ne i64 %da, 0
  %d1 = icmp ne i64 %db, 0
  %m0 = and i1 %c0, %c1
  %m1 = and i1 %c2, %c3
  %m2 = and i1 %d0, %d1
  %m3 = and i1 %m0, %m1
  %ok = and i1 %m3, %m2
  br i1 %ok, label %fast, label %slow
fast:
  %na_v = call i64 @__aod_swar_unpack_unchecked(i64 %na)
  %da_v = call i64 @__aod_swar_unpack_unchecked(i64 %da)
  %nb_v = call i64 @__aod_swar_unpack_unchecked(i64 %nb)
  %db_v = call i64 @__aod_swar_unpack_unchecked(i64 %db)
  %na128 = sext i64 %na_v to i128
  %da128 = sext i64 %da_v to i128
  %nb128 = sext i64 %nb_v to i128
  %db128 = sext i64 %db_v to i128
  %t1 = mul nsw i128 %na128, %db128
  %t2 = mul nsw i128 %nb128, %da128
  %num = sub nsw i128 %t1, %t2
  %den = mul nsw i128 %da128, %db128
  %r = call %DecRatio @__aod_ratio_norm_i128(ptr %ctx, i128 %num, i128 %den)
  ret %DecRatio %r
slow:
  %out = alloca %DecRatio, align 8
  call void @aod_rt_ratio_sub(ptr %ctx, i64 %na, i64 %da, i64 %nb, i64 %db, ptr %out)
  %rs = load %DecRatio, ptr %out, align 8
  ret %DecRatio %rs
}

define internal %DecRatio @__aod_ratio_mul(ptr %ctx, %DecRatio %a, %DecRatio %b) alwaysinline {
entry:
  %na = extractvalue %DecRatio %a, 0
  %da = extractvalue %DecRatio %a, 1
  %nb = extractvalue %DecRatio %b, 0
  %db = extractvalue %DecRatio %b, 1
  %c0 = icmp ult i64 %na, -4611686018427387904
  %c1 = icmp ult i64 %da, -4611686018427387904
  %c2 = icmp ult i64 %nb, -4611686018427387904
  %c3 = icmp ult i64 %db, -4611686018427387904
  %d0 = icmp ne i64 %da, 0
  %d1 = icmp ne i64 %db, 0
  %m0 = and i1 %c0, %c1
  %m1 = and i1 %c2, %c3
  %m2 = and i1 %d0, %d1
  %m3 = and i1 %m0, %m1
  %ok = and i1 %m3, %m2
  br i1 %ok, label %fast, label %slow
fast:
  %na_v = call i64 @__aod_swar_unpack_unchecked(i64 %na)
  %da_v = call i64 @__aod_swar_unpack_unchecked(i64 %da)
  %nb_v = call i64 @__aod_swar_unpack_unchecked(i64 %nb)
  %db_v = call i64 @__aod_swar_unpack_unchecked(i64 %db)
  %na128 = sext i64 %na_v to i128
  %da128 = sext i64 %da_v to i128
  %nb128 = sext i64 %nb_v to i128
  %db128 = sext i64 %db_v to i128
  %num = mul nsw i128 %na128, %nb128
  %den = mul nsw i128 %da128, %db128
  %r = call %DecRatio @__aod_ratio_norm_i128(ptr %ctx, i128 %num, i128 %den)
  ret %DecRatio %r
slow:
  %out = alloca %DecRatio, align 8
  call void @aod_rt_ratio_mul(ptr %ctx, i64 %na, i64 %da, i64 %nb, i64 %db, ptr %out)
  %rs = load %DecRatio, ptr %out, align 8
  ret %DecRatio %rs
}

define internal %DecRatio @__aod_ratio_div(ptr %ctx, %DecRatio %a, %DecRatio %b) alwaysinline {
entry:
  %na = extractvalue %DecRatio %a, 0
  %da = extractvalue %DecRatio %a, 1
  %nb = extractvalue %DecRatio %b, 0
  %db = extractvalue %DecRatio %b, 1
  %c0 = icmp ult i64 %na, -4611686018427387904
  %c1 = icmp ult i64 %da, -4611686018427387904
  %c2 = icmp ult i64 %nb, -4611686018427387904
  %c3 = icmp ult i64 %db, -4611686018427387904
  %d0 = icmp ne i64 %da, 0
  %d1 = icmp ne i64 %db, 0
  %nz = icmp ne i64 %nb, 0
  %m0 = and i1 %c0, %c1
  %m1 = and i1 %c2, %c3
  %m2 = and i1 %d0, %d1
  %m3 = and i1 %m0, %m1
  %m4 = and i1 %m3, %m2
  %ok = and i1 %m4, %nz
  br i1 %ok, label %fast, label %slow
fast:
  %na_v = call i64 @__aod_swar_unpack_unchecked(i64 %na)
  %da_v = call i64 @__aod_swar_unpack_unchecked(i64 %da)
  %nb_v = call i64 @__aod_swar_unpack_unchecked(i64 %nb)
  %db_v = call i64 @__aod_swar_unpack_unchecked(i64 %db)
  %na128 = sext i64 %na_v to i128
  %da128 = sext i64 %da_v to i128
  %nb128 = sext i64 %nb_v to i128
  %db128 = sext i64 %db_v to i128
  %num = mul nsw i128 %na128, %db128
  %den = mul nsw i128 %da128, %nb128
  %r = call %DecRatio @__aod_ratio_norm_i128(ptr %ctx, i128 %num, i128 %den)
  ret %DecRatio %r
slow:
  %out = alloca %DecRatio, align 8
  call void @aod_rt_ratio_div(ptr %ctx, i64 %na, i64 %da, i64 %nb, i64 %db, ptr %out)
  %rs = load %DecRatio, ptr %out, align 8
  ret %DecRatio %rs
}

define internal i8 @__aod_ratio_cmp(ptr %ctx, %DecRatio %a, %DecRatio %b) alwaysinline {
entry:
  %na = extractvalue %DecRatio %a, 0
  %da = extractvalue %DecRatio %a, 1
  %nb = extractvalue %DecRatio %b, 0
  %db = extractvalue %DecRatio %b, 1
  %c0 = icmp ult i64 %na, -4611686018427387904
  %c1 = icmp ult i64 %da, -4611686018427387904
  %c2 = icmp ult i64 %nb, -4611686018427387904
  %c3 = icmp ult i64 %db, -4611686018427387904
  %d0 = icmp ne i64 %da, 0
  %d1 = icmp ne i64 %db, 0
  %m0 = and i1 %c0, %c1
  %m1 = and i1 %c2, %c3
  %m2 = and i1 %d0, %d1
  %m3 = and i1 %m0, %m1
  %ok = and i1 %m3, %m2
  br i1 %ok, label %fast, label %slow
fast:
  %na_v = call i64 @__aod_swar_unpack_unchecked(i64 %na)
  %da_v = call i64 @__aod_swar_unpack_unchecked(i64 %da)
  %nb_v = call i64 @__aod_swar_unpack_unchecked(i64 %nb)
  %db_v = call i64 @__aod_swar_unpack_unchecked(i64 %db)
  %na128 = sext i64 %na_v to i128
  %da128 = sext i64 %da_v to i128
  %nb128 = sext i64 %nb_v to i128
  %db128 = sext i64 %db_v to i128
  %lhs = mul nsw i128 %na128, %db128
  %rhs = mul nsw i128 %nb128, %da128
  %lt = icmp slt i128 %lhs, %rhs
  %gt = icmp sgt i128 %lhs, %rhs
  %lt8 = zext i1 %lt to i8
  %gt8 = zext i1 %gt to i8
  %cmp = sub i8 %gt8, %lt8
  ret i8 %cmp
slow:
  %rs = call i8 @aod_rt_ratio_cmp(ptr %ctx, i64 %na, i64 %da, i64 %nb, i64 %db)
  ret i8 %rs
}

define internal i8 @__aod_t1_add(ptr %ctx, i8 %a, i8 %b) alwaysinline {
entry:
  %s = add i8 %a, %b
  %ge = icmp sge i8 %s, -1
  %le = icmp sle i8 %s, 1
  %ok = and i1 %ge, %le
  br i1 %ok, label %done, label %ovf
ovf:
  call void @aod_rt_fail(ptr %ctx, ptr @.t1_ovf_msg, i64 20)
  ret i8 0
done:
  ret i8 %s
}

define internal i8 @__aod_t1_sub(ptr %ctx, i8 %a, i8 %b) alwaysinline {
entry:
  %s = sub i8 %a, %b
  %ge = icmp sge i8 %s, -1
  %le = icmp sle i8 %s, 1
  %ok = and i1 %ge, %le
  br i1 %ok, label %done, label %ovf
ovf:
  call void @aod_rt_fail(ptr %ctx, ptr @.t1_ovf_msg, i64 20)
  ret i8 0
done:
  ret i8 %s
}

define internal i8 @__aod_t1_mul(ptr %ctx, i8 %a, i8 %b) alwaysinline {
entry:
  %p = mul nsw i8 %a, %b
  ret i8 %p
}
"#;
