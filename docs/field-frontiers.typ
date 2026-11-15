#set document(title: "AOD D.E.C.: Field frontiers", author: "AOD Dec project")
#set page(paper: "a4", margin: (x: 22mm, y: 20mm),
  header: text(size: 8pt, fill: rgb("536471"))[AOD D.E.C. #h(1fr) Active boundaries and archived interiors],
  footer: context align(right, text(size: 8pt, counter(page).display())))
#set text(font: "Libertinus Serif", size: 10.5pt, lang: "en")
#set par(justify: true, leading: .52em)
#show heading.where(level: 1): set text(size: 16pt, fill: rgb("173d50"))
#show raw: set text(font: "DejaVu Sans Mono", size: 8pt)
#set table(inset: 5pt, stroke: .4pt + rgb("cbd5df"))
#show table.cell: set par(justify: false)

#text(size: 25pt, weight: "bold", fill: rgb("173d50"))[Field boundaries as active state]
#v(5pt)
#text(size: 12pt)[Window-scoped compression, recovery and scheduling]
#v(8pt)

*Setup.* Let $n$ count active field boundaries. Retained interior history has
its own size and archive cost. During a declared window, an active boundary
record carries the operands of its response; interior witnesses remain
recoverable without participating in every active step. This is an operational
specialization of Main's cycle-valued field compression and the Manual's
boundary/window clustering.

= Closure, compression and exposure <frontier-compression>

A traced enclosing return, with its scope map and certificate, admits the
enclosing Monon $C_f [Sigma]$. The internal support expression and identities
remain carried provenance. A further distinct compatible enclosing return
provides an outer beat. Folding and exposure retain separate obligations:

$ "Fold" arrow.t : {C_i^"ret"}_i arrow.r K^"prop"
  arrow.r "outer return + certificate" arrow.r C_f [Sigma]. $
$ "Expose" arrow.b : C_f [Sigma]
  arrow.r {C_i^"ret", "history", "certificates"}_i. $

Exposure recovers the retained inner identities, orientations, histories and
certificates, plus wave cadence, current and memory where present. A region
view can also compact the crossing sum of an existing field without claiming
a new enclosing closure. Internal cancellation removes terms from the active
sum while their source occurrences remain recoverable.

The live summary binds field identity, admitted support, scope, window, native
cut and normalized weights. It retains current pressure and orientation,
coupling bindings and input domain, pending obligations, and the archive
references that justify them. Boundary movement additionally needs the swept
field at the newly exposed frontier. A changed window, contact family,
retention condition or route requires recovered interior evidence before a
response consumes an operand absent from the summary. Domain changes are explicit.

= Distinct tracking roles <frontier-types>

#table(columns: (1fr, 3.6fr),
  table.header([*Object*], [*Bound evidence and status*]),
  [Monon], [Certified retained-start return; pending prefix and later recurrence are separate.],
  [Dion / Duon], [Dion is an oriented branch pattern. Duon binds two Dion patterns and a carried Monon pivot. Duad is its seated state.],
  [Trion], [Threefold core, `3:1`. Track transient compatibility, retention and route status from the declared curl trace.],
  [Tetron], [Four-seat enclosure $E_4$. Tetrion names a bare four-cycle; tetratrion is `3:4`; tetrad is seated Tetron.],
  [Wave / field], [Distinct compatible reclosure admits a wave; a field resolves unique recurrent member identities.],
)

These dependencies form a graph. A new label or a four-edge path does not
supply a missing construction or seat witness. The current native producer
admits Duon and the inherited `1:2` / `3:j` construction families; it does not
provide a dedicated Tetron/E4 admission interface. Its outer-enclosure grammar
parameter must not be lowered to infer that interface. Transient Trion status
does not introduce a decay probability or a universal lifetime.

*Base unit.* The first finite address is `00` in octal notation. This
application records its native support normalization as weight 1. A first
certified closure contributes one closure-cycle and zero reclosure beats;
each later distinct compatible reclosure adds one beat. Address, weight,
closure count and cadence remain distinct. The frontier records this weight
as metadata; its response uses separately supplied pressure. Applying a weight
to pressure requires a declared map. Octal digits supply no fixed SI multiplier.

