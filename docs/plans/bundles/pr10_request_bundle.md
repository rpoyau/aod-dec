# Master Purification Architecture & Phase D1 Request Bundle (`R02` — All `P0`/`P1`/`P2` Resolved)

Grounded in `rpoyau/aod-dec@1e5f224` (`v2/zero-python-evolution`), `aod-dec-dev/` (`SHA256SUMS` 15/15 OK), `rpoyau/aod` (`manual/sections/00_dec_ledger.tex`, vendored `S1b@cea71a7`), and `rpoyau/afc/README.md`.

---

## Part 1 — Milestone Architecture Breakdown (`Phase D1` -> `Phase D2` -> `Phase D3`)

| Milestone | Scope & Architectural Deliverables | Invariants & Cross-Phase Readiness (`P2-1`) |
| :--- | :--- | :--- |
| **Phase D1 (PR 10)** | 1. Hardware-honest register & sequence primitives (`Null`/`null`, `u8`, `b64`, `t9`, `bytes`/`b"..."`, `array[T, N]`, `0x`/`0b` literals, `T1`/`T32::superpose_cancel`, `is_hamming1`/`popcnt_xor`/`popcnt`, eager branchless `select(cond, a, b)`) across `aod-core`, `aod-rt`, `DecIR v2`, Exact VM, LLVM 18 AOT, and ORC LLJIT.<br>2. Byte-for-byte vendoring of `docs/FIRST_PRINCIPLES_DERIVATION.md`.<br>3. Live-walk octal-anchor (`00_8`) & monotonic sameness dwell-reduction (`xxxxx... is x`) + AST/type-backed six §5.17 Audit Hooks (`Spine`, `Boundary`, `SADAR`, `SheddicPath`, `Cycle`, `FRG`) in `crates/aodc/src/dec_audit.rs`, `aodc audit-dec`, and `.github/workflows/ci.yml` (`P2-2`).<br>4. Live 3-tier / two-thread barrier-synchronized shared-cache-line **FRG Double-Slit Cross-Return & Operative-Self Hardware Probe** (`T^x - T^0 = r_e`, zero `.max(1)` clamping, 3-state `Op_e` with `retained: Option<i64>`) in `crates/aod-llvm/src/hw_probe.rs`, `learner.rs`, and `aodc probe-hw`. | Zero floats, zero external crates, zero Python, `unsafe` strictly in the 5 allowlisted files, 10/10 `--compat v1`, 30/30 S2, 90-tick S4, 18/18 manual verify, PDF audit. |
| **Phase D2 (PR 11)** | 1. Lowercase `dec <name>:` family blocks with indented signature-combinatorics rows `(a: T, 0) [where pred] -> Ret:` (`AO_mu = <Pi_alpha, H_mu, Pi_Omega>` across `->`, unlisted/blocked rows `adm(e; B) = 0`), tuple return destructuring `let (x, y) = f(...)`, pure indented `use path:` blocks, uniform positional `TypeName(arg1, ...)` constructors (`FIRST_PRINCIPLES_DERIVATION.md` Step 2), and `map[(k, v), ...]` / `map[]` literals.<br>2. Engine-driven AFC Scope/Window & sequence operators (`evolve`, `scan`, `fold_seq`, `map_seq`, `filter_seq`, `tabulate`, `zip_seq`, `sum`, `min`, `max`, `abs`, `pow`, `gcd`, `lcm`, `dot`, `count`, `any`, `all`) across VM, AOT, and ORC LLJIT.<br>3. `SyntaxDiscipline::PureWaveV2` (`--strict-wave`) recorded in `DecIR v2` (`crates/aodc/src/ir.rs`) and `compute_profile_bindings` (`crates/aodc/src/lib.rs`) (`P2-1`) so `aodc exec` re-verifies under the exact same discipline. | All D1 + existing v2 stdlib/manual suites remain 100% green mid-migration; 6-mode VM/DecIR/AOT/JIT parity on all new operators. |
| **Phase D3 (PR 12)** | 1. Port all 7 `aod-dec-dev/dec/{00_primitives..05_self_host_compiler_elf64, kernel}.dec` modules into canonical Dec v2 under `examples/first_principles/` with 6-mode parity & `6/6` `aodc audit-dec` certification.<br>2. Provide `bytes`-backed signatures & fields (`P2-1`) for `aod::qext::ext_decimal(bytes)`, `qext_render`/`qext_format_fixed -> QExt<bytes>`, `addr8(bytes)`, `support(bytes)`, `trace(bytes, bytes, q4)`, `fold(trace, trace, bytes, bytes)`, `coupling(trace, trace, bytes)`, and typed `enum` + `bytes` fields on `SimResult`/`ChamberView`/`RecipientOutcome`/`CadenceAssociation`.<br>3. Migrate all 7 `stdlib/aod/*.dec`, 18 `manual_examples/`, `docs/manual/chapters/*.typ` + `manual-data`, and `tests/fixtures/v2_*` to pure wave Dec v2; flip `CompatMode::V2` unconditionally to `PureWaveV2` (banning `{ }`, single-header/uppercase `Dec`, `if`/`elif`/`else`, `for`, `while`, `break`, `continue`, `mut`, and primitive `str`; `--compat v1` strictly isolated) + permanent repo-wide pure-wave guard test. | 100% pure-wave Dec v2 across repo; `manual-data --check`, `manual-verify`, `verify-s2 --s4-report`, `aod-lint` (13/13), and 10/10 `--compat v1` byte-identical. |

