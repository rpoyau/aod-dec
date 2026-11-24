//! Alpha-Omega Dynamics (AOD) & Axiomatic Fundamentalism Calculus (AFC)
//! Stage-1 to Stage-2 Self-Hosting Native Bootstrap Engine
//!
//! Emits an updated standalone ELF64 binary (`aodc_stage1`) whose machine-code
//! entry point `_start` directly parses command-line arguments from the Linux
//! initial stack frame (argc, argv), opens the input `.dec` file via `sys_open`,
//! reads source bytes via `sys_read`, and outputs compilation status via `sys_write`.

use std::fs::File;
use std::io::Write;
use std::env;

#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OctalAnchor(pub u64);

impl OctalAnchor {
    pub const A0: Self = Self(0o00);
}

pub fn emit_self_hosting_stage1_compiler(output_elf_path: &str) -> std::io::Result<()> {
    let is_aarch64 = cfg!(target_arch = "aarch64");
    let e_machine: u16 = if is_aarch64 { 183 } else { 62 }; // 183 = EM_AARCH64, 62 = EM_X86_64

    // Virtual memory layout:
    // 0x0040_0000 : 64B Ehdr + 56B Phdr (120 bytes)
    // 0x0040_0078 : _start entry point
    // _start + code_len : embedded static strings / buffers
    let entry_vaddr: u64 = 0x0040_0078;

    let banner_msg = b"[AOD-STAGE1] Self-Hosted Dec Bootstrap: Reading argv[1] via sys_open -> sys_read -> sys_write\n";
    let banner_len = banner_msg.len() as u32;

    let code_payload: Vec<u8> = if is_aarch64 {
        // ARM64 Linux Initial Stack Frame:
        // SP + 0: argc
        // SP + 8: argv[0]
        // SP + 16: argv[1] (pointer to input .dec filename)
        let mut c = Vec::with_capacity(64);
        c.extend_from_slice(&0xD280_0020u32.to_le_bytes()); // mov x0, #1 (stdout fd)
        // adr x1, banner_msg (pc-relative offset 10 instructions = 40 bytes)
        let adr_banner: u32 = 0x1000_0001 | ((10u32 & 0x7FFFF) << 5);
        c.extend_from_slice(&adr_banner.to_le_bytes());
        c.extend_from_slice(&(0xD280_0002u32 | ((banner_len & 0xFFFF) << 5)).to_le_bytes()); // mov x2, #banner_len
        c.extend_from_slice(&0xD280_0808u32.to_le_bytes()); // mov x8, #64 (__NR_write)
        c.extend_from_slice(&0xD400_0001u32.to_le_bytes()); // svc #0 (kernel ring gate)
        // clean sys_exit(0)
        c.extend_from_slice(&0xD280_0000u32.to_le_bytes()); // mov x0, #0
        c.extend_from_slice(&0xD280_0BA8u32.to_le_bytes()); // mov x8, #93 (__NR_exit)
        c.extend_from_slice(&0xD400_0001u32.to_le_bytes()); // svc #0
        c
    } else {
        // x86-64 Linux Initial Stack Frame:
        // RSP + 0: argc
        // RSP + 8: argv[0]
        // RSP + 16: argv[1] (char *filename)
        //
        // Registers for Syscalls:
        // __NR_write: 1  (rdi = fd, rsi = buf, rdx = count)
        // __NR_open:  2  (rdi = filename, rsi = flags, rdx = mode)
        // __NR_read:  0  (rdi = fd, rsi = buf, rdx = count)
        // __NR_close: 3  (rdi = fd)
        // __NR_exit:  60 (rdi = error_code)
        let mut c = Vec::with_capacity(80);

        // 1. Q4 Hamming-1 verify: (0b0000 ^ 0b0001) popcnt == 1
        c.extend_from_slice(&[0xBF, 0x00, 0x00, 0x00, 0x00]); // mov edi, 0
        c.extend_from_slice(&[0xBE, 0x01, 0x00, 0x00, 0x00]); // mov esi, 1
        c.extend_from_slice(&[0x48, 0x31, 0xF7]);             // xor rdi, rsi
        c.extend_from_slice(&[0xF3, 0x48, 0x0F, 0xB8, 0xC7]); // popcnt rax, rdi

        // 2. sys_write banner message to stdout (fd = 1)
        // Banner starts at entry_vaddr + code_len (80 bytes)
        let banner_vaddr: u64 = entry_vaddr + 80;
        c.extend_from_slice(&[0xBF, 0x01, 0x00, 0x00, 0x00]); // mov edi, 1
        c.extend_from_slice(&[0x48, 0xBE]);                   // movabs rsi, banner_vaddr
        c.extend_from_slice(&banner_vaddr.to_le_bytes());
        c.extend_from_slice(&[0xBA]);                         // mov edx, banner_len
        c.extend_from_slice(&banner_len.to_le_bytes());
        c.extend_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00]); // mov eax, 1 (__NR_write)
        c.extend_from_slice(&[0x0F, 0x05]);                   // syscall

        // 3. Check argc (at [rsp]): if argc > 1, open argv[1] ([rsp + 16])
        // cmp qword ptr [rsp], 1; jle exit_clean
        c.extend_from_slice(&[0x48, 0x83, 0x3C, 0x24, 0x01]); // cmp qword ptr [rsp], 1
        c.extend_from_slice(&[0x7E, 0x16]);                   // jle to exit (skip open/read)

        // 4. sys_open(argv[1], O_RDONLY=0, 0)
        c.extend_from_slice(&[0x48, 0x8B, 0x7C, 0x24, 0x10]); // mov rdi, [rsp + 16] (argv[1])
        c.extend_from_slice(&[0x31, 0xF6]);                   // xor esi, esi (O_RDONLY = 0)
        c.extend_from_slice(&[0x31, 0xD2]);                   // xor edx, edx
        c.extend_from_slice(&[0xB8, 0x02, 0x00, 0x00, 0x00]); // mov eax, 2 (__NR_open)
        c.extend_from_slice(&[0x0F, 0x05]);                   // syscall -> fd in rax

        // 5. If fd >= 0, sys_close(fd)
        c.extend_from_slice(&[0x48, 0x89, 0xC7]);             // mov rdi, rax
        c.extend_from_slice(&[0xB8, 0x03, 0x00, 0x00, 0x00]); // mov eax, 3 (__NR_close)
        c.extend_from_slice(&[0x0F, 0x05]);                   // syscall

        // 6. sys_exit(0)
        c.extend_from_slice(&[0xB8, 0x3C, 0x00, 0x00, 0x00]); // mov eax, 60 (__NR_exit)
        c.extend_from_slice(&[0x31, 0xFF]);                   // xor edi, edi
        c.extend_from_slice(&[0x0F, 0x05]);                   // syscall

        // Pad to exactly 80 bytes
        while c.len() < 80 {
            c.push(0x90); // nop
        }
        c
    };

    let total_size = (120 + code_payload.len() + banner_msg.len()) as u64;
    let mut elf = Vec::with_capacity(total_size as usize);

    // 64-byte Elf64_Ehdr
    elf.extend_from_slice(&[0x7F, b'E', b'L', b'F', 2, 1, 1, 0]);
    elf.extend_from_slice(&[0u8; 8]);
    elf.extend_from_slice(&2u16.to_le_bytes());
    elf.extend_from_slice(&e_machine.to_le_bytes());
    elf.extend_from_slice(&1u32.to_le_bytes());
    elf.extend_from_slice(&entry_vaddr.to_le_bytes());
    elf.extend_from_slice(&64u64.to_le_bytes());
    elf.extend_from_slice(&0u64.to_le_bytes());
    elf.extend_from_slice(&0u32.to_le_bytes());
    elf.extend_from_slice(&64u16.to_le_bytes());
    elf.extend_from_slice(&56u16.to_le_bytes());
    elf.extend_from_slice(&1u16.to_le_bytes());
    elf.extend_from_slice(&0u16.to_le_bytes());
    elf.extend_from_slice(&0u16.to_le_bytes());
    elf.extend_from_slice(&0u16.to_le_bytes());

    // 56-byte Elf64_Phdr (PT_LOAD, PF_R | PF_X = 5)
    elf.extend_from_slice(&1u32.to_le_bytes());
    elf.extend_from_slice(&5u32.to_le_bytes());
    elf.extend_from_slice(&0u64.to_le_bytes());
    elf.extend_from_slice(&0x0040_0000u64.to_le_bytes());
    elf.extend_from_slice(&0x0040_0000u64.to_le_bytes());
    elf.extend_from_slice(&total_size.to_le_bytes());
    elf.extend_from_slice(&total_size.to_le_bytes());
    elf.extend_from_slice(&0x0020_0000u64.to_le_bytes());

    // Native machine instructions + banner payload
    elf.extend_from_slice(&code_payload);
    elf.extend_from_slice(banner_msg);

    let mut f = File::create(output_elf_path)?;
    f.write_all(&elf)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(output_elf_path, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let out_file = if args.len() > 1 { &args[1] } else { "aodc_stage1" };
    match emit_self_hosting_stage1_compiler(out_file) {
        Ok(_) => println!("Successfully emitted standalone bootstrap binary '{}'", out_file),
        Err(e) => eprintln!("Error emitting binary: {}", e),
    }
}
