# PR 9 Request Bundle (Rev 2 — Approved with P0-1..P0-4 & P1-1..P1-3 Folded)

- **Repository:** `/home/antigravity/rpoyau/aod-dec` (`v2/zero-python-evolution` atop `8dc236c`)
- **Hard Invariants:** Zero `.py` files, zero Python in CI (`zero_python_repository_and_ci_guard`), `Cargo.lock` exact 4 crates, zero floats across all 4 crates, `#![forbid(unsafe_code)]` on `aod-core` & `aodc`, unchanged 5-file `unsafe` allowlist, zero edits to `/home/antigravity/rpoyau/aod`, `SOURCE_BINDING.json`, `bindings/aod/*`, or `tests/fixtures/v2_smoke/` (exact 3 files), zero hardcoded physical outcomes / candidate lists / PDG targets / `Q_ext` in `stdlib/aod/apparatus.dec` or `manual_examples/11_apparatus_opposing_sensors.dec`.

## 0. Mandatory Plan Review Amendments Folded (ALL REQUIRED)

- **`P0-1` (Parallel Observer Soundness, AOT Stdout Capture, `depth` Propagation & Worker `catch_unwind`):**
  - In `crates/aod-rt/src/abi.rs` (`aod_rt_sim_run_ex(ctx, b_h, ticks, parallel_observers: u8)`): if `parallel_observers == 0 || proc_observers.len() <= 1`, take existing sequential fast path. Otherwise snapshot `base_arena = parent_state.arena.clone()` ONCE before the tick loop (preserving all `@__const_*` & pre-sim handles!). On each tick, spawn `std::thread::scope` workers where each worker creates an isolated `OwnedRtContext` with `fuel = start_fuel`, `depth = parent_depth`, `max_depth = parent_max_depth`, `capture_stdout = true`, `arena = base_arena.clone()`, inserts `RtValue::ChamberView(Box::new(snap.clone()))`, and wraps `cb(worker_ctx, snap_h)` in `catch_unwind(AssertUnwindSafe(...))`. Replay worker emits on parent thread in strict declaration order via `parent_state.emit_line(line)` (routes cleanly to `parent_state.emits` in ORC JIT OR locked stdout in standalone AOT ELF!).
  - In `crates/aodc/src/eval.rs` (`#![forbid(unsafe_code)]`): lightweight scoped worker borrowing `&self`'s immutable AST/const maps with local `(fuel, depth, emits)` (no per-tick full AST clone!).
- **`P0-2` (Exact Sequential-Equivalent Fuel Accounting & Error Precedence):**
  - During declaration-order merge of workers `0..k` (in BOTH `abi.rs` and `eval.rs`):
    - `used_i = start_fuel.saturating_sub(worker_i.fuel)`.
    - If `used_i <= remaining_fuel`: `remaining_fuel -= used_i`, replay `worker_i.emits`, and if `worker_i.failed`, propagate `worker_i.error`, set `parent.failed = 1`, `parent.fuel = remaining_fuel`, and halt immediately (discarding workers `> i`).
    - If `used_i > remaining_fuel` (tight-budget boundary): re-run **only** observer `i` once sequentially on parent `ctx` with exact `remaining_fuel` — guaranteeing 100% byte-identical emits, fuel, and `BudgetExhausted` error to sequential execution even under arbitrary tight fuel limits!
- **`P0-3` (Dedicated `manual_data.rs` Output-Dir Guard, Exact `fig_*.typ`/Pass-6 Schema & Zero-LLVM Execution):**
  - Implement `validate_manual_data_output_dir` in `crates/aodc/src/manual_data.rs`: allow outside-repo dirs OR exact `<repo>/docs/manual/data` OR `<repo>/{_build,build,dist,release}/...`, rejecting symlinks along any in-repo prefix, target symlinks, regular-file targets, and all other repo dirs (`.git/`, `crates/`, `bindings/`, `stdlib/`, `SOURCE_BINDING.json`, repo root) with **exit `2` BEFORE executing Dec or touching disk**. Write via `report_guard::write_report_atomic` (zero `.tmp` orphans); on `--check`, read strictly via symlink-rejecting `read_bounded_regular_file` and never create dirs.
  - Preserve EVERY exact key & integer/string type expected by `docs/manual/figures/fig_*.typ` AND `manual_verify.rs` Pass 6 (convert `t1` signs via `int(b.sign)` to exact `-1`/`0`/`1` JSON ints, serialize with `JsonValue::to_pretty_utf8_json()`).
  - Run all Dec generation strictly through Exact Reference VM (`Evaluator`) with ZERO `aod_llvm` calls so `DEC_LLVM_LIBRARY=/nonexistent aodc manual-data --check` succeeds (`AC-12`).
