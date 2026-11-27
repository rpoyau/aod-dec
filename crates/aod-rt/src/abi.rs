#![allow(unsafe_code)]

use std::collections::BTreeMap;
use std::ffi::{c_char, CStr};
use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};

use aod_core::bigint::BigInt;
use aod_core::qext::QExtRatio;
use aod_core::ratio::Ratio;
use aod_core::support::SupportForm;
use aod_core::ternary::{B64, B8, Q4, T1};
use aod_core::trit_int::{swar_superpose_complete, TAG_NULL_SENTINEL};
use aod_core::witness::{Coupling, Fusion};

use crate::arena::DEFAULT_MAX_ARENA_ENTRIES;
use crate::ops::{
    checked_trit_pow, checked_trit_pow_u32, exact_addr8_parse, exact_b64_from_trit_int, exact_b8,
    exact_coupling_contact, exact_q4, exact_qext_binop, exact_support_parse, exact_t1,
    exact_trace_new, exact_trit_int_from_b64, exact_unpack_t9_bits,
    exact_verify_frg_cross_return, exact_verify_sheddic_route, exact_verify_sheddic_route_int,
    RtState, MAX_POW_EXP,
};
use crate::sim::{BoxRegion, ChamberState};
use crate::value::{RtMapKey, RtValue};

pub const DEFAULT_NATIVE_FUEL: i64 = 100_000;
pub const DEFAULT_NATIVE_MAX_DEPTH: i32 = 128;

/// Hardened flat C ABI execution context shared across LLVM IR, AOT ELF, and ORC LLJIT (`P0-2`).
///
/// Exact x86-64 System V layout (`%AodRtContext = type { i64, i32, i32, i8, i8, [6 x i8], ptr }`):
/// - offset 0:  `fuel: i64`
/// - offset 8:  `depth: i32`
/// - offset 12: `max_depth: i32`
/// - offset 16: `failed: u8`
/// - offset 17: `capture_stdout: u8`
/// - offset 18: `_pad: [u8; 6]`
/// - offset 24: `inner: *mut RtState`
#[repr(C)]
pub struct AodRtContext {
    pub fuel: i64,
    pub depth: i32,
    pub max_depth: i32,
    pub failed: u8,
    pub capture_stdout: u8,
    pub _pad: [u8; 6],
    pub inner: *mut RtState,
}

/// Single-register 8-byte Trit-Boxed exact integer (`i64` / `u64` in LLVM IR).
/// - `0 < 0xC000_0000_0000_0000`: inline 32-trit balanced ternary scalar (`00=0, 01=+1, 10=-1`).
/// - `0xD000_0000_0000_0000 | handle`: arena-spilled multi-limb `BigTrit`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecInt(pub u64);

impl DecInt {
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(1);
    pub const NULL_SENTINEL: Self = Self(TAG_NULL_SENTINEL);
}

/// Two-register 16-byte coupled Trit-Boxed exact rational (`%DecRatio = type { i64, i64 }`).
/// - `(num | den) < 0xC000_0000_0000_0000` (`den != 0`): inline canonical 32-trit `(num, den)`.
/// - `num == (0xD000_0000_0000_0000 | handle) && den == 0`: arena-spilled `Ratio`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecRatio {
    pub num: u64,
    pub den: u64,
}

impl DecRatio {
    pub const ZERO: Self = Self { num: 0, den: 1 };
    pub const ONE: Self = Self { num: 1, den: 1 };
}

/// Safe Rust-side RAII owner for an `AodRtContext` used by ORC LLJIT and unit tests.
pub struct OwnedRtContext {
    pub raw: AodRtContext,
}

impl OwnedRtContext {
    pub fn new(fuel: i64, max_depth: i32, capture_stdout: bool) -> Self {
        Self::with_arena_cap(fuel, max_depth, capture_stdout, DEFAULT_MAX_ARENA_ENTRIES)
    }

    pub fn with_arena_cap(
        fuel: i64,
        max_depth: i32,
        capture_stdout: bool,
        max_arena_entries: usize,
    ) -> Self {
        let state = Box::new(RtState::new(capture_stdout, max_arena_entries));
        Self {
            raw: AodRtContext {
                fuel,
                depth: 0,
                max_depth,
                failed: 0,
                capture_stdout: if capture_stdout { 1 } else { 0 },
                _pad: [0; 6],
                inner: Box::into_raw(state),
            },
        }
    }

    pub fn set_cli_args(&mut self, args: Vec<Vec<u8>>) {
        if !self.raw.inner.is_null() {
            let state = unsafe { &mut *self.raw.inner };
            state.cli_args = args;
        }
    }

    pub fn set_rt_namespace(&mut self, rt_ns: crate::ns_fs::RtNamespace) {
        if !self.raw.inner.is_null() {
            let state = unsafe { &mut *self.raw.inner };
            state.rt_ns = rt_ns;
        }
    }

    pub fn set_dyn_vtable(&mut self, vtable: Vec<usize>) {
        if !self.raw.inner.is_null() {
            let state = unsafe { &mut *self.raw.inner };
            state.dyn_vtable = vtable;
        }
    }

    pub fn rt_namespace(&self) -> &crate::ns_fs::RtNamespace {
        &self.state().rt_ns
    }

    pub fn as_mut_ptr(&mut self) -> *mut AodRtContext {
        &mut self.raw
    }

    pub fn state(&self) -> &RtState {
        unsafe { &*self.raw.inner }
    }

    pub fn failed(&self) -> bool {
        self.raw.failed != 0
    }

    pub fn emits(&self) -> &[String] {
        &self.state().emits
    }

