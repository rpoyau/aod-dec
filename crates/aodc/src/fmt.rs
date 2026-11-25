#![forbid(unsafe_code)]

//! Canonical deterministic `.dec` AST formatter (`aodc fmt [--compat v1|v2 | --strict-wave] [--check | --write | -o <out.dec>]`).

use std::fmt::Write;

use crate::ast::{
    AssignTarget, BinaryOp, CallArg, ChamberDecl, ConstDecl, DecArm, DecArmParam, DecFnDecl,
    DecFnKind, EnumDecl, Expr, ExprKind, ForTarget, Item, ObserveDecl, Pattern, PrepareDecl,
    ProbeDecl, ProbeKind, Program, Stmt, StmtKind, StructDecl, TritLiteral, TypeExpr, UnaryOp,
    UseDecl, WallPolicyAst,
};
use crate::diagnostics::{CompatMode, DecResult, Diagnostic, DiagnosticCode, SyntaxDiscipline};
use crate::parser::parse_program_with_discipline;

pub fn format_source(source: &str, compat: CompatMode) -> DecResult<String> {
    format_source_with_discipline(source, compat, SyntaxDiscipline::for_compat(compat))
}

pub fn format_source_with_discipline(
    source: &str,
    compat: CompatMode,
    discipline: SyntaxDiscipline,
) -> DecResult<String> {
    if compat == CompatMode::V1 && discipline == SyntaxDiscipline::PureWaveV2 {
        return Err(Diagnostic::new(
            DiagnosticCode::ParseError,
            "--strict-wave / PureWaveV2 cannot be combined with --compat v1",
        ));
    }
    if compat == CompatMode::V2 && discipline == SyntaxDiscipline::LegacyV1 {
        return Err(Diagnostic::new(
            DiagnosticCode::ParseError,
            "LegacyV1 cannot be combined with CompatMode::V2; Dec v2 is unconditionally PureWaveV2",
        ));
    }
    let prog = parse_program_with_discipline(source, compat, discipline)?;
    let formatted = format_program_with_mode(&prog, compat, discipline);
    // Verify round-trip parse validity before returning (`[P1-1]`).
    parse_program_with_discipline(&formatted, compat, discipline)?;
    Ok(formatted)
}

pub fn format_program(prog: &Program) -> String {
    format_program_with_mode(prog, CompatMode::V2, SyntaxDiscipline::PureWaveV2)
}

pub fn format_program_with_mode(
    prog: &Program,
    compat: CompatMode,
    discipline: SyntaxDiscipline,
) -> String {
    let mut out = String::new();
    for (idx, item) in prog.items.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        fmt_item(&mut out, item, 0, compat, discipline);
    }
    out
}

fn push_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push_str("    ");
    }
}

fn is_valid_engine_ident(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    if matches!(
        s,
        "Dec"
            | "dec"
            | "where"
            | "let"
            | "mut"
            | "const"
            | "return"
            | "emit"
            | "assert"
            | "if"
            | "elif"
            | "else"
            | "for"
            | "in"
            | "while"
            | "break"
            | "continue"
            | "pass"
            | "match"
            | "struct"
            | "enum"
            | "use"
            | "chamber"
            | "prepare"
            | "observe"
            | "simulate"
            | "quarantine"
            | "and"
            | "or"
            | "not"
            | "true"
            | "false"
            | "null"
    ) {
        return false;
    }
    s.bytes().enumerate().all(|(i, b)| {
        if i == 0 {
            b.is_ascii_alphabetic() || b == b'_'
        } else {
            b.is_ascii_alphanumeric() || b == b'_'
        }
    })
}

