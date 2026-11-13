"""Unavailable hardware is a skip for device tests, never a GPU pass."""
from fractions import Fraction
from pathlib import Path
import ctypes as C
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from simulations.chiral_batch import (TicketBatch, TransitionTables, UINT32_MAX,
    buffer_inputs, build_ticket_batch, compile_model_tables, run_scalar)
from simulations.chiral_collision import CollisionConfig
from simulations import chiral_gpu as gpu


class ChiralGPUTests(unittest.TestCase):
    def setUp(self):
        config = CollisionConfig(contact_gain=Fraction(0))
        self.tables = compile_model_tables(config, ticket_denominator=64)
        self.batch = build_ticket_batch(config, range(4), ticket_denominator=64)

    def test_unavailable_has_no_cpu_execution_or_fallback(self):
        report = gpu._unavailable("test device absent")
        with patch.object(gpu, "probe_gpu", return_value=report):
            with self.assertRaises(gpu.GPUUnavailable) as caught:
                gpu.run_gpu(self.tables, self.batch)
        self.assertIs(caught.exception.report, report)
        self.assertFalse(report["gpu_executed"])
        self.assertNotIn("rows", report)

    def test_bad_input_fails_before_device_probe(self):
        with patch.object(gpu, "probe_gpu") as probe:
            with self.assertRaises(TypeError):
                gpu.run_gpu({}, self.batch)
            with self.assertRaises(ValueError):
                gpu.run_gpu(self.tables, self.batch, device_index=True)
            probe.assert_not_called()

    def test_empty_work_never_claims_kernel_execution(self):
        empty = TicketBatch(0, 64, ((0, 1, 1, -1),), ((),))
        probe = {"status": "AVAILABLE", "devices": [{"index": 0, "name": "mock"}]}
        with patch.object(gpu, "probe_gpu", return_value=probe), patch.object(gpu, "compile_kernel") as compile_:
            result = gpu.run_gpu(self.tables, empty)
        compile_.assert_not_called()
        self.assertEqual(result["status"], "NO_WORK")
        self.assertFalse(result["execution"]["gpu_executed"])
        self.assertEqual(result["rows"], ((),))

    def test_driver_cleanup_attempts_all_releases_and_aggregates_failures(self):
        class Driver:
            def __init__(self): self.calls = []
            def cuMemFree_v2(self, pointer):
                self.calls.append(("free", pointer.value))
                return 1 if pointer.value == 2 else 0
            def cuModuleUnload(self, module):
                self.calls.append(("module", module.value))
                return 1
            def cuCtxDestroy_v2(self, context):
                self.calls.append(("context", context.value))
                return 0
            def cuGetErrorName(self, code, name): return 1
        driver = Driver()
        with self.assertRaisesRegex(RuntimeError, "cuMemFree.*cuModuleUnload"):
            gpu._release_driver_resources(driver, [C.c_uint64(1), C.c_uint64(2)],
                                          C.c_void_p(3), C.c_void_p(4))
        self.assertEqual(driver.calls, [("free", 2), ("free", 1), ("module", 3), ("context", 4)])

    def test_driver_cleanup_preserves_primary_error_and_still_destroys_context(self):
        class Driver:
            def __init__(self): self.destroyed = False
            def cuMemFree_v2(self, pointer): return 1
            def cuCtxDestroy_v2(self, context): self.destroyed = True; return 0
            def cuGetErrorName(self, code, name): return 1
        driver, primary = Driver(), RuntimeError("original launch failed")
        gpu._release_driver_resources(driver, [C.c_uint64(1)], C.c_void_p(), C.c_void_p(2), primary)
        self.assertTrue(driver.destroyed)
        self.assertEqual(str(primary), "original launch failed")
        self.assertIn("cuMemFree", " ".join(getattr(primary, "__notes__", getattr(primary, "cleanup_failures", ()))))

    def test_nvrtc_cleanup_does_not_replace_compiler_error(self):
        primary, calls = RuntimeError("original compiler diagnostic"), []
        def failing_destroy():
            calls.append("destroy")
            raise RuntimeError("destroy failed")
        gpu._finish_cleanup([("nvrtcDestroyProgram", failing_destroy)], primary)
        self.assertEqual(calls, ["destroy"])
        self.assertEqual(str(primary), "original compiler diagnostic")
        self.assertIn("nvrtcDestroyProgram", " ".join(getattr(primary, "__notes__", getattr(primary, "cleanup_failures", ()))))

    def test_actual_nvrtc_compilation_is_not_device_execution(self):
        try:
            result = gpu.compile_kernel()
        except gpu.GPUUnavailable as exc:
            self.skipTest(str(exc))
        self.assertEqual(result["status"], "COMPILED")
        self.assertFalse(result["gpu_executed"])
        self.assertIn(b".entry chiral_batch", result["ptx"])
        self.assertNotIn("rows", result)

    def test_invalid_cuda_source_is_failure_not_unavailable(self):
        try:
            gpu._load("nvrtc")
        except gpu.GPUUnavailable as exc:
            self.skipTest(str(exc))
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "invalid.cu"
            path.write_text("this is invalid cuda syntax;\n")
            with patch.object(gpu, "KERNEL", path):
                with self.assertRaisesRegex(RuntimeError, "nvrtcCompileProgram"):
                    gpu.compile_kernel()

    def test_host_compiled_kernel_body_exactly_matches_scalar_without_gpu_claim(self):
        if not shutil.which("g++"):
            self.skipTest("g++ unavailable for host-only kernel-body check")
        # This compiles the same source body with scalar launch-index shims.
        # It tests integer arithmetic, not CUDA scheduling or device execution.
        source = ('#define __global__\n'
                  'struct Index { unsigned int x; };\n'
                  'static Index blockIdx, blockDim, threadIdx;\n'
                  '#include "' + gpu.KERNEL.as_posix() + '"\n'
                  'extern "C" void host_launch(const unsigned int* m, const unsigned int* q, '
                  'const int* i, const unsigned int* t, int* o, unsigned int n, unsigned int s) { '
                  'blockIdx.x=0; blockDim.x=n; for(unsigned int p=0;p<n;++p) { '
                  'threadIdx.x=p; chiral_batch(m,q,i,t,o,n,s); }}\n')
        movement = tuple((i,) * 8 for i in range(8))
        thresholds = tuple(tuple(UINT32_MAX if i == j else UINT32_MAX - 1 for j in range(8)) for i in range(8))
        edge_tables = TransitionTables(movement, thresholds, UINT32_MAX)
        edge_batch = TicketBatch(1, UINT32_MAX, ((0, 0, 1, -1), (0, 1, 1, -1)),
            (((0, 0, UINT32_MAX - 1, 0),), ((0, 0, UINT32_MAX - 1, UINT32_MAX - 2),)))
        with tempfile.TemporaryDirectory() as temp:
            src, library = Path(temp) / "kernel_host.cpp", Path(temp) / "kernel_host.so"
            src.write_text(source)
            subprocess.run(["g++", "-std=c++11", "-O2", "-shared", "-fPIC", str(src), "-o", str(library)],
                           check=True, capture_output=True, text=True, timeout=30)
            lib = C.CDLL(str(library))
            launch = lib.host_launch
            launch.argtypes = [C.c_void_p] * 5 + [C.c_uint32] * 2
            launch.restype = None
            for tables, batch in ((self.tables, self.batch), (edge_tables, edge_batch)):
                buffers = buffer_inputs(tables, batch)
                arrays = [(C.c_uint32 * len(buffers[k]))(*buffers[k])
                          for k in ("move_destinations", "flip_thresholds")]
                arrays += [(C.c_int32 * len(buffers["initial_states"]))(*buffers["initial_states"]),
                           (C.c_uint32 * len(buffers["tickets"]))(*buffers["tickets"])]
                output = (C.c_int32 * (len(batch.initial_states) * batch.steps * 7))()
                launch(*arrays, output, len(batch.initial_states), batch.steps)
                expected = run_scalar(tables, batch)
                self.assertEqual(tuple(output), tuple(v for path in expected["rows"] for row in path for v in row))

    def test_actual_gpu_matches_scalar_supplied_model(self):
        probe = gpu.probe_gpu()
        if probe["status"] != "AVAILABLE":
            self.skipTest(probe["reason"])
        result = gpu.run_gpu(self.tables, self.batch)
        self.assertTrue(result["execution"]["gpu_executed"])
        self.assertEqual(result["semantic_sha256"], run_scalar(self.tables, self.batch)["semantic_sha256"])

    def test_actual_gpu_preserves_uint32_thresholds_and_equality_boundary(self):
        probe = gpu.probe_gpu()
        if probe["status"] != "AVAILABLE":
            self.skipTest(probe["reason"])
        movement = tuple((i,) * 8 for i in range(8))
        thresholds = tuple(tuple(UINT32_MAX if i == j else UINT32_MAX - 1 for j in range(8)) for i in range(8))
        tables = TransitionTables(movement, thresholds, UINT32_MAX)
        batch = TicketBatch(1, UINT32_MAX, ((0, 0, 1, -1), (0, 1, 1, -1)),
                            (((0, 0, UINT32_MAX - 1, 0),), ((0, 0, UINT32_MAX - 1, UINT32_MAX - 2),)))
        actual = gpu.run_gpu(tables, batch)
        expected = run_scalar(tables, batch)
        self.assertEqual(actual["rows"], (((0, 0, -1, 1, 1, 1, 1),), ((0, 1, 1, 1, 0, 1, 0),)))
        self.assertEqual(actual["semantic_sha256"], expected["semantic_sha256"])


if __name__ == "__main__":
    unittest.main()
