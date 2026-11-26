use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use aod_core::json::parse_strict_json;
use aod_core::sha256::Sha256;
use aod_core::support::SupportForm;
use aod_core::ternary::T1;
use aod_core::wavelet::ShedRoute;
use aod_rt::sim::{
    AxisDir, BoxRegion, CarrierState, ChamberDynamicsConfig, ChamberState, WallPolicy,
};
use aodc::{
    audit_dec_source, build_aot_file, check_source, compile_file_to_ir, exec_ir_str, format_source,
    jit_file_captured, migrate_file_formatted, migrate_source_formatted, run_file, OptLevel,
};

static TEST_TMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn fresh_temp_dir(prefix: &str) -> PathBuf {
    let seq = TEST_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "aodc_{prefix}_{}_{}",
        std::process::id(),
        seq
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Decode a Dec `b"..."` stdout line back into raw UTF-8 text.
fn decode_dec_bytes_literal(line: &str) -> String {
    let inner = line
        .strip_prefix("b\"")
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or_else(|| panic!("expected b\"...\" bytes line, got: {line}"));
    let bytes = inner.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 1;
            assert!(i < bytes.len(), "dangling escape in {line}");
            match bytes[i] {
                b'\\' => out.push(b'\\'),
                b'"' => out.push(b'"'),
                b'n' => out.push(b'\n'),
                b'r' => out.push(b'\r'),
                b't' => out.push(b'\t'),
                b'0' => out.push(0),
                b'x' => {
                    let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
                    out.push(u8::from_str_radix(hex, 16).unwrap());
                    i += 2;
                }
                other => panic!("unexpected escape \\{} in {line}", other as char),
            }
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8(out).unwrap()
}

/// Run a Pure-Wave `v3` source across all 6 backends (`VM`, `DecIR v3`, `AOT -O0`, `AOT -O2`,
/// `JIT -O0`, `JIT -O2`) and assert byte-for-byte stdout identity.
fn run_6_modes(src: &str, label: &str) -> String {
    let dir = fresh_temp_dir(label);
    let dec_path = dir.join("prog.dec");
    std::fs::write(&dec_path, src).unwrap();

    // 1. Exact Reference VM
    let vm_out = run_file(&dec_path)
        .unwrap_or_else(|e| panic!("[{label}] VM failed: {e}"))
        .to_stdout_string();

    // 2. Hermetic DecIR v3 round-trip
    let ir_json = compile_file_to_ir(&dec_path)
        .unwrap_or_else(|e| panic!("[{label}] compile_file_to_ir failed: {e}"));
    let ir_out = exec_ir_str(&ir_json)
        .unwrap_or_else(|e| panic!("[{label}] exec_ir_str failed: {e}"))
        .to_stdout_string();
    assert_eq!(ir_out, vm_out, "[{label}] DecIR v3 != VM");

    // 3 & 4. Standalone AOT -O0 and AOT -O2
    for (tag, opt) in [("aot_o0", OptLevel::O0), ("aot_o2", OptLevel::O2)] {
        let exe = dir.join(tag);
        build_aot_file(&dec_path, opt, &exe)
            .unwrap_or_else(|e| panic!("[{label}] build_aot ({tag}) failed: {e}"));
        let out = Command::new(&exe)
            .args(["--fuel", "10000000", "--max-depth", "256"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "[{label}] {tag} exited non-zero: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let aot_stdout = String::from_utf8(out.stdout).unwrap();
        assert_eq!(aot_stdout, vm_out, "[{label}] {tag} != VM");
    }

    // 5 & 6. ORC LLJIT -O0 and -O2
    for (tag, opt) in [("jit_o0", OptLevel::O0), ("jit_o2", OptLevel::O2)] {
        let (jout, jres) = jit_file_captured(&dec_path, opt, 10_000_000, 256)
            .unwrap_or_else(|e| panic!("[{label}] {tag} compile failed: {e}"));
        jres.unwrap_or_else(|e| panic!("[{label}] {tag} runtime trap: {e}"));
        assert_eq!(jout, vm_out, "[{label}] {tag} != VM");
    }

    let _ = std::fs::remove_dir_all(&dir);
    vm_out
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let ty = entry.file_type().unwrap();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dst_path);
        } else if ty.is_file() {
            std::fs::copy(entry.path(), &dst_path).unwrap();
        }
    }
}

