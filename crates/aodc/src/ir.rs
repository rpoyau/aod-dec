use std::collections::BTreeMap;

use aod_core::bigint::BigInt;
use aod_core::json::{parse_strict_json, JsonValue};
use aod_core::sha256::Sha256;

use crate::ast::{
    AssignTarget, DecArm, DecArmParam, DecFnKind, Expr, ExprKind, Item, Pattern, ProbeDecl,
    Program, Stmt, StmtKind, TypeExpr,
};
use crate::checker::{check_unit, CheckedUnit};
use crate::diagnostics::{CompatMode, DecResult, Diagnostic, DiagnosticCode};
use crate::modules::{load_hermetic, BundledModule};

pub const IR_FORMAT: &str = "aod.dec.ir";
pub const MAX_IR_BYTES: usize = 8_000_000;

/// Encode a checked unit into canonical deterministic `DecIR` (`version: 1` for `--compat v1`,
/// `version: 2` for Dec v2 bundling all resolved filesystem module sources + SHA-256 digests).
pub fn encode_ir_json(checked: &CheckedUnit) -> JsonValue {
    let mut root = BTreeMap::new();
    root.insert("format".to_string(), JsonValue::from_str_val(IR_FORMAT));
    root.insert(
        "source".to_string(),
        JsonValue::from_str_val(checked.unit.entry_source.clone()),
    );
    root.insert(
        "source_sha256".to_string(),
        JsonValue::from_str_val(checked.unit.entry_sha256.clone()),
    );

    match checked.unit.compat {
        CompatMode::V1 => {
            root.insert("version".to_string(), JsonValue::from_i64(1));
            let mut funcs = Vec::new();
            for item in &checked.unit.entry_program.items {
                if let Item::DecFn(f) = item {
                    let mut fobj = BTreeMap::new();
                    fobj.insert("line".to_string(), JsonValue::from_usize(f.line));
                    fobj.insert("name".to_string(), JsonValue::from_str_val(f.name.clone()));
                    if let DecFnKind::MultiArm {
                        header_params,
                        output_name,
                        arms,
                        ..
                    } = &f.kind
                    {
                        fobj.insert(
                            "output".to_string(),
                            JsonValue::from_str_val(output_name.clone().unwrap_or_default()),
                        );
                        fobj.insert(
                            "params".to_string(),
                            JsonValue::Array(
                                header_params
                                    .iter()
                                    .map(|p| JsonValue::from_str_val(p.clone()))
                                    .collect(),
                            ),
                        );
                        let branches = arms
                            .iter()
                            .map(|arm| {
                                let mut bobj = BTreeMap::new();
                                bobj.insert("line".to_string(), JsonValue::from_usize(arm.line));
                                bobj.insert(
                                    "output".to_string(),
                                    JsonValue::from_str_val(
                                        arm.output_name.clone().unwrap_or_default(),
                                    ),
                                );
                                bobj.insert(
                                    "params".to_string(),
                                    JsonValue::Array(
                                        arm.params
                                            .iter()
                                            .map(|p| {
                                                let mut pobj = BTreeMap::new();
                                                match p {
                                                    crate::ast::DecArmParam::TypedVar { ty, name } => {
                                                        pobj.insert(
                                                            "name".to_string(),
                                                            JsonValue::from_str_val(name.clone()),
                                                        );
                                                        pobj.insert(
                                                            "type".to_string(),
                                                            JsonValue::from_str_val(format!("{ty:?}")),
                                                        );
                                                    }
                                                    other => {
                                                        pobj.insert(
                                                            "pattern".to_string(),
                                                            JsonValue::from_str_val(format!("{other:?}")),
                                                        );
                                                    }
                                                }
                                                JsonValue::Object(pobj)
                                            })
                                            .collect(),
                                    ),
                                );
                                bobj.insert(
                                    "return_type".to_string(),
                                    JsonValue::from_str_val(format!("{:?}", arm.return_type)),
                                );
                                bobj.insert(
                                    "stmt_count".to_string(),
                                    JsonValue::from_usize(arm.body.len()),
                                );
                                JsonValue::Object(bobj)
                            })
                            .collect();
                        fobj.insert("branches".to_string(), JsonValue::Array(branches));
                    }
                    funcs.push(JsonValue::Object(fobj));
                }
            }
            root.insert("functions".to_string(), JsonValue::Array(funcs));
        }
        CompatMode::V2 => {
            root.insert("version".to_string(), JsonValue::from_i64(2));
            let mut mods_map = BTreeMap::new();
            for (k, m) in &checked.unit.bundled_modules {
                let mut mobj = BTreeMap::new();
                mobj.insert("sha256".to_string(), JsonValue::from_str_val(m.sha256.clone()));
                mobj.insert("source".to_string(), JsonValue::from_str_val(m.source.clone()));
                mods_map.insert(k.clone(), JsonValue::Object(mobj));
            }
            root.insert("modules".to_string(), JsonValue::Object(mods_map));

            let mut decl_names = Vec::new();
            for item in &checked.unit.entry_program.items {
                let tag = match item {
                    Item::Use(_) => "use".to_string(),
                    Item::Const(c) => format!("const:{}", c.name),
                    Item::Struct(s) => format!("struct:{}", s.name),
                    Item::Enum(e) => format!("enum:{}", e.name),
                    Item::DecFn(f) => format!("dec:{}", f.name),
                    Item::Chamber(c) => format!("chamber:{}", c.name),
                    Item::Prepare(p) => format!("prepare:{}", p.name),
                    Item::Observe(o) => format!("observe:{}", o.name),
                    Item::QuarantineMod(q) => format!("quarantine:{}", q.name),
                };
                decl_names.push(JsonValue::from_str_val(tag));
            }
            root.insert("declarations".to_string(), JsonValue::Array(decl_names));
            root.insert(
                "ast_summary".to_string(),
                encode_v2_ast_summary(&checked.unit.entry_program, &checked.unit.parsed_modules),
            );
        }
    }

    JsonValue::Object(root)
}

