# PR 3 Request Bundle (Rev 2 — Reviewer-Amended & Approved) — `rpoyau/aod-dec` v2
## Native Dec Standard Library (`stdlib/aod/*.dec`) & Executable AOD Manual Fixtures (`manual_examples/01..06`)

**Base Commit:** `6b34cec` (PR 2 green across 17/17 checks, 50.21x ORC JIT `-O2` speedup)  
**Upstream Reference Bound (Read-Only):** `rpoyau/aod` `S0@98ddfa0ebb38293adfa99c8c11da5a15cdd9eea3` (`v41.02`, PRs `#43`/`#45`/`#46`)  
**Hard Constraints:** 100% `std`-only Rust workspace (ZERO crates.io deps), ZERO floats (`f32`/`f64`) across ALL 4 crates, `#![forbid(unsafe_code)]` preserved on `aod-core` and `aodc`, ZERO mutations to `/home/antigravity/rpoyau/aod`, strict `R01`/`R02` discipline, ZERO hardcoded physical outcomes.  
**Reviewer Mandatory Amendments Folded:** `P0-1`, `P0-2`, `P1-1`, `P1-2`, `P1-3`.

---

## 1. Compiler Extensions Required in `crates/aodc`

1. **Stdlib Resolver, Strict Precedence & Hermetic `DecIR v2` (`modules.rs`, `P0-2`, `P1-3`):**
   - Preserve built-in `use aod::{core,witness,ledger,wavelet,sim,qext}::...` untouched.
   - Add pure `.dec` stdlib namespace `use stdlib::...` requiring `segs.len() >= 2`; `stdlib_root` ALWAYS points to the `stdlib/` directory itself (containing `aod/ledger.dec`), resolving `<stdlib_root>/<segs[1..]>.dec`.
   - **Strict root precedence (no silent fallback on explicit override):**
     1. If `LoadOptions.stdlib_dir` (`--stdlib <dir>`) is `Some(dir)` -> use ONLY `dir` (fail `ModuleNotFound` if missing; NEVER silently fall back!).
     2. Else if `AOD_STDLIB_DIR` env var is set and non-empty -> use ONLY `AOD_STDLIB_DIR` (NEVER silently fall back!).
     3. Else upward ancestor walk (up to 6 ancestors from `base_dir`, then `current_dir`) for `<dir>/stdlib/aod/ledger.dec`, with compile-time fallback `<CARGO_MANIFEST_DIR>/../../stdlib` if present.
   - **Hermetic `DecIR v2` (`P0-2`):** In `ModuleLoader`, check `if self.embedded.is_some()` FIRST for ALL non-`aod::*` modules (including `stdlib::*`) so `load_hermetic` NEVER inspects `AOD_STDLIB_DIR`, `--stdlib`, or the filesystem. Bundle imported stdlib modules under canonical keys `"stdlib::aod::<mod>"` with exact source bytes + SHA-256.
   - Wire `--stdlib <dir>` across CLI subcommands `check`, `run`, `compile`, `emit-llvm`, `build`, `jit`, `bench`, and `test` (NOT `exec`, which is strictly hermetic).

2. **Topological `const` Initialization Order + Full 2- & 3-Segment Cross-Module `quarantine Q_ext` (`modules.rs`, `checker.rs`, `eval.rs`, `codegen.rs`, `P0-1`):**
   - Record `pub module_load_order: Vec<String>` on `LoadedUnit` (pushed on post-order DFS exit in `modules.rs` so dependencies strictly precede dependents). Iterate `module_load_order` followed by `"<entry>"` in `collect_program_consts` (`checker.rs`), `eval_program_consts` (`eval.rs`), and `index_items` / `@aod_init_consts` (`codegen.rs`).
   - Extend `resolve_qext_submodule` identically across `checker.rs`, `eval.rs`, and `codegen.rs` for BOTH 2-segment (`SiMetrology::DELTA_NU_CS`, `SiMetrology::duration_seconds(...)`) via `ImportedSymbol::ModuleItem` AND 3-segment (`metrology::SiMetrology::DELTA_NU_CS`, `metrology::SiMetrology::duration_seconds(...)`) via `ImportedSymbol::ModuleNamespace` when `qext_modules.contains(&(m, qext_mod))`. Enforce `QExtLeakageForbidden` outside `Effect::QExt` on BOTH 2-segment and 3-segment forms!

