# PR 11 Request Bundle (`R02` — All `P0`/`P1`/`P2` Resolved) — Phase D2: Pure Indented `dec` Signature-Matrix Frontend, Uniform Type Constructors, AFC Scope/Sequence Operators, & `--strict-wave` (`PureWaveV2`) Discipline

**Baseline:** `rpoyau/aod-dec@d12daab` (`v2/zero-python-evolution`, PR 10 committed; `111/111` workspace tests green).  
**Authority:** [`docs/plans/dec_wave_purification_plan.md`](../dec_wave_purification_plan.md), [`docs/plans/bundles/pr10_request_bundle.md`](pr10_request_bundle.md) (Part 1), [`docs/FIRST_PRINCIPLES_DERIVATION.md`](../../FIRST_PRINCIPLES_DERIVATION.md), `rpoyau/aod` (`manual/sections/00_dec_ledger.tex`, vendored `S1b@cea71a7`), and `rpoyau/afc/README.md`.

---

## 1. Objective & Architectural Scope

Implement **Phase D2 (PR 11)** inside `/home/antigravity/rpoyau/aod-dec`:

1. **Pure Indented Syntax & `dec` Signature-Combinatorics Frontend (`{}`-Free):**
   - Lowercase `dec <name>:` family headers (zero header parens/params) with indented `(a: T1, b: T2) [where <guard>] -> Ret:` signature-combinatorics arms across the structural hinge `->` (`0_H`).
   - Support `name: Type` bindings, `pattern: Type` annotations (`_: Type` -> `TypedPattern { Wildcard }`), explicit register-literal patterns (`u8(...)`, `b64(...)`, `t1(...)`, `t9(...)`, `t32(...)`, `q4(...)`), trit/hinge/dwell patterns (`-1t`, `0t`, `+1t`, `0_H`, `0_D` plus contextual `-1`/`0`/`+1` in sibling-typed `t1` slots), `null`, `bytes`, `addr8`, `support`, `ratio`, `int`, `bool`, `enum`, nested tuple, and `_` wildcard patterns, multi-arity/polymorphic-return `dec` families, and `where <bool_expr>` admissibility guards.
   - Pure indented `use <path>:` import blocks alongside indented `struct`, `enum`, `chamber`, `prepare`, `observe` (storing and checking `ProbeDecl::Procedural { param_ty, has_arrow, .. }`, `P0-1`), and `quarantine Q_ext` blocks with zero `{` or `}`.
   - Immutable tuple return destructuring `let (x, y, ...) = expr` (supporting `_` wildcards, per-element `x: T` annotations, and/or outer `: (T1, T2, ...)` annotations).
   - Uniform positional `StructName(arg1, ...)` constructors matching field declaration order, plus optional user-declared `dec StructName:` disjoint constructor overloads returning `StructName` (`P1-1`), and `.num -> int` / `.den -> int` field access on `ratio` (`P0-4`).
   - Brace-free deterministic map literals `map[(k1, v1), ...]` and contextual empty `map[]` (`P2-1`), paired with pure persistent copy-on-write container helpers (`map_put`, `map_remove`, `map_get_or`, `map_keys`, `map_entries`, `list_set`, `list_push`, `concat_seq`, `slice_seq`), plus Phase D3-ready `bytes` overloads on `addr8`, `support`, `trace`, `fold`, `coupling`, `window_dedup_count`, `native_cadence_ratio`, `single_contact_association`, and `aod::qext::{ext_decimal, qext_render_bytes, qext_format_fixed_bytes}` (`QExt<bytes>`, `P0-4`).
2. **Engine-Driven AFC Scope/Window & Sequence/Contraction Operators + Exact Scalar Math:**
   - Higher-order operators (zero heap closures, optional explicit trailing `env: C` boundary context, lowered to direct inline LLVM 18 basic-block loops preserving `CodegenPlan` `!llvm.loop` metadata): `evolve`, `scan`, `fold_seq`, `map_seq`, `filter_seq`, `tabulate`, `zip_seq`, `count`, `any`, `all`.
   - Exact zero-float vector contraction & checked scalar math builtins across VM, `DecIR v2`, AOT, and ORC LLJIT: `sum`, `dot`, `min`, `max`, `abs`, `pow`, `gcd`, `lcm` (preserving `∂` / `boundary_of` on `window` and `range` under `PureWaveV2`).
3. **Dual-Syntax Transition Gate (`SyntaxDiscipline` + `--strict-wave`):**
   - `SyntaxDiscipline::{TransitionalV2, PureWaveV2}` (defaulting to `TransitionalV2` in Phase D2 so all existing D1/`stdlib`/`manual_examples` tests pass untouched; `--compat v1` remains 100% isolated).
   - `--strict-wave` enforces `SyntaxDiscipline::PureWaveV2` **transitively across `<entry>` and all imported modules** (`P0-5`) across `check`, `run`, `compile`, `emit-llvm`, `build`, `jit`, `bench`, `learn`, `audit-dec`, and `test`, embeds `"syntax_discipline": "pure_wave_v2"` in `DecIR v2` (mapping any tampered rebuild failure in `loads_ir` to `IrVerificationError`, `P0-5`), and binds `syntax_discipline` into `compute_profile_bindings` (`crates/aodc/src/lib.rs`).

---

## 2. Exact File-by-File Module & API Plan