---

## Part 2 — Complete File-by-File Request Bundle for Phase D1 (PR 10) (`R02`)

### 1. Exact Files to Create / Modify

#### Create
1. `docs/FIRST_PRINCIPLES_DERIVATION.md` — byte-for-byte copy of `aod-dec-dev/docs/FIRST_PRINCIPLES_DERIVATION.md` (`10,949` bytes, SHA-256 `091d8f1ef7dd3b9c1f96eab7adc9ff5db67194418c2dbfad1a0fcab68b5afabc`).
2. `crates/aodc/src/dec_audit.rs` (`#![forbid(unsafe_code)]`) — `lex_live_walk`, AST/type-backed six §5.17 Audit Hooks, `aod.dec.audit.v1` JSON report, and pre-read `--report` path guard.
3. `tests/fixtures/v2_smoke/hw_register_primitives.dec` (+ `.stdout`) — positive 6-mode parity fixture.
4. `crates/aodc/tests/phase_d1_purification.rs` — full `AC-01`..`AC-14` integration, parity, and negative-control suite.

#### Modify
1. `crates/aod-core/src/ternary.rs` & `crates/aod-core/src/bigint.rs`
2. `crates/aod-rt/src/{value.rs, ops.rs, abi.rs}`
3. `crates/aodc/src/{lexer.rs, ast.rs, types.rs, parser.rs, ir.rs, modules.rs, checker.rs, eval.rs, codegen.rs, lib.rs, main.rs}`
4. `crates/aod-llvm/src/{hw_probe.rs, plan.rs, learner.rs, lib.rs}`
5. `.github/workflows/ci.yml` (`P2-2`: add `./target/release/aodc audit-dec` check across `manual_examples/*.dec` with zero Python tokens) & `README.md`.

**Mandatory Static Token Constraint (`P1-3`):** `crates/aodc/tests/source_binding.rs:263-293` scans every non-`//` line in `crates/*/src/**/*.rs` (including inside string literals, error messages, raw LLVM IR templates, and `/* */` comments). Never write standalone alphanumeric/`_` tokens matching `f32`, `f64`, or `unsafe` (outside the 5 allowlisted files) anywhere in `crates/*/src/**/*.rs` (avoid hex constants like `0xf32` or variable names containing those exact tokens).

---

### 2. Exact Semantics, Signature Matrix, Firewalls, & LLVM 18 Lowering (`P0-1`..`P0-5`, `P1-1`)

#### 2.1 Lexer & `--compat v1` Isolation (`P0-3`)
- Add `tokenize_with_compat(source: &str, compat: CompatMode) -> DecResult<Vec<Token>>` in `crates/aodc/src/lexer.rs` (keeping `tokenize(source)` delegating to `CompatMode::V2` and calling `tokenize_with_compat(source, compat)` from `parse_source`).
- When `compat == CompatMode::V1`:
  - `0x`/`0X`, `0b`/`0B`, and `b"..."`/`b'...'` fail closed in the lexer with `LexError` / `ParseError`.
  - `null` remains an ordinary unreserved identifier in `CompatMode::V1`.
  - `Type::is_v1_type()` returns `false` for `Null`, `U8`, `B64`, `T9`, `Bytes`, `Array`, and all new builtins set `v1_allowed: false`.
