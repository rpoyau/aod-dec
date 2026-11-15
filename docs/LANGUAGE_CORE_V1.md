# Dec native core v1

The semantic authority is the immutable repaired R01 `runtime/dec_language.py`
and `runtime/dec_core.py`, bound by `SOURCE_BINDING.json`. This implementation
adds execution modes, not physical laws or new grammar. Every public lowering
entry reconstructs typed IR from its source and compares canonical JSON before
native code generation. This rejects modified payloads, booleans substituted
for integers, changed source hashes, unknown fields and inconsistent types.

A Dec declaration contains explicitly typed overload rows. Input types determine
exact dispatch; result types do not resolve overloads. Supported statements are
assignment, emit, assertion and return. Expressions are exact literals, names,
direct calls, checked unary/binary operations, comparison, short-circuit Boolean
operations and conditional expressions. All declared bodies are verified,
including unreachable-from-main declarations. Unknown or unsupported syntax is
rejected before code generation.

`int` is unbounded GMP integer. `ratio` is a canonical exact rational with a
positive denominator. `t1` is {-1,0,1}; `t32` is the symmetric integer range
±(3^32-1)/2. Every intermediate t1/t32 operation is checked. Packed t32 uses
00=0, 01=1, 10=-1; 11 is rejected. q4 is 0..15. Booleans are distinct from
integers. Arithmetic has no implicit coercions. Division returns ratio;
quotient rounds toward negative infinity; remainder follows the divisor sign.
All core builtin signatures retain their R01 types and validation ordering.

The LLVM control-flow IR has one native function per overload, explicit helper
calls for every checked operation, source-line operands, direct recursive calls,
and branch/phi nodes for conditional evaluation. Every reference expression,
statement and call decrements logical fuel once. Default fuel is 100000 and
call depth 128. Fuel is not a machine-instruction or time budget. Untaken
conditional and short-circuit operands have no effects and consume no fuel.

Evaluation is left-to-right. First failure prevents subsequent emits, calls and
state changes. Already emitted bytes are preserved. Runtime operation failures retain the
baseline statement-wrapped `line N:` diagnostic and additionally expose that
source line as metadata. The core and frontend exception classes are distinct
in R01, so ordinary operation errors and assertion failures are both located.
Resource/depth failures remain unlocated. Native allocation failure is an
explicit runtime failure, tested through allocation-limit injection.

The CLI prints ordered emits followed by the final rendered value on success.
On failure it prints committed emits, writes `aodc: <error>` to stderr and exits 1.
The embedding API separates emits, result, error, error_line and execution
metadata. Embedded NUL and Unicode string values are length-bearing across the
ABI; canonical retained records use the same escaped JSON as the reference.

Supported: all ten R01 value types, all original builtin signatures, five worked
examples, exact overloads, conditionals, recursion and immutable witness records.
Unsupported: structs, arrays/slices, tuples, explicit loop blocks, modules,
pattern dispatch, file I/O, source-level shared-state concurrency, general cell-complex
folding and general RCD construction. These are future grammar/model increments;
the original `dec/*.dec` sketches are not thereby executable programs.

The separate `aodc batch` interface compiles selected pure int/bool entries to
LLVM/native CPU threads or generated CUDA. GPU inputs/intermediates use an
explicit checked int64 domain. This adds no collection or concurrency grammar;
general Dec/GMP execution on GPU and GPU AOT remain unsupported. See the
[parallel compiler companion](parallel-compiler.typ).

The scalar native target has Linux x86-64 execution coverage; the bounded CUDA
batch target also has recorded NVIDIA device execution. AArch64, Android and other operating
systems are not claimed. The compiler frontend and JIT orchestration currently
require Python; emitted AOT programs do not. Self-hosting is not implemented.
