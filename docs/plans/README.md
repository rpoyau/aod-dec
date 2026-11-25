# `rpoyau/aod-dec` Session Continuity, Plans, & Reference Index

This directory preserves the complete orchestration plans, subagent role definitions, Reviewer-audited Request Bundles, and Validator Verification Bundles inside git so any session has full context without external chat state.

---

## 1. Current Repository & Branch State

- **Consolidated Baseline (`main`):** `68d6705` ([PR #8](https://github.com/rpoyau/aod-dec/pull/8) merged; legacy Python PRs `#1`–`#7` retired).
- **Active Evolution Branch (`v2/zero-python-evolution`):** [PR #9](https://github.com/rpoyau/aod-dec/pull/9) (currently open — **hold unmerged** until Phases `D1` → `D2` → `D3` land cleanly):
  - **PR 7 (`d74651f` — Phase A):** 100% Zero-Python purification, pure-Rust RFC 1950/1951 `aodc audit-pdf`, permanent zero-Python repo/CI guard.
  - **PR 8 (`8dc236c` — Phase B):** Stage 4 (`S4@e013401`, `rpoyau/aod` PR `#53`) convergence, 90-tick `cycle_shedding_trace`, `aodc eval-op`, `aod.s4.rust.report.v1`.
  - **PR 9 (`1e5f224` — Phase C):** `stdlib/aod/apparatus.dec`, zero-LLVM `aodc manual-data --check`, `CodegenPlan` v2 + parallel read-only observers (`97/97` tests debug & release, 3/3 CI green).
  - **PR 10 (`d12daab` — Phase D1):** Wave-hardware register & sequence primitives (`Null`/`u8`/`b64`/`t9`/`bytes`/`array[T, N]`, OR-based `superpose_cancel`, eager `select`), `aodc audit-dec` (6 §5.17 hooks), and 2-thread shared-cache-line FRG double-slit probe (`14/14` ACs PASS, `111/111` tests debug & release, Final Review PASS).
  - **PR 11 (Phase D2):** Pure indented `dec <name>:` signature matrices, positional `Struct(...)` + `dec Struct:` overloads, `let (x, y) = ...`, `map[(k, v)]` / `map[]`, 18 inline-lowered AFC scope/sequence/contraction operators, 9 pure container helpers, `aodc fmt`, `--strict-wave` `PureWaveV2`, and pulled-forward `stdlib/aod/*.dec` (`7/7`) + `manual_examples/*` (`18/18`) migration (`15/15` ACs PASS, `125/125` tests debug & release, Final Review R02 PASS).
- **Immediate Next Step (`Phase D3 / PR 12`):**
  - Execute **`Phase D3 (PR 12)`** of **[`dec_wave_purification_plan.md`](dec_wave_purification_plan.md)** (port 7 `reference/aod-dec-dev/dec/` modules into `examples/first_principles/`, migrate `docs/manual/chapters/*.typ` & `tests/fixtures/v2_*`, eliminate native `str` in Dec v2, flip `CompatMode::V2` unconditionally to `PureWaveV2`, and add `pure_wave_dec_v2_repository_guard`).

---

## 2. Directory Contents

1. **Master Plans & Status:**
   - [`dec_wave_purification_plan.md`](dec_wave_purification_plan.md) — **Active** 3-phase First-Principles Wave-Language Purification Plan (`Phase D1` → `D2` → `D3`: lowercase `dec <name>:` signature matrices, zero `{ }`, zero `if`/`for`/`while`/`break`/`continue`/`mut`, hardware register & `bytes` primitives, AFC scope operators, §5.17 audits, and FRG double-slit hardware probe).
   - [`orchestration_status.md`](orchestration_status.md) — Milestone tracker (`PR 1` through `PR 12`) following the strict **Lead Orchestrator → Planner → Plan Review → Implementer → Validator → Final Code Review → Commit** pipeline.
   - [`aod_dec_v2_evolution_plan.md`](aod_dec_v2_evolution_plan.md) — Zero-Python evolution plan (`Phases A–C` completed).
   - [`aod_dec_v2_plan.md`](aod_dec_v2_plan.md) — Native Rust + LLVM 18 v2 baseline plan (`PR 1`–`PR 6` merged to `main`).
2. **First-Principles Reference Kit (`aod-dec-dev.zip` Preserved Byte-for-Byte):**
   - [`../FIRST_PRINCIPLES_DERIVATION.md`](../FIRST_PRINCIPLES_DERIVATION.md) — Vendored 9-step First-Principles Derivation (`SHA-256: 091d8f1ef7dd3b9c1f96eab7adc9ff5db67194418c2dbfad1a0fcab68b5afabc`).
   - [`../../reference/aod-dec-dev/`](../../reference/aod-dec-dev/) — Complete 15-file original self-hosting & hardware-learning reference archive (`(cd reference/aod-dec-dev && sha256sum -c SHA256SUMS)` verifies 15/15 OK).
3. **Subagent Role Specifications (`agents/`):**
   - [`agents/planner.md`](agents/planner.md), [`agents/reviewer.md`](agents/reviewer.md), [`agents/implementer.md`](agents/implementer.md), [`agents/validator.md`](agents/validator.md).
4. **Milestone Request & Verification Bundles (`bundles/`):**
   - Completed & verified: `pr1` through `pr11` (`pr{N}_request_bundle.md` + `pr{N}_verification_bundle.md`).

---

## 3. Hard Repository Invariants (Enforced by `crates/aodc/tests/source_binding.rs`)

- **Zero Python:** `0` `.py`/`.pyc` files, `0` Python shebangs, `0` Python steps in `.github/workflows/ci.yml`.
- **`std`-Only 4-Crate Rust Workspace:** `aod-core`, `aod-rt`, `aod-llvm`, `aodc` — `0` external `crates.io` dependencies.
- **Zero Floats & Strict `unsafe` Allowlist:** `0` `f32`/`f64` tokens anywhere in `crates/*/src/**/*.rs`; `#![forbid(unsafe_code)]` on `aod-core` and `aodc`; `unsafe` strictly confined to `crates/aod-rt/src/abi.rs` and `crates/aod-llvm/src/{ffi,aot,jit,hw_probe}.rs`.
- **Read-Only Upstream:** Never mutate `rpoyau/aod`, `rpoyau/afc`, `rpoyau/aod-lint`, `bindings/aod/*`, or `tests/fixtures/v1_examples/*`.
