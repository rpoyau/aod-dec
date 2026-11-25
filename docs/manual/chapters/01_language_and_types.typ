#import "/bindings/aod/aod_schema.typ": *

= Clean-Break Dec Language, Rank Polymorphism, and Trit-Boxed Types <sec:types>

#aod-paragraph("Setup.")
Clean-break Dec replaces imperative branches, loops, mutable bindings, binary integer widths, and IEEE-754
floats with pure signature-matrix `dec` declarations, ternary `mux`/`select`/`gate` phase gating, multi-arm
pattern dispatch, J-style compile-time rank lifting on `[T; N]` and `[T]`, fold/scan/boundary adverbs (`+/`, `+\`,
`∂`, `?>`, `+:`, `*:`), `|>` stream pipelines, bounded `walk`/`settle`/`fold`/`scan`/`each` combinators, and
native sandbox apparatus declarations (#aod-cite("aod-main-v41, general-mechanics-style")). All numeric and
structural primitives carry exact discrete representations grounded in AOD Main:
- `t1`: single balanced trit in `{-1t, 0t, +1t}` returned directly by `<=>` and `sgn`.
- `int`: 1-GPR 8-byte Trit-Boxed balanced-ternary integer packing up to 32 trits inline (`00 = 0`, `01 = +1`, `10 = -1` for $|x| <= (3^(32) - 1)/2 = 926510094425920$), using the unused `11` top-lane tag (`0xD000... | handle`) to spill transparently to base-$3^(32)$ `BigTrit` limbs.
- `ratio`: Dec's exact replacement for floating-point numbers, represented as a coupled 2-GPR `{ num, den }` pair reduced via Division-Free Balanced-Ternary Stein GCD ($q > 0$, $gcd(|p|, q) = 1$), plus 1-word `[num][11][den]` `pack_ratio`/`unpack_ratio` when combined trit length fits in `<= 31` trits.
- `b8`, `b64`, and `bytes`: non-arithmetic raw bit and byte containers for binary I/O, SHA-256, ELF64 emission, and 0-cycle `b64(int)`/`int(b64)` register views (rejecting any `11` lane in `int(b64)`).
- `q4`, `addr8`, `support`, and `window`: finite 4-cube vertex $Q_4 = \{0, dots.h, 15\}$ (#mainequation("main:eq:atomic-h1")), leading-zero-preserving octal address (`00_8 != 0_8`, #mainequation("main:eq:first-anchor")), canonical support forms (`3:3:6`, `1:2:6`, `1:2:9_yro`, `3:3:7_yro`, #mainequation("main:eq:hinge-grammar"), #mainequation("main:eq:odd-support-index"), #maintable("main:tab:cc-families")), and half-open intervals $(u, v]$ with $u < v$ (#mainequation("main:eq:closure-phase"), #mainequation("main:eq:closure-trace"), #mainequation("main:eq:closure-certificate")).

#aod-paragraph("Derivation.")
Exact rational arithmetic forbids replacing canonical fractions with rounded decimals (`R02` discipline).
In particular, substituting a two-decimal proxy $9/20 = 0.45$ for the exact fraction $23/50 = 0.46$ creates
a nonzero rational residual:
$ 23 / 50 - 9 / 20 = 1 / 100 != 0. $ <eq:r02-exact-gap>

The following executable snippet verifies #eqref("eq:r02-exact-gap"), exercises the 0-cycle 2-bit balanced-ternary
`b64` view at $-926510094425920$ (`0xAAAAAAAAAAAAAAAA == 12297829382473034410`), and round-trips compact 1-word
`[num][11][den]` `pack_ratio`:

```dec
dec main() -> int:
    let r02_diff = 23/50r - 9/20r
    assert r02_diff == 1/100r and r02_diff != 0/1r
    let neg_lim = -926510094425920
    let word = b64(neg_lim)
    assert u64_int(word) == 12297829382473034410
    assert int(word) == neg_lim
    assert unpack_ratio(pack_ratio(r02_diff)) == r02_diff
    emit r02_diff
    emit u64_int(word)
    0
```

```dec-expect
1/100
12297829382473034410
0
```

Composite aggregates (`[T; N]`, `[T]`, `map[K, V]`, `struct`, `enum`) obey pure immutable value semantics with
static rank lifting and functional copy-on-write updates (`set_at`, `map_put`) across Exact VM, AOT, and ORC LLJIT:

```dec
struct Pair:
    left: int
    right: int

dec main() -> int:
    let a: [int; 3] = [1, 2, 3]
    let b = set_at(a, 0, 99)
    let lifted = (a + [10, 20, 30]) |> +/
    let p1 = Pair(4, 5)
    let p2 = Pair(77, p1.right)
    assert a[0] == 1 and b[0] == 99 and lifted == 66
    assert p1.left == 4 and p2.left == 77
    emit a[0]
    emit b[0]
    emit p1.left
    emit p2.left
    0
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
dec main() -> int:
    let bad = 1.5
    0
```
