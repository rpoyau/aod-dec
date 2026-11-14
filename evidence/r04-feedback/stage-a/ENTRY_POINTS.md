# Stage A admission and consumer inventory

Authoritative producer: `upstream/manual/scripts/aod_native_closure_wave.py`, candidate SHA256 `562b3eb53ad6dd13c0801c43bd6e918110083ab4d12592c024b3007b634e2554`. Line anchors below bind those bytes. The original base is AOD commit `8c3149b9d3d924b80b9385b178da4ba2c39175e8`, producer SHA256 `3ba98c7097bc104aeb6218265b8531f94f2ca5a35239d0df097aba096675dfdf`.

The invariant is local to the finite records admitted together: `(scope, scale, lineage, event_id)` binds one `(vertex, committed_native_cut)`. The existing terminal key also retains its full trace-digest binding. Identical shared records/events are allowed, and different scopes, scales or lineages have separate keys. No process-global registry is introduced.

| Entry point | Delegation and retained union | Executed coverage |
|---|---|---|
| `validate_trace`, line 107 | Exact vertex/role and native index types; both reflection commitments checked before equality/order. Integer cuts remain signed. | Original trace fixtures; all supplied Boolean/float cases; Fraction and int-subclass commitment aliases; negative-cut controls. |
| `certify` / `admitted` | Existing complete certificate, registry and witness reconstruction remain mandatory. Outer certificates invoke `fold_proposal`. | Existing certificate/outer-provenance tests; new hostile records are freshly certified only where individually admissible. |
| `_check_retained_bindings`, line 181 | Shared finite union checker; callers first admit each trace. Checks full terminal trace binding and every scoped event coordinate. | Recurrence, contact, motif, member-wave, nested-inner-wave and identical-sharing controls. |
| `_wave_retained_traces`, line 199 | Traverses admitted wave returns, retained motif components, and inner waves in both return and component certificates. Does not count ancestors as returns of the outer wave. | Cross-motif component and nested outer-wave conflicts; unchanged outer-cycle counts. |
| `validate_record_collection`, line 215 | Admits every supplied certificate and checks the union including inner-wave histories. | Direct closure-cycle union of distinct outer records; original terminal conflict test. |
| `phase_count` / `closure_cycle_count`, lines 231/240 | Delegate admission to `validate_record_collection`; count only distinct supplied return IDs at the requested cut. | Existing phase/window/dwell tests and supplied accessor conflict; positive duplicate reuse. |
| `wave` / `validate_wave`, lines 270/308 | Certified record union first; existing recurrence and motif validation; then union of complete returned wave history, including components across different motif witnesses. | Original recurrence suite; new cross-motif component conflict and ordinary recurrence control. |
| `_validate_wave_collection`, line 316 | Validates each complete wave, then checks their combined retained traces. | Shared dependency of the following consumers. |
| `motif_witness` / `validate_motif`, lines 337/383 | Support and all component records are admitted in one union; preserve chronology, inherited construction, registry and component scope/lineage checks. | Existing inherited support and Duon cases; component-component and component-support conflicts. |
| `field`, line 400 | Validates all members together before projecting to member identities. Retained components/inner histories participate in that union. | Primitive-member, motif-component and nested-inner conflicts, with valid controls. |
| `resolve_fields`, line 407 | Existing identity projection over previously constructed `Field` objects; it receives no retained wave records and does not certify a fresh joint history. | Existing identity-union test. No claim of re-admitting absent witnesses. |
| `leg_comparison`, line 411 | Both certified operands use `validate_record_collection` before the existing support/reflection calculation. | Existing phase-budget cases plus conflicting shared middle event. |
| `contacts`, line 477 | Both source waves are validated together before constructing and individually certifying contact supports. | Valid contact controls and conflicting source-member rejection. |
| `compare`, line 511 | Validates source-wave union; retains all prior contact identity, cut, prefix, direction, certificate and support-shape checks; then checks the union of both wave histories and all support histories before computing outputs. | Both supplied hostile routes reject; exact forward/reverse ratio, bias, lag, drift, duration and contact IDs preserved for valid control. |
| `fold_proposal`, line 603 | Uses joint wave validation before existing scale/provenance proposal. | Reviewed cross-member conflict and valid proposal. |
| `outer_trace` / `fold` / `_validate_folded`, lines 612/621/633 | Delegate inner-union validation through `fold_proposal` and existing outer certificate admission. | Existing outer-prefix/certificate/fold/exposure suite; new nested-inner union test. |
| `shedding` / `expose`, lines 721/763 | Existing folded-record validation delegates to the repaired fold path. No routing or shedding law changed. | Existing exact shedding, route, target and exposure tests. |
| `update`, line 594 | Already checks exact integer state and input cuts; its declared response law is unchanged. | Existing invalid-cut and inverse-pressure response tests. |
| `standing_organization`, line 783 | Delegates pair/contact union to `compare`; now also checks exact integer state commitments before ordering. | Existing standing-pair cases; Boolean/float/Fraction/int-subclass aliases on either state, with valid signed-cut control. |
| `bind_values`, line 855 | Delegates wave admission; now checks exact integer state commitments before matching them to return cuts. | Existing bound-readout/Fourier cases; Boolean/float/Fraction/int-subclass commitment aliases. |
| `declared_readout` / `fourier`, lines 878/891 | Existing routes delegate readout admission to `bind_values`. | Existing exact harmonic identities and provenance regressions. |

This inventory concerns the scoped identity and commitment invariants above. It is not a claim that every possible malformed value at every producer API was exhaustively tested.

## Source-binding consumers coordinated by root

The initial producer repair changed the authoritative producer and its owning test module. Integration then exposed a real archival formation/target event alias; the same author repaired the active anchor adapter and its owning tests through the explicit migration described in [anchor-migration/REPORT.md](anchor-migration/REPORT.md). Root owns the following rebinding work, with historical files retained under `stage-a/original-*` and the pre-migration active anchor records under `stage-a/anchor-migration/original-active/`:

- Upstream canonical conservation contract: protected hashes of the producer and owning test module; independent auditor seal on the revised contract.
- Upstream `ROUND_TRIP_PHASE_FACTOR_SPEC` contract: its producer hash and associated independent auditor seal. This additional consumer must remain in the package inventory.
- Upstream `aod_m3_anchor.py`: active producer fingerprint, distinct from archived artifact authentication.
- Dec vendor exact bytes, adapter fingerprint and metadata that distinguishes the original base commit/tree/hash from the detached candidate hash.
- Other downstream manifests, generated source headers and intentionally regenerated source-dependent evidence identified by the root integration checks.

Historical snapshots are not rewritten to claim the candidate existed at the old base. No old protected entry is removed to obtain a passing gate.

## Untested downstream consumer boundary

Static inspection of `manual/scripts/aod_m3_temporal.py::validate_observation_context` shows that the selected comparison delegates joint admission through `adapt`/`compare`, while additional population waves and contacts are validated individually. That function does not separately invoke the new shared union checker across all otherwise-unselected population/contact records. No hostile counterexample was constructed or executed for that broader context boundary in this work. The regenerated anchor histories come from the fixed authenticated constructor, and the new Stage B World replay binds its own two fields; neither result establishes an exhaustive hostile-input closure for every other temporal-context consumer. This is a recorded coverage limit, not a demonstrated exploit or a change to the running sources.
