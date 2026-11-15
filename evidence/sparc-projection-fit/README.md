# Fitted three-dimensional HI projections

The application was implemented and executed for NGC2403 and NGC3198 using the acquired HALOGAS intensity and line-of-sight velocity maps. Each fitted state has an explicit three-dimensional, nonnegative density, a fixed catalog HI mass, an exponential vertical distribution, and fitted circular mean-velocity coefficients. Inclination, position angle, centre and height determine the forward projection through the measured beam. Full spectral line shapes and native AOD time evolution were not fitted.

## Evaluated results

| Galaxy | Intensity NRMSE, initial → fitted | LOS velocity RMSE, km/s | Intensity-weighted LOS RMSE, km/s |
|---|---:|---:|---:|
| NGC2403 | 0.283119 → 0.264609 | 11.178 → 10.163 | 9.315 → 7.045 |
| NGC3198 | 0.234110 → 0.191106 | 24.151 → 25.189 | 5.770 → 7.489 |

NRMSE values are dimensionless fractions. These are in-sample projection residuals, not the earlier SPARC square-speed MAPE. NGC2403 improves both reported observables. NGC3198 improves intensity and the declared combined loss while both velocity residual measures worsen. Substantial structured residuals remain. No independent-prediction or across-the-board physical-accuracy claim is made.

Both selected inclinations reach the lower limits of the declared catalog-based search intervals. The fitted heights are consequently conditional on the chosen family and bounds, not unique recovered thicknesses. NGC3198 also has one velocity coefficient at its bound and ten fixed-mask velocity pixels using the numerical denominator floor. They remain in the score and report; no successful-match claim is assigned to them.

## Density, boundaries and measured support

The radial hats use cylindrical area 2*pi*integral R*b(R)dR. Their coefficients sum to the fixed HI mass normalization. Vertical support extends over all real z. The displayed +/-6h band retains analytic upper/lower tails, and the dense radial display retains analytic outside mass. Radial and vertical outside sets overlap and must not be added twice.

The dense R–z display and its mass ledger describe a cylinder obtained by rotation about z. The Cartesian export contains point samples on an x/y square, whose corner samples extend beyond that cylinder. It is not a finite-volume mesh whose sample sum equals the analytic mass ledger.

The initial uniform radial basis used too much resolution on distant emission in NGC3198. The executed revision places dense inner knots through the measured 99.5% positive-flux concentration radius plus one beam, and retains outer knots through the furthest selected sample. No scored observations were discarded. The selected NGC3198 model places about 2.354% of its HI mass outside the dense radial view; this is an inferred allocation under the fixed mass constraint, not a measurement of halo matter. Outlying observed components remain unclassified.

The source's zero-filled moment-map exterior is not treated as measured empty physical support. Every selected nonzero finite intensity sample, including negative noise, remains in the intensity fit. A fixed positive-intensity/finite-velocity source mask defines the velocity comparison. The beam renderer includes external emission that scatters into a cropped view, with an independent nested-window test.

Catalog-implied total flux and measured source-footprint flux differ by approximately 4.68% for NGC2403 and 5.60% for NGC3198. These deficits are not attributed to missing height or a physical process. Calibration, masking, support and shared source lineage remain relevant. The source coldens products are not ingested because their BUNIT/HISTORY conflict remains explicit.

**Signed numerical diagnostic:** the raw result field named `model_flux_outside_fit_view_inside_source_frame_jy_km_s` is a difference between two sampled projections and area conventions, not an exact nonnegative physical outside-flux measurement. For NGC3198 it is approximately -0.01047 Jy km/s. This small negative difference is numerical; neither source nor model has been assigned negative physical mass. The separate grid-deficit field has the same numerical/window qualification.

## Numerical and implementation checks

Eleven projection controls and 41 observation-adapter/adjacent SPARC checks pass in both normal and optimized Python: **52 per mode**. The controls include independent analytic cylindrical masses and sky moments, face-on height degeneracy, beam conservation and incoming flux, constrained optima, invalid weights, masked support, and recovery from an independently specified synthetic thin projection. The actual density arrays are finite and nonnegative, and analytic mass partitions close to floating-point precision.

The selected state was checked with quadrature order 16 versus 32 using unchanged coefficients; the final coefficients were separately refitted at order 32. Spatial stride 4 versus 2 was checked on coincident pixels without refitting. Intensity changes are at most 0.119% in relative L2 and LOS changes at most 0.059 km/s for these specific checks, much smaller than the fitted residuals. These checks do not establish a complete physical uncertainty budget or a globally optimized higher-order geometry.

The search fits density coefficients to intensity, then velocity coefficients, and uses a bounded profile/coordinate search for geometry. It is a two-stage local inverse fit, not a proof of the joint global minimum. Optimizer iterations are not physical time. All evaluated candidates are retained in JSON and JSONL.

The real run used two NumPy CPU worker threads. No GPU or speedup result is claimed. The exact Python/NumPy versions, worker identities, elapsed time, driver/model/adapter hashes and model choices are retained in `results/summary.json` and the per-galaxy records.

## Files and reproduction

- `results/<galaxy>/results.json`: fitted parameters, alternatives, source identities, masks, metrics, numerical checks and boundary accounting.
- `results/<galaxy>/reconstruction.npz`: observed/projected maps, residual masks, fitted radial coefficients, R–z density and Cartesian 3D density samples.
- `figures/` (including central-detail views) and `sparc-projection-fit.pdf`: observed/model/residual comparisons and the Typst application companion.
- `review/`: source-bound scope, independent checks, actual output audit, test logs and manuscript lint evidence.
- `catalog-context.json` and `halogas-downloads.json`: retained catalog and public download identities. The raw FITS files remain in the separate acquisition package.

The run used:

```sh
python -B scripts/fit_sparc_projection.py --data /path/to/halogas \
  --context evidence/sparc-projection-fit/catalog-context.json \
  --output build/sparc-projection --workers 2 --stride 4 --rings 16 --quadrature 16
python -B scripts/plot_sparc_projection.py --results build/sparc-projection --output build/sparc-projection/figures
python -B -m unittest tests.test_sparc_projection_fit tests.test_sparc_observations -v
python -B -O -m unittest tests.test_sparc_projection_fit tests.test_sparc_observations -v
```

NumPy and Astropy are optional application dependencies; Matplotlib is used for plots. CI installs Astropy so the observation-adapter controls run, and builds the new Typst companion. The pinned aod-lint scan checks 15 Typst manuscripts including this companion; its scope excludes Python semantics and scientific validity. Typst compilation and rendered inspection are separate checks.

Data: [HALOGAS DR1 v2](https://zenodo.org/records/3715549), DOI10.5281/zenodo.3715549, CC BY4.0; [SPARC](https://astroweb.cwru.edu/SPARC/). The four moment maps are the actual fit inputs; acquired full cubes are not fitted here. The [ALFALFA proposal](https://egg.astro.cornell.edu/alfalfa/docs/alfalfaprop.pdf), equation1, supplies the declared nearby optically-thin HI mass/flux conversion. Input conversion does not impose SI on native AOD tracing.

DEC base:99035b0cd7bad3103e55fe0cf82fffc6b3145b8d. Main/Manual:ab8359209b8f9b2ef4007403101016e3ea58cfe4. AOD/AF ownership and source-bound review are recorded separately from the implementation. No Main/Manual source was changed. The original SPARC baseline/reduced scores and native compiler semantics remain unchanged.

Two preliminary runs were incomplete: one ended with external SIGTERM, and one was stopped to revise radial resolution. Their logs/status and the completed preliminary NGC2403 records remain in the external execution workspace; they are not the delivered final fits.
