# Source review: reusable region, shell and interface currents

The bound Main explicitly supplies a common scoped-current calculation for nested field-fluid participants. The Manual explicitly applies it to wire, fluid-interface and plasma-shell cases. A reusable region/interface implementation is therefore supported at the calculation layer. Its operands, admitted supports, observation maps and realized later response remain part of each application's declaration.

This is a read-only, source-bound review. No Main, Manual, compiler or prior delivery was changed. No simulation or test suite was executed by this review. The recommendations below are implementation obligations and proposed finite falsifiers, not reported test outcomes.

## Binding and ownership

The source is the actual R04 candidate under `AOD_DEC_R04_IMPLEMENTATION_2026-11-14/upstream`, with Git base `8c3149b9d3d924b80b9385b178da4ba2c39175e8`; the byte hashes bind its working candidate rather than claiming it equals the accepted remote head. The downstream base is `8144803e7e889087dfd1e9322c778fdc276c6f32`. The earlier strict component is `AOD_DEC_BOUNDARY_ACCOUNTING_2026-11-14/application/boundary_accounting.py`.

The additional Manual II source is the dated local publication candidate `AOD_M3_FEEDBACK_IMPLEMENTATION_2026-11-10/build-publication-final/manual2`, inspected only for current application context and not substituted for the R04 Main/Manual. The bounded text search covers Main sections/appendices, Manual sections/appendices/scripts/data, and that Manual II candidate's active sections/appendices/scripts. It excludes Manual II evidence copies and vendored older manuscripts. `source-search.json` records searched files and actual hits. Neither `upstream/manual2` nor a workspace-root `manual2` exists; those failed initial locations are not a coverage claim.

The current installed AOD skill and its preserved local amendment govern ownership: Main owns general/scoped mathematical definitions and derivations; Manual/application owns implementation, prepared fixtures, calibration and evaluated outputs. AF supplies inference discipline and the persistent AF review workflow supplies source-bound coverage. AFC is relevant only to the explicitly declared finite antisymmetric-current identity; no new normalized probability kernel is inferred. Skills and selected source bytes are bound in `source-hashes.json`.

## Common calculation and region algebra

`upstream/sections/06_field.tex:10–62` separates normalized AFC memory/kernel flux from pressure-bearing AOD flux. The finite region is `R ⊆ V`; its oriented boundary is pairs `(i,j)` with `i ∈ R, j ∉ R`. For either declared antisymmetric flux, internal ordered pairs cancel and the regional divergence sum equals boundary flux (`eq:afc-stokes-used`). The cancelled scalar does not erase the contributing lineage records. A separately constructed reverse channel may have different directional RCD and magnitude (`eq:flux-types`); it must not be mistaken for the sign reversal of one fixed occurrence.

At `06_field.tex:75–144`, the owner defines:

- `eq:path-history-pressure`: sum `C3_h rho_omega,h` over retained path histories.
- `eq:path-history-compression`: a separate declared compression is required before substituting one channel tuple for the whole family.
- `eq:b-scoped-channel-pressure`: `pD = C3 min(rhoD, window)` under that compression.
- `eq:b-scoped-channel-attention`: `A = lag × intensity`.
- `eq:b-scoped-flux`: `phi = pD × A`.
- `eq:b-scoped-flux-antisymmetry`: opposite bookkeeping orientation of the fixed occurrence changes its sign.
- `eq:b-scoped-stokes`: regional divergence equals the boundary sum.

Consequently, union, intersection and difference can be represented by membership in one shared finite cell universe. For a fixed occurrence family and compatible context, the linear boundary functional obeys inclusion–exclusion:

`J(A ∪ B) = J(A) + J(B) − J(A ∩ B)`.

For `A ⊆ B`, the shell `B \ A` obeys `J(B \ A) = J(B) − J(A)`. A transfer from core to shell is internal to their union. A channel from shell to Moon is internal only to the union that actually includes both endpoints. This follows by the indicator identity on the fixed antisymmetric family; it does not license adding records from different cuts, windows, count domains, scale maps or independently recomputed directional channels.

The reusable implementation should derive boundary membership and oriented views from the full declared occurrence family. Requiring a caller to manually list a purportedly complete crossing family and separately attest to that same list does not establish completeness. The earlier strict component checks the supplied manifest exactly but depends on a trusted producer for that manifest. Region compilation is the appropriate layer to supply it from retained endpoints.

