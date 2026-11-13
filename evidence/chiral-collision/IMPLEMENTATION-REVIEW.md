# Chiral encounter implementation: bounded independent review

Date: 13 November 2026. Target: uncommitted application files on
`codex/chiral-core-collision`, based on Dec commit
`1d50634443ac777cee35546a62d53939de314cab`. Source hashes below identify the
reviewed bytes; a later relevant change makes the affected checks stale.

Disposition: **supported within the declared finite model and recorded checks**.
No implementation discrepancy was found in this scope. Physical proton–proton
applicability remains a missing-premise claim, not a result of these checks.
This reviewer authored the separately derived reference and its tests, then
reviewed the host model evaluator and D.E.C. generator without changing them. Algorithmic
independence of the reference is specified below; reviewer agreement is not
independent empirical evidence.

## Authority, source binding and actual coverage

The review follows the installed `axiomatic-fundamentalism-review` workflow,
`axiomatic-fundamentalism` inference discipline, and `aod` authority/manuscript
guidance including the preserved local Main/Manual amendment. The repository's
`docs/SKILLS.md` reconciles the pinned `rpoyau/aod-skills` revision
`630af16a7c51fb8836e603eb54c67bf8560fb463` with that amendment. No Main/Manual
source, compiler, runtime, or imported producer was repaired by this review.

The earlier focused source review supplies the inherited semantic premises:
`SOURCES.md` SHA-256
`68417c36275acd0fee522c97a0f10ec46b7a641d6ec4696d49cc0e37cd7f79f4`;
`source-binding.json` SHA-256
`54ad1b9ea5c29874348a96f8a1edf4853541924d944cf1d17a3df139476b32ad`.
That record binds the delivered Manual I candidate and verified Matter I
reconstruction; neither is silently promoted to a current accepted release.
This pass does not repeat or enlarge its whole-manuscript coverage.

Fully read: the three new simulation modules, their three focused test modules,
and the generated program's model/return construction (all steps were also
checked by exact regeneration and actual VM execution). Reviewed obligations:
transition operands, exact probabilities, event windows, conditional sampling,
whole-return admission, retained identity, replay validation, and model limits.
The runner, plots, integration documentation, native backend, physical mappings,
and full upstream manuscripts are outside this review. The runtime/producer
hashes bind dependencies; they do not imply a new whole-file verification.

| Path relative to `aod-dec/` | SHA-256 |
|---|---|
| `simulations/chiral_collision.py` | `a1e307047d2dfd1bfe248a57d7b54169ac5f910cb41671540177dfaa5681fe62` |
| `simulations/chiral_reference.py` | `cf7032f01c702e18569b417a1c4785eada592640ba267363a4a941842a573a9b` |
| `simulations/chiral_dec.py` | `2edda722f05fc738b7f5ee9010df35933e4e41fba452181a14041c8378531bff` |
| `tests/test_chiral_collision.py` | `23444c72f4379d95a502285f063c5c966c9cb314bc31ddcc9eb4bb7b630e6743` |
| `tests/test_chiral_reference.py` | `c69acdf5e30485fb7882cc2342afc435c22d8730ef786184709824f408e0944a` |
| `tests/test_chiral_dec.py` | `d26765b313bbb7496b004aedc9c4e56099971084f682d17f9734b4d9f3186556` |
| `examples/chiral_core_collision.dec` | `4691f8b0c67d7a14b6c8c9491dd2e7fa8853e0ec05c5a539357615993d408da0` |
| `simulations/tesseract_cells.py` | `0d8a15ba047e5c1cd17b36977815cd19fb5928426b9e8ae3b6c35053362b6b1a` |
| `simulations/vendor/aod_manual_native.py` | `3ba98c7097bc104aeb6218265b8531f94f2ca5a35239d0df097aba096675dfdf` |
| `runtime/dec_core.py` | `7af9cf5ca99ec6b0aff6074b9d606e1339761715d3608d504fa0a742e2bafa3a` |
| `runtime/dec_language.py` | `900cfd50f134ee525ee05e0a3369f2fbdf817801b3528383856ada6af3037456` |

