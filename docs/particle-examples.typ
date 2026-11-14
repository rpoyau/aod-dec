#set document(title: "AOD D.E.C.: Proton Pair and Electron-Wire Examples", author: "AOD Dec project")
#set page(paper: "a4", margin: (x: 22mm, y: 20mm),
  header: text(size: 8pt, fill: rgb("536471"))[AOD D.E.C. #h(1fr) Particle boundary examples],
  footer: context align(right, text(size: 8pt, counter(page).display())))
#set text(font: "Libertinus Serif", size: 10.5pt, lang: "en")
#set par(justify: true, leading: .58em)
#show heading.where(level: 1): set text(size: 17pt, fill: rgb("173d50"))
#show raw: set text(font: "DejaVu Sans Mono", size: 8pt)
#set table(inset: 6pt, stroke: .4pt + rgb("cbd5df"))
#show table.cell: set par(justify: false)

#text(size: 26pt, weight: "bold", fill: rgb("173d50"))[Particle boundary examples]
#v(5pt)
#text(size: 13pt)[Proton pair: retained encounter, window and routing]
#v(10pt)

*Source and setup.* The Manual maps the proton to `3:3:6` and the electron to
`1:2:6`. These examples adopt those maps and declare their finite interaction
preparations. Main/Manual is bound to base revision `8c3149b9d3d9` and the actual
R04 candidate source bytes. Full hashes, witnesses, exact scripts and execution
records accompany this document.

The proton preparation consumes the unchanged retained-continuation example's
six-attempt history, including blocked support and re-entry. It constructs new
`3:3:6` carriers from the selected orientations. Every target return has the
actual C6 path `(0,1,3,7,6,4,0)`, a turnaround at its third edge and three
strictly earlier bound Monon components. The original encounter and the new
preparation map both remain retained; the carrier target closures number 7 and 8.

= Contact and window calculation <proton-window>

The proton motif's neutral-reflection support accessor is $"RD"=61$. The contact
channel has its own support accessor 3. Enumerated reflected returns give
directional RCD durations 10 and 11; they are not replaced by the motif accessor.
For the A-oriented channel, the actual comparison lag is $-1$. The example
declares capacity 3 and closure cost 24 before comparing the windows:

$ phi_A (w) = -3 min(10,w), quad
  P(w) = 3 min(10,w), quad X(w) = max(0,P(w)-24). $

#table(columns: (1fr, 1.25fr, 1fr, 1fr, 1fr),
  table.header([*Window*], [*Contact current*], [*Burden*], [*Residual*], [*Surplus*]),
  [2], [-6], [6], [-18], [0],
  [4], [-12], [12], [-12], [0],
  [6], [-18], [18], [-6], [0],
  [12], [-30], [30], [6], [6],
)

These rows are alternative views of the same committed history at cut 177.
They are not successive physical slices. Attempting to add them as a window
current is rejected. Within the single contact channel, A's outward value
$-30$ has reverse bookkeeping value $+30$ at B and enclosing-boundary value 0.
The independently reconstructed reverse-direction RCD remains a separate record.

= Retained surplus and receipt <proton-routing>

At window 12 the positive surplus 6 is recorded by the native shedding
interface. The example retains a recurring Duon/E2 route support, the outer
fold, surviving inner waves and the exact route provenance. The support is
complete at cut 183 and the receipt commits later at 184. Pending amount 6 is
therefore accounted through the later receipt without deletion or reuse.

The surplus is a burden quantity. No signed shedding-current projection is
supplied by this preparation, so it remains an explicit unresolved projection
obligation. A requested full-current total is rejected; the contact value is
not silently presented as the complete outgoing current. Receipt of the route
does not, by itself, supply that value map.

*Checks.* Both normal and optimized executions reproduce identical reports and
full witness payloads. Eight hostile controls reject, including duplicated
route consumption, same-cut receipt, alternative-window summation, missing
formation support, omitted crossing and an unresolved full-current request.

#pagebreak()
#text(size: 24pt, weight: "bold", fill: rgb("173d50"))[Electron above a wire]
#v(5pt)
#text(size: 13pt)[One coupling channel, three declared boundaries]
#v(10pt)

*Preparation.* A native `1:2:6` electron support has three actual six-edge C6
returns. A representative wire current is a recurring Duon/E2 lineage with
four elementary two-edge returns. The placement selects a wire-core cell and
an above-wire cell on opposite sides of the declared interface. This placement
does not assign a separation in metres. The wire's enclosure metadata remains
distinct from its actual E2 path geometry.

= Native contact and response <wire-response>

Actual contacts occur at cuts 36 and 46. Counting completed-return hinges
after initial formation gives reflection counts `(1,1)` on both sides. The
contact support is 3, directional RCD is 5, recurrence ratio is $3/2$ and lag
is 1. With capacity 3 and intensity $1/3$, the active window-4 pressure is 12
and the wire-outward current is 4.

The boundary-accounting result is the input to the existing next-cut current
response at pressure one. An outward value supplies the negative input to that
region. Window and coupling availability are explicit model inputs here.

#table(columns: (1fr, 1fr, 1.15fr, 1.1fr, 1.15fr, 1.25fr),
  table.header([*Input cut*], [*Window*], [*Coupled*], [*Wire outward*],
               [*Electron outward*], [*Later electron current*]),
  [52], [4], [yes], [4], [-4], [4],
  [53], [4], [no], [0], [0], [4],
  [54], [4], [yes], [4], [-4], [8],
  [55], [6], [yes], [5], [-5], [13],
)

Wire and electron outward-window totals are 13 and $-13$. The enclosing
boundary is exactly 0 because this channel is internal to the union. Its two
oriented views retain the same physical occurrence; adding both as independent
outward contributions is rejected. The paired later current coordinates are
$(-4,4)$, $(-4,4)$, $(-8,8)$ and $(-13,13)$.

During dropout, the active input is empty while both histories and accumulated
state survive. Recoupling reuses the retained relation. An all-gates-off control
keeps the prepared history and produces zero later current. The region is
stationary, so sweep is explicitly zero. Burdens 12, 0, 12 and 15 against cost
24 give deficits and no positive surplus; those deficits do not erase the
already certified supports.

= Outputs and comparison maps <particle-comparison>

All scoped Stokes and paired-response accounting residuals are exactly zero.
These are arithmetic audit residuals, not measured trajectory errors. The
electron-wire run passes 33 checks in each Python mode, including nine required
hostile rejections, with identical full outputs.

The adopted Manual electron charge map gives $Q_e=-e$, with
$e=1.602176634 times 10^(-19)$ C. The Manual rest-energy targets are
0.51099895069 MeV for the electron and 938.27208943 MeV for the proton.
Those comparison records are retained separately from the current calculation.
The present examples do not output a measured wire deflection or a proton
scattering cross-section. The Manual's distance, speed and energy maps remain
available when their required preparation and time/value inputs are supplied.

*Source anchors.* Manual `manual:rest:ao-field-value-table`,
`manual:rest:electron-external-map`, `manual:boundary-current:wire` and
`manual:boundary-current:worked-substitution`; Main `eq:moving-boundary`,
`eq:window-current`, `eq:window-clipped-rho`, `eq:shedding-route-partition`.
