# PR 6 Independent Verification Bundle (R01 PASS — 15/15)

- **Repository:** `/home/antigravity/rpoyau/aod-dec` (`v2/native-reference-engine`, uncommitted PR 6 diff atop `4ab9a9f`)
- **Validator:** `f3f13c55f80b09d5534d07678ba30524`
- **Verdict:** **PASS — 15/15 Acceptance Criteria Verified (`AC-01`..`AC-15`), 0 FAIL**

## Headline Evidence
- `RUSTFLAGS="-D warnings" cargo build --workspace --offline && cargo test --workspace --offline`: **70/70 PASS** (`17` `aod-core` + `53` `aodc`), 0 warnings.
- `RUSTFLAGS="-D warnings" cargo build --workspace --release --offline && cargo test --workspace --release --offline`: **70/70 PASS**, 0 warnings.
- 4-crate static scan (42 `.rs` files): **0 `f32`/`f64`, 0 float literals, 0 disallowed `unsafe`**, `Cargo.lock` = `['aod-core', 'aod-llvm', 'aod-rt', 'aodc']`, `crates/aod-llvm/` untouched.
- Byte-identical vendored S2 vectors:
  - `bindings/aod/s2_vectors/rational_core.jsonl` (`a852094458a884ec456fd712a1fbfdb8cff46f2e90095b74bafe7644ad72cc99`, 24 vectors)
  - `bindings/aod/s2_vectors/symbolic_and_display.jsonl` (`d2ea7c41ff7fb7b62258cb137576f12f667c38377406d695557fa37b3541d19f`, 6 vectors)
- Live `target/release/aodc verify-s2` & `--json`: **30/30 PASS**, byte-identical across repeated runs, strict zero-float JSON, zero host paths/timestamps, `scientific_status_summary == {"AUTHENTICATED_EXACT": 18, "CONFORMANT_DECLARED_FIXTURE": 5, "RETAINED_DATA_CONFORMANT": 6, "UNRESOLVED_PROVENANCE_QUARANTINED": 1}`.
- 8/8 independent hostile CLI tamper probes in `/tmp` exited 1 with portable zero-float `FAIL` JSON (1-byte tamper, expected output drift, float injection, symlinked `.jsonl`, symlinked `--binding`, missing `--vectors`, `RC-020` upgrade, missing `--binding`).
- 6-way parity (VM, AOT `-O0`/`-O2`, ORC LLJIT `-O0`/`-O2`, hermetic `DecIR v2`) for `manual_examples/07..09.dec`; `aodc test manual_examples` -> `16 passed; 0 failed; 16 total`.
- `aodc manual-verify docs/manual --json` -> `PASS` (`dec_files_checked=16`, `raster_violations=0`, `aod_lint_violations=0`, `errors=[]`), 0 exact 4-regex `aod-lint` hits across all 13 `.typ` files.
- `scripts/test_audit_pdf_zero_raster.py`: 6/6 OK under `python3` & `python3 -O`; `/home/antigravity/rpoyau/aod` clean (`""`).
