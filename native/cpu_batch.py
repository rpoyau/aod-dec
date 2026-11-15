"""Compiler-owned native-thread batches entering real guarded LLVM functions."""
import ctypes as C
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import time

from . import compiler
from .batch import BatchError, entry_binding, require, select_entry, validate_inputs


ROOT = Path(__file__).resolve().parents[1]
MAX_OUTPUT_BYTES = 32*1024*1024


def source_bindings():
    paths = ("native/batch.py", "native/cpu_batch.py", "native/batch_dispatch.cpp",
             "native/compiler.py", "native/runtime.cpp", "native/runtime.hpp",
             "runtime/dec_language.py", "runtime/dec_core.py")
    return {path: hashlib.sha256((ROOT/path).read_bytes()).hexdigest() for path in paths}


class _Argument(C.Structure):
    _fields_ = [("text", C.c_char_p), ("size", C.c_uint64), ("kind", C.c_int32)]


class _Row(C.Structure):
    _fields_ = [("result", C.c_void_p), ("result_size", C.c_uint64),
                ("output", C.c_void_p), ("output_size", C.c_uint64),
                ("error", C.c_void_p), ("error_size", C.c_uint64),
                ("error_line", C.c_int32), ("exit_code", C.c_int32),
                ("worker_id", C.c_uint64), ("dispatch", C.c_int32), ("entered", C.c_int32)]


def _dispatcher():
    runtime, _ = compiler.ensure_runtime()
    cxx = Path(shutil.which("g++") or "g++").resolve()
    binding = {"source_sha256": source_bindings(), "runtime_library_sha256": compiler.file_digest(runtime),
               "cxx": str(cxx), "cxx_sha256": compiler.file_digest(cxx),
               "flags": ["-std=c++17", "-O2", "-fPIC", "-shared", "-pthread"]}
    key = hashlib.sha256(json.dumps(binding, sort_keys=True).encode()).hexdigest()
    directory = ROOT/"build/native/batch"
    directory.mkdir(parents=True, exist_ok=True)
    library, manifest = directory/(key+".so"), directory/(key+".json")
    with compiler.cache_lock(directory):
        valid = False
        if library.is_file() and manifest.is_file():
            try:
                valid = json.loads(manifest.read_text()) == {"binding": binding, "library_sha256": compiler.file_digest(library)}
            except (OSError, ValueError):
                pass
        if not valid:
            with compiler.staged_path(library) as temporary:
                subprocess.run([str(cxx), *binding["flags"], str(ROOT/"native/batch_dispatch.cpp"),
                                str(runtime), "-o", str(temporary)], check=True, capture_output=True)
            compiler.atomic_text(manifest, json.dumps({"binding": binding,
                "library_sha256": compiler.file_digest(library)}, sort_keys=True)+"\n")
    loaded = C.CDLL(str(library))
    loaded.dec_batch_execute.argtypes = [C.c_void_p, C.c_uint64, C.POINTER(_Argument),
        C.c_uint64, C.c_uint32, C.c_uint32, C.c_int64, C.c_int32, C.c_uint64]
    loaded.dec_batch_execute.restype = C.c_void_p
    loaded.dec_batch_error.argtypes = [C.c_void_p]
    loaded.dec_batch_error.restype = C.c_char_p
    loaded.dec_batch_get.argtypes = [C.c_void_p, C.c_uint64, C.POINTER(_Row)]
    loaded.dec_batch_get.restype = C.c_int32
    loaded.dec_batch_free.argtypes = [C.c_void_p]
    loaded.dec_batch_free.restype = None
    return loaded, {"path": str(library), "sha256": compiler.file_digest(library), "binding": binding}


def _thunk_ir(entry, symbol):
    target = compiler.mangle(entry.function, entry.param_types, "guard")
    declared = "".join(", ptr" for _ in entry.param_types)
    lines = [f"declare ptr @{target}(ptr{declared}, i64, ptr)",
             f"define ptr @{symbol}(ptr %ctx, ptr %args, i64 %identity, ptr %dispatch) {{", "entry:"]
    for index in range(len(entry.param_types)):
        lines.extend((f"  %p{index} = getelementptr ptr, ptr %args, i64 {index}",
                      f"  %v{index} = load ptr, ptr %p{index}"))
    actual = "".join(f", ptr %v{index}" for index in range(len(entry.param_types)))
    lines.extend((f"  %result = call ptr @{target}(ptr %ctx{actual}, i64 %identity, ptr %dispatch)",
                  "  ret ptr %result", "}"))
    return "\n".join(lines)+"\n"


