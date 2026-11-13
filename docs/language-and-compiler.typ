// Standalone driver. No external Typst packages or network assets.
// Executable examples are paired dec / expect raw blocks; build_docs.py checks them.
#set document(title: "AOD Dec: Language and Compiler", author: "AOD Dec project")
#set page(paper: "a4", margin: (x: 22mm, y: 20mm))
#set text(font: "Libertinus Serif", size: 10.5pt, lang: "en")
#set par(justify: true, leading: 0.62em)
#set heading(numbering: "1.1")
#set outline(indent: 1em, depth: 2)
#set table(inset: 6pt, stroke: 0.4pt + rgb("cbd5df"))
#show heading.where(level: 1): set text(size: 19pt, fill: rgb("15364a"))
#show heading.where(level: 2): set text(size: 13pt, fill: rgb("15364a"))
#show raw: set text(font: "DejaVu Sans Mono", size: 8.2pt)
#show raw.where(block: true): it => block(
  width: 100%, inset: 9pt, fill: rgb("f2f5f7"), radius: 3pt, it,
)
#let note(body) = block(width: 100%, inset: 10pt,
  fill: rgb("edf5f6"), stroke: (left: 2pt + rgb("2c7181")), body)
#let smalltable(..args) = text(size: 9pt, table(..args))
#let path(s) = raw(s)

#v(25mm)
#text(size: 16pt, fill: rgb("2c7181"))[ALPHA OMEGA DYNAMICS]
#v(8mm)
#text(size: 38pt, weight: "bold", fill: rgb("15364a"))[AOD Dec]
#v(4mm)
#text(size: 24pt)[Language and Compiler]
#v(7mm)
#text(size: 14pt)[Core v1 · Native ABI v1]
#v(12mm)
An executable language reference, compiler guide, and collection of
computational examples for the declared finite AOD implementation.

The examples consume the supplied AOD Main and Manual through the source
bindings in #link(<sources>)[Source authority and reading map]. They retain
distinctions, scope, lineage, return identities, and formation witnesses.

#v(7mm)
#note[
  *Implemented execution:* exact reference evaluation, Linux x86-64 native
  compilation, LLVM ORC JIT, and persistent selection between prevalidated
  implementations. The compiler frontend and JIT host use Python.

  *Current limits:* self-hosting and AArch64 execution are unimplemented.
  The retained performance experiment demonstrated no cost-inclusive speedup.
]
#v(1fr)
13 November 2026 #h(1fr) #link("https://github.com/rpoyau/aod-dec")[rpoyau/aod-dec]
#pagebreak()
#set page(
  header: text(size: 8pt, fill: rgb("536471"))[AOD Dec #h(1fr) Language and compiler],
  footer: context align(right, text(size: 8pt, counter(page).display())),
)
#outline(title: [Contents])
#v(1em)
*Reading routes.* Start with the first program to run Dec. Use the language
chapters and built-in tables as the reference. The computational examples
develop retained closure, composition, comparison, and exact normalization.
The compiler chapters explain artifacts, native execution, embedding, and
validation. All commands assume the repository root as the working directory.

*Terminology.* `Dec` names the programming language and its declaration keyword.
The dotted *D.E.C.* names the Manual's operational ledger. Neither a source
type nor a successful compilation establishes a physical interpretation.

#pagebreak()
= First program <start>

Reference execution needs Python 3 and the repository checkout. Save the
following complete program as `first.dec`. Every complete program contains one
zero-argument `main` signature, including programs used through the embedding API.

```dec
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
        emit add(ratio(1, 3), ratio(1, 6))
        result = t32(0)
```

```sh
python3 aodc.py check first.dec
python3 aodc.py run first.dec
```

`check` prints `OK: Dec core v1`. Execution prints the two emitted values and
then the final result:

```expect
t32(2)
1/2
t32(0)
```

The call chooses an overload from the argument types. `t32(1)` is an explicit
checked conversion; the literal `1` alone has type `int`. The result binding
`c` is declared by the signature and assigned without repeating its type.

With the native prerequisites from #link(<installation>)[Native installation]:

```sh
python3 aodc.py build first.dec -O 2 -o build/first
./build/first
python3 aodc.py jit first.dec -O 2
```

Both commands produce the same three lines. `build` emits an executable;
`jit` executes generated machine code in the Python host. A successful program
exits with operating-system status 0 regardless of its returned language value.
For example, returning `t32(7)` prints `t32(7)` and still exits successfully.

#pagebreak()
= Values and exact arithmetic <values>

#smalltable(columns: (18%, 82%),
  table.header([*Type*], [*Domain and use*]),
  [`int`], [Arbitrary exact signed integer, subject to host resources.],
  [`ratio`], [Reduced exact rational with a positive denominator.],
  [`t1`], [One ternary digit: −1, 0, or +1.],
  [`t32`], [A positional word of 32 balanced ternary digits; checked range
    −926510094425920 through +926510094425920.],
  [`q4`], [A vertex numbered 0 through 15 in the declared four-bit domain.],
  [`bool`], [True or false; a distinct type from `int`.],
  [`str`], [Text for labels and output; Unicode and embedded NUL are retained.],
  [`trace`], [Immutable scope, lineage, start, events, closure certificates,
    and any retained parent records.],
  [`fusion`], [A proposal retaining two closed histories and a named outer scope/lineage.],
  [`coupling`], [Two trace snapshots, an ordinal comparison window, and paired contacts.],
)

For balanced digits $d_i in {-1, 0, 1}$, the positional value and bound are
$ v = sum_(i=0)^31 d_i 3^i, quad abs(v) <= (3^32 - 1)/2. $ <eq:balanced>
Addition carries between positions. Thus `t32(1) + t32(1)` is `t32(2)`, whose
low digits are −1 and +1. Every intermediate operation must remain in range.
An overflowing addition followed by subtraction is still an error.

