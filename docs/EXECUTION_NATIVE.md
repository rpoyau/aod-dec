# Native compiler, ORC and retained hardware selection

The Python frontend validates Dec and emits native LLVM object code. The linked
program executes its lowered bodies through the C++/GMP runtime without Python.
The ORC host uses the same verified source and runtime ABI. Begin at the candidate
root; required toolchain details and installation paths are in BUILD_NATIVE.md.

```sh
python3 aodc.py check examples/ternary.dec
python3 aodc.py run examples/ternary.dec
python3 aodc.py build examples/ternary.dec -O 2 -o build/ternary-native
./build/ternary-native
python3 aodc.py jit examples/ternary.dec -O 2
```

`run` is the unchanged reference engine; `jit` invokes generated native code.
`compile` and `exec` retain their typed JSON IR meanings. The Python compile/JIT
host remains a dependency at those stages. Self-hosting and an AArch64 executable
target are not established by these commands. The old `bootstrap/` ELF demos
remain separate from this compiler.

Use the retained learner independently of the benchmark:

```sh
python3 scripts/adapt_native.py examples/closure.dec \
  --profile build/closure-profile.json --mode passive --train-pairs 12 \
  --json-report build/closure-profiling.json
python3 scripts/adapt_native.py examples/closure.dec \
  --profile build/closure-profile.json --mode active \
  --json-report build/closure-selection.json
```

The first command executes generic code, then profiles prevalidated O0/O2 variants
and saves their costs. The later process reloads those costs and selects a native
implementation before executing the requested program. The native output is
unchanged; the optional JSON report records selection generation, object hashes,
code address, timings and later-invocation preview. The learner variant `generic`
means the O0 module. Its runtime `dispatch` field can still say `specialized` when
the module enters through its valid primary guard; `dispatch: generic` specifically
identifies the generated guard-fallback path. An active call can also collect
new costs using `--train-pairs 1..32`; its current invocation uses only prior state.
`--mode frozen` reads without updating, `--mode generic` forces generic, and
`--mode passive` measures without changing current selection. `--cache PATH`
selects an object-cache directory. Missing, corrupt or mismatched profiles safely
retain generic. A profile belongs to one writer and exact source/toolchain/host
binding. The command performs conservative reference prevalidation on startup;
its total startup cost is reported and must not be called a native speedup.

Run the full prespecified experiment and recompute its analysis:

```sh
python3 scripts/benchmark_native.py --output /absolute/path/to/new-benchmark-run
python3 scripts/analyze_native_benchmark.py /absolute/path/to/new-benchmark-run \
  --output /absolute/path/to/new-benchmark-run/reanalysis.json
python3 -m unittest discover -s tests -p test_learning.py -v
```

The benchmark executes all five examples, large exact rational reductions and
nested fusion; checks native guards and cross-process retained selection; and
compares generic, passive, frozen, active, stale, shuffled and removed profiles.
It retains raw failed and successful runs and every per-criterion outcome belongs
to its exact artifact bindings. Later measurements of the same programs at doubled
batch size form its held-out split; this does not demonstrate unseen-program
performance. Synthetic retained-cost fixtures establish causal policy influence
only. They are explicitly excluded from performance analysis.

The delivered experiment did not establish a cost-inclusive performance benefit.
The policy's held-out decision restores generic when preparation/profiling costs
or uncertainty invalidate benefit. Concurrent conformance/compilation work during
measurement is recorded as an experimental limitation; paired randomized runs
reduce order effects but do not remove resource competition. No CPU model or
nanosecond timing is interpreted as an AOD physical law or relational-time anchor.
# Compiler-owned parallel batches

`python aodc.py batch SOURCE --function ENTRY --inputs rows.json --backend cpu
--workers 4` compiles the selected entry and invokes generated LLVM code from
native C++ threads. `--backend gpu` generates CUDA from the verified source and
requires an available device. Input is a JSON array of argument arrays; exact
integer and Boolean types must match the selected signature. Overloaded entries
require `--param-types` in parameter order. Every lane owns its fuel/error state;
the JSON report preserves input order and records actual worker/device use.

The first target accepts pure int/bool entries without calls or emitted effects.
It supports assignments, assertions, return, comparisons, lazy Boolean and
conditional expressions, unary operations, addition, subtraction and multiplication.
CPU values use exact GMP arithmetic; GPU input and executed intermediates must
fit its checked signed 64-bit domain. A target-domain failure is explicit and
does not silently fall back. The source language and scalar AOT ABI are unchanged.
See [parallel compiled execution](parallel-compiler.typ) for boundary and awareness
applications, target limits and evidence interpretation.
