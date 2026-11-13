// Computational application companion to language-and-compiler.typ.
#let data = json(sys.inputs.at("results", default: "../evidence/cell-jitter/results.json"))
#assert(data.status == "PASS", message: "The application requires a passing result record")
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
#text(size: 14pt)[Emitter, retained cell storage, and boundary transport]
#v(8pt)
This application implements the owner's proposed tesseract-cell simulation and
measures its numerical accuracy under a declared jitter law. It uses explicit
oriented cells, retained interface transfers, exact rational Euler updates, and
an independently assembled continuous reference. Scalar Dec programs reproduce
the finite calculation through the reference VM, AOT, and ORC JIT.

#note[
*Result scope.* Exact topology and conservation checks pass. Refinement measures
error against the declared transport equations, while separate jitter ensembles
measure the response to changed successor weights. The model supplies the
energy-accounting portion of an emitter/cavity example. Maser gain, coherence,
frequency, linewidth, and agreement with physical measurements are not established.
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
= Emitter and cell transport <transport>

*Setup.* The eight cubic facets are storage compartments. Each has six neighbors
through shared squares. Label the source facet `0***` and the output facet `1***`.
An upper-state reservoir $e$ feeds the source; $x_i$ is storage in facet $i$;
$o$, $ell$, and $p$ accumulate output, absorption, and supplied excitation.
All are nonnegative normalized model quantities.

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
= Jitter and the accuracy experiment <experiment>

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

*Error measure.* Compare the twelve terminal components: emitter, eight cell
values, output, loss, and injected total. Report their L1 discrepancy divided by
total supplied initial-plus-pumped value. Reference uncertainty is checked at
a much smaller scale than the observed Euler errors. First-order convergence
is evaluated for these declared cases, not asserted for arbitrary programs.

*Jitter sensitivity.* Independently run 16 retained realizations at each nonzero
amplitude using the continuous reference. The zero-amplitude case has one
deterministic realization. The output range is a finite ensemble spread, not an
empirical confidence interval or a measurement of apparatus accuracy.

#pagebreak()
= Measured computational results <results>

#image(sys.inputs.at("chart", default: "../evidence/cell-jitter/accuracy-and-jitter.png"), width: 100%)

All three fixed-path refinement studies reduce the terminal discrepancy. The
table gives normalized L1 error for the coarse and finest tested resolutions,
and observed order on the final refinement pair.

#tab(columns: (1fr, 1.6fr, 1.6fr, 1fr),
  table.header([*Amplitude*], [*1 substep*], [*32 substeps*], [*Final order*]),
  ..("0", "1/4", "1/2").map(amp => {
    let rows = data.convergence.filter(r => r.amplitude == amp)
    (label(amp), [#fmt(rows.first().relative_l1_error)],
      [#fmt(rows.last().relative_l1_error)], [#fmt(rows.last().observed_order, digits: 4)])
  }).flatten(),
)

The jitter ensemble uses the independently computed continuous reference.
Outputs below are in normalized model units. The plots and tables round values
for display; the JSON records retain exact rational or high-precision results.

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

*Interpretation.* Refinement error is the discrepancy introduced by the chosen
numerical update. Jitter sensitivity is the change caused by a different declared
successor path. Neither quantity alone establishes the physical accuracy of a
hyperfine emitter or maser. That comparison needs calibrated dynamics and data.

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
= Source binding, checks, and next physical model <sources>

The active source bindings are recorded in
`evidence/cell-jitter/source-binding.json`. The execution request's supplied
Main/Manual archive is `AOD_v40_01r10_source.zip`; its SHA-256 is
`a178a5bf06d51c8ab8852e9cabfb6c5eb61456f70739d7391582c08c524f34c7`.
The source record verifies six members and 29 labels across that archive and
the supplied AFC archive. It does not infer a newer accepted manuscript head.

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
  [Manual `manual:dec-ledger:boundary-audit`], [Operational boundary-accounting example.],
  [AFC `choice:memory-state`, `choice:kernel`, `eq:discrete-stokes`], [Normalized
    finite kernels and the declared chain/cochain dictionary.],
)

The owner amendment's section 3.3 places stochastic weights downstream of
admissible events. Its section 3.8 retains independent outer-closure obligations.
Shared cells and adjacency alone do not certify a Dec closure or fusion.
Main owns ontology and scoped mathematics; this computational application owns
its discretization, inputs, executable checks, and output records.

*Normalization.* The transport state is unnormalized energy-like mass with an
external pump. Its closed transport-plus-sinks kernel has row sum one. Normalizing
a positive state before applying that kernel supplies the separate AFC memory
fixture. The normalized bound on AFC flux is not transferred to pumped mass.

*Falsification checks.* Tests include explicit incidence signs, all-region
cancellation, invalid departure bounds, corrupted jitter schedules, nonnegative
states, isolated analytic solutions, and rejection of same-step premature leakage.
Passive retention and compact reporting give identical terminal states.

*Physical continuation.* A maser application must supply atomic level dynamics,
stimulated emission and absorption, saturation, cavity modes and coupling,
decoherence, and an observation map to measured frequency and power. The present
reservoir transfer can exercise conservation and numerical accuracy before those
additional premises are introduced. It supplies no SI anchoring or hardware
performance claim, and changes no upstream Main/Manual source.
