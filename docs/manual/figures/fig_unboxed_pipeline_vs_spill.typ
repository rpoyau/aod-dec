// Pure-vector schematic of Trit-Boxed LLVM 18 fast path vs exact cold spill & hardware learner feedback.
#import "/bindings/aod/aod_schema.typ": *

#let fig-unboxed-pipeline-vs-spill() = {
  aod-flowchart(
    (
      (id: "op", x: 7.25cm, y: 0.65cm, w: 6.6cm, h: 0.95cm, body: [*Trit-Boxed Arithmetic Op (`int` / `ratio`)* \ 1-GPR 32-trit `i64` or 2-GPR `%DecRatio { i64, i64 }`]),
      (id: "fast", x: 3.6cm, y: 2.35cm, w: 5.8cm, h: 1.1cm, body: [*Inline SWAR Hot Path (`< 0xC000...`)* \ `@__aod_swar_unpack` / `@.pack4_lut` / `@llvm.cttz.i64` GCD]),
      (id: "spill", x: 10.9cm, y: 2.35cm, w: 5.8cm, h: 1.1cm, body: [*Cold `BigTrit` Spill (`0xD000... | h`)* \ `@aod_rt_int_*` / `@aod_rt_ratio_*` base-$3^(32)$ limbs]),
      (id: "guard", x: 7.25cm, y: 4.15cm, w: 8.6cm, h: 1.05cm, body: [*Exact Semantic Guard & Zero-Float Paired Bootstrap* \ Compares `(emits, failed, error)` + honest compile-inclusive net savings]),
      (id: "prof", x: 7.25cm, y: 5.85cm, w: 8.8cm, h: 1.0cm, body: [*Rollback-Safe Atomic `RetainedProfile`* \ `COMPILER_EXECUTION_POLICY_NOT_PHYSICAL_AOD_LAW`]),
    ),
    (
      (from: "op", to: "fast"),
      (from: "op", to: "spill"),
      (from: "fast", to: "guard"),
      (from: "spill", to: "guard"),
      (from: "guard", to: "prof"),
    ),
    width: 14.5cm,
    height: 6.6cm,
  )
}