## Moving boundaries, retained status and scales

Main `06_field.tex:173–237` defines slice flux, the window sum, raw capacity crossing count, magnitude current, its declared in/out/pending partition, and

`moving current = crossing − sweep + shedding` (`eq:moving-boundary`).

The regional template at `:239–266` covers retained core, exoshedding, reflected return and pending return. These are provenance/status distinctions, not four independently additive observations of every occurrence. A region change needs a declared cut convention, the old and new membership, and the support swept by that change. Omitting sweep is not justified by retaining more history.

For additive moving-region comparisons, all sweep values must arise from a common signed content representation or an explicitly compatible supplied map. Three independent scalar sweep choices do not imply union cancellation. The engine should expose this binding rather than silently assume it. The same requirement applies to signed shedding-current projection: a positive closure-burden route is not automatically an oriented current amount.

Main `06_field.tex:611–661` distinguishes closure residual, positive surplus and declared route placement (`eq:closure-residual`, `eq:shedding-excess`, `eq:sheddic-path`, `eq:shedding-route-partition`). Endoshedding, exoshedding, redirected support and open loss partition the parent surplus after the route is declared. Negative residual is deficit, not positive shedding. Exoshedding lift and higher-scope admission at `:880–913` retain the lower support and require the later admitted object; a changed layer name is insufficient.

The field-fluid section at `06_field.tex:1113–1182` explicitly includes retained field, current-cycle, background-continuation and shell-support participants on a declared moving/nested cell boundary. `C_J`, the current-cycle participant, is distinct from accumulated current `J^D`. Candidate/support/capacity/window operands bound retained field count; actual recurrence admission is still required. A gas/fluid label is a scoped declaration, not a physical equation of state supplied by its spelling.

Nested inter-layer coupling is at `:1184–1207`, capacity versus closure and exoshedding lift at `:1208–1214`, and the finite moving-boundary dictionary at `:1232–1237`. The latter applies the same slice/window formula to finite SADAR values and retains sweep/shedding. The finite fractal-cycle accumulation at `:1349–1370` sums supplied certificate-bound contributions and their declared couplings; the source explicitly distinguishes it from continuous integration. The unifying statement is supported for the declared AOD calculus, not a proof that arbitrary continuum fluid dynamics follows from boundary algebra alone.

## Exact Manual values suitable for shared-engine fixtures

Manual `sections/04_boundary_current_applications.tex:16–58` gives the common operational formulas and `:90–142` fixes one canonical numerical substitution:

| Operand/output | Exact value |
|---|---:|
| Capacity `C3` | 3 |
| Directional duration `rhoD` | 5 |
| Participation window | 4 |
| Attention `A` | 1/3 |
| Clipped participation | 4 |
| Pressure | 12 |
| Fixed channel current | 4 |
| Reverse view of that channel | −4 |
| Crossing, sweep, shedding | 8, 3, 2 |
| Moving-boundary current | 7 |

The source classes these as D0 exact scoped fixtures. At `:169–189`, the fluid-interface application supplies a water-region boundary; sweep is displaced current from boundary motion and shedding is excess crossing or rerouting through that interface. At `:191–200`, the plasma-shell application explicitly uses the same calculation by changing the declared boundary. At `:215–230`, phase-fluid motion is the realized later state pair, not just its attention vector.

A useful Earth/water-shell/Moon accounting fixture can instantiate those exact values on distinct named cell sets and test core/shell/union/Moon boundaries. Those names are new application placement choices, not a calibrated Earth tide model. The Manual registry already contains Earth–Moon orbital shell (`sections/09_prediction_test_fixture_registry.tex:47`) as O0 target data requiring numeric AOD distance, speed and period records. No executed water-shell/tide preparation was found in the bounded active-source search. This preserves the actual existing correspondence/target entry without presenting its required output as already evaluated.

## Maser source, transport and receiver boundaries

The Manual's maser treatment already has useful accounting interfaces. `sections/03_m3_temporal.tex:170–200` defines observed phase advance as signed winding plus the fractional endpoint difference. It also gives the passive, lossless transport identity

`emitted − arrived = pending_end − pending_start`.

