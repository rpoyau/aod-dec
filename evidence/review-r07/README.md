# PR #6 R07 corrections

The four findings supplied in `AOD_DEC_PR6_REVIEW_R07.zip` are corrected against
reviewed commit `9b3f5036d7d17b022558041a68332342e8f85496`. The input archive is
bound by SHA-256 `e7cb9a81f8b343624c3e5c4aad190ac20b14f33688a5191e5638142d0b114601`;
its CRC and all 102 manifest files were verified unchanged. The original review,
source-bound review artifacts and repair executions remain separate records.

| Finding | Corrected behavior | Counterexample/control |
|---|---|---|
| R07-01: conflicting histories | Support and relation detectors check a bounded union of retained event coordinates, terminal histories and wave prefixes, recursively including components and inner waves. Conflicts or exhausted budgets make coverage partial and withdraw dependent verdicts, including nested enclosure verification. | Supplied contradictory support: two verified formations become zero. Real reopened altered-fold archive: one verified fold/four shedding records become zero/zero. Identical overlap, distinct lineages and lawful shorter prefixes still pass. |
| R07-02: checkpoint failure | Post-execution checkpoint and diagnostic storage errors preserve the computed outcome, emitted prefix and original model error. Trace errors remain separate; unavailable status fields are explicit. | Successful and failing models retain their original outcomes under checkpoint exhaustion, including a second failure while reading status. |
| R07-03: implicit reopening | Validated persisted limits are restored before manifest reads; explicitly supplied limits still govern reads. Bootstrap metadata is limited to 4 KiB of encoded bytes before UTF-8 decoding. | A manifest larger than default limits reopens under its stored policy. Duplicate/missing/extra fields, noninteger limits, malformed UTF-8 and oversized multibyte metadata are rejected. |
| R07-04: Boolean Dion signs | Producer and decoder require exact built-in integer signs, rejecting Boolean, float and integer-subclass aliases. | The Boolean Duon counterexample is rejected. Fourteen valid complete canonical producer outputs are unchanged, including the prior list-container domain; unrelated Boolean metadata remains valid. |

Both standard full-suite runs execute all **540 unique test IDs: 538 pass and two
skip per mode**. Normal and optimized Python use separate ordinary `unittest
discover` invocations. All 617 source/evidence files captured before those runs
remain identical afterward. The two skips are the actual GPU comparison and
uint32 threshold/equality tests; this runtime reports `cuInit: CUDA_ERROR_NO_DEVICE`.
There are no missing-LLVM16 or missing-cohort failures in this validation environment.
Earlier CI and local counts describe their own prior revisions and environments.

The patch adds 29 regressions. Targeted suites pass 61 history/support/relation/sign
tests and 42 store/execution tests per mode; these overlap the full suite and are
not added to its count. The existing 19 admission cases remain discoverable. All
eight mutation-control runs pass their expected outcomes (baseline, identity,
reflection and combined mutations in both modes).

Fresh particle CLI executions and reopened archive replays agree exactly for
support, relations and paths, with complete coverage in both modes. Model outcomes
and those detector reports also agree between modes; host profiling and external
artifact directory fields are explicitly excluded from cross-mode equality.
Pinned `aod-lint` revision `68bc4795e982bbdeb266e346d694b08c7c49c844` passes strict
checks on all nine existing Typst manuscripts. Its scope excludes Python/Markdown
semantics and scientific validation. Import provenance and whitespace checks pass.

Independent patch reviews and an execution-evidence audit found no unresolved
findings in their declared scopes. Review caught and corrected two intermediate
defects: a nested verified enclosure label after union rejection, and SQLite TEXT
character counting in a byte budget. Those counterexamples are retained. An
earlier mixed-import baseline harness is quarantined and excluded; accepted
before/after probes bind every imported simulation module to its intended source.

`RESULTS.json` records acceptance and tested file hashes. `source-rebindings.json`
records the local vendor correction and six exact consumer/harness bindings;
upstream Main/Manual acceptance is not inferred. `validation-evidence.zip` retains
commands, logs, inventories, raw before/after probes, cross-reviews, source
fingerprints and fresh particle archives. `ARCHIVE.json` binds the ZIP; its internal
`SHA256SUMS` covers every evidence member. Absolute paths in execution records
identify the original environment. Older evidence remains immutable; provisional
and pre-correction artifacts retain their dispositions rather than counting as
final validation.
Only the new evidence and top-level checksum manifest are added/refreshed after
the full-suite source freeze; executable files remain exactly those tested.

To reproduce the current regressions from the repository root:

```sh
python3 -B -m unittest discover -s tests -p 'test_*.py' -v
python3 -B -O -m unittest discover -s tests -p 'test_*.py' -v
python3 -B scripts/check_admission_mutations.py --repo . --output build/r07-mutations
python3 -B scripts/check_import_provenance.py
```

Lint setup and tool identity checks are documented in `docs/LINT.md`. The bound
skills and current Main/Manual ownership reconciliation remain in `docs/SKILLS.md`.
No Main/Manual sources were changed. Native tracing still requires no SI
conversion. Full proton/cavity/sensor evolution and field-generated Brownian
motion remain unfinished/unverified; these finite admission and storage repairs
add no physical scattering prediction. No cost-inclusive speedup, M6 self-hosting,
AArch64 completion or new GPU execution result is claimed.
