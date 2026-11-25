#![forbid(unsafe_code)]
//! Canonical Clean-Break Pure-Wave `.dec` Source Formatter (`aodc fmt`).
//!
//! Produces deterministic, idempotent, round-trip parse-verified `.dec` output.

use std::fmt::Write as _;
use std::path::Path;

use crate::ast::{
    AdverbOp, BinaryOp, ChamberDecl, ConstDecl, DecArm, DecArmParam, DecFnDecl, EnumDecl, Expr,
    ExprKind, Item, LetPattern, ObserveDecl, Pattern, PipeStage, PrepareDecl, ProbeDecl,
    ProbeKind, Program, Stmt, StmtKind, StructDecl, TritLiteral, TypeExpr, UnaryOp, UseDecl,
    WallPolicyAst,
};
use crate::diagnostics::{DecResult, Diagnostic, DiagnosticCode};
use crate::parser::parse_source;

pub fn format_source(source: &str) -> DecResult<String> {
    let prog = parse_source(source)?;
    let formatted = format_program(&prog);
    // Verify round-trip parse validity before returning.
    parse_source(&formatted)?;
    Ok(formatted)
}

pub fn format_file(path: &Path) -> DecResult<String> {
    let raw = std::fs::read_to_string(path).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::ParseError,
            format!("failed reading {}: {e}", path.display()),
        )
    })?;
    format_source(&raw)
}

pub fn format_program(prog: &Program) -> String {
    let mut out = String::new();
    for (idx, item) in prog.items.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        fmt_item(&mut out, item, 0);
    }
    out
}

fn push_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push_str("    ");
    }
}

fn indent_str(indent: usize) -> String {
    "    ".repeat(indent)
}

fn fmt_item(out: &mut String, item: &Item, indent: usize) {
    match item {
        Item::Use(u) => fmt_use(out, u, indent),
        Item::Struct(s) => fmt_struct(out, s, indent),
        Item::Enum(e) => fmt_enum(out, e, indent),
        Item::Const(c) => fmt_const(out, c, indent),
        Item::DecFn(f) => fmt_dec_fn(out, f, indent),
        Item::Chamber(c) => fmt_chamber(out, c, indent),
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
                    fmt_item(out, sub, indent + 1);
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
                fmt_expr_at(&c.value, indent)
            );
        }
        None => {
            let _ = writeln!(out, "const {} = {}", c.name, fmt_expr_at(&c.value, indent));
        }
    }
}

fn fmt_dec_fn(out: &mut String, f: &DecFnDecl, indent: usize) {
    if f.arms.len() == 1
        && !f.arms[0].has_arrow
        && f.arms[0].guard.is_none()
        && f.arms[0]
            .params
            .iter()
            .all(|p| matches!(p, DecArmParam::TypedVar { .. }))
    {
        let arm = &f.arms[0];
        let ps: Vec<String> = arm.params.iter().map(fmt_arm_param).collect();
        push_indent(out, indent);
        let _ = writeln!(out, "dec {}({}):", f.name, ps.join(", "));
        fmt_stmts(out, &arm.body, indent + 1);
        return;
    }

    push_indent(out, indent);
    let _ = writeln!(out, "dec {}:", f.name);
    for arm in &f.arms {
        fmt_dec_arm(out, arm, indent + 1);
    }
}

fn fmt_dec_arm(out: &mut String, arm: &DecArm, indent: usize) {
    push_indent(out, indent);
    let ps: Vec<String> = arm.params.iter().map(fmt_arm_param).collect();
    let _ = write!(out, "({})", ps.join(", "));
    if let Some(ref g) = arm.guard {
        let _ = write!(out, " where {}", fmt_expr_at(g, indent));
    }
    let _ = writeln!(out, " -> {}:", fmt_type(&arm.return_type));
    fmt_stmts(out, &arm.body, indent + 1);
}

