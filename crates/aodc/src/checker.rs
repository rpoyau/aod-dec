use std::collections::{BTreeMap, BTreeSet};

use aod_core::ternary::T1;

use crate::ast::{
    AdverbOp, BinaryOp, ChamberDecl, ConstDecl, DecArm, DecArmParam, DecFnDecl, EnumDecl, Expr,
    ExprKind, Item, LetPattern, MatchExprArm, ObserveDecl, Pattern, PipeStage, PrepareDecl,
    ProbeDecl, Program, Stmt, StmtKind, StructDecl, TypeExpr, UnaryOp,
};
use crate::diagnostics::{DecResult, Diagnostic, DiagnosticCode};
use crate::modules::{
    builtin_namespace_items, canonical_builtin_name, ImportedSymbol, LoadedUnit,
};
use crate::types::{
    builtin_enum_variants, builtin_ns_exports_enum, is_reserved_builtin_name, lookup_builtin,
    Effect, Type, BUILTIN_MOD, MODEL_STATUS_VARIANTS, RECIPIENT_STAGE_VARIANTS,
};

#[derive(Clone, Debug)]
struct VarBinding {
    ty: Type,
}

#[derive(Clone, Debug)]
struct FnEntry {
    module_key: String,
    decl: DecFnDecl,
    is_qext: bool,
}

#[derive(Clone, Debug)]
struct ConstEntry {
    ty: Type,
    is_qext: bool,
}

#[derive(Clone, Debug)]
struct StructDef {
    module: String,
    name: String,
    fields: Vec<(String, Type)>,
}

#[derive(Clone, Debug)]
struct EnumDef {
    module: String,
    name: String,
    variants: Vec<(String, Vec<Type>)>,
}

#[derive(Clone, Debug)]
struct CheckContext {
    module_key: String,
    effect: Effect,
    in_main: bool,
    in_const: bool,
    in_prepare: bool,
    in_qext_sink: bool,
    expected_return: Type,
}

/// Verified compilation unit ready for IR serialization, LLVM lowering, and exact evaluation.
#[derive(Clone, Debug)]
pub struct CheckedUnit {
    pub unit: LoadedUnit,
    pub main_return_type: Option<Type>,
    pub test_functions: Vec<String>,
    /// Single source of truth for exact verified types of all module & `quarantine Q_ext` `const`s.
    pub const_types: BTreeMap<(String, String), Type>,
    /// Exact resolved types for polymorphic `sum`/`dot`/`any`/`all` and custom `dec Struct:` overloads.
    pub poly_call_types: BTreeMap<(String, usize, String), Type>,
}

/// Exhaustively typecheck every module, struct, enum, function, signature arm, apparatus block,
/// rank-lifted expression, pipeline, and quarantine module in `unit`.
pub fn check_unit(unit: LoadedUnit, require_main: bool) -> DecResult<CheckedUnit> {
    let mut checker = Checker {
        unit: &unit,
        struct_names: BTreeSet::new(),
        enum_names: BTreeSet::new(),
        structs: BTreeMap::new(),
        enums: BTreeMap::new(),
        functions: BTreeMap::new(),
        consts: BTreeMap::new(),
        chambers: BTreeMap::new(),
        prepares: BTreeMap::new(),
        observes: BTreeMap::new(),
        qext_modules: BTreeSet::new(),
        checked_transitive: BTreeSet::new(),
        poly_call_types: BTreeMap::new(),
    };

    checker.register_builtin_enums();
    checker.collect_type_names()?;
    checker.collect_all_declarations()?;
    checker.check_all_declarations()?;

    let mut test_functions = Vec::new();
    let mut main_ret: Option<Type> = None;

    for item in &unit.entry_program.items {
        if let Item::DecFn(f) = item {
            if f.name == "main" {
                let zero_arg: Vec<&DecArm> = f.arms.iter().filter(|a| a.params.is_empty()).collect();
                if zero_arg.len() != 1 || f.arms.len() != 1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        f.line,
                        "core program requires a single zero-argument main() signature",
                    ));
                }
                main_ret = Some(checker.resolve_type(
                    "<entry>",
                    &zero_arg[0].return_type,
                    zero_arg[0].line,
                )?);
            } else if f.name.starts_with("test_") && f.arms.iter().any(|a| a.params.is_empty()) {
                test_functions.push(f.name.clone());
            }
        }
    }

    if require_main && main_ret.is_none() {
        return Err(Diagnostic::new(
            DiagnosticCode::TypeError,
            "core program requires a main() declaration",
        ));
    }

    let const_types: BTreeMap<(String, String), Type> = checker
        .consts
        .into_iter()
        .map(|(k, entry)| (k, entry.ty))
        .collect();
    let poly_call_types = checker.poly_call_types;

    Ok(CheckedUnit {
        unit,
        main_return_type: main_ret,
        test_functions,
        const_types,
        poly_call_types,
    })
}

struct Checker<'a> {
    unit: &'a LoadedUnit,
    struct_names: BTreeSet<(String, String)>,
    enum_names: BTreeSet<(String, String)>,
    structs: BTreeMap<(String, String), StructDef>,
    enums: BTreeMap<(String, String), EnumDef>,
    functions: BTreeMap<(String, String), FnEntry>,
    consts: BTreeMap<(String, String), ConstEntry>,
    chambers: BTreeMap<(String, String), ChamberDecl>,
    prepares: BTreeMap<(String, String), PrepareDecl>,
    observes: BTreeMap<(String, String), ObserveDecl>,
    qext_modules: BTreeSet<(String, String)>,
    checked_transitive: BTreeSet<(String, String, Vec<Type>, Effect)>,
    poly_call_types: BTreeMap<(String, usize, String), Type>,
}

enum CallTarget {
    Builtin(String),
    UserFn(FnEntry),
}

