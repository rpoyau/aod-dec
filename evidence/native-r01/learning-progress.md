# M4/M5 final checkpoint

Final evidence: learning/run-04/results.json, observations.jsonl, reanalysis.json and REPORT.md. All seven workload control suites PASS; compiler/runtime/harness/policy hashes unchanged during execution. Both native entry paths and fresh-process persistent dispatch executed. No cost-inclusive benefit; generic rollback for every workload. Concurrent compilation/conformance and same-program temporal held-out split remain explicit limitations.

Final compiler SHA-256: 16029e1d7b13b21485a07cba60d88a2a755debd88b32a7c66b75f433ed956854.

Policy/helper tests: 12 PASS normal, 12 PASS -O. Includes exact source-error/effect regression and recomputed-checksum unpaired/inconsistent-generation rejection. Standalone CLI final passive profiling → fresh-process active read PASS with 12 retained real measured pairs and exact output; negative case preserves ordered effect and error line. Synthetic costs used only for causal policy-influence fixtures, never performance.

Acceptance mapping: learning/ACCEPTANCE_M4_M5.json. Candidate source: native/learning.py; scripts/benchmark_native.py, analyze_native_benchmark.py, adapt_native.py; tests/test_learning.py; docs/BENCHMARK_PROTOCOL.md, HARDWARE_LEARNING.md, EXECUTION_NATIVE.md. Earlier run-01/run-02/run-03 source-bound observations retained. No remaining M4/M5 implementation actions known within the declared Linux x86-64/two-variant scope.
