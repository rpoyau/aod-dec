#![allow(unsafe_code)]

use std::ffi::{c_char, c_int, c_uint, c_void, CStr, CString};
use std::sync::OnceLock;

use crate::{LlvmError, LlvmResult};

// Standard glibc dynamic loader (`libdl`) — zero external crates.io dependencies.
#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
}

const RTLD_NOW: c_int = 0x0002;
const RTLD_GLOBAL: c_int = 0x0100;

// Opaque LLVM C API handles
pub type LLVMContextRef = *mut c_void;
pub type LLVMModuleRef = *mut c_void;
pub type LLVMMemoryBufferRef = *mut c_void;
pub type LLVMTargetRef = *mut c_void;
pub type LLVMTargetMachineRef = *mut c_void;
pub type LLVMTargetDataRef = *mut c_void;
pub type LLVMPassBuilderOptionsRef = *mut c_void;
pub type LLVMErrorRef = *mut c_void;

// ORC LLJIT handles
pub type LLVMOrcLLJITBuilderRef = *mut c_void;
pub type LLVMOrcLLJITRef = *mut c_void;
pub type LLVMOrcJITDylibRef = *mut c_void;
pub type LLVMOrcThreadSafeContextRef = *mut c_void;
pub type LLVMOrcThreadSafeModuleRef = *mut c_void;
pub type LLVMOrcSymbolStringPoolEntryRef = *mut c_void;
pub type LLVMOrcMaterializationUnitRef = *mut c_void;
pub type LLVMOrcDefinitionGeneratorRef = *mut c_void;

pub const LLVM_RETURN_STATUS_ACTION: c_uint = 2;
pub const LLVM_RELOC_PIC: c_uint = 2;
pub const LLVM_CODE_MODEL_DEFAULT: c_uint = 0;
pub const LLVM_OBJECT_FILE: c_uint = 1;
pub const LLVM_JIT_SYMBOL_EXPORTED: u8 = 1;

/// Exact LLVM 18 `LLVMJITSymbolFlags` C ABI layout (`Orc.h`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LLVMJITSymbolFlags {
    pub generic_flags: u8,
    pub target_flags: u8,
}

/// Exact LLVM 18 `LLVMJITEvaluatedSymbol` C ABI layout (`Orc.h`, 16 bytes, align 8).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LLVMJITEvaluatedSymbol {
    pub address: u64,
    pub flags: LLVMJITSymbolFlags,
}

/// Exact LLVM 18 `LLVMOrcCSymbolMapPair` C ABI layout (`Orc.h`, 24 bytes, align 8).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LLVMOrcCSymbolMapPair {
    pub name: LLVMOrcSymbolStringPoolEntryRef,
    pub sym: LLVMJITEvaluatedSymbol,
}

// Compile-time layout assertions matching LLVM 18 C headers on x86-64.
const _: () = {
    assert!(std::mem::size_of::<LLVMJITEvaluatedSymbol>() == 16);
    assert!(std::mem::size_of::<LLVMOrcCSymbolMapPair>() == 24);
};

pub struct LlvmApi {
    // Core / IR / Verification / Passes
    pub context_create: unsafe extern "C" fn() -> LLVMContextRef,
    pub context_dispose: unsafe extern "C" fn(LLVMContextRef),
    pub create_mem_buf_copy:
        unsafe extern "C" fn(*const c_char, usize, *const c_char) -> LLVMMemoryBufferRef,
    pub parse_ir_in_context: unsafe extern "C" fn(
        LLVMContextRef,
        LLVMMemoryBufferRef,
        *mut LLVMModuleRef,
        *mut *mut c_char,
    ) -> c_int,
    pub verify_module:
        unsafe extern "C" fn(LLVMModuleRef, c_uint, *mut *mut c_char) -> c_int,
    pub print_module_to_string: unsafe extern "C" fn(LLVMModuleRef) -> *mut c_char,
    pub dispose_module: unsafe extern "C" fn(LLVMModuleRef),
    pub dispose_message: unsafe extern "C" fn(*mut c_char),
    pub get_error_message: unsafe extern "C" fn(LLVMErrorRef) -> *mut c_char,
    pub dispose_error_message: unsafe extern "C" fn(*mut c_char),
    pub set_target: unsafe extern "C" fn(LLVMModuleRef, *const c_char),
    pub set_data_layout: unsafe extern "C" fn(LLVMModuleRef, *const c_char),

