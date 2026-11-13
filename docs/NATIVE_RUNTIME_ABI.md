# Dec native runtime ABI 1

The native runtime implements the repaired core-v1 semantics in `runtime/dec_core.py`. It supplies exact values and witness operations; generated LLVM control flow supplies function calls, overload selection, expression order, branching, statements and logical fuel ticks. It does not execute Dec source or an IR instruction stream. The installed GMP library supplies arbitrary precision integers/rationals; OpenSSL supplies SHA-256. The baseline Rust ELF demonstrations remain separate.

## Ownership and calls

`native/runtime.hpp` is the authoritative C linkage declaration. A context owns a stable arena of immutable values. Generated code borrows opaque `void*` handles; copying a handle does not transfer ownership. All values, their shared immutable retained parents, output bytes and diagnostic buffers are released by `dec_context_free`. Values from another live context are rejected. A freed context or freed handle must never be reused; arbitrary forged context pointers are outside the C ABI contract. Concurrent invocations use distinct contexts.

Integers and ratios use GMP `mpz_class`/`mpq_class` behind opaque handles. Ratios have positive denominators and are reduced by the gcd. `t1`, `t32` and `q4` retain distinct tagged types; checked arithmetic validates every intermediate result. Numeric coercions are explicit. `bool` is a distinct type. Strings use explicit byte lengths and UTF-8. Literal source escape strings and output may contain NUL. JSON escaping follows the reference's ASCII escaping and surrogate-pair notation, including DEL. The compiler validates source and IR before calling this ABI.

`dec_literal`, `dec_unary`, `dec_binary` and `dec_builtin` return a context-owned handle or null on failure. `dec_truth` requires a bool. The compiler must check `dec_ok` and unwind generated frames to the entry after every fallible operation. After the first error, further fallible operations do nothing and preserve the first message and location. C++ exceptions are caught inside the runtime boundary. No C++ object, reference or exception crosses C linkage.

`dec_render` returns a context scratch buffer; read its length with `dec_render_size` immediately afterward. The pointer is valid until the next render or context destruction. `dec_output` and `dec_output_size` expose all committed emits, including the newline after each emit. Read output after execution; a later emit can invalidate its pointer. The runner must write this output even after failure and append the returned value only after successful execution. This preserves effect order and partial output.

`dec_error` returns the first diagnostic text; `dec_error_line` separately returns its source statement line, or zero for a context/fuel/depth error. Runtime errors carry `line N: message` to preserve the reference interpreter's statement wrapper; fuel/depth errors stay unlocated. The reference language and core define distinct `DecError` classes, so core errors are wrapped by the language's ValueError handler.

## Fuel, depth and failures

The compiler emits one `dec_tick` for each statement, expression and call, including builtin calls. Helpers do not tick automatically. The default entry settings are 100000 ticks and depth 128. A tick succeeds when remaining fuel is positive and decrements it; otherwise it records `execution step budget exhausted`. `dec_enter` increments source-function depth and rejects depth greater than the configured limit. A generated function that called enter always calls leave, including an enter-depth failure; a pre-enter tick failure does not call leave.

`dec_set_allocation_limit` sets an explicit per-context maximum number of allocated arena values for deterministic failure testing. Exceeding it produces `runtime allocation limit exceeded`. C++ allocation exceptions become a sticky `runtime allocation failed` status. Error status does not itself require allocating diagnostic text: the fallback message is static. System memory exhaustion within GMP's default allocator is fail-stop under GMP's allocation contract; the OS or GMP may terminate the process. The runtime does not claim recovery from system-wide exhaustion or invalid foreign pointers. Native witness validation is recursive with the same depth-64 trace limit as the reference. The arena keeps values until completion, so long histories can consume substantial memory; this is an explicit implementation tradeoff.

## Retained witnesses

Trace records retain ordered event identities, source/target/orientation, closure ranges, scope, lineage and immutable parent records. Every append validates before and after construction. Closure certification requires the declared Hamming-1 outgoing path, structural hinge and opposite-orientation return to the retained start. Zero net orientation does not erase events. Fusion proposals retain two closed parents and have a distinct outer scope; fusion completion independently requires the proposal's outer closure. Coupling binds distinct lineages in a common scope and equal positive event spans; contacts bind ordered unreplayed certified returns.

Canonical event/trace/proposal JSON and SHA-256 spelling match the reference. No physical clock, SI anchoring, general RCD law or new cell-complex folding rule is introduced.

## Verification

`native/test_runtime.cpp` includes the implementation solely to inject otherwise-inaccessible hostile mutations. It checks exact arithmetic independently, preserved cancellation, altered event IDs, forged certificates, changed scope/lineage/start/parents, contact replay, retained nested histories, foreign handles, fuel/depth and deterministic allocation limits. Production compilation never exports mutation APIs.

`native/check_runtime.py LIBRARY --output RECORD.json` runs the fixed-seed ABI/reference scalar and canonical-witness corpus and retains each input, expected result and actual result. This differential evidence complements the independent properties; agreement alone is not a proof of the scientific model.