impl<'a> Checker<'a> {
    fn register_builtin_enums(&mut self) {
        for (ename, variants) in [
            ("ModelStatus", MODEL_STATUS_VARIANTS),
            ("RecipientStage", RECIPIENT_STAGE_VARIANTS),
        ] {
            let key = (BUILTIN_MOD.to_string(), ename.to_string());
            self.enum_names.insert(key.clone());
            self.enums.insert(
                key,
                EnumDef {
                    module: BUILTIN_MOD.to_string(),
                    name: ename.to_string(),
                    variants: variants
                        .iter()
                        .map(|(vn, _)| ((*vn).to_string(), Vec::new()))
                        .collect(),
                },
            );
        }
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

    fn imports_for(&self, module_key: &str) -> &BTreeMap<String, ImportedSymbol> {
        let base = self.base_module_of(module_key);
        if base == "<entry>" {
            &self.unit.entry_imports
        } else {
            self.unit
                .module_imports
                .get(base)
                .unwrap_or(&self.unit.entry_imports)
        }
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
                    if self
                        .struct_names
                        .contains(&(m.clone(), segs[1].to_string()))
                    {
                        return Some(Type::Struct {
                            module: m.clone(),
                            name: segs[1].to_string(),
                        });
                    }
                    if self.enum_names.contains(&(m.clone(), segs[1].to_string())) {
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
            .struct_names
            .contains(&(base.to_string(), name.to_string()))
        {
            return Some(Type::Struct {
                module: base.to_string(),
                name: name.to_string(),
            });
        }
        if self
            .enum_names
            .contains(&(base.to_string(), name.to_string()))
        {
            return Some(Type::Enum {
                module: base.to_string(),
                name: name.to_string(),
            });
        }
        match self.imports_for(mod_key).get(name) {
            Some(ImportedSymbol::ModuleItem { module, item }) => {
                if self
                    .struct_names
                    .contains(&(module.clone(), item.clone()))
                {
                    return Some(Type::Struct {
                        module: module.clone(),
                        name: item.clone(),
                    });
                }
                if self.enum_names.contains(&(module.clone(), item.clone())) {
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
        Type::resolve_with(expr, line, &|name| self.lookup_user_type(mod_key, name))
    }

    fn collect_type_names(&mut self) -> DecResult<()> {
        self.collect_program_type_names("<entry>", &self.unit.entry_program)?;
        for (mod_key, prog) in &self.unit.parsed_modules {
            self.collect_program_type_names(mod_key, prog)?;
        }
        Ok(())
    }

    fn collect_program_type_names(&mut self, mod_key: &str, prog: &Program) -> DecResult<()> {
        for item in &prog.items {
            match item {
                Item::Struct(s) => {
                    let key = (mod_key.to_string(), s.name.clone());
                    if !self.struct_names.insert(key.clone()) || self.enum_names.contains(&key) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            s.line,
                            format!("duplicate type declaration {}", s.name),
                        ));
                    }
                }
                Item::Enum(e) => {
                    let key = (mod_key.to_string(), e.name.clone());
                    if !self.enum_names.insert(key.clone()) || self.struct_names.contains(&key) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            e.line,
                            format!("duplicate type declaration {}", e.name),
                        ));
                    }
                }
                Item::QuarantineMod(qm) => {
                    self.qext_modules
                        .insert((mod_key.to_string(), qm.name.clone()));
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn collect_all_declarations(&mut self) -> DecResult<()> {
        self.collect_program_types("<entry>", &self.unit.entry_program)?;
        for (mod_key, prog) in &self.unit.parsed_modules {
            self.collect_program_types(mod_key, prog)?;
        }

        self.collect_program_items("<entry>", &self.unit.entry_program)?;
        for (mod_key, prog) in &self.unit.parsed_modules {
            self.collect_program_items(mod_key, prog)?;
        }

        for mod_key in &self.unit.module_load_order {
            if let Some(prog) = self.unit.parsed_modules.get(mod_key) {
                self.collect_program_consts(mod_key, prog)?;
            }
        }
        self.collect_program_consts("<entry>", &self.unit.entry_program)?;
        Ok(())
    }

    fn collect_program_types(&mut self, mod_key: &str, prog: &Program) -> DecResult<()> {
        for item in &prog.items {
            match item {
                Item::Struct(s) => self.register_struct(mod_key, s)?,
                Item::Enum(e) => self.register_enum(mod_key, e)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn register_struct(&mut self, mod_key: &str, s: &StructDecl) -> DecResult<()> {
        let mut seen = BTreeSet::new();
        let mut fields = Vec::with_capacity(s.fields.len());
        for (fname, fty_expr) in &s.fields {
            if !seen.insert(fname.as_str()) {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    s.line,
                    format!("duplicate field {fname} in struct {}", s.name),
                ));
            }
            let fty = self.resolve_type(mod_key, fty_expr, s.line)?;
            if fty.is_qext() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::QExtLeakageForbidden,
                    s.line,
                    format!(
                        "native struct {} cannot declare QExt field {fname}: {fty}",
                        s.name
                    ),
                ));
            }
            fields.push((fname.clone(), fty));
        }
        self.structs.insert(
            (mod_key.to_string(), s.name.clone()),
            StructDef {
                module: mod_key.to_string(),
                name: s.name.clone(),
                fields,
            },
        );
        Ok(())
    }

    fn register_enum(&mut self, mod_key: &str, e: &EnumDecl) -> DecResult<()> {
        let mut seen = BTreeSet::new();
        let mut variants = Vec::with_capacity(e.variants.len());
        for v in &e.variants {
            if !seen.insert(v.name.as_str()) {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    v.line,
                    format!("duplicate variant {} in enum {}", v.name, e.name),
                ));
            }
            let mut payloads = Vec::with_capacity(v.payloads.len());
            for p_expr in &v.payloads {
                let pty = self.resolve_type(mod_key, p_expr, v.line)?;
                if pty.is_qext() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        v.line,
                        format!(
                            "native enum {} variant {} cannot hold QExt payload {pty}",
                            e.name, v.name
                        ),
                    ));
                }
                payloads.push(pty);
            }
            variants.push((v.name.clone(), payloads));
        }
        self.enums.insert(
            (mod_key.to_string(), e.name.clone()),
            EnumDef {
                module: mod_key.to_string(),
                name: e.name.clone(),
                variants,
            },
        );
        Ok(())
    }

    fn collect_program_items(&mut self, mod_key: &str, prog: &Program) -> DecResult<()> {
        for item in &prog.items {
            match item {
                Item::Use(_) | Item::Struct(_) | Item::Enum(_) | Item::Const(_) => {}
                Item::DecFn(f) => {
                    self.register_fn(mod_key, f, false)?;
                }
                Item::Chamber(ch) => {
                    let key = (mod_key.to_string(), ch.name.clone());
                    if self.chambers.contains_key(&key) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            ch.line,
                            format!("duplicate chamber declaration {}", ch.name),
                        ));
                    }
                    self.chambers.insert(key, ch.clone());
                }
                Item::Prepare(p) => {
                    let key = (mod_key.to_string(), p.name.clone());
                    if self.prepares.contains_key(&key) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            p.line,
                            format!("duplicate prepare declaration {}", p.name),
                        ));
                    }
                    self.prepares.insert(key, p.clone());
                }
                Item::Observe(o) => {
                    let key = (mod_key.to_string(), o.name.clone());
                    if self.observes.contains_key(&key) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            o.line,
                            format!("duplicate observe declaration {}", o.name),
                        ));
                    }
                    self.observes.insert(key, o.clone());
                }
                Item::QuarantineMod(qm) => {
                    let sub_key = format!("{mod_key}::{}", qm.name);
                    for sub_item in &qm.items {
                        match sub_item {
                            Item::DecFn(f) => {
                                self.register_fn(&sub_key, f, true)?;
                            }
                            Item::Const(_) => {}
                            _ => {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::QExtLeakageForbidden,
                                    qm.line,
                                    "only const and dec declarations are permitted in quarantine Q_ext modules",
                                ));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn collect_program_consts(&mut self, mod_key: &str, prog: &Program) -> DecResult<()> {
        for item in &prog.items {
            match item {
                Item::Const(c) => self.register_const(mod_key, c, false)?,
                Item::QuarantineMod(qm) => {
                    let sub_key = format!("{mod_key}::{}", qm.name);
                    for sub_item in &qm.items {
                        if let Item::Const(c) = sub_item {
                            self.register_const(&sub_key, c, true)?;
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn register_fn(&mut self, mod_key: &str, f: &DecFnDecl, is_qext: bool) -> DecResult<()> {
        if is_reserved_builtin_name(&f.name) {
            return Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                f.line,
                format!("cannot redefine core builtin {}", f.name),
            ));
        }
        let key = (mod_key.to_string(), f.name.clone());
        if self.functions.contains_key(&key) {
            return Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                f.line,
                format!("put all signature arms in one `dec {}:` block", f.name),
            ));
        }

        let mut seen_plain_sigs: BTreeSet<Vec<Type>> = BTreeSet::new();
        if !is_qext {
            if let Some(sdef) = self.structs.get(&(mod_key.to_string(), f.name.clone())) {
                let canonical_sig: Vec<Type> =
                    sdef.fields.iter().map(|(_, t)| t.clone()).collect();
                seen_plain_sigs.insert(canonical_sig);
            }
        }
        for arm in &f.arms {
            let mut all_typed_vars = arm.guard.is_none();
            let mut plain_sig = Vec::with_capacity(arm.params.len());
            let mut param_names = BTreeSet::new();

            for p in &arm.params {
                match p {
                    DecArmParam::TypedVar { name, ty } => {
                        if !param_names.insert(name.as_str()) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                arm.line,
                                format!("duplicate parameter {name} in arm of {}", f.name),
                            ));
                        }
                        let pty = self.resolve_type(mod_key, ty, arm.line)?;
                        if !is_qext && pty.is_qext() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::QExtLeakageForbidden,
                                arm.line,
                                format!(
                                    "native function {} cannot accept QExt parameter {name}: {pty}",
                                    f.name
                                ),
                            ));
                        }
                        plain_sig.push(pty);
                    }
                    DecArmParam::TypedPattern { ty, .. } => {
                        all_typed_vars = false;
                        let pty = self.resolve_type(mod_key, ty, arm.line)?;
                        if !is_qext && pty.is_qext() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::QExtLeakageForbidden,
                                arm.line,
                                format!(
                                    "native function {} cannot accept QExt pattern parameter: {pty}",
                                    f.name
                                ),
                            ));
                        }
                    }
                    DecArmParam::Pattern(_) => {
                        all_typed_vars = false;
                    }
                }
            }

            let ret_ty = self.resolve_type(mod_key, &arm.return_type, arm.line)?;
            if !is_qext && ret_ty.is_qext() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::QExtLeakageForbidden,
                    arm.line,
                    format!("native function {} cannot return QExt type {ret_ty}", f.name),
                ));
            }
            if is_qext && ret_ty != Type::Unit && !ret_ty.is_qext() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::QExtLeakageForbidden,
                    arm.line,
                    format!(
                        "quarantine Q_ext function {} must return QExt<T> or (), got {ret_ty}",
                        f.name
                    ),
                ));
            }
            if all_typed_vars && !seen_plain_sigs.insert(plain_sig) {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    arm.line,
                    "duplicate input signature; return type cannot resolve dispatch",
                ));
            }
        }

        self.functions.insert(
            key,
            FnEntry {
                module_key: mod_key.to_string(),
                decl: f.clone(),
                is_qext,
            },
        );
        Ok(())
    }

    fn register_const(&mut self, mod_key: &str, c: &ConstDecl, is_qext: bool) -> DecResult<()> {
        let key = (mod_key.to_string(), c.name.clone());
        if self.consts.contains_key(&key) {
            return Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                c.line,
                format!("duplicate const {}", c.name),
            ));
        }
        let ctx = CheckContext {
            module_key: mod_key.to_string(),
            effect: if is_qext {
                Effect::QExt
            } else {
                Effect::PureNative
            },
            in_main: false,
            in_const: true,
            in_prepare: false,
            in_qext_sink: false,
            expected_return: Type::Unit,
        };
        let scopes: Vec<BTreeMap<String, VarBinding>> = Vec::new();
        let val_ty = self.check_expr(&c.value, &scopes, &ctx)?;
        let final_ty = if let Some(ann) = &c.ty {
            let expected = self.resolve_type(mod_key, ann, c.line)?;
            if !is_qext && expected.is_qext() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::QExtLeakageForbidden,
                    c.line,
                    format!("QExt type forbidden on native const {}", c.name),
                ));
            }
            enforce_annotation_compatibility(&val_ty, &expected, c.line, &format!("const {}", c.name))?;
            expected
        } else {
            val_ty
        };
        if is_qext && !final_ty.is_qext() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::QExtLeakageForbidden,
                c.line,
                format!(
                    "const {} inside quarantine Q_ext must have QExt<T> type, got {final_ty}",
                    c.name
                ),
            ));
        }
        if !is_qext && final_ty.is_qext() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::QExtLeakageForbidden,
                c.line,
                format!("QExt value cannot bind to native const {}", c.name),
            ));
        }
        self.consts.insert(
            key,
            ConstEntry {
                ty: final_ty,
                is_qext,
            },
        );
        Ok(())
    }

    fn check_all_declarations(&mut self) -> DecResult<()> {
        let fn_keys: Vec<(String, String)> = self.functions.keys().cloned().collect();
        for key in fn_keys {
            let entry = self.functions.get(&key).cloned().unwrap();
            let eff = if entry.is_qext {
                Effect::QExt
            } else {
                Effect::PureNative
            };
            self.check_fn_entry(&entry, None, eff)?;
        }

        let chambers: Vec<(String, ChamberDecl)> = self
            .chambers
            .iter()
            .map(|((m, _), c)| (m.clone(), c.clone()))
            .collect();
        for (mod_key, ch) in chambers {
            let ctx = CheckContext {
                module_key: mod_key,
                effect: Effect::PureNative,
                in_main: false,
                in_const: false,
                in_prepare: true,
                in_qext_sink: false,
                expected_return: Type::Unit,
            };
            let scopes = Vec::new();
            let space_ty = self.check_expr(&ch.space, &scopes, &ctx)?;
            if space_ty != Type::Box3 {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    ch.line,
                    format!(
                        "chamber {} space must be box([...]), got {space_ty}",
                        ch.name
                    ),
                ));
            }
            if let Some(ref bs) = ch.boundary_step {
                let bs_ty = self.check_expr(bs, &scopes, &ctx)?;
                let expect_tup = Type::Tuple(vec![Type::Int, Type::Int, Type::Int]);
                if bs_ty != expect_tup {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        bs.line,
                        format!(
                            "chamber {} boundary_step must be (int, int, int), got {bs_ty}",
                            ch.name
                        ),
                    ));
                }
            }
            if let Some(ref ttl) = ch.contact_ttl {
                let ttl_ty = self.check_expr(ttl, &scopes, &ctx)?;
                if ttl_ty != Type::Int {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        ttl.line,
                        format!(
                            "chamber {} contact_ttl must be int, got {ttl_ty}",
                            ch.name
                        ),
                    ));
                }
            }
            if let Some(ref fus) = ch.fuse_on_resonance {
                let fus_ty = self.check_expr(fus, &scopes, &ctx)?;
                if fus_ty != Type::Bool {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        fus.line,
                        format!(
                            "chamber {} fuse_on_resonance must be bool, got {fus_ty}",
                            ch.name
                        ),
                    ));
                }
            }
        }

        let prepares: Vec<(String, PrepareDecl)> = self
            .prepares
            .iter()
            .map(|((m, _), p)| (m.clone(), p.clone()))
            .collect();
        for (mod_key, prep) in prepares {
            if !self
                .chambers
                .contains_key(&(mod_key.clone(), prep.chamber.clone()))
            {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    prep.line,
                    format!(
                        "prepare {} references undefined chamber {}",
                        prep.name, prep.chamber
                    ),
                ));
            }
            let mut ctx = CheckContext {
                module_key: mod_key,
                effect: Effect::PureNative,
                in_main: false,
                in_const: false,
                in_prepare: true,
                in_qext_sink: false,
                expected_return: Type::Unit,
            };
            let mut scopes: Vec<BTreeMap<String, VarBinding>> = vec![BTreeMap::new()];
            for stmt in &prep.lets {
                self.check_stmt(stmt, &mut scopes, &mut ctx)?;
            }
            for clk in &prep.clocks {
                let ty = self.check_expr(&clk.period, &scopes, &ctx)?;
                if ty != Type::Int {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        clk.line,
                        format!("clock {} period must be int, got {ty}", clk.name),
                    ));
                }
            }
            for inj in &prep.injections {
                let s_ty = self.check_expr(&inj.support, &scopes, &ctx)?;
                if s_ty != Type::Support {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        inj.line,
                        format!("inject support must have type support, got {s_ty}"),
                    ));
                }
                let p_ty = self.check_expr(&inj.pos, &scopes, &ctx)?;
                if p_ty != Type::Tuple(vec![Type::Int, Type::Int, Type::Int]) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        inj.line,
                        format!("inject position must be (int, int, int), got {p_ty}"),
                    ));
                }
                let o_ty = self.check_expr(&inj.orient, &scopes, &ctx)?;
                if o_ty != Type::Int && o_ty != Type::T1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        inj.line,
                        format!("inject orient must be int or t1, got {o_ty}"),
                    ));
                }
                let sh_ty = self.check_expr(&inj.shells, &scopes, &ctx)?;
                if sh_ty != Type::Int {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        inj.line,
                        format!("inject shells must be int, got {sh_ty}"),
                    ));
                }
            }
        }

        let observes: Vec<(String, ObserveDecl)> = self
            .observes
            .iter()
            .map(|((m, _), o)| (m.clone(), o.clone()))
            .collect();
        for (mod_key, obs) in observes {
            for probe in &obs.probes {
                match probe {
                    ProbeDecl::Declarative { region, line, .. } => {
                        let ctx = CheckContext {
                            module_key: mod_key.clone(),
                            effect: Effect::ReadOnlyObserver,
                            in_main: false,
                            in_const: false,
                            in_prepare: false,
                            in_qext_sink: false,
                            expected_return: Type::Unit,
                        };
                        let scopes = Vec::new();
                        let r_ty = self.check_expr(region, &scopes, &ctx)?;
                        if r_ty != Type::Box3 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                *line,
                                format!("observer region must be box([...]), got {r_ty}"),
                            ));
                        }
                    }
                    ProbeDecl::Procedural {
                        param_name,
                        param_ty,
                        return_type,
                        body,
                        line,
                        ..
                    } => {
                        if let Some(pte) = param_ty {
                            let pty = self.resolve_type(&mod_key, pte, *line)?;
                            if pty == Type::ChamberView {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::ObserverMutationForbidden,
                                    *line,
                                    "procedural observe parameter must be ReadOnly<ChamberView>, got mutable ChamberView",
                                ));
                            }
                            if pty != Type::ReadOnly(Box::new(Type::ChamberView)) {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    *line,
                                    format!(
                                        "procedural observe parameter must be ReadOnly<ChamberView>, got {pty}"
                                    ),
                                ));
                            }
                        }
                        let ret_ty = self.resolve_type(&mod_key, return_type, *line)?;
                        if ret_ty.is_qext() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::QExtLeakageForbidden,
                                *line,
                                "observer probe cannot return QExt<T>",
                            ));
                        }
                        let mut param_scope = BTreeMap::new();
                        param_scope.insert(
                            param_name.clone(),
                            VarBinding {
                                ty: Type::ReadOnly(Box::new(Type::ChamberView)),
                            },
                        );
                        let mut scopes = vec![param_scope, BTreeMap::new()];
                        let mut ctx = CheckContext {
                            module_key: mod_key.clone(),
                            effect: Effect::ReadOnlyObserver,
                            in_main: false,
                            in_const: false,
                            in_prepare: false,
                            in_qext_sink: false,
                            expected_return: ret_ty.clone(),
                        };
                        for stmt in body {
                            self.check_stmt(stmt, &mut scopes, &mut ctx)?;
                        }
                        if ret_ty != Type::Unit && !definitely_returns(body) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                *line,
                                format!("procedural observer probe must return {ret_ty}"),
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn slot_hints_for_arity(&self, entry: &FnEntry, arity: usize) -> DecResult<Vec<Option<Type>>> {
        let mut hints = vec![None; arity];
        // Pass 1: explicit TypedVar / TypedPattern and unambiguous non-IntLit patterns
        for arm in &entry.decl.arms {
            if arm.params.len() != arity {
                continue;
            }
            for (i, p) in arm.params.iter().enumerate() {
                if hints[i].is_some() {
                    continue;
                }
                match p {
                    DecArmParam::TypedVar { ty, .. } | DecArmParam::TypedPattern { ty, .. } => {
                        let resolved = self.resolve_type(&entry.module_key, ty, arm.line)?;
                        if resolved != Type::Unknown {
                            hints[i] = Some(resolved);
                        }
                    }
                    DecArmParam::Pattern(pat) if !matches!(pat, Pattern::IntLit(_)) => {
                        if let Some(t) = self.infer_pattern_type(&entry.module_key, pat, arm.line)?
                        {
                            if t != Type::Unknown {
                                hints[i] = Some(t);
                            }
                        }
                    }
                    DecArmParam::Pattern(_) => {}
                }
            }
        }
        // Pass 2: fill any remaining slots from bare IntLit patterns
        for arm in &entry.decl.arms {
            if arm.params.len() != arity {
                continue;
            }
            for (i, p) in arm.params.iter().enumerate() {
                if hints[i].is_none() {
                    if let DecArmParam::Pattern(Pattern::IntLit(_)) = p {
                        hints[i] = Some(Type::Int);
                    }
                }
            }
        }
        Ok(hints)
    }

    fn infer_arm_param_types_for_entry(
        &self,
        entry: &FnEntry,
        arm: &DecArm,
    ) -> DecResult<Vec<Type>> {
        let hints = self.slot_hints_for_arity(entry, arm.params.len())?;
        let mut out = Vec::with_capacity(arm.params.len());
        for (idx, p) in arm.params.iter().enumerate() {
            let inferred = match p {
                DecArmParam::TypedVar { ty, .. } | DecArmParam::TypedPattern { ty, .. } => {
                    self.resolve_type(&entry.module_key, ty, arm.line)?
                }
                DecArmParam::Pattern(Pattern::IntLit(n)) => {
                    if let Some(ref h) = hints[idx] {
                        if *h.unwrap_read_only() == Type::T1 {
                            T1::from_bigint(n).map_err(|e| {
                                Diagnostic::at_line(
                                    DiagnosticCode::InvalidTritLiteral,
                                    arm.line,
                                    e.to_string(),
                                )
                            })?;
                            Type::T1
                        } else {
                            Type::Int
                        }
                    } else {
                        Type::Int
                    }
                }
                DecArmParam::Pattern(pat) => self
                    .infer_pattern_type(&entry.module_key, pat, arm.line)?
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
        line: usize,
    ) -> DecResult<Option<Type>> {
        match pat {
            Pattern::Wildcard | Pattern::Binding(_) => Ok(None),
            Pattern::NullLit => Ok(Some(Type::Null)),
            Pattern::IntLit(_) => Ok(Some(Type::Int)),
            Pattern::BoolLit(_) => Ok(Some(Type::Bool)),
            Pattern::BytesLit(_) => Ok(Some(Type::Bytes)),
            Pattern::B8Lit(_) => Ok(Some(Type::B8)),
            Pattern::B64Lit(_) => Ok(Some(Type::B64)),
            Pattern::Q4Lit(_) => Ok(Some(Type::Q4)),
            Pattern::TritLit(_) => Ok(Some(Type::T1)),
            Pattern::RatioLit(_) => Ok(Some(Type::Ratio)),
            Pattern::Addr8Lit(_) => Ok(Some(Type::Addr8)),
            Pattern::SupportLit(_) => Ok(Some(Type::Support)),
            Pattern::EnumVariant { path, .. } => {
                let (edef, _) = self.resolve_enum_variant(mod_key, path, line)?;
                Ok(Some(Type::Enum {
                    module: edef.module.clone(),
                    name: edef.name.clone(),
                }))
            }
            Pattern::Tuple(ps) => {
                let mut tys = Vec::with_capacity(ps.len());
                for p in ps {
                    match self.infer_pattern_type(mod_key, p, line)? {
                        Some(t) => tys.push(t),
                        None => return Ok(None),
                    }
                }
                Ok(Some(Type::Tuple(tys)))
            }
        }
    }

    fn check_fn_entry(
        &mut self,
        entry: &FnEntry,
        override_arg_types: Option<&[Type]>,
        effect: Effect,
    ) -> DecResult<()> {
        let is_main = entry.module_key == "<entry>" && entry.decl.name == "main" && !entry.is_qext;
        let mut has_catch_all: BTreeSet<Vec<Type>> = BTreeSet::new();
        let mut enum_groups: BTreeMap<Vec<Type>, (usize, BTreeSet<String>)> = BTreeMap::new();

        for arm in &entry.decl.arms {
            let slot_hints = self.slot_hints_for_arity(entry, arm.params.len())?;
            let arm_tys = self.infer_arm_param_types_for_entry(entry, arm)?;
            if let Some(ov) = override_arg_types {
                if ov.len() != arm_tys.len()
                    || !ov.iter().zip(&arm_tys).all(|(a, p)| {
                        *p == Type::Unknown
                            || types_compatible_with_annotation(
                                a.unwrap_read_only(),
                                p.unwrap_read_only(),
                            )
                    })
                {
                    continue;
                }
            }

            let ret_ty = self.resolve_type(&entry.module_key, &arm.return_type, arm.line)?;
            let mut concrete_sig = Vec::with_capacity(arm.params.len());
            let mut scope = BTreeMap::new();
            for (idx, param) in arm.params.iter().enumerate() {
                let mut resolved_ty = if let Some(ov) = override_arg_types {
                    ov[idx].clone()
                } else if arm_tys[idx] != Type::Unknown {
                    arm_tys[idx].clone()
                } else {
                    slot_hints[idx].clone().unwrap_or(Type::Int)
                };
                if arm_tys[idx].is_read_only() {
                    resolved_ty = resolved_ty.wrap_read_only();
                }
                concrete_sig.push(resolved_ty.clone());
                match param {
                    DecArmParam::TypedVar { name, .. } => {
                        if scope.contains_key(name) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                arm.line,
                                format!("duplicate parameter {name} in arm of {}", entry.decl.name),
                            ));
                        }
                        scope.insert(name.clone(), VarBinding { ty: resolved_ty });
                    }
                    DecArmParam::TypedPattern { pattern, .. } | DecArmParam::Pattern(pattern) => {
                        self.check_pattern(
                            pattern,
                            &resolved_ty,
                            &entry.module_key,
                            &mut scope,
                            arm.line,
                        )?;
                    }
                }
            }

            if override_arg_types.is_none() {
                let all_irrefutable = arm.params.iter().all(|p| match p {
                    DecArmParam::TypedVar { .. } => true,
                    DecArmParam::TypedPattern { pattern, .. } | DecArmParam::Pattern(pattern) => {
                        is_irrefutable_pattern(pattern)
                    }
                });
                if arm.guard.is_none() && all_irrefutable {
                    has_catch_all.insert(concrete_sig.clone());
                }
                if concrete_sig.len() == 1
                    && matches!(concrete_sig[0].unwrap_read_only(), Type::Enum { .. })
                {
                    let grp = enum_groups
                        .entry(concrete_sig.clone())
                        .or_insert_with(|| (arm.line, BTreeSet::new()));
                    if arm.guard.is_none() {
                        match &arm.params[0] {
                            DecArmParam::TypedPattern {
                                pattern: Pattern::EnumVariant { path, bindings },
                                ..
                            }
                            | DecArmParam::Pattern(Pattern::EnumVariant { path, bindings })
                                if bindings.iter().all(is_irrefutable_pattern) =>
                            {
                                if let Some(vn) = path.last() {
                                    grp.1.insert(vn.clone());
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            let mut scopes = vec![scope, BTreeMap::new()];
            let mut ctx = CheckContext {
                module_key: entry.module_key.clone(),
                effect,
                in_main: is_main && arm.params.is_empty() && effect == Effect::PureNative,
                in_const: false,
                in_prepare: false,
                in_qext_sink: false,
                expected_return: ret_ty.clone(),
            };

            if let Some(ref guard) = arm.guard {
                let g_ty = self.check_expr(guard, &scopes, &ctx)?;
                if g_ty != Type::Bool {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        guard.line,
                        format!("dec arm 'where' guard must be bool, got {g_ty}"),
                    ));
                }
            }

            for stmt in &arm.body {
                self.check_stmt(stmt, &mut scopes, &mut ctx)?;
            }
            if ret_ty != Type::Unit && !definitely_returns(&arm.body) {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    arm.line,
                    format!(
                        "function {} arm missing return value of type {ret_ty}",
                        entry.decl.name
                    ),
                ));
            }
        }

        if override_arg_types.is_none() {
            for (sig, (first_line, covered)) in enum_groups {
                if !has_catch_all.contains(&sig) {
                    if let Type::Enum { module, name } = sig[0].unwrap_read_only() {
                        if let Some(edef) = self.enums.get(&(module.clone(), name.clone())) {
                            let missing: Vec<&str> = edef
                                .variants
                                .iter()
                                .map(|(vn, _)| vn.as_str())
                                .filter(|vn| !covered.contains(*vn))
                                .collect();
                            if !missing.is_empty() {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    first_line,
                                    format!(
                                        "non-exhaustive dec signature on enum {name}: missing variant(s) {}",
                                        missing.join(", ")
                                    ),
                                ));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn check_pattern(
        &self,
        pat: &Pattern,
        expected: &Type,
        mod_key: &str,
        scope: &mut BTreeMap<String, VarBinding>,
        line: usize,
    ) -> DecResult<()> {
        let target = expected.unwrap_read_only();
        match pat {
            Pattern::Wildcard => Ok(()),
            Pattern::Binding(name) => {
                if name != "_" {
                    if scope.contains_key(name) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("duplicate pattern binding {name}"),
                        ));
                    }
                    scope.insert(
                        name.clone(),
                        VarBinding {
                            ty: expected.clone(),
                        },
                    );
                }
                Ok(())
            }
            Pattern::NullLit => {
                if *target != Type::Null {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("null pattern does not match {target}"),
                    ));
                }
                Ok(())
            }
            Pattern::IntLit(n) => {
                if *target == Type::T1 {
                    T1::from_bigint(n).map_err(|e| {
                        Diagnostic::at_line(DiagnosticCode::InvalidTritLiteral, line, e.to_string())
                    })?;
                    return Ok(());
                }
                if *target != Type::Int {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("integer pattern does not match {target}"),
                    ));
                }
                Ok(())
            }
            Pattern::BoolLit(_) => {
                if *target != Type::Bool {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("bool pattern does not match {target}"),
                    ));
                }
                Ok(())
            }
            Pattern::BytesLit(_) => {
                if *target != Type::Bytes {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("bytes pattern does not match {target}"),
                    ));
                }
                Ok(())
            }
            Pattern::B8Lit(_) => {
                if *target != Type::B8 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("b8 pattern does not match {target}"),
                    ));
                }
                Ok(())
            }
            Pattern::B64Lit(_) => {
                if *target != Type::B64 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("b64 pattern does not match {target}"),
                    ));
                }
                Ok(())
            }
            Pattern::Q4Lit(_) => {
                if *target != Type::Q4 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("q4 pattern does not match {target}"),
                    ));
                }
                Ok(())
            }
            Pattern::TritLit(_) => {
                if *target != Type::T1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("trit pattern does not match {target}"),
                    ));
                }
                Ok(())
            }
            Pattern::RatioLit(_) => {
                if *target != Type::Ratio {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("ratio pattern does not match {target}"),
                    ));
                }
                Ok(())
            }
            Pattern::Addr8Lit(_) => {
                if *target != Type::Addr8 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("addr8 pattern does not match {target}"),
                    ));
                }
                Ok(())
            }
            Pattern::SupportLit(_) => {
                if *target != Type::Support {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("support pattern does not match {target}"),
                    ));
                }
                Ok(())
            }
            Pattern::EnumVariant { path, bindings } => {
                let (edef, payload_tys) = self.resolve_enum_variant(mod_key, path, line)?;
                let pat_enum_ty = Type::Enum {
                    module: edef.module.clone(),
                    name: edef.name.clone(),
                };
                if *target != pat_enum_ty {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("enum pattern {} does not match {target}", path.join("::")),
                    ));
                }
                if bindings.len() != payload_tys.len() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!(
                            "variant {} expects {} payload(s), got {}",
                            path.join("::"),
                            payload_tys.len(),
                            bindings.len()
                        ),
                    ));
                }
                for (bpat, pty) in bindings.iter().zip(payload_tys) {
                    let bound_ty = wrap_ro_if_needed(pty.clone(), expected.is_read_only());
                    self.check_pattern(bpat, &bound_ty, mod_key, scope, line)?;
                }
                Ok(())
            }
            Pattern::Tuple(pats) => match target {
                Type::Tuple(elem_tys) => {
                    if pats.len() != elem_tys.len() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!(
                                "tuple pattern arity mismatch: expected {}, got {}",
                                elem_tys.len(),
                                pats.len()
                            ),
                        ));
                    }
                    for (p, t) in pats.iter().zip(elem_tys) {
                        let bound_ty = wrap_ro_if_needed(t.clone(), expected.is_read_only());
                        self.check_pattern(p, &bound_ty, mod_key, scope, line)?;
                    }
                    Ok(())
                }
                _ => Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    line,
                    format!("tuple pattern does not match {target}"),
                )),
            },
        }
    }

    fn resolve_enum_variant(
        &self,
        mod_key: &str,
        path: &[String],
        line: usize,
    ) -> DecResult<(&EnumDef, &[Type])> {
        let (enum_mod, enum_name, var_name) = if path.len() == 2 {
            let base = self.base_module_of(mod_key);
            if self
                .enums
                .contains_key(&(base.to_string(), path[0].clone()))
            {
                (base.to_string(), path[0].clone(), path[1].clone())
            } else if let Some(imp) = self.imports_for(mod_key).get(&path[0]) {
                match imp {
                    ImportedSymbol::ModuleItem { module, item } => {
                        (module.clone(), item.clone(), path[1].clone())
                    }
                    ImportedSymbol::Builtin(canon) if builtin_enum_variants(canon).is_some() => {
                        (BUILTIN_MOD.to_string(), canon.clone(), path[1].clone())
                    }
                    _ => {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("unknown enum {}", path[0]),
                        ));
                    }
                }
            } else if !self
                .struct_names
                .contains(&(base.to_string(), path[0].clone()))
                && builtin_enum_variants(&path[0]).is_some()
            {
                (BUILTIN_MOD.to_string(), path[0].clone(), path[1].clone())
            } else {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    line,
                    format!("unknown enum {}", path[0]),
                ));
            }
        } else if path.len() == 3 {
            if let Some(ImportedSymbol::ModuleNamespace(m)) =
                self.imports_for(mod_key).get(&path[0])
            {
                if let Some(ns_tail) = m.strip_prefix("aod::") {
                    if builtin_ns_exports_enum(ns_tail, &path[1]) {
                        (BUILTIN_MOD.to_string(), path[1].clone(), path[2].clone())
                    } else {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("enum {} not exported by {m}", path[1]),
                        ));
                    }
                } else {
                    (m.clone(), path[1].clone(), path[2].clone())
                }
            } else {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    line,
                    format!("unknown module namespace {}", path[0]),
                ));
            }
        } else if path.len() == 4
            && path[0] == "aod"
            && builtin_ns_exports_enum(&path[1], &path[2])
        {
            (BUILTIN_MOD.to_string(), path[2].clone(), path[3].clone())
        } else {
            return Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                line,
                format!("invalid enum variant path {}", path.join("::")),
            ));
        };

        let edef = self.enums.get(&(enum_mod, enum_name)).ok_or_else(|| {
            Diagnostic::at_line(
                DiagnosticCode::TypeError,
                line,
                format!("unknown enum in {}", path.join("::")),
            )
        })?;
        for (vn, payload_tys) in &edef.variants {
            if vn == &var_name {
                return Ok((edef, payload_tys.as_slice()));
            }
        }
        Err(Diagnostic::at_line(
            DiagnosticCode::TypeError,
            line,
            format!("unknown variant {var_name} on enum {}", edef.name),
        ))
    }

    fn resolve_struct_path(
        &self,
        mod_key: &str,
        path: &[String],
        line: usize,
    ) -> DecResult<&StructDef> {
        if path.len() == 1 {
            let base = self.base_module_of(mod_key);
            if let Some(sd) = self.structs.get(&(base.to_string(), path[0].clone())) {
                return Ok(sd);
            }
            if let Some(ImportedSymbol::ModuleItem { module, item }) =
                self.imports_for(mod_key).get(&path[0])
            {
                if let Some(sd) = self.structs.get(&(module.clone(), item.clone())) {
                    return Ok(sd);
                }
            }
        } else if path.len() == 2 {
            if let Some(ImportedSymbol::ModuleNamespace(m)) =
                self.imports_for(mod_key).get(&path[0])
            {
                if let Some(sd) = self.structs.get(&(m.clone(), path[1].clone())) {
                    return Ok(sd);
                }
            }
        }
        Err(Diagnostic::at_line(
            DiagnosticCode::TypeError,
            line,
            format!("unknown struct {}", path.join("::")),
        ))
    }

    fn check_stmt(
        &mut self,
        stmt: &Stmt,
        scopes: &mut Vec<BTreeMap<String, VarBinding>>,
        ctx: &mut CheckContext,
    ) -> DecResult<()> {
        match &stmt.kind {
            StmtKind::Let { pattern, ty, value } => {
                let expected_ann = match ty {
                    Some(t) => {
                        let expected = self.resolve_type(&ctx.module_key, t, stmt.line)?;
                        if ctx.effect != Effect::QExt && expected.is_qext() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::QExtLeakageForbidden,
                                stmt.line,
                                "QExt type annotation forbidden on native let binding",
                            ));
                        }
                        if let Type::Map(ref k, _) = expected {
                            if !k.is_map_key() {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    stmt.line,
                                    format!("unsupported map key type {k}"),
                                ));
                            }
                        }
                        Some(expected)
                    }
                    None => None,
                };
                let val_ty = self.check_expr(value, scopes, ctx)?;
                let final_ty = match expected_ann {
                    Some(expected) => {
                        enforce_annotation_compatibility(&val_ty, &expected, stmt.line, "let")?;
                        expected
                    }
                    None => {
                        if matches!(pattern, LetPattern::Single(_)) && contains_unknown(&val_ty) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                stmt.line,
                                "empty collection requires an explicit type annotation (e.g. `let xs: [int] = []`)",
                            ));
                        }
                        val_ty
                    }
                };
                if ctx.effect != Effect::QExt && final_ty.is_qext() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        stmt.line,
                        "QExt value cannot bind to native let",
                    ));
                }

                match pattern {
                    LetPattern::Single(name) => {
                        if name != "_" {
                            let cur = scopes.last_mut().expect("scope stack non-empty");
                            if cur.contains_key(name) {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    stmt.line,
                                    format!("duplicate local declaration {name}"),
                                ));
                            }
                            cur.insert(name.clone(), VarBinding { ty: final_ty });
                        }
                    }
                    LetPattern::Tuple(elems) => {
                        let is_ro = final_ty.is_read_only();
                        let elem_tys = match final_ty.unwrap_read_only() {
                            Type::Tuple(ts) if ts.len() == elems.len() => ts.clone(),
                            other => {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    stmt.line,
                                    format!(
                                        "cannot destructure {other} into {}-element tuple",
                                        elems.len()
                                    ),
                                ));
                            }
                        };
                        let mut resolved_elem_tys = Vec::with_capacity(elems.len());
                        for ((n, elem_ty_opt), et) in elems.iter().zip(elem_tys) {
                            let actual_et = wrap_ro_if_needed(et, is_ro);
                            let final_et = if let Some(ann_expr) = elem_ty_opt {
                                let ann_ty =
                                    self.resolve_type(&ctx.module_key, ann_expr, stmt.line)?;
                                if ctx.effect != Effect::QExt && ann_ty.is_qext() {
                                    return Err(Diagnostic::at_line(
                                        DiagnosticCode::QExtLeakageForbidden,
                                        stmt.line,
                                        "QExt type annotation forbidden on native let tuple binding",
                                    ));
                                }
                                enforce_annotation_compatibility(
                                    &actual_et,
                                    &ann_ty,
                                    stmt.line,
                                    &format!("let tuple element {n}"),
                                )?;
                                ann_ty
                            } else {
                                if contains_unknown(&actual_et) {
                                    return Err(Diagnostic::at_line(
                                        DiagnosticCode::TypeError,
                                        stmt.line,
                                        "empty collection in tuple destructuring requires an explicit type annotation",
                                    ));
                                }
                                actual_et
                            };
                            resolved_elem_tys.push((n, final_et));
                        }
                        let cur = scopes.last_mut().expect("scope stack non-empty");
                        for (n, bound_ty) in resolved_elem_tys {
                            if n == "_" {
                                continue;
                            }
                            if cur.contains_key(n) {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    stmt.line,
                                    format!("duplicate local declaration {n}"),
                                ));
                            }
                            cur.insert(n.clone(), VarBinding { ty: bound_ty });
                        }
                    }
                }
                Ok(())
            }
            StmtKind::Assert(expr) => {
                let ty = self.check_expr(expr, scopes, ctx)?;
                let ok = ty == Type::Bool
                    || (ctx.effect == Effect::QExt && ty == Type::QExt(Box::new(Type::Bool)));
                if !ok {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        stmt.line,
                        format!("assert requires bool, got {ty}"),
                    ));
                }
                Ok(())
            }
            StmtKind::Emit(expr) => {
                let ty = self.check_expr(expr, scopes, ctx)?;
                if ty == Type::Unit {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        stmt.line,
                        "cannot emit unit ()",
                    ));
                }
                Ok(())
            }
            StmtKind::Return(opt_expr) => {
                if ctx.in_qext_sink {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        stmt.line,
                        "return cannot escape a one-way quarantine Q_ext: sink block into native control flow",
                    ));
                }
                let actual = match opt_expr {
                    Some(e) => self.check_expr(e, scopes, ctx)?,
                    None => Type::Unit,
                };
                enforce_return_compatibility(&actual, &ctx.expected_return, stmt.line)?;
                Ok(())
            }
            StmtKind::Pass => Ok(()),
            StmtKind::QuarantineBlock { body } => {
                if ctx.effect == Effect::ReadOnlyObserver {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        stmt.line,
                        "quarantine Q_ext: is forbidden inside read-only observer context (direct or transitive)",
                    ));
                }
                if !ctx.in_main && ctx.effect != Effect::QExt {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        stmt.line,
                        "quarantine Q_ext: sink is only permitted in dec main() or quarantine Q_ext functions",
                    ));
                }
                let saved_effect = ctx.effect;
                let saved_in_sink = ctx.in_qext_sink;

                ctx.effect = Effect::QExt;
                if !saved_in_sink && saved_effect != Effect::QExt {
                    ctx.in_qext_sink = true;
                }
                scopes.push(BTreeMap::new());
                let res = (|| {
                    for s in body {
                        self.check_stmt(s, scopes, ctx)?;
                    }
                    Ok(())
                })();
                scopes.pop();

                ctx.effect = saved_effect;
                ctx.in_qext_sink = saved_in_sink;
                res
            }
            StmtKind::ExprStmt(expr) => {
                let _ = self.check_expr(expr, scopes, ctx)?;
                Ok(())
            }
        }
    }

    fn check_expr(
        &mut self,
        expr: &Expr,
        scopes: &[BTreeMap<String, VarBinding>],
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        match &expr.kind {
            ExprKind::NullLit => Ok(Type::Null),
            ExprKind::IntLit(_) => Ok(Type::Int),
            ExprKind::BoolLit(_) => Ok(Type::Bool),
            ExprKind::BytesLit(_) => Ok(Type::Bytes),
            ExprKind::RatioLit(_) => Ok(Type::Ratio),
            ExprKind::Addr8Lit(_) => Ok(Type::Addr8),
            ExprKind::SupportLit(_) => Ok(Type::Support),
            ExprKind::TritLit(_) => Ok(Type::T1),
            ExprKind::Ident(name) => {
                if let Some(idx) = find_var_scope_index(scopes, name) {
                    let b = &scopes[idx][name];
                    if ctx.effect != Effect::QExt && b.ty.is_qext() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            expr.line,
                            format!("cannot reference QExt binding {name} outside quarantine Q_ext"),
                        ));
                    }
                    return Ok(b.ty.clone());
                }
                if let Some(c) = self.consts.get(&(ctx.module_key.clone(), name.clone())) {
                    if c.is_qext && ctx.effect != Effect::QExt {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            expr.line,
                            format!("cannot reference QExt const {name} outside quarantine Q_ext"),
                        ));
                    }
                    return Ok(c.ty.clone());
                }
                let base = self.base_module_of(&ctx.module_key).to_string();
                if base != ctx.module_key {
                    if let Some(c) = self.consts.get(&(base.clone(), name.clone())) {
                        return Ok(c.ty.clone());
                    }
                }
                if let Some(ImportedSymbol::ModuleItem { module, item }) =
                    self.imports_for(&ctx.module_key).get(name).cloned()
                {
                    if let Some(c) = self.consts.get(&(module, item)) {
                        if c.is_qext && ctx.effect != Effect::QExt {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::QExtLeakageForbidden,
                                expr.line,
                                format!(
                                    "cannot reference QExt const {name} outside quarantine Q_ext"
                                ),
                            ));
                        }
                        return Ok(c.ty.clone());
                    }
                }
                for (m, qm) in &self.qext_modules {
                    if m == &base {
                        let sub = format!("{m}::{qm}");
                        if self.consts.contains_key(&(sub, name.clone())) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::QExtLeakageForbidden,
                                expr.line,
                                format!(
                                    "QExt const {name} cannot be referenced outside quarantine Q_ext"
                                ),
                            ));
                        }
                    }
                }
                Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    expr.line,
                    format!("undefined name {name}"),
                ))
            }
            ExprKind::Path(segs) => {
                if (2..=4).contains(&segs.len()) {
                    if let Ok((edef, payload_tys)) =
                        self.resolve_enum_variant(&ctx.module_key, segs, expr.line)
                    {
                        if !payload_tys.is_empty() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr.line,
                                format!(
                                    "enum variant {} requires {} payload argument(s)",
                                    segs.join("::"),
                                    payload_tys.len()
                                ),
                            ));
                        }
                        return Ok(Type::Enum {
                            module: edef.module.clone(),
                            name: edef.name.clone(),
                        });
                    }
                }
                if segs.len() == 2 {
                    let ns = &segs[0];
                    let item = &segs[1];
                    if let Some(qsub) = self.resolve_qext_submodule(&ctx.module_key, ns) {
                        if let Some(c) = self.consts.get(&(qsub, item.clone())) {
                            if c.is_qext && ctx.effect != Effect::QExt {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::QExtLeakageForbidden,
                                    expr.line,
                                    format!(
                                        "cannot reference QExt const {ns}::{item} outside quarantine Q_ext"
                                    ),
                                ));
                            }
                            return Ok(c.ty.clone());
                        }
                    }
                    if let Some(ImportedSymbol::ModuleNamespace(mod_key)) =
                        self.imports_for(&ctx.module_key).get(ns).cloned()
                    {
                        if let Some(c) = self.consts.get(&(mod_key, item.clone())) {
                            if c.is_qext && ctx.effect != Effect::QExt {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::QExtLeakageForbidden,
                                    expr.line,
                                    format!(
                                        "cannot reference QExt const {ns}::{item} outside quarantine Q_ext"
                                    ),
                                ));
                            }
                            return Ok(c.ty.clone());
                        }
                    }
                } else if segs.len() == 3 {
                    if let Some(qsub) =
                        self.resolve_qext_submodule_3seg(&ctx.module_key, &segs[0], &segs[1])
                    {
                        if let Some(c) = self.consts.get(&(qsub, segs[2].clone())) {
                            if c.is_qext && ctx.effect != Effect::QExt {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::QExtLeakageForbidden,
                                    expr.line,
                                    format!(
                                        "cannot reference QExt const {} outside quarantine Q_ext",
                                        segs.join("::")
                                    ),
                                ));
                            }
                            return Ok(c.ty.clone());
                        }
                    }
                }
                Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    expr.line,
                    format!("undefined path {}", segs.join("::")),
                ))
            }
            ExprKind::MapLit(entries) => {
                if entries.is_empty() {
                    return Ok(Type::Map(Box::new(Type::Unknown), Box::new(Type::Unknown)));
                }
                let first_k = self.check_expr(&entries[0].0, scopes, ctx)?;
                let first_v = self.check_expr(&entries[0].1, scopes, ctx)?;
                if ctx.effect != Effect::QExt && (first_k.is_qext() || first_v.is_qext()) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        expr.line,
                        "QExt keys/values are forbidden in native map literals",
                    ));
                }
                if !first_k.is_map_key() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        entries[0].0.line,
                        format!("unsupported map key type {first_k}"),
                    ));
                }
                for (ke, ve) in &entries[1..] {
                    let kt = self.check_expr(ke, scopes, ctx)?;
                    let vt = self.check_expr(ve, scopes, ctx)?;
                    if kt != first_k {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ImplicitCoercionForbidden,
                            ke.line,
                            format!("heterogeneous map keys: expected {first_k}, got {kt}"),
                        ));
                    }
                    if vt != first_v {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ImplicitCoercionForbidden,
                            ve.line,
                            format!("heterogeneous map values: expected {first_v}, got {vt}"),
                        ));
                    }
                }
                Ok(Type::Map(Box::new(first_k), Box::new(first_v)))
            }
            ExprKind::SeqLit(items) => {
                if items.is_empty() {
                    return Ok(Type::List(Box::new(Type::Unknown)));
                }
                let first = self.check_expr(&items[0], scopes, ctx)?;
                if ctx.effect != Effect::QExt && first.is_qext() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        items[0].line,
                        "QExt elements are forbidden in native sequence literals",
                    ));
                }
                for it in &items[1..] {
                    let ty = self.check_expr(it, scopes, ctx)?;
                    if ty != first {
                        if let (Type::Array(a, n), Type::Array(b, m)) = (&first, &ty) {
                            if a == b && n != m {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::ShapeMismatch,
                                    it.line,
                                    format!(
                                        "ragged nested array literal: expected {first}, got {ty}"
                                    ),
                                ));
                            }
                        }
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ImplicitCoercionForbidden,
                            it.line,
                            format!("heterogeneous sequence elements: expected {first}, got {ty}"),
                        ));
                    }
                }
                Ok(Type::Array(Box::new(first), items.len()))
            }
            ExprKind::Tuple(elems) => {
                if elems.is_empty() {
                    return Ok(Type::Unit);
                }
                let mut tys = Vec::with_capacity(elems.len());
                for e in elems {
                    tys.push(self.check_expr(e, scopes, ctx)?);
                }
                Ok(Type::Tuple(tys))
            }
            ExprKind::WindowLit { start, end } => {
                let s_ty = self.check_expr(start, scopes, ctx)?;
                let e_ty = self.check_expr(end, scopes, ctx)?;
                if s_ty != Type::Int || e_ty != Type::Int {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr.line,
                        format!("window (u, v] endpoints must be int, got ({s_ty}, {e_ty}]"),
                    ));
                }
                Ok(Type::Window)
            }
            ExprKind::Range { start, end } => {
                let s_ty = self.check_expr(start, scopes, ctx)?;
                let e_ty = self.check_expr(end, scopes, ctx)?;
                if s_ty != Type::Int || e_ty != Type::Int {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr.line,
                        format!("range a..b endpoints must be int, got {s_ty}..{e_ty}"),
                    ));
                }
                Ok(Type::Range)
            }
            ExprKind::BoxExpr(dims) => {
                if dims.len() != 3 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr.line,
                        format!("box([...]) requires 3 axis ranges, got {}", dims.len()),
                    ));
                }
                for d in dims {
                    let ty = self.check_expr(d, scopes, ctx)?;
                    if ty != Type::Range {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            d.line,
                            format!("box axis must be a range a..b, got {ty}"),
                        ));
                    }
                }
                Ok(Type::Box3)
            }
            ExprKind::Simulate {
                prepare,
                ticks,
                observers,
            } => {
                if ctx.effect == Effect::ReadOnlyObserver {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ObserverMutationForbidden,
                        expr.line,
                        "simulate is forbidden inside read-only observer context",
                    ));
                }
                if ctx.effect == Effect::QExt {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        expr.line,
                        "simulate cannot be invoked inside quarantine Q_ext",
                    ));
                }
                if !self
                    .prepares
                    .contains_key(&(ctx.module_key.clone(), prepare.clone()))
                {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr.line,
                        format!("undefined prepare declaration {prepare}"),
                    ));
                }
                let t_ty = self.check_expr(ticks, scopes, ctx)?;
                if t_ty != Type::Int {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr.line,
                        format!("simulate tick count must be int, got {t_ty}"),
                    ));
                }
                for obs in observers {
                    if !self
                        .observes
                        .contains_key(&(ctx.module_key.clone(), obs.clone()))
                    {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            expr.line,
                            format!("undefined observe declaration {obs}"),
                        ));
                    }
                }
                Ok(Type::SimResult)
            }
            ExprKind::Unary { op, operand } => {
                let ty = self.check_expr(operand, scopes, ctx)?;
                self.check_unary_ty(*op, &ty, expr.line, ctx)
            }
            ExprKind::Adverb { op, operand } => {
                let ty = self.check_expr(operand, scopes, ctx)?;
                self.check_adverb_ty(*op, &ty, expr.line, ctx)
            }
            ExprKind::Binary { op, left, right } => {
                let lt = self.check_expr(left, scopes, ctx)?;
                let rt = self.check_expr(right, scopes, ctx)?;
                self.check_binary_ty(*op, &lt, &rt, expr.line, ctx)
            }
            ExprKind::Pipe { input, stage } => {
                let in_ty = self.check_expr(input, scopes, ctx)?;
                self.check_pipe_stage(&in_ty, stage, expr.line, scopes, ctx)
            }
            ExprKind::Match { scrutinee, arms } => {
                self.check_match_expr(scrutinee, arms, expr.line, scopes, ctx)
            }
            ExprKind::Field { base, name } => {
                let base_ty = self.check_expr(base, scopes, ctx)?;
                self.check_field_ty(&base_ty, name, expr.line)
            }
            ExprKind::Index { base, index } => {
                let base_ty = self.check_expr(base, scopes, ctx)?;
                let idx_ty = self.check_expr(index, scopes, ctx)?;
                let is_ro = base_ty.is_read_only();
                match base_ty.unwrap_read_only() {
                    Type::Array(elem, _) | Type::List(elem) => {
                        if *idx_ty.unwrap_read_only() != Type::Int {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr.line,
                                format!("sequence index must be int, got {idx_ty}"),
                            ));
                        }
                        Ok(wrap_ro_if_needed((**elem).clone(), is_ro))
                    }
                    Type::Bytes => {
                        if *idx_ty.unwrap_read_only() != Type::Int {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr.line,
                                format!("bytes index must be int, got {idx_ty}"),
                            ));
                        }
                        Ok(Type::B8)
                    }
                    Type::Map(k_ty, v_ty) => {
                        if !types_compatible_with_annotation(idx_ty.unwrap_read_only(), k_ty) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ImplicitCoercionForbidden,
                                expr.line,
                                format!("map key expects {k_ty}, got {idx_ty}"),
                            ));
                        }
                        Ok(wrap_ro_if_needed((**v_ty).clone(), is_ro))
                    }
                    other => Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr.line,
                        format!("cannot index value of type {other}"),
                    )),
                }
            }
            ExprKind::Call { callee, args } => self.check_call(callee, args, expr.line, scopes, ctx),
        }
    }

    fn check_unary_ty(
        &self,
        op: UnaryOp,
        ty: &Type,
        line: usize,
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        let inner = ty.unwrap_read_only();
        if op == UnaryOp::Boundary {
            return match inner {
                Type::Box3 | Type::SimResult => Ok(Type::Bytes),
                Type::Window | Type::Range => Ok(Type::Tuple(vec![Type::Int, Type::Int])),
                Type::Array(elem, n) if elem.unwrap_read_only().is_numeric() => {
                    if *n < 2 {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ShapeMismatch,
                            line,
                            format!("discrete boundary ∂ on [{elem}; {n}] requires length >= 2"),
                        ));
                    }
                    Ok(Type::Array(
                        Box::new(elem.unwrap_read_only().clone()),
                        *n - 1,
                    ))
                }
                Type::List(elem) if elem.unwrap_read_only().is_numeric() => {
                    Ok(Type::List(Box::new(elem.unwrap_read_only().clone())))
                }
                other => Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    line,
                    format!("boundary operator ∂ unsupported on {other}"),
                )),
            };
        }

        match inner {
            Type::Array(elem, n) => {
                let out = self.check_unary_ty(op, elem, line, ctx)?;
                Ok(Type::Array(Box::new(out), *n))
            }
            Type::List(elem) => {
                let out = self.check_unary_ty(op, elem, line, ctx)?;
                Ok(Type::List(Box::new(out)))
            }
            _ => match op {
                UnaryOp::Not => {
                    if *inner == Type::Bool {
                        Ok(Type::Bool)
                    } else if ctx.effect == Effect::QExt
                        && *inner == Type::QExt(Box::new(Type::Bool))
                    {
                        Ok(Type::QExt(Box::new(Type::Bool)))
                    } else {
                        Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("invalid unary `not` on {ty}"),
                        ))
                    }
                }
                UnaryOp::Pos | UnaryOp::Neg => {
                    if inner.is_numeric() {
                        Ok(inner.clone())
                    } else if ctx.effect == Effect::QExt
                        && *inner == Type::QExt(Box::new(Type::Ratio))
                    {
                        Ok(inner.clone())
                    } else {
                        Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("invalid unary arithmetic on {ty}"),
                        ))
                    }
                }
                UnaryOp::BitNot => {
                    if inner.is_bitwise() {
                        Ok(inner.clone())
                    } else {
                        Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("bitwise `~` requires b8, b64, or q4, got {ty}"),
                        ))
                    }
                }
                UnaryOp::Boundary => unreachable!(),
            },
        }
    }

    fn check_adverb_ty(
        &self,
        op: AdverbOp,
        ty: &Type,
        line: usize,
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        let (elem_ty, fixed_len) = match ty.unwrap_read_only() {
            Type::Array(e, n) => (e.unwrap_read_only().clone(), Some(*n)),
            Type::List(e) => (e.unwrap_read_only().clone(), None),
            Type::Range => (Type::Int, None),
            other => {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    line,
                    format!("adverb {op:?} requires an array, slice, or range, got {other}"),
                ));
            }
        };

        let is_scan = matches!(
            op,
            AdverbOp::ScanAdd | AdverbOp::ScanMul | AdverbOp::ScanMin | AdverbOp::ScanMax
        );
        let wrap_scan = |scalar_ty: Type| match fixed_len {
            Some(n) => Type::Array(Box::new(scalar_ty), n),
            None => Type::List(Box::new(scalar_ty)),
        };

        let reduced = match op {
            AdverbOp::FoldAdd | AdverbOp::ScanAdd => {
                if matches!(elem_ty, Type::Int | Type::Ratio)
                    || (ctx.effect == Effect::QExt && elem_ty == Type::QExt(Box::new(Type::Ratio)))
                {
                    elem_ty
                } else if let Type::Array(sub, m) = &elem_ty {
                    if matches!(sub.as_ref(), Type::Int | Type::Ratio) {
                        Type::Array(sub.clone(), *m)
                    } else {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("additive fold/scan unsupported on element type {elem_ty}"),
                        ));
                    }
                } else {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("additive fold/scan requires int or ratio elements, got {elem_ty}"),
                    ));
                }
            }
            AdverbOp::FoldMul | AdverbOp::ScanMul => {
                if elem_ty.is_numeric()
                    || (ctx.effect == Effect::QExt && elem_ty == Type::QExt(Box::new(Type::Ratio)))
                {
                    elem_ty
                } else {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("multiplicative fold/scan requires numeric elements, got {elem_ty}"),
                    ));
                }
            }
            AdverbOp::FoldMin | AdverbOp::FoldMax | AdverbOp::ScanMin | AdverbOp::ScanMax => {
                if elem_ty.is_ordered()
                    || (ctx.effect == Effect::QExt && elem_ty == Type::QExt(Box::new(Type::Ratio)))
                {
                    elem_ty
                } else {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("min/max fold/scan requires ordered elements, got {elem_ty}"),
                    ));
                }
            }
            AdverbOp::FoldAnd | AdverbOp::FoldOr => {
                if elem_ty == Type::Bool || elem_ty.is_bitwise() {
                    elem_ty
                } else {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("&/ and |/ require bool or bit elements, got {elem_ty}"),
                    ));
                }
            }
            AdverbOp::FoldSuperpose => {
                if matches!(elem_ty, Type::T1 | Type::Int) {
                    elem_ty
                } else {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("superpose/ requires t1 or int elements, got {elem_ty}"),
                    ));
                }
            }
        };

        Ok(if is_scan { wrap_scan(reduced) } else { reduced })
    }

    fn check_binary_ty(
        &self,
        op: BinaryOp,
        lt: &Type,
        rt: &Type,
        line: usize,
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        let l_inner = lt.unwrap_read_only();
        let r_inner = rt.unwrap_read_only();

        // 1. Mask Sieve `mask ?> xs` -> `[T]`
        if op == BinaryOp::Sieve {
            let m_len = match l_inner {
                Type::Array(e, n) if **e == Type::Bool => Some(*n),
                Type::List(e) if **e == Type::Bool => None,
                other => {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("sieve `?>` left-hand mask must be [bool; N] or [bool], got {other}"),
                    ));
                }
            };
            let (elem_ty, x_len) = match r_inner {
                Type::Array(e, n) => ((**e).clone(), Some(*n)),
                Type::List(e) => ((**e).clone(), None),
                Type::Range => (Type::Int, None),
                other => {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("sieve `?>` right-hand operand must be a sequence, got {other}"),
                    ));
                }
            };
            if let (Some(n), Some(m)) = (m_len, x_len) {
                if n != m {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ShapeMismatch,
                        line,
                        format!("sieve `?>` shape mismatch: mask length {n} != sequence length {m}"),
                    ));
                }
            }
            let out_elem = wrap_ro_if_needed(elem_ty, rt.is_read_only());
            return Ok(Type::List(Box::new(out_elem)));
        }

        // 2. Explicit Outer Products `+:` and `*:`
        if matches!(op, BinaryOp::OuterAdd | BinaryOp::OuterMul) {
            let scalar_op = if op == BinaryOp::OuterAdd {
                BinaryOp::Add
            } else {
                BinaryOp::Mul
            };
            return match (l_inner, r_inner) {
                (Type::Array(a, m), Type::Array(b, n)) => {
                    let cell = self.check_binary_ty(scalar_op, a, b, line, ctx)?;
                    Ok(Type::Array(Box::new(Type::Array(Box::new(cell), *n)), *m))
                }
                (Type::Array(a, _) | Type::List(a), Type::Array(b, _) | Type::List(b)) => {
                    let cell = self.check_binary_ty(scalar_op, a, b, line, ctx)?;
                    Ok(Type::List(Box::new(Type::List(Box::new(cell)))))
                }
                _ => Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    line,
                    format!("outer product {op:?} requires two sequences, got {lt} and {rt}"),
                )),
            };
        }

        // 3. Whole-sequence structural `==` / `!=` when both operands have equal positive rank!
        let l_rank = sequence_rank(l_inner);
        let r_rank = sequence_rank(r_inner);
        if matches!(op, BinaryOp::Eq | BinaryOp::NotEq) && l_rank > 0 && l_rank == r_rank {
            return check_same_rank_sequence_eq(l_inner, r_inner, line, ctx);
        }

        // 4. Static Rank Lifting across arrays/slices + scalar broadcast!
        if l_rank > 0 || r_rank > 0 {
            if l_rank > r_rank {
                return match l_inner {
                    Type::Array(a, n) => {
                        let out = self.check_binary_ty(op, a, r_inner, line, ctx)?;
                        Ok(Type::Array(Box::new(out), *n))
                    }
                    Type::List(a) => {
                        let out = self.check_binary_ty(op, a, r_inner, line, ctx)?;
                        Ok(Type::List(Box::new(out)))
                    }
                    _ => unreachable!(),
                };
            }
            if r_rank > l_rank {
                return match r_inner {
                    Type::Array(b, n) => {
                        let out = self.check_binary_ty(op, l_inner, b, line, ctx)?;
                        Ok(Type::Array(Box::new(out), *n))
                    }
                    Type::List(b) => {
                        let out = self.check_binary_ty(op, l_inner, b, line, ctx)?;
                        Ok(Type::List(Box::new(out)))
                    }
                    _ => unreachable!(),
                };
            }
            // Equal positive rank (`l_rank == r_rank > 0`):
            return match (l_inner, r_inner) {
                (Type::Array(a, n), Type::Array(b, m)) => {
                    if n != m {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ShapeMismatch,
                            line,
                            format!(
                                "shape mismatch in rank-lifted {op:?}: {l_inner} vs {r_inner}"
                            ),
                        ));
                    }
                    let out = self.check_binary_ty(op, a, b, line, ctx)?;
                    Ok(Type::Array(Box::new(out), *n))
                }
                (Type::Array(a, _) | Type::List(a), Type::Array(b, _) | Type::List(b)) => {
                    let out = self.check_binary_ty(op, a, b, line, ctx)?;
                    Ok(Type::List(Box::new(out)))
                }
                _ => unreachable!(),
            };
        }

        // 5. Scalar binary operations:
        // 5a. Ternary phase-gating `x * role` (`role: t1`)
        if op == BinaryOp::Mul {
            match (l_inner, r_inner) {
                (Type::Int, Type::T1) | (Type::T1, Type::Int) => return Ok(Type::Int),
                (Type::Ratio, Type::T1) | (Type::T1, Type::Ratio) => return Ok(Type::Ratio),
                _ => {}
            }
        }
        // 5b. Bit shifts `(b8 | b64 | q4) << int`
        if matches!(op, BinaryOp::Shl | BinaryOp::Shr) {
            if l_inner.is_bitwise() && *r_inner == Type::Int {
                return Ok(l_inner.clone());
            }
            return Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                line,
                format!(
                    "bit shift requires (b8 | b64 | q4, int), got ({l_inner}, {r_inner})"
                ),
            ));
        }

        if l_inner != r_inner {
            return Err(Diagnostic::at_line(
                DiagnosticCode::ImplicitCoercionForbidden,
                line,
                format!(
                    "implicit numeric/type coercion is forbidden: {lt} {op:?} {rt}"
                ),
            ));
        }

        let ty = l_inner.clone();
        match op {
            BinaryOp::And | BinaryOp::Or => {
                if ty == Type::Bool {
                    Ok(Type::Bool)
                } else if ctx.effect == Effect::QExt && ty == Type::QExt(Box::new(Type::Bool)) {
                    Ok(Type::QExt(Box::new(Type::Bool)))
                } else {
                    Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("Boolean operation requires bool operands, got {ty}"),
                    ))
                }
            }
            BinaryOp::Eq | BinaryOp::NotEq => {
                if ty.is_qext() {
                    if ctx.effect != Effect::QExt {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            line,
                            "cannot compare QExt values outside quarantine Q_ext",
                        ));
                    }
                    Ok(Type::QExt(Box::new(Type::Bool)))
                } else {
                    Ok(Type::Bool)
                }
            }
            BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => {
                if ty.is_ordered() {
                    Ok(Type::Bool)
                } else if ctx.effect == Effect::QExt && ty == Type::QExt(Box::new(Type::Ratio)) {
                    Ok(Type::QExt(Box::new(Type::Bool)))
                } else {
                    Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("ordering requires an ordered type, got {ty}"),
                    ))
                }
            }
            BinaryOp::Cmp3 => {
                if ty.is_ordered() {
                    Ok(Type::T1)
                } else {
                    Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("three-way comparison `<=>` requires an ordered type, got {ty}"),
                    ))
                }
            }
            BinaryOp::Min | BinaryOp::Max => {
                if ty.is_ordered() {
                    Ok(ty)
                } else if ctx.effect == Effect::QExt && ty == Type::QExt(Box::new(Type::Ratio)) {
                    Ok(ty)
                } else {
                    Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("min/max requires an ordered type, got {ty}"),
                    ))
                }
            }
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => {
                if ty.is_numeric() {
                    Ok(ty)
                } else if ctx.effect == Effect::QExt && ty == Type::QExt(Box::new(Type::Ratio)) {
                    Ok(ty)
                } else {
                    Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("arithmetic requires t1, int, or ratio, got {ty}"),
                    ))
                }
            }
            BinaryOp::Div => {
                if ty.is_numeric() {
                    Ok(Type::Ratio)
                } else if ctx.effect == Effect::QExt && ty == Type::QExt(Box::new(Type::Ratio)) {
                    Ok(Type::QExt(Box::new(Type::Ratio)))
                } else {
                    Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("division requires t1, int, or ratio, got {ty}"),
                    ))
                }
            }
            BinaryOp::FloorDiv | BinaryOp::Mod => {
                if ty == Type::Int {
                    Ok(Type::Int)
                } else {
                    Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("quotient/remainder require int, got {ty}"),
                    ))
                }
            }
            BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor => {
                if ty.is_bitwise() {
                    Ok(ty)
                } else {
                    Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!(
                            "bitwise ops apply to b8, b64, or q4 (not balanced-ternary {ty}; use b64(x) or b64_bits(x))"
                        ),
                    ))
                }
            }
            BinaryOp::Shl | BinaryOp::Shr | BinaryOp::Sieve | BinaryOp::OuterAdd | BinaryOp::OuterMul => {
                unreachable!()
            }
        }
    }

    fn check_pipe_stage(
        &mut self,
        in_ty: &Type,
        stage: &PipeStage,
        line: usize,
        scopes: &[BTreeMap<String, VarBinding>],
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        match stage {
            PipeStage::Adverb(op) => self.check_adverb_ty(*op, in_ty, line, ctx),
            PipeStage::Boundary => self.check_unary_ty(UnaryOp::Boundary, in_ty, line, ctx),
            PipeStage::Section { op, rhs } => {
                let r_ty = self.check_expr(rhs, scopes, ctx)?;
                // Normalize Range input to `[int]` if piped into arithmetic/comparison section
                let norm_in = if *in_ty.unwrap_read_only() == Type::Range {
                    Type::List(Box::new(Type::Int))
                } else {
                    in_ty.clone()
                };
                self.check_binary_ty(*op, &norm_in, &r_ty, line, ctx)
            }
            PipeStage::Call { callee, extra_args } => {
                self.check_call_with_prepended_input(Some(in_ty.clone()), callee, extra_args, line, scopes, ctx)
            }
        }
    }

    fn check_match_expr(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchExprArm],
        line: usize,
        scopes: &[BTreeMap<String, VarBinding>],
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        let scrut_ty = self.check_expr(scrutinee, scopes, ctx)?;
        let mut covered_variants: BTreeSet<String> = BTreeSet::new();
        let mut covered_bools = [false; 2];
        let mut covered_trits = [false; 3]; // -1, 0, +1
        let mut has_irrefutable = false;
        let mut result_ty: Option<Type> = None;

        for arm in arms {
            let mut arm_scope = BTreeMap::new();
            self.check_pattern(
                &arm.pattern,
                &scrut_ty,
                &ctx.module_key,
                &mut arm_scope,
                arm.line,
            )?;
            let mut arm_scopes = scopes.to_vec();
            arm_scopes.push(arm_scope);

            if let Some(ref g) = arm.guard {
                let g_ty = self.check_expr(g, &arm_scopes, ctx)?;
                if g_ty != Type::Bool {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        g.line,
                        format!("match arm 'where' guard must be bool, got {g_ty}"),
                    ));
                }
            } else {
                match &arm.pattern {
                    Pattern::Wildcard | Pattern::Binding(_) => {
                        has_irrefutable = true;
                    }
                    Pattern::NullLit => {
                        has_irrefutable = true;
                    }
                    Pattern::BoolLit(b) => {
                        covered_bools[usize::from(*b)] = true;
                    }
                    Pattern::TritLit(t) => match t.value() {
                        -1 => covered_trits[0] = true,
                        0 => covered_trits[1] = true,
                        1 => covered_trits[2] = true,
                        _ => {}
                    },
                    Pattern::IntLit(n) if *scrut_ty.unwrap_read_only() == Type::T1 => {
                        match n.to_i64() {
                            Some(-1) => covered_trits[0] = true,
                            Some(0) => covered_trits[1] = true,
                            Some(1) => covered_trits[2] = true,
                            _ => {}
                        }
                    }
                    Pattern::EnumVariant { path, bindings } => {
                        if bindings.iter().all(is_irrefutable_pattern) {
                            if let Some(vname) = path.last() {
                                covered_variants.insert(vname.clone());
                            }
                        }
                    }
                    Pattern::Tuple(_) if is_irrefutable_pattern(&arm.pattern) => {
                        has_irrefutable = true;
                    }
                    _ => {}
                }
            }

            let body_ty = self.check_expr(&arm.body, &arm_scopes, ctx)?;
            if let Some(ref prev) = result_ty {
                result_ty = Some(unify_branch_types(prev, &body_ty, arm.line)?);
            } else {
                result_ty = Some(body_ty);
            }
        }

        if !has_irrefutable {
            match scrut_ty.unwrap_read_only() {
                Type::Enum { module, name } => {
                    if let Some(edef) = self.enums.get(&(module.clone(), name.clone())) {
                        let missing: Vec<&str> = edef
                            .variants
                            .iter()
                            .map(|(vn, _)| vn.as_str())
                            .filter(|vn| !covered_variants.contains(*vn))
                            .collect();
                        if !missing.is_empty() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!(
                                    "non-exhaustive match on enum {name}: missing variant(s) {}",
                                    missing.join(", ")
                                ),
                            ));
                        }
                    }
                }
                Type::Bool => {
                    if !covered_bools.iter().all(|&c| c) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            "non-exhaustive match on bool: cover both true and false (or `_`)",
                        ));
                    }
                }
                Type::T1 => {
                    if !covered_trits.iter().all(|&c| c) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            "non-exhaustive match on t1: cover -1t, 0t, and +1t (or `_`)",
                        ));
                    }
                }
                other => {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!(
                            "non-exhaustive match on {other}: add a fallback `_ -> ...` arm"
                        ),
                    ));
                }
            }
        }

        Ok(result_ty.unwrap_or(Type::Unit))
    }

    fn check_field_ty(&self, base_ty: &Type, name: &str, line: usize) -> DecResult<Type> {
        let is_ro = base_ty.is_read_only();
        let inner = base_ty.unwrap_read_only();
        // Struct-field projection across arrays and slices (`bins.t0`)!
        if let Type::Array(elem, n) = inner {
            let elem_field = self.check_field_ty(elem, name, line)?;
            let projected = Type::Array(Box::new(elem_field), *n);
            return Ok(wrap_ro_if_needed(projected, is_ro));
        }
        if let Type::List(elem) = inner {
            let elem_field = self.check_field_ty(elem, name, line)?;
            let projected = Type::List(Box::new(elem_field));
            return Ok(wrap_ro_if_needed(projected, is_ro));
        }

        let field_ty = if let Type::Struct {
            module,
            name: sname,
        } = inner
        {
            self.resolve_struct_field(module, sname, name, line)?
        } else {
            resolve_builtin_field_type(inner, name, line)?
        };
        Ok(wrap_ro_if_needed(field_ty, is_ro))
    }

    fn resolve_struct_field(
        &self,
        module: &str,
        sname: &str,
        field: &str,
        line: usize,
    ) -> DecResult<Type> {
        let sdef = self
            .structs
            .get(&(module.to_string(), sname.to_string()))
            .ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    line,
                    format!("unknown struct {sname}"),
                )
            })?;
        for (n, t) in &sdef.fields {
            if n == field {
                return Ok(t.clone());
            }
        }
        Err(Diagnostic::at_line(
            DiagnosticCode::TypeError,
            line,
            format!("no field .{field} on struct {sname}"),
        ))
    }

    fn check_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        line: usize,
        scopes: &[BTreeMap<String, VarBinding>],
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        self.check_call_with_prepended_input(None, callee, args, line, scopes, ctx)
    }

    fn resolve_core_op_name(
        &self,
        callee: &Expr,
        scopes: &[BTreeMap<String, VarBinding>],
        ctx: &CheckContext,
    ) -> Option<String> {
        match &callee.kind {
            ExprKind::Ident(n) => {
                if find_var_scope_index(scopes, n).is_some() {
                    return None;
                }
                if !is_reserved_builtin_name(n) {
                    let base_mod = self.base_module_of(&ctx.module_key).to_string();
                    if self.functions.contains_key(&(ctx.module_key.clone(), n.clone()))
                        || (base_mod != ctx.module_key
                            && self.functions.contains_key(&(base_mod, n.clone())))
                        || matches!(
                            self.imports_for(&ctx.module_key).get(n),
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
                    && self.imports_for(&ctx.module_key).get(&segs[0])
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

    fn resolve_ho_callback(
        &self,
        fn_expr: &Expr,
        scopes: &[BTreeMap<String, VarBinding>],
        line: usize,
        ctx: &CheckContext,
    ) -> DecResult<CallTarget> {
        if let ExprKind::Ident(ref n) = fn_expr.kind {
            if find_var_scope_index(scopes, n).is_some() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    line,
                    format!(
                        "higher-order callback must be a static function name or module path, not local binding {n}"
                    ),
                ));
            }
        }
        self.resolve_call_target(fn_expr, line, ctx)
    }

    fn unique_ho_param_hint(
        &self,
        target: &CallTarget,
        arity: usize,
        param_idx: usize,
    ) -> Option<Type> {
        match target {
            CallTarget::Builtin(canon) => {
                let sigs: Vec<_> = lookup_builtin(canon)
                    .into_iter()
                    .filter(|s| s.params.len() == arity)
                    .collect();
                if sigs.len() == 1 {
                    sigs[0].params.get(param_idx).cloned()
                } else {
                    None
                }
            }
            CallTarget::UserFn(entry) => {
                let mut found: Option<Type> = None;
                for arm in &entry.decl.arms {
                    if arm.params.len() != arity {
                        continue;
                    }
                    let Ok(tys) = self.infer_arm_param_types_for_entry(entry, arm) else {
                        return None;
                    };
                    let t = &tys[param_idx];
                    if *t == Type::Unknown {
                        continue;
                    }
                    if let Some(ref prev) = found {
                        if prev != t {
                            return None;
                        }
                    } else {
                        found = Some(t.clone());
                    }
                }
                found
            }
        }
    }

    fn check_opt_env_expr(
        &mut self,
        op: &str,
        env_expr: Option<&Expr>,
        line: usize,
        scopes: &[BTreeMap<String, VarBinding>],
        ctx: &CheckContext,
    ) -> DecResult<Option<Type>> {
        let Some(ee) = env_expr else {
            return Ok(None);
        };
        let et = self.check_expr(ee, scopes, ctx)?;
        if et == Type::Unit || contains_unknown(&et) {
            return Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                line,
                format!("{op} environment argument requires a concrete non-unit type"),
            ));
        }
        if ctx.effect != Effect::QExt && et.is_qext() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::QExtLeakageForbidden,
                line,
                format!("{op} cannot pass QExt env outside quarantine Q_ext"),
            ));
        }
        Ok(Some(et))
    }

    fn extract_afc_seq_elem(
        &self,
        op: &str,
        seq_ty: &Type,
        hint: Option<Type>,
        line: usize,
    ) -> DecResult<(Type, Type)> {
        let is_ro = seq_ty.is_read_only();
        match seq_ty.unwrap_read_only() {
            Type::Bytes => Ok((Type::B8, Type::B8)),
            Type::Array(e, _) | Type::List(e) => {
                let raw = if **e == Type::Unknown {
                    hint.map(|h| h.unwrap_read_only().clone())
                        .filter(|h| *h != Type::Unknown)
                        .ok_or_else(|| {
                            Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!(
                                    "{op} on empty sequence requires an explicit element type annotation"
                                ),
                            )
                        })?
                } else {
                    (**e).clone()
                };
                let passed = wrap_ro_if_needed(raw.clone(), is_ro);
                Ok((raw, passed))
            }
            other => Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                line,
                format!("{op} expects [T], [T; N], or bytes, got {other}"),
            )),
        }
    }

    fn has_user_fn_for_struct_path(&self, mod_key: &str, sp: &[String]) -> bool {
        let base = self.base_module_of(mod_key).to_string();
        if sp.len() == 1 {
            if self.functions.contains_key(&(mod_key.to_string(), sp[0].clone()))
                || self.functions.contains_key(&(base, sp[0].clone()))
            {
                return true;
            }
            if let Some(ImportedSymbol::ModuleItem { module, item }) =
                self.imports_for(mod_key).get(&sp[0])
            {
                return self.functions.contains_key(&(module.clone(), item.clone()));
            }
        } else if sp.len() == 2 {
            if let Some(ImportedSymbol::ModuleNamespace(m)) = self.imports_for(mod_key).get(&sp[0])
            {
                return self.functions.contains_key(&(m.clone(), sp[1].clone()));
            }
        }
        false
    }

    fn check_call_with_prepended_input(
        &mut self,
        prepended: Option<Type>,
        callee: &Expr,
        args: &[Expr],
        line: usize,
        scopes: &[BTreeMap<String, VarBinding>],
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        let core_op = self.resolve_core_op_name(callee, scopes, ctx);
        let simple_name = core_op.as_deref();

        // 1. Higher-order pure combinators & AFC scope operators
        //    Note: 4-arg `fold(trace, trace, bytes, bytes)` falls through to `aod::witness::fold`!
        let total_arity = usize::from(prepended.is_some()) + args.len();
        if let Some(cname) = simple_name {
            match (cname, total_arity) {
                ("walk", 3) | ("settle", 3) => {
                    let is_settle = cname == "settle";
                    let (init_ty, count_expr, fn_expr) = if let Some(in_ty) = prepended {
                        (in_ty, &args[0], &args[1])
                    } else {
                        let s0 = self.check_expr(&args[0], scopes, ctx)?;
                        (s0, &args[1], &args[2])
                    };
                    let cnt_ty = self.check_expr(count_expr, scopes, ctx)?;
                    if *cnt_ty.unwrap_read_only() != Type::Int {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("{cname} count/fuel must be int, got {cnt_ty}"),
                        ));
                    }
                    let state_ty = self.check_step_combinator_fn(
                        &init_ty,
                        fn_expr,
                        is_settle,
                        line,
                        scopes,
                        ctx,
                    )?;
                    return Ok(state_ty);
                }
                ("fold", 3) => {
                    let (seq_ty, init_expr, fn_expr) = if let Some(in_ty) = prepended {
                        (in_ty, &args[0], &args[1])
                    } else {
                        let st = self.check_expr(&args[0], scopes, ctx)?;
                        (st, &args[1], &args[2])
                    };
                    let (elem_ty, _) = extract_seq_elem_and_len(&seq_ty, cname, line)?;
                    let init_ty = self.check_expr(init_expr, scopes, ctx)?;
                    return self.check_fold_combinator_fn(
                        &init_ty, &elem_ty, fn_expr, line, scopes, ctx,
                    );
                }
                ("each", 2) => {
                    let (seq_ty, fn_expr) = if let Some(in_ty) = prepended {
                        (in_ty, &args[0])
                    } else {
                        let st = self.check_expr(&args[0], scopes, ctx)?;
                        (st, &args[1])
                    };
                    let (elem_ty, fixed_len) = extract_seq_elem_and_len(&seq_ty, "each", line)?;
                    let out_elem = self.check_unary_fn_expr(&elem_ty, fn_expr, line, scopes, ctx)?;
                    return Ok(match fixed_len {
                        Some(n) => Type::Array(Box::new(out_elem), n),
                        None => Type::List(Box::new(out_elem)),
                    });
                }
                ("evolve", _) => {
                    if total_arity != 3 && total_arity != 4 {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!(
                                "evolve(init, dom, step_fn[, env]) expects 3 or 4 arguments, got {total_arity}"
                            ),
                        ));
                    }
                    let cb_arity = if total_arity == 4 { 3 } else { 2 };
                    let (mut s_ty, dom_expr, fn_expr, env_expr) = if let Some(in_ty) = prepended {
                        (in_ty, &args[0], &args[1], args.get(2))
                    } else {
                        let s0 = self.check_expr(&args[0], scopes, ctx)?;
                        (s0, &args[1], &args[2], args.get(3))
                    };
                    let step_cb = self.resolve_ho_callback(fn_expr, scopes, line, ctx)?;
                    if contains_unknown(&s_ty) {
                        if let Some(hint_s) = self.unique_ho_param_hint(&step_cb, cb_arity, 0) {
                            if types_compatible_with_annotation(&s_ty, &hint_s) {
                                s_ty = hint_s;
                            }
                        }
                    }
                    if s_ty == Type::Unit || contains_unknown(&s_ty) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            "evolve initial state requires a concrete non-unit type",
                        ));
                    }
                    if ctx.effect != Effect::QExt && s_ty.is_qext() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            line,
                            "evolve cannot carry QExt state outside quarantine Q_ext",
                        ));
                    }
                    let dom_ty = self.check_expr(dom_expr, scopes, ctx)?;
                    if !matches!(dom_ty.unwrap_read_only(), Type::Range | Type::Window) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("evolve domain must be range or window, got {dom_ty}"),
                        ));
                    }
                    let env_ty = self.check_opt_env_expr("evolve", env_expr, line, scopes, ctx)?;
                    let mut candidates = vec![s_ty.clone()];
                    let decayed = decay_top_array_to_list(&s_ty);
                    if decayed != s_ty {
                        candidates.push(decayed);
                    }
                    let mut last_err = None;
                    for cand_s in candidates {
                        let mut cb_args = vec![cand_s.clone(), Type::Int];
                        if let Some(ref et) = env_ty {
                            cb_args.push(et.clone());
                        }
                        match self.dispatch_call_target_direct(&step_cb, &cb_args, line, ctx) {
                            Ok(step_ret) => {
                                if step_ret == cand_s {
                                    return Ok(cand_s);
                                }
                                if !cand_s.is_read_only()
                                    && !step_ret.is_read_only()
                                    && types_compatible_with_annotation(&step_ret, &cand_s)
                                {
                                    return Ok(cand_s);
                                }
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    line,
                                    format!(
                                        "evolve step function must return state type {cand_s}, got {step_ret}"
                                    ),
                                ));
                            }
                            Err(e) => last_err = Some(e),
                        }
                    }
                    return Err(last_err.unwrap());
                }
                (op @ ("scan" | "fold_seq"), _) => {
                    if total_arity != 3 && total_arity != 4 {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!(
                                "{op}(init, src, step_fn[, env]) expects 3 or 4 arguments, got {total_arity}"
                            ),
                        ));
                    }
                    let cb_arity = if total_arity == 4 { 3 } else { 2 };
                    let (mut s_ty, src_expr, fn_expr, env_expr) = if let Some(in_ty) = prepended {
                        (in_ty, &args[0], &args[1], args.get(2))
                    } else {
                        let s0 = self.check_expr(&args[0], scopes, ctx)?;
                        (s0, &args[1], &args[2], args.get(3))
                    };
                    let step_cb = self.resolve_ho_callback(fn_expr, scopes, line, ctx)?;
                    if contains_unknown(&s_ty) {
                        if let Some(hint_s) = self.unique_ho_param_hint(&step_cb, cb_arity, 0) {
                            if types_compatible_with_annotation(&s_ty, &hint_s) {
                                s_ty = hint_s;
                            }
                        }
                    }
                    if s_ty == Type::Unit || contains_unknown(&s_ty) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("{op} initial state requires a concrete non-unit type"),
                        ));
                    }
                    if ctx.effect != Effect::QExt && s_ty.is_qext() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            line,
                            format!("{op} cannot carry QExt state outside quarantine Q_ext"),
                        ));
                    }
                    let src_ty = self.check_expr(src_expr, scopes, ctx)?;
                    let passed_elem = if op == "scan"
                        && matches!(src_ty.unwrap_read_only(), Type::Range | Type::Window)
                    {
                        Type::Int
                    } else {
                        let elem_hint = self.unique_ho_param_hint(&step_cb, cb_arity, 1);
                        self.extract_afc_seq_elem(op, &src_ty, elem_hint, line)?.1
                    };
                    let env_ty = self.check_opt_env_expr(op, env_expr, line, scopes, ctx)?;
                    let mut candidates = vec![s_ty.clone()];
                    let decayed = decay_top_array_to_list(&s_ty);
                    if decayed != s_ty {
                        candidates.push(decayed);
                    }
                    let mut last_err = None;
                    for cand_s in candidates {
                        let mut cb_args = vec![cand_s.clone(), passed_elem.clone()];
                        if let Some(ref et) = env_ty {
                            cb_args.push(et.clone());
                        }
                        match self.dispatch_call_target_direct(&step_cb, &cb_args, line, ctx) {
                            Ok(step_ret) => {
                                let final_s = if step_ret == cand_s
                                    || (!cand_s.is_read_only()
                                        && !step_ret.is_read_only()
                                        && types_compatible_with_annotation(&step_ret, &cand_s))
                                {
                                    cand_s
                                } else {
                                    return Err(Diagnostic::at_line(
                                        DiagnosticCode::TypeError,
                                        line,
                                        format!(
                                            "{op} step function must return state type {cand_s}, got {step_ret}"
                                        ),
                                    ));
                                };
                                return Ok(if op == "scan" {
                                    Type::List(Box::new(final_s))
                                } else {
                                    final_s
                                });
                            }
                            Err(e) => last_err = Some(e),
                        }
                    }
                    return Err(last_err.unwrap());
                }
                ("map_seq", _) => {
                    if total_arity != 2 && total_arity != 3 {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!(
                                "map_seq(seq, map_fn[, env]) expects 2 or 3 arguments, got {total_arity}"
                            ),
                        ));
                    }
                    let cb_arity = if total_arity == 3 { 2 } else { 1 };
                    let (seq_ty, fn_expr, env_expr) = if let Some(in_ty) = prepended {
                        (in_ty, &args[0], args.get(1))
                    } else {
                        let st = self.check_expr(&args[0], scopes, ctx)?;
                        (st, &args[1], args.get(2))
                    };
                    let map_cb = self.resolve_ho_callback(fn_expr, scopes, line, ctx)?;
                    let elem_hint = self.unique_ho_param_hint(&map_cb, cb_arity, 0);
                    let (_, passed_elem) =
                        self.extract_afc_seq_elem("map_seq", &seq_ty, elem_hint, line)?;
                    let env_ty = self.check_opt_env_expr("map_seq", env_expr, line, scopes, ctx)?;
                    let mut cb_args = vec![passed_elem];
                    if let Some(et) = env_ty {
                        cb_args.push(et);
                    }
                    let u_ty = self.dispatch_call_target_direct(&map_cb, &cb_args, line, ctx)?;
                    if u_ty == Type::Unit || contains_unknown(&u_ty) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            "map_seq callback must return a concrete non-unit type",
                        ));
                    }
                    if ctx.effect != Effect::QExt && u_ty.is_qext() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            line,
                            "map_seq cannot produce QExt elements outside quarantine Q_ext",
                        ));
                    }
                    return Ok(Type::List(Box::new(u_ty)));
                }
                ("filter_seq", _) => {
                    if total_arity != 2 && total_arity != 3 {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!(
                                "filter_seq(seq, pred_fn[, env]) expects 2 or 3 arguments, got {total_arity}"
                            ),
                        ));
                    }
                    let cb_arity = if total_arity == 3 { 2 } else { 1 };
                    let (seq_ty, fn_expr, env_expr) = if let Some(in_ty) = prepended {
                        (in_ty, &args[0], args.get(1))
                    } else {
                        let st = self.check_expr(&args[0], scopes, ctx)?;
                        (st, &args[1], args.get(2))
                    };
                    let pred_cb = self.resolve_ho_callback(fn_expr, scopes, line, ctx)?;
                    let seq_ro = seq_ty.is_read_only();
                    let elem_hint = self.unique_ho_param_hint(&pred_cb, cb_arity, 0);
                    let (raw_elem, passed_elem) =
                        self.extract_afc_seq_elem("filter_seq", &seq_ty, elem_hint, line)?;
                    let env_ty =
                        self.check_opt_env_expr("filter_seq", env_expr, line, scopes, ctx)?;
                    let mut cb_args = vec![passed_elem];
                    if let Some(et) = env_ty {
                        cb_args.push(et);
                    }
                    let pred_ret =
                        self.dispatch_call_target_direct(&pred_cb, &cb_args, line, ctx)?;
                    if pred_ret.is_qext() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            line,
                            "filter_seq predicate cannot branch on QExt<bool>",
                        ));
                    }
                    if *pred_ret.unwrap_read_only() != Type::Bool {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("filter_seq predicate must return bool, got {pred_ret}"),
                        ));
                    }
                    let out = Type::List(Box::new(raw_elem));
                    return Ok(if seq_ro { out.wrap_read_only() } else { out });
                }
                ("tabulate", _) => {
                    if total_arity != 2 && total_arity != 3 {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!(
                                "tabulate(dom, gen_fn[, env]) expects 2 or 3 arguments, got {total_arity}"
                            ),
                        ));
                    }
                    let (dom_ty, fn_expr, env_expr) = if let Some(in_ty) = prepended {
                        (in_ty, &args[0], args.get(1))
                    } else {
                        let dt = self.check_expr(&args[0], scopes, ctx)?;
                        (dt, &args[1], args.get(2))
                    };
                    let gen_cb = self.resolve_ho_callback(fn_expr, scopes, line, ctx)?;
                    if !matches!(dom_ty.unwrap_read_only(), Type::Range | Type::Window) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("tabulate domain must be range or window, got {dom_ty}"),
                        ));
                    }
                    let env_ty =
                        self.check_opt_env_expr("tabulate", env_expr, line, scopes, ctx)?;
                    let mut cb_args = vec![Type::Int];
                    if let Some(et) = env_ty {
                        cb_args.push(et);
                    }
                    let u_ty = self.dispatch_call_target_direct(&gen_cb, &cb_args, line, ctx)?;
                    if u_ty == Type::Unit || contains_unknown(&u_ty) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            "tabulate callback must return a concrete non-unit type",
                        ));
                    }
                    if ctx.effect != Effect::QExt && u_ty.is_qext() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            line,
                            "tabulate cannot produce QExt elements outside quarantine Q_ext",
                        ));
                    }
                    return Ok(Type::List(Box::new(u_ty)));
                }
                ("count", _) => {
                    if total_arity != 2 && total_arity != 3 {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!(
                                "count(seq, pred_fn[, env]) expects 2 or 3 arguments, got {total_arity}"
                            ),
                        ));
                    }
                    let cb_arity = if total_arity == 3 { 2 } else { 1 };
                    let (seq_ty, fn_expr, env_expr) = if let Some(in_ty) = prepended {
                        (in_ty, &args[0], args.get(1))
                    } else {
                        let st = self.check_expr(&args[0], scopes, ctx)?;
                        (st, &args[1], args.get(2))
                    };
                    let pred_cb = self.resolve_ho_callback(fn_expr, scopes, line, ctx)?;
                    if seq_ty.is_qext() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            line,
                            "count cannot convert QExt sequence into native int",
                        ));
                    }
                    let elem_hint = self.unique_ho_param_hint(&pred_cb, cb_arity, 0);
                    let (_, passed_elem) =
                        self.extract_afc_seq_elem("count", &seq_ty, elem_hint, line)?;
                    let env_ty = self.check_opt_env_expr("count", env_expr, line, scopes, ctx)?;
                    if env_ty.as_ref().is_some_and(Type::is_qext) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            line,
                            "count cannot accept QExt env to produce native int",
                        ));
                    }
                    let mut cb_args = vec![passed_elem];
                    if let Some(et) = env_ty {
                        cb_args.push(et);
                    }
                    let pred_ret =
                        self.dispatch_call_target_direct(&pred_cb, &cb_args, line, ctx)?;
                    if pred_ret.is_qext() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            line,
                            "count predicate cannot return QExt<bool> to produce native int",
                        ));
                    }
                    if *pred_ret.unwrap_read_only() != Type::Bool {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("count predicate must return bool, got {pred_ret}"),
                        ));
                    }
                    return Ok(Type::Int);
                }
                (op @ ("any" | "all"), _) => {
                    if total_arity != 2 && total_arity != 3 {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!(
                                "{op}(seq, pred_fn[, env]) expects 2 or 3 arguments, got {total_arity}"
                            ),
                        ));
                    }
                    let cb_arity = if total_arity == 3 { 2 } else { 1 };
                    let (seq_ty, fn_expr, env_expr) = if let Some(in_ty) = prepended {
                        (in_ty, &args[0], args.get(1))
                    } else {
                        let st = self.check_expr(&args[0], scopes, ctx)?;
                        (st, &args[1], args.get(2))
                    };
                    let pred_cb = self.resolve_ho_callback(fn_expr, scopes, line, ctx)?;
                    let elem_hint = self.unique_ho_param_hint(&pred_cb, cb_arity, 0);
                    let (_, passed_elem) =
                        self.extract_afc_seq_elem(op, &seq_ty, elem_hint, line)?;
                    let env_ty = self.check_opt_env_expr(op, env_expr, line, scopes, ctx)?;
                    let mut cb_args = vec![passed_elem];
                    if let Some(ref et) = env_ty {
                        cb_args.push(et.clone());
                    }
                    let pred_ret =
                        self.dispatch_call_target_direct(&pred_cb, &cb_args, line, ctx)?;
                    let out_ty = if pred_ret == Type::QExt(Box::new(Type::Bool)) {
                        if ctx.effect != Effect::QExt {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::QExtLeakageForbidden,
                                line,
                                format!(
                                    "{op} with QExt<bool> predicate requires quarantine Q_ext context"
                                ),
                            ));
                        }
                        Type::QExt(Box::new(Type::Bool))
                    } else if *pred_ret.unwrap_read_only() == Type::Bool {
                        if (seq_ty.is_qext() || env_ty.as_ref().is_some_and(Type::is_qext))
                            && ctx.effect != Effect::QExt
                        {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::QExtLeakageForbidden,
                                line,
                                format!("{op} on QExt inputs requires quarantine Q_ext context"),
                            ));
                        }
                        Type::Bool
                    } else {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("{op} predicate must return bool or QExt<bool>, got {pred_ret}"),
                        ));
                    };
                    self.poly_call_types.insert(
                        (
                            ctx.module_key.clone(),
                            line,
                            format!("{op}:{callee:?}:{args:?}"),
                        ),
                        out_ty.clone(),
                    );
                    return Ok(out_ty);
                }
                _ => {}
            }
        }

        // Evaluate regular positional argument types
        let mut arg_tys = Vec::with_capacity(total_arity);
        if let Some(in_ty) = prepended {
            arg_tys.push(in_ty);
        }
        for a in args {
            let mut ty = self.check_expr(a, scopes, ctx)?;
            if matches!(simple_name, Some("dot" | "zip_seq"))
                && matches!(a.kind, ExprKind::SeqLit(_))
            {
                if let Type::Array(elem, _) = ty {
                    ty = Type::List(elem);
                }
            }
            arg_tys.push(ty);
        }

        // 2. Polymorphic selection & sequence builtins
        if let Some(cname) = simple_name {
            if let Some(poly_ret) = self.try_check_polymorphic_builtin(cname, &arg_tys, line, ctx)? {
                if matches!(cname, "sum" | "dot") {
                    self.poly_call_types.insert(
                        (
                            ctx.module_key.clone(),
                            line,
                            format!("{cname}:{callee:?}:{args:?}"),
                        ),
                        poly_ret.clone(),
                    );
                }
                return Ok(poly_ret);
            }
        }

        // 3. Enum payload constructor `Enum::Variant(args...)` or `mod::Enum::Variant(args...)`
        if let ExprKind::Path(ref segs) = callee.kind {
            if (2..=4).contains(&segs.len()) {
                if let Ok((edef, payload_tys)) =
                    self.resolve_enum_variant(&ctx.module_key, segs, line)
                {
                    if arg_tys.len() != payload_tys.len() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!(
                                "variant {} expects {} argument(s), got {}",
                                segs.join("::"),
                                payload_tys.len(),
                                arg_tys.len()
                            ),
                        ));
                    }
                    for (actual, expected) in arg_tys.iter().zip(payload_tys) {
                        enforce_struct_or_enum_field_arg(
                            actual,
                            expected,
                            line,
                            &format!("enum variant {}", segs.join("::")),
                        )?;
                    }
                    return Ok(Type::Enum {
                        module: edef.module.clone(),
                        name: edef.name.clone(),
                    });
                }
            }
        }

        // 4. Positional struct constructor `Foo(e1, e2, ...)` or `mod::Foo(...)`
        let struct_path_opt: Option<Vec<String>> = match &callee.kind {
            ExprKind::Ident(n) => Some(vec![n.clone()]),
            ExprKind::Path(segs) => Some(segs.clone()),
            _ => None,
        };
        if let Some(ref sp) = struct_path_opt {
            if let Ok(sdef) = self.resolve_struct_path(&ctx.module_key, sp, line).cloned() {
                if self.has_user_fn_for_struct_path(&ctx.module_key, sp) {
                    if arg_tys.len() == sdef.fields.len() {
                        let mut all_ok = true;
                        for ((fname, fty), actual) in sdef.fields.iter().zip(&arg_tys) {
                            match enforce_struct_or_enum_field_arg(
                                actual,
                                fty,
                                line,
                                &format!("struct {} field {fname}", sdef.name),
                            ) {
                                Ok(()) => {}
                                Err(e)
                                    if e.code == DiagnosticCode::ObserverMutationForbidden =>
                                {
                                    return Err(e);
                                }
                                Err(_) => {
                                    all_ok = false;
                                    break;
                                }
                            }
                        }
                        if all_ok {
                            return Ok(Type::Struct {
                                module: sdef.module,
                                name: sdef.name,
                            });
                        }
                    }
                    self.poly_call_types.insert(
                        (
                            ctx.module_key.clone(),
                            line,
                            format!("struct_ov:{callee:?}:{args:?}"),
                        ),
                        Type::Struct {
                            module: sdef.module,
                            name: sdef.name,
                        },
                    );
                } else {
                    if arg_tys.len() != sdef.fields.len() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!(
                                "struct {} expects {} positional argument(s), got {}",
                                sdef.name,
                                sdef.fields.len(),
                                arg_tys.len()
                            ),
                        ));
                    }
                    for ((fname, fty), actual) in sdef.fields.iter().zip(&arg_tys) {
                        enforce_struct_or_enum_field_arg(
                            actual,
                            fty,
                            line,
                            &format!("struct {} field {fname}", sdef.name),
                        )?;
                    }
                    return Ok(Type::Struct {
                        module: sdef.module,
                        name: sdef.name,
                    });
                }
            }
        }

        // 5. Resolve Builtin or UserFn target and dispatch (with automatic rank-lift fallback!)
        let target = self.resolve_call_target(callee, line, ctx)?;
        self.dispatch_call_target(&target, &arg_tys, line, ctx)
    }

    fn try_check_polymorphic_builtin(
        &self,
        name: &str,
        arg_tys: &[Type],
        line: usize,
        ctx: &CheckContext,
    ) -> DecResult<Option<Type>> {
        if name == "array" {
            if arg_tys.is_empty() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    line,
                    "array(...) requires at least 1 element",
                ));
            }
            let first = arg_tys[0].clone();
            if first == Type::Unit || contains_unknown(&first) {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    line,
                    "array(...) elements must have a concrete non-unit type",
                ));
            }
            if ctx.effect != Effect::QExt && first.is_qext() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::QExtLeakageForbidden,
                    line,
                    "QExt elements are forbidden in native array(...)",
                ));
            }
            for other in &arg_tys[1..] {
                if other != &first {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ImplicitCoercionForbidden,
                        line,
                        format!(
                            "array(...) requires homogeneous elements: expected {first}, got {other}"
                        ),
                    ));
                }
            }
            return Ok(Some(Type::Array(Box::new(first), arg_tys.len())));
        }

        match (name, arg_tys) {
            ("mux", [role, on_neg, on_zero, on_pos]) => {
                return Ok(Some(check_mux_types(role, on_neg, on_zero, on_pos, line)?));
            }
            ("select", [cond, on_true, on_false]) => {
                return Ok(Some(check_select_types(cond, on_true, on_false, line, ctx)?));
            }
            ("gate", [cond, val]) => {
                return Ok(Some(check_gate_types(cond, val, line)?));
            }
            ("argmin" | "argmax", [xs]) => {
                let elem = match xs.unwrap_read_only() {
                    Type::Array(e, _) | Type::List(e) => e.unwrap_read_only(),
                    other => {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("{name} expects a sequence, got {other}"),
                        ));
                    }
                };
                if !elem.is_ordered() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("{name} requires ordered elements, got {elem}"),
                    ));
                }
                return Ok(Some(Type::Int));
            }
            ("len", [xs]) => match xs.unwrap_read_only() {
                Type::Bytes | Type::Array(_, _) | Type::List(_) | Type::Map(_, _) => {
                    return Ok(Some(Type::Int));
                }
                _ => {}
            },
            ("contains", [container, key]) => match container.unwrap_read_only() {
                Type::Map(k_ty, _) => {
                    if types_compatible_with_annotation(key.unwrap_read_only(), k_ty) {
                        return Ok(Some(Type::Bool));
                    }
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ImplicitCoercionForbidden,
                        line,
                        format!("contains(map, key) expects key type {k_ty}, got {key}"),
                    ));
                }
                Type::Array(e_ty, _) | Type::List(e_ty) => {
                    if types_compatible_with_annotation(key.unwrap_read_only(), e_ty) {
                        return Ok(Some(Type::Bool));
                    }
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ImplicitCoercionForbidden,
                        line,
                        format!("contains(seq, item) expects element type {e_ty}, got {key}"),
                    ));
                }
                _ => {}
            },
            ("bytes", [arg]) => {
                if let Type::Array(elem, _) = arg.unwrap_read_only() {
                    if **elem == Type::B8 {
                        return Ok(Some(Type::Bytes));
                    }
                }
            }
            ("zip_seq", [a_ty, b_ty]) => {
                if let (Type::Array(_, na), Type::Array(_, nb)) =
                    (a_ty.unwrap_read_only(), b_ty.unwrap_read_only())
                {
                    if na != nb {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("zip_seq array length mismatch: {na} != {nb}"),
                        ));
                    }
                }
                let (_, elem_a) = self.extract_afc_seq_elem("zip_seq", a_ty, None, line)?;
                let (_, elem_b) = self.extract_afc_seq_elem("zip_seq", b_ty, None, line)?;
                if ctx.effect != Effect::QExt && (elem_a.is_qext() || elem_b.is_qext()) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        line,
                        "zip_seq cannot hold QExt elements outside quarantine Q_ext",
                    ));
                }
                return Ok(Some(Type::List(Box::new(Type::Tuple(vec![elem_a, elem_b])))));
            }
            ("sum", [seq_ty]) => {
                let elem_ty = match seq_ty.unwrap_read_only() {
                    Type::Bytes => Type::B8,
                    Type::List(elem) | Type::Array(elem, _) => (**elem).unwrap_read_only().clone(),
                    other => {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("sum(seq) expects [T], [T; N], or bytes, got {other}"),
                        ));
                    }
                };
                if elem_ty == Type::Unknown {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        "sum(...) on empty list requires an explicit element type annotation",
                    ));
                }
                if elem_ty == Type::QExt(Box::new(Type::Ratio)) {
                    if ctx.effect != Effect::QExt {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            line,
                            "sum(QExt<ratio>) requires quarantine Q_ext context",
                        ));
                    }
                } else if !matches!(elem_ty, Type::Int | Type::B8 | Type::T1 | Type::Ratio) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!(
                            "sum(...) requires sequence of int, b8, t1, or ratio, got {elem_ty}"
                        ),
                    ));
                }
                return Ok(Some(elem_ty));
            }
            ("dot", [a_ty, b_ty]) => {
                let a_unw = a_ty.unwrap_read_only();
                let b_unw = b_ty.unwrap_read_only();
                if contains_unknown(a_unw) || contains_unknown(b_unw) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        "dot(a, b) on empty list requires an explicit element type annotation",
                    ));
                }
                if let (Type::Array(_, na), Type::Array(_, nb)) = (a_unw, b_unw) {
                    if na != nb {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("dot(a, b) array length mismatch: {na} != {nb}"),
                        ));
                    }
                }
                let extract_elem = |t: &Type| -> DecResult<Type> {
                    match t {
                        Type::Bytes => Ok(Type::B8),
                        Type::List(e) | Type::Array(e, _) => Ok((**e).unwrap_read_only().clone()),
                        other => Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("dot(a, b) expects [T], [T; N], or bytes, got {other}"),
                        )),
                    }
                };
                let ea = extract_elem(a_unw)?;
                let eb = extract_elem(b_unw)?;
                if ea != eb {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ImplicitCoercionForbidden,
                        line,
                        format!("dot(a, b) requires matching element types, got {ea} vs {eb}"),
                    ));
                }
                if ea == Type::QExt(Box::new(Type::Ratio)) {
                    if ctx.effect != Effect::QExt {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            line,
                            "dot(QExt<ratio>, QExt<ratio>) requires quarantine Q_ext context",
                        ));
                    }
                } else if !matches!(ea, Type::Int | Type::B8 | Type::T1 | Type::Ratio) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("dot(a, b) requires elements of int, b8, t1, or ratio, got {ea}"),
                    ));
                }
                return Ok(Some(ea));
            }
            ("push" | "list_push", [xs, elem]) => {
                if xs.is_read_only() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ObserverMutationForbidden,
                        line,
                        format!("cannot call {name} on read-only sequence {xs}"),
                    ));
                }
                if name == "list_push" && !matches!(xs, Type::List(_)) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("list_push first argument must be [T], got {xs}"),
                    ));
                }
                let inner_e = match xs {
                    Type::Array(e, _) | Type::List(e) => (**e).clone(),
                    other => {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("{name} expects [T] or [T; N], got {other}"),
                        ));
                    }
                };
                if name == "list_push" && inner_e == Type::Unknown {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        "list_push on empty list requires an explicit [T] type annotation",
                    ));
                }
                let resolved_e = if inner_e == Type::Unknown {
                    elem.clone()
                } else {
                    enforce_struct_or_enum_field_arg(elem, &inner_e, line, name)?;
                    inner_e
                };
                return Ok(Some(Type::List(Box::new(resolved_e))));
            }
            ("concat" | "concat_seq", [a, b]) => {
                if name == "concat_seq" && (contains_unknown(a) || contains_unknown(b)) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        "concat_seq on empty sequence requires an explicit [T] type annotation",
                    ));
                }
                let is_ro = a.is_read_only() || b.is_read_only();
                let ea = match a.unwrap_read_only() {
                    Type::Array(e, _) | Type::List(e) => (**e).clone(),
                    other => {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("{name} expects sequence first argument, got {other}"),
                        ));
                    }
                };
                let eb = match b.unwrap_read_only() {
                    Type::Array(e, _) | Type::List(e) => (**e).clone(),
                    other => {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("{name} expects sequence second argument, got {other}"),
                        ));
                    }
                };
                let out_e = if ea == Type::Unknown {
                    eb
                } else if eb == Type::Unknown {
                    ea
                } else {
                    enforce_annotation_compatibility(&eb, &ea, line, name)?;
                    ea
                };
                let out = Type::List(Box::new(out_e));
                return Ok(Some(if is_ro { out.wrap_read_only() } else { out }));
            }
            ("slice" | "slice_seq", [xs, start, end]) => {
                if *start.unwrap_read_only() != Type::Int || *end.unwrap_read_only() != Type::Int {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("{name}(xs, start, end) requires int bounds"),
                    ));
                }
                if name == "slice_seq" && contains_unknown(xs) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        "slice_seq on empty sequence requires an explicit [T] type annotation",
                    ));
                }
                let is_ro = xs.is_read_only();
                let e = match xs.unwrap_read_only() {
                    Type::Array(e, _) | Type::List(e) => (**e).clone(),
                    other => {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("{name} expects [T] or [T; N], got {other}"),
                        ));
                    }
                };
                let out = Type::List(Box::new(e));
                return Ok(Some(if is_ro { out.wrap_read_only() } else { out }));
            }
            ("set_at" | "array_set" | "list_set", [xs, idx, val]) => {
                if xs.is_read_only() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ObserverMutationForbidden,
                        line,
                        format!("cannot call {name} on read-only sequence {xs}"),
                    ));
                }
                if *idx.unwrap_read_only() != Type::Int {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("{name} index must be int, got {idx}"),
                    ));
                }
                return match (name, xs) {
                    ("array_set", Type::Array(e, n)) | ("set_at", Type::Array(e, n)) => {
                        enforce_struct_or_enum_field_arg(val, e, line, name)?;
                        Ok(Some(Type::Array(e.clone(), *n)))
                    }
                    ("list_set", Type::List(e))
                    | ("list_set", Type::Array(e, _))
                    | ("set_at", Type::List(e)) => {
                        if **e == Type::Unknown {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("{name} on empty list requires an explicit [T] type annotation"),
                            ));
                        }
                        enforce_struct_or_enum_field_arg(val, e, line, name)?;
                        Ok(Some(Type::List(e.clone())))
                    }
                    (_, other) => Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("{name} unexpected sequence type {other}"),
                    )),
                };
            }
            ("map_put", [m, k, v]) => {
                if m.is_read_only() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ObserverMutationForbidden,
                        line,
                        format!("cannot call map_put on read-only map {m}"),
                    ));
                }
                return match m {
                    Type::Map(kt, vt) => {
                        let out_k = if **kt == Type::Unknown {
                            if !k.is_map_key() {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    line,
                                    format!("unsupported map key type {k}"),
                                ));
                            }
                            k.clone()
                        } else {
                            enforce_annotation_compatibility(k, kt, line, "map_put key")?;
                            (**kt).clone()
                        };
                        let out_v = if **vt == Type::Unknown {
                            v.clone()
                        } else {
                            enforce_struct_or_enum_field_arg(v, vt, line, "map_put value")?;
                            (**vt).clone()
                        };
                        Ok(Some(Type::Map(Box::new(out_k), Box::new(out_v))))
                    }
                    other => Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("map_put expects map[K, V], got {other}"),
                    )),
                };
            }
            ("map_remove", [m, k]) => {
                if m.is_read_only() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ObserverMutationForbidden,
                        line,
                        format!("cannot call map_remove on read-only map {m}"),
                    ));
                }
                return match m {
                    Type::Map(kt, vt) => {
                        if contains_unknown(m) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "map_remove on empty map requires an explicit map[K, V] type annotation",
                            ));
                        }
                        if !types_compatible_with_annotation(
                            k.unwrap_read_only(),
                            kt.unwrap_read_only(),
                        ) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ImplicitCoercionForbidden,
                                line,
                                format!("map_remove key expects {kt}, got {k}"),
                            ));
                        }
                        Ok(Some(Type::Map(kt.clone(), vt.clone())))
                    }
                    other => Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("map_remove expects map[K, V], got {other}"),
                    )),
                };
            }
            ("map_get_or", [m, k, default_v]) => {
                let is_ro = m.is_read_only();
                return match m.unwrap_read_only() {
                    Type::Map(kt, vt) => {
                        if contains_unknown(m) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "map_get_or on empty map requires an explicit map[K, V] type annotation",
                            ));
                        }
                        if !types_compatible_with_annotation(
                            k.unwrap_read_only(),
                            kt.unwrap_read_only(),
                        ) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ImplicitCoercionForbidden,
                                line,
                                format!("map_get_or key expects {kt}, got {k}"),
                            ));
                        }
                        let ret_ty = wrap_ro_if_needed((**vt).clone(), is_ro);
                        enforce_struct_or_enum_field_arg(
                            default_v,
                            &ret_ty,
                            line,
                            "map_get_or default",
                        )?;
                        Ok(Some(ret_ty))
                    }
                    other => Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("map_get_or expects map[K, V], got {other}"),
                    )),
                };
            }
            ("map_keys", [m]) => {
                let is_ro = m.is_read_only();
                return match m.unwrap_read_only() {
                    Type::Map(kt, _) => {
                        if contains_unknown(m) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "map_keys on empty map requires an explicit map[K, V] type annotation",
                            ));
                        }
                        let out = Type::List(Box::new((**kt).clone()));
                        Ok(Some(if is_ro { out.wrap_read_only() } else { out }))
                    }
                    other => Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("map_keys expects map[K, V], got {other}"),
                    )),
                };
            }
            ("map_entries", [m]) => {
                let is_ro = m.is_read_only();
                return match m.unwrap_read_only() {
                    Type::Map(kt, vt) => {
                        if contains_unknown(m) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "map_entries on empty map requires an explicit map[K, V] type annotation",
                            ));
                        }
                        let out = Type::List(Box::new(Type::Tuple(vec![
                            (**kt).clone(),
                            (**vt).clone(),
                        ])));
                        Ok(Some(if is_ro { out.wrap_read_only() } else { out }))
                    }
                    other => Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("map_entries expects map[K, V], got {other}"),
                    )),
                };
            }
            _ => {}
        }
        Ok(None)
    }

    fn check_step_combinator_fn(
        &mut self,
        init_ty: &Type,
        fn_expr: &Expr,
        is_settle: bool,
        line: usize,
        scopes: &[BTreeMap<String, VarBinding>],
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        let target = self.resolve_ho_callback(fn_expr, scopes, line, ctx)?;
        // Try 2-arg `(state, tick: int)` first, then 1-arg `(state)`.
        // Also allow `init_ty` `[T; N]` to decay to `[T]` if `step_fn` expects `[T]`!
        let mut candidates = vec![init_ty.clone()];
        let decayed = decay_top_array_to_list(init_ty);
        if decayed != *init_ty {
            candidates.push(decayed);
        }

        let mut last_err = None;
        for cand_s in candidates {
            let try_two = self.dispatch_call_target(
                &target,
                &[cand_s.clone(), Type::Int],
                line,
                ctx,
            );
            if let Ok(ret) = try_two {
                let unified = if ret == cand_s {
                    cand_s
                } else if types_compatible_with_annotation(&ret, &cand_s) {
                    cand_s
                } else if types_compatible_with_annotation(&cand_s, &ret) {
                    ret
                } else {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!(
                            "{} step function state mismatch: takes {cand_s}, returns {ret}",
                            if is_settle { "settle" } else { "walk" }
                        ),
                    ));
                };
                return Ok(unified);
            }
            let try_one =
                self.dispatch_call_target(&target, std::slice::from_ref(&cand_s), line, ctx);
            match try_one {
                Ok(ret) => {
                    let unified = if ret == cand_s {
                        cand_s
                    } else if types_compatible_with_annotation(&ret, &cand_s) {
                        cand_s
                    } else if types_compatible_with_annotation(&cand_s, &ret) {
                        ret
                    } else {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!(
                                "{} step function state mismatch: takes {cand_s}, returns {ret}",
                                if is_settle { "settle" } else { "walk" }
                            ),
                        ));
                    };
                    return Ok(unified);
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            Diagnostic::at_line(
                DiagnosticCode::TypeError,
                line,
                "invalid step function in walk/settle",
            )
        }))
    }

    fn check_fold_combinator_fn(
        &mut self,
        init_ty: &Type,
        elem_ty: &Type,
        fn_expr: &Expr,
        line: usize,
        scopes: &[BTreeMap<String, VarBinding>],
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        let target = self.resolve_ho_callback(fn_expr, scopes, line, ctx)?;
        let mut candidates = vec![init_ty.clone()];
        let decayed = decay_top_array_to_list(init_ty);
        if decayed != *init_ty {
            candidates.push(decayed);
        }
        let mut last_err = None;
        for cand_acc in candidates {
            match self.dispatch_call_target(&target, &[cand_acc.clone(), elem_ty.clone()], line, ctx)
            {
                Ok(ret) => {
                    if types_compatible_with_annotation(&ret, &cand_acc) {
                        return Ok(cand_acc);
                    }
                    if types_compatible_with_annotation(&cand_acc, &ret) {
                        return Ok(ret);
                    }
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!(
                            "fold/scan accumulator mismatch: initialized with {cand_acc}, step returned {ret}"
                        ),
                    ));
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap())
    }

    fn check_unary_fn_expr(
        &mut self,
        elem_ty: &Type,
        fn_expr: &Expr,
        line: usize,
        scopes: &[BTreeMap<String, VarBinding>],
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        let target = self.resolve_ho_callback(fn_expr, scopes, line, ctx)?;
        self.dispatch_call_target(&target, std::slice::from_ref(elem_ty), line, ctx)
    }

    fn resolve_call_target(
        &self,
        callee: &Expr,
        line: usize,
        ctx: &CheckContext,
    ) -> DecResult<CallTarget> {
        let base_mod = self.base_module_of(&ctx.module_key).to_string();
        match &callee.kind {
            ExprKind::Ident(name) => {
                if matches!(
                    name.as_str(),
                    "ext_decimal"
                        | "qext_from_ratio"
                        | "qext_add"
                        | "qext_sub"
                        | "qext_mul"
                        | "qext_div"
                        | "qext_render"
                        | "qext_round_half_even"
                        | "qext_format_fixed"
                        | "qext_to_bytes_exact"
                        | "qext_format_bytes_fixed"
                        | "qext_render_bytes"
                        | "qext_format_fixed_bytes"
                ) {
                    if ctx.effect != Effect::QExt {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            line,
                            format!(
                                "{name} belongs strictly to aod::qext and cannot be called outside quarantine Q_ext"
                            ),
                        ));
                    }
                    if let Some(ImportedSymbol::Builtin(canon)) =
                        self.imports_for(&ctx.module_key).get(name).cloned()
                    {
                        Ok(CallTarget::Builtin(canon))
                    } else {
                        Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            line,
                            format!(
                                "{name} must be explicitly imported from aod::qext (`use aod::qext::{name}` or `aod::qext::{name}`)"
                            ),
                        ))
                    }
                } else if matches!(
                    name.as_str(),
                    "cli_args" | "read_file_bytes" | "write_executable_elf"
                ) {
                    if let Some(ImportedSymbol::Builtin(canon)) =
                        self.imports_for(&ctx.module_key).get(name).cloned()
                    {
                        Ok(CallTarget::Builtin(canon))
                    } else {
                        Err(Diagnostic::at_line(
                            DiagnosticCode::SysIoForbidden,
                            line,
                            format!(
                                "{name} belongs to capability-gated aod::sys and must be explicitly imported (`use aod::sys::{name}`)"
                            ),
                        ))
                    }
                } else if let Some(entry) = self
                    .functions
                    .get(&(ctx.module_key.clone(), name.clone()))
                    .cloned()
                {
                    Ok(CallTarget::UserFn(entry))
                } else if base_mod != ctx.module_key
                    && self
                        .functions
                        .contains_key(&(base_mod.clone(), name.clone()))
                {
                    Ok(CallTarget::UserFn(
                        self.functions[&(base_mod, name.clone())].clone(),
                    ))
                } else if let Some(ImportedSymbol::ModuleItem { module, item }) =
                    self.imports_for(&ctx.module_key).get(name).cloned()
                {
                    if let Some(entry) = self.functions.get(&(module.clone(), item.clone())).cloned()
                    {
                        Ok(CallTarget::UserFn(entry))
                    } else {
                        Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("imported symbol {module}::{item} is not a function"),
                        ))
                    }
                } else if let Some(ImportedSymbol::Builtin(canon)) =
                    self.imports_for(&ctx.module_key).get(name).cloned()
                {
                    Ok(CallTarget::Builtin(canon))
                } else if !lookup_builtin(name).is_empty() {
                    Ok(CallTarget::Builtin(name.clone()))
                } else {
                    let mut found_qext = None;
                    for (m, qm) in &self.qext_modules {
                        if m == &base_mod {
                            let sub = format!("{m}::{qm}");
                            if let Some(e) = self.functions.get(&(sub, name.clone())) {
                                found_qext = Some(e.clone());
                                break;
                            }
                        }
                    }
                    if let Some(e) = found_qext {
                        if ctx.effect != Effect::QExt {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::QExtLeakageForbidden,
                                line,
                                format!(
                                    "cannot call quarantine Q_ext function {name} outside quarantine Q_ext"
                                ),
                            ));
                        }
                        Ok(CallTarget::UserFn(e))
                    } else {
                        Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("undefined function {name}"),
                        ))
                    }
                }
            }
            ExprKind::Path(segs) => {
                if segs.len() == 3 && segs[0] == "aod" {
                    let ns = &segs[0..2];
                    let sym = &segs[2];
                    if let Some(allowed) = builtin_namespace_items(ns) {
                        if allowed.contains(&sym.as_str()) {
                            Ok(CallTarget::Builtin(canonical_builtin_name(ns, sym)))
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("unknown function {}", segs.join("::")),
                            ))
                        }
                    } else {
                        Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("unknown namespace {}", ns.join("::")),
                        ))
                    }
                } else if segs.len() == 2 {
                    let ns = &segs[0];
                    let sym = &segs[1];
                    if let Some(qsub) = self.resolve_qext_submodule(&ctx.module_key, ns) {
                        if let Some(entry) = self.functions.get(&(qsub, sym.clone())).cloned() {
                            Ok(CallTarget::UserFn(entry))
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("undefined function {ns}::{sym}"),
                            ))
                        }
                    } else if let Some(ImportedSymbol::ModuleNamespace(mod_key)) =
                        self.imports_for(&ctx.module_key).get(ns).cloned()
                    {
                        if let Some(rest) = mod_key.strip_prefix("aod::") {
                            let ns_vec = vec!["aod".to_string(), rest.to_string()];
                            if builtin_namespace_items(&ns_vec)
                                .is_some_and(|list| list.contains(&sym.as_str()))
                            {
                                Ok(CallTarget::Builtin(canonical_builtin_name(&ns_vec, sym)))
                            } else {
                                Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    line,
                                    format!("unknown builtin {mod_key}::{sym}"),
                                ))
                            }
                        } else if let Some(entry) =
                            self.functions.get(&(mod_key.clone(), sym.clone())).cloned()
                        {
                            Ok(CallTarget::UserFn(entry))
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("undefined function {mod_key}::{sym}"),
                            ))
                        }
                    } else {
                        Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("undefined function path {}", segs.join("::")),
                        ))
                    }
                } else if segs.len() == 3 {
                    if let Some(qsub) =
                        self.resolve_qext_submodule_3seg(&ctx.module_key, &segs[0], &segs[1])
                    {
                        if let Some(entry) = self.functions.get(&(qsub, segs[2].clone())).cloned() {
                            Ok(CallTarget::UserFn(entry))
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("undefined function {}", segs.join("::")),
                            ))
                        }
                    } else {
                        Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("unsupported call path {}", segs.join("::")),
                        ))
                    }
                } else {
                    Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("unsupported call path {}", segs.join("::")),
                    ))
                }
            }
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                line,
                "calls require a direct function name or module path",
            )),
        }
    }

    fn dispatch_call_target(
        &mut self,
        target: &CallTarget,
        raw_arg_tys: &[Type],
        line: usize,
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        match self.dispatch_call_target_direct(target, raw_arg_tys, line, ctx) {
            Ok(ret) => Ok(ret),
            Err(direct_err) => {
                // Do not rank-lift over firewall or shape-mismatch errors!
                if matches!(
                    direct_err.code,
                    DiagnosticCode::QExtLeakageForbidden
                        | DiagnosticCode::ObserverMutationForbidden
                        | DiagnosticCode::SysIoForbidden
                        | DiagnosticCode::ShapeMismatch
                ) {
                    return Err(direct_err);
                }
                // Automatic rank-lift fallback for unary & binary pure scalar functions!
                if raw_arg_tys.len() == 1 {
                    match raw_arg_tys[0].unwrap_read_only() {
                        Type::Array(e, n) => {
                            let elem_arg = wrap_ro_if_needed((**e).clone(), raw_arg_tys[0].is_read_only());
                            if let Ok(out) = self.dispatch_call_target(target, &[elem_arg], line, ctx) {
                                return Ok(Type::Array(Box::new(out), *n));
                            }
                        }
                        Type::List(e) => {
                            let elem_arg = wrap_ro_if_needed((**e).clone(), raw_arg_tys[0].is_read_only());
                            if let Ok(out) = self.dispatch_call_target(target, &[elem_arg], line, ctx) {
                                return Ok(Type::List(Box::new(out)));
                            }
                        }
                        Type::Range => {
                            if let Ok(out) = self.dispatch_call_target(target, &[Type::Int], line, ctx) {
                                return Ok(Type::List(Box::new(out)));
                            }
                        }
                        _ => {}
                    }
                } else if raw_arg_tys.len() == 2 {
                    let l = raw_arg_tys[0].unwrap_read_only();
                    let r = raw_arg_tys[1].unwrap_read_only();
                    let lr = sequence_rank(l);
                    let rr = sequence_rank(r);
                    if lr > 0 || rr > 0 {
                        return self.rank_lift_binary_call(target, &raw_arg_tys[0], &raw_arg_tys[1], line, ctx)
                            .map_err(|e| {
                                if e.code == DiagnosticCode::ShapeMismatch {
                                    e
                                } else {
                                    direct_err
                                }
                            });
                    }
                }
                Err(direct_err)
            }
        }
    }

    fn rank_lift_binary_call(
        &mut self,
        target: &CallTarget,
        lt: &Type,
        rt: &Type,
        line: usize,
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        let l = lt.unwrap_read_only();
        let r = rt.unwrap_read_only();
        let lr = sequence_rank(l);
        let rr = sequence_rank(r);
        if lr > rr {
            return match l {
                Type::Array(a, n) => {
                    let a_ro = wrap_ro_if_needed((**a).clone(), lt.is_read_only());
                    let out = self.dispatch_call_target(target, &[a_ro, rt.clone()], line, ctx)?;
                    Ok(Type::Array(Box::new(out), *n))
                }
                Type::List(a) => {
                    let a_ro = wrap_ro_if_needed((**a).clone(), lt.is_read_only());
                    let out = self.dispatch_call_target(target, &[a_ro, rt.clone()], line, ctx)?;
                    Ok(Type::List(Box::new(out)))
                }
                _ => unreachable!(),
            };
        }
        if rr > lr {
            return match r {
                Type::Array(b, n) => {
                    let b_ro = wrap_ro_if_needed((**b).clone(), rt.is_read_only());
                    let out = self.dispatch_call_target(target, &[lt.clone(), b_ro], line, ctx)?;
                    Ok(Type::Array(Box::new(out), *n))
                }
                Type::List(b) => {
                    let b_ro = wrap_ro_if_needed((**b).clone(), rt.is_read_only());
                    let out = self.dispatch_call_target(target, &[lt.clone(), b_ro], line, ctx)?;
                    Ok(Type::List(Box::new(out)))
                }
                _ => unreachable!(),
            };
        }
        match (l, r) {
            (Type::Array(a, n), Type::Array(b, m)) => {
                if n != m {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ShapeMismatch,
                        line,
                        format!("shape mismatch in rank-lifted call: {l} vs {r}"),
                    ));
                }
                let a_ro = wrap_ro_if_needed((**a).clone(), lt.is_read_only());
                let b_ro = wrap_ro_if_needed((**b).clone(), rt.is_read_only());
                let out = self.dispatch_call_target(target, &[a_ro, b_ro], line, ctx)?;
                Ok(Type::Array(Box::new(out), *n))
            }
            (Type::Array(a, _) | Type::List(a), Type::Array(b, _) | Type::List(b)) => {
                let a_ro = wrap_ro_if_needed((**a).clone(), lt.is_read_only());
                let b_ro = wrap_ro_if_needed((**b).clone(), rt.is_read_only());
                let out = self.dispatch_call_target(target, &[a_ro, b_ro], line, ctx)?;
                Ok(Type::List(Box::new(out)))
            }
            _ => unreachable!(),
        }
    }

    fn dispatch_call_target_direct(
        &mut self,
        target: &CallTarget,
        raw_arg_tys: &[Type],
        line: usize,
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        match target {
            CallTarget::Builtin(canon) => {
                let sigs = lookup_builtin(canon);
                if sigs.is_empty() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("undefined builtin {canon}"),
                    ));
                }
                let arg_tys: Vec<Type> = raw_arg_tys
                    .iter()
                    .map(|t| t.unwrap_read_only().clone())
                    .collect();

                if sigs.iter().all(|s| s.effect == Effect::QExt) && ctx.effect != Effect::QExt {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        line,
                        format!(
                            "builtin {canon} is quarantined in aod::qext and forbidden outside quarantine Q_ext"
                        ),
                    ));
                }
                if sigs.iter().all(|s| s.effect == Effect::SysIo) {
                    if ctx.effect == Effect::ReadOnlyObserver {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ObserverMutationForbidden,
                            line,
                            format!("aod::sys capability {canon} is forbidden inside read-only observer"),
                        ));
                    }
                    if ctx.effect == Effect::QExt {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            line,
                            format!("aod::sys capability {canon} is forbidden inside quarantine Q_ext"),
                        ));
                    }
                    if ctx.in_const || ctx.in_prepare {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::SysIoForbidden,
                            line,
                            format!("aod::sys capability {canon} is forbidden in const/prepare declarations"),
                        ));
                    }
                }

                let matching: Vec<_> = sigs
                    .into_iter()
                    .filter(|s| {
                        s.params.len() == arg_tys.len()
                            && arg_tys
                                .iter()
                                .zip(&s.params)
                                .all(|(a, p)| types_compatible_with_annotation(a, p))
                    })
                    .collect();
                if matching.len() != 1 {
                    let sig_str = arg_tys
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("unlisted signature {canon}({sig_str})"),
                    ));
                }
                let sig = &matching[0];
                if sig.effect == Effect::QExt && ctx.effect != Effect::QExt {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        line,
                        format!("builtin {canon} requires quarantine Q_ext context"),
                    ));
                }
                Ok(sig.ret.clone())
            }
            CallTarget::UserFn(entry) => {
                if entry.is_qext && ctx.effect != Effect::QExt {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        line,
                        format!(
                            "quarantine Q_ext function {} cannot be called outside quarantine Q_ext",
                            entry.decl.name
                        ),
                    ));
                }

                let mut matched_ret: Option<Type> = None;
                let mut blocked_by_readonly = false;
                let mut shape_err: Option<Diagnostic> = None;
                let mut needs_ro_recheck = false;

                for arm in &entry.decl.arms {
                    let arm_tys = self.infer_arm_param_types_for_entry(entry, arm)?;
                    if arm_tys.len() != raw_arg_tys.len() {
                        continue;
                    }
                    let mut arm_matches = true;
                    let mut arm_stripped_readonly = false;
                    for (p, a) in arm_tys.iter().zip(raw_arg_tys) {
                        if *p == Type::Unknown {
                            if a.is_read_only() {
                                needs_ro_recheck = true;
                            }
                            continue;
                        }
                        if let (Type::Array(ae, an), Type::Array(pe, pn)) =
                            (a.unwrap_read_only(), p.unwrap_read_only())
                        {
                            if ae == pe && an != pn {
                                shape_err = Some(Diagnostic::at_line(
                                    DiagnosticCode::ShapeMismatch,
                                    line,
                                    format!(
                                        "argument shape mismatch in call to {}: expected {p}, got {a}",
                                        entry.decl.name
                                    ),
                                ));
                                arm_matches = false;
                                break;
                            }
                        }
                        if types_compatible_with_annotation(a, p)
                            || (p.is_read_only()
                                && types_compatible_with_annotation(a, p.unwrap_read_only()))
                        {
                            continue;
                        }
                        if a.is_read_only()
                            && !p.is_read_only()
                            && types_compatible_with_annotation(a.unwrap_read_only(), p)
                        {
                            arm_stripped_readonly = true;
                        }
                        arm_matches = false;
                        break;
                    }
                    if arm_matches {
                        let rty = self.resolve_type(&entry.module_key, &arm.return_type, arm.line)?;
                        if let Some(ref prev) = matched_ret {
                            if prev != &rty {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    line,
                                    format!(
                                        "ambiguous return types ({prev} vs {rty}) across matching arms of {}",
                                        entry.decl.name
                                    ),
                                ));
                            }
                        } else {
                            matched_ret = Some(rty);
                        }
                    } else if arm_stripped_readonly {
                        blocked_by_readonly = true;
                    }
                }

                let ret_ty = match matched_ret {
                    Some(r) => r,
                    None if blocked_by_readonly => {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ObserverMutationForbidden,
                            line,
                            format!(
                                "cannot pass ReadOnly value to non-ReadOnly parameter of {}",
                                entry.decl.name
                            ),
                        ));
                    }
                    None if shape_err.is_some() => return Err(shape_err.unwrap()),
                    None => {
                        let sig_str = raw_arg_tys
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ");
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("unlisted signature {}({sig_str})", entry.decl.name),
                        ));
                    }
                };

                if matches!(ctx.effect, Effect::ReadOnlyObserver | Effect::QExt) {
                    self.check_transitive_effect(entry, raw_arg_tys, ctx.effect)?;
                } else if needs_ro_recheck {
                    self.check_transitive_effect(entry, raw_arg_tys, Effect::PureNative)?;
                }
                Ok(ret_ty)
            }
        }
    }

    fn check_transitive_effect(
        &mut self,
        entry: &FnEntry,
        arg_tys: &[Type],
        effect: Effect,
    ) -> DecResult<()> {
        let key = (
            entry.module_key.clone(),
            entry.decl.name.clone(),
            arg_tys.to_vec(),
            effect,
        );
        if !self.checked_transitive.insert(key) {
            return Ok(());
        }
        self.check_fn_entry(entry, Some(arg_tys), effect)
    }
}

