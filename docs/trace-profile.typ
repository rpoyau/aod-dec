#set document(title: "AOD D.E.C.: Trace, Scope and Replay", author: "AOD Dec project")
#set page(paper: "a4", margin: (x: 22mm, y: 20mm),
  header: text(size: 8pt, fill: rgb("536471"))[AOD D.E.C. #h(1fr) Trace, scope and replay],
  footer: context align(right, text(size: 8pt, counter(page).display())))
#set text(font: "Libertinus Serif", size: 10pt, lang: "en")
#set par(justify: true, leading: .45em)
#show heading.where(level: 1): set text(size: 16pt, fill: rgb("173d50"))
#show raw: set text(font: "DejaVu Sans Mono", size: 8pt)
#set table(inset: 5pt, stroke: .4pt + rgb("cbd5df"))
#show table.cell: set par(justify: false)

#text(size: 26pt, weight: "bold", fill: rgb("173d50"))[Retain the calculation]
#v(5pt)
#text(size: 13pt)[Committed events, scoped views and recoverable evidence]
#v(10pt)

*Setup.* A run declares its model, kernel, preparation, source, scope and exact
quantity domains. The execution commits events. Detectors read those events
under the bound return, route and boundary predicates. Observation maps and
performance measurements consume the resulting evidence in their own domains.
This follows the Manual's `manual:dec-ledger` execution order.

= One event, many views <trace-identity>

A versioned envelope binds event identity, causal parents, coordinate, scale,
scope, lineage, exact payload and source. Content-bound IDs and checksummed
storage preserve identity through replay. Identical re-ingestion creates no new
event; a conflicting identity fails.

Core hooks observe retained starts and transitions. The detector keeps alternative
branch/hinge/counterbranch parses until the committed span is validated against
the full raw path. Unconfirmed candidates leave phase counts unresolved; earlier
endpoints remain visible. Persistence, dwell and replay add no reclosure beat.

Support detection covers `1:2:x`, `3:j:k` and Duon construction. Duad names a
seated state of the same Duon. Core, window, seating, coupling and shedding retain
separate witnesses; an unavailable axis is not a negative observation.

An observation window is $[a,b)$ in a declared index domain. An empty view contains
no events. A closure crossing its left edge still consumes retained start memory;
an open return at its right edge is censored. Overlapping windows deduplicate
identities. Alternative projections of one occurrence family are non-additive.

= Address, duration and formation <trace-scope>

Main binds the fractal coordinate to retained octal address words. The origin
anchor is `00_8`; address, graph vertex, fibre zero and boundary scope are distinct.
A spatial or support span is separate from acquisition, comparison, participation
and retention windows. Event ordinals and host timestamps do not define an SI
clock or physical bip rate.

For compatible duration types, Main's `eq:window-clipped-rho` gives
$ rho_omega = min(rho, omega). $
Only duration participation is clipped. RD and directional RCD remain unchanged.
The coordinate context stays attached to both the original and clipped value.

A fusion rung consumes admitted inner compressions and an independently executed
outer closure. The source labels `eq:fusion-ladder-rung` and
`eq:lifted-support-admission` retain those obligations. A positive rational
reporting conversion is a representation map; it supplies no formation witness
or frequency-doubling rule.

= Boundary and future update <trace-reduction>

For a declared finite region $R$ and antisymmetric flux,
$ sum_(i in R) sum_j phi_(i j) = sum_(i in R, j in.not R) phi_(i j). $
Internal cancellation preserves each contributing record. A moving boundary uses
one committed sweep field:
$ J_"moving" = J_"crossing" - J_"sweep" + J_"shedding". $
Shell differences and disjoint partitions use that same occurrence family.
Unmapped burden remains an unresolved signed-current obligation.

Static equality does not itself advance a reduced model. For a declared Markov
quotient, blockwise transition sums must agree for every fine state in a block.
The finite demonstrator checks that condition and compares fine and reduced
trajectories. A counterexample with equal present boundary values and different
future responses tests the missing-state failure directly.

#pagebreak()
#set text(size: 9.5pt)
#set table(inset: 4pt)
#text(size: 25pt, weight: "bold", fill: rgb("173d50"))[Inspect the executed scope]
#v(5pt)
#text(size: 13pt)[Applications, resource limits and measured cost]
#v(10pt)

= Current applications <trace-applications>

The existing host applications execute the same declared preparations with passive
commit capture. Their exact results remain in native current/count domains.

#table(columns: (1.1fr, 2.3fr),
  table.header([*Application*], [*Evaluated current or retained obligation*]),
  [Maser apparatus], [Output and return currents: $4,0,4,5$. All 20 channel occurrences remain retained, including the disabled cut.],
  [Moving water shell], [Expansion: $8-3+2=7$. The shell sequence is $10,7,-2,13$; the enclosing universe remains zero.],
  [Proton pair], [Contact projections: $-6,-12,-18,-30$. Window alternatives are non-additive. The positive burden route and later receipt retain an unresolved signed-shedding map.],
  [Electron–wire], [Wire current: $4,0,4,5$. Later electron current: $4,4,8,13$. Opposing boundary totals cancel with histories intact.],
)

