# Ternary AOD Dec: native compiler, JIT and hardware-learning plan R01

13 November 2026. Status: proposed implementation plan, not an executed milestone.

## Goal and base

Deliver a working compiler that builds native executables from Dec, a JIT that
compiles and executes Dec functions during a running session, and a runtime that
learns which equivalent implementations work best on its actual host. Preserve
ternary semantics, exact arithmetic, scoped closure/fusion witnesses and observable
execution histories across every mode.

Base: `AOD_TERNARY_DEC_REPAIR_R01_2026-11-13.zip`, SHA-256
`289260c911205ea300f6b5406a3019548f27b0d8f7ac9e8fcb892f7661b99e48`.
The bound reference has a Python frontend/typed JSON IR/VM, five worked examples,
57 Python tests repeated under optimization, and nine Rust tests repeated in debug
and optimized modes. Those are recorded R01 results, not tests of this plan.

First target: Linux x86-64. Second target: Linux AArch64 with actual execution
coverage. Android and other operating systems require their own ABI/JIT integration
work and are not implied by CPU support.

## Architecture decision

Use **one Dec semantic IR and lowering pipeline** for interpreter, native object
compilation and JIT. Keep AOD effects explicit through lowering. Use the Python
reference VM as the initial differential oracle, with independent mathematical
checks for exact arithmetic and witness conditions so shared mistakes remain testable.

Recommended default: LLVM native code generation, ORC v2/LLJIT for JIT execution,
and JITLink where appropriate for in-process object linking. These facilities are
provided by LLVM; they do not supply Dec semantics, its runtime ABI or a learning
policy. Use a small C++ backend/ORC component and a Rust runtime behind a C ABI.
The existing Python frontend can submit validated IR in the first milestone; a
native frontend follows without blocking the first native executable.

Pin a stable released LLVM/toolchain combination at implementation entry and
record versions/build hashes. Do not target moving development APIs. A short
entry spike must demonstrate object emission, runtime symbol linkage and one
JIT function with the same chosen toolchain before building out the backend.
Cranelift is a credible alternative if the LLVM integration/deployment cost proves
unacceptable in that spike; it supports emitting callable JIT code. Do not build
two production backends during this first implementation.

The runtime will use an audited arbitrary-precision integer/rational implementation
with pinned dependencies. Dependency/licensing/ABI selection is a concrete task
in M0; the old bounded Rust Ratio must not silently replace Dec's unbounded `ratio`.
A native program may depend on the declared Dec runtime/system libraries, but must
run without Python or a source interpreter after compilation.

### Representation and effects

| Dec object | Initial native treatment | Required preservation |
|---|---|---|
| t1, q4, bool | Validated small scalars | Exact domains; no implicit bool/int aliases |
| t32 | Checked signed scalar within balanced-ternary range; explicit packed conversion | 32-trit meaning, carry, overflow and invalid11 rejection |
| int, ratio | Owned arbitrary-precision runtime values | Exact values, denominator normalization, nonintegral-conversion errors |
| str | Immutable managed bytes/text under fixed encoding | Exact literal/output behavior |
| trace, fusion, coupling | Immutable runtime handles with documented ownership | Scope, lineage, start, event identities, parent histories and witnesses |
| emit/assert/failure | Explicit ordered effects with source location | Same outputs and first failing operation |

The language is ternary regardless of whether a backend uses decoded integer
registers or packed trit lanes. Alternative representations are compiler choices,
with proof obligations and performance tests. Neither is assumed fastest.

Introduce a verified control-flow IR with explicit checked arithmetic, conditional
branches, calls, returns, runtime operations and effect dependencies. Snapshot the
validated source IR before lowering. Arithmetic optimization must preserve
intermediate overflow/error behavior, not only the final mathematical value.
No optimizer may erase a committed event because aggregate currents cancel.

Specify runtime ownership, error/status return conventions, allocation failure,
string encoding and serialization before backend integration. Do not unwind Rust
panics or C++ exceptions across the ABI. Existing VM operation/depth limits are
reference controls: define a portable logical fuel policy and test it if budgets
are part of the public execution contract; do not accidentally count native
instructions as the same thing as Dec events.

## Milestones and acceptance

### M0 — Freeze semantics, ABI and backend entry spike