fn definitely_returns(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| matches!(s.kind, StmtKind::Return(_)))
}

fn is_irrefutable_pattern(pat: &Pattern) -> bool {
    match pat {
        Pattern::Wildcard | Pattern::Binding(_) | Pattern::NullLit => true,
        Pattern::Tuple(ps) => ps.iter().all(is_irrefutable_pattern),
        _ => false,
    }
}

fn sequence_rank(ty: &Type) -> usize {
    match ty.unwrap_read_only() {
        Type::Array(inner, _) | Type::List(inner) => 1 + sequence_rank(inner),
        _ => 0,
    }
}

fn decay_top_array_to_list(ty: &Type) -> Type {
    match ty {
        Type::Array(inner, _) => Type::List(Box::new(decay_top_array_to_list(inner))),
        other => other.clone(),
    }
}

fn extract_seq_elem_and_len(
    seq_ty: &Type,
    cname: &str,
    line: usize,
) -> DecResult<(Type, Option<usize>)> {
    let is_ro = seq_ty.is_read_only();
    match seq_ty.unwrap_read_only() {
        Type::Array(e, n) => Ok((wrap_ro_if_needed((**e).clone(), is_ro), Some(*n))),
        Type::List(e) => Ok((wrap_ro_if_needed((**e).clone(), is_ro), None)),
        Type::Range => Ok((Type::Int, None)),
        other => Err(Diagnostic::at_line(
            DiagnosticCode::TypeError,
            line,
            format!("{cname} expects a sequence or range, got {other}"),
        )),
    }
}

