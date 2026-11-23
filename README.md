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

## Architecture (PR 1 — 100% `std`-only Rust Workspace)

- **Zero external `crates.io` dependencies** (`cargo test --workspace --offline`).
- **`#![forbid(unsafe_code)]`** enforced across both workspace crates.
- **Zero IEEE-754 floating-point types or literals** anywhere in `crates/aod-core` or `crates/aodc`.
- **Two crates:**
  - [`crates/aod-core`](crates/aod-core): arbitrary-precision `BigInt` (with exact Python/v1 floor division and non-negative bitwise ops), canonical exact `Ratio` (`{num}/{den}`, zero decimal parsing on native `Ratio`), quarantined `qext::QExtRatio` & `qext::parse_decimal_exact`, checked `T1`/`T32` (2-bit pack/unpack rejecting `0b11`)/`Q4`/`Addr8` (preserving leading zeros so `00_8 != 0_8`), pure-`std` FIPS 180-4 `Sha256`, strict float-free `JsonValue` (Python ASCII surrogate-pair parity + duplicate-key rejection), `SupportForm` (`Pi`, `RD`, `P^D`, `Q^D`, `X_shedding`, odd-length `_yro` enforcement), v1-exact `Trace`/`Fusion`/`Coupling` dual JSON witness primitives + 12 `DuonVariant` / `seat_duad`, complete M3 `F01`–`F07` `wavelet` & `RecipientLattice` parity, and Manual §4 D.E.C. `ledger` (`LedgerRow`, `Kernel`, `HingeSlide`, `TunnelClip`, `SadarPromotion`).
  - [`crates/aodc`](crates/aodc): indentation-sensitive Dec v1 & v2 lexer/parser, built-in `aod::*` namespaces & filesystem `use` module loader with cycle detection, exhaustive typechecker enforcing all 5 compile-time firewalls (transitive read-only `observe`, one-way `quarantine Q_ext` sink, odd-length `_yro`, zero implicit coercions & zero floats, exhaustive checking + hermetic `DecIR v1`/`v2` rebuild verifier), deterministic sandbox chamber engine, exact reference evaluator, and CLI.

## Compile-Time Firewalls

1. **Transitive Read-Only Observer Firewall (`P1-3`):** `observe` / `tracer` / `sensor` probes and every `Dec` function transitively called inside them check under `Effect::ReadOnlyObserver` with `ReadOnly<ChamberView>`. Mutating `ReadOnly<T>`, mutating outer state, or invoking `simulate` fails with `ObserverMutationForbidden`.
2. **One-Way `quarantine Q_ext` Firewall (`P0-1`):** `ext_decimal` resides strictly in `aod::qext` (`Effect::QExt`). `quarantine Q_ext:` is permitted only as a one-way sink inside `Dec main()` (or `quarantine Q_ext` functions), reading only immutable (`let`/frozen) native bindings, forbidding outer writes and escaping `return`/`break`/`continue`, with zero unwrap from `QExt<T>` to `T`.
3. **Odd-Length `_yro` Firewall:** Odd support lengths without `_yro` (e.g. `1:2:9`) and even lengths with `_yro` (e.g. `3:3:6_yro`) are rejected at compile time.
4. **Zero Implicit Coercions & Zero Floats Firewall:** Float literals (`1.5`, `5.`, `.5`, `1e3`) and implicit mixed-type operations (`int + t32`, `int + ratio`) fail at compile time.
5. **Exhaustive Unbiased Checking & Hermetic `DecIR` Verifier (`P1-2`):** Every item and overload arm (including unreachable code) is checked, and `aodc exec` hermetically rebuilds and verifies canonical JSON equality from embedded module sources + SHA-256 digests with zero ambient filesystem reads.

## CLI Usage

```bash
# Typecheck a Dec v2 or Dec core v1 source file
cargo run --bin aodc -- check tests/fixtures/v2_smoke/opposing_336_chamber.dec
cargo run --bin aodc -- check --compat v1 tests/fixtures/v1_examples/closure.dec

# Execute directly via the exact reference evaluator
cargo run --bin aodc -- run tests/fixtures/v2_smoke/opposing_336_chamber.dec
cargo run --bin aodc -- run --compat v1 tests/fixtures/v1_examples/fusion.dec

# Compile to hermetic DecIR JSON and execute hermetically
cargo run --bin aodc -- compile tests/fixtures/v2_smoke/m3_wavelet_recipient.dec -o /tmp/m3.ir.json
cargo run --bin aodc -- exec /tmp/m3.ir.json

# Run Dec tests on a file or directory (including sibling .stdout parity checks)
cargo run --bin aodc -- test --compat v1 tests/fixtures/v1_examples
cargo run --bin aodc -- test tests/fixtures/v2_smoke
```

## Provenance & Upstream Bindings

All upstream AOD commit/tree hashes, equation numbers (`S0@98ddfa0` resolved against vendored read-only `bindings/aod/main_registry.json` from `S1b@cea71a7`), verbatim historical v1 `v40.01r10` provenance, and the 10 frozen v1 regression hashes are recorded in [`SOURCE_BINDING.json`](SOURCE_BINDING.json).
