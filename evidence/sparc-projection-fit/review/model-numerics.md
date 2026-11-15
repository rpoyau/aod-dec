# Independent numerical review of the 3D projection fit

Scope: the finite observational HI reconstruction in `simulations/sparc_projection_fit.py`, its independent test module, and a focused inspection of the current driver. This review does not certify native galactic time evolution or an independently predicted rotation curve. Actual source hashes are in `model-numerics-sources.json`; earlier model scope and Main/Manual anchors remain in `model-scope.md`.

## Findings and repair verification

**N1 — Incoming beam flux was lost at the rendered window (closed).** The initial implementation evaluated intrinsic density only inside the requested image and then zero-padded the convolution. A beam therefore could not bring emission from just outside that window into it. An independent same-galaxy comparison between a small requested grid and the matching crop of a larger grid failed in every tested basis pixel, with a maximum relative difference of 0.76136052 in this deliberately adverse small window. This is a numerical/window counterexample, not the size of the real-galaxy error. The repaired `projected_basis` density-halo and final-crop blocks, evaluates the density on a six-beam-sigma halo before convolution and crops afterward. The same control now passes at floating-point tolerance. Affine extension is confined to the halo; the original interior coordinates remain intact.

**N2 — A zero total observational velocity weight caused an unclassified arithmetic failure (closed).** Finite negative intensity is admissible observational noise, but if every selected velocity pixel has nonpositive intensity the declared positive-intensity velocity loss has no weight. The original code divided by zero. The repair checks finite intensity on both relevant masks and rejects zero total positive velocity weight explicitly (`fit_at_geometry` selected-input and positive-weight checks). Rejection controls now pass normally and under optimized Python.

The before-fix log and source binding are preserved as `projection-controls-before-fixes.log` and `projection-before-fixes.json`. No implementation edits were made by this reviewer; the parent applied the two repairs after the counterexamples were sent.

## Executed controls

All **11 tests pass** in normal Python and under `-O`. Latest final logs: `projection-final-normal.log`, `projection-final-optimized.log` (parent executed; reviewer inspected). Earlier reviewer-executed normal/optimized logs remain as `projection-controls-normal.log`, `projection-controls-optimized.log`. Runtime used Python 3.12.14 and the task-local NumPy installation. Tests use a manual trapezoidal sum and do not require NumPy 2-only integration APIs.

1. **Cylindrical radial measure:** independently derived hat integrals for uniform and nonuniform knots check the required `2*pi*r dr` measure. The last radial node is fixed to zero in this API and is not a separate fitted basis.
2. **Volumetric mass and tails:** numerical radial integration combined with the analytic exponential vertical integral checks prescribed mass; a finite positive tail remains above 20 heights, while the declared radial endpoint remains compact support.
3. **Face-on nonidentifiability:** three distinct heights produce the same independently calculated column and zero projected circular current. The control establishes that an exact projection fit need not identify height.
4. **Inclined mass/second moments:** a known triangular radial profile has `E[R²]=3a²/10`. Projection gives `E[X²]=E[R²]/2` and `E[Y²]=cos²(i)E[R²]/2+2h²sin²(i)` before adding the beam covariance. Numerical projection matches these independent expectations at stated finite-grid tolerances.
5. **Beam normalization and crop loss:** a central impulse retains its total, centroid and covariance; an edge impulse loses outgoing flux without wrapping it into the opposite edge.
6. **Incoming beam contribution:** a small-window projection equals the matching crop of a full-window projection after the repair.
7. **Constrained optimizers:** simplex and box least-squares results match independent analytic interior and boundary optima.
8. **Zero velocity weight:** nonpositive-only intensity cannot create an apparently valid weighted velocity score.
9. **Nonfinite weight on a different mask:** a NaN outside the intensity fit mask but inside the velocity mask is rejected.
10. **Incompatible observed support:** positive observations outside the compact face-on model remain in the score and denominator-floor count rather than being dropped as model support disappears.
11. **Synthetic forward recovery/missingness:** independently calculated inclined thin-limit intensity and mean velocity recover known coefficients and forward observables; arbitrary excluded values do not change the result. Selected NaNs reject. This is forward-model recovery, not evidence of unique depth or physical law.

