#set document(title: "AOD D.E.C.: three-dimensional HI projection fit", author: "AOD Dec project")
#set page(paper: "a4", margin: (x: 20mm, y: 18mm),
  header: text(size: 8pt, fill: rgb("536471"))[AOD D.E.C. #h(1fr) Inferred galaxy inputs],
  footer: context align(right, text(size: 8pt, counter(page).display())))
#set text(font: "Libertinus Serif", size: 10.5pt, lang: "en")
#set par(justify: true, leading: .5em)
#show heading.where(level: 1): set text(size: 15pt, fill: rgb("173d50"))
#show raw: set text(font: "DejaVu Sans Mono", size: 8pt)
#set table(inset: 5pt, stroke: .4pt + rgb("cbd5df"))
#show table.cell: set par(justify: false)
#let base = sys.inputs.at("results", default: "../evidence/sparc-projection-fit/results")
#let figures = sys.inputs.at("figures", default: "../evidence/sparc-projection-fit/figures")
#let reports = ("NGC2403", "NGC3198").map(name => json(base + "/" + name + "/results.json"))
#let round(value, digits: 3) = calc.round(value, digits: digits)
#let rational(value) = {
  let pieces = value.split("/")
  if pieces.len() == 1 { float(value) } else { float(pieces.at(0)) / float(pieces.at(1)) }
}
#let physical-height(r) = r.selected_fit.geometry.height_arcsec * rational(r.observation_metadata.distance_mpc_exact) * 1000 * calc.pi / (180 * 3600)

#text(size: 24pt, weight: "bold", fill: rgb("173d50"))[Fitting a three-dimensional HI distribution]
#v(5pt)
#text(size: 12pt)[Matched HALOGAS moment projections]
#v(8pt)

The application constructs a three-dimensional neutral-hydrogen density and
mean circular-velocity field, projects that state to the observed sky plane,
convolves it with the measured beam, and fits the resulting moment maps. The
state is inferred from the observations. This is an observational input
reconstruction for AOD D.E.C.; it does not execute a native formation history,
galaxy time evolution or a full stellar, gas and bulge mass reconstruction.

= Setup and observation map <projection-setup>

For nonnegative weights $q_i$ with $sum_i q_i = 1$, normalized piecewise-linear
radial basis functions $b_i(R)$ and positive height $h$, the declared family is

$ rho(R,z) = M_("HI") sum_i q_i b_i(R) exp(-abs(z)/h)/(2h),
  quad integral_0^(R_("max")) 2 pi R b_i(R) dif R = 1. $

The final radial endpoint is zero and the model is zero outside its finite
radial support. Vertical support extends over all real $z$. Inclination,
position angle, centre and height determine each trial projection. Radial
weights and circular-streaming coefficients are fitted at each geometry.
Radial knots resolve the central flux concentration and retain outer basis
functions for all selected observations; their rule is stored with the run.
The projected velocity numerator and intensity are beam-convolved separately
before division; a velocity map is not convolved as an unweighted image.

The data are HALOGAS low-resolution moment-0 intensity and moment-1 LOS
velocity for NGC2403 and NGC3198. Their sky offsets retain FK5/J2000. The
spectral cube's third axis is velocity, not spatial depth, and the full cube
is not fitted here. Source units, masks, beam and exact file identities remain
attached to the observations. The source column-density maps are not used
because their retained unit headers conflict with their transformation history.

= Evaluated fit residuals <projection-results>

