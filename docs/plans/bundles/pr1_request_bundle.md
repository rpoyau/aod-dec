# PR 1 Request Bundle (Rev 2 — Reviewer-Amended & Approved) — `rpoyau/aod-dec` v2

**Target Repo / Branch:** `/home/antigravity/rpoyau/aod-dec` on `v2/native-reference-engine` (base commit `79bf6594a14be02e6203fae365386193b7f89740`)  
**Host Toolchain Verified:** `rustc 1.100.0 (0ef1bc386 2026-11-09)`, `cargo 1.100.0 (3acc79c0c 2026-11-05)`  
**Hard Constraints:** 100% `std`-only Rust workspace (ZERO crates.io deps), `#![forbid(unsafe_code)]`, ZERO IEEE-754 floats (`f32`/`f64`) anywhere in `crates/aod-core` or `crates/aodc`, ZERO mutation of upstream `/home/antigravity/rpoyau/aod`, ZERO hardcoded physical outcomes.  
**Reviewer Findings Folded:** `P0-1`, `P0-2`, `P0-3`, `P0-4`, `P1-1`, `P1-2`, `P1-3`, `P2-1`, `P2-2`.

---

## 1. Provenance & `SOURCE_BINDING.json` Contract (`P2-1`)

### Grounded Commits & Exact SHA-256 Digests
- **Historical v1 Baseline (`rpoyau/aod-dec` `origin/codex/sparc-field-window`):**
  - Commit: `2bc927c6718def4df14e4c9af69b9d52c23008df` | Tree: `0478fa50cb4200a3e7832c86bfa6c03c7de00378`
  - Original `SOURCE_BINDING.json` SHA-256: `e5acb188c5597afd69cb95c41f49b1b7fed1b6d8d1f043ea9f1d4939734a1535`
- **Upstream `rpoyau/aod` Stage `S0` (`m3-manual-si-alignment`, `CANONICAL_VERSION.txt` = `v41.02`):**
  - Commit: `98ddfa0ebb38293adfa99c8c11da5a15cdd9eea3` | Tree: `b4a667e3809e4a145580686d439811834b936343`
  - Reference verifier `manual/scripts/aod_manual_wavelet_si_alignment.py` SHA-256: `aab7da3d5e552f9fcdb04814f01253c19fb8a39120316db7e9032fca5330629d`
- **Upstream `rpoyau/aod` Stage `S1b` (`m4-typst-s1b-vector-figures`, `CANONICAL_VERSION.txt` = `v41.02`):**
  - Commit: `cea71a76f26c08b603866e746867b3982da6c88e` | Tree: `49cccab0bbe29403051a0c63cc2c971824d16791`
  - Vendored read-only snapshots copied byte-for-byte from `/home/antigravity/.gemini/jetski/brain/v1/default/jetski_remote_agents_3p_devbox/ef0a0bf4-8c4a-436f-8e3c-7a7217a2cb94/scratch/inspect/aod-m4/` into `bindings/aod/`:
    - `bindings/aod/main_registry.json` (`sha256: e9044676f91fdcaeffa8f1a1d21d605358a0923ec9fd6788146822b6d496c884`)
    - `bindings/aod/aod_schema.typ` (`sha256: 4e6861360657c906ab18499afd1c114b87c8521ead1df471ddc28ab8e9c44d46`)
