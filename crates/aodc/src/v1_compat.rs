#![forbid(unsafe_code)]

//! Self-contained Frozen Dec v1 (`2bc927c`) compatibility frontend, static typechecker,
//! `DecIR` v1 (`"version": 1`) hermetic serializer/verifier, and exact deterministic evaluator.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use aod_core::bigint::BigInt;
use aod_core::error::CoreError;
use aod_core::json::{parse_strict_json, JsonValue};
use aod_core::ledger::probability;
use aod_core::ratio::Ratio;
use aod_core::sha256::Sha256;
use aod_core::ternary::{Q4, T1};
use aod_core::witness::{Coupling, Fusion, Trace};

use crate::diagnostics::{DecResult, Diagnostic, DiagnosticCode};
use crate::eval::{EvalOutput, Value};

pub const V1_IR_FORMAT: &str = "aod.dec.ir";
pub const V1_IR_VERSION: i64 = 1;
pub const MAX_V1_SOURCE_BYTES: usize = 4_000_000;
pub const MAX_V1_IR_BYTES: usize = 8_000_000;

/// CLI compatibility selector (`--compat v1` vs default `--compat v3`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CompatMode {
    V1,
    #[default]
    V3,
}

impl CompatMode {
    pub fn parse_cli(s: &str) -> Result<Self, String> {
        match s {
            "v1" => Ok(Self::V1),
            "v3" => Ok(Self::V3),
            other => Err(format!(
                "invalid --compat value {other:?}; expected \"v1\" or \"v3\""
            )),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V1 => "v1",
            Self::V3 => "v3",
        }
    }
}

/// Checked 32-trit balanced ternary scalar in `[-926_510_094_425_920, +926_510_094_425_920]`
/// preserving Frozen Dec v1 `t32`, `pack(t32) -> int`, and `unpack(int) -> t32` semantics.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct V1T32(i128);

impl V1T32 {
    pub const LIMIT: i128 = 926_510_094_425_920;

    pub fn new(v: i128) -> Result<Self, String> {
        if (-Self::LIMIT..=Self::LIMIT).contains(&v) {
            Ok(Self(v))
        } else {
            Err(format!("t32 overflow: {v}"))
        }
    }

    pub fn from_bigint(v: &BigInt) -> Result<Self, String> {
        let n = v
            .to_i128()
            .ok_or_else(|| format!("t32 overflow: {v}"))?;
        Self::new(n)
    }

    #[inline]
    pub const fn value(self) -> i128 {
        self.0
    }

    #[inline]
    pub const fn abs(self) -> Self {
        Self(self.0.abs())
    }

    pub fn checked_neg(self) -> Result<Self, String> {
        Self::new(-self.0)
    }

    pub fn checked_add(self, rhs: Self) -> Result<Self, String> {
        let sum = self
            .0
            .checked_add(rhs.0)
            .ok_or_else(|| "t32 addition overflow".to_string())?;
        Self::new(sum)
    }

    pub fn checked_sub(self, rhs: Self) -> Result<Self, String> {
        let diff = self
            .0
            .checked_sub(rhs.0)
            .ok_or_else(|| "t32 subtraction overflow".to_string())?;
        Self::new(diff)
    }

    pub fn checked_mul(self, rhs: Self) -> Result<Self, String> {
        let prod = self
            .0
            .checked_mul(rhs.0)
            .ok_or_else(|| "t32 multiplication overflow".to_string())?;
        Self::new(prod)
    }

    pub fn checked_div_floor(self, rhs: Self) -> Result<Self, String> {
        if rhs.0 == 0 {
            return Err("division by zero".to_string());
        }
        let mut q = self.0 / rhs.0;
        let r = self.0 % rhs.0;
        if r != 0 && ((r < 0) != (rhs.0 < 0)) {
            q -= 1;
        }
        Self::new(q)
    }

    pub fn checked_rem_floor(self, rhs: Self) -> Result<Self, String> {
        if rhs.0 == 0 {
            return Err("division by zero".to_string());
        }
        let mut r = self.0 % rhs.0;
        if r != 0 && ((r < 0) != (rhs.0 < 0)) {
            r += rhs.0;
        }
        Self::new(r)
    }

    pub fn pack(self) -> u64 {
        let mut n = self.0;
        let mut result = 0u64;
        for lane in 0..32u32 {
            let rem = n.rem_euclid(3);
            let digit: i128 = if rem == 2 { -1 } else { rem };
            n = (n - digit) / 3;
            let code: u64 = match digit {
                0 => 0,
                1 => 1,
                -1 => 2,
                _ => unreachable!(),
            };
            result |= code << (2 * lane);
        }
        debug_assert_eq!(n, 0);
        result
    }

    pub fn unpack(bits: u64) -> Result<Self, String> {
        let mut n = 0i128;
        let mut pow3 = 1i128;
        for lane in 0..32u32 {
            let code = ((bits >> (2 * lane)) & 3) as u8;
            let digit: i128 = match code {
                0 => 0,
                1 => 1,
                2 => -1,
                3 => return Err("invalid 2-bit trit lane 0b11 in unpack".to_string()),
                _ => unreachable!(),
            };
            n += digit * pow3;
            if lane < 31 {
                pow3 *= 3;
            }
        }
        Self::new(n)
    }

    pub fn unpack_bigint(bits: &BigInt) -> Result<Self, String> {
        let raw = bits
            .to_u64()
            .ok_or_else(|| format!("packed t32 outside u64 domain: {bits}"))?;
        Self::unpack(raw)
    }

    pub fn render_v1(self) -> String {
        format!("t32({})", self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum V1Type {
    Int,
    Bool,
    Str,
    T1,
    T32,
    Q4,
    Ratio,
    Trace,
    Fusion,
    Coupling,
    TraceList,
}

impl V1Type {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "int" => Some(Self::Int),
            "bool" => Some(Self::Bool),
            "str" => Some(Self::Str),
            "t1" => Some(Self::T1),
            "t32" => Some(Self::T32),
            "q4" => Some(Self::Q4),
            "ratio" => Some(Self::Ratio),
            "trace" => Some(Self::Trace),
            "fusion" => Some(Self::Fusion),
            "coupling" => Some(Self::Coupling),
            _ => None,
        }
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Int => "int",
            Self::Bool => "bool",
            Self::Str => "str",
            Self::T1 => "t1",
            Self::T32 => "t32",
            Self::Q4 => "q4",
            Self::Ratio => "ratio",
            Self::Trace => "trace",
            Self::Fusion => "fusion",
            Self::Coupling => "coupling",
            Self::TraceList => "[trace]",
        }
    }
}