`pack` uses two bits per trit: `00` for zero, `01` for +1, and `10` for −1,
least-significant digit first. `unpack` validates all 32 pairs and rejects `11`.
This binary encoding is an implementation choice; invalid code `11` is not an
ontological Null value. No collision or saturation arithmetic is implied.

```dec
Dec main() -> result:
    () -> int result:
        emit -7 // 3
        emit -7 % 3
        emit 7 / 3
        emit pack(t32(2))
        assert unpack(pack(t32(-17))) == t32(-17)
        result = 0
```
```expect
-3
2
7/3
6
0
```

`/` returns `ratio`, even for integral answers. `//` rounds toward negative
infinity; `%` follows the divisor's sign and satisfies `a == (a // b) * b + a % b`.
Zero divisors fail. `int(ratio(6, 3))` is 2; `int(ratio(1, 2))` fails instead of
discarding a remainder. There are no floating-point values or implicit casts.

#pagebreak()
= Declarations, statements, and dispatch <grammar>

This grammar sketch uses angle-bracket metavariables. It describes layout;
the checker additionally enforces exact types and definite assignment.

```text
Dec <function>(<parameter names>) -> <result name>:
    (<type name>, ...) -> <type result name>:
        <statement>
        ...
    (<another input signature>) -> <type result name>:
        ...
```

Top-level declarations start in column one. Use spaces: four spaces for a
signature and eight for its body is conventional. Indentation must be
consistent within each level; tabs in indentation and nested blocks fail.
Comments start with `#` outside strings. Each statement and expression occupies
one physical line. Blank lines are ignored.

Names use ASCII letters, digits, and underscore, beginning with a letter or
underscore. Reserved expression words cannot be bindings. Each function name
has one `Dec` declaration containing all its overload rows. Built-in names
cannot be redefined. Every row repeats the header's parameter names in the same
order, and its result name must match the header.

#smalltable(columns: (43%, 57%),
  table.header([*Statement*], [*Meaning*]),
  [`int n = 3`], [Declare and initialize a local of the exact stated type.],
  [`n = n + 1`], [Assign a parameter, local, or result already declared.],
  [`emit n`], [Append the rendered value and a newline to output.],
  [`assert n > 0`], [Require a Boolean condition; failure stops execution.],
  [`return n`], [Return an expression of the row's exact result type.],
)

The result binding begins declared but uninitialized. Assign it without a type,
or use an explicit `return`. Reading it before assignment fails. A row that falls
through must have assigned its result; a statement after explicit `return` is
rejected. Locals cannot be redeclared. A call used only for its effects must
appear in a supported statement, such as an assignment; bare call statements
are unsupported.

Dispatch uses the ordered tuple of input types, never the expected result type.
Two rows with identical input types are forbidden even when result types differ.
`add(1, 1)` does not match `add(t32, t32)`. Use `add(t32(1), t32(1))`, or declare
an `int` row. All bodies and both conditional branches are checked, including
functions that `main` never reaches. Forward calls and recursive calls are
supported after signatures have been collected.

#note[Core v1 omits structs, collections, tuples, modules, file I/O, explicit
loop blocks, literal-pattern dispatch, attribute access, and indirect calls.
The files under `dec/` are retained design sketches. Runnable repository
programs live under `examples/`.]

#pagebreak()
= Expressions and control flow <expressions>

Integer literals, quoted strings, and `true`/`false` are supported;
`True`/`False` are also accepted. Integer base prefixes and underscores work,
for example `0xff` and `1_000`. String escapes follow the restricted Python
literal parser. That parser produces an AST which Dec checks; it does not
execute Python source. Calls use a direct function name and positional arguments.

#smalltable(columns: (29%, 71%),
  table.header([*Operators, tightest first*], [*Type rule*]),
  [`+x`, `-x`], [Numeric `int`, `t1`, `t32`, or `ratio`; preserve type.],
  [`*`, `/`, `//`, `%`], [Matching numeric types; `//` and `%` require `int` or
    `t32`; `/` returns `ratio`.],
  [`+`, `-`], [Matching numeric types; preserve type. No string concatenation.],
  [`==`, `!=`, `<`, `<=`, `>`, `>=`], [Matching types; ordering additionally
    requires a numeric type. Chained comparisons fail.],
  [`not`], [Boolean operand and result.],
  [`and`, then `or`], [Boolean operands and result; short-circuit evaluation.],
  [`a if condition else b`], [Boolean condition; `a` and `b` have the same type.],
)

Parentheses override precedence. `q4` is not a numeric operand type, although
`int(q4(3))` is a valid explicit conversion. Equality and inequality also accept
matching record types. No truthiness conversion from integers or strings occurs.

Evaluation is left to right. Only the chosen conditional branch executes, and
an untaken short-circuit operand has no effects or runtime fuel cost. Both sides
still have to be well typed. Recursion with a conditional provides bounded
repetition; it does not supply general loop syntax or tail-call elimination.

```dec
Dec factorial(n) -> value:
    (int n) -> int value:
        assert n >= 0
        value = 1 if n <= 1 else n * factorial(n - 1)

Dec main() -> result:
    () -> int result:
        emit factorial(6)
        emit true or (1 / 0 == ratio(0))
        result = 0
```
```expect
720
true
0
```

The default budget is 100000 logical expression, statement, and function-call
ticks; the default source call-depth limit is 128. These are execution controls,
not physical time or machine-instruction counts. Resource limits and retained
record sizes still matter: exact integers do not mean unlimited memory.

#pagebreak()
= Computational examples and applications <applications>

These examples implement the supplied Main/Manual contracts through core v1's
finite specialization. Main supplies the conceptual and mathematical premises;
the Manual owns operational calculation, records, fixtures, and applications.
The exact source anchors and owner amendment are listed in
#link(<sources>)[Source authority and reading map]. Each example states its
model choices, derives the expected result, and identifies a falsifying input.

