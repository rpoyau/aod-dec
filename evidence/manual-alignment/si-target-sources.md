# SI targets and observation mapping: source-bound review

Scope: read-only review of the delivered Manual I candidate and primary metrology
sources for the Dec tesseract application. No source repair or independent atomic
prediction is claimed. Reviewed 13 November 2026.

## Binding

The local producer at
`AOD_M3_MANUALI_FEEDBACK_2026-11-11/aod` is commit
`8c3149b9d3d924b80b9385b178da4ba2c39175e8`, tree
`b2b55cbcf18354828f7b34418ab3fa26d747a927`. This is a delivered detached
candidate, not an inferred accepted remote head. Relevant owners:

- `manual/sections/03_m3_temporal.tex:17–77`: observation domain, history binding,
  cycle bridge, stipulated gains, nonidentifiability and separated evaluation.
- `manual/sections/03_m3_temporal.tex:154–312`: observed phase advances, the
  exact conditional SI fixture, measured-input reductions, dependence and
  comparison conditions.
- `manual/scripts/aod_m3_metrology.py:223–284`: `phase_advance` and
  `compare_accumulations` enforce coordinate, epoch, window, orientation,
  correction ownership, reference identity and positive reference advance.
- `manual/scripts/aod_m3_metrology.py:322–363`: `reduce_hydrogen` retains
  printed-input arithmetic and reported uncertainty separately.
- `manual/scripts/aod_m3_metrology.py:892–1050`: `assess_comparison` separates
  calibration, evaluation and reference-dependent comparison roles; evaluation
  sources cannot be prediction dependencies. A valid metadata record does not
  establish complete physical correspondence.

## Actual targets and source roles

The BIPM's current [SI second definition](https://www.bipm.org/en/si-base-units/second)
fixes the unperturbed caesium-133 ground-state hyperfine frequency to exactly
9,192,631,770 Hz. This integer defines the unit. It is neither a measured
apparatus frequency with zero uncertainty nor a conversion factor for arbitrary
native events. Reference realization and observation uncertainties remain.

[Hellwig et al. (1970)](https://tf.nist.gov/general/pdf/13.pdf), printed pp. 200
and 207, report the unperturbed hydrogen hyperfine frequency
1,420,405,751.768 Hz with a quoted one-sigma uncertainty of 0.002 Hz. Both
evaluations share the NBS-III caesium reference. This is a historical measured
target, not an exact constant or a new AOD result. The archived PDF's SHA-256
matches its receipt; I visually rechecked the abstract and conclusion. Web OCR
misreads the abstract exponent as −2; the rendered page shows −3 and the
conclusion explicitly gives 0.002 Hz.

The [NIST-F2 evaluation](https://tf.nist.gov/general/pdf/2704.pdf) distinguishes
its typical Table 1 uncertainty budget from the August 2013 campaign. The
bound receipt/code records August fractional standard uncertainties
uA=0.44×10⁻¹⁵ and uB=0.16×10⁻¹⁵, while the typical Table 1 uB is0.11×10⁻¹⁵.
They are dimensionless fractional frequency uncertainties, not percentages.
Multiplying by the defining Cs frequency expresses the corresponding uncertainty
in Hz; multiplying a dimensionless fraction by100 merely changes its display
to percent. Type A/B are evaluation methods, not error classes. The campaign
cannot be reconstructed from published summary values or rounded synthesizer
settings alone.

## Executable example already specified by the Manual

Use the existing exact observed-advance fixture, not transported mass as a proxy
for oscillator cycles. On the matched window (10,22], with declared coordinate,
epoch, orientation and phase-unwrapping convention:

- Compared winding4, endpoints2/3→1/6: A=4+1/6−2/3=7/2 turns.
- Reference winding8, endpoints1/4→3/4: AR=8+3/4−1/4=17/2 turns.
- Splitting the compared window at phase5/6 retains13/6+4/3=7/2.
- A/AR=7/17. Conditional on a separately justified corrected Cs-equivalent
  reference realization, T=17/(2 fCs) seconds and meanν=7 fCs/17 Hz.

These are exact mathematical inputs and conditional finite-window results.
They do not predict the hydrogen frequency. The Manual labels are
`eq:manual-m3-observed-advance`, `eq:manual-m3-accumulated-si` and
`eq:manual-m3-cycle-bridge`. The existing implementation explicitly reports
`physical_correspondence: unestablished`, complete physical uncertainty unknown,
and no universal seconds-per-bip law. Requiring only a positive reference count
would be insufficient; its physical/reference kind and identity matter.

For a genuine hydrogen comparison, freeze the native history, population,
preparation, directed couplings/RCD, observation operator and its calibrated
parameters before admitting the target to the evaluator. Bind oscillator phase
or the instrument's actual beat/population/timestamp observable. Keep target and
prediction payloads separately hashed. A target-only change must preserve the
prediction bytes. Known prior target exposure is retained; a later hash does not
make the experiment blind. Any shared calibration/reference retains its identity
and covariance obligations. Missing physical maps reject a physical point
prediction; they do not prevent exact conditional fixture arithmetic.

## Independent printed-input calculation

Using exact rational parsing of the retained decimal tokens, I reproduced:

| Reduction | Exact result, Hz |
|---|---:|
| Table IV | 14204057517691/10000 |
| Table VI | 14204057517667/10000 |
| Arithmetic mean | 14204057517679/10000 |
| Published rounded mean | 177550718971/125 |

The observation equation is
νH=284νR+νS+νB+δUTC+δN+δW+δD+δZ. Table VI's input already includes the NP3 and
wall corrections; adding those again is rejected. The exact arithmetic above
is exact for printed measurement inputs, not for the physical frequency.
The mean differs from the published rounded display by1/10000 Hz; this is
reporting precision, not solver or physical model error. Table IV minusVI is
3/1250 Hz. Its signed fractional discrepancy relative toVI is
24/14204057517667; its percent display would be2400/14204057517667 percent.
This comparison is reference-dependent; it does not create independent target
validation or justify combining quoted uncertainties as independent variances.

## Byte evidence and coverage

| Bound file | SHA-256 |
|---|---|
| `manual/scripts/aod_m3_metrology.py` | `d2f53146257b1d29939587abc167f87e9e810f904660b3bfd81a5015ef6e9618` |
| Hydrogen source receipt | `397f81e68f5bfa57db0cee5abbf4c2dea5dec6cbc000c99b623e0ef361750bf7` |
| Hydrogen PDF | `e8bba435fbb9e6b54f41fa1d60f9c02df8e06b6a73ce8fab323afb3ea7d02dc4` |
| Hydrogen protocol | `35871c597e922ce18831f004ac03e06236b424cf1547adbc15cce96ce8708940` |
| Hydrogen reported uncertainty | `71e2bf4c2891b28f1076fcc9e2b7266943cc330be394585172ed264119a38560` |
| Caesium source receipt | `43fec317110ec9c033a7da5b3858af15e70132fc1e6af480cbe6bc27182c9298` |
| Caesium PDF | `85482447888dc5524b7ebf3754ea4fdcde3a80d9c36ae927e5d69d9e9f980eb5` |

Completed: direct relevant Manual/code reading; receipt/PDF hash checks; live BIPM
definition and NIST primary-paper access; hydrogen pp.200 and207 visual
inspection; independent exact printed-input sums. Not completed: raw apparatus
replay, full physical covariance, a native preparation/emission law, or empirical
validation of the tesseract transport model. Those are not implied by this memo.
