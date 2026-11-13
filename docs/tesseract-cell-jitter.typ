// Computational application companion to language-and-compiler.typ.
#let data = json(sys.inputs.at("results", default: "../evidence/manual-alignment/results.json"))
#assert(data.status == "PASS", message: "The application requires a passing result record")
#assert(data.version >= 2, message: "Use the corrected per-output and Manual fixture record")
#let fmt(x, digits: 8) = str(calc.round(float(x), digits: digits))
#let label(x) = if x == "0" { [0] } else { raw(x) }
#let source-color = rgb("15364a")
#set document(title: "AOD Dec: Tesseract Cell Jitter", author: "AOD Dec project")
#set page(paper: "a4", margin: (x: 22mm, y: 20mm),
  header: text(size: 8pt, fill: rgb("536471"))[AOD Dec #h(1fr) Computational application],
  footer: context align(right, text(size: 8pt, counter(page).display())))
#set text(font: "Libertinus Serif", size: 10.5pt, lang: "en")
#set par(justify: true, leading: .62em)
#set heading(numbering: "1.1")
#show heading.where(level: 1): set text(size: 19pt, fill: source-color)
#show heading.where(level: 2): set text(size: 13pt, fill: source-color)
#show raw: set text(font: "DejaVu Sans Mono", size: 8pt)
#show raw.where(block: true): it => block(width: 100%, inset: 9pt,
  fill: rgb("f2f5f7"), radius: 3pt, it)
#set table(inset: 6pt, stroke: .4pt + rgb("cbd5df"))
#let tab(..args) = text(size: 9pt, table(..args))
#let note(body) = block(width: 100%, inset: 10pt, fill: rgb("edf5f6"),
  stroke: (left: 2pt + rgb("2c7181")), body)

#text(size: 28pt, weight: "bold", fill: source-color)[Tesseract cell jitter]
#v(3pt)
#text(size: 14pt)[D.E.C. histories, typed comparisons, and SI observation maps]
#v(8pt)
This application contains an oriented tesseract-cell transport control and
separate source-bound Manual fixtures. The transport control checks exact
rational updates against a continuous numerical reference. The D.E.C. fixture
retains admitted recurring waves and directed contacts before computing RCD,
duration, pressure, and boundary values. A further fixture maps declared
observed phase advances to SI coordinates through a reference realization.

#note[
*Calculation scope.* Declare support, ancestry, window, boundary, and update;
admit typed inputs; compute with retained histories; apply an observation map;
then join comparison evidence. Each output carries its own comparator and error
record. Cell mass, normalized AFC memory, wave population, RCD, and oscillator
phase are different quantities. The cell transport is not an admitted maser model.
]

= Explicit cells and spatial cuts <cells>

*Setup.* Fill the four-cube as a declared application complex. A cell is a word
of length four in `0`, `1`, and `*`. A star selects an interval factor; a fixed
bit selects its endpoint. The number of stars gives dimension. This construction
contains the Q4 vertex/edge chart and explicitly adds its higher cells.

#tab(columns: (1fr, 1fr, 2fr),
  table.header([*Dimension*], [*Count*], [*Cell example*]),
  [0], [16], [`0000` · vertex],
  [1], [32], [`*000` · edge],
  [2], [24], [`**00` · square],
  [3], [8], [`***0` · cube],
  [4], [1], [`****` · four-cell],
)

For ordered free axes $a_0 < dots < a_(k-1)$, use the oriented product boundary
$ partial c = sum_(r=0)^(k-1) (-1)^r (c_(a_r=1) - c_(a_r=0)). $ <eq:cell-boundary>
Its second boundary vanishes on every cell. The eight cubic boundary cells share
24 squares, with opposite induced orientations at each shared square.

A spatial cut is the oriented boundary of a selected chain of these cells.
The observation index records successive states of that same declared domain.
Jitter changes successor weights while cell identities and incidence stay fixed.
No geometric displacement or metric length is assumed in this first experiment.

#pagebreak()
= Auxiliary emitter and cell transport <transport>

*Setup.* The eight cubic facets are storage compartments. Each has six neighbors
through shared squares. Label the source facet `0***` and the output facet `1***`.
An upper-state reservoir $e$ feeds the source; $x_i$ is storage in facet $i$;
$o$, $ell$, and $p$ accumulate output, absorption, and supplied excitation.
All are nonnegative model-mass quantities. The pumped total need not be one;
these quantities are neither probabilities nor wave-population density.