fn fmt_item(
    out: &mut String,
    item: &Item,
    indent: usize,
    compat: CompatMode,
    discipline: SyntaxDiscipline,
) {
    match item {
        Item::Use(u) => fmt_use(out, u, indent),
        Item::Struct(s) => fmt_struct(out, s, indent),
        Item::Enum(e) => fmt_enum(out, e, indent),
        Item::Const(c) => fmt_const(out, c, indent),
        Item::DecFn(f) => fmt_dec_fn(out, f, indent, compat),
        Item::Chamber(c) => fmt_chamber(out, c, indent, discipline),
        Item::Prepare(p) => fmt_prepare(out, p, indent),
        Item::Observe(o) => fmt_observe(out, o, indent),
        Item::QuarantineMod(qm) => {
            push_indent(out, indent);
            let _ = writeln!(out, "quarantine Q_ext {}:", qm.name);
            if qm.items.is_empty() {
                push_indent(out, indent + 1);
                out.push_str("pass\n");
            } else {
                for (i, sub) in qm.items.iter().enumerate() {
                    if i > 0 {
                        out.push('\n');
                    }
                    fmt_item(out, sub, indent + 1, compat, discipline);
                }
            }
        }
    }
}

fn fmt_use(out: &mut String, u: &UseDecl, indent: usize) {
    push_indent(out, indent);
    let path = u.path.join("::");
    match &u.items {
        None => {
            let _ = writeln!(out, "use {path}");
        }
        Some(syms) if syms.len() == 1 => {
            let _ = writeln!(out, "use {path}::{}", syms[0]);
        }
        Some(syms) => {
            let _ = writeln!(out, "use {path}:");
            push_indent(out, indent + 1);
            let _ = writeln!(out, "{}", syms.join(", "));
        }
    }
}

fn fmt_struct(out: &mut String, s: &StructDecl, indent: usize) {
    push_indent(out, indent);
    let _ = writeln!(out, "struct {}:", s.name);
    for (fname, fty) in &s.fields {
        push_indent(out, indent + 1);
        let _ = writeln!(out, "{fname}: {}", fmt_type(fty));
    }
}

fn fmt_enum(out: &mut String, e: &EnumDecl, indent: usize) {
    push_indent(out, indent);
    let _ = writeln!(out, "enum {}:", e.name);
    for v in &e.variants {
        push_indent(out, indent + 1);
        if v.payloads.is_empty() {
            let _ = writeln!(out, "{}", v.name);
        } else {
            let ps: Vec<String> = v.payloads.iter().map(fmt_type).collect();
            let _ = writeln!(out, "{}({})", v.name, ps.join(", "));
        }
    }
}

fn fmt_const(out: &mut String, c: &ConstDecl, indent: usize) {
    push_indent(out, indent);
    match &c.ty {
        Some(t) => {
            let _ = writeln!(
                out,
                "const {}: {} = {}",
                c.name,
                fmt_type(t),
                fmt_expr(&c.value)
            );
        }
        None => {
            let _ = writeln!(out, "const {} = {}", c.name, fmt_expr(&c.value));
        }
    }
}

