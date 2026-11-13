// Declared downstream application; no change to Main/Manual definitions.
#let data = json(sys.inputs.at("results", default: "../evidence/chiral-collision/results.json"))
#assert(data.status == "PASS")
#assert(data.cases.core_only.expected_flip_count == "11/8")
#assert(data.native_replay.native_requested, message: "This edition requires native replay evidence")
#let ink = rgb("173d50")
#set document(title: "AOD Dec: Core-Biased Chiral Encounters", author: "AOD Dec project")
#set page(paper: "a4", margin: (x: 22mm, y: 20mm),
  header: text(size: 8pt, fill: rgb("536471"))[AOD Dec #h(1fr) Computational application],
  footer: context align(right, text(size: 8pt, counter(page).display())))
#set text(font: "Libertinus Serif", size: 10.5pt, lang: "en")
#set par(justify: true, leading: .62em)
#set heading(numbering: "1.1")
#show heading.where(level: 1): set text(size: 19pt, fill: ink)
#show heading.where(level: 2): set text(size: 13pt, fill: ink)
#show raw: set text(font: "DejaVu Sans Mono", size: 8pt)
#set table(inset: 6pt, stroke: .4pt + rgb("cbd5df"))
#let tab(..args) = text(size: 9pt, table(..args))
#let note(body) = block(width: 100%, inset: 10pt, fill: rgb("edf5f6"),
  stroke: (left: 2pt + rgb("2c7181")), body)

#text(size: 27pt, weight: "bold", fill: ink)[Core-biased chiral encounters]
#v(3pt)
#text(size: 14pt)[Exact Markov probabilities, complete returns, and retained histories]
#v(8pt)
Two admitted carrier lineages move on the eight cubic facets of a tesseract
boundary. At each selected destination, a rational probability determines
whether the next complete return uses the opposite handed presentation.
The primary experiment makes this probability larger in a declared field core.
It computes an exact joint law and retains sampled histories for replay in Dec.

The probabilistic model supplies the kernel, profile and recipe-selection rule.
The D.E.C. execution engine supplies the checked operations that execute those
recipes and retain their witnesses. This application supplies its probability
expressions as program data and code; the compiler/runtime contain no new
built-in collision or core-bias law.

#note[
*Claim.* This is a finite collision-inspired application of the owner's proposed
core bias. It does not establish that a physical proton-proton collision is a
chiral flip. The carrier-to-proton correspondence, interaction/observation map,
collision energy, momentum and SI clock remain uninstantiated.
]

= Support, formation and state <chiral-setup>

*Setup.* Reuse the oriented tesseract complex: 16 vertices, 32 edges, 24 squares,
8 cubic facets and 1 four-cell. Two facets are adjacent when they share a square.
Each facet has six neighbors. The facet-dual movement is a coarse support
relation; a move across a square is not itself a Q4 Hamming-1 event.

The frozen background contains 11 distinct admitted recurring waves, placed as
follows. Each has two certified returns. The two incoming carrier lineages are
separate members with their own two-return initial histories.

#tab(columns: (1.3fr, 1fr, 2.2fr),
  table.header([*Support*], [*Population*], [*Declared score*]),
  [`*0**`], [4], [$s=1$ (designated core)],
  [Each other facet], [1], [$s=0$ (outer support)],
  [All eight facets], [11], [Count of distinct background identities],
)

Set $s(f)=(N(f)-1)/3$. The profile breaks the symmetry explicitly: bare
tesseract incidence has no distinguished core facet. Population count is not
population density without a cell measure. Directed RCD and its duration
projection $rho^D$ are different objects and are not inferred from these counts.

The transition state is
$ X_n=(f_(A,n),f_(B,n),chi_(A,n),chi_(B,n)), quad chi in {-1,+1}, $
with $8^2 dot 2^2=256$ states and initial value
`("0***", "1***", +1, -1)`. The fixed profile and parameters complete the law.
Archived histories accompany each realization; they do not feed back into this
particular kernel. A chirality-only projection discards position and is not
assumed Markovian. Adding pressure, contact memory or an RCD-dependent law would
require carrying those operands in the transition state.

#pagebreak()
= Update law and complete returns <chiral-kernel>

*Movement.* Each carrier independently holds with probability $1/4$ or selects
one of its six neighbors with probability $1/8$ each. Write this row-stochastic
kernel as $M(f,g)$. Positions are updated before the flip probability is read.

*Core rule.* The primary probabilities are
$ q(g)=q_o+(q_c-q_o)s(g), quad q_o=1/16, quad q_c=1/4. $
For signs $a,b in {-1,+1}$, define
$ B_(q)(a,b)=cases(1-q & "if " b=a, q & "if " b=-a). $
The exact joint transition is
$ K(X,Y)=M(f_A,g_A)M(f_B,g_B)
  B_(q(g_A))(chi_A,eta_A)B_(q(g_B))(chi_B,eta_B). $
