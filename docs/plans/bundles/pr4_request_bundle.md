# PR 4 Request Bundle (Rev 2 — Reviewer-Amended & Approved) — `rpoyau/aod-dec` v2
## Native JIT Hardware Learner (CPU Microarchitecture Discovery, Live JIT Tick/Click Micro-Probes, Parameterized LLVM 18 Codegen Plans, Exact Semantic Guards & Rollback-Safe Retained Profiles)

**Base Commit:** `9207e9c` (PR 3 green across 16/16 checks, 0 P0/P1/P2)  
**Host Verified (`host_hardware.txt`):** `x86_64` Linux (`KVM`), `Intel(R) Xeon(R) CPU @ 2.30GHz` (2 cores), ISA `sse4.2/avx/avx2/bmi1/bmi2/popcnt/fma/cx16/rdtscp` (`avx512f=false`), sysfs caches `L1d=32KiB, L1i=32KiB, L2=256KiB, L3=45MiB, line=64B`, `perf_event_paranoid=2`.  
**Hard Constraints:** 100% `std`-only 4-crate Rust workspace (ZERO crates.io deps), ZERO IEEE-754 floats (`f32`/`f64`) across ALL 4 crates, `#![forbid(unsafe_code)]` preserved on `aod-core` and `aodc`, `unsafe` strictly quarantined to `crates/aod-rt/src/abi.rs` and `crates/aod-llvm/src/{ffi.rs,aot.rs,jit.rs,hw_probe.rs}`, ZERO mutation of `/home/antigravity/rpoyau/aod`, explicitly tagged `COMPILER_EXECUTION_POLICY_NOT_PHYSICAL_AOD_LAW`.  
**Reviewer Mandatory Amendments Folded:** `P0-1`, `P0-2`, `P0-3`, `P0-4`, `P1-1`, `P1-2`.

---

## 1. File Tree & Module Boundaries

- `crates/aod-llvm/`:
  - `Cargo.toml`: local deps `aod-core = { path = "../aod-core" }` and `aod-rt = { path = "../aod-rt" }` (`P1-1`, still 100% acyclic & zero external crates).
  - `src/lib.rs` (`#![deny(unsafe_code)]`, exports `plan`, `hw_info`, `hw_probe`, `learner`)
  - `src/plan.rs` (100% safe: `OptLevel`, `UnrollHint`, `CodegenPlan`, `plan_id()`, JSON encode/decode, `recommend_candidates`)
  - `src/ffi.rs` (`#[allow(unsafe_code)]`: `host_llvm_target_info() -> LlvmResult<(String, String, String)>` (`P0-1`), `create_target_machine(..., target_cpu_native: bool)`, `LLVMOrcJITTargetMachineBuilderCreateFromTargetMachine` & `LLVMOrcLLJITBuilderSetJITTargetMachineBuilder` with exact ownership)
  - `src/aot.rs` & `src/jit.rs` (`#[allow(unsafe_code)]`: `*_with_plan(..., &CodegenPlan)`)
  - `src/hw_info.rs` (100% safe — uses `ffi::host_llvm_target_info()` per `P0-1`: ISA flags via `is_x86_feature_detected!`, `available_parallelism`, Linux sysfs caches, `perf_event_paranoid`, canonical `cpu_sig_sha256`)
  - `src/hw_probe.rs` (`#[allow(unsafe_code)]`: `RDTSCP` guarded by `#[cfg(target_arch="x86_64")]` + `is_x86_feature_detected!("rdtscp")`, Linux `syscall(298)` `perf_event_open` with 136-byte `PerfEventAttr`, 5 live self-verifying ORC LLJIT tick/click micro-probes per `P0-2`)
  - `src/learner.rs` (100% safe: Exact Semantic Guard, zero-float paired percentile bootstrap 2000 resamples seed `20261113`, honest cost-inclusive held-out net-gain gate, `metadata().len() <= 1 MiB` pre-read check, `aod_core::json::parse_strict_json` + `aod_core::sha256::Sha256`, rollback-safe atomic `RetainedProfile` per `P0-4`, `P1-1`)
- `crates/aodc/src/`:
  - `codegen.rs` (`#![forbid(unsafe_code)]`: `lower_to_llvm_ir_with_plan`, strict `P0-3` `is_pure_select_safe` whitelist, unique `distinct` `!llvm.loop` unroll metadata, `inline_ratio_fastpath` toggle)
  - `lib.rs` & `main.rs` (CLI `probe-hw`, `learn`, `jit --profile <path>`)
