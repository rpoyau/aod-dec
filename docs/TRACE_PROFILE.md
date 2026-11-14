# Tracing scoped D.E.C. applications

The trace records committed calculation events and retained evidence. Detectors
classify that history under the declared model. A trace event ordinal orders the
serialized record; it is not an SI time or a physical bip rate.

The implementation follows Main's retained address, return and fusion constructions,
Manual's execution-before-detection rule, and the separately declared finite AFC
reduction. The source review binds the supplied v40.01r10 archive and the later
R04 Main/Manual candidate used by the existing applications. It does not modify
those manuscripts or publish a Manual III.

## Run and inspect

The dedicated host CLI is `scripts/trace_simulation.py`. Examples:

```sh
python3 scripts/trace_simulation.py run --example closure --trace build/closure-trace --output build/closure.json
python3 scripts/trace_simulation.py replay build/closure-trace
python3 scripts/trace_simulation.py inspect build/closure-trace --start 0 --stop 8
python3 scripts/trace_simulation.py run --example regions --trace build/region-trace --output build/regions.json
python3 scripts/trace_simulation.py boundaries build/region-trace
python3 scripts/trace_simulation.py run --example particles --trace build/particle-trace --output build/particles.json
python3 scripts/trace_simulation.py support build/particle-trace
python3 scripts/trace_simulation.py relations build/particle-trace
python3 scripts/trace_simulation.py encounter build/particle-trace
python3 scripts/trace_simulation.py paths build/particle-trace
python3 scripts/trace_simulation.py replay build/particle-trace --output build/particle-replay.json
python3 scripts/trace_simulation.py finite-scope --output build/finite-scope.json
python3 scripts/trace_simulation.py campaign --output build/trace-campaign
```

Use a fresh trace directory for a new run. The API explicitly selects read-only
replay or writer resumption for existing stores. Run manifests bind model/kernel,
preparation, engine, source, scope and RNG declaration. Performance clocks are
recorded separately from canonical scientific values.

`support` infers generic constructions from retained formation operands. Its
optional `--start`/`--stop` select native committed cuts; `--record-window` declares
a separate additive-count participation view. Neither selects a desired motif.
`relations` validates coupling, shedding and seating independently, preserving
unavailable evidence and non-additive alternative windows. A `Duad` state retains
the identity of its witnessed Duon current.

`encounter` reconstructs the recorded initialization and every source-model update,
including pending attempts. `paths` joins the retained source history, proton
preparation, coupling/fold, native shedding, route, support, seating and receipt.
The full `replay` result also includes these path checks. Each query reports the
coverage of the evidence it actually validated.

## Event identity and evidence

`simulations.trace_store.TraceStore` stores checksummed event chunks and a hash
chain in SQLite. Stable content-bound IDs, causal-parent references and scoped
source keys preserve identity across replay. Byte-identical re-ingestion is
idempotent; a conflicting duplicate is rejected. Exact integers exclude Boolean
and float aliases at index/type boundaries. Fractions are normalized
`{"$ratio": [numerator, denominator]}` objects.

`runtime.dec_core.observe_commits` observes actual retained starts and transitions.
Read-only validation does not emit a new transition. Closure detection reads those
primitive events and invokes the bound validators; an endpoint match alone is not
a closure witness. A stored detector result is a separate derived record.

`return_candidates` contains raw paths that satisfy the return predicate. A lawful
counterbranch can pass the retained start and continue before the model commits a
longer closure. The detector therefore keeps bounded alternative segmentations.
It checks each committed closure span against the full raw path, hinge and digest
before counting a phase; the first committed closure has zero reclosure beats.
Earlier valid endpoints remain visible as candidates marked `not_selected`.
Unconfirmed candidates leave `phases` and `reclosure_beats` null, while
`observed_committed_phases` reports the confirmed prefix. Exhausting the parse
budget makes coverage partial instead of selecting an arbitrary interpretation.