impl fmt::Display for V1Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct V1Param {
    pub ty: V1Type,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct V1Branch {
    pub params: Vec<V1Param>,
    pub return_type: V1Type,
    pub output_name: String,
    pub body: Vec<V1Stmt>,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct V1Function {
    pub name: String,
    pub header_params: Vec<String>,
    pub output_name: String,
    pub branches: Vec<V1Branch>,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct V1Program {
    pub source: String,
    pub source_sha256: String,
    pub functions: Vec<V1Function>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct V1Stmt {
    pub kind: V1StmtKind,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum V1StmtKind {
    LocalDecl {
        mutable: bool,
        ty: Option<V1Type>,
        name: String,
        value: V1Expr,
    },
    Assign {
        name: String,
        value: V1Expr,
    },
    Assert(V1Expr),
    Emit(V1Expr),
    If {
        branches: Vec<(V1Expr, Vec<V1Stmt>)>,
        else_body: Vec<V1Stmt>,
    },
    Return(Option<V1Expr>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum V1UnaryOp {
    Neg,
    Pos,
    Not,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum V1BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct V1Expr {
    pub kind: V1ExprKind,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum V1ExprKind {
    IntLit(BigInt),
    BoolLit(bool),
    StrLit(String),
    Ident(String),
    Unary {
        op: V1UnaryOp,
        operand: Box<V1Expr>,
    },
    Binary {
        op: V1BinaryOp,
        left: Box<V1Expr>,
        right: Box<V1Expr>,
    },
    Conditional {
        then_expr: Box<V1Expr>,
        cond: Box<V1Expr>,
        else_expr: Box<V1Expr>,
    },
    Call {
        name: String,
        args: Vec<V1Expr>,
    },
    Index {
        base: Box<V1Expr>,
        index: Box<V1Expr>,
    },
}

// ============================================================================
// Frozen Dec v1 Lexer
// ============================================================================

#[derive(Clone, Debug, Eq, PartialEq)]
enum V1Tok {
    Dec,
    If,
    Elif,
    Else,
    And,
    Or,
    Not,
    True,
    False,
    Assert,
    Emit,
    Return,
    Let,
    Mut,
    Ident(String),
    IntLit(BigInt),
    StrLit(String),
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Arrow,
    Eq,
    EqEq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Plus,
    Minus,
    Star,
    Slash,
    SlashSlash,
    Percent,
    Newline,
    Indent,
    Dedent,
    Eof,
}

#[derive(Clone, Debug)]
struct SpannedTok {
    kind: V1Tok,
    line: usize,
    col: usize,
}

fn is_v3_only_keyword(word: &str) -> bool {
    matches!(
        word,
        "dec"
            | "use"
            | "const"
            | "struct"
            | "enum"
            | "chamber"
            | "prepare"
            | "observe"
            | "quarantine"
            | "where"
            | "match"
            | "for"
            | "while"
            | "break"
            | "continue"
            | "pass"
            | "null"
            | "simulate"
    )
}

fn lex_v1(source: &str) -> DecResult<Vec<SpannedTok>> {
    if source.len() > MAX_V1_SOURCE_BYTES {
        return Err(Diagnostic::new(
            DiagnosticCode::LexError,
            "source exceeds 4 MB limit",
        ));
    }

    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    let mut line = 1usize;
    let mut line_start = 0usize;
    let mut at_line_start = true;
    let mut paren_depth = 0usize;
    let mut indent_stack: Vec<usize> = vec![0];
    let mut tokens: Vec<SpannedTok> = Vec::new();

    while i < len {
        if at_line_start && paren_depth == 0 {
            let mut indent = 0usize;
            while i < len {
                match bytes[i] {
                    b' ' => {
                        indent += 1;
                        i += 1;
                    }
                    b'\t' => {
                        return Err(Diagnostic::at(
                            DiagnosticCode::LexError,
                            line,
                            i - line_start + 1,
                            "tabs are forbidden in Dec indentation; use spaces",
                        ));
                    }
                    _ => break,
                }
            }
            if i >= len {
                break;
            }
            if bytes[i] == b'#' {
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
                if i < len && bytes[i] == b'\n' {
                    i += 1;
                    line += 1;
                    line_start = i;
                }
                continue;
            }
            if bytes[i] == b'\r' && i + 1 < len && bytes[i + 1] == b'\n' {
                i += 2;
                line += 1;
                line_start = i;
                continue;
            }
            if bytes[i] == b'\n' {
                i += 1;
                line += 1;
                line_start = i;
                continue;
            }

            let cur_indent = *indent_stack.last().expect("non-empty indent stack");
            if indent > cur_indent {
                indent_stack.push(indent);
                tokens.push(SpannedTok {
                    kind: V1Tok::Indent,
                    line,
                    col: 1,
                });
            } else if indent < cur_indent {
                while let Some(&top) = indent_stack.last() {
                    if top > indent {
                        indent_stack.pop();
                        tokens.push(SpannedTok {
                            kind: V1Tok::Dedent,
                            line,
                            col: 1,
                        });
                    } else {
                        break;
                    }
                }
                if indent_stack.last().copied() != Some(indent) {
                    return Err(Diagnostic::at(
                        DiagnosticCode::LexError,
                        line,
                        1,
                        "inconsistent dedent level in Frozen Dec v1",
                    ));
                }
            }
            at_line_start = false;
        }

        let b = bytes[i];
        let col = i - line_start + 1;

        match b {
            b' ' | b'\r' => {
                i += 1;
            }
            b'\t' => {
                return Err(Diagnostic::at(
                    DiagnosticCode::LexError,
                    line,
                    col,
                    "tabs are forbidden in Frozen Dec v1",
                ));
            }
            b'#' => {
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'\n' => {
                if paren_depth == 0 {
                    if !matches!(
                        tokens.last().map(|t| &t.kind),
                        Some(V1Tok::Newline | V1Tok::Indent | V1Tok::Dedent) | None
                    ) {
                        tokens.push(SpannedTok {
                            kind: V1Tok::Newline,
                            line,
                            col,
                        });
                    }
                    at_line_start = true;
                }
                i += 1;
                line += 1;
                line_start = i;
            }
            b'(' => {
                paren_depth += 1;
                tokens.push(SpannedTok {
                    kind: V1Tok::LParen,
                    line,
                    col,
                });
                i += 1;
            }
            b')' => {
                if paren_depth == 0 {
                    return Err(Diagnostic::at(
                        DiagnosticCode::ParseError,
                        line,
                        col,
                        "unmatched ')'",
                    ));
                }
                paren_depth -= 1;
                tokens.push(SpannedTok {
                    kind: V1Tok::RParen,
                    line,
                    col,
                });
                i += 1;
            }
            b'[' => {
                paren_depth += 1;
                tokens.push(SpannedTok {
                    kind: V1Tok::LBracket,
                    line,
                    col,
                });
                i += 1;
            }
            b']' => {
                if paren_depth == 0 {
                    return Err(Diagnostic::at(
                        DiagnosticCode::ParseError,
                        line,
                        col,
                        "unmatched ']'",
                    ));
                }
                paren_depth -= 1;
                tokens.push(SpannedTok {
                    kind: V1Tok::RBracket,
                    line,
                    col,
                });
                i += 1;
            }
            b'{' | b'}' => {
                return Err(Diagnostic::at(
                    DiagnosticCode::ParseError,
                    line,
                    col,
                    format!("braces `{}` are not in --compat v1", b as char),
                ));
            }
            b',' => {
                tokens.push(SpannedTok {
                    kind: V1Tok::Comma,
                    line,
                    col,
                });
                i += 1;
            }
            b':' => {
                if i + 1 < len && bytes[i + 1] == b':' {
                    return Err(Diagnostic::at(
                        DiagnosticCode::ParseError,
                        line,
                        col,
                        "module path `::` is not in --compat v1",
                    ));
                }
                tokens.push(SpannedTok {
                    kind: V1Tok::Colon,
                    line,
                    col,
                });
                i += 1;
            }
            b'+' => {
                tokens.push(SpannedTok {
                    kind: V1Tok::Plus,
                    line,
                    col,
                });
                i += 1;
            }
            b'-' => {
                if i + 1 < len && bytes[i + 1] == b'>' {
                    tokens.push(SpannedTok {
                        kind: V1Tok::Arrow,
                        line,
                        col,
                    });
                    i += 2;
                } else {
                    tokens.push(SpannedTok {
                        kind: V1Tok::Minus,
                        line,
                        col,
                    });
                    i += 1;
                }
            }
            b'*' => {
                tokens.push(SpannedTok {
                    kind: V1Tok::Star,
                    line,
                    col,
                });
                i += 1;
            }
            b'/' => {
                if i + 1 < len && bytes[i + 1] == b'/' {
                    tokens.push(SpannedTok {
                        kind: V1Tok::SlashSlash,
                        line,
                        col,
                    });
                    i += 2;
                } else {
                    tokens.push(SpannedTok {
                        kind: V1Tok::Slash,
                        line,
                        col,
                    });
                    i += 1;
                }
            }
            b'%' => {
                tokens.push(SpannedTok {
                    kind: V1Tok::Percent,
                    line,
                    col,
                });
                i += 1;
            }
            b'=' => {
                if i + 1 < len && bytes[i + 1] == b'=' {
                    tokens.push(SpannedTok {
                        kind: V1Tok::EqEq,
                        line,
                        col,
                    });
                    i += 2;
                } else {
                    tokens.push(SpannedTok {
                        kind: V1Tok::Eq,
                        line,
                        col,
                    });
                    i += 1;
                }
            }
            b'!' => {
                if i + 1 < len && bytes[i + 1] == b'=' {
                    tokens.push(SpannedTok {
                        kind: V1Tok::NotEq,
                        line,
                        col,
                    });
                    i += 2;
                } else {
                    return Err(Diagnostic::at(
                        DiagnosticCode::LexError,
                        line,
                        col,
                        "unexpected '!'; use `not` or `!=`",
                    ));
                }
            }
            b'<' => {
                if i + 1 < len && bytes[i + 1] == b'=' {
                    tokens.push(SpannedTok {
                        kind: V1Tok::LtEq,
                        line,
                        col,
                    });
                    i += 2;
                } else {
                    tokens.push(SpannedTok {
                        kind: V1Tok::Lt,
                        line,
                        col,
                    });
                    i += 1;
                }
            }
            b'>' => {
                if i + 1 < len && bytes[i + 1] == b'=' {
                    tokens.push(SpannedTok {
                        kind: V1Tok::GtEq,
                        line,
                        col,
                    });
                    i += 2;
                } else {
                    tokens.push(SpannedTok {
                        kind: V1Tok::Gt,
                        line,
                        col,
                    });
                    i += 1;
                }
            }
            b'"' | b'\'' => {
                let quote = b;
                i += 1;
                let mut s = String::new();
                let mut closed = false;
                while i < len {
                    let ch_b = bytes[i];
                    if ch_b == b'\n' || ch_b == b'\r' {
                        break;
                    }
                    if ch_b == quote {
                        i += 1;
                        closed = true;
                        break;
                    }
                    if ch_b == b'\\' {
                        i += 1;
                        if i >= len {
                            break;
                        }
                        match bytes[i] {
                            b'\\' => s.push('\\'),
                            b'"' => s.push('"'),
                            b'\'' => s.push('\''),
                            b'n' => s.push('\n'),
                            b'r' => s.push('\r'),
                            b't' => s.push('\t'),
                            b'0' => s.push('\0'),
                            other => {
                                return Err(Diagnostic::at(
                                    DiagnosticCode::LexError,
                                    line,
                                    i - line_start + 1,
                                    format!("unsupported escape sequence \\{}", other as char),
                                ));
                            }
                        }
                        i += 1;
                    } else {
                        let rest = std::str::from_utf8(&bytes[i..]).map_err(|_| {
                            Diagnostic::at(
                                DiagnosticCode::LexError,
                                line,
                                i - line_start + 1,
                                "invalid UTF-8 inside string literal",
                            )
                        })?;
                        let ch = rest.chars().next().expect("non-empty str");
                        s.push(ch);
                        i += ch.len_utf8();
                    }
                }
                if !closed {
                    return Err(Diagnostic::at(
                        DiagnosticCode::LexError,
                        line,
                        col,
                        "unterminated string literal",
                    ));
                }
                tokens.push(SpannedTok {
                    kind: V1Tok::StrLit(s),
                    line,
                    col,
                });
            }
            b'0'..=b'9' => {
                let start = i;
                while i < len && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i < len && bytes[i] == b'.' && i + 1 < len && bytes[i + 1].is_ascii_digit() {
                    return Err(Diagnostic::at(
                        DiagnosticCode::FloatLiteralForbidden,
                        line,
                        col,
                        "decimal fraction literals are forbidden in Dec; use exact ratio(num, den)",
                    ));
                }
                if i < len && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
                    let suf_start = i;
                    while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                        i += 1;
                    }
                    let suffix = std::str::from_utf8(&bytes[suf_start..i]).unwrap_or("");
                    return Err(Diagnostic::at(
                        DiagnosticCode::ParseError,
                        line,
                        col,
                        format!(
                            "suffixed numeric literal `{}{}` is v3 syntax and forbidden in --compat v1",
                            std::str::from_utf8(&bytes[start..suf_start]).unwrap_or(""),
                            suffix
                        ),
                    ));
                }
                if i < len
                    && bytes[i] == b':'
                    && i + 1 < len
                    && bytes[i + 1].is_ascii_digit()
                {
                    return Err(Diagnostic::at(
                        DiagnosticCode::ParseError,
                        line,
                        col,
                        "support literal `p:q:l` is v3 syntax and forbidden in --compat v1",
                    ));
                }
                let raw_num = std::str::from_utf8(&bytes[start..i]).expect("ascii digits");
                let val = BigInt::parse_decimal(raw_num).map_err(|e| {
                    Diagnostic::at(DiagnosticCode::LexError, line, col, e.to_string())
                })?;
                tokens.push(SpannedTok {
                    kind: V1Tok::IntLit(val),
                    line,
                    col,
                });
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                if b == b'b' && i + 1 < len && (bytes[i + 1] == b'"' || bytes[i + 1] == b'\'') {
                    return Err(Diagnostic::at(
                        DiagnosticCode::ParseError,
                        line,
                        col,
                        "byte literal `b\"...\"` is v3 syntax and forbidden in --compat v1",
                    ));
                }
                let start = i;
                while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let ident = std::str::from_utf8(&bytes[start..i]).expect("ascii ident");
                if is_v3_only_keyword(ident) {
                    return Err(Diagnostic::at(
                        DiagnosticCode::ParseError,
                        line,
                        col,
                        format!("keyword `{ident}` is v3 syntax and forbidden in --compat v1"),
                    ));
                }
                let kind = match ident {
                    "Dec" => V1Tok::Dec,
                    "if" => V1Tok::If,
                    "elif" => V1Tok::Elif,
                    "else" => V1Tok::Else,
                    "and" => V1Tok::And,
                    "or" => V1Tok::Or,
                    "not" => V1Tok::Not,
                    "true" => V1Tok::True,
                    "false" => V1Tok::False,
                    "assert" => V1Tok::Assert,
                    "emit" => V1Tok::Emit,
                    "return" => V1Tok::Return,
                    "let" => V1Tok::Let,
                    "mut" => V1Tok::Mut,
                    _ => V1Tok::Ident(ident.to_string()),
                };
                tokens.push(SpannedTok { kind, line, col });
            }
            _ => {
                return Err(Diagnostic::at(
                    DiagnosticCode::ParseError,
                    line,
                    col,
                    "unexpected character in --compat v1 source",
                ));
            }
        }
    }

    if paren_depth != 0 {
        return Err(Diagnostic::at_line(
            DiagnosticCode::ParseError,
            line,
            "unclosed '(' or '[' in source",
        ));
    }

    if !matches!(
        tokens.last().map(|t| &t.kind),
        Some(V1Tok::Newline | V1Tok::Dedent) | None
    ) {
        tokens.push(SpannedTok {
            kind: V1Tok::Newline,
            line,
            col: 1,
        });
    }

    while indent_stack.len() > 1 {
        indent_stack.pop();
        tokens.push(SpannedTok {
            kind: V1Tok::Dedent,
            line,
            col: 1,
        });
    }

    tokens.push(SpannedTok {
        kind: V1Tok::Eof,
        line,
        col: 1,
    });
    Ok(tokens)
}

// ============================================================================
// Frozen Dec v1 Parser
// ============================================================================

struct V1Parser {
    tokens: Vec<SpannedTok>,
    pos: usize,
}

impl V1Parser {
    fn new(tokens: Vec<SpannedTok>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &SpannedTok {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_n(&self, offset: usize) -> &SpannedTok {
        let idx = (self.pos + offset).min(self.tokens.len() - 1);
        &self.tokens[idx]
    }

    fn line(&self) -> usize {
        self.peek().line
    }

    fn advance(&mut self) -> SpannedTok {
        let tok = self.peek().clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn check(&self, expected: &V1Tok) -> bool {
        &self.peek().kind == expected
    }

    fn expect(&mut self, expected: &V1Tok, desc: &str) -> DecResult<()> {
        if self.check(expected) {
            self.advance();
            Ok(())
        } else {
            let p = self.peek();
            Err(Diagnostic::at(
                DiagnosticCode::ParseError,
                p.line,
                p.col,
                format!("expected {desc}, found {:?}", p.kind),
            ))
        }
    }

    fn expect_ident(&mut self, desc: &str) -> DecResult<String> {
        let p = self.peek().clone();
        match p.kind {
            V1Tok::Ident(s) => {
                self.advance();
                Ok(s)
            }
            _ => Err(Diagnostic::at(
                DiagnosticCode::ParseError,
                p.line,
                p.col,
                format!("expected {desc}, found {:?}", p.kind),
            )),
        }
    }

    fn skip_newlines(&mut self) {
        while self.check(&V1Tok::Newline) {
            self.advance();
        }
    }

    fn parse_type(&mut self) -> DecResult<V1Type> {
        let p = self.peek().clone();
        if p.kind == V1Tok::LBracket {
            self.advance();
            let inner = self.expect_ident("element type")?;
            self.expect(&V1Tok::RBracket, "']'")?;
            if inner == "trace" {
                return Ok(V1Type::TraceList);
            }
            return Err(Diagnostic::at(
                DiagnosticCode::TypeError,
                p.line,
                p.col,
                format!("unsupported list type [{inner}] in --compat v1"),
            ));
        }
        let tname = self.expect_ident("v1 type")?;
        V1Type::from_name(&tname).ok_or_else(|| {
            Diagnostic::at(
                DiagnosticCode::TypeError,
                p.line,
                p.col,
                format!("unknown type `{tname}` in --compat v1"),
            )
        })
    }

    fn peek_is_type(&self, offset: usize) -> bool {
        match &self.peek_n(offset).kind {
            V1Tok::Ident(s) => V1Type::from_name(s).is_some(),
            V1Tok::LBracket => {
                matches!(&self.peek_n(offset + 1).kind, V1Tok::Ident(s) if s == "trace")
                    && self.peek_n(offset + 2).kind == V1Tok::RBracket
            }
            _ => false,
        }
    }

    fn parse_program(&mut self, source: &str) -> DecResult<V1Program> {
        self.skip_newlines();
        let mut functions = Vec::new();
        while !self.check(&V1Tok::Eof) {
            if !self.check(&V1Tok::Dec) {
                let p = self.peek();
                return Err(Diagnostic::at(
                    DiagnosticCode::ParseError,
                    p.line,
                    p.col,
                    "Frozen Dec v1 allows only uppercase `Dec` declarations at top level",
                ));
            }
            functions.push(self.parse_dec_fn()?);
            self.skip_newlines();
        }
        if functions.is_empty() {
            return Err(Diagnostic::new(
                DiagnosticCode::ParseError,
                "Frozen Dec v1 program contains no `Dec` declarations",
            ));
        }
        Ok(V1Program {
            source: source.to_string(),
            source_sha256: Sha256::digest_hex(source.as_bytes()),
            functions,
        })
    }

    fn looks_like_branch_signature(&self) -> bool {
        if !self.check(&V1Tok::LParen) {
            return false;
        }
        let mut depth = 0usize;
        let mut idx = self.pos;
        while idx < self.tokens.len() {
            match self.tokens[idx].kind {
                V1Tok::LParen => depth += 1,
                V1Tok::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        return self
                            .tokens
                            .get(idx + 1)
                            .is_some_and(|t| t.kind == V1Tok::Arrow);
                    }
                }
                V1Tok::Newline | V1Tok::Eof => return false,
                _ => {}
            }
            idx += 1;
        }
        false
    }

    fn parse_dec_fn(&mut self) -> DecResult<V1Function> {
        let line = self.line();
        self.expect(&V1Tok::Dec, "'Dec'")?;
        let name = self.expect_ident("function name")?;
        if self.check(&V1Tok::Colon) {
            return Err(Diagnostic::at_line(
                DiagnosticCode::ParseError,
                line,
                "`Dec <name>:` without parameter header is not in --compat v1",
            ));
        }
        self.expect(&V1Tok::LParen, "'('")?;

        let mut bare_params = Vec::new();
        let mut typed_params = Vec::new();
        if !self.check(&V1Tok::RParen) {
            loop {
                if self.peek_n(1).kind == V1Tok::Colon {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        self.line(),
                        "colon-typed parameters `name: type` are v3 syntax; use `<type> <name>` in --compat v1",
                    ));
                }
                if self.peek_is_type(0)
                    && matches!(self.peek_n(1).kind, V1Tok::Ident(_))
                {
                    let ty = self.parse_type()?;
                    let pname = self.expect_ident("parameter name")?;
                    typed_params.push(V1Param { ty, name: pname });
                } else {
                    let pname = self.expect_ident("parameter name")?;
                    bare_params.push(pname);
                }
                if self.check(&V1Tok::Comma) {
                    self.advance();
                    if self.check(&V1Tok::RParen) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        self.expect(&V1Tok::RParen, "')'")?;
        if !bare_params.is_empty() && !typed_params.is_empty() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::ParseError,
                line,
                "cannot mix typed and untyped parameters in `Dec` header",
            ));
        }

        self.expect(&V1Tok::Arrow, "'->'")?;
        let header_ret_and_out: (Option<V1Type>, String) = if self.peek_is_type(0)
            && matches!(self.peek_n(1).kind, V1Tok::Ident(_))
        {
            let rty = self.parse_type()?;
            let out_var = self.expect_ident("output variable")?;
            (Some(rty), out_var)
        } else {
            let out_var = self.expect_ident("output variable")?;
            (None, out_var)
        };

        self.expect(&V1Tok::Colon, "':'")?;
        self.expect(&V1Tok::Newline, "newline after `Dec` header")?;
        self.expect(&V1Tok::Indent, "indented `Dec` body")?;
        self.skip_newlines();

        if let (Some(ret_type), out_var) = header_ret_and_out {
            if !bare_params.is_empty() {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    line,
                    "single-header `Dec` requires explicit parameter types `<type> <param>`",
                ));
            }
            let header_params = typed_params.iter().map(|p| p.name.clone()).collect();
            let body = self.parse_indented_stmts()?;
            let branch = V1Branch {
                params: typed_params,
                return_type: ret_type,
                output_name: out_var.clone(),
                body,
                line,
            };
            return Ok(V1Function {
                name,
                header_params,
                output_name: out_var,
                branches: vec![branch],
                line,
            });
        }

        let out_var = header_ret_and_out.1;
        if !typed_params.is_empty() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::ParseError,
                line,
                "multi-row `Dec` header must list untyped parameter names",
            ));
        }
        if !self.looks_like_branch_signature() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::ParseError,
                self.line(),
                "expected indented `(<type> <param>, ...) -> <type> <out_var>:` signature row in --compat v1",
            ));
        }

