# PR 6 Architecture & Implementation Request Bundle (Rev 2 — All P0/P1/P2 Amendments Folded)

**Target Repo:** `/home/antigravity/rpoyau/aod-dec` (branch `v2/native-reference-engine`, base `4ab9a9f`)
**Scope:** Stage 2 (`S2`) 30/30 Golden-Vector Parity, Shared `crates/aod-core` Stage 4 Kernel, Quarantined One-Way `aod::qext` Display Builtins, Pure-`.dec` S2 Sector Modules (`stdlib/aod/sectors.dec` + quarantined `DisplayQuarantine` in `stdlib/aod/metrology.dec`), Executable Manual Examples `07`–`09` + Negative Control `nc07`, `aodc verify-s2`, `SOURCE_BINDING.json` Amendment (`S2@3122c94`), and CI Alignment (`rpoyau/aod` PR `#51` + PR `#52`).

---

## 0. Mandatory Plan Review Amendments Folded (Rev 2)

- **`P0-1` (One-Way Quarantined `aod::qext` Display Builtins):**
  - Wire two strictly one-way quarantined builtins (ZERO unwrap to native!) across `crates/aod-core/src/qext.rs`, `crates/aod-rt/src/abi.rs` (including `runtime_symbol_table()`), and `crates/aodc/src/{modules.rs, types.rs, checker.rs, eval.rs, codegen.rs}` (including `checker.rs` `aod::qext` import/effect gate and `LLVM_MODULE_PREAMBLE`):
    - `aod::qext::qext_round_half_even(QExt<ratio>, int) -> QExt<ratio>`
    - `aod::qext::qext_format_fixed(QExt<ratio>, int) -> QExt<str>`
  - Runtime bounds guard across all 3 engines (VM, AOT, ORC JIT): reject `decimals < 0` or `decimals > 1024` with clean runtime error (zero OOM / zero UB).
  - Test 5-mode parity, negative/excessive `decimals` runtime failure across VM/AOT/ORC JIT, and `QExtLeakageForbidden` static rejection when invoked outside `quarantine Q_ext:` (including `nc07_display_quarantine_leak.dec`, 2-segment `DisplayQuarantine::...`, 3-segment `metrology::DisplayQuarantine::...`, and direct builtin leak).
- **`P0-2` (Strict Native vs. `Q_ext` Firewall Split for Higgs External Mass Map):**
  - `stdlib/aod/sectors.dec` (native AOD): ONLY dimensionless internal candidate scoring (`higgs_candidate_score`, `higgs_select_winner_index`) — ZERO `MeV`/`GeV` and ZERO external LHC/PDG target constants! SPARC G0, cycle-shedding, SWELLS, and Tau non-conflation functions are parameterized (declared-fixture inputs passed explicitly in `manual_examples/07..09.dec`, never smuggled as canonical native laws), with `swells_delta3_tau_audit` explicitly preserving `"UNRESOLVED_PROVENANCE_QUARANTINED"`.
  - `stdlib/aod/metrology.dec` (`quarantine Q_ext DisplayQuarantine:`): post-freeze external energy map (`higgs_freeze_mev`, `higgs_freeze_gev` returning `QExt<ratio>`), LHC comparison (`delta_GeV`, `z_score_exact`, `abs_percent_error_exact`, 6-dp half-even rounding + exact residuals), and half-ULP enclosure (`SD-004..006`).
  - `crates/aod-core/src/s2_kernel.rs` `RC-011` (`higgs_trace_score_and_freeze`): strictly two-phase (winner frozen purely from internal `(p, q, L, C3, Ca, Cb)` before `E_H_0_MeV`/`E_H_0_GeV` are computed from the frozen winner only), matching upstream `rpoyau/aod@3122c94` contract verbatim.
- **`P0-3` (Complete Test & Manual Chapter Updates for 16 Manual Examples):**
  - Adding `07..09.dec` + `nc07_display_quarantine_leak.dec` brings `manual_examples/` from `12` to `16` tests (`01..09` + `nc01..nc07`).
  - Update ALL affected files:
    - `crates/aodc/tests/manual_stdlib_parity.rs` (`12 passed; 0 failed; 12 total` -> `16 passed; 0 failed; 16 total`)
    - `crates/aodc/tests/manual_verify.rs` (`dec_files_checked` `12` -> `16`)
    - `docs/manual/chapters/02_compile_time_firewalls.typ` (`six` -> `seven`, add `// @dec-file: manual_examples/negative_controls/nc07_display_quarantine_leak.dec`)
    - `docs/manual/chapters/03_aod_stdlib_and_manual_fixtures.typ` (add `// @dec-file:` for `07..09.dec`, update `tab:stdlib-modules` & prose strictly respecting `aod-lint` Rule 3)
    - `docs/manual/chapters/06_validation_and_limits.typ` (update `12 -> 16`, add `aodc verify-s2 --json`, update Typst `0.15.1` SHA-256 pin).