    // Targets & PassBuilder
    pub init_x86_target_info: unsafe extern "C" fn(),
    pub init_x86_target: unsafe extern "C" fn(),
    pub init_x86_target_mc: unsafe extern "C" fn(),
    pub init_x86_asm_printer: unsafe extern "C" fn(),
    pub init_x86_asm_parser: unsafe extern "C" fn(),
    pub get_default_target_triple: unsafe extern "C" fn() -> *mut c_char,
    pub get_host_cpu_name: unsafe extern "C" fn() -> *mut c_char,
    pub get_host_cpu_features: unsafe extern "C" fn() -> *mut c_char,
    pub get_target_from_triple:
        unsafe extern "C" fn(*const c_char, *mut LLVMTargetRef, *mut *mut c_char) -> c_int,
    pub create_target_machine: unsafe extern "C" fn(
        LLVMTargetRef,
        *const c_char,
        *const c_char,
        *const c_char,
        c_uint,
        c_uint,
        c_uint,
    ) -> LLVMTargetMachineRef,
    pub dispose_target_machine: unsafe extern "C" fn(LLVMTargetMachineRef),
    pub create_target_data_layout: unsafe extern "C" fn(LLVMTargetMachineRef) -> LLVMTargetDataRef,
    pub copy_string_rep_of_target_data: unsafe extern "C" fn(LLVMTargetDataRef) -> *mut c_char,
    pub dispose_target_data: unsafe extern "C" fn(LLVMTargetDataRef),
    pub create_pass_builder_options: unsafe extern "C" fn() -> LLVMPassBuilderOptionsRef,
    pub dispose_pass_builder_options: unsafe extern "C" fn(LLVMPassBuilderOptionsRef),
    pub run_passes: unsafe extern "C" fn(
        LLVMModuleRef,
        *const c_char,
        LLVMTargetMachineRef,
        LLVMPassBuilderOptionsRef,
    ) -> LLVMErrorRef,
    pub target_machine_emit_to_file: unsafe extern "C" fn(
        LLVMTargetMachineRef,
        LLVMModuleRef,
        *const c_char,
        c_uint,
        *mut *mut c_char,
    ) -> c_int,

    // ORC LLJIT
    pub orc_create_lljit_builder: unsafe extern "C" fn() -> LLVMOrcLLJITBuilderRef,
    pub orc_create_lljit:
        unsafe extern "C" fn(*mut LLVMOrcLLJITRef, LLVMOrcLLJITBuilderRef) -> LLVMErrorRef,
    pub orc_dispose_lljit: unsafe extern "C" fn(LLVMOrcLLJITRef) -> LLVMErrorRef,
    pub orc_lljit_get_main_jit_dylib: unsafe extern "C" fn(LLVMOrcLLJITRef) -> LLVMOrcJITDylibRef,
    pub orc_lljit_get_global_prefix: unsafe extern "C" fn(LLVMOrcLLJITRef) -> c_char,
    pub orc_lljit_get_triple_string: unsafe extern "C" fn(LLVMOrcLLJITRef) -> *const c_char,
    pub orc_lljit_get_data_layout_str: unsafe extern "C" fn(LLVMOrcLLJITRef) -> *const c_char,
    pub orc_lljit_mangle_and_intern:
        unsafe extern "C" fn(LLVMOrcLLJITRef, *const c_char) -> LLVMOrcSymbolStringPoolEntryRef,
    pub orc_absolute_symbols:
        unsafe extern "C" fn(*mut LLVMOrcCSymbolMapPair, usize) -> LLVMOrcMaterializationUnitRef,
    pub orc_dispose_materialization_unit: unsafe extern "C" fn(LLVMOrcMaterializationUnitRef),
    pub orc_jit_dylib_define:
        unsafe extern "C" fn(LLVMOrcJITDylibRef, LLVMOrcMaterializationUnitRef) -> LLVMErrorRef,
    pub orc_create_dynlib_search_gen_for_process: unsafe extern "C" fn(
        *mut LLVMOrcDefinitionGeneratorRef,
        c_char,
        *const c_void,
        *mut c_void,
    ) -> LLVMErrorRef,
    pub orc_jit_dylib_add_generator:
        unsafe extern "C" fn(LLVMOrcJITDylibRef, LLVMOrcDefinitionGeneratorRef),
    pub orc_create_new_thread_safe_context: unsafe extern "C" fn() -> LLVMOrcThreadSafeContextRef,
    pub orc_thread_safe_context_get_context:
        unsafe extern "C" fn(LLVMOrcThreadSafeContextRef) -> LLVMContextRef,
    pub orc_dispose_thread_safe_context: unsafe extern "C" fn(LLVMOrcThreadSafeContextRef),
    pub orc_create_new_thread_safe_module: unsafe extern "C" fn(
        LLVMModuleRef,
        LLVMOrcThreadSafeContextRef,
    ) -> LLVMOrcThreadSafeModuleRef,
    pub orc_dispose_thread_safe_module: unsafe extern "C" fn(LLVMOrcThreadSafeModuleRef),
    pub orc_lljit_add_llvm_ir_module: unsafe extern "C" fn(
        LLVMOrcLLJITRef,
        LLVMOrcJITDylibRef,
        LLVMOrcThreadSafeModuleRef,
    ) -> LLVMErrorRef,
    pub orc_lljit_lookup:
        unsafe extern "C" fn(LLVMOrcLLJITRef, *mut u64, *const c_char) -> LLVMErrorRef,
}