fn needs_ro_propagation(ty: &Type) -> bool {
    match ty {
        Type::Array(_, _)
        | Type::List(_)
        | Type::Map(_, _)
        | Type::Struct { .. }
        | Type::ChamberView
        | Type::SimResult => true,
        Type::Tuple(elems) => elems.iter().any(needs_ro_propagation),
        _ => false,
    }
}

fn wrap_ro_if_needed(ty: Type, is_ro: bool) -> Type {
    if is_ro && needs_ro_propagation(&ty) {
        ty.wrap_read_only()
    } else {
        ty
    }
}

fn check_same_rank_sequence_eq(
    l: &Type,
    r: &Type,
    line: usize,
    ctx: &CheckContext,
) -> DecResult<Type> {
    match (l, r) {
        (Type::Array(a, n), Type::Array(b, m)) => {
            if n != m {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ShapeMismatch,
                    line,
                    format!("array equality shape mismatch: {l} vs {r}"),
                ));
            }
            check_same_rank_sequence_eq_elem(a, b, line, ctx)
        }
        (Type::Array(a, _) | Type::List(a), Type::Array(b, _) | Type::List(b)) => {
            check_same_rank_sequence_eq_elem(a, b, line, ctx)
        }
        _ => unreachable!(),
    }
}

fn check_same_rank_sequence_eq_elem(
    a: &Type,
    b: &Type,
    line: usize,
    ctx: &CheckContext,
) -> DecResult<Type> {
    let ar = sequence_rank(a);
    let br = sequence_rank(b);
    if ar > 0 && ar == br {
        return check_same_rank_sequence_eq(a, b, line, ctx);
    }
    if *a == Type::Unknown || *b == Type::Unknown || a == b {
        if a.is_qext() || b.is_qext() {
            if ctx.effect != Effect::QExt {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::QExtLeakageForbidden,
                    line,
                    "cannot compare QExt sequences outside quarantine Q_ext",
                ));
            }
            Ok(Type::QExt(Box::new(Type::Bool)))
        } else {
            Ok(Type::Bool)
        }
    } else {
        Err(Diagnostic::at_line(
            DiagnosticCode::ImplicitCoercionForbidden,
            line,
            format!("cannot compare sequences of mismatched element types {a} and {b}"),
        ))
    }
}

