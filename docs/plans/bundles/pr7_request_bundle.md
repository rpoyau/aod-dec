# PR 7 Request Bundle (Rev 2 — Approved with P0-1..P0-4 & P1-1..P1-3 Folded)

**Repository:** `/home/antigravity/rpoyau/aod-dec` (`v2/zero-python-evolution` off `main@68d6705`)
**Scope:** Zero-Python Purification & Pure-Rust `std`-only `aodc audit-pdf` (RFC 1950/1951 inflater, zero external crates, `#![forbid(unsafe_code)]`, zero floats, zero `.py` files in repo, zero Python in CI).

## 0. Mandatory Plan Review Amendments Folded (ALL REQUIRED)

- **`P0-1` (Mask Binary Stream Payloads in `root_structure` & Fail Closed on Unsupported `/Filter`):**
  - Build `root_structure: Vec<u8>` with all extracted `stream..endstream` binary payloads replaced by ASCII spaces while keeping all object headers and `<< ... >>` stream dictionaries intact. Run root `/Subtype /Image`, `/Type /Page`, `/Pages /Count`, and literal-string scans ONLY on `root_structure` (never on compressed zlib ciphertext!).
  - Inspect streams by top-level `/Filter` key (depth 1, handling zero-whitespace `/Filter/FlateDecode` & `/Filter[/FlateDecode]`):
    - Absent `/Filter`: inspect raw payload directly.
    - Sole `/Filter /FlateDecode` or `/Filter [/FlateDecode]`: inflate via RFC 1950 `inflate_zlib` and **fail closed** on any inflation error; inspect decompressed bytes only.
    - Any other `/Filter` or multi-filter array (`/DCTDecode`, `/JPXDecode`, `/JBIG2Decode`, `/CCITTFaxDecode`, `/ASCII85Decode`, `/ASCIIHexDecode`, `/LZWDecode`, etc.): **fail closed** with an unsupported/disallowed stream filter error.
- **`P0-2` (Balanced Nested `<< ... >>` Dict Parsing, Indirect `/Length N 0 R`, Zero Greedy EOL Strip):**
  - Match standalone `stream` + (`\r\n` | `\n` | bare `\r` not `\r\n`), advancing cursor past `endstream` in `O(1)`.
  - Extract enclosing stream dictionary immediately preceding `stream` using balanced `<<`/`>>` depth tracking (ignoring `(...)` strings), reading `/Length`, `/Filter`, `/Type`, `/Subtype`, `/Length1` ONLY at top-level depth 1.
  - Treat `/Length N` as direct byte length ONLY if NOT followed by `<digits> R`. On direct `/Length N`, verify `data_start + N <= raw.len()` and standalone `endstream` immediately after optional whitespace/EOL, taking exact `raw[data_start..data_start + N]` with ZERO EOL stripping! On indirect `/Length`, scan for standalone `endstream` and strip at most ONE trailing `\r\n` or `\n` (never greedy `.strip(b"\r\n")` which corrupts Adler-32 ending in `0x0A`/`0x0D`!).
  - Confirm `/Type /XRef ... /Filter /FlateDecode /DecodeParms << /Columns ... /Predictor 12 >>` inflates cleanly via `inflate_zlib`.
- **`P0-3` (`zero_python_repository_and_ci_guard` vs Branch Name `"v2/zero-python-evolution"`):**
  - Keep ALL step names, comments, and `run:` blocks in `.github/workflows/ci.yml` free of `python`/`python3` wording (use `"pure-Rust"` / `"std-only Rust"`).
  - In `zero_python_repository_and_ci_guard` (`crates/aodc/tests/source_binding.rs`):
    - Unconditionally reject `python3`, `python2`, `pypy`, `pytest`, `pip`, `pip3`, `unittest`, `setup-python`, `virtualenv`, `conda`, `poetry`, and `.py` file tokens on every line of `.github/workflows/*.{yml,yaml}`.
    - Reject bare `python` (case-insensitive) on every line EXCEPT after stripping the exact allowlisted literal `"v2/zero-python-evolution"` on `branches:` lines only (never allowlisted in `run:`, `uses:`, `env:`, or `name:`).
    - Repo walk skips `.git` & `target`, uses `entry.file_type()` (never follows symlinks), rejects any `.py`/`.pyc`/`.pyo`/`.pyi` file or `__pycache__` dir, and reads only the first 128 bytes per file for `#!` Python shebang detection. Do NOT scan upstream provenance strings in `SOURCE_BINDING.json` or `bindings/aod/s2_vectors/*.jsonl`.
