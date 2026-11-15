"""Independent exact expectations and generated-code/GPU conformance."""
import ctypes as C
from concurrent.futures import ThreadPoolExecutor
import hashlib
import itertools
from pathlib import Path
import random
import re
import shutil
import subprocess
import threading
import time
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from native import gpu_compiler as gpu
from native.batch import BatchError, select_entry, validate_inputs
from runtime.dec_core import render
from runtime.dec_language import DecError, Interpreter, compile_source


MAIN = "\nDec main() -> result:\n    () -> int result:\n        result = 0\n"


def program(body, parameters=(("int", "x"),), result="int", name="chosen"):
    names = ", ".join(value for _, value in parameters)
    signature = ", ".join(kind+" "+value for kind, value in parameters)
    return ("Dec "+name+"("+names+") -> result:\n    ("+signature+") -> "+result+" result:\n"+
            "\n".join("        "+line for line in body.splitlines())+"\n"+MAIN)


INCIDENCE = program("assert source == 0 or source == 1\n"
                    "assert target == 0 or target == 1\n"
                    "assert available == 0 or available == 1\n"
                    "result = source - target if available == 1 else 0",
                    (("int", "source"), ("int", "target"), ("int", "available")))


def reference(source, rows, *, fuel=100000, max_depth=128):
    ir, results = compile_source(source), []
    for row in rows:
        emitted = []
        vm = Interpreter(ir, emit=emitted.append, max_steps=fuel, max_depth=max_depth)
        try:
            value = vm.call("chosen", row)
            record = {"result": render(value), "stdout": "", "error": None, "error_line": 0, "exit_code": 0}
        except DecError as exc:
            message = str(exc)
            line = re.match(r"line ([0-9]+):", message)
            record = {"result": None, "stdout": "", "error": message,
                      "error_line": int(line[1]) if line else 0, "exit_code": 1}
        results.append(record)
    return results


def core_rows(rows):
    return [{key: row[key] for key in ("result", "stdout", "error", "error_line", "exit_code")} for row in rows]


class GeneratedCodeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.compiler = shutil.which("g++")
        if cls.compiler is None:
            raise unittest.SkipTest("g++ unavailable for generated-body conformance")
        cls.directory = TemporaryDirectory()
        cls.libraries = {}

    @classmethod
    def tearDownClass(cls):
        cls.libraries.clear()
        cls.directory.cleanup()

    def host(self, source, rows, *, fuel=100000, max_depth=128):
        """Compile the generated body as C++; this is not a GPU claim."""
        entry = select_entry(source, "chosen")
        rows = validate_inputs(entry, rows, fuel=fuel, max_depth=max_depth)
        generated = gpu.lower_cuda(source, "chosen")["source"]
        digest = hashlib.sha256(generated.encode()).hexdigest()
        if digest not in self.libraries:
            base = Path(self.directory.name)/digest
            source_path = base.with_suffix(".cpp")
            source_path.write_text(
                "#define __device__\n#define __global__\n"
                "struct Dim { unsigned int x; }; Dim blockIdx, blockDim, threadIdx;\n"+generated+
                'extern "C" void host_launch(const DecInt* a, DecInt* b, int* c, int* d, DecInt* e, '
                'unsigned int n, DecInt fuel, int depth) { blockDim.x=128;'
                'for(unsigned int row=0;row<n;++row){blockIdx.x=row/128;threadIdx.x=row%128;'
                'dec_gpu_rows(a,b,c,d,e,n,fuel,depth);}}')
            library_path = base.with_suffix(".so")
            subprocess.run([self.compiler, "-std=c++11", "-O2", "-ftrapv", "-shared", "-fPIC",
                            str(source_path), "-o", str(library_path)], check=True, capture_output=True, timeout=30)
            lib = C.CDLL(str(library_path))
            lib.host_launch.argtypes = [C.POINTER(C.c_int64), C.POINTER(C.c_int64), C.POINTER(C.c_int32),
                C.POINTER(C.c_int32), C.POINTER(C.c_int64), C.c_uint, C.c_int64, C.c_int32]
            lib.host_launch.restype = None
            self.libraries[digest] = lib
        values = tuple(int(value) for row in rows for value in row)
        inputs = (C.c_int64*max(1, len(values)))(*(values or (0,)))
        outputs = ((C.c_int64*len(rows))(), (C.c_int32*len(rows))(), (C.c_int32*len(rows))(), (C.c_int64*len(rows))())
        self.libraries[digest].host_launch(inputs, *outputs, len(rows), fuel, max_depth)
        raw = tuple(tuple(value[row] for value in outputs) for row in range(len(rows)))
        return gpu._materialize(entry, raw, fuel=fuel)

    def test_independent_incidence_truth_table_and_source_mutation(self):
        rows = [(1, 0, 1), (0, 1, 1), (1, 1, 1), (0, 0, 1), (1, 0, 0), (2, 0, 1)]
        actual = self.host(INCIDENCE, rows)
        self.assertEqual([row["result"] for row in actual], ["1", "-1", "0", "0", "0", None])
        self.assertEqual(actual[-1]["error"], "line 3: assertion failed")
        self.assertEqual(core_rows(actual), reference(INCIDENCE, rows))
        changed = INCIDENCE.replace("source - target", "target - source")
        self.assertEqual([row["result"] for row in self.host(changed, rows[:-1])], ["-1", "1", "0", "0", "0"])
        self.assertNotEqual(gpu.lower_cuda(INCIDENCE, "chosen")["source"], gpu.lower_cuda(changed, "chosen")["source"])

    def test_all_executed_path_fuel_thresholds_and_entry_depth_order(self):
        source = program("assert x != 0\nint y = x + 3\nresult = y * x if x > 0 else -y")
        rows = [(-5,), (0,), (7,)]
        for fuel in range(40):
            with self.subTest(fuel=fuel):
                self.assertEqual(core_rows(self.host(source, rows, fuel=fuel)), reference(source, rows, fuel=fuel))
        for fuel, depth in ((0, 0), (1, 0), (50, 0), (50, 1)):
            with self.subTest(fuel=fuel, depth=depth):
                self.assertEqual(core_rows(self.host(source, rows, fuel=fuel, max_depth=depth)),
                                 reference(source, rows, fuel=fuel, max_depth=depth))

    def test_short_circuit_boolean_conditional_and_remaining_fuel(self):
        source = program("bool gated = enabled and (x + 1 > 0)\nresult = gated or (x < 0)",
                         (("bool", "enabled"), ("int", "x")), "bool")
        rows = [(False, gpu.I64_MAX), (False, -2), (True, 5), (True, -2)]
        for fuel in range(25):
            self.assertEqual(core_rows(self.host(source, rows, fuel=fuel)), reference(source, rows, fuel=fuel))
        conditional = program("result = x if skip else x+1", (("int", "x"), ("bool", "skip")))
        self.assertEqual(self.host(conditional, [(gpu.I64_MAX, True)])[0]["result"], str(gpu.I64_MAX))
        self.assertEqual(self.host(conditional, [(gpu.I64_MAX, False)])[0]["failure_kind"], "target-domain")

    def test_checked_arithmetic_endpoints_and_independent_random_integer_oracle(self):
        rng = random.Random(705)
        endpoints = [gpu.I64_MIN, gpu.I64_MIN+1, -2, -1, 0, 1, 2, gpu.I64_MAX-1, gpu.I64_MAX]
        rows = list(itertools.product(endpoints, repeat=2))
        rows.extend((rng.randrange(gpu.I64_MIN, gpu.I64_MAX+1), rng.randrange(gpu.I64_MIN, gpu.I64_MAX+1)) for _ in range(700))
        for operator, function in (("+", lambda x,y:x+y), ("-", lambda x,y:x-y), ("*", lambda x,y:x*y)):
            source = program("result = x "+operator+" y", (("int", "x"), ("int", "y")))
            for args, row in zip(rows, self.host(source, rows)):
                exact = function(*args)
                if gpu.I64_MIN <= exact <= gpu.I64_MAX:
                    self.assertEqual((row["result"], row["exit_code"]), (str(exact), 0))
                else:
                    self.assertEqual((row["result"], row["exit_code"], row["failure_kind"], row["error_line"]),
                                     (None, 1, "target-domain", 3))

    def test_minimum_literal_unary_and_output_assignment_semantics(self):
        source = program("int y = -9223372036854775808\nresult = y if x != 0 else -y")
        actual = self.host(source, [(1,), (0,)])
        self.assertEqual(actual[0]["result"], str(gpu.I64_MIN))
        self.assertEqual(actual[1]["failure_kind"], "target-domain")
        source = program("int y = +x\nx = x + 1\nresult = y - x\nreturn result")
        self.assertEqual([row["result"] for row in self.host(source, [(-4,), (5,)])], ["-1", "-1"])
        boolean = program("result = not x", (("bool", "x"),), "bool")
        self.assertEqual([row["result"] for row in self.host(boolean, [(False,), (True,)])], ["true", "false"])


