# PR import validation

The completed native compiler/runtime/learning implementation is preserved byte-for-byte from the sealed detached delivery. Repository changes add pinned skills/lint guidance, CI, provenance checks, evidence navigation and native Make targets.

Fresh local validation ran `python3 scripts/verify_native.py --output build/verification-pr-ci` against this checkout. All 12 required commands passed: 79 Python tests in normal mode, 79 under `-O`, nine Rust checks in each debug/optimized mode, native runtime debug/O3 witness checks, 1,979 runtime differential cases, and 94 AOT/JIT/reference source-conformance rows. Commands, versions, timestamps, exit codes and outputs are retained in [validation/results.json](validation/results.json). The complete source-level outcomes are in [validation/conformance/results.json](validation/conformance/results.json).

The source checksum and import-provenance checks pass. The latter binds 196 byte-identical original execution records and 32 unchanged implementation/test/example files; see [IMPORT.json](IMPORT.json), [SOURCE_MANIFEST.json](SOURCE_MANIFEST.json) and [provenance-check.json](provenance-check.json).

`aod-skills` is pinned to `630af16a7c51fb8836e603eb54c67bf8560fb463`; its source validator passes for four skills and eleven snapshots. Its older manuscript ownership table is reconciled with the current owner amendment in [SKILLS.md](../../docs/SKILLS.md). `aod-lint` is pinned to `68bc4795e982bbdeb266e346d694b08c7c49c844`; its fresh locked build and four upstream tests pass. Its actual compiler-repository digest contains zero Typst/LaTeX files, so [lint-coverage.json](lint-coverage.json) reports **NOT_APPLICABLE**. No compiler/scientific success is inferred from that empty manuscript inventory.

The local validation host is Fedora Linux x86-64 with LLVM 16.0.6. The new read-only GitHub Actions job targets Ubuntu 24.04 and pins LLVM 16.0.6; it has not run at the time this import record was written. Remote results must be read from the PR checks after publication. Performance measurements and Python-free Bubblewrap isolation remain the independently recorded original executions; their native implementation is unchanged.

M6 self-hosting, additional grammar and AArch64 execution remain unimplemented. The preserved seven-workload experiment established no cost-inclusive speedup and retained generic rollback. This PR makes no new physical-model or performance claim.
