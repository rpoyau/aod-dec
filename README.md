# `rpoyau/aod-dec` v2 — Native Reference Engine & Dec Compiler

> **Explicit Scope & Non-Claims Disclosure (`P0-2`):**
> The chamber simulator in `crates/aodc/src/sim.rs` is explicitly tagged on every
> simulation record and snapshot as:
>
> `model_status: "DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED"`
>
> It is a deterministic, zero-RNG, exact $\mathbb{Z}/\mathbb{Q}$ sandbox model
> executing Manual §4 kernel/hinge-slide and `SupportForm` shedding rules with
> zero hardcoded physical target outcomes and structural zero-back-reaction for
> attached observers. It is **not** an S3-authenticated physical derivation, does
> **not** infer SI metrology or universal physical constants, and makes **zero**
> edits to upstream `rpoyau/aod`.

## Architecture (100% `std`-only 4-Crate Rust Workspace)

- **Zero external `crates.io` dependencies** (`cargo test --workspace --offline`).
- **Zero IEEE-754 floating-point types or literals** anywhere across all 4 crates (`aod-core`, `aod-rt`, `aod-llvm`, `aodc`).
- **Strict `unsafe` quarantine:** `#![forbid(unsafe_code)]` on `aod-core` and `aodc`; `unsafe` strictly limited to the 5 C ABI / LLVM 18 / hardware probe files (`aod-rt/src/abi.rs`, `aod-llvm/src/{ffi.rs,aot.rs,jit.rs,hw_probe.rs}`).
- **Four acyclic crates (`aod-core` → `aod-rt` → `aod-llvm` → `aodc`):**
  - [`crates/aod-core`](crates/aod-core): arbitrary-precision `BigInt`, canonical exact `Ratio`, quarantined `qext::QExtRatio`, checked `T1`/`T32`/`Q4`/`Addr8`, pure-`std` FIPS 180-4 `Sha256`, strict float-free `JsonValue`, `SupportForm`, v1-exact `Trace`/`Fusion`/`Coupling` dual JSON witness primitives, complete M3 `F01`–`F07` `wavelet` & `RecipientLattice`, and Manual §4 D.E.C. `ledger`.
  - [`crates/aod-rt`](crates/aod-rt): `rlib` + `staticlib` (`libaod_rt.a`) owning the shared sandbox chamber engine, safe 1-based `RtArena` with `max_entries` cap, deep-copy/copy-on-write container semantics, exact cold-spill math, and panic-safe flat C ABI (`#[unsafe(no_mangle)]` + `catch_unwind`).
  - [`crates/aod-llvm`](crates/aod-llvm): `dlopen`-loaded LLVM 18 C API verifier/optimizer (`LLVMReturnStatusAction = 2`, `default<O0..O3>`), parameterized `CodegenPlan`, CPU microarchitecture discovery (`hw_info`), 5 live self-verifying ORC LLJIT tick/click micro-probes (`hw_probe`), zero-float paired percentile bootstrap & rollback-safe `RetainedProfile` learner (`learner`, tagged `COMPILER_EXECUTION_POLICY_NOT_PHYSICAL_AOD_LAW`), standalone PIC AOT ELF emitter/linker, and in-process ORC LLJIT session (`OrcJitSession`).
  - [`crates/aodc`](crates/aodc): Dec v1 & v2 frontend (`struct`, `enum`, exhaustive `match`, deterministic `map[K, V]`, multi-arm pattern dispatch), all 5 compile-time firewalls, hermetic `DecIR v1`/`v2` verifier, exact reference VM, parameterized unboxed-first safe textual LLVM 18 IR lowering (`src/codegen.rs`), and CLI.

## Compile-Time Firewalls

