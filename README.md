# Ternary AOD Dec — executable core R01

Dec now has an executable, explicitly typed ternary core. Source is parsed,
checked and compiled into deterministic typed JSON intermediate code. An exact
reference virtual machine executes that code. Function bodies determine results;
assertions, invalid types, invalid witnesses and arithmetic errors are enforced.

Requirements: Python 3.10+ (standard library only). Optional Linux ELF utilities
and their tests require Rust, a linker and Make. The language compiler/runtime is
currently Python-hosted; self-hosting and native Dec code generation remain later
implementation milestones. The Rust utilities are separate ELF demonstrations.

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