== Manual kernel normalization and hinge slide <manual-kernel>

*Setup.* The Manual's D.E.C. kernel example declares four admissible edge
weights $(1, 1, 4, 2)$. Its hinge-slide example separately declares the
post-block pool $((e_1,-1), (e_2,-1), (e_3,+1))$ with weights $(3,3,1)$.
The blocked row is the hinge state $(e_3,0)$, not the entire edge slot $e_3$.
The new weights are declared inputs; they are not computed by deleting the
third entry of the original vector.

```dec
Dec main() -> result:
    () -> int result:
        int total = 1 + 1 + 4 + 2
        ratio e1 = ratio(1, total)
        ratio e2 = ratio(1, total)
        ratio e3 = ratio(4, total)
        ratio e4 = ratio(2, total)
        assert e1 + e2 + e3 + e4 == ratio(1)
        emit e1
        emit e2
        emit e3
        emit e4
        int slide_total = 3 + 3 + 1
        ratio returned = ratio(3, slide_total) + ratio(3, slide_total)
        ratio outgoing = ratio(1, slide_total)
        assert returned + outgoing == ratio(1)
        emit returned
        emit outgoing
        result = 0
```
```expect
1/8
1/8
1/2
1/4
6/7
1/7
0
```

*Calculation and application.* Dividing each initial weight by 8 gives the
four edge fractions. Dividing the post-block weights by 7 gives two returned
fractions of $3/7$ and one outgoing fraction of $1/7$; returned total is $6/7$.
This reproduces the Manual's exact kernel and slide calculation using scalar
ratios. Four edge slots do not coincide with the three roles of `probability`.
Changing the returned grouping or retaining the blocked hinge row falsifies
the stated slide result.

*Provenance.* Main `subsec:q4-hamming-ternary-kernel` and
`eq:hinge-saturation-weighted`; Manual `manual:dec-ledger:kernel` and
`manual:dec-ledger:hinge-slide` in `manual/sections/00_dec_ledger.tex`.

#pagebreak()
== Manual short-window residual and surplus <manual-window>

*Setup.* Use the Manual's declared scalar fixture $rho^D=9$, $C_3=3$,
and window size 7. Only duration participation $rho^D$ is clipped; the full RD
and RCD objects are not clipped or constructed by this program. Compare closure
compatibilities 18 and 24 on the same inputs.

```dec
Dec positive(x) -> out:
    (int x) -> int out:
        out = x if x > 0 else 0

Dec main() -> result:
    () -> int result:
        int rho = 9
        int window = 7
        int c3 = 3
        int clipped = rho if rho < window else window
        int pending = c3 * positive(rho - window)
        int burden = c3 * clipped
        int lower_residual = burden - 18
        int upper_residual = burden - 24
        emit clipped
        emit pending
        emit burden
        emit lower_residual
        emit positive(lower_residual)
        emit upper_residual
        emit positive(upper_residual)
        result = 0
```
```expect
7
6
21
3
3
-3
0
0
```

*Calculation.* Clipped participation is $min(9,7)=7$, pending value
$Q^D=3 max(0,9-7)=6$, and burden $P^D=3 dot 7=21$. Closure 18 gives signed
residual $21-18=3$ and positive surplus 3. Closure 24 gives residual −3 and
positive surplus 0. A negative residual remains a deficit/open check.

*Application and falsification.* This transcribes the Manual's closure-dependent
$1:2:7$ fixture. Replacing the signed residual by its absolute value would
incorrectly turn the upper-closure deficit into surplus. Computing these scalars
does not certify a sheddic route; a complete positive-surplus audit also declares
and checks that route.

*Provenance.* Main `eq:window-clipped-rho`, `eq:closure-residual`, and
`eq:shedding-excess`; Manual `manual:short-window:equations`,
`manual:short-window:fixture-table`, and `manual:short-window:127`.

#pagebreak()
== Manual boundary-current substitution <manual-boundary>

*Setup.* Use the Manual's signed moving-boundary terms: crossing 8, sweep 3,
and shedding 2. For its declared compressed channel, use $C_(3,12)=3$,
$rho^D_(12)=5$, window size 4, and direction term $A_(12)=1/3$. These are fixture
inputs; the compiler does not infer the channel compression or direction from
the ordinal lag of a `coupling` value.

```dec
Dec main() -> result:
    () -> int result:
        int crossing = 8
        int sweep = 3
        int shedding = 2
        int moving = crossing - sweep + shedding
        int c3 = 3
        int rho = 5
        int window = 4
        int clipped = rho if rho < window else window
        int pressure = c3 * clipped
        ratio direction = ratio(1, 3)
        ratio flux = ratio(pressure) * direction
        ratio reverse = -flux
        assert flux + reverse == ratio(0)
        emit moving
        emit clipped
        emit pressure
        emit flux
        emit reverse
        result = 0
```
```expect
7
4
12
4/1
-4/1
0
```

*Calculation.* Moving-boundary slice flux is $8-3+2=7$. Clipped channel
participation is 4, pressure is $3 dot 4=12$, and oriented flux is
$12 dot (1/3)=4$. The reverse orientation is −4. The `/1` outputs retain the
language's exact rational rendering.

*Application and falsification.* This evaluates a declared compressed-channel
boundary calculation. Reversing the sweep sign gives 13 and violates the named
moving-boundary relation. Retaining +4 in both channel orientations violates
antisymmetry. Full path-history compression, SADAR dynamics, and measured
calibration are outside this scalar substitution.

*Provenance.* Main `eq:b-scoped-channel-pressure`, `eq:b-scoped-flux`,
`eq:b-scoped-flux-antisymmetry`, and `eq:moving-boundary`; Manual
`manual:boundary-current:worked-substitution`.

