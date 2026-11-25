use aod_core::bigint::BigInt;
use aod_core::error::CoreError;
use aod_core::ratio::Ratio;
use aod_core::support::SupportForm;
use aod_core::ternary::Addr8;

use crate::ast::TritLiteral;
use crate::diagnostics::{CompatMode, DecResult, Diagnostic, DiagnosticCode, SyntaxDiscipline};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenKind {
    // Layout
    Indent,
    Dedent,
    Newline,
    Eof,

    // Hard keywords (contextual words like window/shedding/base/next/region/capture/clock/etc. remain Ident!)
    Dec,
    DecLower,
    Let,
    Mut,
    Const,
    Return,
    Emit,
    Assert,
    If,
    Elif,
    Else,
    For,
    In,
    While,
    Break,
    Continue,
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
    Null,
    Where,

    // Identifiers & literals
    Ident(String),
    IntLit(BigInt),
    StrLit(String),
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
    LBrace,
    RBrace,
    Comma,
    Colon,
    ColonColon,
    Dot,
    DotDot,
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
    Amp,
    Pipe,
    Caret,
    Shl,
    Shr,
    BoundaryOp,
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

fn map_keyword(s: &str, compat: CompatMode) -> TokenKind {
    match s {
        "Dec" => TokenKind::Dec,
        "dec" if compat == CompatMode::V2 => TokenKind::DecLower,
        "where" if compat == CompatMode::V2 => TokenKind::Where,
        "let" => TokenKind::Let,
        "mut" => TokenKind::Mut,
        "const" => TokenKind::Const,
        "return" => TokenKind::Return,
        "emit" => TokenKind::Emit,
        "assert" => TokenKind::Assert,
        "if" => TokenKind::If,
        "elif" => TokenKind::Elif,
        "else" => TokenKind::Else,
        "for" => TokenKind::For,
        "in" => TokenKind::In,
        "while" => TokenKind::While,
        "break" => TokenKind::Break,
        "continue" => TokenKind::Continue,
        "and" => TokenKind::And,
        "or" => TokenKind::Or,
        "not" => TokenKind::Not,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "use" => TokenKind::Use,
        "chamber" => TokenKind::Chamber,
        "prepare" => TokenKind::Prepare,
        "observe" => TokenKind::Observe,
        "simulate" => TokenKind::Simulate,
        "quarantine" => TokenKind::Quarantine,
        "pass" => TokenKind::Pass,
        "struct" => TokenKind::Struct,
        "enum" => TokenKind::Enum,
        "match" => TokenKind::Match,
        "null" if compat == CompatMode::V2 => TokenKind::Null,
        _ => TokenKind::Ident(s.to_string()),
    }
}

/// Tokenize Dec v2 source code (convenience wrapper over `tokenize_with_compat`).
pub fn tokenize(source: &str) -> DecResult<Vec<Token>> {
    tokenize_with_compat(source, CompatMode::V2)
}

/// Tokenize Dec v1 / v2 source code under canonical `SyntaxDiscipline::for_compat(compat)`.
pub fn tokenize_with_compat(source: &str, compat: CompatMode) -> DecResult<Vec<Token>> {
    tokenize_with_discipline(source, compat, SyntaxDiscipline::for_compat(compat))
}

pub fn tokenize_with_discipline(
    source: &str,
    compat: CompatMode,
    discipline: SyntaxDiscipline,
) -> DecResult<Vec<Token>> {
    tokenize_with_options(source, compat, discipline)
}

/// Tokenize Dec v1 / v2 source code with explicit `SyntaxDiscipline`:
/// - Aggregate delimiter depth line joining (`[(0, 6], (4, 8]]` works cleanly, `P1-1`).
/// - Strict float rejection vs `..` range (`24..40` lexes cleanly; `1.5`, `5.`, `.5`, `1e3` reject).
/// - Dedicated exact literals: `RatioLit` (`1/3r`), `Addr8Lit` (`00_8`, `00|-`, `0o00`),
///   `SupportLit` (`3:3:6`, `1:2:9_yro`), `TritLit` (`0_H`, `0_D`), `BytesLit` (`b"..."`),
///   `0x`/`0b` integer literals (v2 only), `null` (v2 only), prefix `∂`.
/// - Under `SyntaxDiscipline::PureWaveV2`: rejects `{`/`}`, `"..."`/`'...'`, and uppercase `Dec`.
pub fn tokenize_with_options(
    source: &str,
    compat: CompatMode,
    discipline: SyntaxDiscipline,
) -> DecResult<Vec<Token>> {
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

            // String literals
            if c == '"' || c == '\'' {
                if discipline == SyntaxDiscipline::PureWaveV2 {
                    return Err(Diagnostic::at(
                        DiagnosticCode::LexError,
                        line_no,
                        col,
                        "UTF-8 string literals are forbidden in --strict-wave; use byte literals b\"...\"",
                    ));
                }
                let (s, next_pos) = scan_string(&chars, pos, line_no)?;
                tokens.push(Token {
                    kind: TokenKind::StrLit(s),
                    line: line_no,
                    col,
                });
                pos = next_pos;
                continue;
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
                        "floating-point literals are forbidden in Dec; use exact ratio or aod::qext::ext_decimal",
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

            // Byte string literals `b"..."` / `b'...'` (v2 only)
            if c == 'b' && matches!(chars.get(pos + 1), Some('"' | '\'')) {
                if compat == CompatMode::V1 {
                    return Err(Diagnostic::at(
                        DiagnosticCode::LexError,
                        line_no,
                        col,
                        "byte literals (b\"...\") are not supported in --compat v1",
                    ));
                }
                let (bs, next_pos) = scan_bytes(&chars, pos + 1, line_no)?;
                tokens.push(Token {
                    kind: TokenKind::BytesLit(bs),
                    line: line_no,
                    col,
                });
                pos = next_pos;
                continue;
            }

            // Numeric / RatioLit / Addr8Lit / SupportLit / TritLit
            if c.is_ascii_digit() {
                let (tok_kind, next_pos) = scan_number_like(&chars, pos, line_no, compat)?;
                tokens.push(Token {
                    kind: tok_kind,
                    line: line_no,
                    col,
                });
                pos = next_pos;
                continue;
            }

            // Identifiers & keywords
            if is_ident_start(c) {
                let start = pos;
                pos += 1;
                while pos < chars.len() && is_ident_cont(chars[pos]) {
                    pos += 1;
                }
                let word: String = chars[start..pos].iter().collect();
                tokens.push(Token {
                    kind: map_keyword(&word, compat),
                    line: line_no,
                    col,
                });
                continue;
            }

            // Multi-char & single-char operators/delimiters
            let next = chars.get(pos + 1).copied();
            let (kind, advance) = match (c, next) {
                (':', Some(':')) => (TokenKind::ColonColon, 2),
                ('-', Some('>')) => (TokenKind::Arrow, 2),
                ('=', Some('=')) => (TokenKind::EqEq, 2),
                ('!', Some('=')) => (TokenKind::NotEq, 2),
                ('<', Some('=')) => (TokenKind::LtEq, 2),
                ('>', Some('=')) => (TokenKind::GtEq, 2),
                ('<', Some('<')) => (TokenKind::Shl, 2),
                ('>', Some('>')) => (TokenKind::Shr, 2),
                ('/', Some('/')) => (TokenKind::SlashSlash, 2),
                ('(', _) => {
                    delim_depth += 1;
                    (TokenKind::LParen, 1)
                }
                (')', _) => {
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
                ('[', _) => {
                    delim_depth += 1;
                    (TokenKind::LBracket, 1)
                }
                (']', _) => {
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
                ('{', _) => {
                    if discipline == SyntaxDiscipline::PureWaveV2 {
                        return Err(Diagnostic::at(
                            DiagnosticCode::LexError,
                            line_no,
                            col,
                            "braces `{` / `}` are forbidden in --strict-wave; use indented blocks, positional constructors, or `map[...]`",
                        ));
                    }
                    delim_depth += 1;
                    (TokenKind::LBrace, 1)
                }
                ('}', _) => {
                    if discipline == SyntaxDiscipline::PureWaveV2 {
                        return Err(Diagnostic::at(
                            DiagnosticCode::LexError,
                            line_no,
                            col,
                            "braces `{` / `}` are forbidden in --strict-wave; use indented blocks, positional constructors, or `map[...]`",
                        ));
                    }
                    if delim_depth == 0 {
                        return Err(Diagnostic::at(
                            DiagnosticCode::LexError,
                            line_no,
                            col,
                            "unmatched '}'",
                        ));
                    }
                    delim_depth -= 1;
                    (TokenKind::RBrace, 1)
                }
                (',', _) => (TokenKind::Comma, 1),
                (':', _) => (TokenKind::Colon, 1),
                ('=', _) => (TokenKind::Eq, 1),
                ('<', _) => (TokenKind::Lt, 1),
                ('>', _) => (TokenKind::Gt, 1),
                ('+', _) => (TokenKind::Plus, 1),
                ('-', _) => (TokenKind::Minus, 1),
                ('*', _) => (TokenKind::Star, 1),
                ('/', _) => (TokenKind::Slash, 1),
                ('%', _) => (TokenKind::Percent, 1),
                ('&', _) => (TokenKind::Amp, 1),
                ('|', _) => (TokenKind::Pipe, 1),
                ('^', _) => (TokenKind::Caret, 1),
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
    Ok(tokens)
}

fn scan_string(chars: &[char], start: usize, line: usize) -> DecResult<(String, usize)> {
    let quote = chars[start];
    let mut pos = start + 1;
    let mut out = String::new();
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
                    "unterminated string escape",
                ));
            }
            match chars[pos] {
                'n' => {
                    out.push('\n');
                    pos += 1;
                }
                'r' => {
                    out.push('\r');
                    pos += 1;
                }
                't' => {
                    out.push('\t');
                    pos += 1;
                }
                '0' => {
                    out.push('\0');
                    pos += 1;
                }
                '\\' => {
                    out.push('\\');
                    pos += 1;
                }
                '"' => {
                    out.push('"');
                    pos += 1;
                }
                '\'' => {
                    out.push('\'');
                    pos += 1;
                }
                'x' => {
                    if pos + 2 >= chars.len() {
                        return Err(Diagnostic::at(
                            DiagnosticCode::LexError,
                            line,
                            pos + 1,
                            "incomplete \\xHH escape",
                        ));
                    }
                    let hex: String = chars[pos + 1..pos + 3].iter().collect();
                    let val = u8::from_str_radix(&hex, 16).map_err(|_| {
                        Diagnostic::at(
                            DiagnosticCode::LexError,
                            line,
                            pos + 1,
                            format!("invalid hex escape \\x{hex}"),
                        )
                    })?;
                    out.push(val as char);
                    pos += 3;
                }
                'u' => {
                    if pos + 4 >= chars.len() {
                        return Err(Diagnostic::at(
                            DiagnosticCode::LexError,
                            line,
                            pos + 1,
                            "incomplete \\uXXXX escape",
                        ));
                    }
                    let hex: String = chars[pos + 1..pos + 5].iter().collect();
                    let val = u32::from_str_radix(&hex, 16).map_err(|_| {
                        Diagnostic::at(
                            DiagnosticCode::LexError,
                            line,
                            pos + 1,
                            format!("invalid unicode escape \\u{hex}"),
                        )
                    })?;
                    let ch = char::from_u32(val).ok_or_else(|| {
                        Diagnostic::at(
                            DiagnosticCode::LexError,
                            line,
                            pos + 1,
                            "invalid unicode scalar value",
                        )
                    })?;
                    out.push(ch);
                    pos += 5;
                }
                other => {
                    return Err(Diagnostic::at(
                        DiagnosticCode::LexError,
                        line,
                        pos + 1,
                        format!("unsupported string escape \\{other}"),
                    ));
                }
            }
        } else {
            out.push(c);
            pos += 1;
        }
    }
    Err(Diagnostic::at(
        DiagnosticCode::LexError,
        line,
        start + 1,
        "unterminated string literal",
    ))
}