## Fit interpretation and numerical obligations

The implementation has an actual nonnegative three-dimensional mass-density family and a LOS integration with normalized exponential vertical support. Mean circular current is beam-convolved before division by intensity. Intrinsic coefficients retain fixed normalization; cropped model images are not renormalized to hide missing flux. `volume_density` uses kpc coordinates to return solar masses per cubic kpc.

The driver fits moment-0 and moment-1 maps. It does not fit full spectral shapes, line widths or multi-component profiles from the acquired cubes. The fixed observational masks are explicit. A fitted velocity mean is not a measured full three-component velocity field. Axisymmetry, compact radial support, constant height and circular streaming are declared reconstruction choices.

The fit is two-stage: density weights minimize intensity loss at each geometry, then velocity parameters minimize their declared weighted loss. The geometry search compares a combined score. Its best candidate is not certified as a joint global optimum, and the density subproblem's KKT audit does not establish one. The driver now records this distinction.

Laguerre quadrature and spatial sampling are independent numerical approximations. The driver now contains separate same-coefficient higher-quadrature and nested half-stride checks, plus a separately identified higher-order refit. Their real-data numerical values were not available at this review's completion and must be inspected before claiming an adequately converged result. Large changes require refinement or a restricted numerical claim; a passing synthetic test alone cannot bound every actual candidate.

Full source-footprint flux and computational-window flux are distinct. Reported deficits combine out-of-grid support with quadrature/pixel-area error and must not be called measured lost physical mass. A fixed catalog HI mass is conditional normalization, not an uncertainty-free measurement of total galactic mass. The source products contain their own mask/calibration choices; an approximately five-percent catalog/footprint difference does not identify its cause as missing vertical structure. The primary [NGC3198 analysis](https://arxiv.org/html/1304.4232v1) describes masking in section 3 and separate full-cube modelling in section 4; no source parameter is adopted here as truth.

No unresolved defect was found in this bounded projection/numerical review after the two repairs. Remaining limits are empirical fit quality, actual numerical-refinement results, geometry/model nonuniqueness, incomplete component coverage and unavailable temporal/native histories. These delimit interpretation; they do not prevent executing the authorized inverse reconstruction.

## Final radial-accounting amendment

The final source adds `radial_enclosed_fractions`, which analytically integrates each cylindrical hat up to a requested radius and divides by its full analytic area. I checked its piecewise integration and the independent expected fractions `[1/2,1/24]` at radius 1/2, `[1,1/3]` at radius 1, and `[1,1]` beyond support for knots `[0,1,2]`. These checks were added to the existing area test and pass in both final logs. Sharing a radial search index between intensity and current accumulation changes no mathematical operand.

The driver implements the reviewed 99.5%-concentration inner mesh plus two outer support knots. Dense inner R–z/Cartesian views accompany the full parametric representation. Radial enclosed mass is evaluated analytically, rather than estimated from a visualization grid. For a constant separable height, the joint radial/vertical retained fraction is exactly the product of radial enclosed fraction and `1-exp(-6)` for the displayed vertical band. Radial and vertical outside masses overlap; the output correctly marks that they must not be added. This accounts for the cylindrical radial view. The Cartesian square also samples corner radii larger than the cylinder and must not be assigned that cylinder's mass without an explicit radial mask or label.

The actual run uses quadrature 16 for geometry search followed by a same-state 32-point comparison and a separate 32-point coefficient refit. The finer coefficient refit is not a 32-point geometry optimization. Actual sensitivity values must therefore qualify any selected-height claim. If quadrature changes the geometry ranking or shifts the forward observables materially relative to the residual, search refinement at the higher order is needed; a coefficient-only refit does not establish geometric convergence. The same distinction applies to stride-four search versus the retained stride-two spatial check. No numerical-refinement result is inferred from these planned operations.

## Completed actual-run check

Both actual results have now been inspected. See `actual-fit-review.md` and `actual-fit-audit.json` for quantified refinement changes, active bounds, the NGC3198 velocity tradeoff and mass/accounting qualifications. The earlier pending statements above describe the staged review before those outputs existed.
