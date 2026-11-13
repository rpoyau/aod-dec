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
