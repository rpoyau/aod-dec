# `rpoyau/aod-dec` — Experimental Downstream Reference Engine, Clean-Break Dec Compiler, & Fixed-Point Self-Hosting Toolchain

> **Experimental Reference Engine & Scope Disclosure (`P0-2`):**
> `rpoyau/aod-dec` is an **experimental downstream reference engine** for
> Alpha-Omega Dynamics ($\mathrm{A}\Omega\mathrm{D}$) and Declared Edge Computation
> (D.E.C.), maintained independently until promoted by upstream $\mathrm{A}\Omega\mathrm{D}$
> integration review. Every chamber simulation record and snapshot in
> `crates/aod-rt/src/sim.rs` carries the explicit status tag:
>
> `model_status: "DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED"`
>
> It is a deterministic, zero-RNG, exact $\mathbb{Z}/\mathbb{Q}$ sandbox engine
> executing Manual §4 kernel/hinge-slide rules, `SupportForm` shedding witnesses,
> moving-boundary flux (`boundary_step`), finite contact-window TTL (`contact_ttl`),
> commensurate zero-drift resonance (`Endo` vs `Exo` routing), and composite outer
> carrier fusion (`fuse_on_resonance`) with field-for-field witness hashing in
> `state_json` / `final_state_hash`, structural zero-back-reaction for attached
> observers, and **zero** edits to upstream `rpoyau/aod`.

## Architecture (100% `std`-only 4-Crate Rust Workspace)

