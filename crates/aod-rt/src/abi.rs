#![allow(unsafe_code)]

use std::collections::BTreeMap;
use std::ffi::{c_char, CStr};
use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};

use aod_core::bigint::BigInt;
use aod_core::qext::QExtRatio;
use aod_core::ratio::Ratio;
use aod_core::support::SupportForm;
use aod_core::ternary::{Q4, T1, T32};
use aod_core::witness::{Coupling, Fusion};

use crate::arena::DEFAULT_MAX_ARENA_ENTRIES;
use crate::ops::{
    exact_addr8_parse, exact_coupling_contact, exact_q4, exact_qext_binop, exact_support_parse,
    exact_t1, exact_t32, exact_t32_unpack_bigint, exact_trace_new, RtState,
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

/// Unboxed-first exact integer (`%DecInt = type { i8, i64 }`).
/// - `tag == 0`: inline signed `i64` in `val`.
/// - `tag == 1`: 1-based arena handle `u64` (bit-cast to `i64`) referencing `RtValue::Int(BigInt)`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecInt {
    pub tag: u8,
    pub val: i64,
}

impl DecInt {
    pub const ZERO: Self = Self { tag: 0, val: 0 };
}

/// Unboxed-first exact canonical rational (`%DecRatio = type { i8, i64, i64 }`).
/// - `tag == 0`: inline reduced `num / den` with `den >= 1`.
/// - `tag == 1`: 1-based arena handle `u64` in `num` referencing `RtValue::Ratio(Ratio)`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecRatio {
    pub tag: u8,
    pub num: i64,
    pub den: i64,
}

impl DecRatio {
    pub const ZERO: Self = Self {
        tag: 0,
        num: 0,
        den: 1,
    };
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

    pub fn as_mut_ptr(&mut self) -> *mut AodRtContext {
        &mut self.raw
    }

    pub fn state(&self) -> &RtState {
        // SAFETY: `OwnedRtContext` owns `raw.inner` until `Drop`.
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

    /// Safely mark this context as failed and record an error message without crossing C ABI.
    pub fn fail_with(&mut self, msg: impl Into<String>) {
        self.raw.failed = 1;
        if !self.raw.inner.is_null() {
            // SAFETY: `OwnedRtContext` owns `raw.inner` exclusively via `&mut self` until `Drop`.
            let state = unsafe { &mut *self.raw.inner };
            state.record_error(msg.into());
        }
    }
}

impl Drop for OwnedRtContext {
    fn drop(&mut self) {
        if !self.raw.inner.is_null() {
            // SAFETY: allocated via `Box::into_raw` in `OwnedRtContext::with_arena_cap`.
            unsafe {
                drop(Box::from_raw(self.raw.inner));
            }
            self.raw.inner = std::ptr::null_mut();
        }
    }
}

// ============================================================================
// Panic-Safe FFI Boundary Guards (`P0-2`)
// ============================================================================

#[inline]
unsafe fn read_bytes<'a>(ptr: *const u8, len: usize) -> Result<&'a str, String> {
    if len == 0 {
        return Ok("");
    }
    if ptr.is_null() {
        return Err("null string pointer across runtime ABI".to_string());
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
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
unsafe fn write_int_out(out: *mut DecInt, tag: u8, val: i64) -> Result<(), String> {
    if out.is_null() {
        return Err("null DecInt out-pointer".to_string());
    }
    unsafe {
        (*out).tag = tag;
        (*out).val = val;
    }
    Ok(())
}

#[inline]
unsafe fn write_ratio_out(out: *mut DecRatio, tag: u8, num: i64, den: i64) -> Result<(), String> {
    if out.is_null() {
        return Err("null DecRatio out-pointer".to_string());
    }
    unsafe {
        (*out).tag = tag;
        (*out).num = num;
        (*out).den = den;
    }
    Ok(())
}

#[inline]
unsafe fn read_int_in(state: &RtState, p: *const DecInt) -> Result<BigInt, String> {
    if p.is_null() {
        return Err("null DecInt pointer".to_string());
    }
    let d = unsafe { *p };
    state.get_bigint(d.tag, d.val)
}

#[inline]
unsafe fn read_ratio_in(state: &RtState, p: *const DecRatio) -> Result<Ratio, String> {
    if p.is_null() {
        return Err("null DecRatio pointer".to_string());
    }
    let d = unsafe { *p };
    state.get_ratio(d.tag, d.num, d.den)
}

// ============================================================================
// Context Lifecycle, Standalone AOT CLI Arg Parsing, & Failure Helpers
// ============================================================================

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

/// Standalone AOT ELF CLI flag parser (`--fuel <N>`, `--max-depth <N>`).
/// Returns 0 on success, 1 on CLI usage error (printing `aodc: ...` to stderr).
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
        if argc > 1 && !argv.is_null() {
            let args = unsafe { std::slice::from_raw_parts(argv, argc as usize) };
            let mut i = 1usize;
            while i < args.len() {
                if args[i].is_null() {
                    i += 1;
                    continue;
                }
                let flag = unsafe { CStr::from_ptr(args[i]) }
                    .to_str()
                    .map_err(|_| "non-UTF8 CLI argument".to_string())?;
                match flag {
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
                            format!("invalid integer {val_s:?} for --fuel; expected positive integer")
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
                            return Err("--max-depth must be a positive integer (>= 1)".to_string());
                        }
                        max_depth = parsed;
                        i += 2;
                    }
                    other => {
                        return Err(format!(
                            "unknown argument {other:?} (supported: --fuel <N>, --max-depth <N>)"
                        ));
                    }
                }
            }
        }
        unsafe {
            aod_rt_ctx_init(ctx, fuel, max_depth, 0);
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
            let msg = read_bytes(ptr, len)?.to_string();
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
// DecInt Exact Cold-Spill Operations (`P0-3`)
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_from_str(
    ctx: *mut AodRtContext,
    ptr: *const u8,
    len: usize,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let text = read_bytes(ptr, len)?;
            let n = BigInt::parse_decimal(text).map_err(|e| e.to_string())?;
            let (tag, val) = s.pack_bigint(n)?;
            write_int_out(out, tag, val)
        });
    }
}

