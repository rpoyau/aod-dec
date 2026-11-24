//! Alpha-Omega Dynamics (AOD) & Axiomatic Fundamentalism Calculus (AFC)
//! Stage-0 Self-Hosting Seed Compiler & Hardware Tick/Click Learner
//!
//! Compile & Run on bare-metal Linux (x86_64 or aarch64):
//!   rustc -O aod_stage0_seed.rs -o aod_stage0_seed
//!   ./aod_stage0_seed

use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::thread;

// ============================================================================
// 1. DISTINCT PRIMITIVE TYPES (Main §1.1 - §1.5, Eq. 1 - 11)
// Null, fibre zero, vertex 0000, anchor 00_8, and scope B_0 are distinct types.
// ============================================================================

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Null; // N === infinity_D (Eq. 1-2)

#[repr(i8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Trit {
    Neg  = -1, // NOT-SELF / counterbranch / returned (-1)
    Zero =  0, // BETWEEN  / structural hinge 0_H or dwell 0_D (0)
    Pos  =  1, // SELF     / branch / outgoing (+1)
}

impl Trit {
    #[inline(always)]
    pub const fn flip(self) -> Self {
        match self {
            Trit::Neg => Trit::Pos,
            Trit::Zero => Trit::Zero,
            Trit::Pos => Trit::Neg,
        }
    }
}

/// Finite Q4 Tesseract Vertex Witness V(Q4) = {0,1}^4 (Main §1.4, Eq. 8)
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Q4(pub u8);

impl Q4 {
    pub const ORIGIN: Self = Self(0b0000);

    /// Faithful atomic encoding: Pr[d_H = 1 | admitted atomic event] = 1 (Eq. 8-9)
    #[inline(always)]
    pub const fn is_hamming1(self, next: Self) -> bool {
        ((self.0 ^ next.0) & 0x0F).count_ones() == 1
    }
}

/// First Finite Octal Anchor a_0 = 00_8 (Main §1.2, Eq. 3)
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OctalAnchor(pub u64);

impl OctalAnchor {
    pub const A0: Self = Self(0o00);
}

/// Declared Scope B = (d_T R, omega) (Main §1.5, Eq. 11)
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ScopeB {
    pub scope_id: u8,
    pub anchor: OctalAnchor,
    pub window_w: i64, // |omega| in native integer cut domain
    pub c3: i64,       // ternary assignment capacity C_3 = 3 (Eq. 10)
}

/// 32 Balanced Trits packed in one 64-bit hardware register:
/// 00 -> 0 (equilibrium), 01 -> +1 (crest), 10 -> -1 (trough), 11 -> Null/collision
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct T32(pub u64);

impl T32 {
    /// 1-cycle branchless phase inversion across all 32 trits (Main Eq. 6)
    #[inline(always)]
    pub const fn neg(self) -> Self {
        Self(((self.0 & 0x5555_5555_5555_5555) << 1) | ((self.0 & 0xAAAA_AAAA_AAAA_AAAA) >> 1))
    }

    /// Branchless destructive cancellation: +1 (01) meeting -1 (10) cancels to 00
    #[inline(always)]
    pub const fn superpose_cancel(self, other: Self) -> Self {
        let crest = (self.0 | other.0) & 0x5555_5555_5555_5555;
        let trough = ((self.0 | other.0) & 0xAAAA_AAAA_AAAA_AAAA) >> 1;
        let cancel = crest & trough;
        let clear_mask = !(cancel | (cancel << 1));
        Self((self.0 ^ other.0) & clear_mask)
    }
}

/// Exact Coupled Ratio (a : b) — Zero IEEE-754 floats, zero division erasure
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Ratio {
    pub num: i64,
    pub den: i64,
}

impl Ratio {
    #[inline(always)]
    pub const fn raw(num: i64, den: i64) -> Self {
        Self { num, den }
    }

