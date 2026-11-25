use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use aod_core::bigint::BigInt;
use aod_core::json::{parse_strict_json, JsonValue};
use aodc::{
    adler32, audit_pdf_bytes, audit_pdf_file, inflate_deflate, inflate_zlib, InflateError,
    PdfAuditOptions, MAX_PDF_BYTES, MAX_STREAMS, MAX_STREAM_DECOMP_BYTES, PDF_AUDIT_SCHEMA,
};

static PDF_TEST_SEQ: AtomicU64 = AtomicU64::new(0);

// Authentic RFC 1950 zlib BTYPE=10 (dynamic Huffman) golden vectors.
const GOLDEN_BTYPE10_CLEAN_CONTENT_HEX: &str = "\
78daed91cb4ec3301045f77cc565078bd2ba4220b1035a2416509566c5064d9d\
496ae4476a3b6df3f74c2236d05f886c69461ecf99c7dd43612647e17e2e7706\
edf05460faa2a0148a6a081625ae1e6db3a3c9ca714d58749e9cd1090bd638cc\
f14ed91c181f5c7164af196fe45bb2d8b0ce2678a81b5ca3f816d244dd0eb4e5\
897486f1996b8e205f2252ff5592289abc739c8dc651bcd066543648d4d76882\
a49cc19e836b8ce549368e5199c847b236817d15a2eedba272e56d87b04d1c0f\
1cd35070fdc5a78c7d4b91bcc0f90cfbc931485b294b87c651cde901d34dbbcd\
5dc398bef62f300952646bca92bd8c832686c4c842befc475bb0709cf126f593\
1d643321a2e62093c6ee6158b3839af7d60ef6ae777ecd0e9b01b72cb0bed88f\
928d928d928d928d92fd95ec07d61f21e3";

const GOLDEN_BTYPE10_OBJSTM_HIDDEN_IMAGE_HEX: &str = "\
78daed90cb4ec3301045f77cc56c90608192b4f425559568431fd0d292f40588\
85e34c1297c40e8ed3367c3d4e2ad405e20f90acd1783cc7f7ce5826986059d0\
d6b10656d3846e178c659122180b125651225750d37d0e1833f419e98b23bc95\
605333ad4eed1d0c0733914b8a59c50f8526aac4824605f67aa77371fe7e4014\
894578d2c9ce028af8fa05eaa7fb94f010ae90dfacdc6b30d60c0f28171203d4\
ae7ee46c96a531296c41974cc5084ae6f84b6e3bf7764815186eeea9aa3249aa\
0137cc57911e1d8c31b23052553a10b1906e4aa86eb071cf283aa33e187da6b2\
05ca814852c1cbb5b44b914b4889ef336d948a2429cb4a4020f43a40321a4156\
249e88c1679992cccb15131c1807bfe0246114a23c0812c2b56d4430ad5afdb6\
d16cb53b70d71fd8f7c3d178f2f0389d3dcd17cf8ebb5cad37db9757201ef531\
0823b6fb88132ed24f99a97c7f38165fff66fe34f30d21cdf2ec";

// Dynamic Huffman (BTYPE=10), Adler-32 = 0x7778510a (ends in 0x0A), AND compressed ciphertext
// contains 2 unmatched 0x28 '(' bytes!
const GOLDEN_BTYPE10_ADLER_0A_HEX: &str = "\
78daed90cd4ec3301084ef7d8ab920c1215047880a240e40cb0daad2dcabc5d9\
2446fe49366edabe3d4ec4ad6f802a5b9a95d6fbed783a28ccd35158e4e9cea1\
1d5e0bdcbd2b2885a29a9a4589eb17db3694ad1dd784e5c99333bac79235861c\
9f14cdc0f8e28a85bd667c90df93c5967534c143dde206c54f2265ea7ea2ad8e\
a4238c8f5cb3807c09a1f1691a2231b1711c8dc62155611f51d990babe461bd2\
c819ec2db8d658cea2718cca081fc8da1eecab207ab445e5dadb13c277cf32b0\
f4d3c2cd8e8f11dd9e847c82f31976c991c5196ffad1cb90fe12043587e44d4e\
4f53300e2a1fd54efa30167fd2603be1560536b3ee12f225e47f11f215a8b42c\
bb96ca67355f3cce7e017778510a";

// Dynamic Huffman (BTYPE=10), Adler-32 = 0x7782510d (ends in 0x0D).
const GOLDEN_BTYPE10_ADLER_0D_HEX: &str = "\
78daed90cd4ec3301084ef7d8ab920c12150570854240e40cb0daad2dcabc5d9\
2446fe49376edabe3d8ec5ad6f802a5b9a95d6fbed78765098a6a3f0384b770a\
edf05ae2ee5d412994756e9615ae5f6cd752b172dc1016274fcee81e0bd61866\
f8a46806c617d72cec35e383fc9e2c36aca3091eea1637287f12a950f799b63c\
928e303e72c302f21584c6a76988c4c4d671341a8754857d446d43eafa065d48\
2367b0b7e03a63b988c6316a237c206b7bb0af83e8d116552b6f4f08df3dcbc0\
d2e785eb2d1f23767b12f209ce67d805471667bce9472f43fa4b10341c923739\
3de5601cd46c549bf5612cfea4c526e39625d693dd25e44bc8ff22e42b506559\
b61d55cf4acde7935f7782510d";

fn decode_hex(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    assert_eq!(bytes.len() % 2, 0);
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks(2) {
        let hi = (pair[0] as char).to_digit(16).unwrap() as u8;
        let lo = (pair[1] as char).to_digit(16).unwrap() as u8;
        out.push((hi << 4) | lo);
    }
    out
}

fn aodc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aodc"))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

struct TempDirGuard(PathBuf);

impl TempDirGuard {
    fn new(tag: &str) -> Self {
        let seq = PDF_TEST_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "aodc_pdf_audit_{}_{}_{tag}",
            std::process::id(),
            seq
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Pure-Rust LSB-first bit writer for crafting exact RFC 1950/1951 test vectors.
struct BitWriter {
    out: Vec<u8>,
    cur: u8,
    bits: u8,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            cur: 0,
            bits: 0,
        }
    }

    fn write_bits(&mut self, val: u32, count: u8) {
        for i in 0..count {
            let bit = ((val >> i) & 1) as u8;
            self.cur |= bit << self.bits;
            self.bits += 1;
            if self.bits == 8 {
                self.out.push(self.cur);
                self.cur = 0;
                self.bits = 0;
            }
        }
    }

    /// Write a canonical Huffman code MSB-first (as required by RFC 1951 §3.1.1).
    fn write_huff(&mut self, code: u32, len: u8) {
        for i in (0..len).rev() {
            let bit = (code >> i) & 1;
            self.write_bits(bit, 1);
        }
    }

    fn align_byte(&mut self) {
        if self.bits > 0 {
            self.out.push(self.cur);
            self.cur = 0;
            self.bits = 0;
        }
    }

    fn write_bytes_aligned(&mut self, data: &[u8]) {
        self.align_byte();
        self.out.extend_from_slice(data);
    }

    fn write_fixed_sym(&mut self, sym: u16) {
        match sym {
            0..=143 => self.write_huff(0b0011_0000 + u32::from(sym), 8),
            144..=255 => self.write_huff(0b1_1001_0000 + u32::from(sym - 144), 9),
            256..=279 => self.write_huff(u32::from(sym - 256), 7),
            280..=287 => self.write_huff(0b1100_0000 + u32::from(sym - 280), 8),
            _ => panic!("invalid fixed sym {sym}"),
        }
    }

    fn finish(mut self) -> Vec<u8> {
        self.align_byte();
        self.out
    }
}

fn zlib_wrap(cmf: u8, flg: u8, deflate_body: &[u8], checksum: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + deflate_body.len() + 4);
    out.push(cmf);
    out.push(flg);
    out.extend_from_slice(deflate_body);
    out.extend_from_slice(&checksum.to_be_bytes());
    out
}

fn zlib_stored(payload: &[u8]) -> Vec<u8> {
    let mut bw = BitWriter::new();
    if payload.is_empty() {
        bw.write_bits(1, 1); // BFINAL=1
        bw.write_bits(0, 2); // BTYPE=00
        bw.align_byte();
        bw.write_bytes_aligned(&0u16.to_le_bytes());
        bw.write_bytes_aligned(&(!0u16).to_le_bytes());
    } else {
        let chunks: Vec<&[u8]> = payload.chunks(65535).collect();
        for (i, chunk) in chunks.iter().enumerate() {
            let bfinal = u32::from(i + 1 == chunks.len());
            bw.write_bits(bfinal, 1);
            bw.write_bits(0, 2);
            bw.align_byte();
            let len = chunk.len() as u16;
            bw.write_bytes_aligned(&len.to_le_bytes());
            bw.write_bytes_aligned(&(!len).to_le_bytes());
            bw.write_bytes_aligned(chunk);
        }
    }
    zlib_wrap(0x78, 0x01, &bw.finish(), adler32(payload))
}

