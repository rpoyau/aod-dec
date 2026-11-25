#![forbid(unsafe_code)]
//! Live-Walk Scanner & 6-Hook Native `.dec` Auditor (`aod.dec.audit.v1`).
//!
//! Pure `aod-core` + `aodc` AST/typecheck pass (`#![forbid(unsafe_code)]`, zero floats,
//! zero `aod_llvm` dependency, zero host paths/timestamps in portable JSON reports).

use std::collections::BTreeMap;
use std::path::Path;

use aod_core::bigint::BigInt;
use aod_core::json::JsonValue;
use aod_core::ledger::SadarPromotion;
use aod_core::ratio::Ratio;
use aod_core::s4_report::sanitize_error;
use aod_core::sha256::Sha256;
use aod_core::support::SupportForm;
use aod_core::ternary::{Addr8, Q4, T1};

use crate::ast::{
    BinaryOp, DecArmParam, DecFnDecl, Expr, ExprKind, Item, LetPattern, Pattern, PipeStage,
    ProbeDecl, Program, Stmt, StmtKind, TritLiteral, UnaryOp,
};
use crate::checker::check_unit;
use crate::diagnostics::DiagnosticCode;
use crate::modules::{load_from_source_with_options, LoadOptions};

pub const DEC_AUDIT_SCHEMA: &str = "aod.dec.audit.v1";

/// Sheddic route invariant (§5.17.5): positive shedding excess requires an even enclosure route `>= 2`.
#[inline]
pub fn verify_sheddic_route(x_shedding: i64, target_enclosure: u8) -> bool {
    x_shedding <= 0 || (target_enclosure >= 2 && target_enclosure.is_multiple_of(2))
}

