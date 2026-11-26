use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use aod_core::bigint::BigInt;
use aod_core::ledger::{probability, SadarPromotion};
use aod_core::qext::{parse_decimal_exact, QExtRatio};
use aod_core::ratio::Ratio;
use aod_core::sha256::Sha256;
use aod_core::support::SupportForm;
use aod_core::ternary::{Addr8, B64, B8, Q4, T1};
use aod_core::trit_int::{
    SUBTAG_MASK, TAG_ABSENT_PREFIX, TAG_NULL_SENTINEL, TAG_PAYLOAD_MASK, TAG_SPILL_PREFIX,
    TOP_TAG_MASK, TritInt,
};
use aod_core::wavelet::{
    declare_synthetic_lineage_registry, deduplicated_return_count, native_cadence_ratio,
    AdmittedClosureRecord, M3ClosureTrace, RecipientContext, RecipientLattice, RecipientStage,
    ShedRoute, ShedWitness, SingleContactStageAssociation, StageEventWitness, Wavelet, WaveletRole,
    WaveletSpec,
};
use aod_core::witness::{Coupling, Fusion, SeatState, Trace};

use crate::arena::{RtArena, DEFAULT_MAX_ARENA_ENTRIES};
use crate::sim::{
    AxisDir, BlindMotifSpectrum, BoxRegion, CarrierState, ChamberDynamicsConfig, ChamberState,
    DeclarativeObserver, ProbeKind, ReferenceClock, SensorCurrentSummary, SimRunResult, WallPolicy,
};
use crate::value::{
    model_status_variant_and_tag, recipient_stage_name, recipient_stage_variant_and_tag,
    RtEnumValue, RtMapKey, RtStructValue, RtValue,
};

/// Maximum file size permitted across capability-gated `aod::sys` bootstrap I/O (`64 MiB`).
pub const MAX_SYS_IO_BYTES: usize = 64 * 1024 * 1024;

/// Pending simulation configuration built incrementally before `aod_rt_sim_run`.
#[derive(Clone, Debug)]
pub struct SimBuilder {
    pub chamber_name: String,
    pub space: BoxRegion,
    pub walls: WallPolicy,
    pub dynamics: ChamberDynamicsConfig,
    pub clocks: Vec<ReferenceClock>,
    pub carriers: Vec<CarrierState>,
    pub decl_observers: Vec<DeclarativeObserver>,
    /// Procedural observer callbacks stored as opaque function-pointer addresses (`usize`).
    pub proc_observers: Vec<(String, usize)>,
}

/// Owned Rust state behind `AodRtContext.inner`.
#[derive(Debug)]
pub struct RtState {
    pub arena: RtArena,
    pub seq_bufs: Vec<Vec<RtValue>>,
    pub sim_builders: Vec<SimBuilder>,
    pub emits: Vec<String>,
    pub cli_args: Vec<Vec<u8>>,
    pub rt_ns: crate::ns_fs::RtNamespace,
    pub dyn_vtable: Vec<usize>,
    pub error: Option<String>,
    pub capture_stdout: bool,
}

impl RtState {
    pub fn new(capture_stdout: bool, max_arena_entries: usize) -> Self {
        Self {
            arena: RtArena::new(max_arena_entries),
            seq_bufs: Vec::new(),
            sim_builders: Vec::new(),
            emits: Vec::new(),
            cli_args: Vec::new(),
            rt_ns: crate::ns_fs::RtNamespace::empty(),
            dyn_vtable: Vec::new(),
            error: None,
            capture_stdout,
        }
    }

    pub fn with_default_arena(capture_stdout: bool) -> Self {
        Self::new(capture_stdout, DEFAULT_MAX_ARENA_ENTRIES)
    }

    pub fn record_error(&mut self, msg: String) {
        if self.error.is_none() {
            self.error = Some(msg);
        }
    }

    pub fn emit_line(&mut self, line: String) {
        if self.capture_stdout {
            self.emits.push(line);
        } else {
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{line}");
            let _ = stdout.flush();
        }
    }

    /// Decode a 1-GPR Trit-Boxed `DecInt` word (`w < 0xC000...` inline 32-trit, `0xD000...` arena spill).
    pub fn unpack_dec_int(&self, word: u64) -> Result<BigInt, String> {
        if word < TOP_TAG_MASK {
            TritInt::from_inline_word(word).map_err(|e| e.to_string())
        } else {
            match word & SUBTAG_MASK {
                TAG_SPILL_PREFIX => {
                    let handle = word & TAG_PAYLOAD_MASK;
                    match self.arena.get(handle)? {
                        RtValue::Int(n) => Ok(n.clone()),
                        other => Err(format!("expected spilled int handle, got {}", other.render())),
                    }
                }
                w if word == TAG_NULL_SENTINEL || w == TAG_NULL_SENTINEL => {
                    Err("unexpected Null sentinel in int register".to_string())
                }
                TAG_ABSENT_PREFIX => {
                    Err("unexpected structural absence S_k^{+,-e} sentinel in int register".to_string())
                }
                _ => Err(format!("trapped or invalid 0b11 DecInt word 0x{word:016x}")),
            }
        }
    }

    /// Encode an exact `BigInt` (`TritInt`) into a 1-GPR Trit-Boxed `u64` word.
    pub fn pack_dec_int(&mut self, n: BigInt) -> Result<u64, String> {
        if let Some(w) = n.inline_word() {
            Ok(w)
        } else {
            let h = self.arena.insert(RtValue::Int(n))?;
            if h > TAG_PAYLOAD_MASK {
                return Err("arena handle exceeded 60-bit 0b11 tag payload".to_string());
            }
            Ok(TAG_SPILL_PREFIX | h)
        }
    }

    /// Decode 2-GPR `DecRatio` words `(num, den)` (`(num | den) < 0xC000...` inline, `0xD000...` spill).
    pub fn unpack_dec_ratio(&self, num: u64, den: u64) -> Result<Ratio, String> {
        if num < TOP_TAG_MASK && den < TOP_TAG_MASK && den != 0 {
            Ratio::from_inline_words_reduced(num, den).map_err(|e| e.to_string())
        } else if (num & SUBTAG_MASK) == TAG_SPILL_PREFIX && den == 0 {
            let handle = num & TAG_PAYLOAD_MASK;
            match self.arena.get(handle)? {
                RtValue::Ratio(r) => Ok(r.clone()),
                other => Err(format!("expected spilled ratio handle, got {}", other.render())),
            }
        } else {
            Err(format!(
                "invalid DecRatio words (num=0x{num:016x}, den=0x{den:016x})"
            ))
        }
    }

    /// Encode a canonical `Ratio` into 2-GPR `(num, den)` words.
    pub fn pack_dec_ratio(&mut self, r: Ratio) -> Result<(u64, u64), String> {
        if let Some((nw, dw)) = r.inline_words() {
            Ok((nw, dw))
        } else {
            let h = self.arena.insert(RtValue::Ratio(r))?;
            if h > TAG_PAYLOAD_MASK {
                return Err("arena handle exceeded 60-bit 0b11 tag payload".to_string());
            }
            Ok((TAG_SPILL_PREFIX | h, 0))
        }
    }

    pub fn get_str(&self, handle: u64) -> Result<&str, String> {
        match self.arena.get(handle)? {
            RtValue::Str(s) => Ok(s.as_str()),
            other => Err(format!("expected str handle, got {}", other.render())),
        }
    }

    pub fn get_bytes(&self, handle: u64) -> Result<&[u8], String> {
        match self.arena.get(handle)? {
            RtValue::Bytes(bs) => Ok(bs.as_slice()),
            other => Err(format!("expected bytes handle, got {}", other.render())),
        }
    }

