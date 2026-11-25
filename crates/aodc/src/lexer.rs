use aod_core::bigint::BigInt;
use aod_core::error::CoreError;
use aod_core::ratio::Ratio;
use aod_core::support::SupportForm;
use aod_core::ternary::Addr8;

use crate::ast::{AdverbOp, TritLiteral};
use crate::diagnostics::{DecResult, Diagnostic, DiagnosticCode};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenKind {
    // Layout
    Indent,
    Dedent,
    Newline,
    Eof,

    // Hard keywords (contextual words like over/ticks/with/space/walls/clock/inject/etc. remain Ident!)
    Dec,
    Where,
    Null,
    Let,
    Const,
    Return,
    Emit,
    Assert,
    In,
    And,
    Or,
    Not,
    True,
    False,
    Use,
    Chamber,
    Prepare,
    Observe,
    Simulate,
    Quarantine,
    Pass,
    Struct,
    Enum,
    Match,

    // Identifiers & literals
    Ident(String),
    IntLit(BigInt),
    BytesLit(Vec<u8>),
    RatioLit(Ratio),
    Addr8Lit(Addr8),
    SupportLit(SupportForm),
    TritLit(TritLiteral),

    // Punctuation & operators
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Colon,
    ColonColon,
    Dot,
    DotDot,
    Arrow,
    PipeGt,
    Sieve,
    Cmp3,
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
    OuterAdd,
    OuterMul,
    Amp,
    Pipe,
    Caret,
    Tilde,
    Shl,
    Shr,
    BoundaryOp,
    Adverb(AdverbOp),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub col: usize,
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_cont(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn map_keyword_or_reject(s: &str, line: usize, col: usize) -> DecResult<TokenKind> {
    match s {
        "dec" => Ok(TokenKind::Dec),
        "Dec" => Err(Diagnostic::at(
            DiagnosticCode::ParseError,
            line,
            col,
            "uppercase 'Dec' is removed; use lowercase 'dec <name>:' with indented '(params) -> Ret:' arms",
        )),
        "if" | "elif" | "else" => Err(Diagnostic::at(
            DiagnosticCode::ParseError,
            line,
            col,
            format!(
                "imperative '{s}' is removed; use 'dec' signature arms, 'where' guards, 'select', 'mux', or 'match'"
            ),
        )),
        "while" | "break" | "continue" => Err(Diagnostic::at(
            DiagnosticCode::ParseError,
            line,
            col,
            format!(
                "imperative loop keyword '{s}' is removed; use rank lifting, '|>', 'walk', or 'settle'"
            ),
        )),
        "mut" => Err(Diagnostic::at(
            DiagnosticCode::ParseError,
            line,
            col,
            "mutable bindings ('mut') are removed; use immutable 'let', 'walk', or 'settle'",
        )),
        "where" => Ok(TokenKind::Where),
        "null" => Ok(TokenKind::Null),
        "let" => Ok(TokenKind::Let),
        "const" => Ok(TokenKind::Const),
        "return" => Ok(TokenKind::Return),
        "emit" => Ok(TokenKind::Emit),
        "assert" => Ok(TokenKind::Assert),
        "in" => Ok(TokenKind::In),
        "and" => Ok(TokenKind::And),
        "or" => Ok(TokenKind::Or),
        "not" => Ok(TokenKind::Not),
        "true" => Ok(TokenKind::True),
        "false" => Ok(TokenKind::False),
        "use" => Ok(TokenKind::Use),
        "chamber" => Ok(TokenKind::Chamber),
        "prepare" => Ok(TokenKind::Prepare),
        "observe" => Ok(TokenKind::Observe),
        "simulate" => Ok(TokenKind::Simulate),
        "quarantine" => Ok(TokenKind::Quarantine),
        "pass" => Ok(TokenKind::Pass),
        "struct" => Ok(TokenKind::Struct),
        "enum" => Ok(TokenKind::Enum),
        "match" => Ok(TokenKind::Match),
        _ => Ok(TokenKind::Ident(s.to_string())),
    }
}

/// Tokenize clean-break Dec source code with:
/// - Aggregate delimiter depth line joining (`[(0, 6], (4, 8]]` works cleanly).
/// - Strict float rejection vs `..` range (`24..40` lexes cleanly; `1.5`, `5.`, `.5`, `1e3` reject).
/// - Dedicated exact literals: `IntLit` (decimal + `0x` + `0b`), `BytesLit` (`b"..."`),
///   `RatioLit` (`1/3r`), `Addr8Lit` (`00_8`, `00|-`, `0o00`), `SupportLit` (`3:3:6`, `1:2:9_yro`),
///   `TritLit` (`0t`, `1t`, `0_H`, `0_D`), prefix `∂`, `<=>`, `?>`, `|>`, `+:`, `*:`, and adverbs.
pub fn tokenize(source: &str) -> DecResult<Vec<Token>> {
    let mut tokens: Vec<Token> = Vec::new();
    let mut indent_stack: Vec<usize> = vec![0];
    let mut delim_depth: usize = 0;

    let raw_lines: Vec<&str> = source.split('\n').collect();
    for (idx, raw_line) in raw_lines.iter().enumerate() {
        let line_no = idx + 1;
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let chars: Vec<char> = line.chars().collect();
        let mut pos = 0usize;

        // Measure leading indentation (reject tabs in leading whitespace!)
        let mut indent = 0usize;
        while pos < chars.len() {
            match chars[pos] {
                ' ' => {
                    indent += 1;
                    pos += 1;
                }
                '\t' => {
                    return Err(Diagnostic::at(
                        DiagnosticCode::LexError,
                        line_no,
                        pos + 1,
                        "tabs in indentation are unsupported",
                    ));
                }
                _ => break,
            }
        }

        // Skip blank or comment-only lines
        if pos >= chars.len() || chars[pos] == '#' {
            continue;
        }

        // Emit Indent / Dedent only at top-level delimiter depth == 0
        if delim_depth == 0 {
            let current_indent = *indent_stack.last().unwrap_or(&0);
            if indent > current_indent {
                indent_stack.push(indent);
                tokens.push(Token {
                    kind: TokenKind::Indent,
                    line: line_no,
                    col: 1,
                });
            } else if indent < current_indent {
                while let Some(&top) = indent_stack.last() {
                    if indent < top {
                        indent_stack.pop();
                        tokens.push(Token {
                            kind: TokenKind::Dedent,
                            line: line_no,
                            col: 1,
                        });
                    } else {
                        break;
                    }
                }
                if indent_stack.last().copied() != Some(indent) {
                    return Err(Diagnostic::at(
                        DiagnosticCode::LexError,
                        line_no,
                        1,
                        "inconsistent indentation",
                    ));
                }
            }
        }

        let mut emitted_on_line = false;
        while pos < chars.len() {
            let c = chars[pos];
            let col = pos + 1;

            if c == ' ' || c == '\t' {
                pos += 1;
                continue;
            }
            if c == '#' {
                break;
            }

            emitted_on_line = true;

            // Raw ASCII bytes literal: `b"..."` or `b'...'`
            if c == 'b' && matches!(chars.get(pos + 1), Some('"' | '\'')) {
                let (bs, next_pos) = scan_bytes(&chars, pos + 1, line_no)?;
                tokens.push(Token {
                    kind: TokenKind::BytesLit(bs),
                    line: line_no,
                    col,
                });
                pos = next_pos;
                continue;
            }

            // Unprefixed UTF-8 string literals are eliminated in pure-wave Dec (`b"..."` required)
            if c == '"' || c == '\'' {
                return Err(Diagnostic::at(
                    DiagnosticCode::LexError,
                    line_no,
                    col,
                    "UTF-8 string literals are forbidden in pure-wave Dec; use byte literals b\"...\"",
                ));
            }

            // Boundary prefix symbol ∂ (U+2202)
            if c == '∂' {
                tokens.push(Token {
                    kind: TokenKind::BoundaryOp,
                    line: line_no,
                    col,
                });
                pos += 1;
                continue;
            }

            // Leading-dot float rejection (e.g. `.5`), vs `..` range vs `.` field access
            if c == '.' {
                if pos + 1 < chars.len() && chars[pos + 1] == '.' {
                    tokens.push(Token {
                        kind: TokenKind::DotDot,
                        line: line_no,
                        col,
                    });
                    pos += 2;
                    continue;
                }
                if pos + 1 < chars.len() && chars[pos + 1].is_ascii_digit() {
                    return Err(Diagnostic::at(
                        DiagnosticCode::FloatLiteralForbidden,
                        line_no,
                        col,
                        "floating-point literals are forbidden in Dec; use exact ratio (e.g. 1/2r) or aod::qext::ext_decimal",
                    ));
                }
                tokens.push(Token {
                    kind: TokenKind::Dot,
                    line: line_no,
                    col,
                });
                pos += 1;
                continue;
            }

            // Numeric / Hex / Bin / RatioLit / Addr8Lit / SupportLit / TritLit
            if c.is_ascii_digit() {
                let (tok_kind, next_pos) = scan_number_like(&chars, pos, line_no)?;
                tokens.push(Token {
                    kind: tok_kind,
                    line: line_no,
                    col,
                });
                pos = next_pos;
                continue;
            }

            // Identifiers, keywords, and word-based adverbs (`min/`, `max/`, `superpose/`, `min\`, `max\`)
            if is_ident_start(c) {
                let start = pos;
                pos += 1;
                while pos < chars.len() && is_ident_cont(chars[pos]) {
                    pos += 1;
                }
                let word: String = chars[start..pos].iter().collect();
                match (word.as_str(), chars.get(pos).copied()) {
                    ("min", Some('/')) => {
                        pos += 1;
                        tokens.push(Token {
                            kind: TokenKind::Adverb(AdverbOp::FoldMin),
                            line: line_no,
                            col,
                        });
                        continue;
                    }
                    ("min", Some('\\')) => {
                        pos += 1;
                        tokens.push(Token {
                            kind: TokenKind::Adverb(AdverbOp::ScanMin),
                            line: line_no,
                            col,
                        });
                        continue;
                    }
                    ("max", Some('/')) => {
                        pos += 1;
                        tokens.push(Token {
                            kind: TokenKind::Adverb(AdverbOp::FoldMax),
                            line: line_no,
                            col,
                        });
                        continue;
                    }
                    ("max", Some('\\')) => {
                        pos += 1;
                        tokens.push(Token {
                            kind: TokenKind::Adverb(AdverbOp::ScanMax),
                            line: line_no,
                            col,
                        });
                        continue;
                    }
                    ("superpose", Some('/')) => {
                        pos += 1;
                        tokens.push(Token {
                            kind: TokenKind::Adverb(AdverbOp::FoldSuperpose),
                            line: line_no,
                            col,
                        });
                        continue;
                    }
                    _ => {}
                }
                let kind = map_keyword_or_reject(&word, line_no, col)?;
                tokens.push(Token {
                    kind,
                    line: line_no,
                    col,
                });
                continue;
            }

            // Hard-reject `{` and `}` in clean-break Dec!
            if c == '{' || c == '}' {
                return Err(Diagnostic::at(
                    DiagnosticCode::ParseError,
                    line_no,
                    col,
                    format!(
                        "brace delimiter {c:?} is removed; use indentation for blocks and 'map[(k, v)]' for maps"
                    ),
                ));
            }

            // Multi-char & single-char operators/delimiters
            let next = chars.get(pos + 1).copied();
            let next2 = chars.get(pos + 2).copied();
            let (kind, advance) = match (c, next, next2) {
                ('<', Some('='), Some('>')) => (TokenKind::Cmp3, 3),
                (':', Some(':'), _) => (TokenKind::ColonColon, 2),
                ('-', Some('>'), _) => (TokenKind::Arrow, 2),
                ('|', Some('>'), _) => (TokenKind::PipeGt, 2),
                ('|', Some('/'), _) => (TokenKind::Adverb(AdverbOp::FoldOr), 2),
                ('?', Some('>'), _) => (TokenKind::Sieve, 2),
                ('=', Some('='), _) => (TokenKind::EqEq, 2),
                ('!', Some('='), _) => (TokenKind::NotEq, 2),
                ('<', Some('='), _) => (TokenKind::LtEq, 2),
                ('>', Some('='), _) => (TokenKind::GtEq, 2),
                ('<', Some('<'), _) => (TokenKind::Shl, 2),
                ('>', Some('>'), _) => (TokenKind::Shr, 2),
                ('/', Some('/'), _) => (TokenKind::SlashSlash, 2),
                ('+', Some(':'), _) => (TokenKind::OuterAdd, 2),
                ('*', Some(':'), _) => (TokenKind::OuterMul, 2),
                ('+', Some('/'), _) => (TokenKind::Adverb(AdverbOp::FoldAdd), 2),
                ('*', Some('/'), _) => (TokenKind::Adverb(AdverbOp::FoldMul), 2),
                ('&', Some('/'), _) => (TokenKind::Adverb(AdverbOp::FoldAnd), 2),
                ('+', Some('\\'), _) => (TokenKind::Adverb(AdverbOp::ScanAdd), 2),
                ('*', Some('\\'), _) => (TokenKind::Adverb(AdverbOp::ScanMul), 2),
                ('(', _, _) => {
                    delim_depth += 1;
                    (TokenKind::LParen, 1)
                }
                (')', _, _) => {
                    if delim_depth == 0 {
                        return Err(Diagnostic::at(
                            DiagnosticCode::LexError,
                            line_no,
                            col,
                            "unmatched ')'",
                        ));
                    }
                    delim_depth -= 1;
                    (TokenKind::RParen, 1)
                }
                ('[', _, _) => {
                    delim_depth += 1;
                    (TokenKind::LBracket, 1)
                }
                (']', _, _) => {
                    if delim_depth == 0 {
                        return Err(Diagnostic::at(
                            DiagnosticCode::LexError,
                            line_no,
                            col,
                            "unmatched ']'",
                        ));
                    }
                    delim_depth -= 1;
                    (TokenKind::RBracket, 1)
                }
                (',', _, _) => (TokenKind::Comma, 1),
                (';', _, _) => (TokenKind::Semicolon, 1),
                (':', _, _) => (TokenKind::Colon, 1),
                ('=', _, _) => (TokenKind::Eq, 1),
                ('<', _, _) => (TokenKind::Lt, 1),
                ('>', _, _) => (TokenKind::Gt, 1),
                ('+', _, _) => (TokenKind::Plus, 1),
                ('-', _, _) => (TokenKind::Minus, 1),
                ('*', _, _) => (TokenKind::Star, 1),
                ('/', _, _) => (TokenKind::Slash, 1),
                ('%', _, _) => (TokenKind::Percent, 1),
                ('&', _, _) => (TokenKind::Amp, 1),
                ('|', _, _) => (TokenKind::Pipe, 1),
                ('^', _, _) => (TokenKind::Caret, 1),
                ('~', _, _) => (TokenKind::Tilde, 1),
                _ => {
                    return Err(Diagnostic::at(
                        DiagnosticCode::LexError,
                        line_no,
                        col,
                        format!("unexpected character {c:?}"),
                    ));
                }
            };
            tokens.push(Token {
                kind,
                line: line_no,
                col,
            });
            pos += advance;
        }

        if emitted_on_line && delim_depth == 0 {
            tokens.push(Token {
                kind: TokenKind::Newline,
                line: line_no,
                col: chars.len() + 1,
            });
        }
    }

    if delim_depth != 0 {
        return Err(Diagnostic::new(
            DiagnosticCode::LexError,
            "unclosed delimiter before end of file",
        ));
    }

    let last_line = raw_lines.len().max(1);
    while indent_stack.len() > 1 {
        indent_stack.pop();
        tokens.push(Token {
            kind: TokenKind::Dedent,
            line: last_line,
            col: 1,
        });
    }
    tokens.push(Token {
        kind: TokenKind::Eof,
        line: last_line,
        col: 1,
    });
    for (idx, tok) in tokens.iter().enumerate() {
        if matches!(&tok.kind, TokenKind::Ident(s) if s == "for") {
            let is_sim_for = idx >= 2
                && tokens[idx - 2].kind == TokenKind::Simulate
                && matches!(tokens[idx - 1].kind, TokenKind::Ident(_));
            if !is_sim_for {
                return Err(Diagnostic::at(
                    DiagnosticCode::ParseError,
                    tok.line,
                    tok.col,
                    "imperative loop keyword 'for' is removed; use rank lifting, '|>', 'walk', or 'settle'",
                ));
            }
        }
    }
    Ok(tokens)
}

fn scan_bytes(chars: &[char], quote_pos: usize, line: usize) -> DecResult<(Vec<u8>, usize)> {
    let quote = chars[quote_pos];
    let mut pos = quote_pos + 1;
    let mut out = Vec::new();
    while pos < chars.len() {
        let c = chars[pos];
        if c == quote {
            return Ok((out, pos + 1));
        }
        if c == '\\' {
            pos += 1;
            if pos >= chars.len() {
                return Err(Diagnostic::at(
                    DiagnosticCode::LexError,
                    line,
                    pos + 1,
                    "unterminated bytes escape",
                ));
            }
            match chars[pos] {
                'n' => {
                    out.push(b'\n');
                    pos += 1;
                }
                'r' => {
                    out.push(b'\r');
                    pos += 1;
                }
                't' => {
                    out.push(b'\t');
                    pos += 1;
                }
                '0' => {
                    out.push(0);
                    pos += 1;
                }
                '\\' => {
                    out.push(b'\\');
                    pos += 1;
                }
                '"' => {
                    out.push(b'"');
                    pos += 1;
                }
                '\'' => {
                    out.push(b'\'');
                    pos += 1;
                }
                'x' => {
                    if pos + 2 >= chars.len() {
                        return Err(Diagnostic::at(
                            DiagnosticCode::LexError,
                            line,
                            pos + 1,
                            "incomplete \\xHH escape in bytes literal",
                        ));
                    }
                    let hex: String = chars[pos + 1..pos + 3].iter().collect();
                    let val = u8::from_str_radix(&hex, 16).map_err(|_| {
                        Diagnostic::at(
                            DiagnosticCode::LexError,
                            line,
                            pos + 1,
                            format!("invalid hex escape \\x{hex} in bytes literal"),
                        )
                    })?;
                    out.push(val);
                    pos += 3;
                }
                'u' => {
                    return Err(Diagnostic::at(
                        DiagnosticCode::LexError,
                        line,
                        pos + 1,
                        "unicode escape \\u is not permitted in bytes literal; use \\xHH",
                    ));
                }
                other => {
                    return Err(Diagnostic::at(
                        DiagnosticCode::LexError,
                        line,
                        pos + 1,
                        format!("unsupported bytes escape \\{other}"),
                    ));
                }
            }
        } else if c.is_ascii() && (0x20..=0x7E).contains(&(c as u32)) {
            out.push(c as u8);
            pos += 1;
        } else {
            return Err(Diagnostic::at(
                DiagnosticCode::LexError,
                line,
                pos + 1,
                "bytes literal must contain printable ASCII (0x20..=0x7E) or \\xHH escapes",
            ));
        }
    }
    Err(Diagnostic::at(
        DiagnosticCode::LexError,
        line,
        quote_pos + 1,
        "unterminated bytes literal",
    ))
}

fn scan_radix_int(
    chars: &[char],
    start: usize,
    radix: u32,
    line: usize,
    col: usize,
) -> DecResult<(TokenKind, usize)> {
    let mut pos = start + 2;
    if pos >= chars.len() || chars[pos] == '_' {
        return Err(Diagnostic::at(
            DiagnosticCode::LexError,
            line,
            col,
            format!("invalid base-{radix} integer literal"),
        ));
    }
    let mut acc = BigInt::zero();
    let base = BigInt::from_u64(radix as u64);
    let mut saw_digit = false;
    let mut prev_underscore = false;

    while pos < chars.len() {
        let ch = chars[pos];
        if ch == '_' {
            if prev_underscore {
                return Err(Diagnostic::at(
                    DiagnosticCode::LexError,
                    line,
                    pos + 1,
                    "consecutive underscores in numeric literal",
                ));
            }
            prev_underscore = true;
            pos += 1;
            continue;
        }
        if let Some(d) = ch.to_digit(radix) {
            saw_digit = true;
            prev_underscore = false;
            acc = &(&acc * &base) + &BigInt::from_u64(d as u64);
            pos += 1;
        } else if ch == '.' && chars.get(pos + 1) != Some(&'.') {
            return Err(Diagnostic::at(
                DiagnosticCode::FloatLiteralForbidden,
                line,
                pos + 1,
                "floating-point literals are forbidden in Dec",
            ));
        } else if is_ident_cont(ch) {
            return Err(Diagnostic::at(
                DiagnosticCode::LexError,
                line,
                pos + 1,
                format!("invalid digit {ch:?} in base-{radix} literal"),
            ));
        } else {
            break;
        }
    }

    if !saw_digit || prev_underscore {
        return Err(Diagnostic::at(
            DiagnosticCode::LexError,
            line,
            col,
            format!("malformed base-{radix} integer literal"),
        ));
    }
    Ok((TokenKind::IntLit(acc), pos))
}

fn scan_number_like(chars: &[char], start: usize, line: usize) -> DecResult<(TokenKind, usize)> {
    let col = start + 1;

    // 1a. Hex integer `0x...` / `0X...` and Binary integer `0b...` / `0B...`
    if chars[start] == '0' {
        match chars.get(start + 1).copied() {
            Some('x' | 'X') => return scan_radix_int(chars, start, 16, line, col),
            Some('b' | 'B') => return scan_radix_int(chars, start, 2, line, col),
            Some('o') => {
                // 1b. Octal anchor 0o[0-9]+
                let mut pos = start + 2;
                while pos < chars.len() && chars[pos].is_ascii_digit() {
                    pos += 1;
                }
                let text: String = chars[start..pos].iter().collect();
                let addr = Addr8::parse(&text).map_err(|e| {
                    Diagnostic::at(DiagnosticCode::InvalidOctalAnchor, line, col, e.to_string())
                })?;
                return Ok((TokenKind::Addr8Lit(addr), pos));
            }
            _ => {}
        }
    }

    // Scan leading digits (allowing internal underscores only when not followed by _8/_H/_D)
    let mut pos = start;
    while pos < chars.len() {
        if chars[pos].is_ascii_digit() {
            pos += 1;
        } else if chars[pos] == '_' {
            let next = chars.get(pos + 1).copied();
            let after_next = chars.get(pos + 2).copied();
            if matches!(next, Some('8' | 'H' | 'D')) && !after_next.is_some_and(is_ident_cont) {
                break;
            }
            if next.is_some_and(|ch| ch.is_ascii_digit()) {
                pos += 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    let first_seg: String = chars[start..pos].iter().collect();

    // 2. Check Addr8Lit suffixes: `|-` or `_8`
    if chars.get(pos) == Some(&'|') && chars.get(pos + 1) == Some(&'-') {
        let end = pos + 2;
        let text: String = chars[start..end].iter().collect();
        let addr = Addr8::parse(&text)
            .map_err(|e| Diagnostic::at(DiagnosticCode::InvalidOctalAnchor, line, col, e.to_string()))?;
        return Ok((TokenKind::Addr8Lit(addr), end));
    }
    if chars.get(pos) == Some(&'_')
        && chars.get(pos + 1) == Some(&'8')
        && !chars.get(pos + 2).copied().is_some_and(is_ident_cont)
    {
        let end = pos + 2;
        let text: String = chars[start..end].iter().collect();
        let addr = Addr8::parse(&text)
            .map_err(|e| Diagnostic::at(DiagnosticCode::InvalidOctalAnchor, line, col, e.to_string()))?;
        return Ok((TokenKind::Addr8Lit(addr), end));
    }

    // 3. Check TritLit: `0_H`, `0_D`, and compact balanced trits `0t`/`1t` (with unary +/- for +1t/-1t)
    if first_seg == "0"
        && chars.get(pos) == Some(&'_')
        && !chars.get(pos + 2).copied().is_some_and(is_ident_cont)
    {
        if chars.get(pos + 1) == Some(&'H') {
            return Ok((TokenKind::TritLit(TritLiteral::Hinge0H), pos + 2));
        }
        if chars.get(pos + 1) == Some(&'D') {
            return Ok((TokenKind::TritLit(TritLiteral::Dwell0D), pos + 2));
        }
    }
    if chars.get(pos) == Some(&'t') && !chars.get(pos + 1).copied().is_some_and(is_ident_cont) {
        return match first_seg.as_str() {
            "0" => Ok((TokenKind::TritLit(TritLiteral::Zero), pos + 1)),
            "1" => Ok((TokenKind::TritLit(TritLiteral::PosOne), pos + 1)),
            _ => Err(Diagnostic::at(
                DiagnosticCode::InvalidTritLiteral,
                line,
                col,
                format!("invalid balanced trit literal {first_seg}t; expected 0t, +1t, or -1t"),
            )),
        };
    }

    // 4. Check SupportLit: `[0-9]+:[0-9]+:[0-9]+(_yro)?` (no spaces around `:`)
    if chars.get(pos) == Some(&':') && chars.get(pos + 1).is_some_and(|ch| ch.is_ascii_digit()) {
        let mut p2 = pos + 1;
        while p2 < chars.len() && chars[p2].is_ascii_digit() {
            p2 += 1;
        }
        if chars.get(p2) == Some(&':') && chars.get(p2 + 1).is_some_and(|ch| ch.is_ascii_digit()) {
            let mut p3 = p2 + 1;
            while p3 < chars.len() && chars[p3].is_ascii_digit() {
                p3 += 1;
            }
            if chars[p3..].starts_with(&['_', 'y', 'r', 'o'])
                && !chars.get(p3 + 4).copied().is_some_and(is_ident_cont)
            {
                p3 += 4;
            }
            let text: String = chars[start..p3].iter().collect();
            let sf = SupportForm::parse(&text).map_err(|e| match e {
                CoreError::OddSupportRequiresYro(m) => {
                    Diagnostic::at(DiagnosticCode::OddSupportRequiresYro, line, col, m)
                }
                CoreError::EvenSupportForbidsYro(m) => {
                    Diagnostic::at(DiagnosticCode::EvenSupportForbidsYro, line, col, m)
                }
                other => {
                    Diagnostic::at(DiagnosticCode::InvalidSupportLiteral, line, col, other.to_string())
                }
            })?;
            return Ok((TokenKind::SupportLit(sf), p3));
        }
    }

    // 5. Check RatioLit: `[0-9][0-9_]*/[0-9][0-9_]*r` (no spaces around `/`)
    if chars.get(pos) == Some(&'/') && chars.get(pos + 1).is_some_and(|ch| ch.is_ascii_digit()) {
        let mut dpos = pos + 1;
        while dpos < chars.len() && (chars[dpos].is_ascii_digit() || chars[dpos] == '_') {
            dpos += 1;
        }
        if chars.get(dpos) == Some(&'r') && !chars.get(dpos + 1).copied().is_some_and(is_ident_cont)
        {
            let den_str: String = chars[pos + 1..dpos].iter().collect();
            let num = BigInt::parse_decimal_underscored(&first_seg)
                .map_err(|e| Diagnostic::at(DiagnosticCode::LexError, line, col, e.to_string()))?;
            let den = BigInt::parse_decimal_underscored(&den_str)
                .map_err(|e| Diagnostic::at(DiagnosticCode::LexError, line, col, e.to_string()))?;
            let ratio = Ratio::new(num, den).map_err(|e| match e {
                CoreError::ZeroDivision => Diagnostic::at(
                    DiagnosticCode::ZeroDenominator,
                    line,
                    col,
                    "ratio literal has zero denominator",
                ),
                other => Diagnostic::at(DiagnosticCode::LexError, line, col, other.to_string()),
            })?;
            return Ok((TokenKind::RatioLit(ratio), dpos + 1));
        }
    }

    // 6. Float rejection vs `..` range
    if chars.get(pos) == Some(&'.') && chars.get(pos + 1) != Some(&'.') {
        return Err(Diagnostic::at(
            DiagnosticCode::FloatLiteralForbidden,
            line,
            col,
            "floating-point literals are forbidden in Dec; use exact ratio (e.g. 1/2r) or aod::qext::ext_decimal",
        ));
    }
    if matches!(chars.get(pos), Some('e' | 'E')) {
        let next = chars.get(pos + 1).copied();
        let after = chars.get(pos + 2).copied();
        let is_sci = next.is_some_and(|ch| ch.is_ascii_digit())
            || (matches!(next, Some('+' | '-')) && after.is_some_and(|ch| ch.is_ascii_digit()));
        if is_sci {
            return Err(Diagnostic::at(
                DiagnosticCode::FloatLiteralForbidden,
                line,
                col,
                "scientific-notation float literals are forbidden in Dec",
            ));
        }
    }

    // Reject trailing unknown alpha suffix attached directly to digits (e.g. `123abc`)
    if chars.get(pos).copied().is_some_and(is_ident_cont) {
        return Err(Diagnostic::at(
            DiagnosticCode::LexError,
            line,
            pos + 1,
            format!("unexpected suffix {:?} on integer literal", chars[pos]),
        ));
    }

    // Plain exact integer
    let val = BigInt::parse_decimal_underscored(&first_seg)
        .map_err(|e| Diagnostic::at(DiagnosticCode::LexError, line, col, e.to_string()))?;
    Ok((TokenKind::IntLit(val), pos))
}
