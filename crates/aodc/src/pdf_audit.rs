#![forbid(unsafe_code)]

//! Pure-Rust, `std`-only, zero-float PDF zero-raster, unresolved-reference, page-count,
//! and SHA-256 determinism auditor (`aodc audit-pdf`).
//!
//! Includes a self-contained RFC 1950 (zlib + Adler-32) and RFC 1951 (DEFLATE stored,
//! fixed Huffman, and dynamic Huffman) inflater with strict decompression caps, zero
//! external crates, zero `aod-llvm` dependencies, and portable host-path-free JSON reports.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use aod_core::json::JsonValue;
use aod_core::sha256::Sha256;

pub const PDF_AUDIT_SCHEMA: &str = "aod.dec.pdf.audit.v1";
pub const MAX_PDF_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_STREAM_DECOMP_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_TOTAL_DECOMP_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_STREAMS: usize = 16_384;

const MAX_INLINE_IMAGE_DICT_BYTES: usize = 8192;

const UNRESOLVED_MARKERS: [&str; 5] = [
    "Unresolved Main reference",
    "Unresolved internal reference",
    "Unresolved citation",
    "[unresolved]",
    "PANIC",
];

const INLINE_IMAGE_KEYS: [&[u8]; 16] = [
    b"W",
    b"Width",
    b"H",
    b"Height",
    b"BPC",
    b"BitsPerComponent",
    b"CS",
    b"ColorSpace",
    b"IM",
    b"ImageMask",
    b"F",
    b"Filter",
    b"D",
    b"Decode",
    b"I",
    b"Interpolate",
];

// ============================================================================
// RFC 1950 (zlib / Adler-32) + RFC 1951 (DEFLATE) Inflater
// ============================================================================

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InflateError {
    TruncatedInput,
    UnsupportedCompressionMethod(u8),
    InvalidWindowSize(u8),
    BadHeaderChecksum,
    PresetDictionaryUnsupported,
    InvalidBlockType,
    StoredLengthMismatch,
    InvalidHuffmanTree(&'static str),
    InvalidSymbol(u16),
    InvalidDistance {
        dist: usize,
        available: usize,
        window: usize,
    },
    DecompressionLimitExceeded {
        limit: usize,
    },
    Adler32Mismatch {
        expected: u32,
        actual: u32,
    },
    TrailingGarbage,
}

impl fmt::Display for InflateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InflateError::TruncatedInput => write!(f, "truncated zlib/DEFLATE stream"),
            InflateError::UnsupportedCompressionMethod(cm) => {
                write!(f, "unsupported zlib compression method CM={cm} (expected 8)")
            }
            InflateError::InvalidWindowSize(cinfo) => {
                write!(f, "invalid zlib window size CINFO={cinfo} (must be <= 7)")
            }
            InflateError::BadHeaderChecksum => write!(f, "invalid zlib FCHECK header checksum"),
            InflateError::PresetDictionaryUnsupported => {
                write!(f, "zlib preset dictionary (FDICT=1) is unsupported")
            }
            InflateError::InvalidBlockType => write!(f, "reserved DEFLATE block type BTYPE=3"),
            InflateError::StoredLengthMismatch => {
                write!(f, "stored DEFLATE block LEN != !NLEN")
            }
            InflateError::InvalidHuffmanTree(reason) => {
                write!(f, "invalid DEFLATE Huffman tree: {reason}")
            }
            InflateError::InvalidSymbol(sym) => {
                write!(f, "invalid DEFLATE symbol {sym}")
            }
            InflateError::InvalidDistance {
                dist,
                available,
                window,
            } => write!(
                f,
                "invalid DEFLATE distance {dist} (available={available}, window={window})"
            ),
            InflateError::DecompressionLimitExceeded { limit } => {
                write!(f, "decompression limit exceeded ({limit} bytes)")
            }
            InflateError::Adler32Mismatch { expected, actual } => write!(
                f,
                "Adler-32 checksum mismatch (expected 0x{expected:08x}, actual 0x{actual:08x})"
            ),
            InflateError::TrailingGarbage => {
                write!(f, "unexpected non-whitespace trailing bytes after zlib trailer")
            }
        }
    }
}

impl std::error::Error for InflateError {}

/// Compute RFC 1950 §8.2 Adler-32 checksum without overflow using 5552-byte chunks.
pub fn adler32(data: &[u8]) -> u32 {
    const MOD_ADLER: u32 = 65521;
    let mut s1: u32 = 1;
    let mut s2: u32 = 0;
    for chunk in data.chunks(5552) {
        for &b in chunk {
            s1 += u32::from(b);
            s2 += s1;
        }
        s1 %= MOD_ADLER;
        s2 %= MOD_ADLER;
    }
    (s2 << 16) | s1
}

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bit_buf: u64,
    bit_cnt: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            bit_buf: 0,
            bit_cnt: 0,
        }
    }

    fn read_bit(&mut self) -> Result<u32, InflateError> {
        if self.bit_cnt == 0 {
            if self.pos >= self.data.len() {
                return Err(InflateError::TruncatedInput);
            }
            self.bit_buf = u64::from(self.data[self.pos]);
            self.pos += 1;
            self.bit_cnt = 8;
        }
        let bit = (self.bit_buf & 1) as u32;
        self.bit_buf >>= 1;
        self.bit_cnt -= 1;
        Ok(bit)
    }

    fn read_bits(&mut self, n: u8) -> Result<u32, InflateError> {
        debug_assert!(n <= 24);
        while self.bit_cnt < n {
            if self.pos >= self.data.len() {
                return Err(InflateError::TruncatedInput);
            }
            self.bit_buf |= u64::from(self.data[self.pos]) << self.bit_cnt;
            self.pos += 1;
            self.bit_cnt += 8;
        }
        let mask = if n == 0 { 0 } else { (1u64 << n) - 1 };
        let val = (self.bit_buf & mask) as u32;
        self.bit_buf >>= n;
        self.bit_cnt -= n;
        Ok(val)
    }

    /// Discard padding bits (`bit_cnt & 7`) to reach the next byte boundary AND rewind any
    /// whole unread buffered bytes (`bit_cnt / 8`) so stored blocks and trailers align cleanly.
    fn align_to_byte(&mut self) {
        let unread_whole_bytes = usize::from(self.bit_cnt / 8);
        self.pos -= unread_whole_bytes;
        self.bit_buf = 0;
        self.bit_cnt = 0;
    }

    fn read_aligned_bytes(&mut self, n: usize) -> Result<&'a [u8], InflateError> {
        debug_assert_eq!(self.bit_cnt, 0);
        let end = self
            .pos
            .checked_add(n)
            .ok_or(InflateError::TruncatedInput)?;
        if end > self.data.len() {
            return Err(InflateError::TruncatedInput);
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum TreeKind {
    CodeLength,
    LitLen,
    Distance,
}

struct HuffmanTable {
    counts: [u16; 16],
    symbols: Vec<u16>,
    max_len: usize,
}

impl HuffmanTable {
    fn build(lengths: &[u8], kind: TreeKind) -> Result<Self, InflateError> {
        let max_allowed = match kind {
            TreeKind::CodeLength => 7u8,
            TreeKind::LitLen | TreeKind::Distance => 15u8,
        };
        let mut counts = [0u16; 16];
        let mut total_syms: usize = 0;
        let mut max_len: usize = 0;

        for &len in lengths {
            if len > max_allowed {
                return Err(InflateError::InvalidHuffmanTree(
                    "code length exceeds maximum",
                ));
            }
            if len > 0 {
                let l = usize::from(len);
                counts[l] += 1;
                total_syms += 1;
                if l > max_len {
                    max_len = l;
                }
            }
        }

        if total_syms == 0 {
            return match kind {
                TreeKind::Distance => Ok(Self {
                    counts,
                    symbols: Vec::new(),
                    max_len: 0,
                }),
                TreeKind::CodeLength => Err(InflateError::InvalidHuffmanTree(
                    "empty code-length Huffman table",
                )),
                TreeKind::LitLen => Err(InflateError::InvalidHuffmanTree(
                    "empty literal/length Huffman table",
                )),
            };
        }

        if kind == TreeKind::LitLen && lengths.get(256).copied().unwrap_or(0) == 0 {
            return Err(InflateError::InvalidHuffmanTree(
                "missing end-of-block symbol 256",
            ));
        }

        // Verify Kraft inequality: reject oversubscribed trees and incomplete trees except
        // the canonical single-symbol tree whose sole symbol has length 1.
        let mut left: i32 = 1;
        for &c in &counts[1..=usize::from(max_allowed)] {
            left <<= 1;
            left -= i32::from(c);
            if left < 0 {
                return Err(InflateError::InvalidHuffmanTree(
                    "oversubscribed Huffman tree",
                ));
            }
        }
        if left > 0 && !(total_syms == 1 && counts[1] == 1) {
            return Err(InflateError::InvalidHuffmanTree(
                "incomplete Huffman tree",
            ));
        }

        let mut offsets = [0usize; 16];
        for len in 1..15 {
            offsets[len + 1] = offsets[len] + usize::from(counts[len]);
        }

        let mut symbols = vec![0u16; total_syms];
        for (sym, &len) in lengths.iter().enumerate() {
            if len > 0 {
                let l = usize::from(len);
                symbols[offsets[l]] = sym as u16;
                offsets[l] += 1;
            }
        }

        Ok(Self {
            counts,
            symbols,
            max_len,
        })
    }

    fn fixed_litlen() -> Self {
        let mut lengths = [0u8; 288];
        for v in &mut lengths[0..=143] {
            *v = 8;
        }
        for v in &mut lengths[144..=255] {
            *v = 9;
        }
        for v in &mut lengths[256..=279] {
            *v = 7;
        }
        for v in &mut lengths[280..=287] {
            *v = 8;
        }
        Self::build(&lengths, TreeKind::LitLen).expect("RFC 1951 fixed litlen table is valid")
    }

    fn fixed_distance() -> Self {
        let lengths = [5u8; 32];
        Self::build(&lengths, TreeKind::Distance).expect("RFC 1951 fixed distance table is valid")
    }

    /// Decode one symbol strictly bit-by-bit (zero EOF-peek hazard).
    fn decode(&self, reader: &mut BitReader<'_>) -> Result<u16, InflateError> {
        if self.max_len == 0 {
            return Err(InflateError::InvalidHuffmanTree(
                "cannot decode from empty distance table",
            ));
        }
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: usize = 0;
        for len in 1..=self.max_len {
            let bit = reader.read_bit()? as i32;
            code |= bit;
            let count = i32::from(self.counts[len]);
            if code - count < first {
                return Ok(self.symbols[index + (code - first) as usize]);
            }
            index += count as usize;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(InflateError::InvalidHuffmanTree("invalid Huffman code"))
    }
}

const CL_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
    131, 163, 195, 227, 258,
];

const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];