        let mut branches = Vec::new();
        while !self.check(&V1Tok::Dedent) && !self.check(&V1Tok::Eof) {
            branches.push(self.parse_branch()?);
            self.skip_newlines();
        }
        self.expect(&V1Tok::Dedent, "dedent after `Dec` branches")?;
        if branches.is_empty() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::ParseError,
                line,
                "`Dec` declaration has no signature branches",
            ));
        }
        Ok(V1Function {
            name,
            header_params: bare_params,
            output_name: out_var,
            branches,
            line,
        })
    }

    fn parse_branch(&mut self) -> DecResult<V1Branch> {
        let line = self.line();
        self.expect(&V1Tok::LParen, "'('")?;
        let mut params = Vec::new();
        if !self.check(&V1Tok::RParen) {
            loop {
                if self.peek_n(1).kind == V1Tok::Colon {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        self.line(),
                        "colon-typed parameters `name: type` are v3 syntax; use `<type> <name>` in --compat v1",
                    ));
                }
                let ty = self.parse_type()?;
                let name = self.expect_ident("parameter name")?;
                params.push(V1Param { ty, name });
                if self.check(&V1Tok::Comma) {
                    self.advance();
                    if self.check(&V1Tok::RParen) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        self.expect(&V1Tok::RParen, "')'")?;
        self.expect(&V1Tok::Arrow, "'->'")?;
        let return_type = self.parse_type()?;
        let output_name = self.expect_ident("output variable")?;
        self.expect(&V1Tok::Colon, "':'")?;
        self.expect(&V1Tok::Newline, "newline after branch header")?;
        self.expect(&V1Tok::Indent, "indented branch body")?;
        let body = self.parse_indented_stmts()?;
        Ok(V1Branch {
            params,
            return_type,
            output_name,
            body,
            line,
        })
    }

    fn parse_indented_stmts(&mut self) -> DecResult<Vec<V1Stmt>> {
        self.skip_newlines();
        let mut stmts = Vec::new();
        while !self.check(&V1Tok::Dedent) && !self.check(&V1Tok::Eof) {
            stmts.push(self.parse_stmt()?);
            self.skip_newlines();
        }
        let line = self.line();
        self.expect(&V1Tok::Dedent, "dedent")?;
        if stmts.is_empty() {
            return Err(Diagnostic::at_line(
                DiagnosticCode::ParseError,
                line,
                "indented block cannot be empty",
            ));
        }
        Ok(stmts)
    }

    fn expect_stmt_end(&mut self) -> DecResult<()> {
        if self.check(&V1Tok::Newline) {
            self.advance();
            Ok(())
        } else if self.check(&V1Tok::Dedent) || self.check(&V1Tok::Eof) {
            Ok(())
        } else {
            let p = self.peek();
            Err(Diagnostic::at(
                DiagnosticCode::ParseError,
                p.line,
                p.col,
                format!("expected end of statement, found {:?}", p.kind),
            ))
        }
    }

    fn parse_stmt(&mut self) -> DecResult<V1Stmt> {
        let line = self.line();
        match &self.peek().kind {
            V1Tok::Assert => {
                self.advance();
                let cond = self.parse_expr()?;
                self.expect_stmt_end()?;
                Ok(V1Stmt {
                    kind: V1StmtKind::Assert(cond),
                    line,
                })
            }
            V1Tok::Emit => {
                self.advance();
                let val = self.parse_expr()?;
                self.expect_stmt_end()?;
                Ok(V1Stmt {
                    kind: V1StmtKind::Emit(val),
                    line,
                })
            }
            V1Tok::Return => {
                self.advance();
                let val = if self.check(&V1Tok::Newline)
                    || self.check(&V1Tok::Dedent)
                    || self.check(&V1Tok::Eof)
                {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                self.expect_stmt_end()?;
                Ok(V1Stmt {
                    kind: V1StmtKind::Return(val),
                    line,
                })
            }
            V1Tok::If => {
                self.advance();
                let cond = self.parse_expr()?;
                self.expect(&V1Tok::Colon, "':' after `if` condition")?;
                self.expect(&V1Tok::Newline, "newline after `if`")?;
                self.expect(&V1Tok::Indent, "indented `if` body")?;
                let then_body = self.parse_indented_stmts()?;
                let mut branches = vec![(cond, then_body)];
                self.skip_newlines();
                while self.check(&V1Tok::Elif) {
                    self.advance();
                    let ec = self.parse_expr()?;
                    self.expect(&V1Tok::Colon, "':' after `elif` condition")?;
                    self.expect(&V1Tok::Newline, "newline after `elif`")?;
                    self.expect(&V1Tok::Indent, "indented `elif` body")?;
                    let eb = self.parse_indented_stmts()?;
                    branches.push((ec, eb));
                    self.skip_newlines();
                }
                let else_body = if self.check(&V1Tok::Else) {
                    self.advance();
                    self.expect(&V1Tok::Colon, "':' after `else`")?;
                    self.expect(&V1Tok::Newline, "newline after `else`")?;
                    self.expect(&V1Tok::Indent, "indented `else` body")?;
                    self.parse_indented_stmts()?
                } else {
                    Vec::new()
                };
                Ok(V1Stmt {
                    kind: V1StmtKind::If {
                        branches,
                        else_body,
                    },
                    line,
                })
            }
            V1Tok::Let | V1Tok::Mut => {
                let mut mutable = false;
                if self.check(&V1Tok::Let) {
                    self.advance();
                    if self.check(&V1Tok::Mut) {
                        self.advance();
                        mutable = true;
                    }
                } else {
                    self.advance();
                    mutable = true;
                }
                let (ty, name) = if self.peek_is_type(0)
                    && matches!(
                        self.peek_n(1).kind,
                        V1Tok::Ident(_) | V1Tok::RBracket
                    )
                {
                    let parsed_ty = self.parse_type()?;
                    let vname = self.expect_ident("local variable name")?;
                    (Some(parsed_ty), vname)
                } else {
                    let vname = self.expect_ident("local variable name")?;
                    if self.check(&V1Tok::Colon) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ParseError,
                            line,
                            "colon type annotation `let x: T` is v3 syntax and forbidden in --compat v1",
                        ));
                    }
                    (None, vname)
                };
                self.expect(&V1Tok::Eq, "'='")?;
                let value = self.parse_expr()?;
                self.expect_stmt_end()?;
                Ok(V1Stmt {
                    kind: V1StmtKind::LocalDecl {
                        mutable,
                        ty,
                        name,
                        value,
                    },
                    line,
                })
            }
            _ => {
                if self.peek_is_type(0) {
                    let ty_tok_span = if self.check(&V1Tok::LBracket) { 3 } else { 1 };
                    if matches!(self.peek_n(ty_tok_span).kind, V1Tok::Ident(_))
                        && self.peek_n(ty_tok_span + 1).kind == V1Tok::Eq
                    {
                        let ty = self.parse_type()?;
                        let name = self.expect_ident("local variable name")?;
                        self.expect(&V1Tok::Eq, "'='")?;
                        let value = self.parse_expr()?;
                        self.expect_stmt_end()?;
                        return Ok(V1Stmt {
                            kind: V1StmtKind::LocalDecl {
                                mutable: true,
                                ty: Some(ty),
                                name,
                                value,
                            },
                            line,
                        });
                    }
                }
                if matches!(self.peek().kind, V1Tok::Ident(_))
                    && self.peek_n(1).kind == V1Tok::Eq
                {
                    let name = self.expect_ident("assignment target")?;
                    self.expect(&V1Tok::Eq, "'='")?;
                    let value = self.parse_expr()?;
                    self.expect_stmt_end()?;
                    return Ok(V1Stmt {
                        kind: V1StmtKind::Assign { name, value },
                        line,
                    });
                }
                Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    line,
                    "unsupported statement in --compat v1; expected typed local, assignment, assert, emit, if, or return",
                ))
            }
        }
    }

    fn parse_expr(&mut self) -> DecResult<V1Expr> {
        self.parse_cond_expr()
    }

    fn parse_cond_expr(&mut self) -> DecResult<V1Expr> {
        let then_expr = self.parse_or_expr()?;
        if self.check(&V1Tok::If) {
            let line = self.line();
            self.advance();
            let cond = self.parse_or_expr()?;
            self.expect(&V1Tok::Else, "'else' in conditional expression")?;
            let else_expr = self.parse_cond_expr()?;
            Ok(V1Expr {
                kind: V1ExprKind::Conditional {
                    then_expr: Box::new(then_expr),
                    cond: Box::new(cond),
                    else_expr: Box::new(else_expr),
                },
                line,
            })
        } else {
            Ok(then_expr)
        }
    }

    fn parse_or_expr(&mut self) -> DecResult<V1Expr> {
        let mut expr = self.parse_and_expr()?;
        while self.check(&V1Tok::Or) {
            let line = self.line();
            self.advance();
            let rhs = self.parse_and_expr()?;
            expr = V1Expr {
                kind: V1ExprKind::Binary {
                    op: V1BinaryOp::Or,
                    left: Box::new(expr),
                    right: Box::new(rhs),
                },
                line,
            };
        }
        Ok(expr)
    }

    fn parse_and_expr(&mut self) -> DecResult<V1Expr> {
        let mut expr = self.parse_not_expr()?;
        while self.check(&V1Tok::And) {
            let line = self.line();
            self.advance();
            let rhs = self.parse_not_expr()?;
            expr = V1Expr {
                kind: V1ExprKind::Binary {
                    op: V1BinaryOp::And,
                    left: Box::new(expr),
                    right: Box::new(rhs),
                },
                line,
            };
        }
        Ok(expr)
    }

    fn parse_not_expr(&mut self) -> DecResult<V1Expr> {
        if self.check(&V1Tok::Not) {
            let line = self.line();
            self.advance();
            let operand = self.parse_not_expr()?;
            Ok(V1Expr {
                kind: V1ExprKind::Unary {
                    op: V1UnaryOp::Not,
                    operand: Box::new(operand),
                },
                line,
            })
        } else {
            self.parse_cmp_expr()
        }
    }

    fn parse_cmp_expr(&mut self) -> DecResult<V1Expr> {
        let left = self.parse_add_expr()?;
        let op = match self.peek().kind {
            V1Tok::EqEq => Some(V1BinaryOp::Eq),
            V1Tok::NotEq => Some(V1BinaryOp::NotEq),
            V1Tok::Lt => Some(V1BinaryOp::Lt),
            V1Tok::LtEq => Some(V1BinaryOp::LtEq),
            V1Tok::Gt => Some(V1BinaryOp::Gt),
            V1Tok::GtEq => Some(V1BinaryOp::GtEq),
            _ => None,
        };
        if let Some(bop) = op {
            let line = self.line();
            self.advance();
            let right = self.parse_add_expr()?;
            Ok(V1Expr {
                kind: V1ExprKind::Binary {
                    op: bop,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                line,
            })
        } else {
            Ok(left)
        }
    }

    fn parse_add_expr(&mut self) -> DecResult<V1Expr> {
        let mut expr = self.parse_mul_expr()?;
        loop {
            let op = match self.peek().kind {
                V1Tok::Plus => V1BinaryOp::Add,
                V1Tok::Minus => V1BinaryOp::Sub,
                _ => break,
            };
            let line = self.line();
            self.advance();
            let rhs = self.parse_mul_expr()?;
            expr = V1Expr {
                kind: V1ExprKind::Binary {
                    op,
                    left: Box::new(expr),
                    right: Box::new(rhs),
                },
                line,
            };
        }
        Ok(expr)
    }

    fn parse_mul_expr(&mut self) -> DecResult<V1Expr> {
        let mut expr = self.parse_unary_expr()?;
        loop {
            let op = match self.peek().kind {
                V1Tok::Star => V1BinaryOp::Mul,
                V1Tok::Slash => V1BinaryOp::Div,
                V1Tok::SlashSlash => V1BinaryOp::FloorDiv,
                V1Tok::Percent => V1BinaryOp::Mod,
                _ => break,
            };
            let line = self.line();
            self.advance();
            let rhs = self.parse_unary_expr()?;
            expr = V1Expr {
                kind: V1ExprKind::Binary {
                    op,
                    left: Box::new(expr),
                    right: Box::new(rhs),
                },
                line,
            };
        }
        Ok(expr)
    }

    fn parse_unary_expr(&mut self) -> DecResult<V1Expr> {
        let line = self.line();
        match self.peek().kind {
            V1Tok::Minus => {
                self.advance();
                let operand = self.parse_unary_expr()?;
                Ok(V1Expr {
                    kind: V1ExprKind::Unary {
                        op: V1UnaryOp::Neg,
                        operand: Box::new(operand),
                    },
                    line,
                })
            }
            V1Tok::Plus => {
                self.advance();
                let operand = self.parse_unary_expr()?;
                Ok(V1Expr {
                    kind: V1ExprKind::Unary {
                        op: V1UnaryOp::Pos,
                        operand: Box::new(operand),
                    },
                    line,
                })
            }
            _ => self.parse_postfix_expr(),
        }
    }

    fn parse_postfix_expr(&mut self) -> DecResult<V1Expr> {
        let mut expr = self.parse_primary_expr()?;
        loop {
            if self.check(&V1Tok::LParen) {
                let line = self.line();
                let V1ExprKind::Ident(callee_name) = expr.kind else {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "call target must be a function or builtin identifier",
                    ));
                };
                self.advance();
                let mut args = Vec::new();
                if !self.check(&V1Tok::RParen) {
                    loop {
                        args.push(self.parse_expr()?);
                        if self.check(&V1Tok::Comma) {
                            self.advance();
                            if self.check(&V1Tok::RParen) {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                self.expect(&V1Tok::RParen, "')'")?;
                expr = V1Expr {
                    kind: V1ExprKind::Call {
                        name: callee_name,
                        args,
                    },
                    line,
                };
            } else if self.check(&V1Tok::LBracket) {
                let line = self.line();
                self.advance();
                let index = self.parse_expr()?;
                self.expect(&V1Tok::RBracket, "']'")?;
                expr = V1Expr {
                    kind: V1ExprKind::Index {
                        base: Box::new(expr),
                        index: Box::new(index),
                    },
                    line,
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_primary_expr(&mut self) -> DecResult<V1Expr> {
        let p = self.peek().clone();
        match p.kind {
            V1Tok::IntLit(n) => {
                self.advance();
                Ok(V1Expr {
                    kind: V1ExprKind::IntLit(n),
                    line: p.line,
                })
            }
            V1Tok::True => {
                self.advance();
                Ok(V1Expr {
                    kind: V1ExprKind::BoolLit(true),
                    line: p.line,
                })
            }
            V1Tok::False => {
                self.advance();
                Ok(V1Expr {
                    kind: V1ExprKind::BoolLit(false),
                    line: p.line,
                })
            }
            V1Tok::StrLit(s) => {
                self.advance();
                Ok(V1Expr {
                    kind: V1ExprKind::StrLit(s),
                    line: p.line,
                })
            }
            V1Tok::Ident(name) => {
                self.advance();
                Ok(V1Expr {
                    kind: V1ExprKind::Ident(name),
                    line: p.line,
                })
            }
            V1Tok::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(&V1Tok::RParen, "')'")?;
                Ok(inner)
            }
            _ => Err(Diagnostic::at(
                DiagnosticCode::ParseError,
                p.line,
                p.col,
                format!("unexpected token {:?} in expression", p.kind),
            )),
        }
    }
}

// ============================================================================
// Frozen Dec v1 Static Typechecker
// ============================================================================

fn v1_builtin_sigs(name: &str) -> Vec<(Vec<V1Type>, V1Type)> {
    use V1Type::*;
    match name {
        "t1" => vec![(vec![Int], T1)],
        "t32" => vec![(vec![Int], T32)],
        "q4" => vec![(vec![Int], Q4)],
        "int" => vec![
            (vec![Int], Int),
            (vec![T1], Int),
            (vec![T32], Int),
            (vec![Q4], Int),
            (vec![Ratio], Int),
        ],
        "ratio" => vec![(vec![Int, Int], Ratio), (vec![Int], Ratio)],
        "num" | "den" => vec![(vec![Ratio], Int)],
        "pack" => vec![(vec![T32], Int)],
        "unpack" => vec![(vec![Int], T32)],
        "neg" | "abs" => vec![
            (vec![Int], Int),
            (vec![T1], T1),
            (vec![T32], T32),
            (vec![Ratio], Ratio),
        ],
        "is_hamming1" => vec![(vec![Q4, Q4], Bool)],
        "popcnt_xor" => vec![(vec![Q4, Q4], Int)],
        "trace" => vec![(vec![Str, Str, Q4], Trace)],
        "step" => vec![(vec![Trace, Q4, T1], Trace)],
        "hinge" | "dwell" | "close" => vec![(vec![Trace], Trace)],
        "phases" | "beats" => vec![(vec![Trace], Int)],
        "rate" => vec![(vec![Trace], Ratio)],
        "fold" => vec![(vec![Trace, Trace, Str, Str], Fusion)],
        "outer" => vec![(vec![Fusion, Q4], Trace)],
        "fuse" => vec![(vec![Fusion, Trace], Trace)],
        "expose_inner" => vec![(vec![Trace], TraceList)],
        "coupling" => vec![(vec![Trace, Trace, Str], Coupling)],
        "contact" => vec![(vec![Coupling, Int, Int], Coupling)],
        "cadence" => vec![(vec![Coupling], Ratio)],
        "lag" | "drift" => vec![(vec![Coupling], Int)],
        "probability" => vec![(vec![Int, Int, Int, T1], Ratio)],
        "len" => vec![(vec![Str], Int), (vec![TraceList], Int)],
        _ => Vec::new(),
    }
}

fn is_v1_builtin(name: &str) -> bool {
    !v1_builtin_sigs(name).is_empty()
}

#[derive(Clone, Debug)]
struct VarInfo {
    ty: V1Type,
    mutable: bool,
    initialized: bool,
}

#[derive(Clone, Copy, Debug)]
struct BlockOutcome {
    definitely_returns: bool,
    out_initialized: bool,
}

struct V1Checker {
    fn_sigs: BTreeMap<String, Vec<(Vec<V1Type>, V1Type)>>,
}

impl V1Checker {
    fn check_program(prog: &V1Program) -> DecResult<()> {
        let mut fn_sigs: BTreeMap<String, Vec<(Vec<V1Type>, V1Type)>> = BTreeMap::new();

        for func in &prog.functions {
            if is_v1_builtin(&func.name) {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    func.line,
                    format!("cannot redefine builtin `{}`", func.name),
                ));
            }
            if fn_sigs.contains_key(&func.name) {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    func.line,
                    format!("duplicate `Dec {}` declaration; place overloads in one `Dec` block", func.name),
                ));
            }

            let mut uniq_hdr = BTreeSet::new();
            for hp in &func.header_params {
                if !uniq_hdr.insert(hp.as_str()) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        func.line,
                        format!("duplicate parameter `{hp}` in `Dec {}` header", func.name),
                    ));
                }
                if hp == &func.output_name {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        func.line,
                        format!(
                            "output variable `{}` must differ from parameter names",
                            func.output_name
                        ),
                    ));
                }
            }

            let mut seen_param_tuples: BTreeSet<Vec<V1Type>> = BTreeSet::new();
            let mut overloads = Vec::with_capacity(func.branches.len());
            for br in &func.branches {
                if br.params.len() != func.header_params.len() {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        br.line,
                        format!(
                            "branch parameter count ({}) does not match `Dec {}` header ({})",
                            br.params.len(),
                            func.name,
                            func.header_params.len()
                        ),
                    ));
                }
                if br.output_name != func.output_name {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        br.line,
                        format!(
                            "branch output binding `{}` must match header output `{}`",
                            br.output_name, func.output_name
                        ),
                    ));
                }
                let mut ptys = Vec::with_capacity(br.params.len());
                for (idx, p) in br.params.iter().enumerate() {
                    if p.name != func.header_params[idx] {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            br.line,
                            format!(
                                "branch parameter `{}` must match header parameter `{}`",
                                p.name, func.header_params[idx]
                            ),
                        ));
                    }
                    ptys.push(p.ty.clone());
                }
                if !seen_param_tuples.insert(ptys.clone()) {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        br.line,
                        format!("duplicate overload signature on `Dec {}`", func.name),
                    ));
                }
                overloads.push((ptys, br.return_type.clone()));
            }
            fn_sigs.insert(func.name.clone(), overloads);
        }

        match fn_sigs.get("main") {
            Some(sigs) if sigs.iter().any(|(ps, _)| ps.is_empty()) => {}
            Some(_) => {
                return Err(Diagnostic::new(
                    DiagnosticCode::TypeError,
                    "`Dec main` must accept zero parameters `()`",
                ));
            }
            None => {
                return Err(Diagnostic::new(
                    DiagnosticCode::TypeError,
                    "missing required entrypoint `Dec main() -> ...`",
                ));
            }
        }

        let checker = Self { fn_sigs };
        for func in &prog.functions {
            for br in &func.branches {
                checker.check_branch(func, br)?;
            }
        }
        Ok(())
    }

    fn check_branch(&self, func: &V1Function, br: &V1Branch) -> DecResult<()> {
        let mut scope: BTreeMap<String, VarInfo> = BTreeMap::new();
        for p in &br.params {
            scope.insert(
                p.name.clone(),
                VarInfo {
                    ty: p.ty.clone(),
                    mutable: true,
                    initialized: true,
                },
            );
        }
        scope.insert(
            br.output_name.clone(),
            VarInfo {
                ty: br.return_type.clone(),
                mutable: true,
                initialized: false,
            },
        );

        let outcome =
            self.check_block(&br.body, &mut scope, &br.output_name, &br.return_type)?;
        if !outcome.definitely_returns && !outcome.out_initialized {
            return Err(Diagnostic::at_line(
                DiagnosticCode::TypeError,
                br.line,
                format!(
                    "output variable `{}` in `Dec {}` is not assigned on all control-flow paths",
                    br.output_name, func.name
                ),
            ));
        }
        Ok(())
    }

    fn check_block(
        &self,
        stmts: &[V1Stmt],
        scope: &mut BTreeMap<String, VarInfo>,
        out_name: &str,
        ret_type: &V1Type,
    ) -> DecResult<BlockOutcome> {
        let mut definitely_returns = false;
        for stmt in stmts {
            if definitely_returns {
                return Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    stmt.line,
                    "unreachable statement after `return`",
                ));
            }
            match &stmt.kind {
                V1StmtKind::LocalDecl {
                    mutable,
                    ty,
                    name,
                    value,
                } => {
                    if scope.contains_key(name) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            stmt.line,
                            format!("duplicate local variable declaration `{name}`"),
                        ));
                    }
                    let val_ty = self.check_expr(value, scope)?;
                    let resolved_ty = match ty {
                        Some(expected) => {
                            if &val_ty != expected {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    stmt.line,
                                    format!(
                                        "local `{name}` declared as `{expected}`, got `{val_ty}`"
                                    ),
                                ));
                            }
                            expected.clone()
                        }
                        None => val_ty,
                    };
                    scope.insert(
                        name.clone(),
                        VarInfo {
                            ty: resolved_ty,
                            mutable: *mutable,
                            initialized: true,
                        },
                    );
                }
                V1StmtKind::Assign { name, value } => {
                    let val_ty = self.check_expr(value, scope)?;
                    let info = scope.get_mut(name).ok_or_else(|| {
                        Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            stmt.line,
                            format!("undeclared variable `{name}`; declare locals before assignment"),
                        )
                    })?;
                    if !info.mutable {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            stmt.line,
                            format!("cannot reassign immutable `let` binding `{name}`"),
                        ));
                    }
                    if info.ty != val_ty {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            stmt.line,
                            format!(
                                "assignment to `{name}` expects `{}`, got `{val_ty}`",
                                info.ty
                            ),
                        ));
                    }
                    info.initialized = true;
                }
                V1StmtKind::Assert(cond) => {
                    let cty = self.check_expr(cond, scope)?;
                    if cty != V1Type::Bool {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            stmt.line,
                            format!("`assert` requires `bool`, got `{cty}`"),
                        ));
                    }
                }
                V1StmtKind::Emit(val) => {
                    let _ = self.check_expr(val, scope)?;
                }
                V1StmtKind::Return(opt_expr) => {
                    match opt_expr {
                        Some(expr) => {
                            let rty = self.check_expr(expr, scope)?;
                            if &rty != ret_type {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    stmt.line,
                                    format!(
                                        "`return` expects `{ret_type}`, got `{rty}`"
                                    ),
                                ));
                            }
                        }
                        None => {
                            let out_ok = scope
                                .get(out_name)
                                .is_some_and(|info| info.initialized);
                            if !out_ok {
                                return Err(Diagnostic::at_line(
                                    DiagnosticCode::TypeError,
                                    stmt.line,
                                    format!(
                                        "bare `return` before output variable `{out_name}` is assigned"
                                    ),
                                ));
                            }
                        }
                    }
                    definitely_returns = true;
                }
                V1StmtKind::If {
                    branches,
                    else_body,
                } => {
                    let prior_out_init = scope
                        .get(out_name)
                        .is_some_and(|info| info.initialized);
                    let mut all_return = !else_body.is_empty();
                    let mut all_init_or_return = !else_body.is_empty();

                    for (cond, body) in branches {
                        let cty = self.check_expr(cond, scope)?;
                        if cty != V1Type::Bool {
                            return Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                cond.line,
                                format!("`if` condition requires `bool`, got `{cty}`"),
                            ));
                        }
                        let mut child_scope = scope.clone();
                        let bo = self.check_block(body, &mut child_scope, out_name, ret_type)?;
                        if !bo.definitely_returns {
                            all_return = false;
                            if !bo.out_initialized {
                                all_init_or_return = false;
                            }
                        }
                    }

                    if !else_body.is_empty() {
                        let mut child_scope = scope.clone();
                        let bo =
                            self.check_block(else_body, &mut child_scope, out_name, ret_type)?;
                        if !bo.definitely_returns {
                            all_return = false;
                            if !bo.out_initialized {
                                all_init_or_return = false;
                            }
                        }
                    }

                    if prior_out_init || all_init_or_return {
                        if let Some(out_info) = scope.get_mut(out_name) {
                            out_info.initialized = true;
                        }
                    }
                    if all_return {
                        definitely_returns = true;
                    }
                }
            }
        }

        let out_initialized = scope
            .get(out_name)
            .is_some_and(|info| info.initialized);
        Ok(BlockOutcome {
            definitely_returns,
            out_initialized,
        })
    }

    fn check_expr(
        &self,
        expr: &V1Expr,
        scope: &BTreeMap<String, VarInfo>,
    ) -> DecResult<V1Type> {
        match &expr.kind {
            V1ExprKind::IntLit(_) => Ok(V1Type::Int),
            V1ExprKind::BoolLit(_) => Ok(V1Type::Bool),
            V1ExprKind::StrLit(_) => Ok(V1Type::Str),
            V1ExprKind::Ident(name) => {
                let info = scope.get(name).ok_or_else(|| {
                    Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr.line,
                        format!("unknown identifier `{name}`"),
                    )
                })?;
                if !info.initialized {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr.line,
                        format!("output variable `{name}` read before assignment"),
                    ));
                }
                Ok(info.ty.clone())
            }
            V1ExprKind::Unary { op, operand } => {
                let oty = self.check_expr(operand, scope)?;
                match op {
                    V1UnaryOp::Neg | V1UnaryOp::Pos => match oty {
                        V1Type::Int | V1Type::T1 | V1Type::T32 | V1Type::Ratio => Ok(oty),
                        other => Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            expr.line,
                            format!("unary sign operator requires numeric type, got `{other}`"),
                        )),
                    },
                    V1UnaryOp::Not => {
                        if oty == V1Type::Bool {
                            Ok(V1Type::Bool)
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr.line,
                                format!("`not` requires `bool`, got `{oty}`"),
                            ))
                        }
                    }
                }
            }
            V1ExprKind::Binary { op, left, right } => {
                let lty = self.check_expr(left, scope)?;
                let rty = self.check_expr(right, scope)?;
                if lty != rty {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr.line,
                        format!("binary operator requires matching types, got `{lty}` and `{rty}`"),
                    ));
                }
                match op {
                    V1BinaryOp::And | V1BinaryOp::Or => {
                        if lty == V1Type::Bool {
                            Ok(V1Type::Bool)
                        } else {
                            Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr.line,
                                format!("logical operator requires `bool`, got `{lty}`"),
                            ))
                        }
                    }
                    V1BinaryOp::Eq | V1BinaryOp::NotEq => Ok(V1Type::Bool),
                    V1BinaryOp::Lt | V1BinaryOp::LtEq | V1BinaryOp::Gt | V1BinaryOp::GtEq => {
                        match lty {
                            V1Type::Int
                            | V1Type::T1
                            | V1Type::T32
                            | V1Type::Q4
                            | V1Type::Ratio
                            | V1Type::Str => Ok(V1Type::Bool),
                            other => Err(Diagnostic::at_line(
                                DiagnosticCode::TypeError,
                                expr.line,
                                format!("ordering comparison not supported on `{other}`"),
                            )),
                        }
                    }
                    V1BinaryOp::Add => match lty {
                        V1Type::Int | V1Type::T1 | V1Type::T32 | V1Type::Ratio | V1Type::Str => {
                            Ok(lty)
                        }
                        other => Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            expr.line,
                            format!("`+` not supported on `{other}`"),
                        )),
                    },
                    V1BinaryOp::Sub | V1BinaryOp::Mul => match lty {
                        V1Type::Int | V1Type::T1 | V1Type::T32 | V1Type::Ratio => Ok(lty),
                        other => Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            expr.line,
                            format!("arithmetic operator not supported on `{other}`"),
                        )),
                    },
                    V1BinaryOp::Div => match lty {
                        V1Type::Int | V1Type::T1 | V1Type::T32 | V1Type::Ratio => Ok(V1Type::Ratio),
                        other => Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            expr.line,
                            format!("exact `/` not supported on `{other}`"),
                        )),
                    },
                    V1BinaryOp::FloorDiv | V1BinaryOp::Mod => match lty {
                        V1Type::Int | V1Type::T32 => Ok(lty),
                        other => Err(Diagnostic::at_line(
                            DiagnosticCode::TypeError,
                            expr.line,
                            format!("floor division / remainder requires `int` or `t32`, got `{other}`"),
                        )),
                    },
                }
            }
            V1ExprKind::Conditional {
                then_expr,
                cond,
                else_expr,
            } => {
                let cty = self.check_expr(cond, scope)?;
                if cty != V1Type::Bool {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        cond.line,
                        format!("conditional expression predicate must be `bool`, got `{cty}`"),
                    ));
                }
                let tty = self.check_expr(then_expr, scope)?;
                let ety = self.check_expr(else_expr, scope)?;
                if tty != ety {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr.line,
                        format!(
                            "conditional expression branches must have identical types, got `{tty}` and `{ety}`"
                        ),
                    ));
                }
                Ok(tty)
            }
            V1ExprKind::Call { name, args } => {
                let mut arg_tys = Vec::with_capacity(args.len());
                for a in args {
                    arg_tys.push(self.check_expr(a, scope)?);
                }
                let sigs = if is_v1_builtin(name) {
                    v1_builtin_sigs(name)
                } else if let Some(user_sigs) = self.fn_sigs.get(name) {
                    user_sigs.clone()
                } else {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr.line,
                        format!("unknown function or builtin `{name}` in --compat v1"),
                    ));
                };
                for (ptys, rty) in &sigs {
                    if ptys == &arg_tys {
                        return Ok(rty.clone());
                    }
                }
                Err(Diagnostic::at_line(
                    DiagnosticCode::TypeError,
                    expr.line,
                    format!(
                        "no matching signature for `{name}({})`",
                        arg_tys
                            .iter()
                            .map(V1Type::as_str)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ))
            }
            V1ExprKind::Index { base, index } => {
                let bty = self.check_expr(base, scope)?;
                let ity = self.check_expr(index, scope)?;
                if ity != V1Type::Int {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr.line,
                        format!("index must be `int`, got `{ity}`"),
                    ));
                }
                match bty {
                    V1Type::TraceList => Ok(V1Type::Trace),
                    other => Err(Diagnostic::at_line(
                        DiagnosticCode::TypeError,
                        expr.line,
                        format!("cannot index into `{other}`"),
                    )),
                }
            }
        }
    }
}

