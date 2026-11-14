# Independent particle-example integration check

The final scripts and saved outputs support their declared native accounting
claims. No additional material implementation defect was found in this bounded
review. The proton example correctly retains an unresolved signed-current
projection for its routed burden; it does not report a complete physical current
from that burden. The electron example supplies an explicit sequence of live
inputs and subsequent native state updates.

This was a read-only source and saved-witness review. It did not rerun either
application, the native simulation, the compiler or their tests. Exact arithmetic
was independently evaluated on the saved data. Main/Manual ownership and source
anchors are established in `sources.md` and `source-hashes.json`; the current
review's complete artifact binding is `examples-check-hashes.json`.

## Preparation, history and component accounting

`proton_accounting.py:109–159` constructs each new `3:3` target from three
distinct earlier Monon components and an actual C6 trace
`(0,1,3,7,6,4,0)`, with turn 3. It calls native motif and wave admission. The
seven/eight carrier returns retain the source-to-target certificate mappings,
chirality, earlier component traces and motif witnesses. This resolves the
older orientation helper's two-edge target limitation: enclosure metadata is
not being used as a substitute for actual six-edge target geometry here.

The preparation is explicitly projected from the existing `1:2` retained
encounter. The source model is not silently renamed a proton transition law.
The Manual's `p+→3:3:6` association is adopted separately. The return witnesses
do not establish an additional physical scattering law or cross-section.

Carrier A has seven target returns and 21 distinct formation components;
carrier B has eight targets and 24 components. Target-only traversal counts
are 42 and 48. The separate two-edge formation components contribute another
42 and 48 traversals. These are distinct accounting bases, not independent
quantities to sum as boundary flux. In particular, the retained outer fold and
its two inner carriers are provenance-linked representations; the boundary
slice selects the single contact-current projection, not all three supports.

`electron_wire_accounting.py:53–82` supplies three actual electron-associated
`1:2` C6 targets, using two earlier bound components, and four wire-representative
Duon/E2 targets using one retained Monon pivot. Those components are intentionally
reused by later motif witnesses rather than newly created on every return.
The saved targets contain 18 electron traversals and eight wire traversals;
their unique earlier components contribute four and two, respectively. The wire
report explicitly identifies E2 geometry despite enclosure-6 motif metadata.

## Contacts and directional duration

The proton contact commitments are 58 and 173. Actual disjoint return-ID lists
give reflections `(2,2)` and `(5,6)`. The forward directional duration is
`3+2+5=10`; the reverse diagnostic is `3+2+6=11`. The recurrence counts are six
and seven, giving forward lag −1. Contact support RD 3, these directional RCDs,
and the Manual proton motif RD 61 are correctly kept separate.

The electron-wire adapter declares a different reflection projection: completed
returns after the initial formation, through contacts 36 and 46. Each side
contributes one returned hinge at each contact, so `RCD=3+1+1=5`. The wire's
additional completed return at 52 contributes to the full comparison interval's
recurrence ratio `3/2` and lag `+1`; it is not retrospectively inserted into the
earlier contact at 46. Its full record remains in the history. This convention
is explicit and should not be conflated with the proton adapter's full-return
reflection convention.

Both adapters compute counts from retained occurrence witnesses rather than
accepting the native helper's illustrative default reflection integers. The
electron adapter also rejects substitution of those counts. Complete returns
are counted once; no additional factor of two is applied to their branch pairs.

## Window alternatives and live input time

The final proton rows are four matched alternatives at the same cut 177, each
with its own archive and the same fixed native history. They give:

| Window | Participation | Signed contact current | Closure burden | Burden minus cost 24 | Surplus |
|---:|---:|---:|---:|---:|---:|
| 2 | 2 | −6 | 6 | −18 | 0 |
| 4 | 4 | −12 | 12 | −12 | 0 |
| 6 | 6 | −18 | 18 | −6 | 0 |
| 12 | 10 | −30 | 30 | 6 | 6 |