3. **Hardened `.expect_error` Negative-Control Support in `run_dec_tests` (`lib.rs`, `P1-1`):**
   - Check `sibling_expect_error = file.with_extension("expect_error")` **BEFORE** loading/checking/running so both compile-time (`nc01..nc03`) and runtime (`nc04..nc06`) negative controls work cleanly.
   - Reject empty/whitespace-only `.expect_error` as FAIL.
   - Reject ambiguous files having BOTH `.stdout` AND `.expect_error` as FAIL.
   - Mark test FAIL if an `.expect_error` file unexpectedly succeeds (`Ok(_)`).
   - Thread `LoadOptions` (`--stdlib`) through `run_dec_tests`.

---

## 2. File Tree
- `crates/aodc/src/{modules.rs, checker.rs, eval.rs, codegen.rs, lib.rs, main.rs}`
- `crates/aodc/tests/manual_stdlib_parity.rs`
- `stdlib/aod/{ledger.dec, wavelet.dec, boundary.dec, cells.dec, metrology.dec}`
- `manual_examples/{01_dec_ledger, 02_closure_wavelets, 03_short_window_boundary, 04_cell_measures_tesseract, 05_conditional_si_metrology, 06_unbiased_chamber_sandbox}.{dec, stdout}`
- `manual_examples/negative_controls/{nc01_qext_delta_nu_leak, nc02_observer_mutates_chamber, nc03_odd_support_missing_yro, nc04_reversed_sweep_sign, nc05_origin_j0_as_beat, nc06_duad_drops_hinge_0h}.{dec, expect_error}`

---

## 3. Standard Library & Manual Fixture Specifications (Verified Against `rpoyau/aod@98ddfa0`)

1. **`stdlib/aod/ledger.dec` + `01_dec_ledger.dec`:** Manual §4 isotropic `(1/4)^4`, anisotropic `(1,1,4,2) -> (1/8,1/8,1/2,1/4)` & role sums `(1/8,1/2,3/8)`, blocked dwell + slide `(3,3,1) -> (3/7,3/7,1/7)` (`6/7`, `1/7`), field tunnelling `6/7` (`RD=5` preserved), ADAR/SADAR `4/1` & `-4/1`, `1:2:9_yro` surplus `3`, Tau missing-burden `31/50`, explicit `R02` gap `23/50 - 9/20 == 1/100 != 0`.
2. **`stdlib/aod/wavelet.dec` + `02_closure_wavelets.dec`:** M3 PR `#45`/`#46` 12 Duon variants, same-current Duad seating (`eq:duad-same-current`), `(3,5] -> 0`, `(-1,1]` origin `1 phase / 0 beats`, `(2,6] -> 1`, `(0,6] ∪ (4,8] -> 3` vs `4`, opposing-current cancellation retaining both distinct histories, all 6 recipient stages, repeated-contact `3/5` & `0/10` (`0/1`) vs single-contact `is_cadence_ratio == false`, exact cyclotomic $\mathbb{Z}_{12}$ DFT $\mathbb{Q}[x]/(x^4 - x^2 + 1)$ on pulse `{0,4,8}` overtone $m=3 \Rightarrow \hat{v}(3) = 1/4$ & exact Parseval sum `1/4` (zero floats!).
3. **`stdlib/aod/boundary.dec` + `03_short_window_boundary.dec`:** Manual §§5–6 11-bin double-slit cross-return $\delta_3$ (`sum T^0=410, sum C^x=38, sum T^x=448, sum O=448, sum Delta=0, sum mag=6`, center `96 vs 95 -> (+1t, 1)`), short-window table (`1:2:7_yro`, `1:2:9_yro`, `1:2:10`), `3:3:7_yro` (`Pi=30, RD=61, rho=7, P^D=21, Q^D=162`, saddle `+3/-3`, purely native — ZERO external Higgs mass map!), moving boundary `8 - 3 + 2 = 7`, scoped channel `4/1` & `-4/1`, Stokes region cancellation (`0/1`), proton `3:3:6` & electron `1:2:6` support accessors.
4. **`stdlib/aod/cells.dec` + `04_cell_measures_tesseract.dec` (`P1-2`):** M3 PR `#43` 4-cube `(16,32,24,8,1)` (`81`, Euler `1`), `V_4=81/16, B_3=27/1` at `a=3/2`, disk enclosures `n = 1, 2, 4, 8, 16` via **quarter-symmetry `0..n × 0..n` × 4** (fits comfortably within `DEFAULT_FUEL = 100_000` in Exact VM!) giving `183/64`, `107/32`, `8/1`, `21/16`, 2x1 rectangle `10/1 -> 8/1 -> 10/1`, exact discrete-law moments (`T`: `19999/19998`, `1/399920004`; `F`: `19999001/20000000`, `1018001/400000000000000`; `Rf`: `7/8`, `1/64`; `U`: `1000000/999999`, `1/999998000001000000`), affine covariance `(8, 18, 12)`, AND 16-step loop+array tesseract cell transport conserving exact mass on EVERY step.
5. **`stdlib/aod/metrology.dec` + `05_conditional_si_metrology.dec` (`P1-2`):** Strictly quarantined `quarantine Q_ext SiMetrology:` (`DELTA_NU_CS = 9192631770`, `H_HELLWIG_TABLE_IV = 1420405751.7691`, `H_HELLWIG_TABLE_VI = 1420405751.7667`, separate NIST-F2 Aug 2013 & Table 1 `u_B`), verifying `(3,2)` -> exact reduced `1/4596315885` (`== 2/9192631770`) & `13788947655/1`, `(10,22]` `A=7/2, A_R=17/2` -> `17/18385263540` & `64348422390/17`, nonrecurrent `q_a=0`, signed `C_a=-1/2`, `prop:m3-atomic-gain` `lambda=0` vs `1`, H-maser `14204057517679/10000`, and separate NIST-F2 budgets (exercising both 2-segment `SiMetrology::...` and 3-segment `metrology::SiMetrology::...`).
6. **`06_unbiased_chamber_sandbox.dec`:** Opposing `3:3:6` vs `3:3:6` chamber apparatus importing `stdlib::aod::{ledger,boundary}`, tagged `DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED`, zero observer back-reaction across 0/1/multi observer configs, zero hardcoded physical targets.
7. **`negative_controls/nc01..nc06`:** `nc01` (`QExtLeakageForbidden` on `SiMetrology::DELTA_NU_CS`), `nc02` (`ObserverMutationForbidden`), `nc03` (`OddSupportRequiresYro`), `nc04` (`AssertionFailed` reversed sweep), `nc05` (`AssertionFailed` `j=0` origin as beat), `nc06` (`AssertionFailed` Duad dropping `0_H` hinge).

