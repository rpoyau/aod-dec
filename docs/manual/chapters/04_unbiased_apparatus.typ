#import "/bindings/aod/aod_schema.typ": *
#import "/docs/manual/figures/fig_sandbox_chamber_schematic.typ": fig-sandbox-chamber-schematic

= Unbiased Chamber Sandbox Apparatus <sec:apparatus>

#aod-paragraph("Setup.")
Dec provides first-class declarative apparatus blocks (`chamber`, `prepare`, `observe`, `simulate`)
backed by a shared deterministic sandbox engine in `crates/aod-rt/src/sim.rs`. Both the Exact Reference VM
and compiled native binaries invoke the exact same sandbox stepper so tick transitions, carrier shedding,
hinge saturation (#mainsection("main:sec:afc-stokes-seed"), #mainequation("main:eq:hinge-saturation")),
pending overhang (#mainequation("main:eq:burden-overhang")), closure residual
(#mainequation("main:eq:closure-residual")), and final state hashing (#aod-cite("nist-fips-180-4")) stay
byte-identical across all five execution modes.

#aod-noteblock("Model Provenance")[
  Every `SimResult` emitted by the sandbox engine carries the explicit status string
  `DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED`. The chamber apparatus is a declared computational testbed
  for observer isolation and deterministic replay (#mainsection("main:sec:awareness-learning")); it makes zero
  unproven physical claims and contains zero hardcoded physical outcome targets.
]

#aod-paragraph("Derivation.")
In `06_unbiased_chamber_sandbox.dec`, two opposing `3:3:6` supports (`proton_support()`) are injected inside
`box([0..64, 0..64, 0..64])` with open walls and stepped across `12` ticks under three observer configurations:
zero observers (`r0`), one declarative sensor (`r1`), and combined declarative plus procedural read-only
observers (`r2`). As illustrated in #figref("fig:sandbox-chamber"), the read-only firewall ensures all
three runs produce identical SHA-256 final state hashes (`0` bytes of observer back-reaction).

#figure(
  fig-sandbox-chamber-schematic(),
  caption: [Opposing `3:3:6` sandbox chamber geometry and one-way `ReadOnly<ChamberView>` observer wall, populated live from `/docs/manual/data/sandbox_chamber_summary.json`.],
)<fig:sandbox-chamber>

The inline apparatus check below confirms deterministic provenance tagging and zero observer back-reaction
across Exact VM, AOT `-O2`, and ORC LLJIT `-O2`:

```dec
chamber MiniBox:
    space = box([0..32, 0..32, 0..32])
    walls = open

prepare SingleCarrier in MiniBox:
    inject 3:3:6 at (8, 16, 16) dir +X orient +1t shells 1

observe PassiveEye:
    tracer count_eye(view):
        let _k = view.carrier_count

dec main() -> int:
    let a = simulate SingleCarrier over 4 ticks
    let b = simulate SingleCarrier over 4 ticks with PassiveEye
    assert a.model_status == "DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED"
    assert a.final_state_hash == b.final_state_hash
    emit a.model_status
    emit a.ticks
    emit a.carrier_count
    0
```

```dec-expect
DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED
4
1
0
```

#aod-paragraph("Falsification.")
Any observer side effect that alters carrier positions, counts, shedding events, or the final SHA-256
digest immediately breaks `r0.final_state_hash == r1.final_state_hash == r2.final_state_hash` in
`06_unbiased_chamber_sandbox.dec` (verified live by `aodc manual-data --check` and `aodc manual-verify` Pass 6)
as well as the four-observer concentric shell invariance check in `11_apparatus_opposing_sensors.dec` (built
from pure native helpers in `stdlib::aod::apparatus` and verified byte-identical across sequential and
`--parallel-observers` execution):

// @dec-file: manual_examples/06_unbiased_chamber_sandbox.dec
// @dec-file: manual_examples/11_apparatus_opposing_sensors.dec
