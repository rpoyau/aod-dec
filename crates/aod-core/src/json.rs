use std::collections::BTreeMap;
use std::fmt::Write;

use crate::bigint::BigInt;
use crate::error::{CoreError, CoreResult};
use crate::sha256::Sha256;

const MAX_JSON_DEPTH: usize = 128;

/// Strict exact JSON AST: NO floating-point numbers, sorted object keys (`BTreeMap`),
/// lossless arbitrary-precision integers (`BigInt`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Int(BigInt),
    Str(String),
    Array(Vec<JsonValue>),
    Object(BTreeMap<String, JsonValue>),
}

impl JsonValue {
    pub fn from_i64(v: i64) -> Self {
        JsonValue::Int(BigInt::from_i64(v))
    }

    pub fn from_usize(v: usize) -> Self {
        JsonValue::Int(BigInt::from_u64(v as u64))
    }

    pub fn from_str_val(s: impl Into<String>) -> Self {
        JsonValue::Str(s.into())
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<&BigInt> {
        match self {
            JsonValue::Int(i) => Some(i),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            JsonValue::Array(a) => Some(a.as_slice()),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&BTreeMap<String, JsonValue>> {
        match self {
            JsonValue::Object(m) => Some(m),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        self.as_object().and_then(|m| m.get(key))
    }

    /// Canonical compact ASCII JSON matching Python:
    /// `json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)`
    /// byte-for-byte, including UTF-16 surrogate pairs for Unicode scalar values `> U+FFFF`.
    pub fn to_compact_ascii_json(&self) -> String {
        let mut out = String::new();
        self.write_compact_ascii(&mut out);
        out
    }

    /// Pretty sorted-key UTF-8 JSON matching Python:
    /// `json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n"`.
    pub fn to_pretty_utf8_json(&self) -> String {
        let mut out = String::new();
        self.write_pretty_utf8(&mut out, 0);
        out.push('\n');
        out
    }

    /// SHA-256 hex digest of `self.to_compact_ascii_json().as_bytes()`.
    pub fn sha256_hex(&self) -> String {
        Sha256::digest_hex(self.to_compact_ascii_json().as_bytes())
    }

    fn write_compact_ascii(&self, out: &mut String) {
        match self {
            JsonValue::Null => out.push_str("null"),
            JsonValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            JsonValue::Int(n) => out.push_str(&n.to_decimal_string()),
            JsonValue::Str(s) => write_json_str(s, true, out),
            JsonValue::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write_compact_ascii(out);
                }
                out.push(']');
            }
            JsonValue::Object(map) => {
                out.push('{');
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_json_str(k, true, out);
                    out.push(':');
                    v.write_compact_ascii(out);
                }
                out.push('}');
            }
        }
    }

    fn write_pretty_utf8(&self, out: &mut String, indent: usize) {
        match self {
            JsonValue::Null => out.push_str("null"),
            JsonValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            JsonValue::Int(n) => out.push_str(&n.to_decimal_string()),
            JsonValue::Str(s) => write_json_str(s, false, out),
            JsonValue::Array(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push_str("[\n");
                let child_indent = indent + 2;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(",\n");
                    }
                    for _ in 0..child_indent {
                        out.push(' ');
                    }
                    item.write_pretty_utf8(out, child_indent);
                }
                out.push('\n');
                for _ in 0..indent {
                    out.push(' ');
                }
                out.push(']');
            }
            JsonValue::Object(map) => {
                if map.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push_str("{\n");
                let child_indent = indent + 2;
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        out.push_str(",\n");
                    }
                    for _ in 0..child_indent {
                        out.push(' ');
                    }
                    write_json_str(k, false, out);
                    out.push_str(": ");
                    v.write_pretty_utf8(out, child_indent);
                }
                out.push('\n');
                for _ in 0..indent {
                    out.push(' ');
                }
                out.push('}');
            }
        }
    }
}

