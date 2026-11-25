# PR 5 Request Bundle (Rev 2 — Approved with P0-1..P0-4 & P1-1..P1-2 Folded)

## M4-Aligned Modular Typst Manual, `aodc manual-verify`, Pure-Vector Figures, `aod-lint` Alignment & CI

**Base Commit:** `1488c9f` (PR 4 committed: 16/16 Validator PASS, 58/58 tests debug & release, Reviewer PASS 0 P0/P1/P2)
**Vendored Read-Only M4 Bindings (MUST REMAIN BYTE-FOR-BYTE UNTOUCHED):**
- `bindings/aod/aod_schema.typ` (SHA-256 `4e6861360657c906ab18499afd1c114b87c8521ead1df471ddc28ab8e9c44d46`)
- `bindings/aod/main_registry.json` (`455` entries, `main_sources_sha256`: `a05a787d75633c95e09c3b15fa9900a270d4a4bb31418a2423c7e8121fac9289`, file SHA-256 `e9044676f91fdcaeffa8f1a1d21d605358a0923ec9fd6788146822b6d496c884`)

## 0. Mandatory Plan Review Amendments (ALL REQUIRED)
- **`P0-1` (Root-Relative `/` Typst Paths & Exact `aod_schema.typ` Contracts):**
  - ALL `#import`, `bib-path`, `main-registry-path`, and `json(...)` calls use leading `/` anchored to `--root .`: `#import "/bindings/aod/aod_schema.typ": *`, `bib-path: "/docs/manual/refs.bib"`, `main-registry-path: "/bindings/aod/main_registry.json"`, `expected-main-digest: "a05a787d75633c95e09c3b15fa9900a270d4a4bb31418a2423c7e8121fac9289"`, `json("/docs/manual/data/<file>.json")`.
  - `#aod-flowchart(nodes, edges, width: ..., height: ...)` MUST pass Typst lengths (`cm`/`pt`, NOT bare integers) for `x`, `y`, `w`, `h`.
  - Use `#aod-cite("key")` (never bare `@key`), `#figref` on figures, `#tabref` on `#figure(kind: table, table(...), caption: [...])`.
- **`P0-2` (Pass 1 Typst Lexer/Scanner):**
  - Properly lex `//` comments, nested `/* ... */` comments, fenced raw blocks (exact closing backtick count `>= 3`), inline raw spans `` `...` ``, escaped characters (`\[`, `\]`, `\$`, `\\`), double-quoted strings (NEVER ASCII apostrophes `'` in prose), and **math spans `$ ... $`** (check `$` balance; do NOT cross-check `(` vs `]` inside `$ ... $` so half-open windows `$(0, 6]$` pass cleanly!).
- **`P0-3` (Exact `aod-lint` `TypstAstPass` Rules in Pure `std` Rust Pass 4):**
  - Implement exact pure-Rust equivalents of all 4 `TypstAstPass` rules over non-raw Typst text (prose, captions, table cells, noteblocks):
    1. Unanchored `@ident` not immediately followed by `[`.
    2. Vague ranges (`(?i)\b(see|refer to|in)\s+(§§?\s*\d+\s*[-–—]\s*\d+|sections?\s+\d+\s*[-–—]\s*\d+|the literature)\b`).
    3. Midstream premise imports (`(?i)\b(by|via|using)\s+[a-z0-9_\-\s]+(law|equation|theorem|principle)\b` — note `(?i)` spans spaces and lowercase words!).
    4. Continuum terms (`(?i)\b(smooth\s+manifold|continuous\s+manifold|hilbert\s+space)\b` — forbidden even in negative prose sentences!).
  - Author all `docs/manual/**/*.typ` prose strictly free of all 4 trigger patterns.
- **`P0-4` (Pass 1/5/6 Containment, `SOURCE_BINDING.json`, 3-Mode Negative Controls, RAII Temp Guard & Live Data Parity):**
  - Canonicalize `#include` targets inside `manual_dir` (reject cycles & escaping paths) and `// @dec-file:` targets inside `repo_root` (reject path traversal & escaping symlinks).
  - Verify `SOURCE_BINDING.json` by walking `upstream_bindings[*].vendored_files[*]` (`local_path` + `sha256` against actual on-disk bytes).
  - Pass 5: compile-time negative controls verify compile-time diagnostic substring AND confirm `build_aot`/`jit` reject without emitting an executable; runtime negative controls verify ALL 3 modes (VM, AOT `-O2`, ORC JIT `-O2`) fail at runtime with VM diagnostic matching expected substring; use a RAII `Drop` guard for temp AOT ELF directory so zero temp binaries leak.
  - Pass 6: parse `docs/manual/data/*.json` via `aod_core::json::parse_strict_json` AND execute `manual_examples/{03_short_window_boundary, 04_cell_measures_tesseract, 06_unbiased_chamber_sandbox}.dec` live, asserting field-by-field parity against live stdout!
- **`P1-1` (`scripts/audit_pdf_zero_raster.py`):**
  - Stdlib-only Python 3. Decompress all `/FlateDecode` streams (including PDF `0.13.1` `/ObjStm` & content streams) via `zlib.decompress`; check `/Subtype /Image`, `/ImageB`/`/ImageC`/`/ImageI`, and inline `BI...ID...EI` outside PDF string literals; check unresolved-ref markers (`??`, `[unresolved]`, `Unresolved Main reference`, `Unresolved internal reference`, `Unresolved citation`, `PANIC`); verify `--min-pages` and `--compare-pdf` exact SHA-256 determinism; NEVER use bare `assert` so all checks stay active under `python3 -O`.
