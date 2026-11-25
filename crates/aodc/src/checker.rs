use std::collections::{BTreeMap, BTreeSet};

use aod_core::ternary::T1;

use crate::ast::{
    AssignTarget, BinaryOp, CallArg, ChamberDecl, ConstDecl, DecArm, DecArmParam, DecFnDecl,
    DecFnKind, EnumDecl, Expr, ExprKind, ForTarget, Item, MatchArm, ObserveDecl, Pattern,
    PrepareDecl, ProbeDecl, Program, Stmt, StmtKind, StructDecl, TypeExpr, UnaryOp,
};
use crate::diagnostics::{CompatMode, DecResult, Diagnostic, DiagnosticCode, SyntaxDiscipline};
use crate::modules::{
    builtin_namespace_items, canonical_builtin_name, ImportedSymbol, LoadedUnit,
};
use crate::types::{
    builtin_enum_variants, builtin_ns_exports_enum, is_v1_builtin_name, lookup_builtin, Effect,
    Type, BUILTIN_MOD, MODEL_STATUS_VARIANTS, RECIPIENT_STAGE_VARIANTS,
};

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
    value: Expr,
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
    /// Exact resolved return types for polymorphic `sum`/`dot` calls (needed when `[T]` is empty at runtime).
    pub poly_call_types: BTreeMap<(String, usize, String), Type>,
}

