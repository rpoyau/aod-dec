# `rpoyau/aod-dec` Evolution Plan — Zero-Python Pure-Native Roadmap (Atop `main@68d6705`)

**Current Baseline:** `rpoyau/aod-dec` `main@68d6705` (PR `#8` merged; post-merge CI run `45337785074` **3/3 GREEN**: Rust Native Engine 70/70 + 30/30 S2 vectors, Typst `0.15.1` 18-page zero-raster PDF, Strict `rpoyau/aod-lint` 13/13).  
**Core Principle:** **`aod-dec` requires ZERO Python anywhere** — not in the compiler, not in simulations, not in manual verification, not in PDF auditing, and not in CI. Everything executes through compiled Rust (`aod-core`, `aod-rt`, `aod-llvm`, `aodc`) + native Dec v2 + LLVM 18 + Typst `0.15.1`.

---

## Remaining Python Footprint Found on `main@68d6705`
1. `scripts/audit_pdf_zero_raster.py` & `scripts/test_audit_pdf_zero_raster.py` (the ONLY 2 `.py` files left in the entire repository).
2. 3 `python3` lines in `.github/workflows/ci.yml` (`lines 60, 61, 96`) invoking that PDF auditor.
3. Legacy Python prototype PRs `#1`–`#7` still sitting open/draft on GitHub (`rpoyau/aod-dec`).

---

## Phased Execution Plan (Lead Orchestrator → Planner → Plan Review → Implementer → Validator → Final Review)

### Phase A — PR 7: Zero-Python Purification & Legacy PR Retirement *(Ready Immediately)*
- **Pure-Rust `aodc audit-pdf` (`crates/aodc/src/pdf_audit.rs`):**
  - Replace `scripts/audit_pdf_zero_raster.py` with a 100% `std`-only, `#![forbid(unsafe_code)]`, zero-float, zero-external-dep Rust PDF auditor (`aodc audit-pdf [--min-pages N] [--compare-pdf <pdf2>] [--json] <pdf>`).
  - Include a compact pure-Rust RFC 1950/1951 zlib/DEFLATE inflater (stored, fixed Huffman, and dynamic Huffman blocks) with strict decompression size caps (`<= 32 MiB` per stream) so Typst `0.15.1` `/FlateDecode` `/ObjStm` & content streams are inspected natively with **zero `flate2`, zero `libz`, and zero Python**.
  - Preserve every existing check: `/Subtype /Image` XObjects (raw & Flate `/ObjStm`), inline `BI..ID..EI` raster streams, PDF string-literal filtering (zero false positives on prose or deprecated `/ProcSet` boilerplate), unresolved markers (`??`, `[unresolved]`, `Unresolved Main/internal/citation`, `PANIC`), `--min-pages`, and `--compare-pdf` exact SHA-256 determinism.
- **Delete All Python Files & Enforce Zero-Python Guard:**
  - Delete `scripts/audit_pdf_zero_raster.py` and `scripts/test_audit_pdf_zero_raster.py` (and remove empty `scripts/`).
  - Port all 6+ synthetic PDF unit tests (plus real zlib/DEFLATE compressed stream tests and decompression-bomb caps) to `crates/aodc/tests/pdf_audit.rs`.
  - Add a permanent **Zero-Python Repository Audit** to `crates/aodc/tests/source_binding.rs`: fail `cargo test` if ANY `.py` file exists in the repo OR if `.github/workflows/ci.yml` invokes `python`/`python3`!
- **Zero-Python CI & Docs:**
  - Update `.github/workflows/ci.yml`: pass the built `aodc` release binary (or build `aodc` cleanly) to `typst-pdf-and-zero-raster-audit` so CI runs `aodc audit-pdf --min-pages 6 --compare-pdf ...` + `pdfimages -list == 0` with **zero `python3` invocations anywhere**.
  - Update `docs/manual/chapters/06_validation_and_limits.typ` and `README.md`.
