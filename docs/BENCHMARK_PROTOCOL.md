# Native Dec benchmark protocol v1

Declared 2026-11-13 before candidate-native benchmark observations. Protocol ID
`dec-native-measurement-v1`; randomization seed `20261113`. The frozen protocol
is copied and hashed in each output directory before the first measurement.

The population is this Linux x86-64 host and this compiler/runtime build. Timing
is host performance metrology via Python `time.perf_counter_ns` (monotonic wall
clock), not an AOD clock, physical model or SI anchoring claim. Report OS-visible
CPU model/features/topology and affinity. Probe hardware counters; record missing
tools or permissions as unavailable rather than zero. No GPU/parallel/cache-layout
claims are in scope.

Two prevalidated variants use the identical verified Dec source and native ABI:
generic LLVM O0 and LLVM O2. Before performance runs, compare each with reference
outputs, final value, error and retained trace serialization. A failed variant is
excluded, with its failure retained. Program variation is limited to source-level
workload dimensions within the frozen grammar.

Workloads cover ternary pack/unpack/arithmetic, arbitrary-precision rational
reduction, trace append and closure validation, nested fusion, coupling and
repeated JIT invocation. Exact sources and dimensions are saved with results.
Each run creates a fresh runtime context and retains output/value/error equality.
The initial train dimension is the supplied example and the held-out dimension
is two repeated executions of its complete body in a fresh context; no semantic
claim depends on timing. Where feasible, additional sizes use explicit repeated
statements without introducing grammar extensions.

Measurement order: build both variants (separately time frontend/lowering/object
emission/linking where the API supports them); load each JIT module separately;
perform 3 warmup invocations per variant, then 12 randomized paired tuning samples
and 20 randomized paired held-out samples. Randomize O0/O2 order within every pair;
shuffle control order within each held-out pair. Per sample, run 5 invocations,
report raw batch nanoseconds and per-invocation derived costs. Preserve every
sample, error, timeout and excluded/censored result. Never replace failures with
successful reruns. The same run counts and fresh-state policy apply to controls.

Report compilation, loading, warmup, tuning/profiling, selection/profile I/O,
steady-state execution and total experiment costs separately. Cumulative total
cost includes one-time preparation and measured control costs; amortized results
state their observed invocation horizon. Do not extrapolate a speedup from native
steady-state alone. Native memory allocation/free is part of invocation cost;
peak process RSS is sampled where available and is not allocation attribution.

The deterministic bounded policy retains at most 32 costs per variant and selects
the lowest median only after at least 5 observations per variant. It requires a
5% median improvement and a conservative paired 95% bootstrap interval excluding
zero. A 5% held-out regression bound triggers rollback to generic. Bootstrap uses
2000 resamples and seed 20261113; report paired percentage differences and a
percentile interval. No significance claim is made beyond the measured host and
workloads. Selection at invocation k uses only already committed observations;
observations of k can affect k+1. Exploration is bounded by the 12 paired tuning
samples. Freeze the profile before held-out testing.

Controls: generic (no profile), passive (collect but no selection), frozen
(read-only learned profile), active (select then append observations for later
calls), stale binding, shuffled costs with original integrity digest (reject),
and retained-state removal (generic). A separate synthetic cost fixture may prove
causal policy influence and actual selected native dispatch; it must be labelled
synthetic and never used as performance evidence. Corruption, incompatible host,
software, workload, missing variants and interrupted writes must fall back safely.

Benefit requires the held-out paired confidence interval to exceed 5%, no workload
regression above 5%, and positive gain after measured preparation/profiling/profile
costs at the observed horizon. Otherwise report no established benefit and retain
or restore generic. Functional learning and performance benefit are separate
outcomes. Independent replicas, AArch64 execution, counter-based analysis and
larger datasets remain separate follow-up experiments.

## Frozen workload registry (declared before execution)

| ID | Source | Training batch | Held-out batch |
|---|---|---:|---:|
| ternary | supplied examples/ternary.dec | 5 fresh contexts | 10 fresh contexts |
| weights | supplied examples/weights.dec | 5 | 10 |
| closure | supplied examples/closure.dec | 5 | 10 |
| fusion | supplied examples/fusion.dec | 5 | 10 |
| coupling | supplied examples/coupling.dec | 5 | 10 |
| rational_stress | 40 repeated exact reductions of a 120-digit numerator | 5 | 10 |
| nested_fusion | supplied fusion followed by a second outer fold and closure | 5 | 10 |

The held-out split uses later randomized samples and doubled independent invocation
batches of the same source; it does not claim generalization to unseen programs.
A native helper-only `execute_ns` and full fresh-context invocation wall time are
both saved. The latter includes host dispatch, runtime context lifetime and result
extraction. Primary comparisons use full invocation wall time. Validation of saved
outcomes occurs after the timed batch and is separately attributed to the harness.
