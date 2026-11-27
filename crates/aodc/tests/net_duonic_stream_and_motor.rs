#![forbid(unsafe_code)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use aodc::diagnostics::DiagnosticCode;

static TEMP_SEQ: AtomicU64 = AtomicU64::new(1);

fn aodc_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aodc"))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn stdlib_dir() -> PathBuf {
    repo_root().join("stdlib")
}

struct TempDirGuard(PathBuf);

impl TempDirGuard {
    fn new(prefix: &str) -> Self {
        let id = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "aod_dec_net_test_{}_{}_{}",
            prefix,
            std::process::id(),
            id
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp test dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run_3mode(dec_file: &Path, mounts: &[&str], stdin_bytes: Option<&[u8]>) -> String {
    let stdlib = stdlib_dir();
    let tmp = TempDirGuard::new("m3");
    let aot_bin = tmp.path().join("prog_aot");

    let build_out = Command::new(aodc_bin())
        .arg("build")
        .arg("-O2")
        .arg("--stdlib")
        .arg(&stdlib)
        .arg(dec_file)
        .arg("-o")
        .arg(&aot_bin)
        .output()
        .expect("spawn aodc build");
    assert!(
        build_out.status.success(),
        "aodc build failed:\n{}",
        String::from_utf8_lossy(&build_out.stderr)
    );

    let invoke = |cmd_kind: &str| -> Vec<u8> {
        let mut cmd = if cmd_kind == "aot" {
            let mut c = Command::new(&aot_bin);
            c.arg("--fuel").arg("5000000");
            for m in mounts {
                c.arg("--mount").arg(m);
            }
            c
        } else {
            let mut c = Command::new(aodc_bin());
            c.arg(cmd_kind);
            if cmd_kind == "jit" {
                c.arg("-O2");
            }
            c.arg("--fuel")
                .arg("5000000")
                .arg("--stdlib")
                .arg(&stdlib);
            for m in mounts {
                c.arg("--mount").arg(m);
            }
            c.arg(dec_file);
            c
        };
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        if stdin_bytes.is_some() {
            cmd.stdin(Stdio::piped());
        } else {
            cmd.stdin(Stdio::null());
        }
        let mut child = cmd.spawn().expect("spawn mode");
        if let Some(in_b) = stdin_bytes {
            let mut sin = child.stdin.take().unwrap();
            sin.write_all(in_b).expect("write stdin");
        }
        let out = child.wait_with_output().expect("wait child");
        assert!(
            out.status.success(),
            "{cmd_kind} failed (code {:?}):\nstderr: {}\nstdout: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        );
        out.stdout
    };

    let vm_out = invoke("run");
    let jit_out = invoke("jit");
    let aot_out = invoke("aot");
    assert_eq!(vm_out, jit_out, "VM vs JIT -O2 output mismatch");
    assert_eq!(vm_out, aot_out, "VM vs AOT -O2 output mismatch");
    String::from_utf8(vm_out).expect("utf8 output")
}

#[test]
fn duonic_sensor_motor_stdlib_3mode_parity() {
    let tmp = TempDirGuard::new("stdlib_parity");
    let src_path = tmp.path().join("duonic_unit.dec");
    let src = r#"use aod::core:
    b8, push_byte, slice_bytes
use stdlib::aod::net:
    DuonicPulse, DuonicWirePacket, MotorSlot, admit_sensor_pulse, decode_duonic_packet, encode_duonic_packet, format_ok_ints, measure_cross_return, pack_trit_lanes, parse_i64_bytes, parse_i64_csv, select_motor_contact, split_ascii_words, unpack_trit_lanes

dec main:
    () -> int:
        let cur = admit_sensor_pulse(DuonicPulse(2, -1t, 3, 21, 9, 2/3r))
        assert cur.rho_clipped == 9
        assert cur.pressure == 27
        assert cur.overhang == 36
        assert cur.sadar_flux == -18/1r

        let cr = measure_cross_return(1t, 1t)
        assert cr.isolated_mark == 1 and cr.isolated_pulse == 1 and cr.joint_return == 1
        assert cr.residual == -1 and cr.deficit == 1

        let slots = [
            MotorSlot(0, false, 10, 99/1r),
            MotorSlot(1, true, 3, 1/1r),
            MotorSlot(2, true, 5, 4/1r),
        ]
        let dec_m = select_motor_contact(slots, 1/1r, 1/1r)
        assert dec_m.selected_slot == 2
        assert dec_m.removed_slot == 1
        assert dec_m.restored_slot == 2
        assert dec_m.changed_by_carriage
        assert dec_m.admissible_count == 2
        assert dec_m.slide_prob == 5/8r
        assert dec_m.actuated_sigma == 1t
        assert dec_m.actuated_current == 4/1r

        let trits = [0t, 1t, -1t, 1t, -1t]
        let packed = pack_trit_lanes(trits)
        assert packed == b"\x64\x02"
        let (ok_t, unp) = unpack_trit_lanes(packed, 5)
        assert ok_t and unp == trits
        let (bad_pad, _) = unpack_trit_lanes(b"\x64\x06", 5)
        assert not bad_pad
        let (bad_lane, _) = unpack_trit_lanes(b"\x03", 1)
        assert not bad_lane

        let pkt = DuonicWirePacket(1, 42, 7, -1t, trits, -13/37r)
        let wire = encode_duonic_packet(pkt)
        let (ok_p, rt) = decode_duonic_packet(wire)
        assert ok_p
        assert rt.kind == 1 and rt.cut == 42 and rt.channel == 7
        assert rt.sigma == -1t and rt.signals == trits and rt.current == -13/37r

        let bad_magic = push_byte(slice_bytes(wire, 1, 42), b8(0))
        let (ok_bad, _) = decode_duonic_packet(bad_magic)
        assert not ok_bad

        let words = split_ascii_words(b"  teach  2  99  -2 \r\n")
        assert words == [b"teach", b"2", b"99", b"-2"]
        let (ok_i, iv) = parse_i64_bytes(b"-99")
        assert ok_i and iv == -99
        let (bad_i, _) = parse_i64_bytes(b"NaN")
        assert not bad_i
        let (ok_csv, csv) = parse_i64_csv(b"0,1,-2", 8)
        assert ok_csv and csv == [0, 1, -2]
        assert format_ok_ints([1, -2, 0]) == b"OK 1 -2 0\n"

        emit dec_m.selected_slot
        emit dec_m.removed_slot
        emit dec_m.slide_prob
        emit cur.sadar_flux
        return 0
"#;
    std::fs::write(&src_path, src).unwrap();
    let out = run_3mode(&src_path, &[], None);
    assert_eq!(out, "2\n1\n5/8\n-18/1\n0\n");
}

#[test]
fn stdio_and_unix_stream_line_recovery_3mode_parity() {
    let tmp = TempDirGuard::new("stdio_unix");
    let server_path = tmp.path().join("line_echo.dec");
    let server_src = r#"use aod::core:
    concat_bytes, select, settle
use aod::sys:
    read_stream_line, write_stream_bytes
use stdlib::aod::net:
    format_ok_ints, parse_i64_bytes, split_ascii_words

struct LoopState:
    active: bool
    turns: int
    accum: int

dec handle_words:
    (st: LoopState, words: [bytes]) where words == [b"quit"] -> LoopState:
        let _w = write_stream_bytes(b"/net/bus", b"BYE\n")
        return LoopState(false, st.turns + 1, st.accum)
    (st: LoopState, words: [bytes]) where len(words) == 2 and words[0] == b"add" -> LoopState:
        let (ok, v) = parse_i64_bytes(words[1])
        let next_acc = select(ok, st.accum + v, st.accum)
        let reply = select(ok, format_ok_ints([next_acc, st.turns + 1]), b"ERR invalid_int\n")
        let _w = write_stream_bytes(b"/net/bus", reply)
        return LoopState(true, st.turns + 1, next_acc)
    (st: LoopState, _: [bytes]) -> LoopState:
        let _w = write_stream_bytes(b"/net/bus", b"ERR bad_cmd\n")
        return LoopState(true, st.turns + 1, st.accum)

dec step_session:
    (st: LoopState) where not st.active -> LoopState:
        return st
    (st: LoopState) -> LoopState:
        let (status, raw) = read_stream_line(b"/net/bus", 32)
        return select(
            status == 0t,
            LoopState(false, st.turns, st.accum),
            select(
                status == -1t,
                on_overflow(st, raw),
                handle_words(st, split_ascii_words(raw))
            )
        )

dec on_overflow(st: LoopState, reason: bytes) -> LoopState:
    let _w = write_stream_bytes(b"/net/bus", concat_bytes(b"ERR ", concat_bytes(reason, b"\n")))
    return LoopState(true, st.turns + 1, st.accum)

dec main:
    () -> ():
        let _fin = settle(LoopState(true, 0, 0), 1000, step_session)
"#;
    std::fs::write(&server_path, server_src).unwrap();

    let session_input = concat!(
        "add 12\n",
        "add 0123456789012345678901234567890123456789\n",
        "add NaN\n",
        "add -5\n",
        "quit\n"
    )
    .as_bytes();
    let stdout = run_3mode(&server_path, &["stdio:/net/bus"], Some(session_input));
    assert_eq!(
        stdout,
        "OK 12 1\nERR line exceeds max_bytes\nERR invalid_int\nOK 7 4\nBYE\n"
    );

    // Also verify Unix domain socket listener + client round-trip under standalone AOT sandbox!
    let sock_path = tmp.path().join("bus.sock");
    let aot_server = tmp.path().join("server_aot");
    let build_out = Command::new(aodc_bin())
        .arg("build")
        .arg("-O2")
        .arg("--stdlib")
        .arg(stdlib_dir())
        .arg(&server_path)
        .arg("-o")
        .arg(&aot_server)
        .output()
        .unwrap();
    assert!(build_out.status.success());

    let mount_listen = format!("unix-listen:{}:/net/bus", sock_path.display());
    let srv = Command::new(&aot_server)
        .arg("--mount")
        .arg(&mount_listen)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn unix server");

    for _ in 0..100 {
        if sock_path.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(sock_path.exists(), "unix listener socket was not created");

    let mut client = std::os::unix::net::UnixStream::connect(&sock_path).unwrap();
    client.write_all(b"add 40\nadd 2\nquit\n").unwrap();
    let mut reply = String::new();
    let mut rdr = BufReader::new(client);
    for expected in ["OK 40 1\n", "OK 42 2\n", "BYE\n"] {
        reply.clear();
        rdr.read_line(&mut reply).unwrap();
        assert_eq!(reply, expected);
    }
    let srv_out = srv.wait_with_output().unwrap();
    assert!(srv_out.status.success());
}

#[test]
fn tcp_duonic_binary_sensor_motor_closed_loop_across_vm_jit_aot() {
    let tmp = TempDirGuard::new("tcp_duonic");
    let controller_path = tmp.path().join("controller.dec");
    let plant_path = tmp.path().join("plant.dec");

    // Closed-loop 2-slot motor controller listening on `/net/bus` over binary DN01 frames,
    // serving TWO sequential TCP client episodes on the SAME retained carriage state!
    let controller_src = r#"use aod::core:
    concat_bytes, ratio, select, set_at, settle
use aod::sys:
    accept_stream, read_stream_frame, stream_local_addr, write_stream_bytes, write_stream_frame
use stdlib::aod::net:
    DuonicWirePacket, MotorSlot, decode_duonic_packet, encode_duonic_packet, measure_cross_return, select_motor_contact

struct CtrlState:
    active: bool
    turns: int
    totals: [int]

dec step_client:
    (st: CtrlState) where not st.active -> CtrlState:
        return st
    (st: CtrlState) -> CtrlState:
        let (status, raw) = read_stream_frame(b"/net/bus", 4096)
        return select(status != 1t, CtrlState(false, st.turns, st.totals), dispatch_pkt(st, raw))

dec dispatch_pkt(st: CtrlState, raw: bytes) -> CtrlState:
    let (ok, pkt) = decode_duonic_packet(raw)
    assert ok
    return select(pkt.kind == 1, reply_motor(st, pkt), commit_terminal(st, pkt))

dec reply_motor(st: CtrlState, pkt: DuonicWirePacket) -> CtrlState:
    let adm0 = pkt.signals[0] == 0t
    let adm1 = pkt.signals[1] == 0t
    let slots = [
        MotorSlot(0, adm0, 1, ratio(st.totals[0], 1)),
        MotorSlot(1, adm1, 1, ratio(st.totals[1], 1)),
    ]
    let m = select_motor_contact(slots, 1/1r, 1/1r)
    let changed_trit = select(m.changed_by_carriage, 1t, 0t)
    let out_pkt = DuonicWirePacket(
        2,
        pkt.cut + 1,
        m.selected_slot,
        m.actuated_sigma,
        [changed_trit],
        m.actuated_current
    )
    let _w = write_stream_frame(b"/net/bus", encode_duonic_packet(out_pkt))
    return CtrlState(true, st.turns + 1, st.totals)

dec commit_terminal(st: CtrlState, pkt: DuonicWirePacket) -> CtrlState:
    let r0 = measure_cross_return(pkt.signals[0], pkt.sigma)
    let r1 = measure_cross_return(pkt.signals[1], pkt.sigma)
    let next_totals = set_at(set_at(st.totals, 0, st.totals[0] + r0.deficit), 1, st.totals[1] + r1.deficit)
    let ack = DuonicWirePacket(3, pkt.cut + 1, 0, pkt.sigma, [0t], ratio(next_totals[1], 1))
    let _w = write_stream_frame(b"/net/bus", encode_duonic_packet(ack))
    return CtrlState(false, st.turns + 1, next_totals)

dec main:
    () -> ():
        let addr = stream_local_addr(b"/net/bus")
        let _a = write_stream_bytes(b"/dev/addr", concat_bytes(addr, b"\n"))
        assert accept_stream(b"/net/bus")
        let after_ep1 = settle(CtrlState(true, 0, [0, 0]), 64, step_client)
        assert accept_stream(b"/net/bus")
        let after_ep2 = settle(CtrlState(true, after_ep1.turns, after_ep1.totals), 64, step_client)
        assert after_ep2.totals[1] > after_ep2.totals[0]
"#;

    let plant_src = r#"use aod::sys:
    read_stream_frame, write_stream_frame
use stdlib::aod::net:
    DuonicWirePacket, decode_duonic_packet, encode_duonic_packet

dec run_episode(cut0: int) -> (int, t1, ratio):
    let q = DuonicWirePacket(1, cut0, 0, 0t, [0t, 0t], 0/1r)
    let _w1 = write_stream_frame(b"/net/bus", encode_duonic_packet(q))
    let (s1, raw1) = read_stream_frame(b"/net/bus", 4096)
    assert s1 == 1t
    let (ok1, m1) = decode_duonic_packet(raw1)
    assert ok1 and m1.kind == 2

    # Reward slot 1 with positive terminal pulse (+1t, +1t) => deficit +1 on slot 1!
    let term = DuonicWirePacket(3, cut0 + 2, 1, 1t, [0t, 1t], 0/1r)
    let _w2 = write_stream_frame(b"/net/bus", encode_duonic_packet(term))
    let (s2, raw2) = read_stream_frame(b"/net/bus", 4096)
    assert s2 == 1t
    let (ok2, ack) = decode_duonic_packet(raw2)
    assert ok2 and ack.kind == 3
    return (m1.channel, m1.signals[0], m1.current)

dec main:
    () -> int:
        let (chosen, changed_trit, cur) = run_episode(0)
        emit chosen
        emit changed_trit
        emit cur
        return 0
"#;

    std::fs::write(&controller_path, controller_src).unwrap();
    std::fs::write(&plant_path, plant_src).unwrap();

    let stdlib = stdlib_dir();
    let ctrl_aot = tmp.path().join("ctrl_aot");
    let plant_aot = tmp.path().join("plant_aot");
    for (src, bin) in [(&controller_path, &ctrl_aot), (&plant_path, &plant_aot)] {
        let b = Command::new(aodc_bin())
            .args(["build", "-O2", "--stdlib"])
            .arg(&stdlib)
            .arg(src)
            .arg("-o")
            .arg(bin)
            .output()
            .unwrap();
        assert!(
            b.status.success(),
            "build failed: {}",
            String::from_utf8_lossy(&b.stderr)
        );
    }

    for mode in ["run", "jit", "aot"] {
        let mut srv = if mode == "aot" {
            Command::new(&ctrl_aot)
                .args([
                    "--mount",
                    "tcp-listen:127.0.0.1:0:/net/bus",
                    "--mount",
                    "stdio:/dev/addr",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        } else {
            let mut c = Command::new(aodc_bin());
            c.arg(mode);
            if mode == "jit" {
                c.arg("-O2");
            }
            c.arg("--stdlib")
                .arg(&stdlib)
                .args([
                    "--mount",
                    "tcp-listen:127.0.0.1:0:/net/bus",
                    "--mount",
                    "stdio:/dev/addr",
                ])
                .arg(&controller_path)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        };

        let mut srv_stdout = BufReader::new(srv.stdout.take().unwrap());
        let mut bound_addr = String::new();
        srv_stdout.read_line(&mut bound_addr).unwrap();
        let bound_addr = bound_addr.trim();
        assert!(
            bound_addr.starts_with("127.0.0.1:"),
            "unexpected bound addr in {mode}: {bound_addr:?}"
        );

        let tcp_mount = format!("tcp:{bound_addr}:/net/bus");

        // Episode 1 (fresh controller): chooses tie-break slot 0, changed_by_carriage = t1(0), current = 0/1
        let ep1 = Command::new(&plant_aot)
            .arg("--mount")
            .arg(&tcp_mount)
            .output()
            .unwrap();
        assert!(ep1.status.success());
        assert_eq!(String::from_utf8_lossy(&ep1.stdout), "0\nt1(0)\n0/1\n0\n");

        // Episode 2 (same controller process with retained carriage!): shifts to slot 1, changed_by_carriage = t1(1)!
        let ep2 = Command::new(&plant_aot)
            .arg("--mount")
            .arg(&tcp_mount)
            .output()
            .unwrap();
        assert!(ep2.status.success());
        assert_eq!(String::from_utf8_lossy(&ep2.stdout), "1\nt1(1)\n1/1\n0\n");

        let srv_status = srv.wait().unwrap();
        assert!(srv_status.success(), "server mode {mode} exited non-zero");
    }
}

#[test]
fn stream_compile_time_and_capability_firewalls_enforced() {
    // 1. Unimported bare call -> SysIoForbidden
    let unimp = r#"dec main:
    () -> int:
        let _x = read_stream_line(b"/net/bus", 64)
        return 0
"#;
    let err1 = aodc::check_source(unimp).unwrap_err();
    assert_eq!(err1.code, DiagnosticCode::SysIoForbidden);

    // 2. Stream I/O inside read-only observer -> ObserverMutationForbidden
    let in_obs = r#"use aod::sys::read_stream_line

chamber BoxLab:
    space = box([0..16, 0..16, 0..16])
    walls = open

prepare P in BoxLab:
    inject 3:3:6 at (4, 4, 4) dir +X orient +1t shells 1

observe BadSensor:
    tracer leak(view):
        let _k = view.tick
        let _s = read_stream_line(b"/net/bus", 64)

dec main:
    () -> int:
        let _r = simulate P over 2 ticks with BadSensor
        return 0
"#;
    let err2 = aodc::check_source(in_obs).unwrap_err();
    assert_eq!(err2.code, DiagnosticCode::ObserverMutationForbidden);

    // 3. Stream I/O inside quarantine Q_ext -> QExtLeakageForbidden
    let in_qext = r#"use aod::sys::write_stream_bytes

dec main:
    () -> int:
        quarantine Q_ext:
            let _w = write_stream_bytes(b"/net/bus", b"leak")
        return 0
"#;
    let err3 = aodc::check_source(in_qext).unwrap_err();
    assert_eq!(err3.code, DiagnosticCode::QExtLeakageForbidden);

    // 4. Write to read-only sensor stream (`--mount ro-stdio:/dev/sensor`) rejected at runtime!
    let tmp = TempDirGuard::new("ro_sensor");
    let ro_prog = tmp.path().join("ro_write.dec");
    std::fs::write(
        &ro_prog,
        r#"use aod::sys::write_stream_bytes

dec main:
    () -> int:
        let _w = write_stream_bytes(b"/dev/sensor", b"actuate")
        return 0
"#,
    )
    .unwrap();

    for mode in ["run", "jit"] {
        let mut c = Command::new(aodc_bin());
        c.arg(mode);
        if mode == "jit" {
            c.arg("-O2");
        }
        let out = c
            .args(["--mount", "ro-stdio:/dev/sensor"])
            .arg(&ro_prog)
            .output()
            .unwrap();
        assert!(!out.status.success(), "{mode} must reject write on ro-stdio");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("read-only"),
            "expected read-only error in {mode}, got: {stderr}"
        );
    }
}

#[test]
fn operative_net_domain_blind_awareness_and_dn02_3mode_parity() {
    let tmp = TempDirGuard::new("operative_net_dn02");
    let prog_path = tmp.path().join("operative_check.dec");

    let prog_src = r#"use aod::core:
    contains, len, map_keys
use stdlib::aod::net:
    BoundaryRole, DuonicBoundaryFrame, DuonicInitSpec, DuonicPulse, RetainedRelation, classify_channel_role, decode_boundary_frame, decode_init_spec, decode_motor_frame, encode_boundary_frame, encode_init_spec, encode_motor_frame, extract_source_relations, operative_net_init, present_projection, remove_source_relations, restore_source_relations, retained_relation_count, step_operative_net, try_restore_source_relations, try_step_operative_net

dec main:
    () -> ():
        let spec = DuonicInitSpec(3, 3, 5, 4, true)
        let (ok_s, spec_dec) = decode_init_spec(encode_init_spec(spec))
        assert ok_s and spec_dec.motor_count == 3 and spec_dec.max_relations == 4

        let n0 = operative_net_init(99, 3, 3, 5, 4, true)
        assert present_projection(n0).self_id == 99

        let p_a = DuonicPulse(10, 1t, 3, 4, 5, 1/2r)
        let p_b = DuonicPulse(20, -1t, 3, 4, 5, 1/2r)
        let adm = [true, true, true]

        let f0 = DuonicBoundaryFrame([p_a], adm, 0, 0, 0t)
        let (ok_f0, f0_dec) = decode_boundary_frame(encode_boundary_frame(f0))
        assert ok_f0 and len(f0_dec.pulses) == 1
        let (n1, m0) = step_operative_net(n0, f0_dec)
        assert m0.selected_motor == 0 and m0.action_id == 1

        # Forged / wrong action_echo_id must be rejected cleanly with zero state mutation
        let bad_echo = DuonicBoundaryFrame([p_a], adm, 777, 2, 0t)
        let (ok_bad, n_bad, _) = try_step_operative_net(n1, bad_echo)
        assert (not ok_bad) and n_bad.x.cut == n1.x.cut

        # Certified 2-traversal closure return on source 10 (+1/1r)
        let (n2, m1) = step_operative_net(n1, DuonicBoundaryFrame([p_b], adm, m0.action_id, 2, 0t))
        assert m1.action_id == 2

        # Zero-traversal unclosed return on source 20 (-1/1r)
        let (n3, m2) = step_operative_net(n2, DuonicBoundaryFrame([p_a, p_b], adm, m1.action_id, 0, 0t))
        let (ok_m2, m2_dec) = decode_motor_frame(encode_motor_frame(m2))
        assert ok_m2 and m2_dec.action_id == 3

        # Cancelling +1 and -1 sources sum to 0/1r, yet BOTH remain operative under single-source removal!
        assert contains(m2_dec.operative_sources, 10)
        assert contains(m2_dec.operative_sources, 20)
        assert classify_channel_role(n3, m2_dec.operative_sources, 99) == BoundaryRole::SelfNode
        assert classify_channel_role(n3, m2_dec.operative_sources, 10) == BoundaryRole::BetweenInterface
        assert classify_channel_role(n3, m2_dec.operative_sources, 20) == BoundaryRole::BetweenInterface
        assert classify_channel_role(n3, m2_dec.operative_sources, 30) == BoundaryRole::NotSelf
        assert classify_channel_role(n3, m2_dec.operative_sources, -1) == BoundaryRole::NullRole

        let saved_10 = extract_source_relations(n3, 10)
        assert len(saved_10) == 1 and retained_relation_count(n3) == 2
        let n3_no10 = remove_source_relations(n3, 10)
        assert len(n3_no10.r) == 1

        # Wrong target_j or future commit_cut restoration must be rejected!
        let forged_rel = RetainedRelation(1, 10, 888, 0, 1, 1, 1/1r, 1)
        let (ok_forged, _) = try_restore_source_relations(n3_no10, [forged_rel])
        assert not ok_forged

        let n3_restored = restore_source_relations(n3_no10, saved_10)
        assert retained_relation_count(n3_restored) == 2
        let (_, m_orig) = step_operative_net(n3, DuonicBoundaryFrame([p_a, p_b], adm, m2.action_id, 1, 0t))
        let (_, m_rest) = step_operative_net(n3_restored, DuonicBoundaryFrame([p_a, p_b], adm, m2.action_id, 1, 0t))
        assert m_orig.selected_motor == m_rest.selected_motor and m_orig.operative_sources == m_rest.operative_sources

        emit len(m2_dec.operative_sources)
        emit n3.x.cut
"#;
    std::fs::write(&prog_path, prog_src).unwrap();
    let stdout = run_3mode(&prog_path, &[], None);
    assert_eq!(stdout, "2\n3\n");
}

#[test]
fn key_index_past_65535_membership_and_dn03_4mode_parity() {
    let dir = TempDirGuard::new("key_index_past_65535");
    let prog_path = dir.path().join("key_index_65535.dec");
    let ir_path = dir.path().join("key_index_65535.ir.json");
    let prog_src = r#"use aod::core:
    fold, len, range_seq, select
use stdlib::aod::net:
    DuonicInitSpec, IntTable, KeyIndex, ShardIntTable, decode_init_spec, decode_init_spec_v3, empty_int_table, empty_shard_int_table, encode_init_spec, int_table_add, int_table_contains, int_table_get, int_table_get_or, int_table_nonzero_count, int_table_put, int_table_remove, key_insert_new, key_len, key_lookup_slot, key_remove_swap, key_remove_swap_with_last, shard_int_add, shard_int_contains, shard_int_get, shard_int_len, shard_int_nonzero_count, shard_int_put, shard_int_remove, shard_int_sorted_entries, try_encode_init_spec, try_encode_init_spec_v3

dec add_route_bounded:
    (tbl: ShardIntTable, _: int, 0: int, _: int) -> ShardIntTable:
        return tbl
    (tbl: ShardIntTable, key: int, amount: int, cap: int) -> ShardIntTable:
        let present = shard_int_contains(tbl, key)
        let allowed = (present or (shard_int_len(tbl) < cap))
        return apply_add_route(allowed, tbl, key, amount)

dec apply_add_route:
    (false: bool, tbl: ShardIntTable, _: int, _: int) -> ShardIntTable:
        return tbl
    (true: bool, tbl: ShardIntTable, key: int, amount: int) -> ShardIntTable:
        let (_, _, next_t) = shard_int_add(tbl, key, amount)
        return next_t

dec main:
    () -> ():
        # 1. Exact membership semantics: 99_999_999 value vs absence vs 0 reactivation vs cap refusal
        let s0 = empty_shard_int_table(16)
        let (_, s1) = shard_int_put(s0, 12, 99999999)
        let (_, s2) = shard_int_put(s1, 23, 0)
        assert (shard_int_len(s2) == 2 and shard_int_nonzero_count(s2) == 1)
        assert (shard_int_contains(s2, 12) and shard_int_contains(s2, 23) and (not shard_int_contains(s2, 34)))
        let s3 = add_route_bounded(s2, 12, 1, 2)
        let (p12, v12) = shard_int_get(s3, 12)
        assert (p12 and (v12 == 100000000))
        let s4 = add_route_bounded(s3, 23, -1, 2)
        let (p23, v23) = shard_int_get(s4, 23)
        assert (p23 and (v23 == -1) and shard_int_nonzero_count(s4) == 2)
        let s5 = add_route_bounded(s4, 34, 1, 2)
        assert ((shard_int_len(s5) == 2) and (not shard_int_contains(s5, 34)))
        let s_zero_ign = add_route_bounded(s0, 34, 0, 2)
        assert (shard_int_len(s_zero_ign) == 0)

        # 2. Sorted start/middle/end insertion, swap-remove with moved-slot fixup, and exact restoration
        let t0 = empty_int_table()
        let t1 = int_table_put(t0, 50, 500)
        let t2 = int_table_put(t1, 10, 0)
        let t3 = int_table_put(t2, 90, 900)
        let t4 = int_table_put(t3, 30, 300)
        assert (int_table_contains(t4, 10) and int_table_get_or(t4, 10, -1) == 0 and int_table_nonzero_count(t4) == 3)
        let (rm_ok, rm_v, t5) = int_table_remove(t4, 50)
        assert (rm_ok and (rm_v == 500) and (not int_table_contains(t5, 50)))
        assert (int_table_get_or(t5, 10, -1) == 0 and int_table_get_or(t5, 30, -1) == 300 and int_table_get_or(t5, 90, -1) == 900)
        let t6 = int_table_put(t5, 50, rm_v)
        assert (int_table_get_or(t6, 50, -1) == 500 and int_table_get_or(t6, 30, -1) == 300)
        let (rm_s, idx_s, ds_s, ls_s) = key_remove_swap(t4.index, 50)
        assert (rm_s and ds_s == 0 and ls_s == 3 and key_lookup_slot(idx_s, 30) == 0)

        # 3. Direct KeyIndex transition across 65_535 -> 65_536 -> 65_537 and 109_062
        let ks65535 = range_seq(0, 65535)
        let idx65535 = KeyIndex(ks65535, ks65535)
        assert (key_len(idx65535) == 65535 and key_lookup_slot(idx65535, 0) == 0 and key_lookup_slot(idx65535, 65534) == 65534 and key_lookup_slot(idx65535, 65535) == -1)
        let idx65536 = key_insert_new(idx65535, 65535, 65535)
        assert (key_len(idx65536) == 65536 and key_lookup_slot(idx65536, 65535) == 65535 and key_lookup_slot(idx65536, 0) == 0 and key_lookup_slot(idx65536, 65536) == -1)
        let idx65537 = key_insert_new(idx65536, 65536, 65536)
        assert (key_len(idx65537) == 65537 and key_lookup_slot(idx65537, 65536) == 65536 and key_lookup_slot(idx65537, 32768) == 32768)
        let (rm65, idx_back, dead_s, last_s) = key_remove_swap_with_last(idx65537, 32768, 65536)
        assert (rm65 and (dead_s == 32768) and (last_s == 65536) and key_len(idx_back) == 65536)
        assert (key_lookup_slot(idx_back, 32768) == -1 and key_lookup_slot(idx_back, 65536) == 32768)

        let ks109 = range_seq(0, 109062)
        let idx109 = KeyIndex(ks109, ks109)
        assert (key_len(idx109) == 109062 and key_lookup_slot(idx109, 0) == 0 and key_lookup_slot(idx109, 65535) == 65535 and key_lookup_slot(idx109, 65536) == 65536 and key_lookup_slot(idx109, 109061) == 109061 and key_lookup_slot(idx109, 109062) == -1)

        # 4. Canonical sorted merge on ShardIntTable
        let (_, m1) = shard_int_put(s0, 91, 9)
        let (_, m2) = shard_int_put(m1, 14, 0)
        let (_, m3) = shard_int_put(m2, 42, -1)
        let (sk, sv) = shard_int_sorted_entries(m3)
        assert (sk == [14, 42, 91] and sv == [0, -1, 9])

        # 5. DN02 byte-compatibility + explicit rejection past 65535 + versioned DN03 negotiation
        let spec_v2 = DuonicInitSpec(9, 3, 5, 65535, true)
        let raw_v2 = encode_init_spec(spec_v2)
        assert (len(raw_v2) == 16)
        let (ok_v2, dec_v2) = decode_init_spec(raw_v2)
        assert (ok_v2 and dec_v2.max_relations == 65535)
        let (ok_v3_on_v2, _) = decode_init_spec_v3(raw_v2)
        assert not ok_v3_on_v2

        let spec_big = DuonicInitSpec(9, 3, 5, 109062, true)
        let (ok_v2_big, err_v2_big) = try_encode_init_spec(spec_big)
        assert ((not ok_v2_big) and (err_v2_big == b"ERR dn02_init_spec_out_of_u16_range"))
        let (ok_v3_enc, raw_v3) = try_encode_init_spec_v3(spec_big)
        assert (ok_v3_enc and (len(raw_v3) == 24))
        let (ok_v2_on_v3, _) = decode_init_spec(raw_v3)
        assert not ok_v2_on_v3
        let (ok_v3_dec, dec_v3) = decode_init_spec_v3(raw_v3)
        assert (ok_v3_dec and dec_v3.max_relations == 109062 and dec_v3.motor_count == 9)

        emit key_len(idx109)
        emit dec_v3.max_relations
"#;
    std::fs::write(&prog_path, prog_src).unwrap();
    let stdout = run_3mode(&prog_path, &[], None);
    assert_eq!(stdout, "109062\n109062\n");

    // 4th backend: serialized IR (`aodc compile` -> `aodc exec`) must match VM/JIT/AOT byte-for-byte
    let comp = Command::new(aodc_bin())
        .args([
            "compile",
            "--stdlib",
            &stdlib_dir().to_string_lossy(),
            &prog_path.to_string_lossy(),
            "-o",
            &ir_path.to_string_lossy(),
        ])
        .output()
        .expect("compile to IR");
    assert!(
        comp.status.success(),
        "IR compile failed: {}",
        String::from_utf8_lossy(&comp.stderr)
    );
    let exec_ir = Command::new(aodc_bin())
        .args(["exec", "--fuel", "20000000", &ir_path.to_string_lossy()])
        .output()
        .expect("exec serialized IR");
    assert!(
        exec_ir.status.success(),
        "IR exec failed: {}",
        String::from_utf8_lossy(&exec_ir.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&exec_ir.stdout), stdout);
}

#[test]
fn shard_int_table_109062_native_aot_scale() {
    let dir = TempDirGuard::new("shard_scale_109062");
    let prog_path = dir.path().join("shard_scale_109062.dec");
    let bin_path = dir.path().join("shard_scale_109062.elf");
    let prog_src = r#"use aod::core:
    fold, len, range_seq, select
use stdlib::aod::net:
    ShardIntTable, empty_shard_int_table, shard_int_contains, shard_int_get, shard_int_len, shard_int_nonzero_count, shard_int_put, shard_int_remove, shard_int_sorted_entries

dec insert_step:
    (tbl: ShardIntTable, i: int) -> ShardIntTable:
        let key = (((i * 2654435761) % 1000000000) + i)
        let val = select(((i % 10) == 0), 0, select((i == 65535), 99999999, ((i % 7) - 3)))
        let (_, next_t) = shard_int_put(tbl, key, val)
        return next_t

dec main:
    () -> ():
        let t0 = empty_shard_int_table(512)
        let t65535 = fold(range_seq(0, 65535), t0, insert_step)
        assert (shard_int_len(t65535) == 65535)
        let t65536 = insert_step(t65535, 65535)
        let k65535 = (((65535 * 2654435761) % 1000000000) + 65535)
        let (hit65535, val65535) = shard_int_get(t65536, k65535)
        assert (shard_int_len(t65536) == 65536 and hit65535 and val65535 == 99999999)
        let t65537 = insert_step(t65536, 65536)
        assert (shard_int_len(t65537) == 65537)
        let t109 = fold(range_seq(65537, 109062), t65537, insert_step)
        assert (shard_int_len(t109) == 109062)

        # Verify zero-valued member, removal with swap-fixup, and exact restoration at 109,062 entries
        let k_zero = (((100 * 2654435761) % 1000000000) + 100)
        let (hz, vz) = shard_int_get(t109, k_zero)
        assert (hz and vz == 0)
        let (rm_ok, rm_val, t_less) = shard_int_remove(t109, k65535)
        assert (rm_ok and rm_val == 99999999 and shard_int_len(t_less) == 109061 and (not shard_int_contains(t_less, k65535)))
        let (ins_back, t_restored) = shard_int_put(t_less, k65535, rm_val)
        assert (ins_back and shard_int_len(t_restored) == 109062)
        let nz = shard_int_nonzero_count(t_restored)
        let (sk, sv) = shard_int_sorted_entries(t_restored)
        assert (len(sk) == 109062 and len(sv) == 109062 and sk[0] < sk[109061])
        # Verify start, middle, end, and absent key lookups at 109,062 occupied entries
        let (h_start, v_start) = shard_int_get(t_restored, sk[0])
        let (h_mid, v_mid) = shard_int_get(t_restored, sk[54531])
        let (h_end, v_end) = shard_int_get(t_restored, sk[109061])
        assert (h_start and v_start == sv[0] and h_mid and v_mid == sv[54531] and h_end and v_end == sv[109061])
        assert (not shard_int_contains(t_restored, (sk[109061] + 1)))
        emit shard_int_len(t_restored)
        emit nz
"#;
    std::fs::write(&prog_path, prog_src).unwrap();
    let build = Command::new(aodc_bin())
        .args([
            "build",
            "-O2",
            "--stdlib",
            &stdlib_dir().to_string_lossy(),
            &prog_path.to_string_lossy(),
            "-o",
            &bin_path.to_string_lossy(),
        ])
        .output()
        .expect("build 109062 AOT binary");
    assert!(
        build.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(&bin_path)
        .args(["--fuel", "500000000", "--arena-cap", "262144"])
        .output()
        .expect("run 109062 AOT binary");
    assert!(
        run.status.success(),
        "run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let out = String::from_utf8_lossy(&run.stdout);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "unexpected output: {out}");
    assert_eq!(lines[0], "109062");
    assert!(lines[1].parse::<i64>().unwrap() > 80_000);
}

