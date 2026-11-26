# Migration & Compatibility Guide: Dec `v1` (`2bc927c`) and PR #8 (`68d6705`) to Pure-Wave Trit-Boxed `v3`

This document records the upstream caller audit, the `--compat v1` replay path for frozen `v1` (`2bc927c`) artifacts, the deterministic `aodc migrate` source-to-source translator for upgrading `v1` (`2bc927c`) and PR #8 (`68d6705`) `.dec` programs to Pure-Wave Trit-Boxed `v3`, and the complete line-by-line `stdout` delta audit across `manual_examples/{02,05,06,09}.stdout`.

---

## 1. Upstream Repository Caller Audit

All three upstream Alpha-Omega Dynamics (AOD) repositories were audited for `.dec` sources, `DecIR` bundles, and `aodc` CLI invocations:

| Upstream Repository | Audited Commit | `.dec` Files Found | `aodc` / `aod-dec` Invocations | Breakage Impact |
| :--- | :--- | :---: | :---: | :--- |
| `rpoyau/aod` | `1a80693` | `0` | `0` | None (zero external callers broken) |
| `rpoyau/aod-bundles` | `ca5170f` | `0` | `0` | None (zero external callers broken) |
| `rpoyau/aod-manual-volume-2` | `4820cca` | `0` | `0` | None (zero external callers broken) |

Within `rpoyau/aod-dec` itself, all 10 historical `v1` regression fixtures (`tests/fixtures/v1_examples/*.{dec,stdout}`) remain frozen byte-for-byte under `tests/fixtures/v1_examples/MANIFEST.txt` and `SOURCE_BINDING.json`.

---

## 2. Frozen `v1` (`2bc927c`) Replay Mode (`--compat v1`)

Default `aodc` (`--compat v3`) enforces Pure-Wave Trit-Boxed `v3` unconditionally and rejects legacy syntax (`Dec`, unprefixed `"..."` strings, `t32`, `str`, `if`/`elif`/`else`, `for`/`while`, mutable reassignment).

To check, execute, test, or compile frozen `v1` (`2bc927c`) `.dec` sources and `DecIR v1` bundles **byte-for-byte without modifying frozen inputs**, pass `--compat v1`:

```sh
# 1. Static check of a frozen v1 source
aodc check --compat v1 tests/fixtures/v1_examples/weights.dec

# 2. Direct deterministic VM execution (exact v1 stdout parity)
aodc run --compat v1 tests/fixtures/v1_examples/closure.dec

# 3. Regression suite sweep across all 10 frozen v1 fixtures
aodc test --compat v1 tests/fixtures/v1_examples

# 4. Compile frozen v1 source to hermetic DecIR v1 ("format": "aod.dec.ir", "version": 1)
aodc compile --compat v1 tests/fixtures/v1_examples/fusion.dec -o /tmp/fusion.v1.ir.json

# 5. Hermetically verify SHA-256 + canonical metadata and execute DecIR v1
aodc exec /tmp/fusion.v1.ir.json
```

### Verified `--compat v1` Guarantees

- **Strict grammar separation**: `--compat v1` accepts only Frozen Dec `v1` (`Dec <name>(...) -> <out_var>:`) and rejects `v3`-only syntax (`dec`, `1/3r`, `b"..."`, `1t`, `3:3:6`, `00_8`, `chamber`, `quarantine`, `struct`, `enum`, `use`) with `ParseError`.
- **Static definite-assignment verification**: Output bindings (`<out_var>`) must be initialized on all non-returning control-flow paths; immutable `let` locals cannot be reassigned (`TypeError`).
- **Exact witness & arithmetic invariants**: `Trace::step` (Hamming-1 adjacency), `Fusion::fuse` (independent outer closure), and `Coupling::contact` (distinct, strictly ordered return contacts) propagate `aod_core` domain/validation violations as `DiagnosticCode::RuntimeError` identically in both VM and `DecIR v1` execution.
- **Hermetic `DecIR v1` verification**: `aodc exec` auto-detects `"format": "aod.dec.ir"` with `"version": 1`, verifies `source_sha256` against embedded `source` bytes, re-runs `check_v1_source`, and checks canonical JSON equality before executing.

