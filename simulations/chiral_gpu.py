"""CUDA execution of caller-supplied integer tables and tickets.

No Python GPU package is required: ctypes binds the CUDA Driver API and NVRTC.
Compilation alone is not GPU execution. Missing devices/libraries are explicit
UNAVAILABLE results; compilation, allocation or launch errors otherwise fail.
Full D.E.C. return certificates and routing admission remain host obligations.

Primary API references (CUDA 12.9.1, checked 2026-11-13):
https://docs.nvidia.com/cuda/archive/12.9.1/nvrtc/index.html
https://docs.nvidia.com/cuda/archive/12.9.1/cuda-driver-api/group__CUDA__EXEC.html
https://docs.nvidia.com/cuda/archive/12.9.1/cuda-driver-api/group__CUDA__DEVICE.html
"""
from __future__ import annotations

import ctypes as C
from ctypes.util import find_library
import hashlib
from pathlib import Path
import re
import sys
import time

from .chiral_batch import buffer_inputs, result_record, validate_batch

KERNEL = Path(__file__).with_name("kernels") / "chiral_batch.cu"


class GPUUnavailable(RuntimeError):
    def __init__(self, report):
        self.report = report
        super().__init__(report["reason"])


def _unavailable(reason, **details):
    return {"status": "UNAVAILABLE", "backend": "cuda-gpu", "reason": reason,
            "gpu_executed": False, **details}


def _load(name):
    library = find_library(name)
    if not library:
        raise GPUUnavailable(_unavailable(f"{name} shared library not found"))
    try:
        return C.CDLL(library)
    except OSError as exc:
        raise GPUUnavailable(_unavailable(f"{name} shared library cannot be loaded: {exc}")) from exc


def _function(lib, name, arguments, result=C.c_int):
    function = getattr(lib, name)
    function.argtypes, function.restype = arguments, result
    return function


def _driver():
    lib = _load("cuda")
    p, u, i, size, pointer = C.c_void_p, C.c_uint, C.c_int, C.c_size_t, C.c_uint64
    declarations = {
        "cuInit": [u], "cuDeviceGetCount": [C.POINTER(i)],
        "cuDriverGetVersion": [C.POINTER(i)], "cuDeviceGet": [C.POINTER(i), i],
        "cuDeviceGetName": [p, i, i], "cuDeviceGetAttribute": [C.POINTER(i), i, i],
        "cuGetErrorName": [i, C.POINTER(C.c_char_p)],
        "cuCtxCreate_v2": [C.POINTER(p), u, i], "cuCtxDestroy_v2": [p],
        "cuCtxSynchronize": [], "cuModuleLoadData": [C.POINTER(p), p],
        "cuModuleGetFunction": [C.POINTER(p), p, C.c_char_p], "cuModuleUnload": [p],
        "cuMemAlloc_v2": [C.POINTER(pointer), size], "cuMemFree_v2": [pointer],
        "cuMemcpyHtoD_v2": [pointer, p, size], "cuMemcpyDtoH_v2": [p, pointer, size],
        "cuLaunchKernel": [p, u, u, u, u, u, u, u, p, C.POINTER(p), C.POINTER(p)],
    }
    for name, args in declarations.items():
        _function(lib, name, args)
    return lib


def _cuda_error(lib, code):
    name = C.c_char_p()
    if lib.cuGetErrorName(code, C.byref(name)) == 0 and name.value:
        return name.value.decode()
    return "CUDA error " + str(code)


def _check(lib, code, operation):
    if code:
        raise RuntimeError(operation + ": " + _cuda_error(lib, code))


def _finish_cleanup(actions, primary_error=None):
    """Attempt every release, preserving an error already in flight."""
    failures = []
    for operation, release in actions:
        try:
            release()
        except Exception as exc:
            failures.append(operation + ": " + str(exc))
    if failures:
        message = "GPU cleanup failures: " + "; ".join(failures)
        if primary_error is not None:
            if hasattr(primary_error, "add_note"):
                primary_error.add_note(message)
            else:
                primary_error.cleanup_failures = tuple(failures)
        else:
            raise RuntimeError(message)


def _release_driver_resources(driver, allocated, module, context, primary_error=None):
    actions = [("cuMemFree", lambda pointer=pointer:
                _check(driver, driver.cuMemFree_v2(pointer), "cuMemFree"))
               for pointer in reversed(allocated)]
    if module.value:
        actions.append(("cuModuleUnload", lambda:
                        _check(driver, driver.cuModuleUnload(module), "cuModuleUnload")))
    if context.value:
        actions.append(("cuCtxDestroy", lambda:
                        _check(driver, driver.cuCtxDestroy_v2(context), "cuCtxDestroy")))
    _finish_cleanup(actions, primary_error)


