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

## Chiral encounter application

`python3 scripts/simulate_chiral_collision.py --native --plots` compares six
exact finite probability laws with an independent reference, samples 256 paired
realizations for each of three settings, and replays four retained paths through
six execution modes. The application supplies the probability law; the unchanged
D.E.C. engine executes the supplied expressions and validates trace operations.
Thirty added tests cover this application and evidence-handling failure modes.

`make docs-chiral` compiles the five-page Typst companion from retained evidence.
CI compiles it from fresh results and retains the PDF, figure, full cohort,
witness exemplars, generated sources, typed IR, AOT build manifests and stdout.
The committed cohort uses lossless gzip; its decoded hash is recorded in
`evidence/chiral-collision/validation.json`. No calibrated physical proton-proton
prediction, clock, energy or cross-section map is supplied.

## Routed tracing and parallel application

`make simulate-routing` runs `scripts/simulate_routing_parallel.py` with four CPU
workers, 256 ticket paths, 16 fully traced paths and `--gpu auto --plots`. CI uses
two CPU workers with the same sample and trace counts. The integer candidate
evaluation uses caller-supplied movement and flip tables; deterministic tickets
and ordered rows support exact scalar/multicore/GPU comparisons. Backend
participation, data-transfer/startup costs and full host witness work must remain
separate evidence fields. A kernel timing alone does not establish a speedup for
the full simulation and tracing workflow.

No GPU dependencies are installed for the hosted CI job. `--gpu auto` records
an explicit `UNAVAILABLE` result when no usable GPU exists; that result is not a
GPU pass or an implicit CPU fallback. `--gpu required` makes an unavailable GPU
an error. Actual device execution and output comparison are required before
reporting a verified GPU backend.

The separate [local hardware run](../evidence/routing-gpu-device/validation.json)
uses an NVIDIA RTX A1000 6GB Laptop GPU, compute capability 8.6, with NVRTC 12.9
targeting `compute_86`. It runs with `--gpu required` and compares every retained
GPU candidate row against the CPU outputs. Both actual-device tests pass within
the 11-test CUDA suite, normally and under `-O`. Direct host execution was needed
because the sandbox hid the GPU device. These hardware results do not change
the hosted CI runner's availability or move full witness admission onto the GPU.

The host fixture admits a `3:3` proton-facing preparation and retained sheddic
routing through Duon/E2 form under declared positive-surplus and TTL/seat gates.
Batch candidate rows do not themselves certify these histories. Octaves retain
the octal coordinate presentation with Monon/beat records; event counts remain
separate. No physical frequency-doubling or wavelength map follows.
The physical proton and measured scattering correspondence remain uninstantiated.

`make docs-routing` builds `docs/routing-parallel.typ` from retained evidence.
CI builds it against fresh `results.json` and `routing-parallel.png`, retaining
the PDF, figure, JSON records and compressed trace files as workflow artifacts.
The pinned local manuscript lint and rendered-page inspection remain separate
from backend and routing tests.

## Retained, particle and region accounting

`make verify-retained` executes the counted retained-continuation application
through reference, typed IR, AOT and ORC. The host reconstructs and validates its
complete witnesses separately from the scalar generated update. The admission
repair and source binding are recorded under `evidence/r04-feedback/`.

`make simulate-regions` runs the maser and Earth-water-shell-Moon D0 preparations
through one region adapter. `make simulate-particles` runs the admitted proton
and electron-wire preparations. The workflow executes each in normal and
optimized Python and compares their exact output files, including the proton
witness payload. The full normal/optimized test suite also contains the strict
accounting and region adapter tests. Input preparation, source-family coverage
and output accounting are separate claims; no new physical transition law is
installed by the workflow.

`make verify-region-current` runs `scripts/check_region_current.py --output
build/scoped-current/dec`. Its 44 positive runtime cases and six hostile cases
exercise exact scalar clipping, region incidence, nonnegative swept measure and
moving-current arithmetic through the reference VM and generated ORC specialized
and fallback paths. An AOT executable checks the independent expected sequence
`4, -4, 0, 0, 7`. The workflow runs this checker in both Python modes. Native
addresses and timing fields are observations, so those compiler report files
are not expected to be byte-identical. Full collection/history/coverage work
remains host-side.