---

## 4. Acceptance Criteria (`AC-01`..`AC-16`)
- `AC-01`: 4-crate `std`-only offline build & tests (0 warnings), 0 external deps, 0 floats, `#![forbid(unsafe_code)]` on `aod-core`/`aodc`.
- `AC-02`: Stdlib resolver priority (`--stdlib`, `AOD_STDLIB_DIR`, upward discovery, fallback) with **zero silent fallback on explicit missing override**, intra-stdlib imports, `aod::*` compat, `ModuleNotFound` on missing.
- `AC-03`: Cross-module `quarantine Q_ext` import (both 2-segment `SiMetrology::*` and 3-segment `metrology::SiMetrology::*`) works inside `quarantine Q_ext:` across VM/AOT/JIT AND fails with `QExtLeakageForbidden` outside `Effect::QExt` before codegen; topological `module_load_order` `const` init verified.
- `AC-04`..`AC-09`: Exact Manual parity for `01_dec_ledger`..`06_unbiased_chamber_sandbox` across all 5 modes (VM, AOT `-O0`/`-O2`, ORC JIT `-O0`/`-O2`) under default fuel (`100_000`).
- `AC-10`: All 6 `manual_examples/01..06.dec` produce 100% byte-identical stdout matching sibling `.stdout` across all 5 execution modes.
- `AC-11`: Hermetic `DecIR v2` bundling stdlib modules executes identically from an isolated temp dir with `AOD_STDLIB_DIR` pointing to a nonexistent path; tampering bundled stdlib bytes fails `loads_ir`.
- `AC-12`: All 6 negative controls (`nc01..nc06`) fail with exact expected diagnostic (`nc01..nc03` compile-time across `check`/`run`/`build`/`jit`; `nc04..nc06` runtime across VM/AOT/JIT); `.expect_error` rejects empty files, dual `.stdout`+`.expect_error`, and unexpected passes.
- `AC-13`: `aodc test manual_examples` runs all 6 positive + 6 negative controls -> `12 passed; 0 failed; 12 total` (exit 0).
- `AC-14`: Cross-repo exact numerical verification against `rpoyau/aod@98ddfa0` fixtures with zero `R01`/`R02` violations.
- `AC-15`: Zero regressions on PR 1 & PR 2 (`v1_examples` 10/10, `v2_smoke` 3/3, all 45 prior tests).
- `AC-16`: `/home/antigravity/rpoyau/aod` completely untouched; zero PR 4/PR 5 scope creep.