// SAFETY: Loaded `libLLVM` function pointers are immutable process-global symbols.
unsafe impl Sync for LlvmApi {}
unsafe impl Send for LlvmApi {}

static LLVM_API: OnceLock<Result<LlvmApi, String>> = OnceLock::new();

unsafe fn load_sym<T>(handle: *mut c_void, name: &str) -> Result<T, String> {
    let cname = CString::new(name).map_err(|_| format!("invalid symbol name {name}"))?;
    let ptr = unsafe { dlsym(handle, cname.as_ptr()) };
    if ptr.is_null() {
        return Err(format!("missing LLVM 18 symbol {name}"));
    }
    Ok(unsafe { std::mem::transmute_copy(&ptr) })
}

fn load_llvm_api_inner() -> Result<LlvmApi, String> {
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(env_path) = std::env::var("DEC_LLVM_LIBRARY") {
        if !env_path.is_empty() {
            candidates.push(env_path);
        }
    }
    candidates.extend([
        "/usr/lib/llvm-18/lib/libLLVM.so.18.1".to_string(),
        "/usr/lib/x86_64-linux-gnu/libLLVM-18.so.1".to_string(),
        "libLLVM.so.18.1".to_string(),
        "libLLVM-18.so.1".to_string(),
        "libLLVM-18.so".to_string(),
    ]);

    let mut handle = std::ptr::null_mut();
    let mut last_err = String::new();
    for cand in &candidates {
        let Ok(cstr) = CString::new(cand.as_str()) else {
            continue;
        };
        handle = unsafe { dlopen(cstr.as_ptr(), RTLD_NOW | RTLD_GLOBAL) };
        if !handle.is_null() {
            break;
        }
        let err_ptr = unsafe { dlerror() };
        if !err_ptr.is_null() {
            last_err = unsafe { CStr::from_ptr(err_ptr) }
                .to_string_lossy()
                .into_owned();
        }
    }
    if handle.is_null() {
        return Err(format!(
            "failed to dlopen libLLVM 18 (tried {:?}): {last_err}",
            candidates
        ));
    }

    let api = unsafe {
        LlvmApi {
            context_create: load_sym(handle, "LLVMContextCreate")?,
            context_dispose: load_sym(handle, "LLVMContextDispose")?,
            create_mem_buf_copy: load_sym(handle, "LLVMCreateMemoryBufferWithMemoryRangeCopy")?,
            parse_ir_in_context: load_sym(handle, "LLVMParseIRInContext")?,
            verify_module: load_sym(handle, "LLVMVerifyModule")?,
            print_module_to_string: load_sym(handle, "LLVMPrintModuleToString")?,
            dispose_module: load_sym(handle, "LLVMDisposeModule")?,
            dispose_message: load_sym(handle, "LLVMDisposeMessage")?,
            get_error_message: load_sym(handle, "LLVMGetErrorMessage")?,
            dispose_error_message: load_sym(handle, "LLVMDisposeErrorMessage")?,
            set_target: load_sym(handle, "LLVMSetTarget")?,
            set_data_layout: load_sym(handle, "LLVMSetDataLayout")?,

            init_x86_target_info: load_sym(handle, "LLVMInitializeX86TargetInfo")?,
            init_x86_target: load_sym(handle, "LLVMInitializeX86Target")?,
            init_x86_target_mc: load_sym(handle, "LLVMInitializeX86TargetMC")?,
            init_x86_asm_printer: load_sym(handle, "LLVMInitializeX86AsmPrinter")?,
            init_x86_asm_parser: load_sym(handle, "LLVMInitializeX86AsmParser")?,
            get_default_target_triple: load_sym(handle, "LLVMGetDefaultTargetTriple")?,
            get_host_cpu_name: load_sym(handle, "LLVMGetHostCPUName")?,
            get_host_cpu_features: load_sym(handle, "LLVMGetHostCPUFeatures")?,
            get_target_from_triple: load_sym(handle, "LLVMGetTargetFromTriple")?,
            create_target_machine: load_sym(handle, "LLVMCreateTargetMachine")?,
            dispose_target_machine: load_sym(handle, "LLVMDisposeTargetMachine")?,
            create_target_data_layout: load_sym(handle, "LLVMCreateTargetDataLayout")?,
            copy_string_rep_of_target_data: load_sym(handle, "LLVMCopyStringRepOfTargetData")?,
            dispose_target_data: load_sym(handle, "LLVMDisposeTargetData")?,
            create_pass_builder_options: load_sym(handle, "LLVMCreatePassBuilderOptions")?,
            dispose_pass_builder_options: load_sym(handle, "LLVMDisposePassBuilderOptions")?,
            run_passes: load_sym(handle, "LLVMRunPasses")?,
            target_machine_emit_to_file: load_sym(handle, "LLVMTargetMachineEmitToFile")?,

            orc_create_lljit_builder: load_sym(handle, "LLVMOrcCreateLLJITBuilder")?,
            orc_create_lljit: load_sym(handle, "LLVMOrcCreateLLJIT")?,
            orc_dispose_lljit: load_sym(handle, "LLVMOrcDisposeLLJIT")?,
            orc_lljit_get_main_jit_dylib: load_sym(handle, "LLVMOrcLLJITGetMainJITDylib")?,
            orc_lljit_get_global_prefix: load_sym(handle, "LLVMOrcLLJITGetGlobalPrefix")?,
            orc_lljit_get_triple_string: load_sym(handle, "LLVMOrcLLJITGetTripleString")?,
            orc_lljit_get_data_layout_str: load_sym(handle, "LLVMOrcLLJITGetDataLayoutStr")?,
            orc_lljit_mangle_and_intern: load_sym(handle, "LLVMOrcLLJITMangleAndIntern")?,
            orc_absolute_symbols: load_sym(handle, "LLVMOrcAbsoluteSymbols")?,
            orc_dispose_materialization_unit: load_sym(
                handle,
                "LLVMOrcDisposeMaterializationUnit",
            )?,
            orc_jit_dylib_define: load_sym(handle, "LLVMOrcJITDylibDefine")?,
            orc_create_dynlib_search_gen_for_process: load_sym(
                handle,
                "LLVMOrcCreateDynamicLibrarySearchGeneratorForProcess",
            )?,
            orc_jit_dylib_add_generator: load_sym(handle, "LLVMOrcJITDylibAddGenerator")?,
            orc_create_new_thread_safe_context: load_sym(
                handle,
                "LLVMOrcCreateNewThreadSafeContext",
            )?,
            orc_thread_safe_context_get_context: load_sym(
                handle,
                "LLVMOrcThreadSafeContextGetContext",
            )?,
            orc_dispose_thread_safe_context: load_sym(handle, "LLVMOrcDisposeThreadSafeContext")?,
            orc_create_new_thread_safe_module: load_sym(
                handle,
                "LLVMOrcCreateNewThreadSafeModule",
            )?,
            orc_dispose_thread_safe_module: load_sym(handle, "LLVMOrcDisposeThreadSafeModule")?,
            orc_lljit_add_llvm_ir_module: load_sym(handle, "LLVMOrcLLJITAddLLVMIRModule")?,
            orc_lljit_lookup: load_sym(handle, "LLVMOrcLLJITLookup")?,
        }
    };

    // Initialize X86 native target once
    unsafe {
        (api.init_x86_target_info)();
        (api.init_x86_target)();
        (api.init_x86_target_mc)();
        (api.init_x86_asm_printer)();
        (api.init_x86_asm_parser)();
    }

    Ok(api)
}