Native particle formation witnesses are stored as recoverable compressed bytes,
with bounded chunks and integrity locators. Automatic accounting replay revalidates
the actual route partition, pending destination and later receipt. Full host
admission and supported scalar native execution retain separate evidence.

*Application scope.* The maser and water preparations calculate no new hyperfine
frequency or tide height. A collision study supplies proton, cavity and sensor
populations; detection infers support from the resulting trace. Duration 9 alone
cannot identify `1:2:9`. This runner evolves its `1:2` source before mapping it
into `3:3` proton preparations. Evolution of those protons together with the
cavity and sensor remains incomplete. Historical $31/50$ is a comparator. The finite
orbital example evaluates its declared kernel; a physical Sun–Mercury trajectory
requires its own evolution and observation binding.

= Durable history and incomplete queries <trace-storage>

The store separates resident chunks and a configured cache from durable history.
It caps event bytes, exact-integer size, structural depth and archive growth.
SQLite's page-cache setting is a target, not a hard process-memory limit. These
store limits do not automatically bound the caller's model, detector or
operating-system RSS. The reported measurement names its scope.

A finer query reloads actual witness bytes within its retrieval cap or returns an
unavailable/incomplete result. A digest alone is not a witness. Damaged or missing
chunks, exhausted storage and interrupted execution cannot produce complete
coverage. Checkpoints preserve declared model and RNG state at commit boundaries.
Eviction, clipping and gate dropout are distinct from physical shedding.

= Execution and error records <trace-observation>

Profiles separate model work, detector and aggregation work, replay, trace I/O,
compilation, loading, warmup, steady execution and total elapsed cost where each
phase runs. Repeated paired comparisons include overhead. Device counters carry
an unavailable status when absent. Host timing is never used as an AOD SI anchor.

Observable records bind inputs, kernel, window, scale, evidence, unit and anchor.
Input, sampling, model, truncation, numerical, detection and anchor uncertainties
remain separate in the fresh region and particle current reports. Unknown
physical discrepancy is not zero; proton full current stays unresolved. Shared inputs
retain dependence or a joint interval treatment. Angular $tau$ denotes a full turn
and remains distinct from the named Tau candidate application.

#pagebreak()
#set text(size: 10pt)
#text(size: 25pt, weight: "bold", fill: rgb("173d50"))[Follow the executed path]
#v(5pt)
#text(size: 13pt)[Population, pending updates and branch-specific receipts]
#v(10pt)

= Replay the population history <trace-encounter>

The declared source initializes two `1:2` populations and executes six actual
updates. Initialization records the prepared state and history, source hashes and
requested update count. Each update retains its masks, exact tickets, selected
opportunities, before/after states and world digests. A terminal record binds the
completed count and final state. Replay reconstructs the same bound source model
and checks those operands and outcomes.

Two updates leave carrier A pending while carrier B gains a return. Both attempts
remain visible. Expired relations remain in history with an inactive disposition;
later contacts and new relations have their own identities. Missing initialization,
a skipped update or a missing terminal record prevents complete execution coverage.

= Join the sheddic evidence <trace-paths>

The retained path connects the source population and updates to the explicit
`3:3` C6 preparation mapping, coupling and outer fold, native closure burden,
positive surplus, accounting route, recurring Duon support, Duad seating and later
receipt. Each link is checked against the actual retained witnesses. Seating
preserves the Duon current; a receipt alone does not establish the Duad state.

A routing plan binds the actual input windows, capacity, closure cost, units,
source history and outer return before the alternatives execute. Replay checks
that plan against the bound producer configuration. Each of the four planned
windows requires its own terminal route/receipt ledger, bound to the window, cut
and native shedding record. Deleting a whole branch cannot create a complete
shorter experiment. If the terminal ledger records a receipt but its event is
absent, coverage is partial; missing capture cannot become physically pending.
Alternative windows remain non-additive views of the same occurrence family.

*Execution scope.* Prepared proton beams and cavity/sensor populations are valid
inputs. The existing law here evolves the `1:2` source and then materializes proton
preparations from its completed history. The trace verifies that executed model
and mapping. Full proton/cavity/sensor evolution is still an implementation task;
no expected support signature or Tau score is supplied to force detection.

= Executable entry points <trace-cli>

```text
python scripts/trace_simulation.py run --example particles \
  --trace build/particle-trace --output build/particles.json
python scripts/trace_simulation.py encounter build/particle-trace
python scripts/trace_simulation.py support build/particle-trace
python scripts/trace_simulation.py relations build/particle-trace
python scripts/trace_simulation.py paths build/particle-trace
python scripts/trace_simulation.py replay build/particle-trace
```

`encounter` checks initialization, every update and the terminal outcome. `paths`
joins the source, shedding, route, support, seating and receipt evidence. Full
`replay` includes those path checks. Use a fresh directory for each new run and
read each reported coverage field. The same interface exposes core and region
applications, the finite scope example and a profile campaign.

The proposed Manual III outline is kept in
`docs/MANUAL_III_OUTLINE.md`; no upstream manuscript is changed.
