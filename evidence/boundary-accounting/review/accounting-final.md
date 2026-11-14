# Final bounded accounting and native-adapter review

**PASS for the inspected finite accounting and integration contract.** Four
independently reproduced defects are resolved against component SHA256
`7bdf7a33664bf275debecf51a53c8768d18915102739d9715a6e4e6db4def88f`.
The native replay adapter is bound at
`fb233301281ac039d4d5507a9b234fe3e24ee4e05266007b5e06a468abd6193b`.
Exact file bindings are in `accounting-final-hashes.json`.

This review inspected the actual component, adapter and bound native
interfaces. It independently reran only the four prior falsifiers and a valid
continuous-boundary control, in normal and optimized Python. It read the
stored native integration output and its source bindings; it did not replay
that native simulation or rerun the complete implementation test suite. No
production source was edited by the reviewer.

## Observed failures and verified repairs

| ID | Earlier observed failure | Final independent result |
|---|---|---|
| C1 | Two stationary slices silently changed region A to region B under the same boundary identity, with zero sweep. | Rejected: `disconnected boundary motion schedule`. |
| C2 | Replacing slice totals with binary float 0.5 passed the accumulator. | Rejected: `slice differs from recomputed exact source accounting`. |
| C3 | Two aliases of one physical/additive surplus each supplied a separate route, doubling pending amount. | Rejected: `physical/additive surplus source routed more than once`. |
| C4 | Two successive slices reused one additive occurrence under different physical-ID aliases. | Rejected: `additive support occurrence repeated across slices`. |

All four rejections occurred in normal Python and `-O`. The same valid
continuous stationary A→A sequence still yields current 3 and sweep 0. The
original three adversarial constructions changed only by supplying the newly
required `BoundaryCoverage`; C4 was replayed with the same conflicting
additive support. Results and commands are retained in
`accounting-repair-normal.json` and `accounting-repair-optimized.json`.

The original failing source, probe and results remain in
`pre-repair-boundary_accounting.py`, `accounting_counterexamples.py` and
`accounting-counterexamples.json`. C4's intermediate-hash finding remains in
its separate record. Those historical accepted counterexamples have not been
rewritten as passes; the resolved disposition applies to the new hash above.

The repaired accumulator reconstructs each slice from its bound immutable
archive, motion, maps and coverage, checks exact Fraction totals, then checks
region continuity, archive-prefix preservation, compatible coverage scope/
domain and uniqueness of both physical and additive occurrences across cuts.
Route ledgers now retain source archives and reject duplicate physical or
additive surplus consumption. The new coverage interface requires selected
crossing/shedding identities to equal the declared expected identities. The
review inspected that coverage guard; this narrow rerun did not add a fifth
independent omission test.

## Source admission and the native bridge

`native_accounting_replay.py` calls the existing
`manual_dec_fixture.make_population`, `bound_contacts` and `channel` functions.
The native `compare` performs wave, repeated-contact, scope/scale, prefix,
event-union and commitment checks before the channel value is projected.
Thus this fixture's native operands have actual source admission; an opaque
SHA string in a generic `ContentRecord` would not itself perform that admission.

The fixed source interval is `(0,22]`. RCD durations are 5 and 7, capacities 3
and 2, and signed attentions `1/3` and `−1/2`. The source wave/contact history
is preserved, while each new projected contribution receives a distinct
per-cut physical ID and additive support ID. Repeated projection is not
reported as a fresh native return or extra closure.

The first adapter draft checked a separately recomputed native sum while
leaving the accounting output outside the response path. The inspected final
draft repairs that gap: it checks crossing and moving totals against the
native sum, checks zero sweep/shedding for this stationary case, passes
`result.moving_total` into `native.update`, and requires total current 3.
The response bridge is explicitly the existing
`manual_dec_fixture.history_reduction_check` choice
`next_current=current+channel_flux/pressure`, here at pressure one.
It is not an unannounced identity of current and pressure or a newly supplied
stochastic transition law.

The stored output `outputs/native-accounting.json`, SHA256
`732617c2074fce8942f15e7a2e57720723d2bad33ec371b7a5f4b346944872e4`,
binds both the reviewed adapter and repaired component. Its rows are:

| Input cut | Response cut | Participation count | CD available | Slice current | Later current |
|---:|---:|---:|---|---:|---:|
| 22 | 23 | 4 | yes | 0 | 0 |
| 23 | 24 | 4 | no | 4 | 4 |
| 24 | 25 | 4 | yes | 0 | 4 |
| 25 | 26 | 6 | yes | −1 | 3 |

These values follow exactly from `min(5,w)` and `−min(7,w)` for available
channels, and the chosen pressure-one response. The response is strictly later
than its input cut. The 22–25 current sum is 3; zero signed current at cuts 22
and 24 does not erase either retained channel.

## Window, disposition and scope interpretation

CD inactivity here is an explicit availability input. It removes the channel's
current contribution and changes its active disposition while preserving its
full retained record. It is not structural removal of a Main08 retained
rebalancing coordinate and therefore is not, by itself, an operativity proof.
Archive presence never automatically activates a contribution; old per-cut
projection occurrences are explicitly made inactive.

Because participation drives the validated native response, the change from
window count 4 to 6 is an **explicit response-model input schedule** in this
fixture. It must not be described as passive observation changing the
underlying execution. The earlier fixed-history window probe demonstrates
observation-only projection separately. The response schedule remains a
declared application choice.

Participation is in native-return-count units. TTL=3 and retention-window=2
are separate fixed native-committed-cut gate operands. They pass a compatible
retention comparison but do not execute a remaining-TTL countdown or an expiry
process. Their fresh supply at every cut must not be interpreted as observed
renewal. This distinction leaves the original RD/RCD and history intact.

The generic hierarchy requires earlier lower-scale parent records and
preserves their additive support. A slice cannot sum overlapping inner/outer
representations, and differing scales require an explicit positive
representation map in the same quantity domain. These are accounting
conditions. `BoundaryCoverage.scope` and source digests bind the application's
declared context; they do not derive a native fold, cross-scale contact,
Duon grammar or physical unit conversion. Actual native construction/admission
remains with the producer/adapter, as in the separate awareness carriage
fixture. The generic component is parameterized by finite scale; it is not an
execution of every physical scale.

The inspected native replay uses a stationary boundary. Moving sweep and
route/receipt behavior are separate accounting cases. The component explicitly
leaves stock balance `NOT_ASSERTED`: its crossing-minus-sweep-plus-shedding
identity does not silently supply a stock-difference bridge, geometry generator
or receiving-wave update. No such broader execution is inferred by this review.

## Final coverage

The four demonstrated defects are **resolved at the bound final component**.
The native adapter's source admission, causal cuts, distinct projection IDs,
explicit availability, exact expectations and now-active accounting-to-response
bridge are consistent with the declared fixture. The scope qualifications
above are part of the result, not new implementation failures. No further
testing was performed after these bounded checks passed.
