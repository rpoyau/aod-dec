# Routing/parallel companion: bounded scientific fidelity review

Reviewed `docs/routing-parallel.typ` against the bound sources in `SOURCES.md`,
the executed `build/routing-parallel/results.json`, all 16 decoded retained
fixtures in `traces.json.gz`, and the routing/wavelength implementations.
Exact reviewed hashes are in `doc-review-binding.json`. This pass checks
semantic fidelity and evaluated quantities; it does not inspect rendered page
layout, prove GPU behavior or independently validate physical proton dynamics.

## Findings

**D01 — Minor test-coverage overstatement.** The falsification paragraph says
tests alter targets. The inspected routing tests mutate history/tickets,
source-event payloads, bridge payloads, route record digest provenance,
partition, form, retention and observation count fields, but do not explicitly
mutate `target_scope` or `target_scale`. Remove the target-coverage phrase or
add a targeted test before retaining that claim. The strict rebuilding
validator is intended to reject a changed target, but intention is not evidence
that this test was executed.

**D02 — Optional construction wording precision.** Three ordered 3-member
blocks each reference the same bound triple of distinct prior Monon
occurrences. The implementation does not construct nine distinct component
returns. The document's contrast with three outbound/three return traversals
is correct; adding “each references that same bound triple” would make its
component-count reading unambiguous.

No other scientific-fidelity defect was found in this scoped pass.

## Independently checked result coordinates

Direct JSON arithmetic/count checks, without calling the fixture validator,
passed for all 16 retained fixtures:

- `L` equals the actual sum of both source flip counts; `Delta=L-C`;
  `X=max(0,Delta)`; the four exact partition entries sum to X.
- Every materialized carrier has the declared `3:3` core, ten retained support
  returns, ten closure-phase cycles and nine origin-qualified beats. Ordered
  native construction separations are nine copies of 12.
- Each materialized support return retains three component returns. The wave
  support traces contribute 20 atomic traversals per carrier; component
  formation contributes a separate 60. Neither is relabelled beat count or
  physical wavelength.
- Source WaveState orientation equals A's final retained return sign.
- Each octal address's numeric digits decode to its separately stored committed
  cut. Equal projected endpoint Q4 vertices retain different start/terminal
  event addresses. Scope, lineage, trace namespace, event ID, ordinal, Q4 bits,
  fibre roles and beat records remain separate.
- Positive surplus retains actual Duon/E2 form with separate enclosure 6,
  positive compatible declared window, sufficient available retention, and
  form completion before outer construction and later route commitment.
  Zero/negative records have no route or route form.
- Both carriers' default wavelength records are `NOT_INSTANTIATED`.

The table's selected seeds 1, 0 and 2 correctly evaluate respectively
`(L,C,Delta,X)=(0,1,-1,0)`, `(1,1,0,0)` and `(2,1,1,1)`.
Carrier A's first support return occurs at native cuts 12, 13, 14 with octal
digits `14`, `15`, `16` and Q4 vertices 0, 1, 0. The retained start relation
comes from its actual return witness rather than numerical address equality.

## Scope preserved by the document

The explicit user correction is respected: “octaves” names retained octal
coordinates, not a beat-count alias, musical frequency doubling or a fixed
eight-event period. The finite committed-cut encoding is declared application
representation, distinct from Main's general retained-address domain.

The native construction cadence 12 differs from the prior short witness's
cadence 10 because component materialization introduces additional committed
construction cuts. The separate synthetic wavelength example deliberately
uses supplied separation 10, time scale 1/5, phase cycles per beat 1 and phase
speed 12, giving elapsed time 2, frequency 1/2 and wavelength 24 in the named
SI coordinates. Those inputs do not calibrate the primary collision model.

Seat/TTL operands are declarations, not derived from cadence or RCD. The
probabilistic model remains outside the backend; GPU candidate rows do not
admit motifs. The recorded device status is unavailable, while offline CUDA
compilation is successful. No GPU execution or acceleration claim follows.
Host full tracing has one timing pair with its cost scope stated; candidate
timing is separate. Physical proton preparation/scattering, spatial-phase
correspondence, measured targets and uncertainty remain missing premises.