fn fmt_dec_fn(out: &mut String, f: &DecFnDecl, indent: usize, compat: CompatMode) {
    match &f.kind {
        DecFnKind::SingleArm {
            params,
            return_type,
            has_arrow,
            body,
        } => {
            push_indent(out, indent);
            let kw = if compat == CompatMode::V1 { "Dec" } else { "dec" };
            let ps: Vec<String> = params
                .iter()
                .map(|p| format!("{}: {}", p.name, fmt_type(&p.ty)))
                .collect();
            if *has_arrow {
                let _ = writeln!(
                    out,
                    "{kw} {}({}) -> {}:",
                    f.name,
                    ps.join(", "),
                    fmt_type(return_type)
                );
            } else {
                let _ = writeln!(out, "{kw} {}({}):", f.name, ps.join(", "));
            }
            fmt_stmts(out, body, indent + 1);
        }
        DecFnKind::MultiArm {
            has_header_params,
            header_params,
            header_typed_params,
            output_name,
            header_return_type,
            arms,
        } => {
            push_indent(out, indent);
            if compat == CompatMode::V1 {
                let _ = write!(out, "Dec {}({})", f.name, header_params.join(", "));
                if let Some(oname) = output_name {
                    let _ = write!(out, " -> {oname}");
                }
                out.push_str(":\n");
            } else if *has_header_params {
                if let Some(htp) = header_typed_params {
                    let ps: Vec<String> = htp
                        .iter()
                        .map(|p| format!("{}: {}", p.name, fmt_type(&p.ty)))
                        .collect();
                    let _ = write!(out, "dec {}({})", f.name, ps.join(", "));
                    if let Some(hrt) = header_return_type {
                        let _ = write!(out, " -> {}", fmt_type(hrt));
                    }
                    out.push_str(":\n");
                } else if !header_params.is_empty() || output_name.is_some() {
                    let _ = write!(out, "Dec {}({})", f.name, header_params.join(", "));
                    if let Some(oname) = output_name {
                        let _ = write!(out, " -> {oname}");
                    }
                    out.push_str(":\n");
                } else {
                    let _ = writeln!(out, "dec {}():", f.name);
                }
            } else {
                let _ = writeln!(out, "dec {}:", f.name);
            }
            for arm in arms {
                fmt_dec_arm(out, arm, indent + 1, compat);
            }
        }
    }
}

fn fmt_dec_arm(out: &mut String, arm: &DecArm, indent: usize, compat: CompatMode) {
    push_indent(out, indent);
    let ps: Vec<String> = arm
        .params
        .iter()
        .map(|p| fmt_arm_param(p, compat))
        .collect();
    let _ = write!(out, "({})", ps.join(", "));
    if let Some(ref g) = arm.guard {
        let _ = write!(out, " where {}", fmt_expr(g));
    }
    let _ = write!(out, " -> {}", fmt_type(&arm.return_type));
    if let Some(ref oname) = arm.output_name {
        let _ = write!(out, " {oname}");
    }
    out.push_str(":\n");
    fmt_stmts(out, &arm.body, indent + 1);
}

fn fmt_arm_param(p: &DecArmParam, compat: CompatMode) -> String {
    match p {
        DecArmParam::TypedVar { name, ty } => {
            if compat == CompatMode::V1 {
                format!("{} {name}", fmt_type(ty))
            } else {
                format!("{name}: {}", fmt_type(ty))
            }
        }
        DecArmParam::TypedPattern { pattern, ty } => {
            format!("{}: {}", fmt_pattern(pattern), fmt_type(ty))
        }
        DecArmParam::Pattern(pat) => fmt_pattern(pat),
    }
}

fn fmt_chamber(
    out: &mut String,
    c: &ChamberDecl,
    indent: usize,
    discipline: SyntaxDiscipline,
) {
    push_indent(out, indent);
    let _ = writeln!(out, "chamber {}:", c.name);
    push_indent(out, indent + 1);
    let _ = writeln!(out, "space = {}", fmt_expr(&c.space));
    push_indent(out, indent + 1);
    let w = match c.walls {
        WallPolicyAst::Open => "open",
        WallPolicyAst::Closed => "closed",
    };
    let _ = writeln!(out, "walls = {w}");
    if let Some(ref eng) = c.engine {
        push_indent(out, indent + 1);
        if is_valid_engine_ident(eng) || discipline == SyntaxDiscipline::PureWaveV2 {
            let _ = writeln!(out, "engine = {eng}");
        } else {
            let _ = writeln!(out, "engine = {}", fmt_escaped_str(eng));
        }
    }
}

fn fmt_prepare(out: &mut String, p: &PrepareDecl, indent: usize) {
    push_indent(out, indent);
    let _ = writeln!(out, "prepare {} in {}:", p.name, p.chamber);
    for s in &p.lets {
        fmt_stmt(out, s, indent + 1);
    }
    for clk in &p.clocks {
        push_indent(out, indent + 1);
        let _ = writeln!(out, "clock {} = {}", clk.name, fmt_expr(&clk.period));
    }
    for inj in &p.injections {
        push_indent(out, indent + 1);
        let _ = writeln!(
            out,
            "inject {} at {}, dir {}, orient {}, shells {}",
            fmt_expr(&inj.support),
            fmt_expr(&inj.pos),
            inj.dir.as_str(),
            fmt_expr(&inj.orient),
            fmt_expr(&inj.shells)
        );
    }
}