Conditional independence of the two flips is a declared modeling choice.
Each movement row and each binary row sums to one, so the product row also
sums to one. An independent integer-weight recursion evaluates the same law
without calling the host model evaluator's transition function.

*One update completes one return per carrier.* A selected sign chooses an entire
branch-hinge-return recipe. In the short Dec witness, its events are
$ (0 arrow.r 1, sigma=chi), quad
  (1 arrow.r 1, sigma=0, "hinge"), quad
  (1 arrow.r 0, sigma=-chi). $
The Hamming-1 traversals, mandatory hinge and closure are checked before the
new return is appended. The binary sign $chi$ is a branch-order tag, while
the checked ternary role $sigma$ includes the hinge. Changing the next sign
does not rewrite earlier events or erase a cancelling pair of returns.

*Retained records.* Each update carries the old/new state, crossed square,
movement tickets, exact flip probabilities, flip tickets, selected signs,
carrier identity, predecessor return, new return identity and witness digest.
Full witness exemplars are retained for seeds 7 and 19. The cohort retains all
step records and digests; its large witness payloads can be regenerated from
the bound code, configuration and seed. Distinct counterfactual continuations
receive distinct event identities while sharing their initial prefix.

*Exact sampling.* Separate deterministic streams supply A/B movement and A/B
flip tickets. Movement tickets lie in $0..7$. The paired studies use a common
flip denominator $D=64$, and a flip occurs precisely when $u<D q$ for integer
$u in {0,dots,D-1}$. These are seeded pseudorandom realizations, not measured
collision outcomes. The paired probability interventions retain identical
movement and flip tickets.

#note[
*Optional contact sensitivity.* A separate case sets
$q'=q+(1/4)(1-q)$ when destinations coincide. Its core/outer contact values
are $7/16$ and $19/64$. The primary case has contact gain zero. Co-residence
is only a projected encounter candidate; it admits neither coupling nor fusion.
The Manual's early-coupling classifier needs duration, capacity, phase,
impedance and attention operands that this example does not instantiate.
]

#pagebreak()
= Exact consequences and controls <chiral-results>

*Evaluated window.* Eight updates produce eight returns per carrier and 16 flip opportunities.
The exact primary calculation gives:

#tab(columns: (1fr, 1.4fr, 1.2fr, 1.4fr),
  table.header([*Region*], [*Expected opportunities*], [*Flip probability*], [*Expected flips*]),
  [Core], [2], [$1/4$], [$1/2$],
  [Outer], [14], [$1/16$], [$7/8$],
  [Total], [16], [Exposure dependent], [$11/8$],
)

The core probability is four times the outer probability per opportunity.
Nevertheless the expected outer flip count is larger, because outer exposure
is seven times greater. The uniform $q=1/16$ control has expected total count
1; introducing this core profile raises that expectation by $3/8$.
These statements describe the declared finite law, not physical accuracy.

#figure(image(sys.inputs.at("chart", default: "../evidence/chiral-collision/chiral-study.png"),
  width: 100%), caption: [Probability, exposure-weighted count, parity control
  and one retained path. Decimal chart coordinates are display conversions;
  all underlying records use integers and exact fractions.])

*Falsification controls.* Both implementations agree exactly on terminal
distributions for six cases: core-only, uniform, optional contact feedback,
no flips, forced flips, and constant $q=1/4$. The no-flip control preserves
both signs. Eight forced flips per carrier restore both initial signs despite 16 total
flips. The primary, uniform and contact-feedback cases share the same position law.
Normalization and agreement are computational checks, not empirical validation.

#pagebreak()
= Sampling and typed comparisons <chiral-comparison>

*Conditional sample report.* The primary cohort has
#data.sampling.summaries.core_only.realizations two-carrier realizations, using
seeds #data.sampling.seed_range.at(0) through #data.sampling.seed_range.at(1).
Report each stratum's observed flips $F_s$ and opportunities $O_s$ before
forming $F_s/O_s$. Co-residence stratifies exposure here without modifying
the primary probability.

#let sample = data.sampling.summaries.core_only.strata
#tab(columns: (1.5fr, .8fr, .8fr, 1fr, 1.2fr),
  table.header([*Stratum*], [*$O_s$*], [*$F_s$*], [*$q_s$*], [*$F_s/O_s$*]),
  ..("core_contact", "core_separate", "outer_contact", "outer_separate").map(key => {
    let row = sample.at(key)
    (text(key.replace("_", " ")), str(row.opportunities), str(row.flips),
      raw(row.declared_probability), if row.observed_frequency == none { [Undefined] } else { raw(row.observed_frequency) })
  }).flatten(),
)

