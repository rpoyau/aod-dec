use aod_core::bigint::BigInt;
use aod_core::error::CoreError;
use aod_core::json::{parse_strict_json, JsonValue};
use aod_core::qext::{parse_decimal_exact, QExtRatio};
use aod_core::ratio::Ratio;
use aod_core::sha256::Sha256;
use aod_core::ternary::{Addr8, Q4, T1, T32};

#[test]
fn bigint_large_arithmetic_and_python_floor_division() {
    let a = BigInt::parse_decimal("1234567890123456789012345678901234567890").unwrap();
    let b = BigInt::parse_decimal("98765432109876543210987654321").unwrap();
    let (q, r) = a.div_rem_floor(&b).unwrap();
    assert_eq!(&(&q * &b) + &r, a);
    assert!(!r.is_negative() && r < b);

    // Python // and % exact sign semantics across all 4 sign quadrants
    let cases = [
        (7, 3, 2, 1),
        (-7, 3, -3, 2),
        (7, -3, -3, -2),
        (-7, -3, 2, -1),
        (6, 3, 2, 0),
        (-6, 3, -2, 0),
        (6, -3, -2, 0),
        (-6, -3, 2, 0),
        (0, 5, 0, 0),
        (0, -5, 0, 0),
    ];
    for (x, y, exp_q, exp_r) in cases {
        let bx = BigInt::from_i64(x);
        let by = BigInt::from_i64(y);
        let (q, r) = bx.div_rem_floor(&by).unwrap();
        assert_eq!(q, BigInt::from_i64(exp_q), "{x} // {y}");
        assert_eq!(r, BigInt::from_i64(exp_r), "{x} % {y}");
    }
    assert_eq!(
        BigInt::from_i64(5).div_rem_floor(&BigInt::zero()),
        Err(CoreError::ZeroDivision)
    );

    // gcd, lcm, pow
    assert_eq!(BigInt::from_i64(137).lcm(&BigInt::from_i64(61)), BigInt::from_i64(8357));
    assert_eq!(BigInt::from_i64(-48).gcd(&BigInt::from_i64(18)), BigInt::from_i64(6));
    assert_eq!(BigInt::from_u64(3).pow(32), BigInt::parse_decimal("1853020188851841").unwrap());
}

#[test]
fn bigint_nonnegative_bitwise_rejects_negative_operands() {
    let a = BigInt::from_u64(0b1101_0110);
    let b = BigInt::from_u64(0b1011_0011);
    assert_eq!(a.bitand(&b).unwrap(), BigInt::from_u64(0b1001_0010));
    assert_eq!(a.bitor(&b).unwrap(), BigInt::from_u64(0b1111_0111));
    assert_eq!(a.bitxor(&b).unwrap(), BigInt::from_u64(0b0110_0101));
    assert_eq!(BigInt::one().shl(70).unwrap().shr(70).unwrap(), BigInt::one());

    let neg = BigInt::from_i64(-1);
    assert!(matches!(neg.bitand(&a), Err(CoreError::DomainError(_))));
    assert!(matches!(a.bitand(&neg), Err(CoreError::DomainError(_))));
    assert!(matches!(neg.bitor(&a), Err(CoreError::DomainError(_))));
    assert!(matches!(neg.bitxor(&a), Err(CoreError::DomainError(_))));
    assert!(matches!(neg.shl(1), Err(CoreError::DomainError(_))));
    assert!(matches!(neg.shr(1), Err(CoreError::DomainError(_))));
}

#[test]
fn ratio_canonical_form_rendering_and_exact_int_conversion() {
    assert_eq!(Ratio::from_i64(4, 1).unwrap().render_v1(), "4/1");
    assert_eq!(Ratio::from_i64(0, 99).unwrap().render_v1(), "0/1");
    assert_eq!(Ratio::from_i64(8, -2).unwrap().render_v1(), "-4/1");
    assert_eq!(Ratio::from_i64(-6, -9).unwrap().render_v1(), "2/3");
    assert_eq!(Ratio::from_i64(1, 0), Err(CoreError::ZeroDivision));

    let exact_int = Ratio::from_i64(12, 3).unwrap();
    assert_eq!(exact_int.to_int_exact().unwrap(), BigInt::from_i64(4));
    let non_int = Ratio::from_i64(7, 3).unwrap();
    assert!(matches!(non_int.to_int_exact(), Err(CoreError::DomainError(_))));

    let big_a = Ratio::from_i64(1, 4_000_000_000).unwrap();
    assert_eq!((&big_a + &big_a).render_v1(), "1/2000000000");
}