// ============================================================================
// Deterministic Evaluator
// ============================================================================

#[derive(Clone, Debug, Eq, PartialEq)]
enum V1Value {
    Int(BigInt),
    Bool(bool),
    Str(String),
    T1(T1),
    T32(V1T32),
    Q4(Q4),
    Ratio(Ratio),
    Trace(Box<Trace>),
    Fusion(Box<Fusion>),
    Coupling(Box<Coupling>),
    TraceList(Vec<Trace>),
}

impl V1Value {
    fn type_of(&self) -> V1Type {
        match self {
            Self::Int(_) => V1Type::Int,
            Self::Bool(_) => V1Type::Bool,
            Self::Str(_) => V1Type::Str,
            Self::T1(_) => V1Type::T1,
            Self::T32(_) => V1Type::T32,
            Self::Q4(_) => V1Type::Q4,
            Self::Ratio(_) => V1Type::Ratio,
            Self::Trace(_) => V1Type::Trace,
            Self::Fusion(_) => V1Type::Fusion,
            Self::Coupling(_) => V1Type::Coupling,
            Self::TraceList(_) => V1Type::TraceList,
        }
    }

    fn render_v1(&self) -> String {
        match self {
            Self::Str(s) => s.clone(),
            Self::Int(n) => n.to_decimal_string(),
            Self::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            Self::T1(t) => t.render_v1(),
            Self::T32(v) => v.render_v1(),
            Self::Q4(q) => q.render_v1(),
            Self::Ratio(r) => r.render_v1(),
            Self::Trace(t) => t.render_v1(),
            Self::Fusion(f) => f.render_v1(),
            Self::Coupling(c) => c.render_v1(),
            Self::TraceList(ts) => {
                let inner: Vec<String> = ts.iter().map(Trace::render_v1).collect();
                format!("[{}]", inner.join(", "))
            }
        }
    }

