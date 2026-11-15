# Actual fitted-state and numerical-refinement review

Both revised-basis runs completed. This review binds their actual results and exported density arrays in `actual-fit-audit.json`. The eleven finite-model controls pass in both Python modes; this additional check inspects the real fitted states rather than extrapolating those test results.

## Observed residual tradeoff

| Galaxy | Initial → selected intensity NRMSE, fraction | Initial → selected LOS velocity RMSE, km/s | Initial → selected intensity-weighted LOS RMSE, km/s |
|---|---:|---:|---:|
| NGC2403 | 0.28311862 → 0.26460934 | 11.178288 → 10.163312 | 9.314839 → 7.044799 |
| NGC3198 | 0.23411045 → 0.19110630 | 24.151323 → 25.188742 | 5.769635 → 7.488519 |

NGC2403 improves both displayed observables. NGC3198 improves intensity while worsening both velocity metrics; its combined objective improves because of the intensity improvement. Do not describe the latter as uniformly improved agreement. These are in-sample moment-map residuals within the declared reconstruction family. They are not SPARC square-speed MAPE, a held-out prediction, or a complete spectral-cube match.

## Tested numerical changes

| Galaxy | 16→32 quadrature: intensity relative L2 | 16→32: LOS change RMSE, km/s | Stride4→2: intensity relative L2 | Stride4→2: LOS change RMSE, km/s |
|---|---:|---:|---:|---:|
| NGC2403 | 0.000900199 | 0.0582158 | 0.000300765 | 0.0205135 |
| NGC3198 | 0.0000193672 | 0.000379714 | 0.001186635 | 0.0266837 |

Intensity entries are dimensionless fractions, not percentages. Both checks keep the compared state's coefficients fixed; a separate 32-point coefficient refit supplies the selected maps. The tested numerical changes are small compared with the retained real-data residuals. They support numerical stability of these forward observables under the tested refinements. They do not bound every quadrature/grid choice or certify that the selected geometry is the higher-order/global optimum. In particular, the search used 16-point quadrature and stride 4; it was not rerun as a geometry search at every finer setting.

## Identifiability and active bounds

Both inclination estimates reach their declared lower bound: 60° for NGC2403 and 70° for NGC3198. The corresponding fitted exponential heights, approximately 0.832773 and 0.121991kpc, remain conditional on the model family and inclination range. Neither should be reported as a unique measured thickness. The NGC3198 height subtends 1.82336arcsec, compared with a 35.2327arcsec beam major axis; its physical interpretation needs an uncertainty and degeneracy analysis beyond this fit.

NGC3198 also has one bounded velocity coefficient at its limit, approximately +443.43km/s for a small inner component, while most other fitted coefficients have the opposite sign under this PA convention. Ten selected velocity pixels require the denominator floor. These diagnostics delimit a fitted mean-velocity representation; they do not establish that counter-rotation or an intrinsic rotation curve has been physically recovered. The observed samples remain scored.

## Density and boundary accounting

The R–z arrays have shape 241×256 and the Cartesian arrays 49×65×65. They are finite and nonnegative, and their recorded SHA-256 hashes match the files. Analytic radial, vertical and combined retained-plus-outside mass partitions close to within 1.8e−16 relative; the two independent outside fractions are correctly marked as overlapping rather than additive.

NGC2403 has zero fitted mass outside its dense radial view in this selected finite representation. NGC3198 allocates 2.55849×10^8solar masses, or 0.0235393 of the fixed HI mass, outside that radial view. This is an inferred model allocation; it is not detected halo mass. The input contains unclassified distant emission, and fixed catalog normalization plus masked observational exteriors leaves room for latent outer allocations.

The full-grid NGC2403 flux deficit is −3.23517×10^−6 of the intrinsic flux: a small signed numerical discrepancy, not negative physical loss. For NGC3198 the discretized full-grid flux minus the fit-view flux is −0.01046955Jy km/s, even though a physical outside-region contribution cannot be negative. That field must be read as a signed difference between sampled projections/area conventions, with numerical discrepancy retained; it must not be presented as a physical outside flux. The parent was advised to clarify that field's label or interpretation without changing numerical outputs.

The retained radial accounting describes the cylinder R≤inner_radius. Cartesian sampling covers a square in x/y and includes corners at larger R; the cylinder's mass must not silently be assigned to that square. Exact full parametric support and tails remain available independently of the chosen display.

## Scoped disposition

The revised run demonstrates executable 3D density inference and simulated projected moment maps with preserved mass normalization and explicit residuals. The small observed numerical-refinement changes support the reported forward quantities at the displayed precision. Agreement is incomplete and model-dependent; NGC3198's velocity tradeoff, both active inclination bounds, outlying support, missing full spectral fitting and absent physical time/native histories remain explicit. No new dynamics law or independent physical accuracy claim is established by this review.