The entire eight-cell shell is closed: its internal square boundary cancels.
The output port and absorption are explicitly added sink relations. They are
not inferred to be unshared faces of the closed shell. A subregion's square
boundary flux and its port/loss terms are retained separately.

#tab(columns: (2fr, 1fr, 3fr),
  table.header([*Parameter*], [*Value*], [*Meaning*]),
  [Initial $e$], [1], [Other storage and accumulators start at zero.],
  [Pump $q$], [$1/4$], [External input into the upper-state reservoir.],
  [Release $gamma$], [$1/4$], [Reservoir-to-source transfer coefficient.],
  [Exchange $k$], [$1/16$], [Base coefficient on each shared square.],
  [Output $kappa$], [$1/4$], [Leakage from the declared output facet.],
  [Absorption $alpha$], [$1/32$], [Loss from each facet.],
  [Evolution window], [4], [Four unit intervals in dimensionless parameter $theta$.],
)

*Declared dynamics.* On each interval, coefficients are held fixed:
$
  (d e)/(d theta) &= q - gamma e, \
  (d x_i)/(d theta) &= delta_(i,s) gamma e
    + sum_(j in N(i)) k_(i j) (x_j - x_i)
    - (alpha + delta_(i,r) kappa) x_i, \
  (d o)/(d theta) &= kappa x_r, quad
  (d ell)/(d theta) = alpha sum_i x_i, quad
  (d p)/(d theta) = q.
$ <eq:transport>
Here $s$ and $r$ denote source and output facets, and $N(i)$ is the neighbor set. Symmetric exchange means
$k_(i j)=k_(j i)$. Reservoir release is a supplied transition model; the equations
include no stimulated-emission gain or phase coherence. The dimensionless
parameter $theta$ is a numerical model choice, with no SI clock mapping.

*Calculation.* Paired exchange cancels in the total, leaving
$ e + sum_i x_i + o + ell = e(0) + sum_i x_i(0) + p. $ <eq:balance>
Each Euler step evaluates all transfers from the old state, commits the new
state, and verifies this identity exactly. Newly emitted storage cannot also
leak out within the same step. Nonnegative kernel rows require each step's
total departure fraction to be at most one; invalid steps are rejected.

#pagebreak()
= Jitter and numerical refinement <experiment>

*Jitter choice.* The bound Main calls jitter unresolved successor movement
before seating/return stabilization (`subsec:fractal-jitter`). This application
represents that unresolved choice by varying admissible shared-square weights:
$ k_f(m) = (1/16) (1 + a z_(m f)), quad
  z_(m f) in {-1, -3/4, -1/2, -1/4, 0, 1/4, 1/2, 3/4, 1}. $ <eq:jitter>
The finite distribution is uniform in the declared local seeded generator.
Each square and unit interval gets one multiplier; both directions share it.
Amplitudes $a=0$, $1/4$, and $1/2$ are tested. The complete realized schedule and
its hash are retained, so a seed alone is not the only replay record.

*Refinement.* Use seed 7 and 1, 2, 4, 8, 16, and 32 Euler substeps per unit
interval. Every refinement reuses the exact same piecewise-constant jitter
path and interval boundaries. Subdivision changes numerical resolution; it
does not resample the model. Exact rational updates eliminate floating-point
roundoff within the finite Euler calculation, but they retain discretization error.

*Independent reference.* Assemble the affine generator separately from the
pair-transfer engine and its stochastic kernel. Add a constant coordinate for
the pump and evaluate each interval using a Taylor matrix-exponential action.
Repeat at 60 and 80 decimal digits. Independent analytic emitter-decay,
isolated-leak, pump, and semigroup checks test this implementation.

For a scaled matrix norm $b$ and input norm $M$, the omitted exact Taylor series
after order $n$ has the majorant
$ R_n <= (M b^(n+1))/( (n+1)! ) dot 1/(1-b/(n+2)), quad n+2 > b. $ <eq:tail>
The recorded tail concerns the exact series for represented inputs. It excludes
Decimal rounding and propagation of previous interval errors. Precision agreement
is a diagnostic rather than a certified enclosure of the full trajectory.