    fn into_eval_output(self, mut emits: Vec<String>) -> EvalOutput {
        let result = match self {
            Self::Int(n) => Value::Int(n),
            Self::Bool(b) => Value::Bool(b),
            Self::T1(t) => Value::T1(t),
            Self::Q4(q) => Value::Q4(q),
            Self::Ratio(r) => Value::Ratio(r),
            Self::Trace(t) => Value::Trace(t),
            Self::Fusion(f) => Value::Fusion(f),
            Self::Coupling(c) => Value::Coupling(c),
            Self::TraceList(ts) => Value::List(
                ts.into_iter()
                    .map(|t| Value::Trace(Box::new(t)))
                    .collect(),
            ),
            // `Value` in Clean-Break v3 has neither `T32` nor unquoted `Str`; pushing the exact
            // v1 rendering onto `emits` with `Value::Unit` preserves 100% byte-identical stdout
            // across `EvalOutput::to_stdout_string()` and CLI `flush_emits_and_result`.
            Self::T32(v) => {
                emits.push(v.render_v1());
                Value::Unit
            }
            Self::Str(s) => {
                emits.push(s);
                Value::Unit
            }
        };
        EvalOutput { emits, result }
    }
}

enum V1Flow {
    Continue,
    Return(V1Value),
}

fn core_err(line: usize, e: CoreError) -> Diagnostic {
    Diagnostic::at_line(DiagnosticCode::RuntimeError, line, e.to_string())
}