pub fn llvm_api() -> LlvmResult<&'static LlvmApi> {
    match LLVM_API.get_or_init(load_llvm_api_inner) {
        Ok(api) => Ok(api),
        Err(msg) => Err(LlvmError::new(msg.clone())),
    }
}

/// Convert an owned LLVM C `char*` message (freed via `LLVMDisposeMessage`) into a Rust `String`.
pub unsafe fn take_llvm_message(api: &LlvmApi, ptr: *mut c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let s = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    unsafe {
        (api.dispose_message)(ptr);
    }
    s
}

/// Consume an `LLVMErrorRef` via `LLVMGetErrorMessage` and free the message via
/// `LLVMDisposeErrorMessage` (`P0-1`: NEVER also call `LLVMConsumeError`!).
pub unsafe fn take_llvm_error(api: &LlvmApi, err: LLVMErrorRef) -> String {
    if err.is_null() {
        return String::new();
    }
    let msg_ptr = unsafe { (api.get_error_message)(err) };
    if msg_ptr.is_null() {
        return "unknown LLVM error".to_string();
    }
    let s = unsafe { CStr::from_ptr(msg_ptr) }
        .to_string_lossy()
        .into_owned();
    unsafe {
        (api.dispose_error_message)(msg_ptr);
    }
    s
}

