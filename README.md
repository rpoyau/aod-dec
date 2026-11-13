# Ternary AOD Dec — native compiler and JIT

Dec compiles its explicitly typed ternary core to Linux x86-64 executables and
executes the same lowered code through LLVM ORC JIT. Exact GMP values and retained
closure/fusion/coupling histories use a native runtime. The original typed JSON
IR and exact reference VM remain available for comparison.

Requirements for the native compiler/JIT: Python, LLVM 16.0.6 shared library,
C++17/g++, GMP and OpenSSL development libraries. Emitted executables run without
Python. The frontend and JIT host still use Python. Optional legacy ELF utilities
require Rust and Make. Self-hosting is not implemented.

```sh
python3 aodc.py build examples/ternary.dec -O 2 -o build/ternary-native
./build/ternary-native
python3 aodc.py jit examples/closure.dec -O 2
```

See [native usage](docs/EXECUTION_NATIVE.md), [build requirements](docs/BUILD_NATIVE.md),
[language inventory](docs/LANGUAGE_CORE_V1.md), and [ABI](docs/NATIVE_ABI_V1.md).
The [Typst language and compiler manual](docs/language-and-compiler.typ) contains
the language reference, native architecture, and executable computational
applications drawn from the bound AOD Main and Manual. Build its PDF and check
the examples with `make docs`; see [documentation builds](docs/CI.md#documentation).
The [tesseract-cell jitter companion](docs/tesseract-cell-jitter.typ) adds an
oriented cell transport control, per-output numerical comparisons, and exact
Manual D.E.C. fixtures with recurring population, retained contacts and directed
RCD. A separate observed-cycle fixture demonstrates the conditional SI map and
reproduces published hydrogen-maser input reductions.
Run `make simulate-cells`; add `--native --plots` to
`scripts/simulate_cell_jitter.py` for native replay and charts. This is an
uncalibrated transport control, with jitter on successor weights. Its aggregate
mass discrepancy is not physical accuracy, kernel error or wave-population
density; see the [source-bound correction review](evidence/manual-alignment/REVIEW.md).

The [chiral encounter application](docs/chiral-core-collision.typ) reuses the
tesseract support for a declared two-carrier Markov model. A fixed population
profile raises flip propensity at tagged cores. The primary `core_only` scenario
uses contact gain `0`; `contact_feedback` separately tests gain `1/4`.
The probabilistic law belongs to this application model. The unchanged D.E.C.
engine executes supplied expressions and checks operations and witnesses; it
does not supply a collision law or a core-bias probability.
Exact joint probabilities, sampled paths and complete step–hinge–return histories
remain distinct records. This collision-inspired model has no calibrated physical
proton–proton correspondence, collision-energy map or cross-section prediction.
Run `make simulate-chiral` for the study and chart; add `--native --plots` to
`scripts/simulate_chiral_collision.py` for native replay. Run `make docs-chiral`
to build the Typst PDF from retained evidence. Plotting needs Matplotlib, native
replay needs the compiler toolchain, and the PDF requires Typst.

The [routed tracing companion](docs/routing-parallel.typ) adds explicit shedding
routes and table-driven batch execution. Scalar CPU, multicore CPU and GPU
backends consume the same exact model tables and supplied tickets. Their
candidate rows remain separate from host construction and validation of full
return, preparation and route witnesses. The probabilistic law stays in the
application model; these execution backends add no intrinsic collision law.
The admitted `3:3` proton-facing preparation fixture is distinct from a physical
proton correspondence. Positive declared surplus is routed with retained
Duon/E2 form and explicit TTL/seat gates. Octaves retain the octal coordinate
presentation with Monon/beat records; event counts are separate, and physical
frequency or wavelength requires an observation map.
Run `make simulate-routing` and `make docs-routing`. The study's `--gpu auto`
records `UNAVAILABLE` when no GPU can execute; `--gpu required` fails instead.
Read the retained device/execution evidence before claiming GPU validation or
speedup. Physical proton-proton preparation, scattering and SI calibration remain
unestablished.

Persistent hardware selection is available through `scripts/adapt_native.py`.
The measured seven-workload experiment established functional adaptation but no
cost-inclusive speedup; every measured workload retained the generic variant.

Completed execution records and repository validation are indexed in
[evidence/README.md](evidence/README.md). The repository uses pinned
[AOD skills](docs/SKILLS.md) and [AOD lint](docs/LINT.md); the latter currently
checks the Typst manual with its supported manuscript rules.
[CI and local validation](docs/CI.md) run the compiler's semantic conformance.

## Run

```sh
python3 aodc.py check examples/ternary.dec
python3 aodc.py run examples/ternary.dec
python3 aodc.py run examples/closure.dec
python3 aodc.py run examples/fusion.dec
python3 aodc.py run examples/coupling.dec
mkdir -p build
python3 aodc.py compile examples/closure.dec -o build/closure.dec.json
python3 aodc.py exec build/closure.dec.json
```

Every program has one zero-argument `main` signature. The CLI prints values from
`emit` statements followed by the returned value. Successful execution exits 0;
source, type, witness, arithmetic, assertion or I/O errors exit nonzero. A result
value such as `t32(1)` is a language value, not an operating-system exit code.

```text
Dec add(a, b) -> c:
    (t32 a, t32 b) -> t32 c:
        c = a + b
    (ratio a, ratio b) -> ratio c:
        c = a + b

Dec main() -> result:
    () -> t32 result:
        t32 sum = add(t32(1), t32(1))
        assert sum == t32(2)
        emit sum
        result = t32(0)
```

## Implemented

- Exact overload dispatch with no implicit numeric coercion.
- `t1`, checked 32-digit balanced `t32`, validated `q4`, arbitrary exact `int` and `ratio`.
- Immutable scoped histories with outgoing branch, structural hinge, counterbranch
  and a checked retained-start return; replay creates no additional phase/beat.
- Fusion proposals that retain inner histories and require a separately executed outer return.
- Paired recurrence comparisons with distinct certified contacts, a declared common
  ordinal window, exact cadence ratio and signed unwrapped lag/drift.
- Exact nonnegative ternary weight normalization. The Python core additionally
  provides a finite normalized AFC flux/Stokes fixture.
- Deterministic typed IR, validated again before execution; bounded recursion and
  an operation budget.
- LLVM object emission, native linking, lazy ORC JIT, checked object caches,
  generated entry guards and generic native fallback.
- Bounded persistent selection over prevalidated O0/O2 implementations, with
  matched controls, retained observations and held-out rollback.

The Q4 trace and coupled-window model are **declared finite specializations**.
They do not supply a universal AOD transition law, physical hardware learning,
an SI-second anchor or a full RCD interface. No scientific audit is inferred from
syntax, a banner, a hardcoded truth flag or an arithmetic identity alone.

The original seven files under `dec/` are retained as **design sketches**. Their
structs, arrays, loops, tuple outputs and literal-pattern dispatch are not yet in
the executable grammar; the new checker rejects them explicitly. Working programs
live in `examples/`. The original broad goals remain visible without being
reported as implemented.

## Verify

```sh
sha256sum -c SHA256SUMS
python3 -m unittest discover -s tests -p 'test_*.py' -v
python3 -O -m unittest discover -s tests -p 'test_*.py' -v
make verify                     # includes Rust debug/optimized tests and examples
# Or: make verify RUSTC=/absolute/path/to/rustc
make verify-native              # reference, native runtime, AOT and ORC conformance
python3 scripts/check_import_provenance.py
```

The Rust filenames remain for patch continuity. Their current commands are:

```sh
make all
build/aod_stage0_seed build/exit-demo
build/aod_self_host_compiler build/banner-demo
build/aod_stage2_bootstrap build/file-probe
build/banner-demo
build/file-probe examples/closure.dec
```

The banner executable writes a fixed message. The file probe opens/closes the
supplied file and checks errors. Neither compiles Dec. AArch64 encodings are
included and statically tested; the delivery evidence distinguishes those checks
from the x86-64 execution performed here.

See `docs/SPECIFICATION.md` for the language/model contract,
`docs/FIRST_PRINCIPLES_DERIVATION.md` for scoped derivations, and
`docs/REPAIR_NOTES.md` for the original review findings and their dispositions.
