# Particle and wire examples: actual source contract

The bound Manual supplies electron and proton support maps, measured
rest-energy targets, exact support/window accessor rows, a wire boundary-current
scope and exact current/moving-boundary examples. These are usable ingredients
for the requested examples. The new paired preparation, placement, contact
schedule and receiving response must be identified as application choices;
the supplied maps alone do not specify those trajectories.

This review reads the actual frozen source at
`AOD_DEC_R04_IMPLEMENTATION_2026-11-14/upstream`, git base
`8c3149b9d3d924b80b9385b178da4ba2c39175e8`, plus the existing downstream native
support adapter. It does not rely on older generic `NOT_INSTANTIATED` notes.
No source, prior evidence or implementation was edited, and no simulation or
test was run. Exact source/guidance bindings are in `source-hashes.json`.

## Available preparations, equations and comparisons

| Ingredient | Exact bound source | Available content |
|---|---|---|
| Electron map | Manual `01_rest_energy_prediction.tex:223–245` | `e− → 00|- Dimonhexon ≡ 1:2:6`, charge orientation `q_ext=−1`, external charge `Q_ext=−e`. |
| Proton map | Same file `:194–211` | `p+ → Tritriohexon`, support `3:3:6`, carried field package `C_f[3:3]_AO`. |
| Electron accessor row | Same file `:160–188` | Under the stated neutral-reflection/single-hinge reduction: `Π=4`, `H=1`, `RD=9`, window 6, participation 6, pressure 18, pending burden 9, shedding 0. |
| Proton accessor row | Same table | `Π=30`, `H=1`, `RD=61`, window 6, participation 6, pressure 36, pending burden 330, shedding 0. |
| Measured targets | Same file `:250–293` | Electron `0.51099895069 MeV`, target uncertainty `1.6×10⁻¹⁰ MeV`; proton `938.27208943 MeV`, target uncertainty `2.9×10⁻⁷ MeV`. |
| Wire scope | Manual `04_boundary_current_applications.tex:145–167` | Conductor plus retained field shell, conductor/core region, shell boundary and declared window; compute slice/window current, pressure, SADAR attention and shedding. |
| Canonical channel | Same file `:91–143` | `C3=3`, duration 5, compatible window 4, attention `1/3`: participation 4, pressure 12, flux 4 and bookkeeping reverse −4. |
| Moving boundary | Same file `:91–106`; Main `06_field.tex:225–237` | Crossing 8, sweep 3, shedding 2: `8−3+2=7`. Inputs and their signed roles are explicit. |
| SI/report maps | Manual Appendix B `B_quarantined_external_comparison.tex:32–108` | Declared bip duration permits `d_ext=c n Δt_bip`; integer `Δq/Δn` permits `v_ext=c Δq/Δn`; orientation gives `Q_ext=z e`; candidate rate gives energy via K. |

Current Main Appendix J is the field-support/accessor theory. The material-name
mapping table is in the Manual rest-energy section, and the SI comparison maps
are in Manual Appendix B. The source registry also explicitly lists electron
and proton O0 rows (`09_prediction_test_fixture_registry.tex:34–38`). Their
existence should not be replaced by a blanket statement that no particle
correspondence is supplied.

The accessor reduction is conditional: `RD=Π⁻+H+Π⁺`, specialized to `2Π+1`
when branches are equal and the hinge is unit. Neutral reflection gives
duration equal to RD in these rows; general directional contact RCD is a
different retained object. A complete return already contains both legs and
counts as one closure. The enclosure label, atomic trace length, motif support
accessor, contact duration and external energy remain distinct coordinates.

## Proton–proton preparation and accounting

Main `05_curl_closure_duon_current.tex:94–121`, `eq:motif-admission`, requires
identity, scope, lineage, component construction, trace and certificate. Names,
enclosure, chirality and RD cannot supply missing formation. For `3:q`, the
threefold core and q composition retain their construction provenance.

The actual native constructor
`manual/scripts/aod_native_closure_wave.py:337–380` supports `3:3` through three
ordered blocks of three distinct component occurrence IDs plus retained shell
IDs. Components precede the target construction and share scope, lineage and
scale. It also distinguishes actual Duon grammar—two Dion patterns and a bound
Monon pivot—from a generic two-edge closure.

The existing downstream `simulations/collision_routing.py:_carrier`
(`:106–155`) can materialize a supplied admitted orientation/history as
separately constructed `3:3` supports. It records every source return,
construction start, three prior component certificates, materialized return and
motif witness. This is a usable **declared preparation projection**, not a
deduction that a `1:2` wave intrinsically becomes a proton. The new example can
adopt the Manual's `p+→3:3:6` comparison map explicitly and preserve that
preparation provenance.

Two precise qualifications are necessary when reusing this helper:

1. Its old `physical_proton_correspondence=NOT_INSTANTIATED` field describes
   the helper's original execution context. It does not overrule the actual
   Manual map. The new report should state which Manual map it adopts and
   which incoming preparation or measured observable it supplies, rather than
   propagate the old flag as a universal absence claim.
