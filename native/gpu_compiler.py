"""Source-derived CUDA target for the declared pure int64/Boolean Dec subset.

The frontend and batch contract admit the source before this target generates
device code. Every executed source expression and statement consumes logical
fuel. Device results are consumed directly; neither an interpreter nor a CPU
arithmetic fallback participates in execution. Arbitrary-precision integers,
ratios and retained-history construction remain outside this target.
"""
from __future__ import annotations

import ctypes as C
import hashlib
import json
from pathlib import Path
import re
import sys
import time

from .batch import BatchError, entry_binding, select_entry, validate_inputs
from simulations import chiral_gpu as cuda


GPUUnavailable = cuda.GPUUnavailable
ROOT = Path(__file__).resolve().parents[1]
TARGET = "dec-cuda-int64-bool-v1"
I64_MIN, I64_MAX = -(1 << 63), (1 << 63)-1
KERNEL_NAME = "dec_gpu_rows"
ERRORS = {1: "execution step budget exhausted", 2: "call depth limit exceeded",
          3: "assertion failed", 4: "GPU int64 target domain exceeded"}


def require(condition, message):
    if not condition:
        raise BatchError(message)


def _sha(value):
    return hashlib.sha256(value).hexdigest()


def _canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def _source_bindings():
    paths = ("native/gpu_compiler.py", "native/batch.py", "runtime/dec_language.py",
             "runtime/dec_core.py", "simulations/chiral_gpu.py")
    return {path: _sha((ROOT/path).read_bytes()) for path in paths}


# No signed overflow is evaluated by these helpers. Unsigned magnitudes admit
# the asymmetric INT64_MIN endpoint without converting +2**63 to signed.
_PRELUDE = r"""
typedef long long DecInt;
typedef unsigned long long DecUInt;
static const DecInt DEC_MAX = 9223372036854775807LL;
static const DecInt DEC_MIN = (-9223372036854775807LL - 1LL);
struct DecContext { DecInt fuel; int error; int line; };
__device__ bool dec_gpu_tick(DecContext* c) {
    if (c->fuel <= 0) { c->error = 1; c->line = 0; return false; }
    --c->fuel; return true;
}
__device__ DecInt dec_gpu_domain(DecContext* c, int line) {
    c->error = 4; c->line = line; return 0;
}
__device__ DecInt dec_gpu_add(DecContext* c, DecInt a, DecInt b, int line) {
    if ((b > 0 && a > DEC_MAX-b) || (b < 0 && a < DEC_MIN-b))
        return dec_gpu_domain(c,line);
    return a+b;
}
__device__ DecInt dec_gpu_sub(DecContext* c, DecInt a, DecInt b, int line) {
    if ((b < 0 && a > DEC_MAX+b) || (b > 0 && a < DEC_MIN+b))
        return dec_gpu_domain(c,line);
    return a-b;
}
__device__ DecUInt dec_gpu_magnitude(DecInt a) {
    return a < 0 ? (DecUInt)(-(a+1))+1ULL : (DecUInt)a;
}
__device__ DecInt dec_gpu_mul(DecContext* c, DecInt a, DecInt b, int line) {
    const bool negative = (a < 0) != (b < 0);
    const DecUInt x = dec_gpu_magnitude(a), y = dec_gpu_magnitude(b);
    const DecUInt limit = negative ? (1ULL << 63) : (DecUInt)DEC_MAX;
    if (y && x > limit/y) return dec_gpu_domain(c,line);
    const DecUInt product = x*y;
    if (!negative) return (DecInt)product;
    if (product == (1ULL << 63)) return DEC_MIN;
    return -(DecInt)product;
}
__device__ DecInt dec_gpu_neg(DecContext* c, DecInt a, int line) {
    if (a == DEC_MIN) return dec_gpu_domain(c,line);
    return -a;
}
"""


