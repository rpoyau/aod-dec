use std::collections::BTreeMap;
use std::path::Path;

use aod_core::json::JsonValue;
use aod_core::sha256::Sha256;

use crate::ffi::host_llvm_target_info;
use crate::plan::POLICY_SCOPE;
use crate::LlvmResult;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheLevelInfo {
    pub index: u32,
    pub level: u32,
    pub cache_type: String,
    pub size_bytes: u64,
    pub line_bytes: u64,
}

impl CacheLevelInfo {
    pub fn to_json(&self) -> JsonValue {
        let mut m = BTreeMap::new();
        m.insert(
            "cache_type".to_string(),
            JsonValue::from_str_val(self.cache_type.clone()),
        );
        m.insert("index".to_string(), JsonValue::from_i64(self.index as i64));
        m.insert("level".to_string(), JsonValue::from_i64(self.level as i64));
        m.insert(
            "line_bytes".to_string(),
            JsonValue::from_i64(self.line_bytes as i64),
        );
        m.insert(
            "size_bytes".to_string(),
            JsonValue::from_i64(self.size_bytes as i64),
        );
        JsonValue::Object(m)
    }
}

/// 100% safe host CPU microarchitecture, ISA, cache, and perf policy discovery (`P0-1`, zero `unsafe`!).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostHardwareInfo {
    pub policy_scope: String,
    pub arch: String,
    pub os: String,
    pub triple: String,
    pub llvm_host_cpu: String,
    pub llvm_host_features: String,
    pub vendor_id: String,
    pub model_name: String,
    pub logical_cores: usize,
    pub isa_flags: BTreeMap<String, bool>,
    pub caches: Vec<CacheLevelInfo>,
    pub l1d_bytes: u64,
    pub l2_bytes: u64,
    pub l3_bytes: u64,
    pub cache_line_bytes: u64,
    pub perf_event_paranoid: Option<i32>,
    pub cpu_sig_sha256: String,
}

impl HostHardwareInfo {
    pub fn discover() -> LlvmResult<Self> {
        let (triple, llvm_host_cpu, llvm_host_features) = host_llvm_target_info()?;
        let logical_cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let (vendor_id, model_name) = parse_proc_cpuinfo();
        let isa_flags = detect_isa_flags();
        let caches = discover_sysfs_caches();

        let mut l1d_bytes = 0u64;
        let mut l2_bytes = 0u64;
        let mut l3_bytes = 0u64;
        let mut cache_line_bytes = 0u64;
        for c in &caches {
            if c.line_bytes > 0 && cache_line_bytes == 0 {
                cache_line_bytes = c.line_bytes;
            }
            if c.level == 1 && c.cache_type.eq_ignore_ascii_case("Data") && l1d_bytes == 0 {
                l1d_bytes = c.size_bytes;
            } else if c.level == 2 && l2_bytes == 0 {
                l2_bytes = c.size_bytes;
            } else if c.level == 3 && l3_bytes == 0 {
                l3_bytes = c.size_bytes;
            }
        }

        let perf_event_paranoid = std::fs::read_to_string("/proc/sys/kernel/perf_event_paranoid")
            .ok()
            .and_then(|s| s.trim().parse::<i32>().ok());

        let mut info = Self {
            policy_scope: POLICY_SCOPE.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            os: std::env::consts::OS.to_string(),
            triple,
            llvm_host_cpu,
            llvm_host_features,
            vendor_id,
            model_name,
            logical_cores,
            isa_flags,
            caches,
            l1d_bytes,
            l2_bytes,
            l3_bytes,
            cache_line_bytes,
            perf_event_paranoid,
            cpu_sig_sha256: String::new(),
        };
        let sig_json = info.signature_json().to_compact_ascii_json();
        info.cpu_sig_sha256 = Sha256::digest_hex(sig_json.as_bytes());
        Ok(info)
    }

    fn signature_json(&self) -> JsonValue {
        let mut m = BTreeMap::new();
        m.insert("arch".to_string(), JsonValue::from_str_val(self.arch.clone()));
        m.insert(
            "cache_line_bytes".to_string(),
            JsonValue::from_i64(self.cache_line_bytes as i64),
        );
        let mut isa_map = BTreeMap::new();
        for (k, &v) in &self.isa_flags {
            isa_map.insert(k.clone(), JsonValue::Bool(v));
        }
        m.insert("isa_flags".to_string(), JsonValue::Object(isa_map));
        m.insert(
            "l1d_bytes".to_string(),
            JsonValue::from_i64(self.l1d_bytes as i64),
        );
        m.insert(
            "l2_bytes".to_string(),
            JsonValue::from_i64(self.l2_bytes as i64),
        );
        m.insert(
            "l3_bytes".to_string(),
            JsonValue::from_i64(self.l3_bytes as i64),
        );
        m.insert(
            "llvm_host_cpu".to_string(),
            JsonValue::from_str_val(self.llvm_host_cpu.clone()),
        );
        m.insert(
            "logical_cores".to_string(),
            JsonValue::from_usize(self.logical_cores),
        );
        m.insert(
            "model_name".to_string(),
            JsonValue::from_str_val(self.model_name.clone()),
        );
        m.insert("os".to_string(), JsonValue::from_str_val(self.os.clone()));
        m.insert(
            "triple".to_string(),
            JsonValue::from_str_val(self.triple.clone()),
        );
        m.insert(
            "vendor_id".to_string(),
            JsonValue::from_str_val(self.vendor_id.clone()),
        );
        JsonValue::Object(m)
    }

