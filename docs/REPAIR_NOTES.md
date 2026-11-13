# Repair R01 — 13 November 2026

The intended project is a ternary AOD language. R01 implements and validates an
executable core toward that goal; it does not discard the project because the
original prototype was incomplete. Original archive SHA-256:
`b0ded69f0cf117933596306227cbb14d21735be9d2ba52d259c622cf9d7532c4`.

## Review disposition

| Finding | Action and current status |
|---|---|
| F01 Missing source semantics/self-hosting | Added real parsing, full-body type checking, exact dispatch, typed IR and VM execution. Self-hosting/native Dec backend remain unimplemented, explicitly documented. |
| F02 Broken ELF offsets/argument jump | Replaced byte-count guesses with post-layout label resolution; banner and file-probe executions check exact stdout and error status on x86-64. AArch64 headers/relocations checked statically. |
| F03 Predetermined audits | Removed fake Rust certification path. AOD operations now validate actual retained events and return records; malformed source fails checking. |
| F04 Rational overflow | Python exact Fraction; Rust i128 intermediates and checked reduced i64 range. Original overflow and MIN-value regressions pass. |
| F05 Same-sign annihilation | Defined positional balanced-ternary addition with carry and overflow errors. Encoding11 rejected. Numeric cancellation remains separate from immutable histories. |
| F06 Unwitnessed closure/fusion | Implemented finite retained-start trace validator and independent outer return. Full cell-complex folding/incidence mapping is still beyond this core. |
| F07 Unsupported learning | Removed scheduling counters from compilation and removed hardware-learning/zero-heat claims. Hardware discovery/learning still requires a future measured implementation. |
| F08 Success on I/O failure | Missing source fails; read/write failures propagate; CLI errors are nonzero; native emitters protect `.dec` inputs from accidental overwrite. |
| F09 Invalid primitive/kernel domains | Q4 checks, exact nonnegative normalization, finite AFC computation from p/P, strict event/certificate/contact domains and negative tests. |
| F10 Unproved hardware isomorphism | Rewrote derivation as explicit implementation/model choices with scoped consequences and falsifiers. Hardware equivalence remains an unproved application target. |

The original seven Dec design sketches are preserved under `dec/`. The checker
rejects their unsupported features rather than stamping them certified. Programs
in `examples/` execute end to end. No Main/Manual source or remote repository was
modified. No SI anchor, RCD law, atomic observation law or physical speed/energy
benefit was introduced to make tests pass.

## Evidence

`make verify` passes: 57 Python tests and 9 Rust debug + 9 Rust optimized tests,
followed by five source examples and one compiled-IR example. The Python suite
also passes all 57 under `python3 -O`, confirming that semantic assertions use
explicit checks rather than optimization-removable Python assert statements.
The Rust counts are nine tests run in two modes, not eighteen independent proofs.
The source-mode Python count 57 is likewise the same suite repeated in optimized mode.

Tests cover all Q4 domain values and all valid pair adjacencies, packed trit lanes,
small-range carry arithmetic, exact fraction regressions, unlisted overloads,
unsupported syntax, changed body execution, malformed/edited IR, bounded recursion,
source/IR agreement, twenty byte-identical compilations, open/missing-hinge returns,
wrong/replayed certificates, outer-fusion provenance, coupling reversal/replay,
nonnegative weights, all regions of a finite AFC fixture, and CLI error paths.

An additional review pass found four host API gaps during development: same-scope
outer trace forgery, bool/int aliases, exhausted region iterators and mutable
contact pairs. All were repaired and protected by regressions before final tests.
These were checks by a separate review agent, not independent physical evidence.

Recorded logs are `evidence/verify.txt` and `evidence/python_optimized.txt`.
`evidence/environment.json` binds the actual toolchain and command outcomes.
AArch64 runtime, native Dec compilation and physical AOD/hardware correspondence
were not tested because those execution capabilities are not supplied here.

## Compatibility and next implementation work

The old Rust filename `aod_self_host_compiler.rs` now hosts an explicitly named
banner-demo operation; the actual Dec interface is `aodc.py`. This intentional CLI
change prevents prior acceptance-by-banner behavior. Every Rust emitter requires
an explicit OUTPUT path. The language requires Python 3.10+ standard library; Rust
is needed only for native demonstrations and the full verification target.

The next language extensions are structs/arrays, tuple returns, explicit
literal-pattern precedence, loops and additional source-bound AOD operations.
These can extend the established frontend/IR/runtime contract rather than restore
the old permissive scanner. Native code generation and a compiler written in Dec
then have an executable reference behavior against which to compare. Hardware
measurement/learning remains a separate experiment, not an implicit compiler step.
