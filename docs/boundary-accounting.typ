// Application accounting companion consuming the bound Main and Manual.
#set document(title: "AOD D.E.C.: Boundary, Window and Scale Accounting", author: "AOD Dec project")
#set page(paper: "a4", margin: (x: 22mm, y: 20mm),
  header: text(size: 8pt, fill: rgb("536471"))[AOD D.E.C. #h(1fr) Boundary, window and scale],
  footer: context align(right, text(size: 8pt, counter(page).display())))
#set text(font: "Libertinus Serif", size: 10.5pt, lang: "en")
#set par(justify: true, leading: .56em)
#set heading(numbering: "1.1")
#show heading.where(level: 1): set text(size: 16pt, fill: rgb("173d50"))
#show raw: set text(font: "DejaVu Sans Mono", size: 8pt)
#set table(inset: 5pt, stroke: .4pt + rgb("cbd5df"))

#text(size: 26pt, weight: "bold", fill: rgb("173d50"))[Boundary accounting]
#v(4pt)
#text(size: 13pt)[Retained field dynamics across windows and enclosing scales]
#v(9pt)

*Setup.* A boundary-scoped computation must retain the complete source of each
contribution and account for its disposition when the window, region, coupling
or enclosing scale changes. An outer representation preserves its inner
construction. Its inner and outer values are alternative representations unless
an explicit disjoint additive decomposition says otherwise.

Main is bound to base revision `8c3149b9d3d9` and its actual R04 candidate bytes.
The accompanying source manifests identify every consumed file. Main owns the
definitions; this companion owns the application bookkeeping and exact examples.

= Complete accounting state <accounting-state>

#table(columns: (1fr, 2.55fr),
  table.header([*Record*], [*Required distinction*]),
  [Retained construction], [Immutable event identity, lineage, admitted prefix,
    scope, scale, parent support and full witness. A digest binds a supplied
    witness; it does not construct one.],
  [Present and operative state], [Present pressure/current and committed crown;
    retained operative relations; active, inactive, pending, routed and
    structurally removed dispositions remain distinct.],
  [Boundary coverage], [Explicit crossing and shedding families, region before
    and after, sweep operand, cut, scale, count domain and participation window.
    Empty families and stationary zero are explicit.],
  [Route state], [Positive surplus, exact route partition, destination or
    declared open loss, pending leg and strictly later receipt. One physical
    surplus occurrence cannot be routed again through an alias.],
)

At each crown the predecessor, present crown and admitted successor horizon
belong to the same retained construction. Main `eq:hinge-triad` applies it again
at each admitted enclosing scale. Octal addresses identify retained occurrences;
the operative scope and depth are witnessed by their actual effect on later
continuation. Address length alone does not supply that effect.

= Boundary and window identities <accounting-identities>

The component preserves oriented channel values separately, including cancelling
channels. On the declared boundary its signed result is

$ Phi_"mov" = Phi_"cross" - Phi_"sweep" + Phi_"shedding", quad
  J(omega) = sum_(t in omega) Phi_"mov"(t). $

Every slice binds the archive and its coverage declaration. Accumulation replays
the slice calculation, checks continuous region declarations and rejects reused
projection occurrences. A later projection may read the same native history,
but it is recorded as a separate per-cut calculation, not a new certified return.

RD and directional RCD remain retained. The compatible scalar participation is
$rho_omega = min(rho, abs(omega))$.
TTL uses its own declared retention domain. It is not inferred from excluded
participation. Pressure magnitude, signed current, capacity, closure residual
and nonnegative surplus retain their separate value types. A positive surplus
does not become signed boundary flux without a declared projection bridge.

#pagebreak()
= Exact computational examples <accounting-examples>

*Moving boundary.* Manual `manual:boundary-current:worked-substitution` supplies
crossing 8, sweep 3 and signed shedding 2. The calculation is $8-3+2=7$.
Omitting sweep gives 10; counting shedding twice gives 9. Both change the
declared result. The application requires the motion and complete contribution
families as inputs rather than replacing missing terms by zero.

*Native channels and a later response.* Four admitted recurring waves supply
two directional RCD histories of duration 5 and 7. Their capacities are 3 and 2,
with signed attention $1/3$ and $-1/2$. The existing Manual response bridge
consumes the checked boundary current at pressure one, at the next committed cut.
The history interval remains `(0,22]` throughout this comparison.
Window and availability schedules are explicit inputs to this response model.

#table(columns: (1fr, 1fr, 1.2fr, 1.2fr, 1.2fr),
  table.header([*Input cut*], [*Window*], [*CD coupling*], [*Slice current*], [*Later current*]),
  [22], [4], [active], [0], [0],
  [23], [4], [inactive], [4], [4],
  [24], [4], [active], [0], [4],
  [25], [6], [active], [-1], [3],
)

The window current is 3. Both full histories survive cancellation, dropout and
recoupling. The fixed TTL operands 3 and 2 are a separate comparison; this
example does not evolve a TTL countdown. Availability is an explicit coupling
input. An observation projection alone does not rewrite the present state.

*Retained carriage and operative span.* The unchanged Manual awareness example
constructs actual B1 outer returns from B0, retains a new higher relation and
checks its later effect before a B2 fold offer. At both hops, structural removal
blocks the corresponding offer and exact restoration recovers it. Its separate
availability schedule `1,2,3,0,3` gives later currents `1,3,6,6,9`.
During dropout the active retained input is empty; prior current 6 persists.
The matched lane with the operative relations removed at its starting cut
remains zero. Validation through the immutable archive cannot recover a removed
operative value into the transition.

= Implementation contract and coverage <accounting-contract>

The accounting component accepts exact integer/rational operands. The native
adapter validates full wave/contact witnesses and binds its coverage from all
declared channels, placements and availability. Cross-scale value aggregation
requires an explicit map and retained parent support. Native fold/contact
admission remains with the witness producer; the accounting component does not
replace it. Route receipt acknowledges a later destination event; it does not
silently create a receiver response or a stock evolution law.

The application rejects lost or rewritten archive prefixes, missing coverage,
incompatible domains, overlapping inner/outer amounts, duplicated physical
routes, premature/replayed receipts and altered slice arithmetic. The evidence
records distinguish source replay, component tests and native integration. No
new microscopic probability law or globally sufficient Markov quotient is
introduced by this accounting contract.

*Source anchors.* Main `eq:moving-boundary`, `eq:window-current`,
`eq:window-clipped-rho`, `eq:shedding-route-partition`,
`eq:awareness-state-factorization`, `eq:awareness-active` and
`eq:awareness-carriage`; Manual boundary-current applications and
`aod_awareness.py` / `manual_dec_fixture.py`. The accompanying Markdown report
links the exact implementation, full source witnesses and execution results.