fn fmt_observe(out: &mut String, o: &ObserveDecl, indent: usize) {
    push_indent(out, indent);
    let _ = writeln!(out, "observe {}:", o.name);
    for probe in &o.probes {
        match probe {
            ProbeDecl::Declarative {
                kind,
                name,
                region,
                capture,
                ..
            } => {
                let kw = match kind {
                    ProbeKind::Tracer => "tracer",
                    ProbeKind::Sensor => "sensor",
                };
                push_indent(out, indent + 1);
                let _ = writeln!(out, "{kw} {name}:");
                push_indent(out, indent + 2);
                let _ = writeln!(out, "region = {}", fmt_expr(region));
                if !capture.is_empty() {
                    push_indent(out, indent + 2);
                    let _ = writeln!(out, "capture = [{}]", capture.join(", "));
                }
            }
            ProbeDecl::Procedural {
                kind,
                name,
                param_name,
                param_ty,
                return_type,
                body,
                ..
            } => {
                let kw = match kind {
                    ProbeKind::Tracer => "tracer",
                    ProbeKind::Sensor => "sensor",
                };
                push_indent(out, indent + 1);
                let _ = writeln!(out, "{kw} {name}:");
                push_indent(out, indent + 2);
                match param_ty {
                    Some(pt) => {
                        let _ = writeln!(
                            out,
                            "({param_name}: {}) -> {}:",
                            fmt_type(pt),
                            fmt_type(return_type)
                        );
                    }
                    None => {
                        let _ = writeln!(
                            out,
                            "({param_name}: ReadOnly<ChamberView>) -> {}:",
                            fmt_type(return_type)
                        );
                    }
                }
                fmt_stmts(out, body, indent + 3);
            }
        }
    }
}

fn fmt_stmts(out: &mut String, stmts: &[Stmt], indent: usize) {
    if stmts.is_empty() {
        push_indent(out, indent);
        out.push_str("pass\n");
        return;
    }
    for s in stmts {
        fmt_stmt(out, s, indent);
    }
}