#pagebreak()
== Retained closure and recurrence <closure>

*Setup.* Choose scope `B`, lineage `a`, and retained start `q4(0)`. In this
implementation a step changes exactly one of four bits and has orientation +1
or −1. A segment has a constant outgoing orientation, one explicit structural
hinge, and the opposite counterbranch orientation. Its last event must be an
actual return to the retained start. A dwell preserves position; it is not a
hinge or a new closure.

```dec
Dec recur(t) -> out:
    (trace t) -> trace out:
        t = step(t, q4(1), t1(1))
        t = hinge(t)
        t = step(t, q4(0), t1(-1))
        out = close(t)

Dec main() -> result:
    () -> int result:
        trace a = recur(trace("B", "a", q4(0)))
        emit phases(a)
        emit beats(a)
        a = close(a)
        assert phases(a) == 1
        a = recur(a)
        emit phases(a)
        emit beats(a)
        emit rate(a)
        result = 0
```
```expect
1
0
2
1
1/3
0
```

*Calculation.* Each walk commits three events: step, hinge, step. A distinct
checked return contributes one phase. The first closure has zero reclosure
beats; for $n$ certified returns, beats are $max(0, n - 1)$. Rechecking the same
tail with `close` preserves its identity. After two walks there are six events,
two phases, one beat, and event-ordinal rate $2/6 = 1/3$.

*Application and falsification.* This is an exact retained-history fixture for
recurrence bookkeeping. Removing the hinge, returning with the wrong sign,
or substituting an open tail must fail. `emit a` prints the full record for
inspection. The rate is a declared ordinal quotient, with no SI-second mapping.

#pagebreak()
== Composition with an independent outer return <fusion>

*Setup.* Retain two distinct inner lineages, each closed at its current tail.
`fold` creates a proposal with a different outer scope. `outer` starts a new
trace retaining those parents. The proposal itself contributes no outer phase.

```dec
Dec recur(t) -> out:
    (trace t) -> trace out:
        t = step(t, q4(1), t1(1))
        t = hinge(t)
        t = step(t, q4(0), t1(-1))
        out = close(t)

Dec main() -> result:
    () -> int result:
        trace a = recur(trace("inner", "a", q4(0)))
        trace b = recur(trace("inner", "b", q4(0)))
        fusion p = fold(a, b, "outer", "ab")
        trace c = outer(p, q4(0))
        emit phases(c)
        c = recur(c)
        c = fuse(p, c)
        emit phases(c)
        emit beats(c)
        result = 0
```
```expect
0
1
0
0
```

*Calculation.* Each inner trace has one phase. The new outer trace has zero
phases until its own three-event walk closes. `fuse` then validates that return
against the exact proposal and retained parents. The outer result has one
phase and zero reclosure beats; inner counts are not added to its count.

*Application and falsification.* This is a compositional witness calculation:
retained inputs and an independently completed outer process are both required.
Calling `fuse(p, c)` immediately after `outer`, substituting a different proposal,
or using an open parent fails. Parent histories survive in the result.

Core v1 implements this finite witness contract. Repeated two-parent composition
is supported within resource limits. General folded cell complexes and
incidence-preserving maps require additional model interfaces; their construction
is not inferred from this example.

#pagebreak()
== Comparing recurrent lineages <coupling>

*Setup.* Choose two lineages on scope `B` and a common seven-event ordinal
window. Delay the right lineage by one dwell. Each lineage returns twice;
contacts bind certified event ordinals, not positions in a closure list.

```dec
Dec recur(t) -> out:
    (trace t) -> trace out:
        t = step(t, q4(1), t1(1))
        t = hinge(t)
        t = step(t, q4(0), t1(-1))
        out = close(t)

Dec main() -> result:
    () -> int result:
        trace a = recur(recur(trace("B", "a", q4(0))))
        a = dwell(a)
        trace b = dwell(trace("B", "b", q4(0)))
        b = recur(recur(b))
        coupling pair = coupling(a, b, "seven-events")
        pair = contact(pair, 3, 4)
        pair = contact(pair, 6, 7)
        emit cadence(pair)
        emit lag(pair)
        emit drift(pair)
        result = 0
```
```expect
1/1
-1
0
0
```

*Calculation.* Full-window phase counts are $N_a = N_b = 2$, so cadence is
$N_a/N_b = 1$. At the first contact the lag is $3 - 4 = -1$; at the last it is
$6 - 7 = -1$. Drift is the difference of those lags, hence zero. The left
lineage has an open dwell tail but retains its two certified returns.

*Application and falsification.* This computes relative recurrence and signed,
unwrapped lag for explicitly paired histories. Both histories must have equal
positive event spans, common scope, and distinct lineages. At least two distinct
increasing contacts and two closures per lineage are needed by the accessors.
A contact at event 2 is fabricated because that event is a hinge. Reusing a
contact or choosing unequal windows fails. Contact selection is an input to the
fixture; it does not infer synchronization, a duration unit, or a full RCD model.

#pagebreak()
== Exact normalized continuation weights <weights>

*Setup.* Declare nonnegative weights $(w_-, w_0, w_+) = (2, 1, 3)$ with positive
total $W = 6$. The three roles are returned, hinge, and outgoing, represented
by `t1(-1)`, `t1(0)`, and `t1(1)` in this interface.

```dec
Dec main() -> result:
    () -> int result:
        ratio returned = probability(2, 1, 3, t1(-1))
        ratio hinge_weight = probability(2, 1, 3, t1(0))
        ratio outgoing = probability(2, 1, 3, t1(1))
        emit returned
        emit hinge_weight
        emit outgoing
        assert returned + hinge_weight + outgoing == ratio(1)
        result = 0
```
```expect
1/3
1/6
1/2
0
```

