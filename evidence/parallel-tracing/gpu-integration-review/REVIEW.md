# Source-compiled boundary integration review

Final scoped disposition: **no outstanding arithmetic/accounting/hardware-proof blocker found**. Review was read-only. Production changes responding to review were performed by the owner, separate from these reviewer artifacts.

Reviewed `simulations/compiled_boundaries.py`, `scripts/check_compiled_boundaries.py`, `tests/test_compiled_boundaries.py` and the ordinary source `examples/boundary_coefficients.dec`, with their actual compiler/trace dependencies. Exact final bytes are bound in `SOURCE_BINDINGS.json` and both mode-specific probe records.

## Independent outcomes

- All 48 combinations of mode, endpoint membership, availability and kind match an independently written exact signed-incidence/radix calculation. Event source-minus-target orientation and swept after-minus-before orientation agree with the existing boundary accounting. Pending obligations retain kind 2/bucket 3 and are not converted to signed current.
- The generated CUDA body, compiled on the host using `g++ -ftrapv`, produces the same independent values. This is generated-body conformance, explicitly not evidence of device execution.
- 288 source fuel/depth comparisons per mode cover entry precedence, zero fuel/depth, immediately insufficient fuel, exactly sufficient fuel and one extra tick. All match the reference VM, including source assertion lines and remaining logical fuel.
- Five invalid classification inputs fail at their actual source assertions; no clipping/coercion is used. Three GPU signed-int64/Boolean-alias cases reject before device probing, including fuel zero. Target-domain admission and executed-source budget failures remain distinct.
- The final hardware driver rejects correct numerical rows returned through substituted CPU/scalar paths, using a portable negative probe. It now requires generated-CUDA execution and a positive launch count for the coefficient truth table and all sixteen retained-boundary jobs. It also requires observed native CPU workers and generated entry counts. This is an assertion on recorded execution, not an inference from numerical equality.
- The adapter binds its exact source text and the returned compiler entry to the source-file digest; failed lanes cannot publish current. The stated adapter timing scope correctly leaves canonical validation to downstream trace replay.
- Both reviewer modes pass against identical source hashes. The focused integration unit suite passes in normal and optimized Python (five final tests).

The owner strengthened the hardware proof gate and clarified the scope during review. No initial failing hardware experiment is retained as a completed pre-fix reproduction: the owner correction had already reached the source before the negative driver probe completed. The retained final negative probe demonstrably rejects false execution evidence.

## Reproduction

Use the provided `review_probes.py` with `--repo`, a real retained `--trace` from the sixteen-boundary fixture, and its `--scalar-report` JSON. Use `--output` to write evidence elsewhere; run normally and with `-O`. The probe uses temporary compiled host libraries, produces `boundary-generated.cu`, and does not modify production source. Its mocked driver phase is an explicit rejection test and never claims GPU execution.

## Artifact retention recommendation

For the actual required-device run, retain the generated CUDA and PTX bytes using `lower_cuda` and `compile_cuda` with the executed profile's architecture and optimization request. Require both hashes to match the run's generated-CUDA/PTX bindings before accepting those files as the executed artifact. Retain the actual device profile and full source manifest alongside them. The parent task owns that device run and its evidence.

## Limits

This review covers source-derived finite coefficient computation, exact host assembly and replay proof. It does not establish a full GPU DEC runtime, GPU GMP/ratio arithmetic, GPU witness admission, physical Sun–Mercury trajectory, independent physical prediction, or performance improvement. The active source still requires host retained-history admission and downstream canonical audit validation. Source hashes attest repository bytes, not a hostile remote process or independently attested interpreter image.
