#import "/bindings/aod/aod_schema.typ": *
#import "/docs/manual/figures/fig_unboxed_pipeline_vs_spill.typ": fig-unboxed-pipeline-vs-spill

= LLVM 18 Backend and Hardware-Informed Policy Learner <sec:llvm-learner>

#aod-paragraph("Setup.")
Native execution compiles verified Dec programs into safe textual LLVM 18 IR (`crates/aodc/src/codegen.rs`)
and hands that IR to `crates/aod-llvm` (`dlopen`-loaded `libLLVM.so.18.1`, #aod-cite("llvm-orc-jit")).
Both AOT (`aodc build`) and in-process ORC LLJIT (`aodc jit`) verify pre-pass and post-pass modules via
`LLVMVerifyModule` (`LLVMReturnStatusAction = 2`), keeping standalone AOT ELF executables completely free
of runtime dependencies on `libLLVM` or `libgmp`.

#aod-noteblock("Execution Policy Scope")[
  Every hardware probe report (`aodc probe-hw`) and learned execution policy (`aodc learn`) is explicitly
  tagged `COMPILER_EXECUTION_POLICY_NOT_PHYSICAL_AOD_LAW`. Learned `CodegenPlan` parameters select compilation
  knobs (`opt_level`, `branchless_select`, `unroll_hint`, `target_cpu_native`, `inline_ratio_fastpath`,
  `vectorize_loops`, `interleave_count` in `{0, 2, 4}`, `layout_prefer_soa`, `parallel_observers`,
  `trit_ssa_promotion`) and are strictly forbidden from altering exact AOD semantics.
]

#aod-paragraph("Derivation.")
As shown in #figref("fig:unboxed-pipeline"), Trit-Boxed `int` values occupy a single 64-bit general-purpose
register (`i64`) with loop-free SWAR unpacking (`@__aod_swar_unpack_unchecked`) and 81-byte 4-trit LUT packing
(`@.pack4_lut`), while coupled `ratio` values occupy `%DecRatio = type { i64, i64 }` reduced via inline `i64`
Stein GCD (`@llvm.cttz.i64`, zero `urem`/`udiv`/`sdiv i128` calls). Stream pipelines `(mask ?> vals) |> +/`
and `(a..b) |> +/` fuse into single-pass register loops with zero intermediate sequence allocations. Only
operands exceeding 32 trits (`>= 0xC000_0000_0000_0000`) branch to the cold `BigTrit` helpers in `aod-rt`.

#figure(
  fig-unboxed-pipeline-vs-spill(),
  caption: [Trit-Boxed LLVM 18 hot path, `BigTrit` cold spill boundary, Exact Semantic Guard, and rollback-safe `RetainedProfile` policy loop.],
)<fig:unboxed-pipeline>

Sound branchless `select`/`mux` lowering under `CodegenPlan.branchless_select = true` restricts eager evaluation
to a strict pure-expression whitelist (`is_pure_select_safe`): inline literals, local/const identifiers, `Bool`
negation, `T1` negation and multiplication, and pure scalar comparisons. Any potentially trapping or spilling
expression (such as `10 // x` or `Int` negation) always keeps lazy `br i1` control flow, as verified live below:

```dec
dec main() -> int:
    let x = 0
    let safe_val = select(x != 0, 10 // x, 42)
    let tern = mux(x <=> 0, 10 // x, 7, 10 // x)
    emit safe_val
    emit tern
    0
```

```dec-expect
42
7
0
```

Hardware discovery (`hw_info.rs`) and five live self-verifying ORC LLJIT micro-probes (`hw_probe.rs`:
memory tiers, exact arithmetic fast-vs-spill, branch predictor, unroll/layout, and pure read-only multicore)
measure the host CPU using guarded `RDTSCP` (`CPUID` leaf `0x8000_0001` EDX bit 27) and `CLOCK_MONOTONIC_RAW`,
while probing Linux `perf_event_open` honestly without fabricating PMU counters. Active loop hints
(`unroll_hint`, `vectorize_loops`, `interleave_count` in `{0, 2, 4}`) emit one combined self-referential
`!llvm.loop` metadata node (preserving baseline IR byte-for-byte when inactive), and `parallel_observers = true`
(also selectable via `--parallel-observers` on `aodc run` and `aodc jit`) dispatches multiple read-only
procedural observers on scoped worker threads with pre-simulation arena snapshotting and deterministic
declaration-order merge (including single-observer sequential fallback at tight fuel boundaries so stdout,
fuel, and errors match sequential execution byte-for-byte). The learner (`learner.rs`, `PROFILE_VERSION = 2`,
capped at `<= 6` hardware-guided candidates) runs zero-float paired percentile bootstrap (`2000` resamples,
deterministic seed `20261113`) in exact `i128` basis points, enforces Exact Semantic Guard parity before
admitting any timing pair, and requires honest compile-inclusive net savings
(`sum(heldout_base) > sum(heldout_cand) + cand_compile_ns + dispatch_overhead_ns`) before writing an atomic
SHA-256-sealed `RetainedProfile`.

#aod-paragraph("Falsification.")
Any semantic divergence, invalid `interleave_count`, legacy `profile_version = 1` schema, insufficient
held-out confidence (`lower_95_bps <= 500`), unamortized compile overhead, oversized (`> 1 MiB`) profile,
float token, duplicate JSON key, CPU signature drift, or 1-byte profile tamper triggers deterministic rollback
or baseline fallback (with an explicit `stderr` notice under `aodc jit --profile`), preserving all invariants
established in #secref("sec:firewalls").
