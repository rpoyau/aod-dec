#![allow(unsafe_code)]

use std::collections::BTreeMap;
use std::ffi::{c_char, CString};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use aod_core::ns::ContentHash;
pub use aod_rt::abi::SandboxTier;
use aod_rt::abi::{runtime_symbol_table, AodRtContext, OwnedRtContext, SandboxedJitWorker};

use crate::ffi::{
    create_host_target_machine, llvm_api, parse_and_verify_module, run_module_passes,
    take_llvm_error, take_llvm_message, LLVMJITEvaluatedSymbol, LLVMJITSymbolFlags,
    LLVMMemoryBufferRef, LLVMOrcCSymbolMapPair, LLVMOrcDefinitionGeneratorRef, LLVMOrcLLJITRef,
    LLVM_JIT_SYMBOL_EXPORTED, LLVM_OBJECT_FILE,
};
use crate::plan::{CodegenPlan, OptLevel};
use crate::{LlvmError, LlvmResult};

const MAX_JIT_CACHE_ENTRIES: usize = 256;

static JIT_CACHE_HITS: AtomicU64 = AtomicU64::new(0);
static JIT_CACHE_MISSES: AtomicU64 = AtomicU64::new(0);
static JIT_CODE_CACHE: OnceLock<Mutex<BTreeMap<ContentHash, Arc<[u8]>>>> = OnceLock::new();

fn jit_cache_map() -> &'static Mutex<BTreeMap<ContentHash, Arc<[u8]>>> {
    JIT_CODE_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Process-wide content-addressed `JitCodeCache` telemetry (`Provider.id` / `(IR, CodegenPlan)` deduplication).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JitCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub entries: usize,
}

pub fn jit_cache_stats() -> JitCacheStats {
    let entries = jit_cache_map().lock().map(|m| m.len()).unwrap_or(0);
    JitCacheStats {
        hits: JIT_CACHE_HITS.load(Ordering::Relaxed),
        misses: JIT_CACHE_MISSES.load(Ordering::Relaxed),
        entries,
    }
}

pub fn clear_jit_cache() {
    if let Ok(mut guard) = jit_cache_map().lock() {
        guard.clear();
    }
    JIT_CACHE_HITS.store(0, Ordering::Relaxed);
    JIT_CACHE_MISSES.store(0, Ordering::Relaxed);
}

/// Compile verified LLVM IR with `plan` into PIC ELF object bytes, deduplicating identical
/// `(IR, CodegenPlan)` artifacts across the entire process when `use_cache` is `true`.
fn compile_ir_to_object_bytes(
    ir_text: &str,
    plan: &CodegenPlan,
    use_cache: bool,
) -> LlvmResult<Arc<[u8]>> {
    let plan_id = plan.plan_id();
    let cache_key = ContentHash::digest_domain(
        b"aod.jit.obj.v1",
        &[ir_text.as_bytes(), plan_id.as_bytes()],
    );

    if use_cache {
        if let Ok(guard) = jit_cache_map().lock() {
            if let Some(cached) = guard.get(&cache_key) {
                JIT_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
                return Ok(Arc::clone(cached));
            }
        }
        JIT_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
    }

    let api = llvm_api()?;
    let ctx = unsafe { (api.context_create)() };
    if ctx.is_null() {
        return Err(LlvmError::new("LLVMContextCreate returned null"));
    }

    let emit_res = (|| -> LlvmResult<Arc<[u8]>> {
        let module = unsafe { parse_and_verify_module(api, ctx, ir_text)? };
        let mod_res = (|| -> LlvmResult<Arc<[u8]>> {
            let (tm, triple_c, dl_c) = unsafe {
                create_host_target_machine(
                    api,
                    plan.opt_level.codegen_opt_level(),
                    plan.target_cpu_native,
                )?
            };
            let tm_res = (|| -> LlvmResult<Arc<[u8]>> {
                unsafe {
                    (api.set_target)(module, triple_c.as_ptr());
                    (api.set_data_layout)(module, dl_c.as_ptr());
                    run_module_passes(api, module, tm, plan.opt_level.pass_pipeline())?;
                }
                let mut err_msg: *mut c_char = std::ptr::null_mut();
                let mut obj_buf: LLVMMemoryBufferRef = std::ptr::null_mut();
                let rc = unsafe {
                    (api.target_machine_emit_to_mem_buf)(
                        tm,
                        module,
                        LLVM_OBJECT_FILE,
                        &mut err_msg,
                        &mut obj_buf,
                    )
                };
                if rc != 0 || obj_buf.is_null() {
                    let msg = unsafe { take_llvm_message(api, err_msg) };
                    return Err(LlvmError::new(format!(
                        "LLVMTargetMachineEmitToMemoryBuffer failed: {msg}"
                    )));
                }
                if !err_msg.is_null() {
                    let _ = unsafe { take_llvm_message(api, err_msg) };
                }
                let ptr = unsafe { (api.get_buffer_start)(obj_buf) } as *const u8;
                let len = unsafe { (api.get_buffer_size)(obj_buf) };
                let slice = if len == 0 || ptr.is_null() {
                    &[][..]
                } else {
                    unsafe { std::slice::from_raw_parts(ptr, len) }
                };
                let arc_bytes: Arc<[u8]> = Arc::from(slice);
                unsafe { (api.dispose_memory_buffer)(obj_buf) };
                Ok(arc_bytes)
            })();
            unsafe { (api.dispose_target_machine)(tm) };
            tm_res
        })();
        unsafe { (api.dispose_module)(module) };
        mod_res
    })();

    unsafe { (api.context_dispose)(ctx) };

    let obj_bytes = emit_res?;
    if use_cache {
        if let Ok(mut guard) = jit_cache_map().lock() {
            if guard.len() >= MAX_JIT_CACHE_ENTRIES {
                guard.clear();
            }
            guard.insert(cache_key, Arc::clone(&obj_bytes));
        }
    }
    Ok(obj_bytes)
}