const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];

const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

fn decode_huffman_block(
    reader: &mut BitReader<'_>,
    litlen_table: &HuffmanTable,
    dist_table: &HuffmanTable,
    max_window: usize,
    max_out: usize,
    out: &mut Vec<u8>,
) -> Result<(), InflateError> {
    loop {
        let sym = litlen_table.decode(reader)?;
        if sym < 256 {
            if out.len() >= max_out {
                return Err(InflateError::DecompressionLimitExceeded { limit: max_out });
            }
            out.push(sym as u8);
        } else if sym == 256 {
            return Ok(());
        } else if sym <= 285 {
            let lidx = usize::from(sym - 257);
            let extra = reader.read_bits(LEN_EXTRA[lidx])? as usize;
            let length = usize::from(LEN_BASE[lidx]) + extra;

            let dsym = dist_table.decode(reader)?;
            if dsym >= 30 {
                return Err(InflateError::InvalidSymbol(dsym));
            }
            let dextra = reader.read_bits(DIST_EXTRA[usize::from(dsym)])? as usize;
            let dist = usize::from(DIST_BASE[usize::from(dsym)]) + dextra;

            if dist == 0 || dist > out.len() || dist > max_window {
                return Err(InflateError::InvalidDistance {
                    dist,
                    available: out.len(),
                    window: max_window,
                });
            }
            let new_len = out
                .len()
                .checked_add(length)
                .ok_or(InflateError::DecompressionLimitExceeded { limit: max_out })?;
            if new_len > max_out {
                return Err(InflateError::DecompressionLimitExceeded { limit: max_out });
            }
            if dist == 1 {
                let b = out[out.len() - 1];
                out.resize(new_len, b);
            } else if dist >= length {
                let start = out.len() - dist;
                out.extend_from_within(start..start + length);
            } else {
                out.reserve(length);
                for _ in 0..length {
                    let b = out[out.len() - dist];
                    out.push(b);
                }
            }
        } else {
            return Err(InflateError::InvalidSymbol(sym));
        }
    }
}

fn inflate_deflate_with_window(
    input: &[u8],
    max_window: usize,
    max_out: usize,
) -> Result<(Vec<u8>, usize), InflateError> {
    let mut reader = BitReader::new(input);
    let mut out = Vec::new();
    let mut fixed_tables: Option<(HuffmanTable, HuffmanTable)> = None;

    loop {
        let bfinal = reader.read_bits(1)?;
        let btype = reader.read_bits(2)?;
        match btype {
            0 => {
                // Stored (uncompressed) block
                reader.align_to_byte();
                let hdr = reader.read_aligned_bytes(4)?;
                let len = u16::from_le_bytes([hdr[0], hdr[1]]);
                let nlen = u16::from_le_bytes([hdr[2], hdr[3]]);
                if len != !nlen {
                    return Err(InflateError::StoredLengthMismatch);
                }
                let block_len = usize::from(len);
                let new_len = out
                    .len()
                    .checked_add(block_len)
                    .ok_or(InflateError::DecompressionLimitExceeded { limit: max_out })?;
                if new_len > max_out {
                    return Err(InflateError::DecompressionLimitExceeded { limit: max_out });
                }
                let bytes = reader.read_aligned_bytes(block_len)?;
                out.extend_from_slice(bytes);
            }
            1 => {
                // Fixed Huffman block
                let (litlen, dist) = fixed_tables.get_or_insert_with(|| {
                    (HuffmanTable::fixed_litlen(), HuffmanTable::fixed_distance())
                });
                decode_huffman_block(&mut reader, litlen, dist, max_window, max_out, &mut out)?;
            }
            2 => {
                // Dynamic Huffman block
                let hlit = reader.read_bits(5)? as usize + 257;
                if hlit > 286 {
                    return Err(InflateError::InvalidHuffmanTree("HLIT exceeds 286"));
                }
                let hdist = reader.read_bits(5)? as usize + 1;
                if hdist > 30 {
                    return Err(InflateError::InvalidHuffmanTree("HDIST exceeds 30"));
                }
                let hclen = reader.read_bits(4)? as usize + 4;

                let mut cl_lengths = [0u8; 19];
                for &slot in &CL_ORDER[..hclen] {
                    cl_lengths[slot] = reader.read_bits(3)? as u8;
                }
                let cl_table = HuffmanTable::build(&cl_lengths, TreeKind::CodeLength)?;

                let total = hlit + hdist;
                let mut lens = vec![0u8; total];
                let mut idx = 0usize;
                while idx < total {
                    let sym = cl_table.decode(&mut reader)?;
                    match sym {
                        0..=15 => {
                            lens[idx] = sym as u8;
                            idx += 1;
                        }
                        16 => {
                            if idx == 0 {
                                return Err(InflateError::InvalidHuffmanTree(
                                    "RLE symbol 16 at index 0",
                                ));
                            }
                            let repeat = reader.read_bits(2)? as usize + 3;
                            if idx + repeat > total {
                                return Err(InflateError::InvalidHuffmanTree(
                                    "RLE symbol 16 overflows HLIT + HDIST",
                                ));
                            }
                            let prev = lens[idx - 1];
                            for v in &mut lens[idx..idx + repeat] {
                                *v = prev;
                            }
                            idx += repeat;
                        }
                        17 => {
                            let repeat = reader.read_bits(3)? as usize + 3;
                            if idx + repeat > total {
                                return Err(InflateError::InvalidHuffmanTree(
                                    "RLE symbol 17 overflows HLIT + HDIST",
                                ));
                            }
                            idx += repeat;
                        }
                        18 => {
                            let repeat = reader.read_bits(7)? as usize + 11;
                            if idx + repeat > total {
                                return Err(InflateError::InvalidHuffmanTree(
                                    "RLE symbol 18 overflows HLIT + HDIST",
                                ));
                            }
                            idx += repeat;
                        }
                        other => return Err(InflateError::InvalidSymbol(other)),
                    }
                }

                let litlen_table = HuffmanTable::build(&lens[..hlit], TreeKind::LitLen)?;
                let dist_table = HuffmanTable::build(&lens[hlit..total], TreeKind::Distance)?;
                decode_huffman_block(
                    &mut reader,
                    &litlen_table,
                    &dist_table,
                    max_window,
                    max_out,
                    &mut out,
                )?;
            }
            _ => return Err(InflateError::InvalidBlockType),
        }

        if bfinal == 1 {
            break;
        }
    }

    reader.align_to_byte();
    Ok((out, reader.pos))
}

