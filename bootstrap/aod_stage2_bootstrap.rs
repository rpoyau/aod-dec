//! Legacy filename retained for patch continuity. Emits an open/close file probe.
//! Neither this Rust emitter nor the generated probe compiles Dec source.
mod native_elf;
fn run()->Result<(),Box<dyn std::error::Error>> {
    let args:Vec<_>=std::env::args_os().collect();
    if args.len()!=2 {return Err("Usage: aod_stage2_bootstrap OUTPUT (Linux file-probe ELF demo)".into());}
    native_elf::write_executable(std::path::Path::new(&args[1]),&native_elf::file_probe_elf(native_elf::host_arch()?))?;
    println!("Emitted open/close file-probe ELF demo; no compiler bootstrap performed.");Ok(())
}
fn main(){if let Err(e)=run(){eprintln!("{e}");std::process::exit(1);}}
