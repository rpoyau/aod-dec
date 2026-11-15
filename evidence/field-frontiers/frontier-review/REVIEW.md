# Field-frontier review and regression record

**Final disposition:** no outstanding blocker identified in the reviewed response-only frontier, archive admission/recovery, explicit rebind and indexed scheduling implementation. This is not a claim of general field-dynamics closure, complete response journaling or whole-engine speedup.

The final reviewed `simulations/field_frontier.py` SHA-256 is `dbf302431419c7f638893960fede15da180a30e50ee4ab2d916297642f898465`; the repository base is `430fb143f20abb73662a91aeff84445527eb603e`. Exact producer, strict decoder, retained-union, test, driver and documentation bindings are in `SOURCE_BINDINGS.json`. AOD authority and the local Main/Manual ownership amendment apply; the scientific scope is the declared native inverse-pressure response. This reviewer made no production-source repairs. The implementation owner applied the corrections below.

## Findings and resolution

| ID | Initial counterexample | Final correction and checked outcome |
|---|---|---|
| F1 | Integer `reflected_comparison=1` passed producer and frontier admission, wrote an archive, then failed strict exposure and reopening. | Field-aware witness schemas require the actual Boolean type only at the reflection flag. Rejected before durable output in both modes. |
| F2 | Primitive certificate `scale=False` aliased numeric zero and survived admit/expose/reopen. | All numeric witness schema fields require exact integers. Nested witness traversal applies the same restriction. Rejected before durable output in both modes. |
| F3 | A failure after writing the durable witness rolled back its SQL bindings. The same open archive then admitted a conflicting history, and reopening failed. | A write/commit failure now blocks further admission until reopening rebuilds the derived union. The first durable witness survives; the conflicting next admission fails both before and after reopen. Pure pre-write union conflicts can roll back without poisoning the archive. |
| F4 | The duplicate-existing-file branch used unbounded `read_bytes`, despite the per-witness byte cap. | It now reads at most cap+1. An oversized collision file was refused; the independently instrumented request was exactly 8193 bytes for an 8192-byte cap. Exposure uses the same cap+1 bound. |

The initial source snapshot has SHA-256 `64dbe82519fac8806de38677e15752b6ea8038ed3f2f4cfac750d877012c7f63`. Original counterexample outcomes are retained in `admission-roundtrip.json`, `witness-bool-alias.json`, and `queue-and-write-failure.json`. `review_probes.py` contains the executable hostile constructions and final rejection/recovery checks. The initial snapshot is evidence, not a source repair or an implementation to run in production.

## Final substantive checks

Cold admission checks the immutable typed witness graph and unchanged native admission before writing. `RetainedHistoryUnion` contributes nested event, terminal, wave-type and ordinal-prefix bindings to the transactional disk index. Removing a live field does not erase those archived constraints. Reopening verifies each content hash, strictly decodes and revalidates its witness, then reconstructs the index. Independent nested-wave probes confirm that conflicting inner histories remain rejected after removal and reopening.

The live registration stores copied scalar tuples, including rational numerator/denominator pairs and witness-reference strings. The active summary contains no raw Wave, return list, certificates or nested interior. Registered attention is also snapshotted; changed or stale attention cannot acquire admission by retaining a field name. Exact direct Boolean/float inputs and mismatched scope, scale, cut, generation, current or pressure are rejected by the inspected paths and focused tests.

For fixed admitted context, `advance` invokes the unchanged native law `theta_next = theta + attention/P` on the compact record. Independent exact calculations agree. Hot probes patch archive exposure, file opening and native wave validation to fail: three response steps still complete, with unchanged closure-cycle count, reclosure-beat count, latest return, cadence and archive reference. Response commitment is not certified formation. Pending obligations hold the response instead of being treated as zero or cleared.

The explicit cold `rebind` path validates the latest registered summary and new domain, then exposes the same retained evidence before changing registration. It preserves advanced current, pressure, committed cut, weight, scope, scale, formation counts and pending obligations. It increments generation and invalidates previous attention. Independent checks verify exact post-rebind response, stale-summary rejection, failure atomicity, excluded-cut rejection and pending preservation. It introduces no new coupling law or new scale formation.

The indexed queue retains one pending heap entry per registered field. Independent dictionary-oracle checks run **20,000 operations in each Python mode**, including arbitrary rescheduling, cancellation, removal, re-registration and popping. Every operation checks returned order, heap order, positions, pending content and counts. The source uses indexed sifts and replacement/removal rather than accumulating stale heap entries. Its O(log n) claim is heap work under bounded key/operand costs, not a bound on all coupled dynamics or archive work.

All **17 focused tests pass normally and under -O**. The independent hostile/regression probes also pass in both modes on the final exact source hash, using explicit checks rather than Python assertions. Logs and JSON outcomes are retained. This reviewer did not rerun the full SPARC cohort or whole repository suite; those are separate parent-task checks.

## Window and scope limits

`FieldFrontier.window` is an explicit host validity interval **[start, end)**: the committed response must lie inside it and the next step cannot reach its excluded end. It is distinct from native retained-return/RCD counting windows **(start, end]**. Rebinding this host interval does not calculate a new RCD, native beat, physical duration or SI clock. Changed coupling is an explicit external binding; field weight remains metadata and does not silently become pressure.

The archive retains historical formation evidence. It does not persist every later response state or attention event. Recovery of those later events requires the caller's trace/journal; reopening returns the archived formation, not an invented latest trajectory. New closure certification, arbitrary interior transitions, automatic scale formation, and physically inferred coupling remain outside this implementation. Cold admission traverses supplied history and serializes witnesses; the file cap limits accepted/read bytes, not all transient validation/serialization allocation. Reopening limits remain caller-supplied.

## Reproduction

```text
python3 -B review_probes.py --repo /path/to/aod-dec --output /tmp/frontier-review-normal
python3 -O -B review_probes.py --repo /path/to/aod-dec --output /tmp/frontier-review-optimized
python3 -B -m unittest tests.test_field_frontier -v
python3 -O -B -m unittest tests.test_field_frontier -v
```

The probe script also discovers a surrounding checkout automatically, including when packaged beneath `evidence/field-frontiers/`. Source hashes are captured before/after probes, and the output binds the probe script itself. Run the unittest commands from the repository root.
