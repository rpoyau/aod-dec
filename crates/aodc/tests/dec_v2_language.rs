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
fn v2_literals_scope_ops_boundary_and_contextual_idents() {
    let src = r#"
use aod::core:
    t1, len, select, evolve

use aod::sim::boundary_of

dec window_step:
    (acc: int, i: int) -> int:
        return acc + select(i == 2, 0, i)

dec shedding_clamp:
    (shedding: int) where shedding > 2 -> int:
        return 2
    (shedding: int) -> int:
        return shedding

dec combine:
    (base: int, window_n: int, shedding: int) -> int:
        let acc = evolve(base, 0..window_n, window_step)
        return acc + shedding_clamp(shedding)

dec main:
    () -> int:
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
        assert 0_H == 0t
        assert 0_D == 0t
        assert 1t == t1(1)
        assert 0t == t1(0)
        assert t1(-1) + 1t == 0t
        let wins = [(0, 6], (4, 8]]
        assert len(wins) == 2
        let b = box([24..40, 0..16, 0..16])
        let bd = ∂b
        assert bd == boundary_of(b)
        let v = combine(10, 4, 5)
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
        ("dec main:\n    () -> int:\n        let x = 1.5\n        return 0\n", DiagnosticCode::FloatLiteralForbidden),
        ("dec main:\n    () -> int:\n        let x = 5.\n        return 0\n", DiagnosticCode::FloatLiteralForbidden),
        ("dec main:\n    () -> int:\n        let x = .5\n        return 0\n", DiagnosticCode::FloatLiteralForbidden),
        ("dec main:\n    () -> int:\n        let x = 1e3\n        return 0\n", DiagnosticCode::FloatLiteralForbidden),
        ("dec main:\n    () -> int:\n        let x = 1E-3\n        return 0\n", DiagnosticCode::FloatLiteralForbidden),
        ("dec main:\n    () -> int:\n        let x = 08_8\n        return 0\n", DiagnosticCode::InvalidOctalAnchor),
        ("dec main:\n    () -> int:\n        let x = 9|-\n        return 0\n", DiagnosticCode::InvalidOctalAnchor),
        ("dec main:\n    () -> int:\n        let x = 0o18\n        return 0\n", DiagnosticCode::InvalidOctalAnchor),
        ("dec main:\n    () -> int:\n        let x = 1/0r\n        return 0\n", DiagnosticCode::ZeroDenominator),
        ("dec main:\n    () -> int:\n        let x = 1:2:9\n        return 0\n", DiagnosticCode::OddSupportRequiresYro),
        ("dec main:\n    () -> int:\n        let x = 3:3:6_yro\n        return 0\n", DiagnosticCode::EvenSupportForbidsYro),
        ("dec main:\n    () -> int:\n        let x = 2t\n        return 0\n", DiagnosticCode::InvalidTritLiteral),
        ("dec main:\n    () -> int:\n        let x = 10t\n        return 0\n", DiagnosticCode::InvalidTritLiteral),
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
    let mix1 = "use aod::core::t32\ndec main:\n    () -> int:\n        let x = 1 + t32(2)\n        return 0\n";
    assert_eq!(
        check_source(mix1, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::ImplicitCoercionForbidden
    );
    let mix2 = "dec main:\n    () -> int:\n        let x = 1 + 1/2r\n        return 0\n";
    assert_eq!(
        check_source(mix2, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::ImplicitCoercionForbidden
    );

    // `let` rebinding forbidden
    let immut = "dec main:\n    () -> int:\n        let x = 1\n        x = 2\n        return x\n";
    assert!(check_source(immut, CompatMode::V2).is_err());

    // Firewall 5: unreachable dead function STILL exhaustively typechecked!
    let dead_bad = r#"
dec dead_helper:
    (x: int) -> int:
        return x + 1/2r

dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source(dead_bad, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::ImplicitCoercionForbidden
    );

    // Fuel budget exhaustion
    let inf_loop = "use aod::core::evolve\ndec st:\n    (s: int, _: int) -> int:\n        return s + 1\ndec main:\n    () -> int:\n        return evolve(0, 0..1000, st)\n";
    assert_eq!(
        run_source_with_budget(inf_loop, CompatMode::V2, 200, 32)
            .unwrap_err()
            .code,
        DiagnosticCode::BudgetExhausted
    );

    // Call depth budget exhaustion
    let inf_rec = "dec rec:\n    (n: int) -> int:\n        return rec(n + 1)\n\ndec main:\n    () -> int:\n        return rec(0)\n";
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
        "const FACTOR = 3\n\ndec triple:\n    (x: int) -> int:\n        return x * FACTOR\n",
    )
    .unwrap();
    let main_path = dir.join("main.dec");
    std::fs::write(
        &main_path,
        "use helper:\n    triple, FACTOR\n\ndec main:\n    () -> int:\n        emit triple(7) + FACTOR\n        return 0\n",
    )
    .unwrap();

    let out = run_file(&main_path, CompatMode::V2).unwrap();
    assert_eq!(out.to_stdout_string(), "24\n0\n");

    // Hostile cyclic import A -> B -> A must fail with ModuleCycleError
    std::fs::write(dir.join("cyc_a.dec"), "use cyc_b\n\ndec a:\n    () -> int:\n        return 1\n").unwrap();
    std::fs::write(dir.join("cyc_b.dec"), "use cyc_a\n\ndec b:\n    () -> int:\n        return 2\n").unwrap();
    let cyc_main = dir.join("cyc_main.dec");
    std::fs::write(&cyc_main, "use cyc_a\n\ndec main:\n    () -> int:\n        return 0\n").unwrap();
    let err = check_file(&cyc_main, CompatMode::V2).unwrap_err();
    assert_eq!(err.code, DiagnosticCode::ModuleCycleError);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ac14_struct_enum_deterministic_map_and_pattern_dispatch() {
    let src = r#"
use aod::core:
    t1, len, contains, map_put, map_keys, map_entries, fold_seq

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

dec classify_route:
    (Route::Closed) -> int:
        return 0
    (Route::Exo(k)) where k > 10 -> int:
        return k * 2
    (Route::Exo(k)) -> int:
        return k
    (Route::Open) -> int:
        return -1

# Multi-arm enum + literal + wildcard + where-guard pattern dispatch
dec dispatch:
    (Seat::SeatedDuad, 0) -> int:
        return 100
    (Seat::SeatedDuad, x: int) where x > 0 -> int:
        return 100 + x
    (_: Seat, x: int) -> int:
        return -x

dec trit_weight:
    (0t, _: int) -> int:
        return 0
    (1t, n: int) -> int:
        return n
    (-1t, n: int) -> int:
        return -n

dec emit_entry:
    (_: Null, (k, v): (bytes, int)) -> Null:
        emit k
        emit v
        return null

dec main:
    () -> int:
        let n = Node(1, 1/2r)
        let n2 = Node(2, 3/4r)
        let p = Pair(4, 9)
        assert n.id == 1 and n2.weight == 3/4r
        assert p.a == 4 and p.b == 9
        let n_upd = Node(7, n.weight)
        assert n_upd.id == 7

        assert classify_route(Route::Closed) == 0
        assert classify_route(Route::Exo(5)) == 5
        assert classify_route(Route::Exo(12)) == 24
        assert classify_route(Route::Open) == -1

        assert dispatch(Seat::SeatedDuad, 0) == 100
        assert dispatch(Seat::SeatedDuad, 7) == 107
        assert dispatch(Seat::OpenDuon, 4) == -4
        assert trit_weight(0t, 5) == 0
        assert trit_weight(1t, 5) == 5
        assert trit_weight(t1(-1), 5) == -5

        # Deterministic BTreeMap map[K, V] insertion, len, contains, and sorted key order!
        let m0: map[bytes, int] = map[]
        let m1 = map_put(m0, b"zeta", 3)
        let m2 = map_put(m1, b"alpha", 1)
        let m = map_put(m2, b"mid", 2)
        assert len(m) == 3
        assert contains(m, b"alpha") == true
        assert contains(m, b"missing") == false
        assert m[b"mid"] == 2

        let literal_map = map[(b"c", 30), (b"a", 10), (b"b", 20)]
        assert len(literal_map) == 3 and len(map_keys(literal_map)) == 3

        let _done = fold_seq(null, map_entries(m), emit_entry)
        return 0
"#;
    let out = run_source(src, CompatMode::V2).unwrap();
    assert_eq!(out.to_stdout_string(), "b\"alpha\"\n1\nb\"mid\"\n2\nb\"zeta\"\n3\n0\n");
}

#[test]
fn ac14_hostile_qext_struct_enum_and_v1_rejections() {
    // 1. Legacy `match` statement rejected at parse time in PureWaveV2
    let legacy_match = r#"
enum Route:
    Closed
    Open

dec bad:
    (r: Route) -> int:
        match r:
            Route::Closed:
                return 0
"#;
    assert_eq!(
        check_source(legacy_match, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::ParseError
    );

    // 2. QExt<T> forbidden inside native struct field
    let qext_struct = r#"
struct BadRec:
    q: QExt<ratio>

dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source(qext_struct, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // 3. QExt<T> forbidden inside native enum payload
    let qext_enum = r#"
enum BadEnum:
    Leak(QExt<ratio>)

dec main:
    () -> int:
        return 0
"#;
    assert_eq!(
        check_source(qext_enum, CompatMode::V2).unwrap_err().code,
        DiagnosticCode::QExtLeakageForbidden
    );

    // 4. Immutable struct field mutation rejected
    let immut_struct = r#"
struct S:
    x: int

dec main:
    () -> int:
        let s = S(1)
        s.x = 2
        return s.x
"#;
    assert!(check_source(immut_struct, CompatMode::V2).is_err());

    // 5. --compat v1 rejects struct, enum, match, and map literals!
    for v2_snippet in [
        "struct S:\n    x: int\nDec main() -> r:\n    () -> int r:\n        r = 0\n",
        "enum E:\n    A\nDec main() -> r:\n    () -> int r:\n        r = 0\n",
        "Dec main() -> r:\n    () -> int r:\n        match 1:\n            _: r = 0\n",
        "Dec main() -> r:\n    () -> int r:\n        int x = map[(1, 2)]\n        r = 0\n",
    ] {
        assert_eq!(
            check_source(v2_snippet, CompatMode::V1).unwrap_err().code,
            DiagnosticCode::ParseError,
            "expected v1 ParseError for:\n{v2_snippet}"
        );
    }
}