    pub fn stdout_string(&self) -> String {
        let mut out = String::new();
        for line in &self.state().emits {
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    pub fn error_message(&self) -> Option<&str> {
        self.state().error.as_deref()
    }

    pub fn fail_with(&mut self, msg: impl Into<String>) {
        self.raw.failed = 1;
        if !self.raw.inner.is_null() {
            let state = unsafe { &mut *self.raw.inner };
            state.record_error(msg.into());
        }
    }
}

impl Drop for OwnedRtContext {
    fn drop(&mut self) {
        if !self.raw.inner.is_null() {
            unsafe {
                drop(Box::from_raw(self.raw.inner));
            }
            self.raw.inner = std::ptr::null_mut();
        }
    }
}

// ============================================================================
// Panic-Safe FFI Boundary Guards
// ============================================================================

#[inline]
unsafe fn read_raw_slice<'a>(ptr: *const u8, len: usize) -> Result<&'a [u8], String> {
    if len == 0 {
        return Ok(&[]);
    }
    if ptr.is_null() {
        return Err("null byte pointer across runtime ABI".to_string());
    }
    Ok(unsafe { std::slice::from_raw_parts(ptr, len) })
}

#[inline]
unsafe fn read_utf8_str<'a>(ptr: *const u8, len: usize) -> Result<&'a str, String> {
    let slice = unsafe { read_raw_slice(ptr, len)? };
    std::str::from_utf8(slice).map_err(|_| "invalid UTF-8 across runtime ABI".to_string())
}

#[inline]
unsafe fn read_u64_slice<'a>(ptr: *const u64, count: usize) -> Result<&'a [u64], String> {
    if count == 0 {
        return Ok(&[]);
    }
    if ptr.is_null() {
        return Err("null handle array pointer across runtime ABI".to_string());
    }
    Ok(unsafe { std::slice::from_raw_parts(ptr, count) })
}

#[inline]
unsafe fn guard_ret<T, F>(ctx: *mut AodRtContext, default: T, f: F) -> T
where
    F: FnOnce(&mut AodRtContext, &mut RtState) -> Result<T, String>,
{
    if ctx.is_null() {
        return default;
    }
    let ctx_ref = unsafe { &mut *ctx };
    if ctx_ref.inner.is_null() {
        ctx_ref.failed = 1;
        return default;
    }
    let state_ref = unsafe { &mut *ctx_ref.inner };
    match catch_unwind(AssertUnwindSafe(|| f(ctx_ref, state_ref))) {
        Ok(Ok(val)) => val,
        Ok(Err(msg)) => {
            ctx_ref.failed = 1;
            state_ref.record_error(msg);
            default
        }
        Err(_) => {
            ctx_ref.failed = 1;
            state_ref.record_error("internal runtime panic caught at C ABI boundary".to_string());
            default
        }
    }
}

#[inline]
unsafe fn write_ratio_out(out: *mut DecRatio, num: u64, den: u64) -> Result<(), String> {
    if out.is_null() {
        return Err("null DecRatio out-pointer".to_string());
    }
    unsafe {
        (*out).num = num;
        (*out).den = den;
    }
    Ok(())
}

// ============================================================================
// Context Lifecycle, Standalone AOT CLI Arg Parsing, & Failure Helpers
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_arena_mark(ctx: *mut AodRtContext) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| Ok(s.arena.len() as u64)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_arena_truncate(ctx: *mut AodRtContext, mark: u64) {
    unsafe {
        let _ = guard_ret(ctx, (), |_, s| {
            s.arena.truncate(mark as usize);
            Ok(())
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_arena_reclaim_handle(
    ctx: *mut AodRtContext,
    mark: u64,
    keep_h: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            if keep_h == 0 || keep_h <= mark {
                s.arena.truncate(mark as usize);
                return Ok(keep_h);
            }
            let kept = s.arena.get(keep_h)?.clone();
            s.arena.truncate(mark as usize);
            s.arena.insert(kept)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_arena_reclaim_int(
    ctx: *mut AodRtContext,
    mark: u64,
    val_packed: u64,
) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.reclaim_dec_int(mark, val_packed)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ctx_init(
    ctx: *mut AodRtContext,
    fuel: i64,
    max_depth: i32,
    capture_stdout: u8,
) {
    if ctx.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let state = Box::new(RtState::with_default_arena(capture_stdout != 0));
        unsafe {
            (*ctx).fuel = fuel;
            (*ctx).depth = 0;
            (*ctx).max_depth = max_depth;
            (*ctx).failed = 0;
            (*ctx).capture_stdout = capture_stdout;
            (*ctx)._pad = [0; 6];
            (*ctx).inner = Box::into_raw(state);
        }
    }));
}

/// Standalone AOT ELF CLI parser: consumes `--fuel <N>` and `--max-depth <N>`, rejects unknown
/// `--*` flags, and forwards positional arguments (plus arguments after `--`) into `state.cli_args`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_cli_parse_args(
    argc: i32,
    argv: *const *const c_char,
    ctx: *mut AodRtContext,
) -> i32 {
    if ctx.is_null() {
        return 1;
    }
    let res = catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
        let mut fuel = DEFAULT_NATIVE_FUEL;
        let mut max_depth = DEFAULT_NATIVE_MAX_DEPTH;
        let mut arena_cap = crate::arena::DEFAULT_MAX_ARENA_ENTRIES;
        let mut mount_specs: Vec<crate::ns_fs::RtMountSpec> = Vec::new();
        let mut unconfined = false;
        let mut positional: Vec<Vec<u8>> = Vec::new();
        if argc > 0 && !argv.is_null() {
            let args = unsafe { std::slice::from_raw_parts(argv, argc as usize) };
            if !args[0].is_null() {
                positional.push(unsafe { CStr::from_ptr(args[0]) }.to_bytes().to_vec());
            }
            let mut i = 1usize;
            let mut after_ddash = false;
            while i < args.len() {
                if args[i].is_null() {
                    i += 1;
                    continue;
                }
                let raw_bytes = unsafe { CStr::from_ptr(args[i]) }.to_bytes();
                if after_ddash {
                    positional.push(raw_bytes.to_vec());
                    i += 1;
                    continue;
                }
                let flag = std::str::from_utf8(raw_bytes)
                    .map_err(|_| "non-UTF8 CLI argument".to_string())?;
                match flag {
                    "--" => {
                        after_ddash = true;
                        i += 1;
                    }
                    "--unconfined" => {
                        unconfined = true;
                        i += 1;
                    }
                    "--mount" => {
                        let next = args
                            .get(i + 1)
                            .copied()
                            .filter(|p| !p.is_null())
                            .ok_or_else(|| "missing spec after --mount".to_string())?;
                        let val_s = unsafe { CStr::from_ptr(next) }
                            .to_str()
                            .map_err(|_| "invalid UTF-8 after --mount".to_string())?;
                        mount_specs.push(crate::ns_fs::RtMountSpec::parse_cli(val_s)?);
                        i += 2;
                    }
                    "--fuel" => {
                        let next = args
                            .get(i + 1)
                            .copied()
                            .filter(|p| !p.is_null())
                            .ok_or_else(|| "missing positive integer after --fuel".to_string())?;
                        let val_s = unsafe { CStr::from_ptr(next) }
                            .to_str()
                            .map_err(|_| "invalid UTF-8 after --fuel".to_string())?;
                        let parsed: i64 = val_s.parse().map_err(|_| {
                            format!(
                                "invalid integer {val_s:?} for --fuel; expected positive integer"
                            )
                        })?;
                        if parsed <= 0 {
                            return Err("--fuel must be a positive integer (>= 1)".to_string());
                        }
                        fuel = parsed;
                        i += 2;
                    }
                    "--max-depth" => {
                        let next = args
                            .get(i + 1)
                            .copied()
                            .filter(|p| !p.is_null())
                            .ok_or_else(|| {
                                "missing positive integer after --max-depth".to_string()
                            })?;
                        let val_s = unsafe { CStr::from_ptr(next) }
                            .to_str()
                            .map_err(|_| "invalid UTF-8 after --max-depth".to_string())?;
                        let parsed: i32 = val_s.parse().map_err(|_| {
                            format!(
                                "invalid integer {val_s:?} for --max-depth; expected positive integer"
                            )
                        })?;
                        if parsed <= 0 {
                            return Err(
                                "--max-depth must be a positive integer (>= 1)".to_string()
                            );
                        }
                        max_depth = parsed;
                        i += 2;
                    }
                    "--arena-cap" => {
                        let next = args
                            .get(i + 1)
                            .copied()
                            .filter(|p| !p.is_null())
                            .ok_or_else(|| {
                                "missing positive integer after --arena-cap".to_string()
                            })?;
                        let val_s = unsafe { CStr::from_ptr(next) }
                            .to_str()
                            .map_err(|_| "invalid UTF-8 after --arena-cap".to_string())?;
                        let parsed: usize = val_s.parse().map_err(|_| {
                            format!(
                                "invalid integer {val_s:?} for --arena-cap; expected positive integer"
                            )
                        })?;
                        if parsed == 0 || parsed > 268_435_456 {
                            return Err("--arena-cap must be in 1..=268435456".to_string());
                        }
                        arena_cap = parsed;
                        i += 2;
                    }
                    other if other.starts_with('-') => {
                        return Err(format!(
                            "unknown argument {other:?} (supported: --fuel <N>, --max-depth <N>, --arena-cap <N>, --mount <spec>, --unconfined)"
                        ));
                    }
                    _ => {
                        positional.push(raw_bytes.to_vec());
                        i += 1;
                    }
                }
            }
        }
        if unconfined && !mount_specs.is_empty() {
            return Err("--unconfined cannot be combined with --mount".to_string());
        }
        let mut rt_ns = if unconfined {
            crate::ns_fs::RtNamespace::unconfined_host()
        } else {
            crate::ns_fs::RtNamespace::empty()
        };
        for spec in &mount_specs {
            rt_ns.apply_spec(spec)?;
        }
        if !rt_ns.is_unconfined() {
            let _ = enter_current_process_sandbox(&mut rt_ns)?;
        }
        unsafe {
            aod_rt_ctx_init(ctx, fuel, max_depth, 0);
            if !(*ctx).inner.is_null() {
                if arena_cap != crate::arena::DEFAULT_MAX_ARENA_ENTRIES {
                    (*(*ctx).inner).arena = crate::arena::RtArena::new(arena_cap);
                }
                (*(*ctx).inner).cli_args = positional;
                (*(*ctx).inner).rt_ns = rt_ns;
            }
        }
        Ok(())
    }));
    match res {
        Ok(Ok(())) => 0,
        Ok(Err(msg)) => {
            eprintln!("aodc: {msg}");
            1
        }
        Err(_) => {
            eprintln!("aodc: internal panic while parsing arguments");
            1
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ctx_finish(ctx: *mut AodRtContext) -> i32 {
    if ctx.is_null() {
        return 1;
    }
    let res = catch_unwind(AssertUnwindSafe(|| -> i32 {
        let ctx_ref = unsafe { &mut *ctx };
        let _ = std::io::stdout().flush();
        let failed = ctx_ref.failed != 0;
        let err_msg = if !ctx_ref.inner.is_null() {
            let state = unsafe { Box::from_raw(ctx_ref.inner) };
            ctx_ref.inner = std::ptr::null_mut();
            state.error
        } else {
            None
        };
        if failed {
            let msg = err_msg.unwrap_or_else(|| "runtime error".to_string());
            eprintln!("aodc: {msg}");
            let _ = std::io::stderr().flush();
            1
        } else {
            0
        }
    }));
    res.unwrap_or(1)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_fail(ctx: *mut AodRtContext, ptr: *const u8, len: usize) {
    unsafe {
        guard_ret(ctx, (), |c, s| {
            let msg = read_utf8_str(ptr, len)?.to_string();
            c.failed = 1;
            s.record_error(msg);
            Ok(())
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_assert_failed(ctx: *mut AodRtContext, line: i64) {
    unsafe {
        guard_ret(ctx, (), |c, s| {
            c.failed = 1;
            s.record_error(format!("line {line}: AssertionFailed: assertion failed"));
            Ok(())
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_budget_exhausted(ctx: *mut AodRtContext, kind: u8) {
    unsafe {
        guard_ret(ctx, (), |c, s| {
            c.failed = 1;
            let msg = if kind == 0 {
                "BudgetExhausted: execution step budget exhausted"
            } else {
                "BudgetExhausted: call depth limit exceeded"
            };
            s.record_error(msg.to_string());
            Ok(())
        });
    }
}

// ============================================================================
// 1-GPR Trit-Boxed `DecInt` (`u64`) Operations (Zero stack out-pointers!)
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_from_str(
    ctx: *mut AodRtContext,
    ptr: *const u8,
    len: usize,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let text = read_utf8_str(ptr, len)?;
            let n = BigInt::parse_decimal(text).map_err(|e| e.to_string())?;
            s.pack_dec_int(n)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_from_i64(ctx: *mut AodRtContext, val: i64) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.pack_dec_int(BigInt::from_i64(val))) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_from_u64(ctx: *mut AodRtContext, uval: u64) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.pack_dec_int(BigInt::from_u64(uval))) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_neg(ctx: *mut AodRtContext, a: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            s.pack_dec_int(-av)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_add(ctx: *mut AodRtContext, a: u64, b: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            let bv = s.unpack_dec_int(b)?;
            s.pack_dec_int(&av + &bv)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_sub(ctx: *mut AodRtContext, a: u64, b: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            let bv = s.unpack_dec_int(b)?;
            s.pack_dec_int(&av - &bv)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_mul(ctx: *mut AodRtContext, a: u64, b: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            let bv = s.unpack_dec_int(b)?;
            s.pack_dec_int(&av * &bv)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_div_floor(ctx: *mut AodRtContext, a: u64, b: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            let bv = s.unpack_dec_int(b)?;
            let q = av.div_floor(&bv).map_err(|e| e.to_string())?;
            s.pack_dec_int(q)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_rem_floor(ctx: *mut AodRtContext, a: u64, b: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            let bv = s.unpack_dec_int(b)?;
            let r = av.rem_floor(&bv).map_err(|e| e.to_string())?;
            s.pack_dec_int(r)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_pow(ctx: *mut AodRtContext, a: u64, exp: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            let ev = s.unpack_dec_int(exp)?;
            let res = checked_trit_pow(&av, &ev)?;
            s.pack_dec_int(res)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_gcd(ctx: *mut AodRtContext, a: u64, b: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            let bv = s.unpack_dec_int(b)?;
            s.pack_dec_int(av.gcd(&bv))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_lcm(ctx: *mut AodRtContext, a: u64, b: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            let bv = s.unpack_dec_int(b)?;
            s.pack_dec_int(av.lcm(&bv))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_superpose_cancel(
    ctx: *mut AodRtContext,
    a: u64,
    b: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            if a < aod_core::trit_int::TOP_TAG_MASK
                && b < aod_core::trit_int::TOP_TAG_MASK
                && aod_core::trit_int::is_valid_trit_word(a)
                && aod_core::trit_int::is_valid_trit_word(b)
            {
                return Ok(swar_superpose_complete(a, b).0);
            }
            let av = s.unpack_dec_int(a)?;
            let bv = s.unpack_dec_int(b)?;
            s.pack_dec_int(av.superpose_cancel(&bv))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_trit_shl(ctx: *mut AodRtContext, a: u64, k: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            let kv = s.unpack_dec_int(k)?;
            let ku = kv
                .to_u64()
                .and_then(|u| u32::try_from(u).ok())
                .ok_or_else(|| format!("trit_shl requires non-negative u32 shift, got {kv}"))?;
            let p3 = BigInt::from_i64(3).pow(ku);
            s.pack_dec_int(&av * &p3)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_trit_shr(ctx: *mut AodRtContext, a: u64, k: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            let kv = s.unpack_dec_int(k)?;
            let ku = kv
                .to_u64()
                .and_then(|u| u32::try_from(u).ok())
                .ok_or_else(|| format!("trit_shr requires non-negative u32 shift, got {kv}"))?;
            let p3 = BigInt::from_i64(3).pow(ku);
            let res = av.div_floor(&p3).map_err(|e| e.to_string())?;
            s.pack_dec_int(res)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_b64_from_unsigned_int(ctx: *mut AodRtContext, a: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            B64::from_unsigned_int(&av)
                .map(B64::value)
                .map_err(|e| e.to_string())
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_exact_div(
    ctx: *mut AodRtContext,
    a: u64,
    b: u64,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = s.unpack_dec_int(a)?;
            let bv = s.unpack_dec_int(b)?;
            let r = Ratio::new(av, bv).map_err(|e| e.to_string())?;
            let (rn, rd) = s.pack_dec_ratio(r)?;
            write_ratio_out(out, rn, rd)
        });
    }
}

/// Three-way comparison returning `-1`, `0`, or `+1` (`i8` == `t1` `<=>`!).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_cmp(ctx: *mut AodRtContext, a: u64, b: u64) -> i8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            let bv = s.unpack_dec_int(b)?;
            Ok(match av.cmp(&bv) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_sgn(ctx: *mut AodRtContext, a: u64) -> i8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            Ok(av.signum())
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_to_t1(ctx: *mut AodRtContext, a: u64) -> i8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            exact_t1(&av)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_to_q4(ctx: *mut AodRtContext, a: u64) -> i8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            exact_q4(&av)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_to_b8(ctx: *mut AodRtContext, a: u64) -> u8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            exact_b8(&av)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_to_b64_view(ctx: *mut AodRtContext, a: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            exact_b64_from_trit_int(&av)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_from_b64_view(ctx: *mut AodRtContext, bits: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let ti = exact_trit_int_from_b64(bits)?;
            s.pack_dec_int(ti)
        })
    }
}

// ============================================================================
// 2-GPR `DecRatio` (`{ u64, u64 }`) Operations
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_reduce_i128(
    ctx: *mut AodRtContext,
    num_lo: u64,
    num_hi: i64,
    den_lo: u64,
    den_hi: i64,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let n = ((num_hi as i128) << 64) | (num_lo as i128);
            let d = ((den_hi as i128) << 64) | (den_lo as i128);
            let r = Ratio::from_i128(n, d).map_err(|e| e.to_string())?;
            let (rn, rd) = s.pack_dec_ratio(r)?;
            write_ratio_out(out, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_from_ints(
    ctx: *mut AodRtContext,
    num: u64,
    den: u64,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let nv = s.unpack_dec_int(num)?;
            let dv = s.unpack_dec_int(den)?;
            let r = Ratio::new(nv, dv).map_err(|e| e.to_string())?;
            let (rn, rd) = s.pack_dec_ratio(r)?;
            write_ratio_out(out, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_from_str(
    ctx: *mut AodRtContext,
    num_ptr: *const u8,
    num_len: usize,
    den_ptr: *const u8,
    den_len: usize,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let ns = read_utf8_str(num_ptr, num_len)?;
            let ds = read_utf8_str(den_ptr, den_len)?;
            let nv = BigInt::parse_decimal(ns).map_err(|e| e.to_string())?;
            let dv = BigInt::parse_decimal(ds).map_err(|e| e.to_string())?;
            let r = Ratio::new(nv, dv).map_err(|e| e.to_string())?;
            let (rn, rd) = s.pack_dec_ratio(r)?;
            write_ratio_out(out, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_neg(
    ctx: *mut AodRtContext,
    na: u64,
    da: u64,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = s.unpack_dec_ratio(na, da)?;
            let (rn, rd) = s.pack_dec_ratio(-av)?;
            write_ratio_out(out, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_add(
    ctx: *mut AodRtContext,
    na: u64,
    da: u64,
    nb: u64,
    db: u64,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = s.unpack_dec_ratio(na, da)?;
            let bv = s.unpack_dec_ratio(nb, db)?;
            let (rn, rd) = s.pack_dec_ratio(&av + &bv)?;
            write_ratio_out(out, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_sub(
    ctx: *mut AodRtContext,
    na: u64,
    da: u64,
    nb: u64,
    db: u64,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = s.unpack_dec_ratio(na, da)?;
            let bv = s.unpack_dec_ratio(nb, db)?;
            let (rn, rd) = s.pack_dec_ratio(&av - &bv)?;
            write_ratio_out(out, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_mul(
    ctx: *mut AodRtContext,
    na: u64,
    da: u64,
    nb: u64,
    db: u64,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = s.unpack_dec_ratio(na, da)?;
            let bv = s.unpack_dec_ratio(nb, db)?;
            let (rn, rd) = s.pack_dec_ratio(&av * &bv)?;
            write_ratio_out(out, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_div(
    ctx: *mut AodRtContext,
    na: u64,
    da: u64,
    nb: u64,
    db: u64,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = s.unpack_dec_ratio(na, da)?;
            let bv = s.unpack_dec_ratio(nb, db)?;
            let q = av.checked_div(&bv).map_err(|e| e.to_string())?;
            let (rn, rd) = s.pack_dec_ratio(q)?;
            write_ratio_out(out, rn, rd)
        });
    }
}

/// Three-way comparison returning `-1`, `0`, or `+1` (`i8` == `t1` `<=>`!).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_cmp(
    ctx: *mut AodRtContext,
    na: u64,
    da: u64,
    nb: u64,
    db: u64,
) -> i8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_ratio(na, da)?;
            let bv = s.unpack_dec_ratio(nb, db)?;
            Ok(match av.cmp(&bv) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_sgn(ctx: *mut AodRtContext, na: u64, da: u64) -> i8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_ratio(na, da)?;
            Ok(av.num().signum())
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_to_int_exact(
    ctx: *mut AodRtContext,
    na: u64,
    da: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_ratio(na, da)?;
            let iv = av.to_int_exact().map_err(|e| e.to_string())?;
            s.pack_dec_int(iv)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_pack_ratio(ctx: *mut AodRtContext, na: u64, da: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let rv = s.unpack_dec_ratio(na, da)?;
            let ni = rv
                .num()
                .to_i64()
                .ok_or_else(|| "ratio exceeds compact 1-word 31-trit budget".to_string())?;
            let di = rv
                .den()
                .to_i64()
                .ok_or_else(|| "ratio exceeds compact 1-word 31-trit budget".to_string())?;
            aod_core::trit_int::pack_compact_ratio_u64(ni, di).ok_or_else(|| {
                "ratio exceeds compact 1-word [num][11][den] 31-trit capacity".to_string()
            })
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_unpack_ratio(
    ctx: *mut AodRtContext,
    bits: u64,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let (n, d) = aod_core::trit_int::unpack_compact_ratio_u64(bits).ok_or_else(|| {
                "invalid compact 1-word [num][11][den] ratio encoding".to_string()
            })?;
            let r = Ratio::from_i64(n, d).map_err(|e| e.to_string())?;
            let (rn, rd) = s.pack_dec_ratio(r)?;
            write_ratio_out(out, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_pow(
    ctx: *mut AodRtContext,
    na: u64,
    da: u64,
    exp: u64,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let base = s.unpack_dec_ratio(na, da)?;
            let ev = s.unpack_dec_int(exp)?;
            let ei = ev.to_i64().ok_or_else(|| {
                format!(
                    "overflow: ratio pow exponent {ev} exceeds safety limit {MAX_POW_EXP}"
                )
            })?;
            let r = if ei >= 0 {
                let eu = u32::try_from(ei).map_err(|_| {
                    format!(
                        "overflow: ratio pow exponent {ei} exceeds safety limit {MAX_POW_EXP}"
                    )
                })?;
                let num = checked_trit_pow_u32(base.num(), eu)?;
                let den = checked_trit_pow_u32(base.den(), eu)?;
                Ratio::new(num, den).map_err(|e| e.to_string())?
            } else {
                let eu = ei
                    .checked_neg()
                    .and_then(|v| u32::try_from(v).ok())
                    .ok_or_else(|| {
                        format!(
                            "overflow: ratio pow exponent {ei} exceeds safety limit {MAX_POW_EXP}"
                        )
                    })?;
                let num = checked_trit_pow_u32(base.den(), eu)?;
                let den = checked_trit_pow_u32(base.num(), eu)?;
                Ratio::new(num, den).map_err(|e| e.to_string())?
            };
            let (rn, rd) = s.pack_dec_ratio(r)?;
            write_ratio_out(out, rn, rd)
        });
    }
}

// ============================================================================
// Emit, Boxing/Unboxing, Strings, Bytes, B8/B64, & Handle Equality
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_emit_null(ctx: *mut AodRtContext) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            s.emit_line("null".to_string());
            Ok(())
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_emit_int(ctx: *mut AodRtContext, a: u64) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = s.unpack_dec_int(a)?;
            s.emit_line(av.to_decimal_string());
            Ok(())
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_emit_bool(ctx: *mut AodRtContext, b: u8) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            s.emit_line(if b != 0 { "true" } else { "false" }.to_string());
            Ok(())
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_emit_qext_bool(ctx: *mut AodRtContext, b: u8) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            s.emit_line(format!("Q_ext({})", if b != 0 { "true" } else { "false" }));
            Ok(())
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_emit_t1(ctx: *mut AodRtContext, v: i8) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let t = T1::new(v).map_err(|e| e.to_string())?;
            s.emit_line(t.render());
            Ok(())
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_emit_b8(ctx: *mut AodRtContext, v: u8) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            s.emit_line(B8(v).render());
            Ok(())
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_emit_b64(ctx: *mut AodRtContext, v: u64) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            s.emit_line(B64(v).render());
            Ok(())
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_emit_q4(ctx: *mut AodRtContext, v: i8) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let q = Q4::new(v as u8).map_err(|e| e.to_string())?;
            s.emit_line(q.render());
            Ok(())
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_emit_ratio(ctx: *mut AodRtContext, na: u64, da: u64) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let rv = s.unpack_dec_ratio(na, da)?;
            s.emit_line(rv.render());
            Ok(())
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_emit_handle(ctx: *mut AodRtContext, h: u64) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let rendered = s.arena.get(h)?.render();
            s.emit_line(rendered);
            Ok(())
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_str_new(
    ctx: *mut AodRtContext,
    ptr: *const u8,
    len: usize,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let text = read_utf8_str(ptr, len)?.to_string();
            s.arena.insert(RtValue::Str(text))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_bytes_new(
    ctx: *mut AodRtContext,
    ptr: *const u8,
    len: usize,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let bs = read_raw_slice(ptr, len)?.to_vec();
            s.arena.insert(RtValue::Bytes(std::sync::Arc::new(bs)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_bytes_from_seq(ctx: *mut AodRtContext, seq_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let bs: std::sync::Arc<Vec<u8>> = match s.arena.get(seq_h)? {
                RtValue::Bytes(b) => b.clone(),
                RtValue::Str(st) => std::sync::Arc::new(st.as_bytes().to_vec()),
                RtValue::Addr8(a) => std::sync::Arc::new(a.render().into_bytes()),
                RtValue::Array(items) | RtValue::List(items) => {
                    let mut out = Vec::with_capacity(items.len());
                    for it in items.iter() {
                        match it {
                            RtValue::B8(b) => out.push(b.0),
                            other => {
                                return Err(format!(
                                    "bytes(...) sequence expects b8 elements, got {}",
                                    other.render()
                                ));
                            }
                        }
                    }
                    std::sync::Arc::new(out)
                }
                other => return Err(format!("bytes(...) unsupported on {}", other.render())),
            };
            s.arena.insert(RtValue::Bytes(bs))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_bytes_from_b64(ctx: *mut AodRtContext, v: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            s.arena
                .insert(RtValue::Bytes(std::sync::Arc::new(v.to_le_bytes().to_vec())))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_bytes_get(ctx: *mut AodRtContext, b_h: u64, idx: u64) -> u8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let iv = s.unpack_dec_int(idx)?;
            let i = iv
                .to_i64()
                .ok_or_else(|| format!("bytes index {iv} out of range"))?;
            let bs = s.get_bytes(b_h)?;
            if i < 0 || (i as usize) >= bs.len() {
                return Err(format!("bytes index {i} out of bounds"));
            }
            Ok(bs[i as usize])
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_bytes_slice(
    ctx: *mut AodRtContext,
    b_h: u64,
    start: u64,
    end: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let sv = s.unpack_dec_int(start)?;
            let ev = s.unpack_dec_int(end)?;
            s.op_bytes_slice(b_h, &sv, &ev)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_bytes_concat(ctx: *mut AodRtContext, a_h: u64, b_h: u64) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.op_bytes_concat(a_h, b_h)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_bytes_sha256(ctx: *mut AodRtContext, b_h: u64) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.op_bytes_sha256(b_h)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_bytes_hex(ctx: *mut AodRtContext, b_h: u64) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.op_bytes_hex(b_h)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_str_from_bytes(ctx: *mut AodRtContext, b_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let bs = s.get_bytes(b_h)?;
            let text = std::str::from_utf8(bs)
                .map_err(|_| "str_bytes input is not valid UTF-8".to_string())?
                .to_string();
            s.arena.insert(RtValue::Str(text))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_le_bytes_from_int(
    ctx: *mut AodRtContext,
    a: u64,
    kind: u8,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let n = s.unpack_dec_int(a)?;
            let out = match kind {
                0 => {
                    let u = n
                        .to_u64()
                        .filter(|&v| v <= u16::MAX as u64)
                        .ok_or_else(|| format!("u16_le_bytes out of 0..=65535 range: {n}"))?
                        as u16;
                    u.to_le_bytes().to_vec()
                }
                1 => {
                    let u = n
                        .to_u64()
                        .filter(|&v| v <= u32::MAX as u64)
                        .ok_or_else(|| format!("u32_le_bytes out of 0..=4294967295 range: {n}"))?
                        as u32;
                    u.to_le_bytes().to_vec()
                }
                2 => {
                    let v = n
                        .to_i64()
                        .and_then(|x| i32::try_from(x).ok())
                        .ok_or_else(|| format!("i32_le_bytes out of i32 range: {n}"))?;
                    v.to_le_bytes().to_vec()
                }
                3 => {
                    let v = n
                        .to_i64()
                        .ok_or_else(|| format!("i64_le_bytes out of i64 range: {n}"))?;
                    v.to_le_bytes().to_vec()
                }
                _ => return Err(format!("invalid le_bytes kind {kind}")),
            };
            s.arena.insert(RtValue::Bytes(std::sync::Arc::new(out)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_bytes_push(ctx: *mut AodRtContext, b_h: u64, byte: u8) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let mut out = s.get_bytes(b_h)?.to_vec();
            out.push(byte);
            s.arena.insert(RtValue::Bytes(std::sync::Arc::new(out)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sys_cli_args(ctx: *mut AodRtContext) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.op_sys_cli_args()) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sys_read_file_bytes(ctx: *mut AodRtContext, path_h: u64) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.op_sys_read_file_bytes(path_h)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sys_write_executable_elf(
    ctx: *mut AodRtContext,
    path_h: u64,
    img_h: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let n = s.op_sys_write_executable_elf(path_h, img_h)?;
            s.pack_dec_int(n)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sys_accept_stream(ctx: *mut AodRtContext, path_h: u64) -> i8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let ok = s.op_sys_accept_stream(path_h)?;
            Ok(i8::from(ok))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sys_stream_local_addr(ctx: *mut AodRtContext, path_h: u64) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.op_sys_stream_local_addr(path_h)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sys_read_stream_line(
    ctx: *mut AodRtContext,
    path_h: u64,
    max_raw: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let max_b = s.unpack_dec_int(max_raw)?;
            s.op_sys_read_stream_line(path_h, &max_b)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sys_read_stream_frame(
    ctx: *mut AodRtContext,
    path_h: u64,
    max_raw: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let max_b = s.unpack_dec_int(max_raw)?;
            s.op_sys_read_stream_frame(path_h, &max_b)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sys_write_stream_bytes(
    ctx: *mut AodRtContext,
    path_h: u64,
    payload_h: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let n = s.op_sys_write_stream_bytes(path_h, payload_h)?;
            s.pack_dec_int(n)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sys_write_stream_frame(
    ctx: *mut AodRtContext,
    path_h: u64,
    payload_h: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let n = s.op_sys_write_stream_frame(path_h, payload_h)?;
            s.pack_dec_int(n)
        })
    }
}

fn rt_values_equal(a: &RtValue, b: &RtValue) -> bool {
    match (a, b) {
        (RtValue::Array(xs) | RtValue::List(xs), RtValue::Array(ys) | RtValue::List(ys)) => {
            xs.len() == ys.len() && xs.iter().zip(ys.iter()).all(|(x, y)| rt_values_equal(x, y))
        }
        (RtValue::Tuple(xs), RtValue::Tuple(ys)) => {
            xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| rt_values_equal(x, y))
        }
        _ => a == b,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_handle_eq(ctx: *mut AodRtContext, a: u64, b: u64) -> u8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.arena.get(a)?;
            let bv = s.arena.get(b)?;
            Ok(if rt_values_equal(av, bv) { 1 } else { 0 })
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_handle_cmp(ctx: *mut AodRtContext, a: u64, b: u64) -> i8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.arena.get(a)?;
            let bv = s.arena.get(b)?;
            let ka = RtMapKey::from_value(av)?;
            let kb = RtMapKey::from_value(bv)?;
            Ok(match ka.cmp(&kb) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_unit(ctx: *mut AodRtContext) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.arena.insert(RtValue::Unit)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_null(ctx: *mut AodRtContext) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.arena.insert(RtValue::Null)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_from_int(ctx: *mut AodRtContext, a: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            s.arena.insert(RtValue::Int(av))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_to_int(ctx: *mut AodRtContext, h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let n = match s.arena.get(h)? {
                RtValue::Int(n) => n.clone(),
                other => return Err(format!("expected int value, got {}", other.render())),
            };
            s.pack_dec_int(n)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_from_bool(ctx: *mut AodRtContext, b: u8) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.arena.insert(RtValue::Bool(b != 0))) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_to_bool(ctx: *mut AodRtContext, h: u64) -> u8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| match s.arena.get(h)? {
            RtValue::Bool(b) => Ok(if *b { 1 } else { 0 }),
            other => Err(format!("expected bool value, got {}", other.render())),
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_from_qext_bool(ctx: *mut AodRtContext, b: u8) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.arena.insert(RtValue::QExtBool(b != 0))) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_to_qext_bool(ctx: *mut AodRtContext, h: u64) -> u8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| match s.arena.get(h)? {
            RtValue::QExtBool(b) => Ok(if *b { 1 } else { 0 }),
            other => Err(format!("expected QExt<bool> value, got {}", other.render())),
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_from_t1(ctx: *mut AodRtContext, v: i8) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let t = T1::new(v).map_err(|e| e.to_string())?;
            s.arena.insert(RtValue::T1(t))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_to_t1(ctx: *mut AodRtContext, h: u64) -> i8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| match s.arena.get(h)? {
            RtValue::T1(t) => Ok(t.value()),
            other => Err(format!("expected t1 value, got {}", other.render())),
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_from_b8(ctx: *mut AodRtContext, v: u8) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.arena.insert(RtValue::B8(B8(v)))) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_to_b8(ctx: *mut AodRtContext, h: u64) -> u8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| match s.arena.get(h)? {
            RtValue::B8(b) => Ok(b.0),
            other => Err(format!("expected b8 value, got {}", other.render())),
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_from_b64(ctx: *mut AodRtContext, v: u64) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.arena.insert(RtValue::B64(B64(v)))) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_to_b64(ctx: *mut AodRtContext, h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| match s.arena.get(h)? {
            RtValue::B64(b) => Ok(b.0),
            other => Err(format!("expected b64 value, got {}", other.render())),
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_from_q4(ctx: *mut AodRtContext, v: i8) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let q = Q4::new(v as u8).map_err(|e| e.to_string())?;
            s.arena.insert(RtValue::Q4(q))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_to_q4(ctx: *mut AodRtContext, h: u64) -> i8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| match s.arena.get(h)? {
            RtValue::Q4(q) => Ok(q.value() as i8),
            other => Err(format!("expected q4 value, got {}", other.render())),
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_from_ratio(
    ctx: *mut AodRtContext,
    na: u64,
    da: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let rv = s.unpack_dec_ratio(na, da)?;
            s.arena.insert(RtValue::Ratio(rv))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_to_ratio(
    ctx: *mut AodRtContext,
    h: u64,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let rv = match s.arena.get(h)? {
                RtValue::Ratio(r) => r.clone(),
                other => return Err(format!("expected ratio value, got {}", other.render())),
            };
            let (rn, rd) = s.pack_dec_ratio(rv)?;
            write_ratio_out(out, rn, rd)
        });
    }
}

// ============================================================================
// Domain Primitives: Addr8, Support, Window, Range, Box, Witness, Fusion,
// Coupling, Ledger, Wavelet, & QExt
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_addr8_parse(
    ctx: *mut AodRtContext,
    ptr: *const u8,
    len: usize,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let text = read_utf8_str(ptr, len)?;
            let a = exact_addr8_parse(text)?;
            s.arena.insert(RtValue::Addr8(a))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_addr8_from_str(ctx: *mut AodRtContext, str_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let text = s.get_str_or_utf8_bytes(str_h, "addr8")?;
            let a = exact_addr8_parse(&text)?;
            s.arena.insert(RtValue::Addr8(a))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_support_parse(
    ctx: *mut AodRtContext,
    ptr: *const u8,
    len: usize,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let text = read_utf8_str(ptr, len)?;
            let sf = exact_support_parse(text)?;
            s.arena.insert(RtValue::Support(sf))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_support_from_str(ctx: *mut AodRtContext, str_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let text = s.get_str_or_utf8_bytes(str_h, "support")?;
            let sf = exact_support_parse(&text)?;
            s.arena.insert(RtValue::Support(sf))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_support_new(
    ctx: *mut AodRtContext,
    p: u64,
    q: u64,
    len: u64,
    yro: u8,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let pv = s.unpack_dec_int(p)?;
            let qv = s.unpack_dec_int(q)?;
            let lv = s.unpack_dec_int(len)?;
            let sf = SupportForm::new(pv, qv, lv, yro != 0).map_err(|e| e.to_string())?;
            s.arena.insert(RtValue::Support(sf))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_support_pi(ctx: *mut AodRtContext, h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let pi = s.get_support(h)?.pi().map_err(|e| e.to_string())?;
            s.pack_dec_int(pi)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_support_rd(ctx: *mut AodRtContext, h: u64, h_mu: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let hv = s.unpack_dec_int(h_mu)?;
            let rd = s.get_support(h)?.rd(&hv).map_err(|e| e.to_string())?;
            s.pack_dec_int(rd)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_support_pressure(
    ctx: *mut AodRtContext,
    h: u64,
    c3: u64,
    h_mu: u64,
    w: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let c3v = s.unpack_dec_int(c3)?;
            let hv = s.unpack_dec_int(h_mu)?;
            let wv = s.unpack_dec_int(w)?;
            let res = s
                .get_support(h)?
                .pressure(&c3v, &hv, &wv)
                .map_err(|e| e.to_string())?;
            s.pack_dec_int(res)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_support_overhang(
    ctx: *mut AodRtContext,
    h: u64,
    c3: u64,
    h_mu: u64,
    w: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let c3v = s.unpack_dec_int(c3)?;
            let hv = s.unpack_dec_int(h_mu)?;
            let wv = s.unpack_dec_int(w)?;
            let res = s
                .get_support(h)?
                .overhang(&c3v, &hv, &wv)
                .map_err(|e| e.to_string())?;
            s.pack_dec_int(res)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_shedding_excess(
    ctx: *mut AodRtContext,
    p_d: u64,
    c_close: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let pv = s.unpack_dec_int(p_d)?;
            let cv = s.unpack_dec_int(c_close)?;
            let audit = SupportForm::shedding_audit(&pv, &cv).map_err(|e| e.to_string())?;
            s.pack_dec_int(audit.x_shedding)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sadar_promote(
    ctx: *mut AodRtContext,
    c3: u64,
    rho: u64,
    att_n: u64,
    att_d: u64,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let c3v = s.unpack_dec_int(c3)?;
            let rv = s.unpack_dec_int(rho)?;
            let av = s.unpack_dec_ratio(att_n, att_d)?;
            let res = s.op_sadar_promote(&c3v, &rv, av)?;
            let (rn, rd) = s.pack_dec_ratio(res)?;
            write_ratio_out(out, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_probability(
    ctx: *mut AodRtContext,
    neg: u64,
    hinge: u64,
    pos: u64,
    role: i8,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let nv = s.unpack_dec_int(neg)?;
            let hv = s.unpack_dec_int(hinge)?;
            let pv = s.unpack_dec_int(pos)?;
            let res = s.op_probability(&nv, &hv, &pv, role)?;
            let (rn, rd) = s.pack_dec_ratio(res)?;
            write_ratio_out(out, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_window_new(ctx: *mut AodRtContext, u: u64, v: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let uv = s
                .unpack_dec_int(u)?
                .to_i64()
                .ok_or_else(|| "window start out of i64 range".to_string())?;
            let vv = s
                .unpack_dec_int(v)?
                .to_i64()
                .ok_or_else(|| "window end out of i64 range".to_string())?;
            if uv >= vv {
                return Err(format!("window ({uv}, {vv}] requires u < v"));
            }
            s.arena.insert(RtValue::Window(uv, vv))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_range_new(ctx: *mut AodRtContext, a: u64, b: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s
                .unpack_dec_int(a)?
                .to_i64()
                .ok_or_else(|| "range start out of i64 range".to_string())?;
            let bv = s
                .unpack_dec_int(b)?
                .to_i64()
                .ok_or_else(|| "range end out of i64 range".to_string())?;
            s.arena.insert(RtValue::Range(av, bv))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_range_bounds(
    ctx: *mut AodRtContext,
    r_h: u64,
    out_start: *mut i64,
    out_end: *mut i64,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| match s.arena.get(r_h)? {
            RtValue::Range(a, b) => {
                if !out_start.is_null() {
                    *out_start = *a;
                }
                if !out_end.is_null() {
                    *out_end = *b;
                }
                Ok(())
            }
            other => Err(format!("expected range handle, got {}", other.render())),
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_range_to_list(ctx: *mut AodRtContext, r_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |c, s| {
            let (a, b) = match s.arena.get(r_h)? {
                RtValue::Range(a, b) => (*a, *b),
                other => return Err(format!("expected range handle, got {}", other.render())),
            };
            if b <= a {
                return s.arena.insert(RtValue::List(std::sync::Arc::new(Vec::new())));
            }
            let len = (b as i128 - a as i128) as u64;
            if (c.fuel as u64) < len {
                c.fuel = 0;
                return Err("BudgetExhausted: execution step budget exhausted".to_string());
            }
            c.fuel -= len as i64;
            let mut items = Vec::with_capacity(len as usize);
            for cur in a..b {
                items.push(RtValue::Int(BigInt::from_i64(cur)));
            }
            s.arena.insert(RtValue::List(std::sync::Arc::new(items)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_box_new(
    ctx: *mut AodRtContext,
    r0: u64,
    r1: u64,
    r2: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let get_r = |h: u64| match s.arena.get(h)? {
                RtValue::Range(a, b) => Ok((*a, *b)),
                other => Err(format!("box expects range, got {}", other.render())),
            };
            let b = BoxRegion {
                x: get_r(r0)?,
                y: get_r(r1)?,
                z: get_r(r2)?,
            };
            s.arena.insert(RtValue::Box3(b))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_boundary_of(ctx: *mut AodRtContext, h: u64) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.op_boundary_of(h)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_trace_new(
    ctx: *mut AodRtContext,
    sc_h: u64,
    lin_h: u64,
    q4: i8,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let sc = s.get_str_or_utf8_bytes(sc_h, "trace")?;
            let lin = s.get_str_or_utf8_bytes(lin_h, "trace")?;
            let t = exact_trace_new(&sc, &lin, q4)?;
            s.arena.insert(RtValue::Trace(Box::new(t)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_trace_step(
    ctx: *mut AodRtContext,
    t_h: u64,
    q4: i8,
    t1: i8,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let q = Q4::new(q4 as u8).map_err(|e| e.to_string())?;
            let o = T1::new(t1).map_err(|e| e.to_string())?;
            let next = s.get_trace(t_h)?.step(q, o).map_err(|e| e.to_string())?;
            s.arena.insert(RtValue::Trace(Box::new(next)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_trace_hinge(ctx: *mut AodRtContext, t_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let next = s.get_trace(t_h)?.hinge().map_err(|e| e.to_string())?;
            s.arena.insert(RtValue::Trace(Box::new(next)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_trace_dwell(ctx: *mut AodRtContext, t_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let next = s.get_trace(t_h)?.dwell().map_err(|e| e.to_string())?;
            s.arena.insert(RtValue::Trace(Box::new(next)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_trace_close(ctx: *mut AodRtContext, t_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let next = s.get_trace(t_h)?.close().map_err(|e| e.to_string())?;
            s.arena.insert(RtValue::Trace(Box::new(next)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_trace_phases(ctx: *mut AodRtContext, t_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let p = s.get_trace(t_h)?.phases().map_err(|e| e.to_string())?;
            s.pack_dec_int(BigInt::from_u64(p as u64))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_trace_beats(ctx: *mut AodRtContext, t_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let b = s.get_trace(t_h)?.beats().map_err(|e| e.to_string())?;
            s.pack_dec_int(BigInt::from_u64(b as u64))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_trace_rate(
    ctx: *mut AodRtContext,
    t_h: u64,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let r = s.get_trace(t_h)?.rate().map_err(|e| e.to_string())?;
            let (rn, rd) = s.pack_dec_ratio(r)?;
            write_ratio_out(out, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_fold(
    ctx: *mut AodRtContext,
    a_h: u64,
    b_h: u64,
    sc_h: u64,
    lin_h: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let a = s.get_trace(a_h)?.clone();
            let b = s.get_trace(b_h)?.clone();
            let sc = s.get_str_or_utf8_bytes(sc_h, "fold")?;
            let lin = s.get_str_or_utf8_bytes(lin_h, "fold")?;
            let f = Fusion::fold(&a, &b, sc, lin).map_err(|e| e.to_string())?;
            s.arena.insert(RtValue::Fusion(Box::new(f)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_outer(ctx: *mut AodRtContext, f_h: u64, q4: i8) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let q = Q4::new(q4 as u8).map_err(|e| e.to_string())?;
            let t = s.get_fusion(f_h)?.outer(q).map_err(|e| e.to_string())?;
            s.arena.insert(RtValue::Trace(Box::new(t)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_fuse(ctx: *mut AodRtContext, f_h: u64, t_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let f = s.get_fusion(f_h)?.clone();
            let t = s.get_trace(t_h)?.clone();
            let out = f.fuse(&t).map_err(|e| e.to_string())?;
            s.arena.insert(RtValue::Trace(Box::new(out)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_coupling_new(
    ctx: *mut AodRtContext,
    a_h: u64,
    b_h: u64,
    w_h: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let a = s.get_trace(a_h)?.clone();
            let b = s.get_trace(b_h)?.clone();
            let w = s.get_str_or_utf8_bytes(w_h, "coupling")?;
            let c = Coupling::new(&a, &b, w).map_err(|e| e.to_string())?;
            s.arena.insert(RtValue::Coupling(Box::new(c)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_coupling_contact(
    ctx: *mut AodRtContext,
    c_h: u64,
    a: u64,
    b: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            let bv = s.unpack_dec_int(b)?;
            let c = s.get_coupling(c_h)?.clone();
            let next = exact_coupling_contact(&c, &av, &bv)?;
            s.arena.insert(RtValue::Coupling(Box::new(next)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_coupling_cadence(
    ctx: *mut AodRtContext,
    c_h: u64,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let r = s.get_coupling(c_h)?.cadence().map_err(|e| e.to_string())?;
            let (rn, rd) = s.pack_dec_ratio(r)?;
            write_ratio_out(out, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_coupling_lag(ctx: *mut AodRtContext, c_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let l = s.get_coupling(c_h)?.lag().map_err(|e| e.to_string())?;
            s.pack_dec_int(l)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_coupling_drift(ctx: *mut AodRtContext, c_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let d = s.get_coupling(c_h)?.drift().map_err(|e| e.to_string())?;
            s.pack_dec_int(d)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_window_dedup_count(
    ctx: *mut AodRtContext,
    lin_h: u64,
    sc_h: u64,
    cuts_h: u64,
    wins_h: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let cnt = s.op_window_dedup_count(lin_h, sc_h, cuts_h, wins_h)?;
            s.pack_dec_int(cnt)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_native_cadence_ratio(
    ctx: *mut AodRtContext,
    id_a_h: u64,
    q_a: u64,
    id_r_h: u64,
    q_r: u64,
    coupled: u8,
    bc: u64,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let qav = s.unpack_dec_int(q_a)?;
            let qrv = s.unpack_dec_int(q_r)?;
            let bcv = s.unpack_dec_int(bc)?;
            let r = s.op_native_cadence_ratio(id_a_h, &qav, id_r_h, &qrv, coupled != 0, &bcv)?;
            let (rn, rd) = s.pack_dec_ratio(r)?;
            write_ratio_out(out, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_single_contact_assoc(
    ctx: *mut AodRtContext,
    wid_h: u64,
    pid_h: u64,
    rid_h: u64,
    win_h: u64,
    bc: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let bcv = s.unpack_dec_int(bc)?;
            s.op_single_contact_assoc(wid_h, pid_h, rid_h, win_h, &bcv)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_recipient_eval_demo(
    ctx: *mut AodRtContext,
    ek: u64,
    sk: u64,
    win_h: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let ekv = s.unpack_dec_int(ek)?;
            let skv = s.unpack_dec_int(sk)?;
            s.op_recipient_eval_demo(&ekv, &skv, win_h)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_qext_ext_decimal(ctx: *mut AodRtContext, str_h: u64) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.op_qext_ext_decimal(str_h)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_qext_from_ratio(
    ctx: *mut AodRtContext,
    na: u64,
    da: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let rv = s.unpack_dec_ratio(na, da)?;
            s.arena
                .insert(RtValue::QExtRatio(QExtRatio::from_native_frozen(rv)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_qext_binop(
    ctx: *mut AodRtContext,
    op: u8,
    a_h: u64,
    b_h: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let a = s.get_qext_ratio(a_h)?.clone();
            let b = s.get_qext_ratio(b_h)?.clone();
            let res = exact_qext_binop(op, &a, &b)?;
            s.arena.insert(RtValue::QExtRatio(res))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_qext_neg(ctx: *mut AodRtContext, a_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let a = s.get_qext_ratio(a_h)?.clone();
            s.arena.insert(RtValue::QExtRatio(a.neg()))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_qext_cmp(ctx: *mut AodRtContext, a_h: u64, b_h: u64) -> i8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let a = s.get_qext_ratio(a_h)?;
            let b = s.get_qext_ratio(b_h)?;
            Ok(match a.cmp(b) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_qext_render(ctx: *mut AodRtContext, a_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let rendered = s.get_qext_ratio(a_h)?.render_exact().into_bytes();
            s.arena
                .insert(RtValue::QExtBytes(std::sync::Arc::new(rendered)))
        })
    }
}

fn checked_qext_decimals(v: &BigInt) -> Result<usize, String> {
    let max_d = aod_core::s2_kernel::MAX_ROUNDING_DECIMALS;
    let d = v
        .to_i64()
        .ok_or_else(|| format!("qext decimals {v} out of bounds (expected 0..={max_d})"))?;
    if d < 0 || (d as usize) > max_d {
        return Err(format!(
            "qext decimals {d} out of bounds (expected 0..={max_d})"
        ));
    }
    Ok(d as usize)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_qext_round_half_even(
    ctx: *mut AodRtContext,
    a_h: u64,
    decimals: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let q = s.get_qext_ratio(a_h)?.clone();
            let dv = s.unpack_dec_int(decimals)?;
            let dec = checked_qext_decimals(&dv)?;
            let rounded = q.round_half_even(dec).map_err(|e| e.to_string())?;
            s.arena.insert(RtValue::QExtRatio(rounded))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_qext_format_fixed(
    ctx: *mut AodRtContext,
    a_h: u64,
    decimals: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let q = s.get_qext_ratio(a_h)?.clone();
            let dv = s.unpack_dec_int(decimals)?;
            let dec = checked_qext_decimals(&dv)?;
            let formatted = q.format_fixed(dec).map_err(|e| e.to_string())?.into_bytes();
            s.arena
                .insert(RtValue::QExtBytes(std::sync::Arc::new(formatted)))
        })
    }
}

// ============================================================================
// Containers & Composite Types (Array, List, Map, Struct, Enum, Tuple)
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_array_new(
    ctx: *mut AodRtContext,
    elems_ptr: *const u64,
    count: usize,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let handles = read_u64_slice(elems_ptr, count)?;
            let mut items = Vec::with_capacity(handles.len());
            for &h in handles {
                items.push(s.arena.get(h)?.clone());
            }
            s.arena.insert(RtValue::Array(std::sync::Arc::new(items)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_list_new(
    ctx: *mut AodRtContext,
    elems_ptr: *const u64,
    count: usize,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let handles = read_u64_slice(elems_ptr, count)?;
            let mut items = Vec::with_capacity(handles.len());
            for &h in handles {
                items.push(s.arena.get(h)?.clone());
            }
            s.arena.insert(RtValue::List(std::sync::Arc::new(items)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_seq_get(ctx: *mut AodRtContext, seq_h: u64, idx: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let iv = s.unpack_dec_int(idx)?;
            let v = s.op_seq_get(seq_h, &iv)?;
            s.arena.insert(v)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_seq_get_int(ctx: *mut AodRtContext, seq_h: u64, idx: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let iv = s.unpack_dec_int(idx)?;
            let v = s.op_seq_get(seq_h, &iv)?;
            match v {
                RtValue::Int(n) => s.pack_dec_int(n),
                other => Err(format!("expected int element, got {}", other.render())),
            }
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_seq_get_raw_idx(
    ctx: *mut AodRtContext,
    seq_h: u64,
    idx: i64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let v = s.op_seq_get(seq_h, &BigInt::from_i64(idx))?;
            s.arena.insert(v)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_array_set(
    ctx: *mut AodRtContext,
    arr_h: u64,
    idx: u64,
    val_h: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let iv = s.unpack_dec_int(idx)?;
            s.op_array_set(arr_h, &iv, val_h)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_array_set_int(
    ctx: *mut AodRtContext,
    arr_h: u64,
    idx: u64,
    val_packed: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let iv = s.unpack_dec_int(idx)?;
            let i = iv
                .to_i64()
                .ok_or_else(|| format!("index {iv} out of i64 range"))?;
            let val = RtValue::Int(s.unpack_dec_int(val_packed)?);
            let (is_array, mut out) = match s.arena.get(arr_h)? {
                RtValue::Array(items) => (true, (**items).clone()),
                RtValue::List(items) => (false, (**items).clone()),
                other => {
                    return Err(format!(
                        "set_at expects array or list, got {}",
                        other.render()
                    ));
                }
            };
            if i < 0 || (i as usize) >= out.len() {
                return Err(format!("set_at index {i} out of bounds"));
            }
            out[i as usize] = val;
            if is_array {
                s.arena.insert(RtValue::Array(std::sync::Arc::new(out)))
            } else {
                s.arena.insert(RtValue::List(std::sync::Arc::new(out)))
            }
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_seq_len_raw(ctx: *mut AodRtContext, seq_h: u64) -> i64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| match s.arena.get(seq_h)? {
            RtValue::Array(items) | RtValue::List(items) => Ok(items.len() as i64),
            RtValue::Bytes(bs) => Ok(bs.len() as i64),
            other => Err(format!("expected sequence handle, got {}", other.render())),
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_seq_sieve(
    ctx: *mut AodRtContext,
    mask_h: u64,
    vals_h: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let mask_items = match s.arena.get(mask_h)? {
                RtValue::Array(items) | RtValue::List(items) => items.clone(),
                other => {
                    return Err(format!(
                        "sieve `?>` expects boolean mask sequence, got {}",
                        other.render()
                    ));
                }
            };
            let val_items = match s.arena.get(vals_h)? {
                RtValue::Array(items) | RtValue::List(items) => items.clone(),
                other => {
                    return Err(format!(
                        "sieve `?>` expects value sequence, got {}",
                        other.render()
                    ));
                }
            };
            if mask_items.len() != val_items.len() {
                return Err(format!(
                    "sieve `?>` length mismatch: mask len {} != values len {}",
                    mask_items.len(),
                    val_items.len()
                ));
            }
            let mut out = Vec::new();
            for (m, v) in mask_items.iter().zip(val_items.iter()) {
                match m {
                    RtValue::Bool(true) => out.push(v.clone()),
                    RtValue::Bool(false) => {}
                    other => {
                        return Err(format!(
                            "sieve `?>` mask requires bool elements, got {}",
                            other.render()
                        ));
                    }
                }
            }
            s.arena.insert(RtValue::List(std::sync::Arc::new(out)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_seq_argmin_argmax(
    ctx: *mut AodRtContext,
    seq_h: u64,
    is_max: u8,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let items = match s.arena.get(seq_h)? {
                RtValue::Array(items) | RtValue::List(items) => items,
                other => {
                    return Err(format!(
                        "argmin/argmax expects sequence, got {}",
                        other.render()
                    ));
                }
            };
            if items.is_empty() {
                return Err("argmin/argmax requires non-empty sequence".to_string());
            }
            let mut best_idx = 0usize;
            let mut best_key = RtMapKey::from_value(&items[0])?;
            for (i, item) in items.iter().enumerate().skip(1) {
                let k = RtMapKey::from_value(item)?;
                let better = if is_max != 0 {
                    k > best_key
                } else {
                    k < best_key
                };
                if better {
                    best_key = k;
                    best_idx = i;
                }
            }
            s.pack_dec_int(BigInt::from_u64(best_idx as u64))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_len(ctx: *mut AodRtContext, h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let n = s.op_len(h)?;
            s.pack_dec_int(n)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_seq_push(ctx: *mut AodRtContext, seq_h: u64, elem_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let elem = s.arena.get(elem_h)?.clone();
            let mut out = match s.arena.get(seq_h)? {
                RtValue::Array(items) | RtValue::List(items) => (**items).clone(),
                other => return Err(format!("push expects sequence, got {}", other.render())),
            };
            out.push(elem);
            s.arena.insert(RtValue::List(std::sync::Arc::new(out)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_seq_push_int(
    ctx: *mut AodRtContext,
    seq_h: u64,
    val_packed: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let elem = RtValue::Int(s.unpack_dec_int(val_packed)?);
            let mut out = match s.arena.get(seq_h)? {
                RtValue::Array(items) | RtValue::List(items) => (**items).clone(),
                other => return Err(format!("push expects sequence, got {}", other.render())),
            };
            out.push(elem);
            s.arena.insert(RtValue::List(std::sync::Arc::new(out)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sort_int_pairs(
    ctx: *mut AodRtContext,
    ks_h: u64,
    vs_h: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let ks = match s.arena.get(ks_h)? {
                RtValue::Array(items) | RtValue::List(items) => items.clone(),
                other => {
                    return Err(format!(
                        "sort_int_pairs expects key sequence, got {}",
                        other.render()
                    ));
                }
            };
            let vs = match s.arena.get(vs_h)? {
                RtValue::Array(items) | RtValue::List(items) => items.clone(),
                other => {
                    return Err(format!(
                        "sort_int_pairs expects value sequence, got {}",
                        other.render()
                    ));
                }
            };
            if ks.len() != vs.len() {
                return Err(format!(
                    "sort_int_pairs length mismatch: {} vs {}",
                    ks.len(),
                    vs.len()
                ));
            }
            let mut pairs: Vec<(aod_core::bigint::BigInt, aod_core::bigint::BigInt)> =
                Vec::with_capacity(ks.len());
            for (k, v) in ks.iter().zip(vs.iter()) {
                match (k, v) {
                    (RtValue::Int(ki), RtValue::Int(vi)) => pairs.push((ki.clone(), vi.clone())),
                    _ => {
                        return Err("sort_int_pairs expects integer sequences".to_string());
                    }
                }
            }
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            let mut ok = Vec::with_capacity(pairs.len());
            let mut ov = Vec::with_capacity(pairs.len());
            for (k, v) in pairs {
                ok.push(RtValue::Int(k));
                ov.push(RtValue::Int(v));
            }
            s.arena.insert(RtValue::Tuple(vec![
                RtValue::List(std::sync::Arc::new(ok)),
                RtValue::List(std::sync::Arc::new(ov)),
            ]))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_seq_concat(ctx: *mut AodRtContext, a_h: u64, b_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let a = match s.arena.get(a_h)? {
                RtValue::Array(items) | RtValue::List(items) => items.clone(),
                other => return Err(format!("concat expects sequence, got {}", other.render())),
            };
            let b = match s.arena.get(b_h)? {
                RtValue::Array(items) | RtValue::List(items) => items.clone(),
                other => return Err(format!("concat expects sequence, got {}", other.render())),
            };
            let mut out = Vec::with_capacity(a.len() + b.len());
            out.extend(a.iter().cloned());
            out.extend(b.iter().cloned());
            s.arena.insert(RtValue::List(std::sync::Arc::new(out)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_seq_slice(
    ctx: *mut AodRtContext,
    seq_h: u64,
    start: u64,
    end: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let sv = s.unpack_dec_int(start)?;
            let ev = s.unpack_dec_int(end)?;
            let si = sv
                .to_i64()
                .ok_or_else(|| format!("slice start {sv} out of i64 range"))?;
            let ei = ev
                .to_i64()
                .ok_or_else(|| format!("slice end {ev} out of i64 range"))?;
            let sliced = match s.arena.get(seq_h)? {
                RtValue::Array(items) | RtValue::List(items) => {
                    if si < 0 || ei < si || (ei as usize) > items.len() {
                        return Err(format!(
                            "slice({si}..{ei}) out of bounds for len {}",
                            items.len()
                        ));
                    }
                    items[si as usize..ei as usize].to_vec()
                }
                other => return Err(format!("slice expects sequence, got {}", other.render())),
            };
            s.arena.insert(RtValue::List(std::sync::Arc::new(sliced)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_seq_contains(
    ctx: *mut AodRtContext,
    seq_h: u64,
    elem_h: u64,
) -> u8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let target = s.arena.get(elem_h)?;
            let found = match s.arena.get(seq_h)? {
                RtValue::Array(items) | RtValue::List(items) => {
                    items.iter().any(|x| rt_values_equal(x, target))
                }
                other => return Err(format!("contains expects sequence, got {}", other.render())),
            };
            Ok(if found { 1 } else { 0 })
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_range_seq(ctx: *mut AodRtContext, a: u64, b: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |c, s| {
            let av = s
                .unpack_dec_int(a)?
                .to_i64()
                .ok_or_else(|| "range_seq start out of i64 range".to_string())?;
            let bv = s
                .unpack_dec_int(b)?
                .to_i64()
                .ok_or_else(|| "range_seq end out of i64 range".to_string())?;
            if bv <= av {
                return s.arena.insert(RtValue::List(std::sync::Arc::new(Vec::new())));
            }
            let len = (bv as i128 - av as i128) as u64;
            if len > 1_000_000 {
                return Err(format!(
                    "range {av}..{bv} exceeds 1_000_000 element materialization limit"
                ));
            }
            if (c.fuel as u64) < len {
                c.fuel = 0;
                return Err("BudgetExhausted: execution step budget exhausted".to_string());
            }
            c.fuel -= len as i64;
            let mut items = Vec::with_capacity(len as usize);
            for cur in av..bv {
                items.push(RtValue::Int(BigInt::from_i64(cur)));
            }
            s.arena.insert(RtValue::List(std::sync::Arc::new(items)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_seq_buf_new(ctx: *mut AodRtContext, cap: i64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let c = usize::try_from(cap.max(0)).unwrap_or(0).min(1_000_000);
            let id = s.seq_bufs.len() as u64;
            s.seq_bufs.push(Vec::with_capacity(c));
            Ok(id)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_seq_buf_push(ctx: *mut AodRtContext, buf_id: u64, elem_h: u64) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let elem = s.arena.get(elem_h)?.clone();
            let buf = s
                .seq_bufs
                .get_mut(buf_id as usize)
                .ok_or_else(|| format!("invalid seq_buf id {buf_id}"))?;
            buf.push(elem);
            Ok(())
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_seq_buf_finish(
    ctx: *mut AodRtContext,
    buf_id: u64,
    is_array: u8,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let idx = buf_id as usize;
            if idx >= s.seq_bufs.len() {
                return Err(format!("invalid seq_buf id {buf_id}"));
            }
            let items = if idx + 1 == s.seq_bufs.len() {
                s.seq_bufs.pop().unwrap_or_default()
            } else {
                std::mem::take(&mut s.seq_bufs[idx])
            };
            if is_array != 0 {
                s.arena.insert(RtValue::Array(std::sync::Arc::new(items)))
            } else {
                s.arena.insert(RtValue::List(std::sync::Arc::new(items)))
            }
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_tuple_new(
    ctx: *mut AodRtContext,
    elems_ptr: *const u64,
    count: usize,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let handles = read_u64_slice(elems_ptr, count)?;
            let mut items = Vec::with_capacity(handles.len());
            for &h in handles {
                items.push(s.arena.get(h)?.clone());
            }
            s.arena.insert(RtValue::Tuple(items))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_tuple_get(ctx: *mut AodRtContext, tup_h: u64, idx: usize) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let elem = match s.arena.get(tup_h)? {
                RtValue::Tuple(elems) => elems
                    .get(idx)
                    .cloned()
                    .ok_or_else(|| format!("tuple index {idx} out of bounds"))?,
                other => return Err(format!("expected tuple handle, got {}", other.render())),
            };
            s.arena.insert(elem)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_map_new(
    ctx: *mut AodRtContext,
    keys_ptr: *const u64,
    vals_ptr: *const u64,
    count: usize,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let khs = read_u64_slice(keys_ptr, count)?;
            let vhs = read_u64_slice(vals_ptr, count)?;
            let mut m = BTreeMap::new();
            for (&kh, &vh) in khs.iter().zip(vhs) {
                let kv = s.arena.get(kh)?.clone();
                let vv = s.arena.get(vh)?.clone();
                let mk = RtMapKey::from_value(&kv)?;
                m.insert(mk, vv);
            }
            s.arena.insert(RtValue::Map(Box::new(m)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_map_get(ctx: *mut AodRtContext, map_h: u64, key_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let v = s.op_map_get(map_h, key_h)?;
            s.arena.insert(v)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_map_set(
    ctx: *mut AodRtContext,
    map_h: u64,
    key_h: u64,
    val_h: u64,
) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.op_map_set(map_h, key_h, val_h)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_map_contains(
    ctx: *mut AodRtContext,
    map_h: u64,
    key_h: u64,
) -> u8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            Ok(if s.op_map_contains(map_h, key_h)? {
                1
            } else {
                0
            })
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_map_keys_list(ctx: *mut AodRtContext, map_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let keys: Vec<RtValue> = match s.arena.get(map_h)? {
                RtValue::Map(m) => m.keys().map(RtMapKey::to_value).collect(),
                other => return Err(format!("expected map handle, got {}", other.render())),
            };
            s.arena.insert(RtValue::List(std::sync::Arc::new(keys)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_map_entries_list(ctx: *mut AodRtContext, map_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let entries: Vec<RtValue> = match s.arena.get(map_h)? {
                RtValue::Map(m) => m
                    .iter()
                    .map(|(k, v)| RtValue::Tuple(vec![k.to_value(), v.clone()]))
                    .collect(),
                other => return Err(format!("expected map handle, got {}", other.render())),
            };
            s.arena.insert(RtValue::List(std::sync::Arc::new(entries)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_struct_new(
    ctx: *mut AodRtContext,
    mod_h: u64,
    name_h: u64,
    field_name_handles: *const u64,
    field_val_handles: *const u64,
    count: usize,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let nhs = read_u64_slice(field_name_handles, count)?;
            let vhs = read_u64_slice(field_val_handles, count)?;
            s.op_struct_new(mod_h, name_h, nhs, vhs)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_enum_new(
    ctx: *mut AodRtContext,
    mod_h: u64,
    name_h: u64,
    var_h: u64,
    tag: i32,
    payloads_ptr: *const u64,
    count: usize,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let phs = read_u64_slice(payloads_ptr, count)?;
            s.op_enum_new(mod_h, name_h, var_h, tag, phs)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_enum_tag(ctx: *mut AodRtContext, e_h: u64) -> i32 {
    unsafe {
        guard_ret(ctx, -1, |_, s| match s.arena.get(e_h)? {
            RtValue::Enum(ev) => Ok(ev.tag),
            other => Err(format!("expected enum handle, got {}", other.render())),
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_enum_payload(
    ctx: *mut AodRtContext,
    e_h: u64,
    idx: usize,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let val = match s.arena.get(e_h)? {
                RtValue::Enum(ev) => ev
                    .payloads
                    .get(idx)
                    .cloned()
                    .ok_or_else(|| format!("enum payload index {idx} out of bounds"))?,
                other => return Err(format!("expected enum handle, got {}", other.render())),
            };
            s.arena.insert(val)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_field_get(
    ctx: *mut AodRtContext,
    h: u64,
    field_ptr: *const u8,
    field_len: usize,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let fname = read_utf8_str(field_ptr, field_len)?.to_string();
            let v = s.op_field_get(h, &fname)?;
            s.arena.insert(v)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_field_get_int(
    ctx: *mut AodRtContext,
    h: u64,
    field_ptr: *const u8,
    field_len: usize,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let fname = read_utf8_str(field_ptr, field_len)?.to_string();
            let v = s.op_field_get(h, &fname)?;
            match v {
                RtValue::Int(n) => s.pack_dec_int(n),
                other => Err(format!(
                    "expected int field .{fname}, got {}",
                    other.render()
                )),
            }
        })
    }
}

// ============================================================================
// Sandbox Simulation C ABI
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sim_builder_new(
    ctx: *mut AodRtContext,
    chamber_name_h: u64,
    space_box_h: u64,
    walls_code: u8,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            s.op_sim_builder_new(chamber_name_h, space_box_h, walls_code)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sim_builder_set_dynamics(
    ctx: *mut AodRtContext,
    b_h: u64,
    dx: u64,
    dy: u64,
    dz: u64,
    has_ttl: u8,
    contact_ttl_word: u64,
    fuse_on_resonance: u8,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let dxv = s.unpack_dec_int(dx)?;
            let dyv = s.unpack_dec_int(dy)?;
            let dzv = s.unpack_dec_int(dz)?;
            let ttl_i64 = if has_ttl != 0 {
                let tv = s.unpack_dec_int(contact_ttl_word)?;
                let n = tv
                    .to_i64()
                    .ok_or_else(|| format!("contact_ttl {tv} out of range"))?;
                if n < 0 {
                    return Err(format!("chamber contact_ttl must be >= 0, got {n}"));
                }
                n
            } else {
                -1
            };
            s.op_sim_builder_set_dynamics(b_h, &dxv, &dyv, &dzv, ttl_i64, fuse_on_resonance)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sim_builder_add_clock(
    ctx: *mut AodRtContext,
    b_h: u64,
    name_h: u64,
    period: u64,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let pv = s.unpack_dec_int(period)?;
            s.op_sim_builder_add_clock(b_h, name_h, &pv)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sim_builder_add_carrier(
    ctx: *mut AodRtContext,
    b_h: u64,
    support_h: u64,
    x: u64,
    y: u64,
    z: u64,
    dir_code: u8,
    orient: i8,
    shells: u64,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let xv = s.unpack_dec_int(x)?;
            let yv = s.unpack_dec_int(y)?;
            let zv = s.unpack_dec_int(z)?;
            let sv = s.unpack_dec_int(shells)?;
            s.op_sim_builder_add_carrier(b_h, support_h, &xv, &yv, &zv, dir_code, orient, &sv)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sim_builder_add_decl_obs(
    ctx: *mut AodRtContext,
    b_h: u64,
    kind_code: u8,
    name_h: u64,
    region_box_h: u64,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            s.op_sim_builder_add_decl_obs(b_h, kind_code, name_h, region_box_h)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sim_builder_add_proc_obs(
    ctx: *mut AodRtContext,
    b_h: u64,
    name_h: u64,
    cb_ptr: *const std::ffi::c_void,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            if cb_ptr.is_null() {
                return Err("null procedural observer callback".to_string());
            }
            s.op_sim_builder_add_proc_obs(b_h, name_h, cb_ptr as usize)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sim_run(
    ctx: *mut AodRtContext,
    b_h: u64,
    ticks: u64,
) -> u64 {
    unsafe { aod_rt_sim_run_ex(ctx, b_h, ticks, 0) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sim_run_ex(
    ctx: *mut AodRtContext,
    b_h: u64,
    ticks: u64,
    parallel_observers: u8,
) -> u64 {
    if ctx.is_null() {
        return 0;
    }
    let inner_is_null = unsafe { (*ctx).inner.is_null() };
    if inner_is_null {
        unsafe {
            (*ctx).failed = 1;
        }
        return 0;
    }
    let res = catch_unwind(AssertUnwindSafe(|| -> Result<u64, String> {
        let (builder, tick_count) = {
            let state = unsafe { &mut *(*ctx).inner };
            let tv = state.unpack_dec_int(ticks)?;
            let ti = tv
                .to_i64()
                .ok_or_else(|| format!("simulate tick count {tv} out of i64 range"))?;
            if ti < 0 {
                return Err("simulate tick count must be non-negative".to_string());
            }
            (state.take_sim_builder(b_h)?, ti as u64)
        };

        let mut chamber = ChamberState::new(
            builder.chamber_name,
            builder.space,
            builder.walls,
            builder.carriers,
            builder.clocks,
        )
        .with_dynamics(builder.dynamics);
        let mut decl_observers = builder.decl_observers;
        let proc_observers = builder.proc_observers;

        let use_parallel = parallel_observers != 0 && proc_observers.len() > 1;

        let base_arena = if use_parallel {
            let state = unsafe { &*(*ctx).inner };
            Some(state.arena.clone())
        } else {
            None
        };

        struct WorkerOutcome {
            fuel_after: i64,
            failed: bool,
            error: Option<String>,
            emits: Vec<String>,
        }

        for _ in 0..tick_count {
            let cur_fuel = unsafe { (*ctx).fuel };
            if cur_fuel <= 0 {
                return Err("BudgetExhausted: execution step budget exhausted".to_string());
            }
            unsafe {
                (*ctx).fuel = cur_fuel - 1;
            }

            chamber.step().map_err(|e| e.to_string())?;
            let snap = chamber.snapshot();
            for obs in &mut decl_observers {
                obs.observe_snapshot(&snap);
            }
            if proc_observers.is_empty() {
                continue;
            }

            if !use_parallel {
                let snap_h = {
                    let state = unsafe { &mut *(*ctx).inner };
                    state.arena.insert(RtValue::ChamberView(Box::new(snap)))?
                };
                for (_, cb_addr) in &proc_observers {
                    let cb: unsafe extern "C" fn(*mut AodRtContext, u64) =
                        unsafe { std::mem::transmute(*cb_addr) };
                    unsafe {
                        cb(ctx, snap_h);
                    }
                    let failed_now = unsafe { (*ctx).failed != 0 };
                    if failed_now {
                        return Ok(0);
                    }
                }
            } else {
                let start_fuel = unsafe { (*ctx).fuel };
                let parent_depth = unsafe { (*ctx).depth };
                let parent_max_depth = unsafe { (*ctx).max_depth };
                let base_arena_ref = base_arena
                    .as_ref()
                    .expect("base_arena snapshotted when use_parallel is true");
                let snap_ref = &snap;

                let outcomes: Vec<WorkerOutcome> = std::thread::scope(|scope| {
                    let mut handles = Vec::with_capacity(proc_observers.len());
                    for &(_, cb_addr) in &proc_observers {
                        handles.push(scope.spawn(move || -> WorkerOutcome {
                            let mut wctx = OwnedRtContext::new(start_fuel, parent_max_depth, true);
                            wctx.raw.depth = parent_depth;
                            let snap_h = {
                                let wstate = unsafe { &mut *wctx.raw.inner };
                                wstate.arena = base_arena_ref.clone();
                                match wstate
                                    .arena
                                    .insert(RtValue::ChamberView(Box::new(snap_ref.clone())))
                                {
                                    Ok(h) => h,
                                    Err(msg) => {
                                        wctx.fail_with(msg);
                                        0
                                    }
                                }
                            };
                            if !wctx.failed() {
                                let cb: unsafe extern "C" fn(*mut AodRtContext, u64) =
                                    unsafe { std::mem::transmute(cb_addr) };
                                let call_res = catch_unwind(AssertUnwindSafe(|| unsafe {
                                    cb(wctx.as_mut_ptr(), snap_h);
                                }));
                                if call_res.is_err() {
                                    wctx.fail_with(
                                        "internal runtime panic in parallel observer callback",
                                    );
                                }
                            }
                            let wstate = wctx.state();
                            WorkerOutcome {
                                fuel_after: wctx.raw.fuel,
                                failed: wctx.failed(),
                                error: wstate.error.clone(),
                                emits: wstate.emits.clone(),
                            }
                        }));
                    }
                    handles
                        .into_iter()
                        .map(|h| match h.join() {
                            Ok(o) => o,
                            Err(_) => WorkerOutcome {
                                fuel_after: start_fuel,
                                failed: true,
                                error: Some(
                                    "internal runtime panic in parallel observer worker"
                                        .to_string(),
                                ),
                                emits: Vec::new(),
                            },
                        })
                        .collect()
                });

                let mut remaining_fuel = start_fuel;
                for (idx, outcome) in outcomes.into_iter().enumerate() {
                    let used_i = start_fuel.saturating_sub(outcome.fuel_after);
                    if used_i <= remaining_fuel {
                        remaining_fuel = remaining_fuel.saturating_sub(used_i);
                        {
                            let pstate = unsafe { &mut *(*ctx).inner };
                            for line in outcome.emits {
                                pstate.emit_line(line);
                            }
                        }
                        if outcome.failed {
                            unsafe {
                                (*ctx).fuel = remaining_fuel;
                                (*ctx).failed = 1;
                                if let Some(err) = outcome.error {
                                    (*(*ctx).inner).record_error(err);
                                }
                            }
                            return Ok(0);
                        }
                    } else {
                        unsafe {
                            (*ctx).fuel = remaining_fuel;
                        }
                        let snap_h = {
                            let pstate = unsafe { &mut *(*ctx).inner };
                            pstate.arena.insert(RtValue::ChamberView(Box::new(snap.clone())))?
                        };
                        let cb_addr = proc_observers[idx].1;
                        let cb: unsafe extern "C" fn(*mut AodRtContext, u64) =
                            unsafe { std::mem::transmute(cb_addr) };
                        unsafe {
                            cb(ctx, snap_h);
                        }
                        let failed_now = unsafe { (*ctx).failed != 0 };
                        if failed_now {
                            return Ok(0);
                        }
                        remaining_fuel = unsafe { (*ctx).fuel };
                    }
                }
                unsafe {
                    (*ctx).fuel = remaining_fuel;
                }
            }
        }

        let state = unsafe { &mut *(*ctx).inner };
        state.finalize_sim_result(chamber, decl_observers)
    }));

    match res {
        Ok(Ok(h)) => h,
        Ok(Err(msg)) => {
            unsafe {
                (*ctx).failed = 1;
                if !(*ctx).inner.is_null() {
                    (*(*ctx).inner).record_error(msg);
                }
            }
            0
        }
        Err(_) => {
            unsafe {
                (*ctx).failed = 1;
                if !(*ctx).inner.is_null() {
                    (*(*ctx).inner)
                        .record_error("internal runtime panic in aod_rt_sim_run_ex".to_string());
                }
            }
            0
        }
    }
}

// ============================================================================
// PR 10–12 Intrinsics: Contractions (`sum`/`dot`), `zip_seq`, Map, Ratio Fields,
// `QExt<bytes>`, Sheddic/FRG Audits, & Register Pack/Unpack
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_num(ctx: *mut AodRtContext, na: u64, da: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let rv = s.unpack_dec_ratio(na, da)?;
            s.pack_dec_int(rv.num().clone())
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_den(ctx: *mut AodRtContext, na: u64, da: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let rv = s.unpack_dec_ratio(na, da)?;
            s.pack_dec_int(rv.den().clone())
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_abs(ctx: *mut AodRtContext, a: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            s.pack_dec_int(av.abs())
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_abs(
    ctx: *mut AodRtContext,
    na: u64,
    da: u64,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let rv = s.unpack_dec_ratio(na, da)?;
            let (rn, rd) = s.pack_dec_ratio(rv.abs())?;
            write_ratio_out(out, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_pack_t32(ctx: *mut AodRtContext, a: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            let bits = exact_b64_from_trit_int(&av)?;
            s.pack_dec_int(BigInt::from_u64(bits))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_unpack_t32(ctx: *mut AodRtContext, a: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            let bits = av
                .to_u64()
                .ok_or_else(|| format!("packed 32-trit word outside 64-bit domain: {av}"))?;
            let ti = exact_trit_int_from_b64(bits)?;
            s.pack_dec_int(ti)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_unpack_t9_b64(ctx: *mut AodRtContext, bits: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let ti = exact_unpack_t9_bits(bits)?;
            s.pack_dec_int(ti)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_unpack_t9_int(ctx: *mut AodRtContext, a: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.unpack_dec_int(a)?;
            let bits = av
                .to_u64()
                .ok_or_else(|| format!("packed t9 outside 18-bit domain: {av}"))?;
            let ti = exact_unpack_t9_bits(bits)?;
            s.pack_dec_int(ti)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_t1_superpose_cancel(
    ctx: *mut AodRtContext,
    a: i8,
    b: i8,
) -> i8 {
    unsafe {
        guard_ret(ctx, 0, |_, _| {
            let ta = T1::new(a).map_err(|e| e.to_string())?;
            let tb = T1::new(b).map_err(|e| e.to_string())?;
            Ok(ta.superpose_cancel(tb).value())
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_verify_sheddic_route(
    ctx: *mut AodRtContext,
    x_shed: u64,
    target_enc: u8,
) -> u8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let xs = s.unpack_dec_int(x_shed)?;
            Ok(u8::from(exact_verify_sheddic_route(&xs, target_enc)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_verify_sheddic_route_int(
    ctx: *mut AodRtContext,
    x_shed: u64,
    target_enc: u64,
) -> u8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let xs = s.unpack_dec_int(x_shed)?;
            let enc = s.unpack_dec_int(target_enc)?;
            Ok(u8::from(exact_verify_sheddic_route_int(&xs, &enc)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_verify_frg_cross_return(
    ctx: *mut AodRtContext,
    t0: u64,
    tx: u64,
    cx: u64,
) -> u8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let v0 = s.unpack_dec_int(t0)?;
            let vx = s.unpack_dec_int(tx)?;
            let vc = s.unpack_dec_int(cx)?;
            Ok(u8::from(exact_verify_frg_cross_return(&v0, &vx, &vc)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_window_bounds(
    ctx: *mut AodRtContext,
    w_h: u64,
    out_start: *mut i64,
    out_end: *mut i64,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let (u, v) = s.get_window(w_h)?;
            if !out_start.is_null() {
                *out_start = u;
            }
            if !out_end.is_null() {
                *out_end = v;
            }
            Ok(())
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_zip_seq(ctx: *mut AodRtContext, a_h: u64, b_h: u64) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.op_zip_seq(a_h, b_h)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_map_remove(
    ctx: *mut AodRtContext,
    map_h: u64,
    key_h: u64,
) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.op_map_remove(map_h, key_h)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_map_get_or(
    ctx: *mut AodRtContext,
    map_h: u64,
    key_h: u64,
    default_h: u64,
) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.op_map_get_or(map_h, key_h, default_h)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_struct_set(
    ctx: *mut AodRtContext,
    s_h: u64,
    field_ptr: *const u8,
    field_len: usize,
    val_h: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let fname = read_utf8_str(field_ptr, field_len)?.to_string();
            s.op_struct_set(s_h, &fname, val_h)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sum_int(ctx: *mut AodRtContext, seq_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let mut acc = BigInt::zero();
            for item in s.get_seq_slice(seq_h)? {
                match item {
                    RtValue::Int(n) => acc = &acc + n,
                    _ => return Err("sum(int): expected int element".to_string()),
                }
            }
            s.pack_dec_int(acc)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sum_b8(ctx: *mut AodRtContext, seq_h: u64) -> u8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let mut acc: u8 = 0;
            for v in s.get_any_seq_values(seq_h)? {
                match v {
                    RtValue::B8(b) => {
                        acc = acc
                            .checked_add(b.0)
                            .ok_or_else(|| "sum(b8): b8 addition overflow".to_string())?;
                    }
                    _ => return Err("sum(b8): expected b8 element".to_string()),
                }
            }
            Ok(acc)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sum_t1(ctx: *mut AodRtContext, seq_h: u64) -> i8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let mut acc = T1::ZERO;
            for item in s.get_seq_slice(seq_h)? {
                match item {
                    RtValue::T1(v) => {
                        acc = T1::new(acc.value() + v.value()).map_err(|e| e.to_string())?;
                    }
                    _ => return Err("sum(t1): expected t1 element".to_string()),
                }
            }
            Ok(acc.value())
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sum_ratio(
    ctx: *mut AodRtContext,
    seq_h: u64,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let mut acc = Ratio::zero();
            for item in s.get_seq_slice(seq_h)? {
                match item {
                    RtValue::Ratio(r) => acc = &acc + r,
                    _ => return Err("sum(ratio): expected ratio element".to_string()),
                }
            }
            let (rn, rd) = s.pack_dec_ratio(acc)?;
            write_ratio_out(out, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sum_qext_ratio(ctx: *mut AodRtContext, seq_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let mut acc = QExtRatio::from_native_frozen(Ratio::zero());
            for item in s.get_seq_slice(seq_h)? {
                match item {
                    RtValue::QExtRatio(q) => acc = acc.checked_add(q),
                    _ => return Err("sum(QExt<ratio>): expected QExt<ratio> element".to_string()),
                }
            }
            s.arena.insert(RtValue::QExtRatio(acc))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_dot_int(ctx: *mut AodRtContext, a_h: u64, b_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let xs = s.get_seq_slice(a_h)?;
            let ys = s.get_seq_slice(b_h)?;
            if xs.len() != ys.len() {
                return Err(format!(
                    "dot(a, b) sequence length mismatch: {} != {}",
                    xs.len(),
                    ys.len()
                ));
            }
            let mut acc = BigInt::zero();
            for (x, y) in xs.iter().zip(ys.iter()) {
                match (x, y) {
                    (RtValue::Int(a), RtValue::Int(b)) => acc = &acc + &(a * b),
                    _ => return Err("dot(int): expected int elements".to_string()),
                }
            }
            s.pack_dec_int(acc)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_dot_b8(ctx: *mut AodRtContext, a_h: u64, b_h: u64) -> u8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let xs = s.get_any_seq_values(a_h)?;
            let ys = s.get_any_seq_values(b_h)?;
            if xs.len() != ys.len() {
                return Err(format!(
                    "dot(a, b) sequence length mismatch: {} != {}",
                    xs.len(),
                    ys.len()
                ));
            }
            let mut acc: u8 = 0;
            for (x, y) in xs.into_iter().zip(ys) {
                match (x, y) {
                    (RtValue::B8(a), RtValue::B8(b)) => {
                        let p = a
                            .0
                            .checked_mul(b.0)
                            .ok_or_else(|| "dot(b8): b8 multiplication overflow".to_string())?;
                        acc = acc
                            .checked_add(p)
                            .ok_or_else(|| "dot(b8): b8 addition overflow".to_string())?;
                    }
                    _ => return Err("dot(b8): expected b8 elements".to_string()),
                }
            }
            Ok(acc)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_dot_t1(ctx: *mut AodRtContext, a_h: u64, b_h: u64) -> i8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let xs = s.get_seq_slice(a_h)?;
            let ys = s.get_seq_slice(b_h)?;
            if xs.len() != ys.len() {
                return Err(format!(
                    "dot(a, b) sequence length mismatch: {} != {}",
                    xs.len(),
                    ys.len()
                ));
            }
            let mut acc = T1::ZERO;
            for (x, y) in xs.iter().zip(ys.iter()) {
                match (x, y) {
                    (RtValue::T1(a), RtValue::T1(b)) => {
                        let p = a.value() * b.value();
                        acc = T1::new(acc.value() + p).map_err(|e| e.to_string())?;
                    }
                    _ => return Err("dot(t1): expected t1 elements".to_string()),
                }
            }
            Ok(acc.value())
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_dot_ratio(
    ctx: *mut AodRtContext,
    a_h: u64,
    b_h: u64,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let xs = s.get_seq_slice(a_h)?;
            let ys = s.get_seq_slice(b_h)?;
            if xs.len() != ys.len() {
                return Err(format!(
                    "dot(a, b) sequence length mismatch: {} != {}",
                    xs.len(),
                    ys.len()
                ));
            }
            let mut acc = Ratio::zero();
            for (x, y) in xs.iter().zip(ys.iter()) {
                match (x, y) {
                    (RtValue::Ratio(a), RtValue::Ratio(b)) => acc = &acc + &(a * b),
                    _ => return Err("dot(ratio): expected ratio elements".to_string()),
                }
            }
            let (rn, rd) = s.pack_dec_ratio(acc)?;
            write_ratio_out(out, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_dot_qext_ratio(
    ctx: *mut AodRtContext,
    a_h: u64,
    b_h: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let xs = s.get_seq_slice(a_h)?;
            let ys = s.get_seq_slice(b_h)?;
            if xs.len() != ys.len() {
                return Err(format!(
                    "dot(a, b) sequence length mismatch: {} != {}",
                    xs.len(),
                    ys.len()
                ));
            }
            let mut acc = QExtRatio::from_native_frozen(Ratio::zero());
            for (x, y) in xs.iter().zip(ys.iter()) {
                match (x, y) {
                    (RtValue::QExtRatio(a), RtValue::QExtRatio(b)) => {
                        acc = acc.checked_add(&a.checked_mul(b));
                    }
                    _ => return Err("dot(QExt<ratio>): expected QExt<ratio> elements".to_string()),
                }
            }
            s.arena.insert(RtValue::QExtRatio(acc))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_qext_abs(ctx: *mut AodRtContext, q_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let q = s.get_qext_ratio(q_h)?.clone();
            s.arena.insert(RtValue::QExtRatio(q.abs()))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_qext_to_bytes_exact(ctx: *mut AodRtContext, q_h: u64) -> u64 {
    unsafe { aod_rt_qext_render(ctx, q_h) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_qext_render_bytes(ctx: *mut AodRtContext, q_h: u64) -> u64 {
    unsafe { aod_rt_qext_render(ctx, q_h) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_qext_format_bytes_fixed(
    ctx: *mut AodRtContext,
    q_h: u64,
    decimals: u64,
) -> u64 {
    unsafe { aod_rt_qext_format_fixed(ctx, q_h, decimals) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_qext_format_fixed_bytes(
    ctx: *mut AodRtContext,
    q_h: u64,
    decimals: u64,
) -> u64 {
    unsafe { aod_rt_qext_format_fixed(ctx, q_h, decimals) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_emit_qext_bytes(ctx: *mut AodRtContext, h: u64) {
    unsafe {
        aod_rt_emit_handle(ctx, h);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_addr8_from_bytes(ctx: *mut AodRtContext, h: u64) -> u64 {
    unsafe { aod_rt_addr8_from_str(ctx, h) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_support_from_bytes(ctx: *mut AodRtContext, h: u64) -> u64 {
    unsafe { aod_rt_support_from_str(ctx, h) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_fusion_new(
    ctx: *mut AodRtContext,
    a_h: u64,
    b_h: u64,
    sc_h: u64,
    lin_h: u64,
) -> u64 {
    unsafe { aod_rt_fold(ctx, a_h, b_h, sc_h, lin_h) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ext_decimal(ctx: *mut AodRtContext, h: u64) -> u64 {
    unsafe { aod_rt_qext_ext_decimal(ctx, h) }
}

// ============================================================================
// Complete ORC LLJIT Runtime Symbol Table
// ============================================================================

pub fn runtime_symbol_table() -> Vec<(&'static str, usize)> {
    vec![
        ("aod_rt_ctx_init", aod_rt_ctx_init as *const () as usize),
        ("aod_rt_arena_mark", aod_rt_arena_mark as *const () as usize),
        ("aod_rt_arena_truncate", aod_rt_arena_truncate as *const () as usize),
        (
            "aod_rt_arena_reclaim_handle",
            aod_rt_arena_reclaim_handle as *const () as usize,
        ),
        (
            "aod_rt_arena_reclaim_int",
            aod_rt_arena_reclaim_int as *const () as usize,
        ),
        ("aod_rt_cli_parse_args", aod_rt_cli_parse_args as *const () as usize),
        ("aod_rt_ctx_finish", aod_rt_ctx_finish as *const () as usize),
        ("aod_rt_fail", aod_rt_fail as *const () as usize),
        ("aod_rt_assert_failed", aod_rt_assert_failed as *const () as usize),
        ("aod_rt_budget_exhausted", aod_rt_budget_exhausted as *const () as usize),
        ("aod_rt_int_from_str", aod_rt_int_from_str as *const () as usize),
        ("aod_rt_int_from_i64", aod_rt_int_from_i64 as *const () as usize),
        ("aod_rt_int_from_u64", aod_rt_int_from_u64 as *const () as usize),
        ("aod_rt_int_neg", aod_rt_int_neg as *const () as usize),
        ("aod_rt_int_add", aod_rt_int_add as *const () as usize),
        ("aod_rt_int_sub", aod_rt_int_sub as *const () as usize),
        ("aod_rt_int_mul", aod_rt_int_mul as *const () as usize),
        ("aod_rt_int_div_floor", aod_rt_int_div_floor as *const () as usize),
        ("aod_rt_int_rem_floor", aod_rt_int_rem_floor as *const () as usize),
        ("aod_rt_int_pow", aod_rt_int_pow as *const () as usize),
        ("aod_rt_int_gcd", aod_rt_int_gcd as *const () as usize),
        ("aod_rt_int_lcm", aod_rt_int_lcm as *const () as usize),
        ("aod_rt_int_superpose_cancel", aod_rt_int_superpose_cancel as *const () as usize),
        ("aod_rt_int_exact_div", aod_rt_int_exact_div as *const () as usize),
        ("aod_rt_int_cmp", aod_rt_int_cmp as *const () as usize),
        ("aod_rt_int_sgn", aod_rt_int_sgn as *const () as usize),
        ("aod_rt_int_abs", aod_rt_int_abs as *const () as usize),
        ("aod_rt_int_pack_t32", aod_rt_int_pack_t32 as *const () as usize),
        ("aod_rt_int_unpack_t32", aod_rt_int_unpack_t32 as *const () as usize),
        ("aod_rt_unpack_t9_b64", aod_rt_unpack_t9_b64 as *const () as usize),
        ("aod_rt_unpack_t9_int", aod_rt_unpack_t9_int as *const () as usize),
        ("aod_rt_int_to_t1", aod_rt_int_to_t1 as *const () as usize),
        ("aod_rt_int_to_q4", aod_rt_int_to_q4 as *const () as usize),
        ("aod_rt_int_to_b8", aod_rt_int_to_b8 as *const () as usize),
        ("aod_rt_int_to_b64_view", aod_rt_int_to_b64_view as *const () as usize),
        ("aod_rt_int_from_b64_view", aod_rt_int_from_b64_view as *const () as usize),
        ("aod_rt_ratio_reduce_i128", aod_rt_ratio_reduce_i128 as *const () as usize),
        ("aod_rt_ratio_from_ints", aod_rt_ratio_from_ints as *const () as usize),
        ("aod_rt_ratio_from_str", aod_rt_ratio_from_str as *const () as usize),
        ("aod_rt_ratio_neg", aod_rt_ratio_neg as *const () as usize),
        ("aod_rt_ratio_add", aod_rt_ratio_add as *const () as usize),
        ("aod_rt_ratio_sub", aod_rt_ratio_sub as *const () as usize),
        ("aod_rt_ratio_mul", aod_rt_ratio_mul as *const () as usize),
        ("aod_rt_ratio_div", aod_rt_ratio_div as *const () as usize),
        ("aod_rt_ratio_cmp", aod_rt_ratio_cmp as *const () as usize),
        ("aod_rt_ratio_sgn", aod_rt_ratio_sgn as *const () as usize),
        ("aod_rt_ratio_abs", aod_rt_ratio_abs as *const () as usize),
        ("aod_rt_ratio_num", aod_rt_ratio_num as *const () as usize),
        ("aod_rt_ratio_den", aod_rt_ratio_den as *const () as usize),
        ("aod_rt_ratio_to_int_exact", aod_rt_ratio_to_int_exact as *const () as usize),
        ("aod_rt_emit_null", aod_rt_emit_null as *const () as usize),
        ("aod_rt_emit_int", aod_rt_emit_int as *const () as usize),
        ("aod_rt_emit_bool", aod_rt_emit_bool as *const () as usize),
        ("aod_rt_emit_qext_bool", aod_rt_emit_qext_bool as *const () as usize),
        ("aod_rt_emit_qext_bytes", aod_rt_emit_qext_bytes as *const () as usize),
        ("aod_rt_emit_t1", aod_rt_emit_t1 as *const () as usize),
        ("aod_rt_emit_b8", aod_rt_emit_b8 as *const () as usize),
        ("aod_rt_emit_b64", aod_rt_emit_b64 as *const () as usize),
        ("aod_rt_emit_q4", aod_rt_emit_q4 as *const () as usize),
        ("aod_rt_emit_ratio", aod_rt_emit_ratio as *const () as usize),
        ("aod_rt_emit_handle", aod_rt_emit_handle as *const () as usize),
        ("aod_rt_str_new", aod_rt_str_new as *const () as usize),
        ("aod_rt_bytes_new", aod_rt_bytes_new as *const () as usize),
        ("aod_rt_bytes_from_seq", aod_rt_bytes_from_seq as *const () as usize),
        ("aod_rt_bytes_from_b64", aod_rt_bytes_from_b64 as *const () as usize),
        ("aod_rt_bytes_get", aod_rt_bytes_get as *const () as usize),
        ("aod_rt_bytes_slice", aod_rt_bytes_slice as *const () as usize),
        ("aod_rt_bytes_concat", aod_rt_bytes_concat as *const () as usize),
        ("aod_rt_bytes_sha256", aod_rt_bytes_sha256 as *const () as usize),
        ("aod_rt_bytes_hex", aod_rt_bytes_hex as *const () as usize),
        ("aod_rt_sys_cli_args", aod_rt_sys_cli_args as *const () as usize),
        ("aod_rt_sys_read_file_bytes", aod_rt_sys_read_file_bytes as *const () as usize),
        ("aod_rt_sys_write_executable_elf", aod_rt_sys_write_executable_elf as *const () as usize),
        ("aod_rt_sys_accept_stream", aod_rt_sys_accept_stream as *const () as usize),
        ("aod_rt_sys_stream_local_addr", aod_rt_sys_stream_local_addr as *const () as usize),
        ("aod_rt_sys_read_stream_line", aod_rt_sys_read_stream_line as *const () as usize),
        ("aod_rt_sys_read_stream_frame", aod_rt_sys_read_stream_frame as *const () as usize),
        ("aod_rt_sys_write_stream_bytes", aod_rt_sys_write_stream_bytes as *const () as usize),
        ("aod_rt_sys_write_stream_frame", aod_rt_sys_write_stream_frame as *const () as usize),
        ("aod_rt_handle_eq", aod_rt_handle_eq as *const () as usize),
        ("aod_rt_handle_cmp", aod_rt_handle_cmp as *const () as usize),
        ("aod_rt_val_unit", aod_rt_val_unit as *const () as usize),
        ("aod_rt_val_null", aod_rt_val_null as *const () as usize),
        ("aod_rt_val_from_int", aod_rt_val_from_int as *const () as usize),
        ("aod_rt_val_to_int", aod_rt_val_to_int as *const () as usize),
        ("aod_rt_val_from_bool", aod_rt_val_from_bool as *const () as usize),
        ("aod_rt_val_to_bool", aod_rt_val_to_bool as *const () as usize),
        ("aod_rt_val_from_qext_bool", aod_rt_val_from_qext_bool as *const () as usize),
        ("aod_rt_val_to_qext_bool", aod_rt_val_to_qext_bool as *const () as usize),
        ("aod_rt_val_from_t1", aod_rt_val_from_t1 as *const () as usize),
        ("aod_rt_val_to_t1", aod_rt_val_to_t1 as *const () as usize),
        ("aod_rt_val_from_b8", aod_rt_val_from_b8 as *const () as usize),
        ("aod_rt_val_to_b8", aod_rt_val_to_b8 as *const () as usize),
        ("aod_rt_val_from_b64", aod_rt_val_from_b64 as *const () as usize),
        ("aod_rt_val_to_b64", aod_rt_val_to_b64 as *const () as usize),
        ("aod_rt_val_from_q4", aod_rt_val_from_q4 as *const () as usize),
        ("aod_rt_val_to_q4", aod_rt_val_to_q4 as *const () as usize),
        ("aod_rt_val_from_ratio", aod_rt_val_from_ratio as *const () as usize),
        ("aod_rt_val_to_ratio", aod_rt_val_to_ratio as *const () as usize),
        ("aod_rt_addr8_parse", aod_rt_addr8_parse as *const () as usize),
        ("aod_rt_addr8_from_str", aod_rt_addr8_from_str as *const () as usize),
        ("aod_rt_addr8_from_bytes", aod_rt_addr8_from_bytes as *const () as usize),
        ("aod_rt_support_parse", aod_rt_support_parse as *const () as usize),
        ("aod_rt_support_from_str", aod_rt_support_from_str as *const () as usize),
        ("aod_rt_support_from_bytes", aod_rt_support_from_bytes as *const () as usize),
        ("aod_rt_support_new", aod_rt_support_new as *const () as usize),
        ("aod_rt_support_pi", aod_rt_support_pi as *const () as usize),
        ("aod_rt_support_rd", aod_rt_support_rd as *const () as usize),
        ("aod_rt_support_pressure", aod_rt_support_pressure as *const () as usize),
        ("aod_rt_support_overhang", aod_rt_support_overhang as *const () as usize),
        ("aod_rt_shedding_excess", aod_rt_shedding_excess as *const () as usize),
        ("aod_rt_sadar_promote", aod_rt_sadar_promote as *const () as usize),
        ("aod_rt_probability", aod_rt_probability as *const () as usize),
        ("aod_rt_window_new", aod_rt_window_new as *const () as usize),
        ("aod_rt_window_bounds", aod_rt_window_bounds as *const () as usize),
        ("aod_rt_range_new", aod_rt_range_new as *const () as usize),
        ("aod_rt_range_bounds", aod_rt_range_bounds as *const () as usize),
        ("aod_rt_range_to_list", aod_rt_range_to_list as *const () as usize),
        ("aod_rt_box_new", aod_rt_box_new as *const () as usize),
        ("aod_rt_boundary_of", aod_rt_boundary_of as *const () as usize),
        ("aod_rt_trace_new", aod_rt_trace_new as *const () as usize),
        ("aod_rt_trace_step", aod_rt_trace_step as *const () as usize),
        ("aod_rt_trace_hinge", aod_rt_trace_hinge as *const () as usize),
        ("aod_rt_trace_dwell", aod_rt_trace_dwell as *const () as usize),
        ("aod_rt_trace_close", aod_rt_trace_close as *const () as usize),
        ("aod_rt_trace_phases", aod_rt_trace_phases as *const () as usize),
        ("aod_rt_trace_beats", aod_rt_trace_beats as *const () as usize),
        ("aod_rt_trace_rate", aod_rt_trace_rate as *const () as usize),
        ("aod_rt_fold", aod_rt_fold as *const () as usize),
        ("aod_rt_fusion_new", aod_rt_fusion_new as *const () as usize),
        ("aod_rt_outer", aod_rt_outer as *const () as usize),
        ("aod_rt_fuse", aod_rt_fuse as *const () as usize),
        ("aod_rt_coupling_new", aod_rt_coupling_new as *const () as usize),
        ("aod_rt_coupling_contact", aod_rt_coupling_contact as *const () as usize),
        ("aod_rt_coupling_cadence", aod_rt_coupling_cadence as *const () as usize),
        ("aod_rt_coupling_lag", aod_rt_coupling_lag as *const () as usize),
        ("aod_rt_coupling_drift", aod_rt_coupling_drift as *const () as usize),
        ("aod_rt_window_dedup_count", aod_rt_window_dedup_count as *const () as usize),
        ("aod_rt_native_cadence_ratio", aod_rt_native_cadence_ratio as *const () as usize),
        ("aod_rt_single_contact_assoc", aod_rt_single_contact_assoc as *const () as usize),
        ("aod_rt_recipient_eval_demo", aod_rt_recipient_eval_demo as *const () as usize),
        ("aod_rt_ext_decimal", aod_rt_ext_decimal as *const () as usize),
        ("aod_rt_qext_ext_decimal", aod_rt_qext_ext_decimal as *const () as usize),
        ("aod_rt_qext_from_ratio", aod_rt_qext_from_ratio as *const () as usize),
        ("aod_rt_qext_binop", aod_rt_qext_binop as *const () as usize),
        ("aod_rt_qext_neg", aod_rt_qext_neg as *const () as usize),
        ("aod_rt_qext_abs", aod_rt_qext_abs as *const () as usize),
        ("aod_rt_qext_cmp", aod_rt_qext_cmp as *const () as usize),
        ("aod_rt_qext_render", aod_rt_qext_render as *const () as usize),
        ("aod_rt_qext_render_bytes", aod_rt_qext_render_bytes as *const () as usize),
        ("aod_rt_qext_to_bytes_exact", aod_rt_qext_to_bytes_exact as *const () as usize),
        (
            "aod_rt_qext_round_half_even",
            aod_rt_qext_round_half_even as *const () as usize,
        ),
        (
            "aod_rt_qext_format_fixed",
            aod_rt_qext_format_fixed as *const () as usize,
        ),
        (
            "aod_rt_qext_format_fixed_bytes",
            aod_rt_qext_format_fixed_bytes as *const () as usize,
        ),
        (
            "aod_rt_qext_format_bytes_fixed",
            aod_rt_qext_format_bytes_fixed as *const () as usize,
        ),
        ("aod_rt_array_new", aod_rt_array_new as *const () as usize),
        ("aod_rt_list_new", aod_rt_list_new as *const () as usize),
        ("aod_rt_seq_get", aod_rt_seq_get as *const () as usize),
        ("aod_rt_seq_get_int", aod_rt_seq_get_int as *const () as usize),
        ("aod_rt_seq_get_raw_idx", aod_rt_seq_get_raw_idx as *const () as usize),
        ("aod_rt_array_set", aod_rt_array_set as *const () as usize),
        ("aod_rt_array_set_int", aod_rt_array_set_int as *const () as usize),
        ("aod_rt_seq_len_raw", aod_rt_seq_len_raw as *const () as usize),
        ("aod_rt_seq_sieve", aod_rt_seq_sieve as *const () as usize),
        ("aod_rt_seq_argmin_argmax", aod_rt_seq_argmin_argmax as *const () as usize),
        ("aod_rt_zip_seq", aod_rt_zip_seq as *const () as usize),
        ("aod_rt_len", aod_rt_len as *const () as usize),
        ("aod_rt_tuple_new", aod_rt_tuple_new as *const () as usize),
        ("aod_rt_tuple_get", aod_rt_tuple_get as *const () as usize),
        ("aod_rt_map_new", aod_rt_map_new as *const () as usize),
        ("aod_rt_map_get", aod_rt_map_get as *const () as usize),
        ("aod_rt_map_set", aod_rt_map_set as *const () as usize),
        ("aod_rt_map_remove", aod_rt_map_remove as *const () as usize),
        ("aod_rt_map_get_or", aod_rt_map_get_or as *const () as usize),
        ("aod_rt_map_contains", aod_rt_map_contains as *const () as usize),
        ("aod_rt_map_keys_list", aod_rt_map_keys_list as *const () as usize),
        ("aod_rt_map_entries_list", aod_rt_map_entries_list as *const () as usize),
        ("aod_rt_struct_new", aod_rt_struct_new as *const () as usize),
        ("aod_rt_struct_set", aod_rt_struct_set as *const () as usize),
        ("aod_rt_enum_new", aod_rt_enum_new as *const () as usize),
        ("aod_rt_enum_tag", aod_rt_enum_tag as *const () as usize),
        ("aod_rt_enum_payload", aod_rt_enum_payload as *const () as usize),
        ("aod_rt_field_get", aod_rt_field_get as *const () as usize),
        ("aod_rt_field_get_int", aod_rt_field_get_int as *const () as usize),
        ("aod_rt_sim_builder_new", aod_rt_sim_builder_new as *const () as usize),
        (
            "aod_rt_sim_builder_set_dynamics",
            aod_rt_sim_builder_set_dynamics as *const () as usize,
        ),
        ("aod_rt_sim_builder_add_clock", aod_rt_sim_builder_add_clock as *const () as usize),
        ("aod_rt_sim_builder_add_carrier", aod_rt_sim_builder_add_carrier as *const () as usize),
        ("aod_rt_sim_builder_add_decl_obs", aod_rt_sim_builder_add_decl_obs as *const () as usize),
        ("aod_rt_sim_builder_add_proc_obs", aod_rt_sim_builder_add_proc_obs as *const () as usize),
        ("aod_rt_sim_run", aod_rt_sim_run as *const () as usize),
        ("aod_rt_sim_run_ex", aod_rt_sim_run_ex as *const () as usize),
        ("aod_rt_int_trit_shl", aod_rt_int_trit_shl as *const () as usize),
        ("aod_rt_int_trit_shr", aod_rt_int_trit_shr as *const () as usize),
        ("aod_rt_b64_from_unsigned_int", aod_rt_b64_from_unsigned_int as *const () as usize),
        ("aod_rt_pack_ratio", aod_rt_pack_ratio as *const () as usize),
        ("aod_rt_unpack_ratio", aod_rt_unpack_ratio as *const () as usize),
        ("aod_rt_ratio_pow", aod_rt_ratio_pow as *const () as usize),
        ("aod_rt_str_from_bytes", aod_rt_str_from_bytes as *const () as usize),
        ("aod_rt_le_bytes_from_int", aod_rt_le_bytes_from_int as *const () as usize),
        ("aod_rt_bytes_push", aod_rt_bytes_push as *const () as usize),
        ("aod_rt_seq_push", aod_rt_seq_push as *const () as usize),
        ("aod_rt_seq_push_int", aod_rt_seq_push_int as *const () as usize),
        ("aod_rt_seq_concat", aod_rt_seq_concat as *const () as usize),
        ("aod_rt_sort_int_pairs", aod_rt_sort_int_pairs as *const () as usize),
        ("aod_rt_seq_slice", aod_rt_seq_slice as *const () as usize),
        ("aod_rt_seq_contains", aod_rt_seq_contains as *const () as usize),
        ("aod_rt_range_seq", aod_rt_range_seq as *const () as usize),
        ("aod_rt_seq_buf_new", aod_rt_seq_buf_new as *const () as usize),
        ("aod_rt_seq_buf_push", aod_rt_seq_buf_push as *const () as usize),
        ("aod_rt_seq_buf_finish", aod_rt_seq_buf_finish as *const () as usize),
        ("aod_rt_t1_superpose_cancel", aod_rt_t1_superpose_cancel as *const () as usize),
        ("aod_rt_verify_sheddic_route", aod_rt_verify_sheddic_route as *const () as usize),
        ("aod_rt_verify_sheddic_route_int", aod_rt_verify_sheddic_route_int as *const () as usize),
        ("aod_rt_verify_frg_cross_return", aod_rt_verify_frg_cross_return as *const () as usize),
        ("aod_rt_sum_int", aod_rt_sum_int as *const () as usize),
        ("aod_rt_sum_b8", aod_rt_sum_b8 as *const () as usize),
        ("aod_rt_sum_t1", aod_rt_sum_t1 as *const () as usize),
        ("aod_rt_sum_ratio", aod_rt_sum_ratio as *const () as usize),
        ("aod_rt_sum_qext_ratio", aod_rt_sum_qext_ratio as *const () as usize),
        ("aod_rt_dot_int", aod_rt_dot_int as *const () as usize),
        ("aod_rt_dot_b8", aod_rt_dot_b8 as *const () as usize),
        ("aod_rt_dot_t1", aod_rt_dot_t1 as *const () as usize),
        ("aod_rt_dot_ratio", aod_rt_dot_ratio as *const () as usize),
        ("aod_rt_dot_qext_ratio", aod_rt_dot_qext_ratio as *const () as usize),
        ("aod_rt_resolve_dyn_slot", aod_rt_resolve_dyn_slot as *const () as usize),
    ]
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_resolve_dyn_slot(
    ctx: *mut AodRtContext,
    slot_idx: i32,
    default_fn: *const (),
) -> *const () {
    if ctx.is_null() || slot_idx < 0 {
        return default_fn;
    }
    let raw = unsafe { &*ctx };
    if raw.inner.is_null() {
        return default_fn;
    }
    let state = unsafe { &*raw.inner };
    match state.dyn_vtable.get(slot_idx as usize) {
        Some(&addr) if addr != 0 => addr as *const (),
        _ => default_fn,
    }
}

// ============================================================================
// Default-On Linux Plan 9 Kernel Container & Persistent Sandboxed JIT Worker
// ============================================================================

/// Kernel isolation tier enforced on a sandboxed `aodc jit` worker or AOT process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SandboxTier {
    /// Full Plan 9 `rfork(RFCNAMEG)` Linux namespace jail:
    /// `CLONE_NEWUSER | CLONE_NEWNS | CLONE_NEWNET | CLONE_NEWIPC | CLONE_NEWUTS`
    /// + `pivot_root` onto an empty read-only `/` `tmpfs` with ONLY declared mounts
    /// + `seccomp-bpf` blocking sockets, `execve`, `ptrace`, and mount syscalls.
    LinuxPlan9PivotRoot,
    /// Kernel Landlock LSM + `seccomp-bpf` fallback used when an outer container
    /// restricts unprivileged `CLONE_NEWUSER`.
    LinuxLandlockSeccomp,
    /// Explicit `--unconfined` opt-out (never used by default).
    Unconfined,
}

impl SandboxTier {
    fn to_tag(self) -> u8 {
        match self {
            SandboxTier::LinuxPlan9PivotRoot => 1,
            SandboxTier::LinuxLandlockSeccomp => 2,
            SandboxTier::Unconfined => 3,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, String> {
        match tag {
            1 => Ok(SandboxTier::LinuxPlan9PivotRoot),
            2 => Ok(SandboxTier::LinuxLandlockSeccomp),
            3 => Ok(SandboxTier::Unconfined),
            other => Err(format!("unknown sandbox tier byte {other}")),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            SandboxTier::LinuxPlan9PivotRoot => "linux_plan9_pivot_root",
            SandboxTier::LinuxLandlockSeccomp => "linux_landlock_seccomp",
            SandboxTier::Unconfined => "unconfined",
        }
    }
}

/// Lock the calling single-threaded process into a Linux Plan 9 kernel container
/// matching `rt_ns` (`pivot_root` primary tier; Landlock + `seccomp-bpf` fallback).
pub fn enter_current_process_sandbox(
    rt_ns: &mut crate::ns_fs::RtNamespace,
) -> Result<SandboxTier, String> {
    if rt_ns.is_unconfined() {
        return Ok(SandboxTier::Unconfined);
    }
    #[cfg(target_os = "linux")]
    {
        linux_sandbox::enter_sandbox(rt_ns)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(SandboxTier::Unconfined)
    }
}

/// Persistent `fork()`ed child worker executing pre-materialized JIT machine code
/// inside a real Linux Plan 9 `pivot_root` + `seccomp-bpf` container.
pub struct SandboxedJitWorker {
    #[cfg(target_os = "linux")]
    inner: linux_sandbox::WorkerInner,
    tier: SandboxTier,
}

impl SandboxedJitWorker {
    pub fn spawn(
        entry_addr: u64,
        rt_ns: crate::ns_fs::RtNamespace,
    ) -> Result<Self, String> {
        Self::spawn_internal(entry_addr, rt_ns, false)
    }

    pub fn spawn_landlock_fallback(
        entry_addr: u64,
        rt_ns: crate::ns_fs::RtNamespace,
    ) -> Result<Self, String> {
        Self::spawn_internal(entry_addr, rt_ns, true)
    }

    fn spawn_internal(
        entry_addr: u64,
        rt_ns: crate::ns_fs::RtNamespace,
        force_landlock_only: bool,
    ) -> Result<Self, String> {
        if entry_addr == 0 {
            return Err("cannot spawn sandboxed JIT worker with null entry_addr".to_string());
        }
        #[cfg(target_os = "linux")]
        {
            let (inner, tier) =
                linux_sandbox::WorkerInner::spawn(entry_addr, rt_ns, force_landlock_only)?;
            Ok(Self { inner, tier })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (rt_ns, force_landlock_only);
            Err("Linux kernel container worker requires target_os = linux".to_string())
        }
    }

    pub const fn tier(&self) -> SandboxTier {
        self.tier
    }

    pub fn run_once(
        &mut self,
        fuel: i64,
        max_depth: i32,
        cli_args: &[Vec<u8>],
    ) -> Result<(Vec<String>, bool, Option<String>), String> {
        self.run_once_with_dyn(fuel, max_depth, cli_args, &[])
    }

    pub fn run_once_with_dyn(
        &mut self,
        fuel: i64,
        max_depth: i32,
        cli_args: &[Vec<u8>],
        dyn_vtable: &[u64],
    ) -> Result<(Vec<String>, bool, Option<String>), String> {
        #[cfg(target_os = "linux")]
        {
            self.inner.run_once(fuel, max_depth, cli_args, dyn_vtable)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (fuel, max_depth, cli_args, dyn_vtable);
            Err("Linux kernel container worker requires target_os = linux".to_string())
        }
    }
}

#[cfg(target_os = "linux")]
mod linux_sandbox {
    use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void, CString};
    use std::io::Write;
    use std::os::unix::ffi::OsStrExt;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    use aod_core::ns::MountPerm;

    use super::{AodRtContext, OwnedRtContext, SandboxTier};
    use crate::ns_fs::{PivotBindMount, RtNamespace};

    const CLONE_NEWNS: c_int = 0x0002_0000;
    const CLONE_NEWUTS: c_int = 0x0400_0000;
    const CLONE_NEWIPC: c_int = 0x0800_0000;
    const CLONE_NEWUSER: c_int = 0x1000_0000;
    const CLONE_NEWNET: c_int = 0x4000_0000;

    const MS_NOSUID: c_ulong = 2;
    const MS_NODEV: c_ulong = 4;
    const MS_BIND: c_ulong = 4096;
    const MS_REC: c_ulong = 16384;
    const MS_PRIVATE: c_ulong = 1 << 18;
    const MNT_DETACH: c_int = 2;

    const O_RDONLY: c_int = 0;
    const O_CLOEXEC: c_int = 0x80000;
    #[cfg(target_arch = "x86_64")]
    const O_DIRECTORY: c_int = 0x10000;
    #[cfg(target_arch = "aarch64")]
    const O_DIRECTORY: c_int = 0x4000;
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    const O_DIRECTORY: c_int = 0x10000;

    const AT_FDCWD: c_int = -100;
    const AT_RECURSIVE: c_uint = 0x8000;
    const MOUNT_ATTR_RDONLY: u64 = 0x01;
    const MOUNT_ATTR_SIZE_VER0: usize = 32;

    const PR_SET_PDEATHSIG: c_int = 1;
    const PR_SET_DUMPABLE: c_int = 4;
    const PR_SET_SECCOMP: c_int = 22;
    const PR_SET_NO_NEW_PRIVS: c_int = 38;
    const SECCOMP_MODE_FILTER: c_ulong = 2;
    const SIGKILL: c_ulong = 9;
    const EINTR: c_int = 4;

    #[cfg(target_arch = "x86_64")]
    const SYS_PIVOT_ROOT: c_long = 155;
    #[cfg(target_arch = "aarch64")]
    const SYS_PIVOT_ROOT: c_long = 41;

    const SYS_MOUNT_SETATTR: c_long = 442;
    const SYS_LANDLOCK_CREATE_RULESET: c_long = 444;
    const SYS_LANDLOCK_ADD_RULE: c_long = 445;
    const SYS_LANDLOCK_RESTRICT_SELF: c_long = 446;

    const LANDLOCK_RULE_PATH_BENEATH: c_uint = 1;
    const LANDLOCK_ABI_V1_FS_ALL: u64 = (1u64 << 13) - 1;
    // Read file (1<<2) | Read dir (1<<3)
    const LANDLOCK_FS_RO: u64 = (1u64 << 2) | (1u64 << 3);
    // Read/write/create/remove/mkdir regular files & dirs (no char/block/fifo/sock/sym devices!)
    const LANDLOCK_FS_RW: u64 =
        LANDLOCK_FS_RO | (1u64 << 0) | (1u64 << 1) | (1u64 << 4) | (1u64 << 5) | (1u64 << 7) | (1u64 << 8);

    #[repr(C)]
    struct MountAttr {
        attr_set: u64,
        attr_clr: u64,
        propagation: u64,
        userns_fd: u64,
    }

    #[repr(C)]
    struct LandlockRulesetAttrV1 {
        handled_access_fs: u64,
    }

    #[repr(C, packed)]
    struct LandlockPathBeneathAttr {
        allowed_access: u64,
        parent_fd: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SockFilter {
        code: u16,
        jt: u8,
        jf: u8,
        k: u32,
    }

    #[repr(C)]
    struct SockFprog {
        len: u16,
        filter: *const SockFilter,
    }

    unsafe extern "C" {
        fn unshare(flags: c_int) -> c_int;
        fn mount(
            source: *const c_char,
            target: *const c_char,
            filesystemtype: *const c_char,
            mountflags: c_ulong,
            data: *const c_void,
        ) -> c_int;
        fn umount2(target: *const c_char, flags: c_int) -> c_int;
        fn mkdir(pathname: *const c_char, mode: c_uint) -> c_int;
        fn rmdir(pathname: *const c_char) -> c_int;
        fn chdir(path: *const c_char) -> c_int;
        fn open(pathname: *const c_char, flags: c_int, ...) -> c_int;
        fn close(fd: c_int) -> c_int;
        fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
        fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
        fn pipe2(pipefd: *mut c_int, flags: c_int) -> c_int;
        fn fork() -> c_int;
        fn waitpid(pid: c_int, wstatus: *mut c_int, options: c_int) -> c_int;
        fn _exit(status: c_int) -> !;
        fn getuid() -> c_uint;
        fn getgid() -> c_uint;
        fn prctl(option: c_int, arg2: c_ulong, arg3: c_ulong, arg4: c_ulong, arg5: c_ulong) -> c_int;
        fn syscall(num: c_long, ...) -> c_long;
        fn __errno_location() -> *mut c_int;
    }

    fn last_errno() -> c_int {
        unsafe { *__errno_location() }
    }

    struct FdGuard(c_int);

    impl Drop for FdGuard {
        fn drop(&mut self) {
            if self.0 >= 0 {
                unsafe {
                    let _ = close(self.0);
                }
            }
        }
    }

    #[repr(C)]
    struct StatVfs {
        f_bsize: c_ulong,
        f_frsize: c_ulong,
        f_blocks: u64,
        f_bfree: u64,
        f_bavail: u64,
        f_files: u64,
        f_ffree: u64,
        f_favail: u64,
        f_fsid: c_ulong,
        f_flag: c_ulong,
        f_namemax: c_ulong,
        __f_spare: [c_int; 6],
    }

    unsafe extern "C" {
        fn statvfs(path: *const c_char, buf: *mut StatVfs) -> c_int;
    }

    static PIVOT_ROOT_CAPABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

    /// Probe once per process (in a disposable child) whether the host kernel and MAC policy
    /// (e.g. Ubuntu 24.04 AppArmor `kernel.apparmor_restrict_unprivileged_userns=1`) permit the
    /// full `unshare(CLONE_NEWUSER|CLONE_NEWNS)` + `uid_map` + `mount(tmpfs)` + `pivot_root` chain.
    ///
    /// Crucial detail: on Ubuntu 24.04 with `apparmor_restrict_unprivileged_userns=1`, `unshare`
    /// itself returns `0` and AppArmor then blocks `write("/proc/self/uid_map")` with `EPERM`.
    /// Probing in a disposable child prevents tainting the real worker/AOT process with an
    /// unmapped `overflowuid` user namespace before falling back to Landlock + `seccomp-bpf`.
    pub fn is_pivot_root_capable() -> bool {
        *PIVOT_ROOT_CAPABLE.get_or_init(probe_pivot_root_capable_once)
    }

    fn probe_pivot_root_capable_once() -> bool {
        let host_uid = unsafe { getuid() };
        let host_gid = unsafe { getgid() };
        let ns_flags = CLONE_NEWUSER | CLONE_NEWNS | CLONE_NEWNET | CLONE_NEWIPC | CLONE_NEWUTS;

        let pid = unsafe { fork() };
        if pid < 0 {
            return false;
        }
        if pid == 0 {
            unsafe {
                let _ = prctl(PR_SET_PDEATHSIG, SIGKILL, 0, 0, 0);
                const SYS_CLOSE_RANGE: c_long = 436;
                if syscall(SYS_CLOSE_RANGE, 3u32, u32::MAX, 0u32) != 0 {
                    for fd in 3..1024 {
                        let _ = close(fd);
                    }
                }
                if unshare(ns_flags) != 0 {
                    _exit(1);
                }
            }
            let _ = std::fs::write("/proc/self/setgroups", b"deny");
            if std::fs::write("/proc/self/uid_map", format!("0 {host_uid} 1\n")).is_err() {
                unsafe { _exit(1) };
            }
            if std::fs::write("/proc/self/gid_map", format!("0 {host_gid} 1\n")).is_err() {
                unsafe { _exit(1) };
            }
            unsafe {
                if mount(
                    c"none".as_ptr(),
                    c"/".as_ptr(),
                    std::ptr::null(),
                    MS_REC | MS_PRIVATE,
                    std::ptr::null(),
                ) != 0
                {
                    _exit(1);
                }
                let tmpfs_opts = c"size=1m,mode=0755";
                if mount(
                    c"tmpfs".as_ptr(),
                    c"/tmp".as_ptr(),
                    c"tmpfs".as_ptr(),
                    MS_NOSUID | MS_NODEV,
                    tmpfs_opts.as_ptr() as *const c_void,
                ) != 0
                {
                    _exit(1);
                }
                if mkdir(c"/tmp/.old".as_ptr(), 0o700) != 0 {
                    _exit(1);
                }
                if chdir(c"/tmp".as_ptr()) != 0 {
                    _exit(1);
                }
                if syscall(SYS_PIVOT_ROOT, c".".as_ptr(), c".old".as_ptr()) != 0 {
                    _exit(1);
                }
                if chdir(c"/".as_ptr()) != 0 {
                    _exit(1);
                }
                if umount2(c"/.old".as_ptr(), MNT_DETACH) != 0 {
                    _exit(1);
                }
                _exit(0);
            }
        }

        let mut status: c_int = 0;
        loop {
            let rc = unsafe { waitpid(pid, &mut status, 0) };
            if rc >= 0 {
                break;
            }
            if last_errno() != EINTR {
                return false;
            }
        }
        (status & 0x7f) == 0 && ((status >> 8) & 0xff) == 0
    }

    pub fn enter_sandbox(rt_ns: &mut RtNamespace) -> Result<SandboxTier, String> {
        enter_sandbox_with_mode(rt_ns, false)
    }

    pub fn enter_sandbox_with_mode(
        rt_ns: &mut RtNamespace,
        force_landlock_only: bool,
    ) -> Result<SandboxTier, String> {
        let host_uid = unsafe { getuid() };
        let host_gid = unsafe { getgid() };
        let ns_flags = CLONE_NEWUSER | CLONE_NEWNS | CLONE_NEWNET | CLONE_NEWIPC | CLONE_NEWUTS;

        let deny_fs_open = rt_ns.all_host_bind_dirs().is_empty();
        let allow_stream_accept = rt_ns.has_stream_listener();
        if !force_landlock_only && is_pivot_root_capable() && unsafe { unshare(ns_flags) } == 0 {
            finish_pivot_root_after_unshare(rt_ns, host_uid, host_gid)?;
            harden_no_new_privs_and_dumpable()?;
            install_seccomp_denylist(deny_fs_open, allow_stream_accept)?;
            Ok(SandboxTier::LinuxPlan9PivotRoot)
        } else {
            harden_no_new_privs_and_dumpable()?;
            if let Err((ll_errno, ll_err)) = apply_landlock_fallback(rt_ns) {
                if ll_errno != 38 && ll_errno != 95 {
                    return Err(format!(
                        "Linux container sandbox Landlock fallback failed: {ll_err}"
                    ));
                }
            }
            install_seccomp_denylist(deny_fs_open, allow_stream_accept)?;
            Ok(SandboxTier::LinuxLandlockSeccomp)
        }
    }

    fn harden_no_new_privs_and_dumpable() -> Result<(), String> {
        unsafe {
            if prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(format!(
                    "prctl(PR_SET_NO_NEW_PRIVS) failed: errno={}",
                    last_errno()
                ));
            }
            let _ = prctl(PR_SET_DUMPABLE, 0, 0, 0, 0);
        }
        Ok(())
    }

    fn finish_pivot_root_after_unshare(
        rt_ns: &mut RtNamespace,
        host_uid: c_uint,
        host_gid: c_uint,
    ) -> Result<(), String> {
        let (mounts, pivoted_ns) = rt_ns.prepare_pivot_root_plan();

        // 1. Map uid/gid inside user namespace BEFORE setting dumpable=0
        let _ = std::fs::write("/proc/self/setgroups", b"deny");
        std::fs::write("/proc/self/uid_map", format!("0 {host_uid} 1\n"))
            .map_err(|e| format!("write uid_map failed: {e}"))?;
        std::fs::write("/proc/self/gid_map", format!("0 {host_gid} 1\n"))
            .map_err(|e| format!("write gid_map failed: {e}"))?;

        // 2. Privatize mount tree so zero mount events propagate to host
        if unsafe {
            mount(
                c"none".as_ptr(),
                c"/".as_ptr(),
                std::ptr::null(),
                MS_REC | MS_PRIVATE,
                std::ptr::null(),
            )
        } != 0
        {
            return Err(format!("mount(/, MS_PRIVATE) errno={}", last_errno()));
        }

        // 3. Stage fresh rootfs on `/tmp/.aod_sandbox_stage_<uid>` WITHOUT shadowing sibling `/tmp/*` dirs!
        let stage_path = format!("/tmp/.aod_sandbox_stage_{host_uid}");
        let stage_dir = CString::new(stage_path.as_bytes()).unwrap();
        let mkdir_rc = unsafe { mkdir(stage_dir.as_ptr(), 0o755) };
        if mkdir_rc != 0 {
            let err = last_errno();
            if err != 17 {
                return Err(format!("mkdir({stage_path}) errno={err}"));
            }
        }
        let tmpfs_opts = c"size=16m,mode=0755";
        if unsafe {
            mount(
                c"tmpfs".as_ptr(),
                stage_dir.as_ptr(),
                c"tmpfs".as_ptr(),
                MS_NOSUID | MS_NODEV,
                tmpfs_opts.as_ptr() as *const c_void,
            )
        } != 0
        {
            return Err(format!("mount(tmpfs -> stage) errno={}", last_errno()));
        }

        // 4. Bind-mount ONLY declared Plan 9 mounts into the staged rootfs
        for m in &mounts {
            bind_one_into_new_root(&stage_path, m)?;
        }

        // 5. pivot_root into `stage_path` and completely detach the host rootfs!
        let old_root_c = CString::new(format!("{stage_path}/.old_root")).unwrap();
        if unsafe { mkdir(old_root_c.as_ptr(), 0o700) } != 0 {
            return Err(format!("mkdir(.old_root) errno={}", last_errno()));
        }
        if unsafe { chdir(stage_dir.as_ptr()) } != 0 {
            return Err(format!("chdir(stage_dir) errno={}", last_errno()));
        }
        if unsafe { syscall(SYS_PIVOT_ROOT, c".".as_ptr(), c".old_root".as_ptr()) } != 0 {
            return Err(format!("pivot_root errno={}", last_errno()));
        }
        if unsafe { chdir(c"/".as_ptr()) } != 0 {
            return Err(format!("chdir(/) errno={}", last_errno()));
        }
        if unsafe { umount2(c"/.old_root".as_ptr(), MNT_DETACH) } != 0 {
            return Err(format!("umount2(/.old_root) errno={}", last_errno()));
        }
        let _ = unsafe { rmdir(c"/.old_root".as_ptr()) };

        // 6. Lock `/` itself read-only (non-recursive so explicit `rw` submounts stay writable!)
        set_mount_rdonly(c"/", false)?;

        *rt_ns = pivoted_ns;
        Ok(())
    }

    fn bind_one_into_new_root(stage_root: &str, m: &PivotBindMount) -> Result<(), String> {
        let mut cur = String::from(stage_root);
        for seg in m.container_path.trim_start_matches('/').split('/') {
            if seg.is_empty() {
                continue;
            }
            cur.push('/');
            cur.push_str(seg);
            let cdir = CString::new(cur.as_bytes()).map_err(|_| "NUL in container path")?;
            let rc = unsafe { mkdir(cdir.as_ptr(), 0o755) };
            if rc != 0 {
                let err = last_errno();
                if err != 17 {
                    return Err(format!("mkdir({cur}) errno={err}"));
                }
            }
        }

        let src_cstr = CString::new(m.host_dir.as_os_str().as_bytes())
            .map_err(|_| "NUL in host_dir path")?;
        let dst_cstr = CString::new(cur.as_bytes()).map_err(|_| "NUL in dst path")?;
        if unsafe {
            mount(
                src_cstr.as_ptr(),
                dst_cstr.as_ptr(),
                std::ptr::null(),
                MS_BIND | MS_REC,
                std::ptr::null(),
            )
        } != 0
        {
            return Err(format!(
                "MS_BIND({} -> {cur}) errno={}",
                m.host_dir.display(),
                last_errno()
            ));
        }

        if m.perm == MountPerm::ReadOnly {
            set_mount_rdonly(&dst_cstr, true)?;
        }
        Ok(())
    }

    fn set_mount_rdonly(target: &std::ffi::CStr, recursive: bool) -> Result<(), String> {
        let attr = MountAttr {
            attr_set: MOUNT_ATTR_RDONLY,
            attr_clr: 0,
            propagation: 0,
            userns_fd: 0,
        };
        let flags: c_uint = if recursive { AT_RECURSIVE } else { 0 };
        let rc = unsafe {
            syscall(
                SYS_MOUNT_SETATTR,
                AT_FDCWD,
                target.as_ptr(),
                flags,
                &attr as *const MountAttr,
                MOUNT_ATTR_SIZE_VER0,
            )
        };
        if rc == 0 {
            return Ok(());
        }

        // Fallback to classic `statvfs` + `MS_BIND | MS_REMOUNT | MS_RDONLY` preserving locked mount flags
        let mut st: StatVfs = unsafe { std::mem::zeroed() };
        if unsafe { statvfs(target.as_ptr(), &mut st) } != 0 {
            return Err(format!(
                "mount_setattr/statvfs({target:?}) errno={}",
                last_errno()
            ));
        }
        const MS_RDONLY_F: c_ulong = 1;
        const MS_NOEXEC_F: c_ulong = 8;
        const MS_SYNCHRONOUS_F: c_ulong = 16;
        const MS_REMOUNT_F: c_ulong = 32;
        const MS_NOATIME_F: c_ulong = 1024;
        const MS_NODIRATIME_F: c_ulong = 2048;
        const MS_RELATIME_F: c_ulong = 1 << 21;
        const ST_RELATIME_F: c_ulong = 4096;

        let mut remount_flags = MS_BIND | MS_REMOUNT_F | MS_RDONLY_F;
        if (st.f_flag & MS_NOSUID) != 0 {
            remount_flags |= MS_NOSUID;
        }
        if (st.f_flag & MS_NODEV) != 0 {
            remount_flags |= MS_NODEV;
        }
        if (st.f_flag & MS_NOEXEC_F) != 0 {
            remount_flags |= MS_NOEXEC_F;
        }
        if (st.f_flag & MS_SYNCHRONOUS_F) != 0 {
            remount_flags |= MS_SYNCHRONOUS_F;
        }
        if (st.f_flag & MS_NOATIME_F) != 0 {
            remount_flags |= MS_NOATIME_F;
        }
        if (st.f_flag & MS_NODIRATIME_F) != 0 {
            remount_flags |= MS_NODIRATIME_F;
        }
        if (st.f_flag & ST_RELATIME_F) != 0 {
            remount_flags |= MS_RELATIME_F;
        }
        if unsafe {
            mount(
                std::ptr::null(),
                target.as_ptr(),
                std::ptr::null(),
                remount_flags,
                std::ptr::null(),
            )
        } != 0
        {
            return Err(format!(
                "remount({target:?}, MS_RDONLY) errno={}",
                last_errno()
            ));
        }
        Ok(())
    }

    fn apply_landlock_fallback(rt_ns: &RtNamespace) -> Result<(), (c_int, String)> {
        let attr = LandlockRulesetAttrV1 {
            handled_access_fs: LANDLOCK_ABI_V1_FS_ALL,
        };
        let ruleset_fd = unsafe {
            syscall(
                SYS_LANDLOCK_CREATE_RULESET,
                &attr as *const LandlockRulesetAttrV1,
                std::mem::size_of::<LandlockRulesetAttrV1>(),
                0u32,
            )
        };
        if ruleset_fd < 0 {
            let err = last_errno();
            return Err((err, format!("landlock_create_ruleset errno={err}")));
        }
        let ruleset_guard = FdGuard(ruleset_fd as c_int);

        for (dir_path, perm) in rt_ns.all_host_bind_dirs() {
            let cpath = CString::new(dir_path.as_os_str().as_bytes())
                .map_err(|_| (22, "NUL in Landlock host path".to_string()))?;
            let pfd = unsafe { open(cpath.as_ptr(), O_RDONLY | O_DIRECTORY | O_CLOEXEC) };
            if pfd < 0 {
                let err = last_errno();
                return Err((
                    err,
                    format!("open({}) for Landlock errno={err}", dir_path.display()),
                ));
            }
            let pfd_guard = FdGuard(pfd);
            let allowed = match perm {
                MountPerm::ReadOnly => LANDLOCK_FS_RO,
                MountPerm::ReadWrite => LANDLOCK_FS_RW,
            };
            let rule = LandlockPathBeneathAttr {
                allowed_access: allowed,
                parent_fd: pfd_guard.0,
            };
            let add_rc = unsafe {
                syscall(
                    SYS_LANDLOCK_ADD_RULE,
                    ruleset_guard.0,
                    LANDLOCK_RULE_PATH_BENEATH,
                    &rule as *const LandlockPathBeneathAttr,
                    0u32,
                )
            };
            if add_rc != 0 {
                let err = last_errno();
                return Err((err, format!("landlock_add_rule errno={err}")));
            }
        }

        let restrict_rc = unsafe { syscall(SYS_LANDLOCK_RESTRICT_SELF, ruleset_guard.0, 0u32) };
        if restrict_rc != 0 {
            let err = last_errno();
            return Err((err, format!("landlock_restrict_self errno={err}")));
        }
        Ok(())
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn install_seccomp_denylist(
        deny_fs_open: bool,
        allow_stream_accept: bool,
    ) -> Result<(), String> {
        #[cfg(target_arch = "x86_64")]
        const EXPECTED_ARCH: u32 = 0xC000_003E;
        #[cfg(target_arch = "aarch64")]
        const EXPECTED_ARCH: u32 = 0xC000_00B7;

        #[cfg(target_arch = "x86_64")]
        const DENIED_BASE: &[u32] = &[
            41,  // socket
            42,  // connect
            49,  // bind
            50,  // listen
            53,  // socketpair
            59,  // execve
            101, // ptrace
            155, // pivot_root
            161, // chroot
            165, // mount
            166, // umount2
            298, // perf_event_open
            321, // bpf
            322, // execveat
            425, // io_uring_setup
            442, // mount_setattr
        ];

        #[cfg(target_arch = "x86_64")]
        const DENIED_NET_ACCEPT: &[u32] = &[
            43,  // accept
            288, // accept4
        ];

        #[cfg(target_arch = "x86_64")]
        const DENIED_FS_OPEN: &[u32] = &[
            2,   // open
            85,  // creat
            257, // openat
            304, // open_by_handle_at
            428, // open_tree
            437, // openat2
        ];

        #[cfg(target_arch = "aarch64")]
        const DENIED_BASE: &[u32] = &[
            198, // socket
            199, // socketpair
            200, // bind
            201, // listen
            203, // connect
            221, // execve
            117, // ptrace
            41,  // pivot_root
            51,  // chroot
            40,  // mount
            39,  // umount2
            241, // perf_event_open
            280, // bpf
            281, // execveat
            425, // io_uring_setup
            442, // mount_setattr
        ];

        #[cfg(target_arch = "aarch64")]
        const DENIED_NET_ACCEPT: &[u32] = &[
            202, // accept
            242, // accept4
        ];

        #[cfg(target_arch = "aarch64")]
        const DENIED_FS_OPEN: &[u32] = &[
            56,  // openat
            265, // open_by_handle_at
            428, // open_tree
            437, // openat2
        ];

        let mut denied: Vec<u32> = Vec::with_capacity(
            DENIED_BASE.len() + DENIED_NET_ACCEPT.len() + DENIED_FS_OPEN.len(),
        );
        denied.extend_from_slice(DENIED_BASE);
        if !allow_stream_accept {
            denied.extend_from_slice(DENIED_NET_ACCEPT);
        }
        if deny_fs_open {
            denied.extend_from_slice(DENIED_FS_OPEN);
        }

        #[cfg(target_arch = "x86_64")]
        let has_x32_guard = 1usize;
        #[cfg(not(target_arch = "x86_64"))]
        let has_x32_guard = 0usize;

        // Layout:
        // [0] LD arch
        // [1] JEQ EXPECTED_ARCH (jf -> DENY)
        // [2] LD nr
        // [3] (x86_64) JGE 0x4000_0000 (jt -> DENY)
        // [..] JEQ bad_nr (jt -> DENY)
        // [total - 2] RET ALLOW
        // [total - 1] RET ERRNO|EPERM (0x0005_0001)
        let total = 3 + has_x32_guard + denied.len() + 2;
        let deny_idx = total - 1;
        let mut insns: Vec<SockFilter> = Vec::with_capacity(total);

        // [0]BPF_LD | BPF_W | BPF_ABS (0x20), offset 4 (arch)
        insns.push(SockFilter {
            code: 0x20,
            jt: 0,
            jf: 0,
            k: 4,
        });
        // [1] BPF_JMP | BPF_JEQ | BPF_K (0x15)
        insns.push(SockFilter {
            code: 0x15,
            jt: 0,
            jf: (deny_idx - 2) as u8,
            k: EXPECTED_ARCH,
        });
        // [2] BPF_LD | BPF_W | BPF_ABS (0x20), offset 0 (nr)
        insns.push(SockFilter {
            code: 0x20,
            jt: 0,
            jf: 0,
            k: 0,
        });

        #[cfg(target_arch = "x86_64")]
        {
            let cur = insns.len();
            // BPF_JMP | BPF_JGE | BPF_K (0x35) — reject x32 ABI bit
            insns.push(SockFilter {
                code: 0x35,
                jt: (deny_idx - (cur + 1)) as u8,
                jf: 0,
                k: 0x4000_0000,
            });
        }

        for &bad_nr in &denied {
            let cur = insns.len();
            insns.push(SockFilter {
                code: 0x15,
                jt: (deny_idx - (cur + 1)) as u8,
                jf: 0,
                k: bad_nr,
            });
        }

        // [total - 2]: SECCOMP_RET_ALLOW
        insns.push(SockFilter {
            code: 0x06,
            jt: 0,
            jf: 0,
            k: 0x7fff_0000,
        });
        // [total - 1]: SECCOMP_RET_ERRNO | EPERM (1)
        insns.push(SockFilter {
            code: 0x06,
            jt: 0,
            jf: 0,
            k: 0x0005_0001,
        });
        debug_assert_eq!(insns.len(), total);

        let prog = SockFprog {
            len: insns.len() as u16,
            filter: insns.as_ptr(),
        };
        let rc = unsafe {
            prctl(
                PR_SET_SECCOMP,
                SECCOMP_MODE_FILTER,
                &prog as *const SockFprog as c_ulong,
                0,
                0,
            )
        };
        if rc != 0 {
            return Err(format!("prctl(SECCOMP_MODE_FILTER) errno={}", last_errno()));
        }
        Ok(())
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    fn install_seccomp_denylist(
        _deny_fs_open: bool,
        _allow_stream_accept: bool,
    ) -> Result<(), String> {
        Ok(())
    }

    fn write_all_fd(fd: c_int, mut buf: &[u8]) -> Result<(), String> {
        while !buf.is_empty() {
            let n = unsafe { write(fd, buf.as_ptr() as *const c_void, buf.len()) };
            if n < 0 {
                let err = last_errno();
                if err == EINTR {
                    continue;
                }
                return Err(format!("pipe write failed: errno={err}"));
            }
            if n == 0 {
                return Err("pipe write returned 0".to_string());
            }
            buf = &buf[n as usize..];
        }
        Ok(())
    }

    fn read_exact_fd(fd: c_int, mut buf: &mut [u8]) -> Result<bool, String> {
        let mut read_any = false;
        while !buf.is_empty() {
            let n = unsafe { read(fd, buf.as_mut_ptr() as *mut c_void, buf.len()) };
            if n < 0 {
                let err = last_errno();
                if err == EINTR {
                    continue;
                }
                return Err(format!("pipe read failed: errno={err}"));
            }
            if n == 0 {
                if !read_any {
                    return Ok(false); // Clean EOF on message boundary
                }
                return Err("unexpected EOF from sandboxed JIT worker".to_string());
            }
            read_any = true;
            buf = &mut buf[n as usize..];
        }
        Ok(true)
    }

    fn write_u32_fd(fd: c_int, val: u32) -> Result<(), String> {
        write_all_fd(fd, &val.to_le_bytes())
    }

    fn read_u32_fd(fd: c_int) -> Result<u32, String> {
        let mut b = [0u8; 4];
        if !read_exact_fd(fd, &mut b)? {
            return Err("unexpected EOF reading u32 from worker".to_string());
        }
        Ok(u32::from_le_bytes(b))
    }

    fn write_bytes_fd(fd: c_int, data: &[u8]) -> Result<(), String> {
        write_u32_fd(fd, data.len() as u32)?;
        write_all_fd(fd, data)
    }

    fn read_bytes_fd(fd: c_int, max_len: usize) -> Result<Vec<u8>, String> {
        let len = read_u32_fd(fd)? as usize;
        if len > max_len {
            return Err(format!("worker message length {len} exceeds {max_len}"));
        }
        let mut out = vec![0u8; len];
        if len > 0 && !read_exact_fd(fd, &mut out)? {
            return Err("unexpected EOF reading payload from worker".to_string());
        }
        Ok(out)
    }

    pub struct WorkerInner {
        child_pid: c_int,
        cmd_write_fd: c_int,
        resp_read_fd: c_int,
    }

    impl WorkerInner {
        pub fn spawn(
            entry_addr: u64,
            mut rt_ns: RtNamespace,
            force_landlock_only: bool,
        ) -> Result<(Self, SandboxTier), String> {
            let _ = std::io::stdout().flush();
            let _ = std::io::stderr().flush();
            if !force_landlock_only {
                let _ = is_pivot_root_capable();
            }

            let mut cmd_fds = [-1, -1];
            let mut resp_fds = [-1, -1];
            if unsafe { pipe2(cmd_fds.as_mut_ptr(), O_CLOEXEC) } != 0 {
                return Err(format!("pipe2(cmd) errno={}", last_errno()));
            }
            if unsafe { pipe2(resp_fds.as_mut_ptr(), O_CLOEXEC) } != 0 {
                unsafe {
                    let _ = close(cmd_fds[0]);
                    let _ = close(cmd_fds[1]);
                }
                return Err(format!("pipe2(resp) errno={}", last_errno()));
            }

            let pid = unsafe { fork() };
            if pid < 0 {
                let err = last_errno();
                unsafe {
                    let _ = close(cmd_fds[0]);
                    let _ = close(cmd_fds[1]);
                    let _ = close(resp_fds[0]);
                    let _ = close(resp_fds[1]);
                }
                return Err(format!("fork() for sandboxed JIT worker failed: errno={err}"));
            }

            if pid == 0 {
                // ---- CHILD WORKER PROCESS (Single-Threaded, COW RX JIT Pages) ----
                let mut keep_fds = rt_ns.preserved_raw_fds();
                keep_fds.push(cmd_fds[0]);
                keep_fds.push(resp_fds[1]);
                keep_fds.sort_unstable();
                keep_fds.dedup();
                unsafe {
                    let _ = prctl(PR_SET_PDEATHSIG, SIGKILL, 0, 0, 0);
                    let _ = close(cmd_fds[1]);
                    let _ = close(resp_fds[0]);
                    close_other_fds_except(&keep_fds);
                }
                child_worker_loop(
                    entry_addr,
                    &mut rt_ns,
                    cmd_fds[0],
                    resp_fds[1],
                    force_landlock_only,
                );
            }

            // ---- PARENT PROCESS ----
            unsafe {
                let _ = close(cmd_fds[0]);
                let _ = close(resp_fds[1]);
            }
            let worker = Self {
                child_pid: pid,
                cmd_write_fd: cmd_fds[1],
                resp_read_fd: resp_fds[0],
            };

            let mut tag = [0u8; 2];
            if !read_exact_fd(worker.resp_read_fd, &mut tag)? {
                return Err("sandboxed JIT worker exited before handshake".to_string());
            }
            if tag[0] == 1 {
                let tier = SandboxTier::from_tag(tag[1])?;
                Ok((worker, tier))
            } else {
                let err_bytes = read_bytes_fd(worker.resp_read_fd, 65536)
                    .unwrap_or_else(|_| b"worker sandbox init error".to_vec());
                Err(String::from_utf8_lossy(&err_bytes).to_string())
            }
        }

        pub fn run_once(
            &mut self,
            fuel: i64,
            max_depth: i32,
            cli_args: &[Vec<u8>],
            dyn_vtable: &[u64],
        ) -> Result<(Vec<String>, bool, Option<String>), String> {
            let mut req = Vec::with_capacity(21 + dyn_vtable.len() * 8);
            req.push(1u8); // CMD_RUN
            req.extend_from_slice(&fuel.to_le_bytes());
            req.extend_from_slice(&max_depth.to_le_bytes());
            req.extend_from_slice(&(cli_args.len() as u32).to_le_bytes());
            req.extend_from_slice(&(dyn_vtable.len() as u32).to_le_bytes());
            for &fn_addr in dyn_vtable {
                req.extend_from_slice(&fn_addr.to_le_bytes());
            }
            write_all_fd(self.cmd_write_fd, &req)?;
            for a in cli_args {
                write_bytes_fd(self.cmd_write_fd, a)?;
            }

            let mut hdr = [0u8; 2];
            if !read_exact_fd(self.resp_read_fd, &mut hdr)? {
                return Err(self.diagnose_unexpected_exit());
            }
            let failed = hdr[0] != 0;
            let error = if hdr[1] != 0 {
                let eb = read_bytes_fd(self.resp_read_fd, 1_048_576)?;
                Some(String::from_utf8_lossy(&eb).to_string())
            } else {
                None
            };
            let emits_len = read_u32_fd(self.resp_read_fd)? as usize;
            if emits_len > 4_000_000 {
                return Err("worker emit count exceeded limit".to_string());
            }
            let mut emits = Vec::with_capacity(emits_len.min(4096));
            for _ in 0..emits_len {
                let lb = read_bytes_fd(self.resp_read_fd, 16 * 1024 * 1024)?;
                emits.push(String::from_utf8_lossy(&lb).to_string());
            }
            Ok((emits, failed, error))
        }

        fn diagnose_unexpected_exit(&mut self) -> String {
            let mut status: c_int = 0;
            let rc = unsafe { waitpid(self.child_pid, &mut status, 1 /* WNOHANG */) };
            if rc == self.child_pid {
                self.child_pid = -1;
                let sig = status & 0x7f;
                if sig != 0 {
                    return format!("sandboxed JIT worker terminated by signal {sig}");
                }
                let exit_code = (status >> 8) & 0xff;
                return format!("sandboxed JIT worker exited unexpectedly with code {exit_code}");
            }
            "sandboxed JIT worker closed response pipe unexpectedly".to_string()
        }
    }

    /// Close every file descriptor `>= 3` except sorted `keep_fds` inside the forked worker
    /// so sibling `OrcJitSession` pipe endpoints and unmounted host FDs never leak into the container!
    unsafe fn close_other_fds_except(keep_fds: &[c_int]) {
        const SYS_CLOSE_RANGE: c_long = 436;
        let mut ok = true;
        let mut cur_lo: u32 = 3;
        for &kfd in keep_fds {
            if kfd < 3 {
                continue;
            }
            let ku = kfd as u32;
            if ku > cur_lo {
                if unsafe { syscall(SYS_CLOSE_RANGE, cur_lo, ku - 1, 0u32) } != 0 {
                    ok = false;
                }
            }
            cur_lo = ku.saturating_add(1);
        }
        if cur_lo <= u32::MAX {
            if unsafe { syscall(SYS_CLOSE_RANGE, cur_lo, u32::MAX, 0u32) } != 0 {
                ok = false;
            }
        }
        if !ok {
            for fd in 3..1024 {
                if keep_fds.binary_search(&fd).is_err() {
                    let _ = unsafe { close(fd) };
                }
            }
        }
    }

    impl Drop for WorkerInner {
        fn drop(&mut self) {
            if self.cmd_write_fd >= 0 {
                // Send explicit CMD_SHUTDOWN (0) before closing pipe so child exits immediately
                // even if an external process held a duplicated descriptor.
                let _ = write_all_fd(self.cmd_write_fd, &[0u8]);
                unsafe {
                    let _ = close(self.cmd_write_fd);
                }
                self.cmd_write_fd = -1;
            }
            if self.resp_read_fd >= 0 {
                unsafe {
                    let _ = close(self.resp_read_fd);
                }
                self.resp_read_fd = -1;
            }
            if self.child_pid > 0 {
                let mut status: c_int = 0;
                loop {
                    let rc = unsafe { waitpid(self.child_pid, &mut status, 0) };
                    if rc >= 0 || last_errno() != EINTR {
                        break;
                    }
                }
                self.child_pid = -1;
            }
        }
    }

    fn child_worker_loop(
        entry_addr: u64,
        rt_ns: &mut RtNamespace,
        cmd_fd: c_int,
        resp_fd: c_int,
        force_landlock_only: bool,
    ) -> ! {
        match enter_sandbox_with_mode(rt_ns, force_landlock_only) {
            Ok(tier) => {
                if write_all_fd(resp_fd, &[1u8, tier.to_tag()]).is_err() {
                    unsafe { _exit(1) };
                }
            }
            Err(msg) => {
                let _ = write_all_fd(resp_fd, &[0u8, 0u8]);
                let _ = write_bytes_fd(resp_fd, msg.as_bytes());
                unsafe { _exit(1) };
            }
        }

        let entry_fn: unsafe extern "C" fn(*mut AodRtContext) =
            unsafe { std::mem::transmute(entry_addr as usize) };

        loop {
            let mut cmd_tag = [0u8; 1];
            match read_exact_fd(cmd_fd, &mut cmd_tag) {
                Ok(true) if cmd_tag[0] == 1 => {}
                _ => break,
            }
            let mut cmd_hdr = [0u8; 20];
            match read_exact_fd(cmd_fd, &mut cmd_hdr) {
                Ok(true) => {}
                Ok(false) | Err(_) => break,
            }
            let fuel = i64::from_le_bytes(cmd_hdr[0..8].try_into().unwrap());
            let max_depth = i32::from_le_bytes(cmd_hdr[8..12].try_into().unwrap());
            let args_count = u32::from_le_bytes(cmd_hdr[12..16].try_into().unwrap()) as usize;
            let vtable_count = u32::from_le_bytes(cmd_hdr[16..20].try_into().unwrap()) as usize;
            if vtable_count > 65_536 {
                break;
            }
            let mut dyn_vtable = Vec::with_capacity(vtable_count);
            let mut vt_ok = true;
            for _ in 0..vtable_count {
                let mut slot_buf = [0u8; 8];
                match read_exact_fd(cmd_fd, &mut slot_buf) {
                    Ok(true) => dyn_vtable.push(u64::from_le_bytes(slot_buf) as usize),
                    _ => {
                        vt_ok = false;
                        break;
                    }
                }
            }
            if !vt_ok {
                break;
            }
            let mut cli_args = Vec::with_capacity(args_count.min(256));
            let mut read_ok = true;
            for _ in 0..args_count {
                match read_bytes_fd(cmd_fd, 1_048_576) {
                    Ok(b) => cli_args.push(b),
                    Err(_) => {
                        read_ok = false;
                        break;
                    }
                }
            }
            if !read_ok {
                break;
            }

            let mut ctx = OwnedRtContext::new(fuel, max_depth, true);
            ctx.set_rt_namespace(rt_ns.clone());
            if !dyn_vtable.is_empty() {
                ctx.set_dyn_vtable(dyn_vtable);
            }
            if !cli_args.is_empty() {
                ctx.set_cli_args(cli_args);
            }
            let run_res = catch_unwind(AssertUnwindSafe(|| unsafe {
                entry_fn(ctx.as_mut_ptr());
            }));
            if run_res.is_err() {
                ctx.fail_with("internal panic inside sandboxed JIT entry");
            }

            let failed = ctx.failed();
            let err_opt = ctx.error_message().map(|s| s.as_bytes().to_vec());
            let emits = ctx.emits();
            let hdr = [u8::from(failed), u8::from(err_opt.is_some())];
            if write_all_fd(resp_fd, &hdr).is_err() {
                break;
            }
            if let Some(eb) = err_opt {
                if write_bytes_fd(resp_fd, &eb).is_err() {
                    break;
                }
            }
            if write_u32_fd(resp_fd, emits.len() as u32).is_err() {
                break;
            }
            let mut emits_ok = true;
            for line in emits {
                if write_bytes_fd(resp_fd, line.as_bytes()).is_err() {
                    emits_ok = false;
                    break;
                }
            }
            if !emits_ok {
                break;
            }
        }

        unsafe {
            let _ = close(cmd_fd);
            let _ = close(resp_fd);
            // CRITICAL: `_exit` bypasses parent C++ `atexit` handlers registered by `libLLVM-18.so`!
            _exit(0);
        }
    }
}

