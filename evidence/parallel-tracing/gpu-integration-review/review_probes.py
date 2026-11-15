"""Independent compiler-boundary code/fuel/domain/hardware-gate checks."""
import argparse
from copy import deepcopy
import ctypes as C
import hashlib
import importlib.util
import itertools
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
from tempfile import TemporaryDirectory
from unittest.mock import patch

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--repo',type=Path,required=True)
p.add_argument('--trace',type=Path,required=True)
p.add_argument('--scalar-report',type=Path,required=True)
p.add_argument('--output',type=Path)
a=p.parse_args()
repo=a.repo.resolve();out=a.output or Path(__file__).resolve().parent;out.mkdir(parents=True,exist_ok=True)
sys.path.insert(0,str(repo))
from native import gpu_compiler as gpu
from runtime.dec_language import compile_source,Interpreter,DecError
from simulations import compiled_boundaries as cb


def require(value,message):
 if not value:raise RuntimeError(message)


def hashes():
 paths=('simulations/compiled_boundaries.py','scripts/check_compiled_boundaries.py','tests/test_compiled_boundaries.py',
        'examples/boundary_coefficients.dec','native/gpu_compiler.py','native/batch.py','simulations/trace_parallel.py')
 return {s:hashlib.sha256((repo/s).read_bytes()).hexdigest() for s in paths}


def independent(row):
 mode,left,right,available,kind=row
 if mode:return left-right
 signed=left-right
 selected=bool(available and signed)
 return signed+1+3*selected+6*(left==right==1)+12*(left==right==0)+24*(not available)+48*((kind+1) if selected else 0)


def reference(program,row,fuel,depth=128):
 vm=Interpreter(program,emit=lambda value:None,max_steps=fuel,max_depth=depth)
 try:return (vm.call('coefficient',list(row)),0,0,vm.remaining)
 except DecError as exc:
  message=str(exc);line=re.match(r'line ([0-9]+):',message)
  status=1 if message=='execution step budget exhausted' else 2 if message=='call depth limit exceeded' else 3
  return (0,status,int(line[1]) if line else 0,max(0,vm.remaining))


def run():
 before=hashes();source=cb.SOURCE.read_text();program=compile_source(source)
 table=list(itertools.product(range(2),range(2),range(2),range(2),range(3)))
 expected=[independent(row) for row in table]
 require([reference(program,row,100000)[0] for row in table]==expected,'source differs from independent encoding')
 generated=gpu.lower_cuda(source,'coefficient')['source']
 compiler=shutil.which('g++');require(compiler is not None,'g++ required for generated body probe')
 with TemporaryDirectory() as directory:
  path=Path(directory);cpp=path/'generated.cpp';shared=path/'generated.so'
  cpp.write_text('#define __device__\n#define __global__\nstruct Dim {unsigned int x;}; Dim blockIdx,blockDim,threadIdx;\n'+generated+
   'extern "C" void run(const DecInt* a,DecInt* b,int* c,int* d,DecInt* e,unsigned int n,DecInt f,int depth){blockDim.x=128;for(unsigned int i=0;i<n;++i){blockIdx.x=i/128;threadIdx.x=i%128;dec_gpu_rows(a,b,c,d,e,n,f,depth);}}')
  subprocess.run([compiler,'-std=c++11','-shared','-fPIC','-O2','-ftrapv',str(cpp),'-o',str(shared)],check=True,capture_output=True)
  lib=C.CDLL(str(shared));lib.run.argtypes=[C.POINTER(C.c_int64),C.POINTER(C.c_int64),C.POINTER(C.c_int32),C.POINTER(C.c_int32),C.POINTER(C.c_int64),C.c_uint,C.c_int64,C.c_int32];lib.run.restype=None
  def host(rows,fuel,depth=128):
   inputs=(C.c_int64*(len(rows)*5))(*(x for row in rows for x in row));buffers=((C.c_int64*len(rows))(),(C.c_int32*len(rows))(),(C.c_int32*len(rows))(),(C.c_int64*len(rows))())
   lib.run(inputs,*buffers,len(rows),fuel,depth)
   return [tuple(buffer[i] for buffer in buffers) for i in range(len(rows))]
  require([r[0] for r in host(table,100000)]==expected,'generated code differs from independent encoding')
  comparisons=0
  for row in table:
   needed=100000-reference(program,row,100000)[3]
   for fuel,depth in ((0,0),(1,0),(0,1),(needed-1,1),(needed,1),(needed+1,1)):
    require(host([row],fuel,depth)[0]==reference(program,row,fuel,depth),'fuel/depth source mismatch')
    comparisons+=1
  invalid=[(2,0,0,1,0),(0,2,0,1,0),(0,0,-1,1,0),(0,0,0,2,0),(0,0,0,1,3)]
  for row,actual in zip(invalid,host(invalid,100000)):
   require(actual==reference(program,row,100000),'invalid classifier flag was clipped/admitted')
 for row in ((0,1<<63,0,1,0),(0,-(1<<63)-1,0,1,0),(0,True,0,1,0)):
  with patch.object(gpu.cuda,'probe_gpu',side_effect=RuntimeError('device should not be reached')):
   try:gpu.run_gpu_batch(source,'coefficient',[row],fuel=0)
   except DecError:pass
   else:raise RuntimeError('GPU typed/domain admission failed')
 spec=importlib.util.spec_from_file_location('compiler_driver_probe',repo/'scripts/check_compiled_boundaries.py')
 driver=importlib.util.module_from_spec(spec);spec.loader.exec_module(driver)
 scalar=json.loads(a.scalar_report.read_text())
 def batch(source,function,rows,**kwargs):
  return {'rows':[{'result':str(independent(row)),'stdout':'','error':None,'error_line':0,'exit_code':0} for row in rows],
   'execution':{'backend':'compiled-cpu','native_dispatch_calls':1,'compiled_entries_entered':len(rows),'worker_ids':[1,2]}}
 def views(path,**kwargs):
  report=deepcopy(scalar)
  if kwargs['backend']=='compiled-gpu':
   for job in report['execution']['jobs']:
    job['profile']={'execution':{'backend':'cuda-generated','gpu_executed':False,'launch_count':0}}
  return report
 with TemporaryDirectory() as directory:
  with patch.object(sys,'argv',['check','--trace',str(a.trace),'--output',directory,'--workers','2','--gpu','required']),patch.object(driver,'run_cpu_batch',side_effect=batch),patch.object(gpu,'run_gpu_batch',side_effect=batch),patch.object(driver,'boundary_views',side_effect=views):
   try:driver.main()
   except ValueError as exc:
    reason=str(exc);require('requires generated CUDA launches' in reason,'different driver failure: '+reason)
   else:raise RuntimeError('hardware proof accepted CPU/scalar dispatch as GPU')
 require(hashes()==before,'source changed during integration review')
 (out/'boundary-generated.cu').write_text(generated)
 report={'status':'PASS','mode':'optimized' if sys.flags.optimize else 'normal','source_sha256':before,
  'truth_table_rows':len(table),'fuel_depth_comparisons':comparisons,'invalid_flag_assertions':len(invalid),
  'gpu_domain_and_bool_rejections_before_probe':3,'hardware_gate_rejects_no_device_execution':reason,
  'generated_cuda_sha256':hashlib.sha256(generated.encode()).hexdigest(),
  'scope':'Independent source/generator/fuel/admission/gate checks; generated body executed on host, not a GPU execution claim.'}
 (out/('review-'+report['mode']+'.json')).write_text(json.dumps(report,indent=2,sort_keys=True)+'\n')
 print(json.dumps(report))


if __name__=='__main__':run()