fn scan_bytes(chars: &[char], start: usize, line: usize) -> DecResult<(Vec<u8>, usize)> {
    let quote = chars[start];
    let mut pos = start + 1;
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
                    "unterminated byte literal escape",
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
                            "incomplete \\xHH escape in byte literal",
                        ));
                    }
                    let hex: String = chars[pos + 1..pos + 3].iter().collect();
                    let val = u8::from_str_radix(&hex, 16).map_err(|_| {
                        Diagnostic::at(
                            DiagnosticCode::LexError,
                            line,
                            pos + 1,
                            format!("invalid hex escape \\x{hex} in byte literal"),
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
                        "\\uXXXX unicode escapes are forbidden in byte literals; use \\xHH",
                    ));
                }
                other => {
                    return Err(Diagnostic::at(
                        DiagnosticCode::LexError,
                        line,
                        pos + 1,
                        format!("unsupported byte literal escape \\{other}"),
                    ));
                }
            }
        } else if (0x20..=0x7E).contains(&(c as u32)) {
            out.push(c as u8);
            pos += 1;
        } else {
            return Err(Diagnostic::at(
                DiagnosticCode::LexError,
                line,
                pos + 1,
                "byte literals only permit ASCII 0x20..=0x7E or explicit escapes",
            ));
        }
    }
    Err(Diagnostic::at(
        DiagnosticCode::LexError,
        line,
        start,
        "unterminated byte literal",
    ))
}