- **`SOURCE_BINDING.json` (`schema_version: 2`, strict `P2-1` split):**
  - `upstream_bindings`:
    1. `S0@98ddfa0` record (source commit/tree/version + `aod_manual_wavelet_si_alignment.py` SHA-256) with `numbers_resolved_from: "S1b@cea71a7/typst/lib/main_registry.json"` and **explicitly expanded** equation list (no `..` ranges!), including:
       `eq:first-anchor` `(3)`, `eq:hinge-grammar` `(5)`, `eq:atomic-h1` `(8)`, `eq:closure-trace` `(12)`, `eq:closure-certificate` `(13)`, `eq:closure-phase` `(16)`, `eq:return-identity` `(20)`, `eq:beat-reclosure` `(21)`, `eq:closure-phase-window` `(23)`, `eq:wave-identity` `(24)`, `eq:duon-current` `(30)`, `eq:branch-hinge-rd` `(31)`, `eq:branch-hinge-rd-symmetric` `(32)`, `eq:duad-same-current` `(33)`, `eq:curling-support-model` `(36)`, `eq:key-proposed-admitted-trace` `(38)`, `eq:fold-outer-closure` `(42)`, `eq:expose-inner-lineage` `(43)`, `eq:single-hinge-symmetric-fixture` `(51)`, `eq:aod-wavelet-continuation` `(57)`, `eq:wavelet-overlap-dedup` `(58)`, `eq:wavelet-cancellation-retention` `(59)`, `eq:window-clipped-rho` `(64)`, `eq:cadence-ratio` `(65)`, `eq:relational-time` `(68)`, `eq:wavelet-reference-timing` `(69)`, `eq:hinge-saturation` `(74)`, `eq:hinge-saturation-weighted` `(75)`, `eq:hinge-saturation-branch-continuation` `(76)`, `eq:flux-types` `(77)`, `eq:b-scoped-channel-pressure` `(83)`, `eq:b-scoped-stokes` `(88)`, `eq:moving-boundary` `(94)`, `eq:duonic-pressure` `(97)`, `eq:sadar-orientation-antisymmetry` `(101)`, `eq:burden-overhang` `(110)`, `eq:closure-residual` `(112)`, `eq:shedding-excess` `(113)`, `eq:shed-continuation-witness` `(116)`, `eq:recipient-event-lattice` `(117)`, plus all 9 explicit M3 equations (`168`–`176` resolved straight from `bindings/aod/main_registry.json`) and `eq:odd-support-index` `(179)`.
    2. `S1b@cea71a7` record with vendored paths & exact SHA-256 digests (`main_registry.json`, `aod_schema.typ`).
  - `historical_bindings`: preserves the **verbatim** original v1 `v40.01r10` `SOURCE_BINDING.json` JSON object under `original_record`, with `source_commit`, `source_tree`, and `original_file_sha256`.
  - `frozen_v1_regression_manifest`: references `tests/fixtures/v1_examples/MANIFEST.txt` and all 10 `(src_sha256, stdout_sha256)` pairs copied byte-for-byte from `scratch/inspect/aod-dec-v1/examples/` and `scratch/inspect/aod-dec-v1/expected_stdout/`.

---

## 2. Exact File Tree