/// Spill an unsigned 64-bit value (e.g. `pack(t32)` with bit 63 set!) into `DecInt` (`P0-3`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_from_u64(
    ctx: *mut AodRtContext,
    uval: u64,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let (tag, val) = s.pack_bigint(BigInt::from_u64(uval))?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_neg(
    ctx: *mut AodRtContext,
    a: *const DecInt,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = read_int_in(s, a)?;
            let (tag, val) = s.pack_bigint(-av)?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_add(
    ctx: *mut AodRtContext,
    a: *const DecInt,
    b: *const DecInt,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = read_int_in(s, a)?;
            let bv = read_int_in(s, b)?;
            let (tag, val) = s.pack_bigint(&av + &bv)?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_sub(
    ctx: *mut AodRtContext,
    a: *const DecInt,
    b: *const DecInt,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = read_int_in(s, a)?;
            let bv = read_int_in(s, b)?;
            let (tag, val) = s.pack_bigint(&av - &bv)?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_mul(
    ctx: *mut AodRtContext,
    a: *const DecInt,
    b: *const DecInt,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = read_int_in(s, a)?;
            let bv = read_int_in(s, b)?;
            let (tag, val) = s.pack_bigint(&av * &bv)?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_div_floor(
    ctx: *mut AodRtContext,
    a: *const DecInt,
    b: *const DecInt,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = read_int_in(s, a)?;
            let bv = read_int_in(s, b)?;
            let q = av.div_floor(&bv).map_err(|e| e.to_string())?;
            let (tag, val) = s.pack_bigint(q)?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_rem_floor(
    ctx: *mut AodRtContext,
    a: *const DecInt,
    b: *const DecInt,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = read_int_in(s, a)?;
            let bv = read_int_in(s, b)?;
            let r = av.rem_floor(&bv).map_err(|e| e.to_string())?;
            let (tag, val) = s.pack_bigint(r)?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_exact_div(
    ctx: *mut AodRtContext,
    a: *const DecInt,
    b: *const DecInt,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = read_int_in(s, a)?;
            let bv = read_int_in(s, b)?;
            let r = Ratio::new(av, bv).map_err(|e| e.to_string())?;
            let (tag, num, den) = s.pack_ratio(r)?;
            write_ratio_out(out, tag, num, den)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_bitand(
    ctx: *mut AodRtContext,
    a: *const DecInt,
    b: *const DecInt,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = read_int_in(s, a)?;
            let bv = read_int_in(s, b)?;
            let r = av.bitand(&bv).map_err(|e| e.to_string())?;
            let (tag, val) = s.pack_bigint(r)?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_bitor(
    ctx: *mut AodRtContext,
    a: *const DecInt,
    b: *const DecInt,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = read_int_in(s, a)?;
            let bv = read_int_in(s, b)?;
            let r = av.bitor(&bv).map_err(|e| e.to_string())?;
            let (tag, val) = s.pack_bigint(r)?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_bitxor(
    ctx: *mut AodRtContext,
    a: *const DecInt,
    b: *const DecInt,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = read_int_in(s, a)?;
            let bv = read_int_in(s, b)?;
            let r = av.bitxor(&bv).map_err(|e| e.to_string())?;
            let (tag, val) = s.pack_bigint(r)?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_shl(
    ctx: *mut AodRtContext,
    a: *const DecInt,
    b: *const DecInt,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = read_int_in(s, a)?;
            let bv = read_int_in(s, b)?;
            let shift = bv
                .to_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| "invalid shift count".to_string())?;
            let r = av.shl(shift).map_err(|e| e.to_string())?;
            let (tag, val) = s.pack_bigint(r)?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_shr(
    ctx: *mut AodRtContext,
    a: *const DecInt,
    b: *const DecInt,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = read_int_in(s, a)?;
            let bv = read_int_in(s, b)?;
            let shift = bv
                .to_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| "invalid shift count".to_string())?;
            let r = av.shr(shift).map_err(|e| e.to_string())?;
            let (tag, val) = s.pack_bigint(r)?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_cmp(
    ctx: *mut AodRtContext,
    a: *const DecInt,
    b: *const DecInt,
) -> i32 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = read_int_in(s, a)?;
            let bv = read_int_in(s, b)?;
            Ok(match av.cmp(&bv) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_to_t1(ctx: *mut AodRtContext, a: *const DecInt) -> i8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = read_int_in(s, a)?;
            exact_t1(&av)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_to_t32(ctx: *mut AodRtContext, a: *const DecInt) -> i64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = read_int_in(s, a)?;
            exact_t32(&av)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_int_to_q4(ctx: *mut AodRtContext, a: *const DecInt) -> i8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = read_int_in(s, a)?;
            exact_q4(&av)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_t32_unpack_bigint(
    ctx: *mut AodRtContext,
    a: *const DecInt,
) -> i64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = read_int_in(s, a)?;
            exact_t32_unpack_bigint(&av)
        })
    }
}

// ============================================================================
// DecRatio Exact Cold-Spill Operations (`P0-1`: zero `__udivti3` in JIT!)
// ============================================================================

/// Reduce a split-128-bit fraction `(num_lo, num_hi) / (den_lo, den_hi)` into canonical `DecRatio`
/// without emitting `sdiv/urem i128` in ORC JIT (`P0-1`).
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
            let r = Ratio::new(BigInt::from_i128(n), BigInt::from_i128(d))
                .map_err(|e| e.to_string())?;
            let (tag, num, den) = s.pack_ratio(r)?;
            write_ratio_out(out, tag, num, den)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_from_ints(
    ctx: *mut AodRtContext,
    num: *const DecInt,
    den: *const DecInt,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let nv = read_int_in(s, num)?;
            let dv = read_int_in(s, den)?;
            let r = Ratio::new(nv, dv).map_err(|e| e.to_string())?;
            let (tag, rn, rd) = s.pack_ratio(r)?;
            write_ratio_out(out, tag, rn, rd)
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
            let ns = read_bytes(num_ptr, num_len)?;
            let ds = read_bytes(den_ptr, den_len)?;
            let nv = BigInt::parse_decimal(ns).map_err(|e| e.to_string())?;
            let dv = BigInt::parse_decimal(ds).map_err(|e| e.to_string())?;
            let r = Ratio::new(nv, dv).map_err(|e| e.to_string())?;
            let (tag, rn, rd) = s.pack_ratio(r)?;
            write_ratio_out(out, tag, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_neg(
    ctx: *mut AodRtContext,
    a: *const DecRatio,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = read_ratio_in(s, a)?;
            let (tag, rn, rd) = s.pack_ratio(-av)?;
            write_ratio_out(out, tag, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_add(
    ctx: *mut AodRtContext,
    a: *const DecRatio,
    b: *const DecRatio,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = read_ratio_in(s, a)?;
            let bv = read_ratio_in(s, b)?;
            let (tag, rn, rd) = s.pack_ratio(&av + &bv)?;
            write_ratio_out(out, tag, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_sub(
    ctx: *mut AodRtContext,
    a: *const DecRatio,
    b: *const DecRatio,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = read_ratio_in(s, a)?;
            let bv = read_ratio_in(s, b)?;
            let (tag, rn, rd) = s.pack_ratio(&av - &bv)?;
            write_ratio_out(out, tag, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_mul(
    ctx: *mut AodRtContext,
    a: *const DecRatio,
    b: *const DecRatio,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = read_ratio_in(s, a)?;
            let bv = read_ratio_in(s, b)?;
            let (tag, rn, rd) = s.pack_ratio(&av * &bv)?;
            write_ratio_out(out, tag, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_div(
    ctx: *mut AodRtContext,
    a: *const DecRatio,
    b: *const DecRatio,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = read_ratio_in(s, a)?;
            let bv = read_ratio_in(s, b)?;
            let q = av.checked_div(&bv).map_err(|e| e.to_string())?;
            let (tag, rn, rd) = s.pack_ratio(q)?;
            write_ratio_out(out, tag, rn, rd)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_cmp(
    ctx: *mut AodRtContext,
    a: *const DecRatio,
    b: *const DecRatio,
) -> i32 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = read_ratio_in(s, a)?;
            let bv = read_ratio_in(s, b)?;
            Ok(match av.cmp(&bv) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_ratio_to_int_exact(
    ctx: *mut AodRtContext,
    a: *const DecRatio,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = read_ratio_in(s, a)?;
            let iv = av.to_int_exact().map_err(|e| e.to_string())?;
            let (tag, val) = s.pack_bigint(iv)?;
            write_int_out(out, tag, val)
        });
    }
}

// ============================================================================
// Emit, Boxing/Unboxing, Strings, & Handle Equality
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_emit_int(ctx: *mut AodRtContext, a: *const DecInt) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let av = read_int_in(s, a)?;
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
            s.emit_line(t.render_v1());
            Ok(())
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_emit_t32(ctx: *mut AodRtContext, v: i64) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let t = T32::new(v as i128).map_err(|e| e.to_string())?;
            s.emit_line(t.render_v1());
            Ok(())
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_emit_q4(ctx: *mut AodRtContext, v: i8) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let q = Q4::new(v as u8).map_err(|e| e.to_string())?;
            s.emit_line(q.render_v1());
            Ok(())
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_emit_ratio(ctx: *mut AodRtContext, r: *const DecRatio) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let rv = read_ratio_in(s, r)?;
            s.emit_line(rv.render_v1());
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
            let text = read_bytes(ptr, len)?.to_string();
            s.arena.insert(RtValue::Str(text))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_handle_eq(ctx: *mut AodRtContext, a: u64, b: u64) -> u8 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = s.arena.get(a)?;
            let bv = s.arena.get(b)?;
            Ok(if av == bv { 1 } else { 0 })
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_unit(ctx: *mut AodRtContext) -> u64 {
    unsafe { guard_ret(ctx, 0, |_, s| s.arena.insert(RtValue::Unit)) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_from_int(ctx: *mut AodRtContext, a: *const DecInt) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = read_int_in(s, a)?;
            s.arena.insert(RtValue::Int(av))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_to_int(
    ctx: *mut AodRtContext,
    h: u64,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let n = match s.arena.get(h)? {
                RtValue::Int(n) => n.clone(),
                other => return Err(format!("expected int value, got {}", other.render())),
            };
            let (tag, val) = s.pack_bigint(n)?;
            write_int_out(out, tag, val)
        });
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
pub unsafe extern "C" fn aod_rt_val_from_t32(ctx: *mut AodRtContext, v: i64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let t = T32::new(v as i128).map_err(|e| e.to_string())?;
            s.arena.insert(RtValue::T32(t))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_val_to_t32(ctx: *mut AodRtContext, h: u64) -> i64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| match s.arena.get(h)? {
            RtValue::T32(t) => Ok(t.value() as i64),
            other => Err(format!("expected t32 value, got {}", other.render())),
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
pub unsafe extern "C" fn aod_rt_val_from_ratio(ctx: *mut AodRtContext, r: *const DecRatio) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let rv = read_ratio_in(s, r)?;
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
            let (tag, num, den) = s.pack_ratio(rv)?;
            write_ratio_out(out, tag, num, den)
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
            let text = read_bytes(ptr, len)?;
            let a = exact_addr8_parse(text)?;
            s.arena.insert(RtValue::Addr8(a))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_addr8_from_str(ctx: *mut AodRtContext, str_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let text = s.get_str(str_h)?.to_string();
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
            let text = read_bytes(ptr, len)?;
            let sf = exact_support_parse(text)?;
            s.arena.insert(RtValue::Support(sf))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_support_from_str(ctx: *mut AodRtContext, str_h: u64) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let text = s.get_str(str_h)?.to_string();
            let sf = exact_support_parse(&text)?;
            s.arena.insert(RtValue::Support(sf))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_support_new(
    ctx: *mut AodRtContext,
    p: *const DecInt,
    q: *const DecInt,
    len: *const DecInt,
    yro: u8,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let pv = read_int_in(s, p)?;
            let qv = read_int_in(s, q)?;
            let lv = read_int_in(s, len)?;
            let sf = SupportForm::new(pv, qv, lv, yro != 0).map_err(|e| e.to_string())?;
            s.arena.insert(RtValue::Support(sf))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_support_pi(ctx: *mut AodRtContext, h: u64, out: *mut DecInt) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let pi = s.get_support(h)?.pi().map_err(|e| e.to_string())?;
            let (tag, val) = s.pack_bigint(pi)?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_support_rd(
    ctx: *mut AodRtContext,
    h: u64,
    h_mu: *const DecInt,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let hv = read_int_in(s, h_mu)?;
            let rd = s.get_support(h)?.rd(&hv).map_err(|e| e.to_string())?;
            let (tag, val) = s.pack_bigint(rd)?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_support_pressure(
    ctx: *mut AodRtContext,
    h: u64,
    c3: *const DecInt,
    h_mu: *const DecInt,
    w: *const DecInt,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let c3v = read_int_in(s, c3)?;
            let hv = read_int_in(s, h_mu)?;
            let wv = read_int_in(s, w)?;
            let res = s
                .get_support(h)?
                .pressure(&c3v, &hv, &wv)
                .map_err(|e| e.to_string())?;
            let (tag, val) = s.pack_bigint(res)?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_support_overhang(
    ctx: *mut AodRtContext,
    h: u64,
    c3: *const DecInt,
    h_mu: *const DecInt,
    w: *const DecInt,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let c3v = read_int_in(s, c3)?;
            let hv = read_int_in(s, h_mu)?;
            let wv = read_int_in(s, w)?;
            let res = s
                .get_support(h)?
                .overhang(&c3v, &hv, &wv)
                .map_err(|e| e.to_string())?;
            let (tag, val) = s.pack_bigint(res)?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_shedding_excess(
    ctx: *mut AodRtContext,
    p_d: *const DecInt,
    c_close: *const DecInt,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let pv = read_int_in(s, p_d)?;
            let cv = read_int_in(s, c_close)?;
            let audit = SupportForm::shedding_audit(&pv, &cv).map_err(|e| e.to_string())?;
            let (tag, val) = s.pack_bigint(audit.x_shedding)?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sadar_promote(
    ctx: *mut AodRtContext,
    c3: *const DecInt,
    rho: *const DecInt,
    att: *const DecRatio,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let c3v = read_int_in(s, c3)?;
            let rv = read_int_in(s, rho)?;
            let av = read_ratio_in(s, att)?;
            let res = s.op_sadar_promote(&c3v, &rv, av)?;
            let (tag, num, den) = s.pack_ratio(res)?;
            write_ratio_out(out, tag, num, den)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_probability(
    ctx: *mut AodRtContext,
    neg: *const DecInt,
    hinge: *const DecInt,
    pos: *const DecInt,
    role: i8,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let nv = read_int_in(s, neg)?;
            let hv = read_int_in(s, hinge)?;
            let pv = read_int_in(s, pos)?;
            let res = s.op_probability(&nv, &hv, &pv, role)?;
            let (tag, num, den) = s.pack_ratio(res)?;
            write_ratio_out(out, tag, num, den)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_window_new(
    ctx: *mut AodRtContext,
    u: *const DecInt,
    v: *const DecInt,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let uv = read_int_in(s, u)?
                .to_i64()
                .ok_or_else(|| "window start out of i64 range".to_string())?;
            let vv = read_int_in(s, v)?
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
pub unsafe extern "C" fn aod_rt_range_new(
    ctx: *mut AodRtContext,
    a: *const DecInt,
    b: *const DecInt,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = read_int_in(s, a)?
                .to_i64()
                .ok_or_else(|| "range start out of i64 range".to_string())?;
            let bv = read_int_in(s, b)?
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
            let sc = s.get_str(sc_h)?.to_string();
            let lin = s.get_str(lin_h)?.to_string();
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
pub unsafe extern "C" fn aod_rt_trace_phases(
    ctx: *mut AodRtContext,
    t_h: u64,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let p = s.get_trace(t_h)?.phases().map_err(|e| e.to_string())?;
            let (tag, val) = s.pack_bigint(BigInt::from_u64(p as u64))?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_trace_beats(
    ctx: *mut AodRtContext,
    t_h: u64,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let b = s.get_trace(t_h)?.beats().map_err(|e| e.to_string())?;
            let (tag, val) = s.pack_bigint(BigInt::from_u64(b as u64))?;
            write_int_out(out, tag, val)
        });
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
            let (tag, num, den) = s.pack_ratio(r)?;
            write_ratio_out(out, tag, num, den)
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
            let sc = s.get_str(sc_h)?.to_string();
            let lin = s.get_str(lin_h)?.to_string();
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
            let w = s.get_str(w_h)?.to_string();
            let c = Coupling::new(&a, &b, w).map_err(|e| e.to_string())?;
            s.arena.insert(RtValue::Coupling(Box::new(c)))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_coupling_contact(
    ctx: *mut AodRtContext,
    c_h: u64,
    a: *const DecInt,
    b: *const DecInt,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let av = read_int_in(s, a)?;
            let bv = read_int_in(s, b)?;
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
            let (tag, num, den) = s.pack_ratio(r)?;
            write_ratio_out(out, tag, num, den)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_coupling_lag(
    ctx: *mut AodRtContext,
    c_h: u64,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let l = s.get_coupling(c_h)?.lag().map_err(|e| e.to_string())?;
            let (tag, val) = s.pack_bigint(l)?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_coupling_drift(
    ctx: *mut AodRtContext,
    c_h: u64,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let d = s.get_coupling(c_h)?.drift().map_err(|e| e.to_string())?;
            let (tag, val) = s.pack_bigint(d)?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_window_dedup_count(
    ctx: *mut AodRtContext,
    lin_h: u64,
    sc_h: u64,
    cuts_h: u64,
    wins_h: u64,
    out: *mut DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let cnt = s.op_window_dedup_count(lin_h, sc_h, cuts_h, wins_h)?;
            let (tag, val) = s.pack_bigint(cnt)?;
            write_int_out(out, tag, val)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_native_cadence_ratio(
    ctx: *mut AodRtContext,
    id_a_h: u64,
    q_a: *const DecInt,
    id_r_h: u64,
    q_r: *const DecInt,
    coupled: u8,
    bc: *const DecInt,
    out: *mut DecRatio,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let qav = read_int_in(s, q_a)?;
            let qrv = read_int_in(s, q_r)?;
            let bcv = read_int_in(s, bc)?;
            let r = s.op_native_cadence_ratio(id_a_h, &qav, id_r_h, &qrv, coupled != 0, &bcv)?;
            let (tag, num, den) = s.pack_ratio(r)?;
            write_ratio_out(out, tag, num, den)
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
    bc: *const DecInt,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let bcv = read_int_in(s, bc)?;
            s.op_single_contact_assoc(wid_h, pid_h, rid_h, win_h, &bcv)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_recipient_eval_demo(
    ctx: *mut AodRtContext,
    ek: *const DecInt,
    sk: *const DecInt,
    win_h: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let ekv = read_int_in(s, ek)?;
            let skv = read_int_in(s, sk)?;
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
    r: *const DecRatio,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let rv = read_ratio_in(s, r)?;
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
pub unsafe extern "C" fn aod_rt_qext_cmp(ctx: *mut AodRtContext, a_h: u64, b_h: u64) -> i32 {
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
            let rendered = s.get_qext_ratio(a_h)?.render_exact();
            s.arena.insert(RtValue::QExtStr(rendered))
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
    decimals: *const DecInt,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let q = s.get_qext_ratio(a_h)?.clone();
            let dv = read_int_in(s, decimals)?;
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
    decimals: *const DecInt,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let q = s.get_qext_ratio(a_h)?.clone();
            let dv = read_int_in(s, decimals)?;
            let dec = checked_qext_decimals(&dv)?;
            let formatted = q.format_fixed(dec).map_err(|e| e.to_string())?;
            s.arena.insert(RtValue::QExtStr(formatted))
        })
    }
}

// ============================================================================
// Containers & Composite Types (ALL Deep-Copy / Copy-on-Write per `P0-3`!)
// ============================================================================

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
            s.arena.insert(RtValue::List(items))
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_list_get(
    ctx: *mut AodRtContext,
    list_h: u64,
    idx: *const DecInt,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let iv = read_int_in(s, idx)?;
            let v = s.op_list_get(list_h, &iv)?;
            s.arena.insert(v)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_list_set(
    ctx: *mut AodRtContext,
    list_h: u64,
    idx: *const DecInt,
    val_h: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let iv = read_int_in(s, idx)?;
            s.op_list_set(list_h, &iv, val_h)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_list_len_raw(ctx: *mut AodRtContext, list_h: u64) -> i64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| match s.arena.get(list_h)? {
            RtValue::List(items) => Ok(items.len() as i64),
            other => Err(format!("expected list handle, got {}", other.render())),
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_len(ctx: *mut AodRtContext, h: u64, out: *mut DecInt) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let n = s.op_len(h)?;
            let (tag, val) = s.pack_bigint(n)?;
            write_int_out(out, tag, val)
        });
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
            s.arena.insert(RtValue::List(keys))
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
            s.arena.insert(RtValue::List(entries))
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
pub unsafe extern "C" fn aod_rt_struct_set(
    ctx: *mut AodRtContext,
    s_h: u64,
    field_ptr: *const u8,
    field_len: usize,
    val_h: u64,
) -> u64 {
    unsafe {
        guard_ret(ctx, 0, |_, s| {
            let fname = read_bytes(field_ptr, field_len)?.to_string();
            s.op_struct_set(s_h, &fname, val_h)
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
            let fname = read_bytes(field_ptr, field_len)?.to_string();
            let v = s.op_field_get(h, &fname)?;
            s.arena.insert(v)
        })
    }
}

// ============================================================================
// Sandbox Simulation C ABI (`P1-3`: fuel decrement & observer failure halt!)
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
pub unsafe extern "C" fn aod_rt_sim_builder_add_clock(
    ctx: *mut AodRtContext,
    b_h: u64,
    name_h: u64,
    period: *const DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let pv = read_int_in(s, period)?;
            s.op_sim_builder_add_clock(b_h, name_h, &pv)
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sim_builder_add_carrier(
    ctx: *mut AodRtContext,
    b_h: u64,
    support_h: u64,
    x: *const DecInt,
    y: *const DecInt,
    z: *const DecInt,
    dir_code: u8,
    orient: i8,
    shells: *const DecInt,
) {
    unsafe {
        guard_ret(ctx, (), |_, s| {
            let xv = read_int_in(s, x)?;
            let yv = read_int_in(s, y)?;
            let zv = read_int_in(s, z)?;
            let sv = read_int_in(s, shells)?;
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

/// Run the sandbox chamber for `ticks` ticks, decrementing `ctx.fuel` per tick and
/// checking `ctx.failed != 0` immediately after every procedural observer callback (`P1-3`, `P0-UB`).
///
/// Uses strictly non-overlapping raw-pointer scopes so ZERO `&mut AodRtContext` or
/// `&mut RtState` references are live across the re-entrant callback `cb(ctx, snap_h)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aod_rt_sim_run(
    ctx: *mut AodRtContext,
    b_h: u64,
    ticks: *const DecInt,
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
            let tv = unsafe { read_int_in(state, ticks)? };
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
        );
        let mut decl_observers = builder.decl_observers;
        let proc_observers = builder.proc_observers;

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
            if !proc_observers.is_empty() {
                let snap_h = {
                    let state = unsafe { &mut *(*ctx).inner };
                    state.arena.insert(RtValue::ChamberView(Box::new(snap)))?
                };
                for (_, cb_addr) in &proc_observers {
                    // SAFETY: `cb_addr` is a verified LLVM-compiled procedural observer callback
                    // with exact signature `extern "C" fn(*mut AodRtContext, u64)`. No Rust `&mut`
                    // references to `*ctx` or `*(*ctx).inner` exist across this call.
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
                    (*(*ctx).inner).record_error("internal runtime panic in aod_rt_sim_run".to_string());
                }
            }
            0
        }
    }
}

// ============================================================================
// Complete ORC LLJIT Runtime Symbol Table
// ============================================================================

/// Exported symbol table of every `aod_rt_*` C ABI function for ORC LLJIT registration.
pub fn runtime_symbol_table() -> Vec<(&'static str, usize)> {
    vec![
        ("aod_rt_ctx_init", aod_rt_ctx_init as *const () as usize),
        ("aod_rt_cli_parse_args", aod_rt_cli_parse_args as *const () as usize),
        ("aod_rt_ctx_finish", aod_rt_ctx_finish as *const () as usize),
        ("aod_rt_fail", aod_rt_fail as *const () as usize),
        ("aod_rt_assert_failed", aod_rt_assert_failed as *const () as usize),
        ("aod_rt_budget_exhausted", aod_rt_budget_exhausted as *const () as usize),
        ("aod_rt_int_from_str", aod_rt_int_from_str as *const () as usize),
        ("aod_rt_int_from_u64", aod_rt_int_from_u64 as *const () as usize),
        ("aod_rt_int_neg", aod_rt_int_neg as *const () as usize),
        ("aod_rt_int_add", aod_rt_int_add as *const () as usize),
        ("aod_rt_int_sub", aod_rt_int_sub as *const () as usize),
        ("aod_rt_int_mul", aod_rt_int_mul as *const () as usize),
        ("aod_rt_int_div_floor", aod_rt_int_div_floor as *const () as usize),
        ("aod_rt_int_rem_floor", aod_rt_int_rem_floor as *const () as usize),
        ("aod_rt_int_exact_div", aod_rt_int_exact_div as *const () as usize),
        ("aod_rt_int_bitand", aod_rt_int_bitand as *const () as usize),
        ("aod_rt_int_bitor", aod_rt_int_bitor as *const () as usize),
        ("aod_rt_int_bitxor", aod_rt_int_bitxor as *const () as usize),
        ("aod_rt_int_shl", aod_rt_int_shl as *const () as usize),
        ("aod_rt_int_shr", aod_rt_int_shr as *const () as usize),
        ("aod_rt_int_cmp", aod_rt_int_cmp as *const () as usize),
        ("aod_rt_int_to_t1", aod_rt_int_to_t1 as *const () as usize),
        ("aod_rt_int_to_t32", aod_rt_int_to_t32 as *const () as usize),
        ("aod_rt_int_to_q4", aod_rt_int_to_q4 as *const () as usize),
        ("aod_rt_t32_unpack_bigint", aod_rt_t32_unpack_bigint as *const () as usize),
        ("aod_rt_ratio_reduce_i128", aod_rt_ratio_reduce_i128 as *const () as usize),
        ("aod_rt_ratio_from_ints", aod_rt_ratio_from_ints as *const () as usize),
        ("aod_rt_ratio_from_str", aod_rt_ratio_from_str as *const () as usize),
        ("aod_rt_ratio_neg", aod_rt_ratio_neg as *const () as usize),
        ("aod_rt_ratio_add", aod_rt_ratio_add as *const () as usize),
        ("aod_rt_ratio_sub", aod_rt_ratio_sub as *const () as usize),
        ("aod_rt_ratio_mul", aod_rt_ratio_mul as *const () as usize),
        ("aod_rt_ratio_div", aod_rt_ratio_div as *const () as usize),
        ("aod_rt_ratio_cmp", aod_rt_ratio_cmp as *const () as usize),
        ("aod_rt_ratio_to_int_exact", aod_rt_ratio_to_int_exact as *const () as usize),
        ("aod_rt_emit_int", aod_rt_emit_int as *const () as usize),
        ("aod_rt_emit_bool", aod_rt_emit_bool as *const () as usize),
        ("aod_rt_emit_qext_bool", aod_rt_emit_qext_bool as *const () as usize),
        ("aod_rt_emit_t1", aod_rt_emit_t1 as *const () as usize),
        ("aod_rt_emit_t32", aod_rt_emit_t32 as *const () as usize),
        ("aod_rt_emit_q4", aod_rt_emit_q4 as *const () as usize),
        ("aod_rt_emit_ratio", aod_rt_emit_ratio as *const () as usize),
        ("aod_rt_emit_handle", aod_rt_emit_handle as *const () as usize),
        ("aod_rt_str_new", aod_rt_str_new as *const () as usize),
        ("aod_rt_handle_eq", aod_rt_handle_eq as *const () as usize),
        ("aod_rt_val_unit", aod_rt_val_unit as *const () as usize),
        ("aod_rt_val_from_int", aod_rt_val_from_int as *const () as usize),
        ("aod_rt_val_to_int", aod_rt_val_to_int as *const () as usize),
        ("aod_rt_val_from_bool", aod_rt_val_from_bool as *const () as usize),
        ("aod_rt_val_to_bool", aod_rt_val_to_bool as *const () as usize),
        ("aod_rt_val_from_qext_bool", aod_rt_val_from_qext_bool as *const () as usize),
        ("aod_rt_val_to_qext_bool", aod_rt_val_to_qext_bool as *const () as usize),
        ("aod_rt_val_from_t1", aod_rt_val_from_t1 as *const () as usize),
        ("aod_rt_val_to_t1", aod_rt_val_to_t1 as *const () as usize),
        ("aod_rt_val_from_t32", aod_rt_val_from_t32 as *const () as usize),
        ("aod_rt_val_to_t32", aod_rt_val_to_t32 as *const () as usize),
        ("aod_rt_val_from_q4", aod_rt_val_from_q4 as *const () as usize),
        ("aod_rt_val_to_q4", aod_rt_val_to_q4 as *const () as usize),
        ("aod_rt_val_from_ratio", aod_rt_val_from_ratio as *const () as usize),
        ("aod_rt_val_to_ratio", aod_rt_val_to_ratio as *const () as usize),
        ("aod_rt_addr8_parse", aod_rt_addr8_parse as *const () as usize),
        ("aod_rt_addr8_from_str", aod_rt_addr8_from_str as *const () as usize),
        ("aod_rt_support_parse", aod_rt_support_parse as *const () as usize),
        ("aod_rt_support_from_str", aod_rt_support_from_str as *const () as usize),
        ("aod_rt_support_new", aod_rt_support_new as *const () as usize),
        ("aod_rt_support_pi", aod_rt_support_pi as *const () as usize),
        ("aod_rt_support_rd", aod_rt_support_rd as *const () as usize),
        ("aod_rt_support_pressure", aod_rt_support_pressure as *const () as usize),
        ("aod_rt_support_overhang", aod_rt_support_overhang as *const () as usize),
        ("aod_rt_shedding_excess", aod_rt_shedding_excess as *const () as usize),
        ("aod_rt_sadar_promote", aod_rt_sadar_promote as *const () as usize),
        ("aod_rt_probability", aod_rt_probability as *const () as usize),
        ("aod_rt_window_new", aod_rt_window_new as *const () as usize),
        ("aod_rt_range_new", aod_rt_range_new as *const () as usize),
        ("aod_rt_range_bounds", aod_rt_range_bounds as *const () as usize),
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
        ("aod_rt_qext_ext_decimal", aod_rt_qext_ext_decimal as *const () as usize),
        ("aod_rt_qext_from_ratio", aod_rt_qext_from_ratio as *const () as usize),
        ("aod_rt_qext_binop", aod_rt_qext_binop as *const () as usize),
        ("aod_rt_qext_neg", aod_rt_qext_neg as *const () as usize),
        ("aod_rt_qext_cmp", aod_rt_qext_cmp as *const () as usize),
        ("aod_rt_qext_render", aod_rt_qext_render as *const () as usize),
        (
            "aod_rt_qext_round_half_even",
            aod_rt_qext_round_half_even as *const () as usize,
        ),
        (
            "aod_rt_qext_format_fixed",
            aod_rt_qext_format_fixed as *const () as usize,
        ),
        ("aod_rt_list_new", aod_rt_list_new as *const () as usize),
        ("aod_rt_list_get", aod_rt_list_get as *const () as usize),
        ("aod_rt_list_set", aod_rt_list_set as *const () as usize),
        ("aod_rt_list_len_raw", aod_rt_list_len_raw as *const () as usize),
        ("aod_rt_len", aod_rt_len as *const () as usize),
        ("aod_rt_tuple_new", aod_rt_tuple_new as *const () as usize),
        ("aod_rt_tuple_get", aod_rt_tuple_get as *const () as usize),
        ("aod_rt_map_new", aod_rt_map_new as *const () as usize),
        ("aod_rt_map_get", aod_rt_map_get as *const () as usize),
        ("aod_rt_map_set", aod_rt_map_set as *const () as usize),
        ("aod_rt_map_contains", aod_rt_map_contains as *const () as usize),
        ("aod_rt_map_keys_list", aod_rt_map_keys_list as *const () as usize),
        ("aod_rt_map_entries_list", aod_rt_map_entries_list as *const () as usize),
        ("aod_rt_struct_new", aod_rt_struct_new as *const () as usize),
        ("aod_rt_struct_set", aod_rt_struct_set as *const () as usize),
        ("aod_rt_enum_new", aod_rt_enum_new as *const () as usize),
        ("aod_rt_enum_tag", aod_rt_enum_tag as *const () as usize),
        ("aod_rt_enum_payload", aod_rt_enum_payload as *const () as usize),
        ("aod_rt_field_get", aod_rt_field_get as *const () as usize),
        ("aod_rt_sim_builder_new", aod_rt_sim_builder_new as *const () as usize),
        ("aod_rt_sim_builder_add_clock", aod_rt_sim_builder_add_clock as *const () as usize),
        ("aod_rt_sim_builder_add_carrier", aod_rt_sim_builder_add_carrier as *const () as usize),
        ("aod_rt_sim_builder_add_decl_obs", aod_rt_sim_builder_add_decl_obs as *const () as usize),
        ("aod_rt_sim_builder_add_proc_obs", aod_rt_sim_builder_add_proc_obs as *const () as usize),
        ("aod_rt_sim_run", aod_rt_sim_run as *const () as usize),
    ]
}