    pub fn get_str_or_utf8_bytes(&self, handle: u64, ctx_name: &str) -> Result<String, String> {
        match self.arena.get(handle)? {
            RtValue::Str(s) => Ok(s.clone()),
            RtValue::Bytes(bs) => std::str::from_utf8(bs)
                .map(str::to_string)
                .map_err(|_| format!("{ctx_name}: byte sequence is not valid UTF-8")),
            other => Err(format!(
                "expected bytes or str handle in {ctx_name}, got {}",
                other.render()
            )),
        }
    }

    pub fn get_seq_slice(&self, handle: u64) -> Result<&[RtValue], String> {
        match self.arena.get(handle)? {
            RtValue::Array(xs) | RtValue::List(xs) => Ok(xs.as_slice()),
            other => Err(format!(
                "expected list or array handle, got {}",
                other.render()
            )),
        }
    }

    pub fn get_any_seq_values(&self, handle: u64) -> Result<Vec<RtValue>, String> {
        match self.arena.get(handle)? {
            RtValue::Array(xs) | RtValue::List(xs) => Ok((**xs).clone()),
            RtValue::Bytes(bs) => Ok(bs.iter().map(|&b| RtValue::B8(B8(b))).collect()),
            other => Err(format!(
                "expected list, array, or bytes handle, got {}",
                other.render()
            )),
        }
    }

    pub fn op_zip_seq(&mut self, a_h: u64, b_h: u64) -> Result<u64, String> {
        let xs = self.get_any_seq_values(a_h)?;
        let ys = self.get_any_seq_values(b_h)?;
        if xs.len() != ys.len() {
            return Err(format!(
                "zip_seq(a, b) sequence length mismatch: {} != {}",
                xs.len(),
                ys.len()
            ));
        }
        let zipped: Vec<RtValue> = xs
            .into_iter()
            .zip(ys)
            .map(|(a, b)| RtValue::Tuple(vec![a, b]))
            .collect();
        self.arena
            .insert(RtValue::List(std::sync::Arc::new(zipped)))
    }

    pub fn get_support(&self, handle: u64) -> Result<&SupportForm, String> {
        match self.arena.get(handle)? {
            RtValue::Support(s) => Ok(s),
            other => Err(format!("expected support handle, got {}", other.render())),
        }
    }

    pub fn get_trace(&self, handle: u64) -> Result<&Trace, String> {
        match self.arena.get(handle)? {
            RtValue::Trace(t) => Ok(t),
            other => Err(format!("expected trace handle, got {}", other.render())),
        }
    }

    pub fn get_fusion(&self, handle: u64) -> Result<&Fusion, String> {
        match self.arena.get(handle)? {
            RtValue::Fusion(f) => Ok(f),
            other => Err(format!("expected fusion handle, got {}", other.render())),
        }
    }

    pub fn get_coupling(&self, handle: u64) -> Result<&Coupling, String> {
        match self.arena.get(handle)? {
            RtValue::Coupling(c) => Ok(c),
            other => Err(format!("expected coupling handle, got {}", other.render())),
        }
    }

    pub fn get_qext_ratio(&self, handle: u64) -> Result<&QExtRatio, String> {
        match self.arena.get(handle)? {
            RtValue::QExtRatio(q) => Ok(q),
            other => Err(format!("expected QExt<ratio> handle, got {}", other.render())),
        }
    }

    pub fn get_box(&self, handle: u64) -> Result<BoxRegion, String> {
        match self.arena.get(handle)? {
            RtValue::Box3(b) => Ok(*b),
            other => Err(format!("expected box handle, got {}", other.render())),
        }
    }

    pub fn get_window(&self, handle: u64) -> Result<(i64, i64), String> {
        match self.arena.get(handle)? {
            RtValue::Window(u, v) => Ok((*u, *v)),
            other => Err(format!("expected window handle, got {}", other.render())),
        }
    }

    // --- Sequence, Array, Bytes, & Map operations ---

    pub fn op_seq_get(&self, seq_h: u64, idx: &BigInt) -> Result<RtValue, String> {
        let i = idx
            .to_i64()
            .ok_or_else(|| format!("index {idx} out of i64 range"))?;
        match self.arena.get(seq_h)? {
            RtValue::Array(items) | RtValue::List(items) => {
                if i < 0 || (i as usize) >= items.len() {
                    return Err(format!("index {i} out of bounds"));
                }
                Ok(items[i as usize].clone())
            }
            RtValue::Bytes(bs) => {
                if i < 0 || (i as usize) >= bs.len() {
                    return Err(format!("bytes index {i} out of bounds"));
                }
                Ok(RtValue::B8(B8(bs[i as usize])))
            }
            other => Err(format!("indexing unsupported on {}", other.render())),
        }
    }

    pub fn op_array_set(&mut self, arr_h: u64, idx: &BigInt, val_h: u64) -> Result<u64, String> {
        let i = idx
            .to_i64()
            .ok_or_else(|| format!("index {idx} out of i64 range"))?;
        let new_val = self.arena.get(val_h)?.clone();
        match self.arena.get(arr_h)? {
            RtValue::Array(items) => {
                if i < 0 || (i as usize) >= items.len() {
                    return Err(format!("array index {i} out of bounds"));
                }
                let mut cloned = (**items).clone();
                cloned[i as usize] = new_val;
                self.arena.insert(RtValue::Array(std::sync::Arc::new(cloned)))
            }
            RtValue::List(items) => {
                if i < 0 || (i as usize) >= items.len() {
                    return Err(format!("list index {i} out of bounds"));
                }
                let mut cloned = (**items).clone();
                cloned[i as usize] = new_val;
                self.arena.insert(RtValue::List(std::sync::Arc::new(cloned)))
            }
            other => Err(format!("expected array or list handle, got {}", other.render())),
        }
    }

    pub fn op_bytes_slice(&mut self, b_h: u64, start: &BigInt, end: &BigInt) -> Result<u64, String> {
        let bs = self.get_bytes(b_h)?;
        let s = start
            .to_i64()
            .ok_or_else(|| format!("slice start {start} out of range"))?;
        let e = end
            .to_i64()
            .ok_or_else(|| format!("slice end {end} out of range"))?;
        if s < 0 || e < s || (e as usize) > bs.len() {
            return Err(format!(
                "slice_bytes({s}, {e}) out of bounds for length {}",
                bs.len()
            ));
        }
        let sliced = bs[s as usize..e as usize].to_vec();
        self.arena.insert(RtValue::Bytes(std::sync::Arc::new(sliced)))
    }

    pub fn op_bytes_concat(&mut self, a_h: u64, b_h: u64) -> Result<u64, String> {
        let a = self.get_bytes(a_h)?;
        let b = self.get_bytes(b_h)?;
        let mut out = Vec::with_capacity(a.len() + b.len());
        out.extend_from_slice(a);
        out.extend_from_slice(b);
        self.arena.insert(RtValue::Bytes(std::sync::Arc::new(out)))
    }

    pub fn op_bytes_sha256(&mut self, b_h: u64) -> Result<u64, String> {
        let bs = self.get_bytes(b_h)?;
        let digest = Sha256::digest(bs);
        self.arena
            .insert(RtValue::Bytes(std::sync::Arc::new(digest.to_vec())))
    }

    pub fn op_bytes_hex(&mut self, b_h: u64) -> Result<u64, String> {
        let bs = self.get_bytes(b_h)?;
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = Vec::with_capacity(bs.len() * 2);
        for &byte in bs {
            out.push(HEX[(byte >> 4) as usize]);
            out.push(HEX[(byte & 0x0F) as usize]);
        }
        self.arena.insert(RtValue::Bytes(std::sync::Arc::new(out)))
    }

    pub fn op_sys_cli_args(&mut self) -> Result<u64, String> {
        let items: Vec<RtValue> = self
            .cli_args
            .iter()
            .cloned()
            .map(|b| RtValue::Bytes(std::sync::Arc::new(b)))
            .collect();
        self.arena.insert(RtValue::List(std::sync::Arc::new(items)))
    }