*Comparison coordinate.* Each of the twelve outputs retains its exact Euler
value, approximate numerical comparator, signed discrepancy, quantity kind, and
unit. A fractional discrepancy uses that output's nonzero comparator magnitude
as its explicitly named denominator. The earlier L1 discrepancy divided by
total supplied mass is retained only as a separate aggregate diagnostic.
It is not a probability, kernel error, boundary-output error, or physical accuracy.

*Jitter sensitivity.* Independently run 16 retained realizations at each nonzero
amplitude using the continuous reference. The zero-amplitude case has one
deterministic realization. The output range is a finite ensemble spread, not an
empirical confidence interval or a measurement of apparatus accuracy.

#pagebreak()
= Per-output numerical comparisons <results>

#image(sys.inputs.at("chart", default: "../evidence/manual-alignment/accuracy-and-jitter.png"), width: 100%)

The plot compares the accumulated boundary output with its declared continuous
numerical reference. The table reports the signed fractional discrepancy
$(o_h-o_"ref")/abs(o_"ref")$. These are rounded dimensionless displays of a
solver comparison; no percentage of physical accuracy is inferred.

#tab(columns: (1fr, 1.6fr, 1.6fr),
  table.header([*Amplitude*], [*1 substep*], [*32 substeps*]),
  ..("0", "1/4", "1/2").map(amp => {
    let rows = data.convergence.filter(r => r.amplitude == amp)
    (label(amp), [#fmt(rows.first().output_report.outputs.output.fractional_discrepancy)],
      [#fmt(rows.last().output_report.outputs.output.fractional_discrepancy)])
  }).flatten(),
)

For amplitude $1/4$ and seed 7, the coarse exact output is $1419/1048576$.
Its reference is approximately $0.006465013878630756$ model-mass units.
The complete rational values and reference diagnostics remain in JSON.
The ensemble below uses separate continuous-reference jitter paths; its finite
spread is not a measurement uncertainty or confidence interval.

#tab(columns: (1fr, 1fr, 1.5fr, 1.5fr, 1.5fr),
  table.header([*Amplitude*], [*Paths*], [*Mean output*], [*Minimum*], [*Maximum*]),
  ..data.jitter_ensemble.map(entry => {
    (label(entry.amplitude), [#entry.realizations.len()], [#fmt(entry.output_mean)],
      [#fmt(entry.output_min)], [#fmt(entry.output_max)])
  }).flatten(),
)

*Exact checks.* All 81 cells satisfy the boundary-of-boundary identity. The
four-step retained example checks all 256 facet regions at every step, giving
#data.exhaustive_region_checks region balances. Every tested Euler step verifies
nonnegative state, stochastic departure, a nontrivial cubical region balance,
and zero global conservation residual. The retained example keeps gross
transfers even when the signed net transfer cancels.

*Reporting correction.* The earlier supplied-mass-normalized L1 values
$0.03495997$ and $0.00101139$ do not describe this output's relative discrepancy.
Exact conservation remains a separate identity check, even when a solver's
individual output differs substantially from its comparator.

#pagebreak()
= Exact D.E.C. kernels and retained histories <manual>

*Setup.* The Manual's D.E.C. means Declared Edge Computation ledger.
Raw AFC computation precedes its trace and AOD motif classification. A row
declares scope $B$, node, Hamming-1 edge slot, neighbor, ternary role,
admissibility, integer weight, exact probability, and route status.
The one-node fixture gives
$ w=(1,1,4,2), quad Z=8, quad P=(1/8,1/8,1/2,1/4). $
After a separately declared block removes additional hinge dwell, the slide
pool has weights $(3,3,1)$ and kernel $(3/7,3/7,1/7)$. Its returned and outgoing
fractions are $6/7$ and $1/7$. Zero admissibility removes a candidate; an admitted
candidate of zero weight remains structurally present. No scalar percentage
replaces these exact kernel rows.

*Kernel scope.* The auxiliary transport instead declares $P_h=I+h Q$ on
emitter, eight stores, output, and loss. Nonnegative departure fractions make
its rows stochastic. Pumping is a separate affine input. With a normalized
memory $p^"mem"$, AFC flux is
$ phi^"AFC"_(i j)=p^"mem"_i P_(i j)-p^"mem"_j P_(j i). $
This law does not construct admitted AOD wave or RCD histories. A reduction of
those histories needs its own sufficiency test; clustering alone does not
establish a Markov transition law on the summary.

*Retained construction.* The executable Manual fixture uses the bound producer's
trace, certificate, recurring-wave, field, and directed-contact constructors.
Every return retains event identities, ancestry, scope, path, and reflection
witness. Contacts retain both committed wave prefixes. Four distinct recurring
wave identities populate two declared support cells. Repeated field aliases
resolve to those same four members; they do not add population.

#tab(columns: (1.1fr, 2.2fr),
  table.header([*Quantity*], [*Meaning and required inputs*]),
  [$p^"mem"$], [Normalized AFC state; nonnegative entries sum to one.],
  [Population], [Distinct admitted recurring members assigned by a declared support map.],
  [Density], [Population per declared measure; a physical volume map is not supplied here.],
  [RCD], [Directed support/reflection object retaining lineages and repeated contacts.],
  [$rho^D$], [Declared duration accessor of RCD; it is not population density.],
  [$p^D$], [Sum of path contributions $C_(3,h) rho^D_(omega,h)$ on the declared window.],
)

*Calculation.* The bound additive duration specialization is
$ rho^D_h="RD"_h+sum_s r_(h,s), quad
  rho^D_(omega,h)=min(rho^D_h,abs(omega)_h). $
Clipping changes scalar participation while retaining RD and RCD. Pressure and
oriented values are formed per retained path before summation:
$ p^D=sum_h C_(3,h) rho^D_(omega,h), quad
  Phi^D=sum_h C_(3,h) rho^D_(omega,h) A_h. $
A common compressed attention requires a declared compression law. Reversing
the bookkeeping view of one channel negates its flux; constructing a physical
reverse channel evaluates its own directional RCD.

Here the two histories have $"RD"=3$ and reflected-count sums 2 and 4.
Clipping their durations 5 and 7 at 6, with capacities 3 and 2, gives
$p^D=15+12=27$. Attentions $1/3$ and $-1/2$ give $Phi^D=5-6=-1$;
the separately declared normalized AFC fixture instead gives $phi^"AFC"=-1/8$.

The fixture retains both histories when signed contributions cancel. Its matched
history control keeps the same current scalar summary but changes a retained
reflection history and obtains subsequent responses 5 and 6 under its
declared update. This refutes that summary's sufficiency for that update.

#pagebreak()
= Error classes and comparison records <errors>

*Data support first.* D0 is an exact internal fixture with declared integer
gates, channels, and comparator bins. O0 requires an observable-map value and
a measured-sector target. G0/G1/G2/G3 distinguish radial, proxy, spatial-field,
and time-resolved support. A tesseract geometry or numerical time index does not
by itself supply a physical G2/G3 dataset. Support class, execution success,
and empirical evidence are separate statements.

*Exact residuals.* For an integer fixture prediction $T_k$ and declared
comparator $O_k$,
$ Delta_k^ZZ=T_k-O_k, quad
  delta_(3,k)=(op("sgn")(Delta_k^ZZ),abs(Delta_k^ZZ)). $
The magnitude is an integer. Rational identity checks retain rational residuals;
they are not relabelled integer-event tallies. The emitted kernel fractions,
admitted population counts, and finite boundary identities remain exact.

*Per-output budget.* Each transport output has a kind, unit, exact Euler value,
numerical comparator, and the following separate records. The implementation
does not sum incomparable error categories or replace unknown values by zero.

#tab(columns: (1.1fr, 2.5fr),
  table.header([*Component*], [*Scope of this calculation*]),
  [Arithmetic], [Exact rational finite updates; zero arithmetic residual for those operations.],
  [Resolution], [Finite-step discrepancy is observed; no uniform discretization bound supplied.],
  [Window], [Fixed declared observation window; physical window acquisition uninstantiated.],
  [Model discrepancy], [No calibrated maser-dynamics discrepancy model; unknown.],
  [Calibration], [No physical calibration of the transport outputs; unknown.],
  [Propagation], [Conditioned on an exact schedule; uncertain-input propagation not instantiated.],
  [Solver], [Full reference-rounding enclosure unknown; numerical diagnostics retained separately.],
  [Comparison], [Measured-target budget inapplicable to this numerical control; no physical residual.],
)

An observed discrepancy is not automatically a deterministic upper bound or a
standard uncertainty. The reference's Taylor tail concerns its exact series;
Decimal rounding and propagation across intervals are not certified by that
tail. Agreement at two precisions remains a diagnostic. A finite jitter sample
does not supply a calibrated joint probability law for apparatus parameters.

*External reporting.* For a mapped value $y$ and compatible nonzero target $y_T$,
retain the signed residual $Delta=y-y_T$ and explicitly declared fractional
coordinate $epsilon=Delta/abs(y_T)$. A percentage display, if requested, is
$100 epsilon$ percent. Neither coordinate is a standard uncertainty. A
target-only pull uses only the target's quoted uncertainty and is labelled as
such; a joint standardized residual requires uncertainties of both sides and
their shared-source covariance. Target-only changes must leave the computed
prediction unchanged.

#pagebreak()
= Observed cycles, SI units, and actual targets <si>

*Observation map.* A native return, shed packet, oscillator cycle, estimated
population, and detector timestamp have different kinds. The Manual's observed
coordinate declares net winding $C(W)$ and endpoint phase fractions:
$ A(W)=C(W)+phi_1-phi_0. $
On the same coordinate, epoch, orientation, and window, the worked inputs are

#tab(columns: (1.3fr, 1fr, 1fr, 1fr, 1fr),
  table.header([*Record*], [*Winding*], [*Start*], [*End*], [*Advance*]),
  [Compared], [4], [$2/3$], [$1/6$], [$7/2$],
  [Reference], [8], [$1/4$], [$3/4$], [$17/2$],
)

Splitting the compared window at phase $5/6$ gives $13/6+4/3=7/2$.
Shared endpoint terms cancel exactly. If an independently justified realization
identifies the positive reference advance as corrected Cs-equivalent cycles,
the conditional report map is
$ A/A_R=7/17, quad
  T=A_R/f_"Cs"=17/(2 f_"Cs"), quad
  overline(nu)=A/T=(7 f_"Cs")/17. $
The SI defining value is exactly $f_"Cs"=9192631770$ hertz.
That definition supplies the unit after the reference map is declared;
realization and phase uncertainty remain separate. This worked fixture does not
assign a universal duration to one simulation cut.

*Measured hydrogen target.* Hellwig et al. report the corrected ground-state
hydrogen hyperfine value $1420405751.768$ hertz with published one-sigma
uncertainty $0.002$ hertz. These decimal source tokens are stored as exact
rational representations; the measurement is not thereby exact. Its quoted
uncertainty supplies no unreported distribution or independent raw campaign.

The two corrected table outputs are
$ 14204057517691/10000, quad 14204057517667/10000 $
in hertz. Their exact printed-input mean is $14204057517679/10000$ hertz;
the reported rounded mean differs by $1/10000$ hertz. This is a published-input
reduction, not a computed atomic prediction. Table VI's reference and wall
corrections are already included and must not be added again.

*Application binding.* The transport output has no bound preparation/emission/
phase-observation law to the hydrogen apparatus. Its frequency prediction,
physical residual, and complete uncertainty therefore remain uninstantiated.
The conditional $7/17$ example is not scored against the unrelated hydrogen
target. A future measured comparison must bind the history-to-observable map,
reference realization, calibration exposure, domain/window, target identity,
and joint error budget before interpreting agreement.

*Literature.*
#link("https://www.bipm.org/en/si-base-units/second")[BIPM: SI definition of the second].
#link("https://tf.nist.gov/general/pdf/13.pdf")[Hellwig et al. (1970): Measurement
of the unperturbed hydrogen hyperfine transition frequency], pp. 200 and 207.