class _Lowering:
    """Generate statements, never interpolate user names as C++ identifiers."""
    def __init__(self, entry):
        self.entry = entry
        self.lines, self.count, self.indent = [], 0, 1

    def emit(self, line):
        self.lines.append("    "*self.indent+line)

    def fresh(self):
        self.count += 1
        return "v"+str(self.count)

    def value(self, expression):
        name = self.fresh()
        self.emit("DecInt "+name+" = "+expression+";")
        return name

    def tick(self):
        self.emit("if (!dec_gpu_tick(c)) return 0;")

    def expr(self, expr, env, line):
        self.tick()
        kind = expr["kind"]
        if kind == "name":
            return env[expr["name"]]
        if kind == "literal":
            value = expr["value"]
            if expr["type"] == "bool":
                return self.value("1LL" if value else "0LL")
            require(type(value) is int and I64_MIN <= value <= I64_MAX,
                    "line %d: GPU int64 literal outside target domain" % line)
            return self.value(str(value)+"LL")
        if kind == "unary":
            # The frontend represents the usual spelling of INT64_MIN as
            # unary-minus(positive literal 2**63). Encode that constant safely
            # while retaining both original expression-node fuel ticks.
            child = expr["value"]
            if expr["op"] == "-" and child["kind"] == "literal" and child["type"] == "int" and child["value"] == 1 << 63:
                self.tick()
                return self.value("DEC_MIN")
            operand = self.expr(child, env, line)
            if expr["op"] == "not":
                return self.value("!"+operand)
            if expr["op"] == "+":
                return self.value(operand)
            result = self.value("dec_gpu_neg(c,"+operand+","+str(line)+")")
            self.emit("if (c->error) return 0;")
            return result
        if kind in ("binary", "compare"):
            left = self.expr(expr["left"], env, line)
            right = self.expr(expr["right"], env, line)
            if kind == "compare":
                return self.value(left+" "+expr["op"]+" "+right)
            operation = {"+": "add", "-": "sub", "*": "mul"}[expr["op"]]
            result = self.value("dec_gpu_"+operation+"(c,"+left+","+right+","+str(line)+")")
            self.emit("if (c->error) return 0;")
            return result
        if kind == "boolean":
            result = self.value("1LL" if expr["op"] == "and" else "0LL")
            for child in expr["values"]:
                self.emit("if ("+(result if expr["op"] == "and" else "!"+result)+") {")
                self.indent += 1
                value = self.expr(child, env, line)
                self.emit(result+" = "+value+";")
                self.indent -= 1
                self.emit("}")
            return result
        if kind == "conditional":
            condition = self.expr(expr["condition"], env, line)
            result = self.value("0LL")
            self.emit("if ("+condition+") {")
            self.indent += 1
            yes = self.expr(expr["yes"], env, line)
            self.emit(result+" = "+yes+";")
            self.indent -= 1
            self.emit("} else {")
            self.indent += 1
            no = self.expr(expr["no"], env, line)
            self.emit(result+" = "+no+";")
            self.indent -= 1
            self.emit("}")
            return result
        raise BatchError("unsupported GPU expression "+str(kind))

    def source(self):
        branch = self.entry.branch
        self.lines.append("__device__ DecInt dec_gpu_entry(DecContext* c, const DecInt* args, int max_depth) {")
        self.tick()
        self.emit("if (max_depth < 1) { c->error = 2; c->line = 0; return 0; }")
        env = {parameter["name"]: "args["+str(index)+"]" for index, parameter in enumerate(branch["params"])}
        returned = False
        for statement in branch["body"]:
            self.tick()
            value = self.expr(statement["value"], env, statement["line"])
            kind = statement["kind"]
            if kind == "assign":
                env[statement["name"]] = value
            elif kind == "assert":
                self.emit("if (!"+value+") { c->error = 3; c->line = "+str(statement["line"])+"; return 0; }")
            elif kind == "return":
                self.emit("return "+value+";")
                returned = True
                break
            else:
                raise BatchError("unsupported GPU statement "+str(kind))
        if not returned:
            self.emit("return "+env[branch["output"]]+";")
        self.lines.append("}")
        self.lines.extend((
            'extern "C" __global__ void '+KERNEL_NAME+'(',
            '    const DecInt* inputs, DecInt* values, int* errors, int* lines, DecInt* remaining,',
            '    unsigned int count, DecInt fuel, int max_depth) {',
            '    unsigned int row = blockIdx.x*blockDim.x+threadIdx.x;',
            '    if (row >= count) return;',
            '    DecContext context = {fuel,0,0};',
            '    DecInt value = dec_gpu_entry(&context, inputs+(unsigned long long)row*'+str(len(branch['params']))+', max_depth);',
            '    values[row] = context.error ? 0 : value;',
            '    errors[row] = context.error; lines[row] = context.line; remaining[row] = context.fuel;',
            '}'))
        return '// '+TARGET+'; generated from verified Dec IR\n'+_PRELUDE+'\n'+'\n'.join(self.lines)+'\n'