*Calculation.* For each role $r$, define $p_r = w_r/W$. Exact normalization gives
$ p_- + p_0 + p_+ = 2/6 + 1/6 + 3/6 = 1. $ <eq:weights>
No random sample or trace transition is executed by `probability`.

*Application and falsification.* Use this calculation to inspect a declared
three-choice kernel fragment before introducing an application-specific update
law. Negative weights or zero total fail. Assigning weights does not derive a
physical probability, a pressure-weighted law, or a stochastic evolution rule.

== Extending Manual applications

The Python-only `finite_afc` helper constructs a finite normalized memory/kernel
fixture and its antisymmetric flux, divergence, and boundary sum. It is useful
for exact model checks, but its collection-based interface is not exposed in Dec
core v1. No Dec array syntax should be invented to present it as executable.

A new Manual application should identify its Main premises and exact source
anchors; declare scope, lineage, input domain, observation map, and update law;
then show the calculation and a counterexample tied to the claimed result.
Choose supported Dec operations or explicitly identify a required language
extension. Preserve raw events and witnesses when adding projections or summaries.

Measured-sector examples additionally need data, calibration, units, and a
stated comparison protocol. The compiler's host timings do not provide those
premises. Full RCD/duration, SADAR feedback, physical hardware observation,
and SI anchoring remain separate application work.

#pagebreak()
= Built-in reference <builtins>

The authoritative dispatch table is `BUILTIN_SIGNATURES` in
`runtime/dec_core.py`: 25 names and 33 exact overload rows. In the tables,
`K` denotes each listed type separately, not an implicit union or cast.

== Scalar operations

#smalltable(columns: (49%, 51%),
  table.header([*Signature*], [*Contract*]),
  [`t1(int) -> t1`], [Reject outside −1 through +1.],
  [`t32(int) -> t32`], [Reject outside the balanced 32-digit range.],
  [`q4(int) -> q4`], [Reject outside 0 through 15; no masking.],
  [`int(K) -> int`], [`K` is `int`, `t1`, `t32`, `q4`, or `ratio`.
    A ratio must have no remainder.],
  [`ratio(int) -> ratio`], [Use denominator 1.],
  [`ratio(int, int) -> ratio`], [Normalize numerator/denominator; zero denominator fails.],
  [`pack(t32) -> int`], [Encode all 32 trits in 64 bits.],
  [`unpack(int) -> t32`], [Require a 64-bit unsigned encoding with no `11` pairs.],
  [`neg(K) -> K`], [`K` is `int`, `t1`, `t32`, or `ratio`; checked negation.],
  [`probability(int, int, int, t1)`
    `-> ratio`], [Normalize nonnegative negative/hinge/positive weights,
    then select the specified role; positive total required.],
)

There is no callable `bool` or `str` constructor. Numeric literals are `int`;
typed values require the listed constructors. `ratio(t32(1))` is unlisted:
use `ratio(int(t32(1)))` when that conversion is intended.

== Trace operations

#smalltable(columns: (49%, 51%),
  table.header([*Signature*], [*Contract*]),
  [`trace(str, str, q4) -> trace`], [Scope, lineage, retained start. Names must
    be nonempty after whitespace testing; original spelling is retained.],
  [`step(trace, q4, t1) -> trace`], [Hamming-one move with nonzero orientation.],
  [`hinge(trace) -> trace`], [Record the structural branch transition.],
  [`dwell(trace) -> trace`], [Record an event without changing position.],
  [`close(trace) -> trace`], [Validate return, retain certificate, preserve a
    previously certified identity on replay.],
  [`phases(trace) -> int`], [Count checked return identities.],
  [`beats(trace) -> int`], [Count distinct reclosures after the first.],
  [`rate(trace) -> ratio`], [Divide phase count by event count; an empty event
    span has no rate and fails.],
)

#pagebreak()
== Composition and comparison operations

#smalltable(columns: (50%, 50%),
  table.header([*Signature*], [*Contract*]),
  [`fold(trace, trace, str, str)`
    `-> fusion`], [Closed inner traces with distinct scope/lineage identities;
    an outer scope different from both parents and a named outer lineage.],
  [`outer(fusion, q4) -> trace`], [Start an outer trace retaining the proposal's parents.],
  [`fuse(fusion, trace) -> trace`], [Require the matching proposal/parents and
    independently checked outer closure.],
  [`coupling(trace, trace, str)`
    `-> coupling`], [Bind same-scope, distinct-lineage histories with equal
    positive event spans and a named comparison window.],
  [`contact(coupling, int, int)`
    `-> coupling`], [Append increasing, unreplayed certified event-ordinal pairs.],
  [`cadence(coupling) -> ratio`], [Full-window left phase count / right phase count.],
  [`lag(coupling) -> int`], [Left minus right event ordinal at the last contact.],
  [`drift(coupling) -> int`], [Last contact lag minus first contact lag.],
)

The three comparison accessors require recurrence and at least two contacts.
Each operation returns an immutable value. Reassigning a source variable
changes its binding and does not mutate previously retained histories.

== Output and failures <errors>

`int` prints in decimal; `bool` prints `true` or `false`; `str` prints its text
without quotes; `ratio` always prints numerator/denominator, including `/1`.
Bounded types print `t1(n)`, `t32(n)`, or `q4(n)`. Records print canonical
escaped JSON with retained histories. Unicode and NUL lengths are preserved
across the native ABI; terminal display may not visibly distinguish all bytes.

The first failure stops later effects. Already emitted bytes remain committed.
The CLI writes `aodc: <error>` to stderr and normally exits 1. Operation failures
include the enclosing statement's `line N:`; fuel and call-depth failures remain
unlocated. Assertions execute even when Python is started with `-O`.