- **`P1-1` (Exact Operation Count = 27 Distinct Operations):**
  - `rational_core.jsonl`: 23 distinct operations (`20` positive + `3` typed-reject `RC-021..023`; `sparc_exact_bin_eval` repeats across `RC-015`/`RC-016`).
  - `symbolic_and_display.jsonl`: 4 distinct operations (`sympy_z4_dft` and `higgs_lhc_rounded_display` repeat twice) = **27 distinct operations total (`24` positive + `3` typed-reject)**.
  - `AC-03` anti-memorization perturbed-input tests cover **all 24 positive operations** + fresh invalid inputs for all 3 typed-reject operations!
- **`P1-2` (Strict Canonical Parsing, Negative-Zero Guard, Symlink Detection & Portable JSON):**
  - `parse_exact_int_str` strictly enforces `^(?:0|-?[1-9][0-9]*)$` (rejects `"+5"`, `"-0"`, `"01"`, whitespace); `parse_canonical_rational_pair` additionally enforces `den > 0` and canonical reducedness (`["2","4"]`, `["0","2"]` rejected).
  - `format_rational_fixed` / `QExtRatio::format_fixed` NEVER renders `"-0.000000"` when a tiny negative rational rounds to zero, and caps `decimals <= 1024` and scientific exponent `|exp| <= 1024`.
  - `s2_vectors.rs` uses `std::fs::symlink_metadata` and strictly rejects symlinks (`!meta.file_type().is_symlink() && meta.is_file()`) on `.jsonl` and `--binding` files.
  - `aodc verify-s2`: zero silent fallback on explicit `--vectors` or `--binding`; emits strict zero-float JSON even on `FAIL` when `--json` is passed (exit 1); zero host absolute paths (`/home/...`, `/tmp/...`) and zero timestamps in JSON output; `scientific_status_summary` counts ONLY passing cases.
- **`P1-3` (Complete `aod.s4.vector.v1` Schema & Per-Operation Domain Guards):**
  - Enforce exact 11 keys (reject missing/extra), `schema_version == "aod.s4.vector.v1"`, non-empty string fields, `oracle_basis != "candidate_snapshot"`, `scientific_status` allowlist, exact 24 `RC-001..RC-024` + 6 `SD-001..SD-006` ordering, `RC-020` locked to `"UNRESOLVED_PROVENANCE_QUARANTINED"`, and `integer_encoding` exact-int `max_abs_integer_digits >= 1` (rejecting bool!) + `rust_min_int_type` allowlist + non-empty `overflow_note`.
  - Explicit domain guards in `s2_kernel.rs`: non-empty square matrix + unit-sum memory in `sadar_afc_flux_antisymmetry`; strict `0 <= phase < 1` in `temporal_metrology_*`; `group_order == 12` in `cyclotomic_z12_overtone`; general SymPy-exact `GaussianRatio::format_sympy()` (`0`, `a`, `a/b`, `I`, `-I`, `a*I`, `a*I/b`, `re + im`, `re - |im|`).
- **`P2-1` (Verified Live GitHub Metadata for `S2@3122c94` & Audit Hygiene):**
  - Verified directly via `gh pr view 51 -R rpoyau/aod` & `gh api repos/rpoyau/aod/commits/3122c9444596a9f479e1444c9d497e510a87f825`:
    - `id`: `"S2@3122c94"`
    - `stage`: `"S2"`
    - `status`: `"DRAFT_CANDIDATE_UNDER_S2_REVIEW"`
    - `pr_number`: `51`
    - `ref_name`: `"m4-s2-reference-engine"`
    - `commit`: `"3122c9444596a9f479e1444c9d497e510a87f825"`
    - `tree`: `"d58b43ccf82b428be8f831d11203604e12d66388"`
    - `base_commit_binding`: `"cea71a76f26c08b603866e746867b3982da6c88e"`
  - Look up binding by `id == "S2@3122c94"` (or `stage == "S2"`) in `aodc verify-s2`.
  - Keep `crates/*/src/*.rs` completely free of bare float-type tokens (even inside string literals) per `source_binding.rs:157-200`, and keep `docs/manual/**/*.typ` completely clean on all 4 `aod-lint` rules (including Rule 3).