def _lower_entry(entry):
    binding = entry_binding(entry)
    source = _Lowering(entry).source()
    require(entry_binding(entry) == binding, "GPU entry changed during lowering")
    return {"source": source, "source_binding": {**binding, "target": TARGET,
            "generated_cuda_sha256": _sha(source.encode()), "implementation_sha256": _source_bindings()},
            "result_type": entry.result_type, "param_types": list(entry.param_types)}


def lower_cuda(program, function, *, param_types=None):
    """Return genuine generated CUDA source and provenance; no execution claim."""
    return _lower_entry(select_entry(program, function, param_types=param_types))


def compile_cuda(source, architecture="compute_75", *, opt=0):
    """Compile generated CUDA text to PTX, explicitly separate from execution."""
    require(type(source) is str and bool(source), "nonempty generated CUDA source required")
    require(type(architecture) is str and re.fullmatch(r"compute_[0-9]{2,3}", architecture),
            "explicit CUDA compute architecture required")
    require(type(opt) is int and opt in range(4), "GPU optimization request must be an exact integer in 0..3")
    lib = cuda._load("nvrtc")
    p, i, size = C.c_void_p, C.c_int, C.c_size_t
    for name, arguments in {
        "nvrtcVersion": [C.POINTER(i), C.POINTER(i)],
        "nvrtcCreateProgram": [C.POINTER(p), C.c_char_p, C.c_char_p, i, p, p],
        "nvrtcCompileProgram": [p, i, C.POINTER(C.c_char_p)],
        "nvrtcGetProgramLogSize": [p, C.POINTER(size)], "nvrtcGetProgramLog": [p, p],
        "nvrtcGetPTXSize": [p, C.POINTER(size)], "nvrtcGetPTX": [p, p],
        "nvrtcDestroyProgram": [C.POINTER(p)],
    }.items():
        cuda._function(lib, name, arguments)
    cuda._function(lib, "nvrtcGetErrorString", [i], C.c_char_p)

    def check(code, operation):
        if code:
            raise RuntimeError(operation+": "+lib.nvrtcGetErrorString(code).decode())

    major, minor, program = i(), i(), p()
    check(lib.nvrtcVersion(C.byref(major), C.byref(minor)), "nvrtcVersion")
    raw = source.encode()
    check(lib.nvrtcCreateProgram(C.byref(program), raw, b"dec-generated.cu", 0, None, None), "nvrtcCreateProgram")
    # NVRTC exposes device optimization as a switch rather than LLVM O1/O2/O3.
    # Preserve the request and exact effective flags in the artifact.
    flags = ["--std=c++11", "--gpu-architecture="+architecture,
             "--device-debug" if opt == 0 else "--dopt=on"]
    try:
        options = (C.c_char_p*len(flags))(*(flag.encode() for flag in flags))
        status = lib.nvrtcCompileProgram(program, len(options), options)
        log_size = size()
        check(lib.nvrtcGetProgramLogSize(program, C.byref(log_size)), "nvrtcGetProgramLogSize")
        log = C.create_string_buffer(log_size.value)
        check(lib.nvrtcGetProgramLog(program, log), "nvrtcGetProgramLog")
        if status:
            raise RuntimeError("nvrtcCompileProgram: "+lib.nvrtcGetErrorString(status).decode()+"\n"+log.value.decode())
        ptx_size = size()
        check(lib.nvrtcGetPTXSize(program, C.byref(ptx_size)), "nvrtcGetPTXSize")
        ptx = C.create_string_buffer(ptx_size.value)
        check(lib.nvrtcGetPTX(program, ptx), "nvrtcGetPTX")
        return {"status": "COMPILED", "gpu_executed": False, "target": TARGET,
                "generated_cuda_sha256": _sha(raw), "ptx_sha256": _sha(ptx.raw), "ptx": ptx.raw,
                "architecture": architecture, "nvrtc_version": [major.value, minor.value],
                "requested_opt": opt, "compiler_options": flags, "compiler_log": log.value.decode()}
    finally:
        cuda._finish_cleanup([("nvrtcDestroyProgram", lambda:
            check(lib.nvrtcDestroyProgram(C.byref(program)), "nvrtcDestroyProgram"))], sys.exc_info()[1])