#[test]
fn qext_strict_decimal_parser_and_hostile_rejections() {
    assert_eq!(parse_decimal_exact("1.25").unwrap().render_exact(), "5/4");
    assert_eq!(parse_decimal_exact("-0.005").unwrap().render_exact(), "-1/200");
    assert_eq!(parse_decimal_exact("+42").unwrap().render_exact(), "42/1");
    assert_eq!(parse_decimal_exact("0.0000").unwrap().render_exact(), "0/1");

    let lifted = QExtRatio::from_native_frozen(Ratio::from_i64(2, 5).unwrap());
    let sum = lifted.checked_add(&parse_decimal_exact("0.6").unwrap());
    assert_eq!(sum.render_exact(), "1/1");

    for bad in [
        "", " ", " 1.25", "1.25 ", ".5", "5.", "+.5", "-5.", "1e3", "1E-3", "1.5e2", "NaN", "Inf",
        "-Inf", "1_000", "1.2.3", "+", "-",
    ] {
        assert!(
            matches!(parse_decimal_exact(bad), Err(CoreError::InvalidDecimalLiteral(_))),
            "expected strict rejection for {bad:?}"
        );
    }
}

#[test]
fn ternary_t1_t32_q4_addr8_invariants() {
    assert_eq!(T1::new(-1).unwrap().render_v1(), "t1(-1)");
    assert!(T1::new(2).is_err());
    assert!(T1::POS.checked_add(T1::POS).is_err());

    assert_eq!(T32::LIMIT, 926_510_094_425_920);
    assert!(T32::new(T32::LIMIT + 1).is_err());
    assert!(T32::new(-T32::LIMIT - 1).is_err());

    for v in [0i128, 1, -1, 2, -2, 42, -999_999, T32::LIMIT, -T32::LIMIT] {
        let t = T32::new(v).unwrap();
        let packed = t.pack();
        let unpacked = T32::unpack(packed).unwrap();
        assert_eq!(unpacked, t, "roundtrip failed for {v}");
    }

    // Hostile 0b11 lane must be rejected with InvalidTritEncoding(3)
    assert_eq!(T32::unpack(0b11), Err(CoreError::InvalidTritEncoding(3)));
    assert_eq!(
        T32::unpack(3u64 << 62),
        Err(CoreError::InvalidTritEncoding(3))
    );
    assert!(T32::unpack_bigint(&BigInt::from_i64(-1)).is_err());
    assert!(T32::unpack_bigint(&BigInt::one().shl(64).unwrap()).is_err());

    // Q4 Hamming-1
    let q0 = Q4::new(0).unwrap();
    let q1 = Q4::new(1).unwrap();
    let q3 = Q4::new(3).unwrap();
    assert!(q0.is_hamming1(q1));
    assert!(!q0.is_hamming1(q3));
    assert_eq!(q0.hamming_distance(q3), 2);
    assert!(Q4::new(16).is_err());

    // Addr8 leading-zero preservation and octal validation
    let a00 = Addr8::parse("00_8").unwrap();
    let a00_bar = Addr8::parse("00|-").unwrap();
    let a00_oct = Addr8::parse("0o00").unwrap();
    let a0 = Addr8::parse("0_8").unwrap();
    assert_eq!(a00, a00_bar);
    assert_eq!(a00, a00_oct);
    assert_ne!(a00, a0, "00_8 must stay distinct from 0_8");
    assert!(a00.is_first_anchor());
    assert!(!a0.is_first_anchor());
    assert!(matches!(Addr8::parse("08_8"), Err(CoreError::InvalidOctalAnchor(_))));
    assert!(matches!(Addr8::parse("9|-"), Err(CoreError::InvalidOctalAnchor(_))));
    assert!(matches!(Addr8::parse("0o18"), Err(CoreError::InvalidOctalAnchor(_))));
}

