#import "/bindings/aod/aod_schema.typ": *

= Dec v2 Language and Exact Type System <sec:types>

#aod-paragraph("Setup.")
Dec v2 extends Dec core v1 with algebraic records (`struct`), tagged unions (`enum`), exhaustive `match`
dispatch with guards, deterministic key-ordered maps (`map[K, V]`), multi-file `use` imports, and native
sandbox apparatus declarations, while preserving 100% backward compatibility with all ten frozen Dec v1
regression programs under `--compat v1` (#aod-cite("aod-main-v41, general-mechanics-style")). All scalar
and structural primitives carry exact discrete representations grounded in AOD Main:
- `int`: arbitrary-precision signed integer (`BigInt`) with exact floor division `//` and remainder `%`.
- `ratio`: canonical reduced rational $p / q$ ($q > 0$, $gcd(|p|, q) = 1$) with zero decimal parsing on native `ratio`.
- `t1` and `t32`: checked balanced ternary scalar (`-1t`, `0t`, `+1t`) and 32-trit register bounded by $|x| <= (3^(32) - 1)/2 = 926510094425920$, with 2-bit-per-trit `pack`/`unpack` (`00 = 0`, `01 = +1`, `10 = -1`; `11` rejected).
- `q4` and `addr8`: finite 4-cube vertex $Q_4 = \{0, dots.h, 15\}$ (#mainequation("main:eq:atomic-h1")) and leading-zero-preserving octal address (`00_8 != 0_8`, #mainequation("main:eq:first-anchor")).
- `support` and `window`: canonical support forms (`3:3:6`, `1:2:6`, `1:2:9_yro`, `3:3:7_yro`, #mainequation("main:eq:hinge-grammar"), #mainequation("main:eq:odd-support-index"), #maintable("main:tab:cc-families")) and half-open intervals $(u, v]$ with $u < v$ (#mainequation("main:eq:closure-phase"), #mainequation("main:eq:closure-trace"), #mainequation("main:eq:closure-certificate")).

#aod-paragraph("Derivation.")
Exact rational arithmetic forbids replacing canonical fractions with rounded decimals (`R02` discipline).
In particular, substituting a two-decimal proxy $9/20 = 0.45$ for the exact fraction $23/50 = 0.46$ creates
a nonzero rational residual:
$ 23 / 50 - 9 / 20 = 1 / 100 != 0. $ <eq:r02-exact-gap>

The following executable snippet verifies #eqref("eq:r02-exact-gap") and exercises exact `t32` 64-bit
packing at $-926510094425920$ (where bit 63 is set, spilling cleanly to `BigInt` `12297829382473034410`
and round-tripping losslessly through `unpack`):

```dec
Dec main() -> int:
    let r02_diff = 23/50r - 9/20r
    assert r02_diff == 1/100r and r02_diff != 0/1r
    let neg_lim = t32(-926510094425920)
    let packed = pack(neg_lim)
    assert packed == 12297829382473034410
    assert unpack(packed) == neg_lim
    emit r02_diff
    emit packed
    return 0
```

```dec-expect
1/100
12297829382473034410
0
```

Composite aggregates (`list`, `map`, `struct`, `enum`) obey strict value semantics via copy-on-write
updates across Exact VM, AOT, and ORC LLJIT: mutating `b[0]` after `mut b = a` never mutates `a[0]`:

```dec
struct Pair:
    left: int
    right: int

Dec main() -> int:
    mut a = [1, 2, 3]
    mut b = a
    b[0] = 99
    mut p1 = Pair { left: 4, right: 5 }
    mut p2 = p1
    p2.left = 77
    assert a[0] == 1 and b[0] == 99
    assert p1.left == 4 and p2.left == 77
    emit a[0]
    emit b[0]
    emit p1.left
    emit p2.left
    return 0
```

```dec-expect
1
99
4
77
0
```

#aod-paragraph("Falsification.")
Any floating-point literal (`1.5`, `5.`, `.5`, `1e3`) is rejected during lexing before typechecking or
code generation, ensuring IEEE-754 approximations never enter native calculations:

```dec-fail FloatLiteralForbidden
Dec main() -> int:
    let bad = 1.5
    return 0
```