#[test]
fn blocker1_full_shedding_witness_replay_and_single_witness_mutation() {
    let src = r#"chamber WitnessBox:
    space = box([0..9, 0..9, 0..9])
    walls = closed
    contact_ttl = 20

prepare CoupledPair in WitnessBox:
    inject 3:3:6 at (1, 4, 4) dir +X orient +1t shells 1
    inject 3:3:6 at (7, 4, 4) dir -X orient -1t shells 1

observe WitnessProbe:
    tracer check_view(view):
        let _w = view.shedding_witnesses_json

dec main() -> int:
    let r = simulate CoupledPair over 12 ticks with WitnessProbe
    emit r.shedding_event_count
    emit r.final_state_hash
    emit r.shedding_witnesses_json
    emit r.state_json
    0
"#;

    let stdout = run_6_modes(src, "blocker1_witness_replay");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 5, "unexpected stdout line count: {stdout}");
    assert_eq!(lines[0], "2", "expected 2 shedding witnesses over 12 ticks");

    let final_hash = decode_dec_bytes_literal(lines[1]);
    let witnesses_json_str = decode_dec_bytes_literal(lines[2]);
    let state_json_str = decode_dec_bytes_literal(lines[3]);

    // `final_state_hash` is the exact SHA-256 of canonical `state_json`
    assert_eq!(
        Sha256::digest_hex(state_json_str.as_bytes()),
        final_hash,
        "final_state_hash must equal SHA-256(state_json)"
    );

    let witnesses_val = parse_strict_json(&witnesses_json_str).unwrap();
    let state_val = parse_strict_json(&state_json_str).unwrap();
    let state_shedding = state_val
        .get("shedding_events")
        .expect("state_json must serialize shedding_events array");
    assert_eq!(
        state_shedding, &witnesses_val,
        "state_json.shedding_events must match shedding_witnesses_json field-for-field"
    );

    let arr = witnesses_val.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    // Witness 0 (tick 3, 1 bound contact) routes Exo; Witness 1 (tick 11, >=2 contacts, zero drift) routes Endo.
    assert_eq!(arr[0].get("route").unwrap().as_str(), Some("exo"));
    assert_eq!(arr[1].get("route").unwrap().as_str(), Some("endo"));

    for w in arr {
        for required_key in [
            "cut_coordinate",
            "emit_boundary",
            "emit_cut",
            "parent_id",
            "post_shed_parent_state",
            "route",
            "x_route",
        ] {
            assert!(
                w.get(required_key).is_some(),
                "missing ShedWitness field `{required_key}` in {w:?}"
            );
        }
        let parent_state = w.get("post_shed_parent_state").unwrap().as_str().unwrap();
        let parts: Vec<&str> = parent_state.split(':').collect();
        assert_eq!(parts.len(), 2, "expected 2 SHA-256 parent digests: {parent_state}");
        assert!(parts.iter().all(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit())));
    }

    // Direct single-witness mutation check: mutating any single witness field while keeping
    // `shedding_events.len()`, carrier traces, wavelet records, and scalar totals identical
    // MUST change `state_hash()`.
    let support = SupportForm::parse("3:3:6").unwrap();
    let c0 = CarrierState::new(
        1,
        "WitnessBox",
        support.clone(),
        1,
        [1, 4, 4],
        AxisDir::PosX,
        T1::POS,
    )
    .unwrap();
    let c1 = CarrierState::new(
        2,
        "WitnessBox",
        support,
        1,
        [7, 4, 4],
        AxisDir::NegX,
        T1::NEG,
    )
    .unwrap();
    let mut base_state = ChamberState::new(
        "WitnessBox",
        BoxRegion {
            x: (0, 9),
            y: (0, 9),
            z: (0, 9),
        },
        WallPolicy::Closed,
        vec![c0, c1],
        Vec::new(),
    )
    .with_dynamics(ChamberDynamicsConfig {
        boundary_step: [0, 0, 0],
        contact_ttl: Some(20),
        fuse_on_resonance: false,
    });
    for _ in 0..12 {
        base_state.step().unwrap();
    }
    assert_eq!(base_state.state_hash(), final_hash);
    assert_eq!(base_state.shedding_events.len(), 2);

    // (a) Mutate only `route` on witness 0
    let mut m_route = base_state.clone();
    m_route.shedding_events[0].route = ShedRoute::Redir;
    assert_eq!(m_route.shedding_events.len(), base_state.shedding_events.len());
    assert_eq!(m_route.wavelet_events, base_state.wavelet_events);
    assert_ne!(
        m_route.state_hash(),
        final_hash,
        "mutating ShedWitness.route must alter state_hash()"
    );

    // (b) Mutate only `post_shed_parent_state` on witness 0
    let mut m_parent = base_state.clone();
    m_parent.shedding_events[0].post_shed_parent_state = format!(
        "{}:{}",
        "0".repeat(64),
        "f".repeat(64)
    );
    assert_eq!(m_parent.shedding_events.len(), base_state.shedding_events.len());
    assert_ne!(
        m_parent.state_hash(),
        final_hash,
        "mutating ShedWitness.post_shed_parent_state must alter state_hash()"
    );

    // (c) Mutate only `cut_coordinate` on witness 0
    let mut m_cut = base_state.clone();
    m_cut.shedding_events[0].cut_coordinate = "cell_[4,4,5]_tick_3".to_string();
    assert_ne!(
        m_cut.state_hash(),
        final_hash,
        "mutating ShedWitness.cut_coordinate must alter state_hash()"
    );

    // (d) Mutate only `emit_boundary` on witness 0
    let mut m_bnd = base_state.clone();
    m_bnd.shedding_events[0].emit_boundary = "∂box([0..10, 0..9, 0..9])".to_string();
    assert_ne!(
        m_bnd.state_hash(),
        final_hash,
        "mutating ShedWitness.emit_boundary must alter state_hash()"
    );
}

