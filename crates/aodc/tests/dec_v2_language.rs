use std::path::PathBuf;

use aodc::diagnostics::{CompatMode, DiagnosticCode};
use aodc::{check_file, check_source, run_file, run_source, run_source_with_budget};

fn temp_test_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aodc_lang_{}_{}", std::process::id(), tag));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn v2_literals_control_flow_named_args_boundary_and_contextual_idents() {
    let src = r#"
Dec combine(base: int, window: int, shedding: int) -> int:
    mut acc = base
    for i in 0..window:
        if i == 2:
            continue
        acc = acc + i
    mut k = 0
    while k < shedding:
        k = k + 1
        if k == 2:
            break
    return acc + k

Dec main() -> int:
    let r1 = 1/3r
    let r2 = -1/3r
    assert r1 + r2 == 0/1r
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
    let wins = [(0, 6], (4, 8]]
    assert len(wins) == 2
    let b = box([24..40, 0..16, 0..16])
    let bd = ∂b
    assert bd == boundary_of(b)
    let v = combine(shedding: 5, base: 10, window: 4)
    # 10 + (0 + 1 + 3) + 2 = 16
    assert v == 16
    emit v
    return 0
"#;
    let out = run_source(src, CompatMode::V2).unwrap();
    assert_eq!(out.to_stdout_string(), "16\n0\n");
}

#[test]
fn hostile_lexer_float_octal_support_and_zero_den_rejections() {
    let cases = [
        ("Dec main() -> int:\n    let x = 1.5\n    return 0\n", DiagnosticCode::FloatLiteralForbidden),
        ("Dec main() -> int:\n    let x = 5.\n    return 0\n", DiagnosticCode::FloatLiteralForbidden),
        ("Dec main() -> int:\n    let x = .5\n    return 0\n", DiagnosticCode::FloatLiteralForbidden),
        ("Dec main() -> int:\n    let x = 1e3\n    return 0\n", DiagnosticCode::FloatLiteralForbidden),
        ("Dec main() -> int:\n    let x = 1E-3\n    return 0\n", DiagnosticCode::FloatLiteralForbidden),
        ("Dec main() -> int:\n    let x = 08_8\n    return 0\n", DiagnosticCode::InvalidOctalAnchor),
        ("Dec main() -> int:\n    let x = 9|-\n    return 0\n", DiagnosticCode::InvalidOctalAnchor),
        ("Dec main() -> int:\n    let x = 0o18\n    return 0\n", DiagnosticCode::InvalidOctalAnchor),
        ("Dec main() -> int:\n    let x = 1/0r\n    return 0\n", DiagnosticCode::ZeroDenominator),
        ("Dec main() -> int:\n    let x = 1:2:9\n    return 0\n", DiagnosticCode::OddSupportRequiresYro),
        ("Dec main() -> int:\n    let x = 3:3:6_yro\n    return 0\n", DiagnosticCode::EvenSupportForbidsYro),
        ("Dec main() -> int:\n    let x = 2t\n    return 0\n", DiagnosticCode::InvalidTritLiteral),
        ("Dec main() -> int:\n    let x = 10t\n    return 0\n", DiagnosticCode::InvalidTritLiteral),
    ];

    for (src, expected_code) in cases {
        let err = check_source(src, CompatMode::V2).unwrap_err();
        assert_eq!(err.code, expected_code, "wrong error for:\n{src}\ngot: {err}");
    }

    // Compact trit literals must be rejected under --compat v1
    let v1_trit = "Dec main() -> result:\n    () -> t1 result:\n        result = 0t\n";
    assert_eq!(
        check_source(v1_trit, CompatMode::V1).unwrap_err().code,
        DiagnosticCode::ParseError
    );
}

