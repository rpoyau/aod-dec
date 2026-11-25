#![allow(unsafe_code)]

use std::collections::BTreeMap;
use std::time::Instant;

use aod_core::bigint::BigInt;
use aod_core::json::JsonValue;
use aod_core::ratio::Ratio;
use aod_core::trit_int::{fast_pack_t32, T32_LIMIT};
use aod_rt::abi::{aod_rt_int_from_i64, OwnedRtContext};

use crate::hw_info::HostHardwareInfo;
use crate::jit::OrcJitSession;
use crate::plan::{CodegenPlan, OptLevel, UnrollHint, POLICY_SCOPE};
use crate::{LlvmError, LlvmResult};

// ============================================================================
// RDTSCP & Linux perf_event_open Capability Probing (`P0-2`)
// ============================================================================

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PerfCapability {
    Available,
    Restricted,
    Unsupported,
}

impl PerfCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            PerfCapability::Available => "available",
            PerfCapability::Restricted => "restricted",
            PerfCapability::Unsupported => "unsupported",
        }
    }
}

/// Architectural runtime check for `RDTSCP` via `CPUID` leaf `0x8000_0001` EDX bit 27 (`P0-2`).
pub fn has_rdtscp() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        static HAS_RDTSCP: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *HAS_RDTSCP.get_or_init(|| {
            let max_ext = core::arch::x86_64::__cpuid(0x8000_0000).eax;
            if max_ext < 0x8000_0001 {
                return false;
            }
            let ext = core::arch::x86_64::__cpuid(0x8000_0001);
            (ext.edx & (1 << 27)) != 0
        })
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// Safe guarded RDTSCP reader (`P0-2`): returns `Some(tsc)` ONLY on `x86_64` when runtime
/// CPUID leaf `0x8000_0001` EDX bit 27 confirms `RDTSCP` is supported; `None` otherwise.
#[inline]
pub fn read_rdtscp() -> Option<u64> {
    #[cfg(target_arch = "x86_64")]
    {
        if has_rdtscp() {
            let mut aux: u32 = 0;
            // SAFETY: CPUID leaf 0x8000_0001 bit 27 confirmed `rdtscp` is available on this CPU.
            return Some(unsafe { core::arch::x86_64::__rdtscp(&mut aux) });
        }
    }
    None
}

/// Exact 136-byte Linux `struct perf_event_attr` layout for `perf_event_open(2)` (`P0-2`).
#[repr(C)]
struct PerfEventAttr {
    type_: u32,
    size: u32,
    config: u64,
    sample_period: u64,
    sample_type: u64,
    read_format: u64,
    flags: u64,
    _rest: [u8; 88],
}

const _: () = {
    assert!(std::mem::size_of::<PerfEventAttr>() == 136);
};

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
unsafe extern "C" {
    fn syscall(num: std::ffi::c_long, ...) -> std::ffi::c_long;
    fn close(fd: std::ffi::c_int) -> std::ffi::c_int;
}

/// Honest Linux `perf_event_open` capability check (`P0-2`, NEVER fabricates PMU counters!).
pub fn probe_perf_event_open() -> PerfCapability {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        const SYS_PERF_EVENT_OPEN: std::ffi::c_long = 298;
        // disabled (bit 0) | exclude_kernel (bit 5) | exclude_hv (bit 6)
        let attr = PerfEventAttr {
            type_: 0, // PERF_TYPE_HARDWARE
            size: 136,
            config: 0, // PERF_COUNT_HW_CPU_CYCLES
            sample_period: 0,
            sample_type: 0,
            read_format: 0,
            flags: (1u64 << 0) | (1u64 << 5) | (1u64 << 6),
            _rest: [0u8; 88],
        };
        let fd = unsafe {
            syscall(
                SYS_PERF_EVENT_OPEN,
                &attr as *const PerfEventAttr,
                0 as std::ffi::c_int,
                -1 as std::ffi::c_int,
                -1 as std::ffi::c_int,
                0 as std::ffi::c_ulong,
            )
        };
        if fd >= 0 {
            unsafe {
                let _ = close(fd as std::ffi::c_int);
            }
            return PerfCapability::Available;
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(1) | Some(13) => PerfCapability::Restricted, // EPERM | EACCES
            _ => PerfCapability::Unsupported,
        }
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        PerfCapability::Unsupported
    }
}

fn time_closure<R, F: FnOnce() -> R>(f: F) -> (R, u64, Option<u64>) {
    let c0 = read_rdtscp();
    let t0 = Instant::now();
    let res = f();
    let ns = t0.elapsed().as_nanos().max(1) as u64;
    let c1 = read_rdtscp();
    let cycles = match (c0, c1) {
        (Some(a), Some(b)) if b >= a => Some(b - a),
        _ => None,
    };
    (res, ns, cycles)
}

// ============================================================================
// Structured Zero-Float Probe Reports
// ============================================================================

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryTierSample {
    pub bytes: usize,
    pub hops: u64,
    pub total_ns: u64,
    pub ns_per_hop_x100: u64,
    pub cycles_per_hop_x100: Option<u64>,
}