fn scan_number_like(
    chars: &[char],
    start: usize,
    line: usize,
    compat: CompatMode,
) -> DecResult<(TokenKind, usize)> {
    let col = start + 1;

    // 1. Octal anchor 0o[0-9]+
    if chars[start] == '0' && chars.get(start + 1) == Some(&'o') {
        let mut pos = start + 2;
        while pos < chars.len() && chars[pos].is_ascii_digit() {
            pos += 1;
        }
        let text: String = chars[start..pos].iter().collect();
        let addr = Addr8::parse(&text)
            .map_err(|e| Diagnostic::at(DiagnosticCode::InvalidOctalAnchor, line, col, e.to_string()))?;
        return Ok((TokenKind::Addr8Lit(addr), pos));
    }

    // 1b. Hexadecimal `0[xX]...` and binary `0[bB]...` integer literals (Dec v2 only)
    if chars[start] == '0' && matches!(chars.get(start + 1), Some('x' | 'X' | 'b' | 'B')) {
        if compat == CompatMode::V1 {
            return Err(Diagnostic::at(
                DiagnosticCode::LexError,
                line,
                col,
                "hexadecimal and binary integer literals are not supported in --compat v1",
            ));
        }
        let is_hex = matches!(chars[start + 1], 'x' | 'X');
        let mut pos = start + 2;
        while pos < chars.len() && (chars[pos].is_ascii_alphanumeric() || chars[pos] == '_') {
            pos += 1;
        }
        if chars.get(pos) == Some(&'.') && chars.get(pos + 1) != Some(&'.') {
            return Err(Diagnostic::at(
                DiagnosticCode::FloatLiteralForbidden,
                line,
                col,
                "floating-point literals are forbidden in Dec",
            ));
        }
        let digits: String = chars[start + 2..pos].iter().collect();
        let val = if is_hex {
            BigInt::parse_hex_underscored(&digits)
        } else {
            BigInt::parse_binary_underscored(&digits)
        }
        .map_err(|e| Diagnostic::at(DiagnosticCode::LexError, line, col, e.to_string()))?;
        return Ok((TokenKind::IntLit(val), pos));
    }

    // Scan leading digits (allowing internal underscores only when not followed by _8/_H/_D)
    let mut pos = start;
    while pos < chars.len() {
        if chars[pos].is_ascii_digit() {
            pos += 1;
        } else if chars[pos] == '_' {
            // Stop before _8, _H, _D suffixes!
            let next = chars.get(pos + 1).copied();
            let after_next = chars.get(pos + 2).copied();
            if matches!(next, Some('8' | 'H' | 'D')) && !after_next.is_some_and(is_ident_cont) {
                break;
            }
            // Also check trailing underscore followed by digit
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
            "1" => Ok((TokenKind::TritLit(TritLiteral::One), pos + 1)),
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

    // 6. Float rejection vs `..` range (`P1-1`)
    if chars.get(pos) == Some(&'.') {
        if chars.get(pos + 1) != Some(&'.') {
            return Err(Diagnostic::at(
                DiagnosticCode::FloatLiteralForbidden,
                line,
                col,
                "floating-point literals are forbidden in Dec; use exact ratio (e.g. 1/2r) or aod::qext::ext_decimal",
            ));
        }
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

    // Plain exact integer
    let val = BigInt::parse_decimal_underscored(&first_seg)
        .map_err(|e| Diagnostic::at(DiagnosticCode::LexError, line, col, e.to_string()))?;
    Ok((TokenKind::IntLit(val), pos))
}
