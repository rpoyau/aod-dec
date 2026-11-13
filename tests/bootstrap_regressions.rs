#![allow(dead_code)]
#[path="../bootstrap/primitives.rs"] mod primitives;
#[path="../bootstrap/native_elf.rs"] mod native_elf;
use primitives::{Q4,T32,Ratio};
use native_elf::{Arch,banner_elf,file_probe_elf};
use std::convert::TryInto;

#[test] fn q4_domain_and_all_edges(){
    for a in 0..=255u8 {assert_eq!(Q4::new(a).is_ok(),a<16);}
    for a in 0..16u8 {for b in 0..16u8 {assert_eq!(Q4::new(a).unwrap().is_hamming1(Q4::new(b).unwrap()),(a^b).count_ones()==1);}}
}
#[test] fn ternary_roundtrip_and_carry(){
    for a in -100..=100 {for b in -100..=100 {
        let aa=T32::from_i64(a).unwrap();let bb=T32::from_i64(b).unwrap();
        assert_eq!(aa.value(),a);assert_eq!(aa.add(bb).unwrap().value(),a+b);
        assert_eq!(aa.neg().value(),-a);assert_eq!(aa.neg().neg(),aa);
    }}
    for v in [-T32::MAX,T32::MAX] {assert_eq!(T32::from_i64(v).unwrap().value(),v);}
    assert!(T32::from_i64(T32::MAX).unwrap().add(T32::from_i64(1).unwrap()).is_err());
    assert!(T32::from_i64(-T32::MAX).unwrap().add(T32::from_i64(-1).unwrap()).is_err());
    assert!(T32::from_i64(i64::MAX).is_err());
}
#[test] fn packed_trit_validation(){
    for lane in 0..32 {assert!(T32::from_bits(3u64<<(2*lane)).is_err());
        for digit in 0..3u64 {let w=T32::from_bits(digit<<(2*lane)).unwrap();assert_eq!(T32::from_i64(w.value()).unwrap(),w);}
    }
}
#[test] fn ratio_overflow_regression(){
    let a=Ratio::reduced(1,4_000_000_000).unwrap();assert_eq!(a.add(a).unwrap(),Ratio::reduced(1,2_000_000_000).unwrap());
    assert_eq!(Ratio::reduced(2,i64::MIN).unwrap(),Ratio::reduced(-1,4_611_686_018_427_387_904).unwrap());
    assert!(Ratio::reduced(1,i64::MIN).is_err());assert!(Ratio::reduced(i64::MIN,-1).is_err());
    assert_eq!(Ratio::reduced(i64::MIN,i64::MIN).unwrap(),Ratio::reduced(1,1).unwrap());
    assert!(Ratio::reduced(1,0).is_err());
    assert!(Ratio::reduced(i64::MAX,1).unwrap().add(Ratio::reduced(1,1).unwrap()).is_err());
}
#[test] fn ratio_small_exact(){
    for a in -30..=30 {for b in 1..=30 {let x=Ratio::reduced(a,b).unwrap();let y=Ratio::reduced(b,a.abs()+1).unwrap();let sum=x.add(y).unwrap();
        assert_eq!(sum.numerator() as i128*b as i128*(a.abs()+1) as i128,sum.denominator() as i128*(a as i128*(a.abs()+1) as i128+b as i128*b as i128));
    }}
}
#[test] fn elf_deterministic_and_header(){
    for arch in [Arch::X86_64,Arch::Aarch64] {
        for emit in [banner_elf as fn(Arch)->Vec<u8>,file_probe_elf] {
            let a=emit(arch);assert_eq!(a,emit(arch));assert_eq!(&a[..4],b"\x7fELF");
            assert_eq!(u64::from_le_bytes(a[96..104].try_into().unwrap()) as usize,a.len());
        }
    }
}
#[test] fn arm_relocations_resolve_to_data_and_code(){
    for bytes in [banner_elf(Arch::Aarch64),file_probe_elf(Arch::Aarch64)] {
        let code=&bytes[120..];
        let first_data=code.windows(4).position(|x|x==b"[DEC").unwrap();
        for offset in (0..first_data).step_by(4) {
            let w=u32::from_le_bytes(code[offset..offset+4].try_into().unwrap());
            if w&0x9f000000==0x10000000 {
                let imm=(((w>>5)&0x7ffff)<<2)|((w>>29)&3);
                let delta=((imm<<11) as i32)>>11;let target=(offset as i64+delta as i64) as usize;
                assert!(code[target..].starts_with(b"[DEC")||code[target..].starts_with(b"Usage:"));
            }
            if w&0xff000010==0x54000000 {
                let delta=(((((w>>5)&0x7ffff)<<13) as i32)>>13)*4;
                let target=offset as i64+delta as i64;assert!(target>=0&&(target as usize)<first_data&&target%4==0);
            }
        }
    }
}
fn tempdir()->std::path::PathBuf {
    use std::sync::atomic::{AtomicU64,Ordering};static N:AtomicU64=AtomicU64::new(0);
    let p=std::env::temp_dir().join(format!("dec-native-tests-{}-{}",std::process::id(),N.fetch_add(1,Ordering::SeqCst)));
    std::fs::create_dir(&p).unwrap();p
}
#[test] fn source_overwrite_guard_and_io_failure(){
    let dir=tempdir();let src=dir.join("a.dec");std::fs::write(&src,b"original").unwrap();
    assert!(native_elf::write_executable(&src,b"ELF").is_err());assert_eq!(std::fs::read(src).unwrap(),b"original");
    assert!(native_elf::write_executable(&dir.join("missing/out"),b"ELF").is_err());std::fs::remove_dir_all(dir).unwrap();
}
#[cfg(all(target_os="linux",any(target_arch="x86_64",target_arch="aarch64")))]
#[test] fn emitted_program_execution(){
    let dir=tempdir();let arch=native_elf::host_arch().unwrap();let banner=dir.join("banner");let probe=dir.join("probe");
    native_elf::write_executable(&banner,&banner_elf(arch)).unwrap();native_elf::write_executable(&probe,&file_probe_elf(arch)).unwrap();
    let r=std::process::Command::new(&banner).output().unwrap();assert!(r.status.success());assert_eq!(r.stdout,native_elf::BANNER);assert!(r.stderr.is_empty());
    let r=std::process::Command::new(&probe).output().unwrap();assert_eq!(r.status.code(),Some(2));assert_eq!(r.stderr,native_elf::PROBE_USAGE);
    let r=std::process::Command::new(&probe).arg(dir.join("absent")).output().unwrap();assert_eq!(r.status.code(),Some(1));assert_eq!(r.stderr,native_elf::PROBE_ERROR);
    let r=std::process::Command::new(&probe).arg(&banner).output().unwrap();assert!(r.status.success());assert_eq!(r.stdout,native_elf::PROBE_OK);
    let r=std::process::Command::new(&probe).arg(&banner).arg(&banner).output().unwrap();assert_eq!(r.status.code(),Some(2));
    std::fs::remove_dir_all(dir).unwrap();
}
