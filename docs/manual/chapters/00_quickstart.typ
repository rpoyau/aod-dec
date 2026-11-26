#import "/bindings/aod/aod_schema.typ": *
#import "/docs/manual/figures/fig_architecture_firewalls.typ": fig-architecture-firewalls

= Quickstart and System Architecture <sec:quickstart>

#aod-paragraph("Setup.")
The `rpoyau/aod-dec` workspace provides a `std`-only four-crate Rust compiler, runtime, and fixed-point
self-hosting toolchain for the clean-break Dec programming language, bound read-only to Alpha-Omega Dynamics
(AOD) canonical release `v41.02` (#mainsection("main:sec:introduction"), #mainsection("main:sec:afc-basis"),
#aod-cite("aod-main-v41, afc-foundation")). Its design enforces four non-negotiable engineering boundaries:
zero external `crates.io` dependencies, zero IEEE-754 floating-point types across all four crates,
`#![forbid(unsafe_code)]` on `aod-core` and `aodc`, and strict confinement of `unsafe` blocks to the five C ABI,
LLVM 18 FFI, and hardware-probe interface files.

#figure(
  kind: table,
  caption: [Four-crate acyclic workspace layout and safety boundaries of `rpoyau/aod-dec`.],
  table(
    columns: (auto, auto, 1fr),
    align: (left, left, left),
    toprule,
    [*Crate*], [*Safety Policy*], [*Declared Role*],
    midrule,
    [`aod-core`], [`#![forbid(unsafe_code)]`], [2-bit balanced-ternary `BigTrit`/`BigInt`, Division-Free Ternary Stein GCD `Ratio`, compact 1-word `pack_ratio`, quarantined `QExtRatio`, checked `T1`/`T32`/`Q4`/`Addr8`, FIPS 180-4 SHA-256, strict float-free JSON, Plan 9 `cleanname` and Copy-on-Write Ternary Search Tree `Namespace<P>` (`UnitId`), M3 wavelets, Manual ledger primitives, S2/S4 exact operations, and `aod.s4.rust.report.v1` builder.],
    [`aod-rt`], [`unsafe` only in `abi.rs`], [Shared deterministic sandbox chamber with optional parallel read-only observer execution (`aod_rt_sim_run_ex`), Trit-Boxed `0xD000...` spill arena, default-empty Plan 9 runtime VFS (`RtNamespace`), default-on Linux `pivot_root` + `seccomp-bpf` containerizer (`SandboxedJitWorker`), indirect `mount dyn` capability dispatch, and panic-safe C ABI (`libaod_rt.a`).],
    [`aod-llvm`], [`unsafe` only in `ffi`/`aot`/`jit`/`hw_probe`], [`dlopen`-loaded LLVM 18 verifier and optimizer, content-addressed `JitCodeCache` (`aod.jit.obj.v1`), PIC AOT ELF linker, container-sandboxed ORC LLJIT session, CPU discovery, 6 live JIT micro-probes, and zero-float paired bootstrap learner (`CodegenPlan`, `PROFILE_VERSION = 2`).],
    [`aodc`], [`#![forbid(unsafe_code)]`], [Clean-break Dec frontend, Plan 9 / Inferno Context Namespace & cryptographic `UnitId` package system (`--namespace`, `mount dyn`, `DynamicJitHost`), compile-time firewalls, hermetic `DecIR v3`/`v4` verifier, Exact VM, Trit-Boxed LLVM 18 lowering, `bootstrap-verify`, zero-LLVM `manual-data`, `manual-verify`, `verify-s2 --s4-report`, `eval-op`, `std`-only `audit-pdf`, and CLI.],
    bottomrule,
  ),
)<tab:crate-boundaries>

#aod-paragraph("Derivation.")
Compilation flows strictly top-down as shown in #figref("fig:arch-firewalls") and summarized in
#tabref("tab:crate-boundaries"). Every Dec source program passes exhaustive lexing, indentation-structured
parsing, Plan 9 Context Namespace resolution (mapping clean `/path` imports, `bind before|after|replace` union
overlays, and `use stdlib::aod::*` onto cryptographic `UnitId = SHA256(Provider.id, subpath)` identities with
zero ambient filesystem reads inside `modules.rs`), static shape checking, and all compile-time firewalls before
reaching either the Exact Reference VM or LLVM 18 IR emission. On Linux, both `aodc jit` and standalone AOT ELF
binaries execute inside a default-empty Plan 9 namespace isolated by unprivileged kernel namespaces (`pivot_root`
onto an empty read-only `tmpfs` `/` plus `seccomp-bpf`), exposing only explicitly declared `--mount` capabilities.
Because `aod-core` is the single mathematical oracle for both the VM and the cold `BigTrit` spills of
`aod-rt`, native AOT ELF binaries and ORC LLJIT sessions maintain byte-for-byte output parity with the Exact
Reference VM across every admissible execution path.

#figure(
  fig-architecture-firewalls(),
  caption: [Four-crate compiler pipeline from Dec source and native standard library through all five compile-time firewalls to Exact VM, standalone AOT ELF, and ORC LLJIT execution.],
)<fig:arch-firewalls>

The inline program below verifies Trit-Boxed inline-to-`BigTrit` spill arithmetic and canonical 2-word rational
reduction across Exact VM, AOT `-O2`, and ORC LLJIT `-O2` during `aodc manual-verify`:

```dec
use aod::core:
    ratio

dec main() -> int:
    let big = 926510094425920 + 1
    let r = ratio(6, 8) + 1/4r
    emit big
    emit r
    0
```

```dec-expect
926510094425921
1/1
0
```

#aod-paragraph("Falsification.")
Any introduction of an external `crates.io` dependency, an IEEE-754 float token, an `unsafe` block outside
the five allowlisted interface files, or a divergence between Exact VM, AOT, and ORC LLJIT output causes
the workspace static audit and parity suites to fail immediately (#mainappendix("main:app:glossary")).
