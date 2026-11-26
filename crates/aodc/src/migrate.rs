#![forbid(unsafe_code)]

//! Deterministic source-to-source `.dec` migration translator (`aodc migrate`).
//!
//! Translates Frozen Dec v1 (`2bc927c`), PR #8 (`68d6705`) legacy `.dec` sources, and
//! clean-break Pure-Wave v3 `.dec` sources into canonical 4-space Pure-Wave Trit-Boxed v3.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use crate::diagnostics::{DecResult, Diagnostic, DiagnosticCode};

pub const MAX_MIGRATE_SOURCE_BYTES: usize = 4_000_000;

const BANNED_V3_IDENTS: &[&str] = &[
    "Dec", "if", "elif", "else", "while", "break", "continue", "mut", "match", "str", "u8",
    "int8", "int16", "int32", "int64", "t9", "t32", "float",
];

// ============================================================================
// Unified Legacy + v3 Lexer
// ============================================================================

#[derive(Clone, Debug, Eq, PartialEq)]
enum MigTok {
    Ident(String),
    IntLit(String),
    RatioLit(String),
    TritLit(String),
    Addr8Lit(String),
    SupportLit(String),
    StrLit(Vec<u8>),
    BytesLit(Vec<u8>),
    Boundary,
    Adverb(String),
    Pipe,
    Sieve,
    Cmp3,
    OuterAdd,
    OuterMul,
    Arrow,
    DotDot,
    ColonColon,
    Colon,
    Semi,
    Comma,
    Dot,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Plus,
    Minus,
    Star,
    Slash,
    SlashSlash,
    Percent,
    Amp,
    Bar,
    Caret,
    Tilde,
    Shl,
    Shr,
    Eq,
    EqEq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Newline,
    Indent,
    Dedent,
    Eof,
}

#[derive(Clone, Debug)]
struct MigSpanTok {
    kind: MigTok,
    line: usize,
    col: usize,
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn lex_quoted_bytes(
    bytes: &[u8],
    mut i: usize,
    quote: u8,
    line: usize,
    col: usize,
    line_start: usize,
) -> DecResult<(Vec<u8>, usize)> {
    let len = bytes.len();
    let mut out = Vec::new();
    while i < len {
        let b = bytes[i];
        if b == b'\n' || b == b'\r' {
            break;
        }
        if b == quote {
            return Ok((out, i + 1));
        }
        if b == b'\\' {
            i += 1;
            if i >= len {
                break;
            }
            match bytes[i] {
                b'\\' => out.push(b'\\'),
                b'"' => out.push(b'"'),
                b'\'' => out.push(b'\''),
                b'n' => out.push(b'\n'),
                b'r' => out.push(b'\r'),
                b't' => out.push(b'\t'),
                b'0' => out.push(0),
                b'x' => {
                    if i + 2 >= len {
                        return Err(Diagnostic::at(
                            DiagnosticCode::LexError,
                            line,
                            i - line_start + 1,
                            "incomplete \\xHH escape in literal",
                        ));
                    }
                    let hi = hex_nibble(bytes[i + 1]).ok_or_else(|| {
                        Diagnostic::at(
                            DiagnosticCode::LexError,
                            line,
                            i - line_start + 1,
                            "invalid hex escape in literal",
                        )
                    })?;
                    let lo = hex_nibble(bytes[i + 2]).ok_or_else(|| {
                        Diagnostic::at(
                            DiagnosticCode::LexError,
                            line,
                            i - line_start + 1,
                            "invalid hex escape in literal",
                        )
                    })?;
                    out.push((hi << 4) | lo);
                    i += 2;
                }
                other => {
                    return Err(Diagnostic::at(
                        DiagnosticCode::LexError,
                        line,
                        i - line_start + 1,
                        format!("unsupported escape \\{}", other as char),
                    ));
                }
            }
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }
    Err(Diagnostic::at(
        DiagnosticCode::LexError,
        line,
        col,
        "unterminated quoted literal",
    ))
}

fn extract_leading_comments(source: &str) -> Vec<String> {
    let mut header = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !header.is_empty() {
                break;
            }
            continue;
        }
        if trimmed.starts_with('#') {
            header.push(trimmed.to_string());
        } else {
            break;
        }
    }
    header
}

fn try_lex_support_or_ratio_or_number(
    bytes: &[u8],
    start: usize,
    line: usize,
    col: usize,
) -> DecResult<(MigTok, usize)> {
    let len = bytes.len();
    let mut i = start;

    if bytes[i] == b'0' && i + 1 < len {
        match bytes[i + 1] {
            b'x' | b'X' => {
                i += 2;
                let d0 = i;
                while i < len && (bytes[i].is_ascii_hexdigit() || bytes[i] == b'_') {
                    i += 1;
                }
                if i == d0 {
                    return Err(Diagnostic::at(
                        DiagnosticCode::LexError,
                        line,
                        col,
                        "empty hex literal",
                    ));
                }
                let raw = std::str::from_utf8(&bytes[start..i]).expect("ascii");
                return Ok((MigTok::IntLit(raw.to_string()), i));
            }
            b'b' | b'B' => {
                i += 2;
                let d0 = i;
                while i < len && (bytes[i] == b'0' || bytes[i] == b'1' || bytes[i] == b'_') {
                    i += 1;
                }
                if i == d0 {
                    return Err(Diagnostic::at(
                        DiagnosticCode::LexError,
                        line,
                        col,
                        "empty binary literal",
                    ));
                }
                let raw = std::str::from_utf8(&bytes[start..i]).expect("ascii");
                return Ok((MigTok::IntLit(raw.to_string()), i));
            }
            b'o' => {
                i += 2;
                let d0 = i;
                while i < len && (b'0'..=b'7').contains(&bytes[i]) {
                    i += 1;
                }
                if i == d0 {
                    return Err(Diagnostic::at(
                        DiagnosticCode::InvalidOctalAnchor,
                        line,
                        col,
                        "empty octal anchor literal",
                    ));
                }
                let oct = std::str::from_utf8(&bytes[d0..i]).expect("ascii");
                return Ok((MigTok::Addr8Lit(format!("{oct}_8")), i));
            }
            _ => {}
        }
    }

    while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
        if bytes[i] == b'_' && i + 2 <= len && &bytes[i..i + 2] == b"_8" {
            let after = bytes.get(i + 2).copied();
            if !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_') {
                let digits = std::str::from_utf8(&bytes[start..i]).expect("ascii");
                if digits.bytes().any(|d| !(b'0'..=b'7').contains(&d)) {
                    return Err(Diagnostic::at(
                        DiagnosticCode::InvalidOctalAnchor,
                        line,
                        col,
                        format!("invalid octal anchor `{digits}_8`"),
                    ));
                }
                return Ok((MigTok::Addr8Lit(format!("{digits}_8")), i + 2));
            }
        }
        if bytes[i] == b'_' && i + 2 <= len && &bytes[i..i + 2] == b"_H" {
            let after = bytes.get(i + 2).copied();
            if start + 1 == i
                && bytes[start] == b'0'
                && !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
            {
                return Ok((MigTok::TritLit("0_H".to_string()), i + 2));
            }
        }
        if bytes[i] == b'_' && i + 2 <= len && &bytes[i..i + 2] == b"_D" {
            let after = bytes.get(i + 2).copied();
            if start + 1 == i
                && bytes[start] == b'0'
                && !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
            {
                return Ok((MigTok::TritLit("0_D".to_string()), i + 2));
            }
        }
        i += 1;
    }

    // Check `<digits>|-` octal anchor form
    if i + 2 <= len && &bytes[i..i + 2] == b"|-" {
        let digits = std::str::from_utf8(&bytes[start..i]).expect("ascii");
        if !digits.is_empty() && digits.bytes().all(|d| (b'0'..=b'7').contains(&d)) {
            return Ok((MigTok::Addr8Lit(format!("{digits}_8")), i + 2));
        }
    }

    // Check support literal `<p>:<q>:<l>[_yro]`
    if i < len && bytes[i] == b':' && i + 1 < len && bytes[i + 1].is_ascii_digit() {
        let mut j = i + 1;
        while j < len && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j < len && bytes[j] == b':' && j + 1 < len && bytes[j + 1].is_ascii_digit() {
            let mut k = j + 1;
            while k < len && bytes[k].is_ascii_digit() {
                k += 1;
            }
            if k + 4 <= len && &bytes[k..k + 4] == b"_yro" {
                let after = bytes.get(k + 4).copied();
                if !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_') {
                    let raw = std::str::from_utf8(&bytes[start..k + 4]).expect("ascii");
                    return Ok((MigTok::SupportLit(raw.to_string()), k + 4));
                }
            }
            let raw = std::str::from_utf8(&bytes[start..k]).expect("ascii");
            return Ok((MigTok::SupportLit(raw.to_string()), k));
        }
    }

    // Check ratio literal `<num>/<den>r`
    if i < len && bytes[i] == b'/' && i + 1 < len && bytes[i + 1].is_ascii_digit() {
        let mut j = i + 1;
        while j < len && (bytes[j].is_ascii_digit() || bytes[j] == b'_') {
            j += 1;
        }
        if j < len && bytes[j] == b'r' {
            let after = bytes.get(j + 1).copied();
            if !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_') {
                let num_s: String = std::str::from_utf8(&bytes[start..i])
                    .expect("ascii")
                    .chars()
                    .filter(|&c| c != '_')
                    .collect();
                let den_s: String = std::str::from_utf8(&bytes[i + 1..j])
                    .expect("ascii")
                    .chars()
                    .filter(|&c| c != '_')
                    .collect();
                return Ok((MigTok::RatioLit(format!("{num_s}/{den_s}r")), j + 1));
            }
        }
    }

    // Check decimal fraction (forbidden across all Dec versions unless `..` range)
    if i < len && bytes[i] == b'.' && i + 1 < len && bytes[i + 1].is_ascii_digit() {
        return Err(Diagnostic::at(
            DiagnosticCode::FloatLiteralForbidden,
            line,
            col,
            "decimal fraction literals are forbidden in Dec; use exact ratio literals",
        ));
    }

    // Check trit literal `0t` or `1t`
    if i < len && bytes[i] == b't' {
        let after = bytes.get(i + 1).copied();
        if !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_') {
            let digits = std::str::from_utf8(&bytes[start..i]).expect("ascii");
            if digits == "0" || digits == "1" {
                return Ok((MigTok::TritLit(format!("{digits}t")), i + 1));
            }
            return Err(Diagnostic::at(
                DiagnosticCode::InvalidTritLiteral,
                line,
                col,
                format!("invalid trit literal `{digits}t`"),
            ));
        }
    }

    // Check unit ratio literal `<num>r`
    if i < len && bytes[i] == b'r' {
        let after = bytes.get(i + 1).copied();
        if !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_') {
            let num_s: String = std::str::from_utf8(&bytes[start..i])
                .expect("ascii")
                .chars()
                .filter(|&c| c != '_')
                .collect();
            return Ok((MigTok::RatioLit(format!("{num_s}/1r")), i + 1));
        }
    }

    let cleaned: String = std::str::from_utf8(&bytes[start..i])
        .expect("ascii")
        .chars()
        .filter(|&c| c != '_')
        .collect();
    Ok((MigTok::IntLit(cleaned), i))
}

fn lex_migrate(source: &str) -> DecResult<Vec<MigSpanTok>> {
    if source.len() > MAX_MIGRATE_SOURCE_BYTES {
        return Err(Diagnostic::new(
            DiagnosticCode::LexError,
            "source exceeds 4 MB migration limit",
        ));
    }

    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    let mut line = 1usize;
    let mut line_start = 0usize;
    let mut at_line_start = true;
    let mut delim_depth = 0usize;
    let mut indent_stack: Vec<usize> = vec![0];
    let mut tokens: Vec<MigSpanTok> = Vec::new();

    while i < len {
        if at_line_start && delim_depth == 0 {
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
                tokens.push(MigSpanTok {
                    kind: MigTok::Indent,
                    line,
                    col: 1,
                });
            } else if indent < cur_indent {
                while let Some(&top) = indent_stack.last() {
                    if top > indent {
                        indent_stack.pop();
                        tokens.push(MigSpanTok {
                            kind: MigTok::Dedent,
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
                        "inconsistent dedent level",
                    ));
                }
            }
            at_line_start = false;
        }

        let b = bytes[i];
        let col = i - line_start + 1;

        // UTF-8 `∂` (`0xE2 0x88 0x82`)
        if b == 0xE2 && i + 2 < len && bytes[i + 1] == 0x88 && bytes[i + 2] == 0x82 {
            tokens.push(MigSpanTok {
                kind: MigTok::Boundary,
                line,
                col,
            });
            i += 3;
            continue;
        }

        match b {
            b' ' | b'\r' | b'\t' => {
                i += 1;
            }
            b'#' => {
                while i < len && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'\n' => {
                if delim_depth == 0 {
                    if !matches!(
                        tokens.last().map(|t| &t.kind),
                        Some(MigTok::Newline | MigTok::Indent | MigTok::Dedent) | None
                    ) {
                        tokens.push(MigSpanTok {
                            kind: MigTok::Newline,
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
                delim_depth += 1;
                tokens.push(MigSpanTok {
                    kind: MigTok::LParen,
                    line,
                    col,
                });
                i += 1;
            }
            b')' => {
                if delim_depth == 0 {
                    return Err(Diagnostic::at(
                        DiagnosticCode::ParseError,
                        line,
                        col,
                        "unmatched ')'",
                    ));
                }
                delim_depth -= 1;
                tokens.push(MigSpanTok {
                    kind: MigTok::RParen,
                    line,
                    col,
                });
                i += 1;
            }
            b'[' => {
                delim_depth += 1;
                tokens.push(MigSpanTok {
                    kind: MigTok::LBracket,
                    line,
                    col,
                });
                i += 1;
            }
            b']' => {
                if delim_depth == 0 {
                    return Err(Diagnostic::at(
                        DiagnosticCode::ParseError,
                        line,
                        col,
                        "unmatched ']'",
                    ));
                }
                delim_depth -= 1;
                tokens.push(MigSpanTok {
                    kind: MigTok::RBracket,
                    line,
                    col,
                });
                i += 1;
            }
            b'{' => {
                delim_depth += 1;
                tokens.push(MigSpanTok {
                    kind: MigTok::LBrace,
                    line,
                    col,
                });
                i += 1;
            }
            b'}' => {
                if delim_depth == 0 {
                    return Err(Diagnostic::at(
                        DiagnosticCode::ParseError,
                        line,
                        col,
                        "unmatched '}'",
                    ));
                }
                delim_depth -= 1;
                tokens.push(MigSpanTok {
                    kind: MigTok::RBrace,
                    line,
                    col,
                });
                i += 1;
            }
            b',' => {
                tokens.push(MigSpanTok {
                    kind: MigTok::Comma,
                    line,
                    col,
                });
                i += 1;
            }
            b';' => {
                tokens.push(MigSpanTok {
                    kind: MigTok::Semi,
                    line,
                    col,
                });
                i += 1;
            }
            b'.' => {
                if i + 1 < len && bytes[i + 1] == b'.' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::DotDot,
                        line,
                        col,
                    });
                    i += 2;
                } else {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Dot,
                        line,
                        col,
                    });
                    i += 1;
                }
            }
            b':' => {
                if i + 1 < len && bytes[i + 1] == b':' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::ColonColon,
                        line,
                        col,
                    });
                    i += 2;
                } else {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Colon,
                        line,
                        col,
                    });
                    i += 1;
                }
            }
            b'+' => {
                if i + 1 < len && bytes[i + 1] == b'/' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Adverb("+/".to_string()),
                        line,
                        col,
                    });
                    i += 2;
                } else if i + 1 < len && bytes[i + 1] == b'\\' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Adverb("+\\".to_string()),
                        line,
                        col,
                    });
                    i += 2;
                } else if i + 1 < len && bytes[i + 1] == b':' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::OuterAdd,
                        line,
                        col,
                    });
                    i += 2;
                } else {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Plus,
                        line,
                        col,
                    });
                    i += 1;
                }
            }
            b'-' => {
                if i + 1 < len && bytes[i + 1] == b'>' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Arrow,
                        line,
                        col,
                    });
                    i += 2;
                } else {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Minus,
                        line,
                        col,
                    });
                    i += 1;
                }
            }
            b'*' => {
                if i + 1 < len && bytes[i + 1] == b'/' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Adverb("*/".to_string()),
                        line,
                        col,
                    });
                    i += 2;
                } else if i + 1 < len && bytes[i + 1] == b'\\' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Adverb("*\\".to_string()),
                        line,
                        col,
                    });
                    i += 2;
                } else if i + 1 < len && bytes[i + 1] == b':' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::OuterMul,
                        line,
                        col,
                    });
                    i += 2;
                } else {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Star,
                        line,
                        col,
                    });
                    i += 1;
                }
            }
            b'/' => {
                if i + 1 < len && bytes[i + 1] == b'/' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::SlashSlash,
                        line,
                        col,
                    });
                    i += 2;
                } else {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Slash,
                        line,
                        col,
                    });
                    i += 1;
                }
            }
            b'%' => {
                tokens.push(MigSpanTok {
                    kind: MigTok::Percent,
                    line,
                    col,
                });
                i += 1;
            }
            b'&' => {
                if i + 1 < len && bytes[i + 1] == b'/' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Adverb("&/".to_string()),
                        line,
                        col,
                    });
                    i += 2;
                } else {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Amp,
                        line,
                        col,
                    });
                    i += 1;
                }
            }
            b'|' => {
                if i + 1 < len && bytes[i + 1] == b'>' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Pipe,
                        line,
                        col,
                    });
                    i += 2;
                } else if i + 1 < len && bytes[i + 1] == b'/' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Adverb("|/".to_string()),
                        line,
                        col,
                    });
                    i += 2;
                } else {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Bar,
                        line,
                        col,
                    });
                    i += 1;
                }
            }
            b'^' => {
                tokens.push(MigSpanTok {
                    kind: MigTok::Caret,
                    line,
                    col,
                });
                i += 1;
            }
            b'~' => {
                tokens.push(MigSpanTok {
                    kind: MigTok::Tilde,
                    line,
                    col,
                });
                i += 1;
            }
            b'?' => {
                if i + 1 < len && bytes[i + 1] == b'>' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Sieve,
                        line,
                        col,
                    });
                    i += 2;
                } else {
                    return Err(Diagnostic::at(
                        DiagnosticCode::LexError,
                        line,
                        col,
                        "unexpected '?'",
                    ));
                }
            }
            b'=' => {
                if i + 1 < len && bytes[i + 1] == b'=' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::EqEq,
                        line,
                        col,
                    });
                    i += 2;
                } else {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Eq,
                        line,
                        col,
                    });
                    i += 1;
                }
            }
            b'!' => {
                if i + 1 < len && bytes[i + 1] == b'=' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::NotEq,
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
                if i + 2 < len && bytes[i + 1] == b'=' && bytes[i + 2] == b'>' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Cmp3,
                        line,
                        col,
                    });
                    i += 3;
                } else if i + 1 < len && bytes[i + 1] == b'<' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Shl,
                        line,
                        col,
                    });
                    i += 2;
                } else if i + 1 < len && bytes[i + 1] == b'=' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::LtEq,
                        line,
                        col,
                    });
                    i += 2;
                } else {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Lt,
                        line,
                        col,
                    });
                    i += 1;
                }
            }
            b'>' => {
                if i + 1 < len && bytes[i + 1] == b'>' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Shr,
                        line,
                        col,
                    });
                    i += 2;
                } else if i + 1 < len && bytes[i + 1] == b'=' {
                    tokens.push(MigSpanTok {
                        kind: MigTok::GtEq,
                        line,
                        col,
                    });
                    i += 2;
                } else {
                    tokens.push(MigSpanTok {
                        kind: MigTok::Gt,
                        line,
                        col,
                    });
                    i += 1;
                }
            }
            b'"' | b'\'' => {
                let (raw_bs, next_i) = lex_quoted_bytes(bytes, i + 1, b, line, col, line_start)?;
                tokens.push(MigSpanTok {
                    kind: MigTok::StrLit(raw_bs),
                    line,
                    col,
                });
                i = next_i;
            }
            b'0'..=b'9' => {
                let (tok, next_i) = try_lex_support_or_ratio_or_number(bytes, i, line, col)?;
                tokens.push(MigSpanTok {
                    kind: tok,
                    line,
                    col,
                });
                i = next_i;
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                if b == b'b' && i + 1 < len && (bytes[i + 1] == b'"' || bytes[i + 1] == b'\'') {
                    let quote = bytes[i + 1];
                    let (raw_bs, next_i) =
                        lex_quoted_bytes(bytes, i + 2, quote, line, col, line_start)?;
                    tokens.push(MigSpanTok {
                        kind: MigTok::BytesLit(raw_bs),
                        line,
                        col,
                    });
                    i = next_i;
                    continue;
                }
                let start = i;
                while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let word = std::str::from_utf8(&bytes[start..i]).expect("ascii");
                if matches!(word, "min" | "max" | "superpose") && i < len {
                    if bytes[i] == b'/' {
                        tokens.push(MigSpanTok {
                            kind: MigTok::Adverb(format!("{word}/")),
                            line,
                            col,
                        });
                        i += 1;
                        continue;
                    }
                    if bytes[i] == b'\\' && matches!(word, "min" | "max") {
                        tokens.push(MigSpanTok {
                            kind: MigTok::Adverb(format!("{word}\\")),
                            line,
                            col,
                        });
                        i += 1;
                        continue;
                    }
                }
                tokens.push(MigSpanTok {
                    kind: MigTok::Ident(word.to_string()),
                    line,
                    col,
                });
            }
            _ => {
                return Err(Diagnostic::at(
                    DiagnosticCode::LexError,
                    line,
                    col,
                    "unexpected character in source during migration",
                ));
            }
        }
    }

    if delim_depth != 0 {
        return Err(Diagnostic::at_line(
            DiagnosticCode::ParseError,
            line,
            "unclosed delimiter in source",
        ));
    }

    if !matches!(
        tokens.last().map(|t| &t.kind),
        Some(MigTok::Newline | MigTok::Dedent) | None
    ) {
        tokens.push(MigSpanTok {
            kind: MigTok::Newline,
            line,
            col: 1,
        });
    }
    while indent_stack.len() > 1 {
        indent_stack.pop();
        tokens.push(MigSpanTok {
            kind: MigTok::Dedent,
            line,
            col: 1,
        });
    }
    tokens.push(MigSpanTok {
        kind: MigTok::Eof,
        line,
        col: 1,
    });
    Ok(tokens)
}

