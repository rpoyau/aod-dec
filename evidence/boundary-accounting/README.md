# Strict boundary accounting

The active `simulations/boundary_accounting.py` is byte-identical to the reviewed
local component. Its 19 tests run alongside the 28 region-adapter tests.

[Independent repair review](review/accounting-final.md), [counterexamples](review/accounting-counterexamples.json)
and [source-bound validation](VALIDATION.json) preserve the original evidence.
The later region adapter derives coverage from one committed endpoint family;
it does not make a producer's physical completeness follow from a digest.

The selected historical records retain their original workspace paths. See the
[copied-byte map](../scoped-current/IMPORT.json) and the separate publication
verification for current repository execution.