The initial in-progress source/output had summed these projections to −66.
The final source removes that total, declares the alternatives non-additive,
and records a rejected attempt to accumulate them. No repeated response or
elapsed interval is inferred from changing the window.

For the electron-wire example the source explicitly supplies successive input
cuts 52–55, windows `(4,4,4,6)` and availability `(on,off,on,on)`. Each live cut
creates its own input occurrence and drives the next state at cuts 53–56.
Pressure participation is `(4,4,4,5)`, wire outward flux is `(4,0,4,5)`, and
the resulting sum is 13. The electron outward view sums to −13; the union
boundary is zero. With declared response pressure 1 and the named outward-sign
bridge, the electron current is `(4,4,8,13)` and wire current its negative.
This is a valid declared input timeline; it is not a sum of alternative views.

The closed coupling step produces no new response and retains the previous
current and all source history. Reopening exposes that retained history again.
The all-gates-off control has the identical native history and zero response.
The script correctly labels this as a coupling-gate control, not a
retention-only intervention establishing a broader awareness claim.

## Burden, signed current, routing and causal scope

For every proton row the contact-only accounting slice has an explicitly
stationary boundary and zero sweep. The same AB channel's A/B/union Stokes
views are `−x,+x,0`; the window-12 values are `−30,+30,0`. The B view is an
orientation reversal of the same channel, not the independently recomputed
BA/RCD-11 diagnostic added as another outward contribution.

The window-12 surplus 6 is retained in closure-burden units. Native outer
folding and exposure preserve the inner carriers; an admitted recurring
Duon/E2 recipient is constructed after commitment 177, completes at 183, and
receives the leg at 184. The one-time receipt conserves the routed amount and
follows its support completion. Seating keeps its current at zero. Availability
TTL 10 and seat window 2 use native-cut units and are distinct from the
return-count participation window.

The final row preserves `SIGNED_SHEDDING_CURRENT_UNRESOLVED`, the physical
surplus digest, amount, unit, route and receipt status. It refuses a requested
full current until a bound burden-to-signed-current projection is supplied.
Thus zero shedding in the selected contact-only coverage is not presented as
zero physical shedding or completion of the full moving-boundary current.
This is an explicit remaining application obligation, not a numerical error
or evidence that the Manual lacks shedding or external maps.

Electron active closure burdens are `(12,0,12,15)`. Against cost 24 the
residuals are `(−12,−24,−12,−9)`, hence all surpluses are zero. Those negative
values are closure deficits, not failed accounting identities. The example
does not invent a route or higher-layer certificate in that case. Wire/electron
opposite views carry the same physical occurrence ID and cannot be counted as
two independent outward contributions in one region.

## Verification and interpretation

The execution records report byte-identical normal and optimized outputs:
eight proton hostile controls and 33 electron checks, including nine hostile
rejections. This review inspected those saved records rather than repeating
their executions. Its separate saved-data arithmetic check for the electron
performs 55 checks, including witness occurrence IDs, typed directional counts,
per-cut formulas, all scoped boundary identities, causal response cuts, totals
and preserved gate-off history. All inspected accounting and paired-response
residuals are exact zero. The proton saved-witness check independently reconstructs
all 15 C6 targets and 45 distinct components, their certificate/motif/source-map
bindings, contact partitions and exact window arithmetic. It imports no model
code. Its results are in `../reviews/proton-saved-witness-check.json`; the
electron results are in `electron-output-independent-check.json`.

These residuals are against the declared native calculation. Neither script
uses the stored rest-energy target to construct an interaction trajectory or
claims that native current is a length, force, SI wire current or mass estimate.
The electron's charge sign is retained independently of native chirality and
channel attention. The Manual maps and measured targets remain present with
their proper roles. No fractional residual is silently called a percentage,
and no percentage is assigned to a zero native conservation residual without
a defined denominator.

Coverage is these two scripts, their final saved histories/outputs and relevant
bound Main/Manual contracts. It is not a proof of all possible preparations,
all-scale emergent dynamics, a physical scattering/force fit, or cost-inclusive
compiler performance. No production/source repair or remote write was performed
by this review.
