# Focused Manual fidelity audit: kernel, histories, population and RCD

Date: 2026-11-13. Scope: the user-requested correction of the tesseract
transport example. This is a bounded source/code review, not a full AOD
manuscript validation. Review findings precede and are separate from the repair.

## Source binding and coverage

The original application bound the supplied `AOD_v40_01r10_source.zip`, SHA-256
`a178a5bf06d51c8ab8852e9cabfb6c5eb61456f70739d7391582c08c524f34c7`.
Its D.E.C., short-window and field-application members were checked as historical
context. The later delivered local Manual I candidate inspected here is
`AOD_M3_MANUALI_FEEDBACK_2026-11-11/aod`, commit
`8c3149b9d3d924b80b9385b178da4ba2c39175e8`, tree
`b2b55cbcf18354828f7b34418ab3fa26d747a927`. Local delivery does not establish
acceptance as the current remote Main. File hashes and actual exact diagnostic
outcomes are in `path-rcd-checks.json`.

Fidelity-checked units: Manual Scope calculation spine and arithmetic policy;
D.E.C. rows/kernel/hinge-slide/audit; short-window RD/RCD clipping; field-dynamics
support classes and run-tumble trace; Main field membership, wave/coupling state,
directional RCD and typed flux/path-pressure formulas. Also inspected the
candidate native producer APIs and the existing Dec transport engine/runner.
SI metrology and complete error-component budgets are covered by the separate
error/SI review, not certified here.

## Findings and authoritative dispositions

**P01 — supported auxiliary numerical model, missing AOD realization.**
`simulations/cell_transport.py:transition_kernel` constructs a legitimate
11-state stochastic kernel `P_h=I+hQ`, with diagonal holding probability,
absorbing output/loss and an additive external pump. Its mass update and
conservation checks are valid within that declared model. It is not the
Manual's atomic Hamming-1 continuation kernel and does not instantiate its
ternary row admission, mandatory hinge, zero-admissibility block and reweighting
chain. The distinction is semantic, not a floating-point bug.

Anchors: Manual `00_dec_ledger.tex:19–36`, `:38–65`, `:94–188`; Main
`06_wave_state_relational_time.tex:186–214`, labels
`eq:continuation-domain-split`, `eq:admitted-kernel`.

**P02 — aggregate pair records do not supply retained native path histories.**
The transport records forward/backward pair quantities and endpoint mass.
They do not bind immutable event identities, retained start, lineage,
certificates, admitted repeated returns, directed contacts or reflection
witnesses. Preserving gross transfer alongside net transfer is useful numerical
accounting, but cannot be called preservation of those missing histories.

Main requires complete state and trace in `06_wave_state_relational_time.tex:5–51`,
`eq:complete-wave-state`, `eq:wave-update-trace`, `eq:coupling-trace`. Manual
`06_field_dynamics_applications.tex:19–27` explicitly says clustering declares
retained/discarded state and tests future sufficiency; equal present summaries
do not establish a Markov closure. Its concrete run-tumble example
`:246–295` retains selected signed steps in `h_t in Z^4` before applying `pi_3`;
continuous diffusion summaries are downstream presentation maps.

Actual exact challenge: the producer admits traces `(0,1,0)` and
`(0,1,3,1,0)` with the same endpoint but different return IDs and branch/hinge
supports 3 and 5. Endpoint-only compression loses a quantity needed by duration.
This does not prove every Markov representation impossible: a complete state
may carry the needed history.

**P03 — population, memory, scalar duration and pressure have different types.**
Main `05_curl_closure_duon_current.tex:164–190`, `eq:field-organization`,
requires a field to resolve unique admitted recurring wave identities.
Aliasing a wave does not create a second population member. A per-cell count
requires an explicit placement/support map. A spatial density additionally
requires its denominator/measure; neither a cell mass `x_i` nor RCD duration is
automatically that density.

`06_wave_state_relational_time.tex:53–100` declares
`RD -> RCD -> dur(RCD)=rho^D -> compatible min(rho^D,window)`.
RCD retains directional contacts and reflection witness. Its additive-count
realization has `rho^D=RD+sum(reflection counts)`. Only projected participation
is clipped: neither RD nor RCD is clipped. `rho^D` is a duration-count scalar,
not population density or AFC normalized memory.

