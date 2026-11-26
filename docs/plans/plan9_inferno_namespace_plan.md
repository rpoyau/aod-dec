# Plan 9 & Inferno Namespace + Default Linux Container Sandbox Plan for `rpoyau/aod-dec`

**Baseline:** `rpoyau/aod-dec` `main@a49c896` (4-crate `std`-only workspace: `aod-core` → `aod-rt` → `aod-llvm` → `aodc`, Trit-Boxed Pure-Wave v3, LLVM 18 ORC LLJIT, 4-stage fixed-point `self_host/aodc_self.dec`).

## Core Design Mandate — Don't Reinvent What Plan 9 & Inferno Solved

1. **One Unified Per-Process Namespace (`namespace(6)` + `cleanname(2)` + `Mhead` union mounts):**
   - No separate "package manager DSL" vs. "virtual filesystem". Code modules (`/lib/...`, `/calc`), dynamic capabilities, and runtime data directories (`/in`, `/out`, `/tmp`) live in **one** per-task Plan 9 mount table manipulated by **only** `clear` (`RFCNAMEG`), `fork` (`RFNAMEG`), `mount`, and `bind` (`replace` / `before` / `after`).
2. **Real Linux Kernel Container by Default for `aodc jit` and AOT:**
   - Linux's `CLONE_NEWNS` was ported directly from Plan 9's `rfork(RFNAMEG)`. Because `aodc jit` compiles machine code into RAM *before* execution, it `fork()`s a worker that enters an unprivileged `CLONE_NEWUSER | CLONE_NEWNS | CLONE_NEWNET | CLONE_NEWIPC | CLONE_NEWUTS` + `pivot_root` jail **with an empty `/` (not even `/bin`, `/usr`, `/etc`, `/home`, or network interfaces exist!)**, bind-mounting *only* the runtime paths declared in the program's Plan 9 `Namespace` (with Landlock LSM + `seccomp-bpf` defense-in-depth).
3. **100% Pure-Wave `aod-dec` Syntax & Repo Invariants:**
   - Zero `{ }` braces, zero unprefixed `"..."` or `str`, zero `if`/`for`/`while`/`mut`, zero floats, zero Python, zero external `crates.io` crates (uses `libc` symbols already linked by Rust `std` inside allowlisted `crates/aod-rt/src/abi.rs`), and `6/6` `aodc audit-dec` hook compliance.

---

## Pure-Wave `aod-dec` Surface Syntax

### 1. Plan 9 `namespace(6)` Declarative Block (World Builder)
```dec
namespace default:
    clear
    mount ro b"./stdlib/aod" -> /lib/aod
    mount ro b"sha256:8f3a9c...12@./vendor/calc-1.0.0" -> /calc
    bind before /fast_patch/calc -> /calc
    mount ramfs -> /tmp
```

### 2. Clean Version-Free & Transport-Free Module Imports
```dec
use /calc:
    add
use /lib/aod/ledger:
    RoleSums
    KernelFour

dec main:
    () -> ():
        emit add(2, 3)
```

---

## Four-Milestone Execution Roadmap

1. **Phase N1 — Plan 9 `cleanname(2)` + `Mhead` COW Ternary Mount Table (`aod-core`) & `RtNamespace` VFS (`aod-rt`):**
   - `crates/aod-core/src/ns.rs` (`#![forbid(unsafe_code)]`): `CleanPath`, `ContentHash`, `UnitId`, `BindMode::{Replace, Before, After}`, `MountMode::{Static, Dynamic}`, `MountPerm`, `Provider<P>`, and persistent `Arc` ternary search mount table (`clear`, `fork`, `mount`, eager `bind`, longest-prefix union walk).
   - `crates/aod-rt/src/ns_fs.rs`: `RtNamespace` (`MemFsProvider`, `ScopedHostDirProvider`, guarded-root write protection) routing `aod::sys` in `aod-rt` and `eval.rs` through explicit Plan 9 mounts instead of ambient `std::fs`.
2. **Phase N2 — Default-On Linux Kernel Container Sandbox for `aodc jit` & AOT (`aod-rt` + `aod-llvm`):**
   - Zero-dependency Linux containerizer in `crates/aod-rt/src/abi.rs` (`fork` + `unshare(USER|NS|NET|IPC|UTS)` + `tmpfs` + `MS_BIND` + `pivot_root` + `umount2(MNT_DETACH)` + `PR_SET_NO_NEW_PRIVS` + Landlock LSM + `seccomp-bpf`) wired into `OrcJitSession` (including persistent pipe worker for zero-fork-overhead `bench`/`learn` steady-state loops) and standalone AOT startup.
3. **Phase N3 — Pure-Wave `use /path:`, `namespace <name>:`, & `UnitId` Compiler Purification (`aodc`):**
   - Guard `min/`/`max/`/`superpose/` lexer ambiguity on `/`-paths, parse & format `use /path:` and `namespace <name>:`, refactor `Type::Struct` / `Type::Enum`, `Checker`, `eval.rs`, and `codegen.rs` from `mod_key: String` to content-addressed `UnitId`, and delete ambient filesystem walks from `crates/aodc/src/modules.rs`.
4. **Phase N4 — Inferno `sign`/`load` Dynamic Capabilities (`mount dyn`), `JitCodeCache`, & `DecIR v4`:**
   - Implement `ModuleSig` (`iface_sha256` including `Effect` firewall mask), `AodRtContext::dyn_vtable` indirect call lowering, content-addressed `JitCodeCache`, hermetic `DecIR v4` (preserving `DecIR v3` and `--compat v1`), manual docs, and full repository verification.
