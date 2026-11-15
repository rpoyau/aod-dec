# Revised source mapping: beats, native spans and data reconstruction

This supplements and preserves `SOURCE_MODEL.md/json`. It responds to the owner's correction: use the existing AOD calculus, infer the missing field data from galaxy size/shape/distribution/density constraints, and calculate native spans from the resulting retained histories. The implementation should not ask the owner to choose an arbitrary beat period or invent a separate probability-based engine.

## Correction to the earlier conclusion

The earlier broad phrase “missing galaxy evolution law” was not the most useful task framing. AOD already provides the retained-support, closure, recurrence, directional RCD, pressure, SADAR and current calculus. The concrete next work is to construct the source-bound galaxy input and observation adapters, then execute that calculus while tracing the actual events. The text's general U_B interface does not by itself justify stopping the reconstruction work or asking for a replacement law.

The owner's present instruction authorizes fitting the missing 3D field state to known data. That is more specific than requiring all inputs to be directly measured. The Manual expressly permits such separately recorded proxy/inferred components. Fit uncertainty and nonunique reconstructions must remain visible; they are data limitations rather than an automatic prohibition on calculation.

## What is calculated directly

Main `sections/04_cut_running_fractal_tesseract.tex:75–114` gives return completions k_0<…<k_m, immutable return identities, exact beat counts, and Sep_j=k_(j+1)-k_j. These are computed from admitted traces. Cadence can be jittered. There is no need to ask for a scalar period.

For adjacent certified returns, the local recurrence segment is (k_j,k_(j+1)]; its span is Sep_j. For a whole recorded lineage, the reclosure span is k_m-k_0. The window (k_0,k_m] counts its m reclosures. If a report includes the origin closure, the integer window (k_0-1,k_m] includes it through Main's explicit origin indicator. These interval formulas are direct report constructions from the source event convention, not new physical laws. Different coupled lineages are compared only over a common committed domain with their repeated contact witnesses; the last data cut remains a real boundary with pending returns carried forward.

RD is calculated independently from first-branch support + hinge recurrence + reflected branch support (`sections/05_curl_closure_duon_current.tex:69–88`). RCD then binds directional reflections and contact histories; the declared additive realization gives RD + sum of reflection counts (`sections/06_wave_state_relational_time.tex:53–98`). A compatible window clips scalar participation without shortening the stored RD or RCD. Thus beat span, RD and clipped participation are computed quantities with different operands.

At larger scopes, actual fold witnesses bind the inner identities and actual outer return. One outer certified return contributes one outer closure. Inner beats are not multiplied or renamed into outer beats (`sections/05_curl_closure_duon_current.tex:227–290`). Native range therefore follows retained addresses, actual sections and actual admitted folds, rather than a numerical octave ladder attached to a physical radius.

Maximum supported field count is already given by Main: min(candidate count, support count, floor(raw capacity / per-field capacity), floor(window / per-field duration)). Support count is the retained section count times support-expression cardinality (`sections/06_field.tex:1131–1181`). The native ternary assignment capacity C3(k)=k counts k unconstrained retained positions (`sections/03_afc_basis.tex:145–159`). Mass is not a count in this identity.

## Concrete data-to-calculus algorithm

1. Bind galaxy distance, inclination/orientation, projected photometry, gas distribution, physical extent and available kinematics to actual source rows. Convert units consistently. Preserve radial/angular resolution and uncertainties instead of selecting a guessed physical cell size.
2. Reconstruct positive 3D star/gas/core/background distributions constrained by their observed 2D projections and known shape/mass relations. Fit unresolved thickness/shape or extrapolated tails jointly with their observational constraints. Carry multiple compatible reconstructions or parameter intervals when projection does not identify depth. If the rotation-speed target is itself used in this fit, label its later residual as an in-sample reconstruction comparison.
3. Calculate geometric cell volumes, integrated mass and density from that reconstruction. Bind those quantities through the chosen measured-data/native-support adapter to actual retained support expressions and identities. Record which mappings are already implemented and which are inferred/calibrated. A high-density cell may require more retained participants; its numerical mass does not itself certify those participants or equal ternary capacity.
4. Execute the existing AOD construction/admission/current calculus. Detect certified returns, compute the ordered beat separations, derive local recurrence spans and native range from the event histories, and derive outer spans only after actual outer returns. Field window reports follow these events; data cutoffs and finite compute cutoffs retain pending/open support instead of forcing artificial closure.
5. For each derived window compute RD/RCD, clipped participation, pending burden, pressure/SADAR and crossing−sweep+shedding. Boundaries follow the retained reconstructed region and its evolved support; crossed, shed, decoupled and recoupled paths remain linked even outside a current reporting window.
6. Project the reconstructed/evolved state through the actual observation geometry and compare the same 209 SPARC radial targets. Report physical decade spans, native retained address ranges, section grades, fold depth, native cut spans and cadence statistics separately. Reconstruction fit, input uncertainty, conservation residual and prediction discrepancy are separate outputs.

Steps 1–3 are the authorized data reconstruction. Steps 4–5 must use actual returned-history evidence from the existing engine, not generate closure counts by turning kpc or solar masses into arbitrary integers. Nothing here requires inventing a new stochastic evolution law.

## Existing mass/size and SI hooks

The specific mass/size hook occurs in Manual Appendix C (`manual/appendices/C_3d_coordinate_phase_cycle_delta3_fixture.tex:22–46,83–104`). It allows mass/size data to supply positive integer beta_a and evaluates squared integer displacement Q and phase Q mod beta_a. The text explicitly requires the acquisition adapter to provide that mass/size-to-beta map. Its current producer `manual/scripts/aod_3d_coordinate_phase_delta3.py:23–25` uses fixed beta=5 and lag=1; there is no measured mass/size evaluator in that script. This is a precise adapter gap, not an absence of AOD phase calculations.

Cell weights and physical geometry have finite measure interfaces in Main `sections/07_cell_measures_mappings.tex:7–32`. SI time has the history→cycle-equivalent→seconds map in Manual `sections/03_m3_temporal.tex:30–60`. The existing G0 U mapping supplies explicit (km/s)/kpc units. These hooks can be supplied from fitted data and calibration. The reviewed source does not provide a universal mass→beat, mass→C3 or octal-digit→metres equation; treating one of those as already derived would silently add a premise.

## Exact source example

The Manual's bound returns [2,10,18,26] and [2,12,22] give cadences [8,8,8] and [10,10]. Their common recorded reclosure window (2,26] spans 24 cuts and counts 3 versus 2 reclosures. The cadence ratio is 3/2. A hypothetical 40-cut least-common-multiple period is not substituted for the recorded window. The example separately has directional durations 5 and 7, demonstrating why cadence and RD/RCD cannot be collapsed into one count.

## Source binding and coverage

Bound Main/Manual commit: `ab8359209b8f9b2ef4007403101016e3ea58cfe4`. `BEAT_SPAN.json` records source hashes and exact anchors. Reviewed Main closure, curl/support/folding, directional time, field/range/capacity, weighted measures, awareness/carriage and combinatorial support; reviewed the coordinate mass/size fixture and its actual producer. No manuscript or remote changes were made. The density-fitting algorithm above is an implementation inference under the newly authorized reconstruction scope; it is not represented as a verbatim Main theorem.