#[test]
fn sha256_nist_vectors_and_strict_json_python_parity() {
    assert_eq!(
        Sha256::digest_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        Sha256::digest_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        Sha256::digest_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );

    // Python json.dumps(..., ensure_ascii=True) parity including surrogate pairs and 0x7F
    assert_eq!(JsonValue::from_str_val("\x00").to_compact_ascii_json(), "\"\\u0000\"");
    assert_eq!(JsonValue::from_str_val("\x7f").to_compact_ascii_json(), "\"\\u007f\"");
    assert_eq!(JsonValue::from_str_val("∂").to_compact_ascii_json(), "\"\\u2202\"");
    assert_eq!(
        JsonValue::from_str_val("😀").to_compact_ascii_json(),
        "\"\\ud83d\\ude00\""
    );

    // Round-trip surrogate pair + lossless u64::MAX + BigInt > u128::MAX
    let huge = BigInt::one().shl(150).unwrap();
    let encoded = format!(
        "{{\"big\":{},\"emoji\":\"\\ud83d\\ude00\",\"u64max\":{}}}",
        huge,
        u64::MAX
    );
    let parsed = parse_strict_json(&encoded).unwrap();
    assert_eq!(parsed.get("big").unwrap().as_int(), Some(&huge));
    assert_eq!(parsed.get("emoji").unwrap().as_str(), Some("😀"));
    assert_eq!(
        parsed.get("u64max").unwrap().as_int(),
        Some(&BigInt::from_u64(u64::MAX))
    );
    assert_eq!(parsed.to_compact_ascii_json(), encoded);

    // Reject duplicate keys and floating-point JSON tokens
    assert!(matches!(
        parse_strict_json("{\"a\":1,\"a\":2}"),
        Err(CoreError::DuplicateJsonKey(_))
    ));
    assert!(matches!(
        parse_strict_json("{\"x\":1.5}"),
        Err(CoreError::FloatForbidden(_))
    ));
    assert!(matches!(
        parse_strict_json("[1e5]"),
        Err(CoreError::FloatForbidden(_))
    ));
}

#[test]
fn phase_d1_t9_superpose_radix_and_bytes_primitives() {
    use aod_core::ternary::{render_bytes_literal, T9};

    assert_eq!(
        BigInt::parse_hex_underscored("FF_00").unwrap(),
        BigInt::from_u64(0xFF00)
    );
    assert_eq!(
        BigInt::parse_binary_underscored("1010_0101").unwrap(),
        BigInt::from_u64(0xA5)
    );
    for bad_hex in ["", "_1", "1_", "1__2", "1G"] {
        assert!(BigInt::parse_hex_underscored(bad_hex).is_err());
    }
    for bad_bin in ["", "_0", "0_", "1__0", "102"] {
        assert!(BigInt::parse_binary_underscored(bad_bin).is_err());
    }

    let hi = T9::new(9841).unwrap();
    let lo = T9::new(-9841).unwrap();
    assert!(T9::new(9842).is_err());
    assert!(T9::new(-9842).is_err());
    assert_eq!(T9::unpack(hi.pack() as u64).unwrap(), hi);
    assert_eq!(T9::unpack(lo.pack() as u64).unwrap(), lo);
    assert!(T9::unpack(1 << 18).is_err());
    assert!(matches!(
        T9::unpack(0b11),
        Err(CoreError::InvalidTritEncoding(3))
    ));

    assert_eq!(T1::POS.superpose_cancel(T1::NEG), T1::ZERO);
    assert_eq!(T1::POS.superpose_cancel(T1::POS), T1::POS);
    assert_eq!(T1::NEG.superpose_cancel(T1::NEG), T1::NEG);

    let a32 = T32::new(1 + 3 + 9).unwrap();
    let b32 = T32::new(-1 + 3 - 27).unwrap();
    assert_eq!(a32.superpose_cancel(b32), T32::new(-15).unwrap());

    assert_eq!(Addr8::from_u64_min2(0).render(), "00_8");
    assert_eq!(Addr8::from_u64_min2(7).render(), "07_8");
    assert_eq!(Addr8::from_u64_min2(8).render(), "10_8");
    assert_eq!(Addr8::from_u64_min2(64).render(), "100_8");

    assert_eq!(
        render_bytes_literal(b"A\0\n\r\t\\\"'\xff"),
        "b\"A\\0\\n\\r\\t\\\\\\\"'\\xff\""
    );
}