def _execute(source, rows, fuel, max_depth, opt):
    started = time.perf_counter_ns()
    probe = cuda.probe_gpu()
    if probe["status"] != "AVAILABLE":
        raise GPUUnavailable(probe)
    device_info = probe["devices"][0]
    profile = {"gpu_executed": False, "launch_count": 0, "device": device_info,
               "driver_version": probe["driver_version"], "probe_ns": time.perf_counter_ns()-started}
    if not rows:
        return (), {**profile, "status": "NO_WORK"}
    started = time.perf_counter_ns()
    compiled = compile_cuda(source, "compute_"+"".join(map(str, device_info["compute_capability"])), opt=opt)
    profile["compile_ns"] = time.perf_counter_ns()-started
    profile["compilation"] = {key: value for key, value in compiled.items() if key != "ptx"}
    driver, device = cuda._driver(), C.c_int()
    cuda._check(driver, driver.cuDeviceGet(C.byref(device), 0), "cuDeviceGet")
    context, module, function = C.c_void_p(), C.c_void_p(), C.c_void_p()
    allocated, count = [], len(rows)
    flat = tuple(int(value) for row in rows for value in row)
    inputs = (C.c_int64*max(1, len(flat)))(*(flat or (0,)))
    outputs = ((C.c_int64*count)(), (C.c_int32*count)(), (C.c_int32*count)(), (C.c_int64*count)())
    started = time.perf_counter_ns()
    try:
        cuda._check(driver, driver.cuCtxCreate_v2(C.byref(context), 0, device), "cuCtxCreate")
        ptx = C.create_string_buffer(compiled["ptx"])
        cuda._check(driver, driver.cuModuleLoadData(C.byref(module), ptx), "cuModuleLoadData")
        cuda._check(driver, driver.cuModuleGetFunction(C.byref(function), module, KERNEL_NAME.encode()), "cuModuleGetFunction")
        for host in (inputs, *outputs):
            pointer = C.c_uint64()
            cuda._check(driver, driver.cuMemAlloc_v2(C.byref(pointer), C.sizeof(host)), "cuMemAlloc")
            allocated.append(pointer)
        profile["load_allocate_ns"] = time.perf_counter_ns()-started
        started = time.perf_counter_ns()
        cuda._check(driver, driver.cuMemcpyHtoD_v2(allocated[0], inputs, C.sizeof(inputs)), "cuMemcpyHtoD")
        profile["transfer_to_device_ns"] = time.perf_counter_ns()-started
        parameters = [*allocated, C.c_uint(count), C.c_int64(fuel), C.c_int32(max_depth)]
        arguments = (C.c_void_p*len(parameters))(*[C.cast(C.byref(value), C.c_void_p) for value in parameters])
        started = time.perf_counter_ns()
        cuda._check(driver, driver.cuLaunchKernel(function, (count+127)//128, 1, 1, 128, 1, 1,
                    0, None, arguments, None), "cuLaunchKernel")
        cuda._check(driver, driver.cuCtxSynchronize(), "cuCtxSynchronize")
        profile["launch_synchronize_ns"] = time.perf_counter_ns()-started
        started = time.perf_counter_ns()
        for host, pointer in zip(outputs, allocated[1:]):
            cuda._check(driver, driver.cuMemcpyDtoH_v2(host, pointer, C.sizeof(host)), "cuMemcpyDtoH")
        profile["transfer_from_device_ns"] = time.perf_counter_ns()-started
        result = tuple(tuple(buffer[row] for buffer in outputs) for row in range(count))
    finally:
        started = time.perf_counter_ns()
        cuda._release_driver_resources(driver, allocated, module, context, sys.exc_info()[1])
        profile["cleanup_ns"] = time.perf_counter_ns()-started
    profile.update(status="EXECUTED", gpu_executed=True, launch_count=1,
                   host_to_device_bytes=C.sizeof(inputs), device_to_host_bytes=sum(C.sizeof(buffer) for buffer in outputs),
                   grid=[(count+127)//128, 1, 1], block=[128, 1, 1])
    return result, profile


def _materialize(entry, values, *, fuel):
    result = []
    for value, status, line, remaining in values:
        require(type(value) is int and type(status) is int and type(line) is int and type(remaining) is int,
                "GPU returned noninteger row operands")
        require(I64_MIN <= value <= I64_MAX and status in (0, *ERRORS) and 0 <= remaining <= fuel,
                "GPU returned invalid result/status/fuel")
        require((status in (0, 1, 2) and line == 0) or
                (status in (3, 4) and line in {row['line'] for row in entry.branch['body']}),
                "GPU returned invalid source error line")
        require(not status or value == 0, "GPU failed row retained a numeric claim")
        require(status or entry.result_type != "bool" or value in (0, 1), "GPU returned non-Boolean result")
        message = ERRORS.get(status)
        result.append({"result": None if status else ("true" if value else "false") if entry.result_type == "bool" else str(value),
            "stdout": "", "error": None if not status else ("line %d: " % line if line else "")+message,
            "error_line": line, "exit_code": int(bool(status)),
            "failure_kind": "target-domain" if status == 4 else "source-execution" if status else None,
            "remaining_fuel": remaining})
    return result


def run_gpu_batch(program, function, rows, *, fuel=100000, max_depth=128, opt=0, param_types=None):
    """Compile the selected Dec entry and execute independent exact device rows."""
    started = time.perf_counter_ns()
    sources = _source_bindings()
    require(type(opt) is int and opt in range(4), "GPU optimization request must be an exact integer in 0..3")
    entry = select_entry(program, function, param_types=param_types)
    rows = validate_inputs(entry, rows, fuel=fuel, max_depth=max_depth)
    require(all(type(value) is bool or I64_MIN <= value <= I64_MAX for row in rows for value in row),
            "GPU input outside signed int64 target domain")
    lowered = _lower_entry(entry)
    input_hash = _sha(_canonical({"rows": rows, "param_types": entry.param_types, "fuel": fuel, "max_depth": max_depth}))
    preparation_ns = time.perf_counter_ns()-started
    values, execution = _execute(lowered["source"], rows, fuel, max_depth, opt)
    require(len(values) == len(rows), "GPU result row count differs from source batch")
    assembly_start = time.perf_counter_ns()
    result = _materialize(entry, values, fuel=fuel)
    require(_source_bindings() == sources == lowered['source_binding']['implementation_sha256'],
            "GPU compiler sources changed during execution")
    require(entry_binding(entry) == {key: lowered['source_binding'][key] for key in entry_binding(entry)},
            "GPU entry changed during execution")
    if rows:
        require(execution.get('gpu_executed') is True and execution.get('launch_count', 0) > 0,
                "required GPU batch did not execute")
        require(execution['compilation']['generated_cuda_sha256'] == lowered['source_binding']['generated_cuda_sha256'],
                "GPU compiled artifact differs from lowered source")
    source_binding = {**lowered['source_binding'], "input_sha256": input_hash,
                      "result_sha256": _sha(_canonical(result))}
    if rows:
        source_binding['ptx_sha256'] = execution['compilation']['ptx_sha256']
    execution.update(backend="cuda-generated", target=TARGET, preparation_ns=preparation_ns,
        host_assembly_ns=time.perf_counter_ns()-assembly_start, rows=len(rows), requested_opt=opt,
        total_ns=time.perf_counter_ns()-started,
        timing_scope="Input/source validation, CUDA lowering, device probe, NVRTC compilation, module/context/allocation, transfers, launch, synchronization, cleanup, exact result materialization and source/result bindings; excludes final report serialization.")
    return {"rows": result, "execution": execution, "source_binding": source_binding}
