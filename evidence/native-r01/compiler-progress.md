# LLVM implementation checkpoint

Backend: installed released LLVM 16 C API, `/usr/lib64/llvm16/lib/libLLVM-16.so`.
The host lacks LLVM development headers and binaries; the supported C API permits
object emission and ORC LLJIT through ctypes without replacing LLVM.
Official API anchors consulted: LLVM llvmorg-16.0.6 `llvm-c/LLJIT.h`, `Orc.h`,
`TargetMachine.h`. The frontend is Python; generated code and runtime are native.
First verified full ternary LLVM object emitted to `ternary-first.o`; retained IR
is `ternary-first.ll`. AOT link and ORC generated-code execution being tested.
Core C++ runtime is independently implemented by native_runtime task.

Completed implementation: `native/compiler.py` lowers validated source/typed IR
into actual LLVM control-flow bodies; AOT links the native runtime and ORC invokes
native addresses. All five R01 examples matched reference canonical outputs on
optimized entry and forced generic fallback. Native and JIT ternary commands both
executed successfully. The entry guard compares implementation identity before
source effects; fallback is independently generated O0 code. Self-hosting and
language grammar extensions remain explicitly unsupported.

Detailed current contracts: candidate/docs/LANGUAGE_CORE_V1.md,
NATIVE_ABI_V1.md, BUILD_NATIVE.md. Dependency file and package identities:
candidate/native/toolchain-lock.json. Independent conformance and replay are
owned by the root task; benchmark and policy evidence by profile_learning.

Backend acceptance tests now pass in normal and optimized Python modes: 8 groups, 117 retained observations per mode. Evidence: native-backend-normal.json/.log and native-backend-optimized.json/.log. Includes all cache identity and corruption checks, external runtime inputs, actual executable/nonwritable code-page mapping, ownership/concurrent close, rejected forged IR, byte-identical objects/binaries and 84 logical fuel/depth parity points. Error-diagnostic documentation corrected for distinct core/frontend exception classes.

Final compiler hardening: checked foreign scalar/ratio entry construction, explicit signed numeric parsing, Linux file locks, fsync+atomic runtime/cache publication. Final normal/-O suites: 10 groups, 145 retained observations each; all pass. Final evidence: native-backend-normal-final2.json and native-backend-optimized-final2.json. Source hash 16029e1d7b13b21485a07cba60d88a2a755debd88b32a7c66b75f433ed956854. Original audit findings and outcomes retained in backend-audit.json.