    pub fn op_sys_read_file_bytes(&mut self, path_h: u64) -> Result<u64, String> {
        let raw_path = self.get_bytes(path_h)?.to_vec();
        let data = self.rt_ns.read_file_bytes(&raw_path)?;
        self.arena.insert(RtValue::Bytes(std::sync::Arc::new(data)))
    }

    pub fn op_sys_write_executable_elf(&mut self, path_h: u64, img_h: u64) -> Result<BigInt, String> {
        let path_bytes = self.get_bytes(path_h)?.to_vec();
        let img = self.get_bytes(img_h)?.to_vec();
        let written = self.rt_ns.write_executable_elf(&path_bytes, &img)?;
        Ok(BigInt::from_u64(written as u64))
    }

    pub fn op_sys_accept_stream(&mut self, path_h: u64) -> Result<bool, String> {
        let raw_path = self.get_bytes(path_h)?.to_vec();
        self.rt_ns.accept_stream(&raw_path)
    }

    pub fn op_sys_stream_local_addr(&mut self, path_h: u64) -> Result<u64, String> {
        let raw_path = self.get_bytes(path_h)?.to_vec();
        let addr = self.rt_ns.stream_local_addr(&raw_path)?;
        self.arena.insert(RtValue::Bytes(std::sync::Arc::new(addr)))
    }

    pub fn op_sys_read_stream_line(
        &mut self,
        path_h: u64,
        max_bytes: &BigInt,
    ) -> Result<u64, String> {
        let max_i = max_bytes
            .to_i64()
            .ok_or_else(|| "read_stream_line max_bytes out of i64 range".to_string())?;
        if max_i <= 0 {
            return Err(format!(
                "read_stream_line max_bytes must be >= 1, got {max_bytes}"
            ));
        }
        let raw_path = self.get_bytes(path_h)?.to_vec();
        let (status, bytes) = self.rt_ns.read_stream_line(&raw_path, max_i as usize)?;
        self.arena.insert(RtValue::Tuple(vec![
            RtValue::T1(status),
            RtValue::Bytes(std::sync::Arc::new(bytes)),
        ]))
    }

    pub fn op_sys_read_stream_frame(
        &mut self,
        path_h: u64,
        max_bytes: &BigInt,
    ) -> Result<u64, String> {
        let max_i = max_bytes
            .to_i64()
            .ok_or_else(|| "read_stream_frame max_bytes out of i64 range".to_string())?;
        if max_i < 0 {
            return Err(format!(
                "read_stream_frame max_bytes must be >= 0, got {max_bytes}"
            ));
        }
        let raw_path = self.get_bytes(path_h)?.to_vec();
        let (status, bytes) = self.rt_ns.read_stream_frame(&raw_path, max_i as usize)?;
        self.arena.insert(RtValue::Tuple(vec![
            RtValue::T1(status),
            RtValue::Bytes(std::sync::Arc::new(bytes)),
        ]))
    }

    pub fn op_sys_write_stream_bytes(
        &mut self,
        path_h: u64,
        payload_h: u64,
    ) -> Result<BigInt, String> {
        let raw_path = self.get_bytes(path_h)?.to_vec();
        let payload = self.get_bytes(payload_h)?.to_vec();
        let written = self.rt_ns.write_stream_bytes(&raw_path, &payload)?;
        Ok(BigInt::from_i64(written))
    }

    pub fn op_sys_write_stream_frame(
        &mut self,
        path_h: u64,
        payload_h: u64,
    ) -> Result<BigInt, String> {
        let raw_path = self.get_bytes(path_h)?.to_vec();
        let payload = self.get_bytes(payload_h)?.to_vec();
        let written = self.rt_ns.write_stream_frame(&raw_path, &payload)?;
        Ok(BigInt::from_i64(written))
    }

    pub fn op_map_get(&self, map_h: u64, key_h: u64) -> Result<RtValue, String> {
        let kv = self.arena.get(key_h)?;
        let mk = RtMapKey::from_value(kv)?;
        match self.arena.get(map_h)? {
            RtValue::Map(m) => m
                .get(&mk)
                .cloned()
                .ok_or_else(|| format!("map key {} not found", kv.render())),
            other => Err(format!("expected map handle, got {}", other.render())),
        }
    }

    pub fn op_map_set(&mut self, map_h: u64, key_h: u64, val_h: u64) -> Result<u64, String> {
        let kv = self.arena.get(key_h)?.clone();
        let vv = self.arena.get(val_h)?.clone();
        let mk = RtMapKey::from_value(&kv)?;
        let mut cloned = match self.arena.get(map_h)? {
            RtValue::Map(m) => (**m).clone(),
            other => return Err(format!("expected map handle, got {}", other.render())),
        };
        cloned.insert(mk, vv);
        self.arena.insert(RtValue::Map(Box::new(cloned)))
    }

    pub fn op_map_contains(&self, map_h: u64, key_h: u64) -> Result<bool, String> {
        let kv = self.arena.get(key_h)?;
        let mk = RtMapKey::from_value(kv)?;
        match self.arena.get(map_h)? {
            RtValue::Map(m) => Ok(m.contains_key(&mk)),
            other => Err(format!("expected map handle, got {}", other.render())),
        }
    }

    pub fn op_map_remove(&mut self, map_h: u64, key_h: u64) -> Result<u64, String> {
        let kv = self.arena.get(key_h)?;
        let mk = RtMapKey::from_value(kv)?;
        let mut cloned = match self.arena.get(map_h)? {
            RtValue::Map(m) => (**m).clone(),
            other => return Err(format!("expected map handle, got {}", other.render())),
        };
        cloned.remove(&mk);
        self.arena.insert(RtValue::Map(Box::new(cloned)))
    }

    pub fn op_map_get_or(&mut self, map_h: u64, key_h: u64, default_h: u64) -> Result<u64, String> {
        let kv = self.arena.get(key_h)?;
        let mk = RtMapKey::from_value(kv)?;
        let found = match self.arena.get(map_h)? {
            RtValue::Map(m) => m.get(&mk).cloned(),
            other => return Err(format!("expected map handle, got {}", other.render())),
        };
        match found {
            Some(v) => self.arena.insert(v),
            None => {
                let dv = self.arena.get(default_h)?.clone();
                self.arena.insert(dv)
            }
        }
    }

    pub fn op_struct_set(&mut self, s_h: u64, field: &str, val_h: u64) -> Result<u64, String> {
        let new_val = self.arena.get(val_h)?.clone();
        let mut cloned = match self.arena.get(s_h)? {
            RtValue::Struct(sv) => (**sv).clone(),
            other => return Err(format!("expected struct handle, got {}", other.render())),
        };
        if !cloned.fields.contains_key(field) {
            return Err(format!("no field .{field} on struct {}", cloned.name));
        }
        cloned.fields.insert(field.to_string(), new_val);
        self.arena
            .insert(RtValue::Struct(std::sync::Arc::new(cloned)))
    }

