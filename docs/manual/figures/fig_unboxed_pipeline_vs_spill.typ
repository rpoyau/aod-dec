// Pure-vector schematic of unboxed LLVM 18 fast path vs exact cold spill & hardware learner feedback.
#import "/bindings/aod/aod_schema.typ": *

#let fig-unboxed-pipeline-vs-spill() = {
  aod-flowchart(
    (
      (id: "op", x: 7.25cm, y: 0.65cm, w: 6.4cm, h: 0.95cm, body: [*Exact Arithmetic Op (`int` / `t32` / `ratio`)* \ Tagged value `{ i8 tag, i64 val }` or `{ i8, i64, i64 }`]),
      (id: "fast", x: 3.6cm, y: 2.35cm, w: 5.8cm, h: 1.1cm, body: [*Inline Unboxed Hot Path (`tag == 0`)* \ `@llvm.sadd.with.overflow.i64` / `@llvm.cttz.i128` Stein GCD]),
      (id: "spill", x: 10.9cm, y: 2.35cm, w: 5.8cm, h: 1.1cm, body: [*Cold Exact Spill Path (`tag == 1` or overflow)* \ `@aod_rt_int_*` / `@aod_rt_ratio_*` arbitrary precision]),
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
