// Pure-vector schematic of the 4-crate aod-dec v2 architecture and 5 compile-time firewalls.
#import "/bindings/aod/aod_schema.typ": *

#let fig-architecture-firewalls() = {
  aod-flowchart(
    (
      (id: "src", x: 7.25cm, y: 0.7cm, w: 7.4cm, h: 1.0cm, body: [*Dec v1/v2 Source + `stdlib/aod/*.dec`* \ Zero floats, explicit types, `use stdlib::aod::*`]),
      (id: "chk", x: 7.25cm, y: 2.3cm, w: 8.6cm, h: 1.1cm, body: [*`aodc` Frontend & 5 Compile-Time Firewalls* \ `ReadOnlyObserver` | `Q_ext` sink | `_yro` | zero floats | hermetic `DecIR`]),
      (id: "vm", x: 2.5cm, y: 4.3cm, w: 4.4cm, h: 1.1cm, body: [*Exact Reference VM* \ `aod-core` `BigInt`/`Ratio`/`T32`]),
      (id: "ir", x: 7.25cm, y: 4.3cm, w: 4.4cm, h: 1.1cm, body: [*LLVM 18 IR Lowering* \ Unboxed fast path + `CodegenPlan`]),
      (id: "rt", x: 12.0cm, y: 4.3cm, w: 4.4cm, h: 1.1cm, body: [*`aod-rt` (`libaod_rt.a`)* \ Safe arena + cold exact spills]),
      (id: "aot", x: 4.6cm, y: 6.3cm, w: 4.6cm, h: 1.05cm, body: [*Standalone AOT ELF* \ Zero `libLLVM`/`libpython`/`libgmp`]),
      (id: "jit", x: 9.9cm, y: 6.3cm, w: 4.8cm, h: 1.05cm, body: [*ORC LLJIT + HW Learner* \ 5 live probes + rollback-safe profiles]),
    ),
    (
      (from: "src", to: "chk"),
      (from: "chk", to: "vm"),
      (from: "chk", to: "ir"),
      (from: "ir", to: "rt"),
      (from: "ir", to: "aot"),
      (from: "ir", to: "jit"),
      (from: "rt", to: "aot"),
      (from: "rt", to: "jit"),
    ),
    width: 14.5cm,
    height: 7.2cm,
  )
}