fn fmt_arm_param(p: &DecArmParam) -> String {
    match p {
        DecArmParam::TypedVar { name, ty } => format!("{name}: {}", fmt_type(ty)),
        DecArmParam::TypedPattern { pattern, ty } => {
            format!("{}: {}", fmt_pattern(pattern), fmt_type(ty))
        }
        DecArmParam::Pattern(pat) => fmt_pattern(pat),
    }
}

fn fmt_chamber(out: &mut String, c: &ChamberDecl, indent: usize) {
    push_indent(out, indent);
    let _ = writeln!(out, "chamber {}:", c.name);
    push_indent(out, indent + 1);
    let _ = writeln!(out, "space = {}", fmt_expr_at(&c.space, indent + 1));
    push_indent(out, indent + 1);
    let w = match c.walls {
        WallPolicyAst::Open => "open",
        WallPolicyAst::Closed => "closed",
    };
    let _ = writeln!(out, "walls = {w}");
    if let Some(ref eng) = c.engine {
        push_indent(out, indent + 1);
        let _ = writeln!(out, "engine = {eng}");
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
        let _ = writeln!(
            out,
            "clock {} = {}",
            clk.name,
            fmt_expr_at(&clk.period, indent + 1)
        );
    }
    for inj in &p.injections {
        push_indent(out, indent + 1);
        let _ = writeln!(
            out,
            "inject {} at {}, dir {}, orient {}, shells {}",
            fmt_expr_at(&inj.support, indent + 1),
            fmt_expr_at(&inj.pos, indent + 1),
            inj.dir.as_str(),
            fmt_expr_at(&inj.orient, indent + 1),
            fmt_expr_at(&inj.shells, indent + 1)
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
                let _ = writeln!(out, "region = {}", fmt_expr_at(region, indent + 2));
                if !capture.is_empty() {
                    push_indent(out, indent + 2);
                    let _ = writeln!(out, "capture = [{}]", capture.join(", "));
                }
            }
            ProbeDecl::Procedural {
                kind,
                name,
                param_name,
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
                let _ = writeln!(
                    out,
                    "({param_name}: ReadOnly<ChamberView>) -> {}:",
                    fmt_type(return_type)
                );
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
        StmtKind::Let { pattern, ty, value } => {
            push_indent(out, indent);
            let lhs = match pattern {
                LetPattern::Single(name) => name.clone(),
                LetPattern::Tuple(elems) => {
                    let ps: Vec<String> = elems
                        .iter()
                        .map(|(n, te_opt)| match te_opt {
                            Some(te) => format!("{n}: {}", fmt_type(te)),
                            None => n.clone(),
                        })
                        .collect();
                    format!("({})", ps.join(", "))
                }
            };
            match ty {
                Some(t) => {
                    let _ = writeln!(
                        out,
                        "let {lhs}: {} = {}",
                        fmt_type(t),
                        fmt_expr_at(value, indent)
                    );
                }
                None => {
                    let _ = writeln!(out, "let {lhs} = {}", fmt_expr_at(value, indent));
                }
            }
        }
        StmtKind::Assert(e) => {
            push_indent(out, indent);
            let _ = writeln!(out, "assert {}", fmt_expr_at(e, indent));
        }
        StmtKind::Emit(e) => {
            push_indent(out, indent);
            let _ = writeln!(out, "emit {}", fmt_expr_at(e, indent));
        }
        StmtKind::Return(Some(e)) => {
            push_indent(out, indent);
            let _ = writeln!(out, "return {}", fmt_expr_at(e, indent));
        }
        StmtKind::Return(None) => {
            push_indent(out, indent);
            out.push_str("return\n");
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
            let _ = writeln!(out, "{}", fmt_expr_at(e, indent));
        }
    }
}

fn fmt_type(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Unit => "()".to_string(),
        TypeExpr::Named(n) => n.clone(),
        TypeExpr::List(inner) => format!("[{}]", fmt_type(inner)),
        TypeExpr::Array(inner, n) => format!("[{}; {n}]", fmt_type(inner)),
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
        Pattern::BoolLit(b) => if *b { "true" } else { "false" }.to_string(),
        Pattern::BytesLit(bs) => fmt_escaped_bytes(bs),
        Pattern::B8Lit(b) => format!("b8({})", b.value()),
        Pattern::B64Lit(w) => format!("b64({})", w.value()),
        Pattern::Q4Lit(q) => format!("q4({})", q.value()),
        Pattern::TritLit(t) => format!("{}t", t.value()),
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

fn fmt_adverb(op: AdverbOp) -> &'static str {
    match op {
        AdverbOp::FoldAdd => "+/",
        AdverbOp::FoldMul => "*/",
        AdverbOp::FoldMin => "min/",
        AdverbOp::FoldMax => "max/",
        AdverbOp::FoldAnd => "&/",
        AdverbOp::FoldOr => "|/",
        AdverbOp::FoldSuperpose => "superpose/",
        AdverbOp::ScanAdd => "+\\",
        AdverbOp::ScanMul => "*\\",
        AdverbOp::ScanMin => "min\\",
        AdverbOp::ScanMax => "max\\",
    }
}

fn fmt_binop(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::FloorDiv => "//",
        BinaryOp::Mod => "%",
        BinaryOp::Min => "min",
        BinaryOp::Max => "max",
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
        BinaryOp::Cmp3 => "<=>",
        BinaryOp::And => "and",
        BinaryOp::Or => "or",
        BinaryOp::Sieve => "?>",
        BinaryOp::OuterAdd => "+:",
        BinaryOp::OuterMul => "*:",
    }
}