fn unify_branch_types(a: &Type, b: &Type, line: usize) -> DecResult<Type> {
    if a == b {
        return Ok(a.clone());
    }
    if types_compatible_with_annotation(a, b) {
        return Ok(b.clone());
    }
    if types_compatible_with_annotation(b, a) {
        return Ok(a.clone());
    }
    if let (Type::Array(ae, an), Type::Array(be, bn)) = (a, b) {
        if ae == be && an != bn {
            // Two match arms returning different fixed lengths `[T; N]` vs `[T; M]` unify cleanly to `[T]`!
            return Ok(Type::List(ae.clone()));
        }
    }
    Err(Diagnostic::at_line(
        DiagnosticCode::ImplicitCoercionForbidden,
        line,
        format!("branch result types must match, got {a} vs {b}"),
    ))
}

fn check_mux_types(
    role: &Type,
    on_neg: &Type,
    on_zero: &Type,
    on_pos: &Type,
    line: usize,
) -> DecResult<Type> {
    let r = role.unwrap_read_only();
    match r {
        Type::T1 => {
            let u1 = unify_branch_types(on_neg, on_zero, line)?;
            unify_branch_types(&u1, on_pos, line)
        }
        Type::Array(e, n) => {
            let cr = seq_rank_chk(r);
            let b_neg = extract_broadcast_branch_ranked(on_neg, cr, Some(*n), line)?;
            let b_zero = extract_broadcast_branch_ranked(on_zero, cr, Some(*n), line)?;
            let b_pos = extract_broadcast_branch_ranked(on_pos, cr, Some(*n), line)?;
            let inner = check_mux_types(e, &b_neg, &b_zero, &b_pos, line)?;
            Ok(Type::Array(Box::new(inner), *n))
        }
        Type::List(e) => {
            let cr = seq_rank_chk(r);
            let b_neg = extract_broadcast_branch_ranked(on_neg, cr, None, line)?;
            let b_zero = extract_broadcast_branch_ranked(on_zero, cr, None, line)?;
            let b_pos = extract_broadcast_branch_ranked(on_pos, cr, None, line)?;
            let inner = check_mux_types(e, &b_neg, &b_zero, &b_pos, line)?;
            Ok(Type::List(Box::new(inner)))
        }
        other => Err(Diagnostic::at_line(
            DiagnosticCode::TypeError,
            line,
            format!("mux selector must be t1, [t1; N], or [t1], got {other}"),
        )),
    }
}

