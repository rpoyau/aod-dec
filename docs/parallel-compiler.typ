#set document(title: "AOD D.E.C.: Parallel compiled execution", author: "AOD Dec project")
#set page(paper: "a4", margin: (x: 22mm, y: 20mm),
  header: text(size: 8pt, fill: rgb("536471"))[AOD D.E.C. #h(1fr) Compiled batches and retained response],
  footer: context align(right, text(size: 8pt, counter(page).display())))
#set text(font: "Libertinus Serif", size: 10.5pt, lang: "en")
#set par(justify: true, leading: .5em)
#show heading.where(level: 1): set text(size: 16pt, fill: rgb("173d50"))
#show raw: set text(font: "DejaVu Sans Mono", size: 8pt)
#set table(inset: 5pt, stroke: .4pt + rgb("cbd5df"))
#show table.cell: set par(justify: false)

#text(size: 25pt, weight: "bold", fill: rgb("173d50"))[Parallel compiled execution]
#v(5pt)
#text(size: 12pt)[Independent Dec invocations on CPU threads and CUDA lanes]
#v(8pt)

*Setup.* A typed batch consists of independent argument rows for one selected
Dec signature. Each row retains its own result, logical fuel and first failure;
output order is input order. Shared mutable source statements and formation
histories are not partitioned by this interface.

= Source compilation and native dispatch <compiled-batch>

The CPU target lowers source through the existing LLVM compiler. A generated
uniform entry calls the guarded machine-code function. A bounded C++ thread
dispatcher enters that function with an isolated native context for each row.
Contexts own their exact GMP values, fuel and error state. The owner keeps code
alive until every worker joins. Python prepares and loads the batch but does
not execute its source expressions.

The GPU target lowers the verified integer/Boolean expressions to CUDA source.
NVRTC compiles that generated source to PTX; the CUDA Driver loads and launches
it. Device return values supply the results. Changing the Dec expression changes
the computation; no function name selects a handwritten classifier.

#table(columns: (1fr, 2.5fr, 2.4fr),
  table.header([*Property*], [*CPU batch*], [*GPU batch*]),
  [Execution], [Generated LLVM on native threads], [Generated CUDA on device lanes],
  [Integers], [Exact GMP integers], [Checked signed 64-bit target domain],
  [Control], [Native fuel and source failures], [Executed-path fuel and source failures],
  [Current], [Exact host rational assembly], [Device coefficients consumed by exact host assembly],
)

The initial common subset admits integer/Boolean parameters and results,
assignments, assertions, return, comparisons, lazy Boolean and conditional
expressions, unary operations and addition, subtraction and multiplication.
Calls, recursion, emitted effects and other types are rejected for the selected
entry. GPU inputs and executed integer intermediates must fit its domain;
overflow is an explicit target limitation, never a wrapped exact answer.
A required GPU request does not silently use a CPU.

This is a JIT batch interface. Scalar native executables remain available through
`aodc build`. Standalone GPU executables, all-language GPU support, AArch64 and
self-hosting do not follow from this extension.

= Moving boundaries as a compiler application <compiled-boundaries>

`examples/boundary_coefficients.dec` computes signed incidence
$chi_R(u)-chi_R(v)$ and swept membership
$chi_(R_"after")(c)-chi_(R_"before")(c)$, with occurrence category and availability
checks. A finite integer encoding returns orientation and flags; the host checks
that representation before consuming it.

Original identities, scoped frames, pending obligations and retained witnesses
remain joined on the host. Exact rational assembly computes crossing minus sweep
plus shedding. A full query cannot resolve unsigned burden without its declared
signed-current projection. Canonical replay and an independent joined-row check
validate the complete output. Compiler parallelism changes execution placement,
not native formation or a physical transition law.

#pagebreak()
= Retained awareness and a later invocation <compiled-awareness>

Main distinguishes the present state, immutable closure memory and retained
relational information. Its operativity criterion requires a later realized
consequence under matched removal and restoration. Retaining a log or naming a
backend does not by itself meet that criterion.

The existing Manual runner `aod_awareness.py` already implements persistent
learning, action-return provenance, reflexive queries and fold/carriage controls.
This compiled example reuses its response and query model:
$ J_"next" = J + (A + q + g sum_i r_i)/p. $
For its declared $p=g=1$ integer case, `awareness_response.dec` compiles the
response, sign selector and later query. The host validates actual prior
returns, contacts and retained provenance. Only the retained relation is
removed and restored; present state, closure memory, law and future input are
matched. At cuts 34–37, retained/restored currents are $(1,2,3,4)$ and removed
currents are $(0,0,0,0)$. The computed selector drives lawful H1/fibre edges.
These continuations certify no new closure cycles.