## Claim register and dependency checks

**I01 — Complete-state Markov law: conditional, supported.**
`State`, `_integer_kernel`, `flip_probability` and `simulate_path` use the two
facet positions and two chirality signs. The population profile and probability
parameters are frozen configuration. Each macrostep samples independent facet
moves, then the two sign choices conditional on the destination pair. Thus the
declared mathematical law is Markov on the 256 states. The deterministic seeded
PRNG implements reproducible sampling; it is not itself a proof of statistical
independence across pseudorandom streams. Retained return logs do not enter the
probability function. Adding history-dependent RCD, pressure or contact feedback
later would require adding the relevant operands to the state.

**I02 — Chirality-only projection: counterexample to a general Markov quotient.**
With gain zero and identical signs, compare states
`('*0**','0***',+1,-1)` and `('*1**','0***',+1,-1)`. A starts in the core in the
first and its opposite facet in the second. Exact successor marginalization
gives next-A-flip probabilities `7/64` and `1/16`. The first can hold in the core
with probability `1/4`; the second cannot reach it in one step. These states
share the same projected chirality but different projected transition laws.
The full-state implementation retains the required distinction. This does not
assert that every specially initialized chirality process must be non-Markov.

**I03 — Probability/reward calculation: supported finite exact checks.**
The reference imports only facet incidence, not engine successors, sampling, or
distribution functions. It constructs integer movement weights and all four
sign outcomes independently. Joint integer recursion, a separate position-only
recursion, and complement recursions respectively give terminal law, exposure,
and first-event probabilities. Every row and propagated mass normalizes exactly.
The comparison below covers the common single-core domain; the engine's optional
intermediate populations and reversed-bias controls are outside this independent
reference's domain. The latter rejects `q_core < q_outer` explicitly.

For primary `contact_gain=0`, initial `('0***','1***',+1,-1)` and eight steps,
each carrier has core occupancy `1/8` on every positive step. Initial symmetry
among the six facets on the other axes gives `1/8` at step one; equal core and
opposite-core probabilities `1/8` remain invariant under the declared movement
row. Hence sixteen opportunities give `16/8=2` core and `14` outer opportunities.
Multiplying by the respective conditional flip probabilities yields the exact
results independently checked against the recursion:

| Quantity | Exact value |
|---|---:|
| Expected core opportunities | `2` |
| Expected outer opportunities | `14` |
| Expected core flips | `1/2` |
| Expected outer flips | `7/8` |
| Expected total flips | `11/8` |

All 256 terminal probabilities matched the host integer DP for both gain zero
and the separately chosen gain `1/4`. The latter total is
`499289948161/274877906944`. Count expectation, probability of any flip, and
terminal odd-parity probability remain separate outputs. Fixed-q controls use
`n*q`, `1-(1-q)^n`, and `[1-(1-2*q)^n]/2`; q=0 and q=1 test their degenerate
cases. First-event windows exclude the initial state, including at zero steps.

**I04 — Whole-return history and identity: supported within one sampled run.**
Each selected handedness creates a complete admitted return with outbound role,
structural hinge, and opposite return role. The host constructs and certifies
every return in both full and compact retention modes; compact mode omits
payloads only after construction. Final recurrence checks preserve carrier
identity and the two initial formation returns. Continuations contain the prior
return ID, a distinct return ID, digest and selected witness. Coarse facet motion
is separately recorded through its actual shared-square interface; it is not
identified with the short Q4 closure realization.

`path_id` binds the whole configuration, seed, initial state and ticket grid.
Different counterfactual runs therefore get distinct continuation namespaces
while retaining shared initial carrier identity. Because configuration includes
the horizon, separate simulations with different horizons also have different
continuation IDs. This implementation does not provide a cross-horizon append
API or claim prefix identity between those separately namespaced experiments.

