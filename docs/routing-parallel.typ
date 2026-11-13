#let data = json(sys.inputs.at("results", default: "../evidence/routing-parallel/results.json"))
#assert(data.status == "PASS")
#let traces = data.tracing.summary
#assert(traces.len() >= 3, message: "This worked example requires at least three traced seeds")
#let ink = rgb("173d50")
#set document(title: "AOD Dec: Routing, Octal Traces and Parallel Execution", author: "AOD Dec project")
#set page(paper: "a4", margin: (x: 22mm, y: 20mm),
  header: text(size: 8pt, fill: rgb("536471"))[AOD Dec #h(1fr) Computational application],
  footer: context align(right, text(size: 8pt, counter(page).display())))
#set text(font: "Libertinus Serif", size: 10.5pt, lang: "en")
#set par(justify: true, leading: .62em)
#set heading(numbering: "1.1")
#show heading.where(level: 1): set text(size: 19pt, fill: ink)
#show raw: set text(font: "DejaVu Sans Mono", size: 8pt)
#set table(inset: 6pt, stroke: .4pt + rgb("cbd5df"))
#let tab(..args) = text(size: 9pt, table(..args))
#let note(body) = block(width: 100%, inset: 10pt, fill: rgb("edf5f6"),
  stroke: (left: 2pt + rgb("2c7181")), body)

#text(size: 26pt, weight: "bold", fill: ink)[Routing and parallel execution]
#v(4pt)
#text(size: 14pt)[Proton-facing support, retained octal coordinates and wave observables]
#v(8pt)
This application extends the core-biased chiral encounter with admitted
`3:3` carrier supports, a computed surplus route, octal address records and
parallel execution. Its supplied stochastic model remains separate from the
execution engine. The result is a collision-inspired computational fixture;
the physical proton-proton preparation and scattering correspondence remain
uninstantiated.

= Declared model and execution boundary <routing-setup>

*Setup.* Retain the prior eight-facet support and initial carrier state
`("0***", "1***", +1, -1)`. The primary model uses post-move flip probabilities
$q_o=1/16$ and $q_c=1/4$, with contact gain zero. Eight updates select eight
complete-return orientations per carrier. Independent host streams prepare
movement tickets in $0..7$ and flip tickets in $0..63$ once. Every backend
receives the same tables and tickets.

#tab(columns: (1.1fr, 2.9fr),
  table.header([*Layer*], [*Responsibility*]),
  [Application model], [Supply the population profile, movement table, exact flip thresholds, initial states and tickets. Declare support, load, routing and retention operands.],
  [Scalar / multicore / CUDA], [Apply supplied integer tables. Retain both destination facets, both signs, flip bits and contact at every update. No probability law is intrinsic to the backend.],
  [Host admission], [Construct and validate complete returns, component/motif evidence, outer closure, surplus route and retained histories.],
  [Observation map], [Supply separate time, phase and propagation operands before a wavelength in metres can be computed.],
)

*Proton-facing construction.* Each selected orientation is mapped into a
separately constructed `3:3` support. Three distinct prior Monon component
returns supply three ordered blocks, each referencing that same bound triple,
and a retained three-member shell. A later
support trace and motif witness bind these occurrences. Two formation returns
and eight selected returns establish the recurring support history.
This follows the Matter I support interface; `3:3` is not a claim of three
outbound and three return traversals, nor an automatic physical proton identity.

The source carrier identity, materialized identity and every source-to-support
return map remain explicit. Components precede their support construction.
Continuation namespaces bind the selected path; alternative histories cannot
reuse an event identity with different contents. Both lineages remain retained
when their signed contributions cancel.

#note[
The host construction performs admission after candidate execution. GPU
candidate rows alone are not closure, coupling, fusion or shedding certificates.
This extension adds a CUDA application backend, not a general GPU target for
the D.E.C. language compiler.
]

#pagebreak()
= Shedding as computed surplus routing <routing-surplus>

*Declared bridge.* For the retained encounter window, let $F_A,F_B$ count
selected flips. This example supplies the dimensionless count-derived load
$L=F_A+F_B$ and exact cost $C=1$. It does not identify flip count with SADAR
pressure or SI energy. Compute
$ Delta=L-C, quad X=max(0,Delta). $
Negative $Delta$ is a deficit/open check, zero gives no shedding, and positive
$X$ requires the separately declared surplus route.

#tab(columns: (.6fr, .7fr, .7fr, .8fr, .8fr, 2fr),
  table.header([*Seed*], [*$L$*], [*$C$*], [*$Delta$*], [*$X$*], [*Status*]),
  ..(1, 0, 2).map(index => {
    let row = traces.at(index)
    (str(row.seed), raw(row.load), raw(row.cost), raw(row.residual), raw(row.surplus), text(row.status.replace("_", " ")))
  }).flatten(),
)

