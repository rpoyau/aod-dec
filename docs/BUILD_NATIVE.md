# Build and run

Prerequisites: Python 3, released LLVM 16.0.6 shared library, a C++17 compiler,
GMP C++ development libraries and OpenSSL crypto development libraries. The
recorded host package versions and file hashes are in `native/toolchain-lock.json`.
No Python third-party packages, LLVM headers or clang executable are required.
LLVM C API signatures are pinned to the release's `llvm-c/LLJIT.h`, `Orc.h` and
`TargetMachine.h`. `DEC_LLVM_LIBRARY` can locate the same released LLVM 16.0.6
library on another installation; the resulting byte identity enters cache keys.

From the candidate directory:

```sh
python3 aodc.py build examples/ternary.dec -o build/ternary -O 2
./build/ternary
python3 aodc.py jit examples/closure.dec -O 2
python3 aodc.py run examples/closure.dec
python3 -m unittest discover -s tests -v
```

The compiler invokes g++ to build the runtime with `-std=c++17 -O2 -fPIC`, links
with `-lgmpxx -lgmp -lcrypto`, and asks LLVM to emit position-independent x86-64
objects at the selected optimization level. It saves LLVM IR, object bytes and
binding manifests under `build/native/cache`. AOT executables link the runtime
object and retain ordinary system shared-library dependencies. The legacy Rust
ELF demonstration targets are separate and do not implement this compiler.

For embedding:

```python
from native.compiler import JitSession
with JitSession(source_text, opt=2) as session:
    result = session.run()
    fallback = session.run(guard_key='stale')
    input_result = session.run(function='square', args=[123456789])
```

Explicit foreign scalar types use `(type, text)`, for example `('t32', '5')`
or `('ratio', '-20/30')`. Ratios accept exact N/D or integer text; malformed
spellings are rejected. Domain checks use the same native conversion builtins
as compiled source, before entering the function.
The source must declare the exact entry signature. `stdout` contains emits only;
`result` is the rendered return value. `execute_ns` times the generated entry;
`compile_ns` and `load_ns` are setup costs. The profiling harness separately
records wrapper, selection, persistence and total costs.

This bridge uses LLVM's supported C API because installed development headers
and clang/llvm-config tools were absent. It remains the requested LLVM backend.
Official API references:
- https://github.com/llvm/llvm-project/blob/llvmorg-16.0.6/llvm/include/llvm-c/LLJIT.h
- https://github.com/llvm/llvm-project/blob/llvmorg-16.0.6/llvm/include/llvm-c/Orc.h
- https://github.com/llvm/llvm-project/blob/llvmorg-16.0.6/llvm/include/llvm-c/TargetMachine.h

Licenses: LLVM is Apache-2.0 WITH LLVM-exception; GMP is dual LGPL-3.0-or-later
or GPL-2.0-or-later; OpenSSL 4 uses Apache-2.0; GCC is GPL with the GCC runtime
library exception for its runtime; glibc is LGPL. These are external system
prerequisites, not vendored sources. Original bundled source and foundation
license notices remain preserved. This document does not relicense them.
