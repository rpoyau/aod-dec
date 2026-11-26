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
    BoundaryRole, DuonicBoundaryFrame, DuonicInitSpec, DuonicPulse, classify_channel_role, decode_boundary_frame, decode_init_spec, decode_motor_frame, encode_boundary_frame, encode_init_spec, encode_motor_frame, operative_net_init, present_projection, remove_source_relations, step_operative_net, try_step_operative_net

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

        let n3_no10 = remove_source_relations(n3, 10)
        assert len(map_keys(n3_no10.r)) == 1

        emit len(m2_dec.operative_sources)
        emit n3.x.cut
"#;
    std::fs::write(&prog_path, prog_src).unwrap();
    let stdout = run_3mode(&prog_path, &[], None);
    assert_eq!(stdout, "2\n3\n");
}