#table(columns: (1.05fr, 1.15fr, 1.15fr, 1.15fr, 1.1fr),
  [Galaxy], [Intensity NRMSE, initial], [Intensity NRMSE, fitted],
  [LOS velocity RMSE, initial → fitted / km s#super[−1]], [Fitted height / kpc],
  ..reports.map(r => (
    [#r.galaxy], [#round(r.initial_fit.intensity_nrmse, digits: 4)],
    [#round(r.selected_fit.intensity_nrmse, digits: 4)],
    [#round(r.initial_fit.velocity_rmse_km_s, digits: 2) → #round(r.selected_fit.velocity_rmse_km_s, digits: 2)],
    [#round(physical-height(r))],
  )).flatten())

Intensity NRMSE is $sqrt(frac(sum (I_("fit")-I_("obs"))^2, sum I_("obs")^2))$ on
the fixed intensity mask. LOS velocity RMSE uses its fixed velocity mask.
These observations also select the model parameters: the table reports fit
residuals, not independent prediction accuracy. NRMSE is a dimensionless
ratio, not a percentage or MAPE. The existing SPARC square-speed MAPE was
not recomputed.

NGC3198's unweighted and intensity-weighted velocity residuals both increase;
its weighted RMSE changes from
#round(reports.at(1).initial_fit.intensity_weighted_velocity_rmse_km_s, digits: 2)
to #round(reports.at(1).selected_fit.intensity_weighted_velocity_rmse_km_s, digits: 2)
km s#super[−1]. The combined loss improves through the intensity fit; this is
not an improvement in every observable. Both fitted inclinations reach the
lower search bound. NGC3198 has
#reports.at(1).selected_fit.velocity_audit.at_bound_count velocity coefficient
at its bound and #reports.at(1).selected_fit.velocity_denominator_floor_count
scored pixels using the model-intensity denominator floor.

The local bounded search retains worse alternatives and its geometry profile.
Those alternatives do not establish a confidence interval or unique depth.
Spiral structure, warps, vertical mean motion and multicomponent line profiles
are outside this fitted family.

#for r in reports {
  pagebreak()
  heading(level: 1)[#r.galaxy: projected comparison]
  image(figures + "/" + r.galaxy + "-projection.png", width: 100%)
  v(3pt)
  image(figures + "/" + r.galaxy + "-density-profile.png", width: 100%)
  v(4pt)
  [Selected inclination: #round(r.selected_fit.geometry.inclination_deg)°;
  declared search interval: #r.model.geometry_bounds.inclination_deg.at(0)° to
  #r.model.geometry_bounds.inclination_deg.at(1)°.
  Fitted exponential height: #round(physical-height(r)) kpc.]
  parbreak()
  [The three panels in each map row use the same source mask. Gray pixels are
  excluded or unknown, not measured empty space. Observed and model maps share
  their color scale; residuals are model minus observation. The density panel
  shows a dense inner cylindrical section. Rotating it about the vertical axis
  gives the declared axisymmetric three-dimensional density. The profile
  curves retain the seeded centre and position angle; the marked refined
  solution also changes geometry and uses higher-order quadrature. White in
  the density panel denotes zero model density. The complete radial basis and
  mass outside this displayed inner volume remain in the exported model.
  Density-panel axes are scaled independently; this panel is not a drawing
  of the galaxy's physical aspect ratio.]
}

#pagebreak()
= Mass, footprint and numerical scope <projection-accounting>

The catalog HI mass fixes each model's total normalization. It is a conditional
input estimate with no supplied complete mass uncertainty. The nearby,
optically thin observational conversion is

$ M_("HI")/M_("☉") = 2.356 times 10^5
  (D/"Mpc")^2 (F/("Jy km s"^(-1))). $

This external measurement map supplies intensity normalization; it is not an
AOD time-transition law. The source footprint flux differs from the flux
implied by the fixed catalog mass. That mismatch is retained rather than
silently rescaling the data or treating the catalog value as exact truth.

#table(columns: (1fr, 1.2fr, 1.2fr, 1.2fr),
  [Galaxy], [Fixed HI mass / 10#super[9] M#sub[☉]],
  [Catalog-implied flux / Jy km s#super[−1]], [Source-footprint flux / Jy km s#super[−1]],
  ..reports.map(r => (
    [#r.galaxy], [#round(r.model.total_hi_mass_msun / 1e9)],
    [#round(r.boundary_accounting.intrinsic_total_flux_jy_km_s)],
    [#round(r.boundary_accounting.source_footprint_flux_jy_km_s)],
  )).flatten())

The displayed vertical band is $abs(z) <= 6h$. It contains fraction
$1-exp(-6)$ of the model mass; the upper and lower tails each contain
$exp(-6)/2$. The dense radial view is narrower than the full model support:

#table(columns: (1fr, 1.1fr, 1.5fr),
  [Galaxy], [Dense radial view / kpc], [Model mass fraction outside displayed radial and vertical volume],
  ..reports.map(r => (
    [#r.galaxy], [#round(r.boundary_accounting.dense_radial_view_radius_kpc)],
    [#round(r.boundary_accounting.outside_dense_radial_and_vertical_view_mass_msun / r.model.total_hi_mass_msun, digits: 5)],
  )).flatten())

The radial and vertical exterior masses overlap and are not added twice.
These entries refer to the cylindrical volume obtained by rotating the
displayed $R,z$ domain. The Cartesian export also samples square-grid corners
at larger $R$; the cylindrical entries are not a Cartesian-box mass integral.
Faint outlying emission remains in the fit, with membership unclassified;
the outer fitted density does not authenticate a galactic halo.
The real radial mass beyond the finite model radius is unknown.
The sampled-grid flux difference combines quadrature, pixel sampling and
emission outside the grid; it is not a measured physical loss. Moving-boundary
sweep and time histories are not inferred from these static frames.
For NGC3198, full-grid minus fitting-view flux is
#round(reports.at(1).boundary_accounting.model_flux_outside_fit_view_inside_source_frame_jy_km_s, digits: 5)
Jy km s#super[−1]. This signed numerical difference does not represent negative
physical exterior flux.

#table(columns: (1fr, 1.2fr, 1.2fr),
  [Galaxy], [Intensity relative L2, quadrature change], [LOS velocity change RMSE / km s#super[−1]],
  ..reports.map(r => (
    [#r.galaxy], [#round(r.projection_numerical_check.intensity_relative_l2, digits: 5)],
    [#round(r.projection_numerical_check.velocity_rmse_km_s, digits: 5)],
  )).flatten())

This numerical check changes quadrature at the selected state without
refitting its coefficients. The final exported fit is separately refitted at
the higher order. Numerical sensitivity is not a physical uncertainty estimate.
An additional finer spatial-sampling check retains the same fitted coefficients;
its residuals are recorded with the result arrays and numerical report.
Missing noise covariance, inclination/distance systematics and model
discrepancy remain outside a complete statistical error budget.

= Reproduction and sources <projection-reproduction>

```text
python scripts/fit_sparc_projection.py --data DATA --context CATALOG \
  --output RESULTS --workers 2
python scripts/plot_sparc_projection.py --results RESULTS --output FIGURES
typst compile --root PROJECT_ROOT --input results=RESULTS \
  --input figures=FIGURES docs/sparc-projection-fit.typ projection-fit.pdf
```

The result files bind input SHA-256 values, geometry bounds, retained optimizer
evaluations, mask definitions, numerical comparisons and exported density
arrays. Optimizer history is not physical time. No native-octal metric or
probabilistic transition kernel is inferred from the fitted HI density.

Main/Manual application binding:
`ab8359209b8f9b2ef4007403101016e3ea58cfe4`; existing SPARC reconstruction and
field-window interfaces retain their source ownership. External observations:
#link("https://zenodo.org/records/3715549")[HALOGAS DR1, version 2];
#link("https://astroweb.cwru.edu/SPARC/")[SPARC catalog]; mass conversion:
#link("https://egg.astro.cornell.edu/alfalfa/docs/alfalfaprop.pdf")[ALFALFA survey proposal].
NGC2403 source lineage overlaps the SPARC reduction and is not automatically
an independent validation dataset.
