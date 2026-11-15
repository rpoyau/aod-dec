#set document(title: "AOD D.E.C.: Reduced SPARC response", author: "AOD Dec project")
#set page(paper: "a4", margin: (x: 22mm, y: 20mm),
  header: text(size: 8pt, fill: rgb("536471"))[AOD D.E.C. #h(1fr) Radial response and moving boundaries],
  footer: context align(right, text(size: 8pt, counter(page).display())))
#set text(font: "Libertinus Serif", size: 10.5pt, lang: "en")
#set par(justify: true, leading: .52em)
#show heading.where(level: 1): set text(size: 16pt, fill: rgb("173d50"))
#show raw: set text(font: "DejaVu Sans Mono", size: 8pt)
#set table(inset: 5pt, stroke: .4pt + rgb("cbd5df"))
#show table.cell: set par(justify: false)
#let result = json(sys.inputs.at("results", default: "../evidence/sparc-reduced/summary.json"))
#let model = "sparc-radial-native-response-v1"
#let source = "ab8359209b8f9b2ef4007403101016e3ea58cfe4"
#assert(result.format == model, message: "Unexpected reduced model result")
#assert(result.main_manual_commit == source, message: "Main/Manual source binding differs")
#let primary = result.summaries.spacing
#let chart = sys.inputs.at("chart", default: none)
#let galaxy-count = primary.galaxies.len()
#let galaxy-noun = if galaxy-count == 1 { "galaxy" } else { "galaxies" }

#text(size: 25pt, weight: "bold", fill: rgb("173d50"))[Radial AOD response]
#v(5pt)
#text(size: 12pt)[A declared SPARC specialization with moving control boundaries]
#v(8pt)

*Setup.* The ordered radial bins form a finite adjacent-cell graph. Each cell
carries one certified recurring Duon representative. The application consumes
Main's pressure, count comparison and inverse-pressure response, with an
explicit input adapter and fixed external square-speed map. Its representatives
are a computational preparation; their number and histories are not inferred
stellar particle counts or measured galaxy formations. Radial G0/G1 support
permits this reduced calculation without a complete three-dimensional input.

= Input and committed histories <reduced-preparation>

From each radius and the supplied gas, disk and bulge component proxies, set

$ D_i^0 = r_i (abs(V_i^"gas") + abs(V_i^"disk") + abs(V_i^"bulge")), quad
D_"ref" = max_i D_i^0, quad theta_i (11) = D_i^0 / D_"ref". $

The normalization cancels the physical units of the proxy. An all-zero family
uses normalization one and retains zero state. Observed speeds and their errors
are held separately; they cannot enter this preparation or its evolution.

Each lineage retains a Monon component at cuts 0, 1, 2 and three certified Duon
returns on the path $(0,1,0)$, completing at cuts 5, 8, 11. Their common
component witness, scope and distinct occurrence identities remain available.
The separation of completed returns calculates cadence 3. Subsequent cuts
commit one path event at a time: cuts 12 and 13 retain partial prefixes; cut 14
completes and certifies the next return. Later completions follow the same
declared construction. There is no injected jitter or future closure record.

Adjacent representatives supply two distinct contacts at the latest completed
return cuts. Initially these are 8 and 11, with native window $(8,11]$.
Directional reflection counts come from disjoint completed-return intervals;
they equal one here. The contact support accessor is 3, so its RCD duration is
$3+1+1=5$. The explicit map of one native cut to one participation slot gives
$min(5,3)=3$. Counting the three support positions gives channel pressure
$p=3 times 3=9$. A self-return contributes pressure 9 as well.
These native counts do not identify an SI time or a physical mass.

= Declared response <reduced-response>

For adjacent radii, the primary directional factor is
$w_(i j)=(min_e (Delta r_e)) / (Delta r_(i j))$; thus $0 < w_(i j) <= 1$.
The bound count comparator has equal recurrence counts. Its separately
declared offsets are the prior committed values $b_i=theta_i (k)$, giving

$ phi_(i j) (k)=9 w_(i j) (theta_i (k)-theta_j (k)), quad P_i=9(1+d_i), $
$ theta_i (k+1)=theta_i (k)-1/P_i sum_(j in N_i) phi_(i j) (k). $

Here $N_i$ is the adjacent-cell family and $d_i$ its size. All responses consume
the same committed input cut. Cached contact evidence is reused only until a
new return completes; it is not relabelled as a new contact. The normalization,
offset binding, spacing factor and self-pressure are declared application
choices, not uniquely determined physical calibration laws.

#pagebreak()
= Stock and moving boundaries <reduced-boundaries>

*Calculation.* Define the response stock $q_i=P_i theta_i$. Each channel has
one identity and opposite endpoint incidences, so its contributions cancel in
$sum_i q_i$. The response coefficients are nonnegative and sum to one because
$sum_(j in N_i) 9w_(i j) < P_i$. Consequently the initial range of $theta$ is
preserved without clipping. This exact finite result concerns the declared
response stock, which remains distinct from physical mass, density and the
projected coefficient $D$.

The moving control region selects cells above the fixed initial
pressure-weighted mean

$ theta_"threshold" = (sum_i P_i theta_i (11)) / (sum_i P_i). $

Membership is evaluated before and after the already computed response. It
does not determine the response current. Let $Q_R (k)$ be the stock selected at
cut $k$. Crossing uses after-membership; sweep uses the shared pre-response
stock on entering and leaving cells. The audited identity is

$ Q_R (k+1)-Q_R (k) + Phi_"cross,after" - Phi_"sweep,before"
  + Phi_"shedding,after" = 0. $