fn rt_err(line: usize, msg: impl Into<String>) -> Diagnostic {
    Diagnostic::at_line(DiagnosticCode::RuntimeError, line, msg)
}

struct V1Evaluator<'a> {
    functions: BTreeMap<&'a str, &'a V1Function>,
    fuel: usize,
    max_depth: usize,
    depth: usize,
    emits: Vec<String>,
}

impl<'a> V1Evaluator<'a> {
    fn new(prog: &'a V1Program, fuel: usize, max_depth: usize) -> Self {
        let mut functions = BTreeMap::new();
        for f in &prog.functions {
            functions.insert(f.name.as_str(), f);
        }
        Self {
            functions,
            fuel,
            max_depth,
            depth: 0,
            emits: Vec::new(),
        }
    }

    #[inline]
    fn tick(&mut self, line: usize) -> DecResult<()> {
        if self.fuel == 0 {
            return Err(Diagnostic::at_line(
                DiagnosticCode::BudgetExhausted,
                line,
                "evaluation fuel exhausted",
            ));
        }
        self.fuel -= 1;
        Ok(())
    }

    fn run_main(mut self) -> DecResult<EvalOutput> {
        let main_fn = self.functions.get("main").copied().ok_or_else(|| {
            Diagnostic::new(DiagnosticCode::RuntimeError, "missing `Dec main`")
        })?;
        let ret = self.call_user_fn(main_fn, Vec::new(), main_fn.line)?;
        Ok(ret.into_eval_output(self.emits))
    }

    fn run_main_capturing(mut self) -> (Vec<String>, DecResult<Value>) {
        let Some(main_fn) = self.functions.get("main").copied() else {
            return (
                self.emits,
                Err(Diagnostic::new(
                    DiagnosticCode::RuntimeError,
                    "missing `Dec main`",
                )),
            );
        };
        match self.call_user_fn(main_fn, Vec::new(), main_fn.line) {
            Ok(ret) => {
                let out = ret.into_eval_output(self.emits);
                (out.emits, Ok(out.result))
            }
            Err(diag) => (self.emits, Err(diag)),
        }
    }

    fn call_user_fn(
        &mut self,
        func: &'a V1Function,
        args: Vec<V1Value>,
        call_line: usize,
    ) -> DecResult<V1Value> {
        self.tick(call_line)?;
        if self.depth >= self.max_depth {
            return Err(Diagnostic::at_line(
                DiagnosticCode::BudgetExhausted,
                call_line,
                "maximum call recursion depth exceeded",
            ));
        }
        let arg_tys: Vec<V1Type> = args.iter().map(V1Value::type_of).collect();
        let branch = func
            .branches
            .iter()
            .find(|br| {
                br.params.len() == arg_tys.len()
                    && br.params.iter().zip(&arg_tys).all(|(p, t)| &p.ty == t)
            })
            .ok_or_else(|| {
                rt_err(
                    call_line,
                    format!("no matching branch on `Dec {}`", func.name),
                )
            })?;

        self.depth += 1;
        let mut env: BTreeMap<String, Option<V1Value>> = BTreeMap::new();
        for (param, val) in branch.params.iter().zip(args) {
            env.insert(param.name.clone(), Some(val));
        }
        env.insert(branch.output_name.clone(), None);

        let exec_res = self.eval_block(&branch.body, &mut env, &branch.output_name);
        self.depth -= 1;

        match exec_res? {
            V1Flow::Return(v) => Ok(v),
            V1Flow::Continue => env
                .remove(&branch.output_name)
                .flatten()
                .ok_or_else(|| {
                    rt_err(
                        branch.line,
                        format!(
                            "output variable `{}` was not assigned in `Dec {}`",
                            branch.output_name, func.name
                        ),
                    )
                }),
        }
    }

    fn eval_block(
        &mut self,
        stmts: &'a [V1Stmt],
        env: &mut BTreeMap<String, Option<V1Value>>,
        out_name: &str,
    ) -> DecResult<V1Flow> {
        for stmt in stmts {
            self.tick(stmt.line)?;
            match &stmt.kind {
                V1StmtKind::LocalDecl { name, value, .. } => {
                    let v = self.eval_expr(value, env)?;
                    env.insert(name.clone(), Some(v));
                }
                V1StmtKind::Assign { name, value } => {
                    let v = self.eval_expr(value, env)?;
                    if let Some(slot) = env.get_mut(name) {
                        *slot = Some(v);
                    } else {
                        return Err(rt_err(stmt.line, format!("undeclared variable `{name}`")));
                    }
                }
                V1StmtKind::Assert(cond) => {
                    let v = self.eval_expr(cond, env)?;
                    if v != V1Value::Bool(true) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::AssertionFailed,
                            stmt.line,
                            "assertion failed",
                        ));
                    }
                }
                V1StmtKind::Emit(expr) => {
                    let v = self.eval_expr(expr, env)?;
                    self.emits.push(v.render_v1());
                }
                V1StmtKind::Return(opt_expr) => {
                    let v = match opt_expr {
                        Some(e) => self.eval_expr(e, env)?,
                        None => env
                            .get(out_name)
                            .and_then(Option::clone)
                            .ok_or_else(|| {
                                rt_err(
                                    stmt.line,
                                    format!("output variable `{out_name}` unassigned at `return`"),
                                )
                            })?,
                    };
                    return Ok(V1Flow::Return(v));
                }
                V1StmtKind::If {
                    branches,
                    else_body,
                } => {
                    let outer_keys: BTreeSet<String> = env.keys().cloned().collect();
                    let mut matched_branch = false;
                    for (cond, body) in branches {
                        let cv = self.eval_expr(cond, env)?;
                        if cv == V1Value::Bool(true) {
                            matched_branch = true;
                            let flow = self.eval_block(body, env, out_name)?;
                            env.retain(|k, _| outer_keys.contains(k));
                            if matches!(flow, V1Flow::Return(_)) {
                                return Ok(flow);
                            }
                            break;
                        }
                    }
                    if !matched_branch && !else_body.is_empty() {
                        let flow = self.eval_block(else_body, env, out_name)?;
                        env.retain(|k, _| outer_keys.contains(k));
                        if matches!(flow, V1Flow::Return(_)) {
                            return Ok(flow);
                        }
                    }
                }
            }
        }
        Ok(V1Flow::Continue)
    }

    fn eval_expr(
        &mut self,
        expr: &'a V1Expr,
        env: &BTreeMap<String, Option<V1Value>>,
    ) -> DecResult<V1Value> {
        self.tick(expr.line)?;
        match &expr.kind {
            V1ExprKind::IntLit(n) => Ok(V1Value::Int(n.clone())),
            V1ExprKind::BoolLit(b) => Ok(V1Value::Bool(*b)),
            V1ExprKind::StrLit(s) => Ok(V1Value::Str(s.clone())),
            V1ExprKind::Ident(name) => env
                .get(name)
                .and_then(Option::clone)
                .ok_or_else(|| rt_err(expr.line, format!("variable `{name}` read before assignment"))),
            V1ExprKind::Unary { op, operand } => {
                let v = self.eval_expr(operand, env)?;
                eval_v1_unary(*op, v, expr.line)
            }
            V1ExprKind::Binary { op, left, right } => match op {
                V1BinaryOp::And => {
                    let lv = self.eval_expr(left, env)?;
                    if lv == V1Value::Bool(false) {
                        Ok(V1Value::Bool(false))
                    } else {
                        self.eval_expr(right, env)
                    }
                }
                V1BinaryOp::Or => {
                    let lv = self.eval_expr(left, env)?;
                    if lv == V1Value::Bool(true) {
                        Ok(V1Value::Bool(true))
                    } else {
                        self.eval_expr(right, env)
                    }
                }
                other_op => {
                    let lv = self.eval_expr(left, env)?;
                    let rv = self.eval_expr(right, env)?;
                    eval_v1_binary(*other_op, lv, rv, expr.line)
                }
            },
            V1ExprKind::Conditional {
                then_expr,
                cond,
                else_expr,
            } => {
                let cv = self.eval_expr(cond, env)?;
                if cv == V1Value::Bool(true) {
                    self.eval_expr(then_expr, env)
                } else {
                    self.eval_expr(else_expr, env)
                }
            }
            V1ExprKind::Call { name, args } => {
                let mut eval_args = Vec::with_capacity(args.len());
                for a in args {
                    eval_args.push(self.eval_expr(a, env)?);
                }
                if is_v1_builtin(name) {
                    eval_v1_builtin(name, &eval_args, expr.line)
                } else if let Some(uf) = self.functions.get(name.as_str()).copied() {
                    self.call_user_fn(uf, eval_args, expr.line)
                } else {
                    Err(rt_err(expr.line, format!("unknown function `{name}`")))
                }
            }
            V1ExprKind::Index { base, index } => {
                let bv = self.eval_expr(base, env)?;
                let iv = self.eval_expr(index, env)?;
                match (bv, iv) {
                    (V1Value::TraceList(ts), V1Value::Int(idx)) => {
                        let u = idx
                            .to_u64()
                            .and_then(|v| usize::try_from(v).ok())
                            .filter(|&u| u < ts.len())
                            .ok_or_else(|| {
                                rt_err(
                                    expr.line,
                                    format!("index {idx} out of bounds for [trace] of len {}", ts.len()),
                                )
                            })?;
                        Ok(V1Value::Trace(Box::new(ts[u].clone())))
                    }
                    _ => Err(rt_err(expr.line, "invalid index operation")),
                }
            }
        }
    }
}

