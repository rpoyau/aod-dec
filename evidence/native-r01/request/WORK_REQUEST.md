# Execution scope and required output

Issued 13 November 2026. Request R01. Status: ready for Codex execution; native
compiler/JIT/learning implementation has not been performed by this packaging task.

## Bound starting point

The repaired source in source/ comes byte-for-byte from the `aod-dec/` subtree of
AOD_TERNARY_DEC_REPAIR_R01_2026-11-13.zip, SHA-256
289260c911205ea300f6b5406a3019548f27b0d8f7ac9e8fcb892f7661b99e48.
Its existing verification records report 57 Python tests in normal/optimized modes
and nine Rust tests in debug/optimized modes. Establish fresh baseline evidence;
do not treat these historical counts as a fixed future count.

The full M0–M6 roadmap is plan/ROADMAP.md. The first executable work packet is
M0–M1, but the requested compiler/JIT/hardware-learning outcome extends through
M5. M6 is the separately assessable self-hosting continuation. Begin with Linux
x86-64; add actual Linux AArch64 execution before claiming that target supported.

## M0–M1 first work packet

1. Verify input hashes. Copy source/ into work/candidate/ without overwriting an
   existing candidate. Read source/runtime/dec_language.py, dec_core.py, tests and
   language/model documents; reproduce baseline and record tool availability.
2. Pin the semantic contract, effect/error rules, native ABI and exact numeric
   storage/ownership. Preserve all current semantics; record any proposed change
   separately and keep the baseline version available.
3. Use LLVM as the default shared backend. Pin a stable released version and native
   runtime dependencies. First prove object emission, runtime symbol linkage and
   a minimal ORC/LLJIT function with this toolchain. A switch to Cranelift is allowed
   only as the documented roadmap fallback supported by a concrete integration
   finding; record tradeoffs, not a hidden scope change.
4. Implement scalar typed IR verification/lowering, branches, calls, recursion,
   checked arithmetic, explicit conversions, source-located failures, emit and
   assert. Use arbitrary precision for unbounded int/ratio: the bounded Rust demo
   Ratio does not satisfy the source-language contract.
5. Produce and run native binaries for the scalar/weight examples and a generated
   input corpus. The compiled program must run with Python absent. Prove this with
   an isolated environment/process/dependency inspection; removing Python from
   PATH alone does not exclude an absolute interpreter invocation.
6. Add exact, boundary, randomized and hostile-mutation cases. Compare results,
   ordered effects and failure kind/location against the reference and independent
   expected values. Preserve intermediate t32 overflow checks under optimization.
7. Produce an M0–M1 checkpoint with commands, outputs, toolchain binding, disassembly
   or equivalent generated-code evidence, and criterion results. Continue dependent
   milestones; do not call the full request complete at this checkpoint.

## Continuations

M2: native immutable AOD runtime. All five examples and negative witness cases
must execute without Python. Retain parent records and canonical trace/certificate
serialization. Extend structs/arrays/loops/modules in separate declared grammar
increments when required. A general cell-complex fold or RCD law cannot be supplied
merely by adding those language features.

M3: JIT through the same verified lowering and ABI. Test first-call compilation,
generated-code invocation, cache invalidation, safe code lifetime, guarded entry
fallback and exact behavior. Start with function-entry specialization guards;
mid-function deoptimization requires separately implemented state reconstruction.

M4: host discovery and profiling. Establish a fixed benchmark protocol before
results: workloads, warmup, sample counts, paired/randomized run order, timing
source, uncertainty method, practical improvement threshold and regression bound.
Separate compilation/profiling/loading costs from steady-state and total costs.
Missing performance-counter permission means unavailable data, not success.

M5: persistent adaptation over prevalidated variants. Retained measurements at k
may change selection at k+1. Implement generic, passive, frozen-profile, active,
stale/shuffled-profile and retained-state-removal controls. Demonstrate policy
influence separately from actual performance improvement. Semantic equality is
mandatory; a zero speedup/regression remains a reported result and falls back.

M6: after necessary language features exist, implement frontend/lowering in Dec
and demonstrate seed→stage1→stage2→stage3 with stage2/stage3 comparison. State any
remaining external backend/runtime dependencies. Self-hosting does not block JIT
or hardware learning and is never inferred from an ELF/banner/file-open result.

## Deliverable contract

Supply:
- Candidate source and a detached patch from bundled source/; no remote changes.
- Updated language/IR/ABI documents; compiler/runtime/target feature inventory.
- Dependency lock and toolchain/host identifiers, install/build commands and licenses.
- Tests and test logs, differential and independent checks, native/JIT execution
  evidence, and benchmark raw observations with uncertainty and full overheads.
- ACCEPTANCE_RESULTS.json with every supplied criterion, status PASS/FAIL/BLOCKED/
  NOT_RUN, exact evidence paths and explanation. No schema-only claim of completion.
- Failure/censored runs and unsupported targets; do not remove unsuccessful cases.
- Replay verification of the patch on pristine source, checksums and final report.
- HANDOFF.md if work continues: source digest/revision, completed milestones,
  failing commands, modified files, environment gaps and next bounded action.

Use acceptance/criteria.json as the checklist, with the roadmap for the detailed
scientific/engineering contract. Performance benefit and AOD/hardware physical
interpretations require their own evidence. Do not manufacture a positive result.

## Completion meaning

Report native compilation, JIT execution, functioning persistent adaptation,
held-out performance benefit and self-hosting separately. A functional learner can
be delivered with a measured zero benefit, but must not be described as a speedup.
Overall M0–M5 functionality needs the corresponding acceptance checks; incomplete
or unavailable checks remain visible. M6 status is separately reported.