The selected region, its complement, the whole graph and the fixed initial
region are audited against the same immutable channel occurrences and swept
field. Their partition identities retain internal contributions even where
the enclosing boundary sum vanishes. The trace retains signed crossings,
sweeps, channel operands, state prefixes and comparison evidence.

*Boundary scope.* Radial ports are closed in this reduction. Closing the
out-of-plane ports is an explicit slice ablation; actual exchange there remains
unknown. The fixed-support model supplies no separate shedding channel.
Its zero modeled shedding does not establish absent physical shedding,
decoupling or recoupling. Moving an observation boundary does not itself witness
any of those formation or routing events.

= Horizon, projection and controls <reduced-controls>

For $n$ cells and calculated cadence $c=3$, the response horizon is

$ H=c ceil((n-1)/c). $

This is the first recurrence completion after at least graph-diameter response
steps. No residual selects the stopping cut. The choice does not equate the
graph diameter or the native cut span to seconds or a physical octave range.
Physical radii retain their units and range; an octal address alone supplies
no physical length multiplier.

At the final cut $K=11+H$, project the evolved state through

$ D_i (K)=D_"ref" theta_i (K), quad
U_i (K)=U_i^"bar"+K_"ext" D_i (K), $
$ U_i^"bar"=(V_i^"gas")^2 + 1/2 (V_i^"disk")^2
  + 7/10 (V_i^"bulge")^2. $

The frozen external coefficient is
$K_"ext"=8 thin ("km/s") / "kpc"$. The preparation therefore reproduces
the original G0 projection exactly, including its squared-gas convention.
The evolved $D$ has units kpc km/s and is not relabelled as retained spatial
density or a native capacity count.

The primary case uses spacing factors. The response-off control retains the
same pressure, histories and horizon, with channels unavailable; every projected
value must equal G0 exactly. The unit-factor control sets all $w_(i j)=1$ to
test the declared geometry choice. Neither control is selected by its score.

#pagebreak()
= Recorded comparison <reduced-results>

The bound summary contains #primary.bins radial bins from
#galaxy-count #galaxy-noun. The score coordinate is $U=V^2$, with
equal-bin square-speed MAPE

$ E_U=100/n sum_i abs((U_i-U_i^"obs")/U_i^"obs"). $

The frozen G0 equal-bin score is
*#result.display.baseline_mape_percent%*. The comparison below is read from
the executed summary; no improvement is assumed.

#table(columns: (1.2fr, 1.4fr, 1.4fr, 1.4fr),
  table.header([*Case*], [*Equal-bin MAPE*], [*Equal-galaxy MAPE*], [*Change from G0*]),
  ..(("spacing", "Primary spacing"), ("disabled", "Response off"), ("unit", "Unit factor"))
    .map(pair => {
      let row = result.display.at(pair.at(0))
      ([#pair.at(1)], [#row.mape_percent%], [#row.equal_galaxy_mape_percent%],
        [#row.change_percentage_points pp])
    }).flatten())

Equal-galaxy MAPE first averages bins within each galaxy, then gives each
galaxy equal weight. A percentage-point change subtracts two percentage scores;
it is distinct from their fractional relative change. Exact rational scores and
per-bin signed residuals are retained outside this rounded display.

The primary run records #primary.channel_occurrences channel occurrences.
Selected membership changes at #primary.moving_boundary_cuts of its response cuts.
Its maximum boundary balance residual is #primary.boundary_balance_residual.
Its maximum whole-graph stock residual is #primary.global_stock_residual.
These identities check accounting inside the supplied family. They do not
bound unobserved external exchange or physical prediction error.

The unresolved error terms include radial and out-of-plane exchange, physical
current and clock calibration, missing formation and shedding, and discrepancy
in the reduction and external observation map. No zero uncertainty is assigned
to them. The calculation is a reduced radial response comparison; it does not
claim a complete G2/G3 galaxy evolution or independent physical prediction.

= Source binding and reproduction <reduced-sources>

Main/Manual is bound to #raw(source). Main owns the calculus; this downstream
application owns the finite preparation, operational choices and execution.
The principal source anchors are:

- `sections/06_wave_state_relational_time.tex`: `eq:cadence-ratio`,
  `eq:count-comparator-realization`, `eq:window-clipped-rho`.
- `sections/06_field.tex`: `eq:b-scoped-channel-pressure`,
  `eq:b-scoped-flux`, `eq:duonic-pressure`, `eq:causal-update`;
  the moving boundary/window current construction.
- `manual/sections/06_field_dynamics_applications.tex`: G0/G1 data-support
  classes and the frozen SPARC square-speed projection.

The summary binds input file hashes and implementation hashes. The compressed
trace retains native histories, per-cut operands and all three cases; reopening
and deterministic recomputation are separate from conservation checks.

```text
python3 scripts/simulate_sparc_reduced.py --workers 3 \
  --output build/sparc-reduced
typst compile --root . \
  --input results=../build/sparc-reduced/summary.json \
  docs/sparc-reduced.typ build/sparc-reduced/sparc-reduced.pdf
```

The optional digest cache changes execution cost only; native validation and
history identities remain unchanged. Its on/off equivalence has separate
tests. Source provenance and replay establish what ran, while the fixed
preparation and external map delimit what its comparison can establish.

#if chart != none [
  #pagebreak()
  = Radial square-speed comparison <reduced-chart>

  #figure(image(chart, width: 100%), caption: [Observed square-speed targets,
    the frozen G0 response-off control and the evolved primary spacing case.
    Each panel retains its galaxy's radial and square-speed units.])

  The plotted values come from the recorded per-bin comparison. Differences
  from the targets include both reduced-model and external-map discrepancy;
  the curves do not resolve the unknown physical boundary-exchange error.
]