// ============================================================================
// Migration AST
// ============================================================================

#[derive(Clone, Debug, Eq, PartialEq)]
enum MigType {
    Unit,
    Named(String),
    List(Box<MigType>),
    Array(Box<MigType>, String),
    Map(Box<MigType>, Box<MigType>),
    Tuple(Vec<MigType>),
    ReadOnly(Box<MigType>),
    QExt(Box<MigType>),
}

impl MigType {
    fn migrate(&self) -> MigType {
        match self {
            Self::Unit => Self::Unit,
            Self::Named(n) => {
                let mapped = match n.as_str() {
                    "str" => "bytes",
                    "u8" => "b8",
                    "int8" | "int16" | "int32" | "int64" | "t9" | "t32" => "int",
                    other => other,
                };
                Self::Named(mapped.to_string())
            }
            Self::List(inner) => Self::List(Box::new(inner.migrate())),
            Self::Array(inner, len) => Self::Array(Box::new(inner.migrate()), len.clone()),
            Self::Map(k, v) => Self::Map(Box::new(k.migrate()), Box::new(v.migrate())),
            Self::Tuple(ts) => Self::Tuple(ts.iter().map(MigType::migrate).collect()),
            Self::ReadOnly(inner) => Self::ReadOnly(Box::new(inner.migrate())),
            Self::QExt(inner) => Self::QExt(Box::new(inner.migrate())),
        }
    }

    fn render(&self) -> String {
        match self {
            Self::Unit => "()".to_string(),
            Self::Named(n) => n.clone(),
            Self::List(inner) => format!("[{}]", inner.render()),
            Self::Array(inner, len) => format!("[{}; {len}]", inner.render()),
            Self::Map(k, v) => format!("map[{}, {}]", k.render(), v.render()),
            Self::Tuple(ts) => {
                let ps: Vec<String> = ts.iter().map(MigType::render).collect();
                format!("({})", ps.join(", "))
            }
            Self::ReadOnly(inner) => format!("ReadOnly<{}>", inner.render()),
            Self::QExt(inner) => format!("QExt<{}>", inner.render()),
        }
    }
}

#[derive(Clone, Debug)]
enum MigItem {
    Use {
        path: Vec<String>,
        items: Option<Vec<String>>,
    },
    Const {
        name: String,
        ty: Option<MigType>,
        value: MigExpr,
    },
    Struct {
        name: String,
        fields: Vec<(String, MigType)>,
    },
    Enum {
        name: String,
        variants: Vec<(String, Vec<MigType>)>,
    },
    DecFn(MigFn),
    Chamber {
        name: String,
        space: MigExpr,
        walls: String,
        engine: Option<String>,
    },
    Prepare {
        name: String,
        chamber: String,
        lets: Vec<MigStmt>,
        clocks: Vec<(String, MigExpr)>,
        injections: Vec<MigInject>,
    },
    Observe {
        name: String,
        probes: Vec<MigProbe>,
    },
    QuarantineMod {
        name: String,
        items: Vec<MigItem>,
    },
}

#[derive(Clone, Debug)]
struct MigInject {
    support: MigExpr,
    pos: MigExpr,
    dir: String,
    orient: MigExpr,
    shells: MigExpr,
}

#[derive(Clone, Debug)]
enum MigProbe {
    Declarative {
        kind: String,
        name: String,
        region: MigExpr,
        capture: Vec<String>,
    },
    Procedural {
        kind: String,
        name: String,
        param_name: String,
        param_ty: Option<MigType>,
        return_type: MigType,
        body: Vec<MigStmt>,
    },
}

#[derive(Clone, Debug)]
struct MigFn {
    name: String,
    is_v1_origin: bool,
    arms: Vec<MigArm>,
}

#[derive(Clone, Debug)]
struct MigArm {
    params: Vec<MigArmParam>,
    guard: Option<MigExpr>,
    return_type: MigType,
    output_name: Option<String>,
    body: Vec<MigStmt>,
}

#[derive(Clone, Debug)]
enum MigArmParam {
    TypedVar { name: String, ty: MigType },
    TypedPattern { pat: MigPat, ty: MigType },
    Pattern(MigPat),
}

#[derive(Clone, Debug)]
enum MigPat {
    Wildcard,
    Binding(String),
    NullLit,
    IntLit(String),
    BoolLit(bool),
    BytesLit(Vec<u8>),
    TritLit(String),
    RatioLit(String),
    Addr8Lit(String),
    SupportLit(String),
    EnumOrCtor { path: Vec<String>, args: Vec<MigPat> },
    Tuple(Vec<MigPat>),
}

#[derive(Clone, Debug)]
enum MigLetPat {
    Single(String),
    Tuple(Vec<(String, Option<MigType>)>),
}

#[derive(Clone, Debug)]
enum MigStmt {
    Let {
        #[allow(dead_code)]
        mutable: bool,
        pat: MigLetPat,
        ty: Option<MigType>,
        value: MigExpr,
    },
    AssignVar {
        name: String,
        value: MigExpr,
    },
    AssignIndex {
        name: String,
        index: MigExpr,
        value: MigExpr,
    },
    Assert(MigExpr),
    Emit(MigExpr),
    Return(Option<MigExpr>),
    Pass,
    QuarantineBlock(Vec<MigStmt>),
    If {
        branches: Vec<(MigExpr, Vec<MigStmt>)>,
        else_body: Vec<MigStmt>,
    },
    For {
        var: String,
        iter: MigExpr,
        body: Vec<MigStmt>,
    },
    ExprStmt(MigExpr),
}

#[derive(Clone, Debug)]
enum MigExpr {
    NullLit,
    IntLit(String),
    BoolLit(bool),
    BytesLit(Vec<u8>),
    StrLit(Vec<u8>),
    RatioLit(String),
    TritLit(String),
    Addr8Lit(String),
    SupportLit(String),
    Ident(String),
    Path(Vec<String>),
    Tuple(Vec<MigExpr>),
    SeqLit(Vec<MigExpr>),
    MapLit(Vec<(MigExpr, MigExpr)>),
    StructLit {
        path: Vec<String>,
        fields: Vec<(String, MigExpr)>,
    },
    WindowLit {
        start: Box<MigExpr>,
        end: Box<MigExpr>,
    },
    Range {
        start: Box<MigExpr>,
        end: Box<MigExpr>,
    },
    BoxExpr(Vec<MigExpr>),
    Simulate {
        prepare: String,
        ticks: Box<MigExpr>,
        observers: Vec<String>,
    },
    Unary {
        op: String,
        operand: Box<MigExpr>,
    },
    Adverb {
        op: String,
        operand: Box<MigExpr>,
    },
    Binary {
        op: String,
        left: Box<MigExpr>,
        right: Box<MigExpr>,
    },
    Conditional {
        then_expr: Box<MigExpr>,
        cond: Box<MigExpr>,
        else_expr: Box<MigExpr>,
    },
    Pipe {
        input: Box<MigExpr>,
        stage: Box<MigPipeStage>,
    },
    Field {
        base: Box<MigExpr>,
        name: String,
    },
    Index {
        base: Box<MigExpr>,
        index: Box<MigExpr>,
    },
    Call {
        callee: Box<MigExpr>,
        args: Vec<(Option<String>, MigExpr)>,
    },
}

#[derive(Clone, Debug)]
enum MigPipeStage {
    Adverb(String),
    Boundary,
    Section { op: String, rhs: MigExpr },
    Call { callee: MigExpr, extra_args: Vec<MigExpr> },
}

// ============================================================================
// Unified Parser
// ============================================================================

fn is_legacy_v1_type_word(w: &str) -> bool {
    matches!(
        w,
        "int"
            | "bool"
            | "str"
            | "bytes"
            | "t1"
            | "t9"
            | "t32"
            | "q4"
            | "u8"
            | "b8"
            | "b64"
            | "int8"
            | "int16"
            | "int32"
            | "int64"
            | "ratio"
            | "trace"
            | "fusion"
            | "coupling"
            | "support"
            | "addr8"
            | "window"
            | "range"
    )
}

struct MigParser {
    tokens: Vec<MigSpanTok>,
    pos: usize,
    allow_struct_lit: bool,
}

impl MigParser {
    fn new(tokens: Vec<MigSpanTok>) -> Self {
        Self {
            tokens,
            pos: 0,
            allow_struct_lit: true,
        }
    }