fn write_json_str(s: &str, ensure_ascii: bool, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c if ensure_ascii && (c as u32) >= 0x7f => {
                let cp = c as u32;
                if cp <= 0xffff {
                    let _ = write!(out, "\\u{cp:04x}");
                } else {
                    let v = cp - 0x10000;
                    let hi = 0xd800 + (v >> 10);
                    let lo = 0xdc00 + (v & 0x3ff);
                    let _ = write!(out, "\\u{hi:04x}\\u{lo:04x}");
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Strictly parse JSON rejecting duplicate object keys and floating-point literals.
pub fn parse_strict_json(input: &str) -> CoreResult<JsonValue> {
    let mut parser = JsonParser {
        bytes: input.as_bytes(),
        pos: 0,
        depth: 0,
    };
    parser.skip_ws();
    let value = parser.parse_value()?;
    parser.skip_ws();
    if parser.pos != parser.bytes.len() {
        return Err(CoreError::InvalidJson(format!(
            "trailing characters at byte offset {}",
            parser.pos
        )));
    }
    Ok(value)
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
}

impl<'a> JsonParser<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if matches!(b, b' ' | b'\t' | b'\r' | b'\n') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn parse_value(&mut self) -> CoreResult<JsonValue> {
        self.skip_ws();
        let b = self
            .peek()
            .ok_or_else(|| CoreError::InvalidJson("unexpected end of input".to_string()))?;
        match b {
            b'n' => self.parse_null(),
            b't' | b'f' => self.parse_bool(),
            b'"' => Ok(JsonValue::Str(self.parse_string()?)),
            b'[' => self.parse_array(),
            b'{' => self.parse_object(),
            b'-' | b'0'..=b'9' => self.parse_int(),
            other => Err(CoreError::InvalidJson(format!(
                "unexpected byte {:?} at offset {}",
                other as char, self.pos
            ))),
        }
    }

    fn parse_null(&mut self) -> CoreResult<JsonValue> {
        if self.bytes[self.pos..].starts_with(b"null") {
            self.pos += 4;
            Ok(JsonValue::Null)
        } else {
            Err(CoreError::InvalidJson(format!(
                "invalid literal at offset {}",
                self.pos
            )))
        }
    }

    fn parse_bool(&mut self) -> CoreResult<JsonValue> {
        if self.bytes[self.pos..].starts_with(b"true") {
            self.pos += 4;
            Ok(JsonValue::Bool(true))
        } else if self.bytes[self.pos..].starts_with(b"false") {
            self.pos += 5;
            Ok(JsonValue::Bool(false))
        } else {
            Err(CoreError::InvalidJson(format!(
                "invalid boolean literal at offset {}",
                self.pos
            )))
        }
    }

    fn parse_int(&mut self) -> CoreResult<JsonValue> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let digit_start = self.pos;
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(CoreError::InvalidJson(format!(
                        "leading zero in number at offset {start}"
                    )));
                }
            }
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            _ => {
                return Err(CoreError::InvalidJson(format!(
                    "expected digit at offset {digit_start}"
                )));
            }
        }
        if matches!(self.peek(), Some(b'.' | b'e' | b'E')) {
            return Err(CoreError::FloatForbidden(format!(
                "floating-point JSON number at offset {start}"
            )));
        }
        let s = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| CoreError::InvalidJson("invalid UTF-8 in integer".to_string()))?;
        let n = BigInt::parse_decimal(s)?;
        Ok(JsonValue::Int(n))
    }

    fn parse_string(&mut self) -> CoreResult<String> {
        if self.peek() != Some(b'"') {
            return Err(CoreError::InvalidJson("expected '\"'".to_string()));
        }
        self.pos += 1;
        let mut out = String::new();
        while let Some(b) = self.peek() {
            match b {
                b'"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.pos += 1;
                    let esc = self.peek().ok_or_else(|| {
                        CoreError::InvalidJson("unterminated escape sequence".to_string())
                    })?;
                    self.pos += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\x08'),
                        b'f' => out.push('\x0c'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let code1 = self.parse_hex4()?;
                            if (0xd800..=0xdbff).contains(&code1) {
                                // Must be followed by low surrogate \uXXXX
                                if !self.bytes[self.pos..].starts_with(b"\\u") {
                                    return Err(CoreError::InvalidJson(
                                        "unpaired high surrogate in JSON string".to_string(),
                                    ));
                                }
                                self.pos += 2;
                                let code2 = self.parse_hex4()?;
                                if !(0xdc00..=0xdfff).contains(&code2) {
                                    return Err(CoreError::InvalidJson(
                                        "invalid low surrogate in JSON string".to_string(),
                                    ));
                                }
                                let cp = 0x10000
                                    + (((code1 as u32 - 0xd800) << 10) | (code2 as u32 - 0xdc00));
                                let ch = char::from_u32(cp).ok_or_else(|| {
                                    CoreError::InvalidJson("invalid surrogate scalar".to_string())
                                })?;
                                out.push(ch);
                            } else if (0xdc00..=0xdfff).contains(&code1) {
                                return Err(CoreError::InvalidJson(
                                    "unpaired low surrogate in JSON string".to_string(),
                                ));
                            } else {
                                let ch = char::from_u32(code1 as u32).ok_or_else(|| {
                                    CoreError::InvalidJson("invalid unicode escape".to_string())
                                })?;
                                out.push(ch);
                            }
                        }
                        other => {
                            return Err(CoreError::InvalidJson(format!(
                                "invalid escape \\{}",
                                other as char
                            )));
                        }
                    }
                }
                c if c < 0x20 => {
                    return Err(CoreError::InvalidJson(format!(
                        "unescaped control byte 0x{c:02x} in string"
                    )));
                }
                _ => {
                    let rest = std::str::from_utf8(&self.bytes[self.pos..]).map_err(|_| {
                        CoreError::InvalidJson("invalid UTF-8 in JSON string".to_string())
                    })?;
                    let ch = rest.chars().next().ok_or_else(|| {
                        CoreError::InvalidJson("unterminated string".to_string())
                    })?;
                    out.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
        Err(CoreError::InvalidJson(
            "unterminated JSON string".to_string(),
        ))
    }

    fn parse_hex4(&mut self) -> CoreResult<u16> {
        if self.pos + 4 > self.bytes.len() {
            return Err(CoreError::InvalidJson(
                "incomplete \\uXXXX escape".to_string(),
            ));
        }
        let hex_str = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4])
            .map_err(|_| CoreError::InvalidJson("invalid \\uXXXX escape".to_string()))?;
        let val = u16::from_str_radix(hex_str, 16)
            .map_err(|_| CoreError::InvalidJson(format!("invalid hex escape \\u{hex_str}")))?;
        self.pos += 4;
        Ok(val)
    }

    fn parse_array(&mut self) -> CoreResult<JsonValue> {
        self.depth += 1;
        if self.depth > MAX_JSON_DEPTH {
            return Err(CoreError::InvalidJson(
                "JSON nesting depth limit exceeded".to_string(),
            ));
        }
        self.pos += 1; // '['
        self.skip_ws();
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(JsonValue::Array(items));
        }
        loop {
            items.push(self.parse_value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b']') => {
                    self.pos += 1;
                    self.depth -= 1;
                    return Ok(JsonValue::Array(items));
                }
                _ => {
                    return Err(CoreError::InvalidJson(format!(
                        "expected ',' or ']' at offset {}",
                        self.pos
                    )));
                }
            }
        }
    }

    fn parse_object(&mut self) -> CoreResult<JsonValue> {
        self.depth += 1;
        if self.depth > MAX_JSON_DEPTH {
            return Err(CoreError::InvalidJson(
                "JSON nesting depth limit exceeded".to_string(),
            ));
        }
        self.pos += 1; // '{'
        self.skip_ws();
        let mut map = BTreeMap::new();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(JsonValue::Object(map));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(CoreError::InvalidJson(format!(
                    "expected object key string at offset {}",
                    self.pos
                )));
            }
            let key = self.parse_string()?;
            if map.contains_key(&key) {
                return Err(CoreError::DuplicateJsonKey(key));
            }
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(CoreError::InvalidJson(format!(
                    "expected ':' after key at offset {}",
                    self.pos
                )));
            }
            self.pos += 1;
            let val = self.parse_value()?;
            map.insert(key, val);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b'}') => {
                    self.pos += 1;
                    self.depth -= 1;
                    return Ok(JsonValue::Object(map));
                }
                _ => {
                    return Err(CoreError::InvalidJson(format!(
                        "expected ',' or '}}' at offset {}",
                        self.pos
                    )));
                }
            }
        }
    }
}