The sample discrepancy is the exact rational quantity $F_s-q_s O_s$.
It is not an integer residual unless an explicit common scaling is declared,
and is not automatically a confidence bound or standard uncertainty. No
arbitrary closeness gate is imposed on this finite sample. The core-only, uniform
and contact-feedback settings retain their own matched-ticket histories.

*Three distinct outputs.* For one carrier with constant flip probability $q$
over $n$ independent return selections,
$ E[F]=n q, quad
  P(F>=1)=1-(1-q)^n, quad
  P(chi_n != chi_0)=(1-(1-2q)^n)/2. $
At $q=1/4$ and $n=8$, these are respectively
2, $58975/65536$, and $255/512$. The terminal sign detects odd parity;
it cannot count all flips or recover the discarded path. High flip activity
does not by itself establish a net polarization.

*Support and errors.* The admitted witness/count checks form D0 finite fixtures.
Probability identity comparisons keep rational coordinates. O0 physical
comparison requires an observation-map value and measured target. Geometry and
an update index alone do not provide physical G2/G3 support.

#tab(columns: (1.15fr, 2.75fr),
  table.header([*Component*], [*Evidence and limitation*]),
  [Arithmetic], [Exact integers and fractions; zero arithmetic roundoff for this calculation.],
  [Joint-law comparison], [Exact zero terminal probability residual against the independent recursion, for the six declared cases.],
  [Sampling], [Per-stratum rational discrepancies retained; no confidence bound or standard uncertainty supplied.],
  [Resolution/window], [Eight discrete completed-return updates; no continuous-limit or physical timing approximation claimed.],
  [Projection], [Fixed profile; position and both signs retained. Any new history feedback needs a revised sufficient state.],
  [Physical model], [Proton correspondence, scattering observables and discrepancy unknown.],
  [Calibration/SI], [No seconds-per-return, energy, momentum or cross-section map instantiated.],
)

#pagebreak()
= Source binding and reproduction <chiral-reproduce>

*Ownership.* Chirality, closure, field identity and admissible continuation
belong to the bound Main/Manual. Core placement, symmetric flip probabilities,
independent pair draws and the optional contact sensitivity are declarations
of this downstream application. They do not amend those source definitions.

#tab(columns: (1.1fr, 2.8fr),
  table.header([*Bound source*], [*Definitions used*]),
  [Main, curl/closure], [`eq:hinge-chirality-tag`, `eq:hinge-chirality-flip`, field organization and retained lineage.],
  [Main, wave state], [`eq:complete-wave-state`, `eq:admitted-kernel`, `subsec:duon-continuity`.],
  [Manual I], [`manual:calculation-spine`, `manual:exact-arithmetic-presentation`, `manual:scoped-errors-data-support`, orbital-retention classifier.],
  [Matter I], [`eq:manual2-campaign-orientation`: precedent for complete stochastic return selection; its probability law differs from the present core rule.],
)

Main/Manual I is bound to the delivered local candidate
`8c3149b9d3d924b80b9385b178da4ba2c39175e8`. Matter I is the verified detached
delivery reconstruction based on `904d4a9b2fc90fc96050f6c3b555d5d921c0a8d9`.
Neither is silently promoted to an accepted remote release. The source record
retains exact hashes, label locations and the current owner amendment.

*Execution boundary.* Python enumerates the 256-state law and generates the
seeded cohort. Four generated Dec programs replay supplied positions and
integer tickets, compute probabilities and sign changes, and construct actual
branch-hinge-return traces. Each is checked through the reference VM, typed IR,
AOT at O0/O2 and ORC JIT at O0/O2. Independent output validation inspects every
return event and closure range before differential output comparison. This is
native replay of the selected histories; it is not native execution of the
entire sampler or joint-distribution enumerator.

```sh
python3 scripts/simulate_chiral_collision.py --native --plots
make docs-chiral TYPST=/path/to/typst-0.13.1
python3 -m unittest tests.test_chiral_collision \
  tests.test_chiral_reference tests.test_chiral_dec
```

The study writes exact `results.json`, compact `cohort.json`, full
`exemplars.json`, the figure and four Dec replays with full trace output.
`examples/chiral_core_collision.dec` is the canonical seed-7 primary replay.
The Typst document accepts `results` and `chart` input paths; CI builds it from
fresh study outputs. The pinned AOD lint scan covers this manuscript, while
tests and native executions supply separate behavioral evidence.

*Physical comparison boundary.* A proton-proton application would additionally
need an admitted incoming-proton preparation, interaction law and relation
between native outcomes and an actual target observable. Energy, scattering
angle or momentum transfer and cross section must carry compatible units and
a declared uncertainty budget. No value for these missing operands is fitted
or inferred here. The
#link("https://atlas.cern/Updates/Physics-Briefing/ALFA-scattering")[ATLAS elastic-scattering account]
(8 July 2022) illustrates measured pp observables; it supplies no transition
parameter to this calculation.