    fn peek(&self) -> &MigSpanTok {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_n(&self, offset: usize) -> &MigSpanTok {
        &self.tokens[(self.pos + offset).min(self.tokens.len() - 1)]
    }

    fn line(&self) -> usize {
        self.peek().line
    }

    fn advance(&mut self) -> MigSpanTok {
        let tok = self.peek().clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn check(&self, expected: &MigTok) -> bool {
        &self.peek().kind == expected
    }

    fn check_word(&self, word: &str) -> bool {
        matches!(&self.peek().kind, MigTok::Ident(s) if s == word)
    }

    fn expect(&mut self, expected: &MigTok, desc: &str) -> DecResult<()> {
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
            MigTok::Ident(s) => {
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

    fn expect_word(&mut self, word: &str) -> DecResult<()> {
        if self.check_word(word) {
            self.advance();
            Ok(())
        } else {
            let p = self.peek();
            Err(Diagnostic::at(
                DiagnosticCode::ParseError,
                p.line,
                p.col,
                format!("expected `{word}`, found {:?}", p.kind),
            ))
        }
    }

    fn skip_newlines(&mut self) {
        while self.check(&MigTok::Newline) {
            self.advance();
        }
    }

    fn expect_stmt_end(&mut self) -> DecResult<()> {
        if self.check(&MigTok::Newline) {
            self.advance();
            Ok(())
        } else if self.check(&MigTok::Dedent) || self.check(&MigTok::Eof) {
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

    fn parse_program(&mut self) -> DecResult<Vec<MigItem>> {
        self.skip_newlines();
        let mut items = Vec::new();
        while !self.check(&MigTok::Eof) {
            items.push(self.parse_item()?);
            self.skip_newlines();
        }
        if items.is_empty() {
            return Err(Diagnostic::new(
                DiagnosticCode::ParseError,
                "cannot migrate empty `.dec` source",
            ));
        }
        Ok(items)
    }

    fn parse_item(&mut self) -> DecResult<MigItem> {
        let p = self.peek().clone();
        let MigTok::Ident(kw) = &p.kind else {
            return Err(Diagnostic::at(
                DiagnosticCode::ParseError,
                p.line,
                p.col,
                format!("unexpected top-level token {:?}", p.kind),
            ));
        };
        match kw.as_str() {
            "use" => self.parse_use(),
            "const" => self.parse_const(),
            "struct" => self.parse_struct(),
            "enum" => self.parse_enum(),
            "dec" | "Dec" => Ok(MigItem::DecFn(self.parse_dec_fn()?)),
            "chamber" => self.parse_chamber(),
            "prepare" => self.parse_prepare(),
            "observe" => self.parse_observe(),
            "quarantine" => self.parse_quarantine_mod(),
            other => Err(Diagnostic::at(
                DiagnosticCode::ParseError,
                p.line,
                p.col,
                format!("unexpected top-level declaration `{other}`"),
            )),
        }
    }

    fn parse_use(&mut self) -> DecResult<MigItem> {
        self.expect_word("use")?;
        let mut path = vec![self.expect_ident("module segment")?];
        while self.check(&MigTok::ColonColon) {
            self.advance();
            if self.check(&MigTok::LBrace) {
                self.advance();
                let mut items = Vec::new();
                while !self.check(&MigTok::RBrace) && !self.check(&MigTok::Eof) {
                    items.push(self.expect_ident("imported symbol")?);
                    if self.check(&MigTok::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&MigTok::RBrace, "'}' after use list")?;
                self.expect_stmt_end()?;
                return Ok(MigItem::Use {
                    path,
                    items: Some(items),
                });
            }
            path.push(self.expect_ident("module segment")?);
        }

        if self.check(&MigTok::Colon) {
            self.advance();
            self.expect(&MigTok::Newline, "newline after `use path:`")?;
            self.expect(&MigTok::Indent, "indented import list")?;
            self.skip_newlines();
            let mut items = Vec::new();
            while !self.check(&MigTok::Dedent) && !self.check(&MigTok::Eof) {
                items.push(self.expect_ident("imported symbol")?);
                while self.check(&MigTok::Comma) {
                    self.advance();
                    self.skip_newlines();
                    if self.check(&MigTok::Dedent) || self.check(&MigTok::Eof) {
                        break;
                    }
                    items.push(self.expect_ident("imported symbol")?);
                }
                self.skip_newlines();
            }
            self.expect(&MigTok::Dedent, "dedent after import list")?;
            return Ok(MigItem::Use {
                path,
                items: Some(items),
            });
        }

        self.expect_stmt_end()?;
        Ok(MigItem::Use { path, items: None })
    }

    fn parse_const(&mut self) -> DecResult<MigItem> {
        self.expect_word("const")?;
        let name = self.expect_ident("const name")?;
        let ty = if self.check(&MigTok::Colon) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(&MigTok::Eq, "'=' in const declaration")?;
        let value = self.parse_expr()?;
        self.expect_stmt_end()?;
        Ok(MigItem::Const { name, ty, value })
    }

    fn parse_struct(&mut self) -> DecResult<MigItem> {
        self.expect_word("struct")?;
        let name = self.expect_ident("struct name")?;
        let mut fields = Vec::new();
        if self.check(&MigTok::LBrace) {
            self.advance();
            while !self.check(&MigTok::RBrace) && !self.check(&MigTok::Eof) {
                let fname = self.expect_ident("field name")?;
                self.expect(&MigTok::Colon, "':' after field name")?;
                let fty = self.parse_type()?;
                fields.push((fname, fty));
                if self.check(&MigTok::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(&MigTok::RBrace, "'}' after struct fields")?;
            self.expect_stmt_end()?;
        } else {
            self.expect(&MigTok::Colon, "':' after struct name")?;
            self.expect(&MigTok::Newline, "newline after struct header")?;
            self.expect(&MigTok::Indent, "indented struct fields")?;
            self.skip_newlines();
            while !self.check(&MigTok::Dedent) && !self.check(&MigTok::Eof) {
                let fname = self.expect_ident("field name")?;
                self.expect(&MigTok::Colon, "':' after field name")?;
                let fty = self.parse_type()?;
                fields.push((fname, fty));
                self.expect_stmt_end()?;
                self.skip_newlines();
            }
            self.expect(&MigTok::Dedent, "dedent after struct")?;
        }
        Ok(MigItem::Struct { name, fields })
    }

    fn parse_enum(&mut self) -> DecResult<MigItem> {
        self.expect_word("enum")?;
        let name = self.expect_ident("enum name")?;
        let mut variants = Vec::new();
        if self.check(&MigTok::LBrace) {
            self.advance();
            while !self.check(&MigTok::RBrace) && !self.check(&MigTok::Eof) {
                let vname = self.expect_ident("variant name")?;
                let mut payloads = Vec::new();
                if self.check(&MigTok::LParen) {
                    self.advance();
                    while !self.check(&MigTok::RParen) {
                        payloads.push(self.parse_type()?);
                        if self.check(&MigTok::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    self.expect(&MigTok::RParen, "')' after variant payloads")?;
                }
                variants.push((vname, payloads));
                if self.check(&MigTok::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(&MigTok::RBrace, "'}' after enum variants")?;
            self.expect_stmt_end()?;
        } else {
            self.expect(&MigTok::Colon, "':' after enum name")?;
            self.expect(&MigTok::Newline, "newline after enum header")?;
            self.expect(&MigTok::Indent, "indented enum variants")?;
            self.skip_newlines();
            while !self.check(&MigTok::Dedent) && !self.check(&MigTok::Eof) {
                let vname = self.expect_ident("variant name")?;
                let mut payloads = Vec::new();
                if self.check(&MigTok::LParen) {
                    self.advance();
                    while !self.check(&MigTok::RParen) {
                        payloads.push(self.parse_type()?);
                        if self.check(&MigTok::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    self.expect(&MigTok::RParen, "')' after variant payloads")?;
                }
                variants.push((vname, payloads));
                self.expect_stmt_end()?;
                self.skip_newlines();
            }
            self.expect(&MigTok::Dedent, "dedent after enum")?;
        }
        Ok(MigItem::Enum { name, variants })
    }

    fn parse_chamber(&mut self) -> DecResult<MigItem> {
        let line = self.line();
        self.expect_word("chamber")?;
        let name = self.expect_ident("chamber name")?;
        self.expect(&MigTok::Colon, "':' after chamber name")?;
        self.expect(&MigTok::Newline, "newline after chamber header")?;
        self.expect(&MigTok::Indent, "indented chamber body")?;
        self.skip_newlines();

        let mut space: Option<MigExpr> = None;
        let mut walls: Option<String> = None;
        let mut engine: Option<String> = None;

        while !self.check(&MigTok::Dedent) && !self.check(&MigTok::Eof) {
            let key = self.expect_ident("chamber property")?;
            self.expect(&MigTok::Eq, "'=' in chamber property")?;
            match key.as_str() {
                "space" => space = Some(self.parse_expr()?),
                "walls" => walls = Some(self.expect_ident("walls policy")?),
                "engine" => engine = Some(self.expect_ident("engine name")?),
                other => {
                    return Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        self.line(),
                        format!("unknown chamber property `{other}`"),
                    ));
                }
            }
            self.expect_stmt_end()?;
            self.skip_newlines();
        }
        self.expect(&MigTok::Dedent, "dedent after chamber")?;
        Ok(MigItem::Chamber {
            name,
            space: space.ok_or_else(|| {
                Diagnostic::at_line(DiagnosticCode::ParseError, line, "chamber missing `space`")
            })?,
            walls: walls.unwrap_or_else(|| "open".to_string()),
            engine,
        })
    }

    fn parse_prepare(&mut self) -> DecResult<MigItem> {
        self.expect_word("prepare")?;
        let name = self.expect_ident("prepare name")?;
        self.expect_word("in")?;
        let chamber = self.expect_ident("chamber name")?;
        self.expect(&MigTok::Colon, "':' after prepare header")?;
        self.expect(&MigTok::Newline, "newline after prepare header")?;
        self.expect(&MigTok::Indent, "indented prepare body")?;
        self.skip_newlines();

        let mut lets = Vec::new();
        let mut clocks = Vec::new();
        let mut injections = Vec::new();

        while !self.check(&MigTok::Dedent) && !self.check(&MigTok::Eof) {
            if self.check_word("clock") {
                self.advance();
                let cname = self.expect_ident("clock name")?;
                self.expect(&MigTok::Eq, "'=' after clock name")?;
                let period = self.parse_expr()?;
                self.expect_stmt_end()?;
                clocks.push((cname, period));
            } else if self.check_word("inject") {
                self.advance();
                let support = self.parse_expr()?;
                self.expect_word("at")?;
                let pos = self.parse_expr()?;
                if self.check(&MigTok::Comma) {
                    self.advance();
                }
                self.expect_word("dir")?;
                let sign = match self.peek().kind {
                    MigTok::Plus => {
                        self.advance();
                        "+"
                    }
                    MigTok::Minus => {
                        self.advance();
                        "-"
                    }
                    _ => "",
                };
                let axis = self.expect_ident("axis X, Y, or Z")?;
                let dir = format!("{sign}{axis}");
                if self.check(&MigTok::Comma) {
                    self.advance();
                }
                self.expect_word("orient")?;
                let orient = self.parse_expr()?;
                if self.check(&MigTok::Comma) {
                    self.advance();
                }
                self.expect_word("shells")?;
                let shells = self.parse_expr()?;
                self.expect_stmt_end()?;
                injections.push(MigInject {
                    support,
                    pos,
                    dir,
                    orient,
                    shells,
                });
            } else {
                lets.push(self.parse_stmt()?);
            }
            self.skip_newlines();
        }
        self.expect(&MigTok::Dedent, "dedent after prepare")?;
        Ok(MigItem::Prepare {
            name,
            chamber,
            lets,
            clocks,
            injections,
        })
    }

    fn parse_observe(&mut self) -> DecResult<MigItem> {
        self.expect_word("observe")?;
        let name = self.expect_ident("observe name")?;
        self.expect(&MigTok::Colon, "':' after observe name")?;
        self.expect(&MigTok::Newline, "newline after observe header")?;
        self.expect(&MigTok::Indent, "indented observe body")?;
        self.skip_newlines();

        let mut probes = Vec::new();
        while !self.check(&MigTok::Dedent) && !self.check(&MigTok::Eof) {
            let kind = self.expect_ident("tracer or sensor")?;
            let pname = self.expect_ident("probe name")?;
            if self.check(&MigTok::LParen) {
                self.advance();
                let param_name = self.expect_ident("view parameter")?;
                let param_ty = if self.check(&MigTok::Colon) {
                    self.advance();
                    Some(self.parse_type()?)
                } else {
                    None
                };
                self.expect(&MigTok::RParen, "')'")?;
                let return_type = if self.check(&MigTok::Arrow) {
                    self.advance();
                    self.parse_type()?
                } else {
                    MigType::Unit
                };
                self.expect(&MigTok::Colon, "':'")?;
                self.expect(&MigTok::Newline, "newline")?;
                self.expect(&MigTok::Indent, "indented probe body")?;
                let body = self.parse_indented_stmts()?;
                probes.push(MigProbe::Procedural {
                    kind,
                    name: pname,
                    param_name,
                    param_ty,
                    return_type,
                    body,
                });
            } else {
                self.expect(&MigTok::Colon, "':'")?;
                self.expect(&MigTok::Newline, "newline")?;
                self.expect(&MigTok::Indent, "indented probe body")?;
                self.skip_newlines();
                if self.check(&MigTok::LParen) {
                    self.advance();
                    let param_name = self.expect_ident("view parameter")?;
                    let param_ty = if self.check(&MigTok::Colon) {
                        self.advance();
                        Some(self.parse_type()?)
                    } else {
                        None
                    };
                    self.expect(&MigTok::RParen, "')'")?;
                    let return_type = if self.check(&MigTok::Arrow) {
                        self.advance();
                        self.parse_type()?
                    } else {
                        MigType::Unit
                    };
                    self.expect(&MigTok::Colon, "':'")?;
                    self.expect(&MigTok::Newline, "newline")?;
                    self.expect(&MigTok::Indent, "indented procedural probe body")?;
                    let body = self.parse_indented_stmts()?;
                    self.skip_newlines();
                    self.expect(&MigTok::Dedent, "dedent after probe")?;
                    probes.push(MigProbe::Procedural {
                        kind,
                        name: pname,
                        param_name,
                        param_ty,
                        return_type,
                        body,
                    });
                } else {
                    let mut region: Option<MigExpr> = None;
                    let mut capture: Vec<String> = Vec::new();
                    while !self.check(&MigTok::Dedent) && !self.check(&MigTok::Eof) {
                        let key = self.expect_ident("region or capture")?;
                        self.expect(&MigTok::Eq, "'='")?;
                        if key == "region" {
                            region = Some(self.parse_expr()?);
                        } else if key == "capture" {
                            self.expect(&MigTok::LBracket, "'['")?;
                            while !self.check(&MigTok::RBracket) {
                                capture.push(self.expect_ident("capture item")?);
                                if self.check(&MigTok::Comma) {
                                    self.advance();
                                } else {
                                    break;
                                }
                            }
                            self.expect(&MigTok::RBracket, "']'")?;
                        }
                        self.expect_stmt_end()?;
                        self.skip_newlines();
                    }
                    let line = self.line();
                    self.expect(&MigTok::Dedent, "dedent after declarative probe")?;
                    probes.push(MigProbe::Declarative {
                        kind,
                        name: pname,
                        region: region.ok_or_else(|| {
                            Diagnostic::at_line(
                                DiagnosticCode::ParseError,
                                line,
                                "declarative probe missing `region`",
                            )
                        })?,
                        capture,
                    });
                }
            }
            self.skip_newlines();
        }
        self.expect(&MigTok::Dedent, "dedent after observe")?;
        Ok(MigItem::Observe { name, probes })
    }

    fn parse_quarantine_mod(&mut self) -> DecResult<MigItem> {
        self.expect_word("quarantine")?;
        self.expect_word("Q_ext")?;
        let name = self.expect_ident("quarantine module name")?;
        self.expect(&MigTok::Colon, "':' after quarantine module name")?;
        self.expect(&MigTok::Newline, "newline")?;
        self.expect(&MigTok::Indent, "indented quarantine items")?;
        self.skip_newlines();
        let mut items = Vec::new();
        while !self.check(&MigTok::Dedent) && !self.check(&MigTok::Eof) {
            if self.check_word("pass") {
                self.advance();
                self.expect_stmt_end()?;
            } else {
                items.push(self.parse_item()?);
            }
            self.skip_newlines();
        }
        self.expect(&MigTok::Dedent, "dedent after quarantine module")?;
        Ok(MigItem::QuarantineMod { name, items })
    }

    fn parse_type(&mut self) -> DecResult<MigType> {
        let p = self.peek().clone();
        match p.kind {
            MigTok::LParen => {
                self.advance();
                if self.check(&MigTok::RParen) {
                    self.advance();
                    return Ok(MigType::Unit);
                }
                let mut elems = Vec::new();
                while !self.check(&MigTok::RParen) {
                    elems.push(self.parse_type()?);
                    if self.check(&MigTok::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&MigTok::RParen, "')' in tuple type")?;
                if elems.len() == 1 {
                    Ok(elems.into_iter().next().expect("1 elem"))
                } else {
                    Ok(MigType::Tuple(elems))
                }
            }
            MigTok::LBracket => {
                self.advance();
                let inner = self.parse_type()?;
                if self.check(&MigTok::Semi) {
                    self.advance();
                    let p_len = self.peek().clone();
                    let MigTok::IntLit(n) = p_len.kind else {
                        return Err(Diagnostic::at(
                            DiagnosticCode::ParseError,
                            p_len.line,
                            p_len.col,
                            "expected integer length in `[T; N]`",
                        ));
                    };
                    self.advance();
                    self.expect(&MigTok::RBracket, "']' in `[T; N]`")?;
                    Ok(MigType::Array(Box::new(inner), n))
                } else {
                    self.expect(&MigTok::RBracket, "']' in `[T]`")?;
                    Ok(MigType::List(Box::new(inner)))
                }
            }
            MigTok::Ident(ref s) if s == "array" && self.peek_n(1).kind == MigTok::LBracket => {
                self.advance();
                self.advance();
                let inner = self.parse_type()?;
                self.expect(&MigTok::Comma, "',' in `array[T, N]`")?;
                let p_len = self.peek().clone();
                let MigTok::IntLit(n) = p_len.kind else {
                    return Err(Diagnostic::at(
                        DiagnosticCode::ParseError,
                        p_len.line,
                        p_len.col,
                        "expected integer length in `array[T, N]`",
                    ));
                };
                self.advance();
                self.expect(&MigTok::RBracket, "']' in `array[T, N]`")?;
                Ok(MigType::Array(Box::new(inner), n))
            }
            MigTok::Ident(ref s) if s == "map" && self.peek_n(1).kind == MigTok::LBracket => {
                self.advance();
                self.advance();
                let k = self.parse_type()?;
                self.expect(&MigTok::Comma, "',' in `map[K, V]`")?;
                let v = self.parse_type()?;
                self.expect(&MigTok::RBracket, "']' in `map[K, V]`")?;
                Ok(MigType::Map(Box::new(k), Box::new(v)))
            }
            MigTok::Ident(name) => {
                self.advance();
                let mut segs = vec![name];
                while self.check(&MigTok::ColonColon) {
                    self.advance();
                    segs.push(self.expect_ident("type path segment")?);
                }
                let joined = segs.join("::");
                if self.check(&MigTok::Lt) {
                    self.advance();
                    let inner = self.parse_type()?;
                    self.expect(&MigTok::Gt, "'>' in generic type")?;
                    match joined.as_str() {
                        "ReadOnly" => Ok(MigType::ReadOnly(Box::new(inner))),
                        "QExt" => Ok(MigType::QExt(Box::new(inner))),
                        "List" => Ok(MigType::List(Box::new(inner))),
                        _ => Err(Diagnostic::at(
                            DiagnosticCode::ParseError,
                            p.line,
                            p.col,
                            format!("unsupported generic type `{joined}`"),
                        )),
                    }
                } else {
                    Ok(MigType::Named(joined))
                }
            }
            _ => Err(Diagnostic::at(
                DiagnosticCode::ParseError,
                p.line,
                p.col,
                format!("expected type expression, found {:?}", p.kind),
            )),
        }
    }

    fn looks_like_dec_arm(&self) -> bool {
        if !self.check(&MigTok::LParen) {
            return false;
        }
        let mut depth = 0usize;
        let mut idx = self.pos;
        while idx < self.tokens.len() {
            match &self.tokens[idx].kind {
                MigTok::LParen => depth += 1,
                MigTok::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        let next = self.tokens.get(idx + 1).map(|t| &t.kind);
                        return matches!(
                            next,
                            Some(MigTok::Arrow)
                                | Some(MigTok::Ident(_))
                        );
                    }
                }
                MigTok::Newline | MigTok::Eof => return false,
                _ => {}
            }
            idx += 1;
        }
        false
    }

    fn parse_dec_fn(&mut self) -> DecResult<MigFn> {
        let kw = self.expect_ident("'dec' or 'Dec'")?;
        let is_upper_dec = kw == "Dec";
        let name = self.expect_ident("function name")?;

        // 1. Matrix form `dec <name>:`
        if self.check(&MigTok::Colon) {
            self.advance();
            self.expect(&MigTok::Newline, "newline after `dec name:`")?;
            self.expect(&MigTok::Indent, "indented signature arms")?;
            self.skip_newlines();
            let mut arms = Vec::new();
            while !self.check(&MigTok::Dedent) && !self.check(&MigTok::Eof) {
                arms.push(self.parse_dec_arm()?);
                self.skip_newlines();
            }
            self.expect(&MigTok::Dedent, "dedent after `dec` arms")?;
            return Ok(MigFn {
                name,
                is_v1_origin: false,
                arms,
            });
        }

        // 2. Header with `(...)`
        self.expect(&MigTok::LParen, "'(' after function name")?;
        let mut header_params: Vec<MigArmParam> = Vec::new();
        let mut bare_names: Vec<String> = Vec::new();
        if !self.check(&MigTok::RParen) {
            loop {
                if matches!(&self.peek().kind, MigTok::Ident(_))
                    && self.peek_n(1).kind == MigTok::Colon
                {
                    let pname = self.expect_ident("parameter name")?;
                    self.advance(); // ':'
                    let pty = self.parse_type()?;
                    header_params.push(MigArmParam::TypedVar {
                        name: pname,
                        ty: pty,
                    });
                } else if matches!(&self.peek().kind, MigTok::Ident(w) if is_legacy_v1_type_word(w))
                    && matches!(&self.peek_n(1).kind, MigTok::Ident(_))
                {
                    let pty = self.parse_type()?;
                    let pname = self.expect_ident("parameter name")?;
                    header_params.push(MigArmParam::TypedVar {
                        name: pname,
                        ty: pty,
                    });
                } else {
                    bare_names.push(self.expect_ident("parameter name")?);
                }
                if self.check(&MigTok::Comma) {
                    self.advance();
                    if self.check(&MigTok::RParen) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        self.expect(&MigTok::RParen, "')'")?;

        let mut header_ret: Option<MigType> = None;
        let mut header_out: Option<String> = None;
        if self.check(&MigTok::Arrow) {
            self.advance();
            if matches!(&self.peek().kind, MigTok::Ident(_))
                && self.peek_n(1).kind == MigTok::Colon
            {
                // Could be `-> int:` (PR #8 return type) OR `-> result:` (v1 output binding)!
                // Disambiguated right after indent via `looks_like_dec_arm()`.
                let tok_s = self.expect_ident("return type or output binding")?;
                header_ret = Some(MigType::Named(tok_s.clone()));
                header_out = Some(tok_s);
            } else {
                let rty = self.parse_type()?;
                let out_opt = if matches!(&self.peek().kind, MigTok::Ident(_)) {
                    Some(self.expect_ident("output variable")?)
                } else {
                    None
                };
                header_ret = Some(rty);
                header_out = out_opt;
            }
        }

        self.expect(&MigTok::Colon, "':' after function header")?;
        self.expect(&MigTok::Newline, "newline after function header")?;
        self.expect(&MigTok::Indent, "indented function body")?;
        self.skip_newlines();

        if self.looks_like_dec_arm() {
            let mut arms = Vec::new();
            while !self.check(&MigTok::Dedent) && !self.check(&MigTok::Eof) {
                let mut arm = self.parse_dec_arm()?;
                if !header_params.is_empty() && arm.params.len() == header_params.len() {
                    for (ap, hp) in arm.params.iter_mut().zip(&header_params) {
                        if let (
                            MigArmParam::Pattern(MigPat::Binding(bname)),
                            MigArmParam::TypedVar { ty, .. },
                        ) = (ap.clone(), hp)
                        {
                            *ap = MigArmParam::TypedVar {
                                name: bname,
                                ty: ty.clone(),
                            };
                        }
                    }
                }
                arms.push(arm);
                self.skip_newlines();
            }
            self.expect(&MigTok::Dedent, "dedent after arms")?;
            let is_v1_origin = is_upper_dec && arms.iter().any(|a| a.output_name.is_some());
            return Ok(MigFn {
                name,
                is_v1_origin,
                arms,
            });
        }

        let body = self.parse_indented_stmts()?;
        // If `header_ret` was set from a single identifier before `:`, `header_out` was speculative
        // only for multi-arm v1 headers; for a single-body `Dec f(...) -> int:`, there is NO output var
        // unless `-> <type> <out_var>:` had BOTH type and out_var!
        let single_out = match (&header_ret, &header_out) {
            (Some(MigType::Named(rn)), Some(on)) if rn == on => None,
            _ => header_out,
        };
        let is_v1_origin = is_upper_dec && single_out.is_some();
        Ok(MigFn {
            name,
            is_v1_origin,
            arms: vec![MigArm {
                params: header_params,
                guard: None,
                return_type: header_ret.unwrap_or(MigType::Unit),
                output_name: single_out,
                body,
            }],
        })
    }

    fn parse_dec_arm(&mut self) -> DecResult<MigArm> {
        self.expect(&MigTok::LParen, "'(' starting signature arm")?;
        let mut params = Vec::new();
        if !self.check(&MigTok::RParen) {
            loop {
                params.push(self.parse_dec_arm_param()?);
                if self.check(&MigTok::Comma) {
                    self.advance();
                    if self.check(&MigTok::RParen) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
        self.expect(&MigTok::RParen, "')'")?;

        let guard = if self.check_word("where") || self.check_word("if") {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        let (return_type, output_name) = if self.check(&MigTok::Arrow) {
            self.advance();
            let rty = self.parse_type()?;
            let out_n = if matches!(&self.peek().kind, MigTok::Ident(_)) {
                Some(self.expect_ident("arm output variable")?)
            } else {
                None
            };
            (rty, out_n)
        } else {
            (MigType::Unit, None)
        };

        self.expect(&MigTok::Colon, "':' after arm signature")?;
        let body = if self.check(&MigTok::Newline) {
            self.advance();
            self.expect(&MigTok::Indent, "indented arm body")?;
            self.parse_indented_stmts()?
        } else {
            vec![self.parse_stmt()?]
        };

        Ok(MigArm {
            params,
            guard,
            return_type,
            output_name,
            body,
        })
    }

    fn parse_dec_arm_param(&mut self) -> DecResult<MigArmParam> {
        // 1. `name: Type` (when `name` is a plain identifier or `_`)
        if matches!(&self.peek().kind, MigTok::Ident(_)) && self.peek_n(1).kind == MigTok::Colon {
            let name = self.expect_ident("parameter name")?;
            self.advance(); // ':'
            let ty = self.parse_type()?;
            if name == "_" {
                return Ok(MigArmParam::TypedPattern {
                    pat: MigPat::Wildcard,
                    ty,
                });
            }
            return Ok(MigArmParam::TypedVar { name, ty });
        }
        // 2. Legacy v1 `<type> <name>`
        if (matches!(&self.peek().kind, MigTok::Ident(w) if is_legacy_v1_type_word(w))
            && matches!(&self.peek_n(1).kind, MigTok::Ident(_)))
            || (self.check(&MigTok::LBracket)
                && matches!(&self.peek_n(1).kind, MigTok::Ident(w) if is_legacy_v1_type_word(w))
                && self.peek_n(2).kind == MigTok::RBracket
                && matches!(&self.peek_n(3).kind, MigTok::Ident(_)))
        {
            let ty = self.parse_type()?;
            let name = self.expect_ident("parameter name")?;
            return Ok(MigArmParam::TypedVar { name, ty });
        }
        // 3. Pattern or `pattern: Type`
        let pat = self.parse_pattern()?;
        if self.check(&MigTok::Colon) {
            self.advance();
            let ty = self.parse_type()?;
            Ok(MigArmParam::TypedPattern { pat, ty })
        } else {
            Ok(MigArmParam::Pattern(pat))
        }
    }

    fn parse_pattern(&mut self) -> DecResult<MigPat> {
        let p = self.peek().clone();
        match p.kind {
            MigTok::Minus => {
                self.advance();
                let next = self.peek().clone();
                match next.kind {
                    MigTok::IntLit(n) => {
                        self.advance();
                        Ok(MigPat::IntLit(format!("-{n}")))
                    }
                    MigTok::TritLit(t) => {
                        self.advance();
                        Ok(MigPat::TritLit(format!("-{t}")))
                    }
                    MigTok::RatioLit(r) => {
                        self.advance();
                        Ok(MigPat::RatioLit(format!("-{r}")))
                    }
                    _ => Err(Diagnostic::at(
                        DiagnosticCode::ParseError,
                        p.line,
                        p.col,
                        "invalid negative pattern",
                    )),
                }
            }
            MigTok::Plus => {
                self.advance();
                let next = self.peek().clone();
                match next.kind {
                    MigTok::TritLit(t) => {
                        self.advance();
                        Ok(MigPat::TritLit(t))
                    }
                    MigTok::IntLit(n) => {
                        self.advance();
                        Ok(MigPat::IntLit(n))
                    }
                    _ => Err(Diagnostic::at(
                        DiagnosticCode::ParseError,
                        p.line,
                        p.col,
                        "invalid positive pattern",
                    )),
                }
            }
            MigTok::IntLit(n) => {
                self.advance();
                Ok(MigPat::IntLit(n))
            }
            MigTok::BytesLit(bs) | MigTok::StrLit(bs) => {
                self.advance();
                Ok(MigPat::BytesLit(bs))
            }
            MigTok::TritLit(t) => {
                self.advance();
                Ok(MigPat::TritLit(t))
            }
            MigTok::RatioLit(r) => {
                self.advance();
                Ok(MigPat::RatioLit(r))
            }
            MigTok::Addr8Lit(a) => {
                self.advance();
                Ok(MigPat::Addr8Lit(a))
            }
            MigTok::SupportLit(s) => {
                self.advance();
                Ok(MigPat::SupportLit(s))
            }
            MigTok::LParen => {
                self.advance();
                let mut elems = Vec::new();
                while !self.check(&MigTok::RParen) {
                    elems.push(self.parse_pattern()?);
                    if self.check(&MigTok::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&MigTok::RParen, "')' in tuple pattern")?;
                Ok(MigPat::Tuple(elems))
            }
            MigTok::Ident(s) => {
                self.advance();
                match s.as_str() {
                    "_" => Ok(MigPat::Wildcard),
                    "null" => Ok(MigPat::NullLit),
                    "true" => Ok(MigPat::BoolLit(true)),
                    "false" => Ok(MigPat::BoolLit(false)),
                    _ => {
                        let mut path = vec![s];
                        while self.check(&MigTok::ColonColon) {
                            self.advance();
                            path.push(self.expect_ident("pattern path segment")?);
                        }
                        if self.check(&MigTok::LParen) {
                            self.advance();
                            let mut args = Vec::new();
                            while !self.check(&MigTok::RParen) {
                                args.push(self.parse_pattern()?);
                                if self.check(&MigTok::Comma) {
                                    self.advance();
                                } else {
                                    break;
                                }
                            }
                            self.expect(&MigTok::RParen, "')' in constructor pattern")?;
                            Ok(MigPat::EnumOrCtor { path, args })
                        } else if path.len() > 1 {
                            Ok(MigPat::EnumOrCtor {
                                path,
                                args: Vec::new(),
                            })
                        } else {
                            Ok(MigPat::Binding(path.into_iter().next().expect("1")))
                        }
                    }
                }
            }
            _ => Err(Diagnostic::at(
                DiagnosticCode::ParseError,
                p.line,
                p.col,
                format!("unexpected token {:?} in pattern", p.kind),
            )),
        }
    }

    fn parse_indented_stmts(&mut self) -> DecResult<Vec<MigStmt>> {
        self.skip_newlines();
        let mut stmts = Vec::new();
        while !self.check(&MigTok::Dedent) && !self.check(&MigTok::Eof) {
            stmts.push(self.parse_stmt()?);
            self.skip_newlines();
        }
        self.expect(&MigTok::Dedent, "dedent")?;
        Ok(stmts)
    }

    fn parse_stmt(&mut self) -> DecResult<MigStmt> {
        if self.check_word("let") || self.check_word("mut") {
            let mut mutable = false;
            if self.check_word("let") {
                self.advance();
                if self.check_word("mut") {
                    self.advance();
                    mutable = true;
                }
            } else {
                self.advance();
                mutable = true;
            }

            if self.check(&MigTok::LParen) {
                self.advance();
                let mut elems = Vec::new();
                while !self.check(&MigTok::RParen) {
                    let ename = self.expect_ident("tuple binding")?;
                    let ety = if self.check(&MigTok::Colon) {
                        self.advance();
                        Some(self.parse_type()?)
                    } else {
                        None
                    };
                    elems.push((ename, ety));
                    if self.check(&MigTok::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&MigTok::RParen, "')' in tuple let")?;
                let ty = if self.check(&MigTok::Colon) {
                    self.advance();
                    Some(self.parse_type()?)
                } else {
                    None
                };
                self.expect(&MigTok::Eq, "'='")?;
                let value = self.parse_expr()?;
                self.expect_stmt_end()?;
                return Ok(MigStmt::Let {
                    mutable,
                    pat: MigLetPat::Tuple(elems),
                    ty,
                    value,
                });
            }

            // `let <type> <name> = <expr>` vs `let <name>[: <type>] = <expr>`
            if matches!(&self.peek().kind, MigTok::Ident(w) if is_legacy_v1_type_word(w))
                && matches!(&self.peek_n(1).kind, MigTok::Ident(_))
                && self.peek_n(2).kind == MigTok::Eq
            {
                let ty = self.parse_type()?;
                let name = self.expect_ident("binding name")?;
                self.expect(&MigTok::Eq, "'='")?;
                let value = self.parse_expr()?;
                self.expect_stmt_end()?;
                return Ok(MigStmt::Let {
                    mutable,
                    pat: MigLetPat::Single(name),
                    ty: Some(ty),
                    value,
                });
            }

            let name = self.expect_ident("binding name")?;
            let ty = if self.check(&MigTok::Colon) {
                self.advance();
                Some(self.parse_type()?)
            } else {
                None
            };
            self.expect(&MigTok::Eq, "'='")?;
            let value = self.parse_expr()?;
            self.expect_stmt_end()?;
            return Ok(MigStmt::Let {
                mutable,
                pat: MigLetPat::Single(name),
                ty,
                value,
            });
        }

        if self.check_word("assert") {
            self.advance();
            let cond = self.parse_expr()?;
            self.expect_stmt_end()?;
            return Ok(MigStmt::Assert(cond));
        }

        if self.check_word("emit") {
            self.advance();
            let val = self.parse_expr()?;
            self.expect_stmt_end()?;
            return Ok(MigStmt::Emit(val));
        }

        if self.check_word("return") {
            self.advance();
            let val = if self.check(&MigTok::Newline)
                || self.check(&MigTok::Dedent)
                || self.check(&MigTok::Eof)
            {
                None
            } else {
                Some(self.parse_expr()?)
            };
            self.expect_stmt_end()?;
            return Ok(MigStmt::Return(val));
        }

        if self.check_word("pass") {
            self.advance();
            self.expect_stmt_end()?;
            return Ok(MigStmt::Pass);
        }

        if self.check_word("quarantine") {
            self.advance();
            self.expect_word("Q_ext")?;
            self.expect(&MigTok::Colon, "':' after `quarantine Q_ext`")?;
            self.expect(&MigTok::Newline, "newline")?;
            self.expect(&MigTok::Indent, "indented quarantine block")?;
            let body = self.parse_indented_stmts()?;
            return Ok(MigStmt::QuarantineBlock(body));
        }

        if self.check_word("if") {
            self.advance();
            let cond = self.parse_expr_no_struct_lit()?;
            self.expect(&MigTok::Colon, "':' after `if` condition")?;
            self.expect(&MigTok::Newline, "newline")?;
            self.expect(&MigTok::Indent, "indented `if` body")?;
            let then_body = self.parse_indented_stmts()?;
            let mut branches = vec![(cond, then_body)];
            self.skip_newlines();
            while self.check_word("elif") {
                self.advance();
                let ec = self.parse_expr_no_struct_lit()?;
                self.expect(&MigTok::Colon, "':' after `elif` condition")?;
                self.expect(&MigTok::Newline, "newline")?;
                self.expect(&MigTok::Indent, "indented `elif` body")?;
                let eb = self.parse_indented_stmts()?;
                branches.push((ec, eb));
                self.skip_newlines();
            }
            let else_body = if self.check_word("else") {
                self.advance();
                self.expect(&MigTok::Colon, "':' after `else`")?;
                self.expect(&MigTok::Newline, "newline")?;
                self.expect(&MigTok::Indent, "indented `else` body")?;
                self.parse_indented_stmts()?
            } else {
                Vec::new()
            };
            return Ok(MigStmt::If {
                branches,
                else_body,
            });
        }

        if self.check_word("for") {
            self.advance();
            let var = self.expect_ident("loop induction variable")?;
            self.expect_word("in")?;
            let iter = self.parse_expr_no_struct_lit()?;
            self.expect(&MigTok::Colon, "':' after `for` header")?;
            self.expect(&MigTok::Newline, "newline")?;
            self.expect(&MigTok::Indent, "indented `for` body")?;
            let body = self.parse_indented_stmts()?;
            return Ok(MigStmt::For { var, iter, body });
        }

        // Legacy v1 `<type> <name> = <expr>`
        if matches!(&self.peek().kind, MigTok::Ident(w) if is_legacy_v1_type_word(w))
            && matches!(&self.peek_n(1).kind, MigTok::Ident(_))
            && self.peek_n(2).kind == MigTok::Eq
        {
            let ty = self.parse_type()?;
            let name = self.expect_ident("local variable name")?;
            self.expect(&MigTok::Eq, "'='")?;
            let value = self.parse_expr()?;
            self.expect_stmt_end()?;
            return Ok(MigStmt::Let {
                mutable: true,
                pat: MigLetPat::Single(name),
                ty: Some(ty),
                value,
            });
        }

        let lhs = self.parse_expr()?;
        if self.check(&MigTok::Eq) {
            let line = self.line();
            self.advance();
            let value = self.parse_expr()?;
            self.expect_stmt_end()?;
            return match lhs {
                MigExpr::Ident(name) => Ok(MigStmt::AssignVar { name, value }),
                MigExpr::Index { base, index } => match *base {
                    MigExpr::Ident(name) => Ok(MigStmt::AssignIndex { name, index: *index, value }),
                    _ => Err(Diagnostic::at_line(
                        DiagnosticCode::ParseError,
                        line,
                        "nested index assignment target is not supported",
                    )),
                },
                _ => Err(Diagnostic::at_line(
                    DiagnosticCode::ParseError,
                    line,
                    "unsupported assignment target during migration",
                )),
            };
        }

        self.expect_stmt_end()?;
        Ok(MigStmt::ExprStmt(lhs))
    }

    fn parse_expr_no_struct_lit(&mut self) -> DecResult<MigExpr> {
        let prev = self.allow_struct_lit;
        self.allow_struct_lit = false;
        let res = self.parse_expr();
        self.allow_struct_lit = prev;
        res
    }

    fn parse_expr(&mut self) -> DecResult<MigExpr> {
        self.parse_pipe_expr()
    }

    fn parse_pipe_expr(&mut self) -> DecResult<MigExpr> {
        let mut expr = self.parse_cond_expr()?;
        while self.check(&MigTok::Pipe) {
            self.advance();
            let stage = self.parse_pipe_stage()?;
            expr = MigExpr::Pipe {
                input: Box::new(expr),
                stage: Box::new(stage),
            };
        }
        Ok(expr)
    }

    fn parse_pipe_stage(&mut self) -> DecResult<MigPipeStage> {
        if let MigTok::Adverb(op) = self.peek().kind.clone() {
            self.advance();
            return Ok(MigPipeStage::Adverb(op));
        }
        if self.check(&MigTok::Boundary) {
            self.advance();
            return Ok(MigPipeStage::Boundary);
        }
        if self.check(&MigTok::LParen) {
            if let Some(sec_op) = self.peek_section_binop(1) {
                self.advance(); // '('
                self.advance(); // op
                let rhs = self.parse_expr()?;
                self.expect(&MigTok::RParen, "')' in operator section")?;
                return Ok(MigPipeStage::Section { op: sec_op, rhs });
            }
        }
        let target = self.parse_postfix_expr()?;
        if let MigExpr::Call { callee, args } = target {
            let extra_args = args.into_iter().map(|(_, e)| e).collect();
            Ok(MigPipeStage::Call {
                callee: *callee,
                extra_args,
            })
        } else {
            Ok(MigPipeStage::Call {
                callee: target,
                extra_args: Vec::new(),
            })
        }
    }

    fn peek_section_binop(&self, offset: usize) -> Option<String> {
        match &self.peek_n(offset).kind {
            MigTok::Plus => Some("+".to_string()),
            MigTok::Minus => Some("-".to_string()),
            MigTok::Star => Some("*".to_string()),
            MigTok::Slash => Some("/".to_string()),
            MigTok::SlashSlash => Some("//".to_string()),
            MigTok::Percent => Some("%".to_string()),
            MigTok::EqEq => Some("==".to_string()),
            MigTok::NotEq => Some("!=".to_string()),
            MigTok::Lt => Some("<".to_string()),
            MigTok::LtEq => Some("<=".to_string()),
            MigTok::Gt => Some(">".to_string()),
            MigTok::GtEq => Some(">=".to_string()),
            _ => None,
        }
    }

    fn parse_cond_expr(&mut self) -> DecResult<MigExpr> {
        let then_expr = self.parse_or_expr()?;
        if self.check_word("if") {
            self.advance();
            let cond = self.parse_or_expr()?;
            self.expect_word("else")?;
            let else_expr = self.parse_cond_expr()?;
            Ok(MigExpr::Conditional {
                then_expr: Box::new(then_expr),
                cond: Box::new(cond),
                else_expr: Box::new(else_expr),
            })
        } else {
            Ok(then_expr)
        }
    }

    fn parse_or_expr(&mut self) -> DecResult<MigExpr> {
        let mut expr = self.parse_and_expr()?;
        while self.check_word("or") {
            self.advance();
            let rhs = self.parse_and_expr()?;
            expr = MigExpr::Binary {
                op: "or".to_string(),
                left: Box::new(expr),
                right: Box::new(rhs),
            };
        }
        Ok(expr)
    }

    fn parse_and_expr(&mut self) -> DecResult<MigExpr> {
        let mut expr = self.parse_not_expr()?;
        while self.check_word("and") {
            self.advance();
            let rhs = self.parse_not_expr()?;
            expr = MigExpr::Binary {
                op: "and".to_string(),
                left: Box::new(expr),
                right: Box::new(rhs),
            };
        }
        Ok(expr)
    }

    fn parse_not_expr(&mut self) -> DecResult<MigExpr> {
        if self.check_word("not") {
            self.advance();
            let operand = self.parse_not_expr()?;
            Ok(MigExpr::Unary {
                op: "not".to_string(),
                operand: Box::new(operand),
            })
        } else {
            self.parse_sieve_expr()
        }
    }

    fn parse_sieve_expr(&mut self) -> DecResult<MigExpr> {
        let mut expr = self.parse_cmp_expr()?;
        while self.check(&MigTok::Sieve) {
            self.advance();
            let rhs = self.parse_cmp_expr()?;
            expr = MigExpr::Binary {
                op: "?>".to_string(),
                left: Box::new(expr),
                right: Box::new(rhs),
            };
        }
        Ok(expr)
    }

    fn parse_cmp_expr(&mut self) -> DecResult<MigExpr> {
        let mut expr = self.parse_range_expr()?;
        loop {
            let op = match &self.peek().kind {
                MigTok::EqEq => "==",
                MigTok::NotEq => "!=",
                MigTok::Lt => "<",
                MigTok::LtEq => "<=",
                MigTok::Gt => ">",
                MigTok::GtEq => ">=",
                MigTok::Cmp3 => "<=>",
                _ => break,
            };
            let op_s = op.to_string();
            self.advance();
            let rhs = self.parse_range_expr()?;
            expr = MigExpr::Binary {
                op: op_s,
                left: Box::new(expr),
                right: Box::new(rhs),
            };
        }
        Ok(expr)
    }

    fn parse_range_expr(&mut self) -> DecResult<MigExpr> {
        let start = self.parse_bitor_expr()?;
        if self.check(&MigTok::DotDot) {
            self.advance();
            let end = self.parse_bitor_expr()?;
            Ok(MigExpr::Range {
                start: Box::new(start),
                end: Box::new(end),
            })
        } else {
            Ok(start)
        }
    }

    fn parse_bitor_expr(&mut self) -> DecResult<MigExpr> {
        let mut expr = self.parse_bitxor_expr()?;
        while self.check(&MigTok::Bar) {
            self.advance();
            let rhs = self.parse_bitxor_expr()?;
            expr = MigExpr::Binary {
                op: "|".to_string(),
                left: Box::new(expr),
                right: Box::new(rhs),
            };
        }
        Ok(expr)
    }

    fn parse_bitxor_expr(&mut self) -> DecResult<MigExpr> {
        let mut expr = self.parse_bitand_expr()?;
        while self.check(&MigTok::Caret) {
            self.advance();
            let rhs = self.parse_bitand_expr()?;
            expr = MigExpr::Binary {
                op: "^".to_string(),
                left: Box::new(expr),
                right: Box::new(rhs),
            };
        }
        Ok(expr)
    }

    fn parse_bitand_expr(&mut self) -> DecResult<MigExpr> {
        let mut expr = self.parse_shift_expr()?;
        while self.check(&MigTok::Amp) {
            self.advance();
            let rhs = self.parse_shift_expr()?;
            expr = MigExpr::Binary {
                op: "&".to_string(),
                left: Box::new(expr),
                right: Box::new(rhs),
            };
        }
        Ok(expr)
    }

    fn parse_shift_expr(&mut self) -> DecResult<MigExpr> {
        let mut expr = self.parse_add_expr()?;
        loop {
            let op = match &self.peek().kind {
                MigTok::Shl => "<<",
                MigTok::Shr => ">>",
                _ => break,
            };
            let op_s = op.to_string();
            self.advance();
            let rhs = self.parse_add_expr()?;
            expr = MigExpr::Binary {
                op: op_s,
                left: Box::new(expr),
                right: Box::new(rhs),
            };
        }
        Ok(expr)
    }

    fn parse_add_expr(&mut self) -> DecResult<MigExpr> {
        let mut expr = self.parse_mul_expr()?;
        loop {
            let op = match &self.peek().kind {
                MigTok::Plus => "+",
                MigTok::Minus => "-",
                MigTok::OuterAdd => "+:",
                MigTok::Ident(s) if s == "min" && !matches!(self.peek_n(1).kind, MigTok::LParen) => {
                    "min"
                }
                MigTok::Ident(s) if s == "max" && !matches!(self.peek_n(1).kind, MigTok::LParen) => {
                    "max"
                }
                _ => break,
            };
            let op_s = op.to_string();
            self.advance();
            let rhs = self.parse_mul_expr()?;
            expr = MigExpr::Binary {
                op: op_s,
                left: Box::new(expr),
                right: Box::new(rhs),
            };
        }
        Ok(expr)
    }

    fn parse_mul_expr(&mut self) -> DecResult<MigExpr> {
        let mut expr = self.parse_unary_expr()?;
        loop {
            let op = match &self.peek().kind {
                MigTok::Star => "*",
                MigTok::Slash => "/",
                MigTok::SlashSlash => "//",
                MigTok::Percent => "%",
                MigTok::OuterMul => "*:",
                _ => break,
            };
            let op_s = op.to_string();
            self.advance();
            let rhs = self.parse_unary_expr()?;
            expr = MigExpr::Binary {
                op: op_s,
                left: Box::new(expr),
                right: Box::new(rhs),
            };
        }
        Ok(expr)
    }

    fn parse_unary_expr(&mut self) -> DecResult<MigExpr> {
        match self.peek().kind.clone() {
            MigTok::Minus => {
                self.advance();
                let operand = self.parse_unary_expr()?;
                if let MigExpr::TritLit(ref t) = operand {
                    if t == "1t" {
                        return Ok(MigExpr::TritLit("-1t".to_string()));
                    }
                }
                Ok(MigExpr::Unary {
                    op: "-".to_string(),
                    operand: Box::new(operand),
                })
            }
            MigTok::Plus => {
                self.advance();
                let operand = self.parse_unary_expr()?;
                if let MigExpr::TritLit(ref t) = operand {
                    if t == "1t" {
                        return Ok(MigExpr::TritLit("1t".to_string()));
                    }
                }
                Ok(MigExpr::Unary {
                    op: "+".to_string(),
                    operand: Box::new(operand),
                })
            }
            MigTok::Tilde => {
                self.advance();
                let operand = self.parse_unary_expr()?;
                Ok(MigExpr::Unary {
                    op: "~".to_string(),
                    operand: Box::new(operand),
                })
            }
            MigTok::Boundary => {
                self.advance();
                let operand = self.parse_unary_expr()?;
                Ok(MigExpr::Unary {
                    op: "∂".to_string(),
                    operand: Box::new(operand),
                })
            }
            MigTok::Adverb(op) => {
                self.advance();
                let operand = self.parse_unary_expr()?;
                Ok(MigExpr::Adverb {
                    op,
                    operand: Box::new(operand),
                })
            }
            _ => self.parse_postfix_expr(),
        }
    }

    fn parse_postfix_expr(&mut self) -> DecResult<MigExpr> {
        let mut expr = self.parse_primary_expr()?;
        loop {
            if self.check(&MigTok::LParen) {
                self.advance();
                let mut args = Vec::new();
                while !self.check(&MigTok::RParen) {
                    let arg_name = if matches!(&self.peek().kind, MigTok::Ident(_))
                        && self.peek_n(1).kind == MigTok::Colon
                    {
                        let n = self.expect_ident("argument label")?;
                        self.advance(); // ':'
                        Some(n)
                    } else {
                        None
                    };
                    let val = self.parse_expr()?;
                    args.push((arg_name, val));
                    if self.check(&MigTok::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&MigTok::RParen, "')' after call arguments")?;
                expr = MigExpr::Call {
                    callee: Box::new(expr),
                    args,
                };
            } else if self.check(&MigTok::Dot) {
                self.advance();
                let fname = self.expect_ident("field name")?;
                expr = MigExpr::Field {
                    base: Box::new(expr),
                    name: fname,
                };
            } else if self.check(&MigTok::LBracket) {
                self.advance();
                let idx = self.parse_expr()?;
                self.expect(&MigTok::RBracket, "']'")?;
                expr = MigExpr::Index {
                    base: Box::new(expr),
                    index: Box::new(idx),
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_primary_expr(&mut self) -> DecResult<MigExpr> {
        let p = self.peek().clone();
        match p.kind {
            MigTok::IntLit(n) => {
                self.advance();
                Ok(MigExpr::IntLit(n))
            }
            MigTok::RatioLit(r) => {
                self.advance();
                Ok(MigExpr::RatioLit(r))
            }
            MigTok::TritLit(t) => {
                self.advance();
                Ok(MigExpr::TritLit(t))
            }
            MigTok::Addr8Lit(a) => {
                self.advance();
                Ok(MigExpr::Addr8Lit(a))
            }
            MigTok::SupportLit(s) => {
                self.advance();
                Ok(MigExpr::SupportLit(s))
            }
            MigTok::BytesLit(bs) => {
                self.advance();
                Ok(MigExpr::BytesLit(bs))
            }
            MigTok::StrLit(bs) => {
                self.advance();
                Ok(MigExpr::StrLit(bs))
            }
            MigTok::LParen => {
                self.advance();
                if self.check(&MigTok::RParen) {
                    self.advance();
                    return Ok(MigExpr::Tuple(Vec::new()));
                }
                let first = self.parse_expr()?;
                if self.check(&MigTok::Comma) {
                    self.advance();
                    if self.check(&MigTok::RBracket) {
                        return Err(Diagnostic::at_line(
                            DiagnosticCode::ParseError,
                            p.line,
                            "half-open window `(u, v]` requires two bounds",
                        ));
                    }
                    if self.check(&MigTok::RParen) {
                        self.advance();
                        return Ok(MigExpr::Tuple(vec![first]));
                    }
                    let second = self.parse_expr()?;
                    if self.check(&MigTok::RBracket) {
                        self.advance();
                        return Ok(MigExpr::WindowLit {
                            start: Box::new(first),
                            end: Box::new(second),
                        });
                    }
                    let mut items = vec![first, second];
                    while self.check(&MigTok::Comma) {
                        self.advance();
                        if self.check(&MigTok::RParen) {
                            break;
                        }
                        items.push(self.parse_expr()?);
                    }
                    self.expect(&MigTok::RParen, "')'")?;
                    Ok(MigExpr::Tuple(items))
                } else {
                    self.expect(&MigTok::RParen, "')'")?;
                    Ok(first)
                }
            }
            MigTok::LBracket => {
                self.advance();
                let mut items = Vec::new();
                while !self.check(&MigTok::RBracket) {
                    items.push(self.parse_expr()?);
                    if self.check(&MigTok::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&MigTok::RBracket, "']'")?;
                Ok(MigExpr::SeqLit(items))
            }
            MigTok::LBrace => {
                self.advance();
                let mut entries = Vec::new();
                while !self.check(&MigTok::RBrace) {
                    let k = self.parse_expr()?;
                    self.expect(&MigTok::Colon, "':' in map literal")?;
                    let v = self.parse_expr()?;
                    entries.push((k, v));
                    if self.check(&MigTok::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&MigTok::RBrace, "'}' after map literal")?;
                Ok(MigExpr::MapLit(entries))
            }
            MigTok::Ident(word) => match word.as_str() {
                "null" => {
                    self.advance();
                    Ok(MigExpr::NullLit)
                }
                "true" => {
                    self.advance();
                    Ok(MigExpr::BoolLit(true))
                }
                "false" => {
                    self.advance();
                    Ok(MigExpr::BoolLit(false))
                }
                "box" if self.peek_n(1).kind == MigTok::LParen => {
                    self.advance(); // 'box'
                    self.advance(); // '('
                    self.expect(&MigTok::LBracket, "'[' inside box([...])")?;
                    let mut dims = Vec::new();
                    while !self.check(&MigTok::RBracket) {
                        dims.push(self.parse_expr()?);
                        if self.check(&MigTok::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    self.expect(&MigTok::RBracket, "']' inside box([...])")?;
                    self.expect(&MigTok::RParen, "')' after box([...])")?;
                    Ok(MigExpr::BoxExpr(dims))
                }
                "map" if self.peek_n(1).kind == MigTok::LBracket => {
                    self.advance(); // 'map'
                    self.advance(); // '['
                    let mut entries = Vec::new();
                    while !self.check(&MigTok::RBracket) {
                        self.expect(&MigTok::LParen, "'(' in map[(k, v)]")?;
                        let k = self.parse_expr()?;
                        self.expect(&MigTok::Comma, "',' in map[(k, v)]")?;
                        let v = self.parse_expr()?;
                        self.expect(&MigTok::RParen, "')' in map[(k, v)]")?;
                        entries.push((k, v));
                        if self.check(&MigTok::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    self.expect(&MigTok::RBracket, "']' after map[...]")?;
                    Ok(MigExpr::MapLit(entries))
                }
                "simulate" => {
                    self.advance();
                    let prepare = self.expect_ident("prepare name")?;
                    let mut observers = Vec::new();
                    if self.check_word("with") {
                        self.advance();
                        loop {
                            observers.push(self.expect_ident("observer name")?);
                            if self.check(&MigTok::Comma) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                    if self.check_word("for") || self.check_word("over") {
                        self.advance();
                    }
                    let ticks = self.parse_expr()?;
                    self.expect_word("ticks")?;
                    if self.check_word("with") {
                        self.advance();
                        loop {
                            observers.push(self.expect_ident("observer name")?);
                            if self.check(&MigTok::Comma) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                    Ok(MigExpr::Simulate {
                        prepare,
                        ticks: Box::new(ticks),
                        observers,
                    })
                }
                _ => {
                    self.advance();
                    let mut segs = vec![word];
                    while self.check(&MigTok::ColonColon) {
                        self.advance();
                        segs.push(self.expect_ident("path segment")?);
                    }
                    if self.allow_struct_lit
                        && self.check(&MigTok::LBrace)
                        && segs
                            .last()
                            .is_some_and(|s| s.bytes().next().is_some_and(|b| b.is_ascii_uppercase()))
                    {
                        self.advance(); // '{'
                        let mut fields = Vec::new();
                        while !self.check(&MigTok::RBrace) {
                            let fname = self.expect_ident("struct field name")?;
                            self.expect(&MigTok::Colon, "':' after struct field name")?;
                            let fval = self.parse_expr()?;
                            fields.push((fname, fval));
                            if self.check(&MigTok::Comma) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        self.expect(&MigTok::RBrace, "'}' after struct literal")?;
                        return Ok(MigExpr::StructLit { path: segs, fields });
                    }
                    if segs.len() == 1 {
                        Ok(MigExpr::Ident(segs.into_iter().next().expect("1")))
                    } else {
                        Ok(MigExpr::Path(segs))
                    }
                }
            },
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
// AST Lowering to Pure-Wave Trit-Boxed v3
// ============================================================================

struct LowerCtx {
    struct_fields: BTreeMap<String, Vec<String>>,
    const_types: BTreeMap<String, MigType>,
    fn_ret_types: BTreeMap<String, MigType>,
    local_fn_names: BTreeSet<String>,
    local_type_names: BTreeSet<String>,
    used_calls: BTreeSet<String>,
    needs_sectors_scientific_status: bool,
    step_seq: usize,
}

impl LowerCtx {
    fn new(items: &[MigItem]) -> Self {
        let mut struct_fields = BTreeMap::new();
        let mut const_types = BTreeMap::new();
        let mut fn_ret_types = BTreeMap::new();
        let mut local_fn_names = BTreeSet::new();
        let mut local_type_names = BTreeSet::new();

        for item in items {
            match item {
                MigItem::Struct { name, fields } => {
                    local_type_names.insert(name.clone());
                    struct_fields.insert(
                        name.clone(),
                        fields.iter().map(|(n, _)| n.clone()).collect(),
                    );
                }
                MigItem::Enum { name, .. } => {
                    local_type_names.insert(name.clone());
                }
                MigItem::Const { name, ty, value } => {
                    if let Some(t) = ty {
                        const_types.insert(name.clone(), t.migrate());
                    } else if let Some(inf) = infer_const_expr_type(value) {
                        const_types.insert(name.clone(), inf);
                    }
                }
                MigItem::DecFn(f) => {
                    local_fn_names.insert(f.name.clone());
                    if let Some(first) = f.arms.first() {
                        fn_ret_types.insert(f.name.clone(), first.return_type.migrate());
                    }
                }
                MigItem::QuarantineMod {
                    name,
                    items: sub_items,
                } => {
                    local_type_names.insert(name.clone());
                    for sub in sub_items {
                        if let MigItem::DecFn(f) = sub {
                            if let Some(first) = f.arms.first() {
                                fn_ret_types.insert(
                                    format!("{name}::{}", f.name),
                                    first.return_type.migrate(),
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        Self {
            struct_fields,
            const_types,
            fn_ret_types,
            local_fn_names,
            local_type_names,
            used_calls: BTreeSet::new(),
            needs_sectors_scientific_status: false,
            step_seq: 0,
        }
    }

    fn infer_expr_type(
        &self,
        expr: &MigExpr,
        var_types: &BTreeMap<String, MigType>,
    ) -> Option<MigType> {
        match expr {
            MigExpr::IntLit(_) => Some(MigType::Named("int".to_string())),
            MigExpr::BoolLit(_) => Some(MigType::Named("bool".to_string())),
            MigExpr::BytesLit(_) | MigExpr::StrLit(_) => Some(MigType::Named("bytes".to_string())),
            MigExpr::RatioLit(_) => Some(MigType::Named("ratio".to_string())),
            MigExpr::TritLit(_) => Some(MigType::Named("t1".to_string())),
            MigExpr::Addr8Lit(_) => Some(MigType::Named("addr8".to_string())),
            MigExpr::SupportLit(_) => Some(MigType::Named("support".to_string())),
            MigExpr::Ident(n) => var_types
                .get(n)
                .or_else(|| self.const_types.get(n))
                .cloned(),
            MigExpr::SeqLit(items) => {
                let first = items.first()?;
                Some(MigType::Array(
                    Box::new(self.infer_expr_type(first, var_types)?),
                    items.len().to_string(),
                ))
            }
            MigExpr::Tuple(items) => {
                let mut ts = Vec::with_capacity(items.len());
                for it in items {
                    ts.push(self.infer_expr_type(it, var_types)?);
                }
                Some(MigType::Tuple(ts))
            }
            MigExpr::Unary { op, operand } => match op.as_str() {
                "not" => Some(MigType::Named("bool".to_string())),
                "∂" => Some(MigType::Named("bytes".to_string())),
                _ => self.infer_expr_type(operand, var_types),
            },
            MigExpr::Binary { op, left, .. } => match op.as_str() {
                "==" | "!=" | "<" | "<=" | ">" | ">=" | "and" | "or" => {
                    Some(MigType::Named("bool".to_string()))
                }
                "<=>" => Some(MigType::Named("t1".to_string())),
                "/" => match self.infer_expr_type(left, var_types) {
                    Some(MigType::QExt(inner)) => Some(MigType::QExt(inner)),
                    _ => Some(MigType::Named("ratio".to_string())),
                },
                _ => self.infer_expr_type(left, var_types),
            },
            MigExpr::Conditional { then_expr, .. } => self.infer_expr_type(then_expr, var_types),
            MigExpr::Index { base, .. } => match self.infer_expr_type(base, var_types)? {
                MigType::List(inner) | MigType::Array(inner, _) => Some(*inner),
                MigType::Named(ref s) if s == "bytes" => Some(MigType::Named("b8".to_string())),
                _ => None,
            },
            MigExpr::Call { callee, args } => {
                if let MigExpr::Ident(cname) = callee.as_ref() {
                    match cname.as_str() {
                        "int" | "u64_int" | "t32" | "t9" | "int8" | "int16" | "int32" | "int64"
                        | "len" | "phases" | "beats" | "lag" | "drift" | "num" | "den" | "pack" => {
                            return Some(MigType::Named("int".to_string()));
                        }
                        "u8" | "b8" => return Some(MigType::Named("b8".to_string())),
                        "b64" | "b64_bits" => return Some(MigType::Named("b64".to_string())),
                        "t1" => return Some(MigType::Named("t1".to_string())),
                        "q4" => return Some(MigType::Named("q4".to_string())),
                        "ratio" | "rate" | "cadence" | "probability" => {
                            return Some(MigType::Named("ratio".to_string()));
                        }
                        "trace" | "step" | "hinge" | "dwell" | "close" | "outer" | "fuse" => {
                            return Some(MigType::Named("trace".to_string()));
                        }
                        "fold" if args.len() == 4 => {
                            return Some(MigType::Named("fusion".to_string()));
                        }
                        "coupling" | "contact" => {
                            return Some(MigType::Named("coupling".to_string()));
                        }
                        "select" if args.len() == 3 => {
                            return self.infer_expr_type(&args[1].1, var_types);
                        }
                        "set_at" | "list_set" | "array_set" if args.len() == 3 => {
                            return self.infer_expr_type(&args[0].1, var_types);
                        }
                        "ext_decimal" | "qext_from_ratio" => {
                            return Some(MigType::QExt(Box::new(MigType::Named(
                                "ratio".to_string(),
                            ))));
                        }
                        "qext_render" | "qext_format_fixed" => {
                            return Some(MigType::QExt(Box::new(MigType::Named(
                                "bytes".to_string(),
                            ))));
                        }
                        other => {
                            if let Some(rt) = self.fn_ret_types.get(other) {
                                return Some(rt.clone());
                            }
                            if self.local_type_names.contains(other) {
                                return Some(MigType::Named(other.to_string()));
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }
}

fn infer_const_expr_type(expr: &MigExpr) -> Option<MigType> {
    match expr {
        MigExpr::IntLit(_) => Some(MigType::Named("int".to_string())),
        MigExpr::RatioLit(_) => Some(MigType::Named("ratio".to_string())),
        MigExpr::BoolLit(_) => Some(MigType::Named("bool".to_string())),
        MigExpr::BytesLit(_) | MigExpr::StrLit(_) => Some(MigType::Named("bytes".to_string())),
        MigExpr::Call { callee, .. } => match callee.as_ref() {
            MigExpr::Ident(n) if n == "ext_decimal" || n == "qext_from_ratio" => {
                Some(MigType::QExt(Box::new(MigType::Named("ratio".to_string()))))
            }
            _ => None,
        },
        _ => None,
    }
}

fn sanitize_binder_name(raw: &str) -> String {
    if raw == "_" {
        return "_".to_string();
    }
    if BANNED_V3_IDENTS.contains(&raw) {
        format!("{raw}_val")
    } else {
        raw.to_string()
    }
}

fn is_recipient_stage_literal(bs: &[u8]) -> bool {
    matches!(
        bs,
        b"emitted"
            | b"delivered"
            | b"accepted"
            | b"unaccepted_pass"
            | b"seated"
            | b"pending_unseated"
            | b"closed_origin"
            | b"reclosed"
            | b"reclosed_replay"
            | b"reclosed_out_of_window"
    )
}

struct ArmSsa {
    map: BTreeMap<String, String>,
    used_names: BTreeSet<String>,
    var_types: BTreeMap<String, MigType>,
    seq: usize,
    is_v1_origin: bool,
}

impl ArmSsa {
    fn new(is_v1_origin: bool) -> Self {
        Self {
            map: BTreeMap::new(),
            used_names: BTreeSet::new(),
            var_types: BTreeMap::new(),
            seq: 0,
            is_v1_origin,
        }
    }

    fn bind_initial(&mut self, orig: &str, ty: Option<MigType>) -> String {
        if orig == "_" {
            return "_".to_string();
        }
        let base = sanitize_binder_name(orig);
        let chosen = if !self.used_names.contains(&base) {
            base
        } else {
            self.fresh_for(&base)
        };
        self.used_names.insert(chosen.clone());
        self.map.insert(orig.to_string(), chosen.clone());
        if let Some(t) = ty {
            self.var_types.insert(orig.to_string(), t.clone());
            self.var_types.insert(chosen.clone(), t);
        }
        chosen
    }

    fn rebind(&mut self, orig: &str, ty: Option<MigType>) -> String {
        let base = sanitize_binder_name(orig);
        let chosen = if !self.used_names.contains(&base) {
            base
        } else {
            self.fresh_for(&base)
        };
        self.used_names.insert(chosen.clone());
        self.map.insert(orig.to_string(), chosen.clone());
        if let Some(t) = ty {
            self.var_types.insert(orig.to_string(), t.clone());
            self.var_types.insert(chosen.clone(), t);
        } else if let Some(prev_t) = self.var_types.get(orig).cloned() {
            self.var_types.insert(chosen.clone(), prev_t);
        }
        chosen
    }

    fn fresh_for(&mut self, base: &str) -> String {
        loop {
            self.seq += 1;
            let cand = format!("{base}_s{}", self.seq);
            if !self.used_names.contains(&cand) && !BANNED_V3_IDENTS.contains(&cand.as_str()) {
                return cand;
            }
        }
    }
}

fn collect_assigned_vars_in_stmts(stmts: &[MigStmt], out: &mut Vec<String>) {
    for s in stmts {
        match s {
            MigStmt::AssignVar { name, .. } | MigStmt::AssignIndex { name, .. } => {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            }
            MigStmt::If {
                branches,
                else_body,
            } => {
                for (_, b) in branches {
                    collect_assigned_vars_in_stmts(b, out);
                }
                collect_assigned_vars_in_stmts(else_body, out);
            }
            MigStmt::For { body, .. } | MigStmt::QuarantineBlock(body) => {
                collect_assigned_vars_in_stmts(body, out);
            }
            _ => {}
        }
    }
}

fn collect_read_idents_expr(expr: &MigExpr, out: &mut Vec<String>) {
    match expr {
        MigExpr::Ident(n) => {
            if !out.contains(n) {
                out.push(n.clone());
            }
        }
        MigExpr::Tuple(xs) | MigExpr::SeqLit(xs) | MigExpr::BoxExpr(xs) => {
            for x in xs {
                collect_read_idents_expr(x, out);
            }
        }
        MigExpr::MapLit(kvs) => {
            for (k, v) in kvs {
                collect_read_idents_expr(k, out);
                collect_read_idents_expr(v, out);
            }
        }
        MigExpr::StructLit { fields, .. } => {
            for (_, v) in fields {
                collect_read_idents_expr(v, out);
            }
        }
        MigExpr::WindowLit { start, end } | MigExpr::Range { start, end } => {
            collect_read_idents_expr(start, out);
            collect_read_idents_expr(end, out);
        }
        MigExpr::Simulate { ticks, .. } => collect_read_idents_expr(ticks, out),
        MigExpr::Unary { operand, .. } | MigExpr::Adverb { operand, .. } => {
            collect_read_idents_expr(operand, out);
        }
        MigExpr::Binary { left, right, .. } => {
            collect_read_idents_expr(left, out);
            collect_read_idents_expr(right, out);
        }
        MigExpr::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            collect_read_idents_expr(cond, out);
            collect_read_idents_expr(then_expr, out);
            collect_read_idents_expr(else_expr, out);
        }
        MigExpr::Pipe { input, stage } => {
            collect_read_idents_expr(input, out);
            match stage.as_ref() {
                MigPipeStage::Section { rhs, .. } => collect_read_idents_expr(rhs, out),
                MigPipeStage::Call { extra_args, .. } => {
                    for a in extra_args {
                        collect_read_idents_expr(a, out);
                    }
                }
                _ => {}
            }
        }
        MigExpr::Field { base, .. } => collect_read_idents_expr(base, out),
        MigExpr::Index { base, index } => {
            collect_read_idents_expr(base, out);
            collect_read_idents_expr(index, out);
        }
        MigExpr::Call { callee, args } => {
            if !matches!(callee.as_ref(), MigExpr::Ident(_) | MigExpr::Path(_)) {
                collect_read_idents_expr(callee, out);
            }
            for (_, a) in args {
                collect_read_idents_expr(a, out);
            }
        }
        _ => {}
    }
}

fn collect_free_reads_in_stmts(
    stmts: &[MigStmt],
    bound_in_loop: &mut BTreeSet<String>,
    reads: &mut Vec<String>,
) {
    let add_expr = |e: &MigExpr, bound: &BTreeSet<String>, rds: &mut Vec<String>| {
        let mut tmp = Vec::new();
        collect_read_idents_expr(e, &mut tmp);
        for id in tmp {
            if !bound.contains(&id) && !rds.contains(&id) {
                rds.push(id);
            }
        }
    };

    for s in stmts {
        match s {
            MigStmt::Let { pat, value, .. } => {
                add_expr(value, bound_in_loop, reads);
                match pat {
                    MigLetPat::Single(n) => {
                        bound_in_loop.insert(n.clone());
                    }
                    MigLetPat::Tuple(es) => {
                        for (n, _) in es {
                            bound_in_loop.insert(n.clone());
                        }
                    }
                }
            }
            MigStmt::AssignVar { value, .. } => {
                add_expr(value, bound_in_loop, reads);
            }
            MigStmt::AssignIndex { name, index, value } => {
                if !bound_in_loop.contains(name) && !reads.contains(name) {
                    reads.push(name.clone());
                }
                add_expr(index, bound_in_loop, reads);
                add_expr(value, bound_in_loop, reads);
            }
            MigStmt::Assert(e)
            | MigStmt::Emit(e)
            | MigStmt::Return(Some(e))
            | MigStmt::ExprStmt(e) => {
                add_expr(e, bound_in_loop, reads);
            }
            MigStmt::If {
                branches,
                else_body,
            } => {
                for (c, b) in branches {
                    add_expr(c, bound_in_loop, reads);
                    let mut child_bound = bound_in_loop.clone();
                    collect_free_reads_in_stmts(b, &mut child_bound, reads);
                }
                let mut else_bound = bound_in_loop.clone();
                collect_free_reads_in_stmts(else_body, &mut else_bound, reads);
            }
            MigStmt::For { var, iter, body } => {
                add_expr(iter, bound_in_loop, reads);
                let mut child_bound = bound_in_loop.clone();
                child_bound.insert(var.clone());
                collect_free_reads_in_stmts(body, &mut child_bound, reads);
            }
            MigStmt::QuarantineBlock(body) => {
                let mut child_bound = bound_in_loop.clone();
                collect_free_reads_in_stmts(body, &mut child_bound, reads);
            }
            MigStmt::Return(None) | MigStmt::Pass => {}
        }
    }
}

fn rewrite_status_comparison(
    op: &str,
    left: &MigExpr,
    right: &MigExpr,
    ctx: &mut LowerCtx,
    ssa: &ArmSsa,
) -> Option<MigExpr> {
    if op != "==" && op != "!=" {
        return None;
    }

    // 1. `<base>.model_status == "DECLARED_..."`
    if let (MigExpr::Field { base, name }, MigExpr::StrLit(bs)) = (left, right) {
        if name == "model_status" {
            return Some(MigExpr::Binary {
                op: op.to_string(),
                left: Box::new(MigExpr::Field {
                    base: Box::new(lower_expr(base, ctx, ssa, false)),
                    name: "model_status_bytes".to_string(),
                }),
                right: Box::new(MigExpr::BytesLit(bs.clone())),
            });
        }
        if name == "stage" && is_recipient_stage_literal(bs) {
            return Some(MigExpr::Binary {
                op: op.to_string(),
                left: Box::new(MigExpr::Field {
                    base: Box::new(lower_expr(base, ctx, ssa, false)),
                    name: "stage_bytes".to_string(),
                }),
                right: Box::new(MigExpr::BytesLit(bs.clone())),
            });
        }
        if name == "scientific_status" && bs.as_slice() == b"UNRESOLVED_PROVENANCE_QUARANTINED" {
            ctx.needs_sectors_scientific_status = true;
            return Some(MigExpr::Binary {
                op: op.to_string(),
                left: Box::new(MigExpr::Field {
                    base: Box::new(lower_expr(base, ctx, ssa, false)),
                    name: "scientific_status".to_string(),
                }),
                right: Box::new(MigExpr::Path(vec![
                    "ScientificStatus".to_string(),
                    "UnresolvedProvenanceQuarantined".to_string(),
                ])),
            });
        }
    }

    // Symmetric `"..." == <base>.model_status`
    if let (MigExpr::StrLit(bs), MigExpr::Field { base, name }) = (left, right) {
        if name == "model_status" {
            return Some(MigExpr::Binary {
                op: op.to_string(),
                left: Box::new(MigExpr::BytesLit(bs.clone())),
                right: Box::new(MigExpr::Field {
                    base: Box::new(lower_expr(base, ctx, ssa, false)),
                    name: "model_status_bytes".to_string(),
                }),
            });
        }
        if name == "stage" && is_recipient_stage_literal(bs) {
            return Some(MigExpr::Binary {
                op: op.to_string(),
                left: Box::new(MigExpr::BytesLit(bs.clone())),
                right: Box::new(MigExpr::Field {
                    base: Box::new(lower_expr(base, ctx, ssa, false)),
                    name: "stage_bytes".to_string(),
                }),
            });
        }
    }

    None
}

fn lower_expr(
    expr: &MigExpr,
    ctx: &mut LowerCtx,
    ssa: &ArmSsa,
    inside_close_call: bool,
) -> MigExpr {
    match expr {
        MigExpr::NullLit => MigExpr::NullLit,
        MigExpr::IntLit(n) => MigExpr::IntLit(n.clone()),
        MigExpr::BoolLit(b) => MigExpr::BoolLit(*b),
        MigExpr::BytesLit(bs) | MigExpr::StrLit(bs) => MigExpr::BytesLit(bs.clone()),
        MigExpr::RatioLit(r) => MigExpr::RatioLit(r.clone()),
        MigExpr::TritLit(t) => MigExpr::TritLit(t.clone()),
        MigExpr::Addr8Lit(a) => MigExpr::Addr8Lit(a.clone()),
        MigExpr::SupportLit(s) => MigExpr::SupportLit(s.clone()),
        MigExpr::Ident(name) => {
            let mapped = ssa.map.get(name).cloned().unwrap_or_else(|| name.clone());
            MigExpr::Ident(mapped)
        }
        MigExpr::Path(segs) => MigExpr::Path(segs.clone()),
        MigExpr::Tuple(items) => MigExpr::Tuple(
            items
                .iter()
                .map(|x| lower_expr(x, ctx, ssa, false))
                .collect(),
        ),
        MigExpr::SeqLit(items) => MigExpr::SeqLit(
            items
                .iter()
                .map(|x| lower_expr(x, ctx, ssa, false))
                .collect(),
        ),
        MigExpr::MapLit(kvs) => MigExpr::MapLit(
            kvs.iter()
                .map(|(k, v)| {
                    (
                        lower_expr(k, ctx, ssa, false),
                        lower_expr(v, ctx, ssa, false),
                    )
                })
                .collect(),
        ),
        MigExpr::StructLit { path, fields } => {
            let leaf = path.last().cloned().unwrap_or_default();
            let ordered_args: Vec<(Option<String>, MigExpr)> =
                if let Some(decl_order) = ctx.struct_fields.get(&leaf).cloned() {
                    let mut by_name: BTreeMap<String, MigExpr> = BTreeMap::new();
                    for (fnm, fv) in fields {
                        by_name.insert(fnm.clone(), lower_expr(fv, ctx, ssa, false));
                    }
                    if decl_order.iter().all(|dn| by_name.contains_key(dn))
                        && decl_order.len() == by_name.len()
                    {
                        decl_order
                            .into_iter()
                            .map(|dn| (None, by_name.remove(&dn).expect("present")))
                            .collect()
                    } else {
                        fields
                            .iter()
                            .map(|(_, fv)| (None, lower_expr(fv, ctx, ssa, false)))
                            .collect()
                    }
                } else {
                    fields
                        .iter()
                        .map(|(_, fv)| (None, lower_expr(fv, ctx, ssa, false)))
                        .collect()
                };
            let callee = if path.len() == 1 {
                MigExpr::Ident(path[0].clone())
            } else {
                MigExpr::Path(path.clone())
            };
            MigExpr::Call {
                callee: Box::new(callee),
                args: ordered_args,
            }
        }
        MigExpr::WindowLit { start, end } => MigExpr::WindowLit {
            start: Box::new(lower_expr(start, ctx, ssa, false)),
            end: Box::new(lower_expr(end, ctx, ssa, false)),
        },
        MigExpr::Range { start, end } => MigExpr::Range {
            start: Box::new(lower_expr(start, ctx, ssa, false)),
            end: Box::new(lower_expr(end, ctx, ssa, false)),
        },
        MigExpr::BoxExpr(dims) => MigExpr::BoxExpr(
            dims.iter()
                .map(|d| lower_expr(d, ctx, ssa, false))
                .collect(),
        ),
        MigExpr::Simulate {
            prepare,
            ticks,
            observers,
        } => MigExpr::Simulate {
            prepare: prepare.clone(),
            ticks: Box::new(lower_expr(ticks, ctx, ssa, false)),
            observers: observers.clone(),
        },
        MigExpr::Unary { op, operand } => MigExpr::Unary {
            op: op.clone(),
            operand: Box::new(lower_expr(operand, ctx, ssa, false)),
        },
        MigExpr::Adverb { op, operand } => MigExpr::Adverb {
            op: op.clone(),
            operand: Box::new(lower_expr(operand, ctx, ssa, false)),
        },
        MigExpr::Binary { op, left, right } => {
            if let Some(rewritten) = rewrite_status_comparison(op, left, right, ctx, ssa) {
                return rewritten;
            }
            if matches!(op.as_str(), "<<" | ">>" | "&" | "|" | "^")
                && ctx.infer_expr_type(left, &ssa.var_types)
                    == Some(MigType::Named("int".to_string()))
            {
                ctx.used_calls.insert("b64_bits".to_string());
                ctx.used_calls.insert("u64_int".to_string());
                let l_b64 = MigExpr::Call {
                    callee: Box::new(MigExpr::Ident("b64_bits".to_string())),
                    args: vec![(None, lower_expr(left, ctx, ssa, false))],
                };
                let r_low = if matches!(op.as_str(), "<<" | ">>") {
                    lower_expr(right, ctx, ssa, false)
                } else {
                    MigExpr::Call {
                        callee: Box::new(MigExpr::Ident("b64_bits".to_string())),
                        args: vec![(None, lower_expr(right, ctx, ssa, false))],
                    }
                };
                let bit_op = MigExpr::Binary {
                    op: op.clone(),
                    left: Box::new(l_b64),
                    right: Box::new(r_low),
                };
                return MigExpr::Call {
                    callee: Box::new(MigExpr::Ident("u64_int".to_string())),
                    args: vec![(None, bit_op)],
                };
            }
            MigExpr::Binary {
                op: op.clone(),
                left: Box::new(lower_expr(left, ctx, ssa, false)),
                right: Box::new(lower_expr(right, ctx, ssa, false)),
            }
        }
        MigExpr::Conditional {
            then_expr,
            cond,
            else_expr,
        } => {
            ctx.used_calls.insert("select".to_string());
            MigExpr::Call {
                callee: Box::new(MigExpr::Ident("select".to_string())),
                args: vec![
                    (None, lower_expr(cond, ctx, ssa, false)),
                    (None, lower_expr(then_expr, ctx, ssa, false)),
                    (None, lower_expr(else_expr, ctx, ssa, false)),
                ],
            }
        }
        MigExpr::Pipe { input, stage } => {
            let inp = lower_expr(input, ctx, ssa, false);
            let st = match stage.as_ref() {
                MigPipeStage::Adverb(op) => MigPipeStage::Adverb(op.clone()),
                MigPipeStage::Boundary => MigPipeStage::Boundary,
                MigPipeStage::Section { op, rhs } => MigPipeStage::Section {
                    op: op.clone(),
                    rhs: lower_expr(rhs, ctx, ssa, false),
                },
                MigPipeStage::Call { callee, extra_args } => {
                    if let MigExpr::Ident(cn) = callee {
                        ctx.used_calls.insert(cn.clone());
                    }
                    MigPipeStage::Call {
                        callee: lower_expr(callee, ctx, ssa, false),
                        extra_args: extra_args
                            .iter()
                            .map(|a| lower_expr(a, ctx, ssa, false))
                            .collect(),
                    }
                }
            };
            MigExpr::Pipe {
                input: Box::new(inp),
                stage: Box::new(st),
            }
        }
        MigExpr::Field { base, name } => MigExpr::Field {
            base: Box::new(lower_expr(base, ctx, ssa, false)),
            name: name.clone(),
        },
        MigExpr::Index { base, index } => MigExpr::Index {
            base: Box::new(lower_expr(base, ctx, ssa, false)),
            index: Box::new(lower_expr(index, ctx, ssa, false)),
        },
        MigExpr::Call { callee, args } => {
            let is_close = matches!(callee.as_ref(), MigExpr::Ident(n) if n == "close");
            let mut lowered_args: Vec<(Option<String>, MigExpr)> = args
                .iter()
                .map(|(lbl, a)| (lbl.clone(), lower_expr(a, ctx, ssa, is_close)))
                .collect();

            if let MigExpr::Ident(cname) = callee.as_ref() {
                // Reorder named constructor arguments if `cname` is a local struct
                if lowered_args.iter().any(|(lbl, _)| lbl.is_some()) {
                    if let Some(decl_order) = ctx.struct_fields.get(cname).cloned() {
                        let mut by_name = BTreeMap::new();
                        for (lbl, val) in &lowered_args {
                            if let Some(l) = lbl {
                                by_name.insert(l.clone(), val.clone());
                            }
                        }
                        if decl_order.iter().all(|dn| by_name.contains_key(dn))
                            && decl_order.len() == lowered_args.len()
                        {
                            lowered_args = decl_order
                                .into_iter()
                                .map(|dn| (None, by_name.remove(&dn).expect("present")))
                                .collect();
                        } else {
                            for (lbl, _) in &mut lowered_args {
                                *lbl = None;
                            }
                        }
                    } else {
                        for (lbl, _) in &mut lowered_args {
                             *lbl = None;
                        }
                    }
                }

                match cname.as_str() {
                    "t32" | "t9" | "int8" | "int16" | "int32" | "int64"
                        if lowered_args.len() == 1 =>
                    {
                        let inner = lowered_args.remove(0).1;
                        return match inner {
                            MigExpr::IntLit(_) => inner,
                            MigExpr::Unary { ref op, ref operand }
                                if op == "-" && matches!(operand.as_ref(), MigExpr::IntLit(_)) =>
                            {
                                inner
                            }
                            other => {
                                ctx.used_calls.insert("int".to_string());
                                MigExpr::Call {
                                    callee: Box::new(MigExpr::Ident("int".to_string())),
                                    args: vec![(None, other)],
                                }
                            }
                        };
                    }
                    "u8" if lowered_args.len() == 1 => {
                        ctx.used_calls.insert("b8".to_string());
                        return MigExpr::Call {
                            callee: Box::new(MigExpr::Ident("b8".to_string())),
                            args: lowered_args,
                        };
                    }
                    "t1" if lowered_args.len() == 1 => match &lowered_args[0].1 {
                        MigExpr::IntLit(n) if n == "0" => {
                            return MigExpr::TritLit("0t".to_string());
                        }
                        MigExpr::IntLit(n) if n == "1" => {
                            return MigExpr::TritLit("1t".to_string());
                        }
                        MigExpr::Unary { op, operand }
                            if op == "-"
                                && matches!(operand.as_ref(), MigExpr::IntLit(n) if n == "1") =>
                        {
                            return MigExpr::TritLit("-1t".to_string());
                        }
                        _ => {}
                    },
                    "fuse" if ssa.is_v1_origin && !inside_close_call && lowered_args.len() == 2 => {
                        ctx.used_calls.insert("fuse".to_string());
                        ctx.used_calls.insert("close".to_string());
                        let fuse_call = MigExpr::Call {
                            callee: Box::new(MigExpr::Ident("fuse".to_string())),
                            args: lowered_args,
                        };
                        return MigExpr::Call {
                            callee: Box::new(MigExpr::Ident("close".to_string())),
                            args: vec![(None, fuse_call)],
                        };
                    }
                    _ => {}
                }

                if lowered_args.len() == 4 && cname == "fold" {
                    ctx.used_calls.insert("witness::fold".to_string());
                } else {
                    ctx.used_calls.insert(cname.clone());
                }
            }

            MigExpr::Call {
                callee: Box::new(lower_expr(callee, ctx, ssa, false)),
                args: lowered_args,
            }
        }
    }
}

fn stmts_have_side_effects(stmts: &[MigStmt]) -> bool {
    stmts.iter().any(|s| match s {
        MigStmt::Assert(_) | MigStmt::Emit(_) | MigStmt::QuarantineBlock(_) => true,
        MigStmt::If {
            branches,
            else_body,
        } => {
            branches.iter().any(|(_, b)| stmts_have_side_effects(b))
                || stmts_have_side_effects(else_body)
        }
        MigStmt::For { body, .. } => stmts_have_side_effects(body),
        _ => false,
    })
}

fn block_ends_with_return(stmts: &[MigStmt]) -> bool {
    match stmts.last() {
        Some(MigStmt::Return(Some(_))) => true,
        Some(MigStmt::If {
            branches,
            else_body,
        }) => {
            !else_body.is_empty()
                && branches.iter().all(|(_, b)| block_ends_with_return(b))
                && block_ends_with_return(else_body)
        }
        _ => false,
    }
}

fn extract_pure_return_branch(
    stmts: &[MigStmt],
    ctx: &mut LowerCtx,
    ssa: &mut ArmSsa,
    synthesized_fns: &mut Vec<MigFn>,
    owner_fn: &str,
    out_lets: &mut Vec<MigStmt>,
) -> Option<MigExpr> {
    if stmts_have_side_effects(stmts) || !block_ends_with_return(stmts) {
        return None;
    }
    let lowered = lower_block(stmts, None, false, ctx, ssa, synthesized_fns, owner_fn);
    let last = lowered.last()?;
    let MigStmt::Return(Some(ret_expr)) = last else {
        return None;
    };
    for s in &lowered[..lowered.len() - 1] {
        out_lets.push(s.clone());
    }
    Some(ret_expr.clone())
}

fn stmts_contain_return(stmts: &[MigStmt]) -> bool {
    stmts.iter().any(|s| match s {
        MigStmt::Return(_) => true,
        MigStmt::If {
            branches,
            else_body,
        } => {
            branches.iter().any(|(_, b)| stmts_contain_return(b))
                || stmts_contain_return(else_body)
        }
        MigStmt::For { body, .. } | MigStmt::QuarantineBlock(body) => stmts_contain_return(body),
        _ => false,
    })
}

fn rewrite_returns_to_flags(stmts: &[MigStmt], done_var: &str, val_var: &str) -> Vec<MigStmt> {
    let mut out = Vec::new();
    for s in stmts {
        match s {
            MigStmt::Return(Some(e)) => {
                out.push(MigStmt::AssignVar {
                    name: done_var.to_string(),
                    value: MigExpr::BoolLit(true),
                });
                out.push(MigStmt::AssignVar {
                    name: val_var.to_string(),
                    value: e.clone(),
                });
            }
            MigStmt::If {
                branches,
                else_body,
            } => {
                let new_bs = branches
                    .iter()
                    .map(|(c, b)| (c.clone(), rewrite_returns_to_flags(b, done_var, val_var)))
                    .collect();
                let new_eb = rewrite_returns_to_flags(else_body, done_var, val_var);
                out.push(MigStmt::If {
                    branches: new_bs,
                    else_body: new_eb,
                });
            }
            other => out.push(other.clone()),
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn lower_for_stmt(
    var: &str,
    iter: &MigExpr,
    body: &[MigStmt],
    ctx: &mut LowerCtx,
    ssa: &mut ArmSsa,
    synthesized_fns: &mut Vec<MigFn>,
    owner_fn: &str,
) -> Vec<MigStmt> {
    let mut state_vars = Vec::new();
    collect_assigned_vars_in_stmts(body, &mut state_vars);
    state_vars.retain(|sv| ssa.map.contains_key(sv));

    let mut bound_in_loop = BTreeSet::new();
    bound_in_loop.insert(var.to_string());
    let mut raw_reads = Vec::new();
    collect_free_reads_in_stmts(body, &mut bound_in_loop, &mut raw_reads);

    let mut env_vars = Vec::new();
    for r in raw_reads {
        if !state_vars.contains(&r) && ssa.map.contains_key(&r) {
            env_vars.push(r);
        }
    }

    let is_range_or_window = matches!(iter, MigExpr::Range { .. } | MigExpr::WindowLit { .. });
    let iter_lowered = lower_expr(iter, ctx, ssa, false);

    // If iterating over a sequence `xs`, iterate indices `0..len(xs)` via `evolve`
    let mut seq_iter_tmp_let: Option<(String, MigType, MigStmt)> = None;
    let domain_expr = if is_range_or_window {
        iter_lowered
    } else {
        let seq_ty = ctx
            .infer_expr_type(iter, &ssa.var_types)
            .unwrap_or_else(|| MigType::List(Box::new(MigType::Named("int".to_string()))));
        let tmp_seq_name = ssa.fresh_for("loop_seq");
        ssa.used_names.insert(tmp_seq_name.clone());
        ssa.map.insert(tmp_seq_name.clone(), tmp_seq_name.clone());
        ssa.var_types.insert(tmp_seq_name.clone(), seq_ty.clone());
        env_vars.insert(0, tmp_seq_name.clone());
        let let_stmt = MigStmt::Let {
            mutable: false,
            pat: MigLetPat::Single(tmp_seq_name.clone()),
            ty: None,
            value: iter_lowered,
        };
        seq_iter_tmp_let = Some((tmp_seq_name.clone(), seq_ty, let_stmt));
        ctx.used_calls.insert("len".to_string());
        MigExpr::Range {
            start: Box::new(MigExpr::IntLit("0".to_string())),
            end: Box::new(MigExpr::Call {
                callee: Box::new(MigExpr::Ident("len".to_string())),
                args: vec![(None, MigExpr::Ident(tmp_seq_name))],
            }),
        }
    };

    ctx.step_seq += 1;
    let step_fn_name = format!("{owner_fn}_step_{}", ctx.step_seq);
    ctx.used_calls.insert("evolve".to_string());

    let state_types: Vec<MigType> = if state_vars.is_empty() {
        vec![MigType::Named("int".to_string())]
    } else {
        state_vars
            .iter()
            .map(|sv| {
                ssa.var_types
                    .get(sv)
                    .cloned()
                    .unwrap_or_else(|| MigType::Named("int".to_string()))
            })
            .collect()
    };

    let mut step_ssa = ArmSsa::new(ssa.is_v1_origin);
    let state_param = if state_vars.is_empty() {
        let dummy = step_ssa.bind_initial("_st", Some(MigType::Named("int".to_string())));
        MigArmParam::TypedVar {
            name: dummy,
            ty: MigType::Named("int".to_string()),
        }
    } else if state_vars.len() == 1 {
        let p = step_ssa.bind_initial(&state_vars[0], Some(state_types[0].clone()));
        MigArmParam::TypedVar {
            name: p,
            ty: state_types[0].clone(),
        }
    } else {
        let mut pats = Vec::with_capacity(state_vars.len());
        for (sv, sty) in state_vars.iter().zip(&state_types) {
            let p = step_ssa.bind_initial(sv, Some(sty.clone()));
            pats.push(MigPat::Binding(p));
        }
        MigArmParam::TypedPattern {
            pat: MigPat::Tuple(pats),
            ty: MigType::Tuple(state_types.clone()),
        }
    };

    let idx_param_name = if seq_iter_tmp_let.is_some() {
        step_ssa.bind_initial("loop_idx", Some(MigType::Named("int".to_string())))
    } else {
        step_ssa.bind_initial(var, Some(MigType::Named("int".to_string())))
    };
    let idx_param = MigArmParam::TypedVar {
        name: idx_param_name.clone(),
        ty: MigType::Named("int".to_string()),
    };

    let mut step_params = vec![state_param, idx_param];
    let mut call_env_arg: Option<MigExpr> = None;

    if !env_vars.is_empty() {
        let env_types: Vec<MigType> = env_vars
            .iter()
            .map(|ev| {
                ssa.var_types
                    .get(ev)
                    .cloned()
                    .unwrap_or_else(|| MigType::Named("int".to_string()))
            })
            .collect();
        if env_vars.len() == 1 {
            let cur_ev = ssa
                .map
                .get(&env_vars[0])
                .cloned()
                .unwrap_or_else(|| env_vars[0].clone());
            call_env_arg = Some(MigExpr::Ident(cur_ev));
            let p = step_ssa.bind_initial(&env_vars[0], Some(env_types[0].clone()));
            step_params.push(MigArmParam::TypedVar {
                name: p,
                ty: env_types[0].clone(),
            });
        } else {
            let call_elems: Vec<MigExpr> = env_vars
                .iter()
                .map(|ev| {
                    let cur_ev = ssa.map.get(ev).cloned().unwrap_or_else(|| ev.clone());
                    MigExpr::Ident(cur_ev)
                })
                .collect();
            call_env_arg = Some(MigExpr::Tuple(call_elems));
            let mut pats = Vec::with_capacity(env_vars.len());
            for (ev, ety) in env_vars.iter().zip(&env_types) {
                let p = step_ssa.bind_initial(ev, Some(ety.clone()));
                pats.push(MigPat::Binding(p));
            }
            step_params.push(MigArmParam::TypedPattern {
                pat: MigPat::Tuple(pats),
                ty: MigType::Tuple(env_types),
            });
        }
    }

    let mut step_body = Vec::new();
    if let Some((ref seq_name, ref seq_ty, _)) = seq_iter_tmp_let {
        let elem_ty = match seq_ty {
            MigType::List(inner) | MigType::Array(inner, _) => (**inner).clone(),
            _ => MigType::Named("int".to_string()),
        };
        let seq_in_step = step_ssa
            .map
            .get(seq_name)
            .cloned()
            .unwrap_or_else(|| seq_name.clone());
        let bound_el = step_ssa.bind_initial(var, Some(elem_ty));
        step_body.push(MigStmt::Let {
            mutable: false,
            pat: MigLetPat::Single(bound_el),
            ty: None,
            value: MigExpr::Index {
                base: Box::new(MigExpr::Ident(seq_in_step)),
                index: Box::new(MigExpr::Ident(idx_param_name)),
            },
        });
    }

    let mut body_stmts =
        lower_block(body, None, false, ctx, &mut step_ssa, synthesized_fns, &step_fn_name);
    step_body.append(&mut body_stmts);

    let (step_ret_ty, step_ret_expr) = if state_vars.is_empty() {
        (
            MigType::Named("int".to_string()),
            MigExpr::IntLit("0".to_string()),
        )
    } else if state_vars.len() == 1 {
        let cur = step_ssa
            .map
            .get(&state_vars[0])
            .cloned()
            .unwrap_or_else(|| state_vars[0].clone());
        (state_types[0].clone(), MigExpr::Ident(cur))
    } else {
        let elems: Vec<MigExpr> = state_vars
            .iter()
            .map(|sv| {
                let cur = step_ssa.map.get(sv).cloned().unwrap_or_else(|| sv.clone());
                MigExpr::Ident(cur)
            })
            .collect();
        (MigType::Tuple(state_types), MigExpr::Tuple(elems))
    };
    step_body.push(MigStmt::Return(Some(step_ret_expr)));

    synthesized_fns.push(MigFn {
        name: step_fn_name.clone(),
        is_v1_origin: false,
        arms: vec![MigArm {
            params: step_params,
            guard: None,
            return_type: step_ret_ty,
            output_name: None,
            body: step_body,
        }],
    });

    let init_state_expr = if state_vars.is_empty() {
        MigExpr::IntLit("0".to_string())
    } else if state_vars.len() == 1 {
        let cur = ssa
            .map
            .get(&state_vars[0])
            .cloned()
            .unwrap_or_else(|| state_vars[0].clone());
        MigExpr::Ident(cur)
    } else {
        MigExpr::Tuple(
            state_vars
                .iter()
                .map(|sv| {
                    let cur = ssa.map.get(sv).cloned().unwrap_or_else(|| sv.clone());
                    MigExpr::Ident(cur)
                })
                .collect(),
        )
    };

    let mut evolve_args = vec![
        (None, init_state_expr),
        (None, domain_expr),
        (None, MigExpr::Ident(step_fn_name)),
    ];
    if let Some(ea) = call_env_arg {
        evolve_args.push((None, ea));
    }
    let evolve_call = MigExpr::Call {
        callee: Box::new(MigExpr::Ident("evolve".to_string())),
        args: evolve_args,
    };

    let mut out = Vec::new();
    if let Some((_, _, let_seq)) = seq_iter_tmp_let {
        out.push(let_seq);
    }
    if state_vars.is_empty() {
        out.push(MigStmt::Let {
            mutable: false,
            pat: MigLetPat::Single("_".to_string()),
            ty: None,
            value: evolve_call,
        });
    } else if state_vars.len() == 1 {
        let next_s = ssa.rebind(&state_vars[0], None);
        out.push(MigStmt::Let {
            mutable: false,
            pat: MigLetPat::Single(next_s),
            ty: None,
            value: evolve_call,
        });
    } else {
        let mut tuple_lhs = Vec::with_capacity(state_vars.len());
        for sv in &state_vars {
            let next_s = ssa.rebind(sv, None);
            tuple_lhs.push((next_s, None));
        }
        out.push(MigStmt::Let {
            mutable: false,
            pat: MigLetPat::Tuple(tuple_lhs),
            ty: None,
            value: evolve_call,
        });
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn lower_if_chain(
    branches: &[(MigExpr, Vec<MigStmt>)],
    else_body: &[MigStmt],
    ctx: &mut LowerCtx,
    ssa: &mut ArmSsa,
    synthesized_fns: &mut Vec<MigFn>,
    owner_fn: &str,
) -> Vec<MigStmt> {
    if branches.is_empty() {
        return lower_block(else_body, None, false, ctx, ssa, synthesized_fns, owner_fn);
    }

    let (cond, then_body) = &branches[0];
    let rest_branches = &branches[1..];
    let cond_low = lower_expr(cond, ctx, ssa, false);

    // If all arms return cleanly without side effects, lower to `return select(...)`
    if rest_branches.is_empty() && !else_body.is_empty() {
        let mut ssa_try_t = ArmSsa {
            map: ssa.map.clone(),
            used_names: ssa.used_names.clone(),
            var_types: ssa.var_types.clone(),
            seq: ssa.seq,
            is_v1_origin: ssa.is_v1_origin,
        };
        let mut hoisted = Vec::new();
        if let Some(t_ret) = extract_pure_return_branch(
            then_body,
            ctx,
            &mut ssa_try_t,
            synthesized_fns,
            owner_fn,
            &mut hoisted,
        ) {
            let mut ssa_try_e = ArmSsa {
                map: ssa.map.clone(),
                used_names: ssa_try_t.used_names.clone(),
                var_types: ssa.var_types.clone(),
                seq: ssa_try_t.seq,
                is_v1_origin: ssa.is_v1_origin,
            };
            if let Some(e_ret) = extract_pure_return_branch(
                else_body,
                ctx,
                &mut ssa_try_e,
                synthesized_fns,
                owner_fn,
                &mut hoisted,
            ) {
                ssa.used_names = ssa_try_e.used_names;
                ssa.seq = ssa_try_e.seq;
                ctx.used_calls.insert("select".to_string());
                hoisted.push(MigStmt::Return(Some(MigExpr::Call {
                    callee: Box::new(MigExpr::Ident("select".to_string())),
                    args: vec![(None, cond_low), (None, t_ret), (None, e_ret)],
                })));
                return hoisted;
            }
        }
    }

    // General SSA if-conversion merging modified bindings via `select(cond, v_then, v_else)`
    let pre_map = ssa.map.clone();
    let pre_types = ssa.var_types.clone();

    let mut out = Vec::new();
    let mut then_ssa = ArmSsa {
        map: pre_map.clone(),
        used_names: ssa.used_names.clone(),
        var_types: pre_types.clone(),
        seq: ssa.seq,
        is_v1_origin: ssa.is_v1_origin,
    };
    let mut then_stmts =
        lower_block(then_body, None, false, ctx, &mut then_ssa, synthesized_fns, owner_fn);
    out.append(&mut then_stmts);

    let mut else_ssa = ArmSsa {
        map: pre_map.clone(),
        used_names: then_ssa.used_names.clone(),
        var_types: pre_types,
        seq: then_ssa.seq,
        is_v1_origin: ssa.is_v1_origin,
    };
    let mut else_stmts = if !rest_branches.is_empty() {
        lower_if_chain(
            rest_branches,
            else_body,
            ctx,
            &mut else_ssa,
            synthesized_fns,
            owner_fn,
        )
    } else if !else_body.is_empty() {
        lower_block(else_body, None, false, ctx, &mut else_ssa, synthesized_fns, owner_fn)
    } else {
        Vec::new()
    };
    out.append(&mut else_stmts);

    ssa.used_names = else_ssa.used_names;
    ssa.seq = else_ssa.seq;

    let mut candidate_vars: BTreeSet<String> = BTreeSet::new();
    for k in then_ssa.map.keys().chain(else_ssa.map.keys()) {
        if pre_map.contains_key(k)
            || (then_ssa.map.contains_key(k) && else_ssa.map.contains_key(k))
        {
            candidate_vars.insert(k.clone());
        }
    }

    for var in candidate_vars {
        let t_val = then_ssa.map.get(&var).or_else(|| pre_map.get(&var));
        let e_val = else_ssa.map.get(&var).or_else(|| pre_map.get(&var));
        if let (Some(tv), Some(ev)) = (t_val, e_val) {
            if tv != ev {
                ctx.used_calls.insert("select".to_string());
                let vty = then_ssa
                    .var_types
                    .get(&var)
                    .or_else(|| else_ssa.var_types.get(&var))
                    .cloned();
                let merged = ssa.rebind(&var, vty);
                out.push(MigStmt::Let {
                    mutable: false,
                    pat: MigLetPat::Single(merged),
                    ty: None,
                    value: MigExpr::Call {
                        callee: Box::new(MigExpr::Ident("select".to_string())),
                        args: vec![
                            (None, cond_low.clone()),
                            (None, MigExpr::Ident(tv.clone())),
                            (None, MigExpr::Ident(ev.clone())),
                        ],
                    },
                });
            }
        }
    }

    out
}

fn lower_block(
    stmts: &[MigStmt],
    output_name: Option<&str>,
    is_top_arm: bool,
    ctx: &mut LowerCtx,
    ssa: &mut ArmSsa,
    synthesized_fns: &mut Vec<MigFn>,
    owner_fn: &str,
) -> Vec<MigStmt> {
    let mut out = Vec::new();
    let mut idx = 0usize;

    while idx < stmts.len() {
        let is_last = idx + 1 == stmts.len();
        match &stmts[idx] {
            MigStmt::Let { pat, ty, value, .. } => {
                let mig_ty = ty.as_ref().map(MigType::migrate);
                let inferred = mig_ty
                    .clone()
                    .or_else(|| ctx.infer_expr_type(value, &ssa.var_types));
                let low_val = lower_expr(value, ctx, ssa, false);
                let new_pat = match pat {
                    MigLetPat::Single(n) => {
                        let bound = ssa.rebind(n, inferred);
                        MigLetPat::Single(bound)
                    }
                    MigLetPat::Tuple(elems) => {
                        let mut new_elems = Vec::with_capacity(elems.len());
                        for (en, ety) in elems {
                            let mt = ety.as_ref().map(MigType::migrate);
                            let bound = ssa.rebind(en, mt.clone());
                            new_elems.push((bound, mt));
                        }
                        MigLetPat::Tuple(new_elems)
                    }
                };
                out.push(MigStmt::Let {
                    mutable: false,
                    pat: new_pat,
                    ty: mig_ty,
                    value: low_val,
                });
            }
            MigStmt::AssignVar { name, value } => {
                let low_val = lower_expr(value, ctx, ssa, false);
                if is_top_arm && is_last && output_name == Some(name.as_str()) {
                    out.push(MigStmt::Return(Some(low_val)));
                } else {
                    let inferred = ssa
                        .var_types
                        .get(name)
                        .cloned()
                        .or_else(|| ctx.infer_expr_type(value, &ssa.var_types));
                    let next_name = ssa.rebind(name, inferred);
                    out.push(MigStmt::Let {
                        mutable: false,
                        pat: MigLetPat::Single(next_name),
                        ty: None,
                        value: low_val,
                    });
                }
            }
            MigStmt::AssignIndex { name, index, value } => {
                ctx.used_calls.insert("set_at".to_string());
                let cur_arr = ssa
                    .map
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| name.clone());
                let low_idx = lower_expr(index, ctx, ssa, false);
                let low_val = lower_expr(value, ctx, ssa, false);
                let next_arr = ssa.rebind(name, None);
                out.push(MigStmt::Let {
                    mutable: false,
                    pat: MigLetPat::Single(next_arr),
                    ty: None,
                    value: MigExpr::Call {
                        callee: Box::new(MigExpr::Ident("set_at".to_string())),
                        args: vec![
                            (None, MigExpr::Ident(cur_arr)),
                            (None, low_idx),
                            (None, low_val),
                        ],
                    },
                });
            }
            MigStmt::Assert(e) => {
                out.push(MigStmt::Assert(lower_expr(e, ctx, ssa, false)));
            }
            MigStmt::Emit(e) => {
                out.push(MigStmt::Emit(lower_expr(e, ctx, ssa, false)));
            }
            MigStmt::Return(Some(e)) => {
                out.push(MigStmt::Return(Some(lower_expr(e, ctx, ssa, false))));
            }
            MigStmt::Return(None) => {
                if let Some(out_n) = output_name {
                    if let Some(cur) = ssa.map.get(out_n).cloned() {
                        out.push(MigStmt::Return(Some(MigExpr::Ident(cur))));
                    } else {
                        out.push(MigStmt::Return(None));
                    }
                } else {
                    out.push(MigStmt::Return(None));
                }
            }
            MigStmt::Pass => out.push(MigStmt::Pass),
            MigStmt::QuarantineBlock(body) => {
                let mut child_ssa = ArmSsa {
                    map: ssa.map.clone(),
                    used_names: ssa.used_names.clone(),
                    var_types: ssa.var_types.clone(),
                    seq: ssa.seq,
                    is_v1_origin: ssa.is_v1_origin,
                };
                let lowered_q = lower_block(
                    body,
                    None,
                    false,
                    ctx,
                    &mut child_ssa,
                    synthesized_fns,
                    owner_fn,
                );
                ssa.used_names = child_ssa.used_names;
                ssa.seq = child_ssa.seq;
                out.push(MigStmt::QuarantineBlock(lowered_q));
            }
            MigStmt::If {
                branches,
                else_body,
            } => {
                // Handle `if cond: return a [elif c2: return b] \n <rest ending in return>`
                if else_body.is_empty()
                    && idx + 1 < stmts.len()
                    && branches.iter().all(|(_, b)| block_ends_with_return(b))
                    && block_ends_with_return(&stmts[idx + 1..])
                {
                    let combined_else = stmts[idx + 1..].to_vec();
                    let mut lowered_if = lower_if_chain(
                        branches,
                        &combined_else,
                        ctx,
                        ssa,
                        synthesized_fns,
                        owner_fn,
                    );
                    out.append(&mut lowered_if);
                    break;
                }
                let mut lowered_if =
                    lower_if_chain(branches, else_body, ctx, ssa, synthesized_fns, owner_fn);
                out.append(&mut lowered_if);
            }
            MigStmt::For { var, iter, body } => {
                if stmts_contain_return(body)
                    && idx + 2 == stmts.len()
                    && matches!(&stmts[idx + 1], MigStmt::Return(Some(_)))
                {
                    let MigStmt::Return(Some(fallback_expr)) = &stmts[idx + 1] else {
                        unreachable!()
                    };
                    let done_raw = ssa.fresh_for("ret_done");
                    let val_raw = ssa.fresh_for("ret_val");
                    let val_ty = ctx.infer_expr_type(fallback_expr, &ssa.var_types);
                    let low_fb = lower_expr(fallback_expr, ctx, ssa, false);

                    let done_ssa = ssa.bind_initial(
                        &done_raw,
                        Some(MigType::Named("bool".to_string())),
                    );
                    let val_ssa = ssa.bind_initial(&val_raw, val_ty);
                    out.push(MigStmt::Let {
                        mutable: false,
                        pat: MigLetPat::Single(done_ssa),
                        ty: None,
                        value: MigExpr::BoolLit(false),
                    });
                    out.push(MigStmt::Let {
                        mutable: false,
                        pat: MigLetPat::Single(val_ssa),
                        ty: None,
                        value: low_fb,
                    });

                    let rewritten_inner =
                        rewrite_returns_to_flags(body, &done_raw, &val_raw);
                    let guarded_body = vec![MigStmt::If {
                        branches: vec![(
                            MigExpr::Unary {
                                op: "not".to_string(),
                                operand: Box::new(MigExpr::Ident(done_raw.clone())),
                            },
                            rewritten_inner,
                        )],
                        else_body: Vec::new(),
                    }];
                    let mut lowered_loop = lower_for_stmt(
                        var,
                        iter,
                        &guarded_body,
                        ctx,
                        ssa,
                        synthesized_fns,
                        owner_fn,
                    );
                    out.append(&mut lowered_loop);
                    let final_val = ssa
                        .map
                        .get(&val_raw)
                        .cloned()
                        .unwrap_or(val_raw);
                    out.push(MigStmt::Return(Some(MigExpr::Ident(final_val))));
                    break;
                }
                let mut lowered_loop =
                    lower_for_stmt(var, iter, body, ctx, ssa, synthesized_fns, owner_fn);
                out.append(&mut lowered_loop);
            }
            MigStmt::ExprStmt(e) => {
                let low = lower_expr(e, ctx, ssa, false);
                if is_top_arm && is_last && output_name.is_none() {
                    out.push(MigStmt::Return(Some(low)));
                } else {
                    out.push(MigStmt::ExprStmt(low));
                }
            }
        }
        idx += 1;
    }

    if is_top_arm {
        if let Some(out_n) = output_name {
            let already_returned = matches!(out.last(), Some(MigStmt::Return(_)));
            if !already_returned {
                if let Some(cur) = ssa.map.get(out_n).cloned() {
                    out.push(MigStmt::Return(Some(MigExpr::Ident(cur))));
                }
            }
        }
    }

    out
}

fn bind_pat_vars(pat: &MigPat, ty: Option<&MigType>, ssa: &mut ArmSsa) -> MigPat {
    match pat {
        MigPat::Wildcard => MigPat::Wildcard,
        MigPat::Binding(n) => MigPat::Binding(ssa.bind_initial(n, ty.cloned())),
        MigPat::NullLit => MigPat::NullLit,
        MigPat::IntLit(n) => MigPat::IntLit(n.clone()),
        MigPat::BoolLit(b) => MigPat::BoolLit(*b),
        MigPat::BytesLit(bs) => MigPat::BytesLit(bs.clone()),
        MigPat::TritLit(t) => MigPat::TritLit(t.clone()),
        MigPat::RatioLit(r) => MigPat::RatioLit(r.clone()),
        MigPat::Addr8Lit(a) => MigPat::Addr8Lit(a.clone()),
        MigPat::SupportLit(s) => MigPat::SupportLit(s.clone()),
        MigPat::EnumOrCtor { path, args } => MigPat::EnumOrCtor {
            path: path.clone(),
            args: args.iter().map(|a| bind_pat_vars(a, None, ssa)).collect(),
        },
        MigPat::Tuple(ps) => {
            let elem_tys: Option<&[MigType]> = match ty {
                Some(MigType::Tuple(ts)) if ts.len() == ps.len() => Some(ts.as_slice()),
                _ => None,
            };
            MigPat::Tuple(
                ps.iter()
                    .enumerate()
                    .map(|(i, p)| bind_pat_vars(p, elem_tys.map(|ts| &ts[i]), ssa))
                    .collect(),
            )
        }
    }
}

fn lower_dec_fn(func: &MigFn, ctx: &mut LowerCtx) -> Vec<MigFn> {
    let mut synthesized_fns = Vec::new();
    let mut lowered_arms = Vec::with_capacity(func.arms.len());

    for arm in &func.arms {
        let mut ssa = ArmSsa::new(func.is_v1_origin);
        let mut new_params = Vec::with_capacity(arm.params.len());
        for p in &arm.params {
            match p {
                MigArmParam::TypedVar { name, ty } => {
                    let mty = ty.migrate();
                    let bound = ssa.bind_initial(name, Some(mty.clone()));
                    new_params.push(MigArmParam::TypedVar {
                        name: bound,
                        ty: mty,
                    });
                }
                MigArmParam::TypedPattern { pat, ty } => {
                    let mty = ty.migrate();
                    let bpat = bind_pat_vars(pat, Some(&mty), &mut ssa);
                    new_params.push(MigArmParam::TypedPattern { pat: bpat, ty: mty });
                }
                MigArmParam::Pattern(pat) => {
                    let bpat = bind_pat_vars(pat, None, &mut ssa);
                    new_params.push(MigArmParam::Pattern(bpat));
                }
            }
        }

        let ret_ty = arm.return_type.migrate();
        if let Some(ref out_n) = arm.output_name {
            ssa.var_types.insert(out_n.clone(), ret_ty.clone());
        }

        let new_guard = arm
            .guard
            .as_ref()
            .map(|g| lower_expr(g, ctx, &ssa, false));
        let new_body = lower_block(
            &arm.body,
            arm.output_name.as_deref(),
            true,
            ctx,
            &mut ssa,
            &mut synthesized_fns,
            &func.name,
        );

        lowered_arms.push(MigArm {
            params: new_params,
            guard: new_guard,
            return_type: ret_ty,
            output_name: None,
            body: new_body,
        });
    }

    let mut all = synthesized_fns;
    all.push(MigFn {
        name: sanitize_binder_name(&func.name),
        is_v1_origin: false,
        arms: lowered_arms,
    });
    all
}

fn migrate_import_symbol_name(path: &[String], sym: &str) -> String {
    if path.len() == 2 && path[0] == "aod" && path[1] == "core" {
        match sym {
            "u8" => "b8".to_string(),
            "t9" | "t32" | "int8" | "int16" | "int32" | "int64" => "int".to_string(),
            other => other.to_string(),
        }
    } else {
        sym.to_string()
    }
}

fn builtin_owner_namespace(call_key: &str) -> Option<(&'static str, &str)> {
    match call_key {
        "witness::fold" => Some(("aod::witness", "fold")),
        "trace" | "step" | "hinge" | "dwell" | "close" | "phases" | "beats" | "rate"
        | "outer" | "fuse" | "coupling" | "contact" | "cadence" | "lag" | "drift" => {
            Some(("aod::witness", call_key))
        }
        "probability" | "sadar_promote" | "shedding_excess" => Some(("aod::ledger", call_key)),
        "t1" | "q4" | "b8" | "b64" | "b64_bits" | "u64_int" | "int" | "ratio" | "bytes"
        | "pack" | "unpack" | "num" | "den" | "neg" | "abs" | "is_hamming1" | "popcnt_xor"
        | "select" | "evolve" | "set_at" | "list_set" | "len" | "min" | "max" | "mux" | "walk" => {
            Some(("aod::core", call_key))
        }
        _ => None,
    }
}

fn lower_program(items: Vec<MigItem>) -> Vec<MigItem> {
    let mut ctx = LowerCtx::new(&items);
    let mut non_use_items = Vec::new();
    let mut raw_uses: Vec<(Vec<String>, Option<Vec<String>>)> = Vec::new();

    for item in items {
        match item {
            MigItem::Use { path, items: syms } => {
                raw_uses.push((path, syms));
            }
            MigItem::Const { name, ty, value } => {
                let dummy = ArmSsa::new(false);
                non_use_items.push(MigItem::Const {
                    name: sanitize_binder_name(&name),
                    ty: ty.map(|t| t.migrate()),
                    value: lower_expr(&value, &mut ctx, &dummy, false),
                });
            }
            MigItem::Struct { name, fields } => {
                non_use_items.push(MigItem::Struct {
                    name,
                    fields: fields
                        .into_iter()
                        .map(|(fnm, fty)| (sanitize_binder_name(&fnm), fty.migrate()))
                        .collect(),
                });
            }
            MigItem::Enum { name, variants } => {
                non_use_items.push(MigItem::Enum {
                    name,
                    variants: variants
                        .into_iter()
                        .map(|(vn, ps)| {
                            (vn, ps.into_iter().map(|t| t.migrate()).collect())
                        })
                        .collect(),
                });
            }
            MigItem::DecFn(f) => {
                for lf in lower_dec_fn(&f, &mut ctx) {
                    non_use_items.push(MigItem::DecFn(lf));
                }
            }
            MigItem::Chamber {
                name,
                space,
                walls,
                engine,
            } => {
                let dummy = ArmSsa::new(false);
                non_use_items.push(MigItem::Chamber {
                    name,
                    space: lower_expr(&space, &mut ctx, &dummy, false),
                    walls,
                    engine,
                });
            }
            MigItem::Prepare {
                name,
                chamber,
                lets,
                clocks,
                injections,
            } => {
                let mut p_ssa = ArmSsa::new(false);
                let mut synth = Vec::new();
                let new_lets = lower_block(
                    &lets,
                    None,
                    false,
                    &mut ctx,
                    &mut p_ssa,
                    &mut synth,
                    &name,
                );
                for sf in synth {
                    non_use_items.push(MigItem::DecFn(sf));
                }
                let new_clocks = clocks
                    .into_iter()
                    .map(|(cn, per)| (cn, lower_expr(&per, &mut ctx, &p_ssa, false)))
                    .collect();
                let new_injs = injections
                    .into_iter()
                    .map(|inj| MigInject {
                        support: lower_expr(&inj.support, &mut ctx, &p_ssa, false),
                        pos: lower_expr(&inj.pos, &mut ctx, &p_ssa, false),
                        dir: inj.dir,
                        orient: lower_expr(&inj.orient, &mut ctx, &p_ssa, false),
                        shells: lower_expr(&inj.shells, &mut ctx, &p_ssa, false),
                    })
                    .collect();
                non_use_items.push(MigItem::Prepare {
                    name,
                    chamber,
                    lets: new_lets,
                    clocks: new_clocks,
                    injections: new_injs,
                });
            }
            MigItem::Observe { name, probes } => {
                let mut new_probes = Vec::with_capacity(probes.len());
                for pr in probes {
                    match pr {
                        MigProbe::Declarative {
                            kind,
                            name: pn,
                            region,
                            capture,
                        } => {
                            let dummy = ArmSsa::new(false);
                            new_probes.push(MigProbe::Declarative {
                                kind,
                                name: pn,
                                region: lower_expr(&region, &mut ctx, &dummy, false),
                                capture,
                            });
                        }
                        MigProbe::Procedural {
                            kind,
                            name: pn,
                            param_name,
                            param_ty,
                            return_type,
                            body,
                        } => {
                            let mut pr_ssa = ArmSsa::new(false);
                            let m_pty = Some(
                                param_ty
                                    .map(|t| t.migrate())
                                    .unwrap_or_else(|| {
                                        MigType::ReadOnly(Box::new(MigType::Named(
                                            "ChamberView".to_string(),
                                        )))
                                    }),
                            );
                            let bound_p = pr_ssa.bind_initial(&param_name, m_pty.clone());
                            let mut synth = Vec::new();
                            let new_body = lower_block(
                                &body,
                                None,
                                true,
                                &mut ctx,
                                &mut pr_ssa,
                                &mut synth,
                                &pn,
                            );
                            for sf in synth {
                                non_use_items.push(MigItem::DecFn(sf));
                            }
                            new_probes.push(MigProbe::Procedural {
                                kind,
                                name: pn,
                                param_name: bound_p,
                                param_ty: m_pty,
                                return_type: return_type.migrate(),
                                body: new_body,
                            });
                        }
                    }
                }
                non_use_items.push(MigItem::Observe {
                    name,
                    probes: new_probes,
                });
            }
            MigItem::QuarantineMod {
                name,
                items: sub_items,
            } => {
                let mut new_subs = Vec::new();
                for sub in sub_items {
                    match sub {
                        MigItem::Const {
                            name: cn,
                            ty,
                            value,
                        } => {
                            let dummy = ArmSsa::new(false);
                            new_subs.push(MigItem::Const {
                                name: sanitize_binder_name(&cn),
                                ty: ty.map(|t| t.migrate()),
                                value: lower_expr(&value, &mut ctx, &dummy, false),
                            });
                        }
                        MigItem::DecFn(f) => {
                            for lf in lower_dec_fn(&f, &mut ctx) {
                                new_subs.push(MigItem::DecFn(lf));
                            }
                        }
                        other => new_subs.push(other),
                    }
                }
                non_use_items.push(MigItem::QuarantineMod {
                    name,
                    items: new_subs,
                });
            }
        }
    }

    // Normalize existing `use` items and inject missing builtin imports
    let mut grouped_uses: BTreeMap<Vec<String>, Vec<String>> = BTreeMap::new();
    let mut namespace_uses: Vec<Vec<String>> = Vec::new();
    let mut already_imported_syms: BTreeSet<String> = BTreeSet::new();

    for (path, syms_opt) in raw_uses {
        match syms_opt {
            Some(syms) => {
                let entry = grouped_uses.entry(path.clone()).or_default();
                for s in syms {
                    let mapped = migrate_import_symbol_name(&path, &s);
                    already_imported_syms.insert(mapped.clone());
                    if !entry.contains(&mapped) {
                        entry.push(mapped);
                    }
                }
            }
            None => {
                if path.len() >= 3 && (path[0] == "aod" || path[0] == "stdlib") {
                    let mod_path = path[..path.len() - 1].to_vec();
                    let last_sym = migrate_import_symbol_name(&mod_path, path.last().expect("len"));
                    already_imported_syms.insert(last_sym.clone());
                    let entry = grouped_uses.entry(mod_path).or_default();
                    if !entry.contains(&last_sym) {
                        entry.push(last_sym);
                    }
                } else {
                    if let Some(last) = path.last() {
                        already_imported_syms.insert(last.clone());
                    }
                    if !namespace_uses.contains(&path) {
                        namespace_uses.push(path);
                    }
                }
            }
        }
    }

    if ctx.needs_sectors_scientific_status
        && !already_imported_syms.contains("ScientificStatus")
    {
        let sec_path = vec!["stdlib".to_string(), "aod".to_string(), "sectors".to_string()];
        let entry = grouped_uses.entry(sec_path).or_default();
        entry.push("ScientificStatus".to_string());
        already_imported_syms.insert("ScientificStatus".to_string());
    }

    for call_key in &ctx.used_calls {
        if let Some((ns_str, sym)) = builtin_owner_namespace(call_key) {
            if !already_imported_syms.contains(sym)
                && !ctx.local_fn_names.contains(sym)
                && !ctx.local_type_names.contains(sym)
            {
                let ns_vec: Vec<String> = ns_str.split("::").map(str::to_string).collect();
                let entry = grouped_uses.entry(ns_vec).or_default();
                if !entry.contains(&sym.to_string()) {
                    entry.push(sym.to_string());
                }
                already_imported_syms.insert(sym.to_string());
            }
        }
    }

    let mut final_items = Vec::new();
    for ns_path in namespace_uses {
        final_items.push(MigItem::Use {
            path: ns_path,
            items: None,
        });
    }
    for (mod_path, syms) in grouped_uses {
        if !syms.is_empty() {
            final_items.push(MigItem::Use {
                path: mod_path,
                items: Some(syms),
            });
        }
    }
    final_items.extend(non_use_items);
    final_items
}

// ============================================================================
// Pure-Wave 4-Space v3 Emitter
// ============================================================================

fn emit_pad(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push_str("    ");
    }
}

fn emit_bytes_literal(bs: &[u8]) -> String {
    let mut s = String::with_capacity(bs.len() + 3);
    s.push_str("b\"");
    for &b in bs {
        match b {
            b'"' => s.push_str("\\\""),
            b'\\' => s.push_str("\\\\"),
            b'\n' => s.push_str("\\n"),
            b'\r' => s.push_str("\\r"),
            b'\t' => s.push_str("\\t"),
            0 => s.push_str("\\0"),
            0x20..=0x7E => s.push(b as char),
            _ => {
                let _ = write!(s, "\\x{b:02x}");
            }
        }
    }
    s.push('"');
    s
}

fn emit_pat(pat: &MigPat) -> String {
    match pat {
        MigPat::Wildcard => "_".to_string(),
        MigPat::Binding(n) => n.clone(),
        MigPat::NullLit => "null".to_string(),
        MigPat::IntLit(n) => n.clone(),
        MigPat::BoolLit(b) => if *b { "true" } else { "false" }.to_string(),
        MigPat::BytesLit(bs) => emit_bytes_literal(bs),
        MigPat::TritLit(t) => t.clone(),
        MigPat::RatioLit(r) => r.clone(),
        MigPat::Addr8Lit(a) => a.clone(),
        MigPat::SupportLit(s) => s.clone(),
        MigPat::EnumOrCtor { path, args } => {
            if args.is_empty() {
                path.join("::")
            } else {
                let ps: Vec<String> = args.iter().map(emit_pat).collect();
                format!("{}({})", path.join("::"), ps.join(", "))
            }
        }
        MigPat::Tuple(ps) => {
            let xs: Vec<String> = ps.iter().map(emit_pat).collect();
            format!("({})", xs.join(", "))
        }
    }
}

fn emit_atom(e: &MigExpr) -> String {
    match e {
        MigExpr::Unary { .. }
        | MigExpr::Adverb { .. }
        | MigExpr::Binary { .. }
        | MigExpr::Pipe { .. }
        | MigExpr::Range { .. }
        | MigExpr::Simulate { .. } => format!("({})", emit_expr(e)),
        _ => emit_expr(e),
    }
}

fn emit_expr(e: &MigExpr) -> String {
    match e {
        MigExpr::NullLit => "null".to_string(),
        MigExpr::IntLit(n) => n.clone(),
        MigExpr::BoolLit(b) => if *b { "true" } else { "false" }.to_string(),
        MigExpr::BytesLit(bs) | MigExpr::StrLit(bs) => emit_bytes_literal(bs),
        MigExpr::RatioLit(r) => r.clone(),
        MigExpr::TritLit(t) => t.clone(),
        MigExpr::Addr8Lit(a) => a.clone(),
        MigExpr::SupportLit(s) => s.clone(),
        MigExpr::Ident(n) => n.clone(),
        MigExpr::Path(segs) => segs.join("::"),
        MigExpr::Tuple(xs) => {
            let ps: Vec<String> = xs.iter().map(emit_expr).collect();
            if ps.len() == 1 {
                format!("({},)", ps[0])
            } else {
                format!("({})", ps.join(", "))
            }
        }
        MigExpr::SeqLit(xs) => {
            let ps: Vec<String> = xs.iter().map(emit_expr).collect();
            format!("[{}]", ps.join(", "))
        }
        MigExpr::MapLit(kvs) => {
            let ps: Vec<String> = kvs
                .iter()
                .map(|(k, v)| format!("({}, {})", emit_expr(k), emit_expr(v)))
                .collect();
            format!("map[{}]", ps.join(", "))
        }
        MigExpr::StructLit { path, fields } => {
            let ps: Vec<String> = fields.iter().map(|(_, v)| emit_expr(v)).collect();
            format!("{}({})", path.join("::"), ps.join(", "))
        }
        MigExpr::WindowLit { start, end } => {
            format!("({}, {}]", emit_expr(start), emit_expr(end))
        }
        MigExpr::Range { start, end } => {
            format!("{}..{}", emit_atom(start), emit_atom(end))
        }
        MigExpr::BoxExpr(dims) => {
            let ds: Vec<String> = dims.iter().map(emit_expr).collect();
            format!("box([{}])", ds.join(", "))
        }
        MigExpr::Simulate {
            prepare,
            ticks,
            observers,
        } => {
            if observers.is_empty() {
                format!("simulate {prepare} for {} ticks", emit_expr(ticks))
            } else {
                format!(
                    "simulate {prepare} for {} ticks with {}",
                    emit_expr(ticks),
                    observers.join(", ")
                )
            }
        }
        MigExpr::Unary { op, operand } => match op.as_str() {
            "not" => format!("not {}", emit_atom(operand)),
            _ => format!("{op}{}", emit_atom(operand)),
        },
        MigExpr::Adverb { op, operand } => format!("{op} {}", emit_atom(operand)),
        MigExpr::Binary { op, left, right } => {
            format!("({} {op} {})", emit_expr(left), emit_expr(right))
        }
        MigExpr::Conditional {
            then_expr,
            cond,
            else_expr,
        } => format!(
            "select({}, {}, {})",
            emit_expr(cond),
            emit_expr(then_expr),
            emit_expr(else_expr)
        ),
        MigExpr::Pipe { input, stage } => {
            let lhs = match input.as_ref() {
                MigExpr::Pipe { .. } => emit_expr(input),
                _ => emit_atom(input),
            };
            let st = match stage.as_ref() {
                MigPipeStage::Adverb(op) => op.clone(),
                MigPipeStage::Boundary => "∂".to_string(),
                MigPipeStage::Section { op, rhs } => format!("({op} {})", emit_expr(rhs)),
                MigPipeStage::Call { callee, extra_args } => {
                    if extra_args.is_empty() {
                        emit_expr(callee)
                    } else {
                        let as_s: Vec<String> = extra_args.iter().map(emit_expr).collect();
                        format!("{}({})", emit_expr(callee), as_s.join(", "))
                    }
                }
            };
            format!("({lhs} |> {st})")
        }
        MigExpr::Field { base, name } => format!("{}.{name}", emit_atom(base)),
        MigExpr::Index { base, index } => format!("{}[{}]", emit_atom(base), emit_expr(index)),
        MigExpr::Call { callee, args } => {
            let as_s: Vec<String> = args.iter().map(|(_, a)| emit_expr(a)).collect();
            format!("{}({})", emit_atom(callee), as_s.join(", "))
        }
    }
}

fn emit_stmts(out: &mut String, stmts: &[MigStmt], indent: usize) {
    if stmts.is_empty() {
        emit_pad(out, indent);
        out.push_str("pass\n");
        return;
    }
    for s in stmts {
        emit_stmt(out, s, indent);
    }
}

fn emit_stmt(out: &mut String, stmt: &MigStmt, indent: usize) {
    match stmt {
        MigStmt::Let { pat, ty, value, .. } => {
            emit_pad(out, indent);
            let lhs = match pat {
                MigLetPat::Single(n) => n.clone(),
                MigLetPat::Tuple(elems) => {
                    let ps: Vec<String> = elems
                        .iter()
                        .map(|(n, t)| match t {
                            Some(mt) => format!("{n}: {}", mt.render()),
                            None => n.clone(),
                        })
                        .collect();
                    format!("({})", ps.join(", "))
                }
            };
            match ty {
                Some(t) => {
                    let _ = writeln!(out, "let {lhs}: {} = {}", t.render(), emit_expr(value));
                }
                None => {
                    let _ = writeln!(out, "let {lhs} = {}", emit_expr(value));
                }
            }
        }
        MigStmt::Assert(e) => {
            emit_pad(out, indent);
            let _ = writeln!(out, "assert {}", emit_expr(e));
        }
        MigStmt::Emit(e) => {
            emit_pad(out, indent);
            let _ = writeln!(out, "emit {}", emit_expr(e));
        }
        MigStmt::Return(Some(e)) => {
            emit_pad(out, indent);
            let _ = writeln!(out, "return {}", emit_expr(e));
        }
        MigStmt::Return(None) => {
            emit_pad(out, indent);
            out.push_str("return\n");
        }
        MigStmt::Pass => {
            emit_pad(out, indent);
            out.push_str("pass\n");
        }
        MigStmt::QuarantineBlock(body) => {
            emit_pad(out, indent);
            out.push_str("quarantine Q_ext:\n");
            emit_stmts(out, body, indent + 1);
        }
        MigStmt::ExprStmt(e) => {
            emit_pad(out, indent);
            let _ = writeln!(out, "{}", emit_expr(e));
        }
        MigStmt::AssignVar { name, value } => {
            emit_pad(out, indent);
            let _ = writeln!(out, "let {name} = {}", emit_expr(value));
        }
        MigStmt::AssignIndex { name, index, value } => {
            emit_pad(out, indent);
            let _ = writeln!(
                out,
                "let {name} = list_set({name}, {}, {})",
                emit_expr(index),
                emit_expr(value)
            );
        }
        MigStmt::If { .. } | MigStmt::For { .. } => {
            unreachable!("if and for statements are lowered before v3 code emission")
        }
    }
}

fn emit_arm_param(p: &MigArmParam) -> String {
    match p {
        MigArmParam::TypedVar { name, ty } => format!("{name}: {}", ty.render()),
        MigArmParam::TypedPattern { pat, ty } => format!("{}: {}", emit_pat(pat), ty.render()),
        MigArmParam::Pattern(pat) => emit_pat(pat),
    }
}

fn emit_item(out: &mut String, item: &MigItem, indent: usize) {
    match item {
        MigItem::Use { path, items } => {
            emit_pad(out, indent);
            let p = path.join("::");
            match items {
                None => {
                    let _ = writeln!(out, "use {p}");
                }
                Some(syms) if syms.len() == 1 => {
                    let _ = writeln!(out, "use {p}::{}", syms[0]);
                }
                Some(syms) => {
                    let _ = writeln!(out, "use {p}:");
                    emit_pad(out, indent + 1);
                    let _ = writeln!(out, "{}", syms.join(", "));
                }
            }
        }
        MigItem::Const { name, ty, value } => {
            emit_pad(out, indent);
            match ty {
                Some(t) => {
                    let _ = writeln!(out, "const {name}: {} = {}", t.render(), emit_expr(value));
                }
                None => {
                    let _ = writeln!(out, "const {name} = {}", emit_expr(value));
                }
            }
        }
        MigItem::Struct { name, fields } => {
            emit_pad(out, indent);
            let _ = writeln!(out, "struct {name}:");
            for (fnm, fty) in fields {
                emit_pad(out, indent + 1);
                let _ = writeln!(out, "{fnm}: {}", fty.render());
            }
        }
        MigItem::Enum { name, variants } => {
            emit_pad(out, indent);
            let _ = writeln!(out, "enum {name}:");
            for (vn, payloads) in variants {
                emit_pad(out, indent + 1);
                if payloads.is_empty() {
                    let _ = writeln!(out, "{vn}");
                } else {
                    let ps: Vec<String> = payloads.iter().map(MigType::render).collect();
                    let _ = writeln!(out, "{vn}({})", ps.join(", "));
                }
            }
        }
        MigItem::DecFn(f) => {
            emit_pad(out, indent);
            let _ = writeln!(out, "dec {}:", f.name);
            for arm in &f.arms {
                emit_pad(out, indent + 1);
                let ps: Vec<String> = arm.params.iter().map(emit_arm_param).collect();
                let _ = write!(out, "({})", ps.join(", "));
                if let Some(ref g) = arm.guard {
                    let _ = write!(out, " where {}", emit_expr(g));
                }
                let _ = writeln!(out, " -> {}:", arm.return_type.render());
                emit_stmts(out, &arm.body, indent + 2);
            }
        }
        MigItem::Chamber {
            name,
            space,
            walls,
            engine,
        } => {
            emit_pad(out, indent);
            let _ = writeln!(out, "chamber {name}:");
            emit_pad(out, indent + 1);
            let _ = writeln!(out, "space = {}", emit_expr(space));
            emit_pad(out, indent + 1);
            let _ = writeln!(out, "walls = {walls}");
            if let Some(eng) = engine {
                emit_pad(out, indent + 1);
                let _ = writeln!(out, "engine = {eng}");
            }
        }
        MigItem::Prepare {
            name,
            chamber,
            lets,
            clocks,
            injections,
        } => {
            emit_pad(out, indent);
            let _ = writeln!(out, "prepare {name} in {chamber}:");
            for s in lets {
                emit_stmt(out, s, indent + 1);
            }
            for (cn, per) in clocks {
                emit_pad(out, indent + 1);
                let _ = writeln!(out, "clock {cn} = {}", emit_expr(per));
            }
            for inj in injections {
                emit_pad(out, indent + 1);
                let _ = writeln!(
                    out,
                    "inject {} at {}, dir {}, orient {}, shells {}",
                    emit_expr(&inj.support),
                    emit_expr(&inj.pos),
                    inj.dir,
                    emit_expr(&inj.orient),
                    emit_expr(&inj.shells)
                );
            }
        }
        MigItem::Observe { name, probes } => {
            emit_pad(out, indent);
            let _ = writeln!(out, "observe {name}:");
            for pr in probes {
                match pr {
                    MigProbe::Declarative {
                        kind,
                        name: pn,
                        region,
                        capture,
                    } => {
                        emit_pad(out, indent + 1);
                        let _ = writeln!(out, "{kind} {pn}:");
                        emit_pad(out, indent + 2);
                        let _ = writeln!(out, "region = {}", emit_expr(region));
                        if !capture.is_empty() {
                            emit_pad(out, indent + 2);
                            let _ = writeln!(out, "capture = [{}]", capture.join(", "));
                        }
                    }
                    MigProbe::Procedural {
                        kind,
                        name: pn,
                        param_name,
                        param_ty,
                        return_type,
                        body,
                    } => {
                        emit_pad(out, indent + 1);
                        let _ = writeln!(out, "{kind} {pn}:");
                        emit_pad(out, indent + 2);
                        let pty_s = param_ty
                            .as_ref()
                            .map(MigType::render)
                            .unwrap_or_else(|| "ReadOnly<ChamberView>".to_string());
                        let _ = writeln!(
                            out,
                            "({param_name}: {pty_s}) -> {}:",
                            return_type.render()
                        );
                        emit_stmts(out, body, indent + 3);
                    }
                }
            }
        }
        MigItem::QuarantineMod { name, items } => {
            emit_pad(out, indent);
            let _ = writeln!(out, "quarantine Q_ext {name}:");
            if items.is_empty() {
                emit_pad(out, indent + 1);
                out.push_str("pass\n");
            } else {
                for (i, sub) in items.iter().enumerate() {
                    if i > 0 {
                        out.push('\n');
                    }
                    emit_item(out, sub, indent + 1);
                }
            }
        }
    }
}

fn emit_program(header_comments: &[String], items: &[MigItem]) -> String {
    let mut out = String::new();
    if !header_comments.is_empty() {
        for c in header_comments {
            out.push_str(c);
            out.push('\n');
        }
        out.push('\n');
    }
    let mut prev_was_use = false;
    for (idx, item) in items.iter().enumerate() {
        let is_use = matches!(item, MigItem::Use { .. });
        if idx > 0 && !(prev_was_use && is_use) {
            out.push('\n');
        }
        emit_item(&mut out, item, 0);
        prev_was_use = is_use;
    }
    out
}

// ============================================================================
// Public Entrypoints
// ============================================================================

/// Deterministically translate a Frozen Dec v1 (`2bc927c`), PR #8 (`68d6705`), or Pure-Wave v3
/// `.dec` program into canonical 4-space Pure-Wave Trit-Boxed v3 source text.
pub fn migrate_to_v3(source: &str) -> DecResult<String> {
    let header_comments = extract_leading_comments(source);
    let tokens = lex_migrate(source)?;
    let mut parser = MigParser::new(tokens);
    let parsed_items = parser.parse_program()?;
    let lowered_items = lower_program(parsed_items);
    Ok(emit_program(&header_comments, &lowered_items))
}

/// Read `<path>` and deterministically translate its `.dec` source to Pure-Wave Trit-Boxed v3.
pub fn migrate_file_to_v3(path: &Path) -> DecResult<String> {
    let raw = std::fs::read_to_string(path).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::ParseError,
            format!("failed reading {}: {e}", path.display()),
        )
    })?;
    migrate_to_v3(&raw)
}
