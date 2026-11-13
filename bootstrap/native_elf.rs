//! Minimal Linux ELF demonstrations with labels resolved after layout.
//! No parser, compiler, scientific audit, or hardware-learning claim is made.
use std::collections::BTreeMap;
use std::convert::{TryFrom, TryInto};
use std::io::{self, Write};

#[derive(Clone, Copy, Debug)]
pub enum Arch { X86_64, Aarch64 }
pub fn host_arch() -> io::Result<Arch> {
    if cfg!(target_arch="x86_64") { Ok(Arch::X86_64) }
    else if cfg!(target_arch="aarch64") { Ok(Arch::Aarch64) }
    else { Err(io::Error::new(io::ErrorKind::Unsupported,"native demo requires Linux x86_64 or aarch64")) }
}
const BASE: u64 = 0x0040_0000;
const ENTRY: u64 = BASE + 120;
#[derive(Default)]
struct Code { bytes: Vec<u8>, labels: BTreeMap<&'static str, usize>, fixups: Vec<(usize,&'static str,u8)> }
impl Code {
    fn emit(&mut self,b:&[u8]) { self.bytes.extend_from_slice(b); }
    fn word(&mut self,w:u32) { self.emit(&w.to_le_bytes()); }
    fn label(&mut self,n:&'static str) { assert!(self.labels.insert(n,self.bytes.len()).is_none()); }
    fn jump(&mut self,n:&'static str,condition:u8) {
        self.emit(&[0x0f,condition]); self.fixups.push((self.bytes.len(),n,0)); self.emit(&[0;4]);
    }
    fn arm_jump(&mut self,n:&'static str,condition:u32) {
        self.fixups.push((self.bytes.len(),n,2)); self.word(0x54000000|condition);
    }
    fn write(&mut self,arch:Arch,label:&'static str,fd:u32,len:usize) {
        match arch {
            Arch::X86_64 => {
                self.emit(&[0xbf]);self.emit(&fd.to_le_bytes());
                self.emit(&[0x48,0xbe]);self.fixups.push((self.bytes.len(),label,1));self.emit(&[0;8]);
                self.emit(&[0xba]);self.emit(&(len as u32).to_le_bytes());
                self.emit(&[0xb8,1,0,0,0,0x0f,0x05]);
            },
            Arch::Aarch64 => {
                self.word(0xd2800000|(fd<<5));
                self.fixups.push((self.bytes.len(),label,3)); self.word(0x10000001);
                self.word(0xd2800002|((len as u32)<<5)); self.word(0xd2800808);self.word(0xd4000001);
            }
        }
    }
    fn exit(&mut self,arch:Arch,status:u32) {
        match arch {
            Arch::X86_64 => {self.emit(&[0xbf]);self.emit(&status.to_le_bytes());self.emit(&[0xb8,60,0,0,0,0x0f,0x05]);},
            Arch::Aarch64 => {self.word(0xd2800000|(status<<5));self.word(0xd2800ba8);self.word(0xd4000001);}
        }
    }
    fn check_write(&mut self,arch:Arch,len:usize) {
        match arch {
            Arch::X86_64 => {self.emit(&[0x48,0x3d]);self.emit(&(len as u32).to_le_bytes());self.jump("io_fail",0x85);},
            Arch::Aarch64 => {self.word(0xf100001f|((len as u32)<<10));self.arm_jump("io_fail",1);}
        }
    }
    fn finish(mut self) -> Vec<u8> {
        for &(at,label,kind) in &self.fixups {
            let target=self.labels[label];
            match kind {
                0 => {let delta=i32::try_from(target as i64-(at+4) as i64).unwrap();self.bytes[at..at+4].copy_from_slice(&delta.to_le_bytes());},
                1 => self.bytes[at..at+8].copy_from_slice(&(ENTRY+target as u64).to_le_bytes()),
                2 => {
                    let delta=target as i64-at as i64; assert!(delta%4==0 && (-1_048_576..1_048_576).contains(&delta));
                    let mut w=u32::from_le_bytes(self.bytes[at..at+4].try_into().unwrap());w|=(((delta/4) as u32)&0x7ffff)<<5;
                    self.bytes[at..at+4].copy_from_slice(&w.to_le_bytes());
                },
                3 => {
                    let delta=target as i64-at as i64; assert!((-1_048_576..1_048_576).contains(&delta));
                    let u=delta as u32; let w=0x10000001|((u&3)<<29)|(((u>>2)&0x7ffff)<<5);
                    self.bytes[at..at+4].copy_from_slice(&w.to_le_bytes());
                }, _=>unreachable!()
            }
        }
        self.bytes
    }
}
fn elf(arch:Arch,payload:&[u8])->Vec<u8> {
    let size=120+payload.len() as u64;
    let mut e=Vec::new();
    e.extend_from_slice(&[0x7f,b'E',b'L',b'F',2,1,1,0]);e.extend_from_slice(&[0;8]);
    e.extend_from_slice(&2u16.to_le_bytes());e.extend_from_slice(&(match arch {Arch::X86_64=>62u16,Arch::Aarch64=>183u16}).to_le_bytes());
    e.extend_from_slice(&1u32.to_le_bytes());e.extend_from_slice(&ENTRY.to_le_bytes());e.extend_from_slice(&64u64.to_le_bytes());
    e.extend_from_slice(&0u64.to_le_bytes());e.extend_from_slice(&0u32.to_le_bytes());
    for x in [64u16,56,1,0,0,0] {e.extend_from_slice(&x.to_le_bytes());}
    e.extend_from_slice(&1u32.to_le_bytes());e.extend_from_slice(&5u32.to_le_bytes());e.extend_from_slice(&0u64.to_le_bytes());
    e.extend_from_slice(&BASE.to_le_bytes());e.extend_from_slice(&BASE.to_le_bytes());e.extend_from_slice(&size.to_le_bytes());e.extend_from_slice(&size.to_le_bytes());e.extend_from_slice(&0x1000u64.to_le_bytes());
    assert_eq!(e.len(),120);e.extend_from_slice(payload);e
}
pub const BANNER: &[u8] = b"[DEC-NATIVE-DEMO] Standalone Linux ELF write/exit.\n";
pub fn banner_elf(arch:Arch)->Vec<u8> {
    let mut c=Code::default();c.write(arch,"banner",1,BANNER.len());c.check_write(arch,BANNER.len());c.exit(arch,0);
    c.label("io_fail");c.exit(arch,1);c.label("banner");c.emit(BANNER);elf(arch,&c.finish())
}
pub fn exit_elf(arch:Arch)->Vec<u8> {let mut c=Code::default();c.exit(arch,0);elf(arch,&c.finish())}
pub const PROBE_OK: &[u8] = b"[DEC-FILE-PROBE] Open/close succeeded; no compilation performed.\n";
pub const PROBE_USAGE: &[u8] = b"Usage: dec-file-probe FILE\n";
pub const PROBE_ERROR: &[u8] = b"[DEC-FILE-PROBE] Open/close failed.\n";
pub fn file_probe_elf(arch:Arch)->Vec<u8> {
    let mut c=Code::default();
    match arch {
        Arch::X86_64 => {
            c.emit(&[0x48,0x83,0x3c,0x24,0x02]);c.jump("usage",0x85);
            c.emit(&[0x48,0x8b,0x7c,0x24,0x10,0x31,0xf6,0x31,0xd2,0xb8,2,0,0,0,0x0f,0x05]);
            c.emit(&[0x48,0x85,0xc0]);c.jump("failed",0x88);
            c.emit(&[0x48,0x89,0xc7,0xb8,3,0,0,0,0x0f,0x05,0x48,0x85,0xc0]);c.jump("failed",0x88);
        },
        Arch::Aarch64 => {
            c.word(0xf94003e0);c.word(0xf100081f);c.arm_jump("usage",1);
            c.word(0x92800c60); // mov x0, #-100 (AT_FDCWD)
            c.word(0xf9400be1); // ldr x1, [sp,#16] (argv[1])
            c.word(0xd2800002);c.word(0xd2800003);c.word(0xd2800708);c.word(0xd4000001); // openat
            c.word(0xf100001f);c.arm_jump("failed",11);
            c.word(0xd2800728);c.word(0xd4000001);c.word(0xf100001f);c.arm_jump("failed",11); // close
        }
    }
    c.write(arch,"ok_message",1,PROBE_OK.len());c.check_write(arch,PROBE_OK.len());c.exit(arch,0);
    c.label("usage");c.write(arch,"usage_message",2,PROBE_USAGE.len());c.exit(arch,2);
    c.label("failed");c.write(arch,"error_message",2,PROBE_ERROR.len());c.exit(arch,1);
    c.label("io_fail");c.exit(arch,1);
    c.label("ok_message");c.emit(PROBE_OK);c.label("usage_message");c.emit(PROBE_USAGE);c.label("error_message");c.emit(PROBE_ERROR);
    elf(arch,&c.finish())
}
pub fn write_executable(path:&std::path::Path,bytes:&[u8])->io::Result<()> {
    if path.extension().and_then(|x|x.to_str()) == Some("dec") { return Err(io::Error::new(io::ErrorKind::InvalidInput,"refusing to overwrite a .dec source with an ELF demo")); }
    let mut f=std::fs::File::create(path)?;f.write_all(bytes)?;f.flush()?;
    #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;std::fs::set_permissions(path,std::fs::Permissions::from_mode(0o755))?;}
    Ok(())
}
