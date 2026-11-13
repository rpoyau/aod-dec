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
manuscript coverage in that record is one Typst file. `make docs-native` reproduces the PDF and
example checks with Typst 0.13.1. CI retains its current PDF and validation report
as workflow artifacts. The generated PDF is not committed as a source file.

## Tesseract-cell jitter application

[`cell-jitter/results.json`](cell-jitter/results.json) records explicit cell
topology, 1,024 exact region balances, 18 fixed-path refinement cases, 33
continuous-reference jitter realizations, and four generated Dec programs through
six execution modes. [`cell-jitter/source-binding.json`](cell-jitter/source-binding.json)
binds the Main/Manual and AFC source labels used for this declared application.
[`cell-jitter/validation.json`](cell-jitter/validation.json) records tests, PDF
inspection and the subsequent two-manuscript lint run. The figure is retained
beside the numerical records; binaries and the companion PDF are build artifacts.

Run `python3 scripts/simulate_cell_jitter.py --native --plots` to reproduce the
study. `make docs-cells` builds the companion using the retained evidence; CI
builds it from its fresh study results. Numerical refinement error and response
to jitter are reported separately. Physical maser accuracy remains unestablished.

## Manual-alignment correction

The initial normalized L1 diagnostic is mathematically defined but does not
measure the boundary output's relative error. The source-bound
[`manual-alignment/REVIEW.md`](manual-alignment/REVIEW.md) records this reporting
defect and the separate missing path/population/RCD and SI-map interfaces.
Historical version-1 results and validation remain unchanged above.

[`manual-alignment/results.json`](manual-alignment/results.json) supplies the
corrected per-output reports, exact D.E.C. kernel/history/RCD fixture, conditional
observed-cycle SI map, and published hydrogen input reduction. Its error budgets
keep exact arithmetic, diagnostics, unavailable bounds and unknown physical
quantities separate. Target values cannot change the fixture prediction.
Source bindings, bounded review digests, tests, lint and revised PDF validation
are retained alongside it. The reviewed Manual I candidate is identified as a
delivered local candidate, not silently promoted to accepted remote Main.
