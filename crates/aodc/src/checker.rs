use std::collections::{BTreeMap, BTreeSet};

use crate::ast::{
    AdverbOp, BinaryOp, ChamberDecl, ConstDecl, DecArm, DecArmParam, DecFnDecl, EnumDecl, Expr,
    ExprKind, Item, LetPattern, MatchExprArm, ObserveDecl, Pattern, PipeStage, PrepareDecl,
    ProbeDecl, Program, Stmt, StmtKind, StructDecl, TypeExpr, UnaryOp,
};
use crate::diagnostics::{DecResult, Diagnostic, DiagnosticCode};
use crate::modules::{
    builtin_namespace_items, canonical_builtin_name, ImportedSymbol, LoadedUnit,
};
use crate::types::{is_reserved_builtin_name, lookup_builtin, Effect, Type};

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
    };

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

    Ok(CheckedUnit {
        unit,
        main_return_type: main_ret,
        test_functions,
        const_types,
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
}

enum CallTarget {
    Builtin(String),
    UserFn(FnEntry),
}

impl<'a> Checker<'a> {
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
        if let Some(ImportedSymbol::ModuleItem { module, item }) =
            self.imports_for(mod_key).get(name)
        {
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
                        return_type,
                        body,
                        line,
                        ..
                    } => {
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
        for arm in &entry.decl.arms {
            if arm.params.len() != arity {
                continue;
            }
            let tys = self.infer_arm_param_types(&entry.module_key, arm)?;
            for (i, t) in tys.into_iter().enumerate() {
                if t != Type::Unknown && hints[i].is_none() {
                    hints[i] = Some(t);
                }
            }
        }
        Ok(hints)
    }

    fn infer_arm_param_types(&self, mod_key: &str, arm: &DecArm) -> DecResult<Vec<Type>> {
        let mut out = Vec::with_capacity(arm.params.len());
        for p in &arm.params {
            let inferred = match p {
                DecArmParam::TypedVar { ty, .. } | DecArmParam::TypedPattern { ty, .. } => {
                    self.resolve_type(mod_key, ty, arm.line)?
                }
                DecArmParam::Pattern(pat) => self
                    .infer_pattern_type(mod_key, pat, arm.line)?
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
            Pattern::StrLit(_) => Ok(Some(Type::Str)),
            Pattern::BytesLit(_) => Ok(Some(Type::Bytes)),
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

        for arm in &entry.decl.arms {
            let slot_hints = self.slot_hints_for_arity(entry, arm.params.len())?;
            let arm_tys = self.infer_arm_param_types(&entry.module_key, arm)?;
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
                match param {
                    DecArmParam::TypedVar { name, .. } => {
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
                scope.insert(
                    name.clone(),
                    VarBinding {
                        ty: expected.clone(),
                    },
                );
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
            Pattern::IntLit(_) => {
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
            Pattern::StrLit(_) => {
                if *target != Type::Str {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("string pattern does not match {target}"),
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
                    let bound_ty = if expected.is_read_only() {
                        pty.clone().wrap_read_only()
                    } else {
                        pty.clone()
                    };
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
                        let bound_ty = if expected.is_read_only() {
                            t.clone().wrap_read_only()
                        } else {
                            t.clone()
                        };
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
            } else if let Some(ImportedSymbol::ModuleItem { module, item }) =
                self.imports_for(mod_key).get(&path[0])
            {
                (module.clone(), item.clone(), path[1].clone())
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
                (m.clone(), path[1].clone(), path[2].clone())
            } else {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    line,
                    format!("unknown module namespace {}", path[0]),
                ));
            }
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
                        if contains_unknown(&val_ty) {
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
                    LetPattern::Tuple(names) => {
                        let is_ro = final_ty.is_read_only();
                        let elem_tys = match final_ty.unwrap_read_only() {
                            Type::Tuple(ts) if ts.len() == names.len() => ts.clone(),
                            other => {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    stmt.line,
                                    format!(
                                        "cannot destructure {other} into {}-element tuple",
                                        names.len()
                                    ),
                                ));
                            }
                        };
                        let cur = scopes.last_mut().expect("scope stack non-empty");
                        for (n, et) in names.iter().zip(elem_tys) {
                            if cur.contains_key(n) {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    stmt.line,
                                    format!("duplicate local declaration {n}"),
                                ));
                            }
                            let bound_ty = if is_ro { et.wrap_read_only() } else { et };
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
            ExprKind::StrLit(_) => Ok(Type::Str),
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
                if segs.len() == 2 || segs.len() == 3 {
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
                Type::Box3 | Type::SimResult => Ok(Type::Str),
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

    fn check_call_with_prepended_input(
        &mut self,
        prepended: Option<Type>,
        callee: &Expr,
        args: &[Expr],
        line: usize,
        scopes: &[BTreeMap<String, VarBinding>],
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        let simple_name = match &callee.kind {
            ExprKind::Ident(n) => Some(n.as_str()),
            ExprKind::Path(segs) if segs.len() == 3 && segs[0] == "aod" && segs[1] == "core" => {
                Some(segs[2].as_str())
            }
            _ => None,
        };

        // 1. Higher-order pure combinators (`walk`, `settle`, `fold`, `scan`, `each`)
        //    Note: 4-arg `fold(trace, trace, str, str)` falls through to `aod::witness::fold`!
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
                        ctx,
                    )?;
                    return Ok(state_ty);
                }
                ("fold", 3) | ("scan", 3) => {
                    let is_scan = cname == "scan";
                    let (seq_ty, init_expr, fn_expr) = if let Some(in_ty) = prepended {
                        (in_ty, &args[0], &args[1])
                    } else {
                        let st = self.check_expr(&args[0], scopes, ctx)?;
                        (st, &args[1], &args[2])
                    };
                    let (elem_ty, fixed_len) = extract_seq_elem_and_len(&seq_ty, cname, line)?;
                    let init_ty = self.check_expr(init_expr, scopes, ctx)?;
                    let acc_ty = self.check_fold_combinator_fn(&init_ty, &elem_ty, fn_expr, line, ctx)?;
                    return Ok(if is_scan {
                        match fixed_len {
                            Some(n) => Type::Array(Box::new(acc_ty), n),
                            None => Type::List(Box::new(acc_ty)),
                        }
                    } else {
                        acc_ty
                    });
                }
                ("each", 2) => {
                    let (seq_ty, fn_expr) = if let Some(in_ty) = prepended {
                        (in_ty, &args[0])
                    } else {
                        let st = self.check_expr(&args[0], scopes, ctx)?;
                        (st, &args[1])
                    };
                    let (elem_ty, fixed_len) = extract_seq_elem_and_len(&seq_ty, "each", line)?;
                    let out_elem = self.check_unary_fn_expr(&elem_ty, fn_expr, line, ctx)?;
                    return Ok(match fixed_len {
                        Some(n) => Type::Array(Box::new(out_elem), n),
                        None => Type::List(Box::new(out_elem)),
                    });
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
            arg_tys.push(self.check_expr(a, scopes, ctx)?);
        }

        // 2. Polymorphic selection & sequence builtins
        if let Some(cname) = simple_name {
            if let Some(poly_ret) = self.try_check_polymorphic_builtin(cname, &arg_tys, line, ctx)? {
                return Ok(poly_ret);
            }
        }

        // 3. Enum payload constructor `Enum::Variant(args...)` or `mod::Enum::Variant(args...)`
        if let ExprKind::Path(ref segs) = callee.kind {
            if segs.len() == 2 || segs.len() == 3 {
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
                Type::Str | Type::Bytes | Type::Array(_, _) | Type::List(_) | Type::Map(_, _) => {
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
            ("push", [xs, elem]) => {
                let inner_e = match xs.unwrap_read_only() {
                    Type::Array(e, _) | Type::List(e) => (**e).clone(),
                    other => {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("push expects [T] or [T; N], got {other}"),
                        ));
                    }
                };
                let resolved_e = if inner_e == Type::Unknown {
                    elem.clone()
                } else {
                    enforce_annotation_compatibility(elem, &inner_e, line, "push")?;
                    inner_e
                };
                return Ok(Some(Type::List(Box::new(resolved_e))));
            }
            ("concat", [a, b]) => {
                let ea = match a.unwrap_read_only() {
                    Type::Array(e, _) | Type::List(e) => (**e).clone(),
                    other => {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("concat expects sequence first argument, got {other}"),
                        ));
                    }
                };
                let eb = match b.unwrap_read_only() {
                    Type::Array(e, _) | Type::List(e) => (**e).clone(),
                    other => {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("concat expects sequence second argument, got {other}"),
                        ));
                    }
                };
                let out_e = if ea == Type::Unknown {
                    eb
                } else if eb == Type::Unknown {
                    ea
                } else {
                    enforce_annotation_compatibility(&eb, &ea, line, "concat")?;
                    ea
                };
                return Ok(Some(Type::List(Box::new(out_e))));
            }
            ("slice", [xs, start, end]) => {
                if *start.unwrap_read_only() != Type::Int || *end.unwrap_read_only() != Type::Int {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        "slice(xs, start, end) requires int bounds",
                    ));
                }
                let e = match xs.unwrap_read_only() {
                    Type::Array(e, _) | Type::List(e) => (**e).clone(),
                    other => {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("slice expects [T] or [T; N], got {other}"),
                        ));
                    }
                };
                return Ok(Some(Type::List(Box::new(e))));
            }
            ("set_at", [xs, idx, val]) => {
                if xs.is_read_only() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ObserverMutationForbidden,
                        line,
                        format!("cannot call set_at on read-only sequence {xs}"),
                    ));
                }
                if *idx.unwrap_read_only() != Type::Int {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("set_at index must be int, got {idx}"),
                    ));
                }
                return match xs {
                    Type::Array(e, n) => {
                        enforce_annotation_compatibility(val, e, line, "set_at")?;
                        Ok(Some(Type::Array(e.clone(), *n)))
                    }
                    Type::List(e) => {
                        enforce_annotation_compatibility(val, e, line, "set_at")?;
                        Ok(Some(Type::List(e.clone())))
                    }
                    other => Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("set_at expects [T; N] or [T], got {other}"),
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
                            enforce_annotation_compatibility(v, vt, line, "map_put value")?;
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
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        let target = self.resolve_call_target(fn_expr, line, ctx)?;
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
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        let target = self.resolve_call_target(fn_expr, line, ctx)?;
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
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        let target = self.resolve_call_target(fn_expr, line, ctx)?;
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
                    let arm_tys = self.infer_arm_param_types(&entry.module_key, arm)?;
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
                        if ctx.effect == Effect::ReadOnlyObserver
                            && types_compatible_with_annotation(
                                a.unwrap_read_only(),
                                p.unwrap_read_only(),
                            )
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

fn wrap_ro_if_needed(ty: Type, is_ro: bool) -> Type {
    if is_ro
        && matches!(
            ty,
            Type::Array(_, _)
                | Type::List(_)
                | Type::Map(_, _)
                | Type::Struct { .. }
                | Type::ChamberView
                | Type::SimResult
        )
    {
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
        Type::Array(e, n) if **e == Type::T1 => {
            let b_neg = extract_broadcast_branch(on_neg, Some(*n), line)?;
            let b_zero = extract_broadcast_branch(on_zero, Some(*n), line)?;
            let b_pos = extract_broadcast_branch(on_pos, Some(*n), line)?;
            let u1 = unify_branch_types(&b_neg, &b_zero, line)?;
            let u2 = unify_branch_types(&u1, &b_pos, line)?;
            Ok(Type::Array(Box::new(u2), *n))
        }
        Type::List(e) if **e == Type::T1 => {
            let b_neg = extract_broadcast_branch(on_neg, None, line)?;
            let b_zero = extract_broadcast_branch(on_zero, None, line)?;
            let b_pos = extract_broadcast_branch(on_pos, None, line)?;
            let u1 = unify_branch_types(&b_neg, &b_zero, line)?;
            let u2 = unify_branch_types(&u1, &b_pos, line)?;
            Ok(Type::List(Box::new(u2)))
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
        Type::Bool => unify_branch_types(on_true, on_false, line),
        Type::QExt(inner) if ctx.effect == Effect::QExt && **inner == Type::Bool => {
            unify_branch_types(on_true, on_false, line)
        }
        Type::Array(e, n) if **e == Type::Bool => {
            let bt = extract_broadcast_branch(on_true, Some(*n), line)?;
            let bf = extract_broadcast_branch(on_false, Some(*n), line)?;
            let u = unify_branch_types(&bt, &bf, line)?;
            Ok(Type::Array(Box::new(u), *n))
        }
        Type::List(e) if **e == Type::Bool => {
            let bt = extract_broadcast_branch(on_true, None, line)?;
            let bf = extract_broadcast_branch(on_false, None, line)?;
            let u = unify_branch_types(&bt, &bf, line)?;
            Ok(Type::List(Box::new(u)))
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
        (Type::Bool, t) if t.is_numeric() => Ok(t.clone()),
        (Type::Bool, Type::Array(e, n)) if e.is_numeric() => Ok(Type::Array(e.clone(), *n)),
        (Type::Bool, Type::List(e)) if e.is_numeric() => Ok(Type::List(e.clone())),
        (Type::Array(ce, n), t) if **ce == Type::Bool && t.is_numeric() => {
            Ok(Type::Array(Box::new(t.clone()), *n))
        }
        (Type::Array(ce, n), Type::Array(ve, m)) if **ce == Type::Bool && ve.is_numeric() => {
            if n != m {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ShapeMismatch,
                    line,
                    format!("gate shape mismatch: [{ce}; {n}] vs [{ve}; {m}]"),
                ));
            }
            Ok(Type::Array(ve.clone(), *n))
        }
        (Type::List(ce), t) if **ce == Type::Bool && t.is_numeric() => {
            Ok(Type::List(Box::new(t.clone())))
        }
        (Type::List(ce) | Type::Array(ce, _), Type::List(ve) | Type::Array(ve, _))
            if **ce == Type::Bool && ve.is_numeric() =>
        {
            Ok(Type::List(ve.clone()))
        }
        _ => Err(Diagnostic::at_line(
            DiagnosticCode::TypeError,
            line,
            format!("gate(cond, val) requires bool mask and numeric value, got ({cond}, {val})"),
        )),
    }
}

fn extract_broadcast_branch(branch: &Type, expected_len: Option<usize>, line: usize) -> DecResult<Type> {
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
        (Type::SimResult, "model_status")
        | (Type::SimResult, "chamber_name")
        | (Type::SimResult, "final_state_hash") => Ok(Type::Str),
        (Type::SimResult, "ticks")
        | (Type::SimResult, "carrier_count")
        | (Type::SimResult, "shedding_event_count")
        | (Type::SimResult, "wavelet_event_count")
        | (Type::SimResult, "boundary_crossings")
        | (Type::SimResult, "total_returns")
        | (Type::SimResult, "total_hinges")
        | (Type::SimResult, "total_shedding_events") => Ok(Type::Int),
        (Type::SimResult, "final_snapshot") => Ok(Type::ReadOnly(Box::new(Type::ChamberView))),

        (Type::ChamberView, "model_status") | (Type::ChamberView, "chamber_name") => Ok(Type::Str),
        (Type::ChamberView, "tick")
        | (Type::ChamberView, "carrier_count")
        | (Type::ChamberView, "boundary_crossings")
        | (Type::ChamberView, "shedding_count")
        | (Type::ChamberView, "wavelet_count")
        | (Type::ChamberView, "clock_count") => Ok(Type::Int),
        (Type::ChamberView, "carriers") => {
            Ok(Type::ReadOnly(Box::new(Type::List(Box::new(Type::Int)))))
        }

        (Type::RecipientOutcome, "stage") => Ok(Type::Str),
        (Type::RecipientOutcome, "closure_phase_delta")
        | (Type::RecipientOutcome, "reclosure_bip_delta") => Ok(Type::Int),
        (Type::RecipientOutcome, "blocks_later_seating") => Ok(Type::Bool),

        (Type::CadenceAssociation, "is_cadence_ratio") => Ok(Type::Bool),
        (Type::CadenceAssociation, "wavelet_id") | (Type::CadenceAssociation, "reference_id") => {
            Ok(Type::Str)
        }

        (Type::Support, "is_yro") | (Type::Support, "is_closure_certificate") => Ok(Type::Bool),
        (Type::Addr8, "is_first_anchor") => Ok(Type::Bool),
        (Type::Window, "start") | (Type::Window, "end") | (Type::Window, "len") => Ok(Type::Int),
        _ => Err(Diagnostic::at_line(
            DiagnosticCode::TypeError,
            line,
            format!("no field .{field} on type {base}"),
        )),
    }
}