fn zlib_fixed_literals(payload: &[u8]) -> Vec<u8> {
    let mut bw = BitWriter::new();
    bw.write_bits(1, 1); // BFINAL=1
    bw.write_bits(1, 2); // BTYPE=01
    for &b in payload {
        bw.write_fixed_sym(u16::from(b));
    }
    bw.write_fixed_sym(256); // EOB
    zlib_wrap(0x78, 0x9C, &bw.finish(), adler32(payload))
}

/// Build a tiny DEFLATE zip-bomb expanding to `target_bytes` using length=258 (code 285, 0 extra
/// bits) and distance=1 (code 0, 0 extra bits) inside a fixed Huffman block.
fn zlib_zip_bomb(target_bytes: usize) -> Vec<u8> {
    assert!(target_bytes >= 1);
    let mut bw = BitWriter::new();
    bw.write_bits(1, 1);
    bw.write_bits(1, 2);
    bw.write_fixed_sym(u16::from(b'A'));
    let mut produced = 1usize;
    while produced + 258 <= target_bytes {
        bw.write_fixed_sym(285); // length 258, 0 extra bits
        bw.write_huff(0, 5); // dist code 0 -> distance 1
        produced += 258;
    }
    while produced < target_bytes {
        bw.write_fixed_sym(u16::from(b'A'));
        produced += 1;
    }
    bw.write_fixed_sym(256);
    // Compute Adler-32 of `target_bytes` copies of b'A' without allocating `target_bytes`!
    const MOD_ADLER: u64 = 65521;
    let n = target_bytes as u64;
    let a = u64::from(b'A');
    let s1 = (1 + (n % MOD_ADLER) * a) % MOD_ADLER;
    let n_mod = n % MOD_ADLER;
    let tri = (n_mod * (n_mod + 1) / 2) % MOD_ADLER;
    let s2 = (n_mod + tri * a) % MOD_ADLER;
    let adler = ((s2 as u32) << 16) | (s1 as u32);
    zlib_wrap(0x78, 0x9C, &bw.finish(), adler)
}

/// Build a dynamic Huffman block (`BTYPE=10`) with custom literal/length and distance code-length
/// tables (encoded using a complete 2-symbol code-length tree over symbols 0, 1, 2, 17, 18).
///
/// Specifically constructs a valid dynamic block for testing empty distance tree, 1-symbol trees,
/// and multi-block dynamic decoding.
fn write_simple_dynamic_block(
    bw: &mut BitWriter,
    bfinal: u32,
    litlen_lens: &[u8],
    dist_lens: &[u8],
    write_body: impl FnOnce(&mut BitWriter),
) {
    assert!(litlen_lens.len() >= 257 && litlen_lens.len() <= 286);
    assert!(!dist_lens.is_empty() && dist_lens.len() <= 30);

    bw.write_bits(bfinal, 1);
    bw.write_bits(2, 2); // BTYPE=10
    bw.write_bits((litlen_lens.len() - 257) as u32, 5);
    bw.write_bits((dist_lens.len() - 1) as u32, 5);

    // Use a complete 16-symbol code-length tree of uniform length 4 covering CL symbols 0..=15!
    // Wait: CL_ORDER has 19 slots; assign length 4 to all 16 symbols 0..=15 -> 16 * 2^-4 = 1.0 (complete!).
    // CL symbols 16, 17, 18 get length 0.
    // All 19 slots emitted (hclen = 19 -> write 15 in 4 bits).
    const CL_ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    bw.write_bits(15, 4); // hclen = 19
    for &slot in &CL_ORDER {
        let cl = if slot <= 15 { 4u32 } else { 0u32 };
        bw.write_bits(cl, 3);
    }
    // Canonical 4-bit codes for symbols 0..=15 are literally 0..=15!
    for &l in litlen_lens {
        assert!(l <= 15);
        bw.write_huff(u32::from(l), 4);
    }
    for &l in dist_lens {
        assert!(l <= 15);
        bw.write_huff(u32::from(l), 4);
    }
    write_body(bw);
}

fn make_synthetic_pdf(
    pages: usize,
    extra_uncompressed: &[u8],
    streams: &[(&[u8], &[u8], bool)], // (dict_inner, raw_payload, use_direct_length)
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n");
    let mut obj_id = 1usize;
    for _ in 0..pages {
        out.extend_from_slice(
            format!(
                "{obj_id} 0 obj\n<< /Type /Page /Parent 99 0 R /Resources << /ProcSet [/PDF /Text] >> >>\nendobj\n"
            )
            .as_bytes(),
        );
        obj_id += 1;
    }
    if !extra_uncompressed.is_empty() {
        out.extend_from_slice(format!("{obj_id} 0 obj\n").as_bytes());
        out.extend_from_slice(extra_uncompressed);
        out.extend_from_slice(b"\nendobj\n");
        obj_id += 1;
    }
    for (dict_inner, payload, use_direct_len) in streams {
        out.extend_from_slice(format!("{obj_id} 0 obj\n<< ").as_bytes());
        out.extend_from_slice(dict_inner);
        if *use_direct_len {
            out.extend_from_slice(format!(" /Length {} >>\nstream\n", payload.len()).as_bytes());
        } else {
            out.extend_from_slice(b" /Length 999 0 R >>\nstream\n");
        }
        out.extend_from_slice(payload);
        out.extend_from_slice(b"\nendstream\nendobj\n");
        obj_id += 1;
    }
    out.extend_from_slice(b"%%EOF\n");
    out
}

fn assert_portable_pdf_json(text: &str) -> JsonValue {
    let parsed = parse_strict_json(text)
        .unwrap_or_else(|e| panic!("expected strict zero-float JSON, got error {e}: {text}"));
    for forbidden in ["/home/", "/tmp/", "/var/", "/Users/", "T00:", "T01:", "T02:"] {
        assert!(
            !text.contains(forbidden),
            "host path or timestamp token {forbidden:?} leaked into JSON:\n{text}"
        );
    }
    let obj = parsed.as_object().expect("top-level JSON must be object");
    let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        vec![
            "bytes",
            "compare_sha256",
            "deterministic_match",
            "errors",
            "min_pages",
            "pages",
            "raster_violations",
            "schema",
            "sha256",
            "status",
            "streams_flate_inflated",
            "streams_total",
            "unresolved_violations",
        ],
        "aod.dec.pdf.audit.v1 key set mismatch"
    );
    assert_eq!(
        parsed.get("schema").and_then(JsonValue::as_str),
        Some(PDF_AUDIT_SCHEMA)
    );
    parsed
}

// ============================================================================
// AC-02 & P0-4: RFC 1950/1951 Inflater Tests (Stored, Fixed, Dynamic, Rewind)
// ============================================================================

