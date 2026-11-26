use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use aodc::diagnostics::DiagnosticCode;
use aodc::eval::{Evaluator, DEFAULT_FUEL, DEFAULT_MAX_DEPTH};
use aodc::ir::{dumps_ir, loads_ir};
use aodc::{
    check_file, check_library_file, check_library_source, emit_llvm_file, jit_cache_stats,
    jit_source_captured, DynEffectCaps, DynamicJitHost, OptLevel, RtNamespace,
};

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn fresh_temp_dir(tag: &str) -> PathBuf {
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "aodc_p9_dyn_{tag}_{}_{}",
        std::process::id(),
        seq
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, content).expect("write test file");
}

#[test]
fn inferno_mount_dyn_swaps_mock_and_prod_per_task_with_zero_recompilation() {
    let dir = fresh_temp_dir("swap_per_task");

    // Default capability interface mounted via `mount dyn`
    write_file(
        &dir.join("calc_iface/mod.dec"),
        "\
dec score(x: int, y: int) -> int:
    x + y
",
    );

    // Mock implementation: `x * 10 + y`
    let mock_path = dir.join("mock_calc.dec");
    write_file(
        &mock_path,
        "\
dec score(x: int, y: int) -> int:
    x * 10 + y
",
    );

    // Production implementation with module-level `const` bias: `(x * y) + BIAS`
    let prod_path = dir.join("prod_calc.dec");
    write_file(
        &prod_path,
        "\
const BIAS: int = 500

dec score(x: int, y: int) -> int:
    x * y + BIAS
",
    );

    let main_path = dir.join("main.dec");
    write_file(
        &main_path,
        "\
namespace app:
    clear
    mount dyn b\"./calc_iface\" -> /calc

use /calc:
    score

dec main() -> ():
    emit score(3, 4)
",
    );

    let checked_main = check_file(&main_path).expect("check main with mount dyn");
    assert_eq!(checked_main.dynamic_slots.len(), 1);
    assert_eq!(checked_main.dynamic_slots[0].mount_path, "/calc");
    assert_eq!(checked_main.dynamic_slots[0].fn_name, "score");
    assert_eq!(checked_main.dynamic_slots[0].allowed_caps, DynEffectCaps::PURE);

    // Verify LLVM IR emits indirect capability lookup via `@aod_rt_resolve_dyn_slot`
    let llvm_ir = emit_llvm_file(&main_path, OptLevel::O0).expect("emit O0 LLVM IR");
    assert!(
        llvm_ir.contains("@aod_rt_resolve_dyn_slot"),
        "expected indirect vtable lookup in emitted LLVM IR"
    );

    // Verify DecIR v4 round-trips `dynamic_slots` hermetically without disk files!
    let ir_v4 = dumps_ir(&checked_main);
    assert!(ir_v4.contains("\"dynamic_slots\""));
    let mock_checked = Arc::new(check_library_file(&mock_path).expect("check mock provider"));
    let prod_checked = Arc::new(check_library_file(&prod_path).expect("check prod provider"));

    fs::remove_dir_all(dir.join("calc_iface")).expect("delete calc_iface dir");
    let reloaded_main = loads_ir(&ir_v4).expect("hermetic DecIR v4 reload of mount dyn unit");
    assert_eq!(reloaded_main.dynamic_slots, checked_main.dynamic_slots);

    // Compile `main` ONCE in both Evaluator and DynamicJitHost (Linux Plan 9 container)!
    let mut vm_host =
        Evaluator::new(&reloaded_main, DEFAULT_FUEL, DEFAULT_MAX_DEPTH).expect("init VM host");
    let mut jit_host =
        DynamicJitHost::compile(reloaded_main.clone(), OptLevel::O2).expect("compile JIT host");
    assert_eq!(jit_host.main_compile_count(), 1);

    // Task 0: Unbound -> runs default interface implementation (`3 + 4 = 7`)
    let (vm_t0, vm_r0) = vm_host
        .run_task_capturing(DEFAULT_FUEL, DEFAULT_MAX_DEPTH, RtNamespace::empty(), Vec::new())
        .expect("vm task 0");
    vm_r0.expect("vm task 0 ok");
    assert_eq!(vm_t0, vec!["7"]);

    let (jit_t0, jit_r0) = jit_host
        .run_task_captured(DEFAULT_FUEL, DEFAULT_MAX_DEPTH, RtNamespace::empty(), Vec::new())
        .expect("jit task 0");
    jit_r0.expect("jit task 0 ok");
    assert_eq!(jit_t0, "7\n");

    // Task 1: Bind `/mock_calc` -> `3 * 10 + 4 = 34` (ZERO recompilation of main!)
    vm_host
        .bind_dynamic_provider("/calc", Arc::clone(&mock_checked))
        .expect("vm bind mock");
    jit_host
        .bind_dynamic_provider("/calc", &mock_checked)
        .expect("jit bind mock");
    assert_eq!(jit_host.main_compile_count(), 1);

    let (vm_t1, vm_r1) = vm_host
        .run_task_capturing(DEFAULT_FUEL, DEFAULT_MAX_DEPTH, RtNamespace::empty(), Vec::new())
        .expect("vm task 1");
    vm_r1.expect("vm task 1 ok");
    assert_eq!(vm_t1, vec!["34"]);

    let (jit_t1, jit_r1) = jit_host
        .run_task_captured(DEFAULT_FUEL, DEFAULT_MAX_DEPTH, RtNamespace::empty(), Vec::new())
        .expect("jit task 1");
    jit_r1.expect("jit task 1 ok");
    assert_eq!(jit_t1, "34\n");

    // Task 2: Swap to `/prod_calc` -> `3 * 4 + 500 = 512` (ZERO recompilation of main!)
    vm_host
        .bind_dynamic_provider("/calc", Arc::clone(&prod_checked))
        .expect("vm bind prod");
    jit_host
        .bind_dynamic_provider("/calc", &prod_checked)
        .expect("jit bind prod");
    assert_eq!(jit_host.main_compile_count(), 1);

    let (vm_t2, vm_r2) = vm_host
        .run_task_capturing(DEFAULT_FUEL, DEFAULT_MAX_DEPTH, RtNamespace::empty(), Vec::new())
        .expect("vm task 2");
    vm_r2.expect("vm task 2 ok");
    assert_eq!(vm_t2, vec!["512"]);

    let (jit_t2, jit_r2) = jit_host
        .run_task_captured(DEFAULT_FUEL, DEFAULT_MAX_DEPTH, RtNamespace::empty(), Vec::new())
        .expect("jit task 2");
    jit_r2.expect("jit task 2 ok");
    assert_eq!(jit_t2, "512\n");

    // Task 3: Unbind `/calc` -> reverts to default (`7`), still `main_compile_count == 1`!
    vm_host.unbind_dynamic_mount("/calc").expect("vm unbind");
    jit_host.unbind_dynamic_mount("/calc").expect("jit unbind");
    assert_eq!(jit_host.main_compile_count(), 1);

    let (jit_t3, jit_r3) = jit_host
        .run_task_captured(DEFAULT_FUEL, DEFAULT_MAX_DEPTH, RtNamespace::empty(), Vec::new())
        .expect("jit task 3");
    jit_r3.expect("jit task 3 ok");
    assert_eq!(jit_t3, "7\n");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn inferno_sign_rejects_type_mismatch_and_nominal_struct_impostor() {
    let dir = fresh_temp_dir("sign_nominal");

    // Shared domain package defining `Order` (`Provider.id` = Merkle hash of `domain/mod.dec`)
    write_file(
        &dir.join("domain/mod.dec"),
        "\
struct Order:
    qty: int
    unit_price: int
",
    );

    // Dynamic pricing capability interface importing `Order` from `/domain`
    write_file(
        &dir.join("pricer_iface/ns.dec"),
        "\
namespace pricer_ns:
    clear
    mount ro b\"../domain\" -> /domain
",
    );
    write_file(
        &dir.join("pricer_iface/mod.dec"),
        "\
use /domain:
    Order

dec total(ord: Order) -> int:
    ord.qty * ord.unit_price
",
    );

    // Legitimate plugin importing `Order` from the SAME `/domain` package (`Provider.id` matches!)
    let legit_plugin_path = dir.join("legit_plugin.dec");
    write_file(
        &legit_plugin_path,
        "\
namespace legit_ns:
    clear
    mount ro b\"./domain\" -> /domain

use /domain:
    Order

dec total(ord: Order) -> int:
    ord.qty * ord.unit_price + 99
",
    );

    // Hostile impostor plugin defining its OWN local `struct Order` with identical field names!
    let impostor_plugin_path = dir.join("impostor_plugin.dec");
    write_file(
        &impostor_plugin_path,
        "\
struct Order:
    qty: int
    unit_price: int

dec total(ord: Order) -> int:
    0
",
    );

    // Wrong primitive return type plugin (`ratio` instead of `int`)
    let wrong_ret_plugin = check_library_source(
        "\
dec total(x: int) -> ratio:
    1/2r
",
    )
    .expect("check wrong_ret_plugin");

    let main_path = dir.join("main.dec");
    write_file(
        &main_path,
        "\
namespace app:
    clear
    mount ro b\"./domain\" -> /domain
    mount dyn b\"./pricer_iface\" -> /pricer

use /domain:
    Order
use /pricer:
    total

dec main() -> ():
    let o = Order(3, 20)
    emit total(o)
",
    );

    let checked_main = check_file(&main_path).expect("check nominal main");
    let legit_checked = check_library_file(&legit_plugin_path).expect("check legit plugin");
    let impostor_checked =
        check_library_file(&impostor_plugin_path).expect("check impostor plugin");

    let mut jit_host =
        DynamicJitHost::compile(checked_main, OptLevel::O2).expect("compile nominal JIT host");

    // 1. Wrong primitive signature -> rejected with TypeError (`Inferno sign mismatch`)
    let err_prim = jit_host
        .bind_dynamic_provider("/pricer", &wrong_ret_plugin)
        .expect_err("wrong primitive signature must fail");
    assert_eq!(err_prim.code, DiagnosticCode::TypeError);
    assert!(err_prim.message.contains("Inferno sign mismatch"));

    // 2. Structural impostor (`<entry>:Order` vs `u_<domain_id>:Order`) -> rejected with TypeError!
    let err_imp = jit_host
        .bind_dynamic_provider("/pricer", &impostor_checked)
        .expect_err("nominal Struct impostor must fail Inferno sign check");
    assert_eq!(err_imp.code, DiagnosticCode::TypeError);
    assert!(err_imp.message.contains("Inferno sign mismatch"));

    // 3. Legitimate plugin sharing `/domain`'s `Provider.id` -> binds & runs cleanly inside container!
    jit_host
        .bind_dynamic_provider("/pricer", &legit_checked)
        .expect("legit plugin with matching UnitId must bind");
    let (stdout, res) = jit_host
        .run_task_captured(DEFAULT_FUEL, DEFAULT_MAX_DEPTH, RtNamespace::empty(), Vec::new())
        .expect("run legit plugin");
    res.expect("legit run ok");
    assert_eq!(stdout, "159\n");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn dynamic_capability_effect_firewall_blocks_compile_and_runtime_escalation() {
    let dir = fresh_temp_dir("effect_firewall");

    // 1. Compile-time firewall: an `observe` probe CANNOT call a `mount dyn` function whose
    //    interface declares `SysIo` (`read_file_bytes`)!
    write_file(
        &dir.join("io_iface/ns.dec"),
        "\
namespace io_ns:
    clear
    mount ro b\"builtin:aod/sys\" -> /sys
",
    );
    write_file(
        &dir.join("io_iface/mod.dec"),
        "\
use /sys:
    read_file_bytes

dec fetch_len() -> int:
    len(read_file_bytes(b\"/cfg/a.txt\"))
",
    );

    let bad_obs_main = dir.join("bad_obs_main.dec");
    write_file(
        &bad_obs_main,
        "\
namespace app:
    clear
    mount dyn b\"./io_iface\" -> /io_cap

use /io_cap:
    fetch_len

chamber BoxC:
    space = box([0..4, 0..4, 0..4])
    walls = open

observe BadObs:
    tracer leak(view):
        let _x = fetch_len()
        assert view.tick >= 0

dec main() -> ():
    emit 0
",
    );
    let obs_err = check_file(&bad_obs_main)
        .expect_err("observe probe calling SysIo mount dyn capability must fail at compile time");
    assert_eq!(obs_err.code, DiagnosticCode::ObserverMutationForbidden);

    // 2. Runtime `load` firewall: when `main` mounts a PURE dynamic interface, binding a provider
    //    that uses `/sys` (`SYS_IO`) or `simulate` (`CHAMBER_MUT`) is REJECTED at bind time!
    write_file(
        &dir.join("pure_iface/mod.dec"),
        "\
dec eval_metric() -> int:
    10
",
    );
    let pure_main_path = dir.join("pure_main.dec");
    write_file(
        &pure_main_path,
        "\
namespace app:
    clear
    mount dyn b\"./pure_iface\" -> /metric

use /metric:
    eval_metric

dec main() -> ():
    emit eval_metric()
",
    );
    let checked_pure_main = check_file(&pure_main_path).expect("check pure_main");
    let mut jit_host =
        DynamicJitHost::compile(checked_pure_main, OptLevel::O2).expect("compile pure_main");

    let evil_sys_provider = check_library_source(
        "\
use aod::sys::read_file_bytes

dec eval_metric() -> int:
    len(read_file_bytes(b\"/etc/passwd\"))
",
    )
    .expect("check evil_sys_provider");
    let sys_err = jit_host
        .bind_dynamic_provider("/metric", &evil_sys_provider)
        .expect_err("SysIo escalation on pure slot must be rejected");
    assert_eq!(sys_err.code, DiagnosticCode::SysIoForbidden);

    let evil_sim_provider = check_library_source(
        "\
chamber Hidden:
    space = box([0..4, 0..4, 0..4])
    walls = open

prepare InitH in Hidden:
    clock ref_clk = 2

dec eval_metric() -> int:
    let r = simulate InitH over 2 ticks
    r.boundary_crossings
",
    )
    .expect("check evil_sim_provider");
    let sim_err = jit_host
        .bind_dynamic_provider("/metric", &evil_sim_provider)
        .expect_err("ChamberMut escalation on pure slot must be rejected");
    assert_eq!(sim_err.code, DiagnosticCode::ObserverMutationForbidden);

    // 3. Capability subtyping (`PURE <= SYS_IO`): a pure in-memory mock CAN legally satisfy a `SYS_IO` slot!
    let io_main_path = dir.join("io_main.dec");
    write_file(
        &io_main_path,
        "\
namespace app_io:
    clear
    mount dyn b\"./io_iface\" -> /io_cap

use /io_cap:
    fetch_len

dec main() -> ():
    emit fetch_len()
",
    );
    let checked_io_main = check_file(&io_main_path).expect("check io_main");
    assert_eq!(checked_io_main.dynamic_slots[0].allowed_caps, DynEffectCaps::SYS_IO);
    let mut io_jit_host =
        DynamicJitHost::compile(checked_io_main, OptLevel::O2).expect("compile io_main");

    let pure_mock_for_io = check_library_source(
        "\
dec fetch_len() -> int:
    42
",
    )
    .expect("check pure_mock_for_io");
    io_jit_host
        .bind_dynamic_provider("/io_cap", &pure_mock_for_io)
        .expect("PURE mock must satisfy SYS_IO capability slot");
    let (out, res) = io_jit_host
        .run_task_captured(DEFAULT_FUEL, DEFAULT_MAX_DEPTH, RtNamespace::empty(), Vec::new())
        .expect("run pure mock in SYS_IO slot");
    res.expect("run ok");
    assert_eq!(out, "42\n");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn process_wide_jit_code_cache_deduplicates_identical_provider_and_plan_artifacts() {
    let prog = "\
dec fib_unique_cache_probe(a: int, b: int) -> int:
    a + b + 987654

dec main() -> ():
    emit fib_unique_cache_probe(21, 34)
";

    let s0 = jit_cache_stats();
    let (out1, res1) = jit_source_captured(prog, OptLevel::O2, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
        .expect("cold compile");
    res1.expect("run 1");
    assert_eq!(out1, "987709\n");
    let s1 = jit_cache_stats();
    assert!(s1.misses >= s0.misses + 1);
    assert!(s1.entries >= 1);

    // Second & third JIT sessions with identical `(IR, CodegenPlan::O2)` hit the object cache!
    let hits_before = jit_cache_stats().hits;
    for _ in 0..2 {
        let (out_hit, res_hit) =
            jit_source_captured(prog, OptLevel::O2, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
                .expect("warm cache compile");
        res_hit.expect("warm run");
        assert_eq!(out_hit, "987709\n");
    }
    let s2 = jit_cache_stats();
    assert!(
        s2.hits >= hits_before + 2,
        "expected >= 2 content-addressed object cache hits, got {} -> {}",
        hits_before,
        s2.hits
    );

    // Compiling with `OptLevel::O0` keys under a distinct `CodegenPlan` -> separate cache miss!
    let misses_before_o0 = jit_cache_stats().misses;
    let (out_o0, res_o0) = jit_source_captured(prog, OptLevel::O0, DEFAULT_FUEL, DEFAULT_MAX_DEPTH)
        .expect("O0 compile");
    res_o0.expect("O0 run");
    assert_eq!(out_o0, "987709\n");
    let s3 = jit_cache_stats();
    assert!(s3.misses >= misses_before_o0 + 1);
}
