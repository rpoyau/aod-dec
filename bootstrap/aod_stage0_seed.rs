//! Optional Rust encoding checks and Linux exit-only ELF demonstration.
//! The executable Dec language is implemented by ../aodc.py.
mod primitives;
mod native_elf;
use primitives::{Q4,T32,Ratio};
fn run()->Result<(),Box<dyn std::error::Error>> {
    let args:Vec<_>=std::env::args_os().collect();
    if args.len()!=2 {return Err("Usage: aod_stage0_seed OUTPUT (primitive/ELF demo; use aodc.py for Dec)".into());}
    assert!(Q4::new(0)?.is_hamming1(Q4::new(1)?));
    assert!(Q4::new(17).is_err());
    assert_eq!(T32::from_i64(1)?.add(T32::from_i64(1)?)?.value(),2);
    assert_eq!(Ratio::reduced(1,4_000_000_000)?.add(Ratio::reduced(1,4_000_000_000)?)?,Ratio::reduced(1,2_000_000_000)?);
    native_elf::write_executable(std::path::Path::new(&args[1]),&native_elf::exit_elf(native_elf::host_arch()?))?;
    println!("Primitive checks passed; emitted exit-only ELF demo.");Ok(())
}
fn main(){if let Err(e)=run(){eprintln!("{e}");std::process::exit(1);}}
