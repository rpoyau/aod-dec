#import "/bindings/aod/aod_schema.typ": *

= Verification Contract, Audit Pipeline, and Declared Limits <sec:validation>

#aod-paragraph("Setup.")
Every artifact across #secref("sec:quickstart"), #secref("sec:types"), #secref("sec:firewalls"),
#secref("sec:stdlib"), #secref("sec:apparatus"), and #secref("sec:llvm-learner") is verified by an automated,
zero-dependency Rust toolchain spanning offline unit and integration tests, the six-pass `aodc manual-verify`
checker, the `std`-only zero-raster PDF auditor (`aodc audit-pdf`), and continuous integration
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
    [`cargo test --workspace --offline`], [Offline + CI], [4-crate debug and release builds (`-D warnings`), zero floats, zero external deps, zero Python files or CI invocations, 5-file `unsafe` quarantine, 5-mode parity (`VM`, `AOT -O0/-O2`, `JIT -O0/-O2`).],
    [`aodc test manual_examples`], [Offline + CI], [All 11 positive Manual fixtures (`.stdout` parity, including 90-tick `S4@e013401` cycle-shedding trace and 4-observer concentric shell apparatus) and all 7 negative controls (`.expect_error` verification) -> `18 passed; 0 failed`.],
    [`aodc manual-data --check`], [Offline + CI], [Zero-LLVM Exact Reference VM regeneration and byte-for-byte drift check for all 3 `docs/manual/data/*.json` files (`double_slit_delta3`, `disk_enclosure`, `sandbox_chamber_summary`), rejecting symlinks and disallowed `--output-dir` targets (exit `2`) before execution.],
    [`aodc verify-s2 --s4-report`], [Offline + CI], [All 30 Stage 2 (`S2@3122c94`) cross-language golden vectors (`24` `rational_core` + `6` `symbolic_and_display`), vendored 90-tick `S4@e013401` CSV exact conservation (`102`-digit denominator), `aod-core` policy audit, and byte-identical debug-vs-release `aod.s4.rust.report.v1` JSON (`18`/`5`/`6`/`1` rollup).],
    [`aodc eval-op <op>`], [Offline + CI], [Exact single-operation JSON evaluator covering all 24 positive S2/S4 operations plus multi-tick `cycle_shedding_trace`, enforcing `16 MiB` bounded reads, pre-execution symlink-safe `--report` validation (exit `2`), and atomic output writes.],
    [`aodc manual-verify docs/manual`], [Offline + CI], [6 passes: Typst lexer/containment, `SOURCE_BINDING.json` hashes (including S2 `.jsonl` and S4 `.csv` digests), Main registry & BibTeX refs, zero rasters, exact 4 `aod-lint` rules, 3-mode inline/file Dec execution (`18` `@dec-file` checks), and live JSON data parity.],
    [`aodc audit-pdf [--json]`], [Offline + CI], [Pure-Rust `std`-only RFC 1950/1951 inflater (`#![forbid(unsafe_code)]`, zero LLVM dependency), masks binary stream payloads before root scanning, fails closed on disallowed or corrupt filters, rejects `/Subtype /Image` XObjects, inline `BI..ID..EI` images, and unresolved reference markers, and enforces `--min-pages` and byte-identical `--compare-pdf` SHA-256.],
    [`aodc bootstrap-verify`], [Offline + CI], [Fixed-point self-hosting bootstrap (`Stage 0 -> stage1 -> stage2 -> stage3`) compiling `self_host/aodc_self.dec` to standalone static libc-free Linux x86-64 ELF64 binaries and verifying `SHA256(stage2) == SHA256(stage3)` plus VM/JIT stdout parity.],
    [`aod-lint check --strict`], [CI], [External `rpoyau/aod-lint` AST verification across every `.typ` chapter and figure under `docs/manual/`.],
    bottomrule,
  ),
)<tab:verification-matrix>

#aod-paragraph("Derivation.")
Running `aodc manual-verify docs/manual --json` produces a canonical float-free JSON summary containing exact
counts for scanned files, Main references, internal labels and references, citations, inline positive and
negative Dec blocks, `@dec-file` fixtures (`18`), JSON data files, raster violations, and `aod-lint` violations.
Running `aodc verify-s2 --s4-report --report <path>` independently verifies all 30 vendored Stage 2 golden
vectors, all 90 exact ticks of `wavelet_shedding_simulation.csv`, and the `aod-core` policy invariants into
a validated portable `aod.s4.rust.report.v1` JSON artifact that is byte-for-byte identical between debug and
release builds. Because the offline development environment does not install the `typst` binary, local
verification proves all six compiler/source passes, the 30/30 S2 golden-vector suite, the 90-tick `S4@e013401`
exact convergence check, the pure-Rust `aodc audit-pdf` RFC 1950/1951 inflater and hostile PDF test suite,
and the zero-Python repository/CI guard, while `.github/workflows/ci.yml` installs SHA-256-verified Typst `0.15.1`
(`a6d077d0a95eed5a2eba715b2dae06be954f624ccbf85758a03f389ded33118c`), `fonts-lmodern`, and `fonts-dejavu-core`
to execute double-build deterministic PDF compilation (`--root . --creation-timestamp 0` with
`SOURCE_DATE_EPOCH=0`) and run the built `aodc audit-pdf` binary (in both human and `--json` modes) alongside
`pdfimages -list` against the compiled PDFs without installing LLVM in the Typst job.

#aod-paragraph("Falsification and Declared Limits.")
Four explicit limits remain permanently enforced and auditable (#tabref("tab:verification-matrix")):
1. *No Universal Seconds-Per-Bip Constant:* native AOD cadence ratios stay dimensionless exact rationals;
   all SI frequencies, post-freeze Higgs energy maps, and rounding displays remain quarantined behind
   `quarantine Q_ext` (`R01`/`R02`).
2. *No Unauthenticated Physical Inference:* sandbox chamber models carry `DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED`,
   and learned JIT profiles carry `COMPILER_EXECUTION_POLICY_NOT_PHYSICAL_AOD_LAW`.
3. *Explicit Draft S2/S4 Provenance and Quarantine Preservation:* `S2@3122c94` (#aod-cite("aod-s2-candidate"))
   is tracked explicitly as `DRAFT_CANDIDATE_UNDER_S2_REVIEW` (upstream PR `#51`), `S4@e013401`
   (#aod-cite("aod-s4-candidate")) is tracked explicitly as `DRAFT_CANDIDATE_UNDER_S4_REVIEW` (upstream PR `#53`),
   and `RC-020` permanently preserves `UNRESOLVED_PROVENANCE_QUARANTINED` orthogonal to `status: "PASS"`.
4. *No Fabricated Local PDF Claims:* local offline runs never claim to have executed `typst compile`; PDF
   determinism and zero-raster checks are gated in CI and unit-tested offline on synthetic valid and hostile
   PDF byte streams.