1. **Transitive Read-Only Observer Firewall (`P1-3`):** `observe` / `tracer` / `sensor` probes and every `Dec` function transitively called inside them check under `Effect::ReadOnlyObserver` with `ReadOnly<ChamberView>`. Mutating `ReadOnly<T>`, stripping `ReadOnly<T>` into non-`ReadOnly` parameters/constructors, mutating outer state, or invoking `simulate` fails with `ObserverMutationForbidden`.
2. **One-Way `quarantine Q_ext` Firewall (`P0-1`):** `ext_decimal` resides strictly in `aod::qext` (`Effect::QExt`). `quarantine Q_ext:` is permitted only as a one-way sink inside pure-native `Dec main()` (or `quarantine Q_ext` functions), reading only immutable (`let`/frozen) native bindings, forbidding outer writes, escaping `return`/`break`/`continue`, transitive `simulate`, and `QExt<T>` inside native signatures/`struct`/`enum`/`map`.
3. **Odd-Length `_yro` Firewall:** Odd support lengths without `_yro` (e.g. `1:2:9`) and even lengths with `_yro` (e.g. `3:3:6_yro`) are rejected at compile time.
4. **Zero Implicit Coercions & Zero Floats Firewall:** Float literals (`1.5`, `5.`, `.5`, `1e3`) and implicit mixed-type operations (`int + t32`, `int + ratio`) fail at compile time.
5. **Exhaustive Unbiased Checking & Hermetic `DecIR` Verifier (`P1-2`):** Every item and overload arm (including unreachable code) is checked, and `aodc exec` hermetically rebuilds and verifies canonical JSON equality from embedded module sources + SHA-256 digests with zero ambient filesystem reads.

## CLI Usage

```bash
# Typecheck a Dec v2 or Dec core v1 source file
cargo run --bin aodc -- check tests/fixtures/v2_smoke/opposing_336_chamber.dec
cargo run --bin aodc -- check --compat v1 tests/fixtures/v1_examples/closure.dec

# Execute directly via the exact reference VM
cargo run --bin aodc -- run [--fuel N] [--max-depth N] tests/fixtures/v2_smoke/opposing_336_chamber.dec

# Emit verified & optimized LLVM 18 IR
cargo run --bin aodc -- emit-llvm -O2 tests/fixtures/v2_bench/unboxed_loop_bench.dec

# Build a standalone native AOT ELF executable (zero libLLVM/libpython/libgmp runtime deps)
cargo run --bin aodc -- build -O2 tests/fixtures/v2_bench/unboxed_loop_bench.dec -o /tmp/unboxed_bench
/tmp/unboxed_bench --fuel 100000 --max-depth 128

# Execute in-process via LLVM 18 ORC LLJIT (optionally with validated retained profile)
cargo run --bin aodc -- jit -O2 [--profile /tmp/prof.json] tests/fixtures/v2_bench/unboxed_loop_bench.dec

# Discover host CPU microarchitecture and run 5 live self-verifying JIT micro-probes
cargo run --bin aodc -- probe-hw [--output /tmp/hw_report.json]

# Learn a semantically-guarded, honest compile-inclusive JIT execution policy profile
cargo run --bin aodc -- learn --baseline O0 --fuel 2000000 --pairs 6 --heldout 8 --profile /tmp/prof.json tests/fixtures/v2_bench/learn_amortized_bench.dec

# Run 5-mode exact parity & steady-state speedup benchmark (VM, AOT -O0/-O2, ORC JIT -O0/-O2)
cargo run --bin aodc -- bench tests/fixtures/v2_bench/unboxed_loop_bench.dec

# Compile to hermetic DecIR JSON and execute hermetically
cargo run --bin aodc -- compile tests/fixtures/v2_smoke/m3_wavelet_recipient.dec -o /tmp/m3.ir.json
cargo run --bin aodc -- exec /tmp/m3.ir.json

# Verify the modular Typst manual (6 passes: Typst lexer, Main refs, zero rasters, aod-lint, 3-mode Dec, JSON parity)
cargo run --bin aodc -- manual-verify docs/manual
cargo run --bin aodc -- manual-verify docs/manual --json

# Run Dec tests on a file or directory (including sibling .stdout and .expect_error checks)
cargo run --bin aodc -- test --compat v1 tests/fixtures/v1_examples
cargo run --bin aodc -- test tests/fixtures/v2_smoke
cargo run --bin aodc -- test manual_examples
```

## Native Dec Standard Library (`stdlib/aod/*.dec`) & Executable Manual Fixtures