/// Inflate a raw RFC 1951 DEFLATE stream (32 KiB max window), returning `(output, bytes_consumed)`.
pub fn inflate_deflate(input: &[u8], max_out: usize) -> Result<(Vec<u8>, usize), InflateError> {
    inflate_deflate_with_window(input, 32768, max_out)
}

/// Inflate an RFC 1950 zlib stream, verifying header, window bound, Adler-32 trailer, and
/// exact consumption (permitting only optional trailing PDF whitespace after the 4-byte trailer).
pub fn inflate_zlib(input: &[u8], max_out: usize) -> Result<Vec<u8>, InflateError> {
    if input.len() < 6 {
        return Err(InflateError::TruncatedInput);
    }
    let cmf = input[0];
    let flg = input[1];
    let cm = cmf & 0x0F;
    if cm != 8 {
        return Err(InflateError::UnsupportedCompressionMethod(cm));
    }
    let cinfo = cmf >> 4;
    if cinfo > 7 {
        return Err(InflateError::InvalidWindowSize(cinfo));
    }
    if ((u16::from(cmf) << 8) | u16::from(flg)) % 31 != 0 {
        return Err(InflateError::BadHeaderChecksum);
    }
    if (flg & 0x20) != 0 {
        return Err(InflateError::PresetDictionaryUnsupported);
    }

    let max_window = 1usize << (usize::from(cinfo) + 8);
    let (out, deflate_consumed) = inflate_deflate_with_window(&input[2..], max_window, max_out)?;

    let trailer_pos = 2 + deflate_consumed;
    if trailer_pos + 4 > input.len() {
        return Err(InflateError::TruncatedInput);
    }
    let expected_adler = u32::from_be_bytes([
        input[trailer_pos],
        input[trailer_pos + 1],
        input[trailer_pos + 2],
        input[trailer_pos + 3],
    ]);

    let trailing = &input[trailer_pos + 4..];
    if !trailing
        .iter()
        .all(|&b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
    {
        return Err(InflateError::TrailingGarbage);
    }

    let actual_adler = adler32(&out);
    if actual_adler != expected_adler {
        return Err(InflateError::Adler32Mismatch {
            expected: expected_adler,
            actual: actual_adler,
        });
    }

    Ok(out)
}

// ============================================================================
// PDF Lexer, Stream Framing, and Structure Scanners
// ============================================================================

#[inline]
fn is_pdf_ws(b: u8) -> bool {
    matches!(b, 0x00 | 0x09 | 0x0A | 0x0C | 0x0D | 0x20)
}

#[inline]
fn is_pdf_delim(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

#[inline]
fn is_pdf_boundary(b: u8) -> bool {
    is_pdf_ws(b) || is_pdf_delim(b)
}

#[inline]
fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Normalize ISO 32000-1 §7.3.5 `#XX` hex escapes in PDF name bytes.
fn decode_pdf_name(raw_name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw_name.len());
    let mut i = 0usize;
    while i < raw_name.len() {
        if raw_name[i] == b'#' && i + 2 < raw_name.len() {
            if let (Some(hi), Some(lo)) = (hex_nibble(raw_name[i + 1]), hex_nibble(raw_name[i + 2]))
            {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(raw_name[i]);
        i += 1;
    }
    out
}

/// Skip a PDF literal string `(...)` starting at `data[start] == b'('`, returning the index
/// immediately after the matching `)` (or `data.len()` if unterminated).
fn skip_literal_string(data: &[u8], start: usize) -> usize {
    let n = data.len();
    let mut i = start + 1;
    let mut depth = 1usize;
    while i < n && depth > 0 {
        let c = data[i];
        if c == b'\\' && i + 1 < n {
            i += 2;
            continue;
        }
        if c == b'(' {
            depth += 1;
        } else if c == b')' {
            depth -= 1;
        }
        i += 1;
    }
    i
}

/// Skip a PDF comment `% ...` starting at `data[start] == b'%'` up to (not including) `\r`/`\n`.
fn skip_comment(data: &[u8], start: usize) -> usize {
    let n = data.len();
    let mut i = start + 1;
    while i < n && data[i] != b'\r' && data[i] != b'\n' {
        i += 1;
    }
    i
}

/// Skip a PDF hex string `<...>` starting at `data[start] == b'<'` (caller checked not `<<`).
fn skip_hex_string(data: &[u8], start: usize) -> usize {
    let n = data.len();
    let mut i = start + 1;
    while i < n && data[i] != b'>' {
        i += 1;
    }
    if i < n {
        i + 1
    } else {
        n
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PdfToken {
    DictStart,
    DictEnd,
    ArrayStart,
    ArrayEnd,
    Name(Vec<u8>),
    UInt(usize),
    KeywordR,
    KeywordNull,
    Other,
}

fn tokenize_pdf(data: &[u8]) -> Vec<PdfToken> {
    let mut tokens = Vec::new();
    let n = data.len();
    let mut i = 0usize;

    while i < n {
        let b = data[i];
        if is_pdf_ws(b) {
            i += 1;
            continue;
        }
        if b == b'%' {
            i = skip_comment(data, i);
            continue;
        }
        if b == b'(' {
            i = skip_literal_string(data, i);
            tokens.push(PdfToken::Other);
            continue;
        }
        if b == b'<' {
            if i + 1 < n && data[i + 1] == b'<' {
                tokens.push(PdfToken::DictStart);
                i += 2;
            } else {
                i = skip_hex_string(data, i);
                tokens.push(PdfToken::Other);
            }
            continue;
        }
        if b == b'>' {
            if i + 1 < n && data[i + 1] == b'>' {
                tokens.push(PdfToken::DictEnd);
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if b == b'[' {
            tokens.push(PdfToken::ArrayStart);
            i += 1;
            continue;
        }
        if b == b']' {
            tokens.push(PdfToken::ArrayEnd);
            i += 1;
            continue;
        }
        if b == b'{' || b == b'}' || b == b')' {
            i += 1;
            continue;
        }
        if b == b'/' {
            let start = i + 1;
            i = start;
            while i < n && !is_pdf_boundary(data[i]) {
                i += 1;
            }
            tokens.push(PdfToken::Name(decode_pdf_name(&data[start..i])));
            continue;
        }

        // Regular token (unsigned integer or keyword) with zero heap allocation
        let start = i;
        while i < n && !is_pdf_boundary(data[i]) {
            i += 1;
        }
        let slice = &data[start..i];
        if slice == b"R" {
            tokens.push(PdfToken::KeywordR);
        } else if slice == b"null" {
            tokens.push(PdfToken::KeywordNull);
        } else if !slice.is_empty() && slice.len() <= 20 && slice.iter().all(|c| c.is_ascii_digit())
        {
            if let Ok(s) = std::str::from_utf8(slice) {
                if let Ok(val) = s.parse::<usize>() {
                    tokens.push(PdfToken::UInt(val));
                    continue;
                }
            }
            tokens.push(PdfToken::Other);
        } else {
            tokens.push(PdfToken::Other);
        }
    }

    tokens
}

fn is_unsigned_digits(tok: &PdfToken) -> Option<usize> {
    if let PdfToken::UInt(v) = *tok {
        Some(v)
    } else {
        None
    }
}

fn is_indirect_ref(tokens: &[PdfToken], idx: usize) -> bool {
    if idx + 2 >= tokens.len() {
        return false;
    }
    matches!(
        (&tokens[idx], &tokens[idx + 1], &tokens[idx + 2]),
        (PdfToken::UInt(_), PdfToken::UInt(_), PdfToken::KeywordR)
    )
}

fn skip_pdf_value(tokens: &[PdfToken], mut idx: usize) -> usize {
    if idx >= tokens.len() {
        return idx;
    }
    match &tokens[idx] {
        PdfToken::DictStart => {
            let mut depth = 1usize;
            idx += 1;
            while idx < tokens.len() && depth > 0 {
                match tokens[idx] {
                    PdfToken::DictStart => depth += 1,
                    PdfToken::DictEnd => depth -= 1,
                    _ => {}
                }
                idx += 1;
            }
            idx
        }
        PdfToken::ArrayStart => {
            let mut depth = 1usize;
            idx += 1;
            while idx < tokens.len() && depth > 0 {
                match tokens[idx] {
                    PdfToken::ArrayStart => depth += 1,
                    PdfToken::ArrayEnd => depth -= 1,
                    _ => {}
                }
                idx += 1;
            }
            idx
        }
        _ => {
            if is_indirect_ref(tokens, idx) {
                idx + 3
            } else {
                idx + 1
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum FilterSpec {
    None,
    FlateDecodeOnly,
    Unsupported(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StreamDictInfo {
    direct_length: Option<usize>,
    filter: FilterSpec,
    dict_type: Option<Vec<u8>>,
    subtype: Option<Vec<u8>>,
    has_length1: bool,
}

impl Default for StreamDictInfo {
    fn default() -> Self {
        Self {
            direct_length: None,
            filter: FilterSpec::None,
            dict_type: None,
            subtype: None,
            has_length1: false,
        }
    }
}

/// Parse top-level (depth-1) keys of an enclosing stream dictionary `<< ... >>`.
/// Nested dictionaries (`/DecodeParms << ... >>`, `/Resources << ... >>`) and arrays are skipped
/// cleanly so nested `/Length` or `/Filter` keys never shadow top-level entries.
fn parse_stream_dict(dict_bytes: &[u8]) -> StreamDictInfo {
    let tokens = tokenize_pdf(dict_bytes);
    let mut info = StreamDictInfo::default();
    if tokens.first() != Some(&PdfToken::DictStart) {
        return info;
    }

    let mut idx = 1usize;
    while idx < tokens.len() {
        match &tokens[idx] {
            PdfToken::DictEnd => break,
            PdfToken::Name(key) => {
                let val_idx = idx + 1;
                if val_idx >= tokens.len() || tokens[val_idx] == PdfToken::DictEnd {
                    break;
                }
                match key.as_slice() {
                    b"Length" => {
                        if is_indirect_ref(&tokens, val_idx) {
                            info.direct_length = None;
                            idx = val_idx + 3;
                        } else if let Some(n) = is_unsigned_digits(&tokens[val_idx]) {
                            info.direct_length = Some(n);
                            idx = val_idx + 1;
                        } else {
                            idx = skip_pdf_value(&tokens, val_idx);
                        }
                    }
                    b"Filter" => match &tokens[val_idx] {
                        PdfToken::Name(fname) => {
                            if fname == b"FlateDecode" {
                                info.filter = FilterSpec::FlateDecodeOnly;
                            } else {
                                info.filter = FilterSpec::Unsupported(format!(
                                    "/{}",
                                    String::from_utf8_lossy(fname)
                                ));
                            }
                            idx = val_idx + 1;
                        }
                        PdfToken::ArrayStart => {
                            let mut cur = val_idx + 1;
                            let mut depth = 1usize;
                            let mut names: Vec<Vec<u8>> = Vec::new();
                            let mut non_name = false;
                            while cur < tokens.len() && depth > 0 {
                                match &tokens[cur] {
                                    PdfToken::ArrayStart => {
                                        depth += 1;
                                        non_name = true;
                                    }
                                    PdfToken::ArrayEnd => {
                                        depth -= 1;
                                    }
                                    PdfToken::Name(n) if depth == 1 => {
                                        names.push(n.clone());
                                    }
                                    _ if depth == 1 => {
                                        non_name = true;
                                    }
                                    _ => {}
                                }
                                cur += 1;
                            }
                            if !non_name && names.is_empty() {
                                info.filter = FilterSpec::None;
                            } else if !non_name
                                && names.len() == 1
                                && names[0] == b"FlateDecode"
                            {
                                info.filter = FilterSpec::FlateDecodeOnly;
                            } else {
                                let desc = if non_name {
                                    "[non-name filter entry]".to_string()
                                } else {
                                    let parts: Vec<String> = names
                                        .iter()
                                        .map(|n| format!("/{}", String::from_utf8_lossy(n)))
                                        .collect();
                                    format!("[{}]", parts.join(" "))
                                };
                                info.filter = FilterSpec::Unsupported(desc);
                            }
                            idx = cur;
                        }
                        PdfToken::KeywordNull => {
                            info.filter = FilterSpec::None;
                            idx = val_idx + 1;
                        }
                        _ => {
                            info.filter =
                                FilterSpec::Unsupported("non-literal /Filter value".to_string());
                            idx = skip_pdf_value(&tokens, val_idx);
                        }
                    },
                    b"Type" => {
                        if let PdfToken::Name(t) = &tokens[val_idx] {
                            info.dict_type = Some(t.clone());
                            idx = val_idx + 1;
                        } else {
                            idx = skip_pdf_value(&tokens, val_idx);
                        }
                    }
                    b"Subtype" => {
                        if let PdfToken::Name(s) = &tokens[val_idx] {
                            info.subtype = Some(s.clone());
                            idx = val_idx + 1;
                        } else {
                            idx = skip_pdf_value(&tokens, val_idx);
                        }
                    }
                    b"Length1" => {
                        info.has_length1 = true;
                        idx = skip_pdf_value(&tokens, val_idx);
                    }
                    _ => {
                        idx = skip_pdf_value(&tokens, val_idx);
                    }
                }
            }
            _ => {
                idx = skip_pdf_value(&tokens, idx);
            }
        }
    }

    info
}

fn is_binary_font_or_xref(info: &StreamDictInfo) -> bool {
    if info.has_length1 {
        return true;
    }
    if info.dict_type.as_deref() == Some(b"XRef") {
        return true;
    }
    matches!(
        info.subtype.as_deref(),
        Some(b"Type1C" | b"CIDFontType0C" | b"OpenType")
    )
}

fn find_standalone_endstream(raw: &[u8], start: usize) -> Option<usize> {
    const NEEDLE: &[u8] = b"endstream";
    if raw.len() < start + NEEDLE.len() {
        return None;
    }
    let mut i = start;
    let limit = raw.len() - NEEDLE.len();
    while i <= limit {
        if &raw[i..i + NEEDLE.len()] == NEEDLE {
            let left_ok = i == start || is_pdf_boundary(raw[i - 1]);
            let right_ok =
                i + NEEDLE.len() == raw.len() || is_pdf_boundary(raw[i + NEEDLE.len()]);
            if left_ok && right_ok {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

struct ExtractedStream<'a> {
    index: usize,
    dict_info: StreamDictInfo,
    raw_slice: &'a [u8],
    is_direct_length: bool,
}

/// Scan `raw` once in forward order, locating all `stream..endstream` spans, extracting each
/// stream's enclosing `<< ... >>` dictionary via balanced depth tracking, and building
/// `root_structure` with every stream binary payload masked by ASCII spaces (`0x20`).
fn extract_streams_and_mask_root<'a>(
    raw: &'a [u8],
) -> (Vec<u8>, Vec<ExtractedStream<'a>>, Vec<String>) {
    let mut root_structure = raw.to_vec();
    let mut streams = Vec::new();
    let mut errors = Vec::new();

    let n = raw.len();
    let mut pos = 0usize;
    let mut dict_stack: Vec<usize> = Vec::new();
    let mut last_closed_dict: Option<(usize, usize)> = None;

    while pos < n {
        let b = raw[pos];
        if is_pdf_ws(b) {
            pos += 1;
            continue;
        }
        if b == b'%' {
            pos = skip_comment(raw, pos);
            continue;
        }
        if b == b'(' {
            pos = skip_literal_string(raw, pos);
            last_closed_dict = None;
            continue;
        }
        if b == b'<' {
            if pos + 1 < n && raw[pos + 1] == b'<' {
                dict_stack.push(pos);
                last_closed_dict = None;
                pos += 2;
            } else {
                pos = skip_hex_string(raw, pos);
                last_closed_dict = None;
            }
            continue;
        }
        if b == b'>' && pos + 1 < n && raw[pos + 1] == b'>' {
            if let Some(open_pos) = dict_stack.pop() {
                last_closed_dict = Some((open_pos, pos + 2));
            } else {
                last_closed_dict = None;
            }
            pos += 2;
            continue;
        }

        // Check for standalone `stream` keyword followed by `\r\n`, `\n`, or bare `\r`
        if b == b's'
            && raw[pos..].starts_with(b"stream")
            && (pos == 0 || is_pdf_boundary(raw[pos - 1]))
        {
            let after_kw = pos + 6;
            let data_start_opt = if raw[after_kw..].starts_with(b"\r\n") {
                Some(after_kw + 2)
            } else if after_kw < n && raw[after_kw] == b'\n' {
                Some(after_kw + 1)
            } else if after_kw < n && raw[after_kw] == b'\r' {
                Some(after_kw + 1)
            } else {
                None
            };

            if let (Some(data_start), Some((dict_start, dict_end))) =
                (data_start_opt, last_closed_dict)
            {
                if streams.len() >= MAX_STREAMS {
                    errors.push(format!(
                        "pdf: stream count exceeds maximum limit of {MAX_STREAMS}"
                    ));
                    break;
                }
                let stream_idx = streams.len() + 1;
                let dict_info = parse_stream_dict(&raw[dict_start..dict_end]);
                last_closed_dict = None;

                if let Some(direct_len) = dict_info.direct_length {
                    let valid_direct = data_start.checked_add(direct_len).and_then(|data_end| {
                        if data_end > n {
                            return None;
                        }
                        let mut k = data_end;
                        while k < n && is_pdf_ws(raw[k]) {
                            k += 1;
                        }
                        if raw[k..].starts_with(b"endstream")
                            && (k + 9 == n || is_pdf_boundary(raw[k + 9]))
                        {
                            Some((data_end, k + 9))
                        } else {
                            None
                        }
                    });

                    if let Some((data_end, next_pos)) = valid_direct {
                        for byte in &mut root_structure[data_start..data_end] {
                            *byte = b' ';
                        }
                        streams.push(ExtractedStream {
                            index: stream_idx,
                            dict_info,
                            raw_slice: &raw[data_start..data_end],
                            is_direct_length: true,
                        });
                        pos = next_pos;
                        continue;
                    } else {
                        errors.push(format!(
                            "stream[{stream_idx}]: direct /Length {direct_len} does not align with endstream"
                        ));
                        if let Some(es) = find_standalone_endstream(raw, data_start) {
                            for byte in &mut root_structure[data_start..es] {
                                *byte = b' ';
                            }
                            streams.push(ExtractedStream {
                                index: stream_idx,
                                dict_info,
                                raw_slice: &raw[data_start..es],
                                is_direct_length: false,
                            });
                            pos = es + 9;
                            continue;
                        } else {
                            for byte in &mut root_structure[data_start..n] {
                                *byte = b' ';
                            }
                            errors.push(format!(
                                "stream[{stream_idx}]: unterminated stream (missing endstream)"
                            ));
                            break;
                        }
                    }
                } else if let Some(es) = find_standalone_endstream(raw, data_start) {
                    for byte in &mut root_structure[data_start..es] {
                        *byte = b' ';
                    }
                    streams.push(ExtractedStream {
                        index: stream_idx,
                        dict_info,
                        raw_slice: &raw[data_start..es],
                        is_direct_length: false,
                    });
                    pos = es + 9;
                    continue;
                } else {
                    for byte in &mut root_structure[data_start..n] {
                        *byte = b' ';
                    }
                    errors.push(format!(
                        "stream[{stream_idx}]: unterminated stream (missing endstream)"
                    ));
                    break;
                }
            }
        }

        // Advance past regular token or delimiter; reset dict_stack on `endobj` for resilience.
        if is_pdf_delim(b) {
            last_closed_dict = None;
            pos += 1;
        } else {
            let start = pos;
            while pos < n && !is_pdf_boundary(raw[pos]) {
                pos += 1;
            }
            if &raw[start..pos] == b"endobj" {
                dict_stack.clear();
            }
            last_closed_dict = None;
        }
    }

    (root_structure, streams, errors)
}

/// Inflate an indirect-length `/FlateDecode` stream slice by stripping AT MOST ONE trailing EOL
/// (`\r\n` or `\n` or `\r`), never greedy-stripping multiple bytes (preserving Adler-32 trailers
/// ending in `0x0A` or `0x0D`!).
fn inflate_indirect_flate(slice: &[u8], max_out: usize) -> Result<Vec<u8>, InflateError> {
    let mut candidates: Vec<&[u8]> = Vec::with_capacity(3);
    if let Some(s) = slice.strip_suffix(b"\r\n") {
        candidates.push(s);
        // If the zlib Adler-32 itself ended in 0x0D and was followed by a single `\n` before
        // `endstream`, stripping only `\n` preserves the 0x0D trailer byte.
        candidates.push(&slice[..slice.len() - 1]);
        candidates.push(slice);
    } else if let Some(s) = slice.strip_suffix(b"\n") {
        candidates.push(s);
        candidates.push(slice);
    } else if let Some(s) = slice.strip_suffix(b"\r") {
        candidates.push(s);
        candidates.push(slice);
    } else {
        candidates.push(slice);
    }

    let mut first_err: Option<InflateError> = None;
    for cand in candidates {
        match inflate_zlib(cand, max_out) {
            Ok(out) => return Ok(out),
            Err(e) => {
                if matches!(e, InflateError::DecompressionLimitExceeded { .. }) {
                    return Err(e);
                }
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
    }
    Err(first_err.unwrap_or(InflateError::TruncatedInput))
}

fn strip_single_trailing_eol(slice: &[u8]) -> &[u8] {
    if let Some(s) = slice.strip_suffix(b"\r\n") {
        s
    } else if let Some(s) = slice.strip_suffix(b"\n") {
        s
    } else if let Some(s) = slice.strip_suffix(b"\r") {
        s
    } else {
        slice
    }
}

fn has_subtype_image(tokens: &[PdfToken]) -> bool {
    for pair in tokens.windows(2) {
        if let (PdfToken::Name(k), PdfToken::Name(v)) = (&pair[0], &pair[1]) {
            if k == b"Subtype" && v == b"Image" {
                return true;
            }
        }
    }
    false
}

fn count_page_objects(tokens: &[PdfToken]) -> usize {
    let mut count = 0usize;
    for pair in tokens.windows(2) {
        if let (PdfToken::Name(k), PdfToken::Name(v)) = (&pair[0], &pair[1]) {
            if k == b"Type" && v == b"Page" {
                count += 1;
            }
        }
    }
    count
}

/// Extract maximum `/Count N` from any dictionary also declaring `/Type /Pages` (regardless of
/// whether `/Count N` precedes or follows `/Type /Pages`).
fn max_pages_tree_count(tokens: &[PdfToken]) -> usize {
    #[derive(Default)]
    struct DictFrame {
        has_type_pages: bool,
        count: Option<usize>,
        array_depth: usize,
    }

    let mut stack: Vec<DictFrame> = Vec::new();
    let mut best = 0usize;
    let mut i = 0usize;

    while i < tokens.len() {
        match &tokens[i] {
            PdfToken::DictStart => {
                stack.push(DictFrame::default());
                i += 1;
            }
            PdfToken::DictEnd => {
                if let Some(frame) = stack.pop() {
                    if frame.has_type_pages {
                        if let Some(c) = frame.count {
                            best = best.max(c);
                        }
                    }
                }
                i += 1;
            }
            PdfToken::ArrayStart => {
                if let Some(top) = stack.last_mut() {
                    top.array_depth += 1;
                }
                i += 1;
            }
            PdfToken::ArrayEnd => {
                if let Some(top) = stack.last_mut() {
                    top.array_depth = top.array_depth.saturating_sub(1);
                }
                i += 1;
            }
            PdfToken::Name(key) => {
                let at_dict_top = stack.last().map(|f| f.array_depth == 0).unwrap_or(false);
                if at_dict_top && i + 1 < tokens.len() {
                    if key == b"Type" {
                        if let PdfToken::Name(v) = &tokens[i + 1] {
                            if v == b"Pages" {
                                if let Some(top) = stack.last_mut() {
                                    top.has_type_pages = true;
                                }
                            }
                            i += 2;
                            continue;
                        }
                    } else if key == b"Count" && !is_indirect_ref(tokens, i + 1) {
                        if let Some(c) = is_unsigned_digits(&tokens[i + 1]) {
                            if let Some(top) = stack.last_mut() {
                                top.count = Some(c);
                            }
                            i += 2;
                            continue;
                        }
                    }
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    best
}

#[inline]
fn is_inline_dict_printable_or_ws(b: u8) -> bool {
    (0x20..=0x7E).contains(&b) || matches!(b, 0x09 | 0x0A | 0x0C | 0x0D)
}

fn is_inline_image_key(name: &[u8]) -> bool {
    INLINE_IMAGE_KEYS.contains(&name)
}

/// Linear-time `O(N)` scanner for PDF inline raster images (`BI .. ID<ws> .. EI`).
///
/// Requires:
/// - `BI` outside comments and literal/hex strings;
/// - `BI..ID` dictionary span `<= 8192` bytes containing only printable ASCII + PDF whitespace;
/// - at least one recognized `/` inline-image dictionary key (`/W`, `/Width`, `/H`, `/Height`,
///   `/BPC`, `/BitsPerComponent`, `/CS`, `/ColorSpace`, `/IM`, `/ImageMask`, `/F`, `/Filter`,
///   `/D`, `/Decode`, `/I`, `/Interpolate`);
/// - followed by `ID<ws>` and a token-bounded `EI` scanned in raw pixel bytes (so pixel bytes
///   like `0x28` `'('` cannot swallow `EI`).
fn contains_inline_image(blob: &[u8]) -> bool {
    let n = blob.len();
    let mut i = 0usize;
    let mut active_bi: Option<usize> = None;
    let mut has_inline_key = false;

    while i < n {
        if let Some(bi_start) = active_bi {
            if i.saturating_sub(bi_start + 2) > MAX_INLINE_IMAGE_DICT_BYTES {
                active_bi = None;
                has_inline_key = false;
            }
        }

        let b = blob[i];

        if b == b'%' {
            i = skip_comment(blob, i);
            continue;
        }
        if b == b'(' {
            i = skip_literal_string(blob, i);
            active_bi = None;
            has_inline_key = false;
            continue;
        }
        if b == b'<' {
            if i + 1 < n && blob[i + 1] == b'<' {
                i += 2;
                continue;
            }
            let end = skip_hex_string(blob, i);
            if active_bi.is_some()
                && !blob[i..end]
                    .iter()
                    .all(|&c| is_inline_dict_printable_or_ws(c))
            {
                active_bi = None;
                has_inline_key = false;
            }
            i = end;
            continue;
        }

        if active_bi.is_some() && !is_inline_dict_printable_or_ws(b) {
            active_bi = None;
            has_inline_key = false;
            i += 1;
            continue;
        }

        if b == b'/' {
            let start = i + 1;
            i = start;
            while i < n && !is_pdf_boundary(blob[i]) {
                i += 1;
            }
            if active_bi.is_some() {
                if !blob[start..i]
                    .iter()
                    .all(|&c| is_inline_dict_printable_or_ws(c))
                {
                    active_bi = None;
                    has_inline_key = false;
                } else {
                    let decoded = decode_pdf_name(&blob[start..i]);
                    if is_inline_image_key(&decoded) {
                        has_inline_key = true;
                    }
                }
            }
            continue;
        }

        // Token-bounded `BI`
        if b == b'B'
            && i + 2 < n
            && blob[i + 1] == b'I'
            && (i == 0 || is_pdf_boundary(blob[i - 1]))
            && (is_pdf_ws(blob[i + 2]) || blob[i + 2] == b'/')
        {
            active_bi = Some(i);
            has_inline_key = false;
            i += 2;
            continue;
        }

        // Token-bounded `ID<ws>` closing an active valid inline-image dictionary
        if active_bi.is_some()
            && has_inline_key
            && b == b'I'
            && i + 2 < n
            && blob[i + 1] == b'D'
            && (i == 0 || is_pdf_boundary(blob[i - 1]))
            && is_pdf_ws(blob[i + 2])
        {
            let search_start = i + 3;
            let mut k = search_start;
            while k + 2 <= n {
                if blob[k] == b'E'
                    && blob[k + 1] == b'I'
                    && (k == search_start || is_pdf_boundary(blob[k - 1]))
                    && (k + 2 == n || is_pdf_boundary(blob[k + 2]))
                {
                    return true;
                }
                k += 1;
            }
            // No token-bounded `EI` exists anywhere in the remainder of the stream.
            return false;
        }

        i += 1;
    }

    false
}

/// Extract decoded PDF literal strings `(...)` (respecting `% ...` comments outside strings,
/// nested parentheses, and ISO 32000-1 §7.3.4.2 backslash / octal escapes) as Latin-1 text.
fn extract_pdf_literal_strings(data: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    let n = data.len();
    let mut i = 0usize;

    while i < n {
        if data[i] == b'%' {
            i = skip_comment(data, i);
            continue;
        }
        if data[i] == b'(' {
            let mut depth = 1usize;
            i += 1;
            let mut buf: Vec<u8> = Vec::new();
            while i < n && depth > 0 {
                let c = data[i];
                if c == b'\\' && i + 1 < n {
                    let next = data[i + 1];
                    match next {
                        b'n' => {
                            buf.push(b'\n');
                            i += 2;
                        }
                        b'r' => {
                            buf.push(b'\r');
                            i += 2;
                        }
                        b't' => {
                            buf.push(b'\t');
                            i += 2;
                        }
                        b'b' => {
                            buf.push(0x08);
                            i += 2;
                        }
                        b'f' => {
                            buf.push(0x0C);
                            i += 2;
                        }
                        b'\r' => {
                            i += if i + 2 < n && data[i + 2] == b'\n' {
                                3
                            } else {
                                2
                            };
                        }
                        b'\n' => {
                            i += 2;
                        }
                        b'0'..=b'7' => {
                            let mut val = u16::from(next - b'0');
                            let mut consumed = 2usize;
                            if i + 2 < n && (b'0'..=b'7').contains(&data[i + 2]) {
                                val = (val << 3) | u16::from(data[i + 2] - b'0');
                                consumed = 3;
                                if i + 3 < n && (b'0'..=b'7').contains(&data[i + 3]) {
                                    val = (val << 3) | u16::from(data[i + 3] - b'0');
                                    consumed = 4;
                                }
                            }
                            buf.push((val & 0xFF) as u8);
                            i += consumed;
                        }
                        other => {
                            buf.push(other);
                            i += 2;
                        }
                    }
                    continue;
                }
                if c == b'(' {
                    depth += 1;
                    buf.push(c);
                } else if c == b')' {
                    depth -= 1;
                    if depth > 0 {
                        buf.push(c);
                    }
                } else {
                    buf.push(c);
                }
                i += 1;
            }
            let s: String = buf.into_iter().map(char::from).collect();
            found.push(s);
        } else {
            i += 1;
        }
    }

    found
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }
    let first = needle[0];
    let limit = haystack.len() - needle.len();
    let mut i = 0usize;
    while i <= limit {
        if haystack[i] == first && &haystack[i..i + needle.len()] == needle {
            return true;
        }
        i += 1;
    }
    false
}

// ============================================================================
// Public Audit Options, Report, File I/O, and Entry Points
// ============================================================================

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PdfAuditOptions {
    pub min_pages: usize,
    pub compare_pdf: Option<PathBuf>,
}

impl Default for PdfAuditOptions {
    fn default() -> Self {
        Self {
            min_pages: 1,
            compare_pdf: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PdfAuditReport {
    pub schema: String,
    pub status: String,
    pub bytes: usize,
    pub sha256: String,
    pub compare_sha256: Option<String>,
    pub deterministic_match: Option<bool>,
    pub pages: usize,
    pub min_pages: usize,
    pub streams_total: usize,
    pub streams_flate_inflated: usize,
    pub raster_violations: usize,
    pub unresolved_violations: usize,
    pub errors: Vec<String>,
}

impl PdfAuditReport {
    pub fn is_ok(&self) -> bool {
        self.status == "PASS" && self.errors.is_empty()
    }

    /// Serialize to strictly deterministic, host-path-free, zero-timestamp, zero-float
    /// `aod.dec.pdf.audit.v1` JSON (`BTreeMap` sorted keys).
    pub fn to_json(&self) -> JsonValue {
        let mut m = BTreeMap::new();
        m.insert("bytes".to_string(), JsonValue::from_usize(self.bytes));
        m.insert(
            "compare_sha256".to_string(),
            match &self.compare_sha256 {
                Some(s) => JsonValue::from_str_val(s.clone()),
                None => JsonValue::Null,
            },
        );
        m.insert(
            "deterministic_match".to_string(),
            match self.deterministic_match {
                Some(b) => JsonValue::Bool(b),
                None => JsonValue::Null,
            },
        );
        m.insert(
            "errors".to_string(),
            JsonValue::Array(
                self.errors
                    .iter()
                    .map(|e| JsonValue::from_str_val(e.clone()))
                    .collect(),
            ),
        );
        m.insert(
            "min_pages".to_string(),
            JsonValue::from_usize(self.min_pages),
        );
        m.insert("pages".to_string(), JsonValue::from_usize(self.pages));
        m.insert(
            "raster_violations".to_string(),
            JsonValue::from_usize(self.raster_violations),
        );
        m.insert(
            "schema".to_string(),
            JsonValue::from_str_val(self.schema.clone()),
        );
        m.insert(
            "sha256".to_string(),
            JsonValue::from_str_val(self.sha256.clone()),
        );
        m.insert(
            "status".to_string(),
            JsonValue::from_str_val(self.status.clone()),
        );
        m.insert(
            "streams_flate_inflated".to_string(),
            JsonValue::from_usize(self.streams_flate_inflated),
        );
        m.insert(
            "streams_total".to_string(),
            JsonValue::from_usize(self.streams_total),
        );
        m.insert(
            "unresolved_violations".to_string(),
            JsonValue::from_usize(self.unresolved_violations),
        );
        JsonValue::Object(m)
    }
}

fn read_bounded_regular_file(path: &Path, tag: &str) -> Result<Vec<u8>, String> {
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| format!("{tag}: failed inspecting file ({:?})", e.kind()))?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        return Err(format!("{tag}: symlink rejected"));
    }
    if !ft.is_file() {
        return Err(format!("{tag}: not a regular file"));
    }
    if meta.len() > MAX_PDF_BYTES as u64 {
        return Err(format!(
            "{tag}: file exceeds maximum size limit of {MAX_PDF_BYTES} bytes"
        ));
    }

    let file = File::open(path)
        .map_err(|e| format!("{tag}: failed opening file ({:?})", e.kind()))?;
    let open_meta = file
        .metadata()
        .map_err(|e| format!("{tag}: failed reading file metadata ({:?})", e.kind()))?;
    if !open_meta.is_file() {
        return Err(format!("{tag}: not a regular file"));
    }

    let mut buf = Vec::new();
    file.take((MAX_PDF_BYTES + 1) as u64)
        .read_to_end(&mut buf)
        .map_err(|e| format!("{tag}: failed reading file ({:?})", e.kind()))?;
    if buf.len() > MAX_PDF_BYTES {
        return Err(format!(
            "{tag}: file exceeds maximum size limit of {MAX_PDF_BYTES} bytes"
        ));
    }
    Ok(buf)
}

/// Audit in-memory PDF bytes (with optional comparison PDF bytes for byte-identical determinism).
pub fn audit_pdf_bytes(
    raw: &[u8],
    min_pages: usize,
    compare_raw: Option<&[u8]>,
) -> PdfAuditReport {
    let mut errors: Vec<String> = Vec::new();

    if !raw.starts_with(b"%PDF-") {
        errors.push("pdf: missing %PDF- header".to_string());
    }
    if !raw.trim_ascii_end().ends_with(b"%%EOF") {
        errors.push("pdf: missing %%EOF trailer".to_string());
    }

    let sha256 = Sha256::digest_hex(raw);
    let (root_structure, extracted_streams, framing_errors) = extract_streams_and_mask_root(raw);
    errors.extend(framing_errors);

    let streams_total = extracted_streams.len();
    let mut streams_flate_inflated = 0usize;
    let mut total_decomp_bytes = 0usize;

    struct InspectedStream {
        index: usize,
        dict_info: StreamDictInfo,
        payload: Vec<u8>,
    }

    let mut inspected: Vec<InspectedStream> = Vec::with_capacity(streams_total);

    for stream in extracted_streams {
        match &stream.dict_info.filter {
            FilterSpec::None => {
                let slice = if stream.is_direct_length {
                    stream.raw_slice
                } else {
                    strip_single_trailing_eol(stream.raw_slice)
                };
                inspected.push(InspectedStream {
                    index: stream.index,
                    dict_info: stream.dict_info,
                    payload: slice.to_vec(),
                });
            }
            FilterSpec::FlateDecodeOnly => {
                let remaining = MAX_TOTAL_DECOMP_BYTES.saturating_sub(total_decomp_bytes);
                let cap = MAX_STREAM_DECOMP_BYTES.min(remaining);
                let inflated = if stream.is_direct_length {
                    inflate_zlib(stream.raw_slice, cap)
                } else {
                    inflate_indirect_flate(stream.raw_slice, cap)
                };
                match inflated {
                    Ok(dec) => {
                        streams_flate_inflated += 1;
                        total_decomp_bytes = total_decomp_bytes.saturating_add(dec.len());
                        inspected.push(InspectedStream {
                            index: stream.index,
                            dict_info: stream.dict_info,
                            payload: dec,
                        });
                    }
                    Err(e) => {
                        errors.push(format!(
                            "stream[{}]: /FlateDecode inflation failed: {e}",
                            stream.index
                        ));
                    }
                }
            }
            FilterSpec::Unsupported(desc) => {
                errors.push(format!(
                    "stream[{}]: unsupported/disallowed stream /Filter {desc}",
                    stream.index
                ));
            }
        }
    }

    // Raster audit + page counting
    let mut raster_hits: Vec<String> = Vec::new();
    let root_tokens = tokenize_pdf(&root_structure);
    if has_subtype_image(&root_tokens) {
        raster_hits.push("root: /Subtype /Image XObject detected".to_string());
    }

    let mut page_obj_count = count_page_objects(&root_tokens);
    let mut best_pages_count = max_pages_tree_count(&root_tokens);

    for stream in &inspected {
        let tag = format!("stream[{}]", stream.index);
        let skip_binary_heuristics = is_binary_font_or_xref(&stream.dict_info);
        let tokens = tokenize_pdf(&stream.payload);
        if has_subtype_image(&tokens) {
            raster_hits.push(format!("{tag}: /Subtype /Image XObject detected"));
        }
        if contains_inline_image(&stream.payload) {
            raster_hits.push(format!("{tag}: inline raster image BI..ID..EI detected"));
        }
        if !skip_binary_heuristics {
            page_obj_count += count_page_objects(&tokens);
            best_pages_count = best_pages_count.max(max_pages_tree_count(&tokens));
        }
    }

    for hit in &raster_hits {
        errors.push(format!("raster violation: {hit}"));
    }

    // Unresolved-reference marker audit
    let mut unresolved_hits: Vec<String> = Vec::new();
    {
        let mut seen_root: BTreeSet<&'static str> = BTreeSet::new();
        for lit in extract_pdf_literal_strings(&root_structure) {
            for marker in UNRESOLVED_MARKERS {
                if lit.contains(marker) && seen_root.insert(marker) {
                    unresolved_hits.push(format!("root: unresolved marker {marker:?} in text"));
                }
            }
            if lit.contains("??") && seen_root.insert("??") {
                unresolved_hits.push("root: unresolved marker \"??\" in text".to_string());
            }
        }
    }

    for stream in &inspected {
        let tag = format!("stream[{}]", stream.index);
        let skip_two_byte_q = is_binary_font_or_xref(&stream.dict_info);
        let mut seen_stream: BTreeSet<&'static str> = BTreeSet::new();

        for lit in extract_pdf_literal_strings(&stream.payload) {
            for marker in UNRESOLVED_MARKERS {
                if lit.contains(marker) && seen_stream.insert(marker) {
                    unresolved_hits.push(format!("{tag}: unresolved marker {marker:?} in text"));
                }
            }
            if !skip_two_byte_q && lit.contains("??") && seen_stream.insert("??") {
                unresolved_hits.push(format!("{tag}: unresolved marker \"??\" in text"));
            }
        }

        for marker in UNRESOLVED_MARKERS {
            if !seen_stream.contains(marker)
                && contains_subslice(&stream.payload, marker.as_bytes())
            {
                seen_stream.insert(marker);
                unresolved_hits.push(format!("{tag}: unresolved marker {marker:?}"));
            }
        }
    }

    for hit in &unresolved_hits {
        errors.push(format!("unresolved reference violation: {hit}"));
    }

    let pages = if page_obj_count > 0 {
        page_obj_count
    } else {
        best_pages_count
    };
    if pages < min_pages {
        errors.push(format!(
            "pdf: page count {pages} < required --min-pages {min_pages}"
        ));
    }

    let (compare_sha256, deterministic_match) = match compare_raw {
        Some(cmp) => {
            let cmp_sha = Sha256::digest_hex(cmp);
            let matched = cmp_sha == sha256;
            if !matched {
                errors.push(format!(
                    "compare_pdf: non-deterministic PDF build ({sha256} != {cmp_sha})"
                ));
            }
            (Some(cmp_sha), Some(matched))
        }
        None => (None, None),
    };

    let status = if errors.is_empty() { "PASS" } else { "FAIL" }.to_string();

    PdfAuditReport {
        schema: PDF_AUDIT_SCHEMA.to_string(),
        status,
        bytes: raw.len(),
        sha256,
        compare_sha256,
        deterministic_match,
        pages,
        min_pages,
        streams_total,
        streams_flate_inflated,
        raster_violations: raster_hits.len(),
        unresolved_violations: unresolved_hits.len(),
        errors,
    }
}

/// Audit a PDF file on disk (and optional `--compare-pdf`), enforcing symmetric symlink rejection
/// and `MAX_PDF_BYTES` bounded reads on both inputs with zero host paths in the returned report.
pub fn audit_pdf_file(pdf_path: &Path, options: &PdfAuditOptions) -> PdfAuditReport {
    let primary = read_bounded_regular_file(pdf_path, "pdf");
    let compare = options
        .compare_pdf
        .as_deref()
        .map(|p| read_bounded_regular_file(p, "compare_pdf"));

    match primary {
        Ok(raw) => match compare {
            None => audit_pdf_bytes(&raw, options.min_pages, None),
            Some(Ok(cmp_raw)) => audit_pdf_bytes(&raw, options.min_pages, Some(&cmp_raw)),
            Some(Err(cmp_err)) => {
                let mut report = audit_pdf_bytes(&raw, options.min_pages, None);
                report.deterministic_match = Some(false);
                report.errors.push(cmp_err);
                report.status = "FAIL".to_string();
                report
            }
        },
        Err(pdf_err) => {
            let mut errors = vec![pdf_err];
            let (compare_sha256, deterministic_match) = match compare {
                None => (None, None),
                Some(Ok(cmp_raw)) => (Some(Sha256::digest_hex(&cmp_raw)), Some(false)),
                Some(Err(cmp_err)) => {
                    errors.push(cmp_err);
                    (None, Some(false))
                }
            };
            PdfAuditReport {
                schema: PDF_AUDIT_SCHEMA.to_string(),
                status: "FAIL".to_string(),
                bytes: 0,
                sha256: String::new(),
                compare_sha256,
                deterministic_match,
                pages: 0,
                min_pages: options.min_pages,
                streams_total: 0,
                streams_flate_inflated: 0,
                raster_violations: 0,
                unresolved_violations: 0,
                errors,
            }
        }
    }
}