- When `compat == CompatMode::V2`:
  - Hex & binary integer literals enforce strict grammar `0[xX][0-9a-fA-F](_?[0-9a-fA-F])*` and `0[bB][01](_?[01])*`:
    - Reject empty digits (`0x`, `0b`), leading/trailing/double underscores (`0x_1`, `0xFF_`, `0b1__0`), and trailing alphanumeric characters (`0b102`, `0x1G`) as `LexError` (never split into two tokens!).
    - Reject radix float syntax (`0x1.5`, `0b1.0`) with `FloatLiteralForbidden`.
  - Byte literals `b"..."` / `b'...'` -> `TokenKind::BytesLit(Vec<u8>)`: accept ASCII `0x20..=0x7E` plus `\n`, `\r`, `\t`, `\0`, `\\`, `\"`, `\'`, and `\x[0-9a-fA-F]{2}`; reject raw non-ASCII (`> 0x7F`), `\uXXXX`, or invalid escapes with `LexError`.
  - `null` is a reserved value literal in `CompatMode::V2`: parses as `ExprKind::NullLit` and `Pattern::NullLit`, and is rejected as a variable, parameter, struct-field, or function binder name.
  - `array[T, N]` parses `T: TypeExpr` and compile-time integer literal `N` with `1 <= N <= 65536` (`0` or `> 65536` rejected at compile time).

#### 2.2 Canonical `render()` Strings Across `aod-core`, `eval.rs`, & `aod-rt` (`P1-1`)
Every new runtime value MUST render byte-for-byte identically across `eval::Value::render()` and `aod_rt::RtValue::render()`:
- `Null`: `"null"`
- `U8(v)`: `format!("u8({v})")` (decimal `0..=255`)
- `B64(v)`: `format!("b64(0x{v:016x})")` (lowercase 16-digit zero-padded hex)
- `T9(v)`: `format!("t9({v})")` (signed decimal `-9841..=9841`)
- `Bytes(bs)`: `b"..."` escaping `\\` -> `\\\\`, `\"` -> `\\"`, `\n` -> `\\n`, `\r` -> `\\r`, `\t` -> `\\t`, `\0` -> `\\0`, printable ASCII `0x20..=0x7E` verbatim, and all other bytes as lowercase `\x{:02x}`.
- `Array(elems)`: `format!("[{}]", elems.iter().map(render).collect::<Vec<_>>().join(", "))`.

#### 2.3 Exact `superpose_cancel` Wave Algebra (`P0-1`)
- **`T1::superpose_cancel(a: T1, b: T1) -> T1`:** Exact 9-pair balanced-trit superposition with sameness idempotence (`xx is x`) and opposing cancellation (`+1` meets `-1` -> `0`):
  - `(+1, -1) | (-1, +1) => T1::ZERO`
  - `(+1, +1) => T1::POS`
  - `(-1, -1) => T1::NEG`
  - `(x, 0) | (0, x) => x`
  (Equivalent to `T1::new((a.value() + b.value()).signum())`).
- **`T32::superpose_cancel(self, other: Self) -> Self`:** Lane-wise 32-trit parallel execution using **bitwise OR** (NEVER `^` XOR, which would erase constructive same-sign crests `01 ^ 01 == 00`!):
  ```rust
  let pa = self.pack();
  let pb = other.pack();
  let crest = (pa | pb) & 0x5555_5555_5555_5555;
  let trough = ((pa | pb) & 0xAAAA_AAAA_AAAA_AAAA) >> 1;
  let cancel = crest & trough;
  let clear_mask = !(cancel | (cancel << 1));
  let out_bits = (pa | pb) & clear_mask;
  Self::unpack(out_bits).expect("superpose_cancel clears all 0b11 lanes")
  ```
  Guarantees: `superpose_cancel(x, 0) == x`, `superpose_cancel(x, x) == x`, `superpose_cancel(x, neg(x)) == 0`, and zero `0b11` lanes.

#### 2.4 Complete Builtin & Constructor Signature Matrix (`P0-2`)
All new entries below have `v1_allowed: false` in `types::builtin_signatures()` and are ALSO exported in `aod::core` (`crates/aodc/src/modules.rs:53-71`):
- `u8`:
  - `(int) -> u8`, `(u8) -> u8`, `(bool) -> u8` (`false -> 0`, `true -> 1`), `(q4) -> u8` (`0..=15`), `(t9) -> u8` (checked `0..=255`), `(t32) -> u8` (checked `0..=255`), `(b64) -> u8` (checked `0..=255`).