#pagebreak()
= Dec execution and reproduction <execution>

`examples/tesseract_cell_jitter.dec` is generated from the four-interval,
one-substep, amplitude-$1/4$, seed-7 fixture. It uses existing `ratio` scalars,
assignments, and assertions. The host generator expands the finite cell arrays
into scalar statements; it does not add array syntax or a new native cell type.

```sh
python3 aodc.py run examples/tesseract_cell_jitter.dec
python3 aodc.py build examples/tesseract_cell_jitter.dec \
  -O 2 -o build/cell-jitter-example
./build/cell-jitter-example
python3 aodc.py jit examples/tesseract_cell_jitter.dec -O 2
```

Each program emits the twelve terminal values in the order stated in the
experiment, followed by the integer result `0`. Ratios print with a denominator,
including `/1`. Intermediate assertions check exact total accounting.

The full study checks four generated programs: amplitudes 0 and $1/4$, each
at one and two substeps. All terminal bytes are compared with the exact
pair-transfer engine through reference execution, typed IR, and, when requested,
AOT and ORC at O0/O2. Matching implementations supplement independent model
checks; they do not prove a physical interpretation.

```sh
make simulate-cells
# Add plots and require native AOT/JIT execution:
python3 scripts/simulate_cell_jitter.py --native --plots \
  --output build/cell-jitter
```