/// Exhaustively typecheck every module, struct, enum, function, overload arm, apparatus block,
/// and quarantine module in `unit`, enforcing all 5 compile-time firewalls (`P0-1`, `P0-A`, `P1-3`).
pub fn check_unit(mut unit: LoadedUnit, require_main: bool) -> DecResult<CheckedUnit> {
    let mut test_functions = Vec::new();
    let mut main_ret: Option<Type> = None;

    let (consts_map, fns_map, chambers_map, prepares_map, observes_map, poly_call_types) = {
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

        for item in &unit.entry_program.items {
            if let Item::DecFn(f) = item {
                if f.name == "main" {
                    match &f.kind {
                        DecFnKind::MultiArm { arms, .. } => {
                            let zero_arg: Vec<&DecArm> =
                                arms.iter().filter(|a| a.params.is_empty()).collect();
                            if zero_arg.is_empty()
                                || (unit.compat == CompatMode::V1 && zero_arg.len() != 1)
                            {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    f.line,
                                    "core program requires a zero-argument main() signature",
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
                        DecFnKind::MultiArm { arms, .. } => {
                            arms.iter().any(|a| a.params.is_empty())
                        }
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

        (
            checker.consts,
            checker.functions,
            checker.chambers,
            checker.prepares,
            checker.observes,
            checker.poly_call_types,
        )
    };

    write_back_program(
        "<entry>",
        &mut unit.entry_program,
        &consts_map,
        &fns_map,
        &chambers_map,
        &prepares_map,
        &observes_map,
    );
    for (mk, prog) in &mut unit.parsed_modules {
        write_back_program(
            mk,
            prog,
            &consts_map,
            &fns_map,
            &chambers_map,
            &prepares_map,
            &observes_map,
        );
    }

    let const_types: BTreeMap<(String, String), Type> = consts_map
        .into_iter()
        .map(|(k, entry)| (k, entry.ty))
        .collect();

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

    fn register_builtin_enums(&mut self) {
        if self.compat() != CompatMode::V2 {
            return;
        }
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

    fn lookup_user_type(&self, mod_key: &str, name: &str) -> Option<Type> {
        let base = self.base_module_of(mod_key);
        if name.contains("::") {
            if self.compat() != CompatMode::V2 {
                return None;
            }
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
            Some(ImportedSymbol::Builtin(canon))
                if self.compat() == CompatMode::V2 && builtin_enum_variants(canon).is_some() =>
            {
                return Some(Type::Enum {
                    module: BUILTIN_MOD.to_string(),
                    name: canon.clone(),
                });
            }
            _ => {}
        }
        if self.compat() == CompatMode::V2 && builtin_enum_variants(name).is_some() {
            return Some(Type::Enum {
                module: BUILTIN_MOD.to_string(),
                name: name.to_string(),
            });
        }
        None
    }

    fn resolve_type(&self, mod_key: &str, expr: &TypeExpr, line: usize) -> DecResult<Type> {
        let ty = Type::resolve_with(expr, self.compat(), line, &|name| {
            self.lookup_user_type(mod_key, name)
        })?;
        if (self.compat() == CompatMode::V2 || self.unit.discipline == SyntaxDiscipline::PureWaveV2)
            && ty.contains_str()
        {
            return Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                line,
                format!("legacy `str` type ({ty}) is forbidden in Dec v2; use `bytes`"),
            ));
        }
        Ok(ty)
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
                has_header_params,
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
                    if *has_header_params && arm.params.len() != header_params.len() {
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
        let mut val_expr = c.value.clone();
        let expected_opt = if let Some(ann) = &c.ty {
            let expected = self.resolve_type(mod_key, ann, c.line)?;
            if !is_qext && expected.is_qext() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::QExtLeakageForbidden,
                    c.line,
                    format!("QExt type forbidden on native const {}", c.name),
                ));
            }
            coerce_expr_to_expected(&mut val_expr, &expected);
            Some(expected)
        } else {
            None
        };
        let val_ty = self.check_expr(&mut val_expr, &scopes, &ctx)?;
        let final_ty = if let Some(expected) = expected_opt {
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
                value: val_expr,
            },
        );
        Ok(())
    }

    fn check_all_declarations(&mut self) -> DecResult<()> {
        let fn_keys: Vec<(String, String)> = self.functions.keys().cloned().collect();
        for key in fn_keys {
            let mut entry = self.functions.get(&key).cloned().unwrap();
            let eff = if entry.is_qext {
                Effect::QExt
            } else {
                Effect::PureNative
            };
            self.check_fn_entry(&mut entry, None, eff)?;
            self.functions.insert(key, entry);
        }

        let chambers: Vec<(String, ChamberDecl)> = self
            .chambers
            .iter()
            .map(|((m, _), c)| (m.clone(), c.clone()))
            .collect();
        for (mod_key, mut ch) in chambers {
            let ctx = CheckContext {
                module_key: mod_key.clone(),
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
            let space_ty = self.check_expr(&mut ch.space, &scopes, &ctx)?;
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
            self.chambers.insert((mod_key, ch.name.clone()), ch);
        }

        let prepares: Vec<(String, PrepareDecl)> = self
            .prepares
            .iter()
            .map(|((m, _), p)| (m.clone(), p.clone()))
            .collect();
        for (mod_key, mut prep) in prepares {
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
                module_key: mod_key.clone(),
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
            for stmt in &mut prep.lets {
                self.check_stmt(stmt, &mut scopes, &mut ctx)?;
            }
            for clk in &mut prep.clocks {
                let ty = self.check_expr(&mut clk.period, &scopes, &ctx)?;
                if ty != Type::Int {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        clk.line,
                        format!("clock {} period must be int, got {ty}", clk.name),
                    ));
                }
            }
            for inj in &mut prep.injections {
                let s_ty = self.check_expr(&mut inj.support, &scopes, &ctx)?;
                if s_ty != Type::Support {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        inj.line,
                        format!("inject support must have type support, got {s_ty}"),
                    ));
                }
                let p_ty = self.check_expr(&mut inj.pos, &scopes, &ctx)?;
                if p_ty != Type::Tuple(vec![Type::Int, Type::Int, Type::Int]) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        inj.line,
                        format!("inject position must be (int, int, int), got {p_ty}"),
                    ));
                }
                let o_ty = self.check_expr(&mut inj.orient, &scopes, &ctx)?;
                if o_ty != Type::Int && o_ty != Type::T1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        inj.line,
                        format!("inject orient must be int or t1, got {o_ty}"),
                    ));
                }
                let sh_ty = self.check_expr(&mut inj.shells, &scopes, &ctx)?;
                if sh_ty != Type::Int {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        inj.line,
                        format!("inject shells must be int, got {sh_ty}"),
                    ));
                }
            }
            self.prepares.insert((mod_key, prep.name.clone()), prep);
        }

        let observes: Vec<(String, ObserveDecl)> = self
            .observes
            .iter()
            .map(|((m, _), o)| (m.clone(), o.clone()))
            .collect();
        for (mod_key, mut obs) in observes {
            for probe in &mut obs.probes {
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
                        for stmt in body.iter_mut() {
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
            self.observes.insert((mod_key, obs.name.clone()), obs);
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
            Pattern::NullLit => Ok(Some(Type::Null)),
            Pattern::IntLit(_) => Ok(Some(Type::Int)),
            Pattern::U8Lit(_) => Ok(Some(Type::U8)),
            Pattern::B64Lit(_) => Ok(Some(Type::B64)),
            Pattern::BoolLit(_) => Ok(Some(Type::Bool)),
            Pattern::StrLit(_) => Ok(Some(Type::Str)),
            Pattern::BytesLit(_) => Ok(Some(Type::Bytes)),
            Pattern::TritLit(_) => Ok(Some(Type::T1)),
            Pattern::T9Lit(_) => Ok(Some(Type::T9)),
            Pattern::T32Lit(_) => Ok(Some(Type::T32)),
            Pattern::Q4Lit(_) => Ok(Some(Type::Q4)),
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
        entry: &mut FnEntry,
        override_arg_types: Option<&[Type]>,
        effect: Effect,
    ) -> DecResult<()> {
        let mod_key = entry.module_key.clone();
        let fn_name = entry.decl.name.clone();
        let fn_line = entry.decl.line;
        let is_main = mod_key == "<entry>" && fn_name == "main" && !entry.is_qext;
        let same_mod_struct_fields: Option<Vec<Type>> = if !entry.is_qext {
            self.structs
                .get(&(mod_key.clone(), fn_name.clone()))
                .map(|sd| sd.fields.iter().map(|(_, t)| t.clone()).collect())
        } else {
            None
        };

        match &mut entry.decl.kind {
            DecFnKind::MultiArm {
                has_header_params,
                header_typed_params,
                arms,
                ..
            } => {
                // 1. Contextual `t1` pattern promotion (`-1` / `0` / `1` -> `TritLit`, `P1-1`) when `!*has_header_params`
                if !*has_header_params && override_arg_types.is_none() {
                    let mut arities = BTreeSet::new();
                    for arm in arms.iter() {
                        arities.insert(arm.params.len());
                    }
                    for arity in arities {
                        for slot in 0..arity {
                            let mut has_t1 = false;
                            let mut has_explicit_non_t1_int = false;
                            for arm in arms.iter().filter(|a| a.params.len() == arity) {
                                match &arm.params[slot] {
                                    DecArmParam::TypedVar { ty, .. }
                                    | DecArmParam::TypedPattern { ty, .. } => {
                                        let rt = self.resolve_type(&mod_key, ty, arm.line)?;
                                        if rt == Type::T1 {
                                            has_t1 = true;
                                        } else if matches!(
                                            rt,
                                            Type::Int
                                                | Type::U8
                                                | Type::T9
                                                | Type::T32
                                                | Type::Q4
                                                | Type::B64
                                        ) {
                                            has_explicit_non_t1_int = true;
                                        }
                                    }
                                    DecArmParam::Pattern(Pattern::TritLit(_)) => {
                                        has_t1 = true;
                                    }
                                    DecArmParam::Pattern(
                                        Pattern::U8Lit(_)
                                        | Pattern::B64Lit(_)
                                        | Pattern::T9Lit(_)
                                        | Pattern::T32Lit(_)
                                        | Pattern::Q4Lit(_),
                                    ) => {
                                        has_explicit_non_t1_int = true;
                                    }
                                    _ => {}
                                }
                            }
                            if has_t1 && !has_explicit_non_t1_int {
                                for arm in arms.iter_mut().filter(|a| a.params.len() == arity) {
                                    if let DecArmParam::Pattern(Pattern::IntLit(ref n)) =
                                        arm.params[slot]
                                    {
                                        let trit = T1::from_bigint(n).map_err(|_| {
                                            Diagnostic::at_line(
                                                DiagnosticCode::InvalidTritLiteral,
                                                arm.line,
                                                format!(
                                                    "contextual trit pattern in t1 slot must be -1, 0, or 1, got {n}"
                                                ),
                                            )
                                        })?;
                                        arm.params[slot] =
                                            DecArmParam::Pattern(Pattern::TritLit(trit));
                                    }
                                }
                            }
                        }
                    }
                }

                // 2. Collect per-arity slot hints (`Empty | Unique(Type) | Conflict`)
                #[derive(Clone)]
                enum SlotHint {
                    Empty,
                    Unique(Type),
                    Conflict,
                }
                let mut slot_hints_by_arity: BTreeMap<usize, Vec<SlotHint>> = BTreeMap::new();
                if let Some(htp) = header_typed_params.as_ref() {
                    let slots = slot_hints_by_arity
                        .entry(htp.len())
                        .or_insert_with(|| vec![SlotHint::Empty; htp.len()]);
                    for (i, p) in htp.iter().enumerate() {
                        slots[i] = SlotHint::Unique(self.resolve_type(&mod_key, &p.ty, fn_line)?);
                    }
                }
                for arm in arms.iter() {
                    let arity = arm.params.len();
                    let htp_opt = header_typed_params
                        .as_deref()
                        .filter(|ht| ht.len() == arity);
                    let tys = self.infer_arm_param_types(&mod_key, arm, htp_opt)?;
                    let slots = slot_hints_by_arity
                        .entry(arity)
                        .or_insert_with(|| vec![SlotHint::Empty; arity]);
                    for (i, t) in tys.into_iter().enumerate() {
                        if t != Type::Unknown {
                            slots[i] = match &slots[i] {
                                SlotHint::Empty => SlotHint::Unique(t),
                                SlotHint::Unique(prev) if prev == &t => SlotHint::Unique(t),
                                SlotHint::Unique(_) | SlotHint::Conflict => SlotHint::Conflict,
                            };
                        }
                    }
                }

                let mut group_ret_types: BTreeMap<Vec<Type>, Type> = BTreeMap::new();
                let mut caught_all_groups: BTreeSet<Vec<Type>> = BTreeSet::new();
                for arm in arms.iter_mut() {
                    let arity = arm.params.len();
                    let htp_opt = header_typed_params
                        .as_deref()
                        .filter(|ht| ht.len() == arity);
                    let arm_tys = self.infer_arm_param_types(&mod_key, arm, htp_opt)?;
                    let slots = &slot_hints_by_arity[&arity];
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
                    let ret_ty = self.resolve_type(&mod_key, &arm.return_type, arm.line)?;
                    let mut concrete_group_sig = Vec::with_capacity(arity);
                    for idx in 0..arity {
                        if arm_tys[idx] != Type::Unknown {
                            concrete_group_sig.push(arm_tys[idx].clone());
                        } else {
                            match &slots[idx] {
                                SlotHint::Unique(t) => concrete_group_sig.push(t.clone()),
                                SlotHint::Empty if !*has_header_params => {
                                    return Err(Diagnostic::at_line(
                                        DiagnosticCode::TypeError,
                                        arm.line,
                                        format!(
                                            "ambiguous untyped parameter in {fn_name} slot {}; add `: Type`",
                                            idx + 1
                                        ),
                                    ));
                                }
                                SlotHint::Conflict if !*has_header_params => {
                                    return Err(Diagnostic::at_line(
                                        DiagnosticCode::TypeError,
                                        arm.line,
                                        format!(
                                            "conflicting types for untyped parameter in {fn_name} slot {}; use `_: Type` or `x: Type`",
                                            idx + 1
                                        ),
                                    ));
                                }
                                SlotHint::Empty | SlotHint::Conflict => {
                                    concrete_group_sig.push(Type::Int);
                                }
                            }
                        }
                    }

                    if override_arg_types.is_none() {
                        if let Some(ref struct_fields) = same_mod_struct_fields {
                            let expected_struct_ty = Type::Struct {
                                module: mod_key.clone(),
                                name: fn_name.clone(),
                            };
                            if ret_ty != expected_struct_ty {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    arm.line,
                                    format!(
                                        "constructor overload `dec {fn_name}:` must return {fn_name}, got {ret_ty}"
                                    ),
                                ));
                            }
                            if &concrete_group_sig == struct_fields {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    arm.line,
                                    format!(
                                        "constructor overload `dec {fn_name}:` arm duplicates canonical struct field signature"
                                    ),
                                ));
                            }
                        }

                        if let Some(prev_ret) = group_ret_types.get(&concrete_group_sig) {
                            if prev_ret != &ret_ty {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    arm.line,
                                    format!(
                                        "conflicting return types ({prev_ret} vs {ret_ty}) across arms with identical parameter types in {fn_name}"
                                    ),
                                ));
                            }
                        } else {
                            group_ret_types.insert(concrete_group_sig.clone(), ret_ty.clone());
                        }

                        if caught_all_groups.contains(&concrete_group_sig) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                arm.line,
                                format!(
                                    "unreachable dead arm in {fn_name} after unguarded catch-all row"
                                ),
                            ));
                        }
                        if arm.guard.is_none() && arm.params.iter().all(is_irrefutable_arm_param) {
                            caught_all_groups.insert(concrete_group_sig.clone());
                        }
                    }

                    let mut scope = BTreeMap::new();
                    for (idx, param) in arm.params.iter().enumerate() {
                        let mut resolved_ty = if let Some(ov) = override_arg_types {
                            ov[idx].clone()
                        } else {
                            concrete_group_sig[idx].clone()
                        };
                        if arm_tys[idx].is_read_only() {
                            resolved_ty = resolved_ty.wrap_read_only();
                        }
                        match param {
                            DecArmParam::TypedVar { name, .. } => {
                                if scope.contains_key(name) {
                                    return Err(Diagnostic::at_line(
                                        DiagnosticCode::TypeError,
                                        arm.line,
                                        format!("duplicate parameter binding {name}"),
                                    ));
                                }
                                let param_mutable = self.unit.discipline
                                    != SyntaxDiscipline::PureWaveV2
                                    && !resolved_ty.is_read_only();
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
                                    &mod_key,
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
                        module_key: mod_key.clone(),
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
                    if let Some(ref mut guard) = arm.guard {
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
                    for stmt in arm.body.iter_mut() {
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
                ..
            } => {
                let param_types: Vec<Type> = params
                    .iter()
                    .map(|p| self.resolve_type(&mod_key, &p.ty, fn_line))
                    .collect::<DecResult<_>>()?;
                let ret_ty = self.resolve_type(&mod_key, return_type, fn_line)?;
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
                    module_key: mod_key,
                    effect,
                    in_main: is_main && params.is_empty() && effect == Effect::PureNative,
                    in_qext_sink: false,
                    qext_outer_scope_depth: 0,
                    qext_outer_loop_depth: 0,
                    loop_depth: 0,
                    expected_return: ret_ty.clone(),
                    observer_local_floor: 1,
                };
                for stmt in body.iter_mut() {
                    self.check_stmt(stmt, &mut scopes, &mut ctx)?;
                }
                if ret_ty != Type::Unit && !self.definitely_returns(body) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        fn_line,
                        format!("function {fn_name} missing return of type {ret_ty}"),
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
                        mutable: false,
                        initialized: true,
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
            Pattern::U8Lit(_) => {
                if *target != Type::U8 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("u8 pattern does not match {target}"),
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
            Pattern::T9Lit(_) => {
                if *target != Type::T9 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("t9 pattern does not match {target}"),
                    ));
                }
                Ok(())
            }
            Pattern::T32Lit(_) => {
                if *target != Type::T32 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("t32 pattern does not match {target}"),
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
            } else if let Some(imp) = self.imports_for(mod_key).get(&path[0]) {
                match imp {
                    ImportedSymbol::ModuleItem { module, item } => {
                        (module.clone(), item.clone(), path[1].clone())
                    }
                    ImportedSymbol::Builtin(canon)
                        if self.compat() == CompatMode::V2
                            && builtin_enum_variants(canon).is_some() =>
                    {
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
            } else if self.compat() == CompatMode::V2
                && !self
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
                    if self.compat() == CompatMode::V2
                        && builtin_ns_exports_enum(ns_tail, &path[1])
                    {
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
            && self.compat() == CompatMode::V2
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
        stmt: &mut Stmt,
        scopes: &mut Vec<BTreeMap<String, VarBinding>>,
        ctx: &mut CheckContext,
    ) -> DecResult<()> {
        match &mut stmt.kind {
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
                        coerce_expr_to_expected(value, &expected);
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
            StmtKind::LetTuple { elems, ty, value } => {
                let mut seen_names = BTreeSet::new();
                for el in elems.iter() {
                    if el.name != "_" {
                        if !seen_names.insert(el.name.as_str()) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                stmt.line,
                                format!("duplicate tuple destructuring binding {}", el.name),
                            ));
                        }
                        if find_var_scope_index(scopes, &el.name).is_some() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                stmt.line,
                                format!("tuple destructuring binding {} shadows existing local", el.name),
                            ));
                        }
                    }
                }
                let outer_expected_opt = match ty {
                    Some(te) => {
                        let resolved = self.resolve_type(&ctx.module_key, te, stmt.line)?;
                        if ctx.effect != Effect::QExt && resolved.is_qext() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::QExtLeakageForbidden,
                                stmt.line,
                                "QExt type annotation forbidden on native tuple let",
                            ));
                        }
                        Some(resolved)
                    }
                    None => None,
                };
                let mut elem_expected_opts: Vec<Option<Type>> = Vec::with_capacity(elems.len());
                for (idx, el) in elems.iter().enumerate() {
                    let ann = match &el.ty {
                        Some(ete) => {
                            let ety = self.resolve_type(&ctx.module_key, ete, stmt.line)?;
                            if ctx.effect != Effect::QExt && ety.is_qext() {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::QExtLeakageForbidden,
                                    stmt.line,
                                    format!("QExt type annotation forbidden on native let {}", el.name),
                                ));
                            }
                            Some(ety)
                        }
                        None => match &outer_expected_opt {
                            Some(outer) => match outer.unwrap_read_only() {
                                Type::Tuple(ts) if ts.len() == elems.len() => {
                                    let mut t = ts[idx].clone();
                                    if outer.is_read_only() && t.needs_readonly_propagation() {
                                        t = t.wrap_read_only();
                                    }
                                    Some(t)
                                }
                                _ => None,
                            },
                            None => None,
                        },
                    };
                    if let Some(Type::Map(ref k, _)) = ann {
                        if !k.is_map_key() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                stmt.line,
                                format!("unsupported map key type {k}"),
                            ));
                        }
                    }
                    elem_expected_opts.push(ann);
                }

                if elem_expected_opts.iter().all(Option::is_some) {
                    let expected_tup = Type::Tuple(
                        elem_expected_opts
                            .iter()
                            .map(|o| o.clone().unwrap())
                            .collect(),
                    );
                    coerce_expr_to_expected(value, &expected_tup);
                } else if let Some(ref outer_exp) = outer_expected_opt {
                    coerce_expr_to_expected(value, outer_exp);
                } else if let ExprKind::Tuple(ref mut rhs_elems) = value.kind {
                    if rhs_elems.len() == elems.len() {
                        for (re, exp_opt) in rhs_elems.iter_mut().zip(&elem_expected_opts) {
                            if let Some(exp) = exp_opt {
                                coerce_expr_to_expected(re, exp);
                            }
                        }
                    }
                }

                let val_ty = self.check_expr(value, scopes, ctx)?;
                if ctx.effect != Effect::QExt && val_ty.is_qext() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        stmt.line,
                        "QExt value cannot bind to native tuple let",
                    ));
                }

                let val_is_ro = val_ty.is_read_only();
                let raw_tuple_tys = match val_ty.unwrap_read_only() {
                    Type::Tuple(ts) => ts.clone(),
                    other => {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            stmt.line,
                            format!(
                                "tuple destructuring `let (...) = expr` requires tuple RHS, got {other}"
                            ),
                        ));
                    }
                };
                if raw_tuple_tys.len() != elems.len() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        stmt.line,
                        format!(
                            "tuple destructuring arity mismatch: pattern has {} elements, RHS has {}",
                            elems.len(),
                            raw_tuple_tys.len()
                        ),
                    ));
                }

                if let Some(ref outer_exp) = outer_expected_opt {
                    if val_is_ro && !outer_exp.is_read_only() {
                        match outer_exp {
                            Type::Tuple(ots)
                                if ots.len() == raw_tuple_tys.len()
                                    && raw_tuple_tys
                                        .iter()
                                        .zip(ots)
                                        .any(|(r, o)| r.needs_readonly_propagation() && !o.is_read_only()) =>
                            {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::ObserverMutationForbidden,
                                    stmt.line,
                                    format!("cannot strip ReadOnly from {val_ty} in tuple destructuring"),
                                ));
                            }
                            _ => {}
                        }
                    }
                    let Type::Tuple(ots) = outer_exp.unwrap_read_only() else {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            stmt.line,
                            format!("outer tuple let annotation must be a tuple type, got {outer_exp}"),
                        ));
                    };
                    if ots.len() != elems.len() {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            stmt.line,
                            format!(
                                "tuple let annotation arity ({}) does not match pattern ({})",
                                ots.len(),
                                elems.len()
                            ),
                        ));
                    }
                }

                let cur = scopes.last_mut().expect("scope stack non-empty");
                for (idx, el) in elems.iter().enumerate() {
                    let mut actual_i = raw_tuple_tys[idx].clone();
                    if val_is_ro && actual_i.needs_readonly_propagation() {
                        actual_i = actual_i.wrap_read_only();
                    }
                    let final_i = match &elem_expected_opts[idx] {
                        Some(expected_i) => {
                            if !types_compatible_with_annotation(&actual_i, expected_i) {
                                if actual_i.is_read_only()
                                    && !expected_i.is_read_only()
                                    && types_compatible_with_annotation(
                                        actual_i.unwrap_read_only(),
                                        expected_i,
                                    )
                                {
                                    return Err(Diagnostic::at_line(
                                        DiagnosticCode::ObserverMutationForbidden,
                                        stmt.line,
                                        format!(
                                            "cannot strip ReadOnly from {actual_i} in tuple binding {}",
                                            el.name
                                        ),
                                    ));
                                }
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::ImplicitCoercionForbidden,
                                    stmt.line,
                                    format!(
                                        "tuple element {} declared as {expected_i}, got {actual_i}",
                                        el.name
                                    ),
                                ));
                            }
                            expected_i.clone()
                        }
                        None => {
                            if actual_i == Type::Unit || contains_unknown(&actual_i) {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    stmt.line,
                                    format!(
                                        "tuple element {} requires an explicit concrete type annotation",
                                        el.name
                                    ),
                                ));
                            }
                            actual_i
                        }
                    };
                    if el.name != "_" {
                        cur.insert(
                            el.name.clone(),
                            VarBinding {
                                ty: final_i,
                                mutable: false,
                                initialized: true,
                            },
                        );
                    }
                }
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
                        coerce_expr_to_expected(value, &expected);
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
                coerce_expr_to_expected(value, &expected);
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
            StmtKind::Assign { target, value } => match target {
                AssignTarget::Ident(name) => {
                    let found = find_var_scope_index(scopes, name);
                    if let Some(idx) = found {
                        let expected_ty = scopes[idx][name.as_str()].ty.clone();
                        coerce_expr_to_expected(value, &expected_ty);
                    }
                    let val_ty = self.check_expr(value, scopes, ctx)?;
                    let (scope_idx, binding_ty, mutable) = match found {
                        Some(idx) => {
                            let b = &scopes[idx][name.as_str()];
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
                    scopes[scope_idx].get_mut(name.as_str()).unwrap().initialized = true;
                    Ok(())
                }
                AssignTarget::Field { base, name } => {
                    let base_ty = self.check_expr(base, scopes, ctx)?;
                    if let Type::Struct {
                        ref module,
                        name: ref sname,
                    } = *base_ty.unwrap_read_only()
                    {
                        if let Ok(fty) = self.resolve_struct_field(module, sname, name, stmt.line) {
                            coerce_expr_to_expected(value, &fty);
                        }
                    }
                    let val_ty = self.check_expr(value, scopes, ctx)?;
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
                                let field_ty =
                                    self.resolve_struct_field(module, sname, name, stmt.line)?;
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
                    match base_ty.unwrap_read_only() {
                        Type::List(elem_ty) => {
                            coerce_expr_to_expected(value, elem_ty);
                        }
                        Type::Map(k_ty, v_ty) => {
                            coerce_expr_to_expected(index, k_ty);
                            coerce_expr_to_expected(value, v_ty);
                        }
                        _ => {}
                    }
                    let idx_ty = self.check_expr(index, scopes, ctx)?;
                    let val_ty = self.check_expr(value, scopes, ctx)?;
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
            },
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
                    Some(e) => {
                        coerce_expr_to_expected(e, &ctx.expected_return);
                        self.check_expr(e, scopes, ctx)?
                    }
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
                for s in then_body.iter_mut() {
                    self.check_stmt(s, scopes, ctx)?;
                }
                scopes.pop();
                for (ec, eb) in elifs.iter_mut() {
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
                    for s in eb.iter_mut() {
                        self.check_stmt(s, scopes, ctx)?;
                    }
                    scopes.pop();
                }
                if !else_body.is_empty() {
                    scopes.push(BTreeMap::new());
                    for s in else_body.iter_mut() {
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

                for arm in arms.iter_mut() {
                    let mut arm_scope = BTreeMap::new();
                    self.check_pattern(
                        &arm.pattern,
                        &scrut_ty,
                        &ctx.module_key,
                        &mut arm_scope,
                        arm.line,
                    )?;
                    scopes.push(arm_scope);
                    if let Some(ref mut g) = arm.guard {
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
                            Pattern::Wildcard | Pattern::Binding(_) | Pattern::NullLit => {
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
                    for s in arm.body.iter_mut() {
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
                for s in body.iter_mut() {
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
                for s in body.iter_mut() {
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
                    for s in body.iter_mut() {
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
        expr: &mut Expr,
        scopes: &[BTreeMap<String, VarBinding>],
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        let expr_line = expr.line;
        let ty = self.check_expr_inner(expr, scopes, ctx)?;
        if self.unit.discipline == SyntaxDiscipline::PureWaveV2 && ty.contains_str() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                expr_line,
                format!("legacy `str` value ({ty}) is forbidden in --pure-wave; use `bytes`"),
            ));
        }
        Ok(ty)
    }

    fn check_expr_inner(
        &mut self,
        expr: &mut Expr,
        scopes: &[BTreeMap<String, VarBinding>],
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        let expr_line = expr.line;
        match &mut expr.kind {
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
                // 1. Check unit enum variant `Enum::Variant`, `mod::Enum::Variant`, or `aod::<ns>::Enum::Variant`
                if matches!(segs.len(), 2..=4) {
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
                    .resolve_struct_path(&ctx.module_key, path, expr_line)?
                    .clone();
                if fields.len() != sdef.fields.len() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr_line,
                        format!(
                            "struct {} expects {} field(s), got {}",
                            sdef.name,
                            sdef.fields.len(),
                            fields.len()
                        ),
                    ));
                }
                let mut seen = BTreeSet::new();
                for (fname, fexpr) in fields.iter_mut() {
                    if !seen.insert(fname.clone()) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            expr_line,
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
                                expr_line,
                                format!("unknown field {fname} on struct {}", sdef.name),
                            )
                        })?;
                    coerce_expr_to_expected(fexpr, &expected);
                    let actual = self.check_expr(fexpr, scopes, ctx)?;
                    if !types_compatible_with_annotation(&actual, &expected) {
                        if actual.is_read_only()
                            && !expected.is_read_only()
                            && types_compatible_with_annotation(actual.unwrap_read_only(), &expected)
                        {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ObserverMutationForbidden,
                                fexpr.line,
                                format!(
                                    "cannot strip ReadOnly from {actual} in struct {} field {fname}",
                                    sdef.name
                                ),
                            ));
                        }
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
                let k0_line = entries[0].0.line;
                let first_k = self.check_expr(&mut entries[0].0, scopes, ctx)?;
                let first_v = self.check_expr(&mut entries[0].1, scopes, ctx)?;
                if ctx.effect != Effect::QExt && (first_k.is_qext() || first_v.is_qext()) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        expr_line,
                        "QExt keys/values are forbidden in native map literals",
                    ));
                }
                if !first_k.is_map_key() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        k0_line,
                        format!("unsupported map key type {first_k}"),
                    ));
                }
                for (ke, ve) in entries[1..].iter_mut() {
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
                let first = self.check_expr(&mut items[0], scopes, ctx)?;
                for it in items[1..].iter_mut() {
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
            ExprKind::ArrayLit(items) => {
                if self.compat() == CompatMode::V1 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr_line,
                        "array[T, N] is not in --compat v1",
                    ));
                }
                if items.is_empty() || items.len() > 65_536 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr_line,
                        format!(
                            "array[T, N] requires 1 <= N <= 65536 elements, got {}",
                            items.len()
                        ),
                    ));
                }
                let first = self.check_expr(&mut items[0], scopes, ctx)?;
                if first == Type::Unit || contains_unknown(&first) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr_line,
                        "array[T, N] elements require a concrete non-unit type",
                    ));
                }
                if ctx.effect != Effect::QExt && first.is_qext() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        expr_line,
                        "QExt elements are forbidden in native array literals",
                    ));
                }
                for it in items[1..].iter_mut() {
                    let ty = self.check_expr(it, scopes, ctx)?;
                    if !types_compatible_with_annotation(&ty, &first) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ImplicitCoercionForbidden,
                            it.line,
                            format!("heterogeneous array elements: expected {first}, got {ty}"),
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
                for e in elems.iter_mut() {
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
                        expr_line,
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
                        expr_line,
                        format!("range a..b endpoints must be int, got {s_ty}..{e_ty}"),
                    ));
                }
                Ok(Type::Range)
            }
            ExprKind::BoxExpr(dims) => {
                if dims.len() != 3 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr_line,
                        format!("box([...]) requires 3 axis ranges, got {}", dims.len()),
                    ));
                }
                for d in dims.iter_mut() {
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
                        expr_line,
                        "simulate is forbidden inside read-only observer context",
                    ));
                }
                if ctx.effect == Effect::QExt {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        expr_line,
                        "simulate cannot be invoked inside quarantine Q_ext",
                    ));
                }
                if !self
                    .prepares
                    .contains_key(&(ctx.module_key.clone(), prepare.clone()))
                {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr_line,
                        format!("undefined prepare declaration {prepare}"),
                    ));
                }
                let t_ty = self.check_expr(ticks, scopes, ctx)?;
                if t_ty != Type::Int {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr_line,
                        format!("simulate tick count must be int, got {t_ty}"),
                    ));
                }
                for obs in observers.iter() {
                    if !self
                        .observes
                        .contains_key(&(ctx.module_key.clone(), obs.clone()))
                    {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            expr_line,
                            format!("undefined observe declaration {obs}"),
                        ));
                    }
                }
                Ok(Type::SimResult)
            }
            ExprKind::Unary { op, operand } => {
                let ty = self.check_expr(operand, scopes, ctx)?;
                let inner = ty.unwrap_read_only().clone();
                match op {
                    UnaryOp::Not => {
                        if inner == Type::Bool {
                            Ok(Type::Bool)
                        } else if ctx.effect == Effect::QExt
                            && inner == Type::QExt(Box::new(Type::Bool))
                        {
                            Ok(Type::QExt(Box::new(Type::Bool)))
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr_line,
                                format!("invalid unary `not` on {ty}"),
                            ))
                        }
                    }
                    UnaryOp::Pos => {
                        if inner.is_numeric_v1() || matches!(inner, Type::U8 | Type::T9) {
                            Ok(inner)
                        } else if ctx.effect == Effect::QExt
                            && inner == Type::QExt(Box::new(Type::Ratio))
                        {
                            Ok(inner)
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr_line,
                                format!("invalid unary `+` on {ty}"),
                            ))
                        }
                    }
                    UnaryOp::Neg => {
                        if inner.is_numeric_v1() || inner == Type::T9 {
                            Ok(inner)
                        } else if ctx.effect == Effect::QExt
                            && inner == Type::QExt(Box::new(Type::Ratio))
                        {
                            Ok(inner)
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr_line,
                                format!("invalid unary `-` on {ty}"),
                            ))
                        }
                    }
                    UnaryOp::Boundary => match &inner {
                        Type::Box3 | Type::SimResult => Ok(Type::Bytes),
                        Type::Window | Type::Range => Ok(Type::Tuple(vec![Type::Int, Type::Int])),
                        other => Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            expr_line,
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
                if matches!(op, BinaryOp::Shl | BinaryOp::Shr) && self.compat() == CompatMode::V2 {
                    if *l_inner == Type::U8 && (*r_inner == Type::U8 || *r_inner == Type::Int) {
                        return Ok(Type::U8);
                    }
                    if *l_inner == Type::B64 && (*r_inner == Type::B64 || *r_inner == Type::Int) {
                        return Ok(Type::B64);
                    }
                }
                if l_inner != r_inner {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ImplicitCoercionForbidden,
                        expr_line,
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
                                expr_line,
                                format!("Boolean operation requires bool operands, got {ty}"),
                            ))
                        }
                    }
                    BinaryOp::Eq | BinaryOp::NotEq => {
                        if ty == Type::Unit {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr_line,
                                "cannot compare unit ()",
                            ));
                        }
                        if ty.is_qext() {
                            if ctx.effect != Effect::QExt {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::QExtLeakageForbidden,
                                    expr_line,
                                    "cannot compare QExt values outside quarantine Q_ext",
                                ));
                            }
                            Ok(Type::QExt(Box::new(Type::Bool)))
                        } else {
                            Ok(Type::Bool)
                        }
                    }
                    BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq => {
                        if ty.is_numeric_v1() || matches!(ty, Type::U8 | Type::B64 | Type::T9) {
                            Ok(Type::Bool)
                        } else if ctx.effect == Effect::QExt
                            && ty == Type::QExt(Box::new(Type::Ratio))
                        {
                            Ok(Type::QExt(Box::new(Type::Bool)))
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr_line,
                                format!("ordering requires an ordered scalar type, got {ty}"),
                            ))
                        }
                    }
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => {
                        if ty.is_numeric_v1() || matches!(ty, Type::U8 | Type::T9) {
                            Ok(ty)
                        } else if ctx.effect == Effect::QExt
                            && ty == Type::QExt(Box::new(Type::Ratio))
                        {
                            Ok(ty)
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr_line,
                                format!("arithmetic requires matching explicit numeric types, got {ty}"),
                            ))
                        }
                    }
                    BinaryOp::Div => {
                        if ty.is_numeric_v1() || matches!(ty, Type::U8 | Type::T9) {
                            Ok(Type::Ratio)
                        } else if ctx.effect == Effect::QExt
                            && ty == Type::QExt(Box::new(Type::Ratio))
                        {
                            Ok(Type::QExt(Box::new(Type::Ratio)))
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr_line,
                                format!("division requires numeric type, got {ty}"),
                            ))
                        }
                    }
                    BinaryOp::FloorDiv | BinaryOp::Mod => {
                        if matches!(ty, Type::Int | Type::U8 | Type::T9 | Type::T32) {
                            Ok(ty)
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr_line,
                                format!("quotient/remainder require int, u8, t9, or t32, got {ty}"),
                            ))
                        }
                    }
                    BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor => {
                        if self.compat() == CompatMode::V1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr_line,
                                "bitwise operators are not in --compat v1",
                            ));
                        }
                        if matches!(ty, Type::Int | Type::U8 | Type::B64 | Type::Q4) {
                            Ok(ty)
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr_line,
                                format!("bitwise ops require int, u8, b64, or q4, got {ty}"),
                            ))
                        }
                    }
                    BinaryOp::Shl | BinaryOp::Shr => {
                        if self.compat() == CompatMode::V1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr_line,
                                "bitwise shifts are not in --compat v1",
                            ));
                        }
                        if matches!(ty, Type::Int | Type::U8 | Type::B64) {
                            Ok(ty)
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr_line,
                                format!("shift ops require int, u8, or b64, got {ty}"),
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
                        expr_line,
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
                    self.resolve_struct_field(module, sname, name, expr_line)?
                } else {
                    resolve_builtin_field_type(inner, name, expr_line)?
                };
                if is_ro && field_ty.needs_readonly_propagation() {
                    Ok(field_ty.wrap_read_only())
                } else {
                    Ok(field_ty)
                }
            }
            ExprKind::Index { base, index } => {
                let base_ty = self.check_expr(base, scopes, ctx)?;
                if let Type::Map(k_ty, _) = base_ty.unwrap_read_only() {
                    coerce_expr_to_expected(index, k_ty);
                }
                let idx_ty = self.check_expr(index, scopes, ctx)?;
                let is_ro = base_ty.is_read_only();
                match base_ty.unwrap_read_only() {
                    Type::Bytes => {
                        if idx_ty.unwrap_read_only() != &Type::Int {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr_line,
                                format!("bytes index must be int, got {idx_ty}"),
                            ));
                        }
                        Ok(Type::U8)
                    }
                    Type::Array(elem, _) => {
                        if idx_ty.unwrap_read_only() != &Type::Int {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr_line,
                                format!("array index must be int, got {idx_ty}"),
                            ));
                        }
                        if is_ro && elem.needs_readonly_propagation() {
                            Ok((**elem).clone().wrap_read_only())
                        } else {
                            Ok((**elem).clone())
                        }
                    }
                    Type::List(elem) => {
                        if idx_ty.unwrap_read_only() != &Type::Int {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr_line,
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
                        if idx_ty.unwrap_read_only() != &**k_ty {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ImplicitCoercionForbidden,
                                expr_line,
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
                        expr_line,
                        format!("cannot index value of type {other}"),
                    )),
                }
            }
            ExprKind::Call { callee, args } => {
                self.check_call(callee, args, expr_line, scopes, ctx)
            }
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

    fn precoerce_call_args(
        &self,
        callee: &Expr,
        args: &mut [CallArg],
        line: usize,
        ctx: &CheckContext,
    ) {
        // 1. Enum variant constructor
        if let ExprKind::Path(ref segs) = callee.kind {
            if segs.len() == 2 || segs.len() == 3 {
                if let Ok((_, payload_tys)) =
                    self.resolve_enum_variant(&ctx.module_key, segs, line)
                {
                    if args.len() == payload_tys.len() && args.iter().all(|a| a.name.is_none()) {
                        for (a, pty) in args.iter_mut().zip(payload_tys) {
                            coerce_expr_to_expected(&mut a.value, pty);
                        }
                        return;
                    }
                }
            }
        }

        // 2. Struct constructor (only precoerce slots without conflicting same-arity `dec Struct:` arm!)
        let sp_opt: Option<Vec<String>> = match &callee.kind {
            ExprKind::Ident(n) => Some(vec![n.clone()]),
            ExprKind::Path(segs) => Some(segs.clone()),
            _ => None,
        };
        if let Some(ref sp) = sp_opt {
            if let Ok(sdef) = self.resolve_struct_path(&ctx.module_key, sp, line).cloned() {
                let has_fn_overload = self
                    .functions
                    .get(&(sdef.module.clone(), sdef.name.clone()))
                    .is_some_and(|fe| match &fe.decl.kind {
                        DecFnKind::SingleArm { params, .. } => params.len() == args.len(),
                        DecFnKind::MultiArm { arms, .. } => {
                            arms.iter().any(|arm| arm.params.len() == args.len())
                        }
                    });
                if args.len() == sdef.fields.len()
                    && (!has_fn_overload || args.iter().any(|a| a.name.is_some()))
                {
                    let mut pos_idx = 0usize;
                    for a in args.iter_mut() {
                        match &a.name {
                            None => {
                                if let Some((_, fty)) = sdef.fields.get(pos_idx) {
                                    coerce_expr_to_expected(&mut a.value, fty);
                                }
                                pos_idx += 1;
                            }
                            Some(n) => {
                                if let Some((_, fty)) = sdef.fields.iter().find(|(fnm, _)| fnm == n)
                                {
                                    coerce_expr_to_expected(&mut a.value, fty);
                                }
                            }
                        }
                    }
                    return;
                }
            }
        }

        // 3. User function (SingleArm or MultiArm with unambiguous slot type)
        let base_mod = self.base_module_of(&ctx.module_key).to_string();
        let user_entry_opt = match &callee.kind {
            ExprKind::Ident(name) => self
                .functions
                .get(&(ctx.module_key.clone(), name.clone()))
                .or_else(|| self.functions.get(&(base_mod, name.clone())))
                .or_else(|| {
                    if let Some(ImportedSymbol::ModuleItem { module, item }) =
                        self.imports_for(&ctx.module_key).get(name)
                    {
                        self.functions.get(&(module.clone(), item.clone()))
                    } else {
                        None
                    }
                }),
            ExprKind::Path(segs) if segs.len() == 2 => {
                if let Some(qsub) = self.resolve_qext_submodule(&ctx.module_key, &segs[0]) {
                    self.functions.get(&(qsub, segs[1].clone()))
                } else if let Some(ImportedSymbol::ModuleNamespace(mod_key)) =
                    self.imports_for(&ctx.module_key).get(&segs[0])
                {
                    self.functions.get(&(mod_key.clone(), segs[1].clone()))
                } else {
                    None
                }
            }
            ExprKind::Path(segs) if segs.len() == 3 => self
                .resolve_qext_submodule_3seg(&ctx.module_key, &segs[0], &segs[1])
                .and_then(|qsub| self.functions.get(&(qsub, segs[2].clone()))),
            _ => None,
        };
        if let Some(entry) = user_entry_opt {
            match &entry.decl.kind {
                DecFnKind::SingleArm { params, .. } if args.len() == params.len() => {
                    let mut pos_idx = 0usize;
                    for a in args.iter_mut() {
                        let p_opt = match &a.name {
                            None => {
                                let res = params.get(pos_idx);
                                pos_idx += 1;
                                res
                            }
                            Some(n) => params.iter().find(|p| &p.name == n),
                        };
                        if let Some(p) = p_opt {
                            if let Ok(pty) =
                                self.resolve_type(&entry.module_key, &p.ty, entry.decl.line)
                            {
                                coerce_expr_to_expected(&mut a.value, &pty);
                            }
                        }
                    }
                }
                DecFnKind::MultiArm {
                    header_typed_params,
                    arms,
                    ..
                } if args.iter().all(|a| a.name.is_none()) => {
                    let matching_arms: Vec<&DecArm> = arms
                        .iter()
                        .filter(|arm| arm.params.len() == args.len())
                        .collect();
                    for idx in 0..args.len() {
                        let mut agreed: Option<Type> = None;
                        let mut conflict = false;
                        for arm in &matching_arms {
                            if let Ok(tys) = self.infer_arm_param_types(
                                &entry.module_key,
                                arm,
                                header_typed_params.as_deref(),
                            ) {
                                let t = tys[idx].clone();
                                if t != Type::Unknown {
                                    match &agreed {
                                        None => agreed = Some(t),
                                        Some(prev) if prev == &t => {}
                                        Some(_) => {
                                            conflict = true;
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        if !conflict {
                            if let Some(exp) = agreed {
                                coerce_expr_to_expected(&mut args[idx].value, &exp);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn check_call(
        &mut self,
        callee: &mut Expr,
        args: &mut [CallArg],
        line: usize,
        scopes: &[BTreeMap<String, VarBinding>],
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        self.precoerce_call_args(callee, args, line, ctx);

        // Pre-coerce builtin contextual array arguments & handle higher-order AFC scope operators
        let callee_canon_hint = self.peek_builtin_canon(callee, ctx);
        if let Some(
            op @ ("evolve" | "scan" | "fold_seq" | "map_seq" | "filter_seq" | "tabulate"
            | "zip_seq" | "count" | "any" | "all"),
        ) = callee_canon_hint.as_deref()
        {
            return self.check_higher_order_scope_op(op, args, line, scopes, ctx);
        }

        let mut evaluated_args: Vec<(Option<String>, Type)> = Vec::with_capacity(args.len());
        match callee_canon_hint.as_deref() {
            Some("array_set") if args.len() == 3 && args.iter().all(|a| a.name.is_none()) => {
                let t0 = self.check_expr(&mut args[0].value, scopes, ctx)?;
                let t1 = self.check_expr(&mut args[1].value, scopes, ctx)?;
                if let Type::Array(ref elem_ty, _) = *t0.unwrap_read_only() {
                    coerce_expr_to_expected(&mut args[2].value, elem_ty);
                }
                let t2 = self.check_expr(&mut args[2].value, scopes, ctx)?;
                evaluated_args.push((None, t0));
                evaluated_args.push((None, t1));
                evaluated_args.push((None, t2));
            }
            Some("list_set") if args.len() == 3 && args.iter().all(|a| a.name.is_none()) => {
                let t0 = self.check_expr(&mut args[0].value, scopes, ctx)?;
                let t1 = self.check_expr(&mut args[1].value, scopes, ctx)?;
                if let Type::List(ref elem_ty) = *t0.unwrap_read_only() {
                    if **elem_ty != Type::Unknown {
                        coerce_expr_to_expected(&mut args[2].value, elem_ty);
                    }
                }
                let t2 = self.check_expr(&mut args[2].value, scopes, ctx)?;
                evaluated_args.push((None, t0));
                evaluated_args.push((None, t1));
                evaluated_args.push((None, t2));
            }
            Some("list_push") if args.len() == 2 && args.iter().all(|a| a.name.is_none()) => {
                let t0 = self.check_expr(&mut args[0].value, scopes, ctx)?;
                if let Type::List(ref elem_ty) = *t0.unwrap_read_only() {
                    if **elem_ty != Type::Unknown {
                        coerce_expr_to_expected(&mut args[1].value, elem_ty);
                    }
                }
                let t1 = self.check_expr(&mut args[1].value, scopes, ctx)?;
                evaluated_args.push((None, t0));
                evaluated_args.push((None, t1));
            }
            Some("map_put") if args.len() == 3 && args.iter().all(|a| a.name.is_none()) => {
                let t0 = self.check_expr(&mut args[0].value, scopes, ctx)?;
                if let Type::Map(ref k_ty, ref v_ty) = *t0.unwrap_read_only() {
                    if **k_ty != Type::Unknown {
                        coerce_expr_to_expected(&mut args[1].value, k_ty);
                        coerce_expr_to_expected(&mut args[2].value, v_ty);
                    }
                }
                let t1 = self.check_expr(&mut args[1].value, scopes, ctx)?;
                let t2 = self.check_expr(&mut args[2].value, scopes, ctx)?;
                evaluated_args.push((None, t0));
                evaluated_args.push((None, t1));
                evaluated_args.push((None, t2));
            }
            Some("map_remove" | "contains")
                if args.len() == 2 && args.iter().all(|a| a.name.is_none()) =>
            {
                let t0 = self.check_expr(&mut args[0].value, scopes, ctx)?;
                if let Type::Map(ref k_ty, _) = *t0.unwrap_read_only() {
                    if **k_ty != Type::Unknown {
                        coerce_expr_to_expected(&mut args[1].value, k_ty);
                    }
                }
                let t1 = self.check_expr(&mut args[1].value, scopes, ctx)?;
                evaluated_args.push((None, t0));
                evaluated_args.push((None, t1));
            }
            Some("map_get_or") if args.len() == 3 && args.iter().all(|a| a.name.is_none()) => {
                let t0 = self.check_expr(&mut args[0].value, scopes, ctx)?;
                if let Type::Map(ref k_ty, ref v_ty) = *t0.unwrap_read_only() {
                    if **k_ty != Type::Unknown {
                        coerce_expr_to_expected(&mut args[1].value, k_ty);
                        coerce_expr_to_expected(&mut args[2].value, v_ty);
                    }
                }
                let t1 = self.check_expr(&mut args[1].value, scopes, ctx)?;
                let t2 = self.check_expr(&mut args[2].value, scopes, ctx)?;
                evaluated_args.push((None, t0));
                evaluated_args.push((None, t1));
                evaluated_args.push((None, t2));
            }
            Some("select") if args.len() == 3 && args.iter().all(|a| a.name.is_none()) => {
                let t0 = self.check_expr(&mut args[0].value, scopes, ctx)?;
                let a_is_list = matches!(args[1].value.kind, ExprKind::List(_));
                let b_is_list = matches!(args[2].value.kind, ExprKind::List(_));
                let (t1, t2) = if !a_is_list && b_is_list {
                    let t1 = self.check_expr(&mut args[1].value, scopes, ctx)?;
                    if !contains_unknown(&t1) {
                        coerce_expr_to_expected(&mut args[2].value, &t1);
                    }
                    let t2 = self.check_expr(&mut args[2].value, scopes, ctx)?;
                    (t1, t2)
                } else if a_is_list && !b_is_list {
                    let t2 = self.check_expr(&mut args[2].value, scopes, ctx)?;
                    if !contains_unknown(&t2) {
                        coerce_expr_to_expected(&mut args[1].value, &t2);
                    }
                    let t1 = self.check_expr(&mut args[1].value, scopes, ctx)?;
                    (t1, t2)
                } else {
                    let t1 = self.check_expr(&mut args[1].value, scopes, ctx)?;
                    let t2 = self.check_expr(&mut args[2].value, scopes, ctx)?;
                    (t1, t2)
                };
                evaluated_args.push((None, t0));
                evaluated_args.push((None, t1));
                evaluated_args.push((None, t2));
            }
            Some("dot") if args.len() == 2 && args.iter().all(|a| a.name.is_none()) => {
                let a_is_nonempty_list =
                    matches!(&args[0].value.kind, ExprKind::List(xs) if !xs.is_empty());
                let b_is_nonempty_list =
                    matches!(&args[1].value.kind, ExprKind::List(xs) if !xs.is_empty());
                let (t0, t1) = if !a_is_nonempty_list && b_is_nonempty_list {
                    let t0 = self.check_expr(&mut args[0].value, scopes, ctx)?;
                    if matches!(t0.unwrap_read_only(), Type::Array(_, _)) {
                        coerce_expr_to_expected(&mut args[1].value, &t0);
                    }
                    let t1 = self.check_expr(&mut args[1].value, scopes, ctx)?;
                    (t0, t1)
                } else if a_is_nonempty_list && !b_is_nonempty_list {
                    let t1 = self.check_expr(&mut args[1].value, scopes, ctx)?;
                    if matches!(t1.unwrap_read_only(), Type::Array(_, _)) {
                        coerce_expr_to_expected(&mut args[0].value, &t1);
                    }
                    let t0 = self.check_expr(&mut args[0].value, scopes, ctx)?;
                    (t0, t1)
                } else {
                    let t0 = self.check_expr(&mut args[0].value, scopes, ctx)?;
                    let t1 = self.check_expr(&mut args[1].value, scopes, ctx)?;
                    (t0, t1)
                };
                evaluated_args.push((None, t0));
                evaluated_args.push((None, t1));
            }
            _ => {
                for a in args.iter_mut() {
                    let ty = self.check_expr(&mut a.value, scopes, ctx)?;
                    evaluated_args.push((a.name.clone(), ty));
                }
            }
        }

        // Check enum payload constructor `Enum::Variant(args...)`, `mod::Enum::Variant(args...)`, or `aod::<ns>::Enum::Variant(args...)`
        if let ExprKind::Path(ref segs) = callee.kind {
            if matches!(segs.len(), 2..=4) {
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
                let has_fn_overload = self
                    .functions
                    .contains_key(&(sdef.module.clone(), sdef.name.clone()));
                let use_canonical_struct = if !has_fn_overload
                    || evaluated_args.iter().any(|(n, _)| n.is_some())
                {
                    true
                } else {
                    evaluated_args.len() == sdef.fields.len()
                        && evaluated_args.iter().zip(&sdef.fields).all(|((_, a), (_, f))| {
                            types_compatible_with_annotation(a, f)
                                || (a.is_read_only()
                                    && types_compatible_with_annotation(a.unwrap_read_only(), f))
                        })
                };
                if use_canonical_struct {
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
                } else {
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
                }
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
                } else if !lookup_builtin(name).is_empty() || is_polymorphic_v2_builtin(name) {
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
                if evaluated_args.iter().any(|(n, _)| n.is_some()) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("builtin {canon} takes positional arguments"),
                    ));
                }

                // Polymorphic v2 builtins (`len`, `contains`, `array`, `array_set`, `select`)
                // work identically unqualified, via `aod::core::*`, and via imported aliases (`P0-5`)!
                match canon.as_str() {
                    "len" => {
                        if self.compat() == CompatMode::V1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "len(...) is not in --compat v1",
                            ));
                        }
                        if evaluated_args.len() != 1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("len(...) expects 1 argument, got {}", evaluated_args.len()),
                            ));
                        }
                        match evaluated_args[0].1.unwrap_read_only() {
                            Type::Bytes
                            | Type::List(_)
                            | Type::Array(_, _)
                            | Type::Map(_, _) => return Ok(Type::Int),
                            other => {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    line,
                                    format!("len(...) unsupported on {other}"),
                                ));
                            }
                        }
                    }
                    "contains" => {
                        if self.compat() == CompatMode::V1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "contains(...) is not in --compat v1",
                            ));
                        }
                        if evaluated_args.len() != 2 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!(
                                    "contains(map, key) expects 2 arguments, got {}",
                                    evaluated_args.len()
                                ),
                            ));
                        }
                        if let Type::Map(k_ty, _) = evaluated_args[0].1.unwrap_read_only() {
                            if types_compatible_with_annotation(
                                evaluated_args[1].1.unwrap_read_only(),
                                k_ty,
                            ) {
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
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            line,
                            format!(
                                "contains(map, key) first argument must be map[K, V], got {}",
                                evaluated_args[0].1
                            ),
                        ));
                    }
                    "array" => {
                        if self.compat() == CompatMode::V1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "array(...) is not in --compat v1",
                            ));
                        }
                        if evaluated_args.is_empty() || evaluated_args.len() > 65_536 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!(
                                    "array(...) requires 1 <= N <= 65536 arguments, got {}",
                                    evaluated_args.len()
                                ),
                            ));
                        }
                        let first = evaluated_args[0].1.clone();
                        if first == Type::Unit || contains_unknown(&first) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "array(...) elements require a concrete non-unit type",
                            ));
                        }
                        if ctx.effect != Effect::QExt && first.is_qext() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::QExtLeakageForbidden,
                                line,
                                "array(...) cannot hold QExt elements outside quarantine Q_ext",
                            ));
                        }
                        for (_, ty) in &evaluated_args[1..] {
                            if ty != &first {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::ImplicitCoercionForbidden,
                                    line,
                                    format!(
                                        "array(...) requires homogeneous elements: expected {first}, got {ty}"
                                    ),
                                ));
                            }
                        }
                        return Ok(Type::Array(Box::new(first), evaluated_args.len()));
                    }
                    "array_set" => {
                        if self.compat() == CompatMode::V1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "array_set(...) is not in --compat v1",
                            ));
                        }
                        if evaluated_args.len() != 3 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!(
                                    "array_set(arr, idx, val) expects 3 arguments, got {}",
                                    evaluated_args.len()
                                ),
                            ));
                        }
                        let arr_ty = &evaluated_args[0].1;
                        if arr_ty.is_read_only() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ObserverMutationForbidden,
                                line,
                                format!("cannot call array_set on read-only array {arr_ty}"),
                            ));
                        }
                        let Type::Array(elem_ty, n) = arr_ty else {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!(
                                    "array_set first argument must be array[T, N], got {arr_ty}"
                                ),
                            ));
                        };
                        if evaluated_args[1].1.unwrap_read_only() != &Type::Int {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!(
                                    "array_set index must be int, got {}",
                                    evaluated_args[1].1
                                ),
                            ));
                        }
                        let val_ty = &evaluated_args[2].1;
                        if !types_compatible_with_annotation(val_ty, elem_ty) {
                            if val_ty.is_read_only()
                                && !elem_ty.is_read_only()
                                && types_compatible_with_annotation(
                                    val_ty.unwrap_read_only(),
                                    elem_ty,
                                )
                            {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::ObserverMutationForbidden,
                                    line,
                                    format!(
                                        "cannot strip ReadOnly from {val_ty} in array_set"
                                    ),
                                ));
                            }
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ImplicitCoercionForbidden,
                                line,
                                format!("array_set element expects {elem_ty}, got {val_ty}"),
                            ));
                        }
                        return Ok(Type::Array(elem_ty.clone(), *n));
                    }
                    "select" => {
                        if self.compat() == CompatMode::V1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "select(...) is not in --compat v1",
                            ));
                        }
                        if evaluated_args.len() != 3 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!(
                                    "select(cond, a, b) expects 3 arguments, got {}",
                                    evaluated_args.len()
                                ),
                            ));
                        }
                        let cond_ty = &evaluated_args[0].1;
                        let a_ty = &evaluated_args[1].1;
                        let b_ty = &evaluated_args[2].1;
                        let unified_ty = if a_ty == b_ty {
                            a_ty.clone()
                        } else if contains_unknown(a_ty)
                            && !contains_unknown(b_ty)
                            && types_compatible_with_annotation(a_ty, b_ty)
                        {
                            b_ty.clone()
                        } else if contains_unknown(b_ty)
                            && !contains_unknown(a_ty)
                            && types_compatible_with_annotation(b_ty, a_ty)
                        {
                            a_ty.clone()
                        } else {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ImplicitCoercionForbidden,
                                line,
                                format!(
                                    "select(cond, a, b) arms must have identical types (got {a_ty} vs {b_ty})"
                                ),
                            ));
                        };
                        if unified_ty == Type::Unit {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "select(cond, a, b) arms cannot have unit type ()",
                            ));
                        }
                        if contains_unknown(&unified_ty) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "select(cond, a, b) arms require a concrete type",
                            ));
                        }
                        if ctx.effect != Effect::QExt && unified_ty.is_qext() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::QExtLeakageForbidden,
                                line,
                                "select cannot produce QExt value outside quarantine Q_ext",
                            ));
                        }
                        if *cond_ty == Type::QExt(Box::new(Type::Bool)) {
                            if ctx.effect != Effect::QExt || !unified_ty.is_qext() {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::QExtLeakageForbidden,
                                    line,
                                    "QExt<bool> condition in select requires quarantine Q_ext and QExt<T> arms",
                                ));
                            }
                        } else if cond_ty.unwrap_read_only() != &Type::Bool {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("select condition must be bool, got {cond_ty}"),
                            ));
                        }
                        return Ok(unified_ty);
                    }
                    "map_put" => {
                        if self.compat() == CompatMode::V1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "map_put(...) is not in --compat v1",
                            ));
                        }
                        if evaluated_args.len() != 3 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("map_put(m, k, v) expects 3 arguments, got {}", evaluated_args.len()),
                            ));
                        }
                        let m_ty = &evaluated_args[0].1;
                        if m_ty.is_read_only() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ObserverMutationForbidden,
                                line,
                                format!("cannot call map_put on read-only map {m_ty}"),
                            ));
                        }
                        let Type::Map(k_ty, v_ty) = m_ty else {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("map_put first argument must be map[K, V], got {m_ty}"),
                            ));
                        };
                        if contains_unknown(m_ty) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "map_put on empty map requires an explicit map[K, V] type annotation",
                            ));
                        }
                        check_readonly_and_coercion(&evaluated_args[1].1, k_ty, "map_put key", line)?;
                        check_readonly_and_coercion(&evaluated_args[2].1, v_ty, "map_put value", line)?;
                        return Ok(Type::Map(k_ty.clone(), v_ty.clone()));
                    }
                    "map_remove" => {
                        if self.compat() == CompatMode::V1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "map_remove(...) is not in --compat v1",
                            ));
                        }
                        if evaluated_args.len() != 2 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("map_remove(m, k) expects 2 arguments, got {}", evaluated_args.len()),
                            ));
                        }
                        let m_ty = &evaluated_args[0].1;
                        if m_ty.is_read_only() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ObserverMutationForbidden,
                                line,
                                format!("cannot call map_remove on read-only map {m_ty}"),
                            ));
                        }
                        let Type::Map(k_ty, v_ty) = m_ty else {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("map_remove first argument must be map[K, V], got {m_ty}"),
                            ));
                        };
                        if contains_unknown(m_ty) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "map_remove on empty map requires an explicit map[K, V] type annotation",
                            ));
                        }
                        check_readonly_and_coercion(&evaluated_args[1].1, k_ty, "map_remove key", line)?;
                        return Ok(Type::Map(k_ty.clone(), v_ty.clone()));
                    }
                    "map_get_or" => {
                        if self.compat() == CompatMode::V1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "map_get_or(...) is not in --compat v1",
                            ));
                        }
                        if evaluated_args.len() != 3 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!(
                                    "map_get_or(m, k, default) expects 3 arguments, got {}",
                                    evaluated_args.len()
                                ),
                            ));
                        }
                        let m_ty = &evaluated_args[0].1;
                        let m_ro = m_ty.is_read_only();
                        let Type::Map(k_ty, v_ty) = m_ty.unwrap_read_only() else {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("map_get_or first argument must be map[K, V], got {m_ty}"),
                            ));
                        };
                        if contains_unknown(m_ty) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "map_get_or on empty map requires an explicit map[K, V] type annotation",
                            ));
                        }
                        if !types_compatible_with_annotation(
                            evaluated_args[1].1.unwrap_read_only(),
                            k_ty.unwrap_read_only(),
                        ) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ImplicitCoercionForbidden,
                                line,
                                format!(
                                    "map_get_or key expects {k_ty}, got {}",
                                    evaluated_args[1].1
                                ),
                            ));
                        }
                        let mut ret_ty = (**v_ty).clone();
                        if m_ro && ret_ty.needs_readonly_propagation() {
                            ret_ty = ret_ty.wrap_read_only();
                        }
                        check_readonly_and_coercion(
                            &evaluated_args[2].1,
                            &ret_ty,
                            "map_get_or default",
                            line,
                        )?;
                        return Ok(ret_ty);
                    }
                    "map_keys" => {
                        if self.compat() == CompatMode::V1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "map_keys(...) is not in --compat v1",
                            ));
                        }
                        if evaluated_args.len() != 1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("map_keys(m) expects 1 argument, got {}", evaluated_args.len()),
                            ));
                        }
                        let m_ty = &evaluated_args[0].1;
                        let m_ro = m_ty.is_read_only();
                        let Type::Map(k_ty, _) = m_ty.unwrap_read_only() else {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("map_keys expects map[K, V], got {m_ty}"),
                            ));
                        };
                        if contains_unknown(m_ty) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "map_keys on empty map requires an explicit map[K, V] type annotation",
                            ));
                        }
                        let out = Type::List(k_ty.clone());
                        return Ok(if m_ro { out.wrap_read_only() } else { out });
                    }
                    "map_entries" => {
                        if self.compat() == CompatMode::V1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "map_entries(...) is not in --compat v1",
                            ));
                        }
                        if evaluated_args.len() != 1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!(
                                    "map_entries(m) expects 1 argument, got {}",
                                    evaluated_args.len()
                                ),
                            ));
                        }
                        let m_ty = &evaluated_args[0].1;
                        let m_ro = m_ty.is_read_only();
                        let Type::Map(k_ty, v_ty) = m_ty.unwrap_read_only() else {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("map_entries expects map[K, V], got {m_ty}"),
                            ));
                        };
                        if contains_unknown(m_ty) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "map_entries on empty map requires an explicit map[K, V] type annotation",
                            ));
                        }
                        let out = Type::List(Box::new(Type::Tuple(vec![
                            (**k_ty).clone(),
                            (**v_ty).clone(),
                        ])));
                        return Ok(if m_ro { out.wrap_read_only() } else { out });
                    }
                    "list_set" => {
                        if self.compat() == CompatMode::V1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "list_set(...) is not in --compat v1",
                            ));
                        }
                        if evaluated_args.len() != 3 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!(
                                    "list_set(xs, idx, val) expects 3 arguments, got {}",
                                    evaluated_args.len()
                                ),
                            ));
                        }
                        let xs_ty = &evaluated_args[0].1;
                        if xs_ty.is_read_only() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ObserverMutationForbidden,
                                line,
                                format!("cannot call list_set on read-only list {xs_ty}"),
                            ));
                        }
                        let Type::List(elem_ty) = xs_ty else {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("list_set first argument must be [T], got {xs_ty}"),
                            ));
                        };
                        if contains_unknown(xs_ty) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "list_set on empty list requires an explicit [T] type annotation",
                            ));
                        }
                        if evaluated_args[1].1.unwrap_read_only() != &Type::Int {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("list_set index must be int, got {}", evaluated_args[1].1),
                            ));
                        }
                        check_readonly_and_coercion(
                            &evaluated_args[2].1,
                            elem_ty,
                            "list_set element",
                            line,
                        )?;
                        return Ok(Type::List(elem_ty.clone()));
                    }
                    "list_push" => {
                        if self.compat() == CompatMode::V1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "list_push(...) is not in --compat v1",
                            ));
                        }
                        if evaluated_args.len() != 2 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!(
                                    "list_push(xs, val) expects 2 arguments, got {}",
                                    evaluated_args.len()
                                ),
                            ));
                        }
                        let xs_ty = &evaluated_args[0].1;
                        if xs_ty.is_read_only() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ObserverMutationForbidden,
                                line,
                                format!("cannot call list_push on read-only list {xs_ty}"),
                            ));
                        }
                        let Type::List(elem_ty) = xs_ty else {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("list_push first argument must be [T], got {xs_ty}"),
                            ));
                        };
                        if contains_unknown(xs_ty) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "list_push on empty list requires an explicit [T] type annotation",
                            ));
                        }
                        check_readonly_and_coercion(
                            &evaluated_args[1].1,
                            elem_ty,
                            "list_push element",
                            line,
                        )?;
                        return Ok(Type::List(elem_ty.clone()));
                    }
                    "concat_seq" => {
                        if self.compat() == CompatMode::V1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "concat_seq(...) is not in --compat v1",
                            ));
                        }
                        if evaluated_args.len() != 2 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!(
                                    "concat_seq(a, b) expects 2 arguments, got {}",
                                    evaluated_args.len()
                                ),
                            ));
                        }
                        let a_ty = &evaluated_args[0].1;
                        let b_ty = &evaluated_args[1].1;
                        if contains_unknown(a_ty) || contains_unknown(b_ty) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "concat_seq on empty sequence requires an explicit [T] type annotation",
                            ));
                        }
                        let elem_a = match a_ty.unwrap_read_only() {
                            Type::List(e) | Type::Array(e, _) => (**e).clone(),
                            other => {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    line,
                                    format!("concat_seq expects [T] or array[T, N], got {other}"),
                                ));
                            }
                        };
                        let elem_b = match b_ty.unwrap_read_only() {
                            Type::List(e) | Type::Array(e, _) => (**e).clone(),
                            other => {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    line,
                                    format!("concat_seq expects [T] or array[T, N], got {other}"),
                                ));
                            }
                        };
                        if elem_a.unwrap_read_only() != elem_b.unwrap_read_only() {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ImplicitCoercionForbidden,
                                line,
                                format!(
                                    "concat_seq requires matching element types, got {elem_a} vs {elem_b}"
                                ),
                            ));
                        }
                        if elem_a.is_read_only() != elem_b.is_read_only()
                            && !(a_ty.is_read_only() || b_ty.is_read_only())
                        {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::ObserverMutationForbidden,
                                line,
                                "cannot strip ReadOnly from sequence elements in concat_seq",
                            ));
                        }
                        let out = Type::List(Box::new(elem_a));
                        return Ok(if a_ty.is_read_only() || b_ty.is_read_only() {
                            out.wrap_read_only()
                        } else {
                            out
                        });
                    }
                    "slice_seq" => {
                        if self.compat() == CompatMode::V1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "slice_seq(...) is not in --compat v1",
                            ));
                        }
                        if evaluated_args.len() != 3 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!(
                                    "slice_seq(xs, start, end) expects 3 arguments, got {}",
                                    evaluated_args.len()
                                ),
                            ));
                        }
                        let xs_ty = &evaluated_args[0].1;
                        if contains_unknown(xs_ty) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "slice_seq on empty list requires an explicit [T] type annotation",
                            ));
                        }
                        let xs_ro = xs_ty.is_read_only();
                        let elem = match xs_ty.unwrap_read_only() {
                            Type::List(e) | Type::Array(e, _) => (**e).clone(),
                            other => {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    line,
                                    format!("slice_seq first argument must be [T] or array[T, N], got {other}"),
                                ));
                            }
                        };
                        if evaluated_args[1].1.unwrap_read_only() != &Type::Int
                            || evaluated_args[2].1.unwrap_read_only() != &Type::Int
                        {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "slice_seq start and end bounds must be int",
                            ));
                        }
                        let out = Type::List(Box::new(elem));
                        return Ok(if xs_ro { out.wrap_read_only() } else { out });
                    }
                    "sum" => {
                        if self.compat() == CompatMode::V1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "sum(...) is not in --compat v1",
                            ));
                        }
                        if evaluated_args.len() != 1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("sum(seq) expects 1 argument, got {}", evaluated_args.len()),
                            ));
                        }
                        let elem_ty = match evaluated_args[0].1.unwrap_read_only() {
                            Type::Bytes => Type::U8,
                            Type::List(elem) | Type::Array(elem, _) => (**elem).unwrap_read_only().clone(),
                            other => {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    line,
                                    format!("sum(seq) expects [T], array[T, N], or bytes, got {other}"),
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
                        } else if !matches!(
                            elem_ty,
                            Type::Int | Type::U8 | Type::T1 | Type::T9 | Type::T32 | Type::Ratio
                        ) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!(
                                    "sum(...) requires sequence of int, u8, t1, t9, t32, or ratio, got {elem_ty}"
                                ),
                            ));
                        }
                        self.poly_call_types.insert(
                            (ctx.module_key.clone(), line, format!("sum:{args:?}")),
                            elem_ty.clone(),
                        );
                        return Ok(elem_ty);
                    }
                    "dot" => {
                        if self.compat() == CompatMode::V1 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                "dot(...) is not in --compat v1",
                            ));
                        }
                        if evaluated_args.len() != 2 {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!(
                                    "dot(a, b) expects 2 arguments, got {}",
                                    evaluated_args.len()
                                ),
                            ));
                        }
                        let a_unw = evaluated_args[0].1.unwrap_read_only();
                        let b_unw = evaluated_args[1].1.unwrap_read_only();
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
                                Type::Bytes => Ok(Type::U8),
                                Type::List(e) | Type::Array(e, _) => Ok((**e).unwrap_read_only().clone()),
                                other => Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    line,
                                    format!("dot(a, b) expects [T], array[T, N], or bytes, got {other}"),
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
                        } else if !matches!(
                            ea,
                            Type::Int | Type::U8 | Type::T1 | Type::T9 | Type::T32 | Type::Ratio
                        ) {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!(
                                    "dot(a, b) requires elements of int, u8, t1, t9, t32, or ratio, got {ea}"
                                ),
                            ));
                        }
                        self.poly_call_types.insert(
                            (ctx.module_key.clone(), line, format!("dot:{args:?}")),
                            ea.clone(),
                        );
                        return Ok(ea);
                    }
                    _ => {}
                }

                self.check_nonpoly_builtin(&canon, &evaluated_args, line, ctx)
            }
            CallTarget::UserFn(entry) => {
                self.check_user_fn_evaluated_call(&entry, &evaluated_args, line, ctx)
            }
        }
    }

    fn check_nonpoly_builtin(
        &self,
        canon: &str,
        evaluated_args: &[(Option<String>, Type)],
        line: usize,
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        let sigs = lookup_builtin(canon);
        if sigs.is_empty() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                line,
                format!("undefined builtin {canon}"),
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
                format!(
                    "builtin {canon} is quarantined in aod::qext and forbidden outside quarantine Q_ext"
                ),
            ));
        }

        let matching: Vec<_> = sigs
            .into_iter()
            .filter(|s| {
                (self.compat() == CompatMode::V2 || s.v1_allowed)
                    && (self.compat() == CompatMode::V1
                        || (!s.params.iter().any(Type::contains_str) && !s.ret.contains_str()))
                    && s.params.len() == arg_tys.len()
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
        if (self.compat() == CompatMode::V2 || self.unit.discipline == SyntaxDiscipline::PureWaveV2)
            && (arg_tys.iter().any(Type::contains_str) || sig.ret.contains_str())
        {
            return Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                line,
                format!(
                    "legacy `str` builtin signature on {canon} is forbidden in Dec v2; use `bytes` overloads"
                ),
            ));
        }
        Ok(sig.ret.clone())
    }

    fn resolve_ho_callee(
        &self,
        fn_expr: &Expr,
        line: usize,
        scopes: &[BTreeMap<String, VarBinding>],
        ctx: &CheckContext,
    ) -> DecResult<HoCallee> {
        let base_mod = self.base_module_of(&ctx.module_key).to_string();
        match &fn_expr.kind {
            ExprKind::Ident(name) => {
                // P2-2: Reject local variables and module `const`s!
                if find_var_scope_index(scopes, name).is_some()
                    || self.consts.contains_key(&(ctx.module_key.clone(), name.clone()))
                    || self.consts.contains_key(&(base_mod.clone(), name.clone()))
                {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("higher-order callback {name} cannot be a local binding or const"),
                    ));
                }
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
                                "cannot pass quarantined builtin {name} outside quarantine Q_ext"
                            ),
                        ));
                    }
                    if let Some(ImportedSymbol::Builtin(canon)) =
                        self.imports_for(&ctx.module_key).get(name).cloned()
                    {
                        return Ok(HoCallee::Builtin(canon));
                    }
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        line,
                        format!("{name} must be explicitly imported from aod::qext"),
                    ));
                }
                if let Some(entry) = self
                    .functions
                    .get(&(ctx.module_key.clone(), name.clone()))
                    .cloned()
                {
                    return Ok(HoCallee::UserFn(entry));
                }
                if base_mod != ctx.module_key {
                    if let Some(entry) =
                        self.functions.get(&(base_mod.clone(), name.clone())).cloned()
                    {
                        return Ok(HoCallee::UserFn(entry));
                    }
                }
                if let Some(sym) = self.imports_for(&ctx.module_key).get(name).cloned() {
                    match sym {
                        ImportedSymbol::ModuleItem { module, item } => {
                            if let Some(entry) =
                                self.functions.get(&(module.clone(), item.clone())).cloned()
                            {
                                return Ok(HoCallee::UserFn(entry));
                            }
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                line,
                                format!("imported symbol {module}::{item} is not a `dec` function"),
                            ));
                        }
                        ImportedSymbol::Builtin(canon) => {
                            return Ok(HoCallee::Builtin(canon));
                        }
                        ImportedSymbol::ModuleNamespace(_) => {}
                    }
                }
                for (m, qm) in &self.qext_modules {
                    if m == &base_mod {
                        let sub = format!("{m}::{qm}");
                        if let Some(e) = self.functions.get(&(sub, name.clone())) {
                            if ctx.effect != Effect::QExt {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::QExtLeakageForbidden,
                                    line,
                                    format!(
                                        "cannot pass quarantine Q_ext function {name} outside quarantine Q_ext"
                                    ),
                                ));
                            }
                            return Ok(HoCallee::UserFn(e.clone()));
                        }
                    }
                }
                // Bare struct without `dec Struct:` must fail (`P2-2`)
                if self
                    .structs
                    .contains_key(&(ctx.module_key.clone(), name.clone()))
                    || self.structs.contains_key(&(base_mod, name.clone()))
                {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!(
                            "bare struct constructor {name} cannot be passed as higher-order callback without `dec {name}:`"
                        ),
                    ));
                }
                if !lookup_builtin(name).is_empty() || is_polymorphic_v2_builtin(name) {
                    return Ok(HoCallee::Builtin(name.clone()));
                }
                Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    line,
                    format!(
                        "expected declared `dec` function or `aod::*` builtin as higher-order callback, got {name}"
                    ),
                ))
            }
            ExprKind::Path(segs) if segs.len() == 3 && segs[0] == "aod" => {
                let ns = &segs[0..2];
                if builtin_namespace_items(ns).is_some_and(|list| list.contains(&segs[2].as_str()))
                {
                    let canon = canonical_builtin_name(ns, &segs[2]);
                    if canon.starts_with("aod::qext::") && ctx.effect != Effect::QExt {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            line,
                            format!(
                                "cannot pass quarantined builtin {} outside quarantine Q_ext",
                                segs.join("::")
                            ),
                        ));
                    }
                    Ok(HoCallee::Builtin(canon))
                } else {
                    Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("unknown builtin {}", segs.join("::")),
                    ))
                }
            }
            ExprKind::Path(segs) if segs.len() == 2 => {
                let ns = &segs[0];
                let sym = &segs[1];
                if let Some(qsub) = self.resolve_qext_submodule(&ctx.module_key, ns) {
                    if let Some(entry) = self.functions.get(&(qsub, sym.clone())).cloned() {
                        if entry.is_qext && ctx.effect != Effect::QExt {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::QExtLeakageForbidden,
                                line,
                                format!(
                                    "cannot pass quarantine Q_ext function {ns}::{sym} outside quarantine Q_ext"
                                ),
                            ));
                        }
                        return Ok(HoCallee::UserFn(entry));
                    }
                }
                if let Some(ImportedSymbol::ModuleNamespace(mod_key)) =
                    self.imports_for(&ctx.module_key).get(ns).cloned()
                {
                    if let Some(rest) = mod_key.strip_prefix("aod::") {
                        let ns_vec = vec!["aod".to_string(), rest.to_string()];
                        if builtin_namespace_items(&ns_vec)
                            .is_some_and(|list| list.contains(&sym.as_str()))
                        {
                            let canon = canonical_builtin_name(&ns_vec, sym);
                            if canon.starts_with("aod::qext::") && ctx.effect != Effect::QExt {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::QExtLeakageForbidden,
                                    line,
                                    format!("cannot pass {mod_key}::{sym} outside quarantine Q_ext"),
                                ));
                            }
                            return Ok(HoCallee::Builtin(canon));
                        }
                    } else if let Some(entry) =
                        self.functions.get(&(mod_key.clone(), sym.clone())).cloned()
                    {
                        return Ok(HoCallee::UserFn(entry));
                    }
                }
                Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    line,
                    format!(
                        "expected declared `dec` function or `aod::*` builtin path as higher-order callback, got {}",
                        segs.join("::")
                    ),
                ))
            }
            ExprKind::Path(segs) if segs.len() == 3 => {
                if let Some(qsub) =
                    self.resolve_qext_submodule_3seg(&ctx.module_key, &segs[0], &segs[1])
                {
                    if let Some(entry) = self.functions.get(&(qsub, segs[2].clone())).cloned() {
                        if entry.is_qext && ctx.effect != Effect::QExt {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::QExtLeakageForbidden,
                                line,
                                format!(
                                    "cannot pass quarantine Q_ext function {} outside quarantine Q_ext",
                                    segs.join("::")
                                ),
                            ));
                        }
                        return Ok(HoCallee::UserFn(entry));
                    }
                }
                Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    line,
                    format!(
                        "expected declared `dec` function path as higher-order callback, got {}",
                        segs.join("::")
                    ),
                ))
            }
            _ => Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                line,
                "higher-order callback must be a direct `dec` function name or module path",
            )),
        }
    }

    fn check_ho_call(
        &mut self,
        callee: &HoCallee,
        arg_tys: &[Type],
        line: usize,
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        let eval_args: Vec<(Option<String>, Type)> =
            arg_tys.iter().map(|t| (None, t.clone())).collect();
        match callee {
            HoCallee::UserFn(entry) => {
                self.check_user_fn_evaluated_call(entry, &eval_args, line, ctx)
            }
            HoCallee::Builtin(canon) => self.check_nonpoly_builtin(canon, &eval_args, line, ctx),
        }
    }

    fn unique_ho_param_hint(&self, callee: &HoCallee, arity: usize, slot: usize) -> Option<Type> {
        match callee {
            HoCallee::UserFn(entry) => self.unique_param_hint(entry, arity, slot),
            HoCallee::Builtin(canon) => {
                let sigs: Vec<_> = lookup_builtin(canon)
                    .into_iter()
                    .filter(|s| s.params.len() == arity)
                    .collect();
                if sigs.len() == 1 {
                    sigs[0].params.get(slot).cloned()
                } else {
                    None
                }
            }
        }
    }

    fn unique_param_hint(&self, entry: &FnEntry, arity: usize, slot: usize) -> Option<Type> {
        match &entry.decl.kind {
            DecFnKind::SingleArm { params, .. } if params.len() == arity => self
                .resolve_type(&entry.module_key, &params[slot].ty, entry.decl.line)
                .ok(),
            DecFnKind::MultiArm {
                header_typed_params,
                arms,
                ..
            } => {
                let mut agreed: Option<Type> = None;
                let htp_opt = header_typed_params
                    .as_deref()
                    .filter(|ht| ht.len() == arity);
                for arm in arms.iter().filter(|a| a.params.len() == arity) {
                    let tys = self
                        .infer_arm_param_types(&entry.module_key, arm, htp_opt)
                        .ok()?;
                    let t = tys[slot].clone();
                    if t != Type::Unknown {
                        match &agreed {
                            None => agreed = Some(t),
                            Some(prev) if prev == &t => {}
                            Some(_) => return None,
                        }
                    }
                }
                agreed
            }
            _ => None,
        }
    }

    fn check_opt_env_arg(
        &mut self,
        op: &str,
        env_arg: Option<&mut CallArg>,
        line: usize,
        scopes: &[BTreeMap<String, VarBinding>],
        ctx: &CheckContext,
    ) -> DecResult<Option<Type>> {
        let Some(arg) = env_arg else {
            return Ok(None);
        };
        let env_ty = self.check_expr(&mut arg.value, scopes, ctx)?;
        if env_ty == Type::Unit || contains_unknown(&env_ty) {
            return Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                line,
                format!("{op} trailing env argument requires a concrete non-unit type"),
            ));
        }
        if ctx.effect != Effect::QExt && env_ty.is_qext() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::QExtLeakageForbidden,
                line,
                format!("{op} cannot carry QExt env outside quarantine Q_ext"),
            ));
        }
        Ok(Some(env_ty))
    }

    fn extract_seq_elem(&self, op: &str, seq_ty: &Type, line: usize) -> DecResult<(Type, Type)> {
        if *seq_ty == Type::Unit || contains_unknown(seq_ty) {
            return Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                line,
                format!("{op} on empty sequence requires an explicit [T] type annotation"),
            ));
        }
        let seq_ro = seq_ty.is_read_only();
        let raw_elem = match seq_ty.unwrap_read_only() {
            Type::Bytes => Type::U8,
            Type::List(inner) | Type::Array(inner, _) => (**inner).clone(),
            other => {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    line,
                    format!("{op} expects [T], array[T, N], or bytes, got {other}"),
                ));
            }
        };
        let passed_elem = if seq_ro && raw_elem.needs_readonly_propagation() {
            raw_elem.clone().wrap_read_only()
        } else {
            raw_elem.clone()
        };
        Ok((raw_elem, passed_elem))
    }

    fn check_higher_order_scope_op(
        &mut self,
        op: &str,
        args: &mut [CallArg],
        line: usize,
        scopes: &[BTreeMap<String, VarBinding>],
        ctx: &CheckContext,
    ) -> DecResult<Type> {
        if self.compat() == CompatMode::V1 {
            return Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                line,
                format!("{op}(...) is not in --compat v1"),
            ));
        }
        if args.iter().any(|a| a.name.is_some()) {
            return Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                line,
                format!("builtin {op} takes positional arguments"),
            ));
        }
        match op {
            "evolve" => {
                if args.len() != 3 && args.len() != 4 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!(
                            "evolve(init, dom, step_fn[, env]) expects 3 or 4 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                let cb_arity = if args.len() == 4 { 3 } else { 2 };
                let step_cb = self.resolve_ho_callee(&args[2].value, line, scopes, ctx)?;
                if let Some(hint_s) = self.unique_ho_param_hint(&step_cb, cb_arity, 0) {
                    if matches!(hint_s.unwrap_read_only(), Type::Array(_, _)) {
                        coerce_expr_to_expected(&mut args[0].value, &hint_s);
                    }
                }
                let s_ty = self.check_expr(&mut args[0].value, scopes, ctx)?;
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
                let dom_ty = self.check_expr(&mut args[1].value, scopes, ctx)?;
                if !matches!(dom_ty.unwrap_read_only(), Type::Range | Type::Window) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("evolve domain must be range or window, got {dom_ty}"),
                    ));
                }
                let env_ty = self.check_opt_env_arg("evolve", args.get_mut(3), line, scopes, ctx)?;
                let mut cb_args = vec![s_ty.clone(), Type::Int];
                if let Some(et) = env_ty {
                    cb_args.push(et);
                }
                let step_ret = self.check_ho_call(&step_cb, &cb_args, line, ctx)?;
                if step_ret != s_ty {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!(
                            "evolve step function must return state type {s_ty}, got {step_ret}"
                        ),
                    ));
                }
                Ok(s_ty)
            }
            "scan" | "fold_seq" => {
                if args.len() != 3 && args.len() != 4 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!(
                            "{op}(init, src, step_fn[, env]) expects 3 or 4 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                let cb_arity = if args.len() == 4 { 3 } else { 2 };
                let step_cb = self.resolve_ho_callee(&args[2].value, line, scopes, ctx)?;
                if let Some(hint_s) = self.unique_ho_param_hint(&step_cb, cb_arity, 0) {
                    if matches!(hint_s.unwrap_read_only(), Type::Array(_, _)) {
                        coerce_expr_to_expected(&mut args[0].value, &hint_s);
                    }
                }
                let s_ty = self.check_expr(&mut args[0].value, scopes, ctx)?;
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
                let src_ty = self.check_expr(&mut args[1].value, scopes, ctx)?;
                let passed_elem = if op == "scan"
                    && matches!(src_ty.unwrap_read_only(), Type::Range | Type::Window)
                {
                    Type::Int
                } else {
                    self.extract_seq_elem(op, &src_ty, line)?.1
                };
                let env_ty = self.check_opt_env_arg(op, args.get_mut(3), line, scopes, ctx)?;
                let mut cb_args = vec![s_ty.clone(), passed_elem];
                if let Some(et) = env_ty {
                    cb_args.push(et);
                }
                let step_ret = self.check_ho_call(&step_cb, &cb_args, line, ctx)?;
                if step_ret != s_ty {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!(
                            "{op} step function must return state type {s_ty}, got {step_ret}"
                        ),
                    ));
                }
                if op == "scan" {
                    Ok(Type::List(Box::new(s_ty)))
                } else {
                    Ok(s_ty)
                }
            }
            "map_seq" => {
                if args.len() != 2 && args.len() != 3 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!(
                            "map_seq(seq, map_fn[, env]) expects 2 or 3 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                let map_cb = self.resolve_ho_callee(&args[1].value, line, scopes, ctx)?;
                let seq_ty = self.check_expr(&mut args[0].value, scopes, ctx)?;
                let (_, passed_elem) = self.extract_seq_elem("map_seq", &seq_ty, line)?;
                let env_ty = self.check_opt_env_arg("map_seq", args.get_mut(2), line, scopes, ctx)?;
                let mut cb_args = vec![passed_elem];
                if let Some(et) = env_ty {
                    cb_args.push(et);
                }
                let u_ty = self.check_ho_call(&map_cb, &cb_args, line, ctx)?;
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
                Ok(Type::List(Box::new(u_ty)))
            }
            "filter_seq" => {
                if args.len() != 2 && args.len() != 3 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!(
                            "filter_seq(seq, pred_fn[, env]) expects 2 or 3 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                let pred_cb = self.resolve_ho_callee(&args[1].value, line, scopes, ctx)?;
                let seq_ty = self.check_expr(&mut args[0].value, scopes, ctx)?;
                let seq_ro = seq_ty.is_read_only();
                let (raw_elem, passed_elem) = self.extract_seq_elem("filter_seq", &seq_ty, line)?;
                let env_ty =
                    self.check_opt_env_arg("filter_seq", args.get_mut(2), line, scopes, ctx)?;
                let mut cb_args = vec![passed_elem];
                if let Some(et) = env_ty {
                    cb_args.push(et);
                }
                let pred_ret = self.check_ho_call(&pred_cb, &cb_args, line, ctx)?;
                if pred_ret.is_qext() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        line,
                        "filter_seq predicate cannot branch on QExt<bool>",
                    ));
                }
                if pred_ret.unwrap_read_only() != &Type::Bool {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("filter_seq predicate must return bool, got {pred_ret}"),
                    ));
                }
                let out = Type::List(Box::new(raw_elem));
                Ok(if seq_ro { out.wrap_read_only() } else { out })
            }
            "tabulate" => {
                if args.len() != 2 && args.len() != 3 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!(
                            "tabulate(dom, gen_fn[, env]) expects 2 or 3 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                let gen_cb = self.resolve_ho_callee(&args[1].value, line, scopes, ctx)?;
                let dom_ty = self.check_expr(&mut args[0].value, scopes, ctx)?;
                if !matches!(dom_ty.unwrap_read_only(), Type::Range | Type::Window) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("tabulate domain must be range or window, got {dom_ty}"),
                    ));
                }
                let env_ty =
                    self.check_opt_env_arg("tabulate", args.get_mut(2), line, scopes, ctx)?;
                let mut cb_args = vec![Type::Int];
                if let Some(et) = env_ty {
                    cb_args.push(et);
                }
                let u_ty = self.check_ho_call(&gen_cb, &cb_args, line, ctx)?;
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
                Ok(Type::List(Box::new(u_ty)))
            }
            "zip_seq" => {
                if args.len() != 2 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("zip_seq(a, b) expects 2 arguments, got {}", args.len()),
                    ));
                }
                let a_ty = self.check_expr(&mut args[0].value, scopes, ctx)?;
                let b_ty = self.check_expr(&mut args[1].value, scopes, ctx)?;
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
                let (_, elem_a) = self.extract_seq_elem("zip_seq", &a_ty, line)?;
                let (_, elem_b) = self.extract_seq_elem("zip_seq", &b_ty, line)?;
                if ctx.effect != Effect::QExt && (elem_a.is_qext() || elem_b.is_qext()) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        line,
                        "zip_seq cannot hold QExt elements outside quarantine Q_ext",
                    ));
                }
                Ok(Type::List(Box::new(Type::Tuple(vec![elem_a, elem_b]))))
            }
            "count" => {
                if args.len() != 2 && args.len() != 3 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!(
                            "count(seq, pred_fn[, env]) expects 2 or 3 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                let pred_cb = self.resolve_ho_callee(&args[1].value, line, scopes, ctx)?;
                let seq_ty = self.check_expr(&mut args[0].value, scopes, ctx)?;
                if seq_ty.is_qext() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        line,
                        "count cannot convert QExt sequence into native int",
                    ));
                }
                let (_, passed_elem) = self.extract_seq_elem("count", &seq_ty, line)?;
                let env_ty = self.check_opt_env_arg("count", args.get_mut(2), line, scopes, ctx)?;
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
                let pred_ret = self.check_ho_call(&pred_cb, &cb_args, line, ctx)?;
                if pred_ret.is_qext() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::QExtLeakageForbidden,
                        line,
                        "count predicate cannot return QExt<bool> to produce native int",
                    ));
                }
                if pred_ret.unwrap_read_only() != &Type::Bool {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!("count predicate must return bool, got {pred_ret}"),
                    ));
                }
                Ok(Type::Int)
            }
            "any" | "all" => {
                if args.len() != 2 && args.len() != 3 {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        line,
                        format!(
                            "{op}(seq, pred_fn[, env]) expects 2 or 3 arguments, got {}",
                            args.len()
                        ),
                    ));
                }
                let pred_cb = self.resolve_ho_callee(&args[1].value, line, scopes, ctx)?;
                let seq_ty = self.check_expr(&mut args[0].value, scopes, ctx)?;
                let (_, passed_elem) = self.extract_seq_elem(op, &seq_ty, line)?;
                let env_ty = self.check_opt_env_arg(op, args.get_mut(2), line, scopes, ctx)?;
                let mut cb_args = vec![passed_elem];
                if let Some(ref et) = env_ty {
                    cb_args.push(et.clone());
                }
                let pred_ret = self.check_ho_call(&pred_cb, &cb_args, line, ctx)?;
                let out_ty = if pred_ret == Type::QExt(Box::new(Type::Bool)) {
                    if ctx.effect != Effect::QExt {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::QExtLeakageForbidden,
                            line,
                            format!("{op} with QExt<bool> predicate requires quarantine Q_ext context"),
                        ));
                    }
                    Type::QExt(Box::new(Type::Bool))
                } else if pred_ret.unwrap_read_only() == &Type::Bool {
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
                    (ctx.module_key.clone(), line, format!("{op}:{args:?}")),
                    out_ty.clone(),
                );
                Ok(out_ty)
            }
            _ => unreachable!(),
        }
    }

    fn check_user_fn_evaluated_call(
        &mut self,
        entry: &FnEntry,
        evaluated_args: &[(Option<String>, Type)],
        line: usize,
        ctx: &CheckContext,
    ) -> DecResult<Type> {
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
                    let htp_opt = header_typed_params
                        .as_deref()
                        .filter(|ht| ht.len() == arm.params.len());
                    let arm_tys = self.infer_arm_param_types(&entry.module_key, arm, htp_opt)?;
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
                        let rty =
                            self.resolve_type(&entry.module_key, &arm.return_type, arm.line)?;
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
                let ordered_arg_tys = resolve_single_arm_args(params, evaluated_args, line)?;
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
                            && types_compatible_with_annotation(actual.unwrap_read_only(), &expected)
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

    fn peek_builtin_canon(&self, callee: &Expr, ctx: &CheckContext) -> Option<String> {
        let base_mod = self.base_module_of(&ctx.module_key).to_string();
        match &callee.kind {
            ExprKind::Ident(name) => {
                if self
                    .functions
                    .contains_key(&(ctx.module_key.clone(), name.clone()))
                    || self.functions.contains_key(&(base_mod, name.clone()))
                {
                    return None;
                }
                if let Some(sym) = self.imports_for(&ctx.module_key).get(name) {
                    return match sym {
                        ImportedSymbol::Builtin(c) => Some(c.clone()),
                        _ => None,
                    };
                }
                if !lookup_builtin(name).is_empty() || is_polymorphic_v2_builtin(name) {
                    return Some(name.clone());
                }
                None
            }
            ExprKind::Path(segs) if segs.len() == 3 && segs[0] == "aod" => {
                let ns = &segs[0..2];
                if builtin_namespace_items(ns)
                    .is_some_and(|list| list.contains(&segs[2].as_str()))
                {
                    Some(canonical_builtin_name(ns, &segs[2]))
                } else {
                    None
                }
            }
            ExprKind::Path(segs) if segs.len() == 2 => {
                if let Some(ImportedSymbol::ModuleNamespace(mod_key)) =
                    self.imports_for(&ctx.module_key).get(&segs[0])
                {
                    if let Some(rest) = mod_key.strip_prefix("aod::") {
                        let ns_vec = vec!["aod".to_string(), rest.to_string()];
                        if builtin_namespace_items(&ns_vec)
                            .is_some_and(|list| list.contains(&segs[1].as_str()))
                        {
                            return Some(canonical_builtin_name(&ns_vec, &segs[1]));
                        }
                    }
                }
                None
            }
            _ => None,
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
        let mut cloned_entry = entry.clone();
        self.check_fn_entry(&mut cloned_entry, Some(arg_tys), effect)
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
                Pattern::Wildcard | Pattern::Binding(_) | Pattern::NullLit => return true,
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

#[derive(Clone)]
enum HoCallee {
    UserFn(FnEntry),
    Builtin(String),
}

fn check_readonly_and_coercion(
    actual: &Type,
    expected: &Type,
    ctx_name: &str,
    line: usize,
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
            format!("cannot strip ReadOnly from {actual} in {ctx_name}"),
        ));
    }
    Err(Diagnostic::at_line(
        DiagnosticCode::ImplicitCoercionForbidden,
        line,
        format!("{ctx_name} expects {expected}, got {actual}"),
    ))
}

