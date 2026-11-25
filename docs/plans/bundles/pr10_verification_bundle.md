# PR 10 Independent Verification Bundle (14/14 PASS)

- **Repository:** `/home/antigravity/rpoyau/aod-dec` (`v2/zero-python-evolution` atop `b0e4cc6`)
- **Validator:** `0414657fc81413ec9e5daa10e84616be`
- **Request Bundle Audited:** [`pr10_request_bundle.md`](pr10_request_bundle.md) (`R02`, `AC-01`..`AC-14`)
- **Result:** **14/14 PASS (`AC-01`..`AC-14`), 0 FAIL**.

## Headline Evidence
- **Builds & Tests (`AC-14`):** `111/111` passed in BOTH debug and release (`aod-core`: `18`, `aodc`: `93`, including `phase_d1_purification`: `13/13` and `source_binding`: `3/3`) with `RUSTFLAGS="-D warnings"` (`0` warnings, `--offline`).
- **Wave-Hardware Register & Sequence Substrate (`AC-01`..`AC-08`):**
  - `Null` (`null`) non-conflation (`9/9` `ImplicitCoercionForbidden` vs `()`, `0`, `u8(0)`, `b64(0)`, `0t`, `t9(0)`, `t32(0)`, `q4(0)`, `00_8`), `--compat v1` isolation, and `0b11` rejection in `T32::unpack` / `unpack_t9` across VM/AOT/JIT.
  - `u8` (`0..=255`, 7 constructors, unsigned `icmp ugt i8`, pre-op `//0`, `%0`, `<<8`, `>>8` traps with zero SIGFPE/poison), `b64` (`0x`/`0b` strict lexing & v1 rejection, packed `b64(t32)`/`t32(b64)` 1-cycle wire-swap `== neg(w)` at `0, ±1, ±T32::LIMIT`, `>=64` shift guard, arithmetic banned), and `t9` (`[-9841, +9841]`, exact floor `t9(-7)//t9(3)==t9(-3)` & `t9(-7)%t9(3)==t9(2)`, 18-bit `pack(t9)`/`unpack_t9(int|b64)`).
  - 9-pair `(t1, t1)` + OR-based `T32::superpose_cancel` (`(+1,+1)->+1`, `(-1,-1)->-1`, `(+1,-1)->0`), `q4` bitwise/Hamming, eager left-to-right `select(cond, a, b)` lowering to native `select i1` with `ReadOnly`/`QExt` firewalls.
  - Immutable `bytes` (`b"..."`, 5 constructors, non-clamping `slice_bytes`, `concat_bytes`, NIST `sha256_bytes` + 64B ASCII `hex_bytes`, `MapKey::Bytes`) and fixed-lane `array[T, N]` (`array(...)` & contextual `[...]` runtime `Array` identity, `ReadOnly<array[T, N]>` `ObserverMutationForbidden`, `is_qext` & `is_map_key`).
  - `tests/fixtures/v2_smoke/hw_register_primitives.dec` 6/6 byte-identical stdout (`29b6e2599edd4d63f3cb66e0e2fee12f4311034cad14779d89d078c190a5f4f2`) across VM, `DecIR v2` (`d2ba0b75...`), AOT `-O0`/`-O2`, and ORC LLJIT `-O0`/`-O2`.
- **First-Principles Derivation, Live-Walk Scanner & 6-Hook `aodc audit-dec` (`AC-09`..`AC-11`):**
  - `docs/FIRST_PRINCIPLES_DERIVATION.md` byte-identical (`091d8f1ef7dd3b9c1f96eab7adc9ff5db67194418c2dbfad1a0fcab68b5afabc`) and linked in `README.md`.
  - `lex_live_walk` starts at `00_8` (min width 2), burns dwell monotonically (`4` vs `136`), and preserves `final_anchor = 10_8` across compact vs padded `.dec`.
  - `aodc audit-dec` passes `11/11` `manual_examples/*.dec` (`6/6` hooks, wired in `.github/workflows/ci.yml`), exits `2` before reading input on symlink/in-repo `--report`, and exits `1` (`5/6` hooks, portable `aod.dec.audit.v1` JSON) across all 6 distinct hostile `.dec` programs individually falsifying `spine_audit`, `boundary_audit`, `sadar_audit`, `sheddic_path_audit`, `cycle_audit`, and `frg_audit`.
- **Two-Thread FRG Double-Slit Probe & Regression Gates (`AC-12`, `AC-13`):**
  - `aodc probe-hw` live `E4 (RD=9)`, `C6 (RD=61)`, and barrier-synchronized `C8 (RD=137, T_sync=8357)` (`t_zero=274, t_cross=548, r_e=274, inter_thread_handoffs=1, op_certified=true`, zero `.max(1)` clamping, `COMPILER_EXECUTION_POLICY_NOT_PHYSICAL_AOD_LAW`), plus 3-state `Op_e` (`Some(r>0)`, `Some(0)`, `None`).
  - `10/10` `--compat v1`, `4/4` `v2_smoke`, `18/18` `manual_examples`, `verify-s2 --s4-report` debug == release `cmp` (`6b75094f...`), `manual-data --check`, `manual-verify docs/manual` (`18/18`), `pdf_audit` (`15/15`), external `rpoyau/aod-lint --strict` `13/13` `.typ`, `ldd` zero `llvm`/`python`/`gmp`, `reference/aod-dec-dev/SHA256SUMS` `15/15` OK, and `8/8` sibling repos clean.