def probe_gpu():
    """Probe devices only; never report GPU execution from library presence."""
    try:
        driver = _driver()
    except GPUUnavailable as exc:
        return exc.report
    version = C.c_int()
    _check(driver, driver.cuDriverGetVersion(C.byref(version)), "cuDriverGetVersion")
    status = driver.cuInit(0)
    if status:
        return _unavailable("cuInit: " + _cuda_error(driver, status),
                            driver_version=version.value, cuda_status=status)
    count = C.c_int()
    _check(driver, driver.cuDeviceGetCount(C.byref(count)), "cuDeviceGetCount")
    if count.value == 0:
        return _unavailable("CUDA reported zero available devices", driver_version=version.value)
    devices = []
    for index in range(count.value):
        device, major, minor = C.c_int(), C.c_int(), C.c_int()
        name = C.create_string_buffer(256)
        _check(driver, driver.cuDeviceGet(C.byref(device), index), "cuDeviceGet")
        _check(driver, driver.cuDeviceGetName(name, len(name), device), "cuDeviceGetName")
        _check(driver, driver.cuDeviceGetAttribute(C.byref(major), 75, device), "compute capability major")
        _check(driver, driver.cuDeviceGetAttribute(C.byref(minor), 76, device), "compute capability minor")
        devices.append({"index": index, "name": name.value.decode(),
                        "compute_capability": [major.value, minor.value]})
    return {"status": "AVAILABLE", "backend": "cuda-gpu", "gpu_executed": False,
            "driver_version": version.value, "devices": devices}


def compile_kernel(architecture="compute_75"):
    """Compile actual CUDA source to PTX; works without a GPU, when NVRTC exists.

    Returned PTX bytes are intended for the driver loader; omit them from JSON
    reports and retain the supplied digest instead. UNAVAILABLE raises the same
    typed exception as absent hardware; malformed source/compiler errors fail.
    """
    if not isinstance(architecture, str) or re.fullmatch(r"compute_[0-9]{2,3}", architecture) is None:
        raise ValueError("an explicit CUDA compute architecture is required")
    lib = _load("nvrtc")
    p, i, size = C.c_void_p, C.c_int, C.c_size_t
    for name, args in {
        "nvrtcVersion": [C.POINTER(i), C.POINTER(i)],
        "nvrtcCreateProgram": [C.POINTER(p), C.c_char_p, C.c_char_p, i, p, p],
        "nvrtcCompileProgram": [p, i, C.POINTER(C.c_char_p)],
        "nvrtcGetProgramLogSize": [p, C.POINTER(size)], "nvrtcGetProgramLog": [p, p],
        "nvrtcGetPTXSize": [p, C.POINTER(size)], "nvrtcGetPTX": [p, p],
        "nvrtcDestroyProgram": [C.POINTER(p)],
    }.items():
        _function(lib, name, args)
    _function(lib, "nvrtcGetErrorString", [i], C.c_char_p)

    def check(code, operation):
        if code:
            raise RuntimeError(operation + ": " + lib.nvrtcGetErrorString(code).decode())

    major, minor, program = i(), i(), p()
    check(lib.nvrtcVersion(C.byref(major), C.byref(minor)), "nvrtcVersion")
    source = KERNEL.read_bytes()
    check(lib.nvrtcCreateProgram(C.byref(program), source, b"chiral_batch.cu", 0, None, None),
          "nvrtcCreateProgram")
    try:
        options = (C.c_char_p * 2)(b"--std=c++11", ("--gpu-architecture=" + architecture).encode())
        status = lib.nvrtcCompileProgram(program, len(options), options)
        log_size = size()
        check(lib.nvrtcGetProgramLogSize(program, C.byref(log_size)), "nvrtcGetProgramLogSize")
        log = C.create_string_buffer(log_size.value)
        check(lib.nvrtcGetProgramLog(program, log), "nvrtcGetProgramLog")
        if status:
            raise RuntimeError("nvrtcCompileProgram: " + lib.nvrtcGetErrorString(status).decode()
                               + "\n" + log.value.decode())
        ptx_size = size()
        check(lib.nvrtcGetPTXSize(program, C.byref(ptx_size)), "nvrtcGetPTXSize")
        ptx = C.create_string_buffer(ptx_size.value)
        check(lib.nvrtcGetPTX(program, ptx), "nvrtcGetPTX")
        return {"status": "COMPILED", "gpu_executed": False, "architecture": architecture,
                "nvrtc_version": [major.value, minor.value], "ptx": ptx.raw,
                "source_sha256": hashlib.sha256(source).hexdigest(),
                "ptx_sha256": hashlib.sha256(ptx.raw).hexdigest(), "compiler_log": log.value.decode(),
                "scope": "Actual NVRTC compilation only; no device execution or acceleration demonstrated."}
    finally:
        _finish_cleanup([("nvrtcDestroyProgram", lambda:
            check(lib.nvrtcDestroyProgram(C.byref(program)), "nvrtcDestroyProgram"))],
            sys.exc_info()[1])


