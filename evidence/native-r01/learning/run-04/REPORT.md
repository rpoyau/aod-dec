# M4/M5 final execution result

Final bound experiment: run-04. All seven workload control suites passed. Compiler, runtime, harness and policy source hashes were unchanged across the experiment. All retained observations and prior run-01/run-02/run-03 records are preserved. Compiler binding: `16029e1d7b13b21485a07cba60d88a2a755debd88b32a7c66b75f433ed956854`.

No cost-inclusive performance benefit was established at the measured horizon; every workload saved generic rollback. Positive values below mean observed steady-state savings of O2 relative to O0, before preparation costs.

| Workload | Median gain | 95% paired interval | Cost-inclusive gain (s) |
|---|---:|---:|---:|
| ternary | -1.61% | -4.90% to -0.41% | -4.872 |
| weights | -0.93% | -5.24% to 1.21% | -3.877 |
| closure | -1.58% | -2.11% to -0.50% | -4.115 |
| fusion | -1.02% | -2.39% to -0.41% | -4.233 |
| coupling | -1.55% | -2.34% to 1.30% | -4.439 |
| rational_stress | -0.16% | -1.53% to 0.33% | -6.406 |
| nested_fusion | -1.40% | -1.66% to 0.01% | -4.785 |

The seven controls are generic, passive, frozen, active, stale binding, shuffled profile, and retained-state removal. All preserve exact output, return, error state and retained witness serialization. Both generated entry paths were invoked. Fresh subprocesses revalidated native objects, reloaded committed profiles, selected optimized from explicitly synthetic cost fixtures and invoked native addresses; removing profile state restored generic. Synthetic costs prove policy influence only and are excluded from performance results.

Hardware counters available: False. Probe commands, exit codes and any permission/tool failures are retained in host.json. Total experiment elapsed: 72.785 seconds. Peak process RSS: 166772 KiB (whole process, not per-allocation attribution).

Limitations: this Linux x86-64 host only; later held-out samples of the same source at doubled batch size, not unseen-program generalization. Conformance and compilation workers ran concurrently; load/memory snapshots and paired randomized order are recorded, but do not remove resource competition. Runtime context and host dispatch costs are included; generated-call-only times are retained separately. Profile files use single-writer ownership. No hardware/AOD physical equivalence or speedup is inferred.

Final standalone CLI evidence: ../cli-final/results.json. Passive profiling saved 12 actual paired observations; a later active process read generation 12 and correctly retained generic. The negative program preserved the ordered `123` emission followed by exact `line 4: t32 overflow`, location 4 and exit status 1.

Policy/helper regression suites: 12 PASS normal and 12 PASS optimized. They include independent source-error/effect expectations and recomputed-checksum profiles with unequal paired counts or inconsistent generation. Such malformed histories reject even when checksum-valid; lawful rolled paired windows still load.

Commands:

```sh
python3 work/candidate/scripts/benchmark_native.py --output work/evidence/learning/run-04
python3 work/candidate/scripts/analyze_native_benchmark.py work/evidence/learning/run-04 --output work/evidence/learning/run-04/reanalysis.json
```