Core contact events bind retained return identities, certificates and causal
parents. Repeated valid contacts expose the core's cadence, signed lag and drift;
`coupling.dec` gives `1`, `−1`, and `0` in its declared `a|b` orientation. A single
contact is unresolved. This core comparison does not construct the Manual's
directional RCD, which remains explicitly unavailable unless its own producer
supplies the required records.

`simulations.region_engine.observe_commits` captures admitted region-input cuts.
`simulations.boundary_accounting.observe_commits` captures validated archive,
route and receipt deltas. These are host application commitments. They do not
claim the full application has been lowered to LLVM or that report records are
new primitive closure events. Core observer failures preserve the model result
and mark trace coverage partial; application wrappers stop dependent observation
when capture fails. Previously committed immutable history remains recoverable.

The particle adapter additionally stores real chunked report and witness bytes.
`trace_applications.retrieve_blob` reloads them under a caller-selected byte cap.
A source hash alone is never returned as a recovered witness.

Support and relation queries also admit the union of their inspected native
histories. Scoped event coordinates, terminal returns and overlapping wave
prefixes must agree across captured carriers, components and folded inner waves.
Identical shared bindings and earlier lawful prefixes remain valid. A conflict or
exhausted history budget makes coverage partial and withdraws dependent verified
formations and relations. Python queries expose `max_history_bindings` and
`max_history_nodes`; `history_union` reports their use and any rejection reason.
Dion and pivot construction signs require exact integers at producer admission
and decoding. Boolean metadata remains a separate permitted payload type.

`trace_applications.replay_accounting(store)` reconstructs the actual archive,
route partition and receipt records through the bound accounting validators.
Its replay rejects changed amounts, same-cut or duplicate receipts, wrong
destinations and missing causal parents. The proton trace automatically exposes
its exoshedding route, pending destination and later receipt; alternative windows
stay separate branches. This accounting dialect does not supply the unresolved
burden-to-signed-current projection. Query event and serialized-byte caps return
incomplete coverage rather than dropping a pending route.

## From population to a routed path

The declared source run initializes two `1:2` populations and executes six updates.
`particle.encounter_init` records the initial state, retained-history identities,
source hashes and requested update count. Each `particle.encounter_update` records
the actual masks and exact tickets, selected opportunities, before/after states,
world digests and retained-history dispositions. `particle.encounter_finish`
binds the terminal state, completed update count and final world digest.

Two updates leave carrier A pending while carrier B gains a return. These attempts
remain in the trace. Relations that expire stay in the retained history with an
inactive disposition; later contacts and new relations remain distinct. Encounter
replay reconstructs the bound model from initialization and checks every captured
operand and outcome. Missing initialization, a skipped update or an absent
terminal marker cannot establish complete execution coverage.

The current application then maps the completed source histories into separately
admitted `3:3` C6 proton preparations. The joined path checks that mapping and
follows the actual coupling and outer fold into the native closure burden,
positive surplus, accounting route, recurring Duon support, Duad seating and
later receipt. Seating and receipt are separate events; a received route does not
by itself establish a seated state, and seating preserves the recorded current.

Before evaluating the alternatives, `particle.routing_plan` binds the actual
input windows, capacity, closure cost, units, source history and outer return.
Replay checks that plan against the bound producer configuration. Every planned
window must have its own `particle.routing_finish` marker, bound to its window,
cut and native shedding record. The four terminal ledgers preserve the actual
routes, receipts and pending legs for their own alternatives. Deleting an entire
branch cannot create a complete shorter experiment. If a receipt event is missing
while that branch's terminal ledger records receipt, path coverage is partial;
the missing capture cannot be renamed as physically pending. Alternative views
are never accumulated into extra flow.

Prepared proton beams and cavity/sensor populations are valid experiment inputs.
The existing evolution law here still advances the `1:2` source before proton
materialization. Full evolution of the proton population together with the cavity
and sensor remains an application/engine task. The trace checks above certify the
executed source model and its explicit mapping, rather than that unimplemented
experiment. No expected support signature is inserted into the detector.

## General support and current classification