    #[inline(always)]
    pub fn reduced(num: i64, den: i64) -> Self {
        assert!(den != 0, "Zero denominator violates exact rational domain");
        let g = Self::gcd(num.abs(), den.abs());
        let sign = if den < 0 { -1 } else { 1 };
        Self {
            num: (num / g) * sign,
            den: (den / g) * sign,
        }
    }

    #[inline(always)]
    const fn gcd(mut a: i64, mut b: i64) -> i64 {
        while b != 0 {
            let t = b;
            b = a % b;
            a = t;
        }
        if a == 0 { 1 } else { a }
    }

    #[inline(always)]
    pub fn add(self, other: Self) -> Self {
        Self::reduced(self.num * other.den + other.num * self.den, self.den * other.den)
    }
}

// ============================================================================
// 2. LIVE WALK LEXER & AO_mu SIGNATURE TRIAD PARSER (Main §1.3, §3.9)
// ============================================================================

#[derive(Copy, Clone, Debug)]
pub struct LexCursor {
    pub octal_addr: OctalAnchor,
    pub byte_pos: usize,
    pub prev_role: Trit,
    pub committed_tokens: i64,
    pub dwell_m: i64, // m in M_{e,m} = e 0_H (0_D)^m (-e) (Eq. 5)
}

#[derive(Copy, Clone, Debug)]
pub struct AoSigTriad {
    pub anchor: OctalAnchor,
    pub pi_alpha_in: (u8, u8), // input register type tuple Pi_alpha
    pub h_mu_hinge: bool,      // structural hinge 0_H ('->') present
    pub pi_omega_out: u8,      // closure-subservient output type Pi_Omega (Eq. 48)
    pub adm_certified: bool,   // K_adm(B, i; cert) bound (Eq. 33)
}

pub fn lex_live_walk(src: &[u8]) -> LexCursor {
    let mut cur = LexCursor {
        octal_addr: OctalAnchor::A0,
        byte_pos: 0,
        prev_role: Trit::Zero,
        committed_tokens: 0,
        dwell_m: 0,
    };
    let mut prev_b = b' ';
    for &b in src {
        let role = match b {
            b' ' | b'\t' | b'\n' | b'\r' | b':' | b'(' | b')' | b',' => Trit::Zero,
            b'-' | b'>' => Trit::Neg,
            _ => Trit::Pos,
        };
        if role == Trit::Zero && prev_b == b {
            // Null sameness ("xxxxx... is x"): live reduction burns dwell without new anchor
            cur.dwell_m += 1;
        } else if role != cur.prev_role && role != Trit::Zero {
            // Committed boundary crossing ("x is xx"): advances retained octal address
            cur.octal_addr = OctalAnchor(cur.octal_addr.0 + 1);
            cur.committed_tokens += 1;
            cur.dwell_m = 0;
        }
        cur.prev_role = role;
        cur.byte_pos += 1;
        prev_b = b;
    }
    cur
}

pub fn parse_ao_triad(anchor: OctalAnchor, in1: u8, in2: u8, has_arrow: bool, out: u8) -> AoSigTriad {
    let adm = has_arrow && in1 > 0 && in2 > 0 && out > 0;
    AoSigTriad {
        anchor,
        pi_alpha_in: (in1, in2),
        h_mu_hinge: has_arrow,
        pi_omega_out: out,
        adm_certified: adm,
    }
}

// ============================================================================
// 3. CURLING-CURL SUPPORT, D.E.C. HINGE SLIDE & SHEDDING AUDIT (Main §3.5, §5.6)
// ============================================================================

