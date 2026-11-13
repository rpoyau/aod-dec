# AOD Dec native execution result

The requested M0–M5 core functionality is implemented for Linux x86-64. All 25 required acceptance criteria pass within the declared core-v1 scope. M6 self-hosting is a separate, unimplemented continuation. No cost-inclusive speedup was established.

The candidate compiles actual source bodies into LLVM native objects and linked executables. Its C++/GMP runtime preserves arbitrary exact integers/ratios, checked ternary intermediates, immutable histories and closure/fusion/coupling witnesses. ORC executes the same lowered functions, with lazy loading, checked object caches, native entry guards and generic fallback. Retained measured profiles can influence a later native selection; malformed or incompatible profiles fall back. Compiler/JIT orchestration still uses Python; emitted AOT programs do not.

## Verification

| Evidence | Result |
|---|---|
| Immutable request and fresh baseline | All 61 files verified; 57 Python tests each in normal/optimized modes, nine legacy Rust tests each in debug/optimized modes |
| Final candidate regression suite | 79 tests in normal mode and 79 in optimized mode, all passing |
| Source conformance | 47 programs × O0/O2 = 94 passing AOT/JIT/reference rows, plus malformed-IR rejection |
| Native runtime | 1,979 exact/differential cases and debug/O3 hostile witness/lifetime/allocation checks pass |
| Backend integration | 10 groups / 145 retained observations in each mode, including typed runtime inputs, 84 fuel/depth comparisons, concurrent cache construction and code lifetime |
| Python-free isolation | All five examples and three runtime-input cases pass; final example binaries match isolated bytes |
| Profiling and adaptation | Seven workloads, 12 paired tuning and 20 paired held-out samples each, seven matched controls; fresh-process profile influence and native fallback pass |
| Detached replay | All 60 source files match; replayed compiler builds and runs the native example |

Independent exact values and hostile witness mutations supplement differential agreement. Passing reference/native comparisons is not a general proof of compiler correctness or an AOD physical model.

## Performance and scope

The final experiment is [learning/run-04](learning/run-04/REPORT.md), with raw observations, uncertainty, compilation/loading/profiling and total costs retained. Every measured workload restores generic: no cost-inclusive held-out benefit passes. Synthetic retained-cost fixtures separately demonstrate that persistent state can cause a later optimized native dispatch; those synthetic costs are excluded from performance results.

Held-out samples are later windows of the same programs at doubled batch size, not unseen-program generalization. Concurrent build/conformance activity is recorded as a measurement limitation. Hardware counters were unavailable and are reported as unavailable. Timing provides compiler performance metrology, not an SI anchor or physical AOD dynamics.

Only the original core-v1 grammar is implemented. Structs, arrays/slices, tuples, explicit loops, modules, I/O, pattern dispatch, GPU/UMA, general folded complexes and RCD are not claimed. AArch64 has no execution coverage. Guards validate implementation identity at function entry; speculative input guards and mid-function deoptimization are absent. Profile files have single-writer ownership. Native arenas support checked allocation limits; unrecoverable GMP/system OOM is fail-stop. ASAN could not link because its installed library target was missing; that gap is retained.

## Delivery and provenance

Candidate: `work/candidate/`. Source tree SHA-256: `24f8df17d234b980f47025b78733dc931aaede74a0f689de0589fa950815a07e`.
Compiler SHA-256: `16029e1d7b13b21485a07cba60d88a2a755debd88b32a7c66b75f433ed956854`.
The original source, input/foundation archives, owner amendment and request criteria remain unchanged. Main/Manual were not edited; there were no remote writes.

See [ACCEPTANCE_RESULTS.json](ACCEPTANCE_RESULTS.json) for all 26 dispositions, [HANDOFF.md](HANDOFF.md) for M6 continuation, and [delivery instructions](../delivery/README.md). The detached patch, toolchain/license record, source/evidence hashes, raw observations, failed development attempts and replay log are included. Reconstructible system-library isolation files are excluded from the ZIP; their exact manifest, commands and successful execution evidence are included.