| File | Action | Public API / Module Responsibility |
| :--- | :--- | :--- |
| `crates/aod-core/src/bigint.rs` | Modify | Add `pub fn lcm(&self, other: &Self) -> Self` (`lcm(a, 0) = lcm(0, b) = 0`, else `(\|a\| / gcd(a, b)) * \|b\|`), `pub const MAX_POW_EXP: u32 = 65_536`, and `pub fn checked_pow(&self, exp: u32) -> CoreResult<Self>` enforcing both `exp <= MAX_POW_EXP` and the `1_048_576`-bit result ceiling (matching `BigInt::shl`, `P0-3`). |
| `crates/aod-core/src/ratio.rs` | Modify | Add `pub fn checked_pow(&self, exp: u32) -> CoreResult<Self>` delegating numerator/denominator to `BigInt::checked_pow` (`pow(r, 0) == 1/1`, `1_048_576`-bit cap, `P0-3`). |
| `crates/aod-core/src/ternary.rs` | Modify | Add `T1::abs(self) -> Self`; `T9::{abs, checked_pow, gcd, checked_lcm}`; `T32::{abs, checked_pow, gcd, checked_lcm}` using checked binary exponentiation (fast-paths for `exp == 0` and `\|base\| <= 1`, `P0-3`) and exact checked domain bounds (`[-9841, +9841]` and `[-926_510_094_425_920, +926_510_094_425_920]`). |
| `crates/aod-rt/src/arena.rs` | Modify | Add safe `pub fn get_mut(&mut self, handle: i64) -> Result<&mut RtValue, String>` (`P0-3`; zero `unsafe`). |
| `crates/aod-rt/src/value.rs` | Modify | Add `RtValue::QExtBytes(Vec<u8>)` rendering `format!("Q_ext({})", render_bytes_literal(bs))` (`P0-4`). |
| `crates/aod-rt/src/ops.rs` | Modify | Fix `Window.len` to compute via `i128` (`BigInt::from_i128((v as i128) - (u as i128))`, `P0-3`); add `.num` / `.den` field extraction on `RtValue::Ratio` (`P0-4`); add `op_window_bounds`, `op_array_len_raw`, `op_bytes_len_raw`, `op_list_with_capacity` (clamping pre-allocation hint to `0..=65_536` to prevent pre-fuel OOM, `P0-3`), `op_list_push_unique` (cloning `val_h` *before* `arena.get_mut(list_h)`), `op_list_push_cow`, `op_list_concat`, `op_list_slice`, `op_zip_seq`, `op_map_remove`, `op_map_get_or`, `op_sum`, `op_dot`, `op_pow_{int,t9,t32,ratio}`, `op_gcd_{int,t9,t32}`, `op_lcm_{int,t9,t32}`, `op_emit_qext_bytes`, `op_qext_render_bytes`, `op_qext_format_fixed_bytes`, and UTF-8-validated `bytes`-input helpers for `addr8`, `support`, `ext_decimal`, `trace`, `fold`, `coupling`, `window_dedup_count`, `native_cadence_ratio`, `single_contact_association` (`P0-4`). Zero `unsafe`. |
| `crates/aod-rt/src/abi.rs` | Modify | Expose panic-safe `#[unsafe(no_mangle)] pub unsafe extern "C"` entrypoints (`guard_ret` + `catch_unwind`) for all new `ops.rs` helpers. |
| `crates/aodc/src/diagnostics.rs` | Modify | Add `#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)] pub enum SyntaxDiscipline { #[default] TransitionalV2, PureWaveV2 }` with `as_str()` / `from_str_tag()`. |
| `crates/aodc/src/ast.rs` | Modify | Add `has_header_parens: bool` on `DecFnKind::MultiArm`; add `param_ty: Option<TypeExpr>` and `has_arrow: bool` on `ProbeDecl::Procedural` (`P0-1`); add `LetTupleElem { pub name: String, pub ty: Option<TypeExpr> }` and `StmtKind::LetTuple { elems: Vec<LetTupleElem>, ty: Option<TypeExpr>, value: Expr }`; add `Pattern::{U8Lit(u8), B64Lit(u64), T9Lit(T9), T32Lit(T32), Q4Lit(Q4)}`. |
| `crates/aodc/src/lexer.rs` | Modify | Add `tokenize_with_discipline(source: &str, compat: CompatMode, discipline: SyntaxDiscipline) -> DecResult<Vec<Token>>` (`tokenize` delegates with `TransitionalV2`). Add `TokenKind::DecLower` (`"dec"` in `CompatMode::V2`) and `TokenKind::Where` (`"where"` in `CompatMode::V2`); in `CompatMode::V1`, both remain `TokenKind::Ident`. Under `PureWaveV2`, reject `{`, `}`, and unprefixed string literals `"..."`/`'...'` (`b"..."`/`b'...'` remain valid). |
| `crates/aodc/src/parser.rs` | Modify | Add `parse_program_with_discipline(source, compat, discipline)`. Parse `dec <name>:`, disambiguated `ArmParam` (`P1-1`: `_: Type` -> `TypedPattern { Wildcard }`, delimiter-depth-aware `looks_like_dec_arm`), typed register-literal patterns, `where <expr>` guards, indented `use <path>:`, `let (x, y) = expr`, explicit `map[]` / `map[(k, v), ...]` (`P2-1`), procedural `observe` probes preserving `param_ty` & `has_arrow` (and consuming outer `Dedent` on indented `tracer name:`, `P0-1`), and enforce all `PureWaveV2` bans (including reserving `map` as a binder name in `PureWaveV2`, `P2-1`). |
| `crates/aodc/src/modules.rs` | Modify | Extend `LoadOptions` and `LoadedUnit` with `pub syntax_discipline: SyntaxDiscipline`. Enforce `options.syntax_discipline` **transitively on `<entry>` and every imported/bundled module** with zero `"stdlib::*"` exemption (`P0-5`). Add `load_hermetic_with_discipline`. Export all new Phase D2 builtins in `aod::core` and `qext_render_bytes`/`qext_format_fixed_bytes` in `aod::qext` (`P0-4`). Reject `(CompatMode::V1, SyntaxDiscipline::PureWaveV2)` immediately. |
| `crates/aodc/src/types.rs` | Modify | Add `Type::resolve_with_discipline` rejecting `TypeExpr::Named("str")` under `PureWaveV2` (while permitting `QExt<bytes>`). Register all `bytes`-overloaded signatures (`P0-4`), `pow`/`gcd`/`lcm`, and polymorphic v2 builtin names. |
| `crates/aodc/src/checker.rs` | Modify | Validate `ProbeDecl::Procedural.param_ty == ReadOnly<ChamberView>` (`ObserverMutationForbidden` on bare `ChamberView`, `P0-1`); multi-arity/polymorphic-return `dec` family checking, per-arm duplicate non-`_` binder rejection, `TypedPattern` catch-all dead-arm detection, and arity+type-matched `StructName(...)` vs `dec StructName:` dispatch (`P1-1`); `.num`/`.den -> int` on `Type::Ratio` (`P0-4`); `StmtKind::LetTuple`; `select` empty-collection unification; reject `Type::Unit \| contains_unknown` on higher-order/contraction inputs (`P0-2`); uniform `ReadOnly` & `QExt` firewall propagation (`P0-2`, `P2-2`). |
| `crates/aodc/src/ir.rs` | Modify | Update `AstSummaryCounts` for `ProbeDecl::Procedural.param_ty` (`P0-1`), `StmtKind::LetTuple`, new `Pattern` variants, and `pure_dec_families`. Emit `"syntax_discipline": "pure_wave_v2"` in `DecIR v2` when `PureWaveV2`, rebuild under `PureWaveV2` in `loads_ir`, and wrap **all** `load_hermetic_with_discipline` / `check_unit` errors inside `loads_ir` into `DiagnosticCode::IrVerificationError` (`P0-5`). |
| `crates/aodc/src/dec_audit.rs` | Modify | Support `PureWaveV2` via `LoadOptions`; add `FoldVal::Bytes(Vec<u8>)` auditing `support(b"...")` in `fold_expr`, `audit_call`, and `prepare` `inject` (`P1-2`); audit higher-order `fn_sym` builtin callbacks (`P1-2`); fold `StmtKind::LetTuple` (clearing non-`_` binders from `ConstEnv` when RHS isn't a constant tuple, `P1-2`); map positional `DoubleSlitBin(...)` via `StructDecl` fields (`P1-2`); fold `min`/`max`/`abs`/`pow`/`gcd`/`lcm`/`u8`/`b64`/`t9`/`t32`/`q4`. |
| `crates/aodc/src/eval.rs` | Modify | Fix `Window.len` via `i128` and clamp `tabulate`/`scan`/`map_seq` capacity hints to `65_536` (`P0-3`); overflow-safe `window (u, v]` iteration up to `v == i64::MAX` (`P0-3`); `Value::QExtBytes` (`P0-4`) & `Value::QExtBool` on empty `any`/`all` (`P0-2`); `.num`/`.den` on `Ratio` (`P0-4`); `StructName(...)` vs `dec StructName:` dispatch (`P1-1`); `LetTuple`, new patterns, container helpers, `bytes` overloads, and all 18 AFC operators. |
| `crates/aodc/src/codegen.rs` | Modify | Lower `.num`/`.den` on unboxed `%DecRatio` (extracting fast-path `tag == 0` or boxing handle before `@aod_rt_field_get`, `P0-4`); `QExt<bytes>` emission (`@aod_rt_emit_qext_bytes`); `StructName(...)` vs `dec StructName:` dispatch (`P1-1`); `LetTuple`, new patterns, container helpers, scalar/contraction math, and direct inline LLVM 18 loops (`%prev < %v_i64` -> `add nsw i64 %prev, 1` for `window (u, v]` at `v == i64::MAX`, `P0-3`; `65_536`-clamped capacity hint; `!llvm.loop` metadata). |
| `crates/aodc/src/lib.rs` & `main.rs` | Modify | Re-export `SyntaxDiscipline`; record `syntax_discipline` in `compute_profile_bindings`; wire `--strict-wave` into **all** subcommands (`check`, `run`, `compile`, `emit-llvm`, `build`, `jit`, `bench`, `learn`, `test`, `audit-dec`), mapping `--compat v1 --strict-wave` (and invalid flag combinations) to `CliErr::usage` (`ExitCode::from(2)`) across every subcommand (`P0-5`). |
| `tests/fixtures/v2_smoke/pure_wave_signature_matrix.{dec,stdout}` | Create | Comprehensive 100% `--strict-wave`-compliant smoke fixture (importing only `aod::*` builtin namespaces or pure-wave `.dec` modules, `P0-5`) + golden stdout verified across all 6 modes and `audit-dec --strict-wave --json`. |
| `crates/aodc/tests/phase_d2_pure_wave.rs` | Create | Exhaustive Phase D2 unit, 6-mode parity, `!llvm.loop` IR inspection, and hostile/negative-control falsification suite (`AC-01`..`AC-15`). |
| `.github/workflows/ci.yml`, `README.md`, `docs/plans/orchestration_status.md` | Modify | Add `--strict-wave` `test` and `audit-dec` steps to CI (zero Python tokens), document Phase D2 in `README.md`, and update `orchestration_status.md`. |

**Mandatory Static Token Constraint:** Never write standalone alphanumeric/`_` tokens matching `f32`, `f64`, or `unsafe` (outside the 5 allowlisted files) anywhere on non-`//` lines in `crates/*/src/**/*.rs`.

---

## 3. Exact Grammar, Type Rules, & `--strict-wave` (`PureWaveV2`) Firewalls

### 3.1 Mode & Transitive Discipline Matrix (`P0-1`, `P0-5`)
- **`CompatMode::V1` (`--compat v1`):** Frozen 10-example v1 compatibility mode (`"dec"` and `"where"` lex as ordinary identifiers; all v2/D1/D2 constructs rejected). Combining `--compat v1` with `--strict-wave` (`SyntaxDiscipline::PureWaveV2`) is rejected immediately (`ExitCode::from(2)` via `CliErr::usage` across all CLI subcommands; `DiagnosticCode::ParseError` in library APIs).
- **`CompatMode::V2` + `SyntaxDiscipline::TransitionalV2` (Phase D2 default):** Accepts both legacy transitional v2 constructs and all new Phase D2 pure-wave constructs so zero existing `.dec` files break in PR 11.
- **`CompatMode::V2` + `SyntaxDiscipline::PureWaveV2` (`--strict-wave`):** Enforced **transitively across `<entry>` and all imported `parsed_modules`/`bundled_modules`** (zero `"stdlib::*"` exemption, `P0-5`; note that in Phase D2, `--strict-wave` files import `aod::*` namespaces or pure-wave `.dec` modules since `stdlib/aod/*.dec` is migrated in Phase D3). Hard-rejects:
  1. **`{` and `}` tokens** anywhere outside `#` comments and `b"..."`/`b'...'` byte literals -> `DiagnosticCode::LexError`.
  2. **String literals `"..."` / `'...'` and `str` type** -> `DiagnosticCode::LexError` on unprefixed quotes (`b"..."` / `b'...'` remain valid) and `DiagnosticCode::TypeError` on `TypeExpr::Named("str")`.
  3. **Uppercase `Dec` keyword** -> `DiagnosticCode::ParseError`.
  4. **Header parens/params/return on `dec`** (`dec f(...) -> R:` or `dec main() -> int:`) -> `DiagnosticCode::ParseError`.
  5. **C-style prefix-typed arm params `(Type x)` / `(Type pat)` and v1 result bindings `-> Ret out:`** -> `DiagnosticCode::ParseError`.
  6. **Named call arguments `f(x: 1)`** -> `DiagnosticCode::ParseError`.
  7. **`if`, `elif`, `else` (statements, inline ternary `a if c else b`, and `if` arm guards) & `match` statements** -> `DiagnosticCode::ParseError`.
  8. **Imperative loops (`for ... in ...:`, `while ...:`, `break`, `continue`)** -> `DiagnosticCode::ParseError` in statement position (`simulate <Prep> for <N> ticks` remains valid apparatus syntax).
  9. **Mutable/destructive bindings (`mut`, C-style `Type x = val`, and `StmtKind::Assign` `x = val` / `x.f = val` / `x[i] = val`, plus binding `map` as a local/param name)** -> `DiagnosticCode::ParseError`.
  10. **Procedural `observe` probes (`P0-1`):** `ProbeDecl::Procedural` stores `param_ty: Option<TypeExpr>` and `has_arrow: bool`. Under `PureWaveV2`, `parser.rs` requires `param_ty.is_some()` and `has_arrow == true` (supporting both `tracer name(view: ReadOnly<ChamberView>) -> Ret:` and indented `tracer name:\n    (view: ReadOnly<ChamberView>) -> Ret:` with proper outer `Dedent` consumption). In `checker.rs` (both `TransitionalV2` when `param_ty` is `Some` and `PureWaveV2`), `param_ty` is resolved via `resolve_type`: `Type::ChamberView` fails with `DiagnosticCode::ObserverMutationForbidden`, any type other than `Type::ReadOnly(Box::new(Type::ChamberView))` fails with `DiagnosticCode::TypeError`!

### 3.2 `dec <name>:` Signature Combinatorics & Arm Dispatch Rules (`P1-1`)
```ebnf
DecFamily   ::= "dec" Ident ":" NEWLINE INDENT DecArm+ DEDENT
DecArm      ::= "(" [ArmParam ("," ArmParam)* [","]] ")" ["where" Expr] "->" TypeExpr ":" ArmBody
ArmBody     ::= NEWLINE INDENT Stmt+ DEDENT | Stmt
ArmParam    ::= Ident ":" TypeExpr                       /* TypedVar iff Ident != "_", else TypedPattern { Wildcard } */
              | Pattern ":" TypeExpr                     /* TypedPattern */
              | Pattern                                  /* Pattern */
```
1. **Disambiguated `ArmParam` & `looks_like_dec_arm` (`P1-1`):**
   - Track parenthesis/bracket depth in `looks_like_dec_arm` so `:` inside a `where` guard or nested type never misclassifies the arm header.
   - Under `PureWaveV2`, parse `pat = parse_pattern()?` + optional `:` `TypeExpr`: produce `DecArmParam::TypedVar { ty, name }` iff `pat == Pattern::Binding(name)` (note `"_"` parses as `Pattern::Wildcard`, so `_: Type` produces `DecArmParam::TypedPattern { ty, pattern: Pattern::Wildcard }` and never collides on `(_: t1, _: t1)`!).
   - Under `TransitionalV2`, checkpoint and accept `pat [":" TypeExpr]` when followed by `,` or `)`, otherwise backtrack cleanly to legacy prefix `TypeExpr (Ident | Pattern)`.
   - Reject duplicate non-`_` binder names within any single `Pattern` or `DecArm` parameter list (`(x: int, x: int)` or `((x, x): (int, int))`) with `DiagnosticCode::TypeError`.
2. **Explicit Register-Literal Patterns:** Extend `parse_pattern` (`CompatMode::V2`) to parse `u8(<int>)`, `b64(<int>)`, `t1(<signed_int>)`, `t9(<signed_int>)`, `t32(<signed_int>)`, and `q4(<int>)`, validating exact domain bounds at parse time (`ParseError` / `InvalidTritLiteral`).
3. **Arity-Grouped Slot Hints & Contextual `t1` Patterns (`has_header_parens == false`):**
   - Group arms by arity `k`. For each `(k, i)`:
     - If `>= 1` sibling arm of arity `k` explicitly types slot `i` as `Type::T1` and `0` arms of arity `k` explicitly type slot `i` as `int`/`u8`/`t9`/`t32`/`q4`/`b64`, rewrite `Pattern::IntLit(n)` at `(k, i)` to `Pattern::TritLit` if `n in {-1, 0, 1}` (persisted via `write_back_program`), else reject with `DiagnosticCode::InvalidTritLiteral`.
     - Require a unique concrete `slot_hint(k, i) == Some(T)` for every untyped `Pattern` leaf (`_`, `Binding`, tuple leaf); if `None` or `Conflict`, fail with `DiagnosticCode::TypeError`.
4. **Return-Type Coherence & Dead-Arm Rejection (`P1-1`):**
   - Arms resolving to the exact same `Vec<Type>` MUST declare the exact same `return_type` (`TypeError`).
   - Within the same `Vec<Type>` group, any arm appearing **after** an unguarded irrefutable catch-all arm (`guard.is_none()` and every param is `TypedVar` or an irrefutable `Pattern` / `TypedPattern` such as `_: t1` or `(a, b): (int, int)`) fails at compile time with `DiagnosticCode::TypeError`.
5. **Zero-Arg Entry & Test Discovery:** `dec main:\n    () -> Ret:` and `dec test_*:\n    () -> ():` work identically to `Dec main()` / `Dec test_*()` across checker, VM, codegen, and `aodc test`.

### 3.3 Indented `use`, `LetTuple`, `Struct` Overloads, `ratio` Fields, `map[...]`, & Phase D3 `bytes` Overloads (`P0-2`, `P0-4`, `P1-1`, `P2-1`)
1. **Indented `use <path>:` (`CompatMode::V2`):** Parses newline/comma-separated identifiers between `INDENT` and `DEDENT` (empty block -> `ParseError`). Note: in Phase D2, pure-wave files import `aod::*` builtin namespaces or pure-wave `.dec` modules (`stdlib/aod/*.dec` migrates in Phase D3).
2. **Tuple Destructuring `let (p1, p2, ...) = expr` (`CompatMode::V2`):** Enforces `>= 2` elements, non-duplicate non-`_` binders, no local shadowing, exact tuple arity, `ImplicitCoercionForbidden`, `ObserverMutationForbidden` on `ReadOnly` stripping, and `QExtLeakageForbidden`.
3. **Positional `StructName(...)` + Disjoint `dec StructName:` Overloads (`P1-1`):**
   - A non-`QExt` module declaring `struct S` may also declare `dec S:` in the **same module** provided every arm returns `S` and every arm's concrete `Vec<Type>` differs from `S`'s canonical field-type vector `(T_1, ..., T_k)` (`TypeError` otherwise).
   - Call sites `S(args...)` across `checker.rs` (`precoerce_call_args` & call checking), `eval.rs`, and `codegen.rs` dispatch to the canonical field constructor **only** when named args are present OR (`args.len() == fields.len()` AND all argument types match `(T_1, ..., T_k)`), and otherwise dispatch to `dec S:`.
4. **`.num -> int` & `.den -> int` on `ratio` (`P0-4`):**
   - Supported on `Type::Ratio` across `checker.rs`, `eval.rs`, `ops.rs`, and `codegen.rs` (handling unboxed `%DecRatio` safely without passing `%DecRatio` as an `i64` handle to `@aod_rt_field_get`!).
5. **Phase D3-Ready `bytes` Builtin Overloads & `QExt<bytes>` (`P0-4`):**
   - UTF-8-validated `Type::Bytes` overloads (trapping cleanly across all 6 modes on invalid UTF-8) for: `addr8(bytes) -> addr8`, `support(bytes) -> support`, `aod::qext::ext_decimal(bytes) -> QExt<ratio>`, `trace(bytes, bytes, q4) -> trace`, `fold(trace, trace, bytes, bytes) -> fusion`, `coupling(trace, trace, bytes) -> coupling`, `window_dedup_count(bytes, bytes, [int], [window]) -> int`, `native_cadence_ratio(bytes, int, bytes, int, bool, int) -> ratio`, and `single_contact_association(bytes, bytes, bytes, window, int) -> cadence_association`.
   - `QExt<bytes>` (`Value::QExtBytes(Vec<u8>)` / `RtValue::QExtBytes(Vec<u8>)`, rendering `Q_ext(b"...")`) + `aod::qext::qext_render_bytes(QExt<ratio>) -> QExt<bytes>` and `aod::qext::qext_format_fixed_bytes(QExt<ratio>, int) -> QExt<bytes>` (exported in `aod::qext`).
6. **Brace-Free `map[...]` Literals & Pure Container Helpers (`P0-2`, `P2-1`):**
   - In `CompatMode::V2`, `Ident("map")` + `[` parses as `ExprKind::MapLit` when immediately followed by `]` (`map[]`) or `(` (`map[(k1, v1), ...]`, parsing each pair explicitly as `(k_expr, v_expr)` with optional trailing `,`; in `TransitionalV2`, `map[idx]` where `idx` does not start with `(` or `]` still parses as indexing on a legacy local named `map`, while `map` is reserved from local/param names in `PureWaveV2`).
   - Pure persistent copy-on-write builtins (all rejecting `Type::Unit | contains_unknown`, and enforcing bidirectional `ReadOnly` firewalls, `P0-2`):
     - `map_put(m, k, v) -> map[K, V]` & `map_remove(m, k) -> map[K, V]` (reject `ReadOnly` `m` or `ReadOnly`-stripping `k`/`v` with `ObserverMutationForbidden`).
     - `map_get_or(m, k, default) -> V`, `map_keys(m) -> [K]`, `map_entries(m) -> [(K, V)]` (propagate `ReadOnly` when `m` is `ReadOnly`).
     - `list_set(xs, idx, val) -> [T]` & `list_push(xs, val) -> [T]` (reject `ReadOnly` `xs` **and** `ReadOnly`-stripping `val` with `ObserverMutationForbidden`; `list_set` traps on out-of-bounds `idx`).
     - `concat_seq(a, b) -> [T]` (returns `ReadOnly<[T]>` whenever `a.is_read_only() || b.is_read_only()`, `P0-2`) & `slice_seq(xs, start, end) -> [T]` (propagates `ReadOnly`; traps on invalid bounds).
   - `select(cond, a, b)` unifies `[]` / `map[]` against concrete `[T]` / `map[K, V]` on the opposite arm.

---

## 4. Exact Specification of the 18 AFC Scope/Sequence, Contraction, & Scalar Math Operators

All 18 operators are available unqualified in `CompatMode::V2` and via `aod::core::*` (rejected in `CompatMode::V1`).

### 4.1 Higher-Order AFC Scope & Sequence Operators (`P0-2`, `P0-3`, `P2-2`)
- **Concrete Input Rule (`P0-2`):** Propagate outer expected type into `evolve`/`fold_seq`'s `init` via `coerce_expr_to_expected`, then reject `ty == Type::Unit || contains_unknown(&ty)` on `seq`, `init`, and `env` with `DiagnosticCode::TypeError` (so unannotated `[]` / `map[]` never reaches `codegen.rs`!).
- **Static `fn_sym` Resolution (`P2-2`):** `fn_sym` must be an unshadowed `ExprKind::Ident` or `ExprKind::Path` resolving to a user `dec` family (grouping pattern/guard arms sharing the exact inferred concrete parameter tuple `(S, Elem[, C])` / `(Elem[, C])` / `(int[, C])`) or an `aod::*` builtin; bare `Struct` / `Enum::Variant` constructors without a `dec` family, shadowed locals/`const`s, or ambiguous return types fail with `DiagnosticCode::TypeError`. Checked transitively under caller's `Effect` (`check_transitive_effect`).
- **Uniform `ReadOnly` Propagation (`P0-2`):** For any `seq_ty`, `Elem = if seq_ty.is_read_only() && raw_elem.needs_readonly_propagation() { raw_elem.wrap_read_only() } else { raw_elem }`. `filter_seq` returns `ReadOnly<[raw_elem]>` when `seq_ty.is_read_only()`; `zip_seq(a, b)` wraps composite `Elem_A` / `Elem_B` when `a` / `b` is `ReadOnly`.

| Operator | Signature(s) | Domain / Sequence & Exact Semantics |
| :--- | :--- | :--- |
| `evolve` | `(init: S, dom, step_fn[, env: C]) -> S` | `dom: range` (`a..b` -> `k = a .. b-1`; `a >= b` returns `init`) or `window` (`(u, v]` -> `k = u+1 .. v`, poison/overflow-safe at `v == i64::MAX`, `P0-3`). |
| `scan` | `(init: S, src, step_fn[, env: C]) -> [S]` | `src: range \| window \| [T] \| array[T, N] \| bytes`. Emits post-step trajectory `[s_1, ..., s_m]` (`[]` when empty). |
| `fold_seq` | `(init: S, seq, step_fn[, env: C]) -> S` | `seq: [T] \| array[T, N] \| bytes`. Left-to-right fold; returns `init` if empty. |
| `map_seq` | `(seq, map_fn[, env: C]) -> [U]` | `seq: [T] \| array[T, N] \| bytes`. `U != ()`, concrete. |
| `filter_seq` | `(seq, pred_fn[, env: C]) -> [Elem]` | `seq: [T] \| array[T, N] \| bytes`. Preserves `ReadOnly<[T]>` when `seq` is `ReadOnly` (`P0-2`). |
| `tabulate` | `(dom, gen_fn[, env: C]) -> [U]` | `dom: range \| window`. `U != ()`, concrete. |
| `zip_seq` | `(seq_a, seq_b) -> [(Elem_A, Elem_B)]` | Zero silent truncation (`N != M` compile-time `TypeError` on arrays; 6-mode `RuntimeError` trap on dynamic length mismatch); preserves per-side `ReadOnly` (`P0-2`). |
| `count` | `(seq, pred_fn[, env: C]) -> int` | Rejects `seq_ty.is_qext() \|\| env_ty.is_qext()` or `QExt<bool>` predicate with `QExtLeakageForbidden` (`P0-2`). |
| `any` | `(seq, pred_fn[, env: C]) -> bool` (or `QExt<bool>`) | Left-to-right short-circuit on first `true`; returns `false` (`Value::QExtBool(false)` when `pred_fn -> QExt<bool>`, `P0-2`) on empty. |
| `all` | `(seq, pred_fn[, env: C]) -> bool` (or `QExt<bool>`) | Left-to-right short-circuit on first `false`; returns `true` (`Value::QExtBool(true)` when `pred_fn -> QExt<bool>`, `P0-2`) on empty. |

**LLVM 18 Lowering & Safety (`P0-3`):**
- Lower `window (u, v]` loops by initializing `prev = u`, checking `icmp slt i64 %prev, %v_i64` in `cond`, computing `%k = add nsw i64 %prev, 1` inside `body` (`%prev < %v_i64 <= i64::MAX` guarantees zero `nsw` poison/wrap even at `v == i64::MAX`), and advancing `prev = %k` in `step`.
- Clamp pre-allocation capacity hints in `@aod_rt_list_with_capacity` and `eval.rs` to `0..=65_536` so `tabulate(0..9223372036854775807, f)` under `--fuel 10` traps cleanly on fuel instead of OOM-aborting!
- Attach `cg.alloc_loop_hint_md()` (`!llvm.loop`) to every inline loop backedge.

### 4.2 Exact Contraction & Scalar Math Builtins (`P0-2`, `P0-3`)
- **`sum(seq) -> T` & `dot(seq_a, seq_b) -> T`:** Concrete `T in {int, u8, t1, t9, t32, ratio}` (plus `QExt<ratio>` in `Effect::QExt`); reject `contains_unknown` (`sum([])`, `dot([], [])`), `b64`, mismatched types (`ImplicitCoercionForbidden`), or mismatched lengths (`TypeError` on static arrays, 6-mode `RuntimeError` on dynamic lists).
- **`min(a, b)` & `max(a, b)`:** Identical `T in {int, u8, b64, t1, t9, t32, ratio}` (plus `QExt<ratio>` in `Effect::QExt`).
- **`abs(x)`:** `T in {int, t1, t9, t32, ratio}` (plus `QExt<ratio>` in `Effect::QExt`), routing `int`/`ratio` negation via `@__aod_int_neg` / `@__aod_ratio_neg` so `abs(i64::MIN)` spills cleanly to `+9223372036854775808`.
- **`pow(base, exp)`:** `(int, int) -> int`, `(t9, int|t9) -> t9`, `(t32, int|t32) -> t32`, `(ratio, int) -> ratio`, enforcing `0 <= exp <= 65_536` AND the `1_048_576`-bit result ceiling on `BigInt`/`Ratio` (`P0-3`), trapping cleanly across all 6 modes.
- **`gcd(a, b)` & `lcm(a, b)`:** `(int, int) -> int`, `(t9, t9) -> t9`, `(t32, t32) -> t32` with `i64::MIN` spill safety and checked `t9`/`t32` overflow traps.

---

## 5. Updated Acceptance Criteria (`AC-01` .. `AC-15`)

- **`AC-01` (Pure Indented `dec <name>:` Signature Matrices, Procedural `observe` Typing, & Multi-Arity/Polymorphic Dispatch):** Lowercase `dec <name>:` blocks and typed procedural `observe` probes (`(view: ReadOnly<ChamberView>) -> Ret:`) execute identically across all 6 modes with `name: Type`, `_: Type` (`(_: t1, _: t1)` non-colliding), register-literal patterns (`u8`, `b64`, `t1`, `t9`, `t32`, `q4`), trit/hinge/dwell + contextual `-1`/`0`/`+1` patterns, `where <expr>` guards (including `:` inside nested expressions), multi-arity overloads, and disjoint return-type polymorphism.
- **`AC-02` (Compile-Time Signature-Matrix Soundness Guards):** Rejects (a) ambiguous/conflicting untyped `_` or `x` slots, (b) out-of-range contextual integer in a `t1` slot (`InvalidTritLiteral`), (c) duplicate non-`_` binder names within an arm/pattern (`(x: int, x: int)`, `((x, x): (int, int))`), (d) mismatched return types on identical `Vec<Type>`, and (e) dead arms after an unguarded catch-all (`TypedVar` or irrefutable `Pattern`/`TypedPattern`). Unmatched runtime guard traps cleanly across all 6 modes.
- **`AC-03` (Indented `{}`-Free `use`/`struct`/`enum`, Positional `Struct(...)` + `dec Struct:` Overloads, `.num`/`.den`, & `bytes` Overloads):** Indented `use`/`struct`/`enum`, canonical `Struct(...)` + same/different-arity disjoint `dec Struct:` overloads, `.num`/`.den` on `ratio`, `QExt<bytes>` (`qext_render_bytes`, `qext_format_fixed_bytes`), and `bytes` overloads (`addr8`, `support`, `ext_decimal`, `trace`, `fold`, `coupling`, `window_dedup_count`, `native_cadence_ratio`, `single_contact_association`, rejecting invalid UTF-8) match byte-for-byte across all 6 modes.
- **`AC-04` (Immutable Tuple Return Destructuring `let (x, y, ...) = expr`):** Works across all 6 modes and `DecIR v2`, rejecting arity mismatches, duplicate binders, implicit coercions, `ReadOnly` stripping, and `QExt` leakage.
- **`AC-05` (Brace-Free `map[...]` Literals & Pure Container Helpers):** `map[(k1, v1), ...]`, contextual `map[]` (including in `select`), `map_put`, `map_remove`, `map_get_or`, `map_keys`, `map_entries`, `list_set`, `list_push`, `concat_seq`, and `slice_seq` match across all 6 modes with copy-on-write semantics and bounds traps.
- **`AC-06` (Higher-Order AFC Scope & Sequence Operators — 6-Mode Parity, `v == i64::MAX`, Fuel Cap, & Short-Circuit):** `evolve` (including `window (i64::MAX - 3, i64::MAX]` with zero `nsw` poison/wrap!), `scan`, `fold_seq`, `map_seq`, `filter_seq`, `tabulate` (including `tabulate(0..9223372036854775807, f)` under `--fuel 10` trapping cleanly on fuel without OOM!), `zip_seq`, `count`, `any`, and `all` (including empty-sequence `QExt<bool>` rendering `"Q_ext(false)"`/`"Q_ext(true)"` and left-to-right short-circuiting) match across all 6 modes, while unannotated `[]` inputs (`map_seq([], f)`, `sum([])`) fail cleanly at compile time (`TypeError`).
- **`AC-07` (`zip_seq` & `dot` Zero-Truncation Length Enforcement):** Rejects mismatched static `array` lengths at compile time and traps across all 6 modes on dynamic length mismatches.
- **`AC-08` (Exact Contraction & Scalar Math Edge Cases Across 6 Modes):** `sum`, `dot`, `min`, `max`, `abs(i64::MIN)`, `gcd(i64::MIN, 0)`, `lcm(t32(61), t32(137)) == t32(8357)`, and `pow` (including clean 6-mode traps on `exp < 0`, `exp > 65536`, and `pow(pow(10, 65536), 65536)` exceeding the `1_048_576`-bit ceiling!) match across all 6 modes.
- **`AC-09` (Inline LLVM 18 Lowering & `CodegenPlan` `!llvm.loop` Metadata Preservation):** Verified via `LLVMVerifyModule` and `!llvm.loop` inspection on all lowered higher-order loops with zero heap closures.
- **`AC-10` (`--strict-wave` `PureWaveV2` Hostile Matrix, Transitive Module Enforcement, & CLI Exit `2`):** Rejects all banned constructs (`{ }`, `"..."`, `str`, `Dec`, header parens, `(int x)`, `-> int out:`, named args, `if`/`elif`/`else`, ternary `if`, `if` guards, `match`, `for`, `while`, `break`, `continue`, `mut`, `int x = 1`, `x = 2`, local `let map = ...`, untyped procedural `observe`, and importing a non-`PureWaveV2` `.dec` module under `--strict-wave`); `--compat v1 --strict-wave` exits **`2`** across `check`, `run`, `audit-dec`, and `test`.
- **`AC-11` (`ReadOnlyObserver` & `quarantine Q_ext` Firewalls on Phase D2 Constructs):** Rejects bare `tracer bad(view: ChamberView) -> ():` (`ObserverMutationForbidden`), `map_put`/`map_remove`/`list_set`/`list_push` on `ReadOnly` containers or `ReadOnly`-stripping values (`ObserverMutationForbidden`), `ReadOnly` stripping through `filter_seq`/`concat_seq`/`zip_seq`/`map_seq`/`fold_seq`/`LetTuple` (`ObserverMutationForbidden`), `QExt` callbacks outside `quarantine Q_ext` (`QExtLeakageForbidden`), and any `is_qext()` input/predicate to `count` (`QExtLeakageForbidden`).
- **`AC-12` (Hermetic `DecIR v2` & `RetainedProfile` `PureWaveV2` Round-Trip & `IrVerificationError` Tamper Rejection):** `aodc compile --strict-wave` embeds `"syntax_discipline": "pure_wave_v2"`, separates `compute_profile_bindings`, and `loads_ir` rejects tampered `PureWaveV2` source/module payloads (even with recomputed SHA-256) and invalid `"syntax_discipline"` values specifically with `DiagnosticCode::IrVerificationError`.
- **`AC-13` (6-Hook `aodc audit-dec --strict-wave` Verification Including `support(b"...")` & Higher-Order Callbacks):** Passes `6/6` hooks on `pure_wave_signature_matrix.dec`, catches `support(b"3:3:6_yro")` (`sheddic_path_audit: FAIL`) and higher-order invalid `sadar_promote` / `support_rd` callbacks under `--strict-wave`, and preserves `11/11` `manual_examples/*.dec` under default `TransitionalV2`.
- **`AC-14` (Pure-Wave Smoke Fixture 6-Mode + `-O3` + CLI Parity):** `tests/fixtures/v2_smoke/pure_wave_signature_matrix.dec` matches `.stdout` byte-for-byte across `check`, `test`, `run`, `compile`+`exec`, `build -O0/-O2/-O3`, and `jit -O0/-O2/-O3` under `--strict-wave`.
- **`AC-15` (Zero Regressions, Zero Floats, 5-File `unsafe` Allowlist, & Offline Workspace Gate):** Full debug + release workspace builds/tests (`-D warnings`, `--offline`), `10/10` v1, `5/5` `v2_smoke`, `18/18` `manual_examples`, `manual-data --check`, `manual-verify docs/manual`, `verify-s2 --s4-report`, `aod-lint --strict` `13/13`, `SHA256SUMS` `15/15`, and 0 sibling repo mutations.

---

## 6. Explicit Out-of-Scope & Non-Claims Boundary

1. **Pulled Forward Cleanly into PR 11 vs. Deferred to Phase D3 (PR 12):** Rewriting `stdlib/aod/*.dec` (`7/7`) and `manual_examples/` (`18/18`, with `QExt<bytes>` / `bytes` `.stdout` renders, `*_bytes` accessors, `aodc fmt`, and `--strict-wave` `Unit ()` -> `Null` enforcement) was completed ahead of schedule in PR 11 while preserving 100% `TransitionalV2` default compatibility and `docs/manual/data/*.json` byte parity. Porting `reference/aod-dec-dev/dec/00..05+kernel.dec` into `examples/first_principles/`, migrating inline Dec blocks in `docs/manual/chapters/*.typ` & remaining `tests/fixtures/v2_*`, removing legacy `str` fields/types in Dec v2, and flipping the global `CompatMode::V2` default from `TransitionalV2` to `PureWaveV2` belong strictly to **Phase D3 (PR 12)**.
2. **Zero Invented Physics & Zero Hardcoded Outcomes:** Chamber simulation remains `DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED`; JIT policy learning remains `COMPILER_EXECUTION_POLICY_NOT_PHYSICAL_AOD_LAW`; zero physical target outcomes are hardcoded.
3. **Zero Upstream Mutation:** Zero files modified outside `/home/antigravity/rpoyau/aod-dec`.