    pub fn op_field_get(&mut self, h: u64, field: &str) -> Result<RtValue, String> {
        let val = self.arena.get(h)?.clone();
        match (val, field) {
            (RtValue::Struct(sv), f) => sv
                .fields
                .get(f)
                .cloned()
                .ok_or_else(|| format!("no field .{f} on struct {}", sv.name)),
            // Struct-field or Ratio-field array projection (`bins.t0` or `rs.num` across `[T; N]` / `[T]`):
            (RtValue::Array(items), f) => {
                let mut projected = Vec::with_capacity(items.len());
                for item in items.iter() {
                    match (item, f) {
                        (RtValue::Struct(sv), _) => {
                            let fv = sv
                                .fields
                                .get(f)
                                .cloned()
                                .ok_or_else(|| format!("no field .{f} on struct {}", sv.name))?;
                            projected.push(fv);
                        }
                        (RtValue::Ratio(r), "num") => projected.push(RtValue::Int(r.num().clone())),
                        (RtValue::Ratio(r), "den") => projected.push(RtValue::Int(r.den().clone())),
                        (other, _) => {
                            return Err(format!(
                                "cannot project field .{f} on array element {}",
                                other.render()
                            ));
                        }
                    }
                }
                Ok(RtValue::Array(std::sync::Arc::new(projected)))
            }
            (RtValue::List(items), f) => {
                let mut projected = Vec::with_capacity(items.len());
                for item in items.iter() {
                    match (item, f) {
                        (RtValue::Struct(sv), _) => {
                            let fv = sv
                                .fields
                                .get(f)
                                .cloned()
                                .ok_or_else(|| format!("no field .{f} on struct {}", sv.name))?;
                            projected.push(fv);
                        }
                        (RtValue::Ratio(r), "num") => projected.push(RtValue::Int(r.num().clone())),
                        (RtValue::Ratio(r), "den") => projected.push(RtValue::Int(r.den().clone())),
                        (other, _) => {
                            return Err(format!(
                                "cannot project field .{f} on slice element {}",
                                other.render()
                            ));
                        }
                    }
                }
                Ok(RtValue::List(std::sync::Arc::new(projected)))
            }
            (RtValue::SimResult(s), "model_status") => {
                let (variant, tag) = model_status_variant_and_tag(&s.model_status);
                Ok(RtValue::Enum(std::sync::Arc::new(RtEnumValue {
                    module: "<builtin>".to_string(),
                    name: "ModelStatus".to_string(),
                    variant: variant.to_string(),
                    tag,
                    payloads: Vec::new(),
                })))
            }
            (RtValue::SimResult(s), "model_status_bytes") => {
                Ok(RtValue::Bytes(std::sync::Arc::new(s.model_status.into_bytes())))
            }
            (RtValue::SimResult(s), "chamber_name" | "chamber_name_bytes") => {
                Ok(RtValue::Bytes(std::sync::Arc::new(s.chamber_name.into_bytes())))
            }
            (RtValue::SimResult(s), "final_state_hash" | "final_state_hash_bytes") => {
                Ok(RtValue::Bytes(std::sync::Arc::new(
                    s.final_state_hash.into_bytes(),
                )))
            }
            (RtValue::SimResult(s), "final_state_sha256") => Ok(RtValue::Bytes(
                std::sync::Arc::new(decode_hex_32(&s.final_state_hash)?),
            )),
            (RtValue::SimResult(s), "state_json" | "state_json_bytes") => {
                Ok(RtValue::Bytes(std::sync::Arc::new(s.state_json.into_bytes())))
            }
            (RtValue::SimResult(s), "shedding_witnesses_json" | "shedding_witnesses_json_bytes") => {
                Ok(RtValue::Bytes(std::sync::Arc::new(
                    s.shedding_witnesses_json.into_bytes(),
                )))
            }
            (RtValue::SimResult(s), "boundary_bytes") => Ok(RtValue::Bytes(std::sync::Arc::new(
                s.final_snapshot.space.boundary_descriptor().into_bytes(),
            ))),
            (RtValue::Box3(b), "boundary_bytes") => Ok(RtValue::Bytes(std::sync::Arc::new(
                b.boundary_descriptor().into_bytes(),
            ))),
            (RtValue::SimResult(s), "ticks") => Ok(RtValue::Int(BigInt::from_u64(s.ticks))),
            (RtValue::SimResult(s), "carrier_count") => {
                Ok(RtValue::Int(BigInt::from_u64(s.carrier_count as u64)))
            }
            (RtValue::SimResult(s), "active_carrier_count") => {
                Ok(RtValue::Int(BigInt::from_u64(s.active_carrier_count as u64)))
            }
            (RtValue::SimResult(s), "shedding_event_count") => {
                Ok(RtValue::Int(BigInt::from_u64(s.shedding_event_count as u64)))
            }
            (RtValue::SimResult(s), "wavelet_event_count") => {
                Ok(RtValue::Int(BigInt::from_u64(s.wavelet_event_count as u64)))
            }
            (RtValue::SimResult(s), "boundary_crossings") => {
                Ok(RtValue::Int(BigInt::from_u64(s.boundary_crossings)))
            }
            (RtValue::SimResult(s), "resonance_count") => {
                Ok(RtValue::Int(BigInt::from_u64(s.resonance_count as u64)))
            }
            (RtValue::SimResult(s), "expired_contact_count") => {
                Ok(RtValue::Int(BigInt::from_u64(s.expired_contact_count as u64)))
            }
            (RtValue::SimResult(s), "fusion_count") => {
                Ok(RtValue::Int(BigInt::from_u64(s.fusion_count as u64)))
            }
            (RtValue::SimResult(s), "moving_flux_total") => {
                Ok(RtValue::Int(s.moving_flux_total))
            }
            (RtValue::SimResult(s), "total_returns") => {
                Ok(RtValue::Int(BigInt::from_u64(s.blind_motif_spectrum.total_returns)))
            }
            (RtValue::SimResult(s), "total_hinges") => {
                Ok(RtValue::Int(BigInt::from_u64(s.blind_motif_spectrum.total_hinges)))
            }
            (RtValue::SimResult(s), "total_shedding_events") => Ok(RtValue::Int(BigInt::from_u64(
                s.blind_motif_spectrum.total_shedding_events,
            ))),
            (RtValue::SimResult(s), "final_snapshot") => {
                Ok(RtValue::ChamberView(Box::new(s.final_snapshot)))
            }

            (RtValue::ChamberView(snap), "model_status") => {
                let (variant, tag) = model_status_variant_and_tag(&snap.model_status);
                Ok(RtValue::Enum(std::sync::Arc::new(RtEnumValue {
                    module: "<builtin>".to_string(),
                    name: "ModelStatus".to_string(),
                    variant: variant.to_string(),
                    tag,
                    payloads: Vec::new(),
                })))
            }
            (RtValue::ChamberView(snap), "model_status_bytes") => Ok(RtValue::Bytes(
                std::sync::Arc::new(snap.model_status.into_bytes()),
            )),
            (RtValue::ChamberView(snap), "chamber_name" | "chamber_name_bytes") => Ok(
                RtValue::Bytes(std::sync::Arc::new(snap.chamber_name.into_bytes())),
            ),
            (
                RtValue::ChamberView(snap),
                "shedding_witnesses_json" | "shedding_witnesses_json_bytes",
            ) => Ok(RtValue::Bytes(std::sync::Arc::new(
                snap.shedding_witnesses_json.into_bytes(),
            ))),
            (RtValue::ChamberView(snap), "tick") => Ok(RtValue::Int(BigInt::from_u64(snap.tick))),
            (RtValue::ChamberView(snap), "carrier_count") => {
                Ok(RtValue::Int(BigInt::from_u64(snap.carriers.len() as u64)))
            }
            (RtValue::ChamberView(snap), "active_carrier_count") => Ok(RtValue::Int(
                BigInt::from_u64(snap.active_carrier_count as u64),
            )),
            (RtValue::ChamberView(snap), "boundary_crossings") => {
                Ok(RtValue::Int(BigInt::from_u64(snap.boundary_crossings)))
            }
            (RtValue::ChamberView(snap), "resonance_count") => {
                Ok(RtValue::Int(BigInt::from_u64(snap.resonance_count as u64)))
            }
            (RtValue::ChamberView(snap), "expired_contact_count") => Ok(RtValue::Int(
                BigInt::from_u64(snap.expired_contact_count as u64),
            )),
            (RtValue::ChamberView(snap), "fusion_count") => {
                Ok(RtValue::Int(BigInt::from_u64(snap.fusion_count as u64)))
            }
            (RtValue::ChamberView(snap), "shedding_count") => Ok(RtValue::Int(BigInt::from_u64(
                snap.shedding_events_this_tick.len() as u64,
            ))),
            (RtValue::ChamberView(snap), "wavelet_count") => Ok(RtValue::Int(BigInt::from_u64(
                snap.wavelet_events_this_tick.len() as u64,
            ))),
            (RtValue::ChamberView(snap), "clock_count") => {
                Ok(RtValue::Int(BigInt::from_u64(snap.clocks.len() as u64)))
            }
            (RtValue::ChamberView(snap), "carriers") => Ok(RtValue::List(std::sync::Arc::new(
                snap.carriers
                    .iter()
                    .map(|c| RtValue::Int(BigInt::from_u64(c.id)))
                    .collect(),
            ))),

            (RtValue::RecipientOutcome(o), "stage") => {
                let (variant, tag) = recipient_stage_variant_and_tag(o.stage);
                Ok(RtValue::Enum(std::sync::Arc::new(RtEnumValue {
                    module: "<builtin>".to_string(),
                    name: "RecipientStage".to_string(),
                    variant: variant.to_string(),
                    tag,
                    payloads: Vec::new(),
                })))
            }
            (RtValue::RecipientOutcome(o), "stage_bytes") => Ok(RtValue::Bytes(
                std::sync::Arc::new(recipient_stage_name(o.stage).as_bytes().to_vec()),
            )),
            (RtValue::RecipientOutcome(o), "closure_phase_delta") => {
                Ok(RtValue::Int(BigInt::from_u64(o.closure_phase_delta as u64)))
            }
            (RtValue::RecipientOutcome(o), "reclosure_bip_delta") => {
                Ok(RtValue::Int(BigInt::from_u64(o.reclosure_bip_delta as u64)))
            }
            (RtValue::RecipientOutcome(o), "blocks_later_seating") => {
                Ok(RtValue::Bool(o.blocks_later_seating))
            }

            (RtValue::CadenceAssociation(a), "is_cadence_ratio") => {
                Ok(RtValue::Bool(a.is_cadence_ratio))
            }
            (RtValue::CadenceAssociation(a), "wavelet_id" | "wavelet_id_bytes") => {
                Ok(RtValue::Bytes(std::sync::Arc::new(a.wavelet_id.into_bytes())))
            }
            (RtValue::CadenceAssociation(a), "reference_id" | "reference_id_bytes") => Ok(
                RtValue::Bytes(std::sync::Arc::new(a.reference_id.into_bytes())),
            ),

            (RtValue::Support(s), "is_yro") => Ok(RtValue::Bool(s.is_yro())),
            (RtValue::Support(s), "is_closure_certificate") => {
                Ok(RtValue::Bool(s.is_closure_certificate()))
            }
            (RtValue::Addr8(a), "is_first_anchor") => Ok(RtValue::Bool(a.is_first_anchor())),
            (RtValue::Window(u, _), "start") => Ok(RtValue::Int(BigInt::from_i64(u))),
            (RtValue::Window(_, v), "end") => Ok(RtValue::Int(BigInt::from_i64(v))),
            (RtValue::Window(u, v), "len") => {
                Ok(RtValue::Int(BigInt::from_i128((v as i128) - (u as i128))))
            }
            (RtValue::Ratio(r), "num") => Ok(RtValue::Int(r.num().clone())),
            (RtValue::Ratio(r), "den") => Ok(RtValue::Int(r.den().clone())),
            (other, f) => Err(format!("no field .{f} on {}", other.render())),
        }
    }

