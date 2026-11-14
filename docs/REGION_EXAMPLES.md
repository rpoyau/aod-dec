# Reusable region examples

`run_regions.py` uses the same region engine for both declared finite preparations. It computes channel values with the bound Manual helpers `participation`, `aod_flux` and `stokes`. The preparation supplies capacity, duration, window, attention, interfaces and motion. Region boundaries and their signs are derived from the complete committed channel family. No new probability or force law is supplied by the engine.

```bash
python -B simulations/run_regions.py --example all --output outputs/region-examples.json
python -B simulations/run_regions.py --example maser --output outputs/maser.json
python -B simulations/run_regions.py --example earth-water-moon --output outputs/earth-water-moon.json
```

Use Python 3.10 or later from the repository root. The runner uses the checked-in `simulations/region_engine.py`, strict accounting component and `vendor/aod_manual_native.py`. No sibling manuscript checkout is needed for these examples.

## Shared exact channel calculation

Manual `manual:boundary-current:shared-setup` gives `φ = C3 min(ρ,w) A` with `A = lag × intensity`. The declared D0 operands use capacity 3, contact support 3 plus reflection counts `(1,1)` (duration 5), intensity `1/3`, and an explicitly chosen lag/multiplier for each channel. Thus window 4 and lag 1 give pressure 12 and current 4; window 6 clips at duration 5 and gives current 5. The canonical values come from `manual:boundary-current:worked-substitution`.

The declared reflection counts in these D0 preparations are inputs, not counts recovered from admitted particle histories. This differs from the earlier proton/electron witness examples. Complete input operands and their source hashes are retained next to every channel occurrence.

## Maser boundaries

The cells are emitter, cavity, output and exterior. Five explicit channels are exterior→emitter (pump), emitter→cavity, cavity→output, output→exterior and cavity→exterior (return branch). Windows are `(4,4,4,6)`; all coupling gates are `(on,off,on,on)`.

| Cut | Pump inward | Detected output outward | Cavity return outward | Apparatus net |
|---:|---:|---:|---:|---:|
| 100 | 8 | 4 | 4 | 0 |
| 101 | 0 | 0 | 0 | 0 |
| 102 | 8 | 4 | 4 | 0 |
| 103 | 10 | 5 | 5 | 0 |

Window totals are pump 26, detected output 13 and return 13. Net zero does not mean absence of transport: every gross transfer remains in the immutable ledger. All 20 channel occurrences are retained, including the five unavailable occurrences at cut 101. Internal interface currents cancel from the enclosing apparatus boundary. Gate-off is declared here and does not erase the existing prefix.

This is a balanced finite boundary preparation, not a derived hyperfine-emission or cavity-feedback solution. Physical use supplies its preparation/transition model, geometry/response, and observation map for power or phase.

A separate source-bound Manual run executed `validate_transport_ledger` on a declared ancestry record: emitted 3, arrived 2, pending 1→2, so `3−2=2−1=1`. Its [retained result](../evidence/scoped-current/outputs/maser-packet-ledger.json) and [execution record](../evidence/scoped-current/checks/maser-packet-execution.json) include four hostile controls for deleted, duplicated, out-of-window or erased pending information. These packet counts are not added to pressure-current or phase-turn quantities. That upstream metrology checker is a separately recorded calculation, not a dependency or executable entry point of this repository's region runner.

## Earth, water shell and Moon

The water shell is constructed as `earth-water-outer minus earth-core`. The six finite cells are earth-core, water-inner, water-outer, surface, Moon and exterior. The declared interfaces are core→water current 4, internal water→water current 4, water→Moon current 12, and water→exterior signed shedding current 2. The last is explicitly supplied in signed-current units; it is not inferred from an unresolved closure-burden route.

The surface cell has declared swept content 3 with an explicit factor-one map to swept-current units. The same committed sample is used by the growing water/combined region and the shrinking complementary exterior. This does not infer volume, density, velocity or SI time from the number of cells.

| Cut | Boundary/coupling input | Water cross | Water sweep | Water shedding | Water current | Earth+water+Moon | Exterior |
|---:|---|---:|---:|---:|---:|---:|---:|
| 200 | stationary, coupled | 8 | 0 | 2 | 10 | 2 | −2 |
| 201 | add surface, coupled | 8 | 3 | 2 | 7 | −1 | 1 |
| 202 | stationary, Moon gate off | −4 | 0 | 2 | −2 | 2 | −2 |
| 203 | remove surface, recouple | 8 | −3 | 2 | 13 | 5 | −5 |

The expanding-water row reproduces the Manual calculation `8−3+2=7`. The whole universe boundary is zero at every cut. The Earth+water audit minus the Earth audit equals the independently derived water-shell audit. All disjoint child partitions separately verify cross, sweep, shedding and total-current equality; their histories remain retained.

This exercises the same interface calculation used by the Manual fluid/plasma examples. The chosen interface motion and coupling schedule are inputs here. A tide-height or fluid-velocity prediction additionally needs the Earth/Moon forcing, water response and observation preparation; the Manual's Earth–Moon O0 target record is not relabelled as an executed prediction.

## Checks and output scope

Each region is compared with the native helper's Stokes result. The moving identity is checked at each cut. Eight integration controls reject omission/substitution of a committed interface, deletion of a cut and reuse of a physical occurrence across the two examples. Engine tests separately cover malformed motion, partition overlap, sweep substitution and other contract failures. [Original normal and optimized execution records](../evidence/scoped-current/checks/region-examples-execution.json) and [publication verification](../evidence/scoped-current/README.md) identify their actual source revisions.

The output includes complete ledgers, channel operands, every region frame, source-bound coverage, per-region archives, native comparisons and partition residuals. The result establishes exact accounting for these declared preparations; it does not assert empirical percentage error.
