# Executable Dec core v1 specification

## Language and artifacts

The compiler is `aodc.py`, using `runtime/dec_language.py` and `runtime/dec_core.py`.
Compilation produces canonical JSON with format `aod.dec.ir`, version 1, original
source, SHA-256 and typed function bodies. It contains no timestamps, machine
probe results or output filename. The VM reconstructs and validates the typed
program against that source before execution. This detects inconsistent/edited IR;
it is not a cryptographic signature or a claim that arbitrary source is trustworthy.

Commands: `check SOURCE`, `run SOURCE`, `compile SOURCE -o OUTPUT`, `exec IR`.
No missing input is replaced with a sample. Output cannot be the source path.
Compile-time checking is distinct from successful runtime assertions or AOD
witness checks. The final main result is printed; errors produce nonzero status.

## Grammar

Each top-level function begins `Dec name(a, b) -> result:`. Indented rows give
`(type a, type b) -> type result:`. Body statements indent farther, consistently.
Spaces are used for indentation. Comments begin with `#` outside strings.

All signature parameter names and result names match the outer declaration.
Every declared function is checked, whether or not reached from `main`. Exactly
one zero-argument `main()` signature is required. Overloads are resolved solely
by exact argument type tuple. Duplicate input signatures and builtin redefinition
are errors; different return types alone cannot disambiguate dispatch.

Body statements:

- `type name = expression`: declare a local.
- `name = expression`: assign an existing parameter, local or declared result.
- `assert boolean_expression`: execute an assertion; it remains active under `python -O`.
- `emit expression`: print the typed/scalar value or full retained trace.
- `return expression`: explicit return; otherwise the assigned result is returned.

Expressions: integer/string/Boolean literals; names; direct calls; unary `+`, `-`,
`not`; `+ - * / // %`; `== != < <= > >=`; short-circuit `and`, `or`; and
`value_if_true if condition else value_if_false`. Both conditional branches are
statically checked and share a result type. `true`/`false` and Python-style
`True`/`False` are Boolean literals. Reserved expression words cannot be bindings.

No implicit numeric coercion occurs. Numeric operands must share a type. `/`
returns an exact ratio. `//` and `%` operate on `int` or `t32` using floor quotient
and matching remainder; explicit `int(ratio)` rejects nonintegral values.
Ordered comparisons require matching numeric types. Function recursion is bounded
by the reference VM's depth/operation limits, not an AOD physical limit.

Unsupported syntax is rejected: structs, arrays, loops, tuple results, attribute
access, imports, arbitrary Python calls, literal-pattern signature rows, implicit
constructors and native machine-code intrinsics. Original `dec/*.dec` files are
preserved design sketches requiring future language extensions.

## Types

| Type | Declared domain |
|---|---|
| `int` | Exact arbitrary-precision signed integer, subject to host resources |
| `t1` | Exactly −1, 0, +1 |
| `t32` | 32 balanced ternary digits; ±(3^32−1)/2, checked operations |
| `q4` | Vertex 0..15; no implicit masking |
| `ratio` | Exact reduced rational; positive canonical denominator |
| `bool` | Boolean; distinct from int |
| `str` | Text, used for scope/lineage/window names and output |
| `trace` | Immutable scope, lineage, retained start, events, checked closures and parents |
| `fusion` | Two closed inner histories plus distinct outer scope and named proposal |
| `coupling` | Two histories and explicit paired return contacts in a common ordinal window |

`t32` is a numeric positional balanced-ternary word, not 32 independent saturation
lanes. Numeric addition has carry: 1+1=2, represented by low digits (−1,+1).
Overflow is rejected. `pack` stores each digit as 00/01/10; `unpack` rejects any
11 pair. Negation swaps each pair. There is no Null/collision alias in this value
domain. Aggregate cancellation does not erase retained trace events.

## Builtins

The authoritative dispatch table is `BUILTIN_SIGNATURES` in `runtime/dec_core.py`.