fn is_irrefutable_arm_param(p: &DecArmParam) -> bool {
    match p {
        DecArmParam::TypedVar { .. } => true,
        DecArmParam::TypedPattern { pattern, .. } | DecArmParam::Pattern(pattern) => {
            is_irrefutable_pattern(pattern)
        }
    }
}

fn is_polymorphic_v2_builtin(canon: &str) -> bool {
    matches!(
        canon,
        "len"
            | "contains"
            | "array"
            | "array_set"
            | "select"
            | "evolve"
            | "scan"
            | "fold_seq"
            | "map_seq"
            | "filter_seq"
            | "tabulate"
            | "zip_seq"
            | "count"
            | "any"
            | "all"
            | "map_put"
            | "map_remove"
            | "map_get_or"
            | "map_keys"
            | "map_entries"
            | "list_set"
            | "list_push"
            | "concat_seq"
            | "slice_seq"
            | "sum"
            | "dot"
    )
}

fn is_select_callee_syntax(callee: &Expr) -> bool {
    match &callee.kind {
        ExprKind::Ident(s) => s == "select",
        ExprKind::Path(segs) => segs.last().is_some_and(|s| s == "select"),
        _ => false,
    }
}

fn is_evolve_or_fold_callee_syntax(callee: &Expr) -> bool {
    match &callee.kind {
        ExprKind::Ident(s) => s == "evolve" || s == "fold_seq",
        ExprKind::Path(segs) => segs.last().is_some_and(|s| s == "evolve" || s == "fold_seq"),
        _ => false,
    }
}