#[test]
fn rfc1950_rfc1951_stored_fixed_dynamic_golden_vectors_and_multiblock_rewind() {
    // 1. Stored blocks (BTYPE=00): empty, short, and > 65535 multi-chunk
    assert_eq!(inflate_zlib(&zlib_stored(b""), 1024).unwrap(), b"");
    let short = b"Hello stored RFC 1951 block!";
    assert_eq!(inflate_zlib(&zlib_stored(short), 1024).unwrap(), short);
    let big_stored: Vec<u8> = (0..70_000usize).map(|i| ((i * 31) & 0xFF) as u8).collect();
    assert_eq!(
        inflate_zlib(&zlib_stored(&big_stored), 100_000).unwrap(),
        big_stored
    );

    // 2. Fixed Huffman (BTYPE=01) + overlapping RLE with length code 285 (= 258, 0 extra bits)
    let fixed_msg = b"BT /F1 11 Tf (Exact integer arithmetic) Tj ET";
    assert_eq!(
        inflate_zlib(&zlib_fixed_literals(fixed_msg), 1024).unwrap(),
        fixed_msg
    );
    let rle_bomb_small = zlib_zip_bomb(1033); // 1 + 4*258 = 1033
    assert_eq!(
        inflate_zlib(&rle_bomb_small, 2048).unwrap(),
        vec![b'A'; 1033]
    );

    // 3. Real verified BTYPE=10 (dynamic Huffman) golden vectors!
    let z1 = decode_hex(GOLDEN_BTYPE10_CLEAN_CONTENT_HEX);
    assert_eq!((z1[2] >> 1) & 3, 2, "z1 must be dynamic Huffman BTYPE=10");
    let d1 = inflate_zlib(&z1, MAX_STREAM_DECOMP_BYTES).unwrap();
    assert!(d1.starts_with(b"q 1 0 0 1 72 720 cm BT /F1 11 Tf"));
    assert!(String::from_utf8_lossy(&d1).contains("/Subtype /Image is forbidden in prose text!"));

    let z2 = decode_hex(GOLDEN_BTYPE10_OBJSTM_HIDDEN_IMAGE_HEX);
    assert_eq!((z2[2] >> 1) & 3, 2, "z2 must be dynamic Huffman BTYPE=10");
    let d2 = inflate_zlib(&z2, MAX_STREAM_DECOMP_BYTES).unwrap();
    assert!(String::from_utf8_lossy(&d2).contains("/Subtype /Image"));

    let z_0a = decode_hex(GOLDEN_BTYPE10_ADLER_0A_HEX);
    assert_eq!((z_0a[2] >> 1) & 3, 2, "z_0a must be dynamic Huffman BTYPE=10");
    assert_eq!(*z_0a.last().unwrap(), 0x0A);
    assert!(z_0a.contains(&0x28));
    let d_0a = inflate_zlib(&z_0a, MAX_STREAM_DECOMP_BYTES).unwrap();
    assert!(d_0a.ends_with(b"% adler_pad=1079\n"));

    let z_0d = decode_hex(GOLDEN_BTYPE10_ADLER_0D_HEX);
    assert_eq!((z_0d[2] >> 1) & 3, 2, "z_0d must be dynamic Huffman BTYPE=10");
    assert_eq!(*z_0d.last().unwrap(), 0x0D);
    let d_0d = inflate_zlib(&z_0d, MAX_STREAM_DECOMP_BYTES).unwrap();
    assert!(d_0d.ends_with(b"% adler_pad=1199\n"));

    // 4. Multi-block mixed 01 + 00 + 10 stream testing BitReader alignment & rewind across
    //    fixed -> stored -> dynamic -> Adler-32 trailer boundaries!
    let mut bw = BitWriter::new();
    // Block 1: BFINAL=0, BTYPE=01 (fixed Huffman): emit "AB", EOB (3 + 8 + 8 + 7 = 26 bits -> mid-byte!)
    bw.write_bits(0, 1);
    bw.write_bits(1, 2);
    bw.write_fixed_sym(u16::from(b'A'));
    bw.write_fixed_sym(u16::from(b'B'));
    bw.write_fixed_sym(256);
    // Block 2: BFINAL=0, BTYPE=00 (stored block immediately after mid-byte fixed block): emit "CD"
    bw.write_bits(0, 1);
    bw.write_bits(0, 2);
    bw.align_byte();
    bw.write_bytes_aligned(&2u16.to_le_bytes());
    bw.write_bytes_aligned(&(!2u16).to_le_bytes());
    bw.write_bytes_aligned(b"CD");
    // Block 3: BFINAL=1, BTYPE=10 (dynamic Huffman): symbols 'E' (len 1, code 0), EOB 256 (len 1, code 1)
    let mut ll = vec![0u8; 257];
    ll[usize::from(b'E')] = 1;
    ll[256] = 1;
    let dl = [0u8; 1]; // empty distance tree
    write_simple_dynamic_block(&mut bw, 1, &ll, &dl, |w| {
        w.write_huff(0, 1); // 'E'
        w.write_huff(0, 1); // 'E'
        w.write_huff(1, 1); // EOB
    });
    let expected = b"ABCDEE";
    let mixed_zlib = zlib_wrap(0x78, 0x9C, &bw.finish(), adler32(expected));
    assert_eq!(inflate_zlib(&mixed_zlib, 1024).unwrap(), expected);
}

#[test]
fn rfc1951_empty_distance_tree_and_single_symbol_huffman_invariants() {
    // 1. Empty distance tree (HDIST=1, dist_len=[0]) in literal-only dynamic block -> PASS
    let mut ll = vec![0u8; 257];
    ll[usize::from(b'X')] = 1;
    ll[256] = 1;
    let mut bw = BitWriter::new();
    write_simple_dynamic_block(&mut bw, 1, &ll, &[0], |w| {
        w.write_huff(0, 1); // 'X'
        w.write_huff(0, 1); // 'X'
        w.write_huff(0, 1); // 'X'
        w.write_huff(1, 1); // EOB
    });
    let z = zlib_wrap(0x78, 0x9C, &bw.finish(), adler32(b"XXX"));
    assert_eq!(inflate_zlib(&z, 1024).unwrap(), b"XXX");

    // 2. Empty distance tree attempting to decode a length/distance pair -> FAIL
    let mut ll_len = vec![0u8; 258];
    ll_len[usize::from(b'X')] = 1; // code 0
    ll_len[256] = 2; // code 10
    ll_len[257] = 2; // code 11 (length 3)
    let mut bw_bad = BitWriter::new();
    write_simple_dynamic_block(&mut bw_bad, 1, &ll_len, &[0], |w| {
        w.write_huff(0, 1); // 'X'
        w.write_huff(0b11, 2); // len=3 -> tries to decode distance from empty table!
        w.write_huff(0b10, 2); // EOB
    });
    let z_bad = zlib_wrap(0x78, 0x9C, &bw_bad.finish(), adler32(b"XXXX"));
    assert!(matches!(
        inflate_zlib(&z_bad, 1024),
        Err(InflateError::InvalidHuffmanTree(_))
    ));

    // 3. Valid 1-symbol distance tree (sole symbol has len == 1, code 0 -> dist 1) -> PASS
    let mut bw_one = BitWriter::new();
    write_simple_dynamic_block(&mut bw_one, 1, &ll_len, &[1], |w| {
        w.write_huff(0, 1); // 'X'
        w.write_huff(0b11, 2); // len=3
        w.write_huff(0, 1); // dist code 0 -> dist 1
        w.write_huff(0b10, 2); // EOB
    });
    let z_one = zlib_wrap(0x78, 0x9C, &bw_one.finish(), adler32(b"XXXX"));
    assert_eq!(inflate_zlib(&z_one, 1024).unwrap(), b"XXXX");

    // 4. 1-symbol distance tree fed invalid code `1` -> FAIL
    let mut bw_one_bad_code = BitWriter::new();
    write_simple_dynamic_block(&mut bw_one_bad_code, 1, &ll_len, &[1], |w| {
        w.write_huff(0, 1); // 'X'
        w.write_huff(0b11, 2); // len=3
        w.write_huff(1, 1); // INVALID code 1 for 1-symbol len=1 tree!
        w.write_huff(0b10, 2); // EOB
    });
    let z_one_bad_code = zlib_wrap(0x78, 0x9C, &bw_one_bad_code.finish(), adler32(b"XXXX"));
    assert!(matches!(
        inflate_zlib(&z_one_bad_code, 1024),
        Err(InflateError::InvalidHuffmanTree(_))
    ));

    // 5. Incomplete 1-symbol distance tree with len == 2 -> FAIL
    let mut bw_one_len2 = BitWriter::new();
    write_simple_dynamic_block(&mut bw_one_len2, 1, &ll_len, &[2], |w| {
        w.write_huff(0, 1);
        w.write_huff(0b10, 2);
    });
    let z_one_len2 = zlib_wrap(0x78, 0x9C, &bw_one_len2.finish(), adler32(b"X"));
    assert!(matches!(
        inflate_zlib(&z_one_len2, 1024),
        Err(InflateError::InvalidHuffmanTree(_))
    ));
}

