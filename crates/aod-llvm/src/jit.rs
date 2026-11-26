#![allow(unsafe_code)]

use std::ffi::CString;
use std::sync::Mutex;

pub use aod_rt::abi::SandboxTier;
use aod_rt::abi::{runtime_symbol_table, AodRtContext, OwnedRtContext, SandboxedJitWorker};

use crate::ffi::{
    create_host_target_machine, llvm_api, parse_and_verify_module, run_module_passes,
    take_llvm_error, LLVMJITEvaluatedSymbol, LLVMJITSymbolFlags, LLVMOrcCSymbolMapPair,
    LLVMOrcDefinitionGeneratorRef, LLVMOrcLLJITRef, LLVM_JIT_SYMBOL_EXPORTED,
};
use crate::plan::{CodegenPlan, OptLevel};
use crate::{LlvmError, LlvmResult};

/// Self-contained in-process LLVM 18 ORC LLJIT session supporting parameterized `CodegenPlan`
/// and default-on Linux Plan 9 kernel container execution (`SandboxedJitWorker`).
pub struct OrcJitSession {
    jit: LLVMOrcLLJITRef,
    entry_addr: u64,
    default_rt_ns: aod_rt::ns_fs::RtNamespace,
    default_worker: Mutex<Option<SandboxedJitWorker>>,
}

impl OrcJitSession {
    pub fn compile(ir_text: &str, opt: OptLevel) -> LlvmResult<Self> {
        Self::compile_with_plan(ir_text, &CodegenPlan::for_opt_level(opt))
    }

    pub fn compile_with_plan(ir_text: &str, plan: &CodegenPlan) -> LlvmResult<Self> {
        Self::compile_named_with_plan(ir_text, plan, Some("aod_jit_entry"))
    }