#smalltable(columns: (48%, 52%),
  table.header([*Example failure*], [*Reason*]),
  [`int x = 1 / 2`], [Static type mismatch: expression is `ratio`.],
  [`int x = int(ratio(1, 2))`], [Runtime conversion would lose a remainder.],
  [`t32 x = unpack(3)`], [Runtime invalid trit encoding `11`.],
  [`close(trace("B", "a", q4(0)))`], [No witnessed return exists.],
  [`contact(pair, 2, 4)`], [In the seven-event fixture, event 2 is a hinge,
    not a certified return.],
)

Checking syntax does not execute a body or establish its witnesses. Complete
programs and expected output blocks in this manual are replayed by the
documentation build; these small failing expressions illustrate contracts and
are not standalone programs.

#pagebreak()
= Commands, installation, and artifacts <installation>

Reference commands use Python 3 with no third-party packages. Native build and
JIT additionally require the released LLVM 16.0.6 shared library, a C++17-capable
`g++`, GMP development libraries, and OpenSSL crypto development libraries.
LLVM headers, `clang`, and `llvm-config` are not required by this C API bridge.

On the Ubuntu 24.04 CI image, prerequisites are installed as follows:

```sh
sudo apt-get update
sudo apt-get install build-essential python3 libgmp-dev libssl-dev \
  libllvm16t64=1:16.0.6-23ubuntu4
dpkg-query -L libllvm16t64
```

Set `DEC_LLVM_LIBRARY` to the listed `libLLVM-16.so.1` file before native use.
For another distribution, locate its released LLVM 16.0.6 shared library and
set the variable to that absolute path. The historical Fedora default in the
compiler is not a portable installation path. Actual library version and target
are checked. Running an AOT executable needs its system shared libraries, but
not Python, Dec source, or an LLVM installation.

#smalltable(columns: (42%, 58%),
  table.header([*CLI subcommand*], [*Artifact or behavior*]),
  [`check SOURCE`], [Parse and statically check every declaration.],
  [`run SOURCE`], [Execute the exact reference interpreter.],
  [`compile SOURCE -o FILE`], [Write canonical typed JSON IR; create the
    destination directory first. This command does not emit machine code.],
  [`exec IR`], [Revalidate embedded source/IR, then execute the reference VM.],
  [`build SOURCE -o FILE [-O N]`], [Emit and link a native executable;
    write a neighboring `.build.json` manifest.],
  [`jit SOURCE [-O N]`], [Lazily compile, load, and invoke native code through ORC.],
)

Prefix each subcommand with `python3 aodc.py`. Native `-O` accepts 0 through 3
and defaults to 0; it controls generated LLVM code. The C++ runtime is built at
`-O2` separately. `run` and `jit` expose no CLI resource-budget switches. The
emitted AOT runner accepts optional positional fuel and depth, for example
`./build/first 100000 128`. Embedding exposes named budget arguments.

```sh
mkdir -p build
python3 aodc.py compile examples/closure.dec -o build/closure.json
python3 aodc.py exec build/closure.json
```

Input caps are 1,000,000 UTF-8 bytes for source and 8,000,000 bytes for IR.
Outputs cannot overwrite their source path. These caps and logical fuel are
not a complete resource sandbox for hostile inputs.

#pagebreak()
= Compiler architecture <architecture>

#let stage(body) = block(width: 100%, inset: 7pt, radius: 3pt,
  fill: rgb("edf5f6"), align(center, body))
#stack(dir: ttb, spacing: 5pt,
  stage([*Dec source* · declarations, signatures, ordered bodies]),
  align(center)[↓],
  stage([*Frontend* · parsing, type checking, definite assignment, exact dispatch]),
  align(center)[↓],
  stage([*Verified typed IR* · canonical source-bound JSON]),
  grid(columns: (1fr, 2.1fr), gutter: 10pt,
    stage([↓\ *Reference interpreter*]),
    stack(dir: ttb, spacing: 5pt,
      stage([↓\ LLVM lowering and module verification\
        optimization → native object]),
      grid(columns: (1fr, 1fr), gutter: 7pt,
        stage([↓\ *AOT*\ object + runtime\ + runner\ g++ link → executable]),
        stage([↓\ *JIT*\ object + runtime\ symbols\ ORC → native entry])),
    )),
)

== Source-bound typed IR

The JSON format is `aod.dec.ir`, version 1. It contains original source, its
SHA-256, ordered functions and overload branches, typed parameters/results,
ordered statements, source lines, and typed expression trees. Serialization
sorts keys deterministically and introduces no timestamp or output path.

`loads_ir` reconstructs the program from embedded source and compares canonical
JSON. It rejects unknown or duplicate fields, source/hash mismatches, changed
typed values, and inconsistent dispatch. Public native entries perform the same
verification. A source hash detects inconsistency; it is not a signature that
authenticates a trusted author. The full schema is defined by the frontend's
actual canonical output and described in `docs/SEMANTIC_IR_V1.md`.

== Native lowering

There is one LLVM function per exact overload. Direct calls implement source
calls and recursion. Conditional and short-circuit expressions use branches and
phi nodes; checked operations and effects call the native runtime in source
order. Failure exits preserve the first error. Fuel ticks and source-line
operands preserve the reference execution contract across optimization.

LLVM verifies the module before emission. Both AOT and ORC use emitted native
objects and the same C ABI. The runtime implements operations on values; it does
not interpret a Dec instruction stream. The current emitter uses the baseline
`x86-64` CPU and an empty feature string. Host features contribute to artifact
identity and profile binding; this is not CPU-specific instruction tuning.

#pagebreak()
= Native runtime and embedding <embedding>

`native/runtime.hpp` declares the C ABI; `native/runtime.cpp` implements it with
C++17, exact GMP integers/ratios, and OpenSSL witness digests. Generated functions
take a context pointer, followed by opaque value pointers, and return a value
pointer. A context owns immutable values and retained history graphs.