#[test]
fn rfc1950_rfc1951_hostile_negative_controls_and_zip_bomb() {
    let valid = zlib_stored(b"abc");

    // CM != 8
    let mut bad_cm = valid.clone();
    bad_cm[0] = 0x79;
    assert_eq!(
        inflate_zlib(&bad_cm, 1024),
        Err(InflateError::UnsupportedCompressionMethod(9))
    );

    // CINFO > 7
    let mut bad_cinfo = valid.clone();
    bad_cinfo[0] = 0x88;
    assert_eq!(
        inflate_zlib(&bad_cinfo, 1024),
        Err(InflateError::InvalidWindowSize(8))
    );

    // Bad FCHECK
    let mut bad_fcheck = valid.clone();
    bad_fcheck[1] ^= 1;
    assert_eq!(
        inflate_zlib(&bad_fcheck, 1024),
        Err(InflateError::BadHeaderChecksum)
    );

    // FDICT != 0 (0x78 0xBB -> (0x78BB % 31 == 0) and bit 5 set!)
    assert_eq!(((0x78u16 << 8) | 0xBB) % 31, 0);
    let mut bad_fdict = valid.clone();
    bad_fdict[0] = 0x78;
    bad_fdict[1] = 0xBB;
    assert_eq!(
        inflate_zlib(&bad_fdict, 1024),
        Err(InflateError::PresetDictionaryUnsupported)
    );

    // BTYPE == 3
    let mut bw = BitWriter::new();
    bw.write_bits(1, 1);
    bw.write_bits(3, 2);
    let z_btype3 = zlib_wrap(0x78, 0x9C, &bw.finish(), 1);
    assert_eq!(inflate_zlib(&z_btype3, 1024), Err(InflateError::InvalidBlockType));

    // Stored LEN != !NLEN
    let mut bad_nlen = valid.clone();
    bad_nlen[5] ^= 0xFF;
    assert_eq!(
        inflate_zlib(&bad_nlen, 1024),
        Err(InflateError::StoredLengthMismatch)
    );

    // HLIT > 286 (hlit_bits = 30 -> 287)
    let mut bw_hlit = BitWriter::new();
    bw_hlit.write_bits(1, 1);
    bw_hlit.write_bits(2, 2);
    bw_hlit.write_bits(30, 5); // 287 > 286
    bw_hlit.write_bits(0, 5);
    bw_hlit.write_bits(0, 4);
    assert!(matches!(
        inflate_deflate(&bw_hlit.finish(), 1024),
        Err(InflateError::InvalidHuffmanTree(_))
    ));

    // HDIST > 30 (hdist_bits = 30 -> 31)
    let mut bw_hdist = BitWriter::new();
    bw_hdist.write_bits(1, 1);
    bw_hdist.write_bits(2, 2);
    bw_hdist.write_bits(0, 5);
    bw_hdist.write_bits(30, 5); // 31 > 30
    bw_hdist.write_bits(0, 4);
    assert!(matches!(
        inflate_deflate(&bw_hdist.finish(), 1024),
        Err(InflateError::InvalidHuffmanTree(_))
    ));

    // Empty code-length table
    let mut bw_empty_cl = BitWriter::new();
    bw_empty_cl.write_bits(1, 1);
    bw_empty_cl.write_bits(2, 2);
    bw_empty_cl.write_bits(0, 5);
    bw_empty_cl.write_bits(0, 5);
    bw_empty_cl.write_bits(0, 4); // 4 code lengths all 0
    bw_empty_cl.write_bits(0, 12);
    assert!(matches!(
        inflate_deflate(&bw_empty_cl.finish(), 1024),
        Err(InflateError::InvalidHuffmanTree(_))
    ));

    // Missing EOB 256 in litlen table
    let mut ll_no_eob = vec![0u8; 257];
    ll_no_eob[usize::from(b'A')] = 1;
    ll_no_eob[usize::from(b'B')] = 1;
    let mut bw_no_eob = BitWriter::new();
    write_simple_dynamic_block(&mut bw_no_eob, 1, &ll_no_eob, &[0], |_| {});
    assert!(matches!(
        inflate_deflate(&bw_no_eob.finish(), 1024),
        Err(InflateError::InvalidHuffmanTree(_))
    ));

    // Oversubscribed litlen tree (three symbols with length 1)
    let mut ll_over = vec![0u8; 257];
    ll_over[usize::from(b'A')] = 1;
    ll_over[usize::from(b'B')] = 1;
    ll_over[256] = 1;
    let mut bw_over = BitWriter::new();
    write_simple_dynamic_block(&mut bw_over, 1, &ll_over, &[0], |_| {});
    assert!(matches!(
        inflate_deflate(&bw_over.finish(), 1024),
        Err(InflateError::InvalidHuffmanTree(_))
    ));

    // Incomplete multi-symbol tree (two symbols with length 2)
    let mut ll_incomp = vec![0u8; 257];
    ll_incomp[usize::from(b'A')] = 2;
    ll_incomp[256] = 2;
    let mut bw_incomp = BitWriter::new();
    write_simple_dynamic_block(&mut bw_incomp, 1, &ll_incomp, &[0], |_| {});
    assert!(matches!(
        inflate_deflate(&bw_incomp.finish(), 1024),
        Err(InflateError::InvalidHuffmanTree(_))
    ));

    // RLE symbol 16 at index 0 (canonical order: sym 0 -> code 0, sym 16 -> code 1)
    let mut bw_rle16 = BitWriter::new();
    bw_rle16.write_bits(1, 1);
    bw_rle16.write_bits(2, 2);
    bw_rle16.write_bits(0, 5); // hlit=257
    bw_rle16.write_bits(0, 5); // hdist=1
    bw_rle16.write_bits(0, 4); // hclen=4 -> CL_ORDER[0..4] = [16, 17, 18, 0]
    bw_rle16.write_bits(1, 3); // sym 16 -> len 1 (code 1)
    bw_rle16.write_bits(0, 3); // sym 17 -> 0
    bw_rle16.write_bits(0, 3); // sym 18 -> 0
    bw_rle16.write_bits(1, 3); // sym 0 -> len 1 (code 0)
    bw_rle16.write_huff(1, 1); // emit RLE 16 at index 0!
    bw_rle16.write_bits(0, 2);
    assert_eq!(
        inflate_deflate(&bw_rle16.finish(), 1024),
        Err(InflateError::InvalidHuffmanTree("RLE symbol 16 at index 0"))
    );

    // RLE overflow (> hlit + hdist) via symbol 18 (repeat 138 zeros twice = 276 > 258)
    let mut bw_rle18_ovf = BitWriter::new();
    bw_rle18_ovf.write_bits(1, 1);
    bw_rle18_ovf.write_bits(2, 2);
    bw_rle18_ovf.write_bits(0, 5); // hlit=257
    bw_rle18_ovf.write_bits(0, 5); // hdist=1 -> total=258
    bw_rle18_ovf.write_bits(0, 4); // hclen=4 -> [16, 17, 18, 0]
    bw_rle18_ovf.write_bits(0, 3);
    bw_rle18_ovf.write_bits(0, 3);
    bw_rle18_ovf.write_bits(1, 3); // sym 18 -> len 1 (code 1)
    bw_rle18_ovf.write_bits(1, 3); // sym 0 -> len 1 (code 0)
    bw_rle18_ovf.write_huff(1, 1);
    bw_rle18_ovf.write_bits(127, 7); // 138 zeros
    bw_rle18_ovf.write_huff(1, 1);
    bw_rle18_ovf.write_bits(127, 7); // +138 = 276 > 258!
    assert_eq!(
        inflate_deflate(&bw_rle18_ovf.finish(), 1024),
        Err(InflateError::InvalidHuffmanTree(
            "RLE symbol 18 overflows HLIT + HDIST"
        ))
    );

    // Decoded litlen 286 in fixed Huffman block -> InvalidSymbol(286)
    let mut bw_286 = BitWriter::new();
    bw_286.write_bits(1, 1);
    bw_286.write_bits(1, 2);
    bw_286.write_fixed_sym(286);
    assert_eq!(
        inflate_deflate(&bw_286.finish(), 1024),
        Err(InflateError::InvalidSymbol(286))
    );

    // Decoded distance 30 in fixed Huffman block -> InvalidSymbol(30)
    let mut bw_d30 = BitWriter::new();
    bw_d30.write_bits(1, 1);
    bw_d30.write_bits(1, 2);
    bw_d30.write_fixed_sym(u16::from(b'A'));
    bw_d30.write_fixed_sym(257); // len 3
    bw_d30.write_huff(30, 5); // invalid dist sym 30
    assert_eq!(
        inflate_deflate(&bw_d30.finish(), 1024),
        Err(InflateError::InvalidSymbol(30))
    );

    // Distance > out.len()
    let mut bw_d_oob = BitWriter::new();
    bw_d_oob.write_bits(1, 1);
    bw_d_oob.write_bits(1, 2);
    bw_d_oob.write_fixed_sym(u16::from(b'A'));
    bw_d_oob.write_fixed_sym(257); // len 3
    bw_d_oob.write_huff(1, 5); // dist 2 > out.len() == 1
    assert!(matches!(
        inflate_deflate(&bw_d_oob.finish(), 1024),
        Err(InflateError::InvalidDistance { .. })
    ));

    // Distance > (1 << (cinfo + 8)): cinfo = 0 -> window = 256; output = 300 bytes; dist = 257!
    // CMF = (0 << 4) | 8 = 0x08; FCHECK: (0x0800 | flg) % 31 == 0 -> 0x081D (2077 = 67 * 31)
    assert_eq!(((0x08u16 << 8) | 0x1D) % 31, 0);
    let mut bw_win = BitWriter::new();
    bw_win.write_bits(1, 1);
    bw_win.write_bits(1, 2);
    for _ in 0..300 {
        bw_win.write_fixed_sym(u16::from(b'A'));
    }
    bw_win.write_fixed_sym(257); // len 3
    bw_win.write_huff(16, 5); // dist code 16 -> base 257, 7 extra bits
    bw_win.write_bits(0, 7); // dist = 257 > window 256!
    bw_win.write_fixed_sym(256);
    let z_win = zlib_wrap(0x08, 0x1D, &bw_win.finish(), adler32(&vec![b'A'; 303]));
    assert_eq!(
        inflate_zlib(&z_win, 1024),
        Err(InflateError::InvalidDistance {
            dist: 257,
            available: 300,
            window: 256,
        })
    );

    // Truncated input & truncated Adler-32
    assert_eq!(inflate_zlib(&valid[..4], 1024), Err(InflateError::TruncatedInput));
    assert_eq!(
        inflate_zlib(&valid[..valid.len() - 1], 1024),
        Err(InflateError::TruncatedInput)
    );

    // Corrupt Adler-32
    let mut bad_adler = valid.clone();
    *bad_adler.last_mut().unwrap() ^= 0x01;
    assert!(matches!(
        inflate_zlib(&bad_adler, 1024),
        Err(InflateError::Adler32Mismatch { .. })
    ));

    // Trailing non-whitespace garbage
    let mut garbage = valid.clone();
    garbage.push(0xFF);
    assert_eq!(
        inflate_zlib(&garbage, 1024),
        Err(InflateError::TrailingGarbage)
    );

    // Zip-bomb exceeding MAX_STREAM_DECOMP_BYTES (32 MiB) fails BEFORE growing past cap!
    let bomb = zlib_zip_bomb(MAX_STREAM_DECOMP_BYTES + 1024);
    assert_eq!(
        inflate_zlib(&bomb, MAX_STREAM_DECOMP_BYTES),
        Err(InflateError::DecompressionLimitExceeded {
            limit: MAX_STREAM_DECOMP_BYTES
        })
    );
    let bomb_pdf = make_synthetic_pdf(2, b"", &[(b"/Filter /FlateDecode", &bomb, true)]);
    let bomb_rep = audit_pdf_bytes(&bomb_pdf, 1, None);
    assert!(!bomb_rep.is_ok());
    assert!(bomb_rep
        .errors
        .iter()
        .any(|e| e.contains("decompression limit exceeded")));

    // Multi-stream total cap: 5 streams of 28 MiB each (each < 32 MiB, total 140 MiB > 128 MiB)
    let stream_28m = zlib_zip_bomb(28 * 1024 * 1024);
    let multi_bomb_pdf = make_synthetic_pdf(
        2,
        b"",
        &[
            (b"/Filter /FlateDecode", &stream_28m, true),
            (b"/Filter /FlateDecode", &stream_28m, true),
            (b"/Filter /FlateDecode", &stream_28m, true),
            (b"/Filter /FlateDecode", &stream_28m, true),
            (b"/Filter /FlateDecode", &stream_28m, true),
        ],
    );
    let multi_rep = audit_pdf_bytes(&multi_bomb_pdf, 1, None);
    assert!(!multi_rep.is_ok());
    assert_eq!(multi_rep.streams_flate_inflated, 4);
    assert!(multi_rep
        .errors
        .iter()
        .any(|e| e.contains("decompression limit exceeded")));
}