fn fmt_stmt(out: &mut String, stmt: &Stmt, indent: usize) {
    match &stmt.kind {
        StmtKind::Let { name, ty, value } => {
            push_indent(out, indent);
            match ty {
                Some(t) => {
                    let _ = writeln!(out, "let {name}: {} = {}", fmt_type(t), fmt_expr(value));
                }
                None => {
                    let _ = writeln!(out, "let {name} = {}", fmt_expr(value));
                }
            }
        }
        StmtKind::LetTuple { elems, ty, value } => {
            push_indent(out, indent);
            let ps: Vec<String> = elems
                .iter()
                .map(|e| match &e.ty {
                    Some(t) => format!("{}: {}", e.name, fmt_type(t)),
                    None => e.name.clone(),
                })
                .collect();
            match ty {
                Some(t) => {
                    let _ = writeln!(
                        out,
                        "let ({}): {} = {}",
                        ps.join(", "),
                        fmt_type(t),
                        fmt_expr(value)
                    );
                }
                None => {
                    let _ = writeln!(out, "let ({}) = {}", ps.join(", "), fmt_expr(value));
                }
            }
        }
        StmtKind::Mut { name, ty, value } => {
            push_indent(out, indent);
            match ty {
                Some(t) => {
                    let _ = writeln!(out, "mut {name}: {} = {}", fmt_type(t), fmt_expr(value));
                }
                None => {
                    let _ = writeln!(out, "mut {name} = {}", fmt_expr(value));
                }
            }
        }
        StmtKind::TypedLocal { ty, name, value } => {
            push_indent(out, indent);
            let _ = writeln!(out, "{} {name} = {}", fmt_type(ty), fmt_expr(value));
        }
        StmtKind::Assign { target, value } => {
            push_indent(out, indent);
            let lhs = match target {
                AssignTarget::Ident(n) => n.clone(),
                AssignTarget::Field { base, name } => format!("{}.{name}", fmt_atom(base)),
                AssignTarget::Index { base, index } => {
                    format!("{}[{}]", fmt_atom(base), fmt_expr(index))
                }
            };
            let _ = writeln!(out, "{lhs} = {}", fmt_expr(value));
        }
        StmtKind::Assert(e) => {
            push_indent(out, indent);
            let _ = writeln!(out, "assert {}", fmt_expr(e));
        }
        StmtKind::Emit(e) => {
            push_indent(out, indent);
            let _ = writeln!(out, "emit {}", fmt_expr(e));
        }
        StmtKind::Return(Some(e)) => {
            push_indent(out, indent);
            let _ = writeln!(out, "return {}", fmt_expr(e));
        }
        StmtKind::Return(None) => {
            push_indent(out, indent);
            out.push_str("return\n");
        }
        StmtKind::If {
            cond,
            then_body,
            elifs,
            else_body,
        } => {
            push_indent(out, indent);
            let _ = writeln!(out, "if {}:", fmt_expr(cond));
            fmt_stmts(out, then_body, indent + 1);
            for (ec, eb) in elifs {
                push_indent(out, indent);
                let _ = writeln!(out, "elif {}:", fmt_expr(ec));
                fmt_stmts(out, eb, indent + 1);
            }
            if !else_body.is_empty() {
                push_indent(out, indent);
                out.push_str("else:\n");
                fmt_stmts(out, else_body, indent + 1);
            }
        }
        StmtKind::Match { scrutinee, arms } => {
            push_indent(out, indent);
            let _ = writeln!(out, "match {}:", fmt_expr(scrutinee));
            for arm in arms {
                push_indent(out, indent + 1);
                let _ = write!(out, "{}", fmt_pattern(&arm.pattern));
                if let Some(ref g) = arm.guard {
                    let _ = write!(out, " if {}", fmt_expr(g));
                }
                out.push_str(":\n");
                fmt_stmts(out, &arm.body, indent + 2);
            }
        }
        StmtKind::For { target, iter, body } => {
            push_indent(out, indent);
            let tgt = match target {
                ForTarget::Single(n) => n.clone(),
                ForTarget::Tuple(ns) => format!("({})", ns.join(", ")),
            };
            let _ = writeln!(out, "for {tgt} in {}:", fmt_expr(iter));
            fmt_stmts(out, body, indent + 1);
        }
        StmtKind::While { cond, body } => {
            push_indent(out, indent);
            let _ = writeln!(out, "while {}:", fmt_expr(cond));
            fmt_stmts(out, body, indent + 1);
        }
        StmtKind::Break => {
            push_indent(out, indent);
            out.push_str("break\n");
        }
        StmtKind::Continue => {
            push_indent(out, indent);
            out.push_str("continue\n");
        }
        StmtKind::Pass => {
            push_indent(out, indent);
            out.push_str("pass\n");
        }
        StmtKind::QuarantineBlock { body } => {
            push_indent(out, indent);
            out.push_str("quarantine Q_ext:\n");
            fmt_stmts(out, body, indent + 1);
        }
        StmtKind::ExprStmt(e) => {
            push_indent(out, indent);
            let _ = writeln!(out, "{}", fmt_expr(e));
        }
    }
}

