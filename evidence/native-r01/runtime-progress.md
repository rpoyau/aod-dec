# Native runtime checkpoint

Implemented GMP exact scalar arithmetic and all language-visible builtins from the immutable `source/runtime/dec_core.py`. Internal immutable trace, fusion and coupling handles retain parent histories; canonical JSON/SHA-256 validation precedes witness operations. C ABI declarations are in candidate/native/runtime.hpp. Work is candidate implementation, not an AOD model amendment.

Build tested: `g++ -std=c++17 -fPIC -shared native/runtime.cpp -lgmpxx -lgmp -lcrypto`.

An initial white-box executable segfaulted; the cause and fix are recorded below. Final independent debug/O3 white-box tests pass. AddressSanitizer instrumentation compiled but linker failed because `/usr/lib64/libasan.so.8.0.0` is absent. Preserve this environment gap. `gdb` could not trace inside sandbox (`ptrace: Operation not permitted`); test backtrace handler being used to diagnose.

## Resolved implementation finding

The segfault was traced to copying an intentionally noncanonical GMP rational with a negative denominator. The native constructor now extracts numerator/denominator, normalizes the denominator sign before GMP rational copying, then reduces. The same hostile fixture now passes. Runtime source also has a sticky allocation-failure bit and reference-compatible DEL escaping.

Final scalar differential execution: 1,950 cases, seed 13012026, zero failures. Raw records: `runtime-differential.json`. Library digest is bound there; the diagnostic library is retained in `runtime-build/libruntime-test.so`.

Source-location integration correction: the core and language DecError classes are distinct; all operation errors are statement-wrapped in the VM. Native operation errors now include `line N: ` while fuel/depth errors remain unlocated.

## Final verified runtime checkpoint

- Debug and O3 hostile runtime tests: PASS (`runtime-debug.log`, `runtime-optimized.log`).
- Expanded differential corpus: 1,979 cases, zero failures, including canonical complete trace/fusion/coupling serializations and Unicode/NUL labels (`runtime-differential.json`).
- Commands, source/library/test hashes and explicit limitations: `runtime-results.json`.
- Source ABI/ownership/error/fuel documentation: `../candidate/docs/NATIVE_RUNTIME_ABI.md`.
- No runtime source edits remain pending. Compiler/JIT integration and other milestone acceptance remain the root agent's separate checks.
