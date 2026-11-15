# Existing introspection and reusable compiled execution

The unchanged Manual `learning()` and `mindfulness()` functions execute directly. The observer captures their real call tree, committed-state query, retained relation, native update and subsequent state. The learning run contains 29 transitions and eight separate execution branches; each mindfulness control contains six transitions. Native cuts and call identities remain separate. Trace limits preserve results and original errors.

The compiled adapter retains the same admissions and exact outcomes while reusing owning CPU/GPU sessions. CPU lanes keep fresh GMP/fuel/error contexts. GPU calls keep source-bound kernels and capacity-checked buffers, with the caller's CUDA context restored after every call. Both owners serialize execution and cleanup. A different caller-owned source is rejected even if it computes identical numbers.

The exact Main/Manual binding remains `ab8359209b8f9b2ef4007403101016e3ea58cfe4`. Source files, review findings, failed probes and their repairs are retained separately from runtime evidence. Upstream ontology and native evolution sources are unchanged.

## Measurements

Five paired samples per case/mode retain every observation. Ratios are medians of paired times; the two displayed durations are separately computed medians. Every compiler-reuse and observer-comparison pair favored the optimized implementation. CPU measurements use the existing content-addressed disk cache; runtime construction, first loads, calls, cleanup and full report serialization are included. GPU NVRTC compilation is included. Shared fixture preparation and later semantic comparison are excluded equally from both compiled intervals. No CPU affinity or exclusive-host condition is assumed.

| Mode / compiled target | One-shot (s) | Reusable (s) | Paired ratio |
|---|---:|---:|---:|
| normal / cpu | 1.057426 | 0.658778 | 1.7348× |
| normal / gpu | 10.949473 | 3.098860 | 3.5313× |
| optimized / cpu | 0.846712 | 0.618303 | 1.3657× |
| optimized / gpu | 10.962113 | 3.104453 | 3.5252× |

The comparison uses the current one-shot API against one reused session across the same 18-transition, three-lane continuation. It measures removal of repeated compiler setup, not a speedup over the original uncompiled engine.

| Mode / original engine case | Profile trace (s) | Selected-code trace (s) | Paired ratio | Trace off (s) |
|---|---:|---:|---:|---:|
| normal / delayed | 0.992287 | 0.092753 | 11.1863× | 0.089882 |
| normal / learning | 5.731649 | 0.627117 | 9.7764× | 0.553973 |
| normal / mindfulness | 1.205745 | 0.106936 | 9.5832× | 0.092300 |
| optimized / delayed | 1.022204 | 0.096324 | 10.1479× | 0.096653 |
| optimized / learning | 4.030605 | 0.413761 | 10.5327× | 0.376252 |
| optimized / mindfulness | 1.027422 | 0.102700 | 10.4142× | 0.100453 |

Both observers capture identical complete records. The capture speedup compares the two observers; trace-off controls remain in each report to show the remaining observation cost and timing variation. Independent replay is separately timed. `profile-baseline/` retains the earlier full run and source snapshots that exposed broad-profiler overhead.

Actual hardware: NVIDIA RTX A1000 6GB Laptop GPU, compute capability 8.6. All generated responses are consumed and checked against complete original native outcomes. The retained-query, removal/restoration and delayed-query controls remain unchanged.

## Validation and scope

`review-integration.md` and the executable probes retain the original completeness/type counterexamples and their closure. The verifier checks exact typed source-owned initialization, input schedules, retention, full selected-function topology, combined histories and native outputs. An independently coherent but different witness does not authenticate the original execution.

The CPU/GPU runtime lifecycle review and injected failure probes are in `review-cpu-gpu.md`. `cpu-sequence/` is a preliminary 24-call compiler-only benchmark; it is not the complete native example. `gpu-session/` retains the pre-session implementation and its implementation review.

The 36 CPU/integration, 18 engine-observer and 25 actual-device tests pass in each Python mode (79 total per mode). Seven independent monitoring probes and six hostile trace counterexamples also pass in both modes. Native conformance covers 104 reference/JIT/AOT case/optimization rows plus six malformed-IR cases; debug and optimized native runtime checks pass. Fourteen manuscripts pass the pinned aod-lint. The three-page [Typst companion](parallel-compiler.pdf) was compiled and its changed pages visually inspected. Full CI is reported on PR #7 after push.

`smoke.log` records an early source-binding rejection while the helper was still being edited; it is not a successful run. Its later smoke is preliminary. Final source-bound measurements and two-mode device logs are authoritative for this delivery. The existing slower boundary workloads remain in the earlier parallel-tracing evidence.

## Reproduce

```sh
python -B scripts/check_introspection_evolution.py --repeats 5 --gpu required --output build/evolution
python -B -O scripts/check_introspection_evolution.py --repeats 5 --gpu required --output build/evolution-optimized
python -B scripts/check_awareness_jit.py --gpu required --output build/awareness
python -B scripts/check_introspection_device.py --repeats 5 --output build/evolution-device
```

The device driver requires a fresh output directory and a real available NVIDIA CUDA device. CPU-only replay uses `--gpu off`. These are bounded compiler and existing-Manual examples; the measurements do not establish a general engine speedup or an adaptive cost policy.

Large JSON records are stored as deterministic gzip with uncompressed SHA-256 and size in each artifact manifest. The root checksum manifest covers stored bytes. Preparation and reference-comparison exclusions are explicit in each performance report; no failed/slower sample is discarded.
