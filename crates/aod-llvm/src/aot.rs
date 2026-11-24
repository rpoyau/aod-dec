#![allow(unsafe_code)]

use std::ffi::{c_char, CString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::ffi::{
    create_host_target_machine, llvm_api, parse_and_verify_module, run_module_passes,
    take_llvm_message, LLVM_OBJECT_FILE,
};
use crate::plan::{CodegenPlan, OptLevel};
use crate::{LlvmError, LlvmResult};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn verify_and_optimize_ir(ir_text: &str, opt: OptLevel) -> LlvmResult<String> {
    verify_and_optimize_ir_with_plan(ir_text, &CodegenPlan::for_opt_level(opt))
}

/// Parse, verify, optimize (`plan.opt_level`, `plan.target_cpu_native`), post-verify, and print LLVM IR.
pub fn verify_and_optimize_ir_with_plan(ir_text: &str, plan: &CodegenPlan) -> LlvmResult<String> {
    let api = llvm_api()?;
    let ctx = unsafe { (api.context_create)() };
    if ctx.is_null() {
        return Err(LlvmError::new("LLVMContextCreate failed"));
    }

    let result = (|| -> LlvmResult<String> {
        let module = unsafe { parse_and_verify_module(api, ctx, ir_text)? };
        let res = (|| -> LlvmResult<String> {
            let (tm, triple_c, dl_c) = unsafe {
                create_host_target_machine(
                    api,
                    plan.opt_level.codegen_opt_level(),
                    plan.target_cpu_native,
                )?
            };
            let pass_res = (|| -> LlvmResult<String> {
                unsafe {
                    (api.set_target)(module, triple_c.as_ptr());
                    (api.set_data_layout)(module, dl_c.as_ptr());
                }
                unsafe { run_module_passes(api, module, tm, plan.opt_level.pass_pipeline())? };
                let ir_ptr = unsafe { (api.print_module_to_string)(module) };
                Ok(unsafe { take_llvm_message(api, ir_ptr) })
            })();
            unsafe { (api.dispose_target_machine)(tm) };
            pass_res
        })();
        unsafe { (api.dispose_module)(module) };
        res
    })();

    unsafe { (api.context_dispose)(ctx) };
    result
}

pub fn emit_object_file(ir_text: &str, opt: OptLevel, obj_path: &Path) -> LlvmResult<()> {
    emit_object_file_with_plan(ir_text, &CodegenPlan::for_opt_level(opt), obj_path)
}

pub fn emit_object_file_with_plan(
    ir_text: &str,
    plan: &CodegenPlan,
    obj_path: &Path,
) -> LlvmResult<()> {
    let api = llvm_api()?;
    let obj_str = obj_path
        .to_str()
        .ok_or_else(|| LlvmError::new("non-UTF8 object output path"))?;
    let obj_c = CString::new(obj_str).map_err(|_| LlvmError::new("interior NUL in object path"))?;

    let ctx = unsafe { (api.context_create)() };
    if ctx.is_null() {
        return Err(LlvmError::new("LLVMContextCreate failed"));
    }

    let result = (|| -> LlvmResult<()> {
        let module = unsafe { parse_and_verify_module(api, ctx, ir_text)? };
        let res = (|| -> LlvmResult<()> {
            let (tm, triple_c, dl_c) = unsafe {
                create_host_target_machine(
                    api,
                    plan.opt_level.codegen_opt_level(),
                    plan.target_cpu_native,
                )?
            };
            let emit_res = (|| -> LlvmResult<()> {
                unsafe {
                    (api.set_target)(module, triple_c.as_ptr());
                    (api.set_data_layout)(module, dl_c.as_ptr());
                }
                unsafe { run_module_passes(api, module, tm, plan.opt_level.pass_pipeline())? };
                let mut err_msg: *mut c_char = std::ptr::null_mut();
                let rc = unsafe {
                    (api.target_machine_emit_to_file)(
                        tm,
                        module,
                        obj_c.as_ptr(),
                        LLVM_OBJECT_FILE,
                        &mut err_msg,
                    )
                };
                if rc != 0 {
                    let msg = unsafe { take_llvm_message(api, err_msg) };
                    return Err(LlvmError::new(format!(
                        "LLVMTargetMachineEmitToFile failed: {msg}"
                    )));
                }
                if !err_msg.is_null() {
                    let _ = unsafe { take_llvm_message(api, err_msg) };
                }
                Ok(())
            })();
            unsafe { (api.dispose_target_machine)(tm) };
            emit_res
        })();
        unsafe { (api.dispose_module)(module) };
        res
    })();

    unsafe { (api.context_dispose)(ctx) };
    result
}

pub fn find_aod_rt_lib() -> LlvmResult<PathBuf> {
    if let Ok(p) = std::env::var("AOD_RT_LIB") {
        let pb = PathBuf::from(&p);
        if pb.is_file() {
            return Ok(pb);
        }
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("libaod_rt.a"));
            if let Some(p1) = dir.parent() {
                candidates.push(p1.join("libaod_rt.a"));
                if let Some(p2) = p1.parent() {
                    candidates.push(p2.join("libaod_rt.a"));
                    if let Some(p3) = p2.parent() {
                        candidates.push(p3.join("libaod_rt.a"));
                    }
                }
            }
        }
    }

    let manifest_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf);

    let preferred_profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let fallback_profile = if cfg!(debug_assertions) {
        "release"
    } else {
        "debug"
    };

    if let Some(root) = manifest_root {
        candidates.push(root.join("target").join(preferred_profile).join("libaod_rt.a"));
        candidates.push(root.join("target").join(fallback_profile).join("libaod_rt.a"));
    }

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("target").join(preferred_profile).join("libaod_rt.a"));
        candidates.push(cwd.join("target").join(fallback_profile).join("libaod_rt.a"));
    }

    for c in &candidates {
        if c.is_file() {
            return Ok(c.clone());
        }
    }

    Err(LlvmError::new(format!(
        "could not locate libaod_rt.a (set AOD_RT_LIB or run `cargo build -p aod-rt`); searched {:?}",
        candidates
    )))
}

pub fn link_aot_executable(ir_text: &str, opt: OptLevel, out_exe: &Path) -> LlvmResult<()> {
    link_aot_executable_with_plan(ir_text, &CodegenPlan::for_opt_level(opt), out_exe)
}

pub fn link_aot_executable_with_plan(
    ir_text: &str,
    plan: &CodegenPlan,
    out_exe: &Path,
) -> LlvmResult<()> {
    let rt_lib = find_aod_rt_lib()?;
    let seq = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let obj_path = std::env::temp_dir().join(format!(
        "aodc_aot_{}_{}.o",
        std::process::id(),
        seq
    ));

    let link_res = (|| -> LlvmResult<()> {
        emit_object_file_with_plan(ir_text, plan, &obj_path)?;

        let linker = ["clang", "cc"]
            .into_iter()
            .find(|tool| {
                Command::new(tool)
                    .arg("--version")
                    .output()
                    .is_ok_and(|o| o.status.success())
            })
            .ok_or_else(|| {
                LlvmError::new("neither `clang` nor `cc` found on PATH to link AOT ELF")
            })?;

        let output = Command::new(linker)
            .arg(&obj_path)
            .arg(&rt_lib)
            .args(["-lpthread", "-ldl", "-lm", "-o"])
            .arg(out_exe)
            .output()
            .map_err(|e| LlvmError::new(format!("failed invoking {linker}: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(LlvmError::new(format!("{linker} linking failed:\n{stderr}")));
        }
        Ok(())
    })();

    let _ = std::fs::remove_file(&obj_path);
    link_res
}
