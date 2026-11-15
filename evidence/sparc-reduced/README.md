# Executed reduced SPARC response

The predeclared spacing case worsens equal-bin square-speed MAPE from
**26.570553423860% to 26.977910478548%**, a **+0.407357054687 percentage-point**
change. All 209 bins remain. The coupling-disabled control restores G0 exactly.
The unit-intensity ablation improves pooled MAPE but worsens equal-galaxy MAPE;
it is not selected as a replacement primary.

[Outcome assessment](RESULT_REVIEW.md), [case report](REPORT.md),
[four-page PDF](sparc-reduced.pdf), [exact comparison](comparison.csv),
[summary and bindings](summary.json), [complete trace](trace.json.gz).

`DECLARATION.md` records the choices fixed before comparison. Native recurring
Duon representatives, committed offsets, directional contacts and the existing
inverse-pressure response produce the trajectories. The trace contains all
three cases. Reopening checks 621 transition hashes, 2,484 boundary identities
and 31,437 current operands. All accounting residuals are exactly zero.
The independent recurrence agrees on all 627 case/bin rows and 3,135 numeric
fields. These checks have separate scope from a physical correspondence claim.

The moving control region is passive. The primary changes selected membership
on one response cut. Radial/out-of-plane ports are deliberately closed and
there are no separate shedding channels. Actual external exchange, material
boundary dynamics, formation and the full G2/G3 application remain unresolved.

Reproduce from the repository root:

```sh
python3 -B scripts/simulate_sparc_reduced.py --workers 3 \
  --replay evidence/sparc-reduced/trace.json.gz --output build/sparc-reduced
python3 -B -O scripts/simulate_sparc_reduced.py --workers 3 \
  --replay evidence/sparc-reduced/trace.json.gz --output build/sparc-reduced-optimized
python3 evidence/sparc-reduced/independent_oracle.py --input data/sparc \
  --comparison build/sparc-reduced/comparison.csv --output build/sparc-oracle
make docs-sparc-reduced TYPST=/path/to/typst
```

Source changes deliberately invalidate exact archived replay. The retained
Main/Manual binding and executed implementation hashes are in the summary.
`SOURCE_REVIEW.md` and its bindings record the earlier source-review snapshot;
`validation.json` records the current execution and packaging checks.
`historical-430fb14/` preserves the earlier summary, execution and validation.
That normal local discovery covered 624 tests, followed by all eight newly
added driver tests. Optimized discovery covered all 632. Two CUDA-device
tests skipped in each discovery; driver and 33 focused model tests pass in
both modes. Eleven current manuscripts pass pinned AOD lint. The final PDF
was compiled with Typst 0.13.1 and all four pages were inspected.

The shared-context optimization computes the response and swept-field ledger
once for all four boundary views. Its new complete run matches every scientific
trace value and every comparison CSV byte from `430fb14`; only two implementation
hashes change. The [equivalence record](../field-frontiers/sparc-equivalence.json)
checks the complete canonical trace. Production replay still requires strict
whole-trace equality. Predictions and error scores are unchanged.

The current completed invocation used three workers and took 711.75 seconds,
including serialization; the earlier invocation took 861.63 seconds. These
invocations had different concurrent host workloads and establish no matched
whole-application speedup. Interrupted attempts are excluded. Current frontier,
boundary, review and lint checks are recorded under
[field-frontiers](../field-frontiers/README.md). CI independently regenerates
the complete archive in both Python modes; its final status is reported on the PR.
