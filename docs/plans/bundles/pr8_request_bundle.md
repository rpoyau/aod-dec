# PR 8 Request Bundle (Rev 2 — Approved with P0-1..P0-4 & P1-1..P1-3 Folded)

- **Repository:** `/home/antigravity/rpoyau/aod-dec` (`v2/zero-python-evolution` atop `d74651f`)
- **Upstream `S4` Snapshot:** `rpoyau/aod` PR `#53`, branch `m4-s4-rust-exact-core`, commit `e0134017a816c699ee71a4342044c007b63917db`, tree `789e28b5c0c7c24c91f580b643e1bd0017a89dd6`, base `3a2418defb4fa92e6fbfc8951b81990fc57ce0e6`, root `wavelet_shedding_simulation.csv` (`11,245` bytes CRLF, SHA-256 `d7b0592997d47b2b550508d663afe8943a98ba9d58d087ce77e9d4d0a0c3ff40`).
- **Hard Invariants:** Zero `.py` files, zero Python in CI (`zero_python_repository_and_ci_guard`), `Cargo.lock` exact 4 crates, zero floats across all 4 crates, `#![forbid(unsafe_code)]` on `aod-core` & `aodc`, unchanged 5-file `unsafe` allowlist, zero mutations to `/home/antigravity/rpoyau/aod` or existing `S0`/`S1b`/`S2`/v1 bindings.

## 0. Mandatory Plan Review Amendments Folded (ALL REQUIRED)
- **`P0-1` (Exact `upstream_path` & Byte-for-Byte CRLF Copy):**
  - Copy `scratch/inspect/aod-s4/wavelet_shedding_simulation.csv` byte-for-byte preserving CRLF (`11,245` bytes, SHA-256 `d7b0592997d47b2b550508d663afe8943a98ba9d58d087ce77e9d4d0a0c3ff40`) to `bindings/aod/s4_fixtures/wavelet_shedding_simulation.csv`.
  - In `SOURCE_BINDING.json` (`upstream_bindings[3]`), record `"upstream_path": "wavelet_shedding_simulation.csv"`, `"local_path": "bindings/aod/s4_fixtures/wavelet_shedding_simulation.csv"`, `id: "S4@e013401"`, `stage: "S4"`, `status: "DRAFT_CANDIDATE_UNDER_S4_REVIEW"`, `pr_number: 53`, `ref_name: "m4-s4-rust-exact-core"`, `commit: "e0134017a816c699ee71a4342044c007b63917db"`, `tree: "789e28b5c0c7c24c91f580b643e1bd0017a89dd6"`, `base_commit_binding: "3a2418defb4fa92e6fbfc8951b81990fc57ce0e6"`. Assert all fields & SHA-256 in `crates/aodc/tests/source_binding.rs`.
- **`P0-2` (Exact Upstream `cycle_shedding_trace` JSON Schema — `scratch/inspect/aod-s4/ops.rs:1156-1248`):**
  - Inputs: `initial_burden`, `eta`, `gamma`, `lambda_reclose`, and `ticks: [{ "lead", "lag", "compatibility" }]`. Every rational accepts either canonical `["num", "den"]` pair OR printed decimal string (`parse_printed_rational(s, false, false)`). Enforce `ticks.len() <= 100_000` (`MAX_STEPS_GUARD`).
  - Outputs: `final_burden` (equals `initial_burden` when `ticks` empty), `max_burden`, `max_excess`, `max_span`, `ticks` (7-key objects: `before_clipping`, `local_reclosure`, `outward_shedding`, `padar_burden`, `remainder_sum`, `shed_excess`, `span`), `total_local`, `total_outward` — all canonical `["num", "den"]` pairs.
  - Format `aodc eval-op` output and `serialize_s4_report` via `JsonValue::to_pretty_utf8_json()` (`sort_keys=True`, indent 2, trailing `\n`).
- **`P0-3` (Explicit Exit Code `2` Dispatch & Pre-Execution `--report` Ordering in `main.rs`):**
  - Refactor `main.rs` dispatch to return explicit exit code (`CliErr { code: u8, msg: String }`), preserving exit `1` for existing subcommands while returning exact exit `2` for CLI usage errors and unsafe `--report` paths on `aodc eval-op` and `aodc verify-s2`.
  - Validate `--report` via `report_guard::validate_runtime_output_path` BEFORE reading inputs/vectors/CSV or creating directories; NEVER create or write `--report` when `eval-op` fails with exit `1`.