- **`P0-4` (4 RFC 1950/1951 Inflater Invariants for Real Typst `0.15.1` `miniz_oxide` Streams):**
  1. `BitReader` alignment before stored `BTYPE=00` blocks and before Adler-32 trailer MUST discard `bit_cnt & 7` bits AND rewind `pos -= usize::from(bit_cnt / 8)` (zeroing `bit_buf`/`bit_cnt`), verifying `pos + 4 <= input.len()` (and exact consumption up to optional trailing PDF EOL bytes after the 4-byte big-endian Adler-32; note `inflate_zlib` verifies exact Adler-32 at `pos..pos+4`, and allow `pos + 4 == input.len()` or only trailing PDF whitespace if any, while rejecting truncated trailers!).
  2. Allow empty distance tree (`HDIST=1, dist_len=[0]`) in `HuffmanTable::build` (failing only if a distance symbol is actually decoded); reject empty `code-length` or `litlen` tables and require EOB `256` length `> 0`.
  3. Single-symbol Huffman tree allowed iff sole symbol has `len == 1` (code `0` valid, code `1` invalid); reject all other incomplete or oversubscribed trees.
  4. Reject `CM != 8`, `CINFO > 7`, `(cmf << 8 | flg) % 31 != 0`, `FDICT != 0`, `hlit > 286`, `hdist > 30` (or decoded `dist >= 30`), decoded litlen `286..=287`, `dist == 0`, `dist > out.len()`, `dist > (1 << (cinfo + 8))`, RLE `16` at index 0, RLE overflow `> hlit + hdist`; length code `285` = `258` with `0` extra bits; enforce `MAX_STREAM_DECOMP_BYTES` (`32 MiB`) and `MAX_TOTAL_DECOMP_BYTES` (`128 MiB`) BEFORE growing `out`; decode Huffman bit-by-bit (zero EOF-peek bugs).
- **`P1-1` (Inline-Image `BI..ID..EI`, Binary Font `"??"`, Comments & ISO 32000-1 Names):**
  - `BI..ID..EI` scanner requires `BI..ID` dict span `<= 8192` bytes, printable ASCII + PDF whitespace only, and at least one `/` inline-image dict key (`/W`, `/Width`, `/H`, `/Height`, `/BPC`, `/BitsPerComponent`, `/CS`, `/ColorSpace`, `/IM`, `/ImageMask`, `/F`, `/Filter`, `/D`, `/Decode`, `/I`, `/Interpolate`), followed by `ID<ws>` and token-bounded `EI`.
  - For binary font streams (`/Length1` or `/Subtype` in `{/Type1C, /CIDFontType0C, /OpenType}`) and `/Type /XRef`, still run `/Subtype /Image` and long `UNRESOLVED_MARKERS`, but skip 2-byte `"??"` to avoid font-bytecode false positives.
  - Handle `% ... \r|\n` PDF comments outside strings, zero-whitespace adjacent PDF names (`/Subtype/Image`, `/Type/Page`) terminated by PDF whitespace or delimiters `()<>[]{}/%`, and `/Count N` preceding OR following `/Type /Pages`.