Packet ancestry is therefore retained through an observation cut; delayed arrival is not a missing emission and cannot be recounted at each shell as a new source event. Loss and detector inefficiency need their own counts. The finite phase example on `(10,22]` has compared winding 4 and endpoints `2/3 → 1/6`, giving `7/2`; reference winding 8 and endpoints `1/4 → 3/4` gives `17/2`. A split at `5/6` gives `13/6 + 4/3 = 7/2`. The conditional reference ratio is `7/17`. These are phase records in turns, not pressure-current values renamed as turns.

The actual hydrogen receiver convention and printed-input reduction are at `03_m3_temporal.tex:204–222` (`eq:manual-m3-hydrogen-reduction`). The reference multiple is 284, the nominal reference is 5 MHz, and signed beat/correction ownership is explicit. Table VI's input already contains NP3 and wall corrections, so adding them again is a concrete double-counting error. The printed-input mean `1420405751.7679 Hz` and published rounded `1420405751.768 Hz` belong to that measured-input reduction, not to an engine-generated emission result. Current work can preserve that existing comparison interface without supplying new fitted transition probabilities.

`manual/data/m3/temporal_metrology/hydrogen/protocol.json` identifies the actual hyperfine transition and apparatus records. It explicitly leaves apparatus-specific post-selector populations, selector geometry, source distribution, velocity-dependent preparation probabilities and microscopic cavity/emission kernel unbound. This limits a physical maser point prediction, not the validity or usefulness of boundary transport accounting.

The dated Manual II application `sections/11-h1-prepared-current-application.tex` carries actual prepared histories, bound gate/cross-return paths, native burden 27 and closure-cost alternatives 26/27/28 giving surplus 1/0/0. It exhibits an emitted-phase ambiguity: identical fractional endpoints and packet ancestry permit windings `m` and `m+1` without a supplied emitted-phase law. A reference frequency cannot select that law. This is relevant inherited scope: the regional engine must not make packet transport or current cancellation choose a maser oscillation phase automatically.

## Existing APIs and recommended falsifiers

The bound native producer `manual/scripts/aod_native_closure_wave.py` supplies `aod_flux` at line 811, taking `(i,j,pressure,attention,lineage)` and retaining the two opposite bookkeeping views, and `stokes` at line 826. These are exact arithmetic APIs; supplying one row alone does not certify its physical preparation. `update` at line 594 uses the declared positive-pressure response `current + attention/pressure`, committed one step later. Its supplied update rule should remain distinct from boundary selection.

The metrology producer `manual/scripts/aod_m3_metrology.py` supplies `heterodyne_advance` at line 287, validating a shared observation coordinate/epoch/window/orientation, and `validate_transport_ledger` at line 293. The latter binds unique packet IDs, emission/arrival cuts, pending endpoint counts and the transport identity. `compare_accumulations` at line 259 requires compatible phase records and an independently declared reference realization. Pressure, packet count, phase turns and SI frequency are distinct value domains.

The prior strict component supplies source-bound records, immutable archives, disposition changes, exact boundary slices, accumulated current and route receipts. A reusable region front end should preserve those checks rather than replace them with precomputed sums. Proposed meaningful falsifiers are:

1. Drop a retained channel that crosses a compiled boundary; reconstructing from full endpoints must expose it.
2. Supply both orientations of one occurrence as independent physical contributions; reject the duplicate.
3. Sum core and enclosing-body currents as disjoint regions; require a partition or inclusion–exclusion.
4. Change cell membership without a compatible sweep record, or change the schedule between successive cuts; reject.
5. Add inner and lifted outer representations of the same additive support; reject overlap.
6. Disable coupling and then restore it; active input changes but old archive events and later state survive.
7. Change source/receiver scope, phase window, unit or reference identity; reject comparison before a value is emitted.
8. Drop an emitted-but-not-arrived packet at a cut; the pending identity must fail. Reuse one receipt; reject it.
9. Treat closure burden as signed shedding current without a bound projection; retain the unresolved obligation.
10. For one fixed family, verify empty/whole regions, complementary signs, shell difference, disjoint union and overlap inclusion–exclusion with independent exact expected values.

Source conclusions are supported within the stated coverage. The new regional implementation and its tests are outside this review until separately inspected; no passing-test or completed-physics claim follows from this source review.