#[test]
fn blocker2_raw_dec_moving_boundary_contact_expiry_population_resonance_and_equal_scalar_divergence()
{
    // 1. Moving boundary (`eq:moving-boundary`: Phi_mov = Phi_cross - Phi_sweep + Phi_shedding)
    //    plus Open-wall population drop (`active_carrier_count < carrier_count`, `"route":"open"`).
    let moving_src = r#"chamber MovingOpenBox:
    space = box([0..12, 0..9, 0..9])
    walls = open
    boundary_step = (2, 0, 0)

chamber StaticOpenBox:
    space = box([0..12, 0..9, 0..9])
    walls = open

prepare ThreeTermPrep in MovingOpenBox:
    inject 1:2:4 at (1, 4, 4) dir +X orient +1t shells 1
    inject 3:2:10 at (6, 8, 4) dir +Y orient +1t shells 1
    inject 1:2:4 at (5, 4, 4) dir +X orient +1t shells 1
    inject 1:2:4 at (7, 4, 4) dir -X orient -1t shells 1

prepare SweepVsStaticMov in MovingOpenBox:
    inject 3:3:6 at (1, 4, 4) dir +X orient +1t shells 1

prepare SweepVsStaticRef in StaticOpenBox:
    inject 3:3:6 at (1, 4, 4) dir +X orient +1t shells 1

dec main() -> int:
    let r = simulate ThreeTermPrep over 1 ticks
    let r_mov = simulate SweepVsStaticMov over 2 ticks
    let r_ref = simulate SweepVsStaticRef over 2 ticks
    emit r.carrier_count
    emit r.active_carrier_count
    emit r.moving_flux_total
    emit r.shedding_event_count
    emit r.state_json
    emit r_mov.active_carrier_count
    emit r_mov.moving_flux_total
    emit r_ref.active_carrier_count
    emit r_ref.moving_flux_total
    0
"#;
    let mov_out = run_6_modes(moving_src, "blocker2_moving_boundary");
    let mlines: Vec<&str> = mov_out.lines().collect();
    assert_eq!(mlines[0], "4", "4 injected carriers");
    assert_eq!(mlines[1], "2", "2 carriers exited open boundary, 2 remain active");
    // cross_flux = Pi(3:2:10) = 10; sweep_flux = Pi(1:2:4) = 4; shedding_flux = 3*min(RD=9, w=1) = 3 -> 10 - 4 + 3 = 9
    assert_eq!(mlines[2], "9", "Phi_mov = 10 - 4 + 3 = 9");
    assert_eq!(mlines[3], "3", "1 sweep Open + 1 cross Open + 1 coincidence Exo");
    let mov_state = parse_strict_json(&decode_dec_bytes_literal(mlines[4])).unwrap();
    let bm0 = &mov_state.get("boundary_motions").unwrap().as_array().unwrap()[0];
    assert_eq!(bm0.get("cross_flux").unwrap().as_int().unwrap().to_decimal_string(), "10");
    assert_eq!(bm0.get("sweep_flux").unwrap().as_int().unwrap().to_decimal_string(), "4");
    assert_eq!(bm0.get("shedding_flux").unwrap().as_int().unwrap().to_decimal_string(), "3");
    assert_eq!(bm0.get("net_moving_flux").unwrap().as_int().unwrap().to_decimal_string(), "9");
    let mov_sheds = mov_state.get("shedding_events").unwrap().as_array().unwrap();
    assert_eq!(mov_sheds[0].get("route").unwrap().as_str(), Some("open"));
    assert_eq!(mov_sheds[1].get("route").unwrap().as_str(), Some("open"));
    assert_eq!(mov_sheds[2].get("route").unwrap().as_str(), Some("exo"));
    // Swept continuation vs static box continuation on identical initial carrier
    assert_eq!(mlines[5], "0", "swept carrier deactivated in moving box");
    assert_eq!(mlines[6], "-30", "swept Pi(3:3:6)=30 recorded as -30 sweep flux");
    assert_eq!(mlines[7], "1", "carrier remains active in static box");
    assert_eq!(mlines[8], "0", "zero moving flux in static box");

    // 2. Contact expiry (`contact_ttl = 2` vs `contact_ttl = 20`) & Resonance (`Endo` vs `Exo`)
    //    plus Population Fusion (`fuse_on_resonance = true` retaining both inner parent Traces).
    let window_and_fusion_src = r#"chamber ShortTtlBox:
    space = box([0..9, 0..9, 0..9])
    walls = closed
    contact_ttl = 2

chamber WideTtlBox:
    space = box([0..9, 0..9, 0..9])
    walls = closed
    contact_ttl = 20

chamber FuseBox:
    space = box([0..9, 0..9, 0..9])
    walls = closed
    fuse_on_resonance = true

prepare ShortPair in ShortTtlBox:
    inject 3:3:6 at (1, 4, 4) dir +X orient +1t shells 1
    inject 3:3:6 at (7, 4, 4) dir -X orient -1t shells 1

prepare WidePair in WideTtlBox:
    inject 3:3:6 at (1, 4, 4) dir +X orient +1t shells 1
    inject 3:3:6 at (7, 4, 4) dir -X orient -1t shells 1

prepare FusePair in FuseBox:
    inject 3:3:6 at (1, 4, 4) dir +X orient +1t shells 1
    inject 3:3:6 at (7, 4, 4) dir -X orient -1t shells 1

dec main() -> int:
    let r_short = simulate ShortPair over 12 ticks
    let r_wide = simulate WidePair over 12 ticks
    let r_fuse = simulate FusePair over 6 ticks
    emit r_short.expired_contact_count
    emit r_short.resonance_count
    emit r_short.state_json
    emit r_wide.expired_contact_count
    emit r_wide.resonance_count
    emit r_wide.state_json
    emit r_fuse.carrier_count
    emit r_fuse.active_carrier_count
    emit r_fuse.fusion_count
    emit r_fuse.state_json
    0
"#;
    let wf_out = run_6_modes(window_and_fusion_src, "blocker2_window_and_fusion");
    let wlines: Vec<&str> = wf_out.lines().collect();
    let short_expired: usize = wlines[0].parse().unwrap();
    assert!(short_expired >= 1, "short contact_ttl=2 must expire contact window");
    assert_eq!(wlines[1], "0", "expired window cannot certify 2-contact resonance");
    let short_state = parse_strict_json(&decode_dec_bytes_literal(wlines[2])).unwrap();
    let exp0 = &short_state.get("expired_contacts").unwrap().as_array().unwrap()[0];
    assert_eq!(exp0.get("stage_only").unwrap().as_bool(), Some(true));
    let short_sheds = short_state.get("shedding_events").unwrap().as_array().unwrap();
    assert_eq!(short_sheds[1].get("route").unwrap().as_str(), Some("exo"));

    assert_eq!(wlines[3], "0", "wide contact_ttl=20 retains contact window");
    assert_eq!(wlines[4], "1", "retained window certifies 2-contact zero-drift resonance");
    let wide_state = parse_strict_json(&decode_dec_bytes_literal(wlines[5])).unwrap();
    let res0 = &wide_state.get("resonances").unwrap().as_array().unwrap()[0];
    assert_eq!(res0.get("drift").unwrap().as_int().unwrap().to_decimal_string(), "0");
    let wide_sheds = wide_state.get("shedding_events").unwrap().as_array().unwrap();
    assert_eq!(wide_sheds[1].get("route").unwrap().as_str(), Some("endo"));

    // Composite fusion (`fuse_on_resonance = true`)
    assert_eq!(wlines[6], "3", "2 inner carriers + 1 seated outer fused carrier");
    assert_eq!(wlines[7], "1", "both inner carriers deactivated, 1 composite outer active");
    assert_eq!(wlines[8], "1", "1 certified FusionEventRecord");
    let fuse_state = parse_strict_json(&decode_dec_bytes_literal(wlines[9])).unwrap();
    let carriers_json = fuse_state.get("carriers").unwrap().as_array().unwrap();
    assert_eq!(carriers_json.len(), 3);
    assert_eq!(carriers_json[0].get("active").unwrap().as_bool(), Some(false));
    assert_eq!(carriers_json[1].get("active").unwrap().as_bool(), Some(false));
    assert_eq!(carriers_json[2].get("active").unwrap().as_bool(), Some(true));
    let fused_parents = carriers_json[2]
        .get("trace")
        .unwrap()
        .get("parents")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(
        fused_parents.len(),
        2,
        "composite outer carrier must retain both inner parent Trace histories"
    );

    // 3. Coupled histories with EQUAL displayed scalar totals that DIVERGE in retained raw-D.E.C.
    //    carrier traces, couplings, resonances, shedding witnesses, and final_state_hash!
    let chiral_divergence_src = r#"chamber CoupledChamber:
    space = box([0..9, 0..9, 0..9])
    walls = closed
    contact_ttl = 20

prepare ChiralA in CoupledChamber:
    inject 3:3:6 at (1, 4, 4) dir +X orient +1t shells 1
    inject 3:3:6 at (7, 4, 4) dir -X orient -1t shells 1

prepare ChiralB in CoupledChamber:
    inject 3:3:6 at (1, 4, 4) dir +X orient -1t shells 1
    inject 3:3:6 at (7, 4, 4) dir -X orient +1t shells 1

dec main() -> int:
    let a = simulate ChiralA over 12 ticks
    let b = simulate ChiralB over 12 ticks
    assert a.ticks == b.ticks
    assert a.carrier_count == b.carrier_count
    assert a.active_carrier_count == b.active_carrier_count
    assert a.total_returns == b.total_returns
    assert a.total_hinges == b.total_hinges
    assert a.shedding_event_count == b.shedding_event_count
    assert a.wavelet_event_count == b.wavelet_event_count
    assert a.resonance_count == b.resonance_count
    assert a.expired_contact_count == b.expired_contact_count
    assert a.fusion_count == b.fusion_count
    assert a.moving_flux_total == b.moving_flux_total
    assert a.final_state_hash != b.final_state_hash
    assert a.shedding_witnesses_json != b.shedding_witnesses_json
    assert a.state_json != b.state_json
    emit a.total_returns
    emit b.total_returns
    emit a.shedding_event_count
    emit b.shedding_event_count
    emit a.resonance_count
    emit b.resonance_count
    emit a.final_state_hash
    emit b.final_state_hash
    0
"#;
    let div_out = run_6_modes(chiral_divergence_src, "blocker2_equal_scalar_divergence");
    let dlines: Vec<&str> = div_out.lines().collect();
    assert_eq!(dlines[0], dlines[1], "equal total_returns");
    assert_eq!(dlines[2], dlines[3], "equal shedding_event_count");
    assert_eq!(dlines[4], dlines[5], "equal resonance_count");
    assert_ne!(
        dlines[6], dlines[7],
        "equal-scalar chiral histories must have distinct final_state_hash"
    );
}