The existing `learning()` example continues from the actual fourth compiled
state: retained/restored currents are $(5,6,7)$, removal gives $(4,4,4)$.
The existing `mindfulness()` example starts at current three; compiled
$q_k=-J_k$ uses the committed observation and gives six currents equal to one.
Its delayed control instead gives $(4,2,-1,-2,0,3)$ from the Manual's declared
initial history. Pressure controls $p=1/2$ and $p=2$ retain exact rational host
execution; they are outside this $p=1$ integer target. Passive inspection leaves
state and event bytes unchanged. Generated values must reproduce the Manual.

The existing retained-cost policy is a separate compiler control model:
prior measurements may select between prevalidated native variants with equal
semantic outcomes. This example supplies no cost evidence and keeps generic
O0. Hardware duration adds no AOD clock or awareness transition law.

= Tracing the existing evolution <existing-evolution>

`run_engine_case()` invokes the unchanged Manual `learning()` or
`mindfulness()` function. A scoped observer records its actual call and return
events: the committed state, retained sources, query observation cut,
`transition()` input, native `update()` and resulting state. The query and
later consequence already belong to that execution. The observer supplies
neither operands nor a response rule. On Python 3.12 or later, observation uses
local monitoring events on the selected source functions. The broad profiler
remains an explicit comparison and a compatibility fallback. Both mechanisms
must capture identical records.

Call-tree identities distinguish matched branches that share native cut numbers.
Native histories keep their own immutable identities. Typed retained witnesses
bind each transition and update; verification replays them separately and checks
their combined histories. Event and byte limits produce explicit partial
coverage while preserving the engine's result or original error. A previous
profiler is restored after observation. Complete state and event values must
match execution with tracing disabled.

#pagebreak()
= Reusing the compiled runtime <runtime-reuse>

`CpuBatchSession` keeps one JIT and dispatcher alive across successive calls,
loading each selected entry once. Every lane still owns a fresh native context,
fuel and value arena. `GpuBatchSession` retains one CUDA context, compiled
entries and capacity-checked buffers. Each call restores the caller's CUDA
context; calls and cleanup share the owning lock. Source and input checks
remain in both paths. Context managers release executable and device resources.

The compiled awareness checker uses one CPU session across six families and
one GPU session across five. Its generated response, query and selector still
undergo the same native admission and complete-outcome comparison. Reuse removes
repeated runtime setup. It does not remove retained histories or infer a new
evolution from timing observations.

= Execution and evidence <compiled-reproduction>

The compiler reads an array of typed argument rows. For example,
`[[0, 1, 0, 1, 0], [1, 0, 1, 1, 0]]` selects an occurrence and a sweep:

```sh
python aodc.py batch examples/boundary_coefficients.dec \
  --function coefficient --inputs rows.json --backend cpu --workers 4
python aodc.py batch examples/boundary_coefficients.dec \
  --function coefficient --inputs rows.json --backend gpu
python scripts/trace_simulation.py boundaries TRACE \
  --backend compiled-cpu --workers 4 --coverage-scope declared-current
python scripts/trace_simulation.py boundaries TRACE \
  --backend compiled-gpu --coverage-scope declared-current
```

`make check-compiled-boundaries` compares compiler results with an independent
finite truth table, the reference VM and complete retained boundary views.
`make check-awareness-jit` exercises the matched awareness example. Normal and
optimized Python modes are checked separately. GPU absence is recorded; an
explicitly required device run must execute a real kernel.

`scripts/check_introspection_evolution.py` records the original engine with
tracing on and off, then compares the same compiled continuation using one-shot
and reusable runtimes. Timed intervals include construction, first loading,
execution, cleanup and report serialization. Shared fixture preparation is
excluded from both compiled intervals; verification has a separate recorded
cost. The original engine's trace overhead and the compiled adapter's runtime
reuse ratio are different comparisons. Neither establishes a general engine
speedup or an adaptive compiler policy.

Source, verified IR, generated artifacts and target capabilities bind execution
evidence. All measured samples, including slower outcomes, are retained.
Equality establishes
the bounded computation inspected, not empirical accuracy of an unmeasured
physical observable.

*Source binding.* Main and Manual commit
`ab8359209b8f9b2ef4007403101016e3ea58cfe4`. Awareness anchors: Main 08,
lines 6–97 and 154; Manual `00_awareness`, lines 39–108, 128–173, 225–230
and 315–372.
Exact file hashes and executable fixture bindings accompany retained evidence.
Earlier lineage is recorded separately: the request's
`03_hardware_tiers_frg_awareness.dec` design and the 2026-11-14 boundary-accounting
replay of 32 existing checks. These models predate this backend integration.
Main and Manual remain unchanged.