The first error is sticky. Exported helpers catch C++ exceptions; no exception
unwinds into generated code. Explicit UTF-8 lengths preserve embedded NUL.
Render/output buffers are borrowed: copy bytes before freeing the context or
overwriting render scratch storage. Handles cannot cross context lifetimes;
forged or dangling pointers are outside the embedding contract. Allocation-limit
tests cover runtime value allocation, not recovery from system-wide or GMP OOM.

== Python JIT API

The following host code uses `first.dec` from the first program and calls its
explicit `t32` overload directly:

```python
from pathlib import Path
from native.compiler import JitSession

source = Path("first.dec").read_text(encoding="utf-8")
with JitSession(source, opt=2) as session:
    record = session.run(
        function="add", args=[("t32", "8"), ("t32", "5")],
        fuel=100000, max_depth=128,
    )
    if record["exit_code"]:
        raise RuntimeError(record["error"])
    print(record["result"])  # t32(13)
```

External arguments can be Python `int`, `bool`, `str`, or explicit `(type, text)`
scalars such as `("ratio", "-20/30")`. Native constructors validate the latter.
Record arguments are not exposed through this bridge. The selected function
must have the exact signature; an unlisted entry raises `DecError`.

`run(fuel=100000, max_depth=128, function="main", args=None, guard_key=None)`
returns a record containing:

#smalltable(columns: (34%, 66%),
  table.header([*Field*], [*Meaning*]),
  [`stdout`, `result`], [Emits only, and the separately rendered return string.
    `result` is `None` on a language failure.],
  [`error`, `error_line`, `exit_code`], [Diagnostic, source line (0 when
    unlocated), and success/failure status.],
  [`compile_ns`, `load_ns`], [Session preparation costs retained in later
    results; do not count them again as fresh per-call costs.],
  [`execute_ns`], [Generated entry invocation time, excluding other host overhead.],
  [`address`, dispatch/cache metadata], [Diagnostic code identity and execution
    path; the address is not an owning callable handle.],
)

Compilation/loading is lazy. A session owns ORC code and serializes calls and
close; close waits for an in-flight call. Later calls are rejected. The ORC
symbol table exposes only the declared runtime helpers, without arbitrary
process-wide symbol lookup or Python callbacks.

#pagebreak()
= Caches, guards, and retained selection <adaptation>

== Artifact reuse and entry guards

Generated-object identities bind source/IR, semantic and ABI versions, compiler
and runtime source bytes, LLVM library bytes/version, target/host features, and
optimization options. Cached objects and LLVM IR have integrity manifests.
The runtime cache separately binds its source and generated object/library
integrity; it does not freeze the complete g++/GMP/OpenSSL build environment.
The default cache is `build/native/cache`.

Cooperating publishers use Linux locks, temporary files, synchronization, and
atomic replacement. Corrupt or incomplete cached objects are rebuilt. This is
trusted local build storage, not an authenticated remote artifact service.

A generated entry checks a 64-bit identity derived from the selected artifact
key before source effects. A mismatch calls separately emitted O0 generic
native code. This is a whole-function identity guard, with no input-range
speculation, mid-function deoptimization, or interpreter fallback.

== Persistent policy

`native/learning.py` selects between prevalidated O0 and O2 implementations.
An invocation uses only previously committed observations; new observations
can influence the next invocation. Profiles bind workload, host, software, and
artifact identities, retain at most 32 paired samples, and have single-writer
ownership. Missing, corrupt, or incompatible profiles retain generic execution.

```sh
python3 scripts/adapt_native.py examples/closure.dec \
  --profile build/closure-profile.json --mode passive --train-pairs 12 \
  --json-report build/closure-profile-report.json
python3 scripts/adapt_native.py examples/closure.dec \
  --profile build/closure-profile.json --mode active \
  --json-report build/closure-selection-report.json
```

`passive` measures without changing current selection; `active` uses retained
selection and may update afterward; `frozen` reads without updating; `generic`
forces the O0 variant. Startup reference prevalidation and profiling are real
costs. `--cache PATH` selects an object-cache location.

The policy requires at least five pairs, a ratio of median optimized cost to
median generic cost below 0.95, and a positive paired-bootstrap lower gain
bound before choosing optimized code. Held-out
acceptance additionally requires more than 5% lower-bound improvement and
positive savings after overhead. A checksum detects corruption; it is not
authentication. Synthetic retained-cost fixtures establish policy influence,
not speedup.

*Two uses of generic.* Learner variant `generic` means O0. A JIT record's
`dispatch: generic` means the guard-fallback branch. A selected O0 module can
report `dispatch: specialized` when its primary identity guard matches.

The recorded seven-workload experiment found no cost-inclusive benefit and
restored generic for every workload. Its held-out samples were later doubled
batches of the same programs, with concurrent compilation noted as a limitation.
No unseen-program performance or physical learning law follows from this result.

#pagebreak()
= Validation and practical limits <validation>

```sh
sha256sum -c SHA256SUMS
python3 scripts/check_import_provenance.py
make verify-native
make docs
```

`verify-native` runs the reference tests in normal and optimized Python, retained
Rust tests in debug and optimized modes, native runtime/witness checks, and
reference/AOT/ORC conformance. It records commands, outcomes, and logs under
`build/verification`. `make verify` is the older reference/Rust/example suite;
`make all` builds legacy ELF demonstrations. Neither is the native compiler
entry command.

The documentation build compiles this Typst source to a PDF and checks every
complete `dec` example against its adjacent expected output. Native replay also
checks AOT and ORC at O0/O2. The build's JSON report records exactly which modes
ran. The pinned `aod-lint` tool checks this manuscript's supported Typst rules;
it does not check Dec semantics. See `docs/LINT.md` for the tool revision,
coverage, and reproduction command.

== Troubleshooting