- **Retire Legacy Python PRs `#1`–`#7`:**
  - Close PRs `#1`–`#7` on `rpoyau/aod-dec` cleanly with an archival reference to `main@68d6705` (PR `#8`) and `--compat v1` frozen parity.

---

### Phase B — PR 8: Stage 4 (`S4`) Cross-Repo Alignment with `rpoyau/aod` PR `#53` *(Unblocked Now!)*
Upstream `rpoyau/aod` just opened **PR `#53`** (`Stage 4 (S4): Optional std-only zero-float Rust exact core (crates/aod-core)` @ `e0134017a816c699ee71a4342044c007b63917db`).
- Append `S4@e013401` (`DRAFT_CANDIDATE_UNDER_S4_REVIEW`, PR `#53`) to `SOURCE_BINDING.json`.
- Add the exact **90-tick `cycle_shedding_trace`** kernel (matching upstream `wavelet_shedding_simulation.csv` bit-for-bit in `aod-core` and in a pure-`.dec` Manual example `manual_examples/10_s4_cycle_shedding_90tick.dec` across VM, AOT, and ORC LLJIT!).
- Add `aodc eval-op <operation> --input <json>` and `aod.s4.rust.report.v1` compatible report mode with pre-resolution symlink/repo-source `--report` output-path guard so `aod-dec` and `rpoyau/aod` share one exact zero-float Rust contract (100% native Rust, zero Python).

---

### Phase C — PR 9: Practical Unbiased Apparatus Ergonomics, Native Manual Data Pipeline & Hardware Learner v2 *(Unblocked Now)*
Make writing, running, analyzing, and publishing AOD Manual experiments effortless **entirely inside Dec v2 + `aodc`**:
1. **Unbiased Apparatus Ergonomics & Native Trace Analysis:**
   - Reusable apparatus templates in `stdlib/aod/apparatus.dec` (opposing beam/shell preparation `3:3:6`, concentric virtual shell sensors, passive path/closure tracers, blind post-run summary aggregation, exact-integer JSON/CSV trace export directly from `aodc` — zero Python plotting/analysis scripts!).
   - Clean syntax sugar for multi-shell chambers and read-only sensor sweeps while preserving all 5 compile-time firewalls (`ObserverBackReaction`, `QExtLeakageForbidden`, etc.).
2. **Native Pure-Rust Manual Data Regeneration (`aodc manual-data`):**
   - Regenerate and diff-check `docs/manual/data/*.json` directly from compiled `aodc` runs so the entire Typst pure-vector figure pipeline is 100% Rust + Dec + Typst.
3. **Hardware Learner v2 (Real Microarchitecture-Driven Lowering):**
   - Extend PR 4's `CodegenPlan` beyond `-O`/branchless/unroll to drive **SoA vs AoS array layout lowering** (guided by Probe 4's `prefer_soa`), **CPU-targeted SIMD/vector exact `t32`/`i64` batch loops** (guided by `hw_info` `avx2`/`bmi2` + Probe 1 L1 working set), and **parallel pure read-only observer execution** (guided by Probe 5 scaling), still gated by Exact Semantic Guard, zero-float bootstrap, compile-inclusive amortization, and rollback-safe profiles.

---

### Phase D — PR 10+: Canonical Multi-Shell Chamber Physics *(Gated on Upstream `rpoyau/aod` Stage 3 `S3`)*
- Once `rpoyau/aod` finishes its ongoing Manual refactor and AF-authenticates explicit D.E.C. multi-shell interaction laws (`S3`), implement canonical unbiased chamber simulations (`-----> 3:3:6 ((( ))) [ chamber ] ((( ))) 3:3:6 <-----` for `pp -> Tau`, electron–wire, double-slit, SPARC `G2/G3`) with passive tracers/virtual sensors, blind post-run discovery, and strictly separated `quarantine Q_ext` metrology — zero hardcoded outcomes, zero Python.
