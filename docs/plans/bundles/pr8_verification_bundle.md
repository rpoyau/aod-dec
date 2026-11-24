# PR 8 Independent Verification Bundle (R01 PASS — 14/14)

- **Repository:** `/home/antigravity/rpoyau/aod-dec` (`v2/zero-python-evolution` atop `d74651f`)
- **Validator:** `57d4446eca864eecdb7649616a6cf68c`
- **Verdict:** **PASS — 14/14 Acceptance Criteria (`AC-01`..`AC-14`), 0 FAIL**

## Headline Evidence
- **Builds & Tests (`AC-14`):** `93/93` passed in BOTH debug and release (`RUSTFLAGS="-D warnings"`, `--offline`, 0 warnings, first-try green).
- **Binding & CRLF CSV (`AC-01`):** `bindings/aod/s4_fixtures/wavelet_shedding_simulation.csv` byte-identical CRLF (`11,245` bytes, `91` CRLF lines, SHA-256 `d7b0592997d47b2b550508d663afe8943a98ba9d58d087ce77e9d4d0a0c3ff40`); `SOURCE_BINDING.json` appends `S4@e013401` (`DRAFT_CANDIDATE_UNDER_S4_REVIEW`, PR `#53`, `"upstream_path": "wavelet_shedding_simulation.csv"`).
- **90-Tick Kernel & Pure-Dec 6-Way Parity (`AC-02`, `AC-03`):** `10_s4_cycle_shedding_90tick.dec` 6/6 byte-identical (`VM`, `AOT -O0/-O2`, `JIT -O0/-O2`, hermetic `DecIR v2`) within `DEFAULT_FUEL = 100_000`, matching exact **102-digit** denominator `final_burden` and all 90 CSV ticks (`11` shedding ticks `29..=39`, zero-residual conservation, `<= 1/10^14` legacy CSV tolerance).
- **`aodc eval-op` & `aod.s4.rust.report.v1` (`AC-04`..`AC-10`):**
  - Positive probes across `--input-json`, `--input`, `--input-stdin`, `--report` exit `0` with indent-2 sorted zero-float JSON; hostile probes exit `1` (eval/input) / `2` (usage/unsafe `--report`, zero writes on failure).
  - Debug vs Release `verify-s2 --s4-report --report` byte-identical (`SHA-256: 6b75094f...`), `RC-020` `UNRESOLVED_PROVENANCE_QUARANTINED: 1` preserved, 8/8 hostile `--report` probes (from repo root AND `Cwd=/tmp`) exit `2` with 0 disk writes & 0 `.tmp` orphans; 3/3 tampered/symlink CSV probes exit `1` with portable zero-host-path `FAIL` JSON; backward-compat `verify-s2 --json` `30/30 PASS`.
- **Manual, Zero-Python CI & Hygiene (`AC-11`..`AC-13`):** `manual-verify --json` `PASS` (`dec_files_checked=17`, `citations_checked=18`, 0 `aod-lint` hits across 13 `.typ` files); `manual_examples` `17/17`, `v2_smoke` `3/3`, `v1_examples` `10/10`; `ldd` zero `llvm`/`python`/`gmp`; `0` `.py` files; unchanged 5-file `unsafe` allowlist; `/home/antigravity/rpoyau/aod` clean (`""`).