#pagebreak()
= Exact local response and shared boundary context <frontier-response>

For the declared native inverse-pressure model, keep exact rational pressure
$P>0$, current orientation $theta$ and the committed input $Delta J$:

$ theta(k+1)=theta(k)+(Delta J(k))/P, quad q=P theta. $

The active record retains precisely the admitted identity and operands needed
by this update. Archived inner-wave history remains bound to that identity.
An orientation response preserves the existing closure and beat counts;
a new return requires new evidence.
Reuse of this local response does not assert that every physical interior has
the same sufficient summary. The Manual's falsifier applies: two histories
with the same proposed summary but different next boundary responses require
another retained operand or reopening before that response.

The shared boundary context computes one validated response and commits one
all-cell pre-response swept field. Its selected, complementary and enclosing
views then use that same occurrence family. With after-membership crossings
and pre-response swept stock,

$ Delta Q_R + Phi_"cross,after" - Phi_"sweep,before"
  + Phi_"shedding,after" = 0. $

The host interfaces are `boundary_context`, `context.response()` and
`boundary_view`. The context binds the declared scope, native participation
window and fixed representation scale. Returned views are independent copies;
membership does not mutate the response. Existing full occurrence-family
audits still run for each view. Their cost is separate from event scheduling.

= Scheduling cost and validation boundary <frontier-cost>

An indexed next-event heap can retain one pending event per active field.
Removing the next event and updating that field's entry takes $O(log n)$ heap
work. For $m$ events the heap work is $O(m log n)$; for one event at each of
$n$ fields it is $O(n log n)$. This bound assumes bounded local response work,
bounded coupling degree and bounded operand size. It does not count archive
loading, source admission, expanded history output, arbitrary-precision growth,
or a separate scan of all channels for a boundary audit. A measured speedup
requires those costs to be reported for the actual execution.

The current `FieldFrontier` archives a canonically validated native `Wave`
once. Cold admission checks retained-history consistency in a disk SQLite
index rebuilt from canonical witness files when the archive opens. The response
loop reads no interior history. Its `advance` changes only current and committed
cut; closure and beat counts stay fixed. Explicit `rebind` exposes the archive
on window or coupling changes, preserves the hot state and counts, and
invalidates old attention through a new generation. It infers no physical
coupling and resolves no pending obligation or scale formation. New closures
still need canonical admission; an appended-delta certifier is not implemented.
Its `window` bounds host response validity on $[u,v)$; native retained-return/RCD
counting uses $(u,v]$. `rebind` changes validity without recomputing RCD or
assigning physical time.
`expose` validates and returns the historical archived wave. Recording later
response events in `TraceStore` is the caller's responsibility. Missing or
truncated evidence never becomes verified absence.

*Checks.* Compare compact and full execution on fixed and moving windows,
exposure, changed coupling and recoupling. Include equal present boundary sums
whose interiors differ in later routes or expiry. Check replay without added
beats, archive corruption, partial prefixes, and boundary partitions with
internal cancellation. These are separate obligations from agreement on one
present scalar or a successful hash lookup.

= Source binding <frontier-sources>

Main/Manual: `ab8359209b8f9b2ef4007403101016e3ea58cfe4`.
Main `sections/05_curl_closure_duon_current.tex`:
`eq:cycle-valued-field-compression`, `eq:fold-outer-closure`,
`eq:expose-inner-lineage`, `eq:field-organization`.
Main `sections/06_field.tex`: `eq:b-scoped-stokes`, `eq:moving-boundary`,
`eq:duonic-pressure`, `eq:causal-update`.
Main `sections/03_afc_basis.tex`: `eq:first-anchor`;
`sections/04_cut_running_fractal_tesseract.tex`: `eq:closure-phase`,
`eq:beat-reclosure`; Appendices B and E retain the named-type and index
distinctions. Manual `manual/sections/06_field_dynamics_applications.tex`,
opening clustering paragraphs, supplies the reduction and sufficiency test.
This companion specifies the finite execution boundary and makes no general
physical closure theorem.