fn fmt_type(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Unit => "()".to_string(),
        TypeExpr::Named(n) => n.clone(),
        TypeExpr::List(inner) => format!("[{}]", fmt_type(inner)),
        TypeExpr::Array(inner, n) => format!("array[{}, {n}]", fmt_type(inner)),
        TypeExpr::Map(k, v) => format!("map[{}, {}]", fmt_type(k), fmt_type(v)),
        TypeExpr::Tuple(ts) => {
            let ps: Vec<String> = ts.iter().map(fmt_type).collect();
            format!("({})", ps.join(", "))
        }
        TypeExpr::ReadOnly(arg) => format!("ReadOnly<{}>", fmt_type(arg)),
        TypeExpr::QExt(arg) => format!("QExt<{}>", fmt_type(arg)),
    }
}

fn fmt_pattern(pat: &Pattern) -> String {
    match pat {
        Pattern::Wildcard => "_".to_string(),
        Pattern::Binding(n) => n.clone(),
        Pattern::NullLit => "null".to_string(),
        Pattern::IntLit(n) => n.to_decimal_string(),
        Pattern::U8Lit(b) => format!("u8({b})"),
        Pattern::B64Lit(w) => format!("b64({w})"),
        Pattern::BoolLit(b) => if *b { "true" } else { "false" }.to_string(),
        Pattern::StrLit(s) => fmt_escaped_str(s),
        Pattern::BytesLit(bs) => fmt_escaped_bytes(bs),
        Pattern::TritLit(t) => format!("t1({})", t.value()),
        Pattern::T9Lit(t) => format!("t9({})", t.value()),
        Pattern::T32Lit(t) => format!("t32({})", t.value()),
        Pattern::Q4Lit(q) => format!("q4({})", q.value()),
        Pattern::RatioLit(r) => format!("{}/{}r", r.num(), r.den()),
        Pattern::Addr8Lit(a) => a.render(),
        Pattern::SupportLit(sf) => sf.render(),
        Pattern::EnumVariant { path, bindings } => {
            if bindings.is_empty() {
                path.join("::")
            } else {
                let bs: Vec<String> = bindings.iter().map(fmt_pattern).collect();
                format!("{}({})", path.join("::"), bs.join(", "))
            }
        }
        Pattern::Tuple(ps) => {
            let bs: Vec<String> = ps.iter().map(fmt_pattern).collect();
            format!("({})", bs.join(", "))
        }
    }
}

fn fmt_atom(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Unary { .. }
        | ExprKind::Binary { .. }
        | ExprKind::Ternary { .. }
        | ExprKind::Range { .. } => format!("({})", fmt_expr(e)),
        _ => fmt_expr(e),
    }
}