fn check_select_types(
    cond: &Type,
    on_true: &Type,
    on_false: &Type,
    line: usize,
    ctx: &CheckContext,
) -> DecResult<Type> {
    let c = cond.unwrap_read_only();
    match c {
        Type::Bool => {
            let unified = unify_branch_types(on_true, on_false, line)?;
            if ctx.effect != Effect::QExt && unified.is_qext() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::QExtLeakageForbidden,
                    line,
                    "select cannot produce QExt value outside quarantine Q_ext",
                ));
            }
            Ok(unified)
        }
        Type::QExt(inner) if **inner == Type::Bool => {
            let unified = unify_branch_types(on_true, on_false, line)?;
            if ctx.effect != Effect::QExt || !unified.is_qext() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::QExtLeakageForbidden,
                    line,
                    "select with QExt<bool> condition requires quarantine Q_ext and QExt arm types",
                ));
            }
            Ok(unified)
        }
        Type::Array(e, n) => {
            let cr = seq_rank_chk(c);
            let bt = extract_broadcast_branch_ranked(on_true, cr, Some(*n), line)?;
            let bf = extract_broadcast_branch_ranked(on_false, cr, Some(*n), line)?;
            let inner = check_select_types(e, &bt, &bf, line, ctx)?;
            Ok(Type::Array(Box::new(inner), *n))
        }
        Type::List(e) => {
            let cr = seq_rank_chk(c);
            let bt = extract_broadcast_branch_ranked(on_true, cr, None, line)?;
            let bf = extract_broadcast_branch_ranked(on_false, cr, None, line)?;
            let inner = check_select_types(e, &bt, &bf, line, ctx)?;
            Ok(Type::List(Box::new(inner)))
        }
        other => Err(Diagnostic::at_line(
            DiagnosticCode::TypeError,
            line,
            format!("select condition must be bool, [bool; N], or [bool], got {other}"),
        )),
    }
}

