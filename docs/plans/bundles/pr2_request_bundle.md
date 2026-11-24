# PR 2 Request Bundle (Rev 2 — Reviewer-Amended & Approved) — `rpoyau/aod-dec` v2
## Unboxed-First LLVM 18 AOT & In-Process ORC LLJIT Native Backend

**Base Commit:** `55da6d7` (PR 1 green across all 5 gates)  
**Host Toolchain Verified:** `rustc 1.100.0`, `cargo 1.100.0`, `/usr/lib/llvm-18/lib/libLLVM.so.18.1`, `clang`/`cc`, Rust `edition = "2024"`.  
**Hard Constraints:** 100% `std`-only Rust workspace (ZERO crates.io deps), ZERO IEEE-754 floats (`f32`/`f64`) across ALL 4 crates, `#![forbid(unsafe_code)]` preserved on `aod-core` and `aodc`, `unsafe` strictly quarantined to `crates/aod-rt/src/abi.rs` and `crates/aod-llvm/src/{ffi.rs,jit.rs,aot.rs}`, ZERO mutation of `/home/antigravity/rpoyau/aod`, ZERO hardcoded physical outcomes.  
**Reviewer Mandatory Amendments Folded:** `P0-1`, `P0-2`, `P0-3`, `P1-1`, `P1-2`, `P1-3`.

---

## 1. Acyclic Crate Dependency Graph & Workspace Lint Policy

```text
crates/aod-core   (rlib; #![forbid(unsafe_code)]; ZERO deps; ZERO floats)
       ▲
crates/aod-rt     (rlib + staticlib libaod_rt.a; #![deny(unsafe_code)]; unsafe ONLY in src/abi.rs;
       ▲           owns shared sim engine, RtValue/render, Arena, exact cold-spill ops, C ABI)
crates/aod-llvm   (rlib; #![deny(unsafe_code)]; unsafe ONLY in src/{ffi.rs,jit.rs,aot.rs};
       ▲           loads libLLVM.so.18.1 via dlopen, verifies/optimizes IR, emits AOT ELF & ORC LLJIT)
crates/aodc       (lib + bin; #![forbid(unsafe_code)]; owns frontend, firewalls, DecIR, VM,
                   safe textual LLVM 18 IR lowering in src/codegen.rs, CLI build/jit/emit-llvm/bench)
```