def _load_thunk(session, entry):
    """Called only while owning the session's lifetime lock."""
    session._load()
    symbol = "batch_entry_"+entry.entry_sha256[:24]
    ir = _thunk_ir(entry, symbol)
    binding = {"program_key": session.info["key"], "entry": entry_binding(entry),
               "ir_sha256": hashlib.sha256(ir.encode()).hexdigest(), "sources": source_bindings()}
    key = hashlib.sha256(json.dumps(binding, sort_keys=True).encode()).hexdigest()
    directory = ROOT/"build/native/batch"
    directory.mkdir(parents=True, exist_ok=True)
    obj, manifest = directory/(key+".o"), directory/(key+".object.json")
    with compiler.cache_lock(directory):
        valid = False
        if obj.is_file() and manifest.is_file():
            try:
                valid = json.loads(manifest.read_text()) == {"binding": binding, "object_sha256": compiler.file_digest(obj)}
            except (OSError, ValueError):
                pass
        if not valid:
            with compiler.staged_path(obj) as temporary:
                with compiler._LLVM_LOCK:
                    compiler.llvm().emit(ir, temporary, 0)
            compiler.atomic_text(manifest, json.dumps({"binding": binding, "object_sha256": compiler.file_digest(obj)}, sort_keys=True)+"\n")
            compiler.atomic_text(obj.with_suffix(".ll"), ir)
    if symbol not in session.addresses:
        llvm = compiler.llvm()
        raw = obj.read_bytes()
        buffer = llvm.LLVMCreateMemoryBufferWithMemoryRangeCopy(raw, len(raw), b"batch-entry.o")
        dylib = llvm.LLVMOrcLLJITGetMainJITDylib(session.jit)
        llvm.error(llvm.LLVMOrcLLJITAddObjectFile(session.jit, dylib, buffer))
        address = compiler.U64()
        llvm.error(llvm.LLVMOrcLLJITLookup(session.jit, C.byref(address), symbol.encode()))
        session.addresses[symbol] = address.value
    return session.addresses[symbol], {"symbol": symbol, "object": str(obj),
        "object_sha256": compiler.file_digest(obj), "ir_sha256": binding["ir_sha256"]}


def _execute(session, entry, inputs, *, workers, fuel, max_depth, expected_sources=None, prepared=None):
    """Pack/collect only in Python; the single native dispatch computes all rows."""
    with session.lock:
        require(not session.closed, "JIT session is closed")
        entry_binding(entry)
        require(session.program == entry.program, "batch session belongs to another verified program")
        if prepared is None:
            address, thunk = _load_thunk(session, entry)
            library, dispatch_binding = _dispatcher()
        else:
            address, thunk, library, dispatch_binding = prepared
            require(session.addresses.get(thunk["symbol"]) == address,
                    "prepared batch entry is not owned by this live JIT session")
        require(expected_sources is None or source_bindings() == expected_sources,
                "compiler batch source changed before native dispatch")
        require(session.info["binding"]["runtime_sha256"] == compiler.runtime_digest(),
                "native batch runtime changed during code loading")
        encoded = [str(value).encode() if type(value) is int else (b"true" if value else b"false")
                   for row in inputs for value in row]
        arguments = (_Argument*len(encoded))()
        for index, raw in enumerate(encoded):
            kind = entry.param_types[index % len(entry.param_types)]
            arguments[index] = _Argument(raw, len(raw), 0 if kind == "int" else 1)
        started = time.perf_counter_ns()
        owner = library.dec_batch_execute(address, int(session.info["key"][:16], 16), arguments,
            len(inputs), len(entry.param_types), workers, fuel, max_depth, MAX_OUTPUT_BYTES)
        native_ns = time.perf_counter_ns()-started
        try:
            message = library.dec_batch_error(owner).decode()
            require(not message, message)
            results, worker_ids, entered, dispatches = [], set(), 0, {"specialized": 0, "generic": 0, "not_entered": 0}
            for index in range(len(inputs)):
                row = _Row()
                require(library.dec_batch_get(owner, index, C.byref(row)) == 1, "native batch result row unavailable")
                text = lambda pointer, size: C.string_at(pointer, size).decode() if size else ""
                results.append({"result": None if row.exit_code else text(row.result, row.result_size),
                    "stdout": text(row.output, row.output_size),
                    "error": text(row.error, row.error_size) if row.exit_code else None,
                    "error_line": row.error_line, "exit_code": row.exit_code})
                if row.worker_id:
                    worker_ids.add(row.worker_id)
                entered += row.entered
                dispatches["specialized" if row.dispatch == 1 else "generic" if row.dispatch == 0 else "not_entered"] += 1
            require(entered == len(inputs), "native batch skipped a generated entry")
            return results, {"backend": "compiled-cpu", "native_dispatch_calls": 1,
                "workers_requested": workers, "worker_ids": sorted(worker_ids), "workers_used": len(worker_ids),
                "compiled_entries_entered": entered, "dispatches": dispatches,
                "entry_address": address, "native_dispatch_ns": native_ns,
                "fuel_per_lane": fuel, "max_depth_per_lane": max_depth,
                "status": "EXECUTED" if inputs else "NO_WORK"}, {"compiler": session.info["binding"],
                    "thunk": thunk, "dispatcher": dispatch_binding}
        finally:
            library.dec_batch_free(owner)