def run_gpu(tables, batch, *, device_index=0):
    """Execute supplied path decisions on a CUDA GPU, with no CPU fallback."""
    started = time.perf_counter_ns()
    validate_batch(tables, batch)
    if type(device_index) is not int or device_index < 0:
        raise ValueError("device index requires a nonnegative exact integer")
    probe = probe_gpu()
    if probe["status"] != "AVAILABLE":
        raise GPUUnavailable(probe)
    if device_index >= len(probe["devices"]):
        raise ValueError("requested CUDA device index does not exist")
    count = len(batch.initial_states)
    if not count or not batch.steps:
        report = result_record(tables, batch, ((),) * count, "cuda-gpu", {
            "gpu_executed": False, "launch_count": 0, "device": probe["devices"][device_index],
            "elapsed_ns": time.perf_counter_ns() - started})
        report["status"] = "NO_WORK"
        return report
    device_info = probe["devices"][device_index]
    capability = device_info["compute_capability"]
    compiled = compile_kernel("compute_" + "".join(map(str, capability)))
    driver, device = _driver(), C.c_int()
    _check(driver, driver.cuDeviceGet(C.byref(device), device_index), "cuDeviceGet")
    context, module, function = C.c_void_p(), C.c_void_p(), C.c_void_p()
    allocated = []
    try:
        _check(driver, driver.cuCtxCreate_v2(C.byref(context), 0, device), "cuCtxCreate")
        ptx = C.create_string_buffer(compiled["ptx"])
        _check(driver, driver.cuModuleLoadData(C.byref(module), ptx), "cuModuleLoadData")
        _check(driver, driver.cuModuleGetFunction(C.byref(function), module, b"chiral_batch"), "cuModuleGetFunction")
        inputs = buffer_inputs(tables, batch)
        host_buffers = [(C.c_uint32 * len(inputs[name]))(*inputs[name])
                        for name in ("move_destinations", "flip_thresholds")]
        host_buffers.append((C.c_int32 * len(inputs["initial_states"]))(*inputs["initial_states"]))
        host_buffers.append((C.c_uint32 * len(inputs["tickets"]))(*inputs["tickets"]))
        output = (C.c_int32 * (count * batch.steps * 7))()
        for host in [*host_buffers, output]:
            pointer = C.c_uint64()
            _check(driver, driver.cuMemAlloc_v2(C.byref(pointer), C.sizeof(host)), "cuMemAlloc")
            allocated.append(pointer)
        for host, pointer in zip(host_buffers, allocated):
            _check(driver, driver.cuMemcpyHtoD_v2(pointer, host, C.sizeof(host)), "cuMemcpyHtoD")
        parameters = [*allocated, C.c_uint(count), C.c_uint(batch.steps)]
        arguments = (C.c_void_p * len(parameters))(*[C.cast(C.byref(x), C.c_void_p) for x in parameters])
        _check(driver, driver.cuLaunchKernel(function, (count + 127) // 128, 1, 1,
            128, 1, 1, 0, None, arguments, None), "cuLaunchKernel")
        _check(driver, driver.cuCtxSynchronize(), "cuCtxSynchronize")
        _check(driver, driver.cuMemcpyDtoH_v2(output, allocated[-1], C.sizeof(output)), "cuMemcpyDtoH")
        rows = tuple(tuple(tuple(output[(path * batch.steps + step) * 7 + field] for field in range(7))
                           for step in range(batch.steps)) for path in range(count))
    finally:
        _release_driver_resources(driver, allocated, module, context, sys.exc_info()[1])
    return result_record(tables, batch, rows, "cuda-gpu", {
        "gpu_executed": True, "launch_count": 1, "device": device_info,
        "driver_version": probe["driver_version"],
        "compilation": {key: value for key, value in compiled.items() if key != "ptx"},
        "elapsed_ns": time.perf_counter_ns() - started,
        "timing_scope": "Validation, device probe, NVRTC compilation, context creation, allocation, host-device transfers, kernel, synchronization, transfer back and cleanup; excludes supplied model/ticket preparation, report encoding and full host witness certification."})
