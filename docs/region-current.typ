#set document(title: "AOD D.E.C.: Reusable Region Current", author: "AOD Dec project")
#set page(paper: "a4", margin: (x: 22mm, y: 20mm),
  header: text(size: 8pt, fill: rgb("536471"))[AOD D.E.C. #h(1fr) Regions, shells and current],
  footer: context align(right, text(size: 8pt, counter(page).display())))
#set text(font: "Libertinus Serif", size: 10.5pt, lang: "en")
#set par(justify: true, leading: .55em)
#show heading.where(level: 1): set text(size: 17pt, fill: rgb("173d50"))
#show raw: set text(font: "DejaVu Sans Mono", size: 8pt)
#set table(inset: 6pt, stroke: .4pt + rgb("cbd5df"))
#show table.cell: set par(justify: false)

#text(size: 26pt, weight: "bold", fill: rgb("173d50"))[One current, many boundaries]
#v(5pt)
#text(size: 13pt)[Reusable region and shell accounting]
#v(10pt)

*Source and setup.* Main defines the field-fluid regime, nested inter-layer
coupling and finite fractal-cycle accumulation on declared boundaries and
windows. Manual applies the same scoped flux to wire, water-interface and
plasma-shell cases. This implementation consumes those definitions in a finite
cell preparation. It changes the selected region without changing the identity
of the current occurrence.

= From occurrence to boundary <region-incidence>

Declare one finite cell universe, source-bound channel occurrences, a current
unit, a scope and a participation-window domain. Each committed occurrence has
an identity, two endpoints, a signed value, a channel class and availability.
The region is a subset of those cells. With membership indicator $1_R$, its
oriented contribution is

$ j_e (R) = phi_e (1_R (s_e) - 1_R (t_e)). $

A channel with both endpoints inside is internal. Its boundary value is zero;
its full record stays retained. A crossing contributes once with the appropriate
sign. The reverse view is the sign reversal of this same occurrence, not an
independently reconstructed reverse channel with different directional RCD.

For the same history and compatible scope, cut, unit and scale,

$ J(A union B) = J(A) + J(B) - J(A inter B). $

For an inner region $A subset.eq B$, the shell obeys
$J(B without A)=J(B)-J(A)$. The region adapter derives this membership and its
complete crossing list from the committed input family. The producer remains
responsible for supplying that family; a digest alone cannot establish physical
completeness outside it.

= Motion, history and representation <moving-region>

The operational current is

$ Phi_"moving" = Phi_"crossing" - Phi_"sweep" + Phi_"shedding". $

Crossings use the declared after-cut membership. Entering and leaving cells
must have swept-current values in one shared committed field. Membership sets
their sign. A changed boundary with a missing sample is rejected. A stationary
boundary declares unchanged membership and zero sweep explicitly.

The adapter retains all committed channel events, including unavailable ones.
Coupling dropout changes active participation; it does not erase history.
Unresolved burden-to-current projections remain obligations when a later
boundary exposes them. A requested full declared-current view cannot silently
replace such an obligation by zero.

Spatial nesting is distinct from native recurrence scale. Explicit positive
rational representation maps may change the reporting scale while preserving
the current quantity domain. Higher native formation still requires its own
ancestor-support admission; assigning a layer number does not construct it.

= Executable interface <engine-interface>

The reusable host API supplies `Ledger`, `Channel`, `Region`, `Frame`,
`commit`, `audit_boundary` and `audit_partition`, plus region union and shell
difference. It calls the existing strict boundary-accounting component.
The two applications use this same API. Run:

```text
python simulations/run_regions.py --example all --output results.json
```

The D.E.C. source `examples/region_current.dec` separately executes exact
clipping, oriented membership, sweep and moving-current arithmetic. Reference
VM, LLVM ORC and native executable checks accompany it. Full collections,
history and coverage validation remain on the host.

#pagebreak()
#set text(size: 10pt)
#set par(leading: .5em)
#text(size: 25pt, weight: "bold", fill: rgb("173d50"))[Maser and water shell]
#v(5pt)
#text(size: 13pt)[Two declared preparations using one engine]
#v(10pt)

*Common values.* The Manual D0 substitution declares capacity 3, directional
duration 5, window 4 and attention $1/3$. It gives participation 4, burden 12
and channel current 4. These applications use the bound native flux helper
and declare their channel multiplicities, placement and gate schedule. The
following values are exact native currents.

= Maser apparatus <maser>

The named regions are emitter, cavity, output port and exterior. Pump enters
the emitter; emitter current enters the cavity; the cavity feeds the output
port and a return/leakage channel. The port carries the declared detected-output
current to the exterior.

#table(columns: (1.4fr, 1fr, 1fr, 1fr, 1fr),
  table.header([*Preparation*], [*Pump in*], [*Output out*], [*Return out*], [*Net apparatus*]),
  [Coupled, window 4], [8], [4], [4], [0],
  [Gate off], [0], [0], [0], [0],
  [Recoupled], [8], [4], [4], [0],
  [Window 6], [10], [5], [5], [0],
)

Zero net apparatus current coexists with nonzero internal and exterior channel
records. All 20 input occurrences remain retained, including the five inactive
gate-off records. Summing nested cavity and whole-apparatus views as disjoint
transfers would count shared content twice.

The Manual also supplies emitted/arrived/pending packet accounting and a
separate phase/receiver interface. Packet count, native current and phase turns
retain their own value domains. This finite preparation does not choose the
microscopic emission law or identify pump current with hyperfine frequency.

= Earth, water shell and Moon <earth-water-moon>

Earth core, water cells, a surface cell, Moon and exterior are declared cells.
The shell is obtained by subtracting the core from the encompassing body region.
Core-to-water and water-to-Moon channels become internal when both endpoints
are included in the selected enclosing region.

#table(columns: (1.5fr, 1fr, 1.2fr, 1fr, 1fr),
  table.header([*Preparation*], [*Water shell*], [*Earth + water + Moon*], [*Exterior*], [*Whole*]),
  [Stationary], [10], [2], [-2], [0],
  [Shell expands], [7], [-1], [1], [0],
  [Moon gate off], [-2], [2], [-2], [0],
  [Contracts, recouples], [13], [5], [-5], [0],
)

At expansion the water boundary has crossing 8, swept current 3 and signed
shedding 2, giving $8-3+2=7$. The complementary boundary uses the same
surface sample with opposite membership sign. No new external source is
created by moving the boundary. At contraction the sweep sign reverses.

These are finite prepared current and interface calculations. Earth/Moon
placement is declared; a tide height, orbit or water velocity is not inferred
from the names. Main/Manual supplies the common calculus and external-map
interfaces; a physical comparison must bind its preparation and observable.

*Source record.* Exact Main/Manual anchors and reviewed source bytes are in
`review/sources.md` and `review/source-hashes.json`. The application results,
compiler checks and independent review retain their separate evidence.