    pub fn op_boundary_of(&mut self, h: u64) -> Result<u64, String> {
        let val = self.arena.get(h)?.clone();
        let out = match val {
            RtValue::Box3(b) => {
                RtValue::Bytes(std::sync::Arc::new(b.boundary_descriptor().into_bytes()))
            }
            RtValue::SimResult(s) => RtValue::Bytes(std::sync::Arc::new(
                s.final_snapshot.space.boundary_descriptor().into_bytes(),
            )),
            RtValue::Window(u, v) | RtValue::Range(u, v) => RtValue::Tuple(vec![
                RtValue::Int(BigInt::from_i64(u)),
                RtValue::Int(BigInt::from_i64(v)),
            ]),
            RtValue::Array(xs) => {
                let diff = finite_diff_values(&xs)?;
                RtValue::Array(std::sync::Arc::new(diff))
            }
            RtValue::List(xs) => {
                let diff = finite_diff_values(&xs)?;
                RtValue::List(std::sync::Arc::new(diff))
            }
            other => return Err(format!("boundary_of unsupported on {}", other.render())),
        };
        self.arena.insert(out)
    }

    pub fn op_len(&self, h: u64) -> Result<BigInt, String> {
        match self.arena.get(h)? {
            RtValue::Str(s) => Ok(BigInt::from_u64(s.chars().count() as u64)),
            RtValue::Bytes(bs) => Ok(BigInt::from_u64(bs.len() as u64)),
            RtValue::Array(xs) | RtValue::List(xs) => Ok(BigInt::from_u64(xs.len() as u64)),
            RtValue::Map(m) => Ok(BigInt::from_u64(m.len() as u64)),
            other => Err(format!("len unsupported on {}", other.render())),
        }
    }

    pub fn op_window_dedup_count(
        &self,
        lin_h: u64,
        scope_h: u64,
        cuts_h: u64,
        wins_h: u64,
    ) -> Result<BigInt, String> {
        let lin = self.get_str_or_utf8_bytes(lin_h, "window_dedup_count")?;
        let scope = self.get_str_or_utf8_bytes(scope_h, "window_dedup_count")?;
        let cuts = match self.arena.get(cuts_h)? {
            RtValue::List(items) | RtValue::Array(items) => {
                let mut v = Vec::with_capacity(items.len());
                for it in items.iter() {
                    match it {
                        RtValue::Int(n) => {
                            v.push(n.to_i64().ok_or_else(|| "cut out of i64 range".to_string())?)
                        }
                        other => return Err(format!("expected int cut, got {}", other.render())),
                    }
                }
                v
            }
            other => return Err(format!("expected sequence of cuts, got {}", other.render())),
        };
        let wins = match self.arena.get(wins_h)? {
            RtValue::List(items) | RtValue::Array(items) => {
                let mut v = Vec::with_capacity(items.len());
                for it in items.iter() {
                    match it {
                        RtValue::Window(a, b) => v.push((*a, *b)),
                        other => return Err(format!("expected window, got {}", other.render())),
                    }
                }
                v
            }
            other => return Err(format!("expected sequence of windows, got {}", other.render())),
        };
        let (witnesses, reg) =
            declare_synthetic_lineage_registry(&lin, &scope, &cuts, 0, "native_cut", None)
                .map_err(|e| e.to_string())?;
        let pdesc = &witnesses[0].parent_descriptor;
        let mut wavelets = Vec::with_capacity(wins.len());
        for (idx, (u, v)) in wins.into_iter().enumerate() {
            let selected: Vec<_> = witnesses
                .iter()
                .filter(|w| w.completion_cut <= v)
                .cloned()
                .collect();
            let wl = Wavelet::new(WaveletSpec {
                wavelet_id: &format!("w{idx}"),
                parent_lineage: &lin,
                explicit_parent: Some(pdesc),
                ancestry: vec![format!("anc:{lin}"), format!("w{idx}")],
                window: (u, v),
                sigma_seat: SeatState::SeatedDuad,
                sigma_role: WaveletRole::CoupledSegment,
                records: vec![format!("rec:w{idx}")],
                return_witnesses: selected,
                signed_current: BigInt::zero(),
                is_admitted_wave: false,
                scope_id: &scope,
                witnesses_complete: true,
                scale: Some(0),
                cut_coordinate: "native_cut",
                scope_map: None,
                registry: Some(&reg),
            })
            .map_err(|e| e.to_string())?;
            wavelets.push(wl);
        }
        let count = deduplicated_return_count(&wavelets, Some(pdesc), None, &scope, None, Some(&reg))
            .map_err(|e| e.to_string())?;
        Ok(BigInt::from_u64(count as u64))
    }