```text
aod-dec/
├── Cargo.toml                              # Workspace ["crates/aod-core", "crates/aodc"], resolver="2", ZERO external deps
├── .gitignore
├── README.md                               # v2 architecture, PR 1 CLI usage, explicit non-claims & sandbox model disclosure
├── SOURCE_BINDING.json
├── bindings/
│   └── aod/
│       ├── README.md
│       ├── main_registry.json
│       └── aod_schema.typ
├── crates/
│   ├── aod-core/
│   │   ├── Cargo.toml                      # std-only, ZERO dependencies
│   │   ├── src/
│   │   │   ├── lib.rs                      # #![forbid(unsafe_code)], ZERO f32/f64
│   │   │   ├── error.rs
│   │   │   ├── bigint.rs
│   │   │   ├── ratio.rs                    # Native exact Ratio (NO decimal parser here!)
│   │   │   ├── qext.rs                     # Quarantined external types: QExtRatio, parse_decimal_exact (P0-1)
│   │   │   ├── ternary.rs                  # T1, T32, Q4, Addr8
│   │   │   ├── sha256.rs
│   │   │   ├── json.rs                     # Strict JsonValue (no floats, no dup keys, Python surrogate-pair ASCII escaping)
│   │   │   ├── support.rs                  # SupportForm (BigInt Pi/RD/P^D/Q^D/X_shedding; proposed spec, not closure cert)
│   │   │   ├── witness.rs                  # v1-exact Trace/Fusion/Coupling + M3 eq:fold-outer-closure & eq:duad-same-current
│   │   │   ├── wavelet.rs                  # M3 Wavelet, ParentDescriptor (F04), OccurrenceRegistry (F05/F07), ScopeMap, CoordinateRelation (F03), RecipientLattice + Replay (F06), Cadence
│   │   │   └── ledger.rs                   # Manual §4 LedgerRow (Hamming-1 checked), Kernel, HingeSlide, TunnelClip, SadarPromotion, probability
│   │   └── tests/
│   │       ├── exact_arithmetic.rs
│   │       ├── support_and_ledger.rs
│   │       ├── witness_and_fusion.rs
│   │       └── m3_wavelet_lattice.rs
│   └── aodc/
│       ├── Cargo.toml                      # Depends ONLY on aod-core = { path = "../aod-core" }
│       ├── src/
│       │   ├── main.rs                     # CLI: check | run | compile -o | exec | test
│       │   ├── lib.rs                      # #![forbid(unsafe_code)]
│       │   ├── diagnostics.rs
│       │   ├── lexer.rs                    # Delimiter-depth joining, RatioLit, Addr8Lit, SupportLit, TritLit, WindowLit, float rejection (P1-1)
│       │   ├── ast.rs
│       │   ├── parser.rs                   # Single-arm & multi-arm Dec, inline `a if c else b`, apparatus DSL, `--compat v1`
│       │   ├── modules.rs                  # Built-in aod::* namespaces + filesystem `use` resolver with cycle detection
│       │   ├── types.rs                    # Type lattice, ReadOnly<T>, QExt<T>, Effect
│       │   ├── checker.rs                  # Typechecker + all 5 compile-time firewalls (including P0-1 & P1-3 transitive checks)
│       │   ├── ir.rs                       # Hermetic DecIR v1 & v2 (bundles resolved module sources + SHA-256) + strict rebuild verifier (P1-2)
│       │   ├── sim.rs                      # DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED deterministic exact engine + passive observers (P0-2)
│       │   └── eval.rs                     # Deterministic exact reference evaluator (v1 dual Trace JSON parity, Ratio `{n}/{d}`, fuel/depth budgets)
│       └── tests/
│           ├── v1_compat_parity.rs
│           ├── dec_v2_language.rs
│           ├── apparatus_and_firewalls.rs
│           ├── source_binding.rs
│           └── cli_integration.rs
└── tests/
    └── fixtures/
        ├── v1_examples/                    # All 10 frozen *.dec + *.stdout + MANIFEST.txt
        └── v2_smoke/
            ├── opposing_336_chamber.dec    # Unbiased 3:3:6 opposing chamber smoke fixture (zero hardcoded outcomes)
            ├── m3_wavelet_recipient.dec    # M3 wavelet, overlap dedup, cancellation, RecipientLattice
            └── qext_post_freeze.dec        # Native freeze -> one-way quarantine Q_ext sink
```

---

## 3. Exact `crates/aod-core` Invariants (Including `P0-1`, `P0-3`, `P0-4`, `P2-2`)

1. **`BigInt` (`bigint.rs`)**:
   - Canonical sign-magnitude (`Vec<u32>` limbs base $2^{32}$, zero has empty limbs).
   - `+`, `-`, `*`, unary `-`, `abs()`, `pow(u32)`, `gcd`, `lcm`, exact Python/v1 floor division `div_floor` / `rem_floor` / `div_rem_floor` (`b == 0 -> Err(ZeroDivision)`).
   - Non-negative bitwise operations (`bitand`, `bitor`, `bitxor`, `shl`, `shr` — reject negative operands with `CoreError::DomainError` so semantics are explicit and unambiguous, `P2-2`).
2. **`Ratio` (`ratio.rs`) & Quarantined `qext.rs` (`P0-1`)**:
   - Native `Ratio`: `den > 0`, `gcd(|num|, den) == 1`, `0` -> `0/1`, `to_int_exact()` rejects `den != 1`, `Display`/`render_v1` ALWAYS `"{num}/{den}"` (e.g. `4/1`, `0/1`, `-4/1` — `P0-3`).
   - **No decimal string parsing on native `Ratio`!** Place `QExtRatio(Ratio)` and `qext::parse_decimal_exact(&str) -> CoreResult<QExtRatio>` strictly inside `crates/aod-core/src/qext.rs` with strict grammar `^[+-]?[0-9]+(\.[0-9]+)?$` (rejecting whitespace, `.5`, `5.`, scientific notation, `NaN`/`Inf`, empty string).
