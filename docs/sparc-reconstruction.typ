#set document(title: "AOD D.E.C.: SPARC input reconstruction", author: "AOD Dec project")
#set page(paper: "a4", margin: (x: 22mm, y: 20mm),
  header: text(size: 8pt, fill: rgb("536471"))[AOD D.E.C. #h(1fr) Galaxy inputs and native spans],
  footer: context align(right, text(size: 8pt, counter(page).display())))
#set text(font: "Libertinus Serif", size: 10.5pt, lang: "en")
#set par(justify: true, leading: .55em)
#show heading.where(level: 1): set text(size: 16pt, fill: rgb("173d50"))
#show raw: set text(font: "DejaVu Sans Mono", size: 8pt)
#set table(inset: 5pt, stroke: .4pt + rgb("cbd5df"))
#show table.cell: set par(justify: false)
#let result = json(sys.inputs.at("results", default: "../evidence/sparc-reconstruction/results.json"))
#let decimal(s) = {
  let pieces = s.split("/")
  if pieces.len() == 1 { float(s) } else { float(pieces.at(0)) / float(pieces.at(1)) }
}

#text(size: 25pt, weight: "bold", fill: rgb("173d50"))[From projected profiles to field inputs]
#v(5pt)
#text(size: 12pt)[SPARC reconstruction, inverse requirements and calculated native spans]
#v(8pt)

The existing AOD calculus supplies retained closure, recurrence, directional
duration, pressure and current response. This application reconstructs missing
stellar data and computes the contribution needed to match the observed radial
curve. It does not introduce a galaxy force law. The reconstructed physical
weights still need a binding to native formation and current histories before
the galaxy can be evolved.

= Stellar reconstruction <stellar-inputs>

The five retained galaxies contribute 209 radial bins. Their tabulated disk
surface luminosity $I(R)$ is already corrected for inclination. Linear radial
interpolation is confined to this tabulated annulus; some source profiles
contain decomposition or extrapolation. With the declared stellar
mass-to-light ratio $Upsilon = 1/2$, the axisymmetric input family is

$ Sigma(R) = Upsilon I(R), quad
rho(R,z) = Sigma(R) / (2h) exp(-abs(z)/h). $

Lengths must use the same unit in this formula. The implementation converts
the catalog height in kpc to pc before reporting solar masses per cubic pc.
The empirical SPARC relation supplies the estimated height:

$ h / "kpc" = 0.196 (R_d / "kpc")^(0.633). $

#table(columns: (1.1fr, .9fr, .9fr, 1.3fr, .7fr),
  [Galaxy], [Disk scale / kpc], [Height / kpc], [Radial domain / kpc], [Radial decades],
  ..result.stellar_reconstruction.map(g => (
    [#g.galaxy], [#decimal(g.input_geometry.disk_scale_kpc)],
    [#calc.round(g.estimated_height_kpc, digits: 6)],
    [#g.radial_display.minimum_kpc to #g.radial_display.maximum_kpc],
    [#calc.round(g.radial_display.decades, digits: 4)],
  )).flatten())

The radial range ratio and its decimal logarithm describe physical orders of
magnitude. They are unchanged by converting kpc to pc. They do not assign
physical length multipliers to native octal addresses.

For every positive $h$, integration over all vertical support returns exactly
$Sigma(R)$. Therefore the zero column residual is an interpolation-consistency
result. The column alone does not identify thickness. The height relation is an
input estimate; its scatter and the mass-to-light uncertainty remain unsupplied.

= Boundaries and retained tails <density-tails>

For the characteristic band $abs(z) <= h$, the retained mass fraction is
$1-exp(-1)$ and the two tails together contain $exp(-1)$. Their sum is one.
The code preserves both tails analytically; the band is a diagnostic physical
volume, not a selected native recurrence window.

Every final tabulated disk column rounds to zero. Extending that rounded
endpoint would erase the radial tail. A separate estimate instead continues
the last positive tabulated column exponentially using the catalog disk scale.
Both calculations are recorded. The estimate does not certify the true outer
profile. Central, gas and bulge volume densities remain unresolved.

#pagebreak()
= Native spans calculated from histories <native-spans>

Main defines a beat through a distinct certified reclosure. Given committed
completion cuts $k_0 < k_1 < dots < k_m$, the extractor calculates

$ "Sep"_j = k_(j+1) - k_j, quad omega_j = (k_j,k_(j+1)], quad
"span" = k_m-k_0. $

The window $(k_0,k_m]$ contains $m$ reclosures. Nonuniform separations retain
jitter. Actual fold certificates bind inner dependencies and the outer scale;
inner beats are not counted again as outer beats. Pending recorded tails and
unobserved gaps are separate. No future period is extrapolated from a finite
record. Directional RD/RCD and clipped participation have different operands
and are not replaced by the cadence span.

```python
from simulations.sparc_native_span import extract_native_span
span = extract_native_span(waves, pending_traces=open_tails)
```

This API verifies actual native histories. The galaxy run supplies none, so
its native span is unavailable. Manual Appendix C defines an acquisition
interface from mass/size to phase scale; its fixed numerical fixture is not a
measured galaxy binding. Physical density is not silently treated as native
RCD, capacity, or a count of admitted participants.

= Projection and comparison <sparc-comparison>

The frozen G0 control preserves the original squared-gas convention, disk and
bulge factors, and external factor 8. Each observed bin remains in the score.
The companion inverse calculation solves the same map for its required
effective contribution, retaining negative values and nominal error intervals.

#table(columns: (2fr, 1fr),
  [Comparison], [Equal-bin square-speed MAPE],
  [Frozen G0 control], [#result.display.g0_mape_percent%],
  [In-sample scalar diagnostic], [#result.display.in_sample_mape_percent%])

The diagnostic fits one nonnegative effective factor per galaxy using the
exact weighted-median minimum of square-speed MAPE. It uses these same targets.
It is not a recovered density or a new evolved AOD prediction.
#result.display.negative_required_contributions bins require negative effective
contributions under the fixed map; those values are preserved. Unknown
inclination systematics and reconstruction discrepancy are not zeroed.

The implementation now provides the input reconstruction, exact inverse
requirements, native-span extractor and complete-cohort comparison. A G2/G3
galaxy current evolution and its projected rotation curve remain unexecuted
until the reconstructed data are bound to actual native support and histories.

= Reproduction and sources <sparc-reproduction>

```text
python3 scripts/reconstruct_sparc.py --output build/sparc
typst compile --root . --input results=../build/sparc/results.json \
  docs/sparc-reconstruction.typ build/sparc/sparc-reconstruction.pdf
```

Main/Manual binding: `ab8359209b8f9b2ef4007403101016e3ea58cfe4`.
Main: cut-running tesseract, return identity/beat/cadence; field, inverse-pressure
response and causal update; cell measures and typed maps. Manual: field-dynamics
applications, G0 and orbital-retention stages; coordinate phase-cycle Appendix C.
The evidence directory records exact source hashes, reviewed anchors and tests.

External input: #link("https://astroweb.cwru.edu/SPARC/")[SPARC data],
#link("https://doi.org/10.3847/0004-6256/152/6/157")[Lelli, McGaugh and Schombert (2016), AJ 152, 157],
especially section 3.3 and Table 1. Their geometric reconstruction conventions
are physical input estimates, distinct from native AOD dynamics.