/// FRG cross-return field-ring identity (§5.17.9): `t_cross - t_zero == c_cross`.
#[inline]
pub fn verify_frg_identity(t_zero: i64, t_cross: i64, c_cross: i64) -> bool {
    (t_cross as i128) - (t_zero as i128) == (c_cross as i128)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveWalkSummary {
    pub initial_anchor: Addr8,
    pub final_anchor: Addr8,
    pub committed_crossings: u64,
    pub dwell_burned: u64,
}

impl LiveWalkSummary {
    pub fn to_json(&self) -> JsonValue {
        let mut m = BTreeMap::new();
        m.insert(
            "committed_crossings".to_string(),
            JsonValue::from_i64(self.committed_crossings as i64),
        );
        m.insert(
            "dwell_burned".to_string(),
            JsonValue::from_i64(self.dwell_burned as i64),
        );
        m.insert(
            "final_anchor".to_string(),
            JsonValue::from_str_val(self.final_anchor.render()),
        );
        m.insert(
            "initial_anchor".to_string(),
            JsonValue::from_str_val(self.initial_anchor.render()),
        );
        JsonValue::Object(m)
    }
}

fn addr8_from_u64_min2(mut val: u64) -> Addr8 {
    let mut rev = Vec::new();
    while val > 0 {
        rev.push((val & 7) as u8);
        val >>= 3;
    }
    while rev.len() < 2 {
        rev.push(0);
    }
    rev.reverse();
    Addr8::from_digits(rev).expect("valid octal digits 0..7")
}

/// Live-walk byte scanner starting at `00_8` (`[0, 0]`), monotonically burning dwell across
/// consecutive whitespace, structural delimiters, and `# ... \n` comments without resetting,
/// and advancing the retained octal anchor only on non-zero role boundary crossings.
pub fn lex_live_walk(src: &[u8]) -> LiveWalkSummary {
    let initial_anchor = addr8_from_u64_min2(0);
    let mut anchor_val: u64 = 0;
    let mut prev_role = T1::ZERO;
    let mut dwell_burned: u64 = 0;
    let mut committed_crossings: u64 = 0;
    let mut in_comment = false;

    for &b in src {
        let role = if in_comment {
            if b == b'\n' {
                in_comment = false;
            }
            T1::ZERO
        } else if b == b'#' {
            in_comment = true;
            T1::ZERO
        } else {
            match b {
                b' ' | b'\t' | b'\n' | b'\r' | b':' | b'(' | b')' | b'[' | b']' | b'{' | b'}'
                | b',' => T1::ZERO,
                b'-' | b'>' => T1::NEG,
                _ => T1::POS,
            }
        };

        if role == T1::ZERO && prev_role == T1::ZERO {
            dwell_burned += 1;
        } else if role != T1::ZERO && role != prev_role {
            committed_crossings += 1;
            anchor_val += 1;
        }
        prev_role = role;
    }

    LiveWalkSummary {
        initial_anchor,
        final_anchor: addr8_from_u64_min2(anchor_val),
        committed_crossings,
        dwell_burned,
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HookAuditResult {
    pub violations: Vec<String>,
}

impl HookAuditResult {
    pub fn passed(&self) -> bool {
        self.violations.is_empty()
    }

    pub fn record(&mut self, msg: impl Into<String>) {
        self.violations.push(sanitize_error(&msg.into(), &[]));
    }

    pub fn to_json(&self) -> JsonValue {
        let mut m = BTreeMap::new();
        m.insert(
            "status".to_string(),
            JsonValue::from_str_val(if self.passed() { "PASS" } else { "FAIL" }),
        );
        m.insert(
            "violations".to_string(),
            JsonValue::Array(
                self.violations
                    .iter()
                    .cloned()
                    .map(JsonValue::from_str_val)
                    .collect(),
            ),
        );
        JsonValue::Object(m)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecAuditReport {
    pub schema: String,
    pub status: String,
    pub source_name: String,
    pub source_sha256: String,
    pub bytes: usize,
    pub live_walk: LiveWalkSummary,
    pub spine_audit: HookAuditResult,
    pub boundary_audit: HookAuditResult,
    pub sadar_audit: HookAuditResult,
    pub sheddic_path_audit: HookAuditResult,
    pub cycle_audit: HookAuditResult,
    pub frg_audit: HookAuditResult,
    pub errors: Vec<String>,
}

impl DecAuditReport {
    pub fn passed(&self) -> bool {
        self.status == "PASS"
    }

    pub fn hooks_passed(&self) -> usize {
        [
            &self.spine_audit,
            &self.boundary_audit,
            &self.sadar_audit,
            &self.sheddic_path_audit,
            &self.cycle_audit,
            &self.frg_audit,
        ]
        .into_iter()
        .filter(|h| h.passed())
        .count()
    }

    pub fn to_json(&self) -> JsonValue {
        let mut hooks = BTreeMap::new();
        hooks.insert("boundary_audit".to_string(), self.boundary_audit.to_json());
        hooks.insert("cycle_audit".to_string(), self.cycle_audit.to_json());
        hooks.insert("frg_audit".to_string(), self.frg_audit.to_json());
        hooks.insert("sadar_audit".to_string(), self.sadar_audit.to_json());
        hooks.insert(
            "sheddic_path_audit".to_string(),
            self.sheddic_path_audit.to_json(),
        );
        hooks.insert("spine_audit".to_string(), self.spine_audit.to_json());

        let mut root = BTreeMap::new();
        root.insert("bytes".to_string(), JsonValue::from_i64(self.bytes as i64));
        root.insert(
            "errors".to_string(),
            JsonValue::Array(
                self.errors
                    .iter()
                    .cloned()
                    .map(JsonValue::from_str_val)
                    .collect(),
            ),
        );
        root.insert("hooks".to_string(), JsonValue::Object(hooks));
        root.insert(
            "hooks_passed".to_string(),
            JsonValue::from_i64(self.hooks_passed() as i64),
        );
        root.insert("hooks_total".to_string(), JsonValue::from_i64(6));
        root.insert("live_walk".to_string(), self.live_walk.to_json());
        root.insert(
            "schema".to_string(),
            JsonValue::from_str_val(self.schema.clone()),
        );
        root.insert(
            "source_name".to_string(),
            JsonValue::from_str_val(self.source_name.clone()),
        );
        root.insert(
            "source_sha256".to_string(),
            JsonValue::from_str_val(self.source_sha256.clone()),
        );
        root.insert(
            "status".to_string(),
            JsonValue::from_str_val(self.status.clone()),
        );
        JsonValue::Object(root)
    }
}

/// Construct a portable `FAIL` report when reading `<source.dec>` fails (e.g. symlink / missing).
pub fn failed_read_audit_report(source_name: &str, err_msg: &str) -> DecAuditReport {
    let sanitized = sanitize_error(err_msg, &[]);
    let mut spine = HookAuditResult::default();
    spine.record(&sanitized);
    DecAuditReport {
        schema: DEC_AUDIT_SCHEMA.to_string(),
        status: "FAIL".to_string(),
        source_name: sanitize_source_name(source_name),
        source_sha256: Sha256::digest_hex(b""),
        bytes: 0,
        live_walk: lex_live_walk(b""),
        spine_audit: spine,
        boundary_audit: HookAuditResult::default(),
        sadar_audit: HookAuditResult::default(),
        sheddic_path_audit: HookAuditResult::default(),
        cycle_audit: HookAuditResult::default(),
        frg_audit: HookAuditResult::default(),
        errors: vec![sanitized],
    }
}

/// Run the live-walk scanner and all 6 §5.17 audit hooks (`spine_audit`, `boundary_audit`,
/// `sadar_audit`, `sheddic_path_audit`, `cycle_audit`, `frg_audit`) on in-memory `source`.
pub fn audit_dec_source_with_options(
    source: &str,
    source_name: &str,
    base_dir: Option<&Path>,
    opts: &LoadOptions,
) -> DecAuditReport {
    let raw_bytes = source.as_bytes();
    let live_walk = lex_live_walk(raw_bytes);
    let source_sha256 = Sha256::digest_hex(raw_bytes);

    let mut acc = AuditState::default();

    match load_from_source_with_options(source, base_dir, opts) {
        Err(diag) => {
            let msg = sanitize_error(&diag.to_string(), &[]);
            acc.errors.push(msg.clone());
            match diag.code {
                DiagnosticCode::OddSupportRequiresYro
                | DiagnosticCode::EvenSupportForbidsYro
                | DiagnosticCode::InvalidSupportLiteral => {
                    acc.sheddic_path_audit.record(msg);
                }
                _ => {
                    acc.spine_audit.record(msg);
                }
            }
        }
        Ok(loaded) => {
            // Inspect entry & bundled modules across all 6 §5.17 hooks
            audit_program(&loaded.entry_program, true, &mut acc);
            for prog in loaded.parsed_modules.values() {
                audit_program(prog, false, &mut acc);
            }

            // Full compile-time type & firewall check (`check_unit`)
            if let Err(diag) = check_unit(loaded, false) {
                let msg = sanitize_error(&diag.to_string(), &[]);
                acc.errors.push(msg.clone());
                match diag.code {
                    DiagnosticCode::OddSupportRequiresYro
                    | DiagnosticCode::EvenSupportForbidsYro
                    | DiagnosticCode::InvalidSupportLiteral => {
                        acc.sheddic_path_audit.record(msg);
                    }
                    _ => {
                        if acc.all_hooks_currently_clean() {
                            acc.spine_audit.record(msg);
                        }
                    }
                }
            }
        }
    }

    let all_pass = acc.all_hooks_currently_clean() && acc.errors.is_empty();
    DecAuditReport {
        schema: DEC_AUDIT_SCHEMA.to_string(),
        status: if all_pass {
            "PASS".to_string()
        } else {
            "FAIL".to_string()
        },
        source_name: sanitize_source_name(source_name),
        source_sha256,
        bytes: raw_bytes.len(),
        live_walk,
        spine_audit: acc.spine_audit,
        boundary_audit: acc.boundary_audit,
        sadar_audit: acc.sadar_audit,
        sheddic_path_audit: acc.sheddic_path_audit,
        cycle_audit: acc.cycle_audit,
        frg_audit: acc.frg_audit,
        errors: acc.errors,
    }
}

fn sanitize_source_name(name: &str) -> String {
    Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("<memory>")
        .to_string()
}

#[derive(Default)]
struct AuditState {
    spine_audit: HookAuditResult,
    boundary_audit: HookAuditResult,
    sadar_audit: HookAuditResult,
    sheddic_path_audit: HookAuditResult,
    cycle_audit: HookAuditResult,
    frg_audit: HookAuditResult,
    errors: Vec<String>,
    struct_fields: BTreeMap<String, Vec<String>>,
}

impl AuditState {
    fn all_hooks_currently_clean(&self) -> bool {
        self.spine_audit.passed()
            && self.boundary_audit.passed()
            && self.sadar_audit.passed()
            && self.sheddic_path_audit.passed()
            && self.cycle_audit.passed()
            && self.frg_audit.passed()
    }
}

#[derive(Clone, Debug)]
enum FoldVal {
    Int(BigInt),
    B8(u8),
    B64(u64),
    T1(T1),
    Q4(Q4),
    Bool(bool),
    Bytes(Vec<u8>),
    Ratio(Ratio),
    Support(SupportForm),
    Range(BigInt, BigInt),
    Window(BigInt, BigInt),
    Tuple(Vec<FoldVal>),
    Seq(Vec<FoldVal>),
    UnclosedTrace,
}

type ConstEnv = BTreeMap<String, FoldVal>;

fn callee_leaf_name(callee: &Expr) -> Option<&str> {
    match &callee.kind {
        ExprKind::Ident(s) => Some(s.as_str()),
        ExprKind::Path(segs) => segs.last().map(String::as_str),
        _ => None,
    }
}

fn is_unclosed_trace_expr(expr: &Expr, env: &ConstEnv) -> bool {
    match &expr.kind {
        ExprKind::Ident(name) => matches!(env.get(name), Some(FoldVal::UnclosedTrace)),
        ExprKind::Call { callee, .. } => matches!(
            callee_leaf_name(callee),
            Some("trace" | "step" | "hinge" | "dwell" | "outer" | "fuse")
        ),
        ExprKind::Pipe { stage, .. } => match stage.as_ref() {
            PipeStage::Call { callee, .. } => matches!(
                callee_leaf_name(callee),
                Some("trace" | "step" | "hinge" | "dwell" | "outer" | "fuse")
            ),
            _ => false,
        },
        _ => false,
    }
}

fn fold_expr(expr: &Expr, env: &ConstEnv) -> Option<FoldVal> {
    if is_unclosed_trace_expr(expr, env) {
        return Some(FoldVal::UnclosedTrace);
    }
    match &expr.kind {
        ExprKind::IntLit(n) => Some(FoldVal::Int(n.clone())),
        ExprKind::BoolLit(b) => Some(FoldVal::Bool(*b)),
        ExprKind::BytesLit(bs) => Some(FoldVal::Bytes(bs.clone())),
        ExprKind::RatioLit(r) => Some(FoldVal::Ratio(r.clone())),
        ExprKind::SupportLit(sf) => Some(FoldVal::Support(sf.clone())),
        ExprKind::TritLit(tl) => match tl {
            TritLiteral::NegOne => Some(FoldVal::T1(T1::NEG)),
            TritLiteral::Zero | TritLiteral::Hinge0H | TritLiteral::Dwell0D => {
                Some(FoldVal::T1(T1::ZERO))
            }
            TritLiteral::PosOne => Some(FoldVal::T1(T1::POS)),
        },
        ExprKind::Ident(name) => env.get(name).cloned(),
        ExprKind::Range { start, end } => match (fold_expr(start, env)?, fold_expr(end, env)?) {
            (FoldVal::Int(s), FoldVal::Int(e)) => Some(FoldVal::Range(s, e)),
            _ => None,
        },
        ExprKind::WindowLit { start, end } => match (fold_expr(start, env)?, fold_expr(end, env)?) {
            (FoldVal::Int(s), FoldVal::Int(e)) => Some(FoldVal::Window(s, e)),
            _ => None,
        },
        ExprKind::Tuple(items) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                out.push(fold_expr(it, env)?);
            }
            Some(FoldVal::Tuple(out))
        }
        ExprKind::SeqLit(items) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                out.push(fold_expr(it, env)?);
            }
            Some(FoldVal::Seq(out))
        }
        ExprKind::Field { base, name } => match (fold_expr(base, env)?, name.as_str()) {
            (FoldVal::Ratio(r), "num") => Some(FoldVal::Int(r.num().clone())),
            (FoldVal::Ratio(r), "den") => Some(FoldVal::Int(r.den().clone())),
            _ => None,
        },
        ExprKind::Unary { op, operand } => {
            let v = fold_expr(operand, env)?;
            match (op, v) {
                (UnaryOp::Pos, x) => Some(x),
                (UnaryOp::Neg, FoldVal::Int(n)) => Some(FoldVal::Int(-n)),
                (UnaryOp::Neg, FoldVal::T1(t)) => Some(FoldVal::T1(t.checked_neg())),
                (UnaryOp::Neg, FoldVal::Ratio(r)) => Some(FoldVal::Ratio(-r)),
                (UnaryOp::Not, FoldVal::Bool(b)) => Some(FoldVal::Bool(!b)),
                _ => None,
            }
        }
        ExprKind::Binary { op, left, right } => {
            let lv = fold_expr(left, env)?;
            let rv = fold_expr(right, env)?;
            match (op, lv, rv) {
                (BinaryOp::Add, FoldVal::Int(a), FoldVal::Int(b)) => Some(FoldVal::Int(&a + &b)),
                (BinaryOp::Sub, FoldVal::Int(a), FoldVal::Int(b)) => Some(FoldVal::Int(&a - &b)),
                (BinaryOp::Mul, FoldVal::Int(a), FoldVal::Int(b)) => Some(FoldVal::Int(&a * &b)),
                (BinaryOp::Div, FoldVal::Int(a), FoldVal::Int(b)) => {
                    Ratio::new(a, b).ok().map(FoldVal::Ratio)
                }
                (BinaryOp::Add, FoldVal::Ratio(a), FoldVal::Ratio(b)) => {
                    Some(FoldVal::Ratio(&a + &b))
                }
                (BinaryOp::Sub, FoldVal::Ratio(a), FoldVal::Ratio(b)) => {
                    Some(FoldVal::Ratio(&a - &b))
                }
                (BinaryOp::Mul, FoldVal::Ratio(a), FoldVal::Ratio(b)) => {
                    Some(FoldVal::Ratio(&a * &b))
                }
                (BinaryOp::Div, FoldVal::Ratio(a), FoldVal::Ratio(b)) => {
                    a.checked_div(&b).ok().map(FoldVal::Ratio)
                }
                _ => None,
            }
        }
        ExprKind::Call { callee, args } => {
            let leaf = callee_leaf_name(callee)?;
            let mut fargs = Vec::with_capacity(args.len());
            for a in args {
                fargs.push(fold_expr(a, env)?);
            }
            fold_call_leaf(leaf, &fargs)
        }
        _ => None,
    }
}

fn fold_call_leaf(leaf: &str, fargs: &[FoldVal]) -> Option<FoldVal> {
    match (leaf, fargs) {
        ("b8", [FoldVal::Int(n)]) => n.to_u64().and_then(|v| u8::try_from(v).ok()).map(FoldVal::B8),
        ("b8", [FoldVal::B8(b)]) => Some(FoldVal::B8(*b)),
        ("b8", [FoldVal::Q4(q)]) => Some(FoldVal::B8(q.value())),
        ("b8", [FoldVal::Bool(b)]) => Some(FoldVal::B8(u8::from(*b))),
        ("int", [FoldVal::Int(n)]) => Some(FoldVal::Int(n.clone())),
        ("int", [FoldVal::B8(b)]) => Some(FoldVal::Int(BigInt::from_u64(*b as u64))),
        ("int", [FoldVal::B64(w)]) => Some(FoldVal::Int(BigInt::from_u64(*w))),
        ("int", [FoldVal::T1(t)]) => Some(FoldVal::Int(BigInt::from_i64(t.value() as i64))),
        ("int", [FoldVal::Q4(q)]) => Some(FoldVal::Int(BigInt::from_u64(q.value() as u64))),
        ("b64" | "b64_bits", [FoldVal::Int(n)]) => n.to_u64().map(FoldVal::B64),
        ("b64", [FoldVal::B64(w)]) => Some(FoldVal::B64(*w)),
        ("t1", [FoldVal::Int(n)]) => T1::from_bigint(n).ok().map(FoldVal::T1),
        ("t1", [FoldVal::T1(t)]) => Some(FoldVal::T1(*t)),
        ("q4", [FoldVal::Int(n)]) => Q4::from_bigint(n).ok().map(FoldVal::Q4),
        ("q4", [FoldVal::Q4(q)]) => Some(FoldVal::Q4(*q)),
        ("abs", [FoldVal::Int(n)]) => Some(FoldVal::Int(n.abs())),
        ("abs", [FoldVal::T1(t)]) => Some(FoldVal::T1(if t.value() < 0 { T1::POS } else { *t })),
        ("abs", [FoldVal::Ratio(r)]) => Some(FoldVal::Ratio(r.abs())),
        ("min", [FoldVal::Int(a), FoldVal::Int(b)]) => Some(FoldVal::Int(a.min(b).clone())),
        ("max", [FoldVal::Int(a), FoldVal::Int(b)]) => Some(FoldVal::Int(a.max(b).clone())),
        ("min", [FoldVal::Ratio(a), FoldVal::Ratio(b)]) => Some(FoldVal::Ratio(a.min(b).clone())),
        ("max", [FoldVal::Ratio(a), FoldVal::Ratio(b)]) => Some(FoldVal::Ratio(a.max(b).clone())),
        ("gcd", [FoldVal::Int(a), FoldVal::Int(b)]) => Some(FoldVal::Int(a.gcd(b))),
        ("lcm", [FoldVal::Int(a), FoldVal::Int(b)]) => Some(FoldVal::Int(a.lcm(b))),
        ("pow", [FoldVal::Int(a), FoldVal::Int(b)]) => {
            a.checked_pow_bigint(b).ok().map(FoldVal::Int)
        }
        ("pow", [FoldVal::Ratio(a), FoldVal::Int(b)]) => {
            a.checked_pow_bigint(b).ok().map(FoldVal::Ratio)
        }
        ("num", [FoldVal::Ratio(r)]) => Some(FoldVal::Int(r.num().clone())),
        ("den", [FoldVal::Ratio(r)]) => Some(FoldVal::Int(r.den().clone())),
        ("ratio", [FoldVal::Int(a), FoldVal::Int(b)]) => {
            Ratio::new(a.clone(), b.clone()).ok().map(FoldVal::Ratio)
        }
        ("ratio", [FoldVal::Int(a)]) => Some(FoldVal::Ratio(Ratio::from_int(a.clone()))),
        ("support", [FoldVal::Bytes(bs)]) => std::str::from_utf8(bs)
            .ok()
            .and_then(|s| SupportForm::parse(s).ok())
            .map(FoldVal::Support),
        (
            "support",
            [FoldVal::Int(p), FoldVal::Int(q), FoldVal::Int(l), FoldVal::Bool(y)],
        ) => SupportForm::new(p.clone(), q.clone(), l.clone(), *y)
            .ok()
            .map(FoldVal::Support),
        ("proton_support", []) => SupportForm::parse("3:3:6").ok().map(FoldVal::Support),
        ("electron_support", []) => SupportForm::parse("1:2:6").ok().map(FoldVal::Support),
        ("saddle_337_support", []) => SupportForm::parse("3:3:7_yro").ok().map(FoldVal::Support),
        ("shedding_excess", [FoldVal::Int(pd), FoldVal::Int(cc)]) => {
            SupportForm::shedding_audit(pd, cc)
                .ok()
                .map(|r| FoldVal::Int(r.x_shedding))
        }
        ("array", items) => Some(FoldVal::Seq(items.to_vec())),
        _ => None,
    }
}

fn has_non_pass_stmt(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| !matches!(s.kind, StmtKind::Pass))
}

