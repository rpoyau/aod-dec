# R04 admission and retained-continuation evidence

This directory preserves selected execution records from the completed local
R04 candidate before publication. The downstream native producer repair and
the formation namespace correction are active source changes in this PR.
The upstream manuscript repository is not changed by this PR.

- [Bindings](BINDING.json) and [validation inventory](VALIDATION.json)
- [Admission entry points and coverage](stage-a/ENTRY_POINTS.md)
- [Owning admission tests](stage-a/test-results.json), [strict acceptance](checks/strict-acceptance.json) and [adjacent probes](stage-a/adjacent-fixed-results.json)
- [History-derived finite construction](stage-b/HOST-PROOF.md) and [observation scope](stage-b/OBSERVATION-BINDING.json)
- [Executed compiler routes](stage-b/backend-final-delivery/results.json) and complete compressed witness histories in that directory

The original owning source had 191 tests per mode; strict acceptance had 19
cases against each producer, with 13 adjacent repaired expectations. These are
recorded upstream/local results, not a claim that this repository runs the
upstream suite. The portable repository reruns its own tests and counted
continuation check through CI.

This is a selected evidence copy, not the complete original workspace. Absolute
paths and references to other original artifacts remain historical provenance.
The copied-byte map is [IMPORT.json](../scoped-current/IMPORT.json). Fresh
repository execution is recorded separately under `../scoped-current/publication/`.
