//! Legacy filename retained for patch continuity. This is a banner ELF demo,
//! not a Dec compiler. Use aodc.py for source checking, IR compilation and execution.
mod native_elf;
fn run()->Result<(),Box<dyn std::error::Error>> {
    let args:Vec<_>=std::env::args_os().collect();
    if args.len()!=2 {return Err("Usage: aod_self_host_compiler OUTPUT (banner ELF demo only). Compile Dec using: python3 aodc.py compile SOURCE -o OUTPUT".into());}
    native_elf::write_executable(std::path::Path::new(&args[1]),&native_elf::banner_elf(native_elf::host_arch()?))?;
    println!("Emitted deterministic banner ELF demo; no Dec compilation performed.");Ok(())
}
fn main(){if let Err(e)=run(){eprintln!("{e}");std::process::exit(1);}}