/// Parse textual LLVM IR inside `ctx` and verify it (`LLVMReturnStatusAction = 2`).
/// `LLVMParseIRInContext` ALWAYS takes ownership of and frees `MemBuf` (`P0-1`).
pub unsafe fn parse_and_verify_module(
    api: &LlvmApi,
    ctx: LLVMContextRef,
    ir_text: &str,
) -> LlvmResult<LLVMModuleRef> {
    let buf_name = CString::new("aod_module.ll").unwrap();
    let mem_buf = unsafe {
        (api.create_mem_buf_copy)(
            ir_text.as_ptr() as *const c_char,
            ir_text.len(),
            buf_name.as_ptr(),
        )
    };
    if mem_buf.is_null() {
        return Err(LlvmError::new("LLVMCreateMemoryBufferWithMemoryRangeCopy failed"));
    }

    let mut module: LLVMModuleRef = std::ptr::null_mut();
    let mut err_msg: *mut c_char = std::ptr::null_mut();
    // SAFETY: `LLVMParseIRInContext` always owns and frees `mem_buf` on both Ok and Err (`P0-1`).
    let parse_rc = unsafe { (api.parse_ir_in_context)(ctx, mem_buf, &mut module, &mut err_msg) };
    if parse_rc != 0 || module.is_null() {
        let msg = unsafe { take_llvm_message(api, err_msg) };
        if !module.is_null() {
            unsafe { (api.dispose_module)(module) };
        }
        return Err(LlvmError::new(format!("LLVM IR parse failed: {msg}")));
    }
    if !err_msg.is_null() {
        let _ = unsafe { take_llvm_message(api, err_msg) };
    }

    let mut verify_msg: *mut c_char = std::ptr::null_mut();
    let verify_rc =
        unsafe { (api.verify_module)(module, LLVM_RETURN_STATUS_ACTION, &mut verify_msg) };
    let vmsg = unsafe { take_llvm_message(api, verify_msg) };
    if verify_rc != 0 {
        unsafe { (api.dispose_module)(module) };
        return Err(LlvmError::new(format!("LLVM IR verification failed: {vmsg}")));
    }

    Ok(module)
}

