# PR 2 Independent Verification Bundle (R02 — Final) — `rpoyau/aod-dec` (`v2/native-reference-engine`)

**Overall Verdict:** **PASS — 17/17 Acceptance Criteria Verified (`AC-01`..`AC-17`), 0 FAIL**

- `RUSTFLAGS="-D warnings" cargo test --workspace --offline`: **45/45 PASS**, 0 warnings.
- `RUSTFLAGS="-D warnings" cargo test --workspace --release --offline`: **45/45 PASS**, 0 warnings.
- `RUSTFLAGS="-D warnings" cargo build --workspace --release --offline`: **PASS**.
- 4-crate static scan across all 35 `.rs` files in `crates/*/src/**/*.rs`: `f32/f64=0`, `float_lits=0`, `disallowed_unsafe=0`, `#![forbid(unsafe_code)]` on `aod-core` & `aodc`, `unsafe` strictly in `aod-rt/src/abi.rs` (128/128 `#[unsafe(no_mangle)]` + `catch_unwind`) and `aod-llvm/src/{ffi.rs,jit.rs,aot.rs}`, 0 external crates in `Cargo.lock`.
- `AC-03`: 12/12 positional `Struct(ro)` / `Enum::Var(ro)` probes rejected with `ObserverMutationForbidden`.
- `AC-04` & `AC-05`: LLVM 18 `dlopen`, `LLVMVerifyModule(action=2)`, `LLVMRunPasses(O0..O3)`, unboxed `-O2` IR (`@llvm.sadd.with.overflow.i64`, `@llvm.sadd.with.overflow.i128`, `@llvm.cttz.i128`, ZERO `__udivti3`/`__umodti3`, ZERO hot-path `aod_rt_int_*`/`aod_rt_ratio_*` calls in loop body).
- `AC-06` & `AC-07`: **65/65** exact SHA-256 stdout parity across 10 frozen v1 examples + 3 v2 smoke fixtures × 5 modes (VM, AOT `-O0`/`-O2`, ORC JIT `-O0`/`-O2`).
- `AC-08`: Deep-copy COW aliasing on list/map/struct, full `struct`/`enum`/`match`/pattern-dispatch, AND unannotated + cross-module imported aggregate `const`s verified 100% identical across VM, AOT `-O2`, and ORC JIT `-O2`.
- `AC-09` & `AC-10`: Exact `i64::MIN`/`i64::MAX` spills, 4-quadrant floor div/rem, `1 << 63`/`1 << 100`/`100 >> 64`/`100 >> 200`, negative bitwise error, `pack(t32(-926510094425920))` bit-63 spill + `unpack` round-trip, `T32::LIMIT+1`, `unpack(3)`, huge `Ratio` spill, `int(7/2r)` vs `int(6/2r)`, `1/0r` all verified 3-mode identical.
- `AC-11` & `AC-12`: Standalone AOT ELF `ldd` links ONLY `linux-vdso`, `libm`, `libgcc_s`, `libc`, `ld-linux-x86-64` (ZERO `libLLVM`/`libpython`/`libgmp`); `--fuel`, `--max-depth`, `assert false`, `1 // 0`, failing procedural observer in `simulate` preserve prior `emit` stdout + `aodc:` stderr + exit 1 across VM/AOT/JIT.
- `AC-13`, `AC-14`, `AC-16`, `AC-17`: Arena handle safety & `max_arena_entries` cap PASS; all 12 firewall pre-codegen probes rejected without creating output files; 20 sequential `OrcJitSession` compile/run/drop cycles PASS; `v2_smoke` untouched (`3 passed; 0 failed; 3 total`); `/home/antigravity/rpoyau/aod` completely clean (`""`).
- `AC-15`: `target/release/aodc bench --iters 5 tests/fixtures/v2_bench/unboxed_loop_bench.dec`: **5/5 byte-identical**, `vm_median_us = 17263 us`, **`aot_o2 = 3021 us (5.71x)`**, **`jit_o2 = 331 us (52.15x)`**.
