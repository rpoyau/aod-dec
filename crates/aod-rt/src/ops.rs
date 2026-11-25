use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

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
    AxisDir, BlindMotifSpectrum, BoxRegion, CarrierState, ChamberState, DeclarativeObserver,
    ProbeKind, ReferenceClock, SensorCurrentSummary, SimRunResult, WallPolicy,
};
use crate::value::{recipient_stage_name, RtEnumValue, RtMapKey, RtStructValue, RtValue};

/// Maximum file size permitted across capability-gated `aod::sys` bootstrap I/O (`64 MiB`).
pub const MAX_SYS_IO_BYTES: usize = 64 * 1024 * 1024;

/// Pending simulation configuration built incrementally before `aod_rt_sim_run`.
#[derive(Clone, Debug)]
pub struct SimBuilder {
    pub chamber_name: String,
    pub space: BoxRegion,
    pub walls: WallPolicy,
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
                let mut cloned = items.clone();
                cloned[i as usize] = new_val;
                self.arena.insert(RtValue::Array(cloned))
            }
            RtValue::List(items) => {
                if i < 0 || (i as usize) >= items.len() {
                    return Err(format!("list index {i} out of bounds"));
                }
                let mut cloned = items.clone();
                cloned[i as usize] = new_val;
                self.arena.insert(RtValue::List(cloned))
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
        self.arena.insert(RtValue::Bytes(sliced))
    }

    pub fn op_bytes_concat(&mut self, a_h: u64, b_h: u64) -> Result<u64, String> {
        let a = self.get_bytes(a_h)?;
        let b = self.get_bytes(b_h)?;
        let mut out = Vec::with_capacity(a.len() + b.len());
        out.extend_from_slice(a);
        out.extend_from_slice(b);
        self.arena.insert(RtValue::Bytes(out))
    }

    pub fn op_bytes_sha256(&mut self, b_h: u64) -> Result<u64, String> {
        let bs = self.get_bytes(b_h)?;
        let digest = Sha256::digest(bs);
        self.arena.insert(RtValue::Bytes(digest.to_vec()))
    }

    pub fn op_bytes_hex(&mut self, b_h: u64) -> Result<u64, String> {
        let bs = self.get_bytes(b_h)?;
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = Vec::with_capacity(bs.len() * 2);
        for &byte in bs {
            out.push(HEX[(byte >> 4) as usize]);
            out.push(HEX[(byte & 0x0F) as usize]);
        }
        self.arena.insert(RtValue::Bytes(out))
    }

    pub fn op_sys_cli_args(&mut self) -> Result<u64, String> {
        let items: Vec<RtValue> = self
            .cli_args
            .iter()
            .cloned()
            .map(RtValue::Bytes)
            .collect();
        self.arena.insert(RtValue::List(items))
    }

    pub fn op_sys_read_file_bytes(&mut self, path_h: u64) -> Result<u64, String> {
        let raw_path = self.get_bytes(path_h)?;
        if raw_path.is_empty() || raw_path.contains(&0) {
            return Err("invalid empty or NUL-containing file path".to_string());
        }
        let path_str = std::str::from_utf8(raw_path)
            .map_err(|_| "file path must be valid UTF-8".to_string())?;
        let meta = std::fs::metadata(path_str)
            .map_err(|e| format!("failed to stat {path_str:?}: {e}"))?;
        if meta.len() > (MAX_SYS_IO_BYTES as u64) {
            return Err(format!(
                "file {path_str:?} exceeds {MAX_SYS_IO_BYTES}-byte bootstrap limit"
            ));
        }
        let data = std::fs::read(path_str)
            .map_err(|e| format!("failed to read {path_str:?}: {e}"))?;
        if data.len() > MAX_SYS_IO_BYTES {
            return Err(format!(
                "file {path_str:?} exceeds {MAX_SYS_IO_BYTES}-byte bootstrap limit"
            ));
        }
        self.arena.insert(RtValue::Bytes(data))
    }

    pub fn op_sys_write_executable_elf(&mut self, path_h: u64, img_h: u64) -> Result<BigInt, String> {
        let path_bytes = self.get_bytes(path_h)?.to_vec();
        let img = self.get_bytes(img_h)?.to_vec();
        if path_bytes.is_empty() || path_bytes.contains(&0) {
            return Err("invalid empty or NUL-containing output ELF path".to_string());
        }
        if img.len() < 64 || &img[0..4] != b"\x7FELF" {
            return Err("write_executable_elf requires a valid ELF64 header (\\x7FELF)".to_string());
        }
        if img.len() > MAX_SYS_IO_BYTES {
            return Err(format!(
                "ELF image exceeds {MAX_SYS_IO_BYTES}-byte bootstrap limit"
            ));
        }
        let path_str = std::str::from_utf8(&path_bytes)
            .map_err(|_| "output ELF path must be valid UTF-8".to_string())?;
        let target = Path::new(path_str);
        if let Ok(meta) = std::fs::symlink_metadata(target) {
            if meta.file_type().is_symlink() {
                return Err(format!("refusing to overwrite symlink {path_str:?}"));
            }
        }
        std::fs::write(target, &img)
            .map_err(|e| format!("failed to write ELF {path_str:?}: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("failed to chmod +x {path_str:?}: {e}"))?;
        }
        Ok(BigInt::from_u64(img.len() as u64))
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

    pub fn op_field_get(&mut self, h: u64, field: &str) -> Result<RtValue, String> {
        let val = self.arena.get(h)?.clone();
        match (val, field) {
            (RtValue::Struct(sv), f) => sv
                .fields
                .get(f)
                .cloned()
                .ok_or_else(|| format!("no field .{f} on struct {}", sv.name)),
            // Struct-field array projection (`bins.t0` across `[Struct; N]` or `[Struct]`):
            (RtValue::Array(items), f) => {
                let mut projected = Vec::with_capacity(items.len());
                for item in items {
                    match item {
                        RtValue::Struct(sv) => {
                            let fv = sv
                                .fields
                                .get(f)
                                .cloned()
                                .ok_or_else(|| format!("no field .{f} on struct {}", sv.name))?;
                            projected.push(fv);
                        }
                        other => {
                            return Err(format!(
                                "cannot project field .{f} on array element {}",
                                other.render()
                            ));
                        }
                    }
                }
                Ok(RtValue::Array(projected))
            }
            (RtValue::List(items), f) => {
                let mut projected = Vec::with_capacity(items.len());
                for item in items {
                    match item {
                        RtValue::Struct(sv) => {
                            let fv = sv
                                .fields
                                .get(f)
                                .cloned()
                                .ok_or_else(|| format!("no field .{f} on struct {}", sv.name))?;
                            projected.push(fv);
                        }
                        other => {
                            return Err(format!(
                                "cannot project field .{f} on slice element {}",
                                other.render()
                            ));
                        }
                    }
                }
                Ok(RtValue::List(projected))
            }
            (RtValue::SimResult(s), "model_status") => Ok(RtValue::Str(s.model_status)),
            (RtValue::SimResult(s), "chamber_name") => Ok(RtValue::Str(s.chamber_name)),
            (RtValue::SimResult(s), "final_state_hash") => Ok(RtValue::Str(s.final_state_hash)),
            (RtValue::SimResult(s), "ticks") => Ok(RtValue::Int(BigInt::from_u64(s.ticks))),
            (RtValue::SimResult(s), "carrier_count") => {
                Ok(RtValue::Int(BigInt::from_u64(s.carrier_count as u64)))
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

            (RtValue::ChamberView(snap), "model_status") => Ok(RtValue::Str(snap.model_status)),
            (RtValue::ChamberView(snap), "chamber_name") => Ok(RtValue::Str(snap.chamber_name)),
            (RtValue::ChamberView(snap), "tick") => Ok(RtValue::Int(BigInt::from_u64(snap.tick))),
            (RtValue::ChamberView(snap), "carrier_count") => {
                Ok(RtValue::Int(BigInt::from_u64(snap.carriers.len() as u64)))
            }
            (RtValue::ChamberView(snap), "boundary_crossings") => {
                Ok(RtValue::Int(BigInt::from_u64(snap.boundary_crossings)))
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
            (RtValue::ChamberView(snap), "carriers") => Ok(RtValue::List(
                snap.carriers
                    .iter()
                    .map(|c| RtValue::Int(BigInt::from_u64(c.id)))
                    .collect(),
            )),

            (RtValue::RecipientOutcome(o), "stage") => {
                Ok(RtValue::Str(recipient_stage_name(o.stage).to_string()))
            }
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
            (RtValue::CadenceAssociation(a), "wavelet_id") => Ok(RtValue::Str(a.wavelet_id)),
            (RtValue::CadenceAssociation(a), "reference_id") => Ok(RtValue::Str(a.reference_id)),

            (RtValue::Support(s), "is_yro") => Ok(RtValue::Bool(s.is_yro())),
            (RtValue::Support(s), "is_closure_certificate") => {
                Ok(RtValue::Bool(s.is_closure_certificate()))
            }
            (RtValue::Addr8(a), "is_first_anchor") => Ok(RtValue::Bool(a.is_first_anchor())),
            (RtValue::Window(u, _), "start") => Ok(RtValue::Int(BigInt::from_i64(u))),
            (RtValue::Window(_, v), "end") => Ok(RtValue::Int(BigInt::from_i64(v))),
            (RtValue::Window(u, v), "len") => Ok(RtValue::Int(BigInt::from_i64(v - u))),
            (other, f) => Err(format!("no field .{f} on {}", other.render())),
        }
    }

    pub fn op_boundary_of(&mut self, h: u64) -> Result<u64, String> {
        let val = self.arena.get(h)?.clone();
        let out = match val {
            RtValue::Box3(b) => RtValue::Str(b.boundary_descriptor()),
            RtValue::SimResult(s) => RtValue::Str(s.final_snapshot.space.boundary_descriptor()),
            RtValue::Window(u, v) | RtValue::Range(u, v) => RtValue::Tuple(vec![
                RtValue::Int(BigInt::from_i64(u)),
                RtValue::Int(BigInt::from_i64(v)),
            ]),
            RtValue::Array(xs) => {
                let diff = finite_diff_values(&xs)?;
                RtValue::Array(diff)
            }
            RtValue::List(xs) => {
                let diff = finite_diff_values(&xs)?;
                RtValue::List(diff)
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
        let lin = self.get_str(lin_h)?;
        let scope = self.get_str(scope_h)?;
        let cuts = match self.arena.get(cuts_h)? {
            RtValue::List(items) | RtValue::Array(items) => {
                let mut v = Vec::with_capacity(items.len());
                for it in items {
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
                for it in items {
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
            declare_synthetic_lineage_registry(lin, scope, &cuts, 0, "native_cut", None)
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
                parent_lineage: lin,
                explicit_parent: Some(pdesc),
                ancestry: vec![format!("anc:{lin}"), format!("w{idx}")],
                window: (u, v),
                sigma_seat: SeatState::SeatedDuad,
                sigma_role: WaveletRole::CoupledSegment,
                records: vec![format!("rec:w{idx}")],
                return_witnesses: selected,
                signed_current: BigInt::zero(),
                is_admitted_wave: false,
                scope_id: scope,
                witnesses_complete: true,
                scale: Some(0),
                cut_coordinate: "native_cut",
                scope_map: None,
                registry: Some(&reg),
            })
            .map_err(|e| e.to_string())?;
            wavelets.push(wl);
        }
        let count = deduplicated_return_count(&wavelets, Some(pdesc), None, scope, None, Some(&reg))
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
        let chamber_name = self.get_str(chamber_name_h)?.to_string();
        let space = self.get_box(space_box_h)?;
        let walls = WallPolicy::from_code(walls_code)
            .ok_or_else(|| format!("invalid wall policy code {walls_code}"))?;
        self.sim_builders.push(SimBuilder {
            chamber_name,
            space,
            walls,
            clocks: Vec::new(),
            carriers: Vec::new(),
            decl_observers: Vec::new(),
            proc_observers: Vec::new(),
        });
        Ok(self.sim_builders.len() as u64)
    }

    pub fn op_sim_builder_add_clock(
        &mut self,
        b_h: u64,
        name_h: u64,
        period: &BigInt,
    ) -> Result<(), String> {
        let name = self.get_str(name_h)?.to_string();
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
        let name = self.get_str(name_h)?.to_string();
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
        let name = self.get_str(name_h)?.to_string();
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
        let final_hash = state.state_hash();
        let final_snapshot = state.snapshot();
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
            carrier_count: state.carriers.len(),
            shedding_event_count: state.shedding_events.len(),
            wavelet_event_count: state.wavelet_events.len(),
            boundary_crossings: state.boundary_crossings,
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
        let id_a = self.get_str(id_a_h)?;
        let id_r = self.get_str(id_r_h)?;
        let bc = contacts
            .to_u64()
            .ok_or_else(|| "invalid bound_contacts".to_string())? as usize;
        native_cadence_ratio(id_a, q_a, id_r, q_r, coupled, bc).map_err(|e| e.to_string())
    }

    pub fn op_single_contact_assoc(
        &mut self,
        wid_h: u64,
        pid_h: u64,
        rid_h: u64,
        win_h: u64,
        contacts: &BigInt,
    ) -> Result<u64, String> {
        let wid = self.get_str(wid_h)?.to_string();
        let pid = self.get_str(pid_h)?.to_string();
        let rid = self.get_str(rid_h)?.to_string();
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
        let s = match self.arena.get(str_h)? {
            RtValue::Str(s) => s.clone(),
            RtValue::Bytes(bs) => std::str::from_utf8(bs)
                .map_err(|_| "ext_decimal bytes must be ASCII".to_string())?
                .to_string(),
            other => return Err(format!("expected str or bytes in ext_decimal, got {}", other.render())),
        };
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
        let module = self.get_str(mod_h)?.to_string();
        let name = self.get_str(name_h)?.to_string();
        let mut fields = BTreeMap::new();
        for (&nh, &vh) in field_name_handles.iter().zip(field_val_handles) {
            let fname = self.get_str(nh)?.to_string();
            let fval = self.arena.get(vh)?.clone();
            fields.insert(fname, fval);
        }
        self.arena.insert(RtValue::Struct(Box::new(RtStructValue {
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
        let module = self.get_str(mod_h)?.to_string();
        let name = self.get_str(name_h)?.to_string();
        let variant = self.get_str(var_h)?.to_string();
        let mut payloads = Vec::with_capacity(payload_handles.len());
        for &ph in payload_handles {
            payloads.push(self.arena.get(ph)?.clone());
        }
        self.arena.insert(RtValue::Enum(Box::new(RtEnumValue {
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