- **`P0-4` (`aod.s4.rust.report.v1` Schema Alignment, Pre-Validation Sanitization & Strict CSV Verification):**
  - Top-level keys matching upstream `report.rs:405-428`: `base_commit_binding`, `crate_name`, `crate_version`, `cycle_shedding_fixture`, `overall_status`, `policy_audit`, `schema` (`"aod.s4.rust.report.v1"`), `vectors_verification` (retaining `scientific_status_summary` with `RC-020` `UNRESOLVED_PROVENANCE_QUARANTINED: 1`). Enforce all 20 `FORBIDDEN_REPORT_KEYS`, host-path prefixes, temp markers, and ISO timestamp rejection.
  - Run `sanitize_error(&msg, &extra_roots)` on ALL vector/fixture/loader error strings BEFORE `validate_portable_report(&root, "$")` so failing runs still emit clean portable `FAIL` JSON!
  - `verify_s4_cycle_shedding_csv`: read raw CSV bytes (no symlink, `<= 16 MiB`), verify raw-byte SHA-256 against `SOURCE_BINDING.json` `S4@e013401`, check exact 10-column header, exact 90 rows (`tick == 0..89`, `state == "open"`), exact zero-residual conservation at every tick (`outward + local == remainder_sum == shed_excess`, `padar_burden + shed_excess == before_clipping`, `padar_burden <= compatibility`, exact shedding support `29..=39` with `padar_burden == compatibility`, strict `0/1` outside `29..=39`), `|exact - csv_parsed| <= 1/10^14` on all 5 derived columns, and `max_denominator_digits > 18`.
- **`P1-1` (Ancestor-Based Repo-Root Discovery & Atomic Symlink-Safe Write in `report_guard.rs`):**
  - Walk `--report` path's lexical ancestors (in addition to `cwd`, `current_exe`, and explicit `--binding`/`--vectors`/`--fixture` roots) to add any enclosing repo root (containing `SOURCE_BINDING.json` or `.git`) to `guarded_roots` so targeting `/path/to/aod-dec/README.md` from `/tmp` still exits `2`!
  - Scope parent-symlink rejection to `guarded_roots` (plus unconditional target symlink rejection) so OS `/tmp -> /private/tmp` never false-fails while `_build/escape -> /tmp` and dangling symlinks always exit `2`.
  - In `write_report_atomic`, use `OpenOptions::new().write(true).create_new(true)` on sibling `.tmp.<pid>.<seq>`, remove temp file on any error (zero orphans), and never call `create_dir_all` before `validate_runtime_output_path` succeeds.
- **`P1-2` (Bounded `16 MiB` Reads & Zero Silent Fallback):**
  - Enforce `symlink_metadata` (`!is_symlink() && is_file()`) + `File::take((16 * 1024 * 1024 + 1) as u64)` on `--input` & `--fixture`, and `io::stdin().take((16 * 1024 * 1024 + 1) as u64)` on `--input-stdin`. Zero silent fallback on explicit overrides; backward-compatible `aodc verify-s2 [--json]` without `--s4-report`.
- **`P1-3` (Pure-Dec `10_s4_cycle_shedding_90tick.dec` Exact CSV Parity & Count Sync):**
  - `stdlib/aod/sectors.dec`: strictly pure-native parameterized `CycleSheddingTraceSummary` + `cycle_shedding_trace_summary` (zero `Q_ext`, assert equal array lengths).
  - `crates/aodc/tests/s4_convergence.rs`: cross-check `10_s4_cycle_shedding_90tick.stdout` directly against `aod_core::s2_kernel::cycle_shedding_trace` computed from vendored CSV, plus 5-mode (`VM`, `AOT -O0/-O2`, `JIT -O0/-O2`) + hermetic `DecIR v2` parity within `DEFAULT_FUEL = 100_000`.
  - Bind `// @dec-file: manual_examples/10_s4_cycle_shedding_90tick.dec` in Chapter 3 and sync counts (`16 -> 17`, `9 -> 10`, `upstream.len() 3 -> 4`) across `source_binding.rs`, `manual_stdlib_parity.rs`, `manual_verify.rs`, `docs/manual/`, and `README.md`.
- **`P2-1` (Static Audit, `aod-lint` & Zero-Python CI Hygiene):**
  - Zero bare `f32`/`f64`/`unsafe` identifiers on non-`//` lines in `s4_report.rs`/`report_guard.rs`; zero unresolved-marker literals or `aod-lint` hits in `docs/manual/`; zero Python tokens in `.github/workflows/ci.yml`.

## 1. Exact File Plan
- **Create:** `bindings/aod/s4_fixtures/wavelet_shedding_simulation.csv`, `crates/aod-core/src/s4_report.rs`, `crates/aodc/src/report_guard.rs`, `manual_examples/10_s4_cycle_shedding_90tick.{dec,stdout}`, `crates/aodc/tests/s4_convergence.rs`
- **Modify:** `SOURCE_BINDING.json`, `crates/aod-core/src/{lib.rs, s2_kernel.rs, s2_vectors.rs}`, `crates/aodc/src/{lib.rs, main.rs}`, `stdlib/aod/sectors.dec`, `crates/aodc/tests/{source_binding.rs, manual_stdlib_parity.rs, manual_verify.rs}`, `docs/manual/{refs.bib, main.typ, chapters/00_quickstart.typ, chapters/03_aod_stdlib_and_manual_fixtures.typ, chapters/06_validation_and_limits.typ}`, `.github/workflows/ci.yml`, `README.md`.

## 2. Acceptance Criteria (`AC-01`..`AC-14`)
All 14 ACs from Rev 1 strengthened with `P0-1`..`P0-4` and `P1-1`..`P1-3` above.