fn eval_v1_unary(op: V1UnaryOp, val: V1Value, line: usize) -> DecResult<V1Value> {
    match (op, val) {
        (V1UnaryOp::Pos, v) => Ok(v),
        (V1UnaryOp::Neg, V1Value::Int(n)) => Ok(V1Value::Int(-n)),
        (V1UnaryOp::Neg, V1Value::T1(t)) => Ok(V1Value::T1(t.checked_neg())),
        (V1UnaryOp::Neg, V1Value::T32(v)) => v
            .checked_neg()
            .map(V1Value::T32)
            .map_err(|m| rt_err(line, m)),
        (V1UnaryOp::Neg, V1Value::Ratio(r)) => Ok(V1Value::Ratio(-r)),
        (V1UnaryOp::Not, V1Value::Bool(b)) => Ok(V1Value::Bool(!b)),
        _ => Err(rt_err(line, "invalid unary operand")),
    }
}

fn eval_v1_binary(
    op: V1BinaryOp,
    left: V1Value,
    right: V1Value,
    line: usize,
) -> DecResult<V1Value> {
    if op == V1BinaryOp::Eq {
        return Ok(V1Value::Bool(left == right));
    }
    if op == V1BinaryOp::NotEq {
        return Ok(V1Value::Bool(left != right));
    }

    match (left, right) {
        (V1Value::Int(a), V1Value::Int(b)) => match op {
            V1BinaryOp::Add => Ok(V1Value::Int(&a + &b)),
            V1BinaryOp::Sub => Ok(V1Value::Int(&a - &b)),
            V1BinaryOp::Mul => Ok(V1Value::Int(&a * &b)),
            V1BinaryOp::Div => Ratio::new(a, b)
                .map(V1Value::Ratio)
                .map_err(|e| core_err(line, e)),
            V1BinaryOp::FloorDiv => a
                .div_floor(&b)
                .map(V1Value::Int)
                .map_err(|e| core_err(line, e)),
            V1BinaryOp::Mod => a
                .rem_floor(&b)
                .map(V1Value::Int)
                .map_err(|e| core_err(line, e)),
            V1BinaryOp::Lt => Ok(V1Value::Bool(a < b)),
            V1BinaryOp::LtEq => Ok(V1Value::Bool(a <= b)),
            V1BinaryOp::Gt => Ok(V1Value::Bool(a > b)),
            V1BinaryOp::GtEq => Ok(V1Value::Bool(a >= b)),
            _ => Err(rt_err(line, "unsupported int operator")),
        },
        (V1Value::T1(a), V1Value::T1(b)) => match op {
            V1BinaryOp::Add => a
                .checked_add(b)
                .map(V1Value::T1)
                .map_err(|e| core_err(line, e)),
            V1BinaryOp::Sub => a
                .checked_sub(b)
                .map(V1Value::T1)
                .map_err(|e| core_err(line, e)),
            V1BinaryOp::Mul => a
                .checked_mul(b)
                .map(V1Value::T1)
                .map_err(|e| core_err(line, e)),
            V1BinaryOp::Div => Ratio::from_i64(a.value() as i64, b.value() as i64)
                .map(V1Value::Ratio)
                .map_err(|e| core_err(line, e)),
            V1BinaryOp::Lt => Ok(V1Value::Bool(a < b)),
            V1BinaryOp::LtEq => Ok(V1Value::Bool(a <= b)),
            V1BinaryOp::Gt => Ok(V1Value::Bool(a > b)),
            V1BinaryOp::GtEq => Ok(V1Value::Bool(a >= b)),
            _ => Err(rt_err(line, "unsupported t1 operator")),
        },
        (V1Value::T32(a), V1Value::T32(b)) => match op {
            V1BinaryOp::Add => a
                .checked_add(b)
                .map(V1Value::T32)
                .map_err(|m| rt_err(line, m)),
            V1BinaryOp::Sub => a
                .checked_sub(b)
                .map(V1Value::T32)
                .map_err(|m| rt_err(line, m)),
            V1BinaryOp::Mul => a
                .checked_mul(b)
                .map(V1Value::T32)
                .map_err(|m| rt_err(line, m)),
            V1BinaryOp::Div => Ratio::new(BigInt::from_i128(a.value()), BigInt::from_i128(b.value()))
                .map(V1Value::Ratio)
                .map_err(|e| core_err(line, e)),
            V1BinaryOp::FloorDiv => a
                .checked_div_floor(b)
                .map(V1Value::T32)
                .map_err(|m| rt_err(line, m)),
            V1BinaryOp::Mod => a
                .checked_rem_floor(b)
                .map(V1Value::T32)
                .map_err(|m| rt_err(line, m)),
            V1BinaryOp::Lt => Ok(V1Value::Bool(a < b)),
            V1BinaryOp::LtEq => Ok(V1Value::Bool(a <= b)),
            V1BinaryOp::Gt => Ok(V1Value::Bool(a > b)),
            V1BinaryOp::GtEq => Ok(V1Value::Bool(a >= b)),
            _ => Err(rt_err(line, "unsupported t32 operator")),
        },
        (V1Value::Q4(a), V1Value::Q4(b)) => match op {
            V1BinaryOp::Lt => Ok(V1Value::Bool(a < b)),
            V1BinaryOp::LtEq => Ok(V1Value::Bool(a <= b)),
            V1BinaryOp::Gt => Ok(V1Value::Bool(a > b)),
            V1BinaryOp::GtEq => Ok(V1Value::Bool(a >= b)),
            _ => Err(rt_err(line, "unsupported q4 operator")),
        },
        (V1Value::Ratio(a), V1Value::Ratio(b)) => match op {
            V1BinaryOp::Add => Ok(V1Value::Ratio(&a + &b)),
            V1BinaryOp::Sub => Ok(V1Value::Ratio(&a - &b)),
            V1BinaryOp::Mul => Ok(V1Value::Ratio(&a * &b)),
            V1BinaryOp::Div => a
                .checked_div(&b)
                .map(V1Value::Ratio)
                .map_err(|e| core_err(line, e)),
            V1BinaryOp::Lt => Ok(V1Value::Bool(a < b)),
            V1BinaryOp::LtEq => Ok(V1Value::Bool(a <= b)),
            V1BinaryOp::Gt => Ok(V1Value::Bool(a > b)),
            V1BinaryOp::GtEq => Ok(V1Value::Bool(a >= b)),
            _ => Err(rt_err(line, "unsupported ratio operator")),
        },
        (V1Value::Str(a), V1Value::Str(b)) => match op {
            V1BinaryOp::Add => Ok(V1Value::Str(format!("{a}{b}"))),
            V1BinaryOp::Lt => Ok(V1Value::Bool(a < b)),
            V1BinaryOp::LtEq => Ok(V1Value::Bool(a <= b)),
            V1BinaryOp::Gt => Ok(V1Value::Bool(a > b)),
            V1BinaryOp::GtEq => Ok(V1Value::Bool(a >= b)),
            _ => Err(rt_err(line, "unsupported str operator")),
        },
        _ => Err(rt_err(line, "mismatched operand types")),
    }
}