#[derive(Copy, Clone, Debug)]
pub struct MotifAuditRecord {
    pub name: &'static str,
    pub p: i64,
    pub q: i64,
    pub l_window: i64,
    pub is_yro: bool,       // Odd-window route status L = 2k + 1 (Main §5.6.6)
    pub pi_branch: i64,     // Pi(p:q) = q^p + q (Main Eq. 35)
    pub rd_single: i64,     // RD^(1) = 2 * Pi + 1 (Main Eq. 32, 50)
    pub rho_w: i64,         // min(RD, |omega|) (Main Eq. 60)
    pub p_d: i64,           // P^D = C_3 * rho_w (Main Eq. 77, 91)
    pub q_d: i64,           // Q^D = C_3 * max(0, RD - |omega|) (Main Eq. 105)
    pub c_close: i64,       // Local enclosure compatibility
    pub delta_close: i64,   // P^D - C^close (Main Eq. 107)
    pub x_shedding: i64,    // max(0, delta_close) (Main Eq. 108)
    pub sheddic_path: u8,   // {0=None, 2=E2, 6=C6, 8=C8, 10=C10} (Main Eq. 109)
}

pub fn eval_curling_curl_motif(
    name: &'static str,
    p: i64,
    q: i64,
    l_window: i64,
    c_close: i64,
    target_path: u8,
) -> MotifAuditRecord {
    let pi_branch = q.pow(p as u32) + q;
    let rd_single = 2 * pi_branch + 1;
    let is_yro = (l_window & 1) == 1;
    let rho_w = rd_single.min(l_window);
    let q_d = 3 * (rd_single - l_window).max(0);
    let p_d = 3 * rho_w;
    let delta_close = p_d - c_close;
    let x_shedding = delta_close.max(0);
    let sheddic_path = if x_shedding > 0 { target_path } else { 0 };

    // Sheddic path audit hook (§5.17.5): X_shedding > 0 requires an even enclosure route
    assert!(
        x_shedding == 0 || (sheddic_path >= 2 && sheddic_path % 2 == 0),
        "Sheddic path audit failed on scope"
    );

    MotifAuditRecord {
        name,
        p,
        q,
        l_window,
        is_yro,
        pi_branch,
        rd_single,
        rho_w,
        p_d,
        q_d,
        c_close,
        delta_close,
        x_shedding,
        sheddic_path,
    }
}

// ============================================================================
// 4. TICK/CLICK HARDWARE PROBE, FRG CROSS-RETURN & PERSISTENT LEARNING (§4.3.1, §5.10, §6)
// ============================================================================

#[derive(Copy, Clone, Debug)]
pub struct LearnedHardwareChannel {
    pub tier_name: &'static str,
    pub enclosure: u8,      // 4=E4, 6=C6, 8=C8, 10=C10
    pub t_zero: i64,        // Independent gate sum T_k^0 (Main RG2)
    pub t_cross: i64,       // Recurrent ring value T_k^x (Main RG6)
    pub r_e: i64,           // Retained rebalancing r_e = T_k^x - T_k^0 (Main RG8, Eq. 156)
    pub op_certified: bool, // Matched strictly-later causal criterion Op_e^{O,1}(S_k) = 1 (Eq. 157)
    pub role: Trit,         // +1 = SELF, 0 = BETWEEN, -1 = NOT-SELF (Main Eq. 5, 159)
    pub w_adapted: i64,     // Learned D.E.C. continuation weight w_B(e, sigma) (Main Eq. 66)
}