- Root `Cargo.toml`: `members = ["crates/aod-core", "crates/aod-rt", "crates/aod-llvm", "crates/aodc"]`; keep `warnings = "deny"`, remove workspace-wide `unsafe_code = "forbid"` so per-crate lints apply (`#![forbid(unsafe_code)]` on `aod-core` & `aodc`; `#![deny(unsafe_code)]` on `aod-rt` & `aod-llvm` with `#[allow(unsafe_code)]` ONLY on the 4 allowlisted files).
- Move `crates/aodc/src/sim.rs` (`AxisDir`, `ProbeKind`, `WallPolicy`, `BoxRegion`, `CarrierState`, `ReferenceClock`, `ChamberState`, `ChamberSnapshot`, `DeclarativeObserver`, `SimRunResult`, `MODEL_STATUS`) into `crates/aod-rt/src/sim.rs` and re-export from `crates/aodc/src/sim.rs` & `ast.rs`.
- **Fixture isolation (`P1-2`):** Place the compute benchmark at `tests/fixtures/v2_bench/unboxed_loop_bench.dec` (DO NOT add files to `tests/fixtures/v2_smoke/` so PR 1's `3 passed; 0 failed; 3 total` test remains untouched!).

---

## 2. Exact File Tree & Module Responsibilities

- `Cargo.toml` (4-crate workspace, zero external deps)
- `crates/aod-rt/`: `Cargo.toml` (`crate-type = ["rlib", "staticlib"]`), `src/{lib.rs, sim.rs, value.rs, arena.rs, ops.rs, abi.rs}`
- `crates/aod-llvm/`: `Cargo.toml`, `src/{lib.rs, ffi.rs, aot.rs, jit.rs}`
- `crates/aodc/`: `Cargo.toml`, `src/{lib.rs, main.rs, checker.rs, sim.rs, codegen.rs}`, `tests/{source_binding.rs, apparatus_and_firewalls.rs, native_backend_parity.rs, native_spill_and_hostile.rs, native_bench_and_cli.rs}`
- `tests/fixtures/v2_bench/unboxed_loop_bench.dec` (fits within `DEFAULT_FUEL = 100_000` for VM parity, uses exact `int`/`t32` + bounded-denominator `ratio` updates so the hot loop stays genuinely inline unboxed!).

---

## 3. Hardened C ABI (`crates/aod-rt/src/abi.rs`, `P0-2`, `P0-3`, `P1-3`)

- `#[repr(C)] AodRtContext { fuel: i64, depth: i32, max_depth: i32, failed: u8, capture_stdout: u8, _pad: [u8; 6], inner: *mut RtState }`
- `#[repr(C)] DecInt { tag: u8, val: i64 }` (`tag=0`: inline signed `i64`; `tag=1`: arena handle `u64` to `aod_core::BigInt`)
- `#[repr(C)] DecRatio { tag: u8, num: i64, den: i64 }` (`tag=0`: inline canonical reduced `num/den`, `den >= 1`; `tag=1`: arena handle `u64` in `num` to `aod_core::Ratio`)
- **Flat C ABI Rule (`P0-1`, `P0-2`):**
  - Internal LLVM functions pass/return SSA `%DecInt` / `%DecRatio` by value.
  - Across `extern "C"`, NEVER pass/return 16/24-byte structs or `i128` by value! Pass flat `(tag, val)` or out-pointers (`*mut DecInt`, `*const DecRatio`, `*mut DecRatio`), and pass `i128` split as `(lo: u64, hi: i64)` if needed (`aod_rt_ratio_reduce_i128`).
  - **Rust 2024 `#[unsafe(no_mangle)]` (`P0-2`):** Every exported `extern "C"` fn uses `#[unsafe(no_mangle)]`, validates `ctx != null && (*ctx).inner != null`, and wraps its entire body in `std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ...))`, converting any panic into `(*ctx).failed = 1` + diagnostic error message (zero unwind across `extern "C"`).
- **Deep-Copy / Copy-on-Write Arena Value Semantics (`P0-3`):**
  - `aod_rt_list_set`, `aod_rt_map_set`, and `aod_rt_struct_set` MUST clone the underlying container, apply the mutation to the clone, insert a NEW arena handle, and return that new handle (stored back into the mutated variable's `alloca`) so `mut b = a; b[0] = 99` leaves `a[0]` untouched (100% matching PR 1 VM!).
- **`aod_rt_sim_run` (`P1-3`):**
  - Decrements `ctx.fuel` per simulation tick and checks `ctx.failed != 0` immediately after every procedural observer callback `cb(ctx, snap_h)` to halt cleanly on observer failure or fuel exhaustion.
- `runtime_symbol_table() -> &'static [(&'static str, usize)]` exports all `aod_rt_*` functions cleanly to ORC LLJIT.

---

## 4. Unboxed-First LLVM 18 Lowering (`codegen.rs`) & LLVM 18 FFI (`crates/aod-llvm`) (`P0-1`, `P0-3`, `P1-1`)

### 4.1 Exact Arithmetic & UB Guards (`P0-3`)
- `bool -> i1`, `t1/q4 -> i8` (with `@llvm.ctpop.i8`), `t32 -> i64` (`±926510094425920`, `@llvm.sadd/ssub/smul.with.overflow.i64` + range guard).
- **`pack(t32)` & `unpack(DecInt)` (`P0-3`):**
  - `pack` uses Euclidean mod 3 (NOT raw `srem`) matching `aod_core::T32::pack()`.
  - Check `icmp slt i64 %packed, 0` (when bit 63 is set, e.g. `t32(-T32::LIMIT)` -> `0xAAAA..._u64 > i64::MAX`): spill via `aod_rt_int_from_u64(ctx, packed, out)` to `tag = 1` `BigInt`! Never put `> i64::MAX` in inline signed `%DecInt.val`.
  - `unpack` rejects `tag == 0 && val < 0` (`cold` fail) and dispatches `tag == 1` to `aod_rt_t32_unpack_bigint` (accepting `[2^63, 2^64-1]`, rejecting `< 0`, `>= 2^64`, or any `0b11` lane).
- **`DecInt` (`{ i8, i64 }`) UB Guards (`P0-3`):**
  - Unary `-a`: check `a == i64::MIN` -> cold spill `aod_rt_int_neg`.
  - `+`/`-`/`*`: `@llvm.sadd/ssub/smul.with.overflow.i64` -> cold spill `aod_rt_int_*` on overflow or `tag != 0`.
  - Floor `//` & `%`: check `b == 0` -> fail `ZeroDivision`, AND check `(a == i64::MIN && b == -1)` -> cold spill **BEFORE** `sdiv`/`srem`! Then apply exact Python floor adjustment.
  - Exact `a / b -> ratio`: normalize in `i128`, check reduced `num ∈ [i64::MIN, i64::MAX]` & `den ∈ [1, i64::MAX]`, cold spill on overflow.
  - Bitwise `&`/`|`/`^`/`<<`/`>>`: reject negative operands (`DomainError`); `<<` spills to `aod_rt_int_shl` unless `b <= 62 && a <= (i64::MAX >> b)`; `>>` returns `0` when `a >= 0 && b >= 64` (never emitting LLVM shift-count >= 64 poison!).
- **`DecRatio` (`{ i8, i64, i64 }`) Without `__udivti3` Link Traps (`P0-1`):**
  - Inline `+`/`-`/`*`/`/` compute numerator/denominator in `i128` using `@llvm.sadd/ssub.with.overflow.i128`.
  - For GCD/reduction: use binary Stein's GCD (`@llvm.cttz.i128` + `lshr` + `sub` — ZERO `urem i128`!) when `gcd == 1` skip division cleanly, and ONLY when `gcd > 1` (or on `i128`/`i64` overflow) divide or call flat `aod_rt_ratio_reduce_i128(ctx, num_lo, num_hi, den_lo, den_hi, out)` — **guaranteeing zero unresolved `__udivti3`/`__umodti3` symbols in ORC LLJIT!**
  - Comparisons `== != < <= > >=` use exact `i128` cross-multiply `sext(a.num)*sext(b.den)` vs `sext(b.num)*sext(a.den)` -> single `icmp i128`!
- Entry-block `alloca`s (`mem2reg`), unboxed `i64` `for i in a..b:` loops, inline `%ctx` fuel decrement at fn entry & loop backedges, inline depth check/inc/dec, short-circuit `and`/`or`/ternary PHI blocks, enum `switch`, unreachable functions lowered & verified.

### 4.2 LLVM 18 C API Ownership & Linking (`crates/aod-llvm`, `P0-1`, `P1-1`)
- **`ffi.rs`:** `dlopen` (`DEC_LLVM_LIBRARY` -> `/usr/lib/llvm-18/lib/libLLVM.so.18.1` -> fallback), cached in `OnceLock`; init X86 target once.
- **Exact LLVM 18 Ownership (`P0-1`):**
  - `LLVMParseIRInContext` ALWAYS frees `MemBuf` (both Ok & Err) — NEVER call `LLVMDisposeMemoryBuffer`.
  - `LLVMGetErrorMessage` consumes `LLVMErrorRef` — free via `LLVMDisposeErrorMessage` (NEVER also `LLVMConsumeError`).
  - `LLVMVerifyModule` passes `LLVMReturnStatusAction = 2` (NOT `0`) and frees non-null error msg via `LLVMDisposeMessage`.
  - `LLVMOrcCreateNewThreadSafeModule` consumes `M` (do NOT `LLVMDisposeModule`, DO `LLVMOrcDisposeThreadSafeContext`); `LLVMOrcAbsoluteSymbols` consumes interned symbol refs; `LLVMOrcCSymbolMapPair` uses exact 24-byte `#[repr(C)]` layout with `generic_flags = 1` (`Exported`); `LLVMOrcJITDylibDefine` / `LLVMOrcLLJITAddLLVMIRModule` consume MU/TSM on success (dispose ONLY on error).
  - Attach `LLVMOrcCreateDynamicLibrarySearchGeneratorForProcess` (with `LLVMOrcLLJITGetGlobalPrefix`) to main `JITDylib` so `memcpy`/`memset`/`memmove` resolve cleanly in ORC LLJIT!
- **`aot.rs` (`P1-1`):** Emit PIC `.o` (`LLVMRelocPIC = 2`, `LLVMObjectFile = 1`), search `libaod_rt.a` in order: `AOD_RT_LIB` -> `exe_dir/libaod_rt.a` -> `exe_dir/../libaod_rt.a` -> workspace `target/{debug|release}/libaod_rt.a`, link via `clang` (fallback `cc`) `<obj.o> <libaod_rt.a> -lpthread -ldl -lm -o <exe>`, delete temp `.o`.

---

## 5. CLI Extensions & PR 1 Polish

- `aodc emit-llvm [--compat v1|v2] [-O 0|1|2|3] <source.dec> [-o <out.ll>]` (refuses `out == source`)
- `aodc build [--compat v1|v2] [-O 0|1|2|3] <source.dec> -o <exe>` (standalone ELF accepting `--fuel N`, `--max-depth N`, refuses `out == source`)
- `aodc jit [--compat v1|v2] [-O 0|1|2|3] [--fuel N] [--max-depth N] <source.dec>`
- `aodc bench [--compat v1|v2] [--iters N] [--fuel N] [--max-depth N] <source.dec>` (5-mode stdout parity check + compile & median exec timings in us + integer `u128` speedup ratios, zero floats)
- PR 1 polish: fix `crates/aodc/src/checker.rs:3051 & 3088` so positional `Struct(...)` and `Enum::Var(...)` reject stripping `ReadOnly<T>` identically to brace `Struct { ... }`.

---

## 6. Acceptance Criteria (`AC-01`..`AC-17`, Including Reviewer Amendments)
- `AC-01`: 4-crate `std`-only offline workspace builds & tests with 0 warnings, acyclic DAG, 0 crates.io deps.
- `AC-02`: 4-crate static audit: 0 `f32`/`f64`, `#![forbid(unsafe_code)]` on `aod-core` & `aodc`, `unsafe` ONLY in `aod-rt/src/abi.rs` and `aod-llvm/src/{ffi.rs,jit.rs,aot.rs}`.
- `AC-03`: PR 1 polish: positional `Struct(ro)` and `Enum::Var(ro)` reject stripping `ReadOnly<T>`.
- `AC-04`: LLVM 18 loader (`DEC_LLVM_LIBRARY` + default), `LLVMVerifyModule` (`action=2`), `LLVMRunPasses(O0..O3)`, clean `Err` on malformed IR without aborting process.
- `AC-05`: `aodc emit-llvm -O2` emits verified unboxed IR for `tests/fixtures/v2_bench/unboxed_loop_bench.dec` containing `@llvm.sadd.with.overflow.i64` / inline math and ZERO hot-path `aod_rt_int_*`/`aod_rt_ratio_*` calls in main loop; refuses `-o <source>`.
- `AC-06`: 10/10 frozen v1 examples byte-identical stdout across VM, AOT `-O0`/`-O2`, ORC JIT `-O0`/`-O2`.
- `AC-07`: 3/3 v2 smoke fixtures (`opposing_336_chamber`, `m3_wavelet_recipient`, `qext_post_freeze`) byte-identical across VM, AOT `-O0`/`-O2`, ORC JIT `-O0`/`-O2`.
- `AC-08`: Full Dec v2 core language (including deep-copy aliasing test `mut b = a; b[0] = 99` leaving `a[0]` unchanged for list/map/struct!) byte-identical across VM, AOT `-O0`/`-O2`, ORC JIT `-O0`/`-O2`.
- `AC-09`: Exact `int` overflow spill (`i64::MAX+1`, `i64::MIN-1`, `i64::MAX*i64::MAX`, `-i64::MIN`, `i64::MIN//-1`, `i64::MIN%-1`, spill-and-unbox round-trip, 256-bit, 4-quadrant floor div/rem, bitwise `<< 63`/`>> 64` + negative rejection, div-by-0) matching VM in AOT & ORC JIT.
- `AC-10`: Exact `t1`/`t32`/`q4`/`ratio` overflow & domain parity (`±T32::LIMIT`, `pack(t32(-T32::LIMIT))` bit-63 spill & `unpack` round-trip, `T32::LIMIT+1`, `unpack(3)`, huge `Ratio` spill, `int(7/2r)` fail vs `int(6/2r)==3`, `1/0r` fail) matching VM in AOT & ORC JIT.
- `AC-11`: Standalone AOT ELF isolation (`ldd` has ZERO `libLLVM`, ZERO `libpython`, ZERO `libgmp`), refuses `out == source`, native `--fuel`/`--max-depth` flags.
- `AC-12`: Coarsened native fuel (`--fuel`), `--max-depth`, `assert false`, div-by-0, `t32` overflow, `unpack(3)`, and failing procedural observer callback in `simulate` preserve prior `emit` stdout + `aodc: ...` stderr + exit 1 across VM, AOT, and ORC JIT.
- `AC-13`: Arena handle safety (reject invalid/0/stale handles without UB/panic) & deterministic `max_arena_entries` cap failure.
- `AC-14`: All compile-time firewalls enforced BEFORE emitting IR or creating outputs on `emit-llvm`/`build`/`jit`/`bench`.
- `AC-15`: `aodc bench tests/fixtures/v2_bench/unboxed_loop_bench.dec` verifies 5-mode stdout parity, zero-float timing/ratio calculation, and >= 5x steady-state speedup of AOT `-O2` & ORC JIT `-O2` over Exact VM.
- `AC-16`: 20 sequential `OrcJitSession` compile/run/drop cycles in one process (mixing success & failure) with zero crashes or symbol collisions.
- `AC-17`: All PR 1 tests remain green (`v2_smoke` untouched at `3 passed; 0 failed; 3 total`); `git -C /home/antigravity/rpoyau/aod status --short` clean; zero hardcoded outcomes.