Deliver `LANGUAGE_CORE_V1.md`, a versioned semantic-IR schema/verifier, runtime ABI,
source-to-IR mapping, backend dependency lock, and a requirements/test registry.
Resolve each existing builtin's exact inputs, result, errors, effects and memory
ownership. Define error equivalence independently of diagnostic prose.

Acceptance: the current corpus remains supported; adversarial IR fails verification;
a minimal object and an ORC JIT function both call the same native runtime helper.
Record unsupported targets explicitly. No semantic changes hidden in backend work.

### M1 — First native executable: scalar core

Compile typed functions, exact dispatch, conditions, recursion, emit/assert and
scalar arithmetic into object code, link with the runtime, execute the result.
Start with ternary and normalized-weight examples, then exact-ratio stress cases.
Calls, constants and expressions from source must appear in generated execution;
embedding a source interpreter or precomputed output does not meet this gate.

Acceptance: binaries run in a test environment with no Python; changed input and
changed function bodies change results correctly. Compare values, errors and
output order against reference execution. Check boundary cases, random exact
inputs, deliberately invalid operations and source-located failures. Check
reproducible builds with a pinned target/toolchain and normalized metadata.

First concrete command target:

```
aodc build examples/ternary.dec -o build/ternary
./build/ternary
```

This is the first working native compiler milestone. Other capabilities do not
need to be finished before demonstrating it.

### M2 — Complete R01 core natively, then grow the language

Port all trace/fusion/coupling operations to the native runtime, preserving the
actual event and certificate logic. Compile all five R01 examples. Then add
structs, arrays/slices, tuples, explicit loops and modules through separately
specified grammar/type/IR extensions. Literal-pattern dispatch needs explicit
non-overlap/precedence rules before adoption. Validate each original `dec/` sketch
against source-bound operation semantics before calling it executable.

Acceptance for complete R01 core: interpreter and native execution agree on
canonical trace contents and certificate digests as well as scalar values; open
returns, missing hinges, altered parents, replay and bad contacts fail in both.
Exercise nested lifetimes and allocation errors. Negative tests must be derived
from the semantic requirements rather than only replaying implementation logic.

Acceptance for each extension is separate. General cell-complex folding/RCD
must have their own scientific model binding; adding structs cannot establish them.

### M3 — Working JIT with a generic native fallback

Use the same checked lowering and runtime ABI to compile functions into executable
memory. Begin with eager compilation on first call; add hotness-triggered tiering
only after that works. Provide interpreter/native-AOT/JIT selection and a diagnostic
view of which implementation executed. No silent interpreter fallback in a test
that claims JIT coverage.

Acceptance: compiled function addresses are invoked; source changes invalidate
cached code; runtime symbols resolve only through the declared ABI; runtime
failures match other modes. Test code lifetime, repeated loading/unloading,
recursive calls and source-location mapping. Separate writable allocation from
executable use under the host's memory-permission model.

Cache keys include verified IR digest, semantic version, runtime ABI, compiler
version, target triple, CPU features and optimization settings. Adaptive variants
also bind the profile/policy version. Validate manifests before cache reuse.

Install new variants at function-entry boundaries initially. Guard specialized
assumptions before observable effects. A failed guard calls the generic native
implementation. Do not attempt mid-function deoptimization until frame/state
reconstruction and trace-event replay avoidance are implemented and tested.
In-flight calls retain their old code until completion; then reclamation is safe.

### M4 — Measured hardware profile

Build a profiling harness before enabling learning. Discover/report available
CPU features and OS-visible topology; measure workload-specific throughput,
latency, memory-footprint effects and scaling. Distinguish reported capabilities
from measured performance. Probe permissions are optional: unavailable counters
are unknown, not invented values or zero.

Use actual work: ternary arithmetic/conversion, rational reductions, trace append
and validation, closure-heavy workloads, nested fusion, and repeated JIT calls.
Expand to cache-layout and parallel experiments only after arrays and lawful
parallel operations exist. CPU model names or iteration counts do not establish
AOD motif identities or cache-ring equivalence.

Record compiler/JIT time, loading, warmup, steady-state execution and total
amortized cost separately. Save raw observations, workload dimensions, system
conditions, affinity when available, clock/counter provenance and failures.
Use repeated paired runs with varied order; separate tuning data from held-out
validation. Specify sample counts, uncertainty method, practical improvement
threshold and regression limit in the benchmark protocol before results are seen.
No universal speedup percentage is presumed.