    /// Compile an IR module with `plan` and optionally resolve a primary entry symbol.
    /// Useful both for full Dec programs (`Some("aod_jit_entry")`) and raw hardware micro-probes (`None`).
    pub fn compile_named_with_plan(
        ir_text: &str,
        plan: &CodegenPlan,
        entry_symbol: Option<&str>,
    ) -> LlvmResult<Self> {
        let api = llvm_api()?;

        // 1. Configure TargetMachine -> JITTargetMachineBuilder -> LLJITBuilder (`P0-1` exact ownership!)
        let (jit_tm, _, _) = unsafe {
            create_host_target_machine(
                api,
                plan.opt_level.codegen_opt_level(),
                plan.target_cpu_native,
            )?
        };
        // `LLVMOrcJITTargetMachineBuilderCreateFromTargetMachine` consumes `jit_tm` (do NOT dispose `jit_tm`!).
        let jtmb = unsafe { (api.orc_jtmb_create_from_tm)(jit_tm) };
        if jtmb.is_null() {
            return Err(LlvmError::new(
                "LLVMOrcJITTargetMachineBuilderCreateFromTargetMachine returned null",
            ));
        }

        let builder = unsafe { (api.orc_create_lljit_builder)() };
        if builder.is_null() {
            unsafe { (api.orc_dispose_jtmb)(jtmb) };
            return Err(LlvmError::new("LLVMOrcCreateLLJITBuilder returned null"));
        }
        // `LLVMOrcLLJITBuilderSetJITTargetMachineBuilder` consumes `jtmb`.
        unsafe {
            (api.orc_lljit_builder_set_jtmb)(builder, jtmb);
        }

        let mut jit: LLVMOrcLLJITRef = std::ptr::null_mut();
        // `LLVMOrcCreateLLJIT` consumes `builder` on BOTH Ok and Err.
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

            // Attach process symbol search generator (resolves memcpy/memset/memmove/libc)
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

            // Register all `aod_rt_*` runtime symbols
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
            let mu = unsafe { (api.orc_absolute_symbols)(pairs.as_mut_ptr(), pairs.len()) };
            let err = unsafe { (api.orc_jit_dylib_define)(main_jd, mu) };
            if !err.is_null() {
                unsafe { (api.orc_dispose_materialization_unit)(mu) };
                let msg = unsafe { take_llvm_error(api, err) };
                return Err(LlvmError::new(format!(
                    "LLVMOrcJITDylibDefine(aod_rt) failed: {msg}"
                )));
            }

            // Parse, verify, optimize (with separate owned `pass_tm`), and post-verify IR
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
                let (pass_tm, _, _) = unsafe {
                    create_host_target_machine(
                        api,
                        plan.opt_level.codegen_opt_level(),
                        plan.target_cpu_native,
                    )?
                };
                let pass_res =
                    unsafe { run_module_passes(api, module, pass_tm, plan.opt_level.pass_pipeline()) };
                unsafe { (api.dispose_target_machine)(pass_tm) };
                pass_res
            })();
            if let Err(e) = opt_res {
                unsafe {
                    (api.dispose_module)(module);
                    (api.orc_dispose_thread_safe_context)(ts_ctx);
                }
                return Err(e);
            }

            let tsm = unsafe { (api.orc_create_new_thread_safe_module)(module, ts_ctx) };
            unsafe { (api.orc_dispose_thread_safe_context)(ts_ctx) };
            if tsm.is_null() {
                return Err(LlvmError::new("LLVMOrcCreateNewThreadSafeModule returned null"));
            }

            let err = unsafe { (api.orc_lljit_add_llvm_ir_module)(jit, main_jd, tsm) };
            if !err.is_null() {
                unsafe { (api.orc_dispose_thread_safe_module)(tsm) };
                let msg = unsafe { take_llvm_error(api, err) };
                return Err(LlvmError::new(format!(
                    "LLVMOrcLLJITAddLLVMIRModule failed: {msg}"
                )));
            }

            let addr = if let Some(sym_name) = entry_symbol {
                let entry_c = CString::new(sym_name)
                    .map_err(|_| LlvmError::new(format!("invalid entry symbol {sym_name}")))?;
                let mut a: u64 = 0;
                let err = unsafe { (api.orc_lljit_lookup)(jit, &mut a, entry_c.as_ptr()) };
                if !err.is_null() || a == 0 {
                    let msg = unsafe { take_llvm_error(api, err) };
                    return Err(LlvmError::new(format!(
                        "LLVMOrcLLJITLookup({sym_name}) failed: {msg}"
                    )));
                }
                a
            } else {
                0
            };

            Ok(addr)
        })();

        match init_res {
            Ok(entry_addr) => Ok(Self {
                jit,
                entry_addr,
                default_rt_ns: aod_rt::ns_fs::RtNamespace::empty(),
                default_worker: Mutex::new(None),
            }),
            Err(e) => {
                let dispose_err = unsafe { (api.orc_dispose_lljit)(jit) };
                if !dispose_err.is_null() {
                    let _ = unsafe { take_llvm_error(api, dispose_err) };
                }
                Err(e)
            }
        }
    }

    pub fn with_default_rt_namespace(mut self, rt_ns: aod_rt::ns_fs::RtNamespace) -> Self {
        self.default_rt_ns = rt_ns;
        self
    }

    /// Look up the materialized address of any exported JIT symbol in this session.
    pub fn lookup_symbol_addr(&self, sym_name: &str) -> LlvmResult<u64> {
        let api = llvm_api()?;
        let cstr = CString::new(sym_name)
            .map_err(|_| LlvmError::new(format!("invalid symbol name {sym_name}")))?;
        let mut addr: u64 = 0;
        let err = unsafe { (api.orc_lljit_lookup)(self.jit, &mut addr, cstr.as_ptr()) };
        if !err.is_null() || addr == 0 {
            let msg = unsafe { take_llvm_error(api, err) };
            return Err(LlvmError::new(format!(
                "LLVMOrcLLJITLookup({sym_name}) failed: {msg}"
            )));
        }
        Ok(addr)
    }

    /// Low-level in-process execution hook (used ONLY when `rt_ns.is_unconfined()` is explicitly requested).
    pub fn execute(&self, ctx: &mut OwnedRtContext) {
        if self.entry_addr == 0 {
            ctx.fail_with("OrcJitSession has no default entry symbol");
            return;
        }
        let entry_fn: unsafe extern "C" fn(*mut AodRtContext) =
            unsafe { std::mem::transmute(self.entry_addr as usize) };
        unsafe {
            entry_fn(ctx.as_mut_ptr());
        }
    }

    /// Ensure the persistent default-empty Linux Plan 9 container worker is spawned and return its tier.
    pub fn ensure_default_sandbox_tier(&self) -> LlvmResult<SandboxTier> {
        if self.entry_addr == 0 {
            return Err(LlvmError::new("OrcJitSession has no default entry symbol"));
        }
        #[cfg(target_os = "linux")]
        {
            let mut guard = self
                .default_worker
                .lock()
                .map_err(|_| LlvmError::new("poisoned JIT sandbox worker mutex"))?;
            if guard.is_none() {
                let w = SandboxedJitWorker::spawn(
                    self.entry_addr,
                    aod_rt::ns_fs::RtNamespace::empty(),
                )
                .map_err(LlvmError::new)?;
                *guard = Some(w);
            }
            Ok(guard.as_ref().unwrap().tier())
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok(SandboxTier::Unconfined)
        }
    }

    fn run_sandboxed_semantic(
        &self,
        fuel: i64,
        max_depth: i32,
        rt_ns: aod_rt::ns_fs::RtNamespace,
        cli_args: Vec<Vec<u8>>,
    ) -> (Vec<String>, bool, Option<String>) {
        if self.entry_addr == 0 {
            return (
                Vec::new(),
                true,
                Some("OrcJitSession has no default entry symbol".to_string()),
            );
        }

        if rt_ns.is_unconfined() || !cfg!(target_os = "linux") {
            let mut ctx = OwnedRtContext::new(fuel, max_depth, true);
            ctx.set_rt_namespace(rt_ns);
            if !cli_args.is_empty() {
                ctx.set_cli_args(cli_args);
            }
            self.execute(&mut ctx);
            return (
                ctx.emits().to_vec(),
                ctx.failed(),
                ctx.error_message().map(ToString::to_string),
            );
        }

        // Reuse the persistent container worker for the common empty-namespace case (`bench`/`learn`/`jit`),
        // or spawn a dedicated ephemeral container projected to custom `--mount` specs!
        if rt_ns.is_empty() {
            let mut guard = match self.default_worker.lock() {
                Ok(g) => g,
                Err(_) => {
                    return (
                        Vec::new(),
                        true,
                        Some("poisoned JIT sandbox worker mutex".to_string()),
                    );
                }
            };
            if guard.is_none() {
                match SandboxedJitWorker::spawn(self.entry_addr, rt_ns) {
                    Ok(w) => *guard = Some(w),
                    Err(e) => return (Vec::new(), true, Some(e)),
                }
            }
            let outcome = guard
                .as_mut()
                .unwrap()
                .run_once(fuel, max_depth, &cli_args);
            match outcome {
                Ok(res) => res,
                Err(pipe_err) => {
                    // Evict crashed/killed worker so subsequent calls don't touch a dead pipe.
                    *guard = None;
                    (Vec::new(), true, Some(pipe_err))
                }
            }
        } else {
            match SandboxedJitWorker::spawn(self.entry_addr, rt_ns) {
                Ok(mut w) => match w.run_once(fuel, max_depth, &cli_args) {
                    Ok(res) => res,
                    Err(e) => (Vec::new(), true, Some(e)),
                },
                Err(e) => (Vec::new(), true, Some(e)),
            }
        }
    }

    pub fn run_captured(&self, fuel: i64, max_depth: i32) -> (String, Result<(), String>) {
        self.run_captured_with_ns(
            fuel,
            max_depth,
            self.default_rt_ns.clone(),
            Vec::new(),
        )
    }

    pub fn run_captured_with_ns(
        &self,
        fuel: i64,
        max_depth: i32,
        rt_ns: aod_rt::ns_fs::RtNamespace,
        cli_args: Vec<Vec<u8>>,
    ) -> (String, Result<(), String>) {
        let (emits, failed, err_opt) =
            self.run_sandboxed_semantic(fuel, max_depth, rt_ns, cli_args);
        let mut stdout = String::new();
        for line in &emits {
            stdout.push_str(line);
            stdout.push('\n');
        }
        if failed {
            (
                stdout,
                Err(err_opt.unwrap_or_else(|| "runtime error".to_string())),
            )
        } else {
            (stdout, Ok(()))
        }
    }

    /// Execute and return full semantic signature `(emits, failed, error)` for Exact Semantic Guard
    /// (runs inside the default Linux Plan 9 container worker!).
    pub fn run_semantic_captured(&self, fuel: i64, max_depth: i32) -> (Vec<String>, bool, Option<String>) {
        self.run_sandboxed_semantic(
            fuel,
            max_depth,
            self.default_rt_ns.clone(),
            Vec::new(),
        )
    }
}

impl Drop for OrcJitSession {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.default_worker.lock() {
            let _ = guard.take();
        }
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