#[derive(Default)]
struct AstSummaryCounts {
    null_lit_count: i64,
    bytes_lit_count: i64,
    array_lit_count: i64,
    array_type_specs: Vec<String>,
    single_arm_arrows: Vec<JsonValue>,
}

fn format_type_expr(te: &TypeExpr) -> String {
    match te {
        TypeExpr::Unit => "()".to_string(),
        TypeExpr::Named(n) => n.clone(),
        TypeExpr::List(inner) => format!("[{}]", format_type_expr(inner)),
        TypeExpr::Array(inner, len) => format!("array[{}, {len}]", format_type_expr(inner)),
        TypeExpr::Map(k, v) => format!("map[{}, {}]", format_type_expr(k), format_type_expr(v)),
        TypeExpr::Tuple(ts) => {
            let parts: Vec<String> = ts.iter().map(format_type_expr).collect();
            format!("({})", parts.join(", "))
        }
        TypeExpr::ReadOnly(inner) => format!("ReadOnly<{}>", format_type_expr(inner)),
        TypeExpr::QExt(inner) => format!("QExt<{}>", format_type_expr(inner)),
    }
}

impl AstSummaryCounts {
    fn visit_type_expr(&mut self, te: &TypeExpr) {
        match te {
            TypeExpr::Unit | TypeExpr::Named(_) => {}
            TypeExpr::List(inner) | TypeExpr::ReadOnly(inner) | TypeExpr::QExt(inner) => {
                self.visit_type_expr(inner);
            }
            TypeExpr::Array(inner, _) => {
                self.array_type_specs.push(format_type_expr(te));
                self.visit_type_expr(inner);
            }
            TypeExpr::Map(k, v) => {
                self.visit_type_expr(k);
                self.visit_type_expr(v);
            }
            TypeExpr::Tuple(ts) => {
                for t in ts {
                    self.visit_type_expr(t);
                }
            }
        }
    }

    fn visit_pattern(&mut self, pat: &Pattern) {
        match pat {
            Pattern::NullLit => self.null_lit_count += 1,
            Pattern::BytesLit(_) => self.bytes_lit_count += 1,
            Pattern::EnumVariant { bindings, .. } | Pattern::Tuple(bindings) => {
                for b in bindings {
                    self.visit_pattern(b);
                }
            }
            _ => {}
        }
    }

