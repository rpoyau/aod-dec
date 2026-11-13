# Measured implementation selection v1

`native/learning.py` implements a compiler policy, not an AOD dynamics or hardware
ontology. Inputs are exact-source native variant certificates and measured integer
nanosecond costs. Outputs are the names `generic` (LLVM O0) and `optimized` (LLVM
O2). Every selected implementation uses the same verified source, runtime ABI,
checked arithmetic and retained AOD witnesses.

A `VariantRegistry` admits a variant only after its output, final return, error,
error location and exit status match a supplied reference observation. The
benchmark obtains those observations from the unchanged reference interpreter,
executes both AOT binaries and ORC functions, and saves object/binary hashes plus
full observations. These checks supplement the independent compiler/runtime
conformance suite; differential agreement alone is not a proof.

`RetainedPolicy` retains at most 32 paired costs per variant. The first five pairs
cannot justify a choice until all have been observed; later selection uses the
median ratio and paired percentile bootstrap interval. An optimized selection
requires a measured median gain above 5% and a 95% interval excluding zero. The
bootstrap uses 2000 resamples and fixed seed 20261113. A held-out benefit claim
requires its lower bound above 5% and positive aggregate gain after observed
preparation/profiling overhead. Otherwise the saved rollback bit restores generic.

Call `choose()` before dispatch, then `observe_pair()` and `save()` after completed
runs. The current invocation does not select using its own future observation.
The application records generation and dispatch address. Reconstructing a policy
reads the last fully committed profile; deleting that profile restores generic.
A separate synthetic cost fixture proves policy influence but is excluded from
performance conclusions. Unit tests exercise a fresh Python process reading a
profile. The compiler's native entry guard additionally falls back to generic
when specialization assumptions do not hold.

Profiles bind the policy/version, host/OS/CPU features/affinity, software binding,
workload family and exact validated source/object identities. Loading is limited
to 1 MB and 32 samples per variant; costs must be positive bounded integer values.
Both paired arrays must contain exactly `min(generation, 32)` observations, so a
checksum-valid unpaired or impossible history is rejected.
Unknown versions, changed host/software/workload/artifact bindings, malformed
JSON, duplicate keys, integrity mismatches, missing validation and missing state
retain generic. A SHA-256 digest detects accidental or unsynchronized changes;
it is not an authentication boundary against an attacker who can rewrite both
profile and digest. The profile is a local trusted experiment artifact.

Writes create a temporary file in the destination directory, flush and fsync it,
then atomically replace the target and fsync the directory. An interrupted
uncommitted temporary file is ignored. Each profile has one writer; callers must
use distinct paths for concurrent experiments. This avoids claiming a locking or
multi-writer merge protocol that is not implemented.

Run the complete declared experiment from the candidate root:

```sh
python3 scripts/benchmark_native.py --output /absolute/path/to/new-run-directory
python3 -m unittest discover -s tests -p test_learning.py -v
```

The output directory must not already exist. The harness saves the frozen protocol,
source workloads, host/counter probes, build records, exact reference results,
prevalidation observations, variant certificates, raw timestamped JSONL samples,
retained profiles, per-workload summaries and a final report. It preserves failures
and never overwrites a prior run. Primary comparisons include fresh runtime
context allocation/free, host dispatch and result extraction; native-call-only
costs are also reported. Peak process RSS is a whole-process observation rather
than attribution of individual allocations. Held-out measurements test later
samples of the same programs at doubled batch size and do not establish unseen
program generalization, AArch64 support, GPU benefit or universal speedup.