fn fmt_atom(e: &Expr, indent: usize) -> String {
    match &e.kind {
        ExprKind::Unary { .. }
        | ExprKind::Adverb { .. }
        | ExprKind::Binary { .. }
        | ExprKind::Pipe { .. }
        | ExprKind::Range { .. }
        | ExprKind::Simulate { .. } => format!("({})", fmt_expr_at(e, indent)),
        _ => fmt_expr_at(e, indent),
    }
}

fn fmt_expr_at(e: &Expr, indent: usize) -> String {
    match &e.kind {
        ExprKind::NullLit => "null".to_string(),
        ExprKind::IntLit(n) => n.to_decimal_string(),
        ExprKind::BoolLit(b) => if *b { "true" } else { "false" }.to_string(),
        ExprKind::BytesLit(bs) => fmt_escaped_bytes(bs),
        ExprKind::RatioLit(r) => format!("{}/{}r", r.num(), r.den()),
        ExprKind::Addr8Lit(a) => a.render(),
        ExprKind::SupportLit(sf) => sf.render(),
        ExprKind::TritLit(tl) => match tl {
            TritLiteral::NegOne => "-1t".to_string(),
            TritLiteral::Zero => "0t".to_string(),
            TritLiteral::PosOne => "1t".to_string(),
            TritLiteral::Hinge0H => "0_H".to_string(),
            TritLiteral::Dwell0D => "0_D".to_string(),
        },
        ExprKind::WindowLit { start, end } => {
            format!(
                "({}, {}]",
                fmt_expr_at(start, indent),
                fmt_expr_at(end, indent)
            )
        }
        ExprKind::Range { start, end } => {
            format!("{}..{}", fmt_atom(start, indent), fmt_atom(end, indent))
        }
        ExprKind::BoxExpr(dims) => {
            let ds: Vec<String> = dims.iter().map(|d| fmt_expr_at(d, indent)).collect();
            format!("box([{}])", ds.join(", "))
        }
        ExprKind::Simulate {
            prepare,
            ticks,
            observers,
        } => {
            if observers.is_empty() {
                format!(
                    "simulate {prepare} for {} ticks",
                    fmt_expr_at(ticks, indent)
                )
            } else {
                format!(
                    "simulate {prepare} for {} ticks with {}",
                    fmt_expr_at(ticks, indent),
                    observers.join(", ")
                )
            }
        }
        ExprKind::Ident(n) => n.clone(),
        ExprKind::Path(segs) => segs.join("::"),
        ExprKind::SeqLit(items) => {
            let xs: Vec<String> = items.iter().map(|it| fmt_expr_at(it, indent)).collect();
            format!("[{}]", xs.join(", "))
        }
        ExprKind::Tuple(items) => {
            let xs: Vec<String> = items.iter().map(|it| fmt_expr_at(it, indent)).collect();
            if xs.len() == 1 {
                format!("({},)", xs[0])
            } else {
                format!("({})", xs.join(", "))
            }
        }
        ExprKind::MapLit(entries) => {
            let es: Vec<String> = entries
                .iter()
                .map(|(k, v)| format!("({}, {})", fmt_expr_at(k, indent), fmt_expr_at(v, indent)))
                .collect();
            format!("map[{}]", es.join(", "))
        }
        ExprKind::Unary { op, operand } => match op {
            UnaryOp::Neg => format!("-{}", fmt_atom(operand, indent)),
            UnaryOp::Pos => format!("+{}", fmt_atom(operand, indent)),
            UnaryOp::Not => format!("not {}", fmt_atom(operand, indent)),
            UnaryOp::BitNot => format!("~{}", fmt_atom(operand, indent)),
            UnaryOp::Boundary => format!("∂{}", fmt_atom(operand, indent)),
        },
        ExprKind::Adverb { op, operand } => {
            format!("{} {}", fmt_adverb(*op), fmt_atom(operand, indent))
        }
        ExprKind::Binary { op, left, right } => format!(
            "({} {} {})",
            fmt_expr_at(left, indent),
            fmt_binop(*op),
            fmt_expr_at(right, indent)
        ),
        ExprKind::Pipe { input, stage } => {
            let lhs_s = match &input.kind {
                ExprKind::Pipe { .. } => fmt_expr_at(input, indent),
                _ => fmt_atom(input, indent),
            };
            let st_s = match stage.as_ref() {
                PipeStage::Adverb(op) => fmt_adverb(*op).to_string(),
                PipeStage::Boundary => "∂".to_string(),
                PipeStage::Section { op, rhs } => {
                    format!("({} {})", fmt_binop(*op), fmt_expr_at(rhs, indent))
                }
                PipeStage::Call { callee, extra_args } => {
                    if extra_args.is_empty() {
                        fmt_expr_at(callee, indent)
                    } else {
                        let as_: Vec<String> =
                            extra_args.iter().map(|a| fmt_expr_at(a, indent)).collect();
                        format!("{}({})", fmt_expr_at(callee, indent), as_.join(", "))
                    }
                }
            };
            format!("({lhs_s} |> {st_s})")
        }
        ExprKind::Match { scrutinee, arms } => {
            let mut s = format!("match {}:", fmt_expr_at(scrutinee, indent));
            let arm_pad = indent_str(indent + 1);
            for arm in arms {
                s.push('\n');
                s.push_str(&arm_pad);
                s.push_str(&fmt_pattern(&arm.pattern));
                if let Some(ref g) = arm.guard {
                    let _ = write!(s, " where {}", fmt_expr_at(g, indent + 1));
                }
                let _ = write!(s, " -> {}", fmt_expr_at(&arm.body, indent + 1));
            }
            s
        }
        ExprKind::Field { base, name } => format!("{}.{name}", fmt_atom(base, indent)),
        ExprKind::Index { base, index } => {
            format!("{}[{}]", fmt_atom(base, indent), fmt_expr_at(index, indent))
        }
        ExprKind::Call { callee, args } => {
            let as_: Vec<String> = args.iter().map(|a| fmt_expr_at(a, indent)).collect();
            format!("{}({})", fmt_atom(callee, indent), as_.join(", "))
        }
    }
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
            0 => out.push_str("\\0"),
            0x20..=0x7E => out.push(b as char),
            _ => {
                let _ = write!(out, "\\x{b:02x}");
            }
        }
    }
    out.push('"');
    out
}