    fn visit_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::NullLit => self.null_lit_count += 1,
            ExprKind::BytesLit(_) => self.bytes_lit_count += 1,
            ExprKind::ArrayLit(items) => {
                self.array_lit_count += 1;
                for it in items {
                    self.visit_expr(it);
                }
            }
            ExprKind::List(items) | ExprKind::Tuple(items) | ExprKind::BoxExpr(items) => {
                for it in items {
                    self.visit_expr(it);
                }
            }
            ExprKind::MapLit(entries) => {
                for (k, v) in entries {
                    self.visit_expr(k);
                    self.visit_expr(v);
                }
            }
            ExprKind::StructLit { fields, .. } => {
                for (_, fe) in fields {
                    self.visit_expr(fe);
                }
            }
            ExprKind::WindowLit { start, end } | ExprKind::Range { start, end } => {
                self.visit_expr(start);
                self.visit_expr(end);
            }
            ExprKind::Simulate { ticks, .. } => self.visit_expr(ticks),
            ExprKind::Unary { operand, .. } | ExprKind::Field { base: operand, .. } => {
                self.visit_expr(operand);
            }
            ExprKind::Binary { left, right, .. }
            | ExprKind::Index {
                base: left,
                index: right,
            } => {
                self.visit_expr(left);
                self.visit_expr(right);
            }
            ExprKind::Ternary { cond, yes, no } => {
                self.visit_expr(cond);
                self.visit_expr(yes);
                self.visit_expr(no);
            }
            ExprKind::Call { callee, args } => {
                self.visit_expr(callee);
                for a in args {
                    self.visit_expr(&a.value);
                }
            }
            _ => {}
        }
    }

    fn visit_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Let { ty, value, .. } | StmtKind::Mut { ty, value, .. } => {
                if let Some(te) = ty {
                    self.visit_type_expr(te);
                }
                self.visit_expr(value);
            }
            StmtKind::TypedLocal { ty, value, .. } => {
                self.visit_type_expr(ty);
                self.visit_expr(value);
            }
            StmtKind::Assign { target, value } => {
                match target {
                    AssignTarget::Ident(_) => {}
                    AssignTarget::Field { base, .. } => self.visit_expr(base),
                    AssignTarget::Index { base, index } => {
                        self.visit_expr(base);
                        self.visit_expr(index);
                    }
                }
                self.visit_expr(value);
            }
            StmtKind::Assert(e) | StmtKind::Emit(e) | StmtKind::ExprStmt(e) => self.visit_expr(e),
            StmtKind::Return(Some(e)) => self.visit_expr(e),
            StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue | StmtKind::Pass => {}
            StmtKind::If {
                cond,
                then_body,
                elifs,
                else_body,
            } => {
                self.visit_expr(cond);
                for s in then_body {
                    self.visit_stmt(s);
                }
                for (ec, eb) in elifs {
                    self.visit_expr(ec);
                    for s in eb {
                        self.visit_stmt(s);
                    }
                }
                for s in else_body {
                    self.visit_stmt(s);
                }
            }
            StmtKind::Match { scrutinee, arms } => {
                self.visit_expr(scrutinee);
                for arm in arms {
                    self.visit_pattern(&arm.pattern);
                    if let Some(ref g) = arm.guard {
                        self.visit_expr(g);
                    }
                    for s in &arm.body {
                        self.visit_stmt(s);
                    }
                }
            }
            StmtKind::For { iter, body, .. } | StmtKind::While { cond: iter, body } => {
                self.visit_expr(iter);
                for s in body {
                    self.visit_stmt(s);
                }
            }
            StmtKind::QuarantineBlock { body } => {
                for s in body {
                    self.visit_stmt(s);
                }
            }
        }
    }

    fn visit_dec_arm(&mut self, arm: &DecArm) {
        for p in &arm.params {
            match p {
                DecArmParam::TypedVar { ty, .. } => self.visit_type_expr(ty),
                DecArmParam::TypedPattern { ty, pattern } => {
                    self.visit_type_expr(ty);
                    self.visit_pattern(pattern);
                }
                DecArmParam::Pattern(pat) => self.visit_pattern(pat),
            }
        }
        if let Some(ref g) = arm.guard {
            self.visit_expr(g);
        }
        self.visit_type_expr(&arm.return_type);
        for s in &arm.body {
            self.visit_stmt(s);
        }
    }

    fn visit_program(&mut self, mod_key: &str, prog: &Program) {
        for item in &prog.items {
            match item {
                Item::Struct(sd) => {
                    for (_, fty) in &sd.fields {
                        self.visit_type_expr(fty);
                    }
                }
                Item::Enum(ed) => {
                    for v in &ed.variants {
                        for pty in &v.payloads {
                            self.visit_type_expr(pty);
                        }
                    }
                }
                Item::Const(cd) => {
                    if let Some(ref te) = cd.ty {
                        self.visit_type_expr(te);
                    }
                    self.visit_expr(&cd.value);
                }
                Item::DecFn(fd) => self.visit_dec_fn(mod_key, fd),
                Item::Chamber(ch) => self.visit_expr(&ch.space),
                Item::Prepare(pr) => {
                    for s in &pr.lets {
                        self.visit_stmt(s);
                    }
                    for clk in &pr.clocks {
                        self.visit_expr(&clk.period);
                    }
                    for inj in &pr.injections {
                        self.visit_expr(&inj.support);
                        self.visit_expr(&inj.pos);
                        self.visit_expr(&inj.orient);
                        self.visit_expr(&inj.shells);
                    }
                }
                Item::Observe(ob) => {
                    for probe in &ob.probes {
                        match probe {
                            ProbeDecl::Declarative { region, .. } => self.visit_expr(region),
                            ProbeDecl::Procedural {
                                return_type, body, ..
                            } => {
                                self.visit_type_expr(return_type);
                                for s in body {
                                    self.visit_stmt(s);
                                }
                            }
                        }
                    }
                }
                Item::QuarantineMod(qm) => {
                    let sub_key = format!("{mod_key}::{}", qm.name);
                    for sub in &qm.items {
                        match sub {
                            Item::Const(cd) => {
                                if let Some(ref te) = cd.ty {
                                    self.visit_type_expr(te);
                                }
                                self.visit_expr(&cd.value);
                            }
                            Item::DecFn(fd) => self.visit_dec_fn(&sub_key, fd),
                            _ => {}
                        }
                    }
                }
                Item::Use(_) => {}
            }
        }
    }

    fn visit_dec_fn(&mut self, mod_key: &str, fd: &crate::ast::DecFnDecl) {
        match &fd.kind {
            DecFnKind::SingleArm {
                params,
                has_arrow,
                return_type,
                body,
            } => {
                let mut rec = BTreeMap::new();
                rec.insert(
                    "fn".to_string(),
                    JsonValue::from_str_val(format!("{mod_key}::{}", fd.name)),
                );
                rec.insert("has_arrow".to_string(), JsonValue::Bool(*has_arrow));
                self.single_arm_arrows.push(JsonValue::Object(rec));
                for p in params {
                    self.visit_type_expr(&p.ty);
                }
                self.visit_type_expr(return_type);
                for s in body {
                    self.visit_stmt(s);
                }
            }
            DecFnKind::MultiArm {
                header_typed_params,
                header_return_type,
                arms,
                ..
            } => {
                if let Some(htp) = header_typed_params {
                    for p in htp {
                        self.visit_type_expr(&p.ty);
                    }
                }
                if let Some(hrt) = header_return_type {
                    self.visit_type_expr(hrt);
                }
                for arm in arms {
                    self.visit_dec_arm(arm);
                }
            }
        }
    }
}

