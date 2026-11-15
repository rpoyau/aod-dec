# R10-01: report the admitted routed current

The R10 finding is confirmed at `1edf97b710c0b1622db746ccd8b8638415a7e210`. The solar inspector reconstructed a valid current-zero channel, matched it to its committed archive, then published the original fixture constant 2. The one-line repair publishes the reconstructed `channel.value` after all existing admission, pending-surplus, flip and coverage checks.

## Exact counterexample and correction

| Reopened archive | Original cadence | Alternate lawful cadence |
|---|---|---|
| Route completion cuts | 27,30,33 | 27,33,39 |
| Receiver completion cuts | 27,30,33 | 24,27,30,33,36,39 |
| Comparison lag | 1 | 0 |
| Reconstructed and committed current | 2 | 0 |
| Before repair: reported current | 2 | 2 |
| After repair: reported current | 2 | 0 |
| Status / coverage | verified / complete | verified / complete |

The unmodified supplied probe ran before and after the repair under normal and optimized Python. Every other semantic report field is identical. All four control archives have identical bytes; all four alternate archives likewise have identical bytes. `archive-replicas.json` maps those runs to the two retained compressed archives, and `ARTIFACTS.json` binds their uncompressed bytes. The changed result therefore comes from reporting, not changed inputs.

Load 27, cost 24, unsigned surplus 3 and its unresolved signed projection remain separate from the routed current. The actual source pivot, folded histories, two whole-return flips, current unit and original `[4,-3,0,7,6]` boundary sequence are preserved. Missing, conflicting, partial and budget-limited evidence retains its existing rejection behavior.

The regression constructs and reopens genuine checksummed TraceStores. It establishes independent expected currents 2 and 0, checks the committed channel, native returns, load/cost/surplus, pending obligation and flips, and only then checks the inspector result. `regression-before-normal.log` records its expected failure before the production repair. No inspector, decoder or comparator is replaced with a canned result.

## Validation and review coverage

- Each Python mode: 89 affected tests, 87 passes and two actual-device skips (`CUDA_ERROR_NO_DEVICE`). Existing hostile and partial-coverage tests pass.
- The supplied probe passes both positive and alternate cases in both modes after repair.
- Fourteen manuscripts pass the pinned aod-lint; the updated two-page Typst companion builds and page 2 was visually inspected.
- Full supported CI is recorded on PR #7 after push. This local repair run makes no new actual-GPU execution or performance claim.

The independent source review and exact completion-cut calculation are in `independent-review.md/json`. The supplied feedback and broader review remain under `review/`; their claims retain their original scope. Main/Manual remain bound to `ab8359209b8f9b2ef4007403101016e3ea58cfe4` and were not edited.

The nonblocking Tau-naming note is retained for future public normalization work: `M/pi = 2(M/tau)`. This repair changes no coefficient name or normalization.

## Reproduce from the repository root

```sh
PYTHONPATH=. python -B evidence/r10-feedback/probe_solar_current.py build/r10-normal
PYTHONPATH=. python -B -O evidence/r10-feedback/probe_solar_current.py build/r10-optimized
python -B -m unittest tests.test_solar_boundary_fixture -v
python -B -O -m unittest tests.test_solar_boundary_fixture -v
```

Use fresh output directories. The supplied probe is byte-identical to the bundle; `PYTHONPATH` supplies the reviewed repository without changing its original import bootstrap. Archived SQLite files are deterministic gzip; decompress one as `trace.sqlite3` in a directory and reopen it through `TraceStore.open` for direct inspection. [Updated Typst companion](parallel-boundary-tracing.pdf).