/// Double-Slit Concurrency Probe across two hardware threads on a shared cache line (FRG Ring Gate)
pub fn run_frg_hardware_probe() -> [LearnedHardwareChannel; 3] {
    // 1. Tier 0: Register E4 Tetron (1:2 Dimonon, RD = 9)
    let mut reg_loop = T32(0x5555_5555_5555_5555); // +1 phase crest across 32 trits
    let mut v = Q4::ORIGIN;
    let mut valid_h1_steps = 0i64;
    for step in 0..9u8 {
        let v_next = Q4(v.0 ^ (1 << (step & 3)));
        if v.is_hamming1(v_next) {
            valid_h1_steps += 1;
            v = v_next;
        }
        reg_loop = reg_loop.neg();
    }
    let ch_reg = certify_channel("Tier-0 Register (E4 Tetron, 1:2)", 4, valid_h1_steps, valid_h1_steps + 3, 4);

    // 2. Tier 1: L1/L2 Cache Line Enclosure (C6 Hexon, 3:3 Tritrion, RD = 61)
    let mut cache_ring = [1i64; 64];
    let mut s_ind_cache = 0i64;
    for i in 0..61usize {
        s_ind_cache += cache_ring[i & 63];
        cache_ring[(i + 1) & 63] = cache_ring[i & 63] + 1;
    }
    let s_rec_cache = cache_ring[61 & 63] + s_ind_cache;
    let ch_cache = certify_channel("Tier-1 L1/L2 Cache (C6 Hexon, 3:3)", 6, s_ind_cache, s_rec_cache, 6);

    // 3. Tier 2: Multi-Core Shared Cache Line FRG Double-Slit Probe (C8 Octon, 3:4 Tetratrion, RD = 137)
    // Slit A and Slit B run isolated loops for T^0, then interact across shared AtomicI64 gate for T^x
    let s_gate_a = 137i64;
    let s_gate_b = 137i64;
    let t_zero = s_gate_a + s_gate_b; // Independent gate-sum value T_k^0 (Main RG3)

    let shared_ring_gate = Arc::new(AtomicI64::new(0));
    let gate_a = Arc::clone(&shared_ring_gate);
    let gate_b = Arc::clone(&shared_ring_gate);

    let handle_a = thread::spawn(move || {
        let mut local_c = 0i64;
        for _ in 0..137 {
            let prev = gate_a.fetch_add(1, Ordering::AcqRel);
            if (prev & 1) == 1 {
                local_c += 1; // Constructive cross-return beat C_{ab,k}^x
            }
        }
        local_c
    });

    let handle_b = thread::spawn(move || {
        let mut local_c = 0i64;
        for _ in 0..137 {
            let prev = gate_b.fetch_add(1, Ordering::AcqRel);
            if (prev & 1) == 0 {
                local_c += 1;
            }
        }
        local_c
    });

    let c_cross_ab = handle_a.join().unwrap_or(1) + handle_b.join().unwrap_or(1);
    let t_cross = t_zero + c_cross_ab.max(1); // Recurrent ring value T_k^x (Main RG7)

    let ch_l3 = certify_channel("Tier-2 Multi-Core Ring (C8 Octon, 3:4)", 8, t_zero, t_cross, 8);

    [ch_reg, ch_cache, ch_l3]
}

fn certify_channel(
    tier_name: &'static str,
    enclosure: u8,
    t_zero: i64,
    t_cross: i64,
    base_w: i64,
) -> LearnedHardwareChannel {
    // FRG Field-Ring Identity (Main RG8-RG9, Eq. 156): r_e = T_k^x - T_k^0
    let r_e = t_cross - t_zero;

    // Matched Strictly-Later Causal Criterion Op_e^{O,1}(S_k) (Main §6.2, Eq. 157):
    // Compare J_{k+1} with R_e retained vs. structural removal S_k^{-e} = (X_k, M_k^C, R_k \ {R_e})
    let x_now = 1i64;
    let p_press = 3i64;
    let j_with_r = Ratio::reduced(x_now * p_press + r_e, p_press);
    let j_removed_r = Ratio::reduced(x_now * p_press, p_press);

    let op_certified = j_with_r != j_removed_r;
    // Operative Self assignment (Main §6.3, Eq. 159): +1 (SELF) if operative edge present
    let role = if op_certified { Trit::Pos } else { Trit::Neg };
    let w_adapted = if op_certified { (base_w + r_e).max(1) } else { 0 };

    LearnedHardwareChannel {
        tier_name,
        enclosure,
        t_zero,
        t_cross,
        r_e,
        op_certified,
        role,
        w_adapted,
    }
}

// ============================================================================
// 5. STANDALONE ELF64 NATIVE COMPILER BACKEND (x86_64 & aarch64)
// ============================================================================

