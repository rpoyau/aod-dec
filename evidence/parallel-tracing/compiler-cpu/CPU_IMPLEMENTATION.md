# Native CPU compiler batch implementation

The compiler selects a verified pure typed Dec branch, emits/loads a uniform LLVM thunk calling its existing guarded entry, and passes that address once to a C++ dispatcher. Each native thread executes independent lanes with separate runtime contexts. Actual thread IDs and entered-function counts are reported; Python only validates, packs inputs and copies outputs.

The JIT session lifetime lock covers load, dispatch, join and collection. All worker threads join before native contexts or executable code can be reclaimed. Language assertion/fuel/depth failures remain ordered per-lane results. Native resource failures reject the batch. A no-allocation fallback prevents secondary failure-report allocation from escaping a worker thread.

Admission is explicit: selected pure nonrecursive int/bool entry; no calls/effects; arithmetic +,-,* and unary/comparison/Boolean/conditional forms. Exact input integers use GMP on CPU. Batch caps cover rows, input bytes/bits, workers, fuel and source steps; conservative propagation rejects more than 262144 estimated bits per intermediate or 64 Mi estimated integer bit-work across all rows. Both conditional branches count. These are target admission bounds and are not a claim of fixed process RSS. Scalar Dec semantics are unchanged.

14 tests pass in normal and optimized Python. Checks include actual four-thread execution, exact 700-bit inputs, source changes changing compiled output, source mutation invalidation, 42 fuel budgets against the independent interpreter, depth/assertion/short-circuit and context isolation, no interpreter execution in the batch, zero-parameter/Boolean/return cases, overload/type/effect rejection, caller-list mutation after snapshot, excessive repeated-squaring rejection before native entry, output-budget recovery, lifetime locking, and an isolated C++ allocation-failure injection in the actual failure reporter. No speedup is inferred from these correctness checks.

Exact file hashes are in CPU_IMPLEMENTATION.json. No remote changes were performed by this implementation task.
