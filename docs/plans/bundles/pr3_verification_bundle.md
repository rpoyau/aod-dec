# PR 3 Independent Verification Bundle — `rpoyau/aod-dec` (`v2/native-reference-engine`)

**Overall Verdict:** **PASS — 16/16 Acceptance Criteria Verified (`AC-01`..`AC-16`), 0 FAIL**

- `RUSTFLAGS="-D warnings" cargo test --workspace --offline` & `--release --offline`: **50/50 PASS**, 0 warnings; 0 external crates; 4-crate static scan across 35 `.rs` files: `f32/f64=0`, `float_lits=0`, `disallowed_unsafe=0`.
- `AC-02`: Missing `--stdlib` and missing `AOD_STDLIB_DIR` fail strictly with `ModuleNotFound` (zero silent fallback); `--stdlib` beats bad `AOD_STDLIB_DIR`.
- `AC-03`: 30/30 hostile 2-segment (`SiMetrology::*`) & 3-segment (`metrology::SiMetrology::*`) probes in native `Dec`/`chamber`/`prepare`/`observe` × `check`/`build`/`jit` fail `QExtLeakageForbidden` (0 output binaries created); positive 2-seg/3-seg in `quarantine Q_ext:` + reverse-alphabetical 4-module topological `const` chain pass identically across VM, AOT `-O2`, ORC JIT `-O2`.
- `AC-04`..`AC-10`: **30/30** 5-mode invocations across all 6 `manual_examples/01..06.dec` (VM, AOT `-O0`/`-O2`, ORC JIT `-O0`/`-O2`) match committed sibling `.stdout` SHA-256 digests 100% byte-for-byte under `DEFAULT_FUEL = 100_000`.
- `AC-11`: All 6 hermetic `DecIR v2` bundles execute identically from `/tmp` with `AOD_STDLIB_DIR=/tmp/nonexistent_stdlib`; 1-byte tamper inside bundled `"stdlib::aod::ledger"` fails with `IrVerificationError`.
- `AC-12` & `AC-13`: `aodc test manual_examples` -> `12 passed; 0 failed; 12 total`; `nc01..nc03` fail compile-time (`check`/`run`/`build`/`jit`, 0 binaries created), `nc04..nc06` fail runtime across VM/AOT/JIT; all 4 hostile `.expect_error` edge cases (empty, dual `.stdout`+`.expect_error`, unexpected pass, wrong substring) fail `aodc test`.
- `AC-14`..`AC-16`: Exact rational cross-check vs `rpoyau/aod@98ddfa0` PASS, zero `R01`/`R02` violations, zero hardcoded physical targets in `03`, `06`, `boundary.dec`; `v1_examples` 10/10, `v2_smoke` 3/3; `/home/antigravity/rpoyau/aod` completely clean (`""`).
