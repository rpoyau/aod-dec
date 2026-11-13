# Manual error and SI-map audit

Date: 2026-11-13. Scope: read-only, source-bound audit of the tesseract-cell application's error reporting and its path to an SI comparison. No upstream manuscript or simulation implementation was changed by this audit.

## Source and coverage

- Original execution source: `AOD_v40_01r10_source.zip`, SHA-256 `a178a5bf06d51c8ab8852e9cabfb6c5eb61456f70739d7391582c08c524f34c7`. Its Manual Scope already distinguishes exact integers/fractions, declared decimal value maps, percent presentation against a declared target, and data-support classes.
- Later delivered Manual I candidate: `/home/reginaldpoyau/projects/aod/AOD_M3_MANUALI_FEEDBACK_2026-11-11/aod`, clean tracked source at commit `8c3149b9d3d924b80b9385b178da4ba2c39175e8`, tree `b2b55cbcf18354828f7b34418ab3fa26d747a927`. This is a delivered local candidate, not a claim that it is the accepted remote head.
- Active driver `manual/main.tex` includes Scope, D.E.C., mapping/error, temporal/metrology, and registry sources at lines 56, 59, 61, 62, and 70.
- Inspected candidate sources: `manual/sections/00_scope.tex`, `00_dec_ledger.tex`, `02_m3_mapping_error.tex`, `03_m3_temporal.tex`, data-support portion of `06_field_dynamics_applications.tex`, and `09_prediction_test_fixture_registry.tex`; `ERROR_BUDGET.json`; relevant validator/reduction functions and primary-source receipts. This is not a full manuscript or physical-model review.
- Inspected application source and retained results at `aod-dec` commit `89a3653cca528963297fae73bd2aa6869df73e23`. File hashes and numerical countercheck are in `error-si-binding.json`.

## Findings and minimal repairs

### ESI-01 — The aggregate percentage does not quantify boundary-output accuracy

Disposition: **supported reporting defect**, even though its arithmetic is a valid declared diagnostic.

The application runner `scripts/simulate_cell_jitter.py:114–128` evaluates

`D_L1 = sum_i |Euler_i - Decimal_reference_i| / (initial_mass + injected_mass)`.

The twelve coordinates include emitter, eight storage cells, output, loss, and the injection accumulator. Its denominator is total supplied mass, not the target value of every observable. It is neither a Markov-kernel norm nor a path-history metric nor an empirical error. Calling it a generic accuracy percentage conflates those tasks.

Concrete retained-result countercheck, amplitude `1/4`, seed `7`:

| Resolution | Normalized terminal L1 diagnostic | Exact output | Signed output-relative residual against the continuous reference |
|---|---|---|---|
| 1 substep | `0.0349599721363538208…` | `1419/1048576` | `-0.790678894061851497…` |
| 32 substeps | `0.00101139052479777024…` | exact rational retained in JSON | `-0.0255835575871483467…` |

The reference output is approximately `0.006465013878630755651…` in the declared model coordinate. Thus the previous `3.50% → 0.101%` headline does not imply those errors for output. The output-relative display would instead be approximately `-79.1% → -2.56%`; it too is only a numerical comparison against a supplied reference, not physical uncertainty.

Repair: retain exact rational states, define one named report map for the aggregate, and report each requested output with its own comparator, units/kind, signed residual, denominator, and display convention. Use fraction-based internal records. Keep the reference's finite Decimal precision and unbounded accumulated rounding distinct from the exact Euler arithmetic. Avoid an accuracy percentage in the headline. If percent presentation is retained, identify its exact denominator and its declared numerical comparator every time.

Source: `00_scope.tex:108–156` (`manual:exact-arithmetic-presentation`, `eq:frg-delta-three-components`); `00_scope.tex:171–198` (`manual:scoped-errors-data-support`); `02_m3_mapping_error.tex:135–172`.

### ESI-02 — Exact fixture residuals, data-support classes, and error components are different classifications

Disposition: **missing application records**.

- Exact integer fixtures use `Delta_Z = T - O`, its ternary sign, and integer magnitude (`delta_3`), with declared comparator bins. Rational identities should retain exact rational residuals; do not silently relabel arbitrary rational masses as integer-event bins.
- D0 denotes exact internal fixture support. O0 requires an observable-map value and measured-sector target. G0/G1/G2/G3 describe supplied radial/proxy/3D/time-ordered support. A tesseract or time index alone does not instantiate a G2/G3 physical dataset. The present application is a declared finite transport model with exact internal checks and a separate numerical reference study; no measured O0 maser comparison has been executed.
- Each named output requires the eight separate components: arithmetic, resolution, window, model discrepancy, calibration, propagation, solver, comparison. Component kinds are exact, bound, standard uncertainty, unknown, and not applicable. Unknown/inapplicable have reasons and no numerical substitute.

The rational Euler step can have exact arithmetic while discretization error is nonzero. The Taylor tail is a bound for its exact series on represented inputs; it excludes Decimal rounding and prior-interval propagation. The 60/80-digit comparison is a diagnostic, not a certified full-trajectory uncertainty bound. The ensemble range of 16 seeded paths is neither a standard uncertainty nor a confidence interval. No supplied physical discrepancy/calibration evidence means those physical components remain unknown.

Repair: add a simulation data card and per-output budget with these distinctions. Classify the current reference comparisons as conditional numerical diagnostics; do not insert observed errors into the budget as proven bounds, and do not replace unknown physical components by zero. Keep exact conservation identities as their own check rows.