- **`stdlib/aod/`:** Pure `.dec` modules resolved via `use stdlib::aod::...` (`--stdlib <dir>` / `AOD_STDLIB_DIR` / automatic repo discovery, hermetically bundled into `DecIR v2`):
  - `ledger.dec` (Manual §4 D.E.C. kernels, blocked dwell/hinge slide, tunnelling clip, ADAR/SADAR, Tau missing-burden, `R02` gap)
  - `wavelet.dec` (M3 12 Duon variants, same-current Duad seating, half-open window counts, opposing cancellation, exact cyclotomic $\mathbb{Z}_{12}$ DFT in $\mathbb{Q}[x]/(x^4 - x^2 + 1)$)
  - `boundary.dec` (Manual §§5–6 11-bin double-slit cross-return $\delta_3$, short-window clipping, purely native `3:3:7_yro` saddle, moving boundary, Stokes cancellation)
  - `cells.dec` (M3 4-cube measures, quarter-symmetry unit-disk enclosures `n=1..16`, rectangle metric/coord swap, exact discrete-law moments, affine covariance, 16-step tesseract transport)
  - `metrology.dec` (native turn advances, `prop:m3-atomic-gain`, and strictly quarantined `quarantine Q_ext SiMetrology:` for $\Delta\nu_{\text{Cs}}$, Hellwig H-maser, and separate NIST-F2 uncertainty budgets)
- **`manual_examples/`:** 6 positive fixtures (`01_dec_ledger`..`06_unbiased_chamber_sandbox` with verified sibling `.stdout`) and 6 negative controls (`negative_controls/nc01..nc06` with `.expect_error`), verified byte-identical across Exact VM, AOT `-O0`/`-O2`, ORC LLJIT `-O0`/`-O2`, and hermetic `DecIR v2`.

## Modular M4-Aligned Typst Manual & Verification (`docs/manual/`)

- **`docs/manual/`:** Modular Typst manual (`main.typ`, `refs.bib`, 7 General Mechanics chapters, 5 pure-vector native Typst figures, and 3 exact-integer JSON data files) bound read-only to vendored `/bindings/aod/aod_schema.typ` and `/bindings/aod/main_registry.json`.
- **Six-pass `aodc manual-verify docs/manual [--json]`:** Verifies `#include`/`#import` containment & `SOURCE_BINDING.json` hashes, balanced Typst scanning, all Main registry/internal/BibTeX references, zero rasters/`@preview` packages, pure-Rust equivalents of all 4 `rpoyau/aod-lint` `TypstAstPass` rules, 3-mode (`VM`/`AOT -O2`/`ORC JIT -O2`) live execution of inline Dec blocks and all 12 `@dec-file` Manual fixtures, and live field-by-field parity of `docs/manual/data/*.json`.
- **Zero-raster PDF auditor (`scripts/audit_pdf_zero_raster.py`):** Stdlib-only Python 3 script (zero bare `assert`s) inspecting uncompressed and `/FlateDecode`-compressed PDF streams (including `/ObjStm`), rejecting `/Subtype /Image`, `/ImageB`/`/ImageC`/`/ImageI`, inline `BI..ID..EI` images, and unresolved references while verifying `--min-pages` and byte-identical `--compare-pdf` SHA-256 determinism.

### Offline vs. CI Verification Matrix

| Gate | Offline Local | GitHub Actions CI |
| :--- | :---: | :---: |
| `RUSTFLAGS="-D warnings" cargo test --workspace --offline` (debug & `--release`) | Verified | Verified |
| `aodc test` (`v1_examples` 10/10, `v2_smoke` 3/3, `manual_examples` 12/12) | Verified | Verified |
| `aodc probe-hw` & `aodc learn` (honest compile-inclusive amortization) | Verified | Verified |
| `aodc manual-verify docs/manual` & `--json` (all 6 passes live) | Verified | Verified |
| `scripts/test_audit_pdf_zero_raster.py` (`python3` & `python3 -O`) | Verified | Verified |
| `typst compile --root . --creation-timestamp 0` (`v0.13.1`, double-build PDF + `audit_pdf_zero_raster.py`) | Not run locally (`typst` absent offline) | Verified in `.github/workflows/ci.yml` |
| External `rpoyau/aod-lint check --strict` over `docs/manual/**/*.typ` | Mirrored in Pass 4 offline | Verified in `.github/workflows/ci.yml` |

## Provenance & Upstream Bindings

All upstream AOD commit/tree hashes, equation numbers (`S0@98ddfa0` resolved against vendored read-only `bindings/aod/main_registry.json` from `S1b@cea71a7`), verbatim historical v1 `v40.01r10` provenance, and the 10 frozen v1 regression hashes are recorded in [`SOURCE_BINDING.json`](SOURCE_BINDING.json).