- **`P2-2` (Fuel Budget & Extended Negative Controls):**
  - Confirm `07..09.dec` run comfortably within `DEFAULT_FUEL = 100_000` across VM, AOT, and ORC JIT; test 2-segment, 3-segment, and direct builtin `QExtLeakageForbidden` across `check`, `build` (zero artifact), and `jit`.

---

## 1. Complete File Tree

### Create
1. `bindings/aod/s2_vectors/rational_core.jsonl` (SHA-256 `a852094458a884ec456fd712a1fbfdb8cff46f2e90095b74bafe7644ad72cc99`, 24 vectors)
2. `bindings/aod/s2_vectors/symbolic_and_display.jsonl` (SHA-256 `d2ea7c41ff7fb7b62258cb137576f12f667c38377406d695557fa37b3541d19f`, 6 vectors)
3. `crates/aod-core/src/s2_kernel.rs` (zero-JSON, zero-float, `#![forbid(unsafe_code)]`, all 24 positive operations + numeric/rounding/Gaussian/Cyclo12 primitives)
4. `crates/aod-core/src/s2_vectors.rs` (strict `aod.s4.vector.v1` JSONL loader, full 11-key schema validator, 30-vector dispatcher, `symlink_metadata` guard, SHA-256 digest guard, portable zero-float `S2VerifyReport`)
5. `crates/aod-core/tests/s2_parity.rs` (30/30 golden parity + fresh perturbed anti-memorization tests across all 24 positive + 3 typed-reject operations + `BigInt` overflow & tie-to-even edge cases)
6. `crates/aodc/tests/s2_verify_and_sectors.rs` (`aodc verify-s2` CLI + `--json` + hostile tamper/drift/float/symlink negative controls + 5-mode & hermetic `DecIR v2` parity for `07..09.dec` + `nc07` & extended 2-seg/3-seg/direct `QExt` leak + runtime negative/excessive `decimals` checks)
7. `stdlib/aod/sectors.dec` (strictly native AOD S2 sector functions — zero `MeV`/`GeV` / zero `Q_ext`!)
8. `manual_examples/07_s2_sparc_and_shedding.dec` + `.stdout`
9. `manual_examples/08_s2_awareness_stokes_octants.dec` + `.stdout`
10. `manual_examples/09_s2_higgs_and_quarantined_lensing.dec` + `.stdout`
11. `manual_examples/negative_controls/nc07_display_quarantine_leak.dec` + `.expect_error`

### Modify
1. `SOURCE_BINDING.json` (preserve `S0`, `S1b`, historical v1, frozen manifest byte-for-byte; append `S2@3122c94` with verified commit `3122c9444596a9f479e1444c9d497e510a87f825`, tree `d58b43ccf82b428be8f831d11203604e12d66388`, `ref_name: "m4-s2-reference-engine"`, `status: "DRAFT_CANDIDATE_UNDER_S2_REVIEW"`)
2. `crates/aod-core/src/lib.rs` (export `s2_kernel`, `s2_vectors`)
3. `crates/aod-core/src/qext.rs` (add one-way `QExtRatio::round_half_even` & `QExtRatio::format_fixed`, `decimals <= 1024`, zero `-0.000000`, NO unwrap to native)
4. `crates/aod-rt/src/abi.rs` (add `aod_rt_qext_round_half_even` & `aod_rt_qext_format_fixed` C-ABI symbols with `catch_unwind` & `0 <= decimals <= 1024`, register in `runtime_symbol_table()`)
5. `crates/aodc/src/{modules.rs, types.rs, checker.rs, eval.rs, codegen.rs}` (wire one-way `aod::qext::qext_round_half_even` & `aod::qext::qext_format_fixed` strictly behind `quarantine Q_ext:` effect gate)
6. `crates/aodc/src/{lib.rs, main.rs}` (export `verify_s2_dir` + CLI `aodc verify-s2 [--vectors <dir>] [--binding <path>] [--json]`)
7. `crates/aodc/tests/{source_binding.rs, manual_stdlib_parity.rs, manual_verify.rs}` (update `upstream.len()` `2->3` + `S2@3122c94` assertions; update `12->16` in `manual_stdlib_parity.rs` and `dec_files_checked` `12->16` in `manual_verify.rs`)
8. `stdlib/aod/metrology.dec` (preserve `SiMetrology`; append `quarantine Q_ext DisplayQuarantine:` with post-freeze Higgs `MeV`/`GeV` map, LHC comparison, `qext_round_half_even`, `qext_format_fixed`, exact rounding residuals, and half-ULP enclosure)
9. `docs/manual/{main.typ, refs.bib, chapters/02_compile_time_firewalls.typ, chapters/03_aod_stdlib_and_manual_fixtures.typ, chapters/06_validation_and_limits.typ}`
10. `.github/workflows/ci.yml` (`aodc verify-s2` + `--json`; pinned SHA-256-verified `typst v0.15.1` `a6d077d0a95eed5a2eba715b2dae06be954f624ccbf85758a03f389ded33118c`)
11. `README.md`

