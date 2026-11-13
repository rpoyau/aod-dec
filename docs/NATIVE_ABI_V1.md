# Native ABI v1

`native/runtime.hpp` is the complete C ABI. Generated LLVM functions accept a
context pointer followed by opaque value pointers and return an opaque value
pointer. The context owns an arena of immutable values, UTF-8 strings, exact GMP
integers/ratios and shared immutable trace graphs. A handle remains valid until
its context is freed; handles must never cross context lifetimes. Parent trace
records remain retained by shared ownership. Individual value destruction is
not exposed. Arena resource use is bounded by the declared execution fuel and
an optional value-allocation limit. GMP's system allocator may still terminate
on unrecoverable system OOM; the allocation-injection test covers runtime value
allocation, not every operating-system failure.

All exported helpers catch C++ exceptions and store the first error. No exception
unwinds across the ABI. Calls after failure preserve that failure. Null contexts
or foreign/dangling handles are outside the native embedding contract; the Dec
frontend cannot produce them. Errors carry baseline diagnostic text (including the statement `line N:` prefix
for operation failures) and source-line metadata. Emitted bytes and rendering lengths are explicit.

`dec_tick` decrements logical fuel. A generated user function first ticks, then
calls `dec_enter`; after any entered call, success or error uses `dec_leave`.
`dec_literal` constructs checked typed values; `dec_binary`, `dec_unary` and
`dec_builtin` execute source operations. `dec_truth` reads a verified Boolean.
`dec_emit` captures output; `dec_assert` raises source-located assertion failure.
`dec_render`, `dec_render_size`, `dec_output` and `dec_output_size` expose owned
borrowed bytes. Copy them before freeing the context or overwriting render
scratch storage. The header documents exact integer widths and argument order.

AOT and ORC use identical LLVM object emission and runtime symbols. ORC defines
only the declared runtime symbols as absolute native addresses; it does not use
a process-wide arbitrary-symbol search or Python callbacks. JIT sessions own
ORC memory and serialize entry/close. A close waits for an in-flight call, then
reclaims code; later calls are rejected. Function addresses returned in diagnostic
records are not owning callable handles and must not be invoked externally.

The JIT compiles lazily at the first call. Artifact keys bind canonical verified
IR, source, semantic version, ABI version, compiler bytes/version, runtime bytes,
LLVM library bytes/version, host triple/features and optimization level. Cached
objects and LLVM IR have checked manifests. Generic and guard objects also have
content checks. Linux file locks serialize cooperating cache publishers across
threads/processes. Runtime and generated object/IR/manifest publication uses
temporary files, fsync and atomic replacement; interrupted artifacts are never
accepted without matching manifests. Invalid or corrupt cache files are recompiled. Cache directories
are trusted local build storage, not signed remote artifacts.

The entry wrapper checks the selected implementation identity before source
effects. A match invokes the selected native object; a mismatch invokes a
separately generated O0 generic object. This is an identity guard for prevalidated
whole-function variants, not an input-range speculation. The dispatch result is
returned separately from source output. No mid-function deoptimization,
interpreter fallback, trace replay or live-code patching is implemented.
