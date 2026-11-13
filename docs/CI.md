# Repository validation

`make verify-native` (or `make verify-ci`) runs the same validation driver as the
GitHub Actions job. Set `DEC_LLVM_LIBRARY` if LLVM 16.0.6 is not at the original
Fedora host path. The driver verifies the actual loaded LLVM version and Linux
x86-64 target before attempting native tests, then records commands, outputs,
exit codes and elapsed time under `build/verification/`.

The checks are the complete Python test suite in normal and optimized modes,
legacy Rust debug/optimized checks, native debug/O3 witness and lifetime checks,
the native exact-value differential corpus, and the full source-level
reference/AOT/ORC corpus at O0/O2. The driver fails if any required command fails;
a missing dependency is never reported as a passing native check. GitHub retains
available logs and JSON evidence even after failure.
Before those tests, the workflow checks `SHA256SUMS` and runs
`scripts/check_import_provenance.py` against the recorded imported evidence and
unchanged compiler core.

CI uses the `ubuntu-24.04` runner and pins LLVM's package to
`libllvm16t64=1:16.0.6-23ubuntu4`, the package published for Ubuntu Noble.
It discovers the library file from that installed package and passes its path
through `DEC_LLVM_LIBRARY`. The compiler independently requires LLVM 16.0.6.
Compiler, Python, Rust, GMP and OpenSSL package versions are recorded for each
run. Those system dependencies receive the runner's distribution updates; the
job does not claim a frozen complete OS image or identical performance across
different hosts. The original execution host lock remains historical evidence.

The package/version and action pins were checked against their primary sources:
[Ubuntu LLVM package](https://packages.ubuntu.com/noble/libllvm16t64),
[checkout v4.2.2](https://github.com/actions/checkout/tree/11bd71901bbe5b1630ceea73d27597364c9af683),
and [upload-artifact v4.6.2](https://github.com/actions/upload-artifact/tree/ea165f8d65b6e75b540449e92b4886f43607fa02).

The workflow has read-only repository permissions and does not require credentials
for the private sibling `aod-lint` or `aod-skills` repositories. Their pinned local
application and actual coverage are documented separately in `LINT.md` and
`SKILLS.md`. CI's native conformance is not a substitute for a manuscript checker;
an absent sibling checkout does not create a successful lint claim.

Performance experiments remain separate from CI because shared-runner timing is
not evidence of a cost-inclusive speedup. The full Python-free Bubblewrap check
also remains separately recorded: hosted-runner namespace policy is not assumed.
Only Linux x86-64 is executed here. No AArch64 or self-hosting claim follows from
this workflow. Until a remote workflow actually completes, local driver results
are evidence of local validation only.

## Documentation

The standalone driver is `docs/language-and-compiler.typ`. It uses no external
Typst packages or network assets. Install Typst 0.13.1 and the DejaVu Sans Mono
font, then run:

```sh
make docs
# Or select a compiler outside PATH:
make docs TYPST=/absolute/path/to/typst
# Also replay every example through native AOT and ORC at O0 and O2:
make docs-native TYPST=/absolute/path/to/typst
```

The PDF is `build/docs/aod-dec-language-and-compiler.pdf`. The build also writes
`build/docs/report.json` and extracts the ten complete programs to
`build/docs/examples/`. Each literal `dec` code block is paired with its following
`expect` block. The driver checks source syntax, reference execution, and typed
JSON compile/exec against those expected bytes. `--native` adds executable and
JIT checks at both optimization levels. Snippets marked `text`, `python`, or `sh`
are explanatory and are not counted as executable Dec examples.

The three computational applications reproduce the supplied AOD Manual's kernel
normalization/hinge-slide, short-window residual/surplus, and boundary-current
substitutions. The other examples document language operations and the repaired
core's finite closure/fusion/coupling contracts. Source archive hashes, exact
Main/Manual labels, lint results, and PDF inspection evidence are retained in
`evidence/typst-documentation.json`. Main/Manual manuscripts remain unchanged.

The workflow downloads a checksum-pinned Typst release, builds the PDF, and
replays native examples after compiler conformance. It retains the PDF and build
report as Actions artifacts. Generated PDFs and extracted examples stay under
ignored `build/`; the Typst source is the editable publication artifact. Typst
compilation and example execution are independent of the local-only private
`aod-lint` invocation documented in `LINT.md`.

## Tesseract-cell application

`make simulate-cells` runs the exact topology/conservation checks, 18 refinement
cases, 33 continuous-reference jitter realizations, and four generated Dec
programs through reference/typed IR execution. It writes
`build/cell-jitter/results.json`. Add `--native` to the script for AOT/ORC O0/O2
replay and `--plots` for Matplotlib PNG/PDF charts. Missing requested native or
plotting dependencies fail the run. Thirty model tests are included in the
normal and optimized Python suites.

`make docs-cells` builds `docs/tesseract-cell-jitter.typ` against the retained
evidence. CI instead builds the companion against its newly generated results
and chart. It installs the distribution's `python3-matplotlib`, records that
package version, and retains the application PDF, figures, results, and generated
Dec sources. Exact model results are distinct from rounded plot labels, physical
maser accuracy, and compiler performance measurements.

The corrected version additionally executes the exact Manual D.E.C./RCD and
observed-cycle SI fixtures, preserving complete return/contact records and
per-output error components. Thirty further tests cover these interfaces,
including history-loss controls, target isolation, unit/quantity-kind rejection,
and exact source-reported hydrogen reductions. The native transport replay is
unchanged in scope; the host Manual interfaces are not new native Dec types.
Historical version-1 results stay under `evidence/cell-jitter/`; the corrected
version-2 report and source review are under `evidence/manual-alignment/`.
