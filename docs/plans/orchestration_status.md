# `aod-dec` v2 Orchestration Dashboard (`v2/native-reference-engine`)

**Process:** Lead Orchestrator → Planner → Plan Review Gate → Implementer → Validator → Final Code Review Gate → Commit  
**Target Repo:** `/home/antigravity/rpoyau/aod-dec` (branch `v2/native-reference-engine` off `main@79bf659`; upstream `rpoyau/aod` read-only)

| PR | Scope | Planner | Plan Review | Implementer | Validator | Final Review | Commit |
|---|---|---|---|---|---|---|---|
| **PR 1** | `SOURCE_BINDING.json` (`S0`/`S1b`), frozen v1 vectors, `std`-only zero-float `crates/aod-core`, Rust `aodc` Dec v2 frontend, compile-time firewalls, `DecIR v2`, exact evaluator, `--compat v1` | ✅ Complete | ✅ Approved (Rev 2) | ✅ Complete (R04) | ✅ PASS (23/23 R04) | ✅ PASS (R02) | `55da6d7` |
| **PR 2** | Unboxed-first LLVM 18 AOT & in-process ORC LLJIT (`libLLVM.so.18.1` FFI), inline `i64`/`t32`/`ratio` fast-paths + cold `aod-core` spill, coarsened fuel, differential parity + speedup | ✅ Complete | ✅ Approved (Rev 2) | ✅ Complete (R02) | ✅ PASS (17/17 R03) | ✅ PASS (R02) | `6b34cec` |
| **PR 3** | `stdlib/aod/` (`ledger`, `trace`, `wavelet`, `recipient_lattice`, `boundary`, `cells`, `metrology`) + native `.dec` Manual fixtures `01`–`06` verified against `aod` PR `#43`/`#45`/`#46` | ✅ Complete | ✅ Approved (Rev 2) | ✅ Complete | ✅ PASS (16/16) | ✅ PASS | `9207e9c` |
| **PR 4** | Native JIT Hardware Learner (`aodc probe-hw`, memory-tier/arithmetic/branch/SIMD/threading micro-probes, `RDTSCP`/`perf_event_open`, specialized codegen plans, rollback-safe retained profiles) | ✅ Complete | ✅ Approved (Rev 2) | ✅ Complete (R03) | ✅ PASS (16/16 R03) | ✅ PASS (R02) | `1488c9f` |
| **PR 5** | M4-aligned modular Typst manual (`aod_schema.typ`, `main_registry.json`, pure-vector `0/0/0` plots, `aodc manual-verify` positive + falsifying controls, `aod-lint`, CI) | ✅ Complete | ✅ Approved (Rev 2) | ✅ Complete (R02) | ✅ PASS (14/14 R02) | ✅ PASS (R02) | `4ab9a9f` |
| **PR 6** | **Track 2 (`S2` Parity + `S4` `aod-core` Export):** Amendment binding to `rpoyau/aod` PR `#51` (`3122c944`), 30/30 `aod.s4.vector.v1` golden-vector parity (`RC-001`..`RC-024`, `SD-001`..`SD-006`) in `crates/aod-core`, pure `.dec` S2 sector modules + `manual_examples/07`–`09` + `nc07`, `aodc verify-s2` CLI | ✅ Complete | ✅ Approved (Rev 2) | ✅ Complete | ✅ PASS (15/15) | ✅ PASS (0 P0/P1) | `c6df4d0` |
| **PR 7** | **Phase A (Zero-Python Purification):** Pure-Rust `std`-only `aodc audit-pdf` (RFC 1950/1951 zlib/DEFLATE inflater, 0 crates, 0 unsafe, 0 floats), delete all `.py` files, permanent Zero-Python repo/CI guard, zero-Python `.github/workflows/ci.yml`, retire PRs `#1`–`#7` | ✅ Complete | ✅ Approved (Rev 2) | ✅ Complete (R02) | ✅ PASS (12/12 R02) | ✅ PASS (R02) | `d74651f` |
| **PR 8** | **Phase B (`S4` Convergence with `rpoyau/aod` PR `#53`):** Bind `S4@e013401`, exact 90-tick `cycle_shedding_trace` (Rust + pure `.dec` fixture `10`), `aodc eval-op` & `aod.s4.rust.report.v1` report mode with pre-resolution `--report` safety guard | ✅ Complete | ✅ Approved (Rev 2) | ✅ Complete | ✅ PASS (14/14) | ✅ PASS (0 P0/P1/P2) | `8dc236c` |
| **PR 9** | **Phase C (Practical Apparatus Ergonomics, Native Manual Data & HW Learner v2):** `stdlib/aod/apparatus.dec` (3:3:6 opposing templates, concentric sensors, native JSON/CSV trace export), `aodc manual-data` zero-Python figure data regeneration, HW Learner v2 (SoA/SIMD/parallel observers) | ✅ Complete | ✅ Approved (Rev 2) | ✅ Complete (R02) | ✅ PASS (15/15 R02) | ✅ PASS (0 P0/P1/P2) | `1e5f224` |
| **PR 10** | **Phase D1 (HW Register Primitives, `bytes`, §5.17 Live-Walk Audit & FRG Double-Slit Probe):** `Null`/`u8`/`b64`/`t9`/`bytes`/`array[T, N]`/OR-based `superpose_cancel`/eager `select`, `FIRST_PRINCIPLES_DERIVATION.md`, `aodc audit-dec` (6 §5.17 hooks), 2-thread shared-cache-line FRG probe | ✅ Complete | ✅ Approved (R02) | ✅ Complete | ✅ PASS (14/14) | ✅ PASS (0 P0/P1) | Committed |
| **PR 11** | **Phase D2 (Pure Indented `dec` Signature-Matrix Frontend & AFC Scope Operators):** Lowercase `dec <name>:` + indented `(a: T) [where pred] -> Ret:` rows, positional `Struct(...)`, `map[(k, v)]`, indented `use`, engine-driven `evolve`/`scan`/`fold_seq`/`map_seq`/`sum`/`dot`, `PureWaveV2` | 🔄 In Progress | ⏳ Queued | ⏳ Queued | ⏳ Queued | ⏳ Queued | — |
| **PR 12** | **Phase D3 (Full Stdlib/Manual/First-Principles Migration & Hard `PureWaveV2` Ban):** Port 7 `aod-dec-dev/dec/` modules, rewrite all `stdlib/aod/*.dec`, `manual_examples/`, & Typst manual to pure wave syntax + `bytes`/status `enum`s, flip hard ban on `{ }`/`if`/`for`/`while`/`mut`/`str` in Dec v2 | ⏳ Queued | ⏳ Queued | ⏳ Queued | ⏳ Queued | ⏳ Queued | — |

## Active Log
- **Phases A–C (`d74651f..1e5f224`) + Reference Archive (`b0e4cc6`) green on PR `#9`** (held unmerged pending First-Principles Wave-Language Purification `D1`–`D3`).
- **First-Principles Wave-Language Purification (`D1` → `D2` → `D3`):**
  - **Phase D1 (`PR 10`) Complete:** Planner (`526acbc89d56c5e79bae2e037c7fe4df`) → Plan Review `APPROVE R02` (`36deaa0a7eb7bdd2b0989cde28e6d694`) → Implementer (`086aa38571be80175d13f45b0c0ab4de`) → Validator **14/14 PASS** (`0414657fc81413ec9e5daa10e84616be`, `111/111` debug+release) → Final Code Review **PASS** (`24a0cc82de80a30c9558b8dd0abc10bd`, `0 P0`, `0 P1`, `P2-1`/`P2-2` resolved).
  - Launching Phase D2 (`PR 11`) Planner.





