#import "/bindings/aod/aod_schema.typ": *
#import "/docs/manual/figures/fig_architecture_firewalls.typ": fig-architecture-firewalls

= Quickstart and System Architecture <sec:quickstart>

#aod-paragraph("Setup.")
The `rpoyau/aod-dec` v2 workspace provides a `std`-only four-crate Rust compiler and runtime for the Dec
programming language, bound read-only to Alpha-Omega Dynamics (AOD) canonical release `v41.02`
(#mainsection("main:sec:introduction"), #mainsection("main:sec:afc-basis"), #aod-cite("aod-main-v41, afc-foundation")).
Its design enforces four non-negotiable engineering boundaries: zero external `crates.io` dependencies,
zero IEEE-754 floating-point types across all four crates, `#![forbid(unsafe_code)]` on `aod-core` and
`aodc`, and strict confinement of `unsafe` blocks to the five C ABI, LLVM 18 FFI, and hardware-probe
interface files.

#figure(
  kind: table,
  caption: [Four-crate acyclic workspace layout and safety boundaries of `rpoyau/aod-dec` v2.],
  table(
    columns: (auto, auto, 1fr),
    align: (left, left, left),
    toprule,
    [*Crate*], [*Safety Policy*], [*Declared Role*],
    midrule,
    [`aod-core`], [`#![forbid(unsafe_code)]`], [Exact `BigInt`, canonical `Ratio`, quarantined `QExtRatio`, checked `T1`/`T32`/`Q4`/`Addr8`, FIPS 180-4 SHA-256, strict float-free JSON, M3 wavelets, Manual ledger primitives.],
    [`aod-rt`], [`unsafe` only in `abi.rs`], [Shared deterministic sandbox chamber, safe 1-based arena with entry cap, copy-on-write container helpers, cold-spill exact math, and panic-safe C ABI (`libaod_rt.a`).],
    [`aod-llvm`], [`unsafe` only in `ffi`/`aot`/`jit`/`hw_probe`], [`dlopen`-loaded LLVM 18 verifier and optimizer, PIC AOT ELF linker, ORC LLJIT session, CPU discovery, 5 live JIT micro-probes, and zero-float paired bootstrap learner.],
    [`aodc`], [`#![forbid(unsafe_code)]`], [Dec v1/v2 frontend, 5 compile-time firewalls, hermetic `DecIR` verifier, Exact VM, parameterized LLVM 18 lowering, `manual-verify`, and CLI.],
    bottomrule,
  ),
)<tab:crate-boundaries>

#aod-paragraph("Derivation.")
Compilation flows strictly top-down as shown in #figref("fig:arch-firewalls") and summarized in
#tabref("tab:crate-boundaries"). Every Dec v1 or Dec v2 source program passes exhaustive lexing, parsing,
module resolution (including `use stdlib::aod::*` and built-in `aod::*`), and all five compile-time
firewalls before reaching either the Exact Reference VM or LLVM 18 IR emission. Because `aod-core` is the
single mathematical oracle for both the VM and the cold runtime spills of `aod-rt`, native AOT ELF binaries
and ORC LLJIT sessions maintain byte-for-byte output parity with the Exact Reference VM across every
admissible execution path.

#figure(
  fig-architecture-firewalls(),
  caption: [Four-crate compiler pipeline from Dec source and native standard library through all five compile-time firewalls to Exact VM, standalone AOT ELF, and ORC LLJIT execution.],
)<fig:arch-firewalls>

The inline program below verifies exact arbitrary-precision arithmetic and canonical rational reduction
across Exact VM, AOT `-O2`, and ORC LLJIT `-O2` during `aodc manual-verify`:

```dec
use aod::core::ratio

Dec main() -> int:
    let big = 9223372036854775807 + 1
    let r = ratio(6, 8) + 1/4r
    emit big
    emit r
    return 0
```

```dec-expect
9223372036854775808
1/1
0
```

#aod-paragraph("Falsification.")
Any introduction of an external `crates.io` dependency, an IEEE-754 float token, an `unsafe` block outside
the five allowlisted interface files, or a divergence between Exact VM, AOT, and ORC LLJIT output causes
the workspace static audit and parity suites to fail immediately (#mainappendix("main:app:glossary")).