fn audit_program(prog: &Program, is_entry: bool, acc: &mut AuditState) {
    let mut top_env = ConstEnv::new();
    let mut dec_fn_count = 0usize;

    for item in &prog.items {
        if let Item::Struct(sd) = item {
            acc.struct_fields.insert(
                sd.name.clone(),
                sd.fields.iter().map(|(n, _)| n.clone()).collect(),
            );
        }
    }

    for item in &prog.items {
        match item {
            Item::Const(cd) => {
                audit_expr(&cd.value, false, &top_env, acc);
                if let Some(fv) = fold_expr(&cd.value, &top_env) {
                    top_env.insert(cd.name.clone(), fv);
                }
            }
            Item::DecFn(fd) => {
                dec_fn_count += 1;
                audit_dec_fn(fd, &top_env, acc);
            }
            Item::Chamber(ch) => {
                audit_expr(&ch.space, false, &top_env, acc);
            }
            Item::Prepare(pr) => {
                let mut prep_env = top_env.clone();
                for s in &pr.lets {
                    audit_stmt(s, &mut prep_env, acc);
                }
                for clk in &pr.clocks {
                    audit_expr(&clk.period, false, &prep_env, acc);
                    if let Some(FoldVal::Int(p)) = fold_expr(&clk.period, &prep_env) {
                        if !p.is_positive() {
                            acc.cycle_audit.record(format!(
                                "line {}: prepare {} clock {} requires period >= 1, got {p}",
                                clk.line, pr.name, clk.name
                            ));
                        }
                    }
                }
                for inj in &pr.injections {
                    audit_expr(&inj.support, false, &prep_env, acc);
                    audit_expr(&inj.pos, false, &prep_env, acc);
                    audit_expr(&inj.orient, false, &prep_env, acc);
                    audit_expr(&inj.shells, false, &prep_env, acc);

                    if let Some(FoldVal::Support(sf)) = fold_expr(&inj.support, &prep_env) {
                        if sf.is_yro() {
                            acc.sheddic_path_audit.record(format!(
                                "line {}: prepare {} injects odd _yro support {} without an even sheddic enclosure route",
                                inj.line,
                                pr.name,
                                sf.render()
                            ));
                        }
                    }
                }
            }
            Item::Observe(ob) => {
                for probe in &ob.probes {
                    match probe {
                        ProbeDecl::Declarative { region, .. } => {
                            audit_expr(region, false, &top_env, acc);
                        }
                        ProbeDecl::Procedural {
                            param_name, body, ..
                        } => {
                            let mut p_env = top_env.clone();
                            p_env.remove(param_name);
                            for s in body {
                                audit_stmt(s, &mut p_env, acc);
                            }
                        }
                    }
                }
            }
            Item::QuarantineMod(qm) => {
                let mut q_env = top_env.clone();
                for sub in &qm.items {
                    match sub {
                        Item::Const(cd) => {
                            audit_expr(&cd.value, false, &q_env, acc);
                            if let Some(fv) = fold_expr(&cd.value, &q_env) {
                                q_env.insert(cd.name.clone(), fv);
                            }
                        }
                        Item::DecFn(fd) => {
                            dec_fn_count += 1;
                            audit_dec_fn(fd, &q_env, acc);
                        }
                        _ => {}
                    }
                }
            }
            Item::Struct(_) | Item::Enum(_) | Item::Use(_) => {}
        }
    }

    if is_entry && dec_fn_count == 0 {
        acc.spine_audit
            .record("entry module declares zero dec functions");
    }
}