impl MemoryTierSample {
    pub fn to_json(&self) -> JsonValue {
        let mut m = BTreeMap::new();
        m.insert("bytes".to_string(), JsonValue::from_usize(self.bytes));
        m.insert(
            "cycles_per_hop_x100".to_string(),
            match self.cycles_per_hop_x100 {
                Some(v) => JsonValue::from_i64(v as i64),
                None => JsonValue::Null,
            },
        );
        m.insert("hops".to_string(), JsonValue::from_i64(self.hops as i64));
        m.insert(
            "ns_per_hop_x100".to_string(),
            JsonValue::from_i64(self.ns_per_hop_x100 as i64),
        );
        m.insert(
            "total_ns".to_string(),
            JsonValue::from_i64(self.total_ns as i64),
        );
        JsonValue::Object(m)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryTiersReport {
    pub tiers: Vec<MemoryTierSample>,
    pub recommended_l1_working_set_bytes: usize,
    pub verified: bool,
}

impl MemoryTiersReport {
    pub fn to_json(&self) -> JsonValue {
        let mut m = BTreeMap::new();
        m.insert(
            "recommended_l1_working_set_bytes".to_string(),
            JsonValue::from_usize(self.recommended_l1_working_set_bytes),
        );
        m.insert(
            "tiers".to_string(),
            JsonValue::Array(self.tiers.iter().map(MemoryTierSample::to_json).collect()),
        );
        m.insert("verified".to_string(), JsonValue::Bool(self.verified));
        JsonValue::Object(m)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExactArithReport {
    pub i64_checked_add_ns_per_op_x100: u64,
    pub t32_checked_add_ns_per_op_x100: u64,
    pub ratio_inline_gcd_ns_per_op_x100: u64,
    pub bigint_spill_add_ns_per_op_x100: u64,
    pub spill_to_fast_ratio_x100: u64,
    pub verified: bool,
}

impl ExactArithReport {
    pub fn to_json(&self) -> JsonValue {
        let mut m = BTreeMap::new();
        m.insert(
            "bigint_spill_add_ns_per_op_x100".to_string(),
            JsonValue::from_i64(self.bigint_spill_add_ns_per_op_x100 as i64),
        );
        m.insert(
            "i64_checked_add_ns_per_op_x100".to_string(),
            JsonValue::from_i64(self.i64_checked_add_ns_per_op_x100 as i64),
        );
        m.insert(
            "ratio_inline_gcd_ns_per_op_x100".to_string(),
            JsonValue::from_i64(self.ratio_inline_gcd_ns_per_op_x100 as i64),
        );
        m.insert(
            "spill_to_fast_ratio_x100".to_string(),
            JsonValue::from_i64(self.spill_to_fast_ratio_x100 as i64),
        );
        m.insert(
            "t32_checked_add_ns_per_op_x100".to_string(),
            JsonValue::from_i64(self.t32_checked_add_ns_per_op_x100 as i64),
        );
        m.insert("verified".to_string(), JsonValue::Bool(self.verified));
        JsonValue::Object(m)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BranchPredictorReport {
    pub predictable_branch_ns: u64,
    pub predictable_select_ns: u64,
    pub irregular_branch_ns: u64,
    pub irregular_select_ns: u64,
    pub prefer_branchless_irregular: bool,
    pub verified: bool,
}

impl BranchPredictorReport {
    pub fn to_json(&self) -> JsonValue {
        let mut m = BTreeMap::new();
        m.insert(
            "irregular_branch_ns".to_string(),
            JsonValue::from_i64(self.irregular_branch_ns as i64),
        );
        m.insert(
            "irregular_select_ns".to_string(),
            JsonValue::from_i64(self.irregular_select_ns as i64),
        );
        m.insert(
            "predictable_branch_ns".to_string(),
            JsonValue::from_i64(self.predictable_branch_ns as i64),
        );
        m.insert(
            "predictable_select_ns".to_string(),
            JsonValue::from_i64(self.predictable_select_ns as i64),
        );
        m.insert(
            "prefer_branchless_irregular".to_string(),
            JsonValue::Bool(self.prefer_branchless_irregular),
        );
        m.insert("verified".to_string(), JsonValue::Bool(self.verified));
        JsonValue::Object(m)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnrollLayoutReport {
    pub scalar_generic_ns: u64,
    pub unroll4_generic_ns: u64,
    pub unroll4_native_cpu_ns: u64,
    pub aos_ns: u64,
    pub soa_ns: u64,
    pub recommended_unroll: UnrollHint,
    pub prefer_native_cpu: bool,
    pub prefer_soa: bool,
    pub verified: bool,
}

impl UnrollLayoutReport {
    pub fn to_json(&self) -> JsonValue {
        let mut m = BTreeMap::new();
        m.insert("aos_ns".to_string(), JsonValue::from_i64(self.aos_ns as i64));
        m.insert(
            "prefer_native_cpu".to_string(),
            JsonValue::Bool(self.prefer_native_cpu),
        );
        m.insert("prefer_soa".to_string(), JsonValue::Bool(self.prefer_soa));
        m.insert(
            "recommended_unroll".to_string(),
            JsonValue::from_str_val(self.recommended_unroll.as_str()),
        );
        m.insert(
            "scalar_generic_ns".to_string(),
            JsonValue::from_i64(self.scalar_generic_ns as i64),
        );
        m.insert("soa_ns".to_string(), JsonValue::from_i64(self.soa_ns as i64));
        m.insert(
            "unroll4_generic_ns".to_string(),
            JsonValue::from_i64(self.unroll4_generic_ns as i64),
        );
        m.insert(
            "unroll4_native_cpu_ns".to_string(),
            JsonValue::from_i64(self.unroll4_native_cpu_ns as i64),
        );
        m.insert("verified".to_string(), JsonValue::Bool(self.verified));
        JsonValue::Object(m)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MulticoreReport {
    pub tested_workers: usize,
    pub single_thread_ns: u64,
    pub multi_thread_ns: u64,
    pub scaling_x100: u64,
    pub recommended_workers: usize,
    pub verified: bool,
}

impl MulticoreReport {
    pub fn to_json(&self) -> JsonValue {
        let mut m = BTreeMap::new();
        m.insert(
            "multi_thread_ns".to_string(),
            JsonValue::from_i64(self.multi_thread_ns as i64),
        );
        m.insert(
            "recommended_workers".to_string(),
            JsonValue::from_usize(self.recommended_workers),
        );
        m.insert(
            "scaling_x100".to_string(),
            JsonValue::from_i64(self.scaling_x100 as i64),
        );
        m.insert(
            "single_thread_ns".to_string(),
            JsonValue::from_i64(self.single_thread_ns as i64),
        );
        m.insert(
            "tested_workers".to_string(),
            JsonValue::from_usize(self.tested_workers),
        );
        m.insert("verified".to_string(), JsonValue::Bool(self.verified));
        JsonValue::Object(m)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HwProbeReport {
    pub policy_scope: String,
    pub clock_source: String,
    pub perf_event_open: PerfCapability,
    pub rdtscp_available: bool,
    pub hardware: HostHardwareInfo,
    pub memory_tiers: MemoryTiersReport,
    pub exact_arith: ExactArithReport,
    pub branch_predictor: BranchPredictorReport,
    pub unroll_layout: UnrollLayoutReport,
    pub multicore: MulticoreReport,
    pub total_probe_ms: u64,
}

impl HwProbeReport {
    pub fn to_json(&self) -> JsonValue {
        let mut m = BTreeMap::new();
        m.insert(
            "branch_predictor".to_string(),
            self.branch_predictor.to_json(),
        );
        m.insert(
            "clock_source".to_string(),
            JsonValue::from_str_val(self.clock_source.clone()),
        );
        m.insert("exact_arith".to_string(), self.exact_arith.to_json());
        m.insert("hardware".to_string(), self.hardware.to_json());
        m.insert("memory_tiers".to_string(), self.memory_tiers.to_json());
        m.insert("multicore".to_string(), self.multicore.to_json());
        m.insert(
            "perf_event_open".to_string(),
            JsonValue::from_str_val(self.perf_event_open.as_str()),
        );
        m.insert(
            "policy_scope".to_string(),
            JsonValue::from_str_val(self.policy_scope.clone()),
        );
        m.insert(
            "rdtscp_available".to_string(),
            JsonValue::Bool(self.rdtscp_available),
        );
        m.insert(
            "total_probe_ms".to_string(),
            JsonValue::from_i64(self.total_probe_ms as i64),
        );
        m.insert("unroll_layout".to_string(), self.unroll_layout.to_json());
        JsonValue::Object(m)
    }
}

// ============================================================================
// Probe IR Templates (Exposed for Post-Opt IR Verification in Tests!)
// ============================================================================

/// Returns LLVM 18 IR for Probe 3 (`branching == true` retains an actual conditional `br i1`
/// after `-O2` via volatile stack scratch; `branching == false` emits pure `select`, `P0-2`).
pub fn probe_branch_kernel_ir(branching: bool) -> &'static str {
    if branching {
        r#"
define i64 @probe_branch_kernel(ptr readonly %arr, i64 %len) {
entry:
  %scratch = alloca i64, align 8
  %empty = icmp sle i64 %len, 0
  br i1 %empty, label %done, label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i_next, %merge ]
  %acc = phi i64 [ 0, %entry ], [ %acc_next, %merge ]
  %p = getelementptr inbounds i64, ptr %arr, i64 %i
  %v = load i64, ptr %p
  %bit = and i64 %v, 1
  %cond = icmp ne i64 %bit, 0
  br i1 %cond, label %taken, label %not_taken
taken:
  %tval = add i64 %v, 7
  store volatile i64 %tval, ptr %scratch
  br label %merge
not_taken:
  %fval = sub i64 %v, 3
  store volatile i64 %fval, ptr %scratch
  br label %merge
merge:
  %picked = load volatile i64, ptr %scratch
  %acc_next = add i64 %acc, %picked
  %i_next = add nsw i64 %i, 1
  %more = icmp slt i64 %i_next, %len
  br i1 %more, label %loop, label %done
done:
  %res = phi i64 [ 0, %entry ], [ %acc_next, %merge ]
  ret i64 %res
}
"#
    } else {
        r#"
define i64 @probe_branch_kernel(ptr readonly %arr, i64 %len) {
entry:
  %empty = icmp sle i64 %len, 0
  br i1 %empty, label %done, label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i_next, %loop ]
  %acc = phi i64 [ 0, %entry ], [ %acc_next, %loop ]
  %p = getelementptr inbounds i64, ptr %arr, i64 %i
  %v = load i64, ptr %p
  %bit = and i64 %v, 1
  %cond = icmp ne i64 %bit, 0
  %tval = add i64 %v, 7
  %fval = sub i64 %v, 3
  %picked = select i1 %cond, i64 %tval, i64 %fval
  %acc_next = add i64 %acc, %picked
  %i_next = add nsw i64 %i, 1
  %more = icmp slt i64 %i_next, %len
  br i1 %more, label %loop, label %done
done:
  %res = phi i64 [ 0, %entry ], [ %acc_next, %loop ]
  ret i64 %res
}
"#
    }
}

// ============================================================================
// Execute All 5 Live Self-Verifying ORC LLJIT Micro-Probes
// ============================================================================

pub fn run_all_hw_probes() -> LlvmResult<HwProbeReport> {
    let total_t0 = Instant::now();
    let hardware = HostHardwareInfo::discover()?;
    let perf_event_open = probe_perf_event_open();
    let rdtscp_available = read_rdtscp().is_some();
    let clock_source = if rdtscp_available {
        "rdtscp+CLOCK_MONOTONIC_RAW".to_string()
    } else {
        "CLOCK_MONOTONIC_RAW".to_string()
    };

    let memory_tiers = run_probe_memory_tiers(hardware.l1d_bytes)?;
    let exact_arith = run_probe_exact_arith()?;
    let branch_predictor = run_probe_branch_predictor()?;
    let unroll_layout = run_probe_unroll_layout()?;
    let multicore = run_probe_multicore(hardware.logical_cores)?;

    Ok(HwProbeReport {
        policy_scope: POLICY_SCOPE.to_string(),
        clock_source,
        perf_event_open,
        rdtscp_available,
        hardware,
        memory_tiers,
        exact_arith,
        branch_predictor,
        unroll_layout,
        multicore,
        total_probe_ms: total_t0.elapsed().as_millis() as u64,
    })
}

fn run_probe_memory_tiers(sysfs_l1d_bytes: u64) -> LlvmResult<MemoryTiersReport> {
    let ir = r#"
define i64 @probe_mem_chase(ptr readonly %arr, i64 %hops) {
entry:
  %z = icmp sle i64 %hops, 0
  br i1 %z, label %done, label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i_next, %loop ]
  %idx = phi i64 [ 0, %entry ], [ %next, %loop ]
  %sum = phi i64 [ 0, %entry ], [ %sum_next, %loop ]
  %p = getelementptr inbounds i64, ptr %arr, i64 %idx
  %next = load i64, ptr %p
  %sum_next = add i64 %sum, %next
  %i_next = add nsw i64 %i, 1
  %more = icmp slt i64 %i_next, %hops
  br i1 %more, label %loop, label %done
done:
  %res = phi i64 [ 0, %entry ], [ %sum_next, %loop ]
  ret i64 %res
}
"#;
    let sess = OrcJitSession::compile_named_with_plan(ir, &CodegenPlan::baseline_o2(), None)?;
    let addr = sess.lookup_symbol_addr("probe_mem_chase")?;
    let f: unsafe extern "C" fn(*const i64, i64) -> i64 =
        unsafe { std::mem::transmute(addr as usize) };

    let sizes: [usize; 6] = [4_096, 16_384, 65_536, 524_288, 4_194_304, 33_554_432];
    let hops: i64 = 65_536;
    let mut tiers = Vec::with_capacity(sizes.len());

    for bytes in sizes {
        let n = bytes / 8; // exact power of 2 >= 512
        let mask = n - 1;
        let stride = 131usize; // odd -> coprime to every power-of-2 n, > 64B cache line!
        let mut ring = vec![0i64; n];
        for (i, slot) in ring.iter_mut().enumerate() {
            *slot = ((i + stride) & mask) as i64;
        }

        // Exact Rust reference checksum
        let mut ref_idx = 0usize;
        let mut ref_sum = 0i64;
        for _ in 0..hops {
            ref_idx = ring[ref_idx] as usize;
            ref_sum = ref_sum.wrapping_add(ref_idx as i64);
        }

        // Warmup + timed run
        let _ = unsafe { f(ring.as_ptr(), 1024) };
        let (got, ns, cyc) = time_closure(|| unsafe { f(ring.as_ptr(), hops) });
        if got != ref_sum {
            return Err(LlvmError::new(format!(
                "probe_memory_tiers({bytes}) checksum mismatch: jit={got} vs rust={ref_sum}"
            )));
        }
        let ns_per_hop_x100 = ((ns as u128).saturating_mul(100) / (hops as u128)) as u64;
        let cycles_per_hop_x100 =
            cyc.map(|c| ((c as u128).saturating_mul(100) / (hops as u128)) as u64);
        tiers.push(MemoryTierSample {
            bytes,
            hops: hops as u64,
            total_ns: ns,
            ns_per_hop_x100,
            cycles_per_hop_x100,
        });
    }

    let base_lat = tiers[0].ns_per_hop_x100.max(1);
    let mut rec = tiers[0].bytes;
    for t in &tiers {
        if t.ns_per_hop_x100 <= base_lat.saturating_mul(3) / 2 {
            rec = t.bytes;
        }
    }
    if sysfs_l1d_bytes > 0 && (rec as u64) > sysfs_l1d_bytes {
        rec = sysfs_l1d_bytes as usize;
    }

    Ok(MemoryTiersReport {
        tiers,
        recommended_l1_working_set_bytes: rec,
        verified: true,
    })
}

fn run_probe_exact_arith() -> LlvmResult<ExactArithReport> {
    let ir = r#"
declare { i64, i1 } @llvm.sadd.with.overflow.i64(i64, i64)
declare i128 @llvm.cttz.i128(i128, i1 immarg)
declare i64 @aod_rt_int_add(ptr, i64, i64)

define i64 @probe_i64_checked_add(i64 %n) {
entry:
  %z = icmp sle i64 %n, 0
  br i1 %z, label %done, label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i_next, %ok ]
  %acc = phi i64 [ 0, %entry ], [ %sum, %ok ]
  %step = and i64 %i, 15
  %ov_r = call { i64, i1 } @llvm.sadd.with.overflow.i64(i64 %acc, i64 %step)
  %sum = extractvalue { i64, i1 } %ov_r, 0
  %ov = extractvalue { i64, i1 } %ov_r, 1
  br i1 %ov, label %done, label %ok
ok:
  %i_next = add nsw i64 %i, 1
  %more = icmp slt i64 %i_next, %n
  br i1 %more, label %loop, label %done
done:
  %res = phi i64 [ 0, %entry ], [ -1, %loop ], [ %sum, %ok ]
  ret i64 %res
}

define i64 @probe_t32_checked_add(i64 %n) {
entry:
  %z = icmp sle i64 %n, 0
  br i1 %z, label %done, label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i_next, %ok ]
  %acc = phi i64 [ 0, %entry ], [ %sum, %ok ]
  %step0 = and i64 %i, 7
  %step = sub i64 %step0, 3
  %ov_r = call { i64, i1 } @llvm.sadd.with.overflow.i64(i64 %acc, i64 %step)
  %sum = extractvalue { i64, i1 } %ov_r, 0
  %ov = extractvalue { i64, i1 } %ov_r, 1
  %lo = icmp sge i64 %sum, -926510094425920
  %hi = icmp sle i64 %sum, 926510094425920
  %in_rng = and i1 %lo, %hi
  %not_ov = xor i1 %ov, true
  %good = and i1 %not_ov, %in_rng
  br i1 %good, label %ok, label %done
ok:
  %i_next = add nsw i64 %i, 1
  %more = icmp slt i64 %i_next, %n
  br i1 %more, label %loop, label %done
done:
  %res = phi i64 [ 0, %entry ], [ -1, %loop ], [ %sum, %ok ]
  ret i64 %res
}

define i64 @probe_ratio_inline_gcd(i64 %n) {
entry:
  %z = icmp sle i64 %n, 0
  br i1 %z, label %done, label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i_next, %next ]
  %rn = phi i64 [ 0, %entry ], [ %rn_next, %next ]
  %rd = phi i64 [ 1, %entry ], [ %rd_next, %next ]
  %k = and i64 %i, 7
  %num_a = sext i64 %rn to i128
  %den_a = sext i64 %rd to i128
  %k128 = sext i64 %k to i128
  %t1 = mul i128 %num_a, 8
  %t2 = mul i128 %k128, %den_a
  %num = add i128 %t1, %t2
  %den = mul i128 %den_a, 8
  %nz = icmp eq i128 %num, 0
  br i1 %nz, label %is_zero, label %reduce
is_zero:
  br label %next
reduce:
  %tz_n = call i128 @llvm.cttz.i128(i128 %num, i1 true)
  %tz_d = call i128 @llvm.cttz.i128(i128 %den, i1 true)
  %lt = icmp ult i128 %tz_n, %tz_d
  %sh = select i1 %lt, i128 %tz_n, i128 %tz_d
  %nr = ashr i128 %num, %sh
  %dr = lshr i128 %den, %sh
  %nr64 = trunc i128 %nr to i64
  %dr64 = trunc i128 %dr to i64
  br label %next
next:
  %rn_next = phi i64 [ 0, %is_zero ], [ %nr64, %reduce ]
  %rd_next = phi i64 [ 1, %is_zero ], [ %dr64, %reduce ]
  %i_next = add nsw i64 %i, 1
  %more = icmp slt i64 %i_next, %n
  br i1 %more, label %loop, label %done
done:
  %f_rn = phi i64 [ 0, %entry ], [ %rn_next, %next ]
  %f_rd = phi i64 [ 1, %entry ], [ %rd_next, %next ]
  %comb = add i64 %f_rn, %f_rd
  ret i64 %comb
}

define i64 @probe_bigint_spill(ptr %ctx, i64 %init_a, i64 %step_b, i64 %n) {
entry:
  %z = icmp sle i64 %n, 0
  br i1 %z, label %done, label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i_next, %loop ]
  %cur = phi i64 [ %init_a, %entry ], [ %nxt, %loop ]
  %nxt = call i64 @aod_rt_int_add(ptr %ctx, i64 %cur, i64 %step_b)
  %i_next = add nsw i64 %i, 1
  %more = icmp slt i64 %i_next, %n
  br i1 %more, label %loop, label %done
done:
  %fin = phi i64 [ %init_a, %entry ], [ %nxt, %loop ]
  ret i64 %fin
}
"#;
    let sess = OrcJitSession::compile_named_with_plan(ir, &CodegenPlan::baseline_o2(), None)?;
    let f_i64: unsafe extern "C" fn(i64) -> i64 =
        unsafe { std::mem::transmute(sess.lookup_symbol_addr("probe_i64_checked_add")? as usize) };
    let f_t32: unsafe extern "C" fn(i64) -> i64 =
        unsafe { std::mem::transmute(sess.lookup_symbol_addr("probe_t32_checked_add")? as usize) };
    let f_rat: unsafe extern "C" fn(i64) -> i64 =
        unsafe { std::mem::transmute(sess.lookup_symbol_addr("probe_ratio_inline_gcd")? as usize) };
    let f_big: unsafe extern "C" fn(*mut aod_rt::abi::AodRtContext, u64, u64, i64) -> u64 =
        unsafe { std::mem::transmute(sess.lookup_symbol_addr("probe_bigint_spill")? as usize) };

    let n_fast: i64 = 50_000;
    let n_rat: i64 = 20_000;
    let n_big: i64 = 2_000;

    // 1. i64 checked add
    let ref_i64: i64 = (0..n_fast).map(|i| i & 15).sum();
    let (got_i64, ns_i64, _) = time_closure(|| unsafe { f_i64(n_fast) });
    if got_i64 != ref_i64 {
        return Err(LlvmError::new("probe_i64_checked_add verification mismatch"));
    }

    // 2. Inline 32-trit shadow-ALU checked add
    let mut ref_t32: i64 = 0;
    for i in 0..n_fast {
        ref_t32 += (i & 7) - 3;
        debug_assert!((-T32_LIMIT..=T32_LIMIT).contains(&ref_t32));
    }
    let (got_t32, ns_t32, _) = time_closure(|| unsafe { f_t32(n_fast) });
    if got_t32 != ref_t32 {
        return Err(LlvmError::new("probe_t32_checked_add verification mismatch"));
    }

    // 3. ratio inline GCD
    let mut ref_r = Ratio::from_i64(0, 1).unwrap();
    for i in 0..n_rat {
        ref_r = &ref_r + &Ratio::from_i64(i & 7, 8).unwrap();
    }
    let ref_r_comb = ref_r.num().to_i64().unwrap() + ref_r.den().to_i64().unwrap();
    let (got_rat, ns_rat, _) = time_closure(|| unsafe { f_rat(n_rat) });
    if got_rat != ref_r_comb {
        return Err(LlvmError::new("probe_ratio_inline_gcd verification mismatch"));
    }

    // 4. BigTrit 1-GPR `0b11` spill add via real `aod_rt_int_add`
    let mut rt_ctx = OwnedRtContext::new(100_000, 64, true);
    let init_a = unsafe { aod_rt_int_from_i64(rt_ctx.as_mut_ptr(), T32_LIMIT) };
    let step_b = fast_pack_t32(1);
    let (out_w, ns_big, _) = time_closure(|| unsafe {
        f_big(rt_ctx.as_mut_ptr(), init_a, step_b, n_big)
    });
    if rt_ctx.failed() {
        return Err(LlvmError::new("probe_bigint_spill runtime error"));
    }
    let got_big = rt_ctx
        .state()
        .unpack_dec_int(out_w)
        .map_err(LlvmError::new)?;
    let expected_big = &BigInt::from_i64(T32_LIMIT) + &BigInt::from_i64(n_big);
    if got_big != expected_big {
        return Err(LlvmError::new("probe_bigint_spill verification mismatch"));
    }

    let i64_x100 = ((ns_i64 as u128).saturating_mul(100) / (n_fast as u128)).max(1) as u64;
    let t32_x100 = ((ns_t32 as u128).saturating_mul(100) / (n_fast as u128)).max(1) as u64;
    let rat_x100 = ((ns_rat as u128).saturating_mul(100) / (n_rat as u128)).max(1) as u64;
    let big_x100 = ((ns_big as u128).saturating_mul(100) / (n_big as u128)).max(1) as u64;
    let spill_ratio = ((big_x100 as u128).saturating_mul(100) / (i64_x100 as u128)) as u64;

    Ok(ExactArithReport {
        i64_checked_add_ns_per_op_x100: i64_x100,
        t32_checked_add_ns_per_op_x100: t32_x100,
        ratio_inline_gcd_ns_per_op_x100: rat_x100,
        bigint_spill_add_ns_per_op_x100: big_x100,
        spill_to_fast_ratio_x100: spill_ratio,
        verified: true,
    })
}

fn run_probe_branch_predictor() -> LlvmResult<BranchPredictorReport> {
    let n = 16_384usize;
    let predictable: Vec<i64> = (0..n).map(|i| if i < n / 2 { 2 } else { 3 }).collect();
    let mut irregular = Vec::with_capacity(n);
    let mut rng = 20261113_u64;
    for _ in 0..n {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        irregular.push((rng & 0xFFFF) as i64);
    }

    let ref_eval = |arr: &[i64]| -> i64 {
        let mut acc = 0i64;
        for &v in arr {
            let picked = if (v & 1) != 0 { v + 7 } else { v - 3 };
            acc = acc.wrapping_add(picked);
        }
        acc
    };
    let ref_pred = ref_eval(&predictable);
    let ref_irreg = ref_eval(&irregular);

    let plan = CodegenPlan::baseline_o2();
    let sess_br =
        OrcJitSession::compile_named_with_plan(probe_branch_kernel_ir(true), &plan, None)?;
    let sess_sel =
        OrcJitSession::compile_named_with_plan(probe_branch_kernel_ir(false), &plan, None)?;
    let f_br: unsafe extern "C" fn(*const i64, i64) -> i64 = unsafe {
        std::mem::transmute(sess_br.lookup_symbol_addr("probe_branch_kernel")? as usize)
    };
    let f_sel: unsafe extern "C" fn(*const i64, i64) -> i64 = unsafe {
        std::mem::transmute(sess_sel.lookup_symbol_addr("probe_branch_kernel")? as usize)
    };

    // Warmup + measure (3 runs, take median for stability)
    let measure = |f: unsafe extern "C" fn(*const i64, i64) -> i64, data: &[i64], expected: i64| -> LlvmResult<u64> {
        let _ = unsafe { f(data.as_ptr(), data.len() as i64) };
        let mut samples = [0u64; 3];
        for slot in &mut samples {
            let (got, ns, _) = time_closure(|| unsafe { f(data.as_ptr(), data.len() as i64) });
            if got != expected {
                return Err(LlvmError::new("probe_branch_predictor verification mismatch"));
            }
            *slot = ns;
        }
        samples.sort_unstable();
        Ok(samples[1])
    };

    let predictable_branch_ns = measure(f_br, &predictable, ref_pred)?;
    let predictable_select_ns = measure(f_sel, &predictable, ref_pred)?;
    let irregular_branch_ns = measure(f_br, &irregular, ref_irreg)?;
    let irregular_select_ns = measure(f_sel, &irregular, ref_irreg)?;

    Ok(BranchPredictorReport {
        predictable_branch_ns,
        predictable_select_ns,
        irregular_branch_ns,
        irregular_select_ns,
        prefer_branchless_irregular: irregular_select_ns < irregular_branch_ns,
        verified: true,
    })
}

fn run_probe_unroll_layout() -> LlvmResult<UnrollLayoutReport> {
    let make_sum_ir = |unroll_md: &str| -> String {
        format!(
            r#"
define i64 @probe_sum(ptr readonly %arr, i64 %len) {{
entry:
  %z = icmp sle i64 %len, 0
  br i1 %z, label %done, label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i_next, %loop ]
  %acc = phi i64 [ 0, %entry ], [ %acc_next, %loop ]
  %p = getelementptr inbounds i64, ptr %arr, i64 %i
  %v = load i64, ptr %p
  %t = mul i64 %v, 3
  %acc_next = add i64 %acc, %t
  %i_next = add nsw i64 %i, 1
  %more = icmp slt i64 %i_next, %len
  br i1 %more, label %loop, label %done, !llvm.loop !0
done:
  %res = phi i64 [ 0, %entry ], [ %acc_next, %loop ]
  ret i64 %res
}}
!0 = distinct !{{!0, !1}}
!1 = {unroll_md}
"#
        )
    };

    let ir_no_unroll = make_sum_ir("!{!\"llvm.loop.unroll.disable\"}");
    let ir_unroll4 = make_sum_ir("!{!\"llvm.loop.unroll.count\", i32 4}");
    let ir_layout = r#"
define i64 @probe_aos(ptr readonly %arr, i64 %records) {
entry:
  %z = icmp sle i64 %records, 0
  br i1 %z, label %done, label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i_next, %loop ]
  %acc = phi i64 [ 0, %entry ], [ %acc_next, %loop ]
  %off = shl i64 %i, 2
  %p = getelementptr inbounds i64, ptr %arr, i64 %off
  %v = load i64, ptr %p
  %acc_next = add i64 %acc, %v
  %i_next = add nsw i64 %i, 1
  %more = icmp slt i64 %i_next, %records
  br i1 %more, label %loop, label %done
done:
  %res = phi i64 [ 0, %entry ], [ %acc_next, %loop ]
  ret i64 %res
}

define i64 @probe_soa(ptr readonly %arr, i64 %records) {
entry:
  %z = icmp sle i64 %records, 0
  br i1 %z, label %done, label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i_next, %loop ]
  %acc = phi i64 [ 0, %entry ], [ %acc_next, %loop ]
  %p = getelementptr inbounds i64, ptr %arr, i64 %i
  %v = load i64, ptr %p
  %acc_next = add i64 %acc, %v
  %i_next = add nsw i64 %i, 1
  %more = icmp slt i64 %i_next, %records
  br i1 %more, label %loop, label %done
done:
  %res = phi i64 [ 0, %entry ], [ %acc_next, %loop ]
  ret i64 %res
}
"#;

    let plan_gen_no = CodegenPlan {
        opt_level: OptLevel::O2,
        branchless_select: false,
        unroll_hint: UnrollHint::Disable,
        target_cpu_native: false,
        inline_ratio_fastpath: true,
        ..CodegenPlan::baseline_o2()
    };
    let plan_gen_u4 = CodegenPlan {
        opt_level: OptLevel::O2,
        branchless_select: false,
        unroll_hint: UnrollHint::Unroll4,
        target_cpu_native: false,
        inline_ratio_fastpath: true,
        ..CodegenPlan::baseline_o2()
    };
    let plan_nat_u4 = CodegenPlan {
        opt_level: OptLevel::O2,
        branchless_select: false,
        unroll_hint: UnrollHint::Unroll4,
        target_cpu_native: true,
        inline_ratio_fastpath: true,
        ..CodegenPlan::baseline_o2()
    };

    let s_sc = OrcJitSession::compile_named_with_plan(&ir_no_unroll, &plan_gen_no, None)?;
    let s_u4g = OrcJitSession::compile_named_with_plan(&ir_unroll4, &plan_gen_u4, None)?;
    let s_u4n = OrcJitSession::compile_named_with_plan(&ir_unroll4, &plan_nat_u4, None)?;
    let s_lay = OrcJitSession::compile_named_with_plan(ir_layout, &plan_nat_u4, None)?;

    let f_sc: unsafe extern "C" fn(*const i64, i64) -> i64 =
        unsafe { std::mem::transmute(s_sc.lookup_symbol_addr("probe_sum")? as usize) };
    let f_u4g: unsafe extern "C" fn(*const i64, i64) -> i64 =
        unsafe { std::mem::transmute(s_u4g.lookup_symbol_addr("probe_sum")? as usize) };
    let f_u4n: unsafe extern "C" fn(*const i64, i64) -> i64 =
        unsafe { std::mem::transmute(s_u4n.lookup_symbol_addr("probe_sum")? as usize) };
    let f_aos: unsafe extern "C" fn(*const i64, i64) -> i64 =
        unsafe { std::mem::transmute(s_lay.lookup_symbol_addr("probe_aos")? as usize) };
    let f_soa: unsafe extern "C" fn(*const i64, i64) -> i64 =
        unsafe { std::mem::transmute(s_lay.lookup_symbol_addr("probe_soa")? as usize) };

    let n = 32_768usize;
    let soa: Vec<i64> = (0..n).map(|i| ((i * 7 + 3) % 97) as i64).collect();
    let mut aos = vec![0i64; n * 4];
    for (i, &v) in soa.iter().enumerate() {
        aos[i * 4] = v;
    }
    let ref_sum3: i64 = soa.iter().map(|&v| v * 3).sum();
    let ref_sum1: i64 = soa.iter().sum();

    let run_med = |f: unsafe extern "C" fn(*const i64, i64) -> i64, buf: &[i64], count: usize, exp: i64| -> LlvmResult<u64> {
        let _ = unsafe { f(buf.as_ptr(), count as i64) };
        let mut s = [0u64; 3];
        for slot in &mut s {
            let (got, ns, _) = time_closure(|| unsafe { f(buf.as_ptr(), count as i64) });
            if got != exp {
                return Err(LlvmError::new("probe_unroll_layout verification mismatch"));
            }
            *slot = ns;
        }
        s.sort_unstable();
        Ok(s[1])
    };

    let scalar_generic_ns = run_med(f_sc, &soa, n, ref_sum3)?;
    let unroll4_generic_ns = run_med(f_u4g, &soa, n, ref_sum3)?;
    let unroll4_native_cpu_ns = run_med(f_u4n, &soa, n, ref_sum3)?;
    let aos_ns = run_med(f_aos, &aos, n, ref_sum1)?;
    let soa_ns = run_med(f_soa, &soa, n, ref_sum1)?;

    let recommended_unroll = if unroll4_generic_ns < scalar_generic_ns {
        UnrollHint::Unroll4
    } else {
        UnrollHint::Default
    };
    let prefer_native_cpu = unroll4_native_cpu_ns <= unroll4_generic_ns;
    let prefer_soa = soa_ns < aos_ns;

    Ok(UnrollLayoutReport {
        scalar_generic_ns,
        unroll4_generic_ns,
        unroll4_native_cpu_ns,
        aos_ns,
        soa_ns,
        recommended_unroll,
        prefer_native_cpu,
        prefer_soa,
        verified: true,
    })
}

/// Probe 5 (`P0-2`): Pure read-only worker `extern "C" fn(*const i64, usize) -> i64` executed
/// inside `std::thread::scope` while `OrcJitSession` remains alive strictly past scope join.
fn run_probe_multicore(logical_cores: usize) -> LlvmResult<MulticoreReport> {
    let ir = r#"
define i64 @probe_worker_sum(ptr readonly %arr, i64 %len) {
entry:
  %z = icmp sle i64 %len, 0
  br i1 %z, label %done, label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i_next, %loop ]
  %acc = phi i64 [ 0, %entry ], [ %acc_next, %loop ]
  %p = getelementptr inbounds i64, ptr %arr, i64 %i
  %v = load i64, ptr %p
  %t1 = mul i64 %v, %v
  %t2 = xor i64 %t1, %v
  %acc_next = add i64 %acc, %t2
  %i_next = add nsw i64 %i, 1
  %more = icmp slt i64 %i_next, %len
  br i1 %more, label %loop, label %done
done:
  %res = phi i64 [ 0, %entry ], [ %acc_next, %loop ]
  ret i64 %res
}
"#;
    let sess = OrcJitSession::compile_named_with_plan(ir, &CodegenPlan::baseline_o2(), None)?;
    let addr = sess.lookup_symbol_addr("probe_worker_sum")?;
    let worker: unsafe extern "C" fn(*const i64, usize) -> i64 =
        unsafe { std::mem::transmute(addr as usize) };

