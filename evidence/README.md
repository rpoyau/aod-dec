# Evidence index

This repository imports the completed, detached M0–M5 implementation. The native
compiler/runtime and learning code are unchanged from the sealed candidate;
repository-specific additions provide CI, pinned skills/lint guidance, provenance
checks and navigable documentation. M6 self-hosting and AArch64 remain unimplemented.
No cost-inclusive speedup was established.

## Original execution

[`native-r01/`](native-r01/) contains byte-for-byte records from the detached
execution, including its [report](native-r01/REPORT.md),
[criterion dispositions](native-r01/ACCEPTANCE_RESULTS.json),
[runtime checks](native-r01/runtime-results.json),
[source conformance](native-r01/conformance/run-02-final/results.json),
[Python-free isolation](native-r01/isolation/results.json),
[backend audit](native-r01/backend-audit.json), and
[final benchmark](native-r01/learning/run-04/REPORT.md).
Failed development attempts and repaired findings remain in the records.

The original evidence is bound to source tree
`24f8df17d234b980f47025b78733dc931aaede74a0f689de0589fa950815a07e`.
It records 79 tests in each Python mode, 94 source AOT/JIT comparisons, 1,979
native runtime cases, Python-free execution and seven workload/control suites.
These counts describe that source and run, not a perpetual guarantee for later
repository changes.

Archived files retain their original local paths and links to preserve exact
bytes. Use [`native-r01/ARTIFACT_MAP.json`](native-r01/ARTIFACT_MAP.json) to resolve
an original request path to its repository copy. Original `work/candidate/` source
paths correspond to this repository's root. The source snapshot and native code
hashes are preserved in [`native-r01/delivery/source-manifest.json`](native-r01/delivery/source-manifest.json).
Generated binaries/caches, duplicate earlier experiments and foundation ZIPs are
not imported into Git. Their recorded hashes and the complete original delivery
archive identity remain in the map; source scripts reproduce the compiler checks.
Archived internal links assume the old request directory and may require that map.

## Repository import

[`pr-import/`](pr-import/) holds fresh repository validation and tool bindings.
`IMPORT.json` records the original bundle, preserved implementation hashes,
documentation/integration changes, and exact upstream tool revisions.
Run `python3 scripts/check_import_provenance.py` to check the imported records and
bound implementation. Run `make verify-native` for current semantic validation;
CI retains its own logs as workflow artifacts.

The requested [`aod-lint`](../docs/LINT.md) revision was built and tested. Its
initial import digest contained no Typst/LaTeX files, so that historical applicability is
**NOT_APPLICABLE**, not a compiler lint pass. [`aod-skills`](../docs/SKILLS.md)
supplies source-bound AOD/AF guidance under the current owner amendment.

The earlier request prohibited remote writes during detached execution. The
owner's later instruction explicitly authorizes this repository and PR import.
Historical manuscript milestone labels and publication gates do not govern the
compiler milestones. No upstream manuscript or tooling repository is changed.

## Typst language and compiler manual

[`typst-documentation.json`](typst-documentation.json) records the subsequent
Typst manual build, actual strict manuscript lint, all ten executable examples
through reference/typed IR/AOT/ORC, the Python embedding check, Main/Manual source
anchors, and rendered PDF inspection. The three Manual applications reproduce
exact kernel/hinge-slide fractions, short-window residual/surplus values, and
boundary-current substitutions. Their model scope is explicit in the manuscript.

The original empty lint scan remains unchanged under `pr-import/`; current
manuscript coverage is one Typst file. `make docs-native` reproduces the PDF and
example checks with Typst 0.13.1. CI retains its current PDF and validation report
as workflow artifacts. The generated PDF is not committed as a source file.