- `b64`:
  - `(int) -> b64` (checked `0..=u64::MAX`, rejecting `< 0` and `>= 2^64` with `Overflow`), `(u8) -> b64` (zero-extended), `(q4) -> b64` (`0..=15`), `(b64) -> b64`,
  - **`(t32) -> b64`**: **packed 32-trit 2-bit-per-lane register view `B64(x.pack())`** (NOT two's-complement scalar!),
  - **`(t9) -> b64`**: **packed 9-trit 18-bit register view `B64(x.pack() as u64)`**.
- `t9`:
  - `(int) -> t9`, `(t1) -> t9`, `(u8) -> t9`, `(t9) -> t9`, `(t32) -> t9` (all checked `[-9841, +9841]`).
- `t32`:
  - Existing `(int) -> t32` stays `v1_allowed: true`; add v2 `(t1) -> t32`, `(u8) -> t32`, `(t9) -> t32`, `(t32) -> t32`, and
  - **`(b64) -> t32`**: **unpacks 32-trit packed register `T32::unpack(b.0)`**, rejecting any `0b11` lane with `InvalidTritEncoding(3)`.
- `q4`:
  - Existing `(int) -> q4` stays `v1_allowed: true`; add v2 `(u8) -> q4` (checked `0..=15`) and `(q4) -> q4`.
- `int`:
  - Keep existing 5 v1 signatures; add v2 `(u8) -> int`, `(b64) -> int` (unsigned `BigInt::from_u64`), `(t9) -> int` (signed).
- `pack` / `unpack` / `unpack_t9`:
  - Keep `pack(t32) -> int` and `unpack(int) -> t32` (`v1_allowed: true`).
  - Add v2 `pack(t9) -> int` (`BigInt::from_u64(x.pack() as u64)`), `unpack_t9(int) -> t9`, and `unpack_t9(b64) -> t9`:
    - `T9::unpack(bits: u64) -> CoreResult<T9>` rejects `bits >= (1 << 18)` (`DomainError`) AND rejects any `0b11` lane in `0..9` (`InvalidTritEncoding(3)`), decoding `00 -> 0`, `01 -> +1`, `10 -> -1` with `3^k` weights across `[-9841, +9841]`. `unpack_t9(int)` additionally rejects `< 0` or `>= (1 << 18)` with `DomainError`.
- Hardware / Q4 / Wave ops:
  - `neg(t9) -> t9` (and keep `neg` via unary `-` on `int`, `t1`, `t9`, `t32`, `ratio`; also add `neg(t1) -> t1`, `neg(t32) -> t32` as v2 builtins matching `00_primitives.dec`!),
  - `bitnot(b64) -> b64`,
  - `popcnt(b64) -> int` (exact `0..=64`),
  - `is_hamming1(q4, q4) -> bool`,
  - `popcnt_xor(q4, q4) -> int` (exact `0..=4`),
  - `superpose_cancel(t1, t1) -> t1`,
  - `superpose_cancel(t32, t32) -> t32`.
- Byte sequence ops (zero `str` output dependency, ready for Phase D3 `PureWaveV2`):
  - `bytes([u8]) -> bytes`, `bytes(str) -> bytes` (UTF-8 bytes), `bytes(bytes) -> bytes`, `bytes(b64) -> bytes` (8-byte little-endian register bytes), `bytes(addr8) -> bytes` (canonical octal address ASCII bytes),
  - `slice_bytes(bytes, int, int) -> bytes` (exact checked `0 <= start <= end <= len`, failing closed with `DomainError` / index out of bounds without clamping),
  - `concat_bytes(bytes, bytes) -> bytes`,
  - `sha256_bytes(bytes) -> bytes` (exact 32-byte digest),
  - `hex_bytes(bytes) -> bytes` (lowercase ASCII hex `bytes`, 64 bytes for SHA-256 — zero `str` dependency!).

#### 2.5 `array[T, N]` Representation, `DecIR v2`, & Firewalls (`P0-4`)
- Add `ExprKind::ArrayLit(Vec<Expr>)` to `ast.rs` and `ir.rs` (`DecIR v2` canonical JSON round-trip MUST support `NullLit`, `BytesLit`, `ArrayLit`, `TypeExpr::Array(Box<TypeExpr>, usize)`, and `SingleArm.has_arrow`).
- Construction:
  1. `array(e0, ..., e_{N-1})` (`N >= 1`, homogeneous `T`, `!contains_unknown(&T)` -> `Type::Array(Box::new(T), N)`).
  2. Contextual `[e0, ..., e_{N-1}]` when expected type is `Type::Array(T, N)` (or `ReadOnly<array[T, N]>`): enforce `items.len() == N` and element types `== T` in `checker.rs`, and **ensure runtime evaluation (`eval.rs` & `codegen.rs`) produces `Value::Array` / `RtValue::Array` via `@aod_rt_array_new`** whenever target type is `Type::Array` (by typing/rewriting or passing target-type expectation in `eval`/`codegen` so `Type::Array` NEVER holds `Value::List` / `RtValue::List` at runtime!).
- Firewalls:
  - `Type::is_qext` (`types.rs`) recurses into `Type::Array(inner, _)`.
  - `Type::is_map_key` (`types.rs`) recurses into `Type::Array(inner, _)` (`inner.is_map_key()`), plus `Type::Null | Type::U8 | Type::B64 | Type::T9 | Type::Bytes => true`.
  - Direct index mutation `arr[i] = v` and `bytes[i] = v` fail at compile time (`TypeError`).
  - `ReadOnly<array[T, N]>` wraps non-scalar indexed elements `ro_arr[i]` in `ReadOnly<T>`, `ReadOnly<Struct>.arr_field` yields `ReadOnly<array[T, N]>`, and `array_set(ro_arr, i, v)` rejects `ro_arr.is_read_only()` at compile time with `ObserverMutationForbidden`!

#### 2.6 Polymorphic `select(cond, a, b)` & Qualified `aod::core::*` Calls (`P0-5`)
- Resolve polymorphic builtins (`select`, `array`, `array_set`, `len`, `contains`) identically whether called unqualified (`select(...)`) or via `aod::core::select(...)` (`ExprKind::Path(["aod", "core", name])`, or imported alias).
- **`select(cond, a, b) -> T`:**
  - Rejected in `CompatMode::V1`.
  - Requires exact arity 3; `cond` has type `Type::Bool` (or `QExt<bool>` inside `Effect::QExt` when `a, b` are `QExt<T>`); `a_ty == b_ty` **without unwrapping `ReadOnly`** (mismatch fails with `ImplicitCoercionForbidden`); `a_ty != Type::Unit`; `!contains_unknown(&a_ty)`; if `a_ty.is_qext()` outside `Effect::QExt`, fails with `QExtLeakageForbidden`.
  - **Eager left-to-right semantics across all 6 modes:** Evaluates `cond`, then `a`, then `b` eagerly (charging fuel for all 3 operands; any runtime trap in `a` or `b` such as `10 // 0` triggers deterministically even on the unselected arm!), and in `codegen.rs` emits `select i1 %cond_i1, <LlvmTy> %a_val, <LlvmTy> %b_val`.

#### 2.7 LLVM 18 Lowering & Pre-Op UB Guards (`P1-1`)
- `LlvmTy::I16` (`"i16"`, zero `"0"`) for `Type::T9`; `Type::Null | Type::U8 => LlvmTy::I8`; `Type::B64 => LlvmTy::I64`; `Type::Bytes | Type::Array(_, _) => LlvmTy::Handle`.
- **Zero hardware SIGFPE / LLVM `poison`:**
  - `u8`: `zext i8` (never `sext`), unsigned `icmp ult/ule/ugt/uge i8`; pre-op `rhs == 0` guard calling `@aod_rt_fail` before `udiv`/`urem`; pre-op `rhs >= 8` guard calling `@aod_rt_fail` before `shl`/`lshr` (NEVER attach `nuw`/`nsw` to `shl i8`!).
  - `b64`: unsigned `icmp ult/ule/ugt/uge i64`; pre-op `rhs >= 64` guard calling `@aod_rt_fail` before `shl i64` / `lshr i64`; `@llvm.ctpop.i64` -> `zext`/`@aod_rt_int_from_u64` for `popcnt(b64) -> int`; `@aod_rt_int_from_u64` for `int(b64)`.
  - `t9`: widen via `sext i16 %v to i64`, range-check `[-9841, 9841]` calling `@aod_rt_fail` on overflow, and use exact **floor division & floor remainder** (matching `T9::checked_div_floor` / `checked_rem_floor`, e.g., `t9(-7) // t9(3) == t9(-3)` and `t9(-7) % t9(3) == t9(2)`, with pre-op `rhs == 0` guard and `-9841 // -1` check — NEVER raw truncating `sdiv`/`srem`!).

---

### 3. Live-Walk Audit (`dec_audit.rs`) & Two-Thread FRG Hardware Probe (`hw_probe.rs`) (`P0-6`, `P0-7`, `P1-2`)

#### 3.1 `crates/aodc/src/dec_audit.rs` (`aod.dec.audit.v1`) (`P0-7`, `P1-2`)
- **Live-Walk Scanner (`lex_live_walk`):**
  - Starts at `Addr8` `00_8` (`[0, 0]`, minimum 2 octal digits: `00_8`..`07_8`, `10_8`, ...).
  - `# ...` comments map every byte until `\n` to `T1::ZERO`; whitespace `' '`, `'\t'`, `'\n'`, `'\r'` and structural delimiters `: ( ) [ ] { } ,` map to `T1::ZERO`; `-` and `>` map to `T1::NEG`; all other bytes map to `T1::POS`.
  - `dwell_burned: u64` increments monotonically whenever `role == T1::ZERO && prev_role == T1::ZERO` (never reset!).
  - `committed_crossings: u64` increments and `octal_anchor` advances by `+1` in base 8 whenever `role != T1::ZERO && role != prev_role`.
- **AST `has_arrow` Tracking & Six Zero-Stub §5.17 Audit Hooks (`P0-7`):**
  - Record `has_arrow: bool` on `DecFnKind::SingleArm` in `ast.rs` (and `ir.rs`).
  - `audit_dec_file` / `audit_dec_source` inspects all bundled/entry modules and evaluates all 6 hooks cleanly into `AuditHooksSummary` (`DecAuditReport`, `aod.dec.audit.v1`):
    1. `spine_audit` (§5.17.1): >= 1 `DecFn` in entry module; every `SingleArm` has `has_arrow == true` and non-empty body (not empty and not solely `Stmt::Pass`); every `MultiArm` has >= 1 arm with explicit `->` return type and non-empty body; and typechecking (`check_unit` / `check_bundle`) succeeds unless another specific §5.17 hook explains the domain violation.
    2. `boundary_audit` (§5.17.2): every `WindowLit(u, v)` in AST has `u < v` and every `BoxExpr` range `a..b` on all 3 axes has `a < b` (evaluated on literal/const integer endpoints; note: parser/audit allows inspecting `(6, 6]` or `8..2` so `boundary_audit` reports `false` cleanly!).
    3. `sadar_audit` (§5.17.3): every `sadar_promote` / `sadar_from_clipped` / `scoped_channel_flux` call in AST with literal/const numeric arguments has `c3 > 0` and `rho >= 0`, and `SadarPromotion` satisfies `forward + reverse == 0/1r`.
    4. `sheddic_path_audit` (§5.17.5): every `SupportLit` / `support(...)` literal in AST obeys odd-enclosure `_yro` parity (`L % 2 == 1 <=> is_yro`), AND any call to builtin/audit helper `audit_sheddic_route(x_shedding, target_enclosure)` (also exposed as `pub fn verify_sheddic_route(x_shedding: i64, target_enclosure: u8) -> bool` and Dec v2 builtin `verify_sheddic_route(int, u8) -> bool`) in AST with constant/asserted args satisfies `x_shedding <= 0 || (target_enclosure >= 2 && target_enclosure % 2 == 0)` (rejecting `x_shedding > 0` routed to `0` or odd `L`).
    5. `cycle_audit` (§5.17.7): DAG order `C_f[p:q]` before `RD` — every `support_rd(sup, h_mu)` / `support_pressure` / `support_overhang` call in AST with literal/const `h_mu` has `h_mu >= 1`, every `prepare` `clock` has period `>= 1`, and `rate(...)` / `beats(...)` / `phases(...)` is never called directly on an unclosed `trace(...)` constructor expression (`rate(trace(...))`) prior to `close(...)`.
    6. `frg_audit` (§5.17.9): every call to `verify_frg_cross_return(t_zero, t_cross, c_cross)` (exposed both as `pub fn verify_frg_identity(t_zero: i64, t_cross: i64, c_cross: i64) -> bool` in `dec_audit.rs` and as Dec v2 builtin `verify_frg_cross_return(int, int, int) -> bool`) in AST with literal/const args satisfies `t_cross - t_zero == c_cross`.
  - CLI `aodc audit-dec [--compat v1|v2] [--stdlib <dir>] [--report <path>] [--json] <source.dec>`: validates `--report` BEFORE reading any source file via `collect_guarded_roots` + `validate_runtime_output_path` (exit `2` on symlink/disallowed in-repo path), writes `aod.dec.audit.v1` JSON atomically when `--report` is given, exits `0` when all 6 hooks + typecheck pass, and exits `1` (still emitting the JSON report when `--json`/`--report` is requested!) when any hook is `false`.

#### 3.2 Two-Thread FRG Double-Slit Probe (`crates/aod-llvm/src/hw_probe.rs`) (`P0-6`)
- **Zero `.max(1)` Fabrication:** Never clamp `c_cross_ab` with `.max(1)`.
  - Tier 0 (`E4`, `RD=9`): live ORC LLJIT Q4 Hamming-1 (`llvm.ctpop.i64`) + `T32` 1-cycle phase-flip kernel verified against Rust reference.
  - Tier 1 (`C6`, `RD=61`): live ORC LLJIT 64-slot cache-line ring kernel verified against Rust reference.
  - Tier 2 (`C8`, `RD=137`, `T_sync=8357`): two `std::thread::scope` threads synchronized with `std::sync::Barrier::new(2)`:
    1. **Isolated Slits (`T^0`):** Slit A and Slit B run `137` steps on cache-line-isolated (`#[repr(align(128))]`) private state (`s_gate_a = 137`, `s_gate_b = 137`, `t_zero = 274`, `isolated_slits_ns`).
    2. **Coupled Shared-Cache-Line Ring Gate (`T^x`):** Slit A (`bit 0 = 1`) and Slit B (`bit 1 = 2`) interact across a single `#[repr(align(64))]` shared cache-line `AtomicU64` state word over `137` rounds each: each slit atomically records its arrival and accumulates cross-return beats (`+1` whenever the partner slit's phase mark or coupled sequence step has already touched the shared ring line, plus exact final ring-closure commit of both slits' `137` arrivals), so `c_cross_ab >= 137 > 0` holds by genuine two-thread `AcqRel` shared-cache-line coupling with **zero `.max(1)` clamping**, while separately recording `inter_thread_handoffs` (observed ownership switches between Thread A and Thread B on the shared cache line) and `coupled_ring_ns`!
- **3-State Complete-State `Op_e` Certification (`retained: Option<i64>`):**
  - `pub fn certify_frg_channel(tier_name: &str, enclosure: u8, rd_cadence: i64, t_zero: i64, t_cross: i64, retained: Option<i64>, base_w: i64) -> LlvmResult<FrgChannelReport>`
  - Verifies `retained.unwrap_or(0)` vs `r_e = t_cross - t_zero`:
    - When `retained == Some(r)` with `r == r_e`: `j_with_r = Ratio::from_i64(3 + r, 3)`, `j_removed_r = Ratio::from_i64(3, 3)`, `op_certified = j_with_r != j_removed_r` (`r != 0`).
    - When `retained == None` (structural source removal `S_k^{+,-e}`): `j_with_r == j_removed_r == 1/1`, so `op_certified = false`, `role_trit = -1`, `w_adapted = 0` (`retained_present: false` vs `Some(0)` `retained_present: true, r_e: 0, op_certified: false`).

---

### 4. Updated Acceptance Criteria (`AC-01`..`AC-14`)

- **`AC-01` (`Null` Singleton & `0b11` Non-Conflation):** `Null` (`null`, `render() == "null"`) is distinct & compile-time incomparable (`ImplicitCoercionForbidden`) with `()`, `0`, `u8(0)`, `b64(0)`, `0t`, `t9(0)`, `t32(0)`, `q4(0)`, `00_8`; rejected as an expr/pattern in `--compat v1` (while unreserved as a v1 identifier); `T32::unpack` and `T9::unpack` (`unpack_t9`) reject `0b11` lanes with `InvalidTritEncoding(3)`.
- **`AC-02` (`u8` Checked Unsigned Register & Pre-Op LLVM Guards):** `u8` (`render() == "u8(<n>)"`) enforces `0..=255` across all 7 `u8(...)` signatures; pre-op LLVM guards catch `u8 // 0`, `u8 % 0`, and `u8 << 8` / `u8 >> 8` cleanly without SIGFPE/poison; unary `-` rejected at compile time; `u8(200) > u8(100)` and `int(u8(200)) == 200` hold across VM, AOT, and ORC LLJIT.
- **`AC-03` (`b64` Register View, Strict `0x`/`0b` Lexing, & 1-Cycle `t32` Flip):** `0x`/`0b` reject in `--compat v1` and reject malformed `0x`, `0xFF_`, `0b102`, `0x1G`, `0x1.5`; `b64(t32)` returns packed `T32::pack` and `t32(b64)` unpacks via `T32::unpack` (rejecting `0b11`); `b64` `<<`/`>>` guard `>= 64` cleanly; arithmetic `+`/`-`/`*`/`/` on `b64` fails at compile time; 1-cycle wire-swap formula matches `neg(w)` at `0`, `1`, `-1`, `±T32::LIMIT` across all 6 modes.
- **`AC-04` (`t9` Checked 9-Trit Scalar & `pack`/`unpack_t9`):** `t9` (`render() == "t9(<n>)"`) enforces `[-9841, +9841]`, exact floor `//` & `%` (`t9(-7) // t9(3) == t9(-3)`, `t9(-7) % t9(3) == t9(2)`, pre-op `rhs == 0` guard), and 18-bit `pack(t9) -> int` + `unpack_t9(int|b64) -> t9` round-tripping `-9841`, `0`, `+9841` while rejecting `< 0`, `>= 2^18`, and `0b11` lanes across all 6 modes.
- **`AC-05` (`superpose_cancel` 9-Pair Truth Table, `Q4` Bitwise, & Eager `select`):** Tests all 9 `(t1, t1)` pairs and multi-lane `t32` constructive (`(+1,+1)->+1`, `(-1,-1)->-1`) + destructive (`(+1,-1)->0`) combinations (proving OR-based `superpose_cancel`, never XOR); `q4` `&`/`|`/`^`, `is_hamming1`, `popcnt_xor`; `select(cond, a, b)` (both unqualified and `aod::core::select`) enforces eager left-to-right evaluation (`select(false, 10 // 0, 0)` traps identically across VM/AOT/JIT), preserves `ReadOnly<T>` & `QExt` firewalls, rejects `Unit`, and emits native LLVM `select i1`.
- **`AC-06` (`bytes` & `b"..."` Immutability, Slicing, & `hex_bytes -> bytes`):** `b"..."` (rejected in `--compat v1`; rejects raw non-ASCII), `bytes([u8]|str|bytes|b64|addr8)`, `len`, `b[i] -> u8`, non-clamping `slice_bytes`, `concat_bytes`, `sha256_bytes` (32B), `hex_bytes(bytes) -> bytes` (64B ASCII hex on SHA-256), and `MapKey::Bytes` match across all 6 modes; `b[0] = u8(1)` fails at compile time.
- **`AC-07` (`array[T, N]` Runtime Identity & Firewalls):** `array(e0, ...)` and contextual `[e0, ...]` both evaluate to `Array` (`"[...]"` render, never `List`) across VM/DecIR/AOT/JIT; `N == 0` and wrong lengths fail at compile time; `arr[i] = v` fails at compile time; `ReadOnly<array[T, N]>` blocks `array_set` with `ObserverMutationForbidden` and propagates `ReadOnly` on indexing/struct fields; `Type::is_qext` & `Type::is_map_key` recurse into `Array`.
- **`AC-08` (6-Mode Differential Parity & `DecIR v2` Round-Trip):** `tests/fixtures/v2_smoke/hw_register_primitives.dec` produces byte-identical stdout across Exact VM, `aodc compile` + `aodc exec` (`DecIR v2`), AOT `-O0`, AOT `-O2`, ORC LLJIT `-O0`, and ORC LLJIT `-O2`.
- **`AC-09` (`FIRST_PRINCIPLES_DERIVATION.md` Vendoring):** Byte-for-byte SHA-256 `091d8f1ef7dd3b9c1f96eab7adc9ff5db67194418c2dbfad1a0fcab68b5afabc` + linked in `README.md`.
- **`AC-10` (Live-Walk `00_8` Octal Anchor & Monotonic Dwell Reduction):** `lex_live_walk` starts at `00_8` (2-digit minimum octal width), accumulates `dwell_burned` monotonically across spaces/newlines/comments without resetting, and produces identical `final_anchor` across whitespace/comment-padded vs compact `.dec` sources.
- **`AC-11` (`aodc audit-dec`, Pre-Read Guard, & 6 Hostile `.dec` Negative Controls):** Passes on all 11 `manual_examples/*.dec` (wired into `.github/workflows/ci.yml`), exits `2` before reading input on disallowed/symlink `--report` paths, and exits `1` on **6 distinct hostile `.dec` programs** audited via `aodc audit-dec --json`, individually falsifying `spine_audit` (missing `->` hinge), `boundary_audit` (degenerate `(6, 6]` / `8..2`), `sadar_audit` (negative `c3`/`rho`), `sheddic_path_audit` (`x_shedding > 0` routed to odd `7` or `0`), `cycle_audit` (`support_rd(3:3:6, 0)` / `rate(trace(...))` before `close`), and `frg_audit` (`verify_frg_cross_return` with `t_cross - t_zero != c_cross`).
- **`AC-12` (Live 3-Tier FRG Double-Slit Probe & 3-State `Op_e` Falsification):** `aodc probe-hw` runs live `E4 (RD=9)`, `C6 (RD=61)`, and two-thread barrier-synchronized shared-cache-line `C8 (RD=137, T_sync=8357)` with zero `.max(1)` clamping (`t_zero == 274`, `t_cross - t_zero == r_e > 0`, `op_certified == true`, `COMPILER_EXECUTION_POLICY_NOT_PHYSICAL_AOD_LAW`), plus unit/integration tests verifying all 3 `retained: Option<i64>` states: `Some(r > 0)` (`op_certified == true`), `Some(0)` (`retained_present == true, op_certified == false`), and `None` (`S_k^{+,-e}`, `retained_present == false, op_certified == false`).
- **`AC-13` (Zero Regression on Frozen v1, S2/S4, Manual, & PDF Gates):** 10/10 `--compat v1`, `verify-s2 --s4-report` (debug == release), `manual-data --check`, `manual-verify docs/manual` (18/18), and `pdf_audit` all pass unchanged.
- **`AC-14` (Workspace Hygiene & Static Token Guards):** `RUSTFLAGS="-D warnings" cargo test --workspace --offline` (debug + release), `source_binding.rs` (zero Python, zero external deps, zero `f32`/`f64`/`unsafe` token leaks anywhere in `crates/*/src/**/*.rs`, 5-file `unsafe` allowlist), and zero mutations outside `/home/antigravity/rpoyau/aod-dec`.
