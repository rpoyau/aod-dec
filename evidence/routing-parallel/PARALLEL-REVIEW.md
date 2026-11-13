# Parallel execution integration review

Bounded code and evidence review, 13 November 2026. No physical collision or general speedup claim is reviewed.

## Source binding

- `simulations/chiral_batch.py`: `a31c158f625dfef5357492984802caef1ef19dae5dfbdc81085f22895c8dcf1f`
- `simulations/chiral_gpu.py`: `17bb8d39ed91f2c1f5a5095c08d3690a3487b71a227dfeda502d41afc713b002`
- `simulations/kernels/chiral_batch.cu`: `561b0cfb926805d75b6bf3b95cbbbdcb443f77af8b4e795f608375fb07f4238e`
- `scripts/simulate_routing_parallel.py`: `9caf9423fadc4c2f08298a245bd8791d45d8b67f85d3a332bac17307b4e42916`
- `docs/routing-parallel.typ`: `8b6fe7fede98d7e9dd715203e4414e6eccc9602ab4d0b307fe3ff01c3312bfc1`

## Findings

P01 — Exact buffer contract is preserved. The model adapter owns probability thresholds; scalar/multicore/CUDA executors consume supplied integer tables and tickets. Exact-type checks reject Boolean aliases, non-integers and invalid dimensions. With at most 1,000,000 transitions, the largest flattened output is below 7,000,000 elements. uint32 thresholds preserve D=2^32−1 and strict ticket equality; signs remain ±1 int32.

P02 — Input/result identity comparison is scoped correctly. Common result construction hashes tables, ticket batch and returned rows. The runner compares semantic hashes across scalar, multicore and any executed GPU result, so a different supplied batch cannot silently be called equivalent through normal backend APIs. The 256 retained prior paths are checked ticket-by-ticket and row-by-row.

P03 — Device absence is not a successful fallback. The observed driver probe returns CUDA_ERROR_NO_DEVICE. Offline NVRTC 12.9 compilation succeeds, and the same kernel body is separately executed with CPU launch-index shims. Neither observation validates actual GPU execution. The runner stores UNAVAILABLE in auto mode and raises/persists FAIL in required mode; other compilation/launch errors are not caught as fallback.

P04 — CUDA cleanup defect found: the initial finally block stopped after the first free/unload failure, could omit context destruction, and could mask a primary launch/allocation error. NVRTC destruction could similarly replace a compiler error. A coordinated cleanup fix was requested before final source binding.

P05 — Host traces are freshly regenerated and checked for scalar/multicore identity. Current default tracing seeds 0..15 are also covered by the retained candidate comparison. The runner originally lacked a direct fresh-candidate-to-trace comparison for traced seeds beyond the retained 256; adding that check was recommended to root.

## Coverage limits

Inspected the supplied-table CPU executor, CUDA ctypes declarations/kernel flow, study modes/results, and manuscript backend claims. Actual GPU allocations, launches and driver cleanup were not exercised because no device was accessible. Hardware-probe escalation was aborted by the user; no outside-sandbox GPU result is inferred. Host admission/scientific routing semantics are reviewed separately.

## Cleanup repair addendum

P04 repaired: CUDA release attempts now continue through every buffer, module and context even when one release fails. The active primary exception remains in flight and receives cleanup diagnostics; only cleanup failures without a primary exception raise an aggregate failure. NVRTC destruction uses the same primary-error policy. Three injected-failure tests cover these obligations. Normal and optimized GPU-module suites each ran 11 tests: 9 passed and 2 actual-device tests skipped. No GPU hardware execution is inferred.

- Final `simulations/chiral_gpu.py`: `08d464c5857fb49c7c5551a497c246e939fc31cb4a412a33827dbc5b0fae5618`
- Final `tests/test_chiral_gpu.py`: `4dd8482f212b9583987fdc9d4f181e6f79f46cab56ad5b2f5ed7d110bee5634e`