Detection applies to the admitted support family, including `1:2:x` and `3:j:k`;
Tau is one possible downstream interpretation. The core follows the ordered
component construction, while `x` or `k` qualifies an observed enclosure or an
explicit record-window view. A query window does not create a new physical form.
Core signature, chirality, closure/recurrence, seating, coupling and shedding are
separate outputs with separate evidence.

A Duon requires Dion–monon-pivot–Dion construction. A Duad is the seated state of
that same admitted Duon identity. A seated status string alone supplies no seating
witness. Coupling requires bound recurrent lineages and repeated contact evidence.
Shedding requires its closure-load residual and route accounting; zero residual,
deficit and positive surplus remain distinct. Missing state, contact or route
evidence reports an unresolved axis rather than an inferred absence. Detecting a
support does not by itself establish Duad seating, coupling or shedding.

## Windows, coordinates and scales

Observation views use `[start, stop)` in their named index domain. An empty
observation window has no events. A left-edge return requires retained start and
parent memory; an unfinished right-edge return is censored. Acquisition,
comparison, participation and retention scopes remain distinct.

For compatible duration quantities, participation clips as
`rho_window = min(rho, window)`. RD and the directional RCD object are unchanged.
Alternative proton windows select the same physical history and are non-additive.
Opposing boundary views preserve the original constituent identities even when
summation gives zero.

Main's fractal coordinate is a retained octal address. The source family's
admitted fusion rungs require compressed inner histories and an independently
executed outer closure. A reporting scale factor preserves its declared quantity;
it does not manufacture a higher formation. The trace's family/index metadata is
not a frequency-doubling law. Spatial/support span is separate from all temporal
window domains.

## Bounded execution and replay

Store caps cover an event's bytes, exact-integer size, structural depth/node count,
durable archive size, including journal reserve. SQLite's page-cache setting is a
configured target, not a hard process-memory limit. There is
no Python set containing every historical event identity and no hidden writer
queue. The store caps do not automatically bound the caller's model, detector or
operating-system RSS; the profile records these scopes separately.

A complete producer marks its store complete only after verification. Interrupted
runs remain partial. Checksummed checkpoints retain model/RNG state at declared
commit boundaries. Missing, damaged or budget-exceeding evidence reports
unavailable/incomplete status. Finer queries reload recoverable witnesses or stop;
they cannot turn a digest or lossy telemetry into complete evidence.

After model execution, a checkpoint or diagnostic-storage failure leaves the
computed result, emitted prefix and original model error in `run_core`'s
`outcome`. Trace failure stages are reported separately with partial coverage;
unrecoverable diagnostic fields are unavailable rather than reconstructed.
Implicit read-only reopening restores validated persisted limits before reading
the manifest. Limit metadata has a fixed 4 KiB byte bootstrap; explicit caller
limits remain authoritative for the manifest and subsequent reads.

Exact boundary cancellation preserves the current at the declared cut. Advancing
an aggregated model requires sufficient update state. The finite scope example
checks blockwise Markov transition sums and paired fine/reduced trajectories. Its
counterexample has equal present boundary values but different later responses.
Kernel, boundary, window or recurrence/coupling changes invalidate a reuse record
when its declared dependencies change.

`scoped_reduction.FixedPointReuse` calculates a fixed point before admitting
skipped updates; unchanged kernel, boundary, window, phase and coupling are
required on every reuse. The current executable finite reducer supports level
0. Typed source-bound views can name other admitted levels, but they do not
provide higher-scale dynamics in this reducer.

`recurrence_cache.find_cycle` also searches bounded exact quotient trajectories
for a transient followed by a finite cycle, including periods greater than one.
Its immutable cache checks the kernel, partition and complete update-domain
binding before answering a later step within the admitted window. A search cap
without a repeated complete state reports `not_established`; exhausted byte or
integer budgets report `incomplete`. Equal boundary values do not certify a cycle.

## Applications and interpretation

