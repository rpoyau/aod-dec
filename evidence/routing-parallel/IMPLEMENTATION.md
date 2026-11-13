# Declared routing fixture: implementation and local checks

The two new owned files are `simulations/collision_routing.py` and
`tests/test_collision_routing.py`. Their final SHA-256 bindings are in
`source-binding.json`. The vendored producer, prior chiral model and compiler
runtime were not changed by this task.

The application supplies its support/projection constructions and load rule;
the existing producer checks the corresponding exact certificates. Final
contract: `L=F_A+F_B`, declared `C=1` by default, `Delta=L-C`, `X=max(0,Delta)`.
These operands are dimensionless count-derived model load, not pressure or
physical collision energy. Default positive routing is redirected support;
exoshedding additionally checks actual inner exposure. Endoshedding/open-loss
amounts remain zero in the exact one-active-route partition.

## Interfaces

`build_routing_fixture(path, *, closure_cost=Fraction(1),
route='redirected_support', seat_compatible=True, retention_ticks=2,
window_ticks=2)` accepts the existing in-memory sampled path, validates its
exact replay, and returns a deterministic typed dictionary. Compact paths are
reconstructed with their full witness payloads before materialization.

Key records:

- `source_path`: actual complete source path, with source carrier identities.
- `carriers['a'/'b']['wave']`: separately constructed admitted 3:3 recurrent
  supports. `return_maps` binds each source return/sign to three actual prior
  components, its motif witness and resulting return certificate.
- `observables['a'/'b']`: closure count, origin-qualified beats, ordered cadence,
  atomic traversal counts, octal event-address records and beat-to-event links.
- `outer_fold`: independently certified next-scope fold of the actual two
  materialized carriers.
- `routing`: typed computed record, exact source-state/event/model-bridge
  payloads, actual Duon/E2 route form, explicit seat/retention declaration and
  exact surplus partition.

`validate_routing_fixture(report)` rebuilds every payload from the retained
source path and typed parameters and compares types and values strictly.
`wave_observables(wave)` and `trace_address_records(trace)` expose the separate
count/address coordinates. No process ID, execution time or hardware-dependent
field enters the fixture or record digest, allowing CPU/GPU execution metadata
to remain outside the model result.

## Exercised checks and outcomes

All **13 tests passed** with ordinary Python and `python3 -O`, in 26.556 s and
26.620 s respectively. Logs are `tests-routing-normal.log` and
`tests-routing-optimized.log`. These are local implementation checks, not
independent physical validation or backend performance measurements.

- Actual 3:3 construction: every retained source return maps to the selected
  chirality, three distinct prior Monon components and valid p:q motif witness.
  Two formation returns plus eight selected returns give ten materialized
  closure cycles and nine origin-qualified beats.
- Exact fixed seed controls: seed 1 has total flips 0, residual -1 and no
  route; seed 0 has total flips 1, residual 0 and no route; seed 8 has total
  flips 3, residual/surplus 2 and one redirected-support partition amount 2.
- Positive routing requires the actual two-return Duon/E2 support before the
  outer fold, and routing commitment follows the outer certificate. The form
  is Duon/E2; its separate enclosing parameter is 6.
- Exoshedding exposure returns the actual preserved inner pair unchanged.
- Matched paths preserve their initial materialized formation prefix, while
  changed counterfactual continuations have distinct event identities. Routing
  configurations also namespace outer events when their construction timing
  changes.
- Source state orientation uses the last retained return, not the formation
  orientation. An empty selected window retains formation histories without
  inventing a selected event.
- Missing form, forged source payload, forged bridge, wrong partition,
  changed source tickets, false type aliases, invalid route, incompatible seat
  and insufficient retention are rejected.
- Octal committed-cut digits bind immutable events in a scoped trace
  namespace. They remain distinct from Q4 vertex bits, event ordinal, role and
  beat count. Equal projected endpoint vertices retain distinct terminal/start
  events and addresses. Negative cuts are rejected by this finite address map.

## Limits preserved

TTL availability and seat compatibility are explicit model inputs. Neither is
deduced from cadence, RCD or CPU timing. Source-address octal notation is bound
to Main's retained-address distinction; the numeric cut-to-address map is this
application's declared representation. The owner's clarified meaning removed
the earlier octave-as-beat-count interpretation before the final code.

The term proton-facing names the declared 3:3 support. Physical preparation,
interaction law, cross section, outgoing-particle inventory and SI wavelength
remain uninstantiated here. The parent may attach a separately declared
conditional wavelength observation map. No test in this record exercises a
new native compiler backend or an actual GPU.
