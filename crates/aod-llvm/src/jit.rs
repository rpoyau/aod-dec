#![allow(unsafe_code)]

use std::ffi::CString;

use aod_rt::abi::{runtime_symbol_table, AodRtContext, OwnedRtContext};

use crate::ffi::{
    create_host_target_machine, llvm_api, parse_and_verify_module, run_module_passes,
    take_llvm_error, LLVMJITEvaluatedSymbol, LLVMJITSymbolFlags, LLVMOrcCSymbolMapPair,
    LLVMOrcDefinitionGeneratorRef, LLVMOrcLLJITRef, LLVM_JIT_SYMBOL_EXPORTED,
};
use crate::{LlvmError, LlvmResult, OptLevel};

/// Self-contained in-process LLVM 18 ORC LLJIT session (`P0-1`, `AC-16`).
///
/// Registers all `aod_rt_*` C ABI runtime symbols plus process libc generator
/// (`memcpy`/`memset`/`memmove`), verifies & optimizes IR at `OptLevel`, executes
/// `aod_jit_entry(*mut AodRtContext)`, and cleanly disposes the `LLJIT` on `Drop`.
pub struct OrcJitSession {
    jit: LLVMOrcLLJITRef,
    entry_addr: u64,
}

impl OrcJitSession {
    pub fn compile(ir_text: &str, opt: OptLevel) -> LlvmResult<Self> {
        let api = llvm_api()?;

        let builder = unsafe { (api.orc_create_lljit_builder)() };
        if builder.is_null() {
            return Err(LlvmError::new("LLVMOrcCreateLLJITBuilder returned null"));
        }

        let mut jit: LLVMOrcLLJITRef = std::ptr::null_mut();
        // `LLVMOrcCreateLLJIT` consumes `builder` on both Ok and Err.
        let err = unsafe { (api.orc_create_lljit)(&mut jit, builder) };
        if !err.is_null() || jit.is_null() {
            let msg = unsafe { take_llvm_error(api, err) };
            return Err(LlvmError::new(format!("LLVMOrcCreateLLJIT failed: {msg}")));
        }

        let init_res = (|| -> LlvmResult<u64> {
            let main_jd = unsafe { (api.orc_lljit_get_main_jit_dylib)(jit) };
            if main_jd.is_null() {
                return Err(LlvmError::new("LLVMOrcLLJITGetMainJITDylib returned null"));
            }

            // 1. Attach process symbol search generator (`P0-1`: resolves memcpy/memset/memmove/libc!)
            let prefix = unsafe { (api.orc_lljit_get_global_prefix)(jit) };
            let mut dyn_gen: LLVMOrcDefinitionGeneratorRef = std::ptr::null_mut();
            let err = unsafe {
                (api.orc_create_dynlib_search_gen_for_process)(
                    &mut dyn_gen,
                    prefix,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                )
            };
            if !err.is_null() {
                let msg = unsafe { take_llvm_error(api, err) };
                return Err(LlvmError::new(format!(
                    "LLVMOrcCreateDynamicLibrarySearchGeneratorForProcess failed: {msg}"
                )));
            }
            unsafe {
                (api.orc_jit_dylib_add_generator)(main_jd, dyn_gen);
            }

            // 2. Register all `aod_rt_*` runtime symbols via `LLVMOrcAbsoluteSymbols`
            let syms = runtime_symbol_table();
            let mut pairs: Vec<LLVMOrcCSymbolMapPair> = Vec::with_capacity(syms.len());
            for (name, addr) in syms {
                let cname = CString::new(name)
                    .map_err(|_| LlvmError::new(format!("invalid symbol name {name}")))?;
                let interned = unsafe { (api.orc_lljit_mangle_and_intern)(jit, cname.as_ptr()) };
                pairs.push(LLVMOrcCSymbolMapPair {
                    name: interned,
                    sym: LLVMJITEvaluatedSymbol {
                        address: addr as u64,
                        flags: LLVMJITSymbolFlags {
                            generic_flags: LLVM_JIT_SYMBOL_EXPORTED,
                            target_flags: 0,
                        },
                    },
                });
            }
            // `LLVMOrcAbsoluteSymbols` consumes all interned symbol handles in `pairs`.
            let mu = unsafe { (api.orc_absolute_symbols)(pairs.as_mut_ptr(), pairs.len()) };
            let err = unsafe { (api.orc_jit_dylib_define)(main_jd, mu) };
            if !err.is_null() {
                unsafe { (api.orc_dispose_materialization_unit)(mu) };
                let msg = unsafe { take_llvm_error(api, err) };
                return Err(LlvmError::new(format!(
                    "LLVMOrcJITDylibDefine(aod_rt) failed: {msg}"
                )));
            }

            // 3. Parse, verify, and optimize IR inside a ThreadSafeContext (`P0-1`)
            let ts_ctx = unsafe { (api.orc_create_new_thread_safe_context)() };
            if ts_ctx.is_null() {
                return Err(LlvmError::new("LLVMOrcCreateNewThreadSafeContext returned null"));
            }
            let ctx = unsafe { (api.orc_thread_safe_context_get_context)(ts_ctx) };
            let module = match unsafe { parse_and_verify_module(api, ctx, ir_text) } {
                Ok(m) => m,
                Err(e) => {
                    unsafe { (api.orc_dispose_thread_safe_context)(ts_ctx) };
                    return Err(e);
                }
            };

            let opt_res = (|| -> LlvmResult<()> {
                let triple_ptr = unsafe { (api.orc_lljit_get_triple_string)(jit) };
                let dl_ptr = unsafe { (api.orc_lljit_get_data_layout_str)(jit) };
                unsafe {
                    if !triple_ptr.is_null() {
                        (api.set_target)(module, triple_ptr);
                    }
                    if !dl_ptr.is_null() {
                        (api.set_data_layout)(module, dl_ptr);
                    }
                }
                let (tm, _, _) =
                    unsafe { create_host_target_machine(api, opt.codegen_opt_level())? };
                let pass_res = unsafe { run_module_passes(api, module, tm, opt.pass_pipeline()) };
                unsafe { (api.dispose_target_machine)(tm) };
                pass_res
            })();
            if let Err(e) = opt_res {
                unsafe {
                    (api.dispose_module)(module);
                    (api.orc_dispose_thread_safe_context)(ts_ctx);
                }
                return Err(e);
            }

            // `LLVMOrcCreateNewThreadSafeModule` consumes `module` (do NOT dispose `module`),
            // and we DO dispose our `ts_ctx` handle (`P0-1`).
            let tsm = unsafe { (api.orc_create_new_thread_safe_module)(module, ts_ctx) };
            unsafe { (api.orc_dispose_thread_safe_context)(ts_ctx) };
            if tsm.is_null() {
                return Err(LlvmError::new("LLVMOrcCreateNewThreadSafeModule returned null"));
            }

            // `LLVMOrcLLJITAddLLVMIRModule` consumes `tsm` on success; dispose ONLY on error!
            let err = unsafe { (api.orc_lljit_add_llvm_ir_module)(jit, main_jd, tsm) };
            if !err.is_null() {
                unsafe { (api.orc_dispose_thread_safe_module)(tsm) };
                let msg = unsafe { take_llvm_error(api, err) };
                return Err(LlvmError::new(format!(
                    "LLVMOrcLLJITAddLLVMIRModule failed: {msg}"
                )));
            }

            // 4. Materialize & lookup `aod_jit_entry`
            let entry_c = CString::new("aod_jit_entry").unwrap();
            let mut addr: u64 = 0;
            let err = unsafe { (api.orc_lljit_lookup)(jit, &mut addr, entry_c.as_ptr()) };
            if !err.is_null() || addr == 0 {
                let msg = unsafe { take_llvm_error(api, err) };
                return Err(LlvmError::new(format!(
                    "LLVMOrcLLJITLookup(aod_jit_entry) failed: {msg}"
                )));
            }

            Ok(addr)
        })();

        match init_res {
            Ok(entry_addr) => Ok(Self { jit, entry_addr }),
            Err(e) => {
                let dispose_err = unsafe { (api.orc_dispose_lljit)(jit) };
                if !dispose_err.is_null() {
                    let _ = unsafe { take_llvm_error(api, dispose_err) };
                }
                Err(e)
            }
        }
    }