*Witness chain.* Retain both admitted `3:3` waves. For positive surplus,
construct a separate recurrent Duon with an actual bound Monon pivot, two
Dion patterns and complete support returns. Its core is $E_2$; enclosure 6 is
a separate qualifier. A new outer trace independently closes a proposal bound
to the two carrier histories. Route commitment follows that closure.

The route record binds source state, selected events, exact load and cost,
model-bridge payload, outer certificate, retained form and target scope/scale.
Validation reconstructs these payloads and checks their contents, rather than
accepting a digest-shaped string as evidence. The source state's current
orientation is the final retained return's sign.

*Retention.* This fixture declares seat compatibility, an available retention
of two native construction ticks and a two-tick route window. The required
inequality is $"TTL" >= abs(omega)$. These are supplied application operands;
cadence and RCD duration do not establish them. Missing/incompatible seats,
short retention or an absent admitted form reject positive surplus routing.

*Partition.* The primary case sends all positive surplus to redirected support:
$ X=X_("endo")+X_("exo")+X_("redir")+X_("open"), quad
  (X_("endo"),X_("exo"),X_("redir"),X_("open"))=(0,0,X,0). $
The implementation also checks an exoshedding control which exposes the bound
inner scope while preserving its identities and certificates. Endoshedding and
open-loss destinations have no active implementation in this fixture; their
partition entries remain zero. Moving a coarse cell or scheduling a CPU job is
not thereby sheddic routing.

*Falsification.* Tests alter return histories, source payloads, route partitions,
retained-form evidence and retention operands. The zero/negative
controls must acquire no sheddic route, while the positive control must retain
its admitted form and exact surplus balance.

#pagebreak()
= Octaves: retained octal coordinates <routing-octal>

Main's first finite anchor is $a_0="00"_8$. Retained addresses use finite
strings of octal digits. Octal notation encodes address. It does not identify address with the Q4 vertex,
fibre zero, event ordinal, boundary scope or a count of beats.

*Finite encoding.* The application explicitly maps each immutable event to a
scoped trace namespace and the base-8 digits of its committed construction cut.
This index encoding is an application choice, not a universal address formula
derived from Main. Q4 vertex bits and ternary fibre roles remain separate fields.

#let address-rows = traces.at(0).observables.a.octal_address_records.at(0).rows
#tab(columns: (1fr, 1fr, 1.2fr, 1fr, 1fr),
  table.header([*Event ordinal*], [*Native cut*], [*Octal digits*], [*Q4 integer*], [*Q4 bits*]),
  ..address-rows.map(row => (str(row.event_ordinal), str(row.committed_cut),
    raw(row.retained_address.digits + "_8"), str(row.q4_vertex), raw(row.q4_vertex_bits))).flatten(),
)

The table is carrier A's first materialized support return. Its equal start
and end Q4 vertices do not erase their distinct event identities or octal
addresses. The closure witness binds the terminal event back to the retained
start. Every component Monon also retains its own address records.

*Beat and cadence.* For ordered distinct compatible return completions
$k_0<k_1<dots<k_m$, the first closure supplies the origin and later reclosures
contribute beats. Thus $N$ completed closures give $N$ closure-phase cycles and
$max(0,N-1)$ beats. A beat record points to its own completion event, octal
address and return identity; it is not replaced by the address digits.