2. Its target trace uses the native default two-edge `(0,1,0)` support while
   its motif record carries `enclosure=6`. It does not construct a six-edge
   target trace. The source `retained_continuation` C6 trace geometry is not
   automatically inherited by an orientation projection. If a new example
   claims an actual six-edge target realization, that realization must be
   explicitly constructed and admitted.

An executable paired example may therefore declare two admitted mapped proton
preparations, their actual incoming orientation/contact schedule and the selected
boundary. Reconstruct directional RCD from the bound repeated contacts. Apply
the Main temporal burden `Σ C3 ρ_window |lag|` at that channel scope, separately
declare its closure cost and route partition, and bind the resulting surplus
record and any returned Duon support to the receiving event. The proton's
motif RD=61 must not silently replace a contact support accessor of 3.

The two mapped rest energies may be stated as supplied preparation/reference
data. Their sum is not an incoming collision energy unless the preparation
actually makes that choice; it does not supply a scattering angle, energy
spectrum or cross-section target. This distinction permits the requested
accounting example now, without inventing a compiler-level probability law.

## Electron above a wire

Manual `04:145–167` explicitly treats the conductor plus retained surrounding
shell as the wire field. The enclosing shell carries duration, pressure and
SADAR attention balance. The same section's canonical exact channel and
moving-boundary values provide runnable accounting comparators.

A new electron-wire example can bind an admitted `1:2:6` electron preparation
through the supplied electron map, place it in a declared exterior/shell region
relative to the conductor/core, bind the wire's current participants and their
contacts, and evaluate the existing boundary accounting. “Above” requires an
explicit oriented placement or coordinate map. A metric height, wire current
in amperes or electron velocity, when used, is an additional declared input;
the supplied wire subsection does not provide a numerical set of those inputs.

The charge-orientation tag `−1` and the canonical channel attention `1/3` have
different roles. Do not multiply the current by an additional minus sign merely
because the electron's external charge is negative unless the chosen coupling
map explicitly defines that orientation. Likewise a neighboring shell current
is not automatically a force or displacement prediction. A later motion law
must be the declared response consuming the already committed current/attention.

For a minimal exact example, preserve separate crossing, swept support and
shedding records, use distinct per-cut occurrence IDs, retain the electron/wire
histories under cancellation or inactive coupling, and report the signed window
sum and audit residuals. The existing inverse-pressure response is one named
Manual realization if the example chooses it. Geometry, field population and
the response schedule remain explicit rather than implicit in a carrier name.

## Short windows, shedding and target error types

Manual `03_short_window_rcd_shedding_fixtures.tex:218–373` supplies exact
neutral-reflection equations

`ρ_window=min(ρ,window)`, `Q=C3 max(0,ρ−window)`, `P=C3 ρ_window`,
`Δ_close=P−C_close`, `X=max(0,Δ_close)`.

Its Dimonon examples at duration 9 and capacity 3 give:

- window 7, lower cost 18: pressure 21, pending 6, residual/surplus 3;
- the same window, upper cost 24: residual −3 and surplus 0;
- window 9, cost 24: pressure 27 and surplus 3;
- window 10, cost 27: clipped participation 9 and residual/surplus 0.

These are already supplied finite comparators, not rates inferred from an
external particle mass. The source requires route identity, allocated amount,
conservation and retained-form/lifetime witness after the amount calculation.
Positive surplus alone supplies no complete receiving layer.

Manual04 classifies its errors as exact scoped Stokes, moving-boundary,
current-split and route-conservation residuals. Those can be output for the new
examples directly. An external rest-energy comparison instead uses

`E_ext=K_ext ν_AO`, `ΔE=E_ext−E_target`,
`signed_percent=100 ΔE/E_target`, `z_target=ΔE/σ_target`.

Appendix B declares `K_ext=h_ext η_ext/(10^6 e)` with
`η_ext=137.035999177(21)` and displayed
`K_ext=5.6673535511200×10⁻¹⁹ MeV·s`. The Manual expressly separates the candidate
rate, target, normalizer and uncertainty roles. A target-derived rate
substituted back into this same map is a round-trip identity, not a new mass
prediction. A target pull does not include unresolved model/map uncertainty.
No percentage should be attached to an exact native conservation residual
without its own declared reference denominator.

## Search coverage and source ownership

The actual `manual/main.tex` input list, 37 Main/Manual section and appendix
files, relevant scripts/data and native constructors were inspected or searched.
A second lexical pass stripped TeX command names, braces, punctuation and
whitespace before searching for split forms of `protonproton`,
`electronabovewire`, `hardboundary` and `materialmapping`; none supplied a named
trajectory with those preparations in this frozen release. Symbolic `p+`, `e−`,
`3:3:6` and `1:2:6` rows were read directly, so that lexical result is not used
to deny their explicit maps. It is a bounded finding about the requested
interaction preparation, not a claim that the AOD project has no such material
elsewhere or that the examples cannot be executed.

Main supplies the inherited construction/current/boundary calculus; Manual
owns these carrier mappings, finite application inputs, SI comparisons and
error records. The new examples should execute those equations with explicit
preparations and source-bound witnesses. The compiler/runtime executes the
application and does not originate its transition probabilities. Current AOD
guidance, the preserved local amendment and AF review discipline govern this
separation; no obsolete milestone gate or generic physical-map objection was
substituted for the actual source.