fn encode_v2_ast_summary(
    entry_program: &Program,
    parsed_modules: &BTreeMap<String, Program>,
) -> JsonValue {
    let mut acc = AstSummaryCounts::default();
    acc.visit_program("<entry>", entry_program);
    for (mk, prog) in parsed_modules {
        acc.visit_program(mk, prog);
    }
    let mut m = BTreeMap::new();
    m.insert(
        "array_lit_count".to_string(),
        JsonValue::from_i64(acc.array_lit_count),
    );
    m.insert(
        "array_types".to_string(),
        JsonValue::Array(
            acc.array_type_specs
                .into_iter()
                .map(JsonValue::from_str_val)
                .collect(),
        ),
    );
    m.insert(
        "bytes_lit_count".to_string(),
        JsonValue::from_i64(acc.bytes_lit_count),
    );
    m.insert(
        "null_lit_count".to_string(),
        JsonValue::from_i64(acc.null_lit_count),
    );
    m.insert(
        "single_arms".to_string(),
        JsonValue::Array(acc.single_arm_arrows),
    );
    JsonValue::Object(m)
}

pub fn dumps_ir(checked: &CheckedUnit) -> String {
    encode_ir_json(checked).to_pretty_utf8_json()
}

/// Strictly parse and hermetically rebuild-verify a `DecIR` payload (`P1-2`).
///
/// Rejects:
/// - payloads exceeding 8 MB
/// - duplicate JSON keys or floating-point numbers
/// - unsupported format or version
/// - source or bundled module SHA-256 mismatch
/// - ANY structural/field diff against canonical hermetically rebuilt IR (zero ambient FS reads!).
pub fn loads_ir(text: &str) -> DecResult<CheckedUnit> {
    if text.len() > MAX_IR_BYTES {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            "IR exceeds the 8 MB core limit",
        ));
    }
    let parsed = parse_strict_json(text).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            format!("invalid IR JSON: {e}"),
        )
    })?;
    let obj = parsed.as_object().ok_or_else(|| {
        Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            "IR root must be a JSON object",
        )
    })?;

    if obj.get("format").and_then(JsonValue::as_str) != Some(IR_FORMAT) {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            "unsupported Dec IR format",
        ));
    }
    let version_int = obj
        .get("version")
        .and_then(JsonValue::as_int)
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                "IR version must be an exact integer",
            )
        })?;
    let compat = if *version_int == BigInt::from_i64(1) {
        CompatMode::V1
    } else if *version_int == BigInt::from_i64(2) {
        CompatMode::V2
    } else {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            format!("unsupported Dec IR version {version_int}"),
        ));
    };

    let source = obj
        .get("source")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                "IR source must be a string",
            )
        })?;
    let claimed_sha = obj
        .get("source_sha256")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                "IR source_sha256 must be a string",
            )
        })?;
    let actual_sha = Sha256::digest_hex(source.as_bytes());
    if claimed_sha != actual_sha {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            "IR source_sha256 does not match embedded source",
        ));
    }

    let mut bundled = BTreeMap::new();
    if compat == CompatMode::V2 {
        let mods = obj
            .get("modules")
            .and_then(JsonValue::as_object)
            .ok_or_else(|| {
                Diagnostic::new(
                    DiagnosticCode::IrVerificationError,
                    "DecIR v2 requires a modules object",
                )
            })?;
        for (mod_name, mod_val) in mods {
            let mobj = mod_val.as_object().ok_or_else(|| {
                Diagnostic::new(
                    DiagnosticCode::IrVerificationError,
                    format!("module entry {mod_name:?} must be a JSON object"),
                )
            })?;
            let m_src = mobj
                .get("source")
                .and_then(JsonValue::as_str)
                .ok_or_else(|| {
                    Diagnostic::new(
                        DiagnosticCode::IrVerificationError,
                        format!("module {mod_name:?} missing source"),
                    )
                })?;
            let m_sha = mobj
                .get("sha256")
                .and_then(JsonValue::as_str)
                .ok_or_else(|| {
                    Diagnostic::new(
                        DiagnosticCode::IrVerificationError,
                        format!("module {mod_name:?} missing sha256"),
                    )
                })?;
            if Sha256::digest_hex(m_src.as_bytes()) != m_sha {
                return Err(Diagnostic::new(
                    DiagnosticCode::IrVerificationError,
                    format!("module {mod_name:?} SHA-256 mismatch"),
                ));
            }
            bundled.insert(
                mod_name.clone(),
                BundledModule {
                    source: m_src.to_string(),
                    sha256: m_sha.to_string(),
                },
            );
        }
    }

    // Strict hermetic rebuild with ZERO ambient filesystem access!
    let loaded = load_hermetic(source, &bundled, compat)?;
    let rebuilt = check_unit(loaded, true)?;

    // Canonical JSON equality prevents bool/int tampering, extra fields, or modified metadata.
    let expected_canonical = dumps_ir(&rebuilt);
    let parsed_canonical = parsed.to_pretty_utf8_json();
    if parsed_canonical != expected_canonical {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            "IR payload or metadata differs from hermetically validated source",
        ));
    }

    Ok(rebuilt)
}