The standard-library study writes `results.json` and extracted Dec programs.
It also executes the source-bound host Manual fixtures and typed report checks.
Their full RCD and observation-map interfaces are host application code; the
scalar Dec transport replay does not add those interfaces to the language.
`--plots` additionally requires Matplotlib and exports the chart as PNG and PDF.
The result record names actual execution modes; absent native execution is never
reported as a native pass. Generated Dec expansion is limited to 128 steps,
while the host model supports larger bounded resolutions.

To rebuild this companion with Typst 0.13.1 after running the plotted study:

```sh
typst compile --root . \
  --input results=../build/cell-jitter/results.json \
  --input chart=../build/cell-jitter/accuracy-and-jitter.png \
  docs/tesseract-cell-jitter.typ \
  build/cell-jitter/tesseract-cell-jitter.pdf
```

The default Typst inputs use the retained evidence directory. This source has
no network assets or external Typst packages. The companion consumes the
language/compiler manual's existing execution contract.

#pagebreak()
= Source binding and computational scope <sources>

The original archive bindings remain in `evidence/cell-jitter/source-binding.json`.
The corrected application additionally binds the delivered Manual I candidate
at commit `8c3149b9d3d924b80b9385b178da4ba2c39175e8`, tree
`b2b55cbcf18354828f7b34418ab3fa26d747a927`, in
`evidence/manual-alignment/source-binding.json`. This delivered candidate is
not asserted to be the accepted remote Main. Its native witness producer is
vendored byte-for-byte with its MIT license; only the named fixture interfaces
are exercised here.