    pub fn op_recipient_eval_demo(
        &mut self,
        ek: &BigInt,
        sk: &BigInt,
        win_h: u64,
    ) -> Result<u64, String> {
        let ek_i = ek
            .to_i64()
            .ok_or_else(|| "emit_cut out of i64 range".to_string())?;
        let sk_i = sk
            .to_i64()
            .ok_or_else(|| "start_idx out of i64 range".to_string())?;
        let (u, v) = self.get_window(win_h)?;
        let shed = ShedWitness::new(
            "W_parent",
            "dT_R",
            "shared_k",
            ek_i,
            ShedRoute::Exo,
            Ratio::from_i64(1, 2).map_err(|e| e.to_string())?,
            "post_shed",
        )
        .map_err(|e| e.to_string())?;
        let prior0 = AdmittedClosureRecord::admit_monon(
            M3ClosureTrace::minimal_closed("R_rec", 0, "B_rec", 0, Some(1))
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let fresh = AdmittedClosureRecord::admit_monon(
            M3ClosureTrace::minimal_closed("R_rec", 1, "B_rec", 0, Some(sk_i))
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let ctx = RecipientContext {
            recipient_lineage: "R_rec".to_string(),
            recipient_scope: "B_rec".to_string(),
            cut_coordinate: "shared_k".to_string(),
            closure_cut_coordinate: None,
            window: (u, v),
            coordinate_relation: None,
            prior_certified_returns: vec![prior0],
            seen_return_ids: BTreeSet::new(),
        };
        let mut events = BTreeMap::new();
        events.insert(
            RecipientStage::Delivered,
            StageEventWitness::new("d1", RecipientStage::Delivered, "B_rec", "shared_k", ek_i)
                .map_err(|e| e.to_string())?,
        );
        events.insert(
            RecipientStage::Accepted,
            StageEventWitness::new("a1", RecipientStage::Accepted, "B_rec", "shared_k", ek_i)
                .map_err(|e| e.to_string())?,
        );
        events.insert(
            RecipientStage::Seated,
            StageEventWitness::new("s1", RecipientStage::Seated, "B_rec", "shared_k", ek_i)
                .map_err(|e| e.to_string())?,
        );
        let outcome = RecipientLattice::evaluate(
            &shed,
            &ctx,
            &[
                RecipientStage::Emitted,
                RecipientStage::Delivered,
                RecipientStage::Accepted,
                RecipientStage::Seated,
                RecipientStage::Reclosed,
            ],
            Some(&fresh),
            Some(&events),
            None,
        )
        .map_err(|e| e.to_string())?;
        self.arena
            .insert(RtValue::RecipientOutcome(Box::new(outcome)))
    }

    pub fn op_sim_builder_new(
        &mut self,
        chamber_name_h: u64,
        space_box_h: u64,
        walls_code: u8,
    ) -> Result<u64, String> {
        let chamber_name = self.get_str_or_utf8_bytes(chamber_name_h, "chamber")?;
        let space = self.get_box(space_box_h)?;
        let walls = WallPolicy::from_code(walls_code)
            .ok_or_else(|| format!("invalid wall policy code {walls_code}"))?;
        self.sim_builders.push(SimBuilder {
            chamber_name,
            space,
            walls,
            dynamics: ChamberDynamicsConfig::default(),
            clocks: Vec::new(),
            carriers: Vec::new(),
            decl_observers: Vec::new(),
            proc_observers: Vec::new(),
        });
        Ok(self.sim_builders.len() as u64)
    }

    pub fn op_sim_builder_set_dynamics(
        &mut self,
        b_h: u64,
        dx: &TritInt,
        dy: &TritInt,
        dz: &TritInt,
        contact_ttl: i64,
        fuse_on_resonance: u8,
    ) -> Result<(), String> {
        let dxi = dx
            .to_i64()
            .ok_or_else(|| format!("boundary_step dx {dx} out of i64 range"))?;
        let dyi = dy
            .to_i64()
            .ok_or_else(|| format!("boundary_step dy {dy} out of i64 range"))?;
        let dzi = dz
            .to_i64()
            .ok_or_else(|| format!("boundary_step dz {dz} out of i64 range"))?;
        let ttl = if contact_ttl < 0 {
            None
        } else {
            Some(contact_ttl as u64)
        };
        let idx = builder_idx(b_h, self.sim_builders.len())?;
        self.sim_builders[idx].dynamics = ChamberDynamicsConfig {
            boundary_step: [dxi, dyi, dzi],
            contact_ttl: ttl,
            fuse_on_resonance: fuse_on_resonance != 0,
        };
        Ok(())
    }

    pub fn op_sim_builder_add_clock(
        &mut self,
        b_h: u64,
        name_h: u64,
        period: &BigInt,
    ) -> Result<(), String> {
        let name = self.get_str_or_utf8_bytes(name_h, "clock")?;
        let p = period
            .to_i64()
            .ok_or_else(|| format!("clock {name} period {period} out of i64 range"))?;
        if p <= 0 {
            return Err(format!("clock {name} period must be >= 1, got {p}"));
        }
        let clk = ReferenceClock::new(name, p as u64).map_err(|e| e.to_string())?;
        let idx = builder_idx(b_h, self.sim_builders.len())?;
        self.sim_builders[idx].clocks.push(clk);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn op_sim_builder_add_carrier(
        &mut self,
        b_h: u64,
        support_h: u64,
        x: &BigInt,
        y: &BigInt,
        z: &BigInt,
        dir_code: u8,
        orient: i8,
        shells: &BigInt,
    ) -> Result<(), String> {
        let support = self.get_support(support_h)?.clone();
        let xi = x.to_i64().ok_or_else(|| "carrier x out of i64".to_string())?;
        let yi = y.to_i64().ok_or_else(|| "carrier y out of i64".to_string())?;
        let zi = z.to_i64().ok_or_else(|| "carrier z out of i64".to_string())?;
        let dir = AxisDir::from_code(dir_code)
            .ok_or_else(|| format!("invalid axis dir code {dir_code}"))?;
        let t_orient = T1::new(orient).map_err(|e| e.to_string())?;
        if t_orient == T1::ZERO {
            return Err("inject orient must be nonzero (-1t or +1t)".to_string());
        }
        let sh_i = shells
            .to_i64()
            .ok_or_else(|| format!("inject shells {shells} out of i64"))?;
        if sh_i <= 0 {
            return Err(format!("inject shells must be >= 1, got {sh_i}"));
        }
        let sh = u32::try_from(sh_i)
            .map_err(|_| format!("inject shells {sh_i} exceeds u32"))?;
        let idx = builder_idx(b_h, self.sim_builders.len())?;
        let next_id = (self.sim_builders[idx].carriers.len() + 1) as u64;
        let scope = self.sim_builders[idx].chamber_name.clone();
        let c = CarrierState::new(next_id, &scope, support, sh, [xi, yi, zi], dir, t_orient)
            .map_err(|e| e.to_string())?;
        self.sim_builders[idx].carriers.push(c);
        Ok(())
    }

    pub fn op_sim_builder_add_decl_obs(
        &mut self,
        b_h: u64,
        kind_code: u8,
        name_h: u64,
        region_box_h: u64,
    ) -> Result<(), String> {
        let kind = ProbeKind::from_code(kind_code)
            .ok_or_else(|| format!("invalid probe kind code {kind_code}"))?;
        let name = self.get_str_or_utf8_bytes(name_h, "observer")?;
        let region = self.get_box(region_box_h)?;
        let idx = builder_idx(b_h, self.sim_builders.len())?;
        self.sim_builders[idx]
            .decl_observers
            .push(DeclarativeObserver::new(kind, name, region, Vec::new()));
        Ok(())
    }

    pub fn op_sim_builder_add_proc_obs(
        &mut self,
        b_h: u64,
        name_h: u64,
        cb_addr: usize,
    ) -> Result<(), String> {
        let name = self.get_str_or_utf8_bytes(name_h, "observer")?;
        let idx = builder_idx(b_h, self.sim_builders.len())?;
        self.sim_builders[idx].proc_observers.push((name, cb_addr));
        Ok(())
    }

    pub fn take_sim_builder(&self, b_h: u64) -> Result<SimBuilder, String> {
        let idx = builder_idx(b_h, self.sim_builders.len())?;
        Ok(self.sim_builders[idx].clone())
    }

    pub fn finalize_sim_result(
        &mut self,
        state: ChamberState,
        decl_observers: Vec<DeclarativeObserver>,
    ) -> Result<u64, String> {
        let state_json = state.to_json().to_compact_ascii_json();
        let final_hash = Sha256::digest_hex(state_json.as_bytes());
        let final_snapshot = state.snapshot();
        let active_carrier_count = state.active_carrier_count();
        let shedding_witnesses_json = final_snapshot.shedding_witnesses_json.clone();
        let resonance_count = state.resonances.len();
        let expired_contact_count = state.expired_contacts.len();
        let fusion_count = state.fusions.len();
        let moving_flux_total = state.moving_flux_total();
        let mut support_counts = BTreeMap::new();
        for c in &state.carriers {
            *support_counts.entry(c.support.render()).or_insert(0) += 1;
        }
        let total_returns: u64 = state.carriers.iter().map(|c| c.return_count).sum();
        let total_hinges: u64 = state.carriers.iter().map(|c| c.hinge_count).sum();
        let total_shedding_events = state.shedding_events.len() as u64;
        let sensor_currents: Vec<SensorCurrentSummary> = decl_observers
            .iter()
            .map(|obs| {
                let (cadence_ratio, single_contact_association) = obs.cadence_readout();
                SensorCurrentSummary {
                    sensor_name: obs.name.clone(),
                    boundary_crossings: obs.boundary_crossings,
                    bound_contacts: obs.observed_contacts,
                    cadence_ratio,
                    single_contact_association,
                }
            })
            .collect();
        let res = SimRunResult {
            model_status: state.model_status,
            chamber_name: state.chamber_name,
            ticks: state.tick,
            final_state_hash: final_hash,
            state_json,
            carrier_count: state.carriers.len(),
            active_carrier_count,
            shedding_event_count: state.shedding_events.len(),
            wavelet_event_count: state.wavelet_events.len(),
            boundary_crossings: state.boundary_crossings,
            shedding_witnesses_json,
            resonance_count,
            expired_contact_count,
            fusion_count,
            moving_flux_total,
            blind_motif_spectrum: BlindMotifSpectrum {
                support_counts,
                total_returns,
                total_hinges,
                total_shedding_events,
            },
            sensor_currents,
            procedural_outputs: BTreeMap::new(),
            final_snapshot,
        };
        self.arena.insert(RtValue::SimResult(Box::new(res)))
    }

    pub fn op_sadar_promote(&self, c3: &BigInt, rho: &BigInt, att: Ratio) -> Result<Ratio, String> {
        SadarPromotion::promote(c3, rho, att)
            .map(|r| r.sadar)
            .map_err(|e| e.to_string())
    }

    pub fn op_probability(
        &self,
        neg: &BigInt,
        hinge: &BigInt,
        pos: &BigInt,
        role: i8,
    ) -> Result<Ratio, String> {
        let t = T1::new(role).map_err(|e| e.to_string())?;
        probability(neg, hinge, pos, t).map_err(|e| e.to_string())
    }

    pub fn op_native_cadence_ratio(
        &self,
        id_a_h: u64,
        q_a: &BigInt,
        id_r_h: u64,
        q_r: &BigInt,
        coupled: bool,
        contacts: &BigInt,
    ) -> Result<Ratio, String> {
        let id_a = self.get_str_or_utf8_bytes(id_a_h, "native_cadence_ratio")?;
        let id_r = self.get_str_or_utf8_bytes(id_r_h, "native_cadence_ratio")?;
        let bc = contacts
            .to_u64()
            .ok_or_else(|| "invalid bound_contacts".to_string())? as usize;
        native_cadence_ratio(&id_a, q_a, &id_r, q_r, coupled, bc).map_err(|e| e.to_string())
    }

    pub fn op_single_contact_assoc(
        &mut self,
        wid_h: u64,
        pid_h: u64,
        rid_h: u64,
        win_h: u64,
        contacts: &BigInt,
    ) -> Result<u64, String> {
        let wid = self.get_str_or_utf8_bytes(wid_h, "single_contact_association")?;
        let pid = self.get_str_or_utf8_bytes(pid_h, "single_contact_association")?;
        let rid = self.get_str_or_utf8_bytes(rid_h, "single_contact_association")?;
        let win = self.get_window(win_h)?;
        let bc = contacts
            .to_u64()
            .ok_or_else(|| "invalid bound_contacts".to_string())? as usize;
        let assoc = SingleContactStageAssociation::new(wid, pid, rid, win, bc)
            .map_err(|e| e.to_string())?;
        self.arena
            .insert(RtValue::CadenceAssociation(Box::new(assoc)))
    }

    pub fn op_qext_ext_decimal(&mut self, str_h: u64) -> Result<u64, String> {
        let s = self.get_str_or_utf8_bytes(str_h, "ext_decimal")?;
        let q = parse_decimal_exact(&s).map_err(|e| e.to_string())?;
        self.arena.insert(RtValue::QExtRatio(q))
    }

    pub fn op_struct_new(
        &mut self,
        mod_h: u64,
        name_h: u64,
        field_name_handles: &[u64],
        field_val_handles: &[u64],
    ) -> Result<u64, String> {
        if field_name_handles.len() != field_val_handles.len() {
            return Err("struct field count mismatch".to_string());
        }
        let module = self.get_str_or_utf8_bytes(mod_h, "struct module")?;
        let name = self.get_str_or_utf8_bytes(name_h, "struct name")?;
        let mut fields = BTreeMap::new();
        for (&nh, &vh) in field_name_handles.iter().zip(field_val_handles) {
            let fname = self.get_str_or_utf8_bytes(nh, "struct field")?;
            let fval = self.arena.get(vh)?.clone();
            fields.insert(fname, fval);
        }
        self.arena
            .insert(RtValue::Struct(std::sync::Arc::new(RtStructValue {
                module,
                name,
                fields,
            })))
    }

    pub fn op_enum_new(
        &mut self,
        mod_h: u64,
        name_h: u64,
        var_h: u64,
        tag: i32,
        payload_handles: &[u64],
    ) -> Result<u64, String> {
        let module = self.get_str_or_utf8_bytes(mod_h, "enum module")?;
        let name = self.get_str_or_utf8_bytes(name_h, "enum name")?;
        let variant = self.get_str_or_utf8_bytes(var_h, "enum variant")?;
        let mut payloads = Vec::with_capacity(payload_handles.len());
        for &ph in payload_handles {
            payloads.push(self.arena.get(ph)?.clone());
        }
        self.arena
            .insert(RtValue::Enum(std::sync::Arc::new(RtEnumValue {
                module,
                name,
                variant,
                tag,
                payloads,
            })))
    }
}

fn finite_diff_values(xs: &[RtValue]) -> Result<Vec<RtValue>, String> {
    if xs.len() < 2 {
        return Err("finite difference (∂) requires at least 2 elements".to_string());
    }
    let mut out = Vec::with_capacity(xs.len() - 1);
    for w in xs.windows(2) {
        let d = match (&w[0], &w[1]) {
            (RtValue::Int(a), RtValue::Int(b)) => RtValue::Int(b - a),
            (RtValue::Ratio(a), RtValue::Ratio(b)) => RtValue::Ratio(b - a),
            (RtValue::T1(a), RtValue::T1(b)) => {
                RtValue::T1(b.checked_sub(*a).map_err(|e| e.to_string())?)
            }
            (other_a, _) => {
                return Err(format!(
                    "finite difference (∂) requires numeric elements, got {}",
                    other_a.render()
                ));
            }
        };
        out.push(d);
    }
    Ok(out)
}

fn builder_idx(handle: u64, len: usize) -> Result<usize, String> {
    if handle == 0 || (handle as usize) > len {
        return Err(format!("invalid simulation builder handle {handle}"));
    }
    Ok((handle - 1) as usize)
}

pub fn decode_hex_32(hex: &str) -> Result<Vec<u8>, String> {
    let raw = hex.as_bytes();
    if raw.len() != 64 {
        return Err(format!(
            "expected 64-byte hex SHA-256 digest, got {} bytes",
            raw.len()
        ));
    }
    let hex_val = |b: u8| -> Result<u8, String> {
        match b {
            b'0'..=b'9' => Ok(b - b'0'),
            b'a'..=b'f' => Ok(b - b'a' + 10),
            b'A'..=b'F' => Ok(b - b'A' + 10),
            _ => Err("invalid hex digit in SHA-256 digest".to_string()),
        }
    };
    let mut out = Vec::with_capacity(32);
    for chunk in raw.chunks_exact(2) {
        let hi = hex_val(chunk[0])?;
        let lo = hex_val(chunk[1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

/// Maximum exponent accepted by `pow` (`P1-6`).
pub const MAX_POW_EXP: u32 = 65_536;
/// Maximum result trit-length (`floor(1_048_576 / log2(3)) + 1 = 661_578` trits = 1M-bit ceiling).
pub const MAX_POW_TRITS: usize = 661_578;

/// Checked balanced-ternary exponentiation enforcing `0 <= exp <= 65_536` and a `1_048_576`-bit
/// (`661_578`-trit) result ceiling at every squaring/multiplication step (`P1-6`).
pub fn checked_trit_pow_u32(base: &BigInt, exp: u32) -> Result<BigInt, String> {
    if exp > MAX_POW_EXP {
        return Err(format!(
            "overflow: pow exponent {exp} exceeds safety limit {MAX_POW_EXP}"
        ));
    }
    if exp == 0 {
        return Ok(BigInt::one());
    }
    if base.is_zero() {
        return Ok(BigInt::zero());
    }
    if *base == BigInt::one() {
        return Ok(BigInt::one());
    }
    if *base == BigInt::NEG_ONE {
        return Ok(if (exp & 1) == 0 {
            BigInt::one()
        } else {
            BigInt::NEG_ONE
        });
    }
    let tl = base.trit_len();
    if tl > MAX_POW_TRITS || tl.saturating_sub(1).saturating_mul(exp as usize) >= MAX_POW_TRITS {
        return Err("overflow: pow result exceeds 1048576-bit safety limit".to_string());
    }
    let mut result = BigInt::one();
    let mut cur = base.clone();
    let mut e = exp;
    while e > 0 {
        if (e & 1) == 1 {
            result = &result * &cur;
            if result.trit_len() > MAX_POW_TRITS {
                return Err("overflow: pow result exceeds 1048576-bit safety limit".to_string());
            }
        }
        e >>= 1;
        if e > 0 {
            cur = &cur * &cur;
            if cur.trit_len() > MAX_POW_TRITS {
                return Err("overflow: pow result exceeds 1048576-bit safety limit".to_string());
            }
        }
    }
    Ok(result)
}

pub fn checked_trit_pow(base: &BigInt, exp: &BigInt) -> Result<BigInt, String> {
    if exp.is_negative() {
        return Err(format!(
            "domain error: int pow requires non-negative exponent, got {exp}"
        ));
    }
    let e = exp.to_u64().and_then(|v| u32::try_from(v).ok()).ok_or_else(|| {
        format!("overflow: pow exponent {exp} exceeds safety limit {MAX_POW_EXP}")
    })?;
    checked_trit_pow_u32(base, e)
}

pub fn exact_unpack_t9_bits(bits: u64) -> Result<BigInt, String> {
    if bits >= (1u64 << 18) {
        return Err(format!(
            "domain error: packed t9 outside 18-bit domain: {bits:#x}"
        ));
    }
    if !aod_core::trit_int::is_valid_trit_word(bits) {
        return Err(aod_core::error::CoreError::InvalidTritEncoding(3).to_string());
    }
    TritInt::from_inline_word(bits).map_err(|e| e.to_string())
}

pub fn exact_verify_sheddic_route(x_shed: &BigInt, target_enclosure: u8) -> bool {
    !x_shed.is_positive() || (target_enclosure >= 2 && target_enclosure.is_multiple_of(2))
}

pub fn exact_verify_sheddic_route_int(x_shed: &BigInt, target_enclosure: &BigInt) -> bool {
    !x_shed.is_positive()
        || (target_enclosure >= &BigInt::from_i64(2) && target_enclosure.is_even())
}

pub fn exact_verify_frg_cross_return(t_zero: &BigInt, t_cross: &BigInt, c_cross: &BigInt) -> bool {
    (t_cross - t_zero) == *c_cross
}

pub fn exact_t1(v: &BigInt) -> Result<i8, String> {
    T1::from_bigint(v)
        .map(T1::value)
        .map_err(|e| e.to_string())
}

pub fn exact_q4(v: &BigInt) -> Result<i8, String> {
    Q4::from_bigint(v)
        .map(|q| q.value() as i8)
        .map_err(|e| e.to_string())
}

pub fn exact_b8(v: &BigInt) -> Result<u8, String> {
    B8::from_int(v)
        .map(B8::value)
        .map_err(|e| e.to_string())
}

pub fn exact_b64_from_trit_int(v: &BigInt) -> Result<u64, String> {
    B64::from_trit_int(v)
        .map(B64::value)
        .map_err(|e| e.to_string())
}

pub fn exact_trit_int_from_b64(bits: u64) -> Result<BigInt, String> {
    B64::new(bits)
        .to_trit_int()
        .map_err(|e| e.to_string())
}

pub fn exact_addr8_parse(s: &str) -> Result<Addr8, String> {
    Addr8::parse(s).map_err(|e| e.to_string())
}

pub fn exact_support_parse(s: &str) -> Result<SupportForm, String> {
    SupportForm::parse(s).map_err(|e| e.to_string())
}

pub fn exact_trace_new(sc: &str, lin: &str, q4: i8) -> Result<Trace, String> {
    let st = Q4::new(q4 as u8).map_err(|e| e.to_string())?;
    Trace::new(sc, lin, st).map_err(|e| e.to_string())
}

pub fn exact_coupling_contact(c: &Coupling, a: &BigInt, b: &BigInt) -> Result<Coupling, String> {
    let au = a
        .to_u64()
        .ok_or_else(|| "invalid contact ordinal".to_string())? as usize;
    let bu = b
        .to_u64()
        .ok_or_else(|| "invalid contact ordinal".to_string())? as usize;
    c.contact(au, bu).map_err(|e| e.to_string())
}

pub fn exact_qext_binop(op: u8, a: &QExtRatio, b: &QExtRatio) -> Result<QExtRatio, String> {
    match op {
        0 => Ok(a.checked_add(b)),
        1 => Ok(a.checked_sub(b)),
        2 => Ok(a.checked_mul(b)),
        3 => a.checked_div(b).map_err(|e| e.to_string()),
        other => Err(format!("invalid QExt binop code {other}")),
    }
}