#let obs = traces.at(0).observables.a
#assert(obs.cadence.all(x => x == obs.cadence.first()))
#tab(columns: (1.6fr, 1fr, 2fr),
  table.header([*Quantity, per carrier*], [*Evaluated value*], [*Scope*]),
  [Formation returns], [2], [The first two materialized supports.],
  [Selected returns], [8], [Eight supplied chiral decisions.],
  [Closure-phase cycles], [#obs.closures], [Distinct materialized support returns.],
  [Beats], [#obs.beats], [Origin-qualified reclosures.],
  [Ordered separations], [#obs.cadence.first() each (#obs.cadence.len() intervals)], [Native construction-cut cadence.],
  [Support traversals], [#obs.atomic_traversals], [Component formation traversals are retained separately.],
)

The materialization adds component construction steps, so its cadence differs
from the original short-return fixture. Neither cadence, octal notation nor
fold scale implies a musical frequency octave, a fixed eight-event period or
a physical wavelength. Tesseract support multiplicity and beat count remain
distinct even where the historical Main label is named `eq:beat-count`.

#pagebreak()
= CPU and GPU execution evidence <routing-backends>

*Integer contract.* Tables contain eight-by-eight movement destinations and
flip thresholds. Each path has a complete initial state and four supplied
integer tickets per update. The common denominator is 64. Backends preserve
uint32 ticket comparisons and int32 sign outputs; unsupported bounds, Boolean
aliases and inexact inputs are rejected before launch.

The recorded run evaluates #data.candidate_execution.scalar.path_count paths.
All #data.retained_reference.paths_checked available prior paths are compared
event by event against the previously bound cohort. Spawned CPU workers retain
canonical path order, so changing worker/chunk allocation changes no model
outcome or semantic hash.

#tab(columns: (1.4fr, 1.2fr, 2.3fr),
  table.header([*Operation*], [*Recorded status*], [*Coverage*]),
  [Scalar CPU], [#data.candidate_execution.scalar.status], [Exact supplied-table candidate evaluation.],
  [Multicore CPU], [#data.candidate_execution.multicore.status], [#data.candidate_execution.multicore.execution.participating_worker_pids.len() participating processes; identical candidate records.],
  [CUDA compilation], [#data.gpu.compilation.status], [Offline NVRTC/PTX compilation, when the library is present.],
  [CUDA execution], [#data.gpu.execution.status], [Actual device status; unavailable hardware is never a CPU fallback labelled GPU.],
  [Host trace/admission], [#data.tracing.status], [#data.tracing.paths complete traces; identical scalar and multicore witness/routing records.],
)

The CUDA implementation assigns one supplied path to one GPU thread, with
sequential steps within that path. The Driver API handles device allocation,
copies, launch and synchronization. Its kernel contains table execution,
not a built-in core profile or probability law. Offline compilation and CPU
execution of the kernel body are separate checks; neither proves GPU execution.

#figure(image(sys.inputs.at("chart", default: "../evidence/routing-parallel/routing-parallel.png"), width: 100%),
  caption: [Exact surplus outcomes and one observed host-tracing timing pair.
  Chart coordinates use rounded display conversions.])

Timing includes path generation, construction, admission and canonical
encoding. Multicore timing also includes process startup, communication, merge
and shutdown. Final artifact compression, plotting and PDF writing are excluded.
This is one paired observation on the recorded host, not a general speedup
result. Candidate-only timing is reported separately from complete tracing.

#pagebreak()
= Wavelength map and reproduction <routing-wavelength>

*Observation boundary.* A native recurrence separation $d_j$ does not have SI
seconds or metres by itself. An explicit map must supply a time realization
$tau$ in seconds per native index, $nu$ phase cycles per beat, a phase speed
$v_p$ in metres per second, compatible native scope and provenance. Then
$ T_j=d_j tau, quad f_j=nu/T_j, quad lambda_j=v_p/f_j. $
The time between beat completions is $T_j$; $1/f_j$ is the phase-cycle period
under the supplied map. Neither $nu=1$ nor a propagation speed is inferred
from the octal coordinate presentation.

The primary record leaves the map and physical wavelength uninstantiated.
A separate synthetic dimensional control takes $d=10$, $tau=1/5$,
$nu=1$, $v_p=12$, giving $T=2$ seconds, $f=1/2$ hertz and $lambda=24$
metres. These are conditional values of supplied operands, not proton collision
predictions. Measured targets and uncertainty remain unset.

*Bound sources.* Main/Manual I is the delivered candidate
`8c3149b9d3d924b80b9385b178da4ba2c39175e8`; Matter I is the verified detached
reconstruction based on `904d4a9b2fc90fc96050f6c3b555d5d921c0a8d9`.
The source record binds file hashes and labels including `eq:first-anchor`,
`eq:closure-trace`, `eq:beat-reclosure`, `eq:cumulative-closure-counts`,
`eq:closure-residual`, `eq:shedding-excess`, `eq:shedding-route-partition`
and `subsec:sheddic-path-audit`. No upstream definition or compiler core is
changed. The latest owner clarification selects the octal-coordinate meaning
of “octaves”; it supplies no wavelength or frequency scaling law.

```sh
python3 scripts/simulate_routing_parallel.py --workers 4 \
  --paths 256 --traced-paths 16 --gpu auto --plots
make docs-routing TYPST=/path/to/typst-0.13.1
```

Use `--gpu required` when actual GPU execution is required; unavailable
hardware fails that request. Use `--gpu off` for CPU-only operation. The optional
`--seconds-per-index`, `--cycles-per-beat`, `--phase-speed-m-s` and
`--map-provenance` arguments must all be supplied together for a conditional
wavelength map. The model, candidate records and full traces are retained with
their source and payload hashes. Lossless gzip reduces trace storage without
discarding lineage, component, address or witness records.

The editable Typst companion accepts `results` and `chart` input paths. CI
rebuilds against fresh results and explicitly reports absent GPU hardware.
Pinned AOD lint covers manuscript diagnostics; witness tests, backend comparisons
and physical observation claims remain separate evidence classes.

Exact witness/count fixtures have D0 support. A conditional SI conversion alone
does not supply the measured target and observation correspondence required for
an O0 comparison; unknown physical discrepancy is not set to zero.

*Remaining physical correspondence.* Proton-facing support construction is
available. Actual incoming proton preparation, collision dynamics, scattering
observables, momentum/energy maps, spatial phase correspondence and measured
target comparison are still required for a calibrated proton-proton simulation.