**I05 — Replay, exact types and adversarial inputs: supported tested scope.**
Input probabilities are exact integers/Fractions in `[0,1]`; booleans and binary
floats are rejected. Counts, signs, step limits, seed type and common ticket-grid
divisibility are checked. `validate_path` reconstructs the complete declared
experiment and compares all fields recursively with exact types, avoiding
Python's `True == 1` alias. Tested corruptions include witness predecessor,
probability, Boolean ordinal, string probability, ticket domain, denominator,
movement Boolean, changed sign and missing step. All were rejected.
Separate PRNG streams preserve movement under changed flip laws. Paired flip
comparisons additionally require the same explicit ticket denominator; the tests
supply it. The recorded transition probability is the state-transition weight
(including both sign outcomes), not the probability of a specific raw ticket.

**I06 — Generated D.E.C. computation: supported deterministic replay only.**
The generator validates the host path, then supplies destinations and exact
tickets as inputs. The program computes local/contact probabilities, threshold
decisions, signs, and complete handed return continuations. It does not perform
native random movement sampling or derive the full terminal distribution.
The checked-in example exactly equals regeneration for gain zero, seed 7 and
ticket denominator 64. VM execution produced 64 exact scalar lines, two traces,
ten complete returns and ten hinges per trace, and result zero. Removing hinges
fails actual VM execution. Corrupt output histories, orientations, lineages,
reused certificate IDs, Boolean ordinals and numeric prefixes are rejected.

`validate_native_output` validates event structure, ranges and unique digest
format; it does not recompute arbitrary external JSON certificate digests.
Consequently its PASS is not independent authentication of foreign transcripts.
The compiler/runtime's certificate checks and source-bound execution evidence
remain separate obligations, as its docstring states. Native-machine execution
is outside this review and must be established by the integration runner.

**I07 — Population, contact and physical mapping: finite claim supported;
physical applicability missing premises.**
The declared profile has eleven distinct admitted background waves, separate
from two moving carriers. Core is a selected population/support tag. A symmetric
tesseract shell supplies no privileged metric centre. Population count is not
density without a measure, and neither is duration-valued RCD. `background_record`
leaves density and cell measure unset. Co-residence is marked contact candidate;
coupling/fusion are not admitted by it. Physical proton correspondence and the
SI clock/energy/momentum map are explicitly uninstantiated. No scattering rate,
cross section, collision energy, empirical accuracy or calibrated proton claim
follows from this exact finite-model pass.

## Executed checks and remaining scope

Environment: Python `3.15.2`. Commands run from `aod-dec/`:

```text
python3 -m unittest tests.test_chiral_collision tests.test_chiral_reference tests.test_chiral_dec
python3 -O -m unittest tests.test_chiral_collision tests.test_chiral_reference tests.test_chiral_dec
```

Both commands passed all 27 tests (17.215 s and 17.425 s respectively). Additional
executed exact assertions compared every terminal probability for gains 0 and
1/4, checked the five primary expected totals above, verified per-step core
exposure `1/4` across both carriers, and constructed the I02 counterexample.
Regeneration equality and VM execution of the actual checked-in example passed.
No failed scientific comparison was discarded or converted to a physical claim.

Remaining integration obligations: retain the actual native execution results,
sampling diagnostics without confidence/accuracy promotion, and documentation
consistent with the replay and physical-map limits above. This bounded review
does not certify the runner, figures, full test suite, native backend, upstream
ontology, or a proton–proton experiment. No human review/approval is inferred.

## Owner clarification: probabilistic model and execution engine

The subsequent owner instruction, “careful probabilistics model is not the
engine,” fixes the terminology and interpretation of this application. A focused
follow-up audit inspected the application imports, generated-program helpers,
runner execution interface, relevant README/evidence prose, and the companion's
model/execution passages. This is a naming and dependency audit, not a new
whole-document or runner correctness review. At that audit's checkpoint the
implementation hashes above remained unchanged; no source repair was applied
by this reviewer. The later correction is bound in the final addendum below.

**I08 — Model/execution separation: supported; wording clarification resolved.**

