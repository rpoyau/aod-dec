# Routing controller and observation-map review

13 November 2026. Bounded read-only review of controller integration, coverage,
provenance, exact arithmetic, backend timing scope and required-GPU behavior.
This record does not certify the CUDA device, full source manuscripts, a
physical proton-proton model, or the independent route-admission implementation.
The probabilistic law remains an application input; backend execution does not
supply its physical interpretation.

## Bound final files

| File | SHA-256 |
|---|---|
| `scripts/simulate_routing_parallel.py` | `9caf9423fadc4c2f08298a245bd8791d45d8b67f85d3a332bac17307b4e42916` |
| `simulations/wave_observables.py` | `97d257b0a3fa0465b81d9d2b76c680e06d41b21882f4d7b850d36cb8f7d364e8` |
| `tests/test_routing_parallel.py` | `02b3672a5dc75c3ebceb6bbb03eba58a7a1e0bd792bd4b9894e5c2c7cb0fcd41` |

## Findings and disposition

1. The initial retained comparison zipped backend rows with the old fixture
   without checking backend path length. A truncated path could therefore be
   counted as a complete comparison. Root added `validate_rows` before joining
   records; the hostile truncation regression now passes.
2. The prior `results.json` supplies the expected decoded cohort hash and was
   missing from the study source inventory. Root added this consumed provenance
   input to `source_hashes`; the cohort itself was already bound.
3. The fresh NVRTC PTX file was initially written without an artifact-list
   entry. Root added a conditional artifact record and CI `*.ptx` retention.
   Compilation remains distinct from actual GPU execution.
4. `--gpu required` persists a failed study when device execution is unavailable.
   `--gpu auto` can complete the CPU work while retaining GPU `UNAVAILABLE`, with
   `gpu_executed=false`; neither path relabels CPU output as GPU execution.
5. Scalar and spawned host trace work materializes and validates its own full
   return/preparation/route records. Canonical result digests exclude worker
   PIDs. The observed equality is a reproducibility check of this fixture,
   not empirical validation or an independent proof of the producer.
6. Candidate table preparation is separate from execution. The candidate pool
   reports spawn/IPC/merge/shutdown timing; full trace timing includes generation,
   materialization, admission and canonical encoding. Final compression and
   publication output are explicitly excluded. A single paired timing does not
   support a general or cost-inclusive speedup claim.
7. The SI interface requires explicit positive exact time, phase-cycles and
   phase-speed operands with matching scope and provenance. Its dimensional
   relations are `elapsed = separation * time_unit`, `f = cycles / elapsed`,
   `lambda = phase_speed / f`. Missing maps retain unknown wavelength; target
   residual and physical proton correspondence remain unset. Octal support
   coordinates and beat count have no inferred frequency-doubling role.
8. Minor input-consistency note: an uninstantiated wavelength report accepts a
   whitespace-only `native_scope`, whereas the map constructor rejects it. This
   does not affect the actual bound nonempty fixture scopes. Parent was notified;
   this review makes no claim that arbitrary blank scopes were repaired.

## Executed checks

`python3 -m unittest tests.test_routing_parallel -v`: six tests PASS (16.333 s).
`python3 -O -m unittest tests.test_routing_parallel -v`: six tests PASS (16.375 s).

Coverage: all 256 archived primary paths and all 2,048 transitions compare the
supplied tickets and seven exact output integers; truncated histories fail;
missing GPU never dispatches; required/auto modes preserve their distinct
failure/availability meanings; three complete scalar/spawned witness and route
materializations produce matching canonical records. GPU-unavailability CLI
checks use explicit mocks, not a device execution claim. Actual spawned trace
materialization is executed. Whitespace checks pass. No runner or observation
implementation was edited by this reviewer; only the six regression tests were
added during this review.

## Fresh-trace join and final input-consistency addendum

Root added `trace_candidate_comparison`, joining every materialized source trace
with its candidate seed, initial state, ticket denominator/window, supplied
movement/flip tickets and seven output fields. This closes the prior-cohort
coverage boundary for fresh traced seeds beyond 255 without claiming that those
seeds belong to the historical cohort. Two added tests check a valid seed-300
source path and reject independent seed, sign and ticket mutations. They use the
actual certified source path in a minimal wrapper; route admission remains
covered by the separate full materialization check.

Root also repaired item 8: `wavelength_report` now rejects a whitespace-only
native scope. A direct hostile check of that missing-map input passed. The prior
review hashes and test counts above are historical; the final bindings are:

| File | SHA-256 |
|---|---|
| `scripts/simulate_routing_parallel.py` | `2e0634b623d54520a7e6c48f12fd537c7635e5e983a2dbb1b21bb5839072fcd9` |
| `simulations/wave_observables.py` | `35b11af059931802e49dd3ba56530d496a1e470a5b07d4405f846734205c9c03` |
| `tests/test_routing_parallel.py` | `59d0e94c1ba7357a41aba3228cff4817be57ab5bc98042a3bd0843334a250d8a` |

The complete controller suite now passes eight tests normally (16.686 s) and
under `python3 -O` (16.470 s). The earlier full-cohort, truncation, GPU-availability
and spawned full-witness checks remain included. The two new tests were the only
additional code authored by this reviewer; the implementation repairs were
performed by root. No GPU-device execution or physical calibration is inferred.