fn fmt_expr(e: &Expr) -> String {
    match &e.kind {
        ExprKind::NullLit => "null".to_string(),
        ExprKind::IntLit(n) => n.to_decimal_string(),
        ExprKind::BoolLit(b) => if *b { "true" } else { "false" }.to_string(),
        ExprKind::StrLit(s) => fmt_escaped_str(s),
        ExprKind::BytesLit(bs) => fmt_escaped_bytes(bs),
        ExprKind::RatioLit(r) => format!("{}/{}r", r.num(), r.den()),
        ExprKind::Addr8Lit(a) => a.render(),
        ExprKind::SupportLit(sf) => sf.render(),
        ExprKind::TritLit(tl) => match tl {
            TritLiteral::One => "1t".to_string(),
            TritLiteral::Zero => "0t".to_string(),
            TritLiteral::Hinge0H => "0_H".to_string(),
            TritLiteral::Dwell0D => "0_D".to_string(),
        },
        ExprKind::WindowLit { start, end } => {
            format!("({}, {}]", fmt_expr(start), fmt_expr(end))
        }
        ExprKind::Range { start, end } => format!("{}..{}", fmt_atom(start), fmt_atom(end)),
        ExprKind::BoxExpr(dims) => {
            let ds: Vec<String> = dims.iter().map(fmt_expr).collect();
            format!("box([{}])", ds.join(", "))
        }
        ExprKind::Simulate {
            prepare,
            ticks,
            observers,
        } => {
            if observers.is_empty() {
                format!("simulate {prepare} for {} ticks", fmt_expr(ticks))
            } else {
                format!(
                    "simulate {prepare} for {} ticks with {}",
                    fmt_expr(ticks),
                    observers.join(", ")
                )
            }
        }
        ExprKind::Ident(n) => n.clone(),
        ExprKind::Path(segs) => segs.join("::"),
        ExprKind::List(items) | ExprKind::ArrayLit(items) => {
            let xs: Vec<String> = items.iter().map(fmt_expr).collect();
            format!("[{}]", xs.join(", "))
        }
        ExprKind::Tuple(items) => {
            let xs: Vec<String> = items.iter().map(fmt_expr).collect();
            if xs.len() == 1 {
                format!("({},)", xs[0])
            } else {
                format!("({})", xs.join(", "))
            }
        }
        ExprKind::MapLit(entries) => {
            let es: Vec<String> = entries
                .iter()
                .map(|(k, v)| format!("({}, {})", fmt_expr(k), fmt_expr(v)))
                .collect();
            format!("map[{}]", es.join(", "))
        }
        ExprKind::StructLit { path, fields } => {
            let fs: Vec<String> = fields
                .iter()
                .map(|(n, v)| format!("{n}: {}", fmt_expr(v)))
                .collect();
            format!("{} {{ {} }}", path.join("::"), fs.join(", "))
        }
        ExprKind::Unary { op, operand } => match op {
            UnaryOp::Neg => format!("-{}", fmt_atom(operand)),
            UnaryOp::Pos => format!("+{}", fmt_atom(operand)),
            UnaryOp::Not => format!("not {}", fmt_atom(operand)),
            UnaryOp::Boundary => format!("∂{}", fmt_atom(operand)),
        },
        ExprKind::Binary { op, left, right } => {
            let op_s = match op {
                BinaryOp::Add => "+",
                BinaryOp::Sub => "-",
                BinaryOp::Mul => "*",
                BinaryOp::Div => "/",
                BinaryOp::FloorDiv => "//",
                BinaryOp::Mod => "%",
                BinaryOp::BitAnd => "&",
                BinaryOp::BitOr => "|",
                BinaryOp::BitXor => "^",
                BinaryOp::Shl => "<<",
                BinaryOp::Shr => ">>",
                BinaryOp::Eq => "==",
                BinaryOp::NotEq => "!=",
                BinaryOp::Lt => "<",
                BinaryOp::LtEq => "<=",
                BinaryOp::Gt => ">",
                BinaryOp::GtEq => ">=",
                BinaryOp::And => "and",
                BinaryOp::Or => "or",
            };
            format!("({} {op_s} {})", fmt_expr(left), fmt_expr(right))
        }
        ExprKind::Ternary { cond, yes, no } => {
            format!("({} if {} else {})", fmt_expr(yes), fmt_expr(cond), fmt_expr(no))
        }
        ExprKind::Field { base, name } => format!("{}.{name}", fmt_atom(base)),
        ExprKind::Index { base, index } => format!("{}[{}]", fmt_atom(base), fmt_expr(index)),
        ExprKind::Call { callee, args } => {
            let as_: Vec<String> = args.iter().map(fmt_call_arg).collect();
            format!("{}({})", fmt_atom(callee), as_.join(", "))
        }
    }
}

fn fmt_call_arg(a: &CallArg) -> String {
    match &a.name {
        Some(n) => format!("{n}: {}", fmt_expr(&a.value)),
        None => fmt_expr(&a.value),
    }
}

fn fmt_escaped_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn fmt_escaped_bytes(bs: &[u8]) -> String {
    let mut out = String::with_capacity(bs.len() + 3);
    out.push_str("b\"");
    for &b in bs {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7E => out.push(b as char),
            _ => {
                let _ = write!(out, "\\x{b:02x}");
            }
        }
    }
    out.push('"');
    out
}
