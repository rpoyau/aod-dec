"""Native-thread compiler entry tests with independent source/arithmetical oracles."""
from pathlib import Path
import re
import shutil
import subprocess
from tempfile import TemporaryDirectory
import threading
import unittest
from unittest.mock import patch

from native import batch, compiler, cpu_batch as cpu
from runtime.dec_core import render
from runtime.dec_language import DecError, Interpreter, compile_source


MAIN = "\nDec main() -> result:\n    () -> int result:\n        result = 0\n"


def program(body, parameters=(("int", "x"),), result="int"):
    return ("Dec chosen("+", ".join(name for _, name in parameters)+") -> result:\n    ("+
            ", ".join(kind+" "+name for kind, name in parameters)+") -> "+result+" result:\n"+
            "\n".join("        "+line for line in body.splitlines())+"\n"+MAIN)


def reference(source, rows, *, fuel=100000, max_depth=128):
    ir, result = compile_source(source), []
    for row in rows:
        vm = Interpreter(ir, max_steps=fuel, max_depth=max_depth)
        try:
            result.append({"result": render(vm.call("chosen", row)), "stdout": "", "error": None,
                           "error_line": 0, "exit_code": 0})
        except DecError as exc:
            message = str(exc)
            line = re.match(r"line ([0-9]+):", message)
            result.append({"result": None, "stdout": "", "error": message,
                           "error_line": int(line[1]) if line else 0, "exit_code": 1})
    return result


class InputContractTests(unittest.TestCase):
    def test_exact_inputs_and_unsupported_entries_reject_before_native(self):
        source = program("result = x")
        with patch.object(cpu, "_execute", side_effect=AssertionError("native entered")):
            for rows in ([(True,)], [(1.0,)], [(1, 2)], [(1 << batch.MAX_INPUT_BITS,)], iter([(1,)])):
                with self.subTest(rows=type(rows)), self.assertRaises(DecError):
                    cpu.run_cpu_batch(source, "chosen", rows)
            for kwargs in ({"workers": True}, {"workers": 0}, {"workers": 33}, {"fuel": True},
                           {"fuel": -1}, {"max_depth": True}, {"opt": True}, {"opt": 4}):
                with self.subTest(kwargs=kwargs), self.assertRaises(DecError):
                    cpu.run_cpu_batch(source, "chosen", [(1,)], **kwargs)
            for body in ("emit x\nresult = x", "result = chosen(x)", "result = x // 2", "result = int(x)"):
                with self.subTest(body=body), self.assertRaises(DecError):
                    cpu.run_cpu_batch(program(body), "chosen", [(1,)])
        boolean = batch.select_entry(program("result = x", (("bool", "x"),), "bool"), "chosen")
        with self.assertRaises(DecError):
            batch.validate_inputs(boolean, [(1,)])

    def test_independent_resource_caps_and_explosive_intermediates(self):
        source = program("x = x * x\n"*19+"result = 0")
        with patch.object(cpu, "_execute", side_effect=AssertionError("native entered")):
            with self.assertRaisesRegex(DecError, "intermediate integer growth"):
                cpu.run_cpu_batch(source, "chosen", [(2,)])
        entry = batch.select_entry(program("result = x * x"), "chosen")
        for limit, value, message in (("MAX_ROWS", 1, "bounded"), ("MAX_INPUT_BYTES", 1, "byte budget"),
                                     ("MAX_TOTAL_WORK", 1, "source-work"),
                                     ("MAX_INTEGER_BIT_WORK", 1, "integer-work")):
            with self.subTest(limit=limit), patch.object(batch, limit, value):
                with self.assertRaisesRegex(DecError, message):
                    batch.validate_inputs(entry, [(3,), (4,)])

    def test_entry_bindings_and_signature_mutation_are_rejected(self):
        entry = batch.select_entry(program("result = x * x"), "chosen")
        copied = batch.validate_inputs(entry, [[7]])
        self.assertEqual(copied, ((7,),))
        object.__setattr__(entry, "max_steps", True)
        with self.assertRaisesRegex(DecError, "source-work bound"):
            batch.entry_binding(entry)
        entry = batch.select_entry(program("result = x * x"), "chosen")
        entry.branch["body"][0]["value"]["op"] = "+"
        with self.assertRaisesRegex(DecError, "changed"):
            batch.entry_binding(entry)

    def test_input_snapshot_is_validated_before_caller_list_changes(self):
        entry = batch.select_entry(program("result = x * x"), "chosen")
        row = [7]
        original = batch.require
        def mutate_after_type_check(condition, message):
            original(condition, message)
            if message == "compiled batch input requires exact int":
                row[0] = True
        with patch.object(batch, "require", side_effect=mutate_after_type_check):
            admitted = batch.validate_inputs(entry, [row])
        self.assertEqual(admitted, ((7,),))
        self.assertIs(type(admitted[0][0]), int)
        self.assertEqual(row, [True])

    def test_overload_requires_explicit_signature_and_only_selected_branch_is_pure(self):
        source = ("Dec chosen(x) -> result:\n    (int x) -> int result:\n        result = x + 1\n"
                  "    (bool x) -> bool result:\n        emit x\n        result = x\n"+MAIN)
        with self.assertRaisesRegex(DecError, "selected signature"):
            batch.select_entry(source, "chosen")
        entry = batch.select_entry(source, "chosen", param_types=["int"])
        self.assertEqual(entry.param_types, ("int",))
        with self.assertRaisesRegex(DecError, "effects"):
            batch.select_entry(source, "chosen", param_types=["bool"])


class NativeBatchTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if shutil.which("g++") is None:
            raise unittest.SkipTest("g++ unavailable for native compiler batches")
        compiler.ensure_runtime()
        compiler.llvm()

    def test_generated_entry_on_actual_native_threads_and_exact_big_integers(self):
        source = program("result = x * x - 3")
        values = [0, -1, 2, -(2**200+3), 2**700+11]*4
        rows = [(value,) for value in values]
        with patch.object(Interpreter, "call", side_effect=AssertionError("interpreter used")):
            result = cpu.run_cpu_batch(source, "chosen", rows, workers=4)
        self.assertEqual([row["result"] for row in result["rows"]], [str(value*value-3) for value in values])
        execution = result["execution"]
        self.assertEqual(execution["native_dispatch_calls"], 1)
        self.assertEqual(execution["compiled_entries_entered"], len(rows))
        self.assertEqual(execution["workers_used"], 4)
        self.assertEqual(len(set(execution["worker_ids"])), 4)
        self.assertGreater(execution["entry_address"], 0)
        self.assertEqual(execution["dispatches"]["specialized"], len(rows))
        self.assertIn("native/batch_dispatch.cpp", result["source_binding"]["implementation_sha256"])

    def test_source_and_optimization_changes_drive_actual_compiled_output(self):
        source = program("result = x - 1")
        first = cpu.run_cpu_batch(source, "chosen", [(3,)], workers=1)
        second = cpu.run_cpu_batch(source.replace("x - 1", "x + 2"), "chosen", [(3,)], workers=2, opt=3)
        self.assertEqual((first["rows"][0]["result"], second["rows"][0]["result"]), ("2", "5"))
        self.assertNotEqual(first["source_binding"]["entry"]["source_sha256"],
                            second["source_binding"]["entry"]["source_sha256"])
        self.assertNotEqual(first["source_binding"]["thunk"]["ir_sha256"],
                            second["source_binding"]["thunk"]["ir_sha256"])

    def test_assertions_fuel_depth_short_circuit_and_context_isolation(self):
        source = program("assert not enabled or x != 0\nint y = +x + 3\n"
                         "result = y * x if enabled and x > 0 else -y",
                         (("bool", "enabled"), ("int", "x")))
        rows = [(False, 0), (True, 0), (True, 7), (False, -5), (True, 0), (True, 1)]
        entry = batch.select_entry(source, "chosen")
        with compiler.JitSession(entry.program) as session:
            for fuel in range(42):
                with self.subTest(fuel=fuel):
                    inputs = batch.validate_inputs(entry, rows, fuel=fuel)
                    actual, _, _ = cpu._execute(session, entry, inputs, workers=3, fuel=fuel, max_depth=128)
                    self.assertEqual(actual, reference(source, rows, fuel=fuel))
            for fuel, depth in ((0, 0), (1, 0), (100, 0), (100, 1)):
                actual, _, _ = cpu._execute(session, entry, tuple(rows), workers=2, fuel=fuel, max_depth=depth)
                self.assertEqual(actual, reference(source, rows, fuel=fuel, max_depth=depth))

    def test_boolean_early_return_and_zero_parameter_entries(self):
        source = program("bool y = not x\nreturn y", (("bool", "x"),), "bool")
        self.assertEqual(cpu.run_cpu_batch(source, "chosen", [(False,), (True,)])["rows"],
                         reference(source, [(False,), (True,)]))
        source = program("result = 19", ())
        self.assertEqual([row["result"] for row in cpu.run_cpu_batch(source, "chosen", [(), ()], workers=2)["rows"]],
                         ["19", "19"])

    def test_empty_batch_has_no_worker_or_entered_function_claim(self):
        result = cpu.run_cpu_batch(program("result = x"), "chosen", [], workers=4)
        self.assertEqual(result["rows"], [])
        self.assertEqual(result["execution"]["status"], "NO_WORK")
        self.assertEqual(result["execution"]["worker_ids"], [])
        self.assertEqual(result["execution"]["compiled_entries_entered"], 0)

    def test_output_budget_failure_is_joined_and_following_batch_remains_valid(self):
        source = program("result = x * x")
        with patch.object(cpu, "MAX_OUTPUT_BYTES", 1):
            with self.assertRaisesRegex(DecError, "output byte budget"):
                cpu.run_cpu_batch(source, "chosen", [(12345,)]*8, workers=4)
        result = cpu.run_cpu_batch(source, "chosen", [(7,)], workers=1)
        self.assertEqual(result["rows"][0]["result"], "49")

    def test_source_mutation_rejects_before_dispatch_and_after_execution(self):
        source = program("result = x")
        initial = cpu.source_bindings()
        changed = dict(initial, **{"native/cpu_batch.py": "0"*64})
        entry = batch.select_entry(source, "chosen")
        with compiler.JitSession(entry.program) as session:
            cpu._load_thunk(session, entry)
            library, binding = cpu._dispatcher()
            class NeverDispatch:
                def __getattr__(self, name):
                    if name == "dec_batch_execute":
                        raise AssertionError("mutated sources executed")
                    return getattr(library, name)
            with patch.object(cpu, "_dispatcher", return_value=(NeverDispatch(), binding)), \
                 patch.object(cpu, "source_bindings", return_value=changed):
                with self.assertRaisesRegex(DecError, "before native dispatch"):
                    cpu._execute(session, entry, ((5,),), workers=1, fuel=100, max_depth=128, expected_sources=initial)
        original = cpu._execute
        def changed_after(*args, **kwargs):
            result = original(*args, **kwargs)
            mutation.start()
            return result
        mutation = patch.object(cpu, "source_bindings", return_value=changed)
        try:
            with patch.object(cpu, "_execute", side_effect=changed_after):
                with self.assertRaisesRegex(DecError, "during execution"):
                    cpu.run_cpu_batch(source, "chosen", [(5,)])
        finally:
            mutation.stop()

    def test_session_close_waits_for_native_dispatch_and_closed_session_rejects(self):
        source = program("result = x * x")
        entry = batch.select_entry(source, "chosen")
        session = compiler.JitSession(entry.program)
        library, binding = cpu._dispatcher()
        started, release, closed = threading.Event(), threading.Event(), threading.Event()
        outcomes, errors = [], []
        class BlockedDispatch:
            def __getattr__(self, name):
                return getattr(library, name)
            def dec_batch_execute(self, *args):
                started.set()
                if not release.wait(10):
                    raise RuntimeError("test dispatch release timeout")
                return library.dec_batch_execute(*args)
        def execute():
            try:
                outcomes.append(cpu._execute(session, entry, ((7,), (11,)), workers=2, fuel=100, max_depth=128)[0])
            except BaseException as exc:
                errors.append(exc)
        def close():
            session.close()
            closed.set()
        with patch.object(cpu, "_dispatcher", return_value=(BlockedDispatch(), binding)):
            worker = threading.Thread(target=execute)
            worker.start()
            self.assertTrue(started.wait(10))
            closer = threading.Thread(target=close)
            closer.start()
            try:
                self.assertFalse(closed.wait(0.05))
            finally:
                release.set()
                worker.join(10)
                closer.join(10)
        self.assertFalse(worker.is_alive() or closer.is_alive())
        self.assertEqual(errors, [])
        self.assertEqual([row["result"] for row in outcomes[0]], ["49", "121"])
        self.assertTrue(closed.is_set())
        with self.assertRaisesRegex(DecError, "closed"):
            cpu._execute(session, entry, ((3,),), workers=1, fuel=100, max_depth=128)

    def test_cpp_failure_reporting_survives_allocation_failure(self):
        """Isolated no-OOM fault injection into the actual C++ failure reporter."""
        runtime, _ = compiler.ensure_runtime()
        with TemporaryDirectory() as directory:
            source = Path(directory)/"failure.cpp"
            executable = Path(directory)/"failure"
            source.write_text('#include <cstdlib>\n#include <new>\n'
                'static bool deny_allocation=false;\n'
                'void* operator new(std::size_t n) { if(deny_allocation) throw std::bad_alloc(); '
                'if(auto* p=std::malloc(n)) return p; throw std::bad_alloc(); }\n'
                'void operator delete(void* p) noexcept { std::free(p); }\n'
                'void operator delete(void* p,std::size_t) noexcept { std::free(p); }\n'
                '#include "'+str(cpu.ROOT/'native/batch_dispatch.cpp')+'"\n'
                'int main() { Batch batch(0); static_assert(noexcept(batch.fail("failure")), "must not throw"); '
                'deny_allocation=true; batch.fail("This deliberately long error requires allocation beyond string inline storage"); '
                'deny_allocation=false; if(!batch.failed.load()) return 1; '
                'if(std::string(dec_batch_error(&batch))!="native batch allocation or worker failure") return 2; '
                'return 0; }\n')
            subprocess.run([shutil.which("g++"), "-std=c++17", "-O2", "-pthread", str(source), str(runtime),
                            "-o", str(executable)], check=True, capture_output=True, timeout=30)
            subprocess.run([str(executable)], check=True, capture_output=True, timeout=10)


if __name__ == "__main__":
    unittest.main()