fn audit_dec_fn(fd: &DecFnDecl, top_env: &ConstEnv, acc: &mut AuditState) {
    if fd.arms.is_empty() {
        acc.spine_audit.record(format!(
            "line {}: dec {} declares zero pattern arms",
            fd.line, fd.name
        ));
    }
    for arm in &fd.arms {
        if !arm.has_arrow {
            acc.spine_audit.record(format!(
                "line {}: dec {} missing explicit `->` structural hinge",
                arm.line, fd.name
            ));
        }
        if !has_non_pass_stmt(&arm.body) {
            acc.spine_audit.record(format!(
                "line {}: dec {} arm has empty or pass-only body",
                arm.line, fd.name
            ));
        }
        let mut env = top_env.clone();
        for p in &arm.params {
            match p {
                DecArmParam::TypedVar { name, .. } => {
                    env.remove(name);
                }
                DecArmParam::TypedPattern { pattern, .. } | DecArmParam::Pattern(pattern) => {
                    audit_pattern(pattern, arm.line, &mut env, acc);
                }
            }
        }
        if let Some(ref g) = arm.guard {
            audit_expr(g, false, &env, acc);
        }
        for s in &arm.body {
            audit_stmt(s, &mut env, acc);
        }
    }
}

fn audit_pattern(pat: &Pattern, line: usize, env: &mut ConstEnv, acc: &mut AuditState) {
    match pat {
        Pattern::Binding(name) => {
            env.remove(name);
        }
        Pattern::SupportLit(sf) => {
            if let Err(e) = SupportForm::parse(&sf.render()) {
                acc.sheddic_path_audit
                    .record(format!("line {line}: invalid support pattern: {e}"));
            }
        }
        Pattern::EnumVariant { bindings, .. } | Pattern::Tuple(bindings) => {
            for b in bindings {
                audit_pattern(b, line, env, acc);
            }
        }
        _ => {}
    }
}