#tab(columns: (1.25fr, 2fr),
  table.header([*Source anchor*], [*Application here*]),
  [Main `subsec:fractal-jitter`], [Unresolved successor movement; the bounded
    distribution is an explicit downstream choice.],
  [Main `subsec:finite-q4-witness`], [Q4 support; the complete oriented cubical
    filling is supplied explicitly in this application.],
  [Main `eq:tesseract-admissible-boundary`], [Retained shell-cut context;
    application spatial cuts use a declared cell-chain dictionary.],
  [Main `eq:slice-flux` and `eq:window-current`], [One observation's crossing
    and retained accumulation across a chosen window remain distinct.],
  [Main `eq:b-scoped-stokes`], [Internal cancellation and region/boundary balance.],
  [Main `eq:coupling-trace`, `eq:rcd-coupling`], [Committed contact histories and directed RCD.],
  [Main `eq:path-history-pressure`], [Retained per-history pressure before compression.],
  [Manual `manual:calculation-spine`], [Declared inputs, exact calculation, map, and comparison.],
  [Manual `manual:scoped-errors-data-support`], [Support-specific residual and report coordinate.],
  [Manual `eq:manual-m3-accumulated-si`], [Conditional observed-cycle reference conversion.],
  [AFC `choice:memory-state`, `choice:kernel`, `eq:discrete-stokes`], [Normalized
    finite kernels and the declared chain/cochain dictionary.],
)

The owner amendment's section 3.3 places stochastic weights downstream of
admissible events. Its section 3.8 retains independent outer-closure obligations.
Shared cells and adjacency alone do not certify a Dec closure or fusion.
Main owns ontology and scoped mathematics; this computational application owns
its discretization, inputs, executable checks, and output records.

*Falsification checks.* Tests include explicit incidence signs, all-region
cancellation, invalid departure bounds, corrupted jitter schedules, nonnegative
states, isolated analytic solutions, and rejection of same-step premature leakage.
Manual checks additionally reject replayed returns, forged contact prefixes,
population aliases, mismatched duration domains, absent reference realization,
quantity-kind mismatch, and evaluation-target leakage. A reduction counterexample
retains the histories whose discarded reflection data change the later response.

*Physical continuation.* A maser application must supply atomic level dynamics,
stimulated emission and absorption, saturation, cavity modes and coupling,
decoherence, and an observation map to measured frequency and power. The present
reservoir transfer exercises conservation and solver comparisons. The separate
Manual fixtures establish their exact conditional calculations, not apparatus
performance. No upstream Main/Manual source is changed.