#[test]
fn hostile_implicit_coercions_immutable_let_unreachable_check_and_budgets() {
    // Firewall 4: zero implicit coercions
    let mix1 = "Dec main() -> int:\n    let x = 1 + t32(2)\n    return 0\n";
    assert_eq!(
        check_source(mix1, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::ImplicitCoercionForbidden
    );
    let mix2 = "Dec main() -> int:\n    let x = 1 + 1/2r\n    return 0\n";
    assert_eq!(
        check_source(mix2, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::ImplicitCoercionForbidden
    );

    // `let` immutability enforced
    let immut = "Dec main() -> int:\n    let x = 1\n    x = 2\n    return x\n";
    assert!(check_source(immut, CompatMode::V2).is_err());

    // Firewall 5: unreachable dead function STILL exhaustively typechecked!
    let dead_bad = r#"
Dec dead_helper(x: int) -> int:
    return x + 1/2r

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(dead_bad, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::ImplicitCoercionForbidden
    );

    // Fuel budget exhaustion
    let inf_loop = "Dec main() -> int:\n    mut i = 0\n    while true:\n        i = i + 1\n    return i\n";
    assert_eq!(
        run_source_with_budget(inf_loop, CompatMode::V2, 200, 32)
            .unwrap_err()
            .code,
        DiagnosticCode::BudgetExhausted
    );

    // Call depth budget exhaustion
    let inf_rec = "Dec rec(n: int) -> int:\n    return rec(n + 1)\n\nDec main() -> int:\n    return rec(0)\n";
    assert_eq!(
        run_source_with_budget(inf_rec, CompatMode::V2, 100_000, 16)
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
        "const FACTOR = 3\n\nDec triple(x: int) -> int:\n    return x * FACTOR\n",
    )
    .unwrap();
    let main_path = dir.join("main.dec");
    std::fs::write(
        &main_path,
        "use helper::{triple, FACTOR}\n\nDec main() -> int:\n    emit triple(7) + FACTOR\n    return 0\n",
    )
    .unwrap();

    let out = run_file(&main_path, CompatMode::V2).unwrap();
    assert_eq!(out.to_stdout_string(), "24\n0\n");

    // Hostile cyclic import A -> B -> A must fail with ModuleCycleError
    std::fs::write(dir.join("cyc_a.dec"), "use cyc_b\n\nDec a() -> int:\n    return 1\n").unwrap();
    std::fs::write(dir.join("cyc_b.dec"), "use cyc_a\n\nDec b() -> int:\n    return 2\n").unwrap();
    let cyc_main = dir.join("cyc_main.dec");
    std::fs::write(&cyc_main, "use cyc_a\n\nDec main() -> int:\n    return 0\n").unwrap();
    let err = check_file(&cyc_main, CompatMode::V2).unwrap_err();
    assert_eq!(err.code, DiagnosticCode::ModuleCycleError);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ac14_struct_enum_match_deterministic_map_and_pattern_dispatch() {
    let src = r#"
struct Node:
    id: int
    weight: ratio

struct Pair { a: int, b: int }

enum Route:
    Closed
    Exo(int)
    Open

enum Seat { OpenDuon, SeatedDuad }

Dec classify_route(r: Route) -> int:
    match r:
        Route::Closed:
            return 0
        Route::Exo(k) if k > 10:
            return k * 2
        Route::Exo(k):
            return k
        Route::Open:
            return -1

# Multi-arm enum + literal + wildcard + guard pattern dispatch
Dec dispatch(s, x) -> out:
    (Seat::SeatedDuad, 0) -> int out:
        out = 100
    (Seat::SeatedDuad, int x) if x > 0 -> int out:
        out = 100 + x
    (_, int x) -> int out:
        out = -x

Dec trit_weight(t, n) -> out:
    (t1 0t, int n) -> int out:
        out = 0
    (t1 +1t, int n) -> int out:
        out = n
    (t1 -1t, int n) -> int out:
        out = -n

Dec main() -> int:
    # Struct construction (brace, positional, named) & mut field update
    mut n = Node { id: 1, weight: 1/2r }
    let n2 = Node(2, 3/4r)
    let p = Pair(b: 9, a: 4)
    assert n.id == 1
    assert n2.weight == 3/4r
    assert p.a == 4 and p.b == 9
    n.id = 7
    assert n.id == 7

    # Enum + exhaustive match with guard
    assert classify_route(Route::Closed) == 0
    assert classify_route(Route::Exo(5)) == 5
    assert classify_route(Route::Exo(12)) == 24
    assert classify_route(Route::Open) == -1

    # Multi-arm pattern dispatch
    assert dispatch(Seat::SeatedDuad, 0) == 100
    assert dispatch(Seat::SeatedDuad, 7) == 107
    assert dispatch(Seat::OpenDuon, 4) == -4
    assert trit_weight(0t, 5) == 0
    assert trit_weight(+1t, 5) == 5
    assert trit_weight(-1t, 5) == -5

    # Deterministic BTreeMap map[K, V] insertion, mutation, len, contains, and sorted key order!
    mut m: map[str, int] = {}
    m["zeta"] = 3
    m["alpha"] = 1
    m["mid"] = 2
    assert len(m) == 3
    assert contains(m, "alpha") == true
    assert contains(m, "missing") == false
    assert m["mid"] == 2

    let literal_map = {"c": 30, "a": 10, "b": 20}
    assert len(literal_map) == 3

    for (k, v) in m:
        emit k
        emit v

    mut key_count = 0
    for k in literal_map:
        key_count = key_count + 1
    assert key_count == 3
    return 0
"#;
    let out = run_source(src, CompatMode::V2).unwrap();
    // Deterministic sorted BTreeMap order: alpha(1), mid(2), zeta(3)
    assert_eq!(out.to_stdout_string(), "alpha\n1\nmid\n2\nzeta\n3\n0\n");
}

#[test]
fn ac14_hostile_nonexhaustive_match_qext_struct_enum_and_v1_rejections() {
    // 1. Non-exhaustive match on enum rejected at compile time
    let non_exhaustive = r#"
enum Route:
    Closed
    Exo(int)
    Open

Dec bad(r: Route) -> int:
    match r:
        Route::Closed:
            return 0
        Route::Exo(k):
            return k
    return 0

Dec main() -> int:
    return 0
"#;
    let err = check_source(non_exhaustive, CompatMode::V2).unwrap_err();
    assert_eq!(err.code, DiagnosticCode::TypeError);
    assert!(err.message.contains("non-exhaustive"), "err={err}");

    // 2. Guarded enum arm does NOT satisfy exhaustiveness on its own!
    let guarded_only = r#"
enum Flag { On, Off }

Dec bad(f: Flag) -> int:
    match f:
        Flag::On if false:
            return 1
        Flag::Off:
            return 0
    return 0

Dec main() -> int:
    return 0
"#;
    let err2 = check_source(guarded_only, CompatMode::V2).unwrap_err();
    assert_eq!(err2.code, DiagnosticCode::TypeError);
    assert!(err2.message.contains("non-exhaustive"), "err={err2}");

    // 3. QExt<T> forbidden inside native struct field
    let qext_struct = r#"
struct BadRec:
    q: QExt<ratio>

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(qext_struct, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // 4. QExt<T> forbidden inside native enum payload
    let qext_enum = r#"
enum BadEnum:
    Leak(QExt<ratio>)

Dec main() -> int:
    return 0
"#;
    assert_eq!(
        check_source(qext_enum, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // 5. Immutable struct field mutation rejected
    let immut_struct = r#"
struct S { x: int }

Dec main() -> int:
    let s = S { x: 1 }
    s.x = 2
    return s.x
"#;
    assert!(check_source(immut_struct, CompatMode::V2).is_err());

    // 6. --compat v1 rejects struct, enum, match, and map literals!
    for v2_snippet in [
        "struct S { x: int }\nDec main() -> r:\n    () -> int r:\n        r = 0\n",
        "enum E { A }\nDec main() -> r:\n    () -> int r:\n        r = 0\n",
        "Dec main() -> r:\n    () -> int r:\n        match 1:\n            _: r = 0\n",
        "Dec main() -> r:\n    () -> int r:\n        int x = {\"a\": 1}\n        r = 0\n",
    ] {
        assert_eq!(
            check_source(v2_snippet, CompatMode::V1).unwrap_err().code,
            DiagnosticCode::ParseError,
            "expected v1 ParseError for:\n{v2_snippet}"
        );
    }
}