fn audit_stmt(stmt: &Stmt, env: &mut ConstEnv, acc: &mut AuditState) {
    match &stmt.kind {
        StmtKind::Let { pattern, value, .. } => {
            audit_expr(value, false, env, acc);
            match pattern {
                LetPattern::Single(name) => {
                    if name != "_" {
                        if let Some(fv) = fold_expr(value, env) {
                            env.insert(name.clone(), fv);
                        } else {
                            env.remove(name);
                        }
                    }
                }
                LetPattern::Tuple(elems) => match fold_expr(value, env) {
                    Some(FoldVal::Tuple(items)) if items.len() == elems.len() => {
                        for ((n, _), fv) in elems.iter().zip(items) {
                            if n != "_" {
                                env.insert(n.clone(), fv);
                            }
                        }
                    }
                    _ => {
                        for (n, _) in elems {
                            if n != "_" {
                                env.remove(n);
                            }
                        }
                    }
                },
            }
        }
        StmtKind::Assert(e) | StmtKind::Emit(e) | StmtKind::ExprStmt(e) => {
            audit_expr(e, false, env, acc);
        }
        StmtKind::Return(Some(e)) => audit_expr(e, false, env, acc),
        StmtKind::Return(None) | StmtKind::Pass => {}
        StmtKind::QuarantineBlock { body } => {
            let mut q_env = env.clone();
            for s in body {
                audit_stmt(s, &mut q_env, acc);
            }
        }
    }
}

