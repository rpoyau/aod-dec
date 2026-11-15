# Compiled CPU/GPU execution, boundary tracing and existing awareness examples

This extension executes verified Dec source on native CPU threads and on CUDA.
It also replays complete named boundary schedules using spawned CPU processes or
the separate handwritten CUDA incidence kernel. These are distinct execution
paths, with separate artifact and source bindings.

The compiler batch command selects a pure int/bool signature. CPU lanes call
generated guarded LLVM code through one C++ thread dispatcher and own isolated
GMP/fuel/error contexts. The GPU compiler generates CUDA from the verified IR,
checks its signed 64-bit domain and consumes actual device outputs. It rejects
unsupported source/domain requests without hidden CPU fallback. Exact rational
current assembly and retained native witness admission stay on the host.
This is JIT batch support, not standalone GPU AOT, all-language GPU support,
AArch64 or M6 self-hosting.

## Executed evidence

- [Required-device result](device/results.json): all eight stages pass across
  normal and optimized Python on NVIDIA RTX A1000 6GB, compute capability 8.6,
  driver API 13050, NVRTC 12.9. Both modes pass 29 actual-device/compiler/kernel
  tests without skips.
- [CPU/GPU compiler result](device/normal/compiled/results.json): 48 independent
  classifier rows; 1,536 generated LLVM entries across four observed native
  threads; one generated-CUDA coefficient launch plus 16 generated-CUDA boundary
  jobs. Source, exact input, LLVM/dispatcher and generated CUDA/PTX hashes bind
  execution. [Retained CUDA](device/boundary_coefficients.cu) and
  [PTX](device/boundary_coefficients.ptx) match the executed artifact hashes.
- [Retained tracing](device/normal/tracing/results.json): 145 immutable events,
  16 complete boundary views, four participating process workers and 16 separate
  handwritten-kernel jobs. Exact crossing/sweep/shedding rows agree with scalar
  replay. [Cross-mode check](device/cross-mode.json) verifies identical canonical
  events and eight complete semantic view comparisons across Python modes.
- [Existing awareness models](device/normal/awareness/results.json): compiled
  response/query/selector outputs reproduce the Manual's native continuations.
  Five families execute 62 generated CUDA launches per mode. Learning after the
  fourth actual compiled state gives retained/restored `[5,6,7]` and removal
  `[4,4,4]`. Existing mindfulness begins at current 3 and gives six ones; its
  delayed control gives `[4,2,-1,-2,0,3]`. Pressure 1/2 and 2 remain exact Manual
  host controls outside the declared integer target. Passive observation leaves
  state/event bytes unchanged. These reuse existing examples, documented in
  [earlier source and execution records](awareness-source-review/EXISTING_AWARENESS_EXAMPLES.md).
- [Native conformance](native-conformance.json): all 104 reference/JIT/AOT
  case/optimization checks pass, plus six malformed-IR rejection checks.
- Final focused suite: 79 tests pass in each Python mode, with three sandbox
  device skips; required-device runs separately cover the actual hardware cases.
  The local monolithic suites ended with status 143 before completion and are
  retained as incomplete. The [complete implementation workflow](ci-202c94d.json) passes all three jobs, including both full Python suites, native conformance, all applications and documentation.
- [Pinned aod-lint](aod-lint.json): all 14 applicable Typst manuscripts pass.
  Both new companions build to two pages and were visually inspected:
  [compiler](parallel-compiler.pdf) and [boundary tracing](parallel-boundary-tracing.pdf).

Large raw JSON reports use deterministic gzip. `device/RETAINED_FILES.json`
records the uncompressed hashes and lengths. Both recoverable SQLite archives
are retained; the final queries did not modify their event chain.

## Boundary and performance scope

The Sun–Mercury preparation retains actual recurring Duons, two supplied
whole-return chiral flips, an independently certified outer fold and a later
Duon/E2 route. Selected currents are `[4,-3,0,7,6]`; complementary currents
negate them and the enclosing view cancels internal transfers. Unsigned closure
burden 3 and signed route current 2 retain different units. Full selected and
complementary queries remain explicitly partial until a signed projection is
provided. This finite example is not an orbital/perihelion prediction.

No cost-inclusive speedup is demonstrated by these small replay workloads.
Normal-mode scalar trace replay took 0.825 s, process replay 2.271 s and the
handwritten GPU path 9.825 s. In the separately timed compiler check, scalar
replay took 0.955 s and compiled CPU replay 1.658 s; the generated-GPU check,
including its independent coefficient batch and complete replay, took 13.664 s.
Runs occurred under other local validation load; these are retained execution
costs, not a controlled performance benchmark. Optimization changes no physical
accuracy claim. The earlier SPARC result and its limitations remain unchanged.

Independent source reviews and falsifiers are retained under `compiler-cpu`,
`compiler-gpu`, `cpu-integration-review`, `gpu-integration-review`,
`awareness-source-review` and `solar-source-review`. They distinguish source
eligibility, execution, retained causal evidence and physical interpretation.

The original [language and compiler manual](language-and-compiler.pdf) now also
identifies the bounded CPU/CUDA targets. Its ten literal examples pass reference
and typed-IR execution; changed page 20 was inspected. The inventory correction
changes documentation only; implementation/device evidence remains bound to the
same source bytes.
