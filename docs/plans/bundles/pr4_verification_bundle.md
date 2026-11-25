# PR 4 Independent Verification Bundle (Final — R02 PASS 16/16)

- **Repository:** `/home/antigravity/rpoyau/aod-dec` (`v2/native-reference-engine`, uncommitted PR 4 diff atop `9207e9c`)
- **Validator:** `3c8f2a9417076546cf765d71d5b6ccca`
- **R01:** 15/16 PASS, 1 FAIL (`AC-13`: `crates/aodc/src/main.rs:338` discarded `_fallback_note`, producing empty CLI stderr on hostile profile fallback).
- **R02 (after Implementer fix):** **16/16 PASS (`AC-01`..`AC-16`), 0 FAIL**.

## Key Independent Measurements
- `RUSTFLAGS="-D warnings" cargo test --workspace --offline`: **57/57 PASS**, 0 warnings.
- `RUSTFLAGS="-D warnings" cargo test --workspace --release --offline`: **57/57 PASS**, 0 warnings.
- `RUSTFLAGS="-D warnings" cargo build --workspace --release --offline`: **PASS**, 0 warnings.
- 4-crate static audit (39 `.rs` files): **0 `f32`/`f64`, 0 float literals, 0 disallowed `unsafe`**, `Cargo.lock` = `['aod-core', 'aod-llvm', 'aod-rt', 'aodc']`.
- Live `aodc probe-hw`: **272 ms** (`< 2s`), deterministic `cpu_sig_sha256="e1c0b5e35eea74ad..."`, real Xeon Haswell sysfs (`L1d=32768, L2=262144, L3=47185920, line=64`), honest `perf_event_open="unsupported"` (`perf_event_paranoid=2`), `policy_scope="COMPILER_EXECUTION_POLICY_NOT_PHYSICAL_AOD_LAW"`.
  - Probe 1 (memory ladder 4K..32M): `1.85 ns/hop` -> `128.94 ns/hop`, `recommended_l1_working_set_bytes=16384`.
  - Probe 2 (exact arith ladder): `i64_checked=0.65 ns/op`, `bigint_spill=328.52 ns/op` (`424x–505x`).
  - Probe 3 (branch predictor): `irregular_branch_ns=92928` vs `irregular_select_ns=3199` (`prefer_branchless_irregular=true`, post-`-O2` `br i1` retained).
  - Probe 4 (unroll & layout): `unroll4`, `native_cpu=true`, `soa=3286ns` vs `aos=20284ns` (`prefer_soa=true`).
  - Probe 5 (multicore): 2 pure read-only workers inside `thread::scope`, `verified=true`.
- Live `aodc learn`:
  - `learn_amortized_bench.dec`: **`decision="retain"`**, `heldout_ci: point=7835 bps [lower95=7817, upper95=7851]`, `compile=66589057ns + dispatch=3536ns = 66592593ns`, `base=323848518ns vs cand=70080832ns`, `net_savings=+187175093ns`, `break_even_calls=Some(3)`, `semantic_mismatches=0`, 0 orphan `.tmp` files.
  - `unboxed_loop_bench.dec`: **`decision="rollback"`**, honest compile-inclusive `ovh=97709502ns`, `net=-95849971ns`.
- Live `aodc jit --profile` (`AC-12` + `AC-13` R02):
  - Valid retained profile: exit 0, stdout exact, `stderr=""`.
  - 5 hostile tamper cases (bad `profile_sha256`, `cpu_sig_sha256` drift, duplicate JSON key, float literal, `> 1 MiB`): all 5 exit 0, 100% byte-identical stdout, non-empty `stderr` starting with `"aodc: profile fallback to baseline: ..."`.
- Trapping safety (`AC-09`): fresh `trapping_safety.dec` (`10 // x`, `(-1) & 1`, `INT64_MIN // -1`) exact `'117\n40\n0\n'` across VM, AOT `-O2`, ORC JIT `-O2`, and `learn`.
- `branch_irregular_bench.dec` (`AC-15`): exact `'1130250\n0\n'` across VM, AOT `-O2`, ORC JIT `-O2`, and `learn`.
- Regression (`AC-16`): `manual_examples` 12/12, `v2_smoke` 3/3, `v1_examples` 10/10; `/home/antigravity/rpoyau/aod` clean (`""`).
