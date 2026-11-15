# Source-bound scope review: 3D inverse projection

Review date: 15 November 2026. User authorizes simulating and fitting a three-dimensional distribution whose projected observables match acquired data. This is an inverse reconstruction, not an independent prediction. The active repair/implementation is separate from this read-only review. Exact file hashes, actual heads and external source coverage are in `model-scope-sources.json`.

## Source commitments and current implementation

Main/Manual commit `ab8359209b8f9b2ef4007403101016e3ea58cfe4`; DEC starting commit `99035b0cd7bad3103e55fe0cf82fffc6b3145b8d`. AOD manuscript ownership and its current local amendment govern notation/style; AF governs inference. No AFC probability kernel is needed merely to solve this observation-level inverse problem.

- `manual/sections/06_field_dynamics_applications.tex:17–24` permits declared macroscopic clustering, finite boundaries/windows, support families and value maps, and rejects an automatic Markov closure from a common present summary.
- The same file at lines 43–48 distinguishes observable-map/G0/G1 work from G2 density/velocity/current support and G3 time-resolved tracer histories. Lines 74–101 require separate density, velocity, window, boundary, coupling, route and mapping uncertainties. Lines 571–591 allow source or explicitly declared proxy rows, including latent core/background and time-flow components.
- Its two-dimensional/three-dimensional toy, lines 156–198, distinguishes projection residuals from wrong-boundary residuals. It does not quantify how much of the observed SPARC mismatch is caused by missing dimensions.
- Main `sections/07_cell_measures_mappings.tex:73–87` explicitly permits many-to-one projection and requires a reconstruction map/metric before assigning reconstruction error. Different preimages can have identical observables.
- Main already contains scoped current/causal dynamics, including `sections/06_field.tex` labels `eq:moving-boundary` and `eq:causal-update`. The issue for this dataset is construction of its native operands and histories, not absence of AOD calculus.
- DEC `simulations/sparc_reconstruction.py` currently constructs an axisymmetric exponential stellar family using a catalog height relation. Its `project_column` is independent of every positive height. `simulations/sparc_inverse.py` fits one effective scalar per galaxy; it does not fit density. A new volumetric inverse forward operator is substantive work beyond both.

## Concrete implementable fit

Use the acquired NGC3198 and NGC2403 HI cubes as **spectral observations** of a finite nonnegative three-spatial-dimensional gas-emissivity family. The third cube axis is velocity, not depth. Fit a declared emissivity distribution and velocity field, then simulate the actual observation of that state.

A useful bounded family consists of nonnegative radial ring weights, normalized vertical profiles, and optional low-order angular structure. For fixed geometry this gives a linear nonnegative intensity problem; profile a small grid or bounded optimizer over geometry/height/kinematics. Store the inferred 3D coefficients or voxel state, not merely the projected image. Start with one disk and a declared thin/thick alternative; do not assign an arbitrary free density to every observed pixel, which can reproduce that image without testing meaningful structure.

For each candidate: rotate galaxy coordinates to the measured sky orientation, integrate along the line of sight, place emission into the measured spectral channel response using its projected velocity distribution, then apply the measured angular beam. Retain the exposure mask and a padded projection domain. Use the same operator for synthetic-recovery controls and real-data fits. Inclination, position angle, center, systemic velocity, radial weights, height and velocity-dispersion choices must identify their source/fitted status. A Gaussian line profile or separable vertical family is an explicit reconstruction choice, not a newly derived AOD evolution law.

