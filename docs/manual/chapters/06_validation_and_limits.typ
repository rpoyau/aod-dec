#import "/bindings/aod/aod_schema.typ": *

= Verification Contract, Audit Pipeline, and Declared Limits <sec:validation>

#aod-paragraph("Setup.")
Every artifact across #secref("sec:quickstart"), #secref("sec:types"), #secref("sec:firewalls"),
#secref("sec:stdlib"), #secref("sec:apparatus"), and #secref("sec:llvm-learner") is verified by an automated,
zero-dependency toolchain spanning Rust offline unit and integration tests, the six-pass `aodc manual-verify`
checker, the zero-raster PDF auditor (`scripts/audit_pdf_zero_raster.py`), and continuous integration
(`.github/workflows/ci.yml`).

#figure(
  kind: table,
  caption: [Verification matrix separating offline local checks from CI PDF and strict linter jobs.],
  table(
    columns: (auto, auto, 1fr),
    align: (left, left, left),
    toprule,
    [*Gate*], [*Environment*], [*Enforced Invariants*],
    midrule,
    [`cargo test --workspace --offline`], [Offline + CI], [4-crate debug and release builds (`-D warnings`), zero floats, zero external deps, 5-file `unsafe` quarantine, 5-mode parity (`VM`, `AOT -O0/-O2`, `JIT -O0/-O2`).],
    [`aodc test manual_examples`], [Offline + CI], [All 6 positive Manual fixtures (`.stdout` parity) and all 6 negative controls (`.expect_error` verification) -> `12 passed; 0 failed`.],
    [`aodc manual-verify docs/manual`], [Offline + CI], [6 passes: Typst lexer/containment, `SOURCE_BINDING.json` hashes, Main registry & BibTeX refs, zero rasters, exact 4 `aod-lint` rules, 3-mode inline/file Dec execution, and live JSON data parity.],
    [`scripts/audit_pdf_zero_raster.py`], [CI + Unit Tests], [Decompresses `/FlateDecode` & `/ObjStm` streams, rejects `/Subtype /Image` and inline `BI..EI` outside string literals, rejects unresolved refs, enforces `--min-pages` and byte-identical `--compare-pdf` SHA-256.],
    [`aod-lint check --strict`], [CI], [External `rpoyau/aod-lint` AST verification across every `.typ` chapter and figure under `docs/manual/`.],
    bottomrule,
  ),
)<tab:verification-matrix>

#aod-paragraph("Derivation.")
Running `aodc manual-verify docs/manual --json` produces a canonical float-free JSON summary containing exact
counts for scanned files, Main references, internal labels and references, citations, inline positive and
negative Dec blocks, `@dec-file` fixtures, JSON data files, raster violations, and `aod-lint` violations.
Because the offline development environment does not install the `typst` binary, local verification proves
all six compiler/source passes and the Python PDF auditor unit tests (under both `python3` and `python3 -O`),
while the CI workflow in `.github/workflows/ci.yml` installs pinned Typst `0.13.1`, `fonts-lmodern`, and
`fonts-dejavu-core` to execute double-build deterministic PDF compilation (`--root . --creation-timestamp 0`
with `SOURCE_DATE_EPOCH=0`) and run `scripts/audit_pdf_zero_raster.py` against the compiled PDFs.

#aod-paragraph("Falsification and Declared Limits.")
Three explicit limits remain permanently enforced and auditable (#tabref("tab:verification-matrix")):
1. *No Universal Seconds-Per-Bip Constant:* native AOD cadence ratios stay dimensionless exact rationals;
   all SI frequencies and uncertainties remain quarantined behind `quarantine Q_ext` (`R01`/`R02`).
2. *No Unauthenticated Physical Inference:* sandbox chamber models carry `DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED`,
   and learned JIT profiles carry `COMPILER_EXECUTION_POLICY_NOT_PHYSICAL_AOD_LAW`.
3. *No Fabricated Local PDF Claims:* local offline runs never claim to have executed `typst compile`; PDF
   determinism and zero-raster checks are gated in CI and unit-tested offline on synthetic valid and hostile
   PDF byte streams.
