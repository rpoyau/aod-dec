# Canonical chiral example provenance refresh

The canonical chiral example at `f1edabcaf2dcb1ffe9009a3075018155d2a5ac1b`
still contained the producer hash from before the R07 sign correction. Its
retained-path hash also included that earlier source binding. CI run
[43653776001](https://github.com/rpoyau/aod-dec/actions/runs/43653776001) therefore
failed the exact generated-example comparison in the chiral study and skipped
nine later execution/build stages. The equality guard correctly exposed stale
provenance; it remains intact.

`examples/chiral_core_collision.dec` is regenerated through the existing
`dec_source` generator using the study's actual `core_only` configuration, seed 7
and shared ticket denominator 64. Only comments at lines 6 and 8 change. The
generator, evaluator, configuration and ticket hashes remain identical. The
example's checksum is refreshed in `SHA256SUMS`.

Both normal and optimized Python confirm exact generator equality and unchanged
executable statements and VM stdout. The study's canonical equality check also
passes in both parent modes; its subprocess interpreter/compiled-IR replay uses
the helper's unchanged normal-mode invocation. All 12 focused chiral generation
and study tests pass per mode. Pinned AOD lint passes all nine existing Typst
manuscripts; no manuscript bytes change. An independent review checks the narrow
diff and execution bindings. No compiler, producer, simulation law or test source
is changed by this repair, and no new physical prediction follows.

`RESULTS.json` binds the before/after bytes and local executions.
`validation-evidence.zip` contains the reproduction driver, original CI logs and
step dispositions, both generated sources, VM outputs, focused test logs, lint
result and independent review. `ARCHIVE.json` binds the archive; its internal
manifest covers all members. Absolute execution paths identify this local run.
The complete workflow is rerun on the published correction; its authoritative
result is the GitHub Actions run attached to that commit, separately from these
pre-push local checks.