### Explicitly Untouched
- `/home/antigravity/rpoyau/aod`
- `bindings/aod/{aod_schema.typ, main_registry.json}`
- `tests/fixtures/v2_smoke/` (strictly 3 files)
- `crates/aod-llvm/` (zero edits required)
- `Cargo.lock` (exact 4 crates, zero external deps; unchanged 5-file `unsafe` allowlist)

---

## 2. Acceptance Criteria (`AC-01`..`AC-15`)
- `AC-01`: Amendment binding & immutability (`S0`/`S1b`/v1/`bindings/aod/{aod_schema.typ,main_registry.json}` untouched; `S2@3122c94` verified commit/tree/ref/draft status + vendored `.jsonl` SHA-256).
- `AC-02`: 30/30 golden-vector parity (`24` RC + `6` SD) & exact `scientific_status_summary` (`{AUTHENTICATED_EXACT: 18, CONFORMANT_DECLARED_FIXTURE: 5, RETAINED_DATA_CONFORMANT: 6, UNRESOLVED_PROVENANCE_QUARANTINED: 1}`, passing cases only).
- `AC-03`: Anti-memorization fresh perturbed-input tests across **all 24 positive operations** + fresh invalid cases for **all 3 typed-reject operations** + tie-to-even & negative-zero formatting edge cases.
- `AC-04`: `RC-021..023` REJECT controls & full 11-key schema + domain guards (`sadar_afc_flux_antisymmetry`, `[0,1)` phase, `group_order == 12`, unreduced pairs, `candidate_snapshot`, `max_abs_integer_digits` bool rejection).
- `AC-05`: `RC-024` `BigInt`/`i128` exactness (including `> i128::MAX` perturbed test) & `RC-020` `UNRESOLVED_PROVENANCE_QUARANTINED` preservation (rejecting tampered upgrade).
- `AC-06`: `aodc verify-s2` & `--json` deterministic portable zero-float ASCII JSON (zero host paths, zero timestamps, emits strict JSON even on `FAIL` with exit 1, zero silent fallback on explicit flags).
- `AC-07`: Hostile tamper (1-byte `.jsonl`, expected-value drift with temp `--binding`, injected float, symlinked `.jsonl` & symlinked `--binding`) -> exit 1.
- `AC-08`: Strictly native `stdlib/aod/sectors.dec` (zero `MeV`/`GeV` / zero `Q_ext`) + quarantined `DisplayQuarantine` & one-way `qext_round_half_even`/`qext_format_fixed` builtins (`0 <= decimals <= 1024`) + `QExtLeakageForbidden` negative controls (`nc07`, 2-seg, 3-seg, direct builtin) across `check`, `build` (zero artifact), and `jit`.
- `AC-09`: 5-mode (`VM`, `AOT -O0/-O2`, `ORC LLJIT -O0/-O2`) & hermetic `DecIR v2` parity for `manual_examples/07..09.dec` within `DEFAULT_FUEL = 100_000`; `aodc test manual_examples` -> `16 passed; 0 failed; 16 total`.
- `AC-10`: `aodc manual-verify docs/manual` & `--json` -> `PASS` with `dec_files_checked: 16`, 0 raster/aod-lint errors, including Pass 0 SHA-256 verification of both S2 `.jsonl` files.
- `AC-11`: 4-crate static audit (`Cargo.lock` 4 crates, 0 float tokens, `#![forbid(unsafe_code)]` on `aod-core` & `aodc`, unchanged 5-file `unsafe` allowlist).
- `AC-12`: Zero regression across debug & release workspace tests (`-D warnings`), `v1_examples` 10/10, `v2_smoke` 3/3, `scripts/test_audit_pdf_zero_raster.py` (`python3` & `python3 -O`).
- `AC-13`: `.github/workflows/ci.yml` runs `aodc verify-s2` + `--json` and pins SHA-256-verified `typst v0.15.1` (`a6d077d0a95eed5a2eba715b2dae06be954f624ccbf85758a03f389ded33118c`).
- `AC-14`: Zero mutations to `/home/antigravity/rpoyau/aod`.
- `AC-15`: Explicit non-claims & governance honesty (draft PR status preserved, offline vs CI boundary preserved).