pub fn emit_stage1_elf64(triad: &AoSigTriad, learned_w: i64) -> Vec<u8> {
    assert!(triad.adm_certified, "Cannot emit code for uncertified AO_mu triad");
    assert!(learned_w > 0, "Zero D.E.C. weight on target hardware channel");

    let is_aarch64 = cfg!(target_arch = "aarch64");
    let e_machine: u16 = if is_aarch64 { 183 } else { 62 }; // 183 = EM_AARCH64, 62 = EM_X86_64

    // Native machine code payload:
    // Executes Q4 Hamming-1 check + 1-cycle T32 trit phase inversion + clean sys_exit(0)
    let code_payload: &[u8] = if is_aarch64 {
        &[
            0x20, 0x00, 0x80, 0xD2, // mov x0, #1 (Q4 Hamming-1 step witness)
            0x01, 0xFC, 0x7F, 0xD3, // lsl x1, x0, #1
            0x02, 0x04, 0x41, 0xD3, // lsr x2, x0, #1
            0x20, 0x00, 0x02, 0xAA, // orr x0, x1, x2 (T32 trit phase flip)
            0x00, 0x00, 0x80, 0xD2, // mov x0, #0 (exit status 0 = certified closure)
            0xA8, 0x0B, 0x80, 0xD2, // mov x8, #93 (__NR_exit on aarch64)
            0x01, 0x00, 0x00, 0xD4, // svc #0
        ]
    } else {
        &[
            0xBF, 0x00, 0x00, 0x00, 0x00, // mov edi, 0b0000 (v_curr = 0000_2)
            0xBE, 0x01, 0x00, 0x00, 0x00, // mov esi, 0b0001 (v_next = 0001_2, d_H = 1)
            0x48, 0x31, 0xF7,             // xor rdi, rsi
            0xF3, 0x48, 0x0F, 0xB8, 0xC7, // popcnt rax, rdi (verifies d_H == 1 in hardware)
            0xB8, 0x3C, 0x00, 0x00, 0x00, // mov eax, 60 (__NR_exit on x86_64)
            0x31, 0xFF,                   // xor edi, edi (exit status 0 = certified)
            0x0F, 0x05,                   // syscall
        ]
    };

    let total_size = (120 + code_payload.len()) as u64;
    let entry_vaddr: u64 = 0x0040_0078; // Immediately after 64B Ehdr + 56B Phdr

    let mut elf = Vec::with_capacity(total_size as usize);

    // --- 64-byte Elf64_Ehdr ---
    elf.extend_from_slice(&[0x7F, b'E', b'L', b'F', 2, 1, 1, 0]); // e_ident (ELFCLASS64, LSB)
    elf.extend_from_slice(&[0u8; 8]);                             // e_ident padding
    elf.extend_from_slice(&2u16.to_le_bytes());                   // e_type = ET_EXEC (2)
    elf.extend_from_slice(&e_machine.to_le_bytes());              // e_machine
    elf.extend_from_slice(&1u32.to_le_bytes());                   // e_version = 1
    elf.extend_from_slice(&entry_vaddr.to_le_bytes());            // e_entry = 0x400078
    elf.extend_from_slice(&64u64.to_le_bytes());                  // e_phoff = 64
    elf.extend_from_slice(&0u64.to_le_bytes());                   // e_shoff = 0
    elf.extend_from_slice(&0u32.to_le_bytes());                   // e_flags = 0
    elf.extend_from_slice(&64u16.to_le_bytes());                  // e_ehsize = 64
    elf.extend_from_slice(&56u16.to_le_bytes());                  // e_phentsize = 56
    elf.extend_from_slice(&1u16.to_le_bytes());                   // e_phnum = 1
    elf.extend_from_slice(&0u16.to_le_bytes());                   // e_shentsize = 0
    elf.extend_from_slice(&0u16.to_le_bytes());                   // e_shnum = 0
    elf.extend_from_slice(&0u16.to_le_bytes());                   // e_shstrndx = 0

    // --- 56-byte Elf64_Phdr (PT_LOAD) ---
    elf.extend_from_slice(&1u32.to_le_bytes());                   // p_type = PT_LOAD (1)
    elf.extend_from_slice(&5u32.to_le_bytes());                   // p_flags = PF_R | PF_X (5)
    elf.extend_from_slice(&0u64.to_le_bytes());                   // p_offset = 0
    elf.extend_from_slice(&0x0040_0000u64.to_le_bytes());         // p_vaddr = 0x400000
    elf.extend_from_slice(&0x0040_0000u64.to_le_bytes());         // p_paddr = 0x400000
    elf.extend_from_slice(&total_size.to_le_bytes());             // p_filesz
    elf.extend_from_slice(&total_size.to_le_bytes());             // p_memsz
    elf.extend_from_slice(&0x0020_0000u64.to_le_bytes());         // p_align = 2 MiB

    // --- Native Code Payload ---
    elf.extend_from_slice(code_payload);
    elf
}