Source: `00_scope.tex:117–156,171–224`; `06_field_dynamics_applications.tex:29–60`; `02_m3_mapping_error.tex:135–145`; `contracts/scientific/ERROR_BUDGET.json`; `aod_mapping_error.py:316–381`.

### ESI-03 — D.E.C. is the ledger around an admitted raw run, not a new transition law

Disposition: **conditional finite model is supported; full Manual application mapping is missing**.

Manual sequence: declare fields/ancestry/window/boundary/update → admit source-bound inputs → compute while retaining dynamical state → map through observation/clock/calibration → join frozen comparison and shared-dependency audit. D.E.C. specifically declares admissible edge slots and integer weights, computes exact normalized probabilities, retains route status, and then forms the oriented current/SADAR/boundary audit. The Manual's example uses `(1,1,4,2)/8` and, after a declared hinge block, a separately declared renormalized slide pool. Stochastic weights are model choices, not automatic physical probabilities.

The current facet-dual transport matrix is a valid declared downstream kernel if its state set, row convention, mass/probability distinction, source term, and preservation are explicit. It does not by itself construct admitted waves, RCD, SADAR, or a target-frequency map. Normalizing an aggregate mass vector also loses identity information unless histories are retained separately. No normalized L1 endpoint diagnostic establishes history fidelity.

Repair: write the calculation card and exact row schema; explicitly identify the limited interface implemented and what is uninstantiated. Preserve the scalar transport model as a comparison fixture unless the required AOD formation/history constructors are actually added. Do not label symmetric pair exchange as an RCD law.

Source: `00_scope.tex:16–52,247–257`; `00_dec_ledger.tex:4–67,94–108,128–156,197–285`; `03_m3_temporal.tex:17–28`.

### ESI-04 — SI units require an observation map and real target identity

Disposition: **missing premise for physical prediction**, not a reason to omit source-bound target/reduction examples.

The Manual's native-to-observer bridge is

`C_Cs_hat = g_theta(reference history, window, population, couplings, preparation)`;
`T_hat = C_Cs_hat / 9192631770 Hz`.

The exact Cs defining integer supplies a unit conversion after corrected Cs-equivalent cycles have been established. It does not assign seconds to each cut. For compared/reference phase advances, retain integer winding and endpoint fractions: `A=C+phi1-phi0`. In the Manual fixture `A=7/2`, `A_R=17/2`; the conditional report is `T=17/(2 f_Cs)` seconds and `nu_bar=7 f_Cs/17` hertz. Matching coordinate/epoch/window/orientation and a positive corrected reference advance are required.

Concrete maser target/reduction available in the bound candidate:

- Primary source: Hellwig et al. 1970, `https://tf.nist.gov/general/pdf/13.pdf`, archived SHA-256 `e8bba435fbb9e6b54f41fa1d60f9c02df8e06b6a73ce8fab323afb3ea7d02dc4`.
- Measured corrected target token: `1420405751.768` Hz, reported one-sigma uncertainty token `0.002` Hz. Preserve the quoted category and shared NBS-III reference; do not infer a probability law or independence.
- The published-input reduction is `nu_H = 284 nu_R + nu_S + nu_B + delta_UTC + delta_N + delta_W + delta_D + delta_Z`. Table VI already includes NP3/wall corrections, so applying them again is wrong.
- Exact printed-input replay gives Table IV `14204057517691/10000` Hz, Table VI `14204057517667/10000` Hz, and their mean `14204057517679/10000` Hz. This differs from the published rounded mean token and must not replace it silently.
- The candidate's source-authenticated reduction functions were executed in this audit and reproduced those exact values. Their status remains `measured_input_reduction`, with `independent_atomic_prediction=false` and missing raw phase records.

For a new cell-model comparison, freeze a physically specified state/preparation/emission/observation map and its model parameters without evaluation leakage. Map histories/phase/cycles to the target observable in Hz (or independently map energy/count/area/time to an output power or flux). Then join a matching source-bound target and compute signed residual and any declared relative report. A joint standardized residual requires both sides' uncertainties and covariance; a target-only pull is separately named. Establish per-output budgets, correction ownership, source roles, and predeclared acceptance tolerance. The present reservoir output coordinate does not supply oscillator phase, a transition frequency, detector response, physical density, energy conversion, or a realization clock, so no Hz/W/SI-density prediction exists yet.

Source: `03_m3_temporal.tex:17–78,120–126,154–202,204–238,240–305`; `aod_m3_metrology.py:258–284,322–381`; primary source receipts. The source figures above are bound historical transcriptions. A new live metrology claim should verify the official primary sources separately.

## Executed checks and remaining review

- Verified candidate Git commit/tree and clean tracked source.
- Recomputed aggregate-versus-output diagnostic from retained application JSON using exact `Fraction` parsing and 60-digit Decimal arithmetic for its numerical reference.
- Executed candidate `load_primary_receipt` for hydrogen and cesium, hydrogen reductions for both tables and published mean, and the August 2013 cesium reduction. These authenticate/reproduce the bound source reductions, not physical model prediction.
- Source repair was not applied. Review of field-population/RCD construction, full path-history identity, and implementation of repairs belongs to the other task units. Full source scientific validity, raw apparatus data, and physical accuracy are not checked here.
