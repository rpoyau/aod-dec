# AOD lint integration and coverage

This repository uses [rpoyau/aod-lint](https://github.com/rpoyau/aod-lint/tree/68bc4795e982bbdeb266e346d694b08c7c49c844)
at commit `68bc4795e982bbdeb266e346d694b08c7c49c844`. Its actual implementation
parses `.typ` and `.tex` manuscripts. It has no Dec, Python, C++, Rust-source or
Markdown checker. The supplied compiler repository has no Typst or LaTeX
manuscript files, so manuscript coverage is **NOT_APPLICABLE**. This result does
not establish compiler correctness or scientific acceptance.

`scripts/check_aod_lint.py` checks the tool's revision and clean tracked source,
invokes its real `digest` command, and invokes strict `check` for every manuscript
found. It reports coverage and raw tool output. A successful empty digest is
reported as `NOT_APPLICABLE`, not a manuscript pass. If manuscript files are added
later, they are listed individually and their actual diagnostics are retained.
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