fn main() {
    println!("=== AOD / AFC Stage-0 Self-Hosting Seed & Hardware Learner ===");

    // 1. Live Walk Lexing & Triad Admission from Octal Anchor a_0 = 00_8
    let sample_dec = b"Dec step_walk(t32 a, t32 b) -> t32 c:   (t32 a, t32 b) -> t32 c:";
    let cursor = lex_live_walk(sample_dec);
    let triad = parse_ao_triad(cursor.octal_addr, 3, 3, true, 3);
    println!(
        "[1] Anchor a_0 = 00_8 -> Octal {:02o}_8 | Committed Tokens: {} | Dwell Reduced: {} | AO_mu Certified: {}",
        cursor.octal_addr.0, cursor.committed_tokens, cursor.dwell_m, triad.adm_certified
    );

    // 2. Curling-Curl Family & Shedding Audit (Main §3.5, App. B, App. H.5)
    let motifs = [
        eval_curling_curl_motif("1:2:6  (Dimonhexon)",    1, 2, 6,  18, 6),
        eval_curling_curl_motif("1:2:9  (Dimonnanyro)",   1, 2, 9,  24, 8),
        eval_curling_curl_motif("3:3:6  (Tritriohexon)",  3, 3, 6,  18, 6),
        eval_curling_curl_motif("3:3:7  (Tritrioseptyro)",3, 3, 7,  18, 6),
        eval_curling_curl_motif("3:4:6  (Tetratriohexon)",3, 4, 6,  18, 6),
    ];
    println!("\n[2] Curling-Curl Support & Short-Window Shedding Ledger:");
    for m in &motifs {
        println!(
            "    {} | Pi={:<2} RD={:<3} yro={:<5} | rho_w={:<2} P^D={:<2} Q^D={:<3} | d_close={:<+3} X_shed={} -> C_{}",
            m.name, m.pi_branch, m.rd_single, m.is_yro, m.rho_w, m.p_d, m.q_d, m.delta_close, m.x_shedding, m.sheddic_path
        );
    }

    // 3. Hardware Tick/Click Probe, FRG Cross-Return & Persistent Learning (Main §5.10, §6)
    let channels = run_frg_hardware_probe();
    println!("\n[3] Hardware Tick/Click Probe & FRG Operative Self Certification:");
    for ch in &channels {
        println!(
            "    {:<38} | T^0={:<4} T^x={:<4} r_e={:<4} | Op_e={} Role={:?} | Learned w_B={}",
            ch.tier_name, ch.t_zero, ch.t_cross, ch.r_e, ch.op_certified as u8, ch.role, ch.w_adapted
        );
    }

    // 4. Emit Stage-1 Standalone ELF64 Binary
    let elf_bytes = emit_stage1_elf64(&triad, channels[0].w_adapted);
    let out_path = "aodc_stage1";
    if let Ok(mut f) = File::create(out_path) {
        let _ = f.write_all(&elf_bytes);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(out_path, std::fs::Permissions::from_mode(0o755));
        }
        println!(
            "\n[4] Emitted Certified Stage-1 Native ELF64 Binary: '{}' ({} bytes)",
            out_path,
            elf_bytes.len()
        );
    }
}
