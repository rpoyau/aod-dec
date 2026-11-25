use std::mem::size_of;

use aod_core::trit_int::{
    fast_pack_t32, TAG_ABSENT_PREFIX, TAG_NULL_SENTINEL, TAG_SPILL_PREFIX, TAG_TRAP_PREFIX,
    T32_LIMIT, TOP_TAG_MASK,
};
use aod_rt::abi::{
    aod_rt_bytes_concat, aod_rt_bytes_hex, aod_rt_bytes_new, aod_rt_bytes_sha256,
    aod_rt_bytes_slice, aod_rt_emit_int, aod_rt_emit_ratio, aod_rt_int_add, aod_rt_int_cmp,
    aod_rt_int_from_i64, aod_rt_int_mul, aod_rt_int_neg, aod_rt_ratio_add, DecInt, DecRatio,
    OwnedRtContext,
};

#[test]
fn dec_int_8b_and_dec_ratio_16b_abi_sizes_and_0b11_tags() {
    assert_eq!(size_of::<DecInt>(), 8, "DecInt must fit in a single 64-bit GPR");
    assert_eq!(size_of::<DecRatio>(), 16, "DecRatio must fit in two 64-bit GPRs");

    let mut ctx = OwnedRtContext::new(10_000, 64, true);
    let p = ctx.as_mut_ptr();

    unsafe {
        // Inline 32-trit boundary: `-T32_LIMIT` stays inline (`< TOP_TAG_MASK`) without arena spill!
        let w_neg_lim = aod_rt_int_from_i64(p, -T32_LIMIT);
        assert!(!ctx.failed());
        assert!(w_neg_lim < TOP_TAG_MASK);
        assert_eq!(ctx.state().arena.len(), 0, "inline -T32_LIMIT must not allocate in arena");

        // Crossing `T32_LIMIT` spills cleanly via `TAG_SPILL_PREFIX` (`0xD000...`):
        let w_pos_lim = aod_rt_int_from_i64(p, T32_LIMIT);
        let w_one = fast_pack_t32(1);
        let w_spilled = aod_rt_int_add(p, w_pos_lim, w_one);
        assert!(!ctx.failed());
        assert_eq!(w_spilled & 0xF000_0000_0000_0000, TAG_SPILL_PREFIX);
        assert_eq!(ctx.state().arena.len(), 1);

        // Negating & comparing spilled vs inline works seamlessly:
        let w_neg_spilled = aod_rt_int_neg(p, w_spilled);
        assert_eq!(aod_rt_int_cmp(p, w_neg_spilled, w_neg_lim), -1);
        assert_eq!(aod_rt_int_cmp(p, w_spilled, w_pos_lim), 1);

        // Huge product & emit parity:
        let w_huge = aod_rt_int_mul(p, w_spilled, w_spilled);
        aod_rt_emit_int(p, w_huge);
        let expected_sq = ((T32_LIMIT as i128 + 1).pow(2)).to_string();
        assert_eq!(ctx.emits(), &[expected_sq]);

        // 2-GPR `DecRatio` inline addition & spill promotion:
        let mut r_out = DecRatio::ZERO;
        aod_rt_ratio_add(
            p,
            fast_pack_t32(23),
            fast_pack_t32(50),
            fast_pack_t32(-9),
            fast_pack_t32(20),
            &mut r_out,
        );
        assert!(!ctx.failed());
        assert!((r_out.num | r_out.den) < TOP_TAG_MASK);
        aod_rt_emit_ratio(p, r_out.num, r_out.den);
        assert_eq!(ctx.emits().last().unwrap(), "1/100");
    }

    // Disjoint `0b11` sentinels (`Null`, `Absent`, `Trap`) must fail closed if passed as `int`!
    for bad in [TAG_NULL_SENTINEL, TAG_ABSENT_PREFIX | 1, TAG_TRAP_PREFIX | 7] {
        let mut hctx = OwnedRtContext::new(1_000, 16, true);
        unsafe {
            let _ = aod_rt_int_neg(hctx.as_mut_ptr(), bad);
        }
        assert!(hctx.failed(), "expected fail-closed on non-int 0b11 sentinel 0x{bad:016x}");
    }
}

#[test]
fn bytes_sha256_hex_and_slice_abi_roundtrip() {
    let mut ctx = OwnedRtContext::new(10_000, 64, true);
    let p = ctx.as_mut_ptr();
    unsafe {
        let a = aod_rt_bytes_new(p, b"ab".as_ptr(), 2);
        let b = aod_rt_bytes_new(p, b"c".as_ptr(), 1);
        let abc = aod_rt_bytes_concat(p, a, b);
        let sub = aod_rt_bytes_slice(p, abc, fast_pack_t32(0), fast_pack_t32(3));
        let dig = aod_rt_bytes_sha256(p, sub);
        let hex = aod_rt_bytes_hex(p, dig);
        assert!(!ctx.failed());
        assert_eq!(
            ctx.state().get_bytes(hex).unwrap(),
            b"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