This approach is consistent with primary observational work: [Gentile et al., HALOGAS NGC3198](https://arxiv.org/abs/1304.4232) reports a conditional 3D fit with extraplanar gas and uncertain lag; [Di Teodoro and Fraternali](https://arxiv.org/abs/1505.07834) describes fitting tilted-ring models directly to spectral cubes. Neither source makes the chosen reconstruction unique or supplies native AOD histories. No numerical parameter from those fits is imposed here as measured truth.

NGC2403/NGC3198 gas fitting does not determine their stellar distribution. Existing SPARC surface luminosity can supply a separately labelled stellar reconstruction family with its own mass-to-light/height assumptions. The two newly acquired NGC6503 dispersions cannot constrain another galaxy by substitution. A combined model must retain separate gas, stellar and unresolved central/background components; a gas-only successful fit must be reported as such.

## Units, mass and retained boundaries

Fit the validated cube intensity or moment-0 intensity directly. Both archived `coldens` products retain an intensity BUNIT despite their historical numerical conversion. If a column-density map is used, retain an explicit source reconciliation and calibration record rather than changing its header silently.

Cube units are Jy/beam. Spatial integration into Jy needs pixel-to-beam solid-angle conversion under the declared beam model. Channel integration uses the positive channel width; NGC2403 has a descending velocity axis. Beam size, angular pixel dimensions, orientation and legacy optical-velocity interpretation are external observation-map quantities with numerical approximation error. Do not turn diagonal/square-root/trigonometric representation error into an assumed physical error floor.

Normalize each spatial basis over its declared three-dimensional domain. Record retained mass/emission, analytic or numerical tails, contributions projected outside the angular footprint and spectral tails outside acquired channels. Padded beam convolution must not silently discard boundary flux or renormalize it back into the image. A cropped field is an observed window, not proof of zero mass beyond it.

Catalog HI mass is a gas constraint, not full galactic mass. If imposing it, bind its distance and source and account for the difference between whole-galaxy coverage and the fitted footprint. Do not give an uncertainty-free catalog number an invented Gaussian uncertainty. Use a declared conditional normalization or a sourced interval, and expose inconsistency with the observed flux rather than quietly rescaling one side. NGC2403 has shared Fraternali2002 source lineage between SPARC and HALOGAS, so these cannot be treated as independent likelihood factors by default.

Optimization iterations enumerate candidate states; they are not physical time cuts. For actual evolving inputs, Manual04 lines 47–55 requires crossing minus sweep plus shedding on the declared moving boundary. A single acquired snapshot does not supply that temporal history. Preserve this distinction in any trace label; geometry-fit history may be replayed without being called galactic evolution.

## Scores and uncertainty

Use signed per-voxel/map residuals, integrated-flux residual, intensity RMSE or another declared finite objective. Pixelwise MAPE is unsuitable for zeros and sign-changing noise unless a fixed eligibility rule and its exclusions are separately disclosed. Keep MAPE fractions versus percent and percentage-point changes explicit. New intensity/velocity scores are not directly comparable to the existing square-speed MAPE of 209 radial SPARC bins.

Noise estimates from source-free channels or regions are empirical estimates with masks recorded, not the complete calibration/covariance model. Beam-correlated samples and filtered channel noise are not independent. Preserve finite negative observations; impose nonnegativity on latent emission rather than clipping the data. Unknown velocity values and missing directions are not zero velocities/current.

Report a family or sensitivity/profile curve over height, inclination and chosen vertical model. Exact zero face-on column error for several heights is an explicit nonidentifiability control. A modest best-fit improvement alone does not establish recovered height. If train/validation splitting is used, separate blocks beyond a declared beam/channel correlation scale; otherwise label it an internal diagnostic, not independent physical validation.

## Claim register and falsifiers

| Claim | Disposition now | Required check |
|---|---|---|
| Current code already fits 3D density | Counterexample | Existing height-independent projection and scalar-only inverse contradict it |
| New finite family can simulate acquired projected observables | Conditional implementation target | Independent synthetic projection and recovery fixtures |
| 3D state can be inferred without complete direct 3D measurements | Supported as an inverse formulation | Preserve declared prior/family and unresolved alternatives |
| Matching projection uniquely recovers 3D state | Counterexample in general | Distinct normalized heights with identical face-on columns |
| HI fit recovers total galactic mass distribution | Missing support | Separate stellar/core/background inputs and mass constraints |
| Snapshot fit constitutes native AOD time evolution | Missing binding | Actual native support, current operands, committed histories and window maps |
| Missing dimensions explain a quantified share of old MAPE | Not checked | Matched same-target ablation with controlled projection and all data dependencies |

Meaningful implementation checks should include: (1) an analytic face-on normalized column and finite tail balance; (2) known inclined asymmetric geometry with correct sky handedness/approaching–receding signs; (3) total mass/flux conservation through projection and normalized beam response, including cropped/padded loss; (4) ascending/descending spectral-axis equivalence; (5) corrupted units/NaN/negative-noise handling; (6) recovery of a known synthetic member from perturbed starting parameters; (7) equal-projection different-height degeneracy; and (8) retained residuals/fit failures when a deliberately incompatible observation is supplied. These are proposed obligations, not executed tests in this review.

This review inspected the named source portions and current input reconstruction code. It did not audit the entire AOD manuscript, run the real-data fit, change scientific source, validate a new transition law, or certify physical accuracy. Main/Manual edits are outside this review's scope.