    /// Execute the compiled `aod_jit_entry` in-process with the given `OwnedRtContext`.
    pub fn execute(&self, ctx: &mut OwnedRtContext) {
        // SAFETY: `self.entry_addr` was verified by LLVM 18 and materialized with exact signature
        // `extern "C" fn(*mut AodRtContext)`.
        let entry_fn: unsafe extern "C" fn(*mut AodRtContext) =
            unsafe { std::mem::transmute(self.entry_addr as usize) };
        unsafe {
            entry_fn(ctx.as_mut_ptr());
        }
    }

    /// Convenience helper: execute with explicit fuel & call-depth budget, capturing stdout.
    pub fn run_captured(&self, fuel: i64, max_depth: i32) -> (String, Result<(), String>) {
        let mut ctx = OwnedRtContext::new(fuel, max_depth, true);
        self.execute(&mut ctx);
        let stdout = ctx.stdout_string();
        if ctx.failed() {
            let msg = ctx
                .error_message()
                .unwrap_or("runtime error")
                .to_string();
            (stdout, Err(msg))
        } else {
            (stdout, Ok(()))
        }
    }
}

impl Drop for OrcJitSession {
    fn drop(&mut self) {
        if !self.jit.is_null() {
            if let Ok(api) = llvm_api() {
                let err = unsafe { (api.orc_dispose_lljit)(self.jit) };
                if !err.is_null() {
                    let _ = unsafe { take_llvm_error(api, err) };
                }
            }
            self.jit = std::ptr::null_mut();
        }
    }
}