| Application | Fresh executed result | Preserved limit |
|---|---|---|
| Maser D0 region preparation | Pump/output/return channels, dropout and whole-apparatus cancellation | No new hyperfine emission or SI power prediction |
| Earth–water–Moon D0 preparation | Moving water current `8 − 3 + 2 = 7`; shell, parent and complement accounting | Prescribed sweep and currents; no computed tide height |
| Proton pair | Source population and six updates replayed; mapped C6 carrier witnesses; contact projections `−6, −12, −18, −30`; joined surplus/route/support/seating/receipt evidence | Alternative windows non-additive; burden-to-signed-shedding-current projection remains unresolved |
| Electron above wire | Wire current `4, 0, 4, 5`; later electron current `4, 4, 8, 13`; union current zero | Declared finite placement/response; no computed SI force |
| Finite nested scope | Fine/reduced evolution under an explicit finite kernel and checked update sufficiency | Computational mechanism, not a Sun–Mercury trajectory |
| Collision cavity / Tau search | Generic support detection and replay of the declared source-to-proton preparation mapping | The actual proton, cavity and sensor populations are not yet evolved together by this runner |

The inputs to the Tau search are the proton collision and collision/sensor cavity
setup. Manual's `manual:dec-ledger` orders raw execution, trace recording and
detector naming. A candidate-specific Tau recipe or an expected `1:2:9` label is
not an input requirement. Support detection must derive the core from committed
component structure and bind its actual enclosure/window qualifier. Record length
`L_M`, RD and directional RCD remain distinct: both the Manual's `1:2:9` and
`1:2:10` fixtures have scalar duration 9, so that scalar alone cannot identify
`1:2:9`. Route evidence separately supports the `yro` status.

Generic recognition of a retained construction does not establish that a proton
collision produced it. The present runner first evolves its `1:2` source and then
maps those histories into `3:3` C6 carriers. Prepared proton inputs are legitimate;
evolving that population in the cavity and sensor setup remains implementation
work. A
detector must distinguish no occurrence in a covered trace from insufficient
evidence or unsupported generation. The historical `31/50` diagnostic is a
comparison record; reproducing its original scoring calculation is a separate
provenance task and does not block a fresh run or force its outcome.

The Sun–Mercury source provides O0 comparison maps and targets. A celestial
trajectory additionally requires executable retained preparations, initial state,
coupling and boundary-current evolution, and the observation map. The finite scope
demonstrator does not assign physical meaning to arbitrary transition numbers.

## Profiles, errors and validation

Profiles separate model work, detectors, aggregation/replay, trace I/O, compile,
load/warmup and steady execution where those phases actually run. Each metric
names its measurement method, unit, sample count and availability. Include all
costs in paired repeated comparisons. Missing device counters are unavailable,
not successful device measurements; a scalar JIT check is not full native region
execution.

Observable records retain input, model, truncation, sampling, numerical, detection
and anchor error categories with their dependencies. Unknown bounds stay unknown.
Fresh region outputs include 14 observations: each scoped boundary total and the
maser detected-output total. Particle outputs include four proton contact
projections, four explicitly unresolved full currents and the wire/electron/union
totals. Each binds preparation and kernel source hashes, native units, the cut
window, participation windows and retrievable report evidence. `result:` evidence
locators are JSON pointers in that output; `artifact:` locators refer to unchanged
particle report or witness files. Exact numerical bounds are conditional on the
declared finite operands. Physical model discrepancy remains unknown, and no SI
calibration is invented. The original example arrays and particle files retain
their previous exact values.
Shared inputs require covariance or a joint interval treatment. Angular tau is
an exact full-turn representation, distinct from the Tau application name.
Wall-clock profiling never supplies the model's SI anchor.

All 19 strict admission cases are discoverable in repository CI. Isolated identity,
reflection-type and combined mutations must trigger exactly their 3, 4 and 7 named
failures in each Python mode. Trace integration additionally exercises passive
output/error equivalence, hostile detectors, replay integrity, retrieval caps,
storage failures, boundary edges and source rebinding. The per-criterion delivery
record, not a total test count, reports actual TP-01 through TP-30 coverage.