fn audit_expr(expr: &Expr, negated: bool, env: &ConstEnv, acc: &mut AuditState) {
    match &expr.kind {
        ExprKind::SupportLit(sf) => {
            if let Err(e) = SupportForm::parse(&sf.render()) {
                acc.sheddic_path_audit
                    .record(format!("line {}: invalid support literal: {e}", expr.line));
            }
        }
        ExprKind::WindowLit { start, end } => {
            audit_expr(start, false, env, acc);
            audit_expr(end, false, env, acc);
            if let (Some(FoldVal::Int(u)), Some(FoldVal::Int(v))) =
                (fold_expr(start, env), fold_expr(end, env))
            {
                if u >= v {
                    acc.boundary_audit.record(format!(
                        "line {}: half-open window ({u}, {v}] violates u < v",
                        expr.line
                    ));
                }
            }
        }
        ExprKind::Range { start, end } => {
            audit_expr(start, false, env, acc);
            audit_expr(end, false, env, acc);
        }
        ExprKind::BoxExpr(dims) => {
            for d in dims {
                audit_expr(d, false, env, acc);
                if let Some(FoldVal::Range(a, b)) = fold_expr(d, env) {
                    if a >= b {
                        acc.boundary_audit.record(format!(
                            "line {}: box([...]) axis range {a}..{b} violates a < b",
                            d.line
                        ));
                    }
                }
            }
        }
        ExprKind::SeqLit(items) | ExprKind::Tuple(items) => {
            for it in items {
                audit_expr(it, false, env, acc);
            }
        }
        ExprKind::MapLit(entries) => {
            for (k, v) in entries {
                audit_expr(k, false, env, acc);
                audit_expr(v, false, env, acc);
            }
        }
        ExprKind::Simulate { ticks, .. } => audit_expr(ticks, false, env, acc),
        ExprKind::Unary { op, operand } => {
            let child_negated = matches!(op, UnaryOp::Not) && !negated;
            audit_expr(operand, child_negated, env, acc);
        }
        ExprKind::Adverb { operand, .. } => audit_expr(operand, false, env, acc),
        ExprKind::Field { base, .. } => audit_expr(base, false, env, acc),
        ExprKind::Binary { op, left, right } => {
            let left_neg = (*op == BinaryOp::Eq
                && matches!(right.kind, ExprKind::BoolLit(false)))
                || (*op == BinaryOp::NotEq && matches!(right.kind, ExprKind::BoolLit(true)));
            let right_neg = (*op == BinaryOp::Eq
                && matches!(left.kind, ExprKind::BoolLit(false)))
                || (*op == BinaryOp::NotEq && matches!(left.kind, ExprKind::BoolLit(true)));
            audit_expr(left, left_neg, env, acc);
            audit_expr(right, right_neg, env, acc);
        }
        ExprKind::Index { base, index } => {
            audit_expr(base, false, env, acc);
            audit_expr(index, false, env, acc);
        }
        ExprKind::Match { scrutinee, arms } => {
            audit_expr(scrutinee, false, env, acc);
            for arm in arms {
                let mut a_env = env.clone();
                audit_pattern(&arm.pattern, arm.line, &mut a_env, acc);
                if let Some(ref g) = arm.guard {
                    audit_expr(g, false, &a_env, acc);
                }
                audit_expr(&arm.body, false, &a_env, acc);
            }
        }
        ExprKind::Call { callee, args } => {
            audit_expr(callee, false, env, acc);
            for a in args {
                audit_expr(a, false, env, acc);
            }
            if let Some(leaf) = callee_leaf_name(callee) {
                audit_call(leaf, args, expr.line, negated, env, acc);
            }
        }
        ExprKind::Pipe { input, stage } => {
            audit_expr(input, false, env, acc);
            match stage.as_ref() {
                PipeStage::Adverb(_) | PipeStage::Boundary => {}
                PipeStage::Section { rhs, .. } => audit_expr(rhs, false, env, acc),
                PipeStage::Call { callee, extra_args } => {
                    audit_expr(callee, false, env, acc);
                    for a in extra_args {
                        audit_expr(a, false, env, acc);
                    }
                    if let Some(leaf) = callee_leaf_name(callee) {
                        let mut eff_args = Vec::with_capacity(1 + extra_args.len());
                        eff_args.push((**input).clone());
                        eff_args.extend(extra_args.iter().cloned());
                        audit_call(leaf, &eff_args, expr.line, negated, env, acc);
                    }
                }
            }
        }
        _ => {}
    }
}