- **Zero external `crates.io` dependencies** (`cargo test --workspace --offline`).
- **Zero IEEE-754 floating-point types or literals** anywhere across all 4 crates (`aod-core`, `aod-rt`, `aod-llvm`, `aodc`) or Dec source programs (`ratio` is Dec's exact replacement for floats).
- **2-Bit Balanced Ternary `int` & Coupled 2-GPR `ratio` ("Trit-Boxed"):**
  - `null`: distinct ontological null primitive (`null == 0` and `null == false` are compile-time errors).
  - `t1`: single balanced trit in `{-1t, 0t, +1t}` (`0_H`, `0_D`) returned directly by `<=>`, `sgn`/`signum`, and `superpose_cancel`.
  - `int`: 1-GPR 8-byte Trit-Boxed `u64` packing up to 32 balanced trits inline (`00 = 0`, `01 = +1`, `10 = -1`, covering `±926,510,094,425,920`) and using the unused `11` top-lane tag (`0xD000... | handle`) to spill transparently to base-$3^{32}$ `BigTrit` limbs.
  - `ratio`: coupled 2-GPR 16-byte `{ num, den }` pair reduced via Division-Free Balanced-Ternary Stein GCD, plus compact 1-word `[num][11][den]` `pack_ratio` / `unpack_ratio` for `<= 31` combined trits.
  - `b8`, `b64`, `bytes`: non-arithmetic raw bit/byte containers (`b"..."` byte literals; UTF-8 `str` and `"..."` are eliminated) for binary I/O, SHA-256 (`sha256_bytes`, `hex_bytes`), ELF64 emission, `b64_bits(int)` / `u64_int(b64)`, and 0-cycle `b64(int)` / `int(b64)` 32-trit register views.
- **Clean-Break Pure-Wave Language, Rank Polymorphism, & 18 AFC Scope Operators:**
  - Pure indentation-structured `dec` signature-matrix arms with `where` guards, `mux`/`select`/`gate` phase gating, exhaustive `match` (`pat -> expr`), J-style static rank lifting on `[T; N]` / `[T]` (`ShapeMismatch` at compile time on mismatched fixed shapes), struct-field projection `bins.t0`, outer products `+:`, `*:`, adverbs `+/`, `*/`, `min/`, `max/`, `&/`, `|/`, `superpose/`, `+\`, `*\`, `min\`, `max\`, `∂`, `argmin`, `argmax`, mask-sieve `?>`, `|>` stream pipelines, `walk`/`settle`, and all **18 AFC scope, contraction, & exact math operators** (`evolve`, `scan`, `fold_seq`, `map_seq`, `filter_seq`, `tabulate`, `zip_seq`, `count`, `any`, `all`, `sum`, `dot`, `min`, `max`, `abs`, `pow`, `gcd`, `lcm`, plus `num`/`den`/`.num`/`.den`/`signum`/`clamp`) with zero `if`/`for`/`while`/`mut` or `{}` braces.
- **Strict `unsafe` quarantine:** `#![forbid(unsafe_code)]` on `aod-core` and `aodc` (including `dec_audit.rs` and `fmt.rs`); `unsafe` strictly limited to the 5 C ABI / LLVM 18 / hardware probe files (`aod-rt/src/abi.rs`, `aod-llvm/src/{ffi.rs,aot.rs,jit.rs,hw_probe.rs}`).
- **Four acyclic crates (`aod-core` → `aod-rt` → `aod-llvm` → `aodc`):**
  - [`crates/aod-core`](crates/aod-core): balanced-ternary `BigTrit`/`BigInt` (`checked_pow_bigint` 1M-bit ceiling), Division-Free Ternary Stein GCD `Ratio` + `pack_ratio`, quarantined `qext::QExtRatio`, checked `T1`/`B8`/`B64`/`Q4`/`Addr8`, pure-`std` FIPS 180-4 `Sha256`, strict float-free `JsonValue`, `SupportForm`, `Trace`/`Fusion`/`Coupling` witness primitives, complete M3 `F01`–`F07` `wavelet` & `RecipientLattice`, Manual §4 D.E.C. `ledger`, and Stage 2/Stage 4 exact reference verifiers (`s2_kernel`, `s2_vectors`, `s4_report`).
  - [`crates/aod-rt`](crates/aod-rt): `rlib` + `staticlib` (`libaod_rt.a`) owning the shared sandbox chamber engine (`ModelStatus`, `final_state_sha256`), Trit-Boxed `0xD000...` spill arena, `RtArena` with `max_entries` cap, copy-on-write container & `seq_buf` helpers, `aod::sys` ELF64 bootstrap I/O, and panic-safe flat C ABI (`#[unsafe(no_mangle)]` + `catch_unwind`).
  - [`crates/aod-llvm`](crates/aod-llvm): `dlopen`-loaded LLVM 18 C API verifier/optimizer (`LLVMReturnStatusAction = 2`, `default<O0..O3>`), parameterized `CodegenPlan` (including `trit_ssa_promotion`), CPU microarchitecture discovery (`hw_info`), **6 live self-verifying ORC LLJIT micro-probes** (`hw_probe`, including the 2-thread barrier-synchronized FRG double-slit cross-return ring-gate probe `frg_ring_gate` over `E4 RD=9`, `C6 RD=61`, and `C8 RD=137 T_sync=8357`), zero-float paired percentile bootstrap & rollback-safe `RetainedProfile` learner (`learner`, tagged `COMPILER_EXECUTION_POLICY_NOT_PHYSICAL_AOD_LAW`), standalone PIC AOT ELF emitter/linker, and in-process ORC LLJIT session (`OrcJitSession`).
  - [`crates/aodc`](crates/aodc): Clean-break Pure-Wave Dec frontend, deterministic canonical AST formatter (`src/fmt.rs`, `aodc fmt`), live-walk `00_8` scanner & 6-hook §5.17 native `.dec` auditor (`src/dec_audit.rs`, `aodc audit-dec`, `aod.dec.audit.v1`), all 5 compile-time firewalls, hermetic `DecIR v3` verifier, exact reference VM, Trit-Boxed SWAR & unboxed-`evolve` LLVM 18 IR lowering (`src/codegen.rs`), 4-stage fixed-point self-hosting `bootstrap-verify`, and CLI.

## Compile-Time Firewalls

1. **Transitive Read-Only Observer Firewall (`P1-3`):** `observe` / `tracer` / `sensor` probes and every `dec` function transitively called inside them check under `Effect::ReadOnlyObserver` with `ReadOnly<ChamberView>`. Calling copy-on-write mutators (`set_at`, `array_set`, `list_set`, `push`, `list_push`, `map_put`, `map_remove`) on `ReadOnly<T>`, stripping `ReadOnly<T>` into non-`ReadOnly` parameters/constructors, invoking `aod::sys` I/O, or invoking `simulate` fails with `ObserverMutationForbidden`.
2. **One-Way `quarantine Q_ext` Firewall (`P0-1`):** `ext_decimal(b"...")` resides strictly in `aod::qext` (`Effect::QExt`). `quarantine Q_ext:` is permitted only as a one-way sink inside pure-native `dec main()` (or `quarantine Q_ext` functions), reading only immutable native bindings, formatting strictly as `QExt<bytes>` (`Q_ext(b"...")`), and forbidding escaping `return`, transitive `simulate`, and `QExt<T>` inside native signatures/`struct`/`enum`/`map`.
3. **Odd-Length `_yro` Firewall:** Odd support lengths without `_yro` (e.g. `1:2:9`) and even lengths with `_yro` (e.g. `3:3:6_yro`) are rejected at compile time.
4. **Zero Implicit Coercions, Static Shape Agreement, & Zero Floats Firewall:** Float literals (`1.5`, `5.`, `.5`, `1e3`), static shape mismatches (`[int; 3] + [int; 4]`), and implicit mixed-type operations (`int + ratio`, `int + b8`, `int + b64`, `null == 0`) fail at compile time.
5. **Exhaustive Unbiased Checking & Hermetic `DecIR v3` Verifier (`P1-2`):** Every item and signature-matrix arm (including unreachable code) is checked, and `aodc exec` hermetically rebuilds and verifies canonical JSON equality (`"schema": "aod.dec.ir.v3"`, `"syntax_discipline": "pure_wave_v2"`) from embedded module sources + SHA-256 digests with zero ambient filesystem reads.

## CLI Usage

```bash
# Typecheck, canonically format, or run the 6-hook §5.17 native auditor on a clean-break Dec file
cargo run --bin aodc -- check examples/first_principles/00_primitives.dec
cargo run --bin aodc -- fmt [--check | --write | -o /tmp/formatted.dec] examples/first_principles/00_primitives.dec
cargo run --bin aodc -- audit-dec [--json] [--report /tmp/audit.json] examples/first_principles/00_primitives.dec

# Execute directly via the exact reference VM
cargo run --bin aodc -- run [--fuel N] [--max-depth N] tests/fixtures/v2_smoke/opposing_336_chamber.dec

# Emit verified & optimized Trit-Boxed LLVM 18 IR
cargo run --bin aodc -- emit-llvm -O2 tests/fixtures/v2_bench/unboxed_loop_bench.dec

# Build a standalone native AOT ELF executable (zero libLLVM/libgmp runtime deps)
cargo run --bin aodc -- build -O2 tests/fixtures/v2_bench/unboxed_loop_bench.dec -o /tmp/unboxed_bench
/tmp/unboxed_bench --fuel 100000 --max-depth 128

# Execute in-process via LLVM 18 ORC LLJIT (optionally with validated retained profile)
cargo run --bin aodc -- jit -O2 [--profile /tmp/prof.json] tests/fixtures/v2_bench/unboxed_loop_bench.dec

# Verify 4-stage fixed-point self-hosting bootstrap (Stage 0 -> stage1 -> stage2 -> stage3, SHA256(stage2) == SHA256(stage3))
cargo run --release --bin aodc -- bootstrap-verify [--json]

# Discover host CPU microarchitecture and run all 6 live self-verifying JIT micro-probes (incl. 2-thread frg_ring_gate)
cargo run --bin aodc -- probe-hw [--output /tmp/hw_report.json]

# Learn a semantically-guarded, honest compile-inclusive JIT execution policy profile
cargo run --bin aodc -- learn --baseline O0 --fuel 2000000 --pairs 6 --heldout 8 --profile /tmp/prof.json tests/fixtures/v2_bench/learn_amortized_bench.dec

# Run 5-mode exact parity & steady-state speedup benchmark (VM, AOT -O0/-O2, ORC JIT -O0/-O2)
cargo run --bin aodc -- bench tests/fixtures/v2_bench/unboxed_loop_bench.dec

# Compile to hermetic DecIR v3 JSON and execute hermetically
cargo run --bin aodc -- compile tests/fixtures/v2_smoke/m3_wavelet_recipient.dec -o /tmp/m3.ir.json
cargo run --bin aodc -- exec /tmp/m3.ir.json

# Regenerate or verify docs/manual/data/*.json strictly via Exact Reference VM (zero LLVM)
cargo run --bin aodc -- manual-data
cargo run --bin aodc -- manual-data --check --json

# Verify the modular Typst manual (6 passes: Typst lexer, Main refs, zero rasters, aod-lint, 3-mode Dec, JSON parity)
cargo run --bin aodc -- manual-verify docs/manual
cargo run --bin aodc -- manual-verify docs/manual --json

# Run multi-observer sandbox apparatus sequentially or with parallel read-only observer threads
cargo run --bin aodc -- run --parallel-observers manual_examples/11_apparatus_opposing_sensors.dec
cargo run --bin aodc -- jit -O2 --parallel-observers manual_examples/11_apparatus_opposing_sensors.dec

# Verify all 30 Stage 2 (S2@3122c94) vectors + 90-tick S4@e013401 convergence + canonical aod.s4.rust.report.v1
cargo run --bin aodc -- verify-s2
cargo run --bin aodc -- verify-s2 --json
cargo run --bin aodc -- verify-s2 --s4-report --report /tmp/s4_report.json

# Evaluate any single exact S2/S4 operation (24 positive golden ops + cycle_shedding_trace)
cargo run --bin aodc -- eval-op cycle_shedding_trace --input-json '{"initial_burden":"0","eta":"0.82","gamma":"0.55","lambda_reclose":"0.38","ticks":[{"lead":"0.8","lag":"0.2","compatibility":"0.55"}]}'

# Audit compiled PDFs (pure-Rust std-only RFC 1950/1951 zero-raster, unresolved-ref, page-count, & SHA-256 determinism)
cargo run --bin aodc -- audit-pdf --min-pages 6 --compare-pdf /tmp/manual-2.pdf /tmp/manual-1.pdf
cargo run --bin aodc -- audit-pdf --min-pages 6 --compare-pdf /tmp/manual-2.pdf --json /tmp/manual-1.pdf

# Run Dec tests on a file or directory (including sibling .stdout and .expect_error checks)
cargo run --bin aodc -- test tests/fixtures/v3_examples
cargo run --bin aodc -- test tests/fixtures/v2_smoke
cargo run --bin aodc -- test examples/first_principles
cargo run --bin aodc -- test manual_examples

# Replay frozen v1 (2bc927c) fixtures & DecIR v1 bundles byte-for-byte via isolated --compat v1
cargo run --bin aodc -- test --compat v1 tests/fixtures/v1_examples
cargo run --bin aodc -- compile --compat v1 tests/fixtures/v1_examples/fusion.dec -o /tmp/fusion.v1.ir.json
cargo run --bin aodc -- exec /tmp/fusion.v1.ir.json

# Deterministically migrate legacy v1 (2bc927c) or PR #8 (68d6705) sources to Pure-Wave v3
cargo run --bin aodc -- migrate [--check | --write | -o /tmp/upgraded.dec] legacy_input.dec
```

See [`docs/MIGRATION_V1_V2_TO_V3.md`](docs/MIGRATION_V1_V2_TO_V3.md) for the upstream caller audit, `--compat v1` replay guarantees, `aodc migrate` transformation table, and line-by-line `stdout` delta audit.

## Native Dec Standard Library, First-Principles Modules, Manual Fixtures, & Self-Host Compiler

- **`stdlib/aod/`:** Seven pure `.dec` modules resolved via `use stdlib::aod::...` (`--stdlib <dir>` / `AOD_STDLIB_DIR` / automatic repo discovery, hermetically bundled into `DecIR v3`):
  - `ledger.dec`, `wavelet.dec`, `boundary.dec`, `cells.dec`, `metrology.dec`, `sectors.dec`, `apparatus.dec`.
- **`examples/first_principles/`:** Seven canonically formatted (`aodc fmt --check`), 6-hook-audited (`aodc audit-dec --json`), 6-mode-verified first-principles modules (`00_primitives.dec`..`05_self_host_compiler_elf64.dec` + `kernel.dec`) paired with [`docs/FIRST_PRINCIPLES_DERIVATION.md`](docs/FIRST_PRINCIPLES_DERIVATION.md).
- **`manual_examples/`:** 11 positive fixtures (`01_dec_ledger`..`11_apparatus_opposing_sensors` with verified sibling `.stdout`) and 7 negative controls (`negative_controls/nc01..nc07` with `.expect_error`), verified byte-identical across Exact VM (sequential & `--parallel-observers`), AOT `-O0`/`-O2`, ORC LLJIT `-O0`/`-O2` (sequential & `--parallel-observers`), and hermetic `DecIR v3`.
- **`self_host/aodc_self.dec`:** Pure clean-break `.dec` compiler emitting standalone static libc-free Linux x86-64 ELF64 executables directly, passing all 6 `aodc audit-dec` hooks and verified to 4-stage fixed-point convergence (`SHA256(stage2) == SHA256(stage3)`) via `aodc bootstrap-verify`.

## Provenance & Upstream Bindings

All upstream AOD commit/tree hashes, equation numbers (`S0@98ddfa0` resolved against vendored read-only `bindings/aod/main_registry.json` from `S1b@cea71a7`), the Stage 2 golden-vector draft binding (`S2@3122c94`, `DRAFT_CANDIDATE_UNDER_S2_REVIEW`), the Stage 4 Rust exact-core convergence draft binding (`S4@e013401`, `DRAFT_CANDIDATE_UNDER_S4_REVIEW`), verbatim historical v1 `v40.01r10` provenance, and the 10 frozen `tests/fixtures/v1_examples/` SHA-256 hashes are preserved byte-for-byte in [`SOURCE_BINDING.json`](SOURCE_BINDING.json).