/// Contextual `ExprKind::List` -> `ExprKind::ArrayLit` rewrite (`P0-4`) when target type is `array[T, N]`.
fn coerce_expr_to_expected(expr: &mut Expr, expected: &Type) {
    if let ExprKind::Call { callee, args } = &mut expr.kind {
        if is_evolve_or_fold_callee_syntax(callee) && (args.len() == 3 || args.len() == 4) {
            coerce_expr_to_expected(&mut args[0].value, expected);
            return;
        }
    }
    match expected.unwrap_read_only() {
        Type::Array(elem_ty, _) => match &mut expr.kind {
            ExprKind::List(items) => {
                let mut taken = std::mem::take(items);
                for it in &mut taken {
                    coerce_expr_to_expected(it, elem_ty);
                }
                expr.kind = ExprKind::ArrayLit(taken);
            }
            ExprKind::ArrayLit(items) => {
                for it in items {
                    coerce_expr_to_expected(it, elem_ty);
                }
            }
            ExprKind::Ternary { yes, no, .. } => {
                coerce_expr_to_expected(yes, expected);
                coerce_expr_to_expected(no, expected);
            }
            ExprKind::Call { callee, args }
                if is_select_callee_syntax(callee) && args.len() == 3 =>
            {
                coerce_expr_to_expected(&mut args[1].value, expected);
                coerce_expr_to_expected(&mut args[2].value, expected);
            }
            _ => {}
        },
        Type::List(elem_ty) if **elem_ty != Type::Unknown => match &mut expr.kind {
            ExprKind::List(items) => {
                for it in items {
                    coerce_expr_to_expected(it, elem_ty);
                }
            }
            ExprKind::Ternary { yes, no, .. } => {
                coerce_expr_to_expected(yes, expected);
                coerce_expr_to_expected(no, expected);
            }
            ExprKind::Call { callee, args }
                if is_select_callee_syntax(callee) && args.len() == 3 =>
            {
                coerce_expr_to_expected(&mut args[1].value, expected);
                coerce_expr_to_expected(&mut args[2].value, expected);
            }
            _ => {}
        },
        Type::Tuple(elem_tys) => match &mut expr.kind {
            ExprKind::Tuple(elems) if elems.len() == elem_tys.len() => {
                for (e, t) in elems.iter_mut().zip(elem_tys) {
                    coerce_expr_to_expected(e, t);
                }
            }
            ExprKind::Ternary { yes, no, .. } => {
                coerce_expr_to_expected(yes, expected);
                coerce_expr_to_expected(no, expected);
            }
            _ => {}
        },
        Type::Map(k_ty, v_ty) if **k_ty != Type::Unknown => {
            if let ExprKind::MapLit(entries) = &mut expr.kind {
                for (ke, ve) in entries {
                    coerce_expr_to_expected(ke, k_ty);
                    coerce_expr_to_expected(ve, v_ty);
                }
            }
        }
        _ => {}
    }
}