// ============================================================================
// AC-03..AC-06 & P0-1/P0-2/P1-1/P2-2: PDF Framing, Scanners & Edge Cases
// ============================================================================

#[test]
fn pdf_clean_vector_prose_procset_and_adler32_0a_0d_trailers() {
    let z_clean = decode_hex(GOLDEN_BTYPE10_CLEAN_CONTENT_HEX);
    let z_0a = decode_hex(GOLDEN_BTYPE10_ADLER_0A_HEX);
    let z_0d = decode_hex(GOLDEN_BTYPE10_ADLER_0D_HEX);

    // 1. Clean vector PDF with prose mentioning `/Subtype /Image`, Typst `/ProcSet` boilerplate,
    //    and both direct `/Length` + indirect `/Length 999 0 R` streams with 0x0A/0x0D trailers.
    let pdf = make_synthetic_pdf(
        6,
        b"<< /ProcSet [/PDF /Text /ImageB /ImageC /ImageI] >>",
        &[
            (b"/Filter /FlateDecode", &z_clean, true),
            (b"/Filter[/FlateDecode]", &z_0a, true),
            (b"/Filter/FlateDecode", &z_0d, true),
            (b"/Filter /FlateDecode", &z_0a, false),
            (b"/Filter /FlateDecode", &z_0d, false),
        ],
    );
    let rep = audit_pdf_bytes(&pdf, 6, Some(&pdf));
    assert!(rep.is_ok(), "expected PASS, got errors: {:?}", rep.errors);
    assert_eq!(rep.pages, 6);
    assert_eq!(rep.streams_total, 5);
    assert_eq!(rep.streams_flate_inflated, 5);
    assert_eq!(rep.raster_violations, 0);
    assert_eq!(rep.unresolved_violations, 0);
    assert_eq!(rep.deterministic_match, Some(true));

    // Also test `\r\nendstream` with indirect `/Length` for both 0x0A and 0x0D Adler-32 trailers!
    let mut crlf_pdf = Vec::new();
    crlf_pdf.extend_from_slice(b"%PDF-1.7\n1 0 obj\n<< /Type /Page >>\nendobj\n");
    crlf_pdf.extend_from_slice(b"2 0 obj\n<< /Filter /FlateDecode /Length 99 0 R >>\r\nstream\r\n");
    crlf_pdf.extend_from_slice(&z_0a);
    crlf_pdf.extend_from_slice(b"\r\nendstream\r\nendobj\n");
    crlf_pdf.extend_from_slice(b"3 0 obj\n<< /Filter /FlateDecode /Length 98 0 R >>\r\nstream\r\n");
    crlf_pdf.extend_from_slice(&z_0d);
    crlf_pdf.extend_from_slice(b"\r\nendstream\r\nendobj\n%%EOF\n");
    let crlf_rep = audit_pdf_bytes(&crlf_pdf, 1, None);
    assert!(
        crlf_rep.is_ok(),
        "CRLF indirect streams with 0x0A/0x0D trailers must pass: {:?}",
        crlf_rep.errors
    );
    assert_eq!(crlf_rep.streams_flate_inflated, 2);
}

#[test]
fn pdf_nested_xref_decodeparms_indirect_length_and_binary_font_streams() {
    // Nested `/Type /XRef` stream with `/DecodeParms << /Columns 5 /Predictor 12 /Filter /Bogus /Length 1 >>`
    // and indirect `/Length 99 0 R` containing binary xref rows that happen to include `(??)`.
    let xref_payload = b"\x02\x00\x01\x00\x00(??)\x02\x00\x02\x00\x00";
    let xref_comp = zlib_fixed_literals(xref_payload);

    let font_payload_ok = b"\x00\x01\x00\x00(binary ?? font bytecode)\x00\xff";
    let font_comp_ok = zlib_stored(font_payload_ok);

    let pdf = make_synthetic_pdf(
        2,
        b"",
        &[
            (
                b"/Type /XRef /Size 20 /W [1 2 1] /Filter /FlateDecode /DecodeParms << /Columns 5 /Predictor 12 /Filter /Bogus /Length 1 >> /Resources << /Font << /F1 5 0 R >> >>",
                &xref_comp,
                false,
            ),
            (
                b"/Length1 32 /Filter /FlateDecode",
                &font_comp_ok,
                true,
            ),
            (
                b"/Subtype /CIDFontType0C /Filter /FlateDecode",
                &font_comp_ok,
                true,
            ),
            (
                b"/Subtype /Type1C /Filter /FlateDecode",
                &font_comp_ok,
                true,
            ),
            (
                b"/Subtype /OpenType /Filter /FlateDecode",
                &font_comp_ok,
                true,
            ),
        ],
    );
    let rep = audit_pdf_bytes(&pdf, 2, None);
    assert!(
        rep.is_ok(),
        "nested XRef + binary font streams must pass: {:?}",
        rep.errors
    );
    assert_eq!(rep.streams_total, 5);
    assert_eq!(rep.streams_flate_inflated, 5);
    assert_eq!(rep.unresolved_violations, 0);

    // Binary font stream MUST STILL reject `/Subtype /Image` and long `UNRESOLVED_MARKERS`!
    let bad_font = zlib_stored(b"\x00\x01Unresolved citation: bad /Subtype /Image");
    let bad_font_pdf = make_synthetic_pdf(
        2,
        b"",
        &[(b"/Length1 40 /Filter /FlateDecode", &bad_font, true)],
    );
    let bad_rep = audit_pdf_bytes(&bad_font_pdf, 2, None);
    assert!(!bad_rep.is_ok());
    assert!(bad_rep.raster_violations > 0);
    assert!(bad_rep.unresolved_violations > 0);
}