3. **`T1`, `T32`, `Q4`, `Addr8` (`ternary.rs`)**:
   - `T1`: `{-1, 0, +1}`, checked ops, `render_v1` -> `"t1(v)"`.
   - `T32`: `[-926_510_094_425_920, +926_510_094_425_920]`, checked `i128` ops + floor div/rem, 2-bit `pack() -> u64`, `unpack(u64)` / `unpack_bigint(&BigInt)` rejecting `< 0`, `>= 2^64`, or any `0b11` lane with `InvalidTritEncoding(3)`, `render_v1` -> `"t32(v)"`.
   - `Q4`: `0..16`, `hamming_distance`, `is_hamming1`, `render_v1` -> `"q4(v)"`.
   - `Addr8`: retained octal digits (`Vec<u8>` non-empty, each `0..8`, preserving leading zeros so `00_8 != 0_8`), parses `00_8`, `00|-`, `0o00`, rejects `8`/`9`.
4. **`Sha256` & `JsonValue` (`sha256.rs`, `json.rs`, `P2-2`)**:
   - Pure `std` FIPS 180-4 SHA-256.
   - Strict `JsonValue` (no floats!). `to_compact_ascii_json` matches Python `json.dumps(..., sort_keys=True, separators=(",", ":"), ensure_ascii=True)` byte-for-byte including UTF-16 surrogate pairs for Unicode scalar values `> U+FFFF` and lossless `BigInt` serialization (`u64::MAX` etc.). `parse_strict_json` rejects duplicate object keys and float tokens.
5. **`SupportForm` (`support.rs`, `P2-2`)**:
   - Proposed specification + finite support-model accessor (`eq:key-proposed-admitted-trace`, explicitly NOT a closure certificate).
   - All computations (`Pi = q^p + q`, `RD = 2*Pi + H_mu`, `rho_window`, `pressure`, `overhang`, `shedding_audit`) use `BigInt`; rejects `p < 1`, `q < 1`, `len < 1`, `h_mu < 1`, `c3 < 0`, `c_close < 0`, odd `len` without `is_yro`, even `len` with `is_yro`.
6. **Witness Primitives (`witness.rs`, `P0-3`)**:
   - **Preserve v1 dual `Trace` JSON serialization byte-for-byte:**
     - `trace_prefix_json` (for SHA-256 closure/fusion digest): `"start": <int>`, `"parents": ["<last_closure_digest>", ...]`, NO `"closures"` field (matching `dec_core.py:213`).
     - `render_v1` (for `emit`): `"start": {"value": <int>}`, `"parents": [<full recursive Trace JSON>]`, WITH `"closures"` field (matching `dec_core.py:494`, verified against `closure.stdout` & `fusion.stdout`).
   - `Fusion::fold`, `outer`, `fuse`: exact v1 rules PLUS M3 `eq:fold-outer-closure` outer/inner closure digest disjointness.
   - `Coupling`: exact v1 rules (`cadence`, `lag`, `drift`).
   - M3 `eq:duad-same-current`: `DionPolarity`, `DuonVariant` (all 12 distinct variants), `DuonCurrent`, `seat_duad` / `seat_duad_checked` requiring `OpenDuon`, `hinge_0h == true`, and identical `duon_id`.