`make docs-scoped TYPST=/path/to/typst` compiles the retained-continuation,
boundary-accounting, particle-examples and region-current companions. CI retains
these PDFs, the fresh exact application outputs, full proton witnesses, counted
continuation artifacts and scalar compiler reports. Local rendered-page and
supported AOD-lint checks are recorded separately. The source-bound reviews and
publication evidence live under `evidence/particle-accounting/` and
`evidence/scoped-current/`.

The particle application adopts the Manual's proton `3:3:6` and electron
`1:2:6` associations. Its proton burden-to-signed-shedding-current projection
remains explicitly unresolved. The region adapter supports declared source-scale
zero currents and explicit rational reporting-scale maps; higher native formation
still requires ancestral support. The maser and Earth-water-Moon examples supply
their motion, gates and current operands. Their exact accounting identities are
not measured hyperfine, power, tide-height or fluid-velocity predictions.

## Reduced SPARC execution

The separate `sparc-reduced` matrix executes all five galaxies and all three
predeclared cases under normal and optimized Python. Each run reopens the
retained compressed trace, recomputes every result and requires complete
equality. Each job retains its trace, all-bin comparison table, summary, radial
comparison figure and a Typst PDF built from fresh results. Full retained-history
admission makes this application slower than its scalar recurrence alone;
each matrix job has a 40-minute budget and does not extend the compiler job.

Locally, run `make simulate-sparc-reduced`, then repeat
`python3 -B -O scripts/simulate_sparc_reduced.py --workers 2
--replay build/sparc-reduced/trace.json.gz --output build/sparc-reduced-replay`.
The full test suites also exercise exact independent recurrence answers,
moving-boundary partitions, source-change rejection, malformed histories,
cache equivalence and one-worker/two-worker determinism on small cohorts.
Neither exact conservation nor replay certifies the unknown physical exchange
at the deliberately closed radial/out-of-plane ports.

## Field boundary frontiers

`make check-field-frontiers` runs `scripts/check_field_frontiers.py` normally
and under `-O`, writing `build/field-frontiers/normal.json` and `optimized.json`.
The fixture varies active-field count and archived interior size, checks an
independent exact supplied-attention response, bounds the queue to one pending
event per active field, requires zero interior archive reads in the response
loop, and verifies exposure afterward. It records queue operation counts,
source hashes, archive/summary sizes, and separate cold and hot timings.
The reports contain mode and timing observations and are not expected to be
byte-identical. No timing threshold is used for acceptance.

`make docs-field-frontiers TYPST=/path/to/typst` builds the source-bound
two-page companion. CI runs both Make targets after the trace companion and
retains the complete `build/field-frontiers/` directory, including available
results after failure. The existing full Python suites separately exercise
admission, archive-integrity, cross-history conflicts, domain changes, queue
replacement and boundary-view isolation.

The frontier's `n` is the number of active fields. One pending event per field
gives `O(m log n)` heap work for `m` events under bounded local response cost,
coupling degree and operand size. This bound excludes canonical admission,
archive I/O, attention generation, expanded output and arbitrary-precision
growth. Cold admission uses a disk SQLite union index rebuilt from canonical
witness files when the archive opens; the response loop reads no interior
history. A new closure still needs canonical admission. Explicit `rebind` on a
changed window or coupling identity validates the archived wave, preserves hot
current, committed cut and closure/beat counts, and invalidates old bound
attention through a new generation. It establishes neither a new physical
coupling nor a resolution of pending obligations or scale formation. Exposure
returns the archived historical wave, while later response journaling through
`TraceStore` remains the caller's responsibility.
The frontier window is the host response-validity interval `[start,end)`;
native retained-return/RCD counting uses `(start,end]`. Rebinding the validity
domain neither recomputes RCD nor establishes physical time.

The separate local approximately 1.55× timing result applies only to a matched
four-view boundary helper, including its context creation and full audits.
It does not establish a FieldFrontier, SPARC, or whole-engine speedup. The CI
fixture's timing records likewise remain observations on the actual host.
