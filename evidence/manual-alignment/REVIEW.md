# Manual-alignment review and repair

Review target: PR #2 at `89a3653cca528963297fae73bd2aa6869df73e23`.
Date: 13 November 2026. Scope: error reporting, declared kernels, path-history
retention, population/RCD typing, D.E.C. calculation structure, and SI observation
and target records. This is a bounded source/code review, not a full manuscript
review or empirical validation of a maser. Review extraction preceded repair;
no upstream Main, Manual, skills or lint source was changed.

The original execution archive remains bound by
`../cell-jitter/source-binding.json`. `source-binding.json` additionally binds
the delivered local Manual I candidate `8c3149b9d3d924b80b9385b178da4ba2c39175e8`
and its tree/files. The observed remote Main was `1a80693cf6235809244479a5f52ed333662bf51d`;
the candidate is not promoted to accepted Main. The AOD owner amendment controls
style and ownership; AF governs inference and AFC applies only to the declared
normalized finite fixture. Individual fidelity digests and executed checks are
in `error-si.md`, `path-rcd.md`, `si-target-sources.md`, and their binding records.

## Finding register

| ID | Finding and evidence class | Disposition and repair |
|---|---|---|
| F1 / ESI-01 | Supported reporting defect: the supplied-mass-normalized terminal L1 diagnostic is not an individual output's relative error. Exact retained Euler values and a separately assembled numerical reference provide a concrete countercheck. | Replace the generic accuracy headline with per-output kind/unit/comparator/denominator records. Preserve the old aggregate as a named diagnostic and the original evidence unchanged. |
| F2 / P01–P04 | Missing AOD realization in the valid auxiliary transport model: its aggregate masses and pair transfers contain no recurring-wave identities, bound contacts, RCD or path-pressure construction. | Keep transport scoped as a numerical control. Add a separate D0 fixture using the byte-bound Manual producer's actual admission/history APIs, with explicit population placement, duration domain, attention operands and compression rule. |
| F3 / ESI-02 | Missing typed error records: exact arithmetic, an observed solver discrepancy, a reference tail bound, finite ensemble spread and physical uncertainty have different meanings. | Add separate per-output eight-component records; unknowns and inapplicable components have reasons and no numerical replacement. Integer delta3 rejects fractional/bin-relabelled inputs. |
| F4 / ESI-04 | Missing premise for transport-to-SI prediction: model mass is not phase, cycles, energy or an apparatus frequency. | Add the Manual's conditional observed-cycle SI fixture and the source-reported hydrogen reduction/target. Leave the transport's physical frequency prediction, residual and complete uncertainty null until an observation law/calibration is supplied. |

## Concrete counterchecks

For amplitude `1/4`, seed `7`, the one-substep accumulated output is exactly
`1419/1048576` in model-mass units. Its continuous numerical comparator is
approximately `0.00646501387863075565`. The signed output-relative discrepancies
are approximately `-0.790678894` and `-0.025583558` at one and 32 substeps. These
are named numerical diagnostics, not measurement uncertainty. The old aggregate
displays `0.034959972` and `0.001011391` cannot substitute for them.

The corrected D0 fixture retains four distinct admitted recurring waves;
aliases do not add population. Its two directed histories yield support 3,
durations 5 and 7, participation 5 and 6, and pressures 15 and 12. Declared
attentions `1/3` and `-1/2` give total oriented value `-1`; the separate normalized
AFC memory/kernel fixture gives flux `-1/8`. Neither value is population density.
Density remains uninstantiated because no cell measure is declared.

The history reduction check preserves the same current wave/population summary
and common contact prefix, then retains two explicitly distinct reflected-contact
alternatives. Their later responses are 5 and 6 under the declared update. This
is a counterexample to that reduced summary's sufficiency, not a claim that no
complete Markov representation could exist. The original contact is not mutated.
Signed cancellation retains both contributing path identities.

The SI fixture preserves winding and endpoint phases: `A=7/2`, `A_R=17/2`,
ratio `7/17`, and exact split `13/6+4/3=7/2`. Its conditional Cs-equivalent map
gives `T=17/(2 f_Cs)` and mean frequency `7 f_Cs/17`. The exact SI definition
does not establish a measured reference realization. The hydrogen target is a
separate source-reported measurement; target-only mutation leaves the fixture
prediction and published-input reduction unchanged. The unrelated `7/17`
fixture is not scored against that hydrogen target.

## Coverage and limits

Fidelity-checked: the named Scope, D.E.C., field-history/RCD, mapping/error,
temporal/metrology passages, their active Manual inclusion, and the source/code
anchors listed in the bindings. Substantive checks: exact kernel/fixture values,
history and admission mutations, source-reported hydrogen arithmetic, per-output
numerical discrepancy, typed report rejection, target isolation, and native
transport replay. Unit-test and document-build outcomes are in `validation.json`.

Not established: a full scientific review of either manuscript; the unexercised
interfaces of the vendored producer; a tesseract population-density evolution
law; empirical accuracy, stimulated gain, coherence, linewidth, or a calibrated
history-to-hyperfine-emission map. The repairs supply exact worked fixtures and
honest comparison interfaces without filling these missing physical premises.