**P04 — pressure and AOD flux require their path-history operands.**
Main `06_field.tex:77–121`, `eq:path-history-pressure`,
`eq:path-history-compression`, `eq:b-scoped-flux` gives
`p_ij^D=sum_h C3_h rho^D_window,h`. A compressed channel form requires an
explicit compression, and its directional value is `p^D A`. Aggregating
different attentions requires preserving the sum `sum_h p_h A_h`, not an
unjustified arithmetic mean of attentions. In contrast AFC flux is
`p_i^mem P_ij-p_j^mem P_ji`, with normalized memory and row-stochastic P.

The reverse bookkeeping view of one fixed antisymmetric channel is its negative;
a separately constructed reverse physical channel can have a different RCD and
magnitude. Main `06_field.tex:10–23`, Manual `00_dec_ledger.tex:61–65`.

Actual exact diagnostics: symmetric two-state kernel with memory `(1,0)` gives
flux `+1/2,-1/2`, showing why kernel symmetry alone is insufficient. Two
oriented contributions `+4,-4` give zero boundary sum while retaining both
lineage labels. Their source-driven diagnostic records are preserved, not
promoted to a complete physical model.

**P05 — exact internal outcomes and decimal solver summaries need separate roles.**
Manual `00_scope.tex:108–157`, `manual:exact-arithmetic-presentation` and
`eq:frg-delta-three-components`, keeps counts and rational ratios exact;
integer fixture comparisons record `Delta^Z` and `(sign,magnitude)`.
Decimals and percentage comparisons are report coordinates, with declared
target/denominator, precision and scope. The old normalized Euler L1 error
is an auxiliary solver diagnostic against its continuous model, not a
fractional error in a source-derived Markov kernel or field population model.
No known target in SI was used in that score.

## Minimum repair and implemented bounded fixture

Keep the previous exact transport results as an auxiliary numerical study;
state which discrete kernel and continuous generator are compared. Do not
relabel their solver discrepancy as an empirical AOD/maser error.

Add a separate exact source-bound D.E.C. fixture using the delivered candidate's
actual native APIs. The applied repair is confined to
`simulations/vendor/aod_manual_native.py` (byte-identical), its MIT `LICENSE`,
`simulations/manual_dec_fixture.py`, and `tests/test_manual_dec_fixture.py`.
No upstream source or native compiler implementation is changed.

The fixture exposes `build_fixture()` as a JSON-ready exact record and includes:

- one-node probabilities `1/8,1/8,1/2,1/4`; slide probabilities
  `3/7,3/7,1/7`; blocked fibre dwell and retained mandatory hinge; explicit
  empty structural support versus nonempty zero-weight support;
- four certified recurring wave identities, complete returns/certificates,
  two bound contact families, alias resolution and declared two-cell placement;
  counts `2,2` and normalized occupancy `1/2,1/2`, with spatial density absent
  because no positive cell measure is supplied;
- forward RCD durations `5,7`, participation `5,6` under a count window `6`,
  pressures `15,12`, path-pressure sum `27`, and flux `5-6=-1`;
- the explicit compression `A_eff=(-1)/27`, whose reconstructed flux is `-1`,
  plus a separately built reverse channel sum `0` versus bookkeeping reverse `1`;
- a separately declared AFC memory/kernel yielding flux `-1/8`, with types
  kept separate from pressure-bearing AOD values;
- a cancellation control on the same retained histories with contributions
  `+5,-5`, keeping both lineages;
- a matched history-reduction challenge: identical recurring waves, population
  and present scalar state, first contact unchanged, separately identified last
  reflection alternative; duration changes `5 -> 7` and the declared producer
  response at cut 23 changes `5 -> 6`. Inputs are committed through cut 22.
  The counterexample concerns that displayed reduction and response only.

Hostile checks cover event replay, aliased population, wrong contact prefix or
lineage, branch bypass, zero-admissibility/zero-weight confusion, incompatible
duration domain, duplicate kernel candidate aliases and missing recurrence.
All native quantities remain exact. No SI time, spatial volume, physical
emission law or apparatus target is invented.
