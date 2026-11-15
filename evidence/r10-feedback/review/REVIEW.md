# AOD Dec PR #7 — R10 updated review

**One P2 reporting defect remains at `1edf97b710c0b1622db746ccd8b8638415a7e210`: the solar-role inspector can verify a current-zero archive and report current two. Repair that query result before accepting this revision. No additional functional defect was established in the other covered passes.**

PR #6 remains at the previously accepted `dbf79dcc14d5a36a68969212c617bb8d486fee2d`. The update is [PR #7](https://github.com/rpoyau/aod-dec/pull/7), stacked on that head. Review date: 15 November 2026, Europe/Zurich. Source and GitHub were unchanged.

## R10-01 — publish the current reconstructed from the retained histories (P2)

[simulations/solar_boundary_fixture.py:460](https://github.com/rpoyau/aod-dec/blob/1edf97b710c0b1622db746ccd8b8638415a7e210/simulations/solar_boundary_fixture.py#L460) assigns `declared_routed_current=Q(2)`.

Immediately above, the inspector correctly reconstructs the route/receiver comparison, regenerates its signed channel and verifies that exact channel against the committed archive. Its returned number then substitutes the original example's constant.

| Reopened archive | Original cadence control | Alternate lawful cadence |
|---|---:|---:|
| Routed completion cuts | 27, 30, 33 | 27, 33, 39 |
| Receiver completion cuts | 27, 30, 33 | 24, 27, 30, 33, 36, 39 |
| Exact comparison lag | 1 | 0 |
| Reconstructed and committed current | 2 | **0** |
| Inspector's reported current | 2 | **2** |
| Inspector status / coverage | Verified / complete | Verified / complete |

The alternate final comparison contains one route return and two receiver returns, with the declared bias: `1 - 2 + 1 = 0`. The route retains admitted E2 support, its actual source pivot, the validated fold, load/cost 27/24, unsigned surplus 3 with its pending obligation, and the original whole-return flip witnesses. Current is committed at cut 40 after all required returns.

Both counterexample archives are genuine checksummed TraceStores, closed and reopened through the actual inspector. No comparison or decoder is replaced. Normal and optimized Python reproduce the error; the parent integration pass repeated both runs independently.

**Impact is specific:** the inspected numeric summary is false for an admitted retained cadence. The original fixed fixture's current two and boundary sequence `[4,-3,0,7,6]` are not disproved. No physical solar prediction is involved.

**Smallest repair:** publish the already reconstructed and matched `channel.value`. Add a regression for the alternate lawful cadence and retain the current-two control. Preserve the separate unsigned surplus and partial full-current coverage. A deliberately fixed-preparation inspector would instead need an explicit admission restriction; it must not verify a different preparation and return the constant.

Evidence: `checks/frontier/probe_solar_current.py`, its complete archives/results, and `checks/parent-solar-*.json`. The focused report uses local label FRONTIER-01 for this same finding. See `FEEDBACK.md` for acceptance steps.

## Source binding and review coverage

The candidate introduces 418 changed files, including substantial retained evidence. The GitHub compare response lists only its first 300 files; review inventory was reconstructed from complete Git trees, so the later compiler/simulation/test paths were not omitted.

The head and tested merge `e6cb2cfc58fd7609e2bf2465024e6682150e7545` share tree `279268e64d6b2dfa87ab5df31c6bd4dc98e7a738`. Final PR refresh still points to the reviewed head. All 358 materialized blobs match Git identities; all 357 locally available checksum-listed files match. The hosted checksum step covers the full 1,025-entry manifest; this review does not claim that every historical file was independently downloaded.

Bounded passes covered CPU batching, generated CUDA/session behavior, compact field/frontier and moving-boundary tracing, original-engine awareness observation, SPARC preparation/reduction/error coordinates, and current execution evidence.

| Area | Independent result |
|---|---|
| CPU batch/dispatcher | Exact operand rejection, reference/fuel controls, stable ordering and isolated lane contexts checked; actual C++ dispatcher/runtime harness passes 725 scalar assertions |
| GPU lowering/session | 35 additional semantics, injected driver-failure and malformed-output checks pass per mode; no local CUDA execution claimed |
| Original-engine observation | 18 observer plus four host admission tests pass per mode; 27 limit controls, ten hostile rejections, three consistent renamings and nine concurrent cases pass per mode |
| Frontier/boundary integration | 88 tests per mode: 85 pass, three unavailable-library/device skips; independent rebind/pending controls and 10,000 queue operations pass; R10-01 remains |
| SPARC/accounting | 95 focused tests per mode pass; independent complete-cohort calculation and 256 moving-subset identities pass |

These counts describe different evidence classes and overlapping suite subsets. They are not added to the repository's full test total. Focused reports and reproducible probes are under `checks/`.

The original awareness source is byte-identical to independently retrieved AOD `ab8359209b8f9b2ef4007403101016e3ea58cfe4`, SHA-256 `6846300c020aca4af2c7fc51c0727dcd0ce7e302453e0a514904ce8aa5331d7b`. Actual original `learning()`/`mindfulness()` calls and typed retained histories are observed. The compiled integer specialization and rational host controls remain distinct.

## Current CI and device evidence

[Workflow 43728633037](https://github.com/rpoyau/aod-dec/actions/runs/43728633037) passes all three jobs. Raw full-suite records reconcile exactly to the current discovery inventory:

| Per Python mode | Methods |
|---|---:|
| Discovered | 771 |
| Passed | 758 |
| Individually skipped | 8 |
| Not entered because their class setup skipped | 5 |
| Failures/errors | 0 |

Unittest prints `Ran 766` and nine skip records: eight individual skips plus one class-level skip covering five methods. Subtracting nine from 766 would give the wrong pass count. All unavailable checks here depend on CUDA/NVRTC availability.

Current hosted native evidence verifies 104 source/optimization conformance records, six malformed-IR rejections and 1,979 exact runtime differential records. New compiled-boundary and introspection stages complete. Hosted GPU availability remains separate from the publisher's NVIDIA runs.

The source-bound publisher records contain 25 test methods passing per mode, including five actual-device methods; the remaining methods cover host lowering/admission/lifecycle. Recorded awareness execution has 62 GPU launches across five families, with 18 additional retained evolution calls. Source, inputs, outputs and generated CUDA bindings were checked. Current PTX hashes cross-bind records, but the current PTX bytes are not present in that evidence prefix. No fresh local GPU run is claimed.

Pinned LLVM16 is absent locally, so actual generated CPU/JIT claims come from authenticated current CI. Local C++ dispatcher tests use an explicitly described ABI harness and do not substitute for LLVM generation. These successful checks do not override the independent R10-01 counterexample.

## SPARC and measured-cost conclusions

An independent standard-library/Fraction oracle checks all 209 raw bins, 627 comparison rows, 621 complete state transitions and history links, 2,484 boundary identities, and 31,437 current/lag/pressure/window operands. Normal/optimized complete trace, CSV and summary bytes agree.

The predeclared spacing primary remains worse: equal-bin square-speed MAPE changes from **26.570553423860%** to **26.977910478548%**, an exact-before-rounding increase of **0.407357054687 percentage points**. The unit-intensity control improves the pooled coordinate but worsens the equal-galaxy coordinate. No improved physical accuracy follows from exact accounting residuals. Input/observation propagation and unknown projection/model errors remain separately identified.

Recomputed five-pair medians reproduce the published compiler reuse ratios: CPU 1.7348×/1.3657× and GPU 3.5313×/3.5252× for normal/optimized Python. The timing driver includes construction, first loads, execution, cleanup and report serialization; shared preparation and later reference comparisons are excluded from both compiled intervals. CPU disk caching is explicit. These measure repeated setup removed within the adapter, not acceleration of the original uncompiled engine. Broad-profiler versus selected-code capture and trace-off overhead are separate recorded comparisons.

A nonblocking owner-alignment note remains: the reconstruction API exposes `mass_over_pi`. A Tau-normalized field should preserve the exact normalization, with a factor-of-two conversion rather than a name-only substitution. This is not a numerical defect.

## Disposition and retained evidence

Hold acceptance only for the new verified-current reporting defect. The reviewed compiler/session, observer, frontier and SPARC results remain useful within their stated scope. R09's conclusions remain bound to unchanged PR #6; this broader PR #7 review is a separate decision.

The bundle contains reports, the concrete fix request, exact counterexample archives, independent probes/logs, source/CI identities, selected raw CI archives and publisher device records. Large full SPARC archives were inspected but are referenced by their GitHub IDs and verified digests rather than duplicated in this download. PDF layout was not independently re-reviewed here; manuscript source claims and build evidence were inspected.