    let n = 2_097_152usize;
    let data: Vec<i64> = (0..n).map(|i| ((i * 13 + 5) & 1023) as i64).collect();
    let ref_sum: i64 = data
        .iter()
        .fold(0i64, |acc, &v| acc.wrapping_add((v * v) ^ v));

    // Warmup + single-thread timing
    let _ = unsafe { worker(data.as_ptr(), n) };
    let (single_got, single_thread_ns, _) = time_closure(|| unsafe { worker(data.as_ptr(), n) });
    if single_got != ref_sum {
        return Err(LlvmError::new("probe_multicore single-thread verification mismatch"));
    }

    let tested_workers = logical_cores.clamp(1, 4);
    let (multi_got, multi_thread_ns, _) = time_closure(|| {
        let chunk = n.div_ceil(tested_workers);
        std::thread::scope(|s| {
            let mut handles = Vec::with_capacity(tested_workers);
            for w in 0..tested_workers {
                let start = w * chunk;
                let end = (start + chunk).min(n);
                let slice = &data[start..end];
                handles.push(s.spawn(move || unsafe { worker(slice.as_ptr(), slice.len()) }));
            }
            let mut total = 0i64;
            for h in handles {
                total = total.wrapping_add(h.join().unwrap_or(0));
            }
            total
        })
    });
    // `sess` is dropped strictly AFTER `std::thread::scope` has joined all workers (`P0-2`)!
    drop(sess);

    if multi_got != ref_sum {
        return Err(LlvmError::new("probe_multicore multi-thread verification mismatch"));
    }

    let scaling_x100 =
        ((single_thread_ns as u128).saturating_mul(100) / (multi_thread_ns.max(1) as u128)) as u64;
    let recommended_workers = if tested_workers > 1 && scaling_x100 >= 120 {
        tested_workers
    } else {
        1
    };

    Ok(MulticoreReport {
        tested_workers,
        single_thread_ns,
        multi_thread_ns,
        scaling_x100,
        recommended_workers,
        verified: true,
    })
}