class AdmissionAndDriverTests(unittest.TestCase):
    def test_rejects_unsupported_types_calls_effects_and_out_of_domain_literals(self):
        cases = [program("result = x", (("ratio", "x"),), "ratio"),
                 program("result = x // 2"), program("result = x % 2"), program("result = int(x)"),
                 program("result = x if true else int(x)"), program("emit x\nresult = x"),
                 program("return x\nemit x\nresult = x"), program("result = 9223372036854775808")]
        for source in cases:
            with self.subTest(source=source), self.assertRaises(DecError):
                gpu.lower_cuda(source, "chosen")

    def test_exact_input_types_limits_and_opt_reject_before_device_probe(self):
        source = program("result = x")
        with patch.object(gpu, "_execute", side_effect=AssertionError("device called")):
            for rows in ([(True,)], [(1.0,)], [(gpu.I64_MAX+1,)], [(gpu.I64_MIN-1,)], [[1, 2]]):
                with self.subTest(rows=rows), self.assertRaises(DecError):
                    gpu.run_gpu_batch(source, "chosen", rows)
            for kwargs in ({"fuel": True}, {"max_depth": True}, {"opt": True}, {"opt": 9}):
                with self.subTest(kwargs=kwargs), self.assertRaises(DecError):
                    gpu.run_gpu_batch(source, "chosen", [(1,)], **kwargs)
        bool_source = program("result = x", (("bool", "x"),), "bool")
        with self.assertRaises(DecError):
            gpu.run_gpu_batch(bool_source, "chosen", [(1,)])

    def test_required_gpu_unavailable_has_no_cpu_fallback(self):
        report = {"status": "UNAVAILABLE", "reason": "isolated device", "gpu_executed": False}
        with patch.object(gpu.cuda, "probe_gpu", return_value=report):
            with self.assertRaises(gpu.GPUUnavailable) as raised:
                gpu.run_gpu_batch(INCIDENCE, "chosen", [(1, 0, 1)])
        self.assertEqual(raised.exception.report, report)

    def test_returned_device_values_are_consumed_and_bad_status_is_rejected(self):
        source = program("result = x - 1")
        generated_hash = gpu.lower_cuda(source, "chosen")["source_binding"]["generated_cuda_sha256"]
        profile = {"gpu_executed": True, "launch_count": 1,
                   "compilation": {"generated_cuda_sha256": generated_hash, "ptx_sha256": "1"*64}}
        with patch.object(gpu, "_execute", return_value=(((77, 0, 0, 99995),), profile)):
            self.assertEqual(gpu.run_gpu_batch(source, "chosen", [(5,)])["rows"][0]["result"], "77")
        entry = select_entry(source, "chosen")
        for row in ((0, 5, 0, 1), (7, 3, 3, 1), (0, 3, 99, 1), (0, 1, 3, 1), (0, 0, 0, -1)):
            with self.subTest(row=row), self.assertRaises(DecError):
                gpu._materialize(entry, [row], fuel=10)

    def test_source_mutation_and_wrong_compilation_binding_reject(self):
        source = program("result = x")
        hashes = gpu._source_bindings()
        altered = dict(hashes)
        altered["native/gpu_compiler.py"] = "0"*64
        profile = {"gpu_executed": True, "launch_count": 1,
                   "compilation": {"generated_cuda_sha256": "0"*64, "ptx_sha256": "1"*64}}
        with patch.object(gpu, "_execute", return_value=(((5, 0, 0, 99997),), profile)):
            with self.assertRaisesRegex(DecError, "compiled artifact differs"):
                gpu.run_gpu_batch(source, "chosen", [(5,)])
            with patch.object(gpu, "_source_bindings", side_effect=(hashes, hashes, altered)):
                with self.assertRaisesRegex(DecError, "sources changed"):
                    gpu.run_gpu_batch(source, "chosen", [(5,)])

    def test_actual_nvrtc_compiles_generated_source_without_execution_claim(self):
        generated = gpu.lower_cuda(INCIDENCE, "chosen")
        for opt in (0, 3):
            try:
                compiled = gpu.compile_cuda(generated["source"], opt=opt)
            except gpu.GPUUnavailable as exc:
                self.skipTest(exc.report["reason"])
            self.assertEqual(compiled["status"], "COMPILED")
            self.assertFalse(compiled["gpu_executed"])
            self.assertEqual(compiled["generated_cuda_sha256"], generated["source_binding"]["generated_cuda_sha256"])
            self.assertIn(gpu.KERNEL_NAME.encode(), compiled["ptx"])

    def test_empty_batch_is_not_device_execution(self):
        report = {"status": "AVAILABLE", "driver_version": 1,
                  "devices": [{"index": 0, "name": "test", "compute_capability": [8, 6]}]}
        with patch.object(gpu.cuda, "probe_gpu", return_value=report), \
             patch.object(gpu, "compile_cuda", side_effect=AssertionError("empty compilation")):
            result = gpu.run_gpu_batch(INCIDENCE, "chosen", [])
        self.assertEqual(result["rows"], [])
        self.assertEqual(result["execution"]["status"], "NO_WORK")
        self.assertFalse(result["execution"]["gpu_executed"])