fn audit_call(
    leaf: &str,
    args: &[Expr],
    line: usize,
    negated: bool,
    env: &ConstEnv,
    acc: &mut AuditState,
) {
    match leaf {
        // Hook 3 (`sadar_audit`, §5.17.3): `c3 > 0`, `rho >= 0`, `|att| <= 1`, forward + reverse == 0/1r
        "sadar_promote" | "sadar_from_clipped" if args.len() == 3 => {
            let c3_f = fold_expr(&args[0], env);
            let rho_f = fold_expr(&args[1], env);
            let att_f = fold_expr(&args[2], env);
            if let Some(FoldVal::Int(ref c3)) = c3_f {
                if !c3.is_positive() {
                    acc.sadar_audit
                        .record(format!("line {line}: {leaf} requires c3 > 0, got {c3}"));
                }
            }
            if let Some(FoldVal::Int(ref rho)) = rho_f {
                if rho.is_negative() {
                    acc.sadar_audit
                        .record(format!("line {line}: {leaf} requires rho >= 0, got {rho}"));
                }
            }
            if let Some(FoldVal::Ratio(ref att)) = att_f {
                if att.abs() > Ratio::one() {
                    acc.sadar_audit.record(format!(
                        "line {line}: {leaf} requires |att| <= 1, got {}",
                        att.render_v1()
                    ));
                }
            }
            if let (
                Some(FoldVal::Int(c3)),
                Some(FoldVal::Int(rho)),
                Some(FoldVal::Ratio(att)),
            ) = (c3_f, rho_f, att_f)
            {
                if acc.sadar_audit.passed() {
                    match (
                        SadarPromotion::promote(&c3, &rho, att.clone()),
                        SadarPromotion::promote(&c3, &rho, -att),
                    ) {
                        (Ok(fwd), Ok(rev)) => {
                            if (&fwd.sadar + &rev.sadar) != Ratio::zero() {
                                acc.sadar_audit.record(format!(
                                    "line {line}: {leaf} violated forward + reverse == 0/1r"
                                ));
                            }
                        }
                        (Err(e), _) | (_, Err(e)) => {
                            acc.sadar_audit
                                .record(format!("line {line}: {leaf} failed SADAR bounds: {e}"));
                        }
                    }
                }
            }
        }
        "scoped_channel_flux" if args.len() == 4 => {
            if let Some(FoldVal::Int(c3)) = fold_expr(&args[0], env) {
                if !c3.is_positive() {
                    acc.sadar_audit.record(format!(
                        "line {line}: scoped_channel_flux requires c3 > 0, got {c3}"
                    ));
                }
            }
            if let Some(FoldVal::Int(rho)) = fold_expr(&args[1], env) {
                if rho.is_negative() {
                    acc.sadar_audit.record(format!(
                        "line {line}: scoped_channel_flux requires rho >= 0, got {rho}"
                    ));
                }
            }
            if let Some(FoldVal::Int(win_len)) = fold_expr(&args[2], env) {
                if win_len.is_negative() {
                    acc.sadar_audit.record(format!(
                        "line {line}: scoped_channel_flux requires win_len >= 0, got {win_len}"
                    ));
                }
            }
            if let Some(FoldVal::Ratio(att)) = fold_expr(&args[3], env) {
                if att.abs() > Ratio::one() {
                    acc.sadar_audit.record(format!(
                        "line {line}: scoped_channel_flux requires |att| <= 1, got {}",
                        att.render_v1()
                    ));
                }
            }
        }

        // Hook 4 (`sheddic_path_audit`, §5.17.5): `_yro` support parity + even sheddic route
        "support" => {
            if args.len() == 1 {
                if let Some(FoldVal::Bytes(bs)) = fold_expr(&args[0], env) {
                    match std::str::from_utf8(&bs) {
                        Ok(s) => {
                            if let Err(e) = SupportForm::parse(s) {
                                acc.sheddic_path_audit.record(format!(
                                    "line {line}: support(b{s:?}) failed _yro audit: {e}"
                                ));
                            }
                        }
                        Err(_) => {
                            acc.sheddic_path_audit
                                .record(format!("line {line}: support(bytes) received invalid UTF-8"));
                        }
                    }
                }
            } else if args.len() == 4 {
                if let (
                    Some(FoldVal::Int(p)),
                    Some(FoldVal::Int(q)),
                    Some(FoldVal::Int(l)),
                    Some(FoldVal::Bool(yro)),
                ) = (
                    fold_expr(&args[0], env),
                    fold_expr(&args[1], env),
                    fold_expr(&args[2], env),
                    fold_expr(&args[3], env),
                ) {
                    if let Err(e) = SupportForm::new(p, q, l, yro) {
                        acc.sheddic_path_audit
                            .record(format!("line {line}: support(...) failed _yro audit: {e}"));
                    }
                }
            }
        }
        "verify_sheddic_route" | "audit_sheddic_route" if !negated && args.len() == 2 => {
            let xs_opt = match fold_expr(&args[0], env) {
                Some(FoldVal::Int(n)) => Some(n),
                _ => None,
            };
            let enc_opt = match fold_expr(&args[1], env) {
                Some(FoldVal::B8(b)) => Some(b),
                Some(FoldVal::Int(n)) => n.to_u64().and_then(|v| u8::try_from(v).ok()),
                _ => None,
            };
            if let (Some(xs), Some(enc)) = (xs_opt, enc_opt) {
                if xs.is_positive() && (enc < 2 || !enc.is_multiple_of(2)) {
                    acc.sheddic_path_audit.record(format!(
                        "line {line}: sheddic route audit failed: x_shedding={xs} > 0 requires even enclosure >= 2, got {enc}"
                    ));
                }
            }
        }

        // Hook 5 (`cycle_audit`, §5.17.7): DAG order `C_f[p:q]` before `RD` (`h_mu >= 1` & `close(...)` before `rate/beats/phases`)
        "support_rd" if args.len() == 2 => {
            if let Some(FoldVal::Int(h_mu)) = fold_expr(&args[1], env) {
                if !h_mu.is_positive() {
                    acc.cycle_audit.record(format!(
                        "line {line}: support_rd requires h_mu >= 1, got {h_mu}"
                    ));
                }
            }
        }
        "support_pressure" | "support_overhang" if args.len() == 4 => {
            if let Some(FoldVal::Int(h_mu)) = fold_expr(&args[2], env) {
                if !h_mu.is_positive() {
                    acc.cycle_audit.record(format!(
                        "line {line}: {leaf} requires h_mu >= 1, got {h_mu}"
                    ));
                }
            }
        }
        "rate" | "beats" | "phases" if args.len() == 1 => {
            if is_unclosed_trace_expr(&args[0], env) {
                acc.cycle_audit.record(format!(
                    "line {line}: {leaf}(...) called on unclosed trace prior to close(...)"
                ));
            }
        }

        // Hook 6 (`frg_audit`, §5.17.9): `t_cross - t_zero == c_cross`
        "DoubleSlitBin" => {
            let field_names = acc
                .struct_fields
                .get("DoubleSlitBin")
                .cloned()
                .unwrap_or_else(|| {
                    vec![
                        "bin".to_string(),
                        "t0".to_string(),
                        "cx".to_string(),
                        "tx".to_string(),
                    ]
                });
            if args.len() == field_names.len() {
                let mut t0_opt = None;
                let mut cx_opt = None;
                let mut tx_opt = None;
                for (fname, arg) in field_names.iter().zip(args) {
                    if let Some(FoldVal::Int(v)) = fold_expr(arg, env) {
                        match fname.as_str() {
                            "t0" => t0_opt = Some(v),
                            "cx" => cx_opt = Some(v),
                            "tx" => tx_opt = Some(v),
                            _ => {}
                        }
                    }
                }
                if let (Some(t0), Some(cx), Some(tx)) = (t0_opt, cx_opt, tx_opt) {
                    if (&tx - &t0) != cx {
                        acc.frg_audit.record(format!(
                            "line {line}: DoubleSlitBin violates FRG identity tx - t0 == cx ({tx} - {t0} != {cx})"
                        ));
                    }
                }
            }
        }
        "verify_frg_cross_return" if !negated && args.len() == 3 => {
            if let (
                Some(FoldVal::Int(t0)),
                Some(FoldVal::Int(tx)),
                Some(FoldVal::Int(cx)),
            ) = (
                fold_expr(&args[0], env),
                fold_expr(&args[1], env),
                fold_expr(&args[2], env),
            ) {
                if &(&tx - &t0) != &cx {
                    acc.frg_audit.record(format!(
                        "line {line}: FRG cross-return identity failed: t_cross({tx}) - t_zero({t0}) != c_cross({cx})"
                    ));
                }
            }
        }
        "double_slit_bin_at" if args.len() == 4 => {
            if let (
                Some(FoldVal::Int(s)),
                Some(FoldVal::Int(cx)),
                Some(FoldVal::Int(ob)),
            ) = (
                fold_expr(&args[1], env),
                fold_expr(&args[2], env),
                fold_expr(&args[3], env),
            ) {
                let t0 = &s + &s;
                let tx = &t0 + &cx;
                if s.is_negative() || ob.is_negative() || tx.is_negative() || &(&tx - &t0) != &cx {
                    acc.frg_audit.record(format!(
                        "line {line}: double_slit_bin_at(s={s}, cx={cx}, ob={ob}) violates non-negative FRG cross-return domain"
                    ));
                }
            }
        }

        // Higher-order AFC scope & sequence callbacks
        "evolve" | "scan" | "fold_seq" if args.len() == 3 || args.len() == 4 => {
            if let Some(cb_leaf) = callee_leaf_name(&args[2]) {
                let init_fv = fold_expr(&args[0], env);
                let env_fv = args.get(3).and_then(|a| fold_expr(a, env));
                let elems = fold_iter_elems(&args[1], env);
                if let Some(mut cur_s) = init_fv {
                    for el in elems {
                        let mut cb_args = vec![cur_s.clone(), el];
                        if let Some(ref efv) = env_fv {
                            cb_args.push(efv.clone());
                        } else if args.len() == 4 {
                            break;
                        }
                        audit_folded_builtin(cb_leaf, &cb_args, line, acc);
                        if let Some(next_s) = fold_builtin_step(cb_leaf, &cb_args) {
                            cur_s = next_s;
                        }
                    }
                }
            }
        }
        "fold" if args.len() == 3 => {
            if let Some(cb_leaf) = callee_leaf_name(&args[2]) {
                if let Some(mut cur_s) = fold_expr(&args[1], env) {
                    for el in fold_iter_elems(&args[0], env) {
                        let cb_args = vec![cur_s.clone(), el];
                        audit_folded_builtin(cb_leaf, &cb_args, line, acc);
                        if let Some(next_s) = fold_builtin_step(cb_leaf, &cb_args) {
                            cur_s = next_s;
                        }
                    }
                }
            }
        }
        "each" if args.len() == 2 => {
            if let Some(cb_leaf) = callee_leaf_name(&args[1]) {
                for el in fold_iter_elems(&args[0], env) {
                    audit_folded_builtin(cb_leaf, &[el], line, acc);
                }
            }
        }
        "map_seq" | "filter_seq" | "tabulate" | "count" | "any" | "all"
            if args.len() == 2 || args.len() == 3 =>
        {
            if let Some(cb_leaf) = callee_leaf_name(&args[1]) {
                let env_fv = args.get(2).and_then(|a| fold_expr(a, env));
                if args.len() == 2 || env_fv.is_some() {
                    for el in fold_iter_elems(&args[0], env) {
                        let mut cb_args = vec![el];
                        if let Some(ref efv) = env_fv {
                            cb_args.push(efv.clone());
                        }
                        audit_folded_builtin(cb_leaf, &cb_args, line, acc);
                    }
                }
            }
        }
        _ => {}
    }
}