#smalltable(columns: (38%, 62%),
  table.header([*Symptom*], [*Action*]),
  [LLVM cannot be loaded], [Install released LLVM 16.0.6 and set
    `DEC_LLVM_LIBRARY` to its actual shared library file.],
  [Missing GMP/OpenSSL headers or linker libraries], [Install the distribution's
    development packages and a working C++17 `g++`.],
  [An unlisted signature], [Compare exact input types and use explicit
    conversions. The result type cannot choose an overload.],
  [A result is never assigned], [Assign the signature result without redeclaring
    it, or use an explicit `return`.],
  [A witness is rejected], [Inspect the complete emitted history, scope,
    lineage, hinge, return identity, and parent/contact bindings.],
  [A program exceeds fuel or depth], [Reduce the computation or explicitly
    choose suitable embedding/AOT limits; these are host execution budgets.],
  [Profile still selects generic], [Inspect the report's binding and cost
    evidence. Correct selection does not require choosing optimized code.],
)

== Supported boundary

Execution is covered on Linux x86-64. AArch64, Android, other operating systems,
GPU execution, source-level parallel execution, and a Dec-written compiler are not implemented
targets. Static AArch64 encodings in legacy Rust utilities do not supply a Dec
backend. Self-hosting requires a genuine compiler and a reproducible staged
bootstrap; the old banner/file-probe executables are demonstrations only.

Semantics and exact witnesses are checked separately from performance.
Differential agreement is supplemented by independent exact answers and hostile
mutations; agreement between two implementations alone is not a proof of the
underlying ontology or a physical prediction.

#pagebreak()
= Source authority and reading map <sources>

This manual describes the repaired core v1 and native implementation imported
at repository revision `ab64e3822276a77fd121050484bf155b770a5cf4`. Documentation
changes do not redefine those semantics. Current owner instructions govern
scope; the preserved local Main/Manual amendment governs ownership, as reconciled
in `docs/SKILLS.md`. Main retains ontology and explicitly scoped mathematics;
this application manual contains operational computation and compiler usage.

The execution request binds `AOD_v40_01r10_source.zip`, SHA-256
`a178a5bf06d51c8ab8852e9cabfb6c5eb61456f70739d7391582c08c524f34c7`, and
`OWNER_M1_PLAN.md`, SHA-256
`d3ed657888a6a3c63b13dd34b92a38557189aaccda81775144598267c053859c`.
These identify supplied source snapshots, not an inferred latest publication.
The owner amendment's closure-first, retained-return, and independent-outer-witness
requirements take precedence over conflicting older wording within their scope.

== Main and Manual destinations

All labels in the computational examples were checked in the bound archive.
They are semantic source identifiers, not guessed printed equation numbers.
The Main/Manual sources remain unchanged.

#smalltable(columns: (52%, 48%),
  table.header([*Archive member*], [*Use in this manual*]),
  [`sections/`\ `04_cut_running_fractal_tesseract.tex`], [Q4 adjacency and
    weighted admissible branching; exact kernel and hinge-slide example.],
  [`sections/`\ `05_curl_closure_duon_current.tex`], [Closure/recurrence
    context, reconciled with owner amendment sections 3.4 and 3.7.],
  [`sections/06_field.tex`], [Window-clipped participation, compressed channel
    pressure/flux, moving boundary, signed closure residual, and positive surplus.],
  [`manual/sections/00_dec_ledger.tex`], [Kernel fractions and the declared
    post-block hinge-slide pool.],
  [`manual/sections/`\ `03_short_window_rcd_`\ `shedding_fixtures.tex`], [The
    1:2:7 closure-dependent calculation and signed deficit/surplus distinction.],
  [`manual/sections/`\ `04_boundary_current_applications.tex`], [Canonical
    moving-boundary and channel-flux substitution.],
)

Main `eq:fusion-closure` and owner amendment section 3.8 supply the
proposal/independent-outer-return distinction. Owner amendment sections 3.3,
3.4, and 3.7 constrain the finite closure fixture; sections 3.9, 3.10, and 3.11
constrain comparison and the metrology boundary. The three-role normalized
weight helper is the declared core D6 fragment, not a replacement for the
Manual's edge-state pool.

#pagebreak()

== Implementation destinations

#smalltable(columns: (47%, 53%),
  table.header([*Source*], [*Owning definition or evidence*]),
  [`runtime/dec_language.py`], [Grammar, typing, canonical IR, and reference execution.],
  [`runtime/dec_core.py`], [Value domains, exact operations, builtin signatures,
    immutable histories, witness validators, and rendering.],
  [`native/compiler.py`], [Verified LLVM lowering, object emission, AOT linking,
    ORC sessions, artifact caches, and entry guards.],
  [`native/runtime.hpp`, `.cpp`], [Native C ABI, lifetimes, GMP values, and retained witnesses.],
  [`native/learning.py`], [Bounded retained selection policy and profile validation.],
  [`docs/FIRST_PRINCIPLES_DERIVATION.md`], [Declared finite implementation
    choices D1 through D6, with exact falsification targets.],
  [`evidence/native-r01/SOURCE_BINDING.json`], [Original source and foundation
    hashes, scope, and owner-amendment binding.],
  [`evidence/native-r01/ACCEPTANCE_RESULTS.json`], [Per-criterion execution
    outcomes, including unimplemented self-hosting and no measured speedup.],
)

For deeper interfaces, read `docs/SEMANTIC_IR_V1.md`, `docs/NATIVE_ABI_V1.md`,
`docs/BUILD_NATIVE.md`, `docs/EXECUTION_NATIVE.md`, and `docs/BENCHMARK_PROTOCOL.md`.
The repository's `evidence/README.md` indexes raw records and validation runs.
`docs/SKILLS.md` binds the requested `aod-skills` revision;
`docs/LINT.md` binds `aod-lint` and its actual manuscript coverage.