fn check_gate_types(cond: &Type, val: &Type, line: usize) -> DecResult<Type> {
    let c = cond.unwrap_read_only();
    let v = val.unwrap_read_only();
    match (c, v) {
        (Type::Bool, t) if is_numeric_or_numeric_seq(t) => Ok(t.clone()),
        (Type::Array(ce, n), _) => {
            let cr = seq_rank_chk(c);
            let bv = extract_broadcast_branch_ranked(v, cr, Some(*n), line)?;
            let inner = check_gate_types(ce, &bv, line)?;
            Ok(Type::Array(Box::new(inner), *n))
        }
        (Type::List(ce), _) => {
            let cr = seq_rank_chk(c);
            let bv = extract_broadcast_branch_ranked(v, cr, None, line)?;
            let inner = check_gate_types(ce, &bv, line)?;
            Ok(Type::List(Box::new(inner)))
        }
        _ => Err(Diagnostic::at_line(
            DiagnosticCode::TypeError,
            line,
            format!("gate(cond, val) requires bool mask and numeric value, got ({cond}, {val})"),
        )),
    }
}

fn seq_rank_chk(ty: &Type) -> usize {
    match ty.unwrap_read_only() {
        Type::Array(inner, _) | Type::List(inner) => 1 + seq_rank_chk(inner),
        _ => 0,
    }
}