class SessionDriver:
    """Explicit fake Driver for lifetime tests, never device-execution evidence.

    Its launch computes the independent identity fixture only. Actual code and
    device semantics are covered by the generated-body and hardware tests.
    """
    def __init__(self):
        self.local = threading.local()
        self.contexts, self.memory, self.modules = set(), {}, {}
        self.next_context, self.next_module = 100, 1000
        self.calls, self.failures = {}, {}
        self.active, self.maximum_active = 0, 0

    def stack(self):
        if not hasattr(self.local, 'stack'):
            self.local.stack = [77]  # Caller-owned context, never ours to free.
        return self.local.stack

    def call(self, name):
        self.calls[name] = self.calls.get(name, 0)+1
        if self.failures.get(name, 0):
            self.failures[name] -= 1
            return 999
        return 0

    def cuGetErrorName(self, code, pointer):
        pointer._obj.value = b'CUDA_ERROR_INJECTED'
        return 0

    def cuDeviceGet(self, pointer, index):
        pointer._obj.value = 0
        return self.call('device')

    def cuCtxCreate_v2(self, pointer, flags, device):
        error = self.call('create')
        if error:
            return error
        self.next_context += 1
        pointer._obj.value = self.next_context
        self.contexts.add(self.next_context)
        self.stack().append(self.next_context)
        return 0

    def cuCtxPushCurrent_v2(self, context):
        error = self.call('push')
        if error:
            return error
        if context.value not in self.contexts:
            raise RuntimeError('pushed foreign/dead context')
        self.stack().append(context.value)
        return 0

    def cuCtxPopCurrent_v2(self, pointer):
        error = self.call('pop')
        if error:
            return error
        pointer._obj.value = self.stack().pop()
        return 0

    def cuCtxDestroy_v2(self, context):
        error = self.call('destroy')
        if error:
            return error
        self.contexts.remove(context.value)
        self.memory = {p: value for p, value in self.memory.items() if value[0] != context.value}
        self.modules = {p: owner for p, owner in self.modules.items() if owner != context.value}
        self.local.stack = [value for value in self.stack() if value != context.value]
        return 0

    def cuModuleLoadData(self, pointer, ptx):
        error = self.call('load')
        if error:
            return error
        self.next_module += 1
        pointer._obj.value = self.next_module
        self.modules[self.next_module] = self.stack()[-1]
        return 0

    def cuModuleGetFunction(self, pointer, module, name):
        pointer._obj.value = module.value
        return self.call('function')

    def cuModuleUnload(self, module):
        error = self.call('unload')
        if not error:
            del self.modules[module.value]
        return error

    def cuMemAlloc_v2(self, pointer, size):
        error = self.call('allocate')
        if error:
            return error
        buffer = C.create_string_buffer(size)
        pointer._obj.value = C.addressof(buffer)
        self.memory[pointer._obj.value] = self.stack()[-1], buffer
        return 0

    def cuMemFree_v2(self, pointer):
        error = self.call('free')
        if not error:
            del self.memory[pointer.value]
        return error

    def cuMemcpyHtoD_v2(self, pointer, host, size):
        error = self.call('to_device')
        if not error:
            if size > C.sizeof(self.memory[pointer.value][1]):
                raise RuntimeError('input write beyond allocated capacity')
            C.memmove(pointer.value, host, size)
        return error

    def cuMemcpyDtoH_v2(self, host, pointer, size):
        error = self.call('from_device')
        if not error:
            if size > C.sizeof(self.memory[pointer.value][1]):
                raise RuntimeError('output read beyond allocated capacity')
            C.memmove(host, pointer.value, size)
        return error

    def cuCtxSynchronize(self):
        return self.call('sync')

    def cuLaunchKernel(self, function, gx, gy, gz, bx, by, bz, shared, stream, args, extra):
        error = self.call('launch')
        if error:
            return error
        self.active += 1
        self.maximum_active = max(self.maximum_active, self.active)
        try:
            time.sleep(0.002)  # Release the GIL so lock serialization is tested.
            addresses = [C.cast(args[i], C.POINTER(C.c_uint64)).contents.value for i in range(5)]
            count = C.cast(args[5], C.POINTER(C.c_uint)).contents.value
            fuel = C.cast(args[6], C.POINTER(C.c_int64)).contents.value
            depth = C.cast(args[7], C.POINTER(C.c_int32)).contents.value
            source = C.cast(addresses[0], C.POINTER(C.c_int64))
            values, errors, lines, remaining = (C.cast(address, C.POINTER(kind)) for address, kind in
                zip(addresses[1:], (C.c_int64, C.c_int32, C.c_int32, C.c_int64)))
            for row in range(count):
                status = 1 if not fuel else 2 if not depth else 1 if fuel < 3 else 0
                values[row], errors[row], lines[row] = 0 if status else source[row], status, 0
                remaining[row] = max(0, fuel-(1 if status == 2 else 3))
        finally:
            self.active -= 1
        return 0


