# Ternary Dec: declared model and derived properties

This document supplies the constructive dependency order for the repaired core.
The original hardware-language proposal motivates future work. Its physical
claims are not premises of the executable model below. Source bindings are in
`SOURCE_BINDING.json`; the supplied AOD source identifies v40.01r10 and the owner
M1 plan supplies the closure/recurrence/fusion amendment used here.

## D1. Values and representation

**Setup.** Declare ternary digits T={−1,0,+1}, exact integers and exact rational
pairs with nonzero denominator. Define a 32-digit balanced value by
v=Σ(d_i 3^i), i=0..31. Thus |v|≤(3^32−1)/2. Define Q4 vertices as integers 0..15.
These are implementation domains, not additional physical axioms.

**Derivation.** Repeated remainder extraction with digit −1 for remainder 2 gives
a unique balanced representation in this range. Store each digit as 00,01,10 for
0,+1,−1. Swapping the two bits negates each digit and therefore negates the value;
applying the swap twice is identity. Code 11 is rejected. It does not represent
ontological Null or a physical collision.

**Specialization.** Python uses exact integer arithmetic with explicit bounded
`t32` checks. Rust uses checked finite encodings and wider ratio intermediates;
unrepresentable reduced ratios are errors. Integer quotient/remainder are explicit
operations; `/` returns a ratio and does not discard a remainder.

**Falsification.** Packed round-trip, negation involution, invalid 11 encoding,
range boundaries, same-sign carry and opposite-sign sum tests. The previous
1/4,000,000,000 + 1/4,000,000,000 overflow is a required regression.

## D2. Dec dispatch and execution

**Setup.** Declare each function's input type tuples and result type. No two rows
may have identical input type tuples. Return type does not resolve an ambiguous
input signature. Bodies use typed expressions, assignments and explicit effects.

**Derivation.** A call is admitted exactly when its argument type tuple selects
one declared row. Static name/type checking follows the source statements in
order. Runtime calls use the same signature relation. Compilation serializes the
checked program and source hash without scheduling or hardware observations.

**Falsification.** Unknown names/types, absent signatures, duplicate signatures,
invalid returns and altered IR fail. Changing an emitted body value changes the
observed execution. Repeated compilation yields identical IR bytes.

## D3. Retained closure and recurrence

**Setup.** Declare a scope, lineage, Q4 start and immutable event sequence. An
atomic move changes one bit and has nonzero orientation. This finite specialization
uses one structural hinge per closure segment, constant outgoing orientation and
opposite counterbranch orientation. Dwell does not create a return or a hinge.
A segment closes only when its final event is a counterbranch return to its
retained start. All prior records and identities are preserved.

**Derivation.** A checked return contributes one phase. Revalidating its same
identity contributes none. After the first closure, each further distinct checked
return contributes one beat. This is the explicit initial-closure convention:
phases=n and beats=max(0,n−1).

**Falsification.** An open edge, missing hinge, wrong sign, discontinuity, altered
history, repeated certificate or wrong scope/lineage cannot supply a new closure.

**Provenance.** Owner M1 plan §§3.3–3.7; AOD scope/trace/formation distinction.
The particular finite segment validator is a declared realization. It does not
assert that every admitted AOD motif is expressible in this subset.

## D4. Fusion

**Setup.** Retain two distinct closed inner histories. Form a proposal naming a
new outer scope/lineage. Carry the complete inner histories into an outer trace.

**Derivation.** Inner closure alone leaves the outer trace unclosed. Execute the
outer branch/hinge/counterbranch walk and check its return. Only that checked outer
closure can discharge the proposal. Its certificate remains bound to the parents.

**Falsification.** Missing/open/swapped parents, a different proposal, missing
outer closure or altered lineage fail. No automatic increment of outer phase is
performed during proposal creation.

**Provenance.** Owner M1 plan §3.8. This records retained composition and witnessed
outer closure on a finite Q4 walk; it does not yet construct a general folded
cubical complex, prove all incidence-preservation maps, or implement a fusion ladder.

## D5. Coupled-window comparison

**Setup.** Declare two distinct recurrent lineages on the same scope and equal
positive event-count spans. The common event-ordinal domain and named window are
model choices. Bind at least two distinct increasing pairs of checked return
identities. Contact pairing is an explicit input to this fixture.

**Derivation.** For this common span, ratio of closure rates is N_a/N_b. The signed
lag is k_a−k_b at a bound pair; drift is final lag minus first lag. No modulus is
introduced. Swapping both lineages and contact order reciprocates the ratio and
negates lag/drift. Positive denominators and recurrence are required.

**Falsification.** Zero spans, unequal spans, missing recurrence, same lineage,
wrong scope, fabricated contacts and contact replay fail.

**Provenance.** Owner M1 plan §§3.9–3.11. These count/lag coordinates are a partial
comparison interface. Neither RCD/duration nor SI seconds is supplied. An ordinal
index is not identified with a measured physical time unit.

## D6. Exact ternary normalization and AFC check

**Setup.** Declare three nonnegative exact weights with positive total. Normalize
by that total. For the Python finite AFC helper, separately declare normalized
memory p and row-stochastic kernel P over a finite index set, and a region R.

**Derivation.** Each normalized weight is nonnegative and their sum is one.
Define φ_ij=p_i P_ij−p_j P_ji, and div_i=Σ_j φ_ij. Pairwise internal cancellation
gives Σ_{i∈R} div_i=Σ_{i∈R,j∉R} φ_ij. The helper constructs divergences from the
same kernel; a caller cannot substitute unrelated divergences that only preserve
the regional total.

**Falsification.** Negative weights, zero total, invalid memory/kernel rows and
out-of-domain regions fail. Exact tests cover all regions of a finite fixture.

**Provenance.** AFC supplied `tex/sections/04_choices.tex` and
`tex/corollaries/00_stokes.tex`. Normalized AFC flux is not pressure-weighted SADAR.

## Hardware and later implementations

The bit encoding is an implementation choice on a binary machine. The provided
ELF utilities demonstrate Linux ABI operations with resolved labels and checked
errors. No one-cycle, zero-heat, double-slit equivalence, automatic cache learning
or silicon-to-AOD isomorphism is derived here. Such experiments require a declared
hardware observation map, measured inputs and matched controls. Native Dec code
generation and a Dec-written compiler must preserve the executable semantics
above before self-hosting can be claimed.
