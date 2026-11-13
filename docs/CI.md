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