- **`P1-2` (`.github/workflows/ci.yml`):**
  - 3 jobs on `ubuntu-24.04`:
    1. `rust-native-and-manual-verify`: install `llvm-18` (`libllvm18`, `clang`), `RUSTFLAGS="-D warnings" cargo test --workspace --offline` & `--release`, `aodc test` (`v1_examples`, `v2_smoke`, `manual_examples`), `aodc probe-hw`, `aodc manual-verify docs/manual --json`, Python PDF auditor unit tests (`python3` & `python3 -O`).
    2. `typst-pdf-and-zero-raster-audit`: install `fonts-lmodern` + `fonts-dejavu-core`, pinned `typst v0.13.1`, double-build with `--root . --creation-timestamp 0` (and `SOURCE_DATE_EPOCH=0`), run `scripts/audit_pdf_zero_raster.py --compare-pdf --min-pages 6`, upload PDF artifact.
    3. `aod-lint-strict`: checkout `rpoyau/aod-lint` with `token: ${{ secrets.AOD_LINT_TOKEN || github.token }}`, build `aod-lint`, run `aod-lint check --strict` over every `.typ` under `docs/manual/` (no `continue-on-error`).

## 1. Exact File Tree
- `.github/workflows/ci.yml`
- `README.md`
- `crates/aodc/src/manual_verify.rs` (NEW, `#![forbid(unsafe_code)]`, 100% `std`, zero floats)
- `crates/aodc/src/{lib.rs, main.rs}` (`aodc manual-verify [--stdlib <dir>] [--registry <path>] [--json] <dir>`)
- `crates/aodc/tests/manual_verify.rs` (live `docs/manual`, `--json`, 10+ hostile tamper tests)
- `scripts/audit_pdf_zero_raster.py` + `scripts/test_audit_pdf_zero_raster.py`
- `docs/manual/{main.typ, refs.bib}`
- `docs/manual/chapters/{00_quickstart, 01_language_and_types, 02_compile_time_firewalls, 03_aod_stdlib_and_manual_fixtures, 04_unbiased_apparatus, 05_llvm_and_hw_learner, 06_validation_and_limits}.typ`
- `docs/manual/figures/{fig_architecture_firewalls, fig_double_slit_delta3, fig_disk_enclosure, fig_sandbox_chamber_schematic, fig_unboxed_pipeline_vs_spill}.typ`
- `docs/manual/data/{double_slit_delta3, disk_enclosure, sandbox_chamber_summary}.json`

## 2. Acceptance Criteria (`AC-01`..`AC-14`)
- `AC-01`: 4-crate `std`-only offline debug & release builds + tests (`-D warnings`), 0 external crates, 0 floats, `#![forbid(unsafe_code)]` on `aod-core` & `aodc`, unchanged 5-file `unsafe` allowlist.
- `AC-02`: Vendored `bindings/aod/*` & `SOURCE_BINDING.json` byte-for-byte intact; `/home/antigravity/rpoyau/aod` clean.
- `AC-03`: Complete modular `docs/manual/` (`main.typ`, `refs.bib`, 7 chapters, 5 pure-vector figures, 3 exact-integer JSON data files) with GM Setup-Derivation-Falsification discipline, `R02` preservation (`23/50 - 9/20 = 1/100`), and honest scope notices (`DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED`, `COMPILER_EXECUTION_POLICY_NOT_PHYSICAL_AOD_LAW`).
- `AC-04`: Live `aodc manual-verify docs/manual` exits 0 verifying >= 30 Main refs, internal labels/refs, BibTeX citations, >= 5 inline positive + >= 4 inline negative blocks across VM/AOT/ORC JIT, all 12 `manual_examples/` via `// @dec-file:`, and all 3 data files live-verified against fixtures.
- `AC-05`: `aodc manual-verify docs/manual --json` strict zero-float JSON with `"status": "PASS"`, `"raster_violations": 0`, `"aod_lint_violations": 0`, `"errors": []`.
- `AC-06`: Hostile inline output tamper, negative block unexpectedly passing, and unpaired ```` ```dec ```` rejection.
- `AC-07`: Hostile Main ref missing/prefixless/wrong-kind, stale digest, broken internal ref, duplicate label, missing citation, missing `#include`, path traversal / include cycle rejection.
- `AC-08`: Hostile raster / `image(` / unanchored `@ref` / vague range / `(?i)` multi-word midstream import / continuum term rejection.
- `AC-09`: Hostile JSON data float, duplicate key, and live-fixture drift rejection.
- `AC-10`: Pure-vector figures structural check (zero `image(`, zero rasters, zero `@preview/...`, balanced Typst, length units in `#aod-flowchart`, valid `/docs/manual/data/*.json` paths).
- `AC-11`: `scripts/test_audit_pdf_zero_raster.py` passes under both `python3` and `python3 -O` (clean vector PDF, uncompressed & Flate `/ObjStm` `/Subtype /Image`, PDF string literal non-false-positive, unresolved `??`, min-pages, determinism match/mismatch).
- `AC-12`: `.github/workflows/ci.yml` valid YAML with all 3 jobs, `llvm-18`+`clang`, `fonts-lmodern`+`fonts-dejavu-core`, pinned Typst `0.13.1` double-build determinism, PDF auditor, and `aod-lint check --strict` with token fallback.
- `AC-13`: Honest `README.md` offline vs CI matrix (zero fabricated local PDF build claims).
- `AC-14`: Full PR 1–4 regression gate green in debug & release offline builds.