    pub fn to_json(&self) -> JsonValue {
        let mut m = BTreeMap::new();
        m.insert("arch".to_string(), JsonValue::from_str_val(self.arch.clone()));
        m.insert(
            "cache_line_bytes".to_string(),
            JsonValue::from_i64(self.cache_line_bytes as i64),
        );
        m.insert(
            "caches".to_string(),
            JsonValue::Array(self.caches.iter().map(CacheLevelInfo::to_json).collect()),
        );
        m.insert(
            "cpu_sig_sha256".to_string(),
            JsonValue::from_str_val(self.cpu_sig_sha256.clone()),
        );
        let mut isa_map = BTreeMap::new();
        for (k, &v) in &self.isa_flags {
            isa_map.insert(k.clone(), JsonValue::Bool(v));
        }
        m.insert("isa_flags".to_string(), JsonValue::Object(isa_map));
        m.insert(
            "l1d_bytes".to_string(),
            JsonValue::from_i64(self.l1d_bytes as i64),
        );
        m.insert(
            "l2_bytes".to_string(),
            JsonValue::from_i64(self.l2_bytes as i64),
        );
        m.insert(
            "l3_bytes".to_string(),
            JsonValue::from_i64(self.l3_bytes as i64),
        );
        m.insert(
            "llvm_host_cpu".to_string(),
            JsonValue::from_str_val(self.llvm_host_cpu.clone()),
        );
        m.insert(
            "llvm_host_features".to_string(),
            JsonValue::from_str_val(self.llvm_host_features.clone()),
        );
        m.insert(
            "logical_cores".to_string(),
            JsonValue::from_usize(self.logical_cores),
        );
        m.insert(
            "model_name".to_string(),
            JsonValue::from_str_val(self.model_name.clone()),
        );
        m.insert("os".to_string(), JsonValue::from_str_val(self.os.clone()));
        m.insert(
            "perf_event_paranoid".to_string(),
            match self.perf_event_paranoid {
                Some(p) => JsonValue::from_i64(p as i64),
                None => JsonValue::Null,
            },
        );
        m.insert(
            "policy_scope".to_string(),
            JsonValue::from_str_val(self.policy_scope.clone()),
        );
        m.insert(
            "triple".to_string(),
            JsonValue::from_str_val(self.triple.clone()),
        );
        m.insert(
            "vendor_id".to_string(),
            JsonValue::from_str_val(self.vendor_id.clone()),
        );
        JsonValue::Object(m)
    }
}

fn detect_isa_flags() -> BTreeMap<String, bool> {
    let mut flags = BTreeMap::new();
    #[cfg(target_arch = "x86_64")]
    {
        flags.insert("sse4.2".to_string(), std::arch::is_x86_feature_detected!("sse4.2"));
        flags.insert("avx".to_string(), std::arch::is_x86_feature_detected!("avx"));
        flags.insert("avx2".to_string(), std::arch::is_x86_feature_detected!("avx2"));
        flags.insert("fma".to_string(), std::arch::is_x86_feature_detected!("fma"));
        flags.insert("bmi1".to_string(), std::arch::is_x86_feature_detected!("bmi1"));
        flags.insert("bmi2".to_string(), std::arch::is_x86_feature_detected!("bmi2"));
        flags.insert("popcnt".to_string(), std::arch::is_x86_feature_detected!("popcnt"));
        flags.insert("rdtscp".to_string(), crate::hw_probe::has_rdtscp());
        flags.insert("avx512f".to_string(), std::arch::is_x86_feature_detected!("avx512f"));
    }
    flags
}

fn parse_proc_cpuinfo() -> (String, String) {
    let Ok(text) = std::fs::read_to_string("/proc/cpuinfo") else {
        return ("unknown".to_string(), "unknown".to_string());
    };
    let mut vendor = String::new();
    let mut model = String::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim();
            let val = v.trim();
            if key == "vendor_id" && vendor.is_empty() {
                vendor = val.to_string();
            } else if key == "model name" && model.is_empty() {
                model = val.to_string();
            }
            if !vendor.is_empty() && !model.is_empty() {
                break;
            }
        }
    }
    if vendor.is_empty() {
        vendor = "unknown".to_string();
    }
    if model.is_empty() {
        model = "unknown".to_string();
    }
    (vendor, model)
}

fn parse_size_bytes(raw: &str) -> u64 {
    let s = raw.trim();
    if let Some(num) = s.strip_suffix('K').or_else(|| s.strip_suffix('k')) {
        num.trim().parse::<u64>().unwrap_or(0).saturating_mul(1024)
    } else if let Some(num) = s.strip_suffix('M').or_else(|| s.strip_suffix('m')) {
        num.trim()
            .parse::<u64>()
            .unwrap_or(0)
            .saturating_mul(1024 * 1024)
    } else if let Some(num) = s.strip_suffix('G').or_else(|| s.strip_suffix('g')) {
        num.trim()
            .parse::<u64>()
            .unwrap_or(0)
            .saturating_mul(1024 * 1024 * 1024)
    } else {
        s.parse::<u64>().unwrap_or(0)
    }
}

fn discover_sysfs_caches() -> Vec<CacheLevelInfo> {
    let root = Path::new("/sys/devices/system/cpu/cpu0/cache");
    let mut out = Vec::new();
    for idx in 0u32..16 {
        let dir = root.join(format!("index{idx}"));
        if !dir.is_dir() {
            continue;
        }
        let level = std::fs::read_to_string(dir.join("level"))
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0);
        let cache_type = std::fs::read_to_string(dir.join("type"))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "Unknown".to_string());
        let size_bytes = std::fs::read_to_string(dir.join("size"))
            .map(|s| parse_size_bytes(&s))
            .unwrap_or(0);
        let line_bytes = std::fs::read_to_string(dir.join("coherency_line_size"))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);
        out.push(CacheLevelInfo {
            index: idx,
            level,
            cache_type,
            size_bytes,
            line_bytes,
        });
    }
    out
}