7. **M3 Wavelets & Recipient Lattice (`wavelet.rs`, `P0-4` `F01`–`F07` Complete Parity)**:
   - Keep M3 closure/return structures cleanly separate from v1 `Trace` (`M3ClosureTrace`, `AdmittedClosureRecord`, `ReturnWitness`).
   - **`F04` (`ParentDescriptor`):** typed enum `ParentDescriptor::{AdmittedNativeParent, AdmittedOriginClosure, DeclaredSyntheticParent, MappedConditionalParent}` carrying canonical `parent_root_key` distinguishing `(scope, lineage, scale, motif, origin_return_id, origin_ancestor, inner_wave_origins, motif_core)`. Enforce `origin_ancestor ∈ ancestry` for native wavelets; reject `is_admitted_wave = true` on `AdmittedOriginClosure`, `OpenDuon`, `ShedContinuation`, or `< 1` eligible return.
   - **`F01`/`F02`/`F05`/`F07` (`OccurrenceRegistry` & `ScopeMap`):** authoritative, non-mutating `OccurrenceRegistry` rejecting `return_id` payload collisions, occurrence-slot aliasing, cut-slot aliasing, non-monotonic cuts, and absent selected `return_id`s; provide pure `extend_occurrence_registry`. Explicit `ScopeMap` required across distinct scopes (rejects absent/boolean/fold-proposal substitutes and rejects attaching a `ScopeMap` when scopes already match). In `validate_complete_windows` with a `ScopeMap`, filter canonical records by full `source_parent_root_key` (`F07`) so unrelated parents sharing `(scope, lineage, scale)` do not trigger false failures while omitted/unbound own-parent returns fail strictly.
   - **`F03`/`F06` (`ShedWitness`, `CoordinateRelation`, `RecipientLattice`):** require `x_route > 0`; enforce explicit `CoordinateRelation` whenever `shed_witness.cut_coordinate != recipient.cut_coordinate` (and reject attaching one when coordinates already match); require explicit `StageEventWitness` for every visited stage with monotonic chronology `eff_emit_cut <= prev_stage_cut <= completion_cut`; require strongly typed `AdmittedClosureRecord` (no bare booleans); support cross-scope mapped replay (`F06`) returning `ReclosedReplay { closure_phase_delta: 0, reclosure_bip_delta: 0, occurrence_j: original_j }` on replaying `j=1` or `j=2` entries of `prior_certified_returns`; enforce `ClosedOrigin` -> `(1, 0)`, non-closing stages -> `(0, 0, blocks_later_seating=false)`, fresh in-window `Reclosed` -> `(1, 1)`.
   - `window_union_dedup` (`eq:wavelet-overlap-dedup`), `cancel_retain` (`eq:wavelet-cancellation-retention`), `native_cadence_ratio` (`>= 2` contacts, `q_r > 0`, coupled, non-aliased) vs `SingleContactStageAssociation` (`bound_contacts == 1`, `is_cadence_ratio = false`).
8. **Manual §4 D.E.C. Ledger (`ledger.rs`, `P2-2`)**:
   - `LedgerRow` enforces `node_v.is_hamming1(neighbor_v_e)` (`eq:atomic-h1`).
   - `Kernel::normalize`, `HingeSlide::block_dwell_and_slide` (blocking `(e_slot, 0_D)` only, retaining `0_H`, rejecting `sigma==0` in post-block slide pool), `TunnelClip::evaluate` (supporting both full slide-pool sum and single-branch tunnel selection e.g. `(e_3, +1)` -> `6/7`), `SadarPromotion::promote`, `normalize_weights`, `probability`.

---

## 4. Dec v2 Lexer, Parser, Firewalls, IR & Sandbox Engine (`crates/aodc`)

### 4.1 Concrete Lexer & Parser Disambiguation (`P1-1`, `P0-3`)
1. **Aggregate delimiter depth for implicit line joining:** track total open count across `(`, `[`, `{` vs `)`, `]`, `}` so half-open `[(0, 6], (4, 8]]` lexes cleanly without false bracket mismatch. Parser disambiguates after `( e1 , e2`: `]` -> `WindowLit(e1, e2)`, `)` -> 2-tuple, `,` -> N-tuple requiring `)`.
2. **`RatioLit`:** `[0-9][0-9_]*/[0-9][0-9_]*r` with **no spaces** around `/` (unary `-` produces `-1/3r`; `den == 0` rejected at compile time). Spaced/unsuffixed `a / b` stays binary division returning `ratio`.
3. **`Addr8Lit`:** `[0-9]+(\|-|_8)` (no space before `|-`/`_8`) and `0o[0-9]+` lex as `Addr8Lit` of type `addr8`, preserving exact leading-zero digits (`00_8 != 0_8`) and rejecting any digit `8`/`9` with `InvalidOctalAnchor`.
4. **`SupportLit`:** `[0-9]+:[0-9]+:[0-9]+(_yro)?` (no spaces around `:`); reject odd length without `_yro` and even length with `_yro` at compile time.
5. **Float rejection vs `..` range:** after `[0-9][0-9_]*`, reject `.` followed by `[0-9]` or dangling `.` (NOT `..`), and reject scientific notation `[0-9]+[eE][+-]?[0-9]+` (`FloatLiteralForbidden`). Thus `24..40` lexes as `Int(24) DotDot Int(40)`.
6. **`Dec` single-arm vs multi-arm:** after `Dec name(...) -> ... : NEWLINE INDENT`, peek for `(` starting an arm signature `(...) -> ... :` -> multi-arm overload/pattern dispatch; otherwise concise single-arm body.
7. **Inline ternary conditional:** `yes if cond else no` (right-associative, precedence below `or`, required by 4/10 v1 examples).
8. **v1 compat in both `--compat v1` and default `run` mode (`P0-3`):**
   - `Type name = expr` locals and multi-arm params/result-bindings are mutable (so `closure.dec` passes in both modes); `let` is strictly immutable; `mut` is explicit v2 mutable.
   - `int`/`t1`/`t32`/`q4`/`ratio`/`trace`/`fusion`/`coupling` work in both type and call-constructor positions.
   - Contextual identifiers (NOT hard keywords): `window`, `shedding`, `base`, `next`, `region`, `capture`, `clock`, `shells`, `space`, `walls`, `engine`, `at`, `dir`, `orient`, `center`, `radius`, `boundary`, `ticks`, `inject`.