fn is_numeric_or_numeric_seq(ty: &Type) -> bool {
    match ty.unwrap_read_only() {
        t if t.is_numeric() => true,
        Type::Array(e, _) | Type::List(e) => is_numeric_or_numeric_seq(e),
        _ => false,
    }
}

fn extract_broadcast_branch_ranked(
    branch: &Type,
    cond_rank: usize,
    expected_len: Option<usize>,
    line: usize,
) -> DecResult<Type> {
    let br = seq_rank_chk(branch);
    if br < cond_rank {
        return Ok(branch.clone());
    }
    match (branch.unwrap_read_only(), expected_len) {
        (Type::Array(e, m), Some(n)) => {
            if *m != n {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ShapeMismatch,
                    line,
                    format!("selector branch shape mismatch: expected length {n}, got {m}"),
                ));
            }
            Ok((**e).clone())
        }
        (Type::Array(e, _), None) | (Type::List(e), _) => Ok((**e).clone()),
        (scalar, _) => Ok(scalar.clone()),
    }
}

pub fn types_compatible_with_annotation(actual: &Type, expected: &Type) -> bool {
    if actual == expected {
        return true;
    }
    match (actual, expected) {
        (Type::List(a), Type::List(_)) if **a == Type::Unknown => true,
        (Type::Array(a, n), Type::Array(e, m)) => {
            n == m && types_compatible_with_annotation(a, e)
        }
        (Type::Array(a, _), Type::List(e)) => types_compatible_with_annotation(a, e),
        (Type::List(a), Type::List(e)) => types_compatible_with_annotation(a, e),
        (Type::Map(ak, av), Type::Map(_, _))
            if **ak == Type::Unknown && **av == Type::Unknown =>
        {
            true
        }
        (Type::Tuple(as_), Type::Tuple(es)) if as_.len() == es.len() => as_
            .iter()
            .zip(es)
            .all(|(a, e)| types_compatible_with_annotation(a, e)),
        _ => false,
    }
}

fn enforce_annotation_compatibility(
    actual: &Type,
    expected: &Type,
    line: usize,
    ctx_label: &str,
) -> DecResult<()> {
    if types_compatible_with_annotation(actual, expected) {
        return Ok(());
    }
    if is_shape_mismatch(actual, expected) {
        return Err(Diagnostic::at_line(
            DiagnosticCode::ShapeMismatch,
            line,
            format!("{ctx_label} shape mismatch: expected {expected}, got {actual}"),
        ));
    }
    Err(Diagnostic::at_line(
        DiagnosticCode::ImplicitCoercionForbidden,
        line,
        format!("{ctx_label} declared as {expected}, got {actual}"),
    ))
}

fn enforce_return_compatibility(actual: &Type, expected: &Type, line: usize) -> DecResult<()> {
    if types_compatible_with_annotation(actual, expected) {
        return Ok(());
    }
    if is_shape_mismatch(actual, expected) {
        return Err(Diagnostic::at_line(
            DiagnosticCode::ShapeMismatch,
            line,
            format!("return shape mismatch: expected {expected}, got {actual}"),
        ));
    }
    Err(Diagnostic::at_line(
        DiagnosticCode::TypeError,
        line,
        format!("return type mismatch: expected {expected}, got {actual}"),
    ))
}

fn enforce_struct_or_enum_field_arg(
    actual: &Type,
    expected: &Type,
    line: usize,
    label: &str,
) -> DecResult<()> {
    if types_compatible_with_annotation(actual, expected) {
        return Ok(());
    }
    if actual.is_read_only()
        && !expected.is_read_only()
        && types_compatible_with_annotation(actual.unwrap_read_only(), expected)
    {
        return Err(Diagnostic::at_line(
            DiagnosticCode::ObserverMutationForbidden,
            line,
            format!("cannot strip ReadOnly from {actual} in {label}"),
        ));
    }
    if is_shape_mismatch(actual, expected) {
        return Err(Diagnostic::at_line(
            DiagnosticCode::ShapeMismatch,
            line,
            format!("{label} shape mismatch: expects {expected}, got {actual}"),
        ));
    }
    Err(Diagnostic::at_line(
        DiagnosticCode::ImplicitCoercionForbidden,
        line,
        format!("{label} expects {expected}, got {actual}"),
    ))
}

fn is_shape_mismatch(a: &Type, b: &Type) -> bool {
    match (a.unwrap_read_only(), b.unwrap_read_only()) {
        (Type::Array(ae, an), Type::Array(be, bn)) => {
            (an != bn && (ae == be || is_shape_mismatch(ae, be)))
                || (an == bn && is_shape_mismatch(ae, be))
        }
        (Type::List(ae), Type::Array(be, _)) if ae == be => true,
        _ => false,
    }
}

fn contains_unknown(ty: &Type) -> bool {
    match ty {
        Type::Unknown => true,
        Type::Array(inner, _) | Type::List(inner) | Type::ReadOnly(inner) | Type::QExt(inner) => {
            contains_unknown(inner)
        }
        Type::Map(k, v) => contains_unknown(k) || contains_unknown(v),
        Type::Tuple(elems) => elems.iter().any(contains_unknown),
        _ => false,
    }
}

fn find_var_scope_index(scopes: &[BTreeMap<String, VarBinding>], name: &str) -> Option<usize> {
    for (idx, scope) in scopes.iter().enumerate().rev() {
        if scope.contains_key(name) {
            return Some(idx);
        }
    }
    None
}

pub fn resolve_builtin_field_type(base: &Type, field: &str, line: usize) -> DecResult<Type> {
    match (base, field) {
        (Type::SimResult, "model_status") | (Type::ChamberView, "model_status") => Ok(Type::Enum {
            module: BUILTIN_MOD.to_string(),
            name: "ModelStatus".to_string(),
        }),
        (
            Type::SimResult,
            "chamber_name"
            | "chamber_name_bytes"
            | "final_state_hash"
            | "final_state_hash_bytes"
            | "final_state_sha256"
            | "model_status_bytes"
            | "boundary_bytes"
            | "state_json"
            | "shedding_witnesses_json",
        ) => Ok(Type::Bytes),
        (Type::Box3, "boundary_bytes") => Ok(Type::Bytes),
        (Type::SimResult, "ticks")
        | (Type::SimResult, "carrier_count")
        | (Type::SimResult, "active_carrier_count")
        | (Type::SimResult, "shedding_event_count")
        | (Type::SimResult, "wavelet_event_count")
        | (Type::SimResult, "boundary_crossings")
        | (Type::SimResult, "total_returns")
        | (Type::SimResult, "total_hinges")
        | (Type::SimResult, "total_shedding_events")
        | (Type::SimResult, "resonance_count")
        | (Type::SimResult, "expired_contact_count")
        | (Type::SimResult, "fusion_count")
        | (Type::SimResult, "moving_flux_total") => Ok(Type::Int),
        (Type::SimResult, "final_snapshot") => Ok(Type::ReadOnly(Box::new(Type::ChamberView))),

        (
            Type::ChamberView,
            "chamber_name"
            | "model_status_bytes"
            | "chamber_name_bytes"
            | "shedding_witnesses_json",
        ) => Ok(Type::Bytes),
        (Type::ChamberView, "tick")
        | (Type::ChamberView, "carrier_count")
        | (Type::ChamberView, "active_carrier_count")
        | (Type::ChamberView, "boundary_crossings")
        | (Type::ChamberView, "shedding_count")
        | (Type::ChamberView, "wavelet_count")
        | (Type::ChamberView, "clock_count")
        | (Type::ChamberView, "resonance_count")
        | (Type::ChamberView, "expired_contact_count")
        | (Type::ChamberView, "fusion_count") => Ok(Type::Int),
        (Type::ChamberView, "carriers") => {
            Ok(Type::ReadOnly(Box::new(Type::List(Box::new(Type::Int)))))
        }

        (Type::RecipientOutcome, "stage") => Ok(Type::Enum {
            module: BUILTIN_MOD.to_string(),
            name: "RecipientStage".to_string(),
        }),
        (Type::RecipientOutcome, "stage_bytes") => Ok(Type::Bytes),
        (Type::RecipientOutcome, "closure_phase_delta")
        | (Type::RecipientOutcome, "reclosure_bip_delta") => Ok(Type::Int),
        (Type::RecipientOutcome, "blocks_later_seating") => Ok(Type::Bool),

        (Type::CadenceAssociation, "is_cadence_ratio") => Ok(Type::Bool),
        (
            Type::CadenceAssociation,
            "wavelet_id" | "wavelet_id_bytes" | "reference_id" | "reference_id_bytes",
        ) => Ok(Type::Bytes),

        (Type::Support, "is_yro") | (Type::Support, "is_closure_certificate") => Ok(Type::Bool),
        (Type::Addr8, "is_first_anchor") => Ok(Type::Bool),
        (Type::Window, "start") | (Type::Window, "end") | (Type::Window, "len") => Ok(Type::Int),
        (Type::Ratio, "num") | (Type::Ratio, "den") => Ok(Type::Int),
        _ => Err(Diagnostic::at_line(
            DiagnosticCode::TypeError,
            line,
            format!("no field .{field} on type {base}"),
        )),
    }
}
