# PR 7 Independent Verification Bundle (R01 PASS — 12/12)

- **Repository:** `/home/antigravity/rpoyau/aod-dec` (`v2/zero-python-evolution` off `main@68d6705`)
- **Validator:** `57d4446eca864eecdb7649616a6cf68c`
- **Verdict:** **PASS — 12/12 Acceptance Criteria (`AC-01`..`AC-12`), 0 FAIL**

## Headline Evidence
- **Zero Python Anywhere (`AC-01`):** `scripts/` deleted, `0` `.py`/`.pyc`/`.pyo`/`.pyi` files, `0` `__pycache__` dirs, `0` Python shebangs, `0` Python invocations in `.github/workflows/ci.yml`; permanent `zero_python_repository_and_ci_guard` passing debug & release.
- **Live E2E Real 18-Page Typst `0.15.1` PDF (`AC-02`..`AC-08`):** Downloaded `aod-dec-v2-manual-pdf` from CI run `45337785074` (`329,977` bytes, SHA-256 `20f865cec460ba...`):
  - `./target/release/aodc audit-pdf --min-pages 6 --compare-pdf <real.pdf> --json <real.pdf>` -> exit `0`, `status="PASS"`, `pages=18`, `streams_total=38`, `streams_flate_inflated=37` (exact 37/37 `/FlateDecode` + 1 uncompressed XML), `raster_violations=0`, `unresolved_violations=0`, `deterministic_match=true`, zero host paths, zero floats.
  - 1-byte flip in stream 1 -> exit `1` (`Adler-32 checksum mismatch`); appended `/Subtype /Image` -> exit `1` (`raster_violations=1`); `--min-pages 19` -> exit `1`; 1-byte appended `--compare-pdf` -> exit `1` (`deterministic_match=false`); symlinks on `<pdf>` & `--compare-pdf` -> exit `1`.
- **Zero `libLLVM`/`libpython`/`libgmp` (`AC-08`):** `ldd ./target/release/aodc` links only `linux-vdso.so.1`, `libgcc_s.so.1`, `libc.so.6`, `ld-linux-x86-64.so.2`; succeeds with `DEC_LLVM_LIBRARY=/nonexistent/libLLVM.so`.
- **Workspace & Regression (`AC-10`..`AC-12`):** `82/82` tests pass in both debug & release (`-D warnings`), `manual-verify --json` `PASS` (`dec_files_checked=16`, 0 `aod-lint` hits across 13 `.typ` files), `verify-s2 --json` `30/30 PASS`, `v1_examples` `10/10`, `v2_smoke` `3/3`, `manual_examples` `16/16`, `Cargo.lock` exact 4 crates, 0 floats, unchanged 5-file `unsafe` allowlist, `/home/antigravity/rpoyau/aod` clean (`""`).
