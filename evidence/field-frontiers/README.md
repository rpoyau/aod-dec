# Active fields and recoverable interiors

Here `n` counts active fields in the declared response window. `FieldFrontier`
keeps response operands and a reference to an admitted native Wave; complete
returns, certificates and nested histories remain in a durable archive.
`IndexedFieldQueue` keeps one pending event per field. Its indexed heap gives
`O(log n)` scheduling/removal and `O(n log n)` heap work for one event per field,
under bounded local work, coupling degree and operand size.

The implemented response is the existing exact supplied-attention update.
Current and committed cut advance; closure cycles and reclosure beats do not.
Cold admission checks the combined retained histories in a disk SQLite index,
which is reconstructed from validated witness files on reopen. The response
loop neither reads that index nor loads the interior. A window/coupling change
requires explicit `rebind`, which exposes the historical wave while preserving
current state and invalidating previously bound attention. Pending obligations
remain unresolved. The host validity interval `[start,end)` is separate from
the native retained-return/RCD count window `(start,end]`.

The count fixture exercises 8, 32 and 128 active fields for four responses each.
There are zero hot archive reads, and peak pending entries equal the active
field count. The exact independent answer is current `13/9`. For eight folded
fields, increasing each interior from 1 to 4 to 16 inner waves preserves the
4,520-byte serialized initial summary family while the archive grows. Exposure
recovers each checked complete historical witness. `normal.json` and
`optimized.json` record source hashes, operation counts and cold, hot and
single-field exposure timings. These observations do not establish a
whole-engine speedup or an arbitrary-precision complexity bound.

The shared boundary context is a separate optimization integrated into the
SPARC runner. One response and swept-field ledger serve the selected,
complementary, whole and stationary views. Existing full audits still run.
The matched 24/48-cell helper benchmark includes context creation, validation,
response copying and all four views, and returns exactly the original records.
It measures approximately 1.55× improvement for that helper; preparation,
application history generation and serialization are outside the interval.
See [the measurement scope and raw runs](boundary/REPORT.md).

Source interpretation and repair are separate records:

- [Main/Manual source review](source-review/BOUNDARY_TRACKING.md), with exact
  hashes, claims and executable type/admission probes.
- [Implementation contract](tracer-review/REPORT.md), with final code bindings.
- [Independent implementation review](frontier-review/REVIEW.md), retaining
  reproduced defects, executable probes and the final correction checks.
- [SPARC trace equivalence](sparc-equivalence.json), allowing only the top-level
  implementation binding to change; production replay remains fully strict.
- [Local validation](validation.json), [AOD lint](aod-lint.json), and the
  [two-page Typst PDF](field-frontiers.pdf).

The implemented contract does not generate attention, new formations or
physical couplings. Exposure returns historical formation evidence; callers
must separately journal response events in `TraceStore`. A changed native
scale still requires its formation evidence. Dedicated Tetron/E4 admission and
an incremental new-return certifier remain unimplemented. The native base
weight is explicit metadata, not an inferred physical octave multiplier.

The final 31 focused tests pass in both Python modes: 17 frontier/admission/
rebind/scheduler cases and 14 shared-boundary cases. The independent review
also exercises 20,000 queue operations, nested conflicting witnesses, bounded
archive reads, partial-write recovery and atomic rebinding. Twelve manuscripts
pass the pinned AOD lint scan. Full-suite and remote results have their own
coverage records; interrupted runs are not counted as passes.

From the repository root:

```sh
make check-field-frontiers
python3 -B -m unittest tests.test_field_frontier tests.test_reduced_boundary_response
python3 -B -O -m unittest tests.test_field_frontier tests.test_reduced_boundary_response
python3 evidence/field-frontiers/frontier-review/review_probes.py --repo . \
  --output build/field-frontiers/review
make docs-field-frontiers TYPST=/path/to/typst
```

The preserved `boundary/benchmark.py` retains its original measured bytes and
task-relative layout. To reproduce it, create a temporary directory with an
`aod-dec` symlink to this repository, copy `boundary/` to
`closure-optimization/boundary/` beside that symlink, then run the copied
`benchmark.py` normally and under `-O`. Its frozen baseline is from `430fb14`.
Run timings on an otherwise idle host; there is no timing acceptance threshold.