class SessionLifetimeTests(unittest.TestCase):
    def setUp(self):
        self.driver = SessionDriver()
        self.probe = {'status': 'AVAILABLE', 'driver_version': 1,
                      'devices': [{'index': 0, 'name': 'fake-lifetime-driver', 'compute_capability': [8, 6]}]}
        def compile_fake(source, architecture, opt=0):
            raw = b'lifetime test PTX\0'
            return {'status': 'COMPILED', 'gpu_executed': False, 'ptx': raw,
                    'generated_cuda_sha256': hashlib.sha256(source.encode()).hexdigest(),
                    'ptx_sha256': hashlib.sha256(raw).hexdigest(), 'requested_opt': opt,
                    'architecture': architecture, 'nvrtc_version': [0, 0], 'compiler_options': []}
        self.patches = [patch.object(gpu.cuda, 'probe_gpu', return_value=self.probe),
                        patch.object(gpu.cuda, '_driver', return_value=self.driver),
                        patch.object(gpu.cuda, '_function', side_effect=lambda *args: None),
                        patch.object(gpu, 'compile_cuda', side_effect=compile_fake)]
        for item in self.patches:
            item.start()
        self.addCleanup(lambda: [item.stop() for item in reversed(self.patches)])
        self.source = program('result = x')

    def test_context_kernel_and_buffers_reuse_growth_and_no_stale_rows(self):
        with gpu.GpuBatchSession(self.source) as session:
            first = session.run('chosen', [(1,), (2,)])
            repeated = session.run('chosen', [(-3,)])
            grown = session.run('chosen', [(i,) for i in range(12)])
            shrunk = session.run('chosen', [(99,)])
            self.assertFalse(first['execution']['context_reused'])
            self.assertTrue(repeated['execution']['context_reused'])
            self.assertTrue(repeated['execution']['compilation_cache_hit'])
            self.assertEqual(repeated['execution']['buffers_reused'], 5)
            self.assertEqual(grown['execution']['buffers_reused'], 0)
            self.assertEqual(shrunk['execution']['buffers_reused'], 5)
            self.assertEqual([r['result'] for r in grown['rows']], list(map(str, range(12))))
            self.assertEqual([r['result'] for r in shrunk['rows']], ['99'])
            self.assertEqual(session.stats['contexts_created'], 1)
            self.assertEqual(session.stats['kernels_compiled'], 1)
            self.assertEqual(self.driver.stack(), [77])
            # Returned metadata is a copy; it cannot mutate a cached binding.
            repeated['execution']['compilation']['ptx_sha256'] = 'bad'
            self.assertNotEqual(session.run('chosen', [(4,)])['source_binding']['ptx_sha256'], 'bad')
        self.assertEqual((self.driver.contexts, self.driver.memory, self.driver.modules), (set(), {}, {}))
        self.assertEqual(self.driver.stack(), [77])

    def test_source_snapshot_signature_cache_and_post_close_rejection(self):
        source = self.source.replace(MAIN, '\nDec second(x) -> result:\n    (int x) -> int result:\n        result = x\n'+MAIN)
        ir = compile_source(source)
        session = gpu.GpuBatchSession(ir)
        ir['source'] = 'caller changed original dict'
        with session:
            for entry in ('chosen', 'second', 'chosen'):
                self.assertEqual(session.run(entry, [(8,)])['rows'][0]['result'], '8')
            self.assertEqual(session.stats['kernels_compiled'], 2)
            with patch.object(gpu, 'MAX_SESSION_KERNELS', 2):
                self.assertEqual(session.run('chosen', [(8,)])['rows'][0]['result'], '8')
                with self.assertRaisesRegex(DecError, 'compiled-entry budget'):
                    session.run('main', [()])
            with self.assertRaises(DecError):
                session.run('chosen', [(True,)])
            self.assertEqual(session.run('chosen', [(8,)])['rows'][0]['result'], '8')
        session.close()
        with self.assertRaisesRegex(DecError, 'closed'):
            session.run('chosen', [(8,)])

    def test_lane_budget_error_then_success_and_input_guard_recovery(self):
        with gpu.GpuBatchSession(self.source) as session:
            self.assertEqual(session.run('chosen', [(8,)], fuel=0)['rows'][0]['error'], 'execution step budget exhausted')
            self.assertEqual(session.run('chosen', [(8,)], max_depth=0)['rows'][0]['error'], 'call depth limit exceeded')
            with self.assertRaises(DecError):
                session.run('chosen', [(gpu.I64_MAX+1,)])
            self.assertEqual(session.run('chosen', [(8,)])['rows'][0]['result'], '8')
            self.assertEqual(session.stats['contexts_created'], 1)
            self.assertEqual(session.stats['failed_device_calls'], 0)

    def test_failed_launch_and_failed_growth_discard_then_recover(self):
        with gpu.GpuBatchSession(self.source) as session:
            session.run('chosen', [(2,)])
            self.driver.failures['launch'] = 1
            with self.assertRaisesRegex(RuntimeError, 'cuLaunchKernel'):
                session.run('chosen', [(3,)])
            self.assertEqual((self.driver.contexts, self.driver.memory, self.driver.modules), (set(), {}, {}))
            self.assertEqual(session.run('chosen', [(4,)])['rows'][0]['result'], '4')
            self.driver.failures['allocate'] = 1
            with self.assertRaisesRegex(RuntimeError, 'cuMemAlloc'):
                session.run('chosen', [(i,) for i in range(10)])
            self.assertEqual(session.run('chosen', [(5,)])['rows'][0]['result'], '5')
            self.assertEqual(session.stats['failed_device_calls'], 2)
            self.assertEqual(session.stats['contexts_created'], 3)

    def test_close_attempts_all_resources_preserves_primary_and_can_retry_destroy(self):
        session = gpu.GpuBatchSession(self.source)
        session.run('chosen', [(2,)])
        self.driver.failures.update(free=1, unload=1, destroy=1)
        primary = ValueError('original model outcome')
        session.__exit__(ValueError, primary, None)
        self.assertEqual(str(primary), 'original model outcome')
        self.assertIn('cleanup failures', '\n'.join(primary.__notes__))
        self.assertEqual(self.driver.calls['free'], 5)
        self.assertEqual(self.driver.calls['unload'], 1)
        self.assertEqual(self.driver.calls['destroy'], 1)
        self.assertTrue(session.stats['broken'])
        session.close()
        self.assertEqual((self.driver.contexts, self.driver.memory, self.driver.modules), (set(), {}, {}))
        self.assertEqual(self.driver.stack(), [77])

    def test_failed_context_restore_is_fail_closed(self):
        session = gpu.GpuBatchSession(self.source)
        self.driver.failures['pop'] = 1
        with self.assertRaisesRegex(RuntimeError, 'cuCtxPopCurrent'):
            session.run('chosen', [(2,)])
        with self.assertRaisesRegex(DecError, 'context/cleanup failed'):
            session.run('chosen', [(3,)])
        session.close()
        self.assertEqual(self.driver.stack(), [77])

    def test_thread_migration_serializes_and_restores_each_calling_thread(self):
        session = gpu.GpuBatchSession(self.source)
        def invoke(value):
            row = session.run('chosen', [(value,)])['rows'][0]
            self.assertEqual(self.driver.stack(), [77])
            return row['result']
        with ThreadPoolExecutor(max_workers=3) as executor:
            self.assertEqual(list(executor.map(invoke, range(9))), list(map(str, range(9))))
            executor.submit(session.close).result()
        self.assertEqual(session.stats['contexts_created'], 1)
        self.assertEqual(self.driver.maximum_active, 1)
        self.assertEqual(self.driver.contexts, set())

    def test_source_change_between_calls_rejects_without_launching(self):
        with gpu.GpuBatchSession(self.source) as session:
            session.run('chosen', [(1,)])
            altered = gpu._source_bindings()
            altered['native/gpu_compiler.py'] = '0'*64
            with patch.object(gpu, '_source_bindings', return_value=altered):
                with self.assertRaisesRegex(DecError, 'sources changed'):
                    session.run('chosen', [(2,)])
            self.assertEqual(self.driver.calls['launch'], 1)
            self.assertEqual(session.run('chosen', [(3,)])['rows'][0]['result'], '3')


class ActualDeviceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        probe = gpu.cuda.probe_gpu()
        if probe["status"] != "AVAILABLE":
            raise unittest.SkipTest(probe["reason"])

    def test_actual_source_compilation_truth_table_mutation_and_bindings(self):
        rows = list(itertools.product((0, 1), repeat=3))+[(2, 0, 1)]
        first = gpu.run_gpu_batch(INCIDENCE, "chosen", rows)
        self.assertEqual(core_rows(first["rows"]), reference(INCIDENCE, rows))
        self.assertTrue(first["execution"]["gpu_executed"])
        self.assertEqual(first["execution"]["launch_count"], 1)
        changed = INCIDENCE.replace("source - target", "target - source")
        second = gpu.run_gpu_batch(changed, "chosen", rows, opt=3)
        self.assertEqual(core_rows(second["rows"]), reference(changed, rows))
        self.assertNotEqual(first["source_binding"]["generated_cuda_sha256"], second["source_binding"]["generated_cuda_sha256"])
        self.assertNotEqual(first["source_binding"]["ptx_sha256"], second["source_binding"]["ptx_sha256"])

    def test_actual_source_fuel_depth_assertions_and_short_circuit(self):
        source = program("assert not enabled or x >= 0\nresult = x if not enabled else x+1",
                         (("bool", "enabled"), ("int", "x")))
        rows = [(False, gpu.I64_MAX), (True, -1), (True, 3)]
        for fuel, depth in ((0, 0), (1, 0), (7, 1), (100, 1)):
            with self.subTest(fuel=fuel, depth=depth):
                actual = gpu.run_gpu_batch(source, "chosen", rows, fuel=fuel, max_depth=depth)
                self.assertEqual(core_rows(actual["rows"]), reference(source, rows, fuel=fuel, max_depth=depth))

    def test_actual_checked_overflow_and_zero_argument_entry(self):
        source = program("result = x * y", (("int", "x"), ("int", "y")))
        rows = [(gpu.I64_MIN, -1), (gpu.I64_MIN, 1), (gpu.I64_MAX, 2), (0, gpu.I64_MIN), (-2, 7)]
        actual = gpu.run_gpu_batch(source, "chosen", rows)["rows"]
        self.assertEqual([row["result"] for row in actual], [None, str(gpu.I64_MIN), None, "0", "-14"])
        self.assertEqual(actual[0]["failure_kind"], "target-domain")
        source = program("result = -9223372036854775808", ())
        self.assertEqual(gpu.run_gpu_batch(source, "chosen", [(), ()])["rows"][0]["result"], str(gpu.I64_MIN))

    def test_actual_session_reuse_growth_entry_change_and_error_recovery(self):
        source = program('assert x >= 0\nresult = x+1')
        source = source.replace(MAIN, '\nDec negate(x) -> result:\n    (int x) -> int result:\n        result = -x\n'+MAIN)
        with gpu.GpuBatchSession(source) as session:
            first = session.run('chosen', [(i,) for i in range(4)])
            failed = session.run('chosen', [(-1,), (2,)], fuel=100)
            grown = session.run('chosen', [(i,) for i in range(129)])
            negate = session.run('negate', [(gpu.I64_MIN,), (3,)])
            last = session.run('chosen', [(9,)])
            self.assertEqual([row['result'] for row in first['rows']], ['1','2','3','4'])
            self.assertEqual([row['result'] for row in failed['rows']], [None,'3'])
            self.assertEqual([row['result'] for row in grown['rows']], list(map(str, range(1,130))))
            self.assertEqual([row['result'] for row in negate['rows']], [None,'-3'])
            self.assertEqual(last['rows'][0]['result'], '10')
            self.assertTrue(last['execution']['compilation_cache_hit'])
            self.assertEqual(last['execution']['buffers_reused'], 5)
            self.assertEqual(session.stats['contexts_created'], 1)
            self.assertEqual(session.stats['kernels_compiled'], 2)
        self.assertFalse(session.stats['live_context'])

    def test_actual_session_restores_caller_context_and_migrates_threads(self):
        driver = gpu.cuda._driver()
        gpu.cuda._function(driver, 'cuCtxGetCurrent', [C.POINTER(C.c_void_p)])
        device, caller = C.c_int(), C.c_void_p()
        gpu.cuda._check(driver, driver.cuDeviceGet(C.byref(device), 0), 'cuDeviceGet')
        gpu.cuda._check(driver, driver.cuCtxCreate_v2(C.byref(caller), 0, device), 'cuCtxCreate caller')
        try:
            with gpu.GpuBatchSession(program('result = x+1')) as session:
                session.run('chosen', [(1,)])
                current = C.c_void_p()
                gpu.cuda._check(driver, driver.cuCtxGetCurrent(C.byref(current)), 'cuCtxGetCurrent')
                self.assertEqual(current.value, caller.value)
                with ThreadPoolExecutor(max_workers=2) as executor:
                    def invoke(value):
                        return session.run('chosen', [(value,)])['rows'][0]['result']
                    self.assertEqual(list(executor.map(invoke, range(6))), list(map(str, range(1,7))))
                    executor.submit(session.close).result()
                gpu.cuda._check(driver, driver.cuCtxGetCurrent(C.byref(current)), 'cuCtxGetCurrent')
                self.assertEqual(current.value, caller.value)
                self.assertEqual(session.stats['contexts_created'], 1)
        finally:
            gpu.cuda._check(driver, driver.cuCtxDestroy_v2(caller), 'cuCtxDestroy caller')


if __name__ == "__main__":
    unittest.main()
