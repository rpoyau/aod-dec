#!/usr/bin/env python3
"""Independent CPU JIT / generated-device-host semantics probe; no GPU claim."""
import ctypes as C
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
ROOT=Path(__file__).resolve().parents[2]/"aod-dec"
sys.path.insert(0,str(ROOT))
from native.cpu_batch import run_cpu_batch
from native.gpu_compiler import lower_cuda
from runtime.dec_language import compile_source,Interpreter
from runtime.dec_core import render

CASES={
 "branch": """Dec chosen(x, gate) -> result:
    (int x, bool gate) -> int result:
        int a = -x if gate else x + 2
        assert a >= -9
        result = a * 3 - 2
Dec main() -> result:
    () -> int result:
        result = 0
""",
 "lazy": """Dec chosen(x, gate) -> result:
    (int x, bool gate) -> bool result:
        bool a = gate or x * x > 100
        result = a and (gate or x + 1 < 20)
Dec main() -> result:
    () -> int result:
        result = 0
"""}
ROWS=((-4,False),(4,True),(20,True),(0,False))
KEYS=("stdout","result","error","error_line","exit_code")
def expected(program,row,fuel,depth):
 vm=Interpreter(program,emit=lambda _:None,max_steps=fuel,max_depth=depth)
 try:
  value=vm.call("chosen",list(row))
  return dict(stdout="",result=render(value),error=None,error_line=0,exit_code=0)
 except ValueError as exc:
  error=str(exc);line=re.match(r"line ([0-9]+):",error)
  return dict(stdout="",result=None,error=error,error_line=int(line[1]) if line else 0,exit_code=1)
def core(row):return {k:row[k] for k in KEYS}
counts={"CPU_rows":0,"generated_device_host_rows":0,"sources":{},"GPU_executed":False}
with tempfile.TemporaryDirectory() as directory:
 for label,source in CASES.items():
  program=compile_source(source);lowered=lower_cuda(source,"chosen")
  counts["sources"][label]={"source":source,"sha256":hashlib.sha256(source.encode()).hexdigest(),"generated_sha256":lowered["source_binding"]["generated_cuda_sha256"]}
  fragment=lowered["source"].split('extern "C" __global__')[0]
  cpp=Path(directory)/(label+".cpp");so=cpp.with_suffix(".so")
  cpp.write_text("#define __device__\n"+fragment+"\nextern \"C\" void invoke(long long* args,long long fuel,int depth,long long* out) { DecContext c={fuel,0,0};out[0]=dec_gpu_entry(&c,args,depth);out[1]=c.error;out[2]=c.line;out[3]=c.fuel;}\n")
  subprocess.run(["g++","-std=c++11","-O2","-ftrapv","-shared","-fPIC",str(cpp),"-o",str(so)],check=True)
  lib=C.CDLL(str(so));lib.invoke.argtypes=[C.POINTER(C.c_int64),C.c_int64,C.c_int,C.POINTER(C.c_int64)]
  for depth in (0,1):
   for fuel in range(0,26):
    cpu=run_cpu_batch(source,"chosen",ROWS,workers=3,fuel=fuel,max_depth=depth)
    for row,actual in zip(ROWS,cpu["rows"]):
     oracle=expected(program,row,fuel,depth)
     if core(actual)!=oracle:raise RuntimeError((label,row,fuel,depth,"CPU",actual,oracle))
     counts["CPU_rows"]+=1
     values=(C.c_int64*2)(*map(int,row));out=(C.c_int64*4)();lib.invoke(values,fuel,depth,out)
     value,status,line,remaining=tuple(out)
     error={1:"execution step budget exhausted",2:"call depth limit exceeded",3:"assertion failed"}.get(status)
     device=dict(stdout="",result=None if status else (("true" if value else "false") if label=="lazy" else str(value)),error=None if not status else (f"line {line}: " if line else "")+str(error),error_line=line,exit_code=int(bool(status)))
     if device!=oracle:raise RuntimeError((label,row,fuel,depth,"device_host",device,oracle))
     counts["generated_device_host_rows"]+=1
counts["status"]="PASS"
counts["scope"]="Independent Interpreter oracle, actual LLVM CPU batch, compiled generated device functions on host; no CUDA device execution in this probe."
files=("native/batch.py","native/cpu_batch.py","native/batch_dispatch.cpp","native/gpu_compiler.py","native/compiler.py","native/runtime.cpp")
counts["source_sha256"]={p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in files}
(Path(__file__).parent/("probe-semantics-optimized.json" if sys.flags.optimize else "probe-semantics-normal.json")).write_text(json.dumps(counts,indent=2)+"\n")
print(json.dumps({k:v for k,v in counts.items() if k not in ("sources","source_sha256")},indent=2))