class CpuBatchSession:
    """Own one verified program's reusable generated CPU entries.

    Entry selection, thunk loading and dispatcher loading are retained. Every
    run still validates exact inputs and source bindings, and its native lanes
    allocate fresh Contexts. Calls and close share the JIT lifetime lock. Use a
    context manager so the executable code is released after all calls finish.
    """
    def __init__(self, program, *, workers=1, opt=0):
        require(type(workers) is int and 1 <= workers <= 32, "workers must be an exact integer in 1..32")
        require(type(opt) is int and opt in (0, 1, 2, 3), "optimization level must be an exact integer in 0..3")
        self._sources = source_bindings()
        self._session = compiler.JitSession(program, opt=opt)
        self._program_digest = hashlib.sha256(compiler.dumps_ir(self._session.program).encode()).hexdigest()
        self._workers = workers
        self._selected, self._loaded = {}, {}
        self._dispatcher = None
        self._calls = 0
        if source_bindings() != self._sources:
            self.close()
            raise BatchError("compiler batch source changed during session creation")

    @property
    def closed(self):
        return self._session.closed

    def run(self, function, rows, *, fuel=100000, max_depth=128, param_types=None):
        """Execute one ordered batch; return the same public outcome contract."""
        started = time.perf_counter_ns()
        session = self._session
        with session.lock:
            require(not session.closed, "JIT session is closed")
            require(source_bindings() == self._sources, "compiler batch source changed during reusable session")
            require(hashlib.sha256(compiler.dumps_ir(session.program).encode()).hexdigest() == self._program_digest,
                    "reusable batch program changed after admission")
            require(type(function) is str and function, "exact entry function name required")
            require(param_types is None or (type(param_types) in (tuple, list) and
                    all(type(value) is str for value in param_types)),
                    "explicit exact parameter type sequence required")
            key = (function, None if param_types is None else tuple(param_types))
            if key not in self._selected:
                self._selected[key] = select_entry(session.program, function, param_types=key[1])
            entry = self._selected[key]
            inputs = validate_inputs(entry, rows, fuel=fuel, max_depth=max_depth)
            selected = entry_binding(entry)
            entry_hit = entry.entry_sha256 in self._loaded
            if not entry_hit:
                self._loaded[entry.entry_sha256] = _load_thunk(session, entry)
            dispatcher_hit = self._dispatcher is not None
            if not dispatcher_hit:
                self._dispatcher = _dispatcher()
            address, thunk = self._loaded[entry.entry_sha256]
            library, dispatch_binding = self._dispatcher
            results, execution, artifacts = _execute(session, entry, inputs,
                workers=self._workers, fuel=fuel, max_depth=max_depth, expected_sources=self._sources,
                prepared=(address, thunk, library, dispatch_binding))
            require(source_bindings() == self._sources and entry_binding(entry) == selected and
                    hashlib.sha256(compiler.dumps_ir(session.program).encode()).hexdigest() == self._program_digest,
                    "compiler batch source or program changed during reusable execution")
            self._calls += 1
            execution.update(reusable_session=True, session_call=self._calls,
                loaded_entry_count=len(self._loaded), entry_load_cache_hit=entry_hit,
                dispatcher_load_cache_hit=dispatcher_hit, total_ns=time.perf_counter_ns()-started,
                timing_scope="This call: source/IR/input validation, first-use compile/load when needed, packing, "
                    "native-thread dispatch, exact runtime, collection and source checks; session construction "
                    "and final close are separate and must be included in lifecycle benchmarks")
            # Reports must not expose mutable references into the owning cache.
            return {"rows": results, "execution": execution, "source_binding": deepcopy({"entry": selected,
                "implementation_sha256": self._sources, **artifacts})}

    def close(self):
        with self._session.lock:
            self._session.close()
            self._selected.clear()
            self._loaded.clear()
            self._dispatcher = None

    def __enter__(self):
        require(not self.closed, "JIT session is closed")
        return self

    def __exit__(self, *args):
        self.close()


def run_cpu_batch(program, function, rows, *, workers=1, fuel=100000, max_depth=128, opt=0, param_types=None):
    """Compile one selected pure entry and execute ordered independent native lanes.

    Output/error ordering is input ordering. Each lane owns a native runtime
    Context; no CPU thread shares fuel, output, arena handles or mutable values.
    Source and implementation mutations invalidate the entire batch report.
    """
    started = time.perf_counter_ns()
    require(type(workers) is int and 1 <= workers <= 32, "workers must be an exact integer in 1..32")
    require(type(opt) is int and opt in (0, 1, 2, 3), "optimization level must be an exact integer in 0..3")
    binding = source_bindings()
    entry = select_entry(program, function, param_types=param_types)
    inputs = validate_inputs(entry, rows, fuel=fuel, max_depth=max_depth)
    selected = entry_binding(entry)
    with compiler.JitSession(entry.program, opt=opt) as session:
        results, execution, artifacts = _execute(session, entry, inputs,
            workers=workers, fuel=fuel, max_depth=max_depth, expected_sources=binding)
    require(source_bindings() == binding and entry_binding(entry) == selected,
            "compiler batch source changed during execution")
    execution["total_ns"] = time.perf_counter_ns()-started
    execution["timing_scope"] = "eligibility, packing, compile/cache/load, native-thread dispatch, exact runtime, collection and source checks"
    return {"rows": results, "execution": execution,
            "source_binding": {"entry": selected, "implementation_sha256": binding, **artifacts}}
