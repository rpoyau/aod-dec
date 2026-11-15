#set document(title: "AOD D.E.C.: Parallel boundary tracing", author: "AOD Dec project")
#set page(paper: "a4", margin: (x: 22mm, y: 20mm),
  header: text(size: 8pt, fill: rgb("536471"))[AOD D.E.C. #h(1fr) Boundary replay and native route evidence],
  footer: context align(right, text(size: 8pt, counter(page).display())))
#set text(font: "Libertinus Serif", size: 10.5pt, lang: "en")
#set par(justify: true, leading: .5em)
#show heading.where(level: 1): set text(size: 16pt, fill: rgb("173d50"))
#show raw: set text(font: "DejaVu Sans Mono", size: 8pt)
#set table(inset: 5pt, stroke: .4pt + rgb("cbd5df"))
#show table.cell: set par(justify: false)

#text(size: 25pt, weight: "bold", fill: rgb("173d50"))[Parallel boundary tracing]
#v(5pt)
#text(size: 12pt)[CPU schedules, CUDA incidence and one retained ledger]
#v(8pt)

*Setup.* Independent boundary views of one committed field history can be
replayed concurrently. Each view retains its scope, complete ordered motion
schedule, participation windows and immutable occurrence identities. The
coordinator joins those views without creating new returns, changing the
source archive or treating transport order as a physical clock.

= Computation and joined evidence <parallel-ledger>

The multicore unit is a complete named boundary schedule. A spawned process
reconstructs and validates its source ledger, frame sequence and exact current
audit. Splitting a temporal schedule would require its preceding membership
and pending obligations; these remain with the complete job. Native formation,
fold, coupling, shedding and receipt dependencies remain joined on the host.

The coordinator reads one bounded archive snapshot, binds the implementation
and source chain, and submits at most one outstanding job per worker. Each
returned job must match its input identity and digest. The merge checks every
source occurrence, physical identity, endpoint, orientation, cut, window,
snapshot, swept value and unresolved projection. Results are emitted in stable
boundary order. Missing jobs, changed sources and failed workers cannot acquire
complete coverage. The original trace remains the single authoritative archive.

#table(columns: (1fr, 2.6fr, 2.4fr),
  table.header([*Stage*], [*Parallel work*], [*Retained check*]),
  [CPU], [Independent complete boundary schedules], [Full frame history, source binding and exact joined rows],
  [CUDA], [Channel incidence and swept membership], [Bounded integer buffers and original occurrence ordering],
  [Host], [Exact rational assembly and canonical audit], [Unchanged source identities, pending routes and witness dependencies],
)

= Device operation and exact current <parallel-device>

For channel $e=(u,v)$ and after-membership indicator $chi_R$, CUDA evaluates
$ sigma_e = chi_R(u)-chi_R(v) in {-1,0,1}. $
It classifies selected, internal, exterior and unavailable occurrences and
keeps crossing, shedding and pending-projection categories distinct. For each
changed cell it evaluates the signed membership difference
$ s_c = chi_(R_"after")(c)-chi_(R_"before")(c). $

The host consumes these coefficients with the original exact rational current
and swept-field operands. No current is rounded to a GPU floating-point value.
Its complete output is checked against the existing canonical audit:
$ J_(partial R) = J_"cross,after" - J_"sweep" + J_"shedding,after". $
Unavailable signed channels and unsigned pending burden retain their own
records. A zero signed sum does not resolve an absent projection. The kernel
does not decide a native motif, invent a chiral transition or certify a route.

The active-field scheduler still has one pending event per field. Its
$O(n log n)$ work for one event at each of $n$ active fields is separate from
these complete replay queries, their channel counts and host admission cost.
Profiling separates validation/packing, aggregate classification, exact
assembly and provenance hashing. Classification includes device probing,
compilation, transfers, execution and cleanup; their individual durations are
not separated. Enclosing replay timing also includes canonical revalidation.
Device use alone establishes no speedup.

#pagebreak()
= A declared Sun–Mercury boundary fixture <parallel-solar>

The fixture uses Sun and Mercury as boundary roles in a finite native
preparation. Actual recurring Duons carry the retained Monon pivots and
certified branch/hinge/counterbranch returns. The prescribed Mercury-role
whole-return orientations include two flips; earlier returns remain immutable.
The trace records those changes without supplying a transition probability.

Real counted contact witnesses give directional RCD duration 5. The declared
return-count participation window is 4, capacity is 3, and signed attention
has magnitude $1/3$. Thus the bound native pressure and main current are
$ p=3 min(4,5)=12, quad Phi=12 (plus.minus 1/3)=plus.minus 4. $
Membership then includes and excludes the Mercury-role cell under a declared
swept operand. The selected and complementary boundary currents are opposite;
the enclosing view cancels their internal transfers.

A later independently certified enclosing return supplies the fold. The
Manual's declared closure-load fixture has load 27 and cost 24, leaving
unsigned surplus 3. The admitted later E2 Duon route retains the Sun-role
terminal return as its pivot. Its separate contact/attention preparation uses
pressure 12 and attention $1/6$, giving signed current 2. These values have
different quantity domains: the implementation retains an unresolved
surplus-to-current obligation instead of identifying burden 3 with current 2.

For this declared schedule the selected current sequence is
$ (4,-3,0,7,6). $
The complementary sequence is its negative and all enclosing currents are
zero. These are exact native-current checks on the supplied finite operands.
The `declared-current` query retains all supplied signed channels and exposes
pending projection identities. A `full` query remains incomplete where that
unsigned burden crosses the requested boundary.

Reopened evidence must recover native returns, recurrence, contacts, the fold,
the declared shedding record, the later route pivot and the whole-return flip
markers. Names and positive surplus alone are not detection evidence. The
solar fixture adapter validates its declared load bridge separately from the
older proton-specific bridge in the general relation detector.
The inspector publishes the reconstructed committed channel value. A lawful
alternative cadence with comparison lag zero therefore reports zero, while
unsigned surplus remains three.

= Solar observable boundary and reproduction <parallel-observable>

The Manual declares a solar field boundary and a Mercury perihelion window,
with an observable signature consuming native attention and boundary current.
This fixture exercises that native boundary interface. An orbital trajectory
and an evaluated perihelion observable map are separate application operands.
The carried O0 comparison and its theory benchmark never enter these currents
or flip preparations. No SI conversion is required for this trace.

```sh
python3 scripts/check_parallel_tracing.py --workers 4 \
  --gpu required --output build/parallel-device
python3 scripts/trace_simulation.py boundaries build/parallel-device/trace \
  --workers 4 --coverage-scope declared-current
python3 scripts/trace_simulation.py boundaries build/parallel-device/trace \
  --backend gpu --coverage-scope declared-current
```

*Source binding.* Main/Manual commit
`ab8359209b8f9b2ef4007403101016e3ea58cfe4`:
Main `sections/05_curl_closure_duon_current.tex`, handed branch exchange;
`sections/06_field.tex`, nested SADAR and moving-boundary current.
Manual `04_boundary_current_applications.tex`, clipped pressure/current;
`00_dec_ledger.tex`, closure load 27 and cost 24;
`05_solar_system_field_tests.tex`, solar/perihelion boundary and observable
signature. Full paths, hashes, inspected claims and execution evidence are
retained under `evidence/parallel-tracing/`.