unsafe fn add_object_bytes_to_main_jd(
    api: &crate::ffi::LlvmApi,
    jit: LLVMOrcLLJITRef,
    obj_bytes: &[u8],
) -> LlvmResult<()> {
    let main_jd = unsafe { (api.orc_lljit_get_main_jit_dylib)(jit) };
    if main_jd.is_null() {
        return Err(LlvmError::new("LLVMOrcLLJITGetMainJITDylib returned null"));
    }
    let buf_name = CString::new("aod_jit_obj.o").unwrap();
    let mem_buf = unsafe {
        (api.create_mem_buf_copy)(
            obj_bytes.as_ptr() as *const c_char,
            obj_bytes.len(),
            buf_name.as_ptr(),
        )
    };
    if mem_buf.is_null() {
        return Err(LlvmError::new(
            "LLVMCreateMemoryBufferWithMemoryRangeCopy(obj) failed",
        ));
    }
    // Note: LLVMOrcLLJITAddObjectFile takes ownership of `mem_buf` unconditionally.
    let err = unsafe { (api.orc_lljit_add_object_file)(jit, main_jd, mem_buf) };
    if !err.is_null() {
        let msg = unsafe { take_llvm_error(api, err) };
        return Err(LlvmError::new(format!(
            "LLVMOrcLLJITAddObjectFile failed: {msg}"
        )));
    }
    Ok(())
}

