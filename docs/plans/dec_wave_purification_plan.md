# `rpoyau/aod-dec` First-Principles Wave-Language Purification Plan (`Phase D1` → `D2` → `D3`)

**Baseline:** `rpoyau/aod-dec` branch `v2/zero-python-evolution` ([PR #9](https://github.com/rpoyau/aod-dec/pull/9) open — held unmerged until `D1`–`D3` land cleanly).  
**Authority:** [`reference/aod-dec-dev/`](../../reference/aod-dec-dev/) (`SHA256SUMS` 15/15 verified), [`docs/FIRST_PRINCIPLES_DERIVATION.md`](../FIRST_PRINCIPLES_DERIVATION.md), `rpoyau/aod` `manual/sections/00_dec_ledger.tex`, `rpoyau/afc`, and [`SOURCE_BINDING.json`](../../SOURCE_BINDING.json) (`S0`, `S1b`, `S2@3122c94`, `S4@e013401`).  
**Process Status:** Planner ✅ Complete → Plan Review ✅ **`APPROVE` (`R02`, all 7 `P0` / 3 `P1` / 2 `P2` resolved — see [`bundles/pr10_request_bundle.md`](bundles/pr10_request_bundle.md))**.

> [!IMPORTANT]
> **Target Dec v2 Language Identity (Enforced Repo-Wide in `D3`, with `--compat v1` Strictly Isolated for Legacy Fixtures):**
> - **Explicit Signature Combinatorics:** Lowercase `dec <name>:` family header with indented `(a: T1, b: T2) -> Ret:` $\mathcal{AO}_\mu = \langle \Pi_\alpha, H_\mu, \Pi_\Omega \rangle$ rows across the structural hinge `->` (`0_H`). Unlisted/blocked combinations have zero admissibility ($\operatorname{adm}(e; B) = 0 \implies P(e; B) = 0$).
> - **Zero `{ }` Anywhere:** Pure indentation for `struct`, `enum`, `use`, `chamber`, `prepare`, `observe`, `quarantine`; uniform positional `TypeName(arg1, ...)` constructors; `map[(k, v), ...]` / `map[]`.
> - **Zero `if`/`elif`/`else`, `for`, `while`, `break`, `continue`, `mut`:** Replaced by `dec` signature-row pattern/`where` admissibility dispatch, branchless wave algebra (`min`, `max`, `neg`, `superpose_cancel`, eager `select(cond, a, b)`), and engine-driven AFC Scope/Window execution (`evolve`, `scan`, `fold_seq`, `map_seq`, `filter_seq`, `tabulate`, `sum`, `dot`, `∂`).
> - **Hardware Register & Byte-Sequence Primitives (No Native Domain `str`):** `Null` (`null`), `u8`, `b64`, `t9`, `t1`, `t32`, `q4`, `addr8`, `int`, `ratio`, `bytes` (`b"..."` immutable `[u8]` slice), and `array[T, N]`; discrete status/route fields use typed `enum` / `u8`.

---

## Target Syntax Preview

```dec
use stdlib::aod::ledger:
    RoleSums
    KernelFour

struct ShedState:
    burden: ratio
    total_outward: ratio
    total_local: ratio

dec add_role:
    (acc: RoleSums, p: ratio, -1t) -> RoleSums:
        return RoleSums(acc.neg_sum + p, acc.hinge_sum, acc.pos_sum)
    (acc: RoleSums, p: ratio, 0t) -> RoleSums:
        return RoleSums(acc.neg_sum, acc.hinge_sum + p, acc.pos_sum)
    (acc: RoleSums, p: ratio, +1t) -> RoleSums:
        return RoleSums(acc.neg_sum, acc.hinge_sum, acc.pos_sum + p)

dec clip_min:
    (a: int, b: int) where a < b -> int:
        return a
    (a: int, b: int) -> int:
        return b
```

---

## Atomic 3-Milestone Execution Roadmap

Splitting the overhaul into **three atomic commits (`PR 10` → `PR 11` → `PR 12`)** keeps `RUSTFLAGS="-D warnings" cargo test --workspace --offline` (debug + release), S2 (30/30), S4 (90-tick), Typst Manual (`18/18`), PDF zero-raster audit, and `aod-lint` (`13/13`) green at every boundary.

### Phase D1 — PR 10: Hardware Register Primitives, `bytes`, §5.17 Live-Walk Audit, & FRG Double-Slit Probe *(Reviewer `APPROVE` `R02` — Full Spec in [`bundles/pr10_request_bundle.md`](bundles/pr10_request_bundle.md))*
1. **Hardware-Honest Register & Sequence Types** ([`ternary.rs`](../../crates/aod-core/src/ternary.rs), [`value.rs`](../../crates/aod-rt/src/value.rs), [`abi.rs`](../../crates/aod-rt/src/abi.rs), [`types.rs`](../../crates/aodc/src/types.rs), [`checker.rs`](../../crates/aodc/src/checker.rs), [`eval.rs`](../../crates/aodc/src/eval.rs), [`codegen.rs`](../../crates/aodc/src/codegen.rs), [`ir.rs`](../../crates/aodc/src/ir.rs)):
   - `Null` (`null` singleton, distinct from `()`, `0`, `0t`, `q4(0)`, `00_8`, and `0b11` collision lanes).
   - `u8` (`0..=255` unsigned register with pre-op LLVM zero-div & `>= 8` shift guards), `b64` (raw 64-bit register view with `0x`/`0b` literals, packed `b64(t32)` / `t32(b64)` 1-cycle wire-swap view, `popcnt`, `bitnot`, `>= 64` shift guard), and `t9` (`[-9841, +9841]` checked 9-trit scalar with floor `//`/`%` and 18-bit `pack(t9)` / `unpack_t9(int|b64)`).
   - OR-based `superpose_cancel(t1|t32, ...)` (`(pa | pb) & clear_mask`, preserving same-sign constructive crests `(+1,+1) -> +1` while cancelling opposing `(+1,-1) -> 0` — fixing the XOR bug in `reference/aod-dec-dev/bootstrap/aod_stage0_seed.rs`), `q4` bitwise `&`/`|`/`^` + `is_hamming1` + `popcnt_xor`, and eager branchless `select(cond, a, b)` lowering directly to LLVM `select i1`.
   - Immutable `bytes` (`b"..."`, `bytes([u8]|str|bytes|b64|addr8)`, `b[i] -> u8`, `slice_bytes`, `concat_bytes`, `sha256_bytes` -> 32B, `hex_bytes` -> 64B ASCII `bytes` with zero `str` dependency) and fixed-lane `array[T, N]` (`array(...)`, `array_set`, `ReadOnly`/`QExt`/`MapKey` firewalls) verified across all **6 execution modes** (`VM`, `DecIR v2`, `AOT -O0/-O2`, `ORC LLJIT -O0/-O2`).
2. **First-Principles Derivation & `aodc audit-dec` (§5.17 Audit Hooks):**
   - Vendored [`docs/FIRST_PRINCIPLES_DERIVATION.md`](../FIRST_PRINCIPLES_DERIVATION.md) byte-for-byte (`091d8f1ef7dd3b9c1f96eab7adc9ff5db67194418c2dbfad1a0fcab68b5afabc`).
   - Add `crates/aodc/src/dec_audit.rs` (`#![forbid(unsafe_code)]`) + `aodc audit-dec` + CI step: live-walk `00_8` octal anchor & monotonic sameness dwell reduction (`xxxxx... is x`) + AST/type-backed verification of all 6 §5.17 hooks (`Spine`, `Boundary`, `SADAR`, `SheddicPath`, `Cycle`, `FRG`) tested against 11 positive examples and 6 hostile `.dec` falsifications.
3. **Live 3-Tier / Two-Thread FRG Double-Slit Cross-Return Hardware Probe** ([`hw_probe.rs`](../../crates/aod-llvm/src/hw_probe.rs), [`learner.rs`](../../crates/aod-llvm/src/learner.rs)):
   - Live `E4 (RD=9)`, `C6 (RD=61)`, and barrier-synchronized two-thread shared-cache-line `C8 (RD=137, T_sync=8357)` probe measuring $T_k^\times - T_k^0 = r_e$ with zero `.max(1)` clamping and 3-state Complete-State `Op_e` source-removal verification (`Some(r > 0)`, `Some(0)`, `None` $S_k^{+,-e}$), strictly scoped as `COMPILER_EXECUTION_POLICY_NOT_PHYSICAL_AOD_LAW`.

---

### Phase D2 — PR 11: Pure Indented `dec` Signature-Matrix Frontend & AFC Scope Operators
1. **Pure Indented Grammar & Type Constructors** ([`lexer.rs`](../../crates/aodc/src/lexer.rs), [`parser.rs`](../../crates/aodc/src/parser.rs), [`checker.rs`](../../crates/aodc/src/checker.rs)):
   - Lowercase `dec <name>:` family headers (zero header params) + indented `(a: T1, 0) [where pred] -> Ret:` signature arms (`name: Type`, literal/trit/enum/tuple patterns, tuple return destructuring `let (x, y) = f(...)`).
   - Pure indented `use path:` blocks, uniform positional `TypeName(arg1, ...)` `dec` constructors for all `struct`s, and `map[(k, v), ...]` / `map[]` literals.
2. **Engine-Driven AFC Scope/Window & Vector Contractions** ([`eval.rs`](../../crates/aodc/src/eval.rs), [`codegen.rs`](../../crates/aodc/src/codegen.rs), [`abi.rs`](../../crates/aod-rt/src/abi.rs)):
   - First-class AFC scope & sequence operators compiled directly to native LLVM loops/SIMD inside the engine (zero user `for`/`while`!): `evolve`, `scan`, `fold_seq`, `map_seq`, `filter_seq`, `tabulate`, `zip_seq`, `sum`, `min`, `max`, `abs`, `pow`, `gcd`, `lcm`, `dot`, `count`, `any`, `all`.
   - Add `SyntaxDiscipline::PureWaveV2` (`--strict-wave`, recorded in `DecIR v2`) and verify 6-mode parity before flipping the global default in `D3`.

---

### Phase D3 — PR 12: Full Stdlib / Manual / First-Principles Migration & Hard `PureWaveV2` Enforcement
1. **Port Original `reference/aod-dec-dev/dec/` Suite (`examples/first_principles/`):**
   - Port all 7 modules (`00_primitives` .. `05_self_host_compiler_elf64` + `kernel.dec`) into canonical brace-free, loop-free Dec v2 with 6-mode VM/DecIR/AOT/JIT parity and `6/6` `aodc audit-dec` pass.
2. **Rewrite All Stdlib, Manual Examples, & Typst Manual Chapters:**
   - Rewrite all 7 `stdlib/aod/*.dec` modules, all 18 `manual_examples/*.dec` fixtures, `docs/manual/chapters/*.typ` + `aodc manual-data`, and `tests/fixtures/v2_*` into pure `dec <name>:` signature-matrix wave style.
   - Replace stringly-typed native status/stage fields (`model_status`, `scientific_status`, `stage`) with typed `enum`s and `bytes`-backed `Q_ext` / hash interfaces (`ext_decimal(bytes)`, `QExt<bytes>`, 32-byte `sha256_bytes`).
3. **Flip `CompatMode::V2` Unconditionally to `PureWaveV2` + Permanent Repo Guard:**
   - Hard-reject `{`, `}`, single-line/uppercase `Dec`, `if`/`elif`/`else`, `for`, `while`, `break`, `continue`, `mut`, and primitive `str` in Dec v2 (`--compat v1` remains strictly isolated for the 10 frozen v1 fixtures).
   - Add permanent `pure_wave_dec_v2_repository_guard` in [`source_binding.rs`](../../crates/aodc/tests/source_binding.rs) + full live 3-job GitHub Actions CI green check on PR `#9`.