Acceptance: another run can reproduce the measurement procedure; measurement
instrumentation/passive reporting preserves program outputs and retained traces.
Host timing is performance metrology, not an SI anchor for AOD relational time.

### M5 — Persistent hardware learning and guarded specialization

Implement a bounded deterministic selection policy over semantically validated
variants. Begin with two or three choices: packed versus decoded ternary kernels,
plain versus optimized code generation, and validated inlining choices. Add batch
size/layout/thread choices only where the language semantics permit them.

An initial policy can use controlled exploration and a robust measured-cost table;
a neural model is not required. Declare its objective, update rule, exploration
budget, confidence criterion and fallback. Retain measurements and decisions across
runs with host/software/workload bindings. Never convert missing evidence into a
forced positive learning signal.

At window k: observe completed runs, retain evidence, update the policy. At a
strictly later invocation/window k+1: choose a guarded variant. Record the chosen
variant, validity predicates, result checks, performance and rollback decision.
Optimization selection may change implementation costs; it must not change Dec
mathematics, error order, closure counts or retained events.

Required controls use identical workloads and initial states:

1. Generic baseline with no learning.
2. Passive measurement with no adaptation.
3. Frozen previously learned profile.
4. Active learning applied to later calls.
5. Stale, shuffled or deliberately invalid profiles, rejected or neutralized.
6. Removal of retained profile evidence to test its causal role in later selection.

Acceptance: the retained state demonstrably changes later selection in a fixture;
semantic equivalence holds in every control; a performance-benefit claim passes
held-out measurement including profiling/compilation cost. A zero benefit or
regression is a valid recorded result and triggers baseline retention/rollback.
Cache/profile migration, corruption, host feature changes and interrupted writes
must be tested. A learned result never certifies its own scientific premises.

AOD-facing interpretation remains explicit: retained measurements → declared
response weights/selection → later implementation choice → observed effect. This
is a proposed compiler control model; hardware equivalence to FRG/awareness is a
separate claim requiring source binding and corresponding causal tests.

### M6 — Native frontend and genuine self-hosting

After collections/modules/I/O are usable, implement the compiler frontend and
lowering in Dec. The frozen seed builds compiler stage1; stage1 builds stage2;
stage2 builds stage3. Compare stage2/stage3 artifacts under reproducible-build
conditions and compare behavior across the entire corpus. Record runtime/linker
and backend dependencies still required. Remove Python from the production path
only when this chain works; retain the reference implementation as a test oracle.

Self-hosting does not block M1–M5. It is separately demonstrated, never inferred
from a generated ELF file, a banner or an ability to open a source file.

## Deliverables and work organization

Suggested source areas: `frontend/`, `ir/`, `runtime-native/`, `backend-llvm/`,
`jit/`, `profile/`, `learning/`, `conformance/`, `benchmarks/`, and `bootstrap/`.
Preserve the existing reference runtime and review evidence. Extend one versioned
CLI with `check`, `run --engine=reference|jit`, `build`, `profile`, `learn`, and
`inspect`. These are proposed commands; only R01's commands exist today.

Every milestone ships runnable source, precise supported-feature inventory,
source/toolchain hashes, negative cases, differential results, build instructions,
a detached patch and SHA-256 manifests. Keep benchmark evidence distinct from
semantic correctness. First backend coverage is x86-64; AArch64 must actually run
before release claims include it. GPU and heterogeneous-memory learning are later
extensions after a measured CPU baseline and explicit concurrency semantics.

The immediate implementation packet is **M0 + M1**, with full-native-core runtime
work following and profiling harness design allowed in parallel. This gives an
observable native executable early while keeping JIT/learning on the same path.
No arbitrary schedule is promised before the backend/dependency spike establishes
the available build environment and integration cost.

## Technical references checked for this plan

- LLVM ORC v2 design: https://llvm.org/docs/ORCv2.html
- LLVM JITLink: https://llvm.org/docs/JITLink.html
- LLVM target-independent code generation: https://llvm.org/docs/CodeGenerator.html
- Cranelift JITModule alternative: https://docs.wasmtime.dev/api/cranelift_jit/struct.JITModule.html

These provide backend capabilities and API context. All Dec semantics, acceptance
criteria, runtime architecture and learning policy above are proposed project
choices, not consequences claimed from those external documents.