---

## 3. Source-to-Source Migration Translator (`aodc migrate`)

To upgrade legacy `v1` (`2bc927c`) or PR #8 (`68d6705`) `.dec` programs to canonical 4-space Pure-Wave Trit-Boxed `v3`, run `aodc migrate`:

```sh
# Print migrated Pure-Wave v3 source to stdout
aodc migrate legacy_input.dec

# Write migrated Pure-Wave v3 source to a distinct output file
aodc migrate legacy_input.dec -o upgraded_v3.dec

# Rewrite a working copy in place (refuses guarded frozen fixtures)
aodc migrate --write working_copy.dec

# Check whether a file is already canonical migrated v3 (exit 0 if clean, non-zero on diff)
aodc migrate --check working_copy.dec
```

### Summary of `aodc migrate` Transformations

| Legacy Construct (`v1` @ `2bc927c` / PR #8 @ `68d6705`) | Pure-Wave Trit-Boxed `v3` Output |
| :--- | :--- |
| `Dec name(p1, ...) -> out:` + `(T p1, ...) -> Ret out:` | `dec name:` with indented `(p1: T', ...) -> Ret':` signature arm(s) |
| `Dec name(p1: T, ...) -> Ret:` | `dec name:` with indented `(p1: T', ...) -> Ret':` signature arm |
| Output assignment `out = expr` | SSA binding + explicit `return <out_ssa>` |
| `use a::b::{x, y}` brace imports | Indented `use a::b:\n    x, y` (or `use a::b::x` for 1 item) |
| Missing `v1` builtin imports (`trace`, `step`, `probability`, `select`, ...) | Auto-injected `use aod::core:...`, `use aod::witness:...`, `use aod::ledger:...` |
| Unprefixed `"..."` / `'...'` string literals | `b"..."` byte literals with canonical ASCII / `\xHH` escaping |
| `str` / `QExt<str>` | `bytes` / `QExt<bytes>` |
| `u8` / `u8(x)` | `b8` / `b8(x)` |
| `t32`, `t9`, `int8`, `int16`, `int32`, `int64` types | Unified Trit-Boxed `int` |
| `t32(N)` / `t32(expr)` | `N` (integer literal) / `int(expr)` |
| `t1(-1)`, `t1(0)`, `t1(1)` | `-1t`, `0t`, `1t` |
| `array[T, N]` | `[T'; N]` |
| Brace `struct S { x: T }` / `enum E { A, B(T) }` | Indented `struct S:` / `enum E:` |
| Brace struct literal `S { field: val, ... }` | Positional constructor `S(val, ...)` (ordered by declared struct fields) |
| Brace map literal `{k: v, ...}` | `map[(k, v), ...]` |
| `a if cond else b` | `select(cond', a', b')` |
| `if` / `elif` / `else` blocks (returns or local mutations) | `return select(...)` chains and SSA `select(cond', v_then, v_else)` merges |
| Mutable locals (`T x = ...`, `mut x = ...`, `x = ...`, `xs[i] = v`) | Immutable SSA `let` bindings and `set_at(xs, i, v)` |
| Imperative `for i in a..b:` / `for x in xs:` loops | Synthesized pure step `dec` + `evolve(init, dom, step_fn[, env])` |
| `r0.model_status == "DECLARED_..."` | `r0.model_status_bytes == b"DECLARED_..."` |
| `rec.stage == "reclosed"` | `rec.stage_bytes == b"reclosed"` |
| `audit.scientific_status == "UNRESOLVED_PROVENANCE_QUARANTINED"` | `audit.scientific_status == ScientificStatus::UnresolvedProvenanceQuarantined` |

---

## 4. Line-by-Line `stdout` Delta Audit: PR #8 (`68d6705`) vs Pure-Wave `v3`

Across `manual_examples/01..09`, fixtures `01`, `03`, `04`, `07`, and `08` have **zero `stdout` delta** (100% byte-identical between PR #8 `68d6705` and `v3`). Every changed line in `02`, `05`, `06`, and `09` is accounted for below.

### 4.1 `manual_examples/02_closure_wavelets.stdout`

Lines `1–10` and `13–18` are unchanged. Only lines `11–12` changed due to `str` elimination (`RecipientOutcome.stage` is now the typed `<builtin>` `RecipientStage` enum, while raw status octets live on `.stage_bytes: bytes`):

| Line | PR #8 (`68d6705`) `stdout` | Pure-Wave `v3` `stdout` | Exact Cause of Delta |
| :---: | :--- | :--- | :--- |
| `11` | `reclosed` | `b"reclosed"` | `rec_ok.stage` (`str`) split into typed `RecipientStage::Reclosed` (`.stage`) + `bytes` literal rendering on `emit rec_ok.stage_bytes`. |
| `12` | `reclosed_out_of_window` | `b"reclosed_out_of_window"` | `rec_oow.stage` (`str`) split into typed `RecipientStage::ReclosedOutOfWindow` (`.stage`) + `bytes` literal rendering on `emit rec_oow.stage_bytes`. |

### 4.2 `manual_examples/05_conditional_si_metrology.stdout`

Lines `1–6` and `18` are unchanged. Lines `7–17` changed because `qext_render` now returns `QExt<bytes>` instead of `QExt<str>` (eliminating UTF-8 `str` across the `Q_ext` boundary while preserving every exact rational numerator and denominator):

| Line(s) | PR #8 (`68d6705`) `stdout` | Pure-Wave `v3` `stdout` | Exact Cause of Delta |
| :---: | :--- | :--- | :--- |
| `7` | `Q_ext(1/4596315885)` | `Q_ext(b"1/4596315885")` | `QExt<str>` -> `QExt<bytes>` rendering on `qext_render(t_3_2)`. |
| `8` | `Q_ext(13788947655/1)` | `Q_ext(b"13788947655/1")` | `QExt<str>` -> `QExt<bytes>` rendering on `qext_render(nu_3_2)`. |
| `9` | `Q_ext(17/18385263540)` | `Q_ext(b"17/18385263540")` | `QExt<str>` -> `QExt<bytes>` rendering on `qext_render(t_10_22)`. |
| `10` | `Q_ext(64348422390/17)` | `Q_ext(b"64348422390/17")` | `QExt<str>` -> `QExt<bytes>` rendering on `qext_render(nu_10_22)`. |
| `11` | `Q_ext(1/919263177)` | `Q_ext(b"1/919263177")` | `QExt<str>` -> `QExt<bytes>` rendering on `qext_render(t_nonrec)`. |
| `12` | `Q_ext(0/1)` | `Q_ext(b"0/1")` | `QExt<str>` -> `QExt<bytes>` rendering on `qext_render(nu_nonrec)`. |
| `13` | `Q_ext(-4596315885/1)` | `Q_ext(b"-4596315885/1")` | `QExt<str>` -> `QExt<bytes>` rendering on `qext_render(nu_signed)`. |
| `14` | `Q_ext(14204057517679/10000)` | `Q_ext(b"14204057517679/10000")` | `QExt<str>` -> `QExt<bytes>` rendering on `qext_render(h_mid)`. |
| `15` | `Q_ext(10111894947/2500000000000000)` | `Q_ext(b"10111894947/2500000000000000")` | `QExt<str>` -> `QExt<bytes>` rendering on `qext_render(aug_ua_hz)`. |
| `16` | `Q_ext(919263177/625000000000000)` | `Q_ext(b"919263177/625000000000000")` | `QExt<str>` -> `QExt<bytes>` rendering on `qext_render(aug_ub_hz)`. |
| `17` | `Q_ext(10111894947/10000000000000000)` | `Q_ext(b"10111894947/10000000000000000")` | `QExt<str>` -> `QExt<bytes>` rendering on `qext_render(tab1_ub_hz)`. |

### 4.3 `manual_examples/06_unbiased_chamber_sandbox.stdout`

Lines `2–7` (`12`, `2`, `1`, `1`, `8`, `8`) and line `10` (`0`) are unchanged. Lines `1`, `8`, and `9` changed due to `str` -> `bytes`, typed `<builtin>` `ModelStatus` (`ModelStatus::DeclaredSandboxNotS3Authenticated` on `r0.model_status` vs raw bytes on `r0.model_status_bytes`), and full `ShedWitness` serialization in `ChamberState::to_json` / `final_state_hash`:

| Line | PR #8 (`68d6705`) `stdout` | Pure-Wave `v3` `stdout` | Exact Cause of Delta |
| :---: | :--- | :--- | :--- |
| `1` | `DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED` | `b"DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED"` | `SimResult.model_status` is now typed `<builtin>` `ModelStatus::DeclaredSandboxNotS3Authenticated`; `06` asserts both `r0.model_status` and `r0.model_status_bytes` and emits `r0.model_status_bytes` (`bytes`). |
| `8` | `ecaa97d8719459d69913923f9653f1c2a6df208a34bfe9a5f3d43145e376d4c5` | `b"75d25f3f9b5c1e84b14f983040fd3540198dfb1ee81febe85d7f84e75bef90cd"` | (1) `final_state_hash` changed from `str` to 64-byte hex `bytes` (`b"..."`), and (2) `ChamberState::to_json` now hashes the full retained `shedding_events` (`Vec<ShedWitness>`) and chamber dynamics records rather than only `shedding_event_count`, while preserving `r0.final_state_hash == r1.final_state_hash == r2.final_state_hash`. |
| `9` | `∂box([0..64, 0..64, 0..64])` | `b"\xe2\x88\x82box([0..64, 0..64, 0..64])"` | Boundary operator `∂r0` now returns `bytes` instead of `str`, escaping the 3-byte UTF-8 prefix of `∂` (`0xE2 0x88 0x82`) in canonical `b"..."` form. |

### 4.4 `manual_examples/09_s2_higgs_and_quarantined_lensing.stdout`

Lines `1–6` (`0/1`, `79/50`, `0`, `61`, `8`, `1/100`) and line `16` (`0`) are unchanged. Line `7` changed from a raw string tag to the typed `ScientificStatus` enum, and lines `8–15` changed from `QExt<str>` to `QExt<bytes>`:

| Line | PR #8 (`68d6705`) `stdout` | Pure-Wave `v3` `stdout` | Exact Cause of Delta |
| :---: | :--- | :--- | :--- |
| `7` | `UNRESOLVED_PROVENANCE_QUARANTINED` | `ScientificStatus::UnresolvedProvenanceQuarantined` | `SwellsTauAudit.scientific_status` upgraded from `str` tag to typed `ScientificStatus` enum (`stdlib::aod::sectors::ScientificStatus`). |
| `8` | `Q_ext(15616/125)` | `Q_ext(b"15616/125")` | `QExt<str>` -> `QExt<bytes>` on `qext_render(e_gev)`. |
| `9` | `Q_ext(-0.933333)` | `Q_ext(b"-0.933333")` | `QExt<str>` -> `QExt<bytes>` on `DisplayQuarantine::round_display(cms_z, 6)`. |
| `10` | `Q_ext(1/3000000)` | `Q_ext(b"1/3000000")` | `QExt<str>` -> `QExt<bytes>` on `qext_render(cms_z_res)`. |
| `11` | `Q_ext(0.089571)` | `Q_ext(b"0.089571")` | `QExt<str>` -> `QExt<bytes>` on `DisplayQuarantine::round_display(cms_ape, 6)`. |
| `12` | `Q_ext(-2.085714)` | `Q_ext(b"-2.085714")` | `QExt<str>` -> `QExt<bytes>` on `DisplayQuarantine::round_display(atl_z, 6)`. |
| `13` | `Q_ext(0.233190)` | `Q_ext(b"0.233190")` | `QExt<str>` -> `QExt<bytes>` on `DisplayQuarantine::round_display(atl_ape, 6)`. |
| `14` | `Q_ext(2467/20000000)` | `Q_ext(b"2467/20000000")` | `QExt<str>` -> `QExt<bytes>` on `qext_render(lo)`. |
| `15` | `Q_ext(2469/20000000)` | `Q_ext(b"2469/20000000")` | `QExt<str>` -> `QExt<bytes>` on `qext_render(hi)`. |