9. **Full apparatus syntax (`P1-1`):** `use path::{A, B}`, named call args `name: expr`, `chamber Name:` (`space`, `walls`, `engine`), `prepare Name in Chamber:` (`inject`, `clock`, `let`, `dir: +X/-X/+Y/-Y/+Z/-Z`), `observe Name:` (`tracer`, `sensor`, `region = box([a..b, ...])`, `capture = [...]`, plus procedural read-only bodies `tracer name(view): ...` / `sensor name(view) -> T: ...`), `simulate Prep for N ticks`, dot field/method access, prefix `∂expr` / `boundary_of(expr)`, and `quarantine Q_ext`.

### 4.2 Hardened Compile-Time Firewalls (`P0-1`, `P1-3`)
1. **Read-Only Observer Firewall (Transitive):** `observe` / `tracer` / `sensor` declarations and ANY `Dec` function transitively called inside them check under `Effect::ReadOnlyObserver`. Chamber/boundary views carry `ReadOnly<T>`. Any field/index write on `ReadOnly<T>`, any outer-scope mutation, or any call to a `ChamberMut` / `simulate` / `QExt` operation fails with `ObserverMutationForbidden`. (Local scratch `mut` accumulators inside a pure observer helper are allowed ONLY if they do not mutate `ReadOnly<T>` or outer state; mutating `ReadOnly<T>` or outer state is strictly forbidden.)
2. **One-Way `quarantine Q_ext` Firewall (`P0-1`):**
   - `quarantine Q_ext Name:` top-level modules declare `const` and `Dec` items with `Effect::QExt` and types wrapped in `QExt<T>`.
   - `ext_decimal(str) -> QExt<Ratio>` lives ONLY in `aod::qext` (`Effect::QExt`).
   - Statement `quarantine Q_ext:` is **forbidden** inside `chamber`, `prepare`, `observe`, and native helper `Dec` functions (`QExtLeakageForbidden`); it is permitted as a **one-way sink** inside `Dec main()` (and inside `quarantine Q_ext` functions) reading only immutable (`let`/frozen) native bindings.
   - Inside `quarantine Q_ext:`: **forbid** writing to any outer native `mut` variable, **forbid** `return`/`break`/`continue` escaping out of the `quarantine Q_ext:` block into native control flow, and provide **zero** unwrap from `QExt<T>` to `T`.
   - Referencing any `QExt` binding/const/fn outside `quarantine Q_ext` fails with `QExtLeakageForbidden`.
3. **Odd-Length `_yro` Firewall**, **Zero Implicit Coercions & Zero Floats Firewall**, and **Exhaustive Unbiased Check + Hermetic `DecIR` Verifier (`P1-2`):**
   - `DecIR` uses `version: 1` for `--compat v1` and `version: 2` for Dec v2, embedding all resolved filesystem `use` module sources + SHA-256 digests in a canonical sorted `modules` map so `aodc exec` rebuilds hermetically with zero ambient filesystem reads.

