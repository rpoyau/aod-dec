# Particle accounting examples

The proton-pair and electron-above-wire applications use the checked-in R04
native producer and strict boundary-accounting component. Run from the repository
root with Python 3.10 or later:

```bash
python -B simulations/proton_accounting.py --output build/particles/proton-accounting.json
python -B simulations/electron_wire_accounting.py --output build/particles/electron-wire.json
```

The proton command also writes `proton-accounting-witnesses.json.gz` beside its
report. It contains the complete encounter world, materialized carriers,
formation components, contacts, folds and route witnesses. The electron report
contains its complete native histories, synchronization, boundary views,
archives and later responses. Every value is an integer or exact rational.

The [Typst companion](particle-examples.typ) documents both preparations and
their Main/Manual source anchors. Neither runner requires a sibling manuscript
checkout. `source_sha256` reports hashes of actual local executable dependencies;
`recorded_manuscript_binding` preserves the source paths, candidate base and
manuscript hashes established by the original review. It explicitly does not
claim that absent manuscript files were reverified during this execution.

## Proton pair

The example adopts the Manual proton association `p+ → 3:3:6`. It maps the
retained continuation example's selected orientations into new admitted `3:3`
carriers. Every target is an actual six-edge C6 return with three strictly earlier
Monon components. The old `1:2` continuation preparation is retained as its source;
it is not relabelled as a derived proton transition law.

The four rows are alternative participation windows on the same history at the
same cut. They cannot be added as successive current slices:

| Window | Contact current | Closure burden | Surplus above cost 24 |
|---:|---:|---:|---:|
| 2 | −6 | 6 | 0 |
| 4 | −12 | 12 | 0 |
| 6 | −18 | 18 | 0 |
| 12 | −30 | 30 | 6 |

The window-12 surplus is routed and receipted once after its recipient support
completes. Its signed shedding-current projection remains explicitly unresolved.
A complete outgoing-current request rejects; the contact-only current is not
substituted for it. Eight hostile controls also check receipt timing/reuse,
missing formation or crossing support, sweep and duplicate projections.

## Electron above a wire

The example adopts the Manual electron association `1:2:6`, constructs three C6
returns and declares an above-wire cell opposite a representative Duon/E2 wire
lineage. Native contact and reflection histories give RCD 5; capacity 3,
intensity 1/3 and an explicit outward-sign response bridge supply the inputs.

| Input cut | Window | Coupled | Wire outward | Electron outward | Later electron current |
|---:|---:|:---:|---:|---:|---:|
| 52 | 4 | yes | 4 | −4 | 4 |
| 53 | 4 | no | 0 | 0 | 4 |
| 54 | 4 | yes | 4 | −4 | 8 |
| 55 | 6 | yes | 5 | −5 | 13 |

These are successive live inputs. Their window totals are wire 13, electron −13
and union 0. Gate-off removes the active input while retaining the previous
state and history. The all-gates-off control keeps the same preparation and gives
zero later current. The runner performs 24 positive checks and nine hostile
rejections, including coverage omission, archive loss and incompatible units.

The Manual's particle and SI comparison maps remain separate from these native
current calculations. No electron force, measured trajectory or proton
scattering cross-section is produced by these finite preparations. The same
scoped arithmetic and complete witness records are preserved by the portable
publication; only operational source paths and their verification metadata change.
