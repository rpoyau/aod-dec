#!/usr/bin/env python3
"""Read-only function-count probe of the existing engine and conformance adapter.

Profiler-instrumented wall time is diagnostic only, never a speedup benchmark.
"""
from collections import Counter
from contextlib import nullcontext
import hashlib
import json
from pathlib import Path
import sys
import time
ROOT=Path(__file__).resolve().parents[2]/"aod-dec"
sys.path.insert(0,str(ROOT))
from simulations import awareness_jit_fixture as f
from simulations.vendor.aod_trace_digest_cache import trace_digest_cache
from scripts import check_awareness_jit as check
m=f.load_manual(); preparation=f.prepare()
interesting={"transition","validate_state","validate_world","validate_retention","validate_wave",
    "compare","certify","admitted","validate_record_collection","_validate_wave_collection",
    "_check_retained_bindings","trace_digest","digest","wave_prefix","update","kernel","fibre_step",
    "load_manual","response_inputs","apply_compiled","select_entry","validate_inputs","_dispatcher",
    "_load_thunk","run_cpu_batch","_execute","execute_compiled","JitSession","__init__"}
source_paths=("simulations/vendor/aod_awareness.py","simulations/vendor/aod_manual_native.py",
    "simulations/vendor/aod_trace_digest_cache.py","simulations/awareness_jit_fixture.py",
    "scripts/check_awareness_jit.py","native/batch.py","native/cpu_batch.py","native/compiler.py")
before={p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in source_paths}
records=[]
def profile(label, operation, cache=False):
    counts=Counter()
    def record(frame,event,arg):
        path=Path(frame.f_code.co_filename)
        if event=="call" and frame.f_code.co_name in interesting and path.is_relative_to(ROOT):
            counts[str(path.relative_to(ROOT))+":"+str(frame.f_code.co_firstlineno)+":"+frame.f_code.co_name]+=1
    with trace_digest_cache(f.native) if cache else nullcontext(None) as stats:
        started=time.perf_counter_ns();sys.setprofile(record)
        try: value=operation()
        finally: sys.setprofile(None)
        elapsed=time.perf_counter_ns()-started
    item={"case":label,"transitions":12,"profiled_wall_ns":elapsed,"call_counts":dict(sorted(counts.items())),"digest_cache":stats}
    records.append(item)
    return value

def engine(trace):
    return {name:m.execute(state,preparation["world"],preparation["model"],preparation["inputs"],trace)
            for name,state in preparation["lanes"].items()}
plain=profile("existing_engine_trace_off",lambda:engine(False))
traced=profile("existing_engine_passive_trace",lambda:engine(True))
cached=profile("existing_engine_passive_trace_exact_digest_cache",lambda:engine(True),True)
compiled=profile("current_compiled_conformance_adapter",lambda:check.execute_compiled(preparation,workers=2))["lanes"]
for name in plain:
    reference=m.execution_bytes(plain[name])
    if any(m.execution_bytes(other[name])!=reference for other in (traced,cached,compiled)):
        raise ValueError("different actual native evolution: "+name)
after={p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in source_paths}
if before!=after:raise ValueError("profile source changed during execution")
report={"status":"PASS","scope":"Counts on exactly12 transitions, three matched lanes, four inputs; context/reference preparation excluded. Profiled durations include profiling overhead and are not benchmark results.","source_sha256":before,"all_complete_state_and_event_bytes_equal":True,"records":records}
out=Path(__file__).with_name("PROFILE_EXISTING.json");out.write_text(json.dumps(report,indent=2)+"\n")
print(json.dumps({"status":"PASS","output":str(out),"cases":len(records)},indent=2))