- `crates/aodc/tests/`:
  - `source_binding.rs` (add ONLY `crates/aod-llvm/src/hw_probe.rs` to unsafe allowlist; keep 4-crate zero-float & zero-external-dep audit)
  - `hw_learner_and_probes.rs` (covering `AC-01`..`AC-16`)
- `tests/fixtures/v2_bench/`:
  - `branch_irregular_bench.dec` (fits within `DEFAULT_FUEL = 100_000`, uses `is_pure_select_safe` leaves so `branchless_select=true` emits `select` while `false` emits `br i1` per `P1-2`)
  - `learn_amortized_bench.dec` (`P0-4`: native-scale ~350k-iter exact unboxed `int`/`t32` loop run under `--fuel 2000000` where `JIT -O0` vs `JIT -O2` genuinely amortizes real LLVM `O2` compile overhead across held-out runs -> 100% unfudged `Retain`!)

---

## 2. Parameterized `CodegenPlan` & Sound LLVM 18 Lowering (`P0-1`, `P0-3`)

- `CodegenPlan { opt_level, branchless_select, unroll_hint: Default|Unroll4|Disable, target_cpu_native, inline_ratio_fastpath }`.
- **Strict `is_pure_select_safe` Whitelist (`P0-3`):**
  - Allowed ONLY: `IntLit` (`to_i64().is_some()`), `BoolLit`, `TritLit`, `RatioLit` (both `num`/`den` fit `i64`), `Ident` (local `alloca` or initialized `const`), `Unary::Not` on `Bool`, `Unary::Pos`, `Unary::Neg` ONLY on `T1`/`T32` (NEVER `Int`/`Ratio`!), `Binary(Eq|NotEq|Lt|LtEq|Gt|GtEq)` ONLY on unboxed scalars `Bool`/`T1`/`T32`/`Q4` (NEVER `DecInt`/`DecRatio`/handle!), and `Binary::Mul` ONLY on `T1`.
  - Forbidden (MUST keep lazy `br i1`): ALL `Div`, `FloorDiv`, `Mod`, `Add`, `Sub`, `Mul` (except `T1`), bitwise `&`/`|`/`^`/`<<`/`>>`, `Call`, `Index`, `Field`, container/string literals, `Simulate`.
  - Single-stmt `if cond: v = a else: v = b` lowers via `select` ONLY when `plan.branchless_select && elifs.is_empty() && then_body.len()==1 && else_body.len()==1`, same `AssignTarget::Ident(v)`, and both `a`/`b` pass `is_pure_select_safe`.
- **LLVM 18 `!llvm.loop` Metadata (`P0-3`):** unique self-referential `!N = distinct !{!N, !M}` (`!{!"llvm.loop.unroll.count", i32 4}` or `!{!"llvm.loop.unroll.disable"}`), verified by `LLVMVerifyModule`.
- **LLVM 18 Ownership (`P0-1`):** `LLVMOrcJITTargetMachineBuilderCreateFromTargetMachine(tm)` consumes `tm` (never double-dispose); `LLVMOrcLLJITBuilderSetJITTargetMachineBuilder(builder, jtmb)` consumes `jtmb`; `LLVMOrcCreateLLJIT` consumes `builder` on both Ok & Err; `LLVMGetHostCPUName` & `LLVMGetHostCPUFeatures` freed via `LLVMDisposeMessage`.

---

## 3. CPU Discovery (`hw_info.rs`) & 5 Live Self-Verifying JIT Micro-Probes (`hw_probe.rs`, `P0-2`)

- `RDTSCP`: `#[cfg(target_arch="x86_64")]` + runtime `is_x86_feature_detected!("rdtscp")` (`cycles: Option<u64>`, `None` if unsupported).
- `perf_event_open`: glibc `syscall(298, &attr, 0, -1, -1, 0)` with zeroed 136-byte `#[repr(C)]` `PerfEventAttr` (`type_=0`, `size=136`, `config=0`, `disabled | exclude_kernel | exclude_hv`); closes `fd >= 0` (`Available`); maps `EACCES`/`EPERM` -> `Restricted`, `ENOSYS`/`ENOENT`/`ENODEV`/`EOPNOTSUPP` -> `Unsupported` (NEVER fabricated!).
- 5 live self-verifying ORC LLJIT probes (verifying exact Rust reference checksums, `< 500 ms` total):
  1. `probe_memory_tiers`: pointer-chase permutation across `[4K, 16K, 64K, 512K, 4M, 32M]` -> `recommended_l1_working_set_bytes`.
  2. `probe_exact_arith`: `i64_checked_add`, `t32_checked_add`, `ratio_inline_gcd`, `bigint_spill_add` -> `spill_to_fast_ratio_x100`.
  3. `probe_branch_predictor`: `branchless` (`select`) vs `branching` (retaining an actual conditional `br i1` after `-O2` via non-foldable stack scratch store/load, verified in post-opt IR!) across `predictable` & `irregular` inputs -> `prefer_branchless_irregular`.
  4. `probe_unroll_layout`: `scalar_generic` vs `unroll4_generic` vs `unroll4_native_cpu` + `AoS` vs `SoA` -> `recommended_unroll`, `prefer_native_cpu`, `prefer_soa`.
  5. `probe_multicore`: pure read-only worker `extern "C" fn(*const i64, usize) -> i64` (zero `AodRtContext`, zero shared mutable globals) across `1` vs `logical_cores.clamp(1, 4)` threads inside `std::thread::scope` while `OrcJitSession` remains alive strictly past scope join -> `scaling_x100`, `recommended_workers`.