### 4.3 Declared Deterministic Sandbox Engine (`crates/aodc/src/sim.rs`, `P0-2`)
Explicitly tagged everywhere (struct metadata, JSON archive, `README.md`) as:
`model_status: "DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED"` (deterministic, zero RNG, exact $\mathbb{Z}/\mathbb{Q}$, zero hardcoded physical outcomes):
- Carrier state: `(id, support: SupportForm, shells: u32, pos: [i64; 3], dir: AxisDir, orient: T1, q4: Q4, trace: Trace, dwell_blocked: bool)`.
- Per-tick deterministic step:
  1. Advance `pos` by `dir` (`OpenWall`: exit/absorb at `∂space`; `ClosedWall`: reflect `dir` and invert `orient` through structural hinge).
  2. Update each carrier's `Q4` trace deterministically via Manual §4 `Kernel` (`[1,1,4,2]`) / `HingeSlide` (`[3,3,1]`, deterministic tie-break, zero RNG).
  3. On cell coincidence between carriers, apply dwell-blocking (`(e_3, 0_D)` -> `HingeSlide`) and compute exact `SupportForm::shedding_audit` excess ($X_{\mathrm{shedding}} = \max(0, P^D - C_{\mathrm{close}})$), recording `ShedWitness` and `Wavelet` events when `X_shedding > 0`.
  4. Step decoupled `ReferenceClock` independently.
- **Structural Zero-Back-Reaction Guarantee:** `ChamberState::step()` takes NO observer references. After each tick, `&ChamberSnapshot` is passed read-only to attached `Tracer`s and `ShellSensor`s.
- Blind readouts (`blind_motif_spectrum`, `sensor_currents`) report purely structural counts (observed `SupportForm`s, returns, hinges, shedding counts, boundary crossings, and cadence strictly via `native_cadence_ratio` when `bound_contacts >= 2 && q_r > 0`, else `SingleContactStageAssociation` / `None`).

### 4.4 CLI Contract (`P0-3`, `P1-2`)
- `aodc run` & `aodc exec` print all `emit` outputs in order, followed by `render(main_result)` (matching all 10 frozen v1 `.stdout` files byte-for-byte; omit final line only if `main()` has unit return type `()`).
- `aodc compile src -o out` canonicalizes both paths before refusing `out == src`.
- `aodc test <file_or_dir>`: for a `.dec` file, typecheck and run all zero-arg `Dec test_*()` functions (or `main()` if none exist; if a sibling `<stem>.stdout` exists, also check exact stdout parity); for a directory, recursively discover sorted `*.dec` files; exit 0 iff all pass and `>= 1` test ran, else exit 1.

---

## 5. Acceptance Criteria (`AC-01`..`AC-23` — All Reviewer Amendments Included)
- `AC-01`..`AC-10`, `AC-13`..`AC-17`, `AC-19`..`AC-23`: unchanged from Rev 1, PLUS:
- `AC-03` also verifies `P2-1` (`numbers_resolved_from` split and explicit expanded equation list with zero `..` ranges).
- `AC-05` verifies `qext::parse_decimal_exact` resides strictly in `qext.rs` with strict decimal grammar rejections (`.5`, `5.`, `1e3`, whitespace, `NaN`).
- `AC-11` & `AC-12` explicitly cover ALL `F01`–`F07` cases from `aod_manual_wavelet_si_alignment.py` (`ParentDescriptor` scale-0 vs scale-1 root key, `AdmittedOriginClosure` non-promotion, `CoordinateRelation` required/forbidden checks, non-mutating authoritative registry + `extend_occurrence_registry`, cross-scope mapped replay `F06` at `j=1`/`j=2`, and multi-parent `F07` `source_parent_root_key` domain filtering).
- `AC-16` verifies `model_status == "DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED"` and bit-for-bit identical chamber final-state hash across 0, 1, and 4 observers.
- `AC-17` includes transitive helper mutation rejection inside `observe`.
- `AC-18` explicitly tests hostile (i) outer native `mut` write inside `quarantine Q_ext:`, (ii) `return` escaping `quarantine Q_ext:` in `main()`, (iii) `ext_decimal("1.25")` called in native `Dec`, `chamber`, `prepare`, and `observe`, plus positive `qext_post_freeze.dec` one-way sink.
- `AC-21` includes hermetic multi-file `DecIR v2` execution after deleting/moving original source directory (proving zero ambient filesystem reads).
