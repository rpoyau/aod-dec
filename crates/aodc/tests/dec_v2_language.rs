use std::path::PathBuf;

use aodc::diagnostics::DiagnosticCode;
use aodc::{check_file, check_source, run_file, run_source, run_source_with_budget};

fn temp_test_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aodc_lang_{}_{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn clean_break_literals_rank_lifting_adverbs_pipelines_and_trit_boxed_numerics() {
    let src = r#"
dec step_settle(k: int) -> int:
    select(k < 2, k + 1, k)

dec combine(base: int, window: int, shedding: int) -> int:
    let rng = range_seq(0, window)
    let kept = (rng != 2) ?> rng
    let acc = base + (kept |> +/)
    let settled = settle(0, shedding, step_settle)
    acc + settled

dec main() -> int:
    let r1 = 1/3r
    let r2 = -1/3r
    assert r1 + r2 == 0/1r
    assert unpack_ratio(pack_ratio(1/3r)) == 1/3r
    assert unpack_ratio(pack_ratio(-7/9r)) == -7/9r
    assert 00_8 == 00|-
    assert 00_8 == 0o00
    assert 00_8 != 0_8
    let sf = 3:3:6
    let yro = 1:2:9_yro
    assert sf.is_yro == false
    assert yro.is_yro == true
    assert yro.is_closure_certificate == false
    assert 0_H == t1(0)
    assert 0_D == t1(0)
    assert +1t == t1(1)
    assert 0t == t1(0)
    assert -1t == t1(-1)
    assert +1t + -1t == 0t
    assert (5 <=> 2) == +1t
    assert (2 <=> 5) == -1t
    assert (4 <=> 4) == 0t
    assert sgn(-42) == -1t
    assert mux(-1t, 10, 20, 30) == 10
    assert mux(0t, 10, 20, 30) == 20
    assert mux(+1t, 10, 20, 30) == 30
    assert gate(true, 9) == 9
    assert gate(false, 9) == 0

    # J-style static rank lifting, outer product, scan, boundary, and fused sieve-fold
    let a: [int; 3] = [1, 2, 3]
    let b: [int; 3] = [10, 20, 30]
    let s = a + b
    assert s == [11, 22, 33]
    assert (a * 2) == [2, 4, 6]
    assert (a |> +\) == [1, 3, 6]
    assert (s |> ∂) == [11, 11]
    assert (s |> argmax) == 2
    assert (s |> argmin) == 0
    let outer = [1, 2] *: [3, 4]
    assert outer == [[3, 4], [6, 8]]
    let fused = ([true, false, true] ?> [5, 99, 7]) |> +/
    assert fused == 12

    # Inline 32-trit boundary (±926,510,094,425,920) & transparent BigTrit spill
    let max_inline = 926510094425920
    let min_inline = -926510094425920
    assert int(b64(max_inline)) == max_inline
    assert int(b64(min_inline)) == min_inline
    let spilled = max_inline + 1
    assert spilled - 1 == max_inline
    let big_prod = max_inline * max_inline
    assert big_prod // max_inline == max_inline

    # Raw bit/byte containers
    let raw = b64_bits(0xDEADBEEF)
    assert len(b64_le_bytes(raw)) == 8
    assert hex_bytes(bytes("Dec")) == "446563"

    let wins = [(0, 6], (4, 8]]
    assert len(wins) == 2
    let bx = box([24..40, 0..16, 0..16])
    let bd = ∂bx
    assert bd == boundary_of(bx)
    let v = combine(10, 4, 5)
    assert v == 16
    emit v
    0
"#;
    let out = run_source(src).unwrap();
    assert_eq!(out.to_stdout_string(), "16\n0\n");
}

#[test]
fn hostile_lexer_clean_break_shape_trit_and_zero_den_rejections() {
    let cases = [
        ("dec main() -> int:\n    let x = 1.5\n    0\n", DiagnosticCode::FloatLiteralForbidden),
        ("dec main() -> int:\n    let x = 5.\n    0\n", DiagnosticCode::FloatLiteralForbidden),
        ("dec main() -> int:\n    let x = .5\n    0\n", DiagnosticCode::FloatLiteralForbidden),
        ("dec main() -> int:\n    let x = 1e3\n    0\n", DiagnosticCode::FloatLiteralForbidden),
        ("dec main() -> int:\n    let x = 1E-3\n    0\n", DiagnosticCode::FloatLiteralForbidden),
        ("dec main() -> int:\n    let x = 08_8\n    0\n", DiagnosticCode::InvalidOctalAnchor),
        ("dec main() -> int:\n    let x = 9|-\n    0\n", DiagnosticCode::InvalidOctalAnchor),
        ("dec main() -> int:\n    let x = 0o18\n    0\n", DiagnosticCode::InvalidOctalAnchor),
        ("dec main() -> int:\n    let x = 1/0r\n    0\n", DiagnosticCode::ZeroDenominator),
        ("dec main() -> int:\n    let x = 1:2:9\n    0\n", DiagnosticCode::OddSupportRequiresYro),
        ("dec main() -> int:\n    let x = 3:3:6_yro\n    0\n", DiagnosticCode::EvenSupportForbidsYro),
        ("dec main() -> int:\n    let x = 2t\n    0\n", DiagnosticCode::InvalidTritLiteral),
        ("dec main() -> int:\n    let x = 10t\n    0\n", DiagnosticCode::InvalidTritLiteral),
        // Static shape mismatch on fixed-rank arrays
        (
            "dec main() -> int:\n    let a: [int; 3] = [1, 2, 3]\n    let b: [int; 4] = [1, 2, 3, 4]\n    let c = a + b\n    0\n",
            DiagnosticCode::ShapeMismatch,
        ),
    ];

    for (src, expected_code) in cases {
        let err = check_source(src).unwrap_err();
        assert_eq!(err.code, expected_code, "wrong error for:\n{src}\ngot: {err}");
    }

    // Imperative keywords, braces, t32, int64, and uppercase Dec are strictly rejected
    for removed in [
        "Dec main() -> int:\n    0\n",
        "dec main() -> int:\n    mut x = 1\n    x\n",
        "dec main() -> int:\n    if true:\n        1\n    0\n",
        "dec main() -> int:\n    for i in 0..3:\n        emit i\n    0\n",
        "dec main() -> int:\n    while false:\n        emit 1\n    0\n",
        "struct S { x: int }\ndec main() -> int:\n    0\n",
        "dec main(x: t32) -> int:\n    0\n",
        "dec main(x: int64) -> int:\n    0\n",
    ] {
        assert!(
            check_source(removed).is_err(),
            "expected clean-break rejection for:\n{removed}"
        );
    }

    // Runtime rejection of invalid 0b11 trit lane in `int(b64)`
    let bad_lane = "dec main() -> int:\n    int(b64_bits(3))\n";
    let rerr = run_source(bad_lane).unwrap_err();
    assert_eq!(rerr.code, DiagnosticCode::RuntimeError);
    assert!(
        rerr.message.contains("trit") || rerr.message.contains("0b11") || rerr.message.contains("3"),
        "unexpected error message: {rerr}"
    );
}

#[test]
fn hostile_implicit_coercions_immutable_let_unreachable_check_and_budgets() {
    // Firewall 4: zero implicit coercions
    let mix1 = "dec main() -> int:\n    let x = 1 + 1/2r\n    0\n";
    assert_eq!(
        check_source(mix1).unwrap_err().code,
        DiagnosticCode::ImplicitCoercionForbidden
    );
    let mix2 = "dec main() -> int:\n    let x = 1 + b64(2)\n    0\n";
    assert!(check_source(mix2).is_err());

    // `let` immutability enforced (reassignment rejected)
    let immut = "dec main() -> int:\n    let x = 1\n    x = 2\n    x\n";
    assert!(check_source(immut).is_err());

    // Firewall 5: unreachable dead function STILL exhaustively typechecked!
    let dead_bad = r#"
dec dead_helper(x: int) -> int:
    x + 1/2r

dec main() -> int:
    0
"#;
    assert_eq!(
        check_source(dead_bad).unwrap_err().code,
        DiagnosticCode::ImplicitCoercionForbidden
    );

    // Fuel budget exhaustion via bounded combinator exceeding caller's fuel budget
    let fuel_prog = "dec inc(x: int) -> int:\n    x + 1\n\ndec main() -> int:\n    walk(0, 5000, inc)\n";
    assert_eq!(
        run_source_with_budget(fuel_prog, 200, 32)
            .unwrap_err()
            .code,
        DiagnosticCode::BudgetExhausted
    );

    // Call depth budget exhaustion
    let inf_rec = "dec rec(n: int) -> int:\n    rec(n + 1)\n\ndec main() -> int:\n    rec(0)\n";
    assert_eq!(
        run_source_with_budget(inf_rec, 100_000, 16)
            .unwrap_err()
            .code,
        DiagnosticCode::BudgetExhausted
    );
}

#[test]
fn multi_file_module_resolution_and_cycle_detection() {
    let dir = temp_test_dir("modules");
    std::fs::write(
        dir.join("helper.dec"),
        "const FACTOR = 3\n\ndec triple(x: int) -> int:\n    x * FACTOR\n",
    )
    .unwrap();
    let main_path = dir.join("main.dec");
    std::fs::write(
        &main_path,
        "use helper:\n    triple, FACTOR\n\ndec main() -> int:\n    emit triple(7) + FACTOR\n    0\n",
    )
    .unwrap();

    let out = run_file(&main_path).unwrap();
    assert_eq!(out.to_stdout_string(), "24\n0\n");

    // Hostile cyclic import A -> B -> A must fail with ModuleCycleError
    std::fs::write(dir.join("cyc_a.dec"), "use cyc_b\n\ndec a() -> int:\n    1\n").unwrap();
    std::fs::write(dir.join("cyc_b.dec"), "use cyc_a\n\ndec b() -> int:\n    2\n").unwrap();
    let cyc_main = dir.join("cyc_main.dec");
    std::fs::write(&cyc_main, "use cyc_a\n\ndec main() -> int:\n    0\n").unwrap();
    let err = check_file(&cyc_main).unwrap_err();
    assert_eq!(err.code, DiagnosticCode::ModuleCycleError);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ac14_struct_enum_match_deterministic_map_and_signature_matrix() {
    let src = r#"
struct Node:
    id: int
    weight: ratio

struct Pair:
    a: int
    b: int

enum Route:
    Closed
    Exo(int)
    Open

enum Seat:
    OpenDuon
    SeatedDuad

dec classify_route(r: Route) -> int:
    match r:
        Route::Closed -> 0
        Route::Exo(k) where k > 10 -> k * 2
        Route::Exo(k) -> k
        Route::Open -> -1

# Multi-arm enum + literal + wildcard + where-guard signature matrix
dec dispatch:
    (Seat::SeatedDuad, 0: int) -> int:
        100
    (Seat::SeatedDuad, x: int) where x > 0 -> int:
        100 + x
    (_: Seat, x: int) -> int:
        -x

dec trit_weight:
    (0t: t1, n: int) -> int:
        0
    (+1t: t1, n: int) -> int:
        n
    (-1t: t1, n: int) -> int:
        -n

dec emit_entry(kv: (str, int)) -> int:
    let (k, v) = kv
    emit k
    emit v
    0

dec main() -> int:
    let n = Node(1, 1/2r)
    let n2 = Node(2, 3/4r)
    let p = Pair(4, 9)
    assert n.id == 1
    assert n2.weight == 3/4r
    assert p.a == 4 and p.b == 9

    # Struct field projection across sequences
    let nodes = [n, n2]
    assert nodes.id == [1, 2]

    # Enum + exhaustive match with where guard
    assert classify_route(Route::Closed) == 0
    assert classify_route(Route::Exo(5)) == 5
    assert classify_route(Route::Exo(12)) == 24
    assert classify_route(Route::Open) == -1

    # Multi-arm signature matrix
    assert dispatch(Seat::SeatedDuad, 0) == 100
    assert dispatch(Seat::SeatedDuad, 7) == 107
    assert dispatch(Seat::OpenDuon, 4) == -4
    assert trit_weight(0t, 5) == 0
    assert trit_weight(+1t, 5) == 5
    assert trit_weight(-1t, 5) == -5

    # Deterministic immutable BTreeMap map[K, V]
    let m0: map[str, int] = map[]
    let m1 = map_put(m0, "zeta", 3)
    let m2 = map_put(m1, "alpha", 1)
    let m = map_put(m2, "mid", 2)
    assert len(m) == 3
    assert contains(m, "alpha") == true
    assert contains(m, "missing") == false
    assert m["mid"] == 2

    let literal_map = map["c": 30, "a": 10, "b": 20]
    assert len(literal_map) == 3
    assert len(map_keys(literal_map)) == 3

    let _ = each(map_entries(m), emit_entry)
    0
"#;
    let out = run_source(src).unwrap();
    assert_eq!(out.to_stdout_string(), "alpha\n1\nmid\n2\nzeta\n3\n0\n");
}

#[test]
fn ac14_hostile_nonexhaustive_match_and_qext_struct_enum_rejections() {
    // 1. Non-exhaustive match on enum rejected at compile time
    let non_exhaustive = r#"
enum Route:
    Closed
    Exo(int)
    Open

dec bad(r: Route) -> int:
    match r:
        Route::Closed -> 0
        Route::Exo(k) -> k

dec main() -> int:
    0
"#;
    let err = check_source(non_exhaustive).unwrap_err();
    assert_eq!(err.code, DiagnosticCode::TypeError);
    assert!(err.message.contains("non-exhaustive"), "err={err}");

    // 2. Guarded enum arm does NOT satisfy exhaustiveness on its own!
    let guarded_only = r#"
enum Flag:
    On
    Off

dec bad(f: Flag) -> int:
    match f:
        Flag::On where false -> 1
        Flag::Off -> 0

dec main() -> int:
    0
"#;
    let err2 = check_source(guarded_only).unwrap_err();
    assert_eq!(err2.code, DiagnosticCode::TypeError);
    assert!(err2.message.contains("non-exhaustive"), "err={err2}");

    // 3. QExt<T> forbidden inside native struct field
    let qext_struct = r#"
struct BadRec:
    q: QExt<ratio>

dec main() -> int:
    0
"#;
    assert_eq!(
        check_source(qext_struct).unwrap_err().code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // 4. QExt<T> forbidden inside native enum payload
    let qext_enum = r#"
enum BadEnum:
    Leak(QExt<ratio>)

dec main() -> int:
    0
"#;
    assert_eq!(
        check_source(qext_enum).unwrap_err().code,
        DiagnosticCode::QExtLeakageForbidden
    );
}