- **`P0-4` (`CodegenPlan` v2 Validation, Combined LLVM 18 `!llvm.loop` Metadata & `PROFILE_VERSION = 2`):**
  - Validate `interleave_count` strictly in `{0, 2, 4}` in `CodegenPlan::validate()`, `CodegenPlan::from_json()`, and `lower_to_llvm_ir_with_plan()`. Include all 9 fields in `plan_id()`, `to_json()`, `from_json()`, with conservative defaults (`vectorize_loops: false`, `interleave_count: 0`, `layout_prefer_soa: false`, `parallel_observers: false`) on `baseline_o0`/`baseline_o2`/`for_opt_level`.
  - Replace `alloc_loop_unroll_md` with `alloc_loop_hint_md` emitting **one** combined self-referential node `!N = distinct !{!N, !M1, ...}` across active hints (`llvm.loop.unroll.*`, `!{!"llvm.loop.vectorize.enable", i1 true}`, `!{!"llvm.loop.interleave.count", i32 2|4}`), returning `None` when no hint is active (preserving baseline IR byte-for-byte!).
  - Bump `PROFILE_VERSION = 2`, update all 5 `CodegenPlan` struct literals and both `profile_version` sites in `crates/aodc/tests/hw_learner_and_probes.rs`, and test that legacy `"profile_version": 1` JSON safely yields `ProfileLoadOutcome::FallbackToBaseline`.
- **`P1-1` (`aod_rt_sim_run` Backward Compat & Symbol Registration):**
  - Keep `aod_rt_sim_run` delegating to `aod_rt_sim_run_ex(..., 0)`. Register `aod_rt_sim_run_ex` in BOTH `runtime_symbol_table()` (`abi.rs`) AND `LLVM_MODULE_PREAMBLE` (`codegen.rs`); emit `@aod_rt_sim_run_ex(ptr %ctx, i64 {b_h}, ptr {tp}, i8 {par_flag})` in `emit_simulate`.
- **`P1-2` (Apparatus Module Scoping):**
  - Keep `stdlib/aod/apparatus.dec` strictly pure structs + `Dec` functions (zero `Q_ext`, using verified `ChamberView`/`SimResult` fields); declare `chamber`, `prepare`, and the 4 `observe` blocks locally in `manual_examples/11_apparatus_opposing_sensors.dec`.
- **`P1-3` (`aodc manual-data` Portable JSON, `--parallel-observers` CLI & `17 -> 18` Count Sync):**
  - Emit strictly host-path-free zero-float JSON (`schema: "aod.dec.manual_data.v1"`, `mode`, `status`, `files_checked`, `files_written`, `files_changed`, `file_digests`, `errors`) under `aodc manual-data --json`; exit `2` for usage/disallowed `--output-dir`, exit `1` for `--check` drift/symlink/missing, exit `0` for PASS.
  - Wire `--parallel-observers` on `aodc run` and `aodc jit` in `main.rs`; bind `// @dec-file: manual_examples/11_apparatus_opposing_sensors.dec` in `docs/manual/chapters/04_unbiased_apparatus.typ`; sync `17 -> 18` counts across `manual_verify.rs:117`, `manual_stdlib_parity.rs:429`, `docs/manual/`, and `README.md`.
- **`P2-1` (Candidate Cap & Hygiene):**
  - Cap `recommend_candidates(&HwPlanHints, &baseline)` at `<= 6` deduplicated plans; zero bare `f32`/`f64`/`unsafe` identifiers on non-`//` lines in `crates/*/src/*.rs`; zero `aod-lint` hits or unresolved-marker literals in `docs/manual/`; zero Python tokens in `.github/workflows/ci.yml`.

## 1. Exact File Plan
- **Create:** `stdlib/aod/apparatus.dec`, `manual_examples/11_apparatus_opposing_sensors.{dec,stdout}`, `crates/aodc/src/manual_data.rs`, `tests/fixtures/v2_bench/soa_vs_aos_bench.dec`, `crates/aodc/tests/pr9_apparatus_data_learner_v2.rs`.
- **Modify:** `stdlib/aod/boundary.dec`, `docs/manual/data/{double_slit_delta3,disk_enclosure,sandbox_chamber_summary}.json`, `crates/aod-llvm/src/{plan.rs,learner.rs,lib.rs}`, `crates/aod-rt/src/abi.rs`, `crates/aodc/src/{eval.rs,codegen.rs,lib.rs,main.rs}`, `crates/aodc/tests/{manual_stdlib_parity.rs,manual_verify.rs,hw_learner_and_probes.rs}`, `docs/manual/chapters/{00_quickstart,03_aod_stdlib_and_manual_fixtures,04_unbiased_apparatus,05_llvm_and_hw_learner,06_validation_and_limits}.typ`, `.github/workflows/ci.yml`, `README.md`.

## 2. Acceptance Criteria (`AC-01`..`AC-15`)
All 15 ACs from Rev 1 strengthened with `P0-1`..`P0-4` and `P1-1`..`P1-3` above (including tight-fuel sequential==parallel equivalence test in `AC-09`!).
