# PR 5 Independent Verification Bundle (R01 PASS — 14/14)

- **Repository:** `/home/antigravity/rpoyau/aod-dec` (`v2/native-reference-engine`, uncommitted PR 5 diff atop `1488c9f`)
- **Validator:** `3c8f2a9417076546cf765d71d5b6ccca`
- **Verdict:** **PASS — 14/14 Acceptance Criteria Verified (`AC-01`..`AC-14`), 0 FAIL**

## Headline Evidence
- `RUSTFLAGS="-D warnings" cargo test --workspace --offline`: **62/62 PASS**, 0 warnings.
- `RUSTFLAGS="-D warnings" cargo test --workspace --release --offline`: **62/62 PASS**, 0 warnings.
- `RUSTFLAGS="-D warnings" cargo build --workspace --release --offline`: **PASS**, 0 warnings.
- 4-crate static scan (40 `.rs` files): **0 `f32`/`f64`, 0 float literals, 0 disallowed `unsafe`**, `Cargo.lock` = `['aod-core', 'aod-llvm', 'aod-rt', 'aodc']`.
- Vendored M4 bindings intact: `aod_schema.typ` (`4e686136...d46`), `main_registry.json` (`e9044676...884`), `SOURCE_BINDING.json` unchanged, `/home/antigravity/rpoyau/aod` clean (`""`).
- Live `target/release/aodc manual-verify docs/manual` & `--json`:
  - `"status": "PASS"`, `files_scanned=13`, `main_refs_checked=47`, `internal_labels=16`, `internal_refs_checked=16`, `citations_checked=14`, `inline_positive_checked=6`, `inline_negative_checked=5`, `dec_files_checked=12`, `data_files_checked=3`, `raster_violations=0`, `aod_lint_violations=0`, `errors=[]`, zero leaked `/tmp` ELFs.
- Independent exact 4-regex `rpoyau/aod-lint/src/passes/typst_ast.rs` audit across all 13 `.typ` files: **0 hits**.
- Independent hostile CLI tamper probes in isolated `/tmp/aod_dec_pr5_val` sandbox: **17/17 rejected (`rc=1`)** including inline output 1-digit tamper, unpaired ```` ```dec ````, passing negative block, missing/prefixless/wrong-kind Main ref, stale digest, broken `#secref`, missing `#aod-cite`, escaping `#include`, escaping symlink, `#image("x.png")`, unanchored `@foo`, vague range, lowercase multi-word midstream import, `"smooth manifold"`, JSON float (`1.5`), and live-fixture drift (`410 -> 411`).
- `scripts/test_audit_pdf_zero_raster.py`: **6/6 PASS** under both `python3` and `python3 -O`; AST confirms 0 bare `assert`s and stdlib-only imports.
- Regression (`AC-14`): `v1_examples` 10/10, `v2_smoke` 3/3, `manual_examples` 12/12, `probe-hw` 260 ms.
- Hygiene note (`P3`): remove `scripts/__pycache__/` and add `__pycache__/` + `*.pyc` to `.gitignore` before committing.