- **`P1-2` (Host-Path-Free `aod.dec.pdf.audit.v1` JSON & Symmetric Symlink/Size Guards):**
  - Zero host paths and zero timestamps in `PdfAuditReport::to_json()` (logical tags `"pdf"`, `"compare_pdf"`, `"root"`, `"stream[N]"` in `errors`, sorted `BTreeMap` keys: `bytes`, `compare_sha256`, `deterministic_match`, `errors`, `min_pages`, `pages`, `raster_violations`, `schema`, `sha256`, `status`, `streams_flate_inflated`, `streams_total`, `unresolved_violations`).
  - Apply `symlink_metadata` (`!is_symlink() && is_file()`) + `File::take((MAX_PDF_BYTES + 1) as u64)` bounded read symmetrically to BOTH `<pdf>` and `--compare-pdf <pdf2>`.
  - Reuse `parse_positive_usize` for `--min-pages` in `main.rs`, reject unknown `-*` flags, emit compact JSON on stdout under `--json` for both `PASS` (exit 0) and `FAIL` (exit 1).
- **`P1-3` (`AC-08` Zero-LLVM Proof & Safe CI `ldd` / Artifact Handoff):**
  - Verify in `tests/pdf_audit.rs`: (a) zero `aod_llvm` imports in `pdf_audit.rs`, (b) `ldd env!("CARGO_BIN_EXE_aodc")` has zero `llvm`/`python`/`gmp`, (c) `DEC_LLVM_LIBRARY=/nonexistent/libLLVM.so` + `aodc audit-pdf` exits 0.
  - In `.github/workflows/ci.yml`: capture `LDD_OUT="$(ldd ./target/release/aodc)"`, print it, check `if echo "$LDD_OUT" | grep -qiE 'llvm|python|gmp'; then exit 1; fi`; `actions/upload-artifact@v4` with `if-no-files-found: error` (`name: aodc-release-bin`, `path: target/release/aodc`); `typst-pdf-and-zero-raster-audit` declares `needs: [rust-native-and-manual-verify]`, downloads to `/tmp/aodc-bin`, `chmod +x /tmp/aodc-bin/aodc`, runs both human and `--json` `audit-pdf` + `pdfimages -list == 0`.
- **`P2-1` & `P2-2` (Docs Hygiene & Required Edge-Case Tests):**
  - Avoid literal `"??"`, `"[unresolved]"`, `"PANIC"`, or `aod-lint` triggers in `docs/manual/chapters/06_validation_and_limits.typ`; fix stale `/ImageB|/ImageC|/ImageI` claim in `README.md`; keep `pdf_audit.rs` free of bare `f32`/`f64`/`unsafe` identifiers on non-`//` lines.
  - Cover all `P2-2` edge cases in `crates/aodc/tests/pdf_audit.rs`: verified `BTYPE=10` dynamic-Huffman golden vectors (`(z[2] >> 1) & 3 == 2`), multi-block mixed `00+01+10` rewind, empty & 1-symbol distance trees, Adler-32 ending in `0x0A`/`0x0D`, nested `/XRef` `/DecodeParms << /Columns 5 /Predictor 12 >>` + indirect `/Length 99 0 R`, compressed stream containing unmatched `0x28` followed by uncompressed `/Subtype /Image`, unsupported `/Filter /ASCIIHexDecode`, symmetric symlink + `> MAX_PDF_BYTES` bounded-read rejection, `/Count N` preceding `/Type /Pages`, and `#XX` PDF name hex normalization if added.

## 1. Exact File Plan
- **Create:** `crates/aodc/src/pdf_audit.rs`, `crates/aodc/tests/pdf_audit.rs`
- **Modify:** `crates/aodc/src/{lib.rs, main.rs}`, `crates/aodc/tests/source_binding.rs`, `.github/workflows/ci.yml`, `docs/manual/chapters/{00_quickstart.typ, 06_validation_and_limits.typ}`, `README.md`, `.gitignore`
- **Delete:** `scripts/audit_pdf_zero_raster.py`, `scripts/test_audit_pdf_zero_raster.py`

## 2. Acceptance Criteria (`AC-01`..`AC-12`)
`AC-01`..`AC-12` per Rev 1 strengthened with `P0-1`..`P0-4` and `P1-1`..`P1-3` above.