/// Create a host `LLVMTargetMachineRef` (PIC relocation) and configure the module's triple & layout.
pub unsafe fn create_host_target_machine(
    api: &LlvmApi,
    opt_level: u32,
) -> LlvmResult<(LLVMTargetMachineRef, CString, CString)> {
    let triple_ptr = unsafe { (api.get_default_target_triple)() };
    let triple_str = unsafe { take_llvm_message(api, triple_ptr) };
    let triple_c = CString::new(triple_str.as_str())
        .map_err(|_| LlvmError::new("invalid host target triple"))?;

    let mut target: LLVMTargetRef = std::ptr::null_mut();
    let mut err_msg: *mut c_char = std::ptr::null_mut();
    let rc = unsafe { (api.get_target_from_triple)(triple_c.as_ptr(), &mut target, &mut err_msg) };
    if rc != 0 || target.is_null() {
        let msg = unsafe { take_llvm_message(api, err_msg) };
        return Err(LlvmError::new(format!(
            "LLVMGetTargetFromTriple({triple_str}) failed: {msg}"
        )));
    }
    if !err_msg.is_null() {
        let _ = unsafe { take_llvm_message(api, err_msg) };
    }

    let cpu_ptr = unsafe { (api.get_host_cpu_name)() };
    let cpu_str = unsafe { take_llvm_message(api, cpu_ptr) };
    let cpu_c = CString::new(cpu_str).unwrap_or_else(|_| CString::new("generic").unwrap());

    let feat_ptr = unsafe { (api.get_host_cpu_features)() };
    let feat_str = unsafe { take_llvm_message(api, feat_ptr) };
    let feat_c = CString::new(feat_str).unwrap_or_else(|_| CString::new("").unwrap());

    let tm = unsafe {
        (api.create_target_machine)(
            target,
            triple_c.as_ptr(),
            cpu_c.as_ptr(),
            feat_c.as_ptr(),
            opt_level,
            LLVM_RELOC_PIC,
            LLVM_CODE_MODEL_DEFAULT,
        )
    };
    if tm.is_null() {
        return Err(LlvmError::new("LLVMCreateTargetMachine returned null"));
    }

    let td = unsafe { (api.create_target_data_layout)(tm) };
    let dl_ptr = unsafe { (api.copy_string_rep_of_target_data)(td) };
    let dl_str = unsafe { take_llvm_message(api, dl_ptr) };
    unsafe { (api.dispose_target_data)(td) };
    let dl_c = CString::new(dl_str).unwrap_or_else(|_| CString::new("").unwrap());

    Ok((tm, triple_c, dl_c))
}

pub unsafe fn run_module_passes(
    api: &LlvmApi,
    module: LLVMModuleRef,
    tm: LLVMTargetMachineRef,
    pipeline: &str,
) -> LlvmResult<()> {
    let pipe_c = CString::new(pipeline).map_err(|_| LlvmError::new("invalid pass pipeline"))?;
    let opts = unsafe { (api.create_pass_builder_options)() };
    let err = unsafe { (api.run_passes)(module, pipe_c.as_ptr(), tm, opts) };
    unsafe { (api.dispose_pass_builder_options)(opts) };
    if !err.is_null() {
        let msg = unsafe { take_llvm_error(api, err) };
        return Err(LlvmError::new(format!("LLVMRunPasses({pipeline}) failed: {msg}")));
    }

    // Post-pass verification (`P2`): verify optimized IR cleanly before AOT emission or ORC JIT.
    let mut verify_msg: *mut c_char = std::ptr::null_mut();
    let verify_rc =
        unsafe { (api.verify_module)(module, LLVM_RETURN_STATUS_ACTION, &mut verify_msg) };
    let vmsg = unsafe { take_llvm_message(api, verify_msg) };
    if verify_rc != 0 {
        return Err(LlvmError::new(format!(
            "post-pass LLVM IR verification failed ({pipeline}): {vmsg}"
        )));
    }
    Ok(())
}