fn eval_v1_builtin(name: &str, args: &[V1Value], line: usize) -> DecResult<V1Value> {
    match (name, args) {
        ("t1", [V1Value::Int(n)]) => T1::from_bigint(n)
            .map(V1Value::T1)
            .map_err(|e| core_err(line, e)),
        ("t32", [V1Value::Int(n)]) => V1T32::from_bigint(n)
            .map(V1Value::T32)
            .map_err(|m| rt_err(line, m)),
        ("q4", [V1Value::Int(n)]) => Q4::from_bigint(n)
            .map(V1Value::Q4)
            .map_err(|e| core_err(line, e)),
        ("int", [V1Value::Int(n)]) => Ok(V1Value::Int(n.clone())),
        ("int", [V1Value::T1(t)]) => Ok(V1Value::Int(BigInt::from_i64(t.value() as i64))),
        ("int", [V1Value::T32(v)]) => Ok(V1Value::Int(BigInt::from_i128(v.value()))),
        ("int", [V1Value::Q4(q)]) => Ok(V1Value::Int(BigInt::from_u64(q.value() as u64))),
        ("int", [V1Value::Ratio(r)]) => r
            .to_int_exact()
            .map(V1Value::Int)
            .map_err(|e| core_err(line, e)),
        ("ratio", [V1Value::Int(a), V1Value::Int(b)]) => Ratio::new(a.clone(), b.clone())
            .map(V1Value::Ratio)
            .map_err(|e| core_err(line, e)),
        ("ratio", [V1Value::Int(a)]) => Ok(V1Value::Ratio(Ratio::from_int(a.clone()))),
        ("num", [V1Value::Ratio(r)]) => Ok(V1Value::Int(r.num().clone())),
        ("den", [V1Value::Ratio(r)]) => Ok(V1Value::Int(r.den().clone())),
        ("pack", [V1Value::T32(v)]) => Ok(V1Value::Int(BigInt::from_u64(v.pack()))),
        ("unpack", [V1Value::Int(bits)]) => V1T32::unpack_bigint(bits)
            .map(V1Value::T32)
            .map_err(|m| rt_err(line, m)),
        ("neg", [v]) => eval_v1_unary(V1UnaryOp::Neg, v.clone(), line),
        ("abs", [V1Value::Int(n)]) => Ok(V1Value::Int(n.abs())),
        ("abs", [V1Value::T1(t)]) => Ok(V1Value::T1(
            T1::new(t.value().abs()).expect("abs of t1 in {0, 1}"),
        )),
        ("abs", [V1Value::T32(v)]) => Ok(V1Value::T32(v.abs())),
        ("abs", [V1Value::Ratio(r)]) => Ok(V1Value::Ratio(r.abs())),
        ("is_hamming1", [V1Value::Q4(a), V1Value::Q4(b)]) => Ok(V1Value::Bool(a.is_hamming1(*b))),
        ("popcnt_xor", [V1Value::Q4(a), V1Value::Q4(b)]) => {
            Ok(V1Value::Int(BigInt::from_u64(a.hamming_distance(*b) as u64)))
        }
        ("trace", [V1Value::Str(scope), V1Value::Str(lineage), V1Value::Q4(start)]) => {
            Trace::new(scope.clone(), lineage.clone(), *start)
                .map(|t| V1Value::Trace(Box::new(t)))
                .map_err(|e| core_err(line, e))
        }
        ("step", [V1Value::Trace(t), V1Value::Q4(dst), V1Value::T1(orient)]) => t
            .step(*dst, *orient)
            .map(|nt| V1Value::Trace(Box::new(nt)))
            .map_err(|e| core_err(line, e)),
        ("hinge", [V1Value::Trace(t)]) => t
            .hinge()
            .map(|nt| V1Value::Trace(Box::new(nt)))
            .map_err(|e| core_err(line, e)),
        ("dwell", [V1Value::Trace(t)]) => t
            .dwell()
            .map(|nt| V1Value::Trace(Box::new(nt)))
            .map_err(|e| core_err(line, e)),
        ("close", [V1Value::Trace(t)]) => t
            .close()
            .map(|nt| V1Value::Trace(Box::new(nt)))
            .map_err(|e| core_err(line, e)),
        ("phases", [V1Value::Trace(t)]) => t
            .phases()
            .map(|n| V1Value::Int(BigInt::from_u64(n as u64)))
            .map_err(|e| core_err(line, e)),
        ("beats", [V1Value::Trace(t)]) => t
            .beats()
            .map(|n| V1Value::Int(BigInt::from_u64(n as u64)))
            .map_err(|e| core_err(line, e)),
        ("rate", [V1Value::Trace(t)]) => t
            .rate()
            .map(V1Value::Ratio)
            .map_err(|e| core_err(line, e)),
        (
            "fold",
            [
                V1Value::Trace(a),
                V1Value::Trace(b),
                V1Value::Str(scope),
                V1Value::Str(lineage),
            ],
        ) => Fusion::fold(a, b, scope.clone(), lineage.clone())
            .map(|f| V1Value::Fusion(Box::new(f)))
            .map_err(|e| core_err(line, e)),
        ("outer", [V1Value::Fusion(f), V1Value::Q4(start)]) => f
            .outer(*start)
            .map(|t| V1Value::Trace(Box::new(t)))
            .map_err(|e| core_err(line, e)),
        ("fuse", [V1Value::Fusion(f), V1Value::Trace(t)]) => f
            .fuse(t)
            .map(|nt| V1Value::Trace(Box::new(nt)))
            .map_err(|e| core_err(line, e)),
        ("expose_inner", [V1Value::Trace(t)]) => t
            .expose_inner()
            .map(|parents| V1Value::TraceList(parents.to_vec()))
            .map_err(|e| core_err(line, e)),
        ("coupling", [V1Value::Trace(a), V1Value::Trace(b), V1Value::Str(window)]) => {
            Coupling::new(a, b, window.clone())
                .map(|c| V1Value::Coupling(Box::new(c)))
                .map_err(|e| core_err(line, e))
        }
        ("contact", [V1Value::Coupling(c), V1Value::Int(a), V1Value::Int(b)]) => {
            let au = a
                .to_u64()
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(|| rt_err(line, format!("contact ordinal out of range: {a}")))?;
            let bu = b
                .to_u64()
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(|| rt_err(line, format!("contact ordinal out of range: {b}")))?;
            c.contact(au, bu)
                .map(|nc| V1Value::Coupling(Box::new(nc)))
                .map_err(|e| core_err(line, e))
        }
        ("cadence", [V1Value::Coupling(c)]) => c
            .cadence()
            .map(V1Value::Ratio)
            .map_err(|e| core_err(line, e)),
        ("lag", [V1Value::Coupling(c)]) => c
            .lag()
            .map(V1Value::Int)
            .map_err(|e| core_err(line, e)),
        ("drift", [V1Value::Coupling(c)]) => c
            .drift()
            .map(V1Value::Int)
            .map_err(|e| core_err(line, e)),
        (
            "probability",
            [
                V1Value::Int(neg_w),
                V1Value::Int(hinge_w),
                V1Value::Int(pos_w),
                V1Value::T1(role),
            ],
        ) => probability(neg_w, hinge_w, pos_w, *role)
            .map(V1Value::Ratio)
            .map_err(|e| core_err(line, e)),
        ("len", [V1Value::Str(s)]) => Ok(V1Value::Int(BigInt::from_u64(s.chars().count() as u64))),
        ("len", [V1Value::TraceList(ts)]) => Ok(V1Value::Int(BigInt::from_u64(ts.len() as u64))),
        _ => Err(rt_err(line, format!("invalid builtin call `{name}`"))),
    }
}

// ============================================================================
// DecIR v1 Serializer & Hermetic Verifier
// ============================================================================

fn encode_v1_ir_json(prog: &V1Program) -> JsonValue {
    let mut root = BTreeMap::new();
    root.insert("format".to_string(), JsonValue::from_str_val(V1_IR_FORMAT));
    root.insert(
        "version".to_string(),
        JsonValue::from_i64(V1_IR_VERSION),
    );
    root.insert(
        "source".to_string(),
        JsonValue::from_str_val(prog.source.clone()),
    );
    root.insert(
        "source_sha256".to_string(),
        JsonValue::from_str_val(prog.source_sha256.clone()),
    );

    let mut funcs = Vec::with_capacity(prog.functions.len());
    for f in &prog.functions {
        let mut fobj = BTreeMap::new();
        fobj.insert("line".to_string(), JsonValue::from_usize(f.line));
        fobj.insert("name".to_string(), JsonValue::from_str_val(f.name.clone()));
        fobj.insert(
            "output".to_string(),
            JsonValue::from_str_val(f.output_name.clone()),
        );
        fobj.insert(
            "params".to_string(),
            JsonValue::Array(
                f.header_params
                    .iter()
                    .map(|p| JsonValue::from_str_val(p.clone()))
                    .collect(),
            ),
        );
        let branches = f
            .branches
            .iter()
            .map(|br| {
                let mut bobj = BTreeMap::new();
                bobj.insert("line".to_string(), JsonValue::from_usize(br.line));
                bobj.insert(
                    "output".to_string(),
                    JsonValue::from_str_val(br.output_name.clone()),
                );
                bobj.insert(
                    "params".to_string(),
                    JsonValue::Array(
                        br.params
                            .iter()
                            .map(|p| {
                                let mut pobj = BTreeMap::new();
                                pobj.insert(
                                    "name".to_string(),
                                    JsonValue::from_str_val(p.name.clone()),
                                );
                                pobj.insert(
                                    "type".to_string(),
                                    JsonValue::from_str_val(p.ty.as_str()),
                                );
                                JsonValue::Object(pobj)
                            })
                            .collect(),
                    ),
                );
                bobj.insert(
                    "return_type".to_string(),
                    JsonValue::from_str_val(br.return_type.as_str()),
                );
                bobj.insert(
                    "stmt_count".to_string(),
                    JsonValue::from_usize(br.body.len()),
                );
                JsonValue::Object(bobj)
            })
            .collect();
        fobj.insert("branches".to_string(), JsonValue::Array(branches));
        funcs.push(JsonValue::Object(fobj));
    }
    root.insert("functions".to_string(), JsonValue::Array(funcs));
    JsonValue::Object(root)
}

fn load_and_verify_v1_ir(ir_json: &str) -> DecResult<V1Program> {
    if ir_json.len() > MAX_V1_IR_BYTES {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            "DecIR v1 exceeds the 8 MB core limit",
        ));
    }
    let parsed = parse_strict_json(ir_json).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            format!("invalid DecIR v1 JSON: {e}"),
        )
    })?;
    let obj = parsed.as_object().ok_or_else(|| {
        Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            "DecIR v1 root must be a JSON object",
        )
    })?;

    if obj.get("format").and_then(JsonValue::as_str) != Some(V1_IR_FORMAT) {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            "unsupported DecIR format",
        ));
    }
    let version = obj
        .get("version")
        .and_then(JsonValue::as_int)
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                "DecIR version must be an exact integer",
            )
        })?;
    if *version != BigInt::from_i64(V1_IR_VERSION) {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            format!("expected DecIR version 1, got {version}"),
        ));
    }
    if obj.contains_key("syntax_discipline") {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            "syntax_discipline is not supported in DecIR v1",
        ));
    }

    let source = obj
        .get("source")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                "DecIR v1 `source` must be a string",
            )
        })?;
    let claimed_sha = obj
        .get("source_sha256")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::IrVerificationError,
                "DecIR v1 `source_sha256` must be a string",
            )
        })?;
    let actual_sha = Sha256::digest_hex(source.as_bytes());
    if claimed_sha != actual_sha {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            "DecIR v1 `source_sha256` does not match embedded source",
        ));
    }

    let rebuilt = check_v1_source(source).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            format!("DecIR v1 hermetic source check failed: {e}"),
        )
    })?;
    let expected_canonical = encode_v1_ir_json(&rebuilt).to_pretty_utf8_json();
    let parsed_canonical = parsed.to_pretty_utf8_json();
    if parsed_canonical != expected_canonical {
        return Err(Diagnostic::new(
            DiagnosticCode::IrVerificationError,
            "DecIR v1 payload or metadata differs from hermetically validated source",
        ));
    }
    Ok(rebuilt)
}

// ============================================================================
// Public Entrypoints
// ============================================================================

fn read_v1_file(path: &Path) -> DecResult<String> {
    std::fs::read_to_string(path).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::ParseError,
            format!("failed reading {}: {e}", path.display()),
        )
    })
}

pub fn check_v1_source(source: &str) -> DecResult<V1Program> {
    let tokens = lex_v1(source)?;
    let mut parser = V1Parser::new(tokens);
    let prog = parser.parse_program(source)?;
    V1Checker::check_program(&prog)?;
    Ok(prog)
}

pub fn check_v1_file(path: &Path) -> DecResult<V1Program> {
    let source = read_v1_file(path)?;
    check_v1_source(&source)
}

pub fn run_v1_source(source: &str, fuel: usize, max_depth: usize) -> DecResult<EvalOutput> {
    let prog = check_v1_source(source)?;
    V1Evaluator::new(&prog, fuel, max_depth).run_main()
}

pub fn run_v1_file(path: &Path, fuel: usize, max_depth: usize) -> DecResult<EvalOutput> {
    let source = read_v1_file(path)?;
    run_v1_source(&source, fuel, max_depth)
}

pub fn run_v1_file_capturing(
    path: &Path,
    fuel: usize,
    max_depth: usize,
) -> DecResult<(Vec<String>, DecResult<Value>)> {
    let prog = check_v1_file(path)?;
    Ok(V1Evaluator::new(&prog, fuel, max_depth).run_main_capturing())
}

pub fn compile_v1_source_to_ir(source: &str) -> DecResult<String> {
    let prog = check_v1_source(source)?;
    Ok(encode_v1_ir_json(&prog).to_pretty_utf8_json())
}

pub fn compile_v1_file_to_ir(path: &Path) -> DecResult<String> {
    let source = read_v1_file(path)?;
    compile_v1_source_to_ir(&source)
}

pub fn is_v1_ir_json(ir_json: &str) -> bool {
    if ir_json.len() > MAX_V1_IR_BYTES {
        return false;
    }
    let Ok(JsonValue::Object(obj)) = parse_strict_json(ir_json) else {
        return false;
    };
    obj.get("format").and_then(JsonValue::as_str) == Some(V1_IR_FORMAT)
        && obj.get("version").and_then(JsonValue::as_int) == Some(&BigInt::from_i64(V1_IR_VERSION))
}

pub fn exec_v1_ir_str(ir_json: &str, fuel: usize, max_depth: usize) -> DecResult<EvalOutput> {
    let prog = load_and_verify_v1_ir(ir_json)?;
    V1Evaluator::new(&prog, fuel, max_depth).run_main()
}

pub fn exec_v1_ir_str_capturing(
    ir_json: &str,
    fuel: usize,
    max_depth: usize,
) -> DecResult<(Vec<String>, DecResult<Value>)> {
    let prog = load_and_verify_v1_ir(ir_json)?;
    Ok(V1Evaluator::new(&prog, fuel, max_depth).run_main_capturing())
}
