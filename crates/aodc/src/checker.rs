use std::collections::{BTreeMap, BTreeSet};

use crate::ast::{
    AssignTarget, BinaryOp, CallArg, ChamberDecl, ConstDecl, DecArm, DecArmParam, DecFnDecl,
    DecFnKind, EnumDecl, Expr, ExprKind, ForTarget, Item, MatchArm, ObserveDecl, Pattern,
    PrepareDecl, ProbeDecl, Program, Stmt, StmtKind, StructDecl, TypeExpr, UnaryOp,
};
use crate::diagnostics::{CompatMode, DecResult, Diagnostic, DiagnosticCode};
use crate::modules::{
    builtin_namespace_items, canonical_builtin_name, ImportedSymbol, LoadedUnit,
};
use crate::types::{is_v1_builtin_name, lookup_builtin, Effect, Type};

#[derive(Clone, Debug)]
struct VarBinding {
    ty: Type,
    mutable: bool,
    initialized: bool,
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
    in_qext_sink: bool,
    qext_outer_scope_depth: usize,
    qext_outer_loop_depth: usize,
    loop_depth: usize,
    expected_return: Type,
    observer_local_floor: usize,
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

/// Exhaustively typecheck every module, struct, enum, function, overload arm, apparatus block,
/// and quarantine module in `unit`, enforcing all 5 compile-time firewalls (`P0-1`, `P0-A`, `P1-3`).
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
                match &f.kind {
                    DecFnKind::MultiArm { arms, .. } => {
                        let zero_arg: Vec<&DecArm> =
                            arms.iter().filter(|a| a.params.is_empty()).collect();
                        if zero_arg.len() != 1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                f.line,
                                "core program requires exactly one main() signature",
                            ));
                        }
                        main_ret = Some(checker.resolve_type(
                            "<entry>",
                            &zero_arg[0].return_type,
                            zero_arg[0].line,
                        )?);
                    }
                    DecFnKind::SingleArm {
                        params,
                        return_type,
                        ..
                    } => {
                        if !params.is_empty() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                f.line,
                                "main() must take zero arguments",
                            ));
                        }
                        main_ret = Some(checker.resolve_type("<entry>", return_type, f.line)?);
                    }
                }
            } else if f.name.starts_with("test_") {
                let is_zero_arg = match &f.kind {
                    DecFnKind::SingleArm { params, .. } => params.is_empty(),
                    DecFnKind::MultiArm { arms, .. } => arms.iter().any(|a| a.params.is_empty()),
                };
                if is_zero_arg {
                    test_functions.push(f.name.clone());
                }
            }
        }
    }

    if require_main && main_ret.is_none() {
        return Err(Diagnostic::new(
            DiagnosticCode::TypeError,
            "core program requires exactly one main() signature",
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

impl<'a> Checker<'a> {
    fn compat(&self) -> CompatMode {
        self.unit.compat
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

    /// Resolve a `quarantine Q_ext` local, sibling (`P2-3`), or imported (`P0-1`) submodule.
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

    /// Resolve a 3-segment cross-module `quarantine Q_ext` submodule (`mod_ns::QMod::...`, `P0-1`).
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
        Type::resolve_with(expr, self.compat(), line, &|name| {
            self.lookup_user_type(mod_key, name)
        })
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
        // Resolve all structs & enums first so functions/consts can reference them
        self.collect_program_types("<entry>", &self.unit.entry_program)?;
        for (mod_key, prog) in &self.unit.parsed_modules {
            self.collect_program_types(mod_key, prog)?;
        }

        // Collect functions, apparatus blocks, and quarantine modules before const evaluation
        // so consts inside quarantine modules can reference sibling quarantine modules (`P2-3`).
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
                                    "only const and Dec declarations are permitted in quarantine Q_ext modules",
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
        if is_v1_builtin_name(&f.name) {
            return Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                f.line,
                format!("cannot redefine builtin {}", f.name),
            ));
        }
        let key = (mod_key.to_string(), f.name.clone());
        if self.functions.contains_key(&key) {
            return Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                f.line,
                format!("put overloads in one Dec declaration ({})", f.name),
            ));
        }

        match &f.kind {
            DecFnKind::MultiArm {
                header_params,
                header_typed_params,
                output_name,
                header_return_type,
                arms,
            } => {
                let mut uniq_h = BTreeSet::new();
                for hp in header_params {
                    if !uniq_h.insert(hp.as_str()) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            f.line,
                            "expected distinct parameter identifiers",
                        ));
                    }
                }
                if let Some(out_n) = output_name {
                    if header_params.iter().any(|p| p == out_n) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            f.line,
                            "result binding must differ from parameters",
                        ));
                    }
                }
                if let Some(htp) = header_typed_params {
                    for p in htp {
                        let pty = self.resolve_type(mod_key, &p.ty, f.line)?;
                        if !is_qext && pty.is_qext() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::QExtLeakageForbidden,
                                f.line,
                                format!(
                                    "native function {} cannot accept QExt parameter {}: {pty}",
                                    f.name, p.name
                                ),
                            ));
                        }
                    }
                }
                if let Some(hrt) = header_return_type {
                    let rty = self.resolve_type(mod_key, hrt, f.line)?;
                    if !is_qext && rty.is_qext() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            f.line,
                            format!("native function {} cannot return QExt type {rty}", f.name),
                        ));
                    }
                }

                let mut seen_plain_sigs: BTreeSet<Vec<Type>> = BTreeSet::new();
                for arm in arms {
                    if arm.params.len() != header_params.len() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            arm.line,
                            format!(
                                "arm parameter count ({}) does not match Dec header ({})",
                                arm.params.len(),
                                header_params.len()
                            ),
                        ));
                    }
                    let mut all_typed_vars = arm.guard.is_none();
                    let mut plain_sig = Vec::with_capacity(arm.params.len());
                    for (idx, p) in arm.params.iter().enumerate() {
                        match p {
                            DecArmParam::TypedVar { ty, name } => {
                                if self.compat() == CompatMode::V1 && name != &header_params[idx] {
                                    return Err(Diagnostic::at_line(
                                        DiagnosticCode::TypeError,
                                        arm.line,
                                        "signature bindings must match the Dec declaration",
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
                    if self.compat() == CompatMode::V1 && arm.output_name != *output_name {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            arm.line,
                            "signature bindings must match the Dec declaration",
                        ));
                    }
                    let ret_ty = self.resolve_type(mod_key, &arm.return_type, arm.line)?;
                    if !is_qext && ret_ty.is_qext() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            arm.line,
                            format!(
                                "native function {} cannot return QExt type {ret_ty}",
                                f.name
                            ),
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
            }
            DecFnKind::SingleArm {
                params,
                return_type,
                ..
            } => {
                let mut uniq = BTreeSet::new();
                for p in params {
                    if !uniq.insert(p.name.as_str()) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            f.line,
                            format!("duplicate parameter {}", p.name),
                        ));
                    }
                    let pty = self.resolve_type(mod_key, &p.ty, f.line)?;
                    if !is_qext && pty.is_qext() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            f.line,
                            format!(
                                "native function {} cannot accept QExt parameter {}: {pty}",
                                f.name, p.name
                            ),
                        ));
                    }
                }
                let ret_ty = self.resolve_type(mod_key, return_type, f.line)?;
                if !is_qext && ret_ty.is_qext() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        f.line,
                        format!("native function {} cannot return QExt type {ret_ty}", f.name),
                    ));
                }
                if is_qext && ret_ty != Type::Unit && !ret_ty.is_qext() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        f.line,
                        format!(
                            "quarantine Q_ext function {} must return QExt<T> or (), got {ret_ty}",
                            f.name
                        ),
                    ));
                }
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
            in_qext_sink: false,
            qext_outer_scope_depth: 0,
            qext_outer_loop_depth: 0,
            loop_depth: 0,
            expected_return: Type::Unit,
            observer_local_floor: 0,
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
            if !types_compatible_with_annotation(&val_ty, &expected) {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    c.line,
                    format!("const {} declared as {expected}, got {val_ty}", c.name),
                ));
            }
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
                in_qext_sink: false,
                qext_outer_scope_depth: 0,
                qext_outer_loop_depth: 0,
                loop_depth: 0,
                expected_return: Type::Unit,
                observer_local_floor: 0,
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
                in_qext_sink: false,
                qext_outer_scope_depth: 0,
                qext_outer_loop_depth: 0,
                loop_depth: 0,
                expected_return: Type::Unit,
                observer_local_floor: 0,
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
                            in_qext_sink: false,
                            qext_outer_scope_depth: 0,
                            qext_outer_loop_depth: 0,
                            loop_depth: 0,
                            expected_return: Type::Unit,
                            observer_local_floor: 0,
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
                                mutable: false,
                                initialized: true,
                            },
                        );
                        let mut scopes = vec![param_scope, BTreeMap::new()];
                        let mut ctx = CheckContext {
                            module_key: mod_key.clone(),
                            effect: Effect::ReadOnlyObserver,
                            in_main: false,
                            in_qext_sink: false,
                            qext_outer_scope_depth: 0,
                            qext_outer_loop_depth: 0,
                            loop_depth: 0,
                            expected_return: ret_ty.clone(),
                            observer_local_floor: 1,
                        };
                        for stmt in body {
                            self.check_stmt(stmt, &mut scopes, &mut ctx)?;
                        }
                        if ret_ty != Type::Unit && !self.definitely_returns(body) {
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

    fn infer_arm_param_types(
        &self,
        mod_key: &str,
        arm: &DecArm,
        header_typed: Option<&[crate::ast::TypedParam]>,
    ) -> DecResult<Vec<Type>> {
        let mut out = Vec::with_capacity(arm.params.len());
        for (idx, p) in arm.params.iter().enumerate() {
            let inferred = match p {
                DecArmParam::TypedVar { ty, .. } => self.resolve_type(mod_key, ty, arm.line)?,
                DecArmParam::TypedPattern { ty, .. } => self.resolve_type(mod_key, ty, arm.line)?,
                DecArmParam::Pattern(pat) => {
                    if let Some(t) = self.infer_pattern_type(mod_key, pat, arm.line)? {
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

    fn infer_pattern_type(
        &self,
        mod_key: &str,
        pat: &Pattern,
        line: usize,
    ) -> DecResult<Option<Type>> {
        match pat {
            Pattern::Wildcard | Pattern::Binding(_) => Ok(None),
            Pattern::IntLit(_) => Ok(Some(Type::Int)),
            Pattern::BoolLit(_) => Ok(Some(Type::Bool)),
            Pattern::StrLit(_) => Ok(Some(Type::Str)),
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
        match &entry.decl.kind {
            DecFnKind::MultiArm {
                header_typed_params,
                arms,
                ..
            } => {
                // Collect slot types across all arms so `_` wildcards inherit known slot types.
                let mut slot_hints: Vec<Option<Type>> = vec![
                    None;
                    arms.first().map_or(0, |a| a.params.len())
                ];
                if let Some(htp) = header_typed_params {
                    for (i, p) in htp.iter().enumerate() {
                        slot_hints[i] = Some(self.resolve_type(&entry.module_key, &p.ty, entry.decl.line)?);
                    }
                }
                for arm in arms {
                    let tys = self.infer_arm_param_types(
                        &entry.module_key,
                        arm,
                        header_typed_params.as_deref(),
                    )?;
                    for (i, t) in tys.into_iter().enumerate() {
                        if t != Type::Unknown && slot_hints[i].is_none() {
                            slot_hints[i] = Some(t);
                        }
                    }
                }

                for arm in arms {
                    let arm_tys = self.infer_arm_param_types(
                        &entry.module_key,
                        arm,
                        header_typed_params.as_deref(),
                    )?;
                    if let Some(ov) = override_arg_types {
                        if ov.len() != arm_tys.len()
                            || !ov.iter().zip(&arm_tys).all(|(a, p)| {
                                *p == Type::Unknown
                                    || a.unwrap_read_only() == p.unwrap_read_only()
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
                                let param_mutable = !resolved_ty.is_read_only();
                                scope.insert(
                                    name.clone(),
                                    VarBinding {
                                        ty: resolved_ty,
                                        mutable: param_mutable,
                                        initialized: true,
                                    },
                                );
                            }
                            DecArmParam::TypedPattern { pattern, .. }
                            | DecArmParam::Pattern(pattern) => {
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
                    if let Some(ref out_name) = arm.output_name {
                        scope.insert(
                            out_name.clone(),
                            VarBinding {
                                ty: ret_ty.clone(),
                                mutable: true,
                                initialized: false,
                            },
                        );
                    }
                    let mut scopes = vec![scope];
                    // P0-A: `in_main` is ONLY true when `effect == Effect::PureNative`!
                    let mut ctx = CheckContext {
                        module_key: entry.module_key.clone(),
                        effect,
                        in_main: is_main
                            && arm.params.is_empty()
                            && effect == Effect::PureNative,
                        in_qext_sink: false,
                        qext_outer_scope_depth: 0,
                        qext_outer_loop_depth: 0,
                        loop_depth: 0,
                        expected_return: ret_ty.clone(),
                        observer_local_floor: 0,
                    };
                    if let Some(ref guard) = arm.guard {
                        let g_ty = self.check_expr(guard, &scopes, &ctx)?;
                        if g_ty != Type::Bool {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                guard.line,
                                format!("Dec arm guard must be bool, got {g_ty}"),
                            ));
                        }
                    }
                    let mut returned = false;
                    for stmt in &arm.body {
                        if returned && self.compat() == CompatMode::V1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                stmt.line,
                                "unreachable statement after return",
                            ));
                        }
                        self.check_stmt(stmt, &mut scopes, &mut ctx)?;
                        if matches!(stmt.kind, StmtKind::Return(_)) {
                            returned = true;
                        }
                    }
                    let output_assigned = arm
                        .output_name
                        .as_ref()
                        .and_then(|n| scopes[0].get(n))
                        .is_some_and(|b| b.initialized);
                    if ret_ty != Type::Unit
                        && !returned
                        && !self.definitely_returns(&arm.body)
                        && !output_assigned
                    {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            arm.line,
                            "result is never assigned",
                        ));
                    }
                }
            }
            DecFnKind::SingleArm {
                params,
                return_type,
                body,
            } => {
                let param_types: Vec<Type> = params
                    .iter()
                    .map(|p| self.resolve_type(&entry.module_key, &p.ty, entry.decl.line))
                    .collect::<DecResult<_>>()?;
                let ret_ty = self.resolve_type(&entry.module_key, return_type, entry.decl.line)?;
                let mut param_scope = BTreeMap::new();
                for (idx, p) in params.iter().enumerate() {
                    let mut actual_ty = override_arg_types
                        .map(|ov| ov[idx].clone())
                        .unwrap_or_else(|| param_types[idx].clone());
                    if param_types[idx].is_read_only() {
                        actual_ty = actual_ty.wrap_read_only();
                    }
                    param_scope.insert(
                        p.name.clone(),
                        VarBinding {
                            ty: actual_ty,
                            mutable: false,
                            initialized: true,
                        },
                    );
                }
                let mut scopes = vec![param_scope, BTreeMap::new()];
                // P0-A: `in_main` is ONLY true when `effect == Effect::PureNative`!
                let mut ctx = CheckContext {
                    module_key: entry.module_key.clone(),
                    effect,
                    in_main: is_main && params.is_empty() && effect == Effect::PureNative,
                    in_qext_sink: false,
                    qext_outer_scope_depth: 0,
                    qext_outer_loop_depth: 0,
                    loop_depth: 0,
                    expected_return: ret_ty.clone(),
                    observer_local_floor: 1,
                };
                for stmt in body {
                    self.check_stmt(stmt, &mut scopes, &mut ctx)?;
                }
                if ret_ty != Type::Unit && !self.definitely_returns(body) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        entry.decl.line,
                        format!(
                            "function {} missing return of type {ret_ty}",
                            entry.decl.name
                        ),
                    ));
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
                scope.insert(
                    name.clone(),
                    VarBinding {
                        ty: expected.clone(),
                        mutable: false,
                        initialized: true,
                    },
                );
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
            StmtKind::Let { name, ty, value } => {
                let expected_ann = match ty {
                    Some(t) => {
                        let expected = self.resolve_type(&ctx.module_key, t, stmt.line)?;
                        if ctx.effect != Effect::QExt && expected.is_qext() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::QExtLeakageForbidden,
                                stmt.line,
                                format!("QExt type annotation forbidden on native let {name}"),
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
                        if !types_compatible_with_annotation(&val_ty, &expected) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ImplicitCoercionForbidden,
                                stmt.line,
                                format!("let {name} declared as {expected}, got {val_ty}"),
                            ));
                        }
                        expected
                    }
                    None => {
                        if contains_unknown(&val_ty) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                stmt.line,
                                format!("empty collection {name} requires an explicit type annotation"),
                            ));
                        }
                        val_ty
                    }
                };
                if ctx.effect != Effect::QExt && final_ty.is_qext() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        stmt.line,
                        format!("QExt value cannot bind to native let {name}"),
                    ));
                }
                let cur = scopes.last_mut().expect("scope stack non-empty");
                if cur.contains_key(name) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        stmt.line,
                        format!("duplicate local declaration {name}"),
                    ));
                }
                cur.insert(
                    name.clone(),
                    VarBinding {
                        ty: final_ty,
                        mutable: false,
                        initialized: true,
                    },
                );
                Ok(())
            }
            StmtKind::Mut { name, ty, value } => {
                let expected_ann = match ty {
                    Some(t) => {
                        let expected = self.resolve_type(&ctx.module_key, t, stmt.line)?;
                        if ctx.effect != Effect::QExt && expected.is_qext() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::QExtLeakageForbidden,
                                stmt.line,
                                format!("QExt type annotation forbidden on native mut {name}"),
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
                        if !types_compatible_with_annotation(&val_ty, &expected) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ImplicitCoercionForbidden,
                                stmt.line,
                                format!("mut {name} declared as {expected}, got {val_ty}"),
                            ));
                        }
                        expected
                    }
                    None => {
                        if contains_unknown(&val_ty) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                stmt.line,
                                format!("empty collection {name} requires an explicit type annotation"),
                            ));
                        }
                        val_ty
                    }
                };
                if ctx.effect != Effect::QExt && final_ty.is_qext() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        stmt.line,
                        format!("QExt value cannot bind to native mut {name}"),
                    ));
                }
                let cur = scopes.last_mut().expect("scope stack non-empty");
                if cur.contains_key(name) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        stmt.line,
                        format!("duplicate local declaration {name}"),
                    ));
                }
                cur.insert(
                    name.clone(),
                    VarBinding {
                        ty: final_ty,
                        mutable: true,
                        initialized: true,
                    },
                );
                Ok(())
            }
            StmtKind::TypedLocal { ty, name, value } => {
                let expected = self.resolve_type(&ctx.module_key, ty, stmt.line)?;
                if ctx.effect != Effect::QExt && expected.is_qext() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        stmt.line,
                        format!("QExt value cannot bind to native local {name}"),
                    ));
                }
                let cur = scopes.last().expect("scope stack non-empty");
                if cur.contains_key(name) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        stmt.line,
                        format!("duplicate local declaration {name}"),
                    ));
                }
                let val_ty = self.check_expr(value, scopes, ctx)?;
                if !types_compatible_with_annotation(&val_ty, &expected) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ImplicitCoercionForbidden,
                        stmt.line,
                        format!("assignment to {name} requires {expected}, got {val_ty}"),
                    ));
                }
                scopes.last_mut().unwrap().insert(
                    name.clone(),
                    VarBinding {
                        ty: expected,
                        mutable: true,
                        initialized: true,
                    },
                );
                Ok(())
            }
            StmtKind::Assign { target, value } => {
                let val_ty = self.check_expr(value, scopes, ctx)?;
                match target {
                    AssignTarget::Ident(name) => {
                        let found = find_var_scope_index(scopes, name);
                        let (scope_idx, binding_ty, mutable) = match found {
                            Some(idx) => {
                                let b = &scopes[idx][name];
                                (idx, b.ty.clone(), b.mutable)
                            }
                            None => {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    stmt.line,
                                    format!("declare local {name} with an explicit type"),
                                ));
                            }
                        };
                        if ctx.in_qext_sink && scope_idx < ctx.qext_outer_scope_depth {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::QExtLeakageForbidden,
                                stmt.line,
                                format!(
                                    "cannot write outer native binding {name} from inside one-way quarantine Q_ext sink"
                                ),
                            ));
                        }
                        if ctx.effect == Effect::ReadOnlyObserver
                            && (binding_ty.is_read_only() || scope_idx < ctx.observer_local_floor)
                        {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ObserverMutationForbidden,
                                stmt.line,
                                format!("observer cannot mutate read-only or outer binding {name}"),
                            ));
                        }
                        if !mutable {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                stmt.line,
                                format!("cannot assign to immutable binding {name}"),
                            ));
                        }
                        if !types_compatible_with_annotation(&val_ty, &binding_ty) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ImplicitCoercionForbidden,
                                stmt.line,
                                format!("assignment to {name} requires {binding_ty}, got {val_ty}"),
                            ));
                        }
                        scopes[scope_idx].get_mut(name).unwrap().initialized = true;
                        Ok(())
                    }
                    AssignTarget::Field { base, name } => {
                        let base_ty = self.check_expr(base, scopes, ctx)?;
                        if base_ty.is_read_only()
                            || matches!(base_ty.unwrap_read_only(), Type::ChamberView)
                        {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ObserverMutationForbidden,
                                stmt.line,
                                format!("cannot mutate field .{name} on read-only view ({base_ty})"),
                            ));
                        }
                        if let ExprKind::Ident(ref vname) = base.kind {
                            if let Some(idx) = find_var_scope_index(scopes, vname) {
                                if ctx.in_qext_sink && idx < ctx.qext_outer_scope_depth {
                                    return Err(Diagnostic::at_line(
                                        DiagnosticCode::QExtLeakageForbidden,
                                        stmt.line,
                                        format!("cannot mutate outer struct {vname}.{name} inside quarantine Q_ext sink"),
                                    ));
                                }
                                if ctx.effect == Effect::ReadOnlyObserver
                                    && idx < ctx.observer_local_floor
                                {
                                    return Err(Diagnostic::at_line(
                                        DiagnosticCode::ObserverMutationForbidden,
                                        stmt.line,
                                        format!("observer cannot mutate outer struct {vname}.{name}"),
                                    ));
                                }
                                let b = &scopes[idx][vname];
                                if let Type::Struct {
                                    module,
                                    name: sname,
                                } = &b.ty
                                {
                                    if !b.mutable {
                                        return Err(Diagnostic::at_line(
                                            DiagnosticCode::TypeError,
                                            stmt.line,
                                            format!("cannot mutate field .{name} of immutable binding {vname}"),
                                        ));
                                    }
                                    let field_ty = self
                                        .resolve_struct_field(module, sname, name, stmt.line)?;
                                    if !types_compatible_with_annotation(&val_ty, &field_ty) {
                                        return Err(Diagnostic::at_line(
                                            DiagnosticCode::ImplicitCoercionForbidden,
                                            stmt.line,
                                            format!(
                                                "field {sname}.{name} expects {field_ty}, got {val_ty}"
                                            ),
                                        ));
                                    }
                                    return Ok(());
                                }
                            }
                        }
                        if ctx.effect == Effect::ReadOnlyObserver {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ObserverMutationForbidden,
                                stmt.line,
                                format!("cannot mutate field .{name} in read-only observer"),
                            ));
                        }
                        Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            stmt.line,
                            format!("field .{name} on {base_ty} is immutable"),
                        ))
                    }
                    AssignTarget::Index { base, index } => {
                        let base_ty = self.check_expr(base, scopes, ctx)?;
                        let idx_ty = self.check_expr(index, scopes, ctx)?;
                        if base_ty.is_read_only()
                            || matches!(base_ty.unwrap_read_only(), Type::ChamberView)
                        {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ObserverMutationForbidden,
                                stmt.line,
                                format!("cannot mutate element of read-only view ({base_ty})"),
                            ));
                        }
                        if let ExprKind::Ident(ref vname) = base.kind {
                            if let Some(idx) = find_var_scope_index(scopes, vname) {
                                if ctx.in_qext_sink && idx < ctx.qext_outer_scope_depth {
                                    return Err(Diagnostic::at_line(
                                        DiagnosticCode::QExtLeakageForbidden,
                                        stmt.line,
                                        "cannot mutate outer collection inside quarantine Q_ext sink",
                                    ));
                                }
                                if ctx.effect == Effect::ReadOnlyObserver
                                    && idx < ctx.observer_local_floor
                                {
                                    return Err(Diagnostic::at_line(
                                        DiagnosticCode::ObserverMutationForbidden,
                                        stmt.line,
                                        format!("observer cannot mutate outer collection {vname}"),
                                    ));
                                }
                                let b = &scopes[idx][vname];
                                if !b.mutable {
                                    return Err(Diagnostic::at_line(
                                        DiagnosticCode::TypeError,
                                        stmt.line,
                                        format!("cannot index-assign immutable binding {vname}"),
                                    ));
                                }
                                match &b.ty {
                                    Type::List(elem_ty) => {
                                        if idx_ty != Type::Int {
                                            return Err(Diagnostic::at_line(
                                                DiagnosticCode::TypeError,
                                                stmt.line,
                                                format!("list index must be int, got {idx_ty}"),
                                            ));
                                        }
                                        if !types_compatible_with_annotation(&val_ty, elem_ty) {
                                            return Err(Diagnostic::at_line(
                                                DiagnosticCode::ImplicitCoercionForbidden,
                                                stmt.line,
                                                format!("list element requires {elem_ty}, got {val_ty}"),
                                            ));
                                        }
                                        return Ok(());
                                    }
                                    Type::Map(k_ty, v_ty) => {
                                        if idx_ty != **k_ty {
                                            return Err(Diagnostic::at_line(
                                                DiagnosticCode::ImplicitCoercionForbidden,
                                                stmt.line,
                                                format!("map key requires {k_ty}, got {idx_ty}"),
                                            ));
                                        }
                                        if !types_compatible_with_annotation(&val_ty, v_ty) {
                                            return Err(Diagnostic::at_line(
                                                DiagnosticCode::ImplicitCoercionForbidden,
                                                stmt.line,
                                                format!("map value requires {v_ty}, got {val_ty}"),
                                            ));
                                        }
                                        return Ok(());
                                    }
                                    _ => {}
                                }
                            }
                        }
                        if ctx.effect == Effect::ReadOnlyObserver {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ObserverMutationForbidden,
                                stmt.line,
                                "cannot mutate indexed value in read-only observer",
                            ));
                        }
                        Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            stmt.line,
                            format!("cannot index-assign value of type {base_ty}"),
                        ))
                    }
                }
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
                if !types_compatible_with_annotation(&actual, &ctx.expected_return) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        stmt.line,
                        format!(
                            "return type mismatch: expected {}, got {actual}",
                            ctx.expected_return
                        ),
                    ));
                }
                Ok(())
            }
            StmtKind::If {
                cond,
                then_body,
                elifs,
                else_body,
            } => {
                let c_ty = self.check_expr(cond, scopes, ctx)?;
                let cond_ok = c_ty == Type::Bool
                    || (ctx.effect == Effect::QExt && c_ty == Type::QExt(Box::new(Type::Bool)));
                if !cond_ok {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        stmt.line,
                        format!("if condition must be bool, got {c_ty}"),
                    ));
                }
                scopes.push(BTreeMap::new());
                for s in then_body {
                    self.check_stmt(s, scopes, ctx)?;
                }
                scopes.pop();
                for (ec, eb) in elifs {
                    let et = self.check_expr(ec, scopes, ctx)?;
                    let ok = et == Type::Bool
                        || (ctx.effect == Effect::QExt && et == Type::QExt(Box::new(Type::Bool)));
                    if !ok {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            ec.line,
                            format!("elif condition must be bool, got {et}"),
                        ));
                    }
                    scopes.push(BTreeMap::new());
                    for s in eb {
                        self.check_stmt(s, scopes, ctx)?;
                    }
                    scopes.pop();
                }
                if !else_body.is_empty() {
                    scopes.push(BTreeMap::new());
                    for s in else_body {
                        self.check_stmt(s, scopes, ctx)?;
                    }
                    scopes.pop();
                }
                Ok(())
            }
            StmtKind::Match { scrutinee, arms } => {
                let scrut_ty = self.check_expr(scrutinee, scopes, ctx)?;
                let mut covered_variants: BTreeSet<String> = BTreeSet::new();
                let mut has_irrefutable_wildcard = false;

                for arm in arms {
                    let mut arm_scope = BTreeMap::new();
                    self.check_pattern(
                        &arm.pattern,
                        &scrut_ty,
                        &ctx.module_key,
                        &mut arm_scope,
                        arm.line,
                    )?;
                    scopes.push(arm_scope);
                    if let Some(ref g) = arm.guard {
                        let g_ty = self.check_expr(g, scopes, ctx)?;
                        if g_ty != Type::Bool {
                            scopes.pop();
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                g.line,
                                format!("match arm guard must be bool, got {g_ty}"),
                            ));
                        }
                    } else {
                        match &arm.pattern {
                            Pattern::Wildcard | Pattern::Binding(_) => {
                                has_irrefutable_wildcard = true;
                            }
                            Pattern::EnumVariant { path, bindings } => {
                                if bindings.iter().all(is_irrefutable_pattern) {
                                    if let Some(vname) = path.last() {
                                        covered_variants.insert(vname.clone());
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    for s in &arm.body {
                        self.check_stmt(s, scopes, ctx)?;
                    }
                    scopes.pop();
                }

                // Compile-time exhaustiveness checking for enum scrutinees!
                if let Type::Enum { module, name } = scrut_ty.unwrap_read_only() {
                    if !has_irrefutable_wildcard {
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
                                    stmt.line,
                                    format!(
                                        "non-exhaustive match on enum {name}: missing variant(s) {}",
                                        missing.join(", ")
                                    ),
                                ));
                            }
                        }
                    }
                }
                Ok(())
            }
            StmtKind::For { target, iter, body } => {
                let it_ty = self.check_expr(iter, scopes, ctx)?;
                let mut loop_scope = BTreeMap::new();
                match (target, it_ty) {
                    (ForTarget::Single(var), Type::Range) => {
                        loop_scope.insert(
                            var.clone(),
                            VarBinding {
                                ty: Type::Int,
                                mutable: false,
                                initialized: true,
                            },
                        );
                    }
                    (ForTarget::Single(var), Type::List(inner)) => {
                        loop_scope.insert(
                            var.clone(),
                            VarBinding {
                                ty: *inner,
                                mutable: false,
                                initialized: true,
                            },
                        );
                    }
                    (ForTarget::Single(var), Type::Map(k, _)) => {
                        loop_scope.insert(
                            var.clone(),
                            VarBinding {
                                ty: *k,
                                mutable: false,
                                initialized: true,
                            },
                        );
                    }
                    (ForTarget::Tuple(vars), Type::Map(k, v)) if vars.len() == 2 => {
                        loop_scope.insert(
                            vars[0].clone(),
                            VarBinding {
                                ty: *k,
                                mutable: false,
                                initialized: true,
                            },
                        );
                        loop_scope.insert(
                            vars[1].clone(),
                            VarBinding {
                                ty: *v,
                                mutable: false,
                                initialized: true,
                            },
                        );
                    }
                    (ForTarget::Tuple(vars), Type::List(inner)) => match *inner {
                        Type::Tuple(elems) if elems.len() == vars.len() => {
                            for (vn, et) in vars.iter().zip(elems) {
                                loop_scope.insert(
                                    vn.clone(),
                                    VarBinding {
                                        ty: et,
                                        mutable: false,
                                        initialized: true,
                                    },
                                );
                            }
                        }
                        other => {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                stmt.line,
                                format!("cannot tuple-destructure list element type {other}"),
                            ));
                        }
                    },
                    (ForTarget::Single(var), Type::ReadOnly(inner)) => match *inner {
                        Type::List(elem) => {
                            loop_scope.insert(
                                var.clone(),
                                VarBinding {
                                    ty: elem.wrap_read_only(),
                                    mutable: false,
                                    initialized: true,
                                },
                            );
                        }
                        Type::Map(k, _) => {
                            loop_scope.insert(
                                var.clone(),
                                VarBinding {
                                    ty: k.wrap_read_only(),
                                    mutable: false,
                                    initialized: true,
                                },
                            );
                        }
                        other => {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                stmt.line,
                                format!("cannot iterate over ReadOnly<{other}>"),
                            ));
                        }
                    },
                    (ForTarget::Tuple(vars), Type::ReadOnly(inner)) if vars.len() == 2 => {
                        match *inner {
                            Type::Map(k, v) => {
                                loop_scope.insert(
                                    vars[0].clone(),
                                    VarBinding {
                                        ty: k.wrap_read_only(),
                                        mutable: false,
                                        initialized: true,
                                    },
                                );
                                loop_scope.insert(
                                    vars[1].clone(),
                                    VarBinding {
                                        ty: v.wrap_read_only(),
                                        mutable: false,
                                        initialized: true,
                                    },
                                );
                            }
                            other => {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    stmt.line,
                                    format!("cannot tuple-iterate over ReadOnly<{other}>"),
                                ));
                            }
                        }
                    }
                    (_, other) => {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            stmt.line,
                            format!("for loop target incompatible with iterable type {other}"),
                        ));
                    }
                }
                scopes.push(loop_scope);
                ctx.loop_depth += 1;
                for s in body {
                    self.check_stmt(s, scopes, ctx)?;
                }
                ctx.loop_depth -= 1;
                scopes.pop();
                Ok(())
            }
            StmtKind::While { cond, body } => {
                let c_ty = self.check_expr(cond, scopes, ctx)?;
                if c_ty != Type::Bool {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        stmt.line,
                        format!("while condition must be bool, got {c_ty}"),
                    ));
                }
                scopes.push(BTreeMap::new());
                ctx.loop_depth += 1;
                for s in body {
                    self.check_stmt(s, scopes, ctx)?;
                }
                ctx.loop_depth -= 1;
                scopes.pop();
                Ok(())
            }
            StmtKind::Break | StmtKind::Continue => {
                if ctx.loop_depth == 0 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        stmt.line,
                        "break/continue outside of loop",
                    ));
                }
                if ctx.in_qext_sink && ctx.loop_depth <= ctx.qext_outer_loop_depth {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        stmt.line,
                        "break/continue cannot escape a one-way quarantine Q_ext: sink block into an enclosing native loop",
                    ));
                }
                Ok(())
            }
            StmtKind::Pass => Ok(()),
            StmtKind::QuarantineBlock { body } => {
                // P0-A: explicitly reject ReadOnlyObserver BEFORE checking in_main!
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
                        "quarantine Q_ext: sink is only permitted in Dec main() or quarantine Q_ext functions",
                    ));
                }
                let saved_effect = ctx.effect;
                let saved_in_sink = ctx.in_qext_sink;
                let saved_outer_scope = ctx.qext_outer_scope_depth;
                let saved_outer_loop = ctx.qext_outer_loop_depth;

                ctx.effect = Effect::QExt;
                if !saved_in_sink && saved_effect != Effect::QExt {
                    ctx.in_qext_sink = true;
                    ctx.qext_outer_scope_depth = scopes.len();
                    ctx.qext_outer_loop_depth = ctx.loop_depth;
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
                ctx.qext_outer_scope_depth = saved_outer_scope;
                ctx.qext_outer_loop_depth = saved_outer_loop;
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
            ExprKind::IntLit(_) => Ok(Type::Int),
            ExprKind::BoolLit(_) => Ok(Type::Bool),
            ExprKind::StrLit(_) => Ok(Type::Str),
            ExprKind::RatioLit(_) => Ok(Type::Ratio),
            ExprKind::Addr8Lit(_) => Ok(Type::Addr8),
            ExprKind::SupportLit(_) => Ok(Type::Support),
            ExprKind::TritLit(_) => Ok(Type::T1),
            ExprKind::Ident(name) => {
                if let Some(idx) = find_var_scope_index(scopes, name) {
                    let b = &scopes[idx][name];
                    if !b.initialized {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            expr.line,
                            format!("undefined or uninitialized name {name}"),
                        ));
                    }
                    if ctx.in_qext_sink && idx < ctx.qext_outer_scope_depth && b.mutable {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            expr.line,
                            format!(
                                "quarantine Q_ext: sink may only read immutable (let/frozen) native bindings, not mutable {name}"
                            ),
                        ));
                    }
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
                // Check enclosing base module const if inside quarantine submodule
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
                                format!("QExt const {name} cannot be referenced outside quarantine Q_ext"),
                            ));
                        }
                    }
                }
                Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    expr.line,
                    format!("undefined or uninitialized name {name}"),
                ))
            }
            ExprKind::Path(segs) => {
                // 1. Check unit enum variant `Enum::Variant` or `mod::Enum::Variant`
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
                // 2. Sibling or local quarantine submodule const (`P2-3`)
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
                    // 3. Cross-module 3-segment `quarantine Q_ext` const (`mod::QMod::CONST`, `P0-1`)
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
            ExprKind::StructLit { path, fields } => {
                let sdef = self
                    .resolve_struct_path(&ctx.module_key, path, expr.line)?
                    .clone();
                if fields.len() != sdef.fields.len() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr.line,
                        format!(
                            "struct {} expects {} field(s), got {}",
                            sdef.name,
                            sdef.fields.len(),
                            fields.len()
                        ),
                    ));
                }
                let mut seen = BTreeSet::new();
                for (fname, fexpr) in fields {
                    if !seen.insert(fname.as_str()) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            expr.line,
                            format!("duplicate field {fname} in struct {} literal", sdef.name),
                        ));
                    }
                    let expected = sdef
                        .fields
                        .iter()
                        .find(|(n, _)| n == fname)
                        .map(|(_, t)| t.clone())
                        .ok_or_else(|| {
                            Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr.line,
                                format!("unknown field {fname} on struct {}", sdef.name),
                            )
                        })?;
                    let actual = self.check_expr(fexpr, scopes, ctx)?;
                    if !types_compatible_with_annotation(&actual, &expected) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ImplicitCoercionForbidden,
                            fexpr.line,
                            format!("field {}.{} expects {expected}, got {actual}", sdef.name, fname),
                        ));
                    }
                }
                Ok(Type::Struct {
                    module: sdef.module,
                    name: sdef.name,
                })
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
            ExprKind::List(items) => {
                if items.is_empty() {
                    return Ok(Type::List(Box::new(Type::Unknown)));
                }
                let first = self.check_expr(&items[0], scopes, ctx)?;
                for it in &items[1..] {
                    let ty = self.check_expr(it, scopes, ctx)?;
                    if ty != first {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ImplicitCoercionForbidden,
                            it.line,
                            format!("heterogeneous list elements: expected {first}, got {ty}"),
                        ));
                    }
                }
                Ok(Type::List(Box::new(first)))
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
                match op {
                    UnaryOp::Not => {
                        if ty == Type::Bool {
                            Ok(Type::Bool)
                        } else if ctx.effect == Effect::QExt
                            && ty == Type::QExt(Box::new(Type::Bool))
                        {
                            Ok(Type::QExt(Box::new(Type::Bool)))
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr.line,
                                format!("invalid unary `not` on {ty}"),
                            ))
                        }
                    }
                    UnaryOp::Pos | UnaryOp::Neg => {
                        if ty.is_numeric_v1() {
                            Ok(ty)
                        } else if ctx.effect == Effect::QExt
                            && ty == Type::QExt(Box::new(Type::Ratio))
                        {
                            Ok(ty)
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr.line,
                                format!("invalid unary arithmetic on {ty}"),
                            ))
                        }
                    }
                    UnaryOp::Boundary => match ty.unwrap_read_only() {
                        Type::Box3 | Type::SimResult => Ok(Type::Str),
                        Type::Window | Type::Range => Ok(Type::Tuple(vec![Type::Int, Type::Int])),
                        other => Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            expr.line,
                            format!("boundary operator ∂ unsupported on {other}"),
                        )),
                    },
                }
            }
            ExprKind::Binary { op, left, right } => {
                let lt = self.check_expr(left, scopes, ctx)?;
                let rt = self.check_expr(right, scopes, ctx)?;
                let l_inner = lt.unwrap_read_only();
                let r_inner = rt.unwrap_read_only();
                if l_inner != r_inner {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ImplicitCoercionForbidden,
                        expr.line,
                        format!(
                            "implicit numeric/type coercion is forbidden: {lt} {:?} {rt}",
                            op
                        ),
                    ));
                }
                let ty = l_inner.clone();
                match op {
                    BinaryOp::And | BinaryOp::Or => {
                        if ty == Type::Bool {
                            Ok(Type::Bool)
                        } else if ctx.effect == Effect::QExt
                            && ty == Type::QExt(Box::new(Type::Bool))
                        {
                            Ok(Type::QExt(Box::new(Type::Bool)))
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr.line,
                                format!("Boolean operation requires bool operands, got {ty}"),
                            ))
                        }
                    }
                    BinaryOp::Eq | BinaryOp::NotEq => {
                        if ty.is_qext() {
                            if ctx.effect != Effect::QExt {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::QExtLeakageForbidden,
                                    expr.line,
                                    "cannot compare QExt values outside quarantine Q_ext",
                                ));
                            }
                            Ok(Type::QExt(Box::new(Type::Bool)))
                        } else {
                            Ok(Type::Bool)
                        }
                    }
                    BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => {
                        if ty.is_numeric_v1() {
                            Ok(Type::Bool)
                        } else if ctx.effect == Effect::QExt
                            && ty == Type::QExt(Box::new(Type::Ratio))
                        {
                            Ok(Type::QExt(Box::new(Type::Bool)))
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr.line,
                                format!("ordering requires a numeric type, got {ty}"),
                            ))
                        }
                    }
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => {
                        if ty.is_numeric_v1() {
                            Ok(ty)
                        } else if ctx.effect == Effect::QExt
                            && ty == Type::QExt(Box::new(Type::Ratio))
                        {
                            Ok(ty)
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr.line,
                                format!("arithmetic requires matching explicit numeric types, got {ty}"),
                            ))
                        }
                    }
                    BinaryOp::Div => {
                        if ty.is_numeric_v1() {
                            Ok(Type::Ratio)
                        } else if ctx.effect == Effect::QExt
                            && ty == Type::QExt(Box::new(Type::Ratio))
                        {
                            Ok(Type::QExt(Box::new(Type::Ratio)))
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr.line,
                                format!("division requires numeric type, got {ty}"),
                            ))
                        }
                    }
                    BinaryOp::FloorDiv | BinaryOp::Mod => {
                        if ty == Type::Int || ty == Type::T32 {
                            Ok(ty)
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr.line,
                                format!("quotient/remainder require int or t32, got {ty}"),
                            ))
                        }
                    }
                    BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::Shl
                    | BinaryOp::Shr => {
                        if self.compat() == CompatMode::V1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr.line,
                                "bitwise operators are not in --compat v1",
                            ));
                        }
                        if ty == Type::Int {
                            Ok(Type::Int)
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr.line,
                                format!("bitwise ops require int, got {ty}"),
                            ))
                        }
                    }
                }
            }
            ExprKind::Ternary { cond, yes, no } => {
                let c_ty = self.check_expr(cond, scopes, ctx)?;
                let y_ty = self.check_expr(yes, scopes, ctx)?;
                let n_ty = self.check_expr(no, scopes, ctx)?;
                if c_ty != Type::Bool || y_ty != n_ty {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr.line,
                        format!(
                            "conditional requires bool condition and matching result types (got {c_ty}, {y_ty}, {n_ty})"
                        ),
                    ));
                }
                Ok(y_ty)
            }
            ExprKind::Field { base, name } => {
                let base_ty = self.check_expr(base, scopes, ctx)?;
                let is_ro = base_ty.is_read_only();
                let inner = base_ty.unwrap_read_only();
                let field_ty = if let Type::Struct {
                    module,
                    name: sname,
                } = inner
                {
                    self.resolve_struct_field(module, sname, name, expr.line)?
                } else {
                    resolve_builtin_field_type(inner, name, expr.line)?
                };
                if is_ro
                    && matches!(
                        field_ty,
                        Type::List(_)
                            | Type::Map(_, _)
                            | Type::Struct { .. }
                            | Type::ChamberView
                            | Type::SimResult
                    )
                {
                    Ok(field_ty.wrap_read_only())
                } else {
                    Ok(field_ty)
                }
            }
            ExprKind::Index { base, index } => {
                let base_ty = self.check_expr(base, scopes, ctx)?;
                let idx_ty = self.check_expr(index, scopes, ctx)?;
                let is_ro = base_ty.is_read_only();
                match base_ty.unwrap_read_only() {
                    Type::List(elem) => {
                        if idx_ty != Type::Int {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr.line,
                                format!("list index must be int, got {idx_ty}"),
                            ));
                        }
                        if is_ro {
                            Ok((**elem).clone().wrap_read_only())
                        } else {
                            Ok((**elem).clone())
                        }
                    }
                    Type::Map(k_ty, v_ty) => {
                        if idx_ty != **k_ty {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ImplicitCoercionForbidden,
                                expr.line,
                                format!("map key expects {k_ty}, got {idx_ty}"),
                            ));
                        }
                        if is_ro {
                            Ok((**v_ty).clone().wrap_read_only())
                        } else {
                            Ok((**v_ty).clone())
                        }
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
        args: &[CallArg],
        line: usize,
        scopes: &[BTreeMap<String, VarBinding>],
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        let mut evaluated_args: Vec<(Option<String>, Type)> = Vec::with_capacity(args.len());
        for a in args {
            let ty = self.check_expr(&a.value, scopes, ctx)?;
            evaluated_args.push((a.name.clone(), ty));
        }

        // Polymorphic v2 collection builtins `len(...)` and `contains(m, k)`
        if let ExprKind::Ident(ref name) = callee.kind {
            if self.compat() == CompatMode::V2 && name == "len" && evaluated_args.len() == 1 {
                if evaluated_args[0].0.is_none() {
                    match evaluated_args[0].1.unwrap_read_only() {
                        Type::Str | Type::List(_) | Type::Map(_, _) => return Ok(Type::Int),
                        _ => {}
                    }
                }
            }
            if self.compat() == CompatMode::V2 && name == "contains" && evaluated_args.len() == 2 {
                if evaluated_args[0].0.is_none() && evaluated_args[1].0.is_none() {
                    if let Type::Map(k_ty, _) = evaluated_args[0].1.unwrap_read_only() {
                        if evaluated_args[1].1.unwrap_read_only() == &**k_ty {
                            return Ok(Type::Bool);
                        }
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ImplicitCoercionForbidden,
                            line,
                            format!(
                                "contains(map, key) expects key type {k_ty}, got {}",
                                evaluated_args[1].1
                            ),
                        ));
                    }
                }
            }
        }

        // Check enum payload constructor `Enum::Variant(args...)` or `mod::Enum::Variant(args...)`
        if let ExprKind::Path(ref segs) = callee.kind {
            if segs.len() == 2 || segs.len() == 3 {
                if let Ok((edef, payload_tys)) =
                    self.resolve_enum_variant(&ctx.module_key, segs, line)
                {
                    if evaluated_args.iter().any(|(n, _)| n.is_some()) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            "enum variant constructors take positional arguments",
                        ));
                    }
                    if evaluated_args.len() != payload_tys.len() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!(
                                "variant {} expects {} argument(s), got {}",
                                segs.join("::"),
                                payload_tys.len(),
                                evaluated_args.len()
                            ),
                        ));
                    }
                    for ((_, actual), expected) in evaluated_args.iter().zip(payload_tys) {
                        if !types_compatible_with_annotation(actual, expected) {
                            if actual.is_read_only()
                                && !expected.is_read_only()
                                && types_compatible_with_annotation(
                                    actual.unwrap_read_only(),
                                    expected,
                                )
                            {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::ObserverMutationForbidden,
                                    line,
                                    format!(
                                        "cannot strip ReadOnly from {actual} in enum variant {}",
                                        segs.join("::")
                                    ),
                                ));
                            }
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ImplicitCoercionForbidden,
                                line,
                                format!(
                                    "variant {} expects {expected}, got {actual}",
                                    segs.join("::")
                                ),
                            ));
                        }
                    }
                    return Ok(Type::Enum {
                        module: edef.module.clone(),
                        name: edef.name.clone(),
                    });
                }
            }
        }

        // Check struct constructor call `Foo(1, 2)` or `Foo(x: 1, y: 2)`
        let struct_path_opt: Option<Vec<String>> = match &callee.kind {
            ExprKind::Ident(n) => Some(vec![n.clone()]),
            ExprKind::Path(segs) => Some(segs.clone()),
            _ => None,
        };
        if let Some(ref sp) = struct_path_opt {
            if let Ok(sdef) = self.resolve_struct_path(&ctx.module_key, sp, line).cloned() {
                let param_defs: Vec<crate::ast::TypedParam> = sdef
                    .fields
                    .iter()
                    .map(|(n, _)| crate::ast::TypedParam {
                        name: n.clone(),
                        ty: TypeExpr::Unit,
                    })
                    .collect();
                let ordered = resolve_single_arm_args(&param_defs, &evaluated_args, line)?;
                for ((fname, fty), actual) in sdef.fields.iter().zip(ordered) {
                    if !types_compatible_with_annotation(&actual, fty) {
                        if actual.is_read_only()
                            && !fty.is_read_only()
                            && types_compatible_with_annotation(actual.unwrap_read_only(), fty)
                        {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ObserverMutationForbidden,
                                line,
                                format!(
                                    "cannot strip ReadOnly from {actual} in struct {} field {fname}",
                                    sdef.name
                                ),
                            ));
                        }
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ImplicitCoercionForbidden,
                            line,
                            format!(
                                "struct {} field {fname} expects {fty}, got {actual}",
                                sdef.name
                            ),
                        ));
                    }
                }
                return Ok(Type::Struct {
                    module: sdef.module,
                    name: sdef.name,
                });
            }
        }

        enum CallTarget {
            Builtin(String),
            UserFn(FnEntry),
        }

        let base_mod = self.base_module_of(&ctx.module_key).to_string();

        let target = match &callee.kind {
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
                        CallTarget::Builtin(canon)
                    } else {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            line,
                            format!(
                                "{name} must be explicitly imported from aod::qext (`use aod::qext::{name}` or `aod::qext::{name}`)"
                            ),
                        ));
                    }
                } else if let Some(entry) = self
                    .functions
                    .get(&(ctx.module_key.clone(), name.clone()))
                    .cloned()
                {
                    CallTarget::UserFn(entry)
                } else if base_mod != ctx.module_key
                    && self
                        .functions
                        .contains_key(&(base_mod.clone(), name.clone()))
                {
                    CallTarget::UserFn(self.functions[&(base_mod, name.clone())].clone())
                } else if let Some(ImportedSymbol::ModuleItem { module, item }) =
                    self.imports_for(&ctx.module_key).get(name).cloned()
                {
                    if let Some(entry) = self.functions.get(&(module.clone(), item.clone())).cloned()
                    {
                        CallTarget::UserFn(entry)
                    } else {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("imported symbol {module}::{item} is not a function"),
                        ));
                    }
                } else if let Some(ImportedSymbol::Builtin(canon)) =
                    self.imports_for(&ctx.module_key).get(name).cloned()
                {
                    CallTarget::Builtin(canon)
                } else if !lookup_builtin(name).is_empty() {
                    CallTarget::Builtin(name.clone())
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
                        CallTarget::UserFn(e)
                    } else {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("undefined function {name}"),
                        ));
                    }
                }
            }
            ExprKind::Path(segs) => {
                if segs.len() == 3 && segs[0] == "aod" {
                    let ns = &segs[0..2];
                    let sym = &segs[2];
                    if let Some(allowed) = builtin_namespace_items(ns) {
                        if allowed.contains(&sym.as_str()) {
                            CallTarget::Builtin(canonical_builtin_name(ns, sym))
                        } else {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("unknown function {}", segs.join("::")),
                            ));
                        }
                    } else {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("unknown namespace {}", ns.join("::")),
                        ));
                    }
                } else if segs.len() == 2 {
                    let ns = &segs[0];
                    let sym = &segs[1];
                    // P2-3: resolve sibling or local quarantine Q_ext submodule `ModA::fn`!
                    if let Some(qsub) = self.resolve_qext_submodule(&ctx.module_key, ns) {
                        if let Some(entry) = self.functions.get(&(qsub, sym.clone())).cloned() {
                            CallTarget::UserFn(entry)
                        } else {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("undefined function {ns}::{sym}"),
                            ));
                        }
                    } else if let Some(ImportedSymbol::ModuleNamespace(mod_key)) =
                        self.imports_for(&ctx.module_key).get(ns).cloned()
                    {
                        if let Some(rest) = mod_key.strip_prefix("aod::") {
                            let ns_vec = vec!["aod".to_string(), rest.to_string()];
                            if builtin_namespace_items(&ns_vec)
                                .is_some_and(|list| list.contains(&sym.as_str()))
                            {
                                CallTarget::Builtin(canonical_builtin_name(&ns_vec, sym))
                            } else {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    line,
                                    format!("unknown builtin {mod_key}::{sym}"),
                                ));
                            }
                        } else if let Some(entry) =
                            self.functions.get(&(mod_key.clone(), sym.clone())).cloned()
                        {
                            CallTarget::UserFn(entry)
                        } else {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("undefined function {mod_key}::{sym}"),
                            ));
                        }
                    } else {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("undefined function path {}", segs.join("::")),
                        ));
                    }
                } else if segs.len() == 3 {
                    // P0-1: 3-segment cross-module `quarantine Q_ext` call (`mod::QMod::fn(...)`)
                    if let Some(qsub) =
                        self.resolve_qext_submodule_3seg(&ctx.module_key, &segs[0], &segs[1])
                    {
                        if let Some(entry) = self.functions.get(&(qsub, segs[2].clone())).cloned() {
                            CallTarget::UserFn(entry)
                        } else {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("undefined function {}", segs.join("::")),
                            ));
                        }
                    } else {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("unsupported call path {}", segs.join("::")),
                        ));
                    }
                } else {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("unsupported call path {}", segs.join("::")),
                    ));
                }
            }
            _ => {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    line,
                    "calls require a direct function name or module path",
                ));
            }
        };

        match target {
            CallTarget::Builtin(canon) => {
                let sigs = lookup_builtin(&canon);
                if sigs.is_empty() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("undefined builtin {canon}"),
                    ));
                }
                if evaluated_args.iter().any(|(n, _)| n.is_some()) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("builtin {canon} takes positional arguments"),
                    ));
                }
                let arg_tys: Vec<Type> = evaluated_args
                    .iter()
                    .map(|(_, t)| t.unwrap_read_only().clone())
                    .collect();

                if sigs.iter().all(|s| s.effect == Effect::QExt) && ctx.effect != Effect::QExt {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        line,
                        format!("builtin {canon} is quarantined in aod::qext and forbidden outside quarantine Q_ext"),
                    ));
                }

                let matching: Vec<_> = sigs
                    .into_iter()
                    .filter(|s| {
                        (self.compat() == CompatMode::V2 || s.v1_allowed) && s.params == arg_tys
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
                match &entry.decl.kind {
                    DecFnKind::MultiArm {
                        header_typed_params,
                        arms,
                        ..
                    } => {
                        if evaluated_args.iter().any(|(n, _)| n.is_some()) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "multi-arm Dec calls use positional arguments",
                            ));
                        }
                        let raw_arg_tys: Vec<Type> =
                            evaluated_args.iter().map(|(_, t)| t.clone()).collect();
                        let mut matched_ret: Option<Type> = None;
                        let mut blocked_by_readonly = false;
                        let mut needs_ro_recheck = false;
                        for arm in arms {
                            let arm_tys = self.infer_arm_param_types(
                                &entry.module_key,
                                arm,
                                header_typed_params.as_deref(),
                            )?;
                            if arm_tys.len() != raw_arg_tys.len() {
                                continue;
                            }
                            let mut arm_matches = true;
                            let mut arm_stripped_readonly = false;
                            for (p, a) in arm_tys.iter().zip(&raw_arg_tys) {
                                if *p == Type::Unknown {
                                    if a.is_read_only() {
                                        needs_ro_recheck = true;
                                    }
                                    continue;
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
                                let rty = self.resolve_type(
                                    &entry.module_key,
                                    &arm.return_type,
                                    arm.line,
                                )?;
                                if let Some(ref prev) = matched_ret {
                                    if prev != &rty {
                                        return Err(Diagnostic::at_line(
                                            DiagnosticCode::TypeError,
                                            line,
                                            format!(
                                                "ambiguous return types ({prev} vs {rty}) across matching pattern arms of {}",
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
                            self.check_transitive_effect(&entry, &raw_arg_tys, ctx.effect)?;
                        } else if needs_ro_recheck {
                            self.check_transitive_effect(&entry, &raw_arg_tys, Effect::PureNative)?;
                        }
                        Ok(ret_ty)
                    }
                    DecFnKind::SingleArm {
                        params,
                        return_type,
                        ..
                    } => {
                        let ordered_arg_tys =
                            resolve_single_arm_args(params, &evaluated_args, line)?;
                        for (idx, p) in params.iter().enumerate() {
                            let expected =
                                self.resolve_type(&entry.module_key, &p.ty, entry.decl.line)?;
                            let actual = &ordered_arg_tys[idx];
                            let compatible = if types_compatible_with_annotation(actual, &expected)
                                || (expected.is_read_only()
                                    && types_compatible_with_annotation(
                                        actual,
                                        expected.unwrap_read_only(),
                                    ))
                            {
                                true
                            } else {
                                ctx.effect == Effect::ReadOnlyObserver
                                    && types_compatible_with_annotation(
                                        actual.unwrap_read_only(),
                                        expected.unwrap_read_only(),
                                    )
                            };
                            if !compatible {
                                if actual.is_read_only()
                                    && !expected.is_read_only()
                                    && types_compatible_with_annotation(
                                        actual.unwrap_read_only(),
                                        &expected,
                                    )
                                {
                                    return Err(Diagnostic::at_line(
                                        DiagnosticCode::ObserverMutationForbidden,
                                        line,
                                        format!(
                                            "cannot pass ReadOnly value {actual} to non-ReadOnly parameter {} of {}",
                                            p.name, entry.decl.name
                                        ),
                                    ));
                                }
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::ImplicitCoercionForbidden,
                                    line,
                                    format!(
                                        "argument {} of {} expects {expected}, got {actual}",
                                        p.name, entry.decl.name
                                    ),
                                ));
                            }
                        }
                        if matches!(ctx.effect, Effect::ReadOnlyObserver | Effect::QExt) {
                            self.check_transitive_effect(&entry, &ordered_arg_tys, ctx.effect)?;
                        }
                        self.resolve_type(&entry.module_key, return_type, entry.decl.line)
                    }
                }
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

    fn definitely_returns(&self, stmts: &[Stmt]) -> bool {
        for s in stmts {
            match &s.kind {
                StmtKind::Return(_) => return true,
                StmtKind::If {
                    then_body,
                    elifs,
                    else_body,
                    ..
                } => {
                    if !else_body.is_empty()
                        && self.definitely_returns(then_body)
                        && elifs.iter().all(|(_, b)| self.definitely_returns(b))
                        && self.definitely_returns(else_body)
                    {
                        return true;
                    }
                }
                StmtKind::Match { arms, .. } => {
                    if self.match_is_syntactically_exhaustive(arms)
                        && arms.iter().all(|a| self.definitely_returns(&a.body))
                    {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn match_is_syntactically_exhaustive(&self, arms: &[MatchArm]) -> bool {
        let mut enum_covered: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for arm in arms {
            if arm.guard.is_some() {
                continue;
            }
            match &arm.pattern {
                Pattern::Wildcard | Pattern::Binding(_) => return true,
                Pattern::EnumVariant { path, bindings }
                    if bindings.iter().all(is_irrefutable_pattern) =>
                {
                    if path.len() >= 2 {
                        let ename = path[path.len() - 2].clone();
                        let vname = path[path.len() - 1].clone();
                        enum_covered.entry(ename).or_default().insert(vname);
                    }
                }
                _ => {}
            }
        }
        for (ename, covered) in enum_covered {
            for edef in self.enums.values() {
                if edef.name == ename
                    && edef.variants.iter().all(|(vn, _)| covered.contains(vn))
                {
                    return true;
                }
            }
        }
        false
    }
}

fn is_irrefutable_pattern(pat: &Pattern) -> bool {
    match pat {
        Pattern::Wildcard | Pattern::Binding(_) => true,
        Pattern::Tuple(ps) => ps.iter().all(is_irrefutable_pattern),
        _ => false,
    }
}

fn types_compatible_with_annotation(actual: &Type, expected: &Type) -> bool {
    if actual == expected {
        return true;
    }
    match (actual, expected) {
        (Type::List(a), Type::List(_)) if **a == Type::Unknown => true,
        (Type::Map(ak, av), Type::Map(_, _))
            if **ak == Type::Unknown && **av == Type::Unknown =>
        {
            true
        }
        _ => false,
    }
}

fn contains_unknown(ty: &Type) -> bool {
    match ty {
        Type::Unknown => true,
        Type::List(inner) | Type::ReadOnly(inner) | Type::QExt(inner) => contains_unknown(inner),
        Type::Map(k, v) => contains_unknown(k) || contains_unknown(v),
        Type::Tuple(elems) => elems.iter().any(contains_unknown),
        _ => false,
    }
}

fn resolve_single_arm_args(
    params: &[crate::ast::TypedParam],
    args: &[(Option<String>, Type)],
    line: usize,
) -> DecResult<Vec<Type>> {
    if args.len() != params.len() {
        return Err(Diagnostic::at_line(
            DiagnosticCode::TypeError,
            line,
            format!("expected {} arguments, got {}", params.len(), args.len()),
        ));
    }
    let mut slots: Vec<Option<Type>> = vec![None; params.len()];
    let mut pos_idx = 0usize;
    let mut saw_named = false;
    for (name_opt, ty) in args {
        match name_opt {
            None => {
                if saw_named {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        "positional argument cannot follow named argument",
                    ));
                }
                slots[pos_idx] = Some(ty.clone());
                pos_idx += 1;
            }
            Some(name) => {
                saw_named = true;
                let idx = params
                    .iter()
                    .position(|p| &p.name == name)
                    .ok_or_else(|| {
                        Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!("unknown named argument {name}"),
                        )
                    })?;
                if slots[idx].is_some() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("duplicate argument for parameter {name}"),
                    ));
                }
                slots[idx] = Some(ty.clone());
            }
        }
    }
    slots
        .into_iter()
        .enumerate()
        .map(|(i, s)| {
            s.ok_or_else(|| {
                Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    line,
                    format!("missing argument for parameter {}", params[i].name),
                )
            })
        })
        .collect()
}

fn find_var_scope_index(scopes: &[BTreeMap<String, VarBinding>], name: &str) -> Option<usize> {
    for (idx, scope) in scopes.iter().enumerate().rev() {
        if scope.contains_key(name) {
            return Some(idx);
        }
    }
    None
}

fn resolve_builtin_field_type(base: &Type, field: &str, line: usize) -> DecResult<Type> {
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