fn fold_iter_elems(src: &Expr, env: &ConstEnv) -> Vec<FoldVal> {
    match fold_expr(src, env) {
        Some(FoldVal::Seq(items)) => items,
        Some(FoldVal::Bytes(bs)) => bs.into_iter().map(FoldVal::B8).collect(),
        Some(FoldVal::Range(a, b)) => {
            if let (Some(ai), Some(bi)) = (a.to_i64(), b.to_i64()) {
                if ai < bi {
                    return (ai..bi)
                        .take(256)
                        .map(|k| FoldVal::Int(BigInt::from_i64(k)))
                        .collect();
                }
            }
            Vec::new()
        }
        Some(FoldVal::Window(u, v)) => {
            if let (Some(ui), Some(vi)) = (u.to_i64(), v.to_i64()) {
                if ui < vi {
                    let mut out = Vec::new();
                    let mut prev = ui;
                    while prev < vi && out.len() < 256 {
                        let k = prev + 1;
                        out.push(FoldVal::Int(BigInt::from_i64(k)));
                        prev = k;
                    }
                    return out;
                }
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn fold_builtin_step(cb_leaf: &str, cb_args: &[FoldVal]) -> Option<FoldVal> {
    match (cb_leaf, cb_args) {
        ("sadar_promote", [FoldVal::Int(c3), FoldVal::Int(rho), FoldVal::Ratio(att)]) => {
            SadarPromotion::promote(c3, rho, att.clone())
                .ok()
                .map(|r| FoldVal::Ratio(r.sadar))
        }
        _ => None,
    }
}

fn audit_folded_builtin(cb_leaf: &str, cb_args: &[FoldVal], line: usize, acc: &mut AuditState) {
    match (cb_leaf, cb_args) {
        ("sadar_promote" | "sadar_from_clipped", [c3_f, rho_f, att_f]) => {
            if let FoldVal::Int(c3) = c3_f {
                if !c3.is_positive() {
                    acc.sadar_audit
                        .record(format!("line {line}: {cb_leaf} requires c3 > 0, got {c3}"));
                }
            }
            if let FoldVal::Int(rho) = rho_f {
                if rho.is_negative() {
                    acc.sadar_audit
                        .record(format!("line {line}: {cb_leaf} requires rho >= 0, got {rho}"));
                }
            }
            if let FoldVal::Ratio(att) = att_f {
                if att.abs() > Ratio::one() {
                    acc.sadar_audit.record(format!(
                        "line {line}: {cb_leaf} requires |att| <= 1, got {}",
                        att.render_v1()
                    ));
                }
            }
        }
        ("support", [FoldVal::Bytes(bs)]) => match std::str::from_utf8(bs) {
            Ok(s) => {
                if let Err(e) = SupportForm::parse(s) {
                    acc.sheddic_path_audit
                        .record(format!("line {line}: support(b{s:?}) failed _yro audit: {e}"));
                }
            }
            Err(_) => {
                acc.sheddic_path_audit
                    .record(format!("line {line}: support(bytes) received invalid UTF-8"));
            }
        },
        ("support_rd", [_, FoldVal::Int(h_mu)]) => {
            if !h_mu.is_positive() {
                acc.cycle_audit
                    .record(format!("line {line}: support_rd requires h_mu >= 1, got {h_mu}"));
            }
        }
        ("verify_sheddic_route" | "audit_sheddic_route", [FoldVal::Int(xs), enc_fv]) => {
            let enc_opt = match enc_fv {
                FoldVal::B8(b) => Some(*b),
                FoldVal::Int(n) => n.to_u64().and_then(|v| u8::try_from(v).ok()),
                _ => None,
            };
            if let Some(enc) = enc_opt {
                if xs.is_positive() && (enc < 2 || !enc.is_multiple_of(2)) {
                    acc.sheddic_path_audit.record(format!(
                        "line {line}: sheddic route audit failed: x_shedding={xs} > 0 requires even enclosure >= 2, got {enc}"
                    ));
                }
            }
        }
        _ => {}
    }
}
