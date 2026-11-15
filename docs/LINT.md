# AOD lint integration and coverage

This repository uses [rpoyau/aod-lint](https://github.com/rpoyau/aod-lint/tree/68bc4795e982bbdeb266e346d694b08c7c49c844)
at commit `68bc4795e982bbdeb266e346d694b08c7c49c844`. Its actual implementation
parses `.typ` and `.tex` manuscripts. It has no Dec, Python, C++, Rust-source or
Markdown checker. The language and compiler manual at
`docs/language-and-compiler.typ` is now an applicable manuscript input. Its
strict check and digest are retained with the documentation evidence. This
does not establish compiler correctness or scientific acceptance.

`scripts/check_aod_lint.py` checks the tool's revision and clean tracked source,
invokes its real `digest` command, and invokes strict `check` for every manuscript
found. It reports coverage and raw tool output. A successful empty digest is
reported as `NOT_APPLICABLE`, not a manuscript pass. Manuscript files are listed
individually and their actual diagnostics are retained.
The wrapper's exit status is zero when applicability was established without a
tool failure; consumers must read the JSON `status` and `manuscript_file_count`.

The tool's upstream `Cargo.lock` is ignored. The dependency resolution actually
used here is preserved as `evidence/pr-import/lint.Cargo.lock`; its hash, source
revision, binary hash, Rust/Cargo versions and commands are recorded in
`evidence/pr-import/lint-build.json`. The initial missing-cache attempt and the
successful fresh offline build are retained. No source in the lint repository
was modified.

With access to that private repository, reproduce the pinned tool and its checks:

```sh
git clone git@github.com:rpoyau/aod-lint.git ../aod-lint
git -C ../aod-lint checkout --detach 68bc4795e982bbdeb266e346d694b08c7c49c844
cp evidence/pr-import/lint.Cargo.lock ../aod-lint/Cargo.lock
cargo test --locked --manifest-path ../aod-lint/Cargo.toml
cargo build --locked --manifest-path ../aod-lint/Cargo.toml
python3 scripts/check_aod_lint.py --checkout ../aod-lint \
  --binary ../aod-lint/target/debug/aod-lint --output build/aod-lint.json
```

Use `--offline` with Cargo only when the locked crates are already cached. Do not
feed `.dec`, Python or Markdown files to upstream `check`: its CLI routes every
non-`.tex` file through Typst syntax and that would misstate coverage. The tool's
frozen-main rules refer to manuscript ownership, not a Git branch named `main`.
Current owner instructions and the bound compiler model govern this project.

Upstream's finite Stokes tests use floating-point tolerances and caller-provided
flux/divergence. They are tested as tool regression cases; they do not replace
this compiler's exact rational and witness conformance evidence. Native/compiler
acceptance remains the versioned test corpus, runtime tests, isolation records
and measured adaptation controls described in the execution evidence.

The CI native semantic tests are independent of the sibling private repositories.
A missing private-tool checkout is an unavailable lint run, not a lint pass.

The original `evidence/pr-import/lint-coverage.json` remains the historical empty
scan made before the manual existed. Current documentation evidence is recorded
in `evidence/typst-documentation.json`; the manuscript build and executable
examples have separate results. The upstream Typst pass checks a limited set of
reference, vague-range, midstream-import and undeclared-manifold diagnostics.
It does not establish Typst compilation, resolve Main/Manual LaTeX equation
labels, validate Dec code blocks, or check rendered layout. Its digest also treats label literals inside `link`
calls as labels; the compiled Typst document resolves the actual internal links.
These limitations are preserved rather than interpreting the tool's rubric bits
as a scientific review.

The tesseract-cell companion adds a second actual Typst input. The subsequent
two-file digest and strict checks are retained in
`evidence/cell-jitter/aod-lint.json`, with PDF compilation and rendered inspection
reported separately in `evidence/cell-jitter/validation.json`. This later coverage
does not change the earlier one-file and empty-scan records.

The Manual-alignment correction retains its new two-file scan and PDF checks in
`evidence/manual-alignment/`. Its scientific findings come from a bounded
source/code review and exact counterchecks, not the manuscript lint pass.

`docs/chiral-core-collision.typ` adds a third applicable manuscript. Its current
scan and application validation are recorded under `evidence/chiral-collision/`;
read the recorded file list, diagnostics and status for actual coverage. The
Markov kernel, exact tickets, retained return histories and native replay require
their separate executable checks. Manuscript lint establishes no physical
proton–proton calibration or cross-section prediction.

`docs/routing-parallel.typ` adds a fourth manuscript. Its actual scan and build
coverage are retained in `evidence/routing-parallel/`. Lint does not check GPU
availability, exact backend equivalence, worker participation, retained route
admission, or physical proton correspondence. Those require their source-bound
reviews and executable evidence; a missing GPU is not a successful hardware run.

The retained-continuation, boundary-accounting, particle-examples and
region-current companions add four actual Typst inputs. Current publication
evidence records the inspected files and hashes under `evidence/r04-feedback/`,
`evidence/particle-accounting/` and `evidence/scoped-current/`. The earlier
four-file and smaller scans remain historical records. Use the emitted current
file list to determine coverage rather than inferring it from the historical
counts above.

Strict manuscript checks do not validate source-family completeness, moving
sweep coverage, exact cached current values, pending projection persistence,
particle formation witnesses or compiler execution. Those claims use their
separate source-bound tests and review. Typst compilation and rendered-page
inspection likewise remain separate from lint.

The SPARC reconstruction companion adds an actual Typst manuscript. Its strict
scan, PDF build and reconstruction checks are recorded under
`evidence/sparc-reconstruction/`. Its stellar column projection and inverse
fit are scoped data calculations. Neither lint nor those fits establish a
G2/G3 galaxy evolution, native beat calibration or a complete physical error
budget. The native span extractor requires actual validated histories.

The reduced SPARC companion adds `docs/sparc-reduced.typ`. Its current strict
manuscript scan is retained under `evidence/sparc-reduced/`, separately from
the executed recurrence, complete-cohort score, exact moving-boundary audits
and deterministic trace comparisons. No physical accuracy claim follows from
the manuscript lint result.