#[test]
fn blocker3_v1_compat_cli_and_migrate_v1_and_pr8_sources() {
    let root = repo_root();
    let v1_dir = root.join("tests/fixtures/v1_examples");

    // 1. All 10 frozen `v1` examples migrate cleanly to canonical Pure-Wave `v3`,
    //    pass `check_source`, `format_source` idempotence, and 6-hook `audit_dec_source`.
    for entry in std::fs::read_dir(&v1_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("dec") {
            continue;
        }
        let migrated = migrate_file_formatted(&path)
            .unwrap_or_else(|e| panic!("migrate_file_formatted failed on {}: {e}", path.display()));
        check_source(&migrated)
            .unwrap_or_else(|e| panic!("migrated v1 source failed v3 check ({}): {e}", path.display()));
        let reformatted = format_source(&migrated).unwrap();
        assert_eq!(reformatted, migrated, "migrated output must already be fmt-canonical");
        let audit = audit_dec_source(&migrated);
        assert!(
            audit.passed(),
            "migrated v1 source failed 6-hook audit ({}): status={}",
            path.display(),
            audit.status
        );
    }

    // 2. Representative PR #8 (`68d6705`) source combining legacy `Dec`, `t32`, `str`, `"..."`,
    //    brace `struct`/`enum`, `if`/`else`, `for i in 0..4:`, `rec.stage == "reclosed"`,
    //    and `r0.model_status == "DECLARED_..."` migrates to Pure-Wave `v3` and executes identically
    //    across all 6 backends!
    let pr8_legacy_src = r#"use aod::wavelet::{recipient_eval_demo}
use aod::qext::{ext_decimal, qext_render}

struct Tally {
    count: t32,
    label: str,
}

enum GateState {
    Open,
    Closed(t32),
}

chamber LegacyBox:
    space = box([0..32, 0..32, 0..32])
    walls = open

prepare LegacyPrep in LegacyBox:
    inject 3:3:6 at (8, 16, 16) dir +X orient +1t shells 1

Dec accumulate(limit: t32) -> t32:
    t32 acc = t32(0)
    for i in 0..4:
        if i < limit:
            acc = acc + i
        else:
            acc = acc + t32(1)
    return acc

Dec main() -> t32:
    let t = Tally { count: accumulate(t32(3)), label: "ok" }
    let _g = GateState::Closed(t.count)
    let rec_ok = recipient_eval_demo(t32(5), t32(6), (4, 10])
    assert rec_ok.stage == "reclosed"
    let r0 = simulate LegacyPrep over 2 ticks
    assert r0.model_status == "DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED"
    emit t.count
    emit t.label
    quarantine Q_ext:
        let c = ext_decimal("1420405751.7679")
        emit qext_render(c)
    return t32(0)
"#;
    let migrated_pr8 = migrate_source_formatted(pr8_legacy_src)
        .unwrap_or_else(|e| panic!("PR #8 sample migration failed: {e}"));
    let audit_pr8 = audit_dec_source(&migrated_pr8);
    assert!(audit_pr8.passed(), "migrated PR #8 sample failed audit: {migrated_pr8}");
    let out_pr8 = run_6_modes(&migrated_pr8, "blocker3_pr8_migrated");
    let plines: Vec<&str> = out_pr8.lines().collect();
    // i=0 (<3): +0, i=1 (<3): +1, i=2 (<3): +2, i=3 (>=3): +1 -> acc = 4
    assert_eq!(plines[0], "4");
    assert_eq!(plines[1], "b\"ok\"");
    assert_eq!(plines[2], "Q_ext(b\"14204057517679/10000\")");
    assert_eq!(plines[3], "0");

    // 3. CLI `aodc migrate` modes (`-o`, `--write`, `--check`, and hostile guard exit 2)
    let bin = env!("CARGO_BIN_EXE_aodc");
    let dir = fresh_temp_dir("blocker3_cli_migrate");
    let work_file = dir.join("legacy.dec");
    let out_file = dir.join("upgraded.dec");
    std::fs::write(&work_file, pr8_legacy_src).unwrap();

    // `--check` exits non-zero on unmigrated legacy input
    let chk_before = Command::new(bin)
        .args(["migrate", "--check", work_file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!chk_before.status.success(), "--check must fail before migration");

    // `-o upgraded.dec` writes migrated Pure-Wave v3 output without mutating input
    let to_out = Command::new(bin)
        .args([
            "migrate",
            work_file.to_str().unwrap(),
            "-o",
            out_file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(to_out.status.success(), "migrate -o failed: {:?}", to_out);
    assert_eq!(std::fs::read_to_string(&out_file).unwrap(), migrated_pr8);

    // `--write` rewrites working copy in place, after which `--check` exits 0
    let write_in_place = Command::new(bin)
        .args(["migrate", "--write", work_file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(write_in_place.status.success(), "migrate --write failed");
    let chk_after = Command::new(bin)
        .args(["migrate", "--check", work_file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(chk_after.status.success(), "--check must exit 0 after --write");

    // Refuses `--write` on frozen `tests/fixtures/v1_examples/weights.dec` (exit 2) and on symlinks (exit 2)
    let frozen_weights = v1_dir.join("weights.dec");
    let refuse_frozen = Command::new(bin)
        .args(["migrate", "--write", frozen_weights.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(refuse_frozen.status.code(), Some(2));

    #[cfg(unix)]
    {
        let link_path = dir.join("sym.dec");
        std::os::unix::fs::symlink(&work_file, &link_path).unwrap();
        let refuse_sym = Command::new(bin)
            .args(["migrate", "--write", link_path.to_str().unwrap()])
            .output()
            .unwrap();
        assert_eq!(refuse_sym.status.code(), Some(2));
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn blocker4_nested_git_repository_manual_data_verify_s2_and_write_guards() {
    let root = repo_root();
    let bin = env!("CARGO_BIN_EXE_aodc");
    let tmp = fresh_temp_dir("blocker4_nested_repo");

    // Create an outer parent repository containing `.git` (simulating upstream `aod/`)
    let outer = tmp.join("outer_aod_repo");
    std::fs::create_dir_all(outer.join(".git")).unwrap();
    std::fs::write(outer.join("README.md"), "outer repo sentinel\n").unwrap();

    // Test both `<outer>/aod-dec` and `<outer>/vendor/aod-dec` nested layouts
    for rel_nested in ["aod-dec", "vendor/aod-dec"] {
        let nested = outer.join(rel_nested);
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::copy(root.join("Cargo.toml"), nested.join("Cargo.toml")).unwrap();
        std::fs::copy(
            root.join("SOURCE_BINDING.json"),
            nested.join("SOURCE_BINDING.json"),
        )
        .unwrap();
        for dir_name in ["bindings", "docs", "manual_examples", "stdlib"] {
            copy_dir_recursive(&root.join(dir_name), &nested.join(dir_name));
        }

        // 1. Read-only `aodc manual-data --check` from inside nested repo MUST exit 0
        let chk_inner = Command::new(bin)
            .current_dir(&nested)
            .args(["manual-data", "--check"])
            .output()
            .unwrap();
        assert!(
            chk_inner.status.success(),
            "[{rel_nested}] manual-data --check from inner cwd failed (code {:?}):\nstdout: {}\nstderr: {}",
            chk_inner.status.code(),
            String::from_utf8_lossy(&chk_inner.stdout),
            String::from_utf8_lossy(&chk_inner.stderr),
        );

        // 2. Read-only `aodc manual-data --check --output-dir <rel>/docs/manual/data` from OUTER cwd MUST exit 0
        let rel_data = format!("{rel_nested}/docs/manual/data");
        let chk_outer = Command::new(bin)
            .current_dir(&outer)
            .args(["manual-data", "--check", "--output-dir", &rel_data])
            .output()
            .unwrap();
        assert!(
            chk_outer.status.success(),
            "[{rel_nested}] manual-data --check from outer cwd failed (code {:?}):\nstderr: {}",
            chk_outer.status.code(),
            String::from_utf8_lossy(&chk_outer.stderr),
        );

        // 3. Write mode into `<nested>/docs/manual/data` and `<nested>/_build/manual_data` MUST exit 0
        let write_default = Command::new(bin)
            .current_dir(&nested)
            .args(["manual-data"])
            .output()
            .unwrap();
        assert!(write_default.status.success(), "[{rel_nested}] manual-data write failed");

        let write_build = Command::new(bin)
            .current_dir(&nested)
            .args(["manual-data", "--output-dir", "_build/manual_data"])
            .output()
            .unwrap();
        assert!(
            write_build.status.success(),
            "[{rel_nested}] manual-data --output-dir _build/manual_data failed: {}",
            String::from_utf8_lossy(&write_build.stderr)
        );

        // 4. `verify-s2 --s4-report --report _build/s4_report.json` inside nested repo MUST exit 0
        let s4_ok = Command::new(bin)
            .current_dir(&nested)
            .args(["verify-s2", "--s4-report", "--report", "_build/s4_report.json"])
            .output()
            .unwrap();
        assert!(
            s4_ok.status.success(),
            "[{rel_nested}] verify-s2 --s4-report in nested repo failed: {}",
            String::from_utf8_lossy(&s4_ok.stderr)
        );
    }

    // 5. Write protections on both `<outer>` and `<outer>/aod-dec` remain strictly enforced (exit 2):
    let inner = outer.join("aod-dec");

    // (a) Reject writing `docs/manual/data` into the OUTER parent repo (`<outer>/docs/manual/data`)
    let bad_outer_data = Command::new(bin)
        .current_dir(&inner)
        .args([
            "manual-data",
            "--output-dir",
            outer.join("docs/manual/data").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(bad_outer_data.status.code(), Some(2));
    assert!(!outer.join("docs").exists());

    // (b) Reject writing into `<inner>/crates`
    let bad_inner_crates = Command::new(bin)
        .current_dir(&inner)
        .args(["manual-data", "--output-dir", "crates"])
        .output()
        .unwrap();
    assert_eq!(bad_inner_crates.status.code(), Some(2));

    // (c) Reject overwriting `<outer>/README.md` via `--report`
    let bad_outer_report = Command::new(bin)
        .current_dir(&inner)
        .args([
            "verify-s2",
            "--s4-report",
            "--report",
            outer.join("README.md").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(bad_outer_report.status.code(), Some(2));
    assert_eq!(
        std::fs::read_to_string(outer.join("README.md")).unwrap(),
        "outer repo sentinel\n"
    );

    // (d) Reject symlinked `--output-dir` inside `<inner>/_build`
    #[cfg(unix)]
    {
        let outside_victim = tmp.join("outside_victim");
        std::fs::create_dir_all(&outside_victim).unwrap();
        let sym_out = inner.join("_build/sym_escape");
        std::os::unix::fs::symlink(&outside_victim, &sym_out).unwrap();
        let bad_sym = Command::new(bin)
            .current_dir(&inner)
            .args(["manual-data", "--output-dir", "_build/sym_escape"])
            .output()
            .unwrap();
        assert_eq!(bad_sym.status.code(), Some(2));
    }

    let _ = std::fs::remove_dir_all(&tmp);
}