---

## 4. Zero-Float Paired Bootstrap, Honest Cost-Inclusive Gating & Rollback-Safe Profile (`learner.rs`, `P0-4`, `P1-1`)

- `MAX_SAMPLES = 32`, `MIN_PAIRS = 5`, `BOOTSTRAP_RESAMPLES = 2000`, `BOOTSTRAP_SEED = 20261113_u64`.
- Exact integer `gain_bps = ((sum_base - sum_cand) * 10_000) / sum_base` in `i128`; `lower_95_bps = sorted[50]`.
- Exact Semantic Guard compares `(emits, final_result, failed, error)` byte-for-byte against baseline before admitting ANY timing pair.
- **Honest Overhead (`P0-4`):** `measured_overhead_ns = cand_compile_ns + dispatch_overhead_ns` (never excluding compilation!), plus exact `break_even_calls: Option<u64>`.
- Held-out gate (interleaved): `heldout_lower_95_bps > 500`, `sum(heldout_base) > sum(heldout_cand) + measured_overhead_ns`, zero semantic mismatches; else `Rollback`.
- `RetainedProfile` (`P1-1`): pre-read `metadata().len() <= 1_048_576`, `aod_core::json::parse_strict_json` + `aod_core::sha256::Sha256`, `profile_version == 1`, `policy_scope == "COMPILER_EXECUTION_POLICY_NOT_PHYSICAL_AOD_LAW"`, bound to `(cpu_sig_sha256, kernel_ir_sha256, workload_sha256)`, `training_pairs.len() == min(generation, 32)`, `heldout_pairs.len() >= 5`, all timings `> 0`, atomic temp + `sync_all()` + `rename` + parent dir `sync_all()`; any mismatch/corruption -> `FallbackToBaseline`.

---

## 5. CLI & Acceptance Criteria (`AC-01`..`AC-16`)
- CLI: `aodc probe-hw [--output <r.json>]`, `aodc learn [--compat] [--stdlib] [--baseline O0|O2] [--pairs N] [--heldout N] [--fuel N] [--max-depth N] [--profile <p.json>] <src.dec>` (refusing `--profile == <src>`), `aodc jit --profile <p.json> <src.dec>`.
- `AC-01`..`AC-13`: unchanged from Rev 1 (with `P0-1..P0-3` & `P1-1` enforcement).
- **`AC-14` (`P0-4` Honest Net-Gain Verification):**
  - `aodc probe-hw --output <r.json>` completes cleanly and writes canonical JSON (`policy_scope`, all 5 probes, `cpu_sig_sha256`).
  - `aodc learn --baseline O0 --fuel 2000000 --pairs 6 --heldout 8 --profile <p.json> tests/fixtures/v2_bench/learn_amortized_bench.dec` genuinely achieves **`decision == retain`** with honest compile-inclusive `measured_overhead_ns > 0`, `heldout_lower_95_bps > 500`, and `sum(heldout_base) > sum(heldout_cand) + measured_overhead_ns`!
  - `aodc learn --baseline O0 --pairs 5 --heldout 5 --profile <p2.json> tests/fixtures/v2_bench/unboxed_loop_bench.dec` reports honest cost-inclusive accounting (`retain` or `rollback` strictly matching actual measured net gain vs `measured_overhead_ns`).
  - `aodc learn --profile <src> <src>` refuses self-overwrite!
- **`AC-15` (`P1-2`):** `tests/fixtures/v2_bench/branch_irregular_bench.dec` produces byte-identical stdout across VM, AOT, ORC JIT, `branchless_select=false` (`br i1` IR), and `branchless_select=true` (`select` IR), and runs cleanly through `aodc learn`.
- **`AC-16`:** PR 1–3 tests 100% green (`v2_smoke` untouched at 3/3, `manual_examples` 12/12), `/home/antigravity/rpoyau/aod` clean, zero PR 5 scope creep.