| Operation | Signature and behavior |
|---|---|
| Constructors | `t1(int)`, `t32(int)`, `q4(int)`, `ratio(int[, int])`; checked domains |
| Exact conversion | `int(int|t1|t32|q4|ratio)`; ratios must be integral |
| Encoding | `pack(t32)->int`, `unpack(int)->t32`; validate all 32 lanes |
| Negation | `neg(int|t1|t32|ratio)`; same return type |
| Trace start | `trace(str scope,str lineage,q4 start)->trace` |
| Atomic move | `step(trace,q4 next,t1 orientation)->trace`; Hamming-1, nonzero sign |
| Hinge / idle | `hinge(trace)->trace`, `dwell(trace)->trace` |
| Return check | `close(trace)->trace`; adds at most one new checked return identity |
| Counts | `phases(trace)->int`, `beats(trace)->int`, `rate(trace)->ratio` |
| Proposal | `fold(trace a,trace b,str outer_scope,str outer_lineage)->fusion` |
| Outer trace | `outer(fusion,q4 start)->trace` with retained parents |
| Fusion check | `fuse(fusion,trace outer)->trace`; requires matching proposal and outer return |
| Coupling | `coupling(trace a,trace b,str window)->coupling`; equal positive spans, common scope, distinct lineages |
| Contact | `contact(coupling,int a_return_id,int b_return_id)->coupling`; validated increasing pairs |
| Comparison | `cadence(coupling)->ratio`, `lag(coupling)->int`, `drift(coupling)->int` |
| Ternary kernel | `probability(int w_neg,int w_hinge,int w_pos,t1 role)->ratio`; exact nonnegative normalization |

## Trace model

An event identity is its ordinal within the retained scope/lineage history. Event
variants are `step`, structural `hinge`, and `dwell`. A closure segment begins at
the retained Q4 start. It has one outgoing branch orientation, then one structural
hinge, then the opposite orientation on its counterbranch, ending at the retained
start on an actual move. Intermediate atomic steps are Hamming-1. Dwell preserves
position and does not count as closure. This explicit finite specialization is
not a general solver for every AOD motif.

Validation recomputes the retained path, orientation constraints and certificate
digests. Digests bind scope, lineage, start, event prefix and fusion-parent records;
they are content checks, not authorities independent of the trace. Repeating
`close` on an already certified return preserves its identity and count. The first
closure contributes one phase and zero reclosure beats; subsequent distinct
returns contribute one phase and one beat each.

`rate` divides the number of checked closure phases by the committed event count.
It is a declared event-ordinal rate. It is not automatically closures per physical
bip, a duration in seconds or a hardware frequency.

Fusion retains complete inner histories and a separate outer trace. A proposal
has no outer closure. `fuse` succeeds only after the outer walk passes the same
return rules, with its proposal and parents intact. This is the witness/state
contract for the core; general cell-complex fold/incidence maps remain unimplemented.

## Coupled comparison

Coupling binds two immutable trace snapshots on the same named scope, distinct
lineages, and equal positive committed event spans. This equality declares a
common ordinal comparison window; it does not infer experimental synchronization.
At least two distinct increasing pairs of validated closure return identities
are required before accessors run, and each lineage must have at least two
closures. User-supplied contact pairing is part of the declared fixture.

For full-window closure counts N_a,N_b, cadence=N_a/N_b. At a paired return,
lag=k_a−k_b; drift is final minus initial lag. Counts and lags are exact and
unwrapped. Full phase-bias, directional RCD/duration, stochastic evolution, SADAR
feedback, physical hardware observation and SI anchoring are separate extensions.

## AFC helper and kernel scope

`probability` exposes an executable three-choice exact normalized kernel fragment.
It does not select a random outcome or add a new stochastic transition law.
Python-only `normalize_weights` and `finite_afc` provide general finite normalized
fixtures. `finite_afc` constructs pP, antisymmetric net flux, per-node divergence
and the region boundary sum from the same p/P data. It does not accept arbitrary
precomputed divergences. Those collection-based functions are not yet callable
from Dec source because arrays are not in core v1.

## Reference limits and future backends

The parser accepts at most 1 MB source and the IR loader at most 8 MB. The VM uses
an operation budget and call-depth limit. These prevent accidental unbounded
execution in the reference implementation; they are not a complete untrusted-code
resource sandbox. Native compilation, a Dec-written compiler and hardware learning
must preserve the language/model contract before they can be advertised.
