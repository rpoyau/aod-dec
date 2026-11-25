# AOD / AFC Declared Edge Computation (Dec) Language & Bootstrap Specification

See `docs/FIRST_PRINCIPLES_DERIVATION.md` for the step-by-step first-principles derivation showing how explicit signature combinatorics, register encodings, Landauer erasure avoidance, loop memory, double-slit concurrency, balanced ternary trits, and exact ratios converge directly onto AFC and Alpha-Omega Dynamics.

## 1. Core Architectural Principles

1. **Explicit Signature Combinatorics (Dec Blocks):**
   - Every function, operator, and type constructor (`t32`, `ratio`, `bytes`, `list`, `dict`) is an explicit `Dec` dispatch matrix.
   - Zero C-style casts, zero implicit coercions, zero hidden object vtables. Unlisted combinations have zero admissibility (`adm(e; B) = 0`) and fail at compile time.
   - Each branch forms an explicit branch/hinge/counterbranch triad `AO_mu(C) = <Pi_alpha, H_mu, Pi_Omega>` where `Pi_Omega = Close_B(Pi_alpha, H_mu)`.

2. **Hardware Truth: Balanced Ternary on Binary Silicon & Zero Floats:**
   - Distinct base types: `Null`, ternary fibre `t1` in `{-1, 0, +1}`, finite tesseract vertex `q4` in `{0..15}`, octal anchor `00_8`, and scope `B = (d_T R, omega)`.
   - `t32` packs 32 balanced trits into one 64-bit register (`00` = `0`, `01` = `+1`, `10` = `-1`, `11` = `Null` / wave collision). Phase negation swaps bit lanes in 1 cycle with zero ripple-carry heat.
   - IEEE-754 `float` and irrational Euclidean diagonals are eliminated internally. Fractional phases stay alive as coupled `ratio(num, den)` register pairs; geometry uses integer `quadrance` and `spread`.

3. **Memory as a Closed Loop & Double-Slit Concurrency (FRG):**
   - Static memory is replaced by live walk reduction (`xxxxx... is x` burns dwell; `x is xx` retains boundary crossings; `x is x is x = x` certifies Monon closure).
   - Hardware tiers map to nested curling-curl enclosures and wave cadences:
     - **CPU Registers (`B_0`):** `E_4` Tetron, `1:2` Dimonon (`Pi = 4, RD = 9`).
     - **L1/L2 Cache (`B_1`):** `C_6` Hexon, `3:3` Tritrion (`Pi = 30, RD = 61`).
     - **L3 Mesh Ring (`B_2`):** `C_8` Octon, `3:4` Tetratrion (`Pi = 68, RD = 137`, syncs with `3:3` at `T_sync = 8357`).
     - **System DRAM (`B_3`) & NVMe/SSD (`B_4`):** `C_10` Decon & `C_{2n}` higher even stacks with seat-compatible retention `TTL_C(dF, omega)`.
   - Multi-core and GPU boundaries execute Fractal Field Ring Gate (FRG) cross-return `T_k^x - T_k^0 = C_{ab,k}^x(B)` and Complete-State Awareness (`S_k^+ = (X_k, M_k^C, R_k)`).
   - Discrete GPUs lift surplus via `X_exo` across PCIe (`Lift_{B_r -> B_{r+1}}`); Unified Memory Architectures (Apple Silicon / APU) instantiate the two-field `Disoma` scope `B_12` with zero-copy in-place `X_endo + X_redir` rebalancing.

## 2. Repository Layout

- `docs/FIRST_PRINCIPLES_DERIVATION.md` — Complete 9-step first-principles derivation from `Dec` signature blocks to AFC/AOD hardware learning.
- `docs/SPECIFICATION.md` — Architectural reference and module index.
- `Makefile` — Deterministic `rustc` Stage-0/Stage-1 build and ELF64 verification targets (`x86_64` & `aarch64`).
- `scripts/build_and_verify.sh` — Single-command build and self-test runner.
- `bootstrap/aod_stage0_seed.rs` — Stage-0 seed, curling-curl ledger verifier, double-slit multi-core FRG probe, and minimal ELF64 emitter.
- `bootstrap/aod_self_host_compiler.rs` — `.dec` dwell-reducing parser, §5.17 compile-time/runtime audit verifier, and libc-free ELF64 code generator.
- `bootstrap/aod_stage2_bootstrap.rs` — Emits standalone ELF64 binary (`aodc_stage1`) that inspects Linux stack `(argc, argv)` and executes raw `sys_open`/`sys_write`/`sys_close`/`sys_exit` syscalls without `libc`.
- `dec/kernel.dec` — Core Linux syscall Tick/Click ring gate and runtime `Dec` blocks.
- `dec/00_primitives.dec` .. `dec/05_self_host_compiler_elf64.dec` — Modular `Dec` specifications.