/// Self-contained in-process LLVM 18 ORC LLJIT session supporting parameterized `CodegenPlan`,
/// process-wide content-addressed `JitCodeCache`, dynamic Inferno `load` modules, and default-on
/// Linux Plan 9 kernel container execution (`SandboxedJitWorker`).
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
        Self::compile_named_internal(ir_text, plan, Some("aod_jit_entry"), true)
    }

    /// Cold-compile bypassing `JitCodeCache` (used strictly by `aodc learn` so candidate compilation
    /// overhead `cand_compile_ns` is always measured honestly from scratch).
    pub fn compile_uncached_with_plan(ir_text: &str, plan: &CodegenPlan) -> LlvmResult<Self> {
        Self::compile_named_internal(ir_text, plan, Some("aod_jit_entry"), false)
    }

    /// Compile an IR module with `plan` and optionally resolve a primary entry symbol.
    /// Useful both for full Dec programs (`Some("aod_jit_entry")`) and raw hardware micro-probes (`None`).
    pub fn compile_named_with_plan(
        ir_text: &str,
        plan: &CodegenPlan,
        entry_symbol: Option<&str>,
    ) -> LlvmResult<Self> {
        Self::compile_named_internal(ir_text, plan, entry_symbol, true)
    }

    fn compile_named_internal(
        ir_text: &str,
        plan: &CodegenPlan,
        entry_symbol: Option<&str>,
        use_cache: bool,
    ) -> LlvmResult<Self> {
        let obj_bytes = compile_ir_to_object_bytes(ir_text, plan, use_cache)?;
        let api = llvm_api()?;

        let (jit_tm, _, _) = unsafe {
            create_host_target_machine(
                api,
                plan.opt_level.codegen_opt_level(),
                plan.target_cpu_native,
            )?
        };
        let jtmb = unsafe { (api.orc_jtmb_create_from_tm)(jit_tm) };
        if jtmb.is_null() {
            unsafe { (api.dispose_target_machine)(jit_tm) };
            return Err(LlvmError::new(
                "LLVMOrcJITTargetMachineBuilderCreateFromTargetMachine returned null",
            ));
        }

        let builder = unsafe { (api.orc_create_lljit_builder)() };
        if builder.is_null() {
            unsafe { (api.orc_dispose_jtmb)(jtmb) };
            return Err(LlvmError::new("LLVMOrcCreateLLJITBuilder returned null"));
        }
        unsafe {
            (api.orc_lljit_builder_set_jtmb)(builder, jtmb);
        }

        let mut jit: LLVMOrcLLJITRef = std::ptr::null_mut();
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

            unsafe { add_object_bytes_to_main_jd(api, jit, &obj_bytes)? };

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

    /// Dynamically link an additional IR module (e.g. an Inferno `mount dyn` capability provider)
    /// into this `OrcJitSession` via the content-addressed `JitCodeCache` WITHOUT recompiling `main`,
    /// evicting any cached pre-`fork` worker so subsequent container runs inherit the new RX pages!
    pub fn add_module_with_plan(&self, ir_text: &str, plan: &CodegenPlan) -> LlvmResult<()> {
        let obj_bytes = compile_ir_to_object_bytes(ir_text, plan, true)?;
        let api = llvm_api()?;
        unsafe { add_object_bytes_to_main_jd(api, self.jit, &obj_bytes)? };
        if let Ok(mut guard) = self.default_worker.lock() {
            let _ = guard.take();
        }
        Ok(())
    }

    /// Look up the materialized address of any exported JIT symbol in this session.
    /// Note: triggering `LLVMOrcLLJITLookup` materializes the object into RX pages; evict any
    /// pre-materialization worker so a freshly spawned container inherits the finalized pages!
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
        if let Ok(mut guard) = self.default_worker.lock() {
            let _ = guard.take();
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
        dyn_vtable: &[u64],
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
            if !dyn_vtable.is_empty() {
                ctx.set_dyn_vtable(dyn_vtable.iter().map(|&a| a as usize).collect());
            }
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
                .run_once_with_dyn(fuel, max_depth, &cli_args, dyn_vtable);
            match outcome {
                Ok(res) => res,
                Err(pipe_err) => {
                    *guard = None;
                    (Vec::new(), true, Some(pipe_err))
                }
            }
        } else {
            match SandboxedJitWorker::spawn(self.entry_addr, rt_ns) {
                Ok(mut w) => match w.run_once_with_dyn(fuel, max_depth, &cli_args, dyn_vtable) {
                    Ok(res) => res,
                    Err(e) => (Vec::new(), true, Some(e)),
                },
                Err(e) => (Vec::new(), true, Some(e)),
            }
        }
    }

    pub fn run_captured(&self, fuel: i64, max_depth: i32) -> (String, Result<(), String>) {
        self.run_captured_with_ns_and_dyn(
            fuel,
            max_depth,
            self.default_rt_ns.clone(),
            Vec::new(),
            &[],
        )
    }

    pub fn run_captured_with_ns(
        &self,
        fuel: i64,
        max_depth: i32,
        rt_ns: aod_rt::ns_fs::RtNamespace,
        cli_args: Vec<Vec<u8>>,
    ) -> (String, Result<(), String>) {
        self.run_captured_with_ns_and_dyn(fuel, max_depth, rt_ns, cli_args, &[])
    }

    pub fn run_captured_with_ns_and_dyn(
        &self,
        fuel: i64,
        max_depth: i32,
        rt_ns: aod_rt::ns_fs::RtNamespace,
        cli_args: Vec<Vec<u8>>,
        dyn_vtable: &[u64],
    ) -> (String, Result<(), String>) {
        let (emits, failed, err_opt) =
            self.run_sandboxed_semantic(fuel, max_depth, rt_ns, cli_args, dyn_vtable);
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
            &[],
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