| Layer | Declared responsibility |
|---|---|
| Probabilistic application model | Facet movement weights, population/support profile, local and optional contact probabilities, conditional factorization, state and whole-return recipe selection |
| Host model evaluator/sampler | `simulations/chiral_collision.py` evaluates that model using exact recursion or seeded tickets; `chiral_reference.py` independently calculates its consequences |
| D.E.C. execution engine | The unchanged compiler, VM and native runtime provide generic checked arithmetic, control flow, trace operations and witness validation |

The application generator supplies the `q_local` and `q_contact` function bodies
as D.E.C. program source, with configuration values as literals. It supplies
host-sampled motion/tickets as the replay input. The language engine executes
those expressions and selected complete returns; it neither invents nor embeds
this collision probability law. The existing generic `probability` operation
normalizes supplied weights and does not sample or add a chirality transition
rule. Changes to the declared q expressions or parameters are application-model
changes, not changes to compiler/runtime semantics.

Executed dependency checks found no working-tree changes under `aodc.py`,
`runtime/` or `native/`, and no occurrence there of `chiral`, `CollisionConfig`,
`q_core`, `q_outer`, `population_counts` or `contact_gain`. Application imports
point toward the topology/producer and generic execution facilities; the
compiler/runtime do not import the chiral application. The generator and runner
retain that separation. No substantive conflation or interface defect was found
in this bounded inspection.

The companion states the distinction explicitly. At the initial naming audit,
some names within this new application used “engine” to mean the host
implementation, obscuring the owner's intended boundary. The recommendations
recorded at that checkpoint were:

- `sampled engine` / `sampled-engine` → `host model evaluator` or `host sampler`,
  according to the referenced function;
- generated comment `Host engine source SHA-256` →
  `Host model evaluator source SHA-256`;
- report key `engine_reference_comparison` →
  `host_reference_comparison` when the output-schema change is intentionally
  propagated, or explicitly document the present key as comparing two
  application-model evaluators.

These are naming improvements, not evidence that the probability law has entered
the language engine. Renaming hash-bound program comments or report fields
requires regenerating the affected artifacts; this review did not modify a
running study or silently rewrite its provenance.

## Final source-binding addendum: I08 resolved

The integrator applied the terminology and metadata corrections after the
initial audit. A final targeted read verified that the companion now says
“host model evaluator,” the generated provenance comment says “Host model
evaluator source SHA-256,” and the independent reference describes the sampled
model/evaluator rather than a language engine. The runner now uses
`model_reference_comparison` and includes `model_execution_boundary` metadata
that separately identifies the application probability law, host evaluation,
and unchanged D.E.C. execution engine. The canonical D.E.C. example was
regenerated with the corrected comment and generator hash.

These corrections affect prose, comments, provenance hashes and report metadata;
they do not change the probability law, sampler, exact recursion, return
construction or compiler/runtime semantics. The host model evaluator source
remains exactly
`a1e307047d2dfd1bfe248a57d7b54169ac5f910cb41671540177dfaa5681fe62`.
The old hashes above remain the historical bindings of the executed review
checks. The following hashes bind the corrected final sources; they supersede
the corresponding old bindings for current naming/provenance review.

| Corrected source | Final SHA-256 |
|---|---|
| `simulations/chiral_reference.py` | `b7ecbc0acf0bb6a5cf22d813ceb14903e2e950b70e74ff715282f0ac6099b2d5` |
| `simulations/chiral_dec.py` | `32d253536cffef0f023afa67bd4ec97b75c6d3047b3b53f1a59bc0777f5377d0` |
| `examples/chiral_core_collision.dec` | `d68374983626e03e762e94ed3154fa53387ef5fcdd3f7a8e3e334b03ba42acc4` |
| `scripts/simulate_chiral_collision.py` | `16f58d06be01424915ab2ff8153a5189a59aaa24fbafde837d1a9c8396e050a7` |
| `docs/chiral-core-collision.typ` | `ed95a1dfcdb440547ad8896c35fe05e1d94360189b043e1a5ff47d394b9fe89a` |

I08 is resolved: both the implementation boundary and its explicit names now
distinguish model from engine. This addendum does not invent a new test result
or expand the original bounded review to the complete runner/document. The
release study supplies its own regenerated artifact and execution bindings.
