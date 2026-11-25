# PR 9 Independent Verification Bundle (R02 Final — 15/15 PASS)

- **Repository:** `/home/antigravity/rpoyau/aod-dec` (`v2/zero-python-evolution` atop `8dc236c`)
- **Validator:** `57d4446eca864eecdb7649616a6cf68c`
- **R01:** 14/15 PASS, 1 FAIL (`AC-04` probe `(c)`: generate-mode symlink target inside `--output-dir` wrote 2 sibling files and exited `1` instead of pre-write exit `2`).
- **R02 (after Implementer fix):** **15/15 PASS (`AC-01`..`AC-15`), 0 FAIL**.

## Headline Evidence
- **Builds & Tests (`AC-14`):** `97/97` passed in BOTH debug and release (`RUSTFLAGS="-D warnings"`, `--offline`, 0 warnings).
- **Apparatus & 8-Way Parity (`AC-01`, `AC-02`):** `stdlib/aod/apparatus.dec` zero `Q_ext`/hardcoded outcomes; `11_apparatus_opposing_sensors.dec` 8/8 byte-identical across `run`, `run --parallel-observers`, `build -O0/-O2` + ELF, `jit -O0/-O2`, `jit -O2 --parallel-observers`, and hermetic `DecIR v2` within `DEFAULT_FUEL = 100_000`.
- **Pure-Rust `aodc manual-data` (`AC-03`, `AC-04`):** `--check --json` `PASS` (`files_checked=3`, `files_changed=0`, zero floats/host paths), `DEC_LLVM_LIBRARY=/nonexistent` exit `0`, regenerated files `cmp -s` byte-identical to `docs/manual/data/*.json`, 0 `.tmp` orphans; hostile symlink target generate exits **`2`** with **0** sibling files created & victim untouched (`--check` exits `1`); disallowed `bindings/aod` & `.git` exit `2`.
- **Hardware Learner v2 & Parallel Observers (`AC-05`..`AC-10`):** `CodegenPlan` v2 (`interleave_count in {0,2,4}` strictly validated, `<= 6` candidate cap, combined `!llvm.loop` verified by LLVM 18), `PROFILE_VERSION = 2` with legacy v1 safe fallback, `soa_vs_aos_bench.dec` 5-mode exact parity, 20-tick × 4-observer = 80-emit live parallel parity (0 interleaving), tight-fuel sweep `5..90`, 40-digit heap-spilled module `const`, deterministic error precedence, `ObserverMutationForbidden` preserved.
- **Manual, Zero-Python CI & Cleanliness (`AC-11`..`AC-15`):** `manual-verify --json` `PASS` (`dec_files_checked=18`, `data_files_checked=3`, 0 `aod-lint` hits across 13 `.typ` files); `manual_examples` `18/18`, `v2_smoke` `3/3`, `v1_examples` `10/10`; `verify-s2 --s4-report` `30/30` + `90` ticks; `ldd` zero `llvm`/`python`/`gmp`; `0` `.py` files; `0` floats; unchanged 5-file `unsafe` allowlist; `/home/antigravity/rpoyau/aod` clean (`""`).