#[test]
fn pdf_ciphertext_unmatched_paren_masking_and_obfuscated_subtype_image() {
    // 1. Compressed stream containing unmatched 0x28 '(' bytes in zlib ciphertext, followed by an
    //    uncompressed object declaring `/Subtype /Image`. Without `root_structure` masking, the
    //    unmatched '(' in ciphertext would swallow the subsequent `/Subtype /Image` object!
    let z_unmatched_paren = decode_hex(GOLDEN_BTYPE10_ADLER_0A_HEX);
    let opens = z_unmatched_paren.iter().filter(|&&b| b == b'(').count();
    let closes = z_unmatched_paren.iter().filter(|&&b| b == b')').count();
    assert!(
        opens > closes,
        "test vector must have unmatched '(' in compressed ciphertext"
    );

    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.7\n1 0 obj\n<< /Type /Page >>\nendobj\n");
    pdf.extend_from_slice(
        format!(
            "2 0 obj\n<< /Filter /FlateDecode /Length {} >>\nstream\n",
            z_unmatched_paren.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(&z_unmatched_paren);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    pdf.extend_from_slice(
        b"3 0 obj\n<< /Type /XObject /Subtype /Image /Width 4 /Height 4 >>\nendobj\n%%EOF\n",
    );
    let rep = audit_pdf_bytes(&pdf, 1, None);
    assert!(!rep.is_ok());
    assert!(rep.raster_violations > 0);
    assert!(rep
        .errors
        .iter()
        .any(|e| e.contains("root: /Subtype /Image")));

    // 2. Hidden `/Subtype /Image` inside dynamic-Huffman `/ObjStm` stream
    let z_objstm = decode_hex(GOLDEN_BTYPE10_OBJSTM_HIDDEN_IMAGE_HEX);
    let objstm_pdf = make_synthetic_pdf(
        2,
        b"",
        &[(b"/Type /ObjStm /Filter /FlateDecode", &z_objstm, true)],
    );
    let objstm_rep = audit_pdf_bytes(&objstm_pdf, 1, None);
    assert!(!objstm_rep.is_ok());
    assert!(objstm_rep.raster_violations > 0);
    assert!(objstm_rep
        .errors
        .iter()
        .any(|e| e.contains("stream[1]: /Subtype /Image")));

    // 3. Zero-whitespace `/Subtype/Image`, comment-separated, and `#XX` hex-escaped names
    for payload in [
        b"<< /Type/XObject/Subtype/Image /Width 2 /Height 2 >>".as_slice(),
        b"<< /Subtype % hidden by comment\n /Image /Width 2 /Height 2 >>".as_slice(),
        b"<< /Sub#74ype /Im#61ge /Width 2 /Height 2 >>".as_slice(),
    ] {
        let p = make_synthetic_pdf(1, payload, &[]);
        let r = audit_pdf_bytes(&p, 1, None);
        assert!(
            !r.is_ok() && r.raster_violations > 0,
            "expected raster violation for {:?}",
            String::from_utf8_lossy(payload)
        );
    }
}

#[test]
fn pdf_inline_image_detection_and_false_positive_guards() {
    // 1. Real inline raster image (with 0x28 '(' inside binary pixel bytes!) -> FAIL
    let inline = zlib_fixed_literals(b"q BI /W 2 /H 2 /BPC 8 /CS /G ID \x00\x28\xff\x00 EI Q");
    let p_inline = make_synthetic_pdf(2, b"", &[(b"/Filter /FlateDecode", &inline, true)]);
    let r_inline = audit_pdf_bytes(&p_inline, 1, None);
    assert!(!r_inline.is_ok());
    assert!(r_inline.raster_violations > 0);
    assert!(r_inline
        .errors
        .iter()
        .any(|e| e.contains("inline raster image")));

    // 2. Prose literal string containing `BI..ID..EI` -> PASS
    let prose = zlib_stored(b"BT /F1 10 Tf (BI /W 2 /H 2 /CS /G ID xxxx EI) Tj ET");
    let p_prose = make_synthetic_pdf(2, b"", &[(b"/Filter /FlateDecode", &prose, true)]);
    let r_prose = audit_pdf_bytes(&p_prose, 1, None);
    assert!(r_prose.is_ok(), "prose BI..EI must pass: {:?}", r_prose.errors);

    // 3. `BI..ID..EI` without any valid `/` inline-image dictionary key -> PASS
    let no_key = zlib_stored(b"BI hello world ID foo EI");
    let p_no_key = make_synthetic_pdf(2, b"", &[(b"/Filter /FlateDecode", &no_key, true)]);
    let r_no_key = audit_pdf_bytes(&p_no_key, 1, None);
    assert!(r_no_key.is_ok(), "BI..ID..EI without `/` key must pass");

    // 4. `BI..ID` span > 8192 bytes -> PASS (no catastrophic backtracking!)
    let mut long_span = b"BI /W 2 ".to_vec();
    long_span.resize(long_span.len() + 9000, b' ');
    long_span.extend_from_slice(b"ID \x00 EI");
    let long_z = zlib_stored(&long_span);
    let p_long = make_synthetic_pdf(2, b"", &[(b"/Filter /FlateDecode", &long_z, true)]);
    let r_long = audit_pdf_bytes(&p_long, 1, None);
    assert!(r_long.is_ok(), "BI..ID > 8192B must not trigger inline image");

    // 5. Non-printable binary byte in BI..ID dict -> PASS
    let non_ascii = zlib_stored(b"BI /W 2 \x01 ID \x00 EI");
    let p_na = make_synthetic_pdf(2, b"", &[(b"/Filter /FlateDecode", &non_ascii, true)]);
    assert!(audit_pdf_bytes(&p_na, 1, None).is_ok());
}

#[test]
fn pdf_unsupported_filters_corrupt_flate_and_unresolved_markers() {
    // 1. Unsupported /Filter /ASCIIHexDecode, /DCTDecode, and multi-filter array -> FAIL CLOSED
    for filter_dict in [
        b"/Filter /ASCIIHexDecode".as_slice(),
        b"/Filter /DCTDecode".as_slice(),
        b"/Filter [/FlateDecode /ASCII85Decode]".as_slice(),
    ] {
        let p = make_synthetic_pdf(2, b"", &[(filter_dict, b"414243>", true)]);
        let r = audit_pdf_bytes(&p, 1, None);
        assert!(!r.is_ok(), "filter {:?} must fail closed", String::from_utf8_lossy(filter_dict));
        assert!(r.errors.iter().any(|e| e.contains("unsupported/disallowed stream /Filter")));
    }

    // 2. Corrupt /FlateDecode stream -> FAIL CLOSED
    let p_corrupt = make_synthetic_pdf(
        2,
        b"",
        &[(b"/Filter /FlateDecode", b"\x78\x9c\xff\xff\xff\xff", true)],
    );
    let r_corrupt = audit_pdf_bytes(&p_corrupt, 1, None);
    assert!(!r_corrupt.is_ok());
    assert!(r_corrupt
        .errors
        .iter()
        .any(|e| e.contains("/FlateDecode inflation failed")));

    // 3. Unresolved reference markers (including octal-escaped `\077\077`!)
    for marker in [
        b"BT (See ??) Tj ET".as_slice(),
        b"BT (See \\077\\077) Tj ET".as_slice(),
        b"BT (Unresolved Main reference: main:eq:missing) Tj ET".as_slice(),
        b"BT (Unresolved internal reference: sec:missing) Tj ET".as_slice(),
        b"BT (Unresolved citation: bad) Tj ET".as_slice(),
        b"BT ([unresolved]) Tj ET".as_slice(),
        b"BT (PANIC) Tj ET".as_slice(),
        b"% raw stream PANIC marker".as_slice(),
    ] {
        let comp = zlib_stored(marker);
        let p = make_synthetic_pdf(2, b"", &[(b"/Filter /FlateDecode", &comp, true)]);
        let r = audit_pdf_bytes(&p, 1, None);
        assert!(
            !r.is_ok() && r.unresolved_violations > 0,
            "marker {:?} must fail with unresolved violation",
            String::from_utf8_lossy(marker)
        );
    }
}

#[test]
fn pdf_page_counting_count_before_and_after_type_pages_and_determinism() {
    // 1. `/Count N` PRECEDING `/Type /Pages` (no individual `/Type /Page` objects)
    let count_before = make_synthetic_pdf(
        0,
        b"<< /Count 7 /Kids [2 0 R] /Type /Pages >>",
        &[],
    );
    let r_before = audit_pdf_bytes(&count_before, 7, None);
    assert!(r_before.is_ok(), "Count before /Type /Pages must pass: {:?}", r_before.errors);
    assert_eq!(r_before.pages, 7);

    // 2. `/Count N` FOLLOWING `/Type /Pages` with zero-whitespace `/Type/Pages`
    let count_after = make_synthetic_pdf(
        0,
        b"<< /Type/Pages /Kids [2 0 R] /Count 9 >>",
        &[],
    );
    let r_after = audit_pdf_bytes(&count_after, 9, None);
    assert!(r_after.is_ok());
    assert_eq!(r_after.pages, 9);

    // 3. Zero-whitespace `/Type/Page` across root + compressed `/ObjStm`
    let objstm_pages = zlib_stored(b"10 0 << /Type/Page >> 11 20 << /Type /Page >>");
    let p_objstm = make_synthetic_pdf(
        1,
        b"",
        &[(b"/Type /ObjStm /Filter /FlateDecode", &objstm_pages, true)],
    );
    let r_objstm = audit_pdf_bytes(&p_objstm, 3, None);
    assert!(r_objstm.is_ok(), "{:?}", r_objstm.errors);
    assert_eq!(r_objstm.pages, 3);

    // 4. min_pages failure and determinism match vs mismatch
    let p_short = make_synthetic_pdf(3, b"", &[]);
    let r_short = audit_pdf_bytes(&p_short, 6, None);
    assert!(!r_short.is_ok());
    assert!(r_short.errors.iter().any(|e| e.contains("page count 3 < required")));

    let mut p_diff = p_short.clone();
    p_diff.push(b' ');
    let r_diff = audit_pdf_bytes(&p_short, 3, Some(&p_diff));
    assert!(!r_diff.is_ok());
    assert_eq!(r_diff.deterministic_match, Some(false));
    assert!(r_diff.errors.iter().any(|e| e.contains("non-deterministic")));
}

// ============================================================================
// AC-07, AC-08, P1-2, P1-3: Symmetric Symlink/Size Guards, CLI, Zero-LLVM Proof
// ============================================================================

#[test]
fn pdf_symmetric_symlink_and_max_size_bounded_read_rejection() {
    let tmp = TempDirGuard::new("symlink_and_size");
    let clean_path = tmp.path().join("clean.pdf");
    std::fs::write(&clean_path, make_synthetic_pdf(2, b"", &[])).unwrap();

    // 1. Symlink rejected symmetrically on BOTH `<pdf>` and `--compare-pdf`
    let link_path = tmp.path().join("link.pdf");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&clean_path, &link_path).unwrap();

    let rep_primary_sym = audit_pdf_file(&link_path, &PdfAuditOptions::default());
    assert!(!rep_primary_sym.is_ok());
    assert!(rep_primary_sym
        .errors
        .iter()
        .any(|e| e == "pdf: symlink rejected"));
    assert_portable_pdf_json(&rep_primary_sym.to_json().to_compact_ascii_json());

    let rep_cmp_sym = audit_pdf_file(
        &clean_path,
        &PdfAuditOptions {
            min_pages: 1,
            compare_pdf: Some(link_path.clone()),
        },
    );
    assert!(!rep_cmp_sym.is_ok());
    assert_eq!(rep_cmp_sym.deterministic_match, Some(false));
    assert!(rep_cmp_sym
        .errors
        .iter()
        .any(|e| e == "compare_pdf: symlink rejected"));
    assert_portable_pdf_json(&rep_cmp_sym.to_json().to_compact_ascii_json());

    // 2. Directory (non-regular file) rejected symmetrically
    let rep_dir = audit_pdf_file(tmp.path(), &PdfAuditOptions::default());
    assert!(!rep_dir.is_ok());
    assert!(rep_dir
        .errors
        .iter()
        .any(|e| e == "pdf: not a regular file"));

    // 3. Oversized sparse file (256 MiB > MAX_PDF_BYTES = 64 MiB) rejected symmetrically without OOM
    let big_path = tmp.path().join("oversized.pdf");
    {
        let f = File::create(&big_path).unwrap();
        f.set_len((MAX_PDF_BYTES as u64) * 4).unwrap();
    }
    let rep_big_primary = audit_pdf_file(&big_path, &PdfAuditOptions::default());
    assert!(!rep_big_primary.is_ok());
    assert!(rep_big_primary
        .errors
        .iter()
        .any(|e| e.starts_with("pdf: file exceeds maximum size limit")));
    assert_portable_pdf_json(&rep_big_primary.to_json().to_compact_ascii_json());

    let rep_big_cmp = audit_pdf_file(
        &clean_path,
        &PdfAuditOptions {
            min_pages: 1,
            compare_pdf: Some(big_path),
        },
    );
    assert!(!rep_big_cmp.is_ok());
    assert_eq!(rep_big_cmp.deterministic_match, Some(false));
    assert!(rep_big_cmp
        .errors
        .iter()
        .any(|e| e.starts_with("compare_pdf: file exceeds maximum size limit")));
    assert_portable_pdf_json(&rep_big_cmp.to_json().to_compact_ascii_json());
}

#[test]
fn cli_audit_pdf_human_json_flags_and_ac08_zero_llvm_proof() {
    let tmp = TempDirGuard::new("cli_zero_llvm");
    let clean_a = tmp.path().join("clean_a.pdf");
    let clean_b = tmp.path().join("clean_b.pdf");
    let bad_pdf = tmp.path().join("bad_raster.pdf");

    let z_clean = decode_hex(GOLDEN_BTYPE10_CLEAN_CONTENT_HEX);
    let clean_bytes = make_synthetic_pdf(6, b"", &[(b"/Filter /FlateDecode", &z_clean, true)]);
    std::fs::write(&clean_a, &clean_bytes).unwrap();
    std::fs::write(&clean_b, &clean_bytes).unwrap();
    std::fs::write(
        &bad_pdf,
        make_synthetic_pdf(
            2,
            b"<< /Type /XObject /Subtype /Image /Width 8 /Height 8 >>",
            &[],
        ),
    )
    .unwrap();

    // 1. CLI PASS human mode
    let human = Command::new(aodc_bin())
        .args([
            "audit-pdf",
            "--min-pages",
            "6",
            "--compare-pdf",
            clean_b.to_str().unwrap(),
            clean_a.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        human.status.success(),
        "human audit-pdf failed:\n{}\n{}",
        String::from_utf8_lossy(&human.stdout),
        String::from_utf8_lossy(&human.stderr)
    );
    assert!(String::from_utf8_lossy(&human.stdout).contains("-> PASS"));

    // 2. CLI PASS --json mode (also with DEC_LLVM_LIBRARY=/nonexistent/libLLVM.so -> AC-08!)
    let json_pass = Command::new(aodc_bin())
        .env("DEC_LLVM_LIBRARY", "/nonexistent/libLLVM.so")
        .args([
            "audit-pdf",
            "--min-pages",
            "6",
            "--compare-pdf",
            clean_b.to_str().unwrap(),
            "--json",
            clean_a.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        json_pass.status.success(),
        "audit-pdf with nonexistent DEC_LLVM_LIBRARY must still exit 0:\n{}\n{}",
        String::from_utf8_lossy(&json_pass.stdout),
        String::from_utf8_lossy(&json_pass.stderr)
    );
    let pass_text = String::from_utf8_lossy(&json_pass.stdout);
    let parsed_pass = assert_portable_pdf_json(pass_text.trim());
    assert_eq!(parsed_pass.get("status").and_then(JsonValue::as_str), Some("PASS"));
    assert_eq!(
        parsed_pass.get("pages").and_then(JsonValue::as_int),
        Some(&BigInt::from_i64(6))
    );
    assert_eq!(
        parsed_pass.get("deterministic_match").and_then(JsonValue::as_bool),
        Some(true)
    );

    // 3. CLI FAIL --json mode (exits 1, still emits valid host-path-free JSON on stdout!)
    let json_fail = Command::new(aodc_bin())
        .args(["audit-pdf", "--json", bad_pdf.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!json_fail.status.success());
    let fail_text = String::from_utf8_lossy(&json_fail.stdout);
    let parsed_fail = assert_portable_pdf_json(fail_text.trim());
    assert_eq!(parsed_fail.get("status").and_then(JsonValue::as_str), Some("FAIL"));
    assert_eq!(
        parsed_fail.get("raster_violations").and_then(JsonValue::as_int),
        Some(&BigInt::from_i64(1))
    );

    // 4. Reject invalid flags (`--min-pages 0`, `--min-pages abc`, unknown `-*`, missing `<pdf>`)
    for bad_args in [
        vec!["audit-pdf", "--min-pages", "0", clean_a.to_str().unwrap()],
        vec!["audit-pdf", "--min-pages", "abc", clean_a.to_str().unwrap()],
        vec!["audit-pdf", "--bogus-flag", clean_a.to_str().unwrap()],
        vec!["audit-pdf"],
    ] {
        let out = Command::new(aodc_bin()).args(&bad_args).output().unwrap();
        assert!(
            !out.status.success(),
            "expected CLI rejection for {:?}",
            bad_args
        );
    }

    // 5. AC-08 Zero-LLVM proof:
    //    (a) zero `aod_llvm` references in `crates/aodc/src/pdf_audit.rs`
    let pdf_audit_src =
        std::fs::read_to_string(repo_root().join("crates/aodc/src/pdf_audit.rs")).unwrap();
    assert!(
        !pdf_audit_src.contains("aod_llvm"),
        "pdf_audit.rs must have zero aod_llvm imports/references"
    );

    //    (b) `ldd` on `aodc` binary has ZERO dynamic links to llvm, python, or gmp
    let ldd = Command::new("ldd").arg(aodc_bin()).output().unwrap();
    assert!(ldd.status.success(), "ldd failed on aodc binary");
    let ldd_lower = String::from_utf8_lossy(&ldd.stdout).to_ascii_lowercase();
    for forbidden in ["llvm", "python", "gmp"] {
        assert!(
            !ldd_lower.contains(forbidden),
            "aodc binary must not dynamically link {forbidden}:\n{ldd_lower}"
        );
    }
}

// ============================================================================
// PR 7 Final Review Regressions: P1-1, P1-2, P1-3, and P2-1
// ============================================================================

#[test]
fn p1_1_missing_pdf_header_and_missing_eof_trailer_rejected() {
    // 1. Missing `%PDF-` header (valid `%%EOF` trailer present)
    let missing_header = b"1 0 obj\n<< /Type /Page >>\nendobj\n%%EOF\n";
    let rep_hdr = audit_pdf_bytes(missing_header, 1, None);
    assert!(!rep_hdr.is_ok());
    assert!(
        rep_hdr.errors.iter().any(|e| e == "pdf: missing %PDF- header"),
        "expected missing %PDF- header error, got {:?}",
        rep_hdr.errors
    );

    // 2. Missing `%%EOF` trailer (valid `%PDF-` header present, truncated tail)
    let missing_eof = b"%PDF-1.7\n1 0 obj\n<< /Type /Page >>\nendobj\n";
    let rep_eof = audit_pdf_bytes(missing_eof, 1, None);
    assert!(!rep_eof.is_ok());
    assert!(
        rep_eof.errors.iter().any(|e| e == "pdf: missing %%EOF trailer"),
        "expected missing %%EOF trailer error, got {:?}",
        rep_eof.errors
    );

    // 3. Valid `%PDF-` + `%%EOF` with trailing ASCII whitespace passes
    let valid_ws_eof = b"%PDF-1.7\n1 0 obj\n<< /Type /Page >>\nendobj\n%%EOF \r\n\t";
    let rep_ok = audit_pdf_bytes(valid_ws_eof, 1, None);
    assert!(
        rep_ok.is_ok(),
        "valid PDF with trailing whitespace after %%EOF must pass: {:?}",
        rep_ok.errors
    );
}

#[test]
fn p1_2_max_streams_cap_rejects_excess_streams_fail_closed() {
    let mut pdf = Vec::with_capacity((MAX_STREAMS + 1) * 40 + 128);
    pdf.extend_from_slice(b"%PDF-1.7\n1 0 obj\n<< /Type /Page >>\nendobj\n");
    for _ in 0..=MAX_STREAMS {
        pdf.extend_from_slice(b"<< /Length 0 >>\nstream\n\nendstream\n");
    }
    pdf.extend_from_slice(b"%%EOF\n");

    let rep = audit_pdf_bytes(&pdf, 1, None);
    assert!(!rep.is_ok());
    assert_eq!(rep.streams_total, MAX_STREAMS);
    assert!(
        rep.errors
            .iter()
            .any(|e| e == "pdf: stream count exceeds maximum limit of 16384"),
        "expected MAX_STREAMS fail-closed error, got {:?}",
        rep.errors
    );
}

#[test]
fn p1_3_length1_font_stream_cannot_smuggle_inline_raster_image() {
    let smuggled = zlib_fixed_literals(b"q BI /W 2 /H 2 /BPC 8 /CS /G ID \x00\xff\xff\x00 EI Q");
    let pdf = make_synthetic_pdf(
        2,
        b"",
        &[(b"/Length1 32 /Filter /FlateDecode", &smuggled, true)],
    );
    let rep = audit_pdf_bytes(&pdf, 1, None);
    assert!(
        !rep.is_ok(),
        "inline raster image inside /Length1 stream must be rejected"
    );
    assert!(rep.raster_violations > 0);
    assert!(
        rep.errors
            .iter()
            .any(|e| e.contains("inline raster image BI..ID..EI detected")),
        "expected inline raster violation, got {:?}",
        rep.errors
    );
}

#[test]
fn p2_1_dynamic_huffman_rle_16_and_17_spanning_hlit_into_hdist() {
    const CL_ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];

    // Case A: RLE symbol 16 starts at index 257 (< hlit = 258) and writes 5 copies of length 2,
    // spanning across hlit=258 into all 4 entries of hdist (indices 258..=261).
    // - litlen: 'A'(65) -> len 1 (code 0), EOB(256) -> len 2 (code 10), len=3(257) -> len 2 (code 11)
    // - dist:   0..=3 -> len 2 (codes 00, 01, 10, 11 -> distances 1, 2, 3, 4)
    // - CL tree: {1, 2, 16, 18} all length 2 -> canonical codes: 1->00, 2->01, 16->10, 18->11
    let mut bw16 = BitWriter::new();
    bw16.write_bits(1, 1); // BFINAL = 1
    bw16.write_bits(2, 2); // BTYPE = 10
    bw16.write_bits(1, 5); // HLIT = 258
    bw16.write_bits(3, 5); // HDIST = 4 (total = 262)
    bw16.write_bits(14, 4); // HCLEN = 18 (covers up to sym 1 at CL_ORDER[17])
    for &slot in &CL_ORDER[..18] {
        let cl = if matches!(slot, 1 | 2 | 16 | 18) { 2 } else { 0 };
        bw16.write_bits(cl, 3);
    }
    // 0..65 -> 65 zeros via RLE 18 (54 + 11 = 65)
    bw16.write_huff(0b11, 2);
    bw16.write_bits(54, 7);
    // index 65 ('A') -> length 1
    bw16.write_huff(0b00, 2);
    // 66..256 -> 190 zeros via RLE 18 (138) + RLE 18 (52)
    bw16.write_huff(0b11, 2);
    bw16.write_bits(127, 7); // 138 zeros
    bw16.write_huff(0b11, 2);
    bw16.write_bits(41, 7); // 52 zeros
    // index 256 (EOB) -> length 2
    bw16.write_huff(0b01, 2);
    // indices 257..=261 -> 5 copies of previous length 2 via RLE 16 (crosses hlit=258!)
    bw16.write_huff(0b10, 2);
    bw16.write_bits(2, 2); // repeat = 2 + 3 = 5

    // Compressed body: 'A', (len=3, dist=1 -> "AAAA"), (len=3, dist=4 -> "AAAAAAA"), EOB
    bw16.write_huff(0, 1); // 'A'
    bw16.write_huff(0b11, 2); // len=3
    bw16.write_huff(0b00, 2); // dist[0] = 1
    bw16.write_huff(0b11, 2); // len=3
    bw16.write_huff(0b11, 2); // dist[3] = 4 (populated across hlit boundary!)
    bw16.write_huff(0b10, 2); // EOB

    let expected_a = b"AAAAAAA";
    let z16 = zlib_wrap(0x78, 0x9C, &bw16.finish(), adler32(expected_a));
    assert_eq!((z16[2] >> 1) & 3, 2);
    assert_eq!(inflate_zlib(&z16, 1024).unwrap(), expected_a);

    // Case B: RLE symbol 17 starts at index 258 (< hlit = 260) and writes 3 zeros spanning across
    // hlit=260 into dist[0]=0, followed by dist[1]=1 (1-symbol distance tree for distance 2).
    // - litlen: 'A'(65)->1 (code 0), EOB(256)->2 (code 10), len=3(257)->2 (code 11), 258..=259 -> 0
    // - dist:   dist[0]=0, dist[1]=1 (code 0 -> distance 2)
    // - CL tree: {1->2(00), 2->2(01), 17->2(10), 18->2(11)}
    let mut bw17 = BitWriter::new();
    bw17.write_bits(1, 1); // BFINAL = 1
    bw17.write_bits(2, 2); // BTYPE = 10
    bw17.write_bits(3, 5); // HLIT = 260
    bw17.write_bits(1, 5); // HDIST = 2 (total = 262)
    bw17.write_bits(14, 4); // HCLEN = 18
    for &slot in &CL_ORDER[..18] {
        let cl = if matches!(slot, 1 | 2 | 17 | 18) { 2 } else { 0 };
        bw17.write_bits(cl, 3);
    }
    // 0..65 -> 65 zeros
    bw17.write_huff(0b11, 2);
    bw17.write_bits(54, 7);
    // index 65 ('A') -> 1
    bw17.write_huff(0b00, 2);
    // 66..256 -> 190 zeros
    bw17.write_huff(0b11, 2);
    bw17.write_bits(127, 7);
    bw17.write_huff(0b11, 2);
    bw17.write_bits(41, 7);
    // index 256 (EOB) -> 2, index 257 (len=3) -> 2
    bw17.write_huff(0b01, 2);
    bw17.write_huff(0b01, 2);
    // indices 258, 259, 260 (= dist[0]) -> 3 zeros via RLE 17 spanning across hlit=260!
    bw17.write_huff(0b10, 2);
    bw17.write_bits(0, 3); // repeat = 0 + 3 = 3
    // index 261 (= dist[1]) -> length 1
    bw17.write_huff(0b00, 2);

    // Body: 'A', 'A', (len=3, dist=2 -> "AAAAA"), EOB
    bw17.write_huff(0, 1);
    bw17.write_huff(0, 1);
    bw17.write_huff(0b11, 2); // len=3
    bw17.write_huff(0, 1); // dist[1] = 2
    bw17.write_huff(0b10, 2); // EOB

    let expected_b = b"AAAAA";
    let z17 = zlib_wrap(0x78, 0x9C, &bw17.finish(), adler32(expected_b));
    assert_eq!((z17[2] >> 1) & 3, 2);
    assert_eq!(inflate_zlib(&z17, 1024).unwrap(), expected_b);
}