fn write_back_program(
    mod_key: &str,
    prog: &mut Program,
    consts: &BTreeMap<(String, String), ConstEntry>,
    fns: &BTreeMap<(String, String), FnEntry>,
    chambers: &BTreeMap<(String, String), ChamberDecl>,
    prepares: &BTreeMap<(String, String), PrepareDecl>,
    observes: &BTreeMap<(String, String), ObserveDecl>,
) {
    for item in &mut prog.items {
        match item {
            Item::Const(c) => {
                if let Some(ce) = consts.get(&(mod_key.to_string(), c.name.clone())) {
                    c.value = ce.value.clone();
                }
            }
            Item::DecFn(f) => {
                if let Some(fe) = fns.get(&(mod_key.to_string(), f.name.clone())) {
                    *f = fe.decl.clone();
                }
            }
            Item::Chamber(ch) => {
                if let Some(cd) = chambers.get(&(mod_key.to_string(), ch.name.clone())) {
                    *ch = cd.clone();
                }
            }
            Item::Prepare(pr) => {
                if let Some(pd) = prepares.get(&(mod_key.to_string(), pr.name.clone())) {
                    *pr = pd.clone();
                }
            }
            Item::Observe(ob) => {
                if let Some(od) = observes.get(&(mod_key.to_string(), ob.name.clone())) {
                    *ob = od.clone();
                }
            }
            Item::QuarantineMod(qm) => {
                let sub_key = format!("{mod_key}::{}", qm.name);
                for sub_item in &mut qm.items {
                    match sub_item {
                        Item::Const(c) => {
                            if let Some(ce) = consts.get(&(sub_key.clone(), c.name.clone())) {
                                c.value = ce.value.clone();
                            }
                        }
                        Item::DecFn(f) => {
                            if let Some(fe) = fns.get(&(sub_key.clone(), f.name.clone())) {
                                *f = fe.decl.clone();
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

fn is_irrefutable_pattern(pat: &Pattern) -> bool {
    match pat {
        Pattern::Wildcard | Pattern::Binding(_) | Pattern::NullLit => true,
        Pattern::Tuple(ps) => ps.iter().all(is_irrefutable_pattern),
        _ => false,
    }
}

fn types_compatible_with_annotation(actual: &Type, expected: &Type) -> bool {
    if actual == expected {
        return true;
    }
    match (actual, expected) {
        (Type::List(a), Type::List(e)) => {
            **a == Type::Unknown || types_compatible_with_annotation(a, e)
        }
        (Type::Array(a, an), Type::Array(e, en)) if an == en => {
            types_compatible_with_annotation(a, e)
        }
        (Type::Map(ak, av), Type::Map(ek, ev)) => {
            (**ak == Type::Unknown && **av == Type::Unknown)
                || (ak == ek && types_compatible_with_annotation(av, ev))
        }
        (Type::Tuple(as_), Type::Tuple(es)) if as_.len() == es.len() => as_
            .iter()
            .zip(es)
            .all(|(a, e)| types_compatible_with_annotation(a, e)),
        _ => false,
    }
}

fn contains_unknown(ty: &Type) -> bool {
    match ty {
        Type::Unknown => true,
        Type::List(inner) | Type::Array(inner, _) | Type::ReadOnly(inner) | Type::QExt(inner) => {
            contains_unknown(inner)
        }
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
            | "boundary_bytes",
        ) => Ok(Type::Bytes),
        (Type::Box3, "boundary_bytes") => Ok(Type::Bytes),
        (Type::SimResult, "ticks")
        | (Type::SimResult, "carrier_count")
        | (Type::SimResult, "shedding_event_count")
        | (Type::SimResult, "wavelet_event_count")
        | (Type::SimResult, "boundary_crossings")
        | (Type::SimResult, "total_returns")
        | (Type::SimResult, "total_hinges")
        | (Type::SimResult, "total_shedding_events") => Ok(Type::Int),
        (Type::SimResult, "final_snapshot") => Ok(Type::ReadOnly(Box::new(Type::ChamberView))),

        (Type::ChamberView, "chamber_name" | "model_status_bytes" | "chamber_name_bytes") => {
            Ok(Type::Bytes)
        }
        (Type::ChamberView, "tick")
        | (Type::ChamberView, "carrier_count")
        | (Type::ChamberView, "boundary_crossings")
        | (Type::ChamberView, "shedding_count")
        | (Type::ChamberView, "wavelet_count")
        | (Type::ChamberView, "clock_count") => Ok(Type::Int),
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
