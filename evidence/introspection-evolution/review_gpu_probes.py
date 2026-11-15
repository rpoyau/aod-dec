"""Independent GPU resource fault matrix using a fake driver; no device claim."""
from contextlib import ExitStack
import hashlib
import json
from pathlib import Path
import sys
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[1]/'aod-dec'
sys.path.insert(0,str(ROOT))
from native import gpu_compiler as gpu
from tests.test_gpu_compiler import SessionDriver,program

probe={'status':'AVAILABLE','driver_version':1,
    'devices':[{'index':0,'name':'review-fake-driver','compute_capability':[8,6]}]}
source=program('result = x')
def compiled(text,architecture,opt=0):
    raw=b'independent fault matrix PTX\0'
    return {'status':'COMPILED','gpu_executed':False,'ptx':raw,
        'generated_cuda_sha256':hashlib.sha256(text.encode()).hexdigest(),
        'ptx_sha256':hashlib.sha256(raw).hexdigest(),'requested_opt':opt,
        'architecture':architecture,'nvrtc_version':[0,0],'compiler_options':[]}

results=[]
for operation in ('device','create','load','function','allocate','to_device','launch','sync','from_device','pop'):
    driver=SessionDriver()
    driver.failures[operation]=1
    with ExitStack() as stack:
        stack.enter_context(patch.object(gpu.cuda,'probe_gpu',return_value=probe))
        stack.enter_context(patch.object(gpu.cuda,'_driver',return_value=driver))
        stack.enter_context(patch.object(gpu.cuda,'_function',side_effect=lambda *args:None))
        stack.enter_context(patch.object(gpu,'compile_cuda',side_effect=compiled))
        session=gpu.GpuBatchSession(source)
        try:
            session.run('chosen',[(3,),(7,)])
        except RuntimeError as exc:
            message=str(exc)
        else:
            raise ValueError('failed '+operation+' published a result')
        session.close()
        if driver.contexts or driver.memory or driver.modules or driver.stack()!=[77]:
            raise ValueError('failed '+operation+' leaked owned resources or caller context')
        results.append({'fault':operation,'error':message,'resources_released':True,'caller_context_restored':True})

# A newly allocated replacement must also be owned if freeing the old buffer
# fails during growth. The next explicit run may recover after clean discard.
driver=SessionDriver()
with ExitStack() as stack:
    stack.enter_context(patch.object(gpu.cuda,'probe_gpu',return_value=probe))
    stack.enter_context(patch.object(gpu.cuda,'_driver',return_value=driver))
    stack.enter_context(patch.object(gpu.cuda,'_function',side_effect=lambda *args:None))
    stack.enter_context(patch.object(gpu,'compile_cuda',side_effect=compiled))
    with gpu.GpuBatchSession(source) as session:
        session.run('chosen',[(1,)])
        driver.failures['free']=1
        try:
            session.run('chosen',[(i,) for i in range(20)])
        except RuntimeError as exc:
            if 'growth' not in str(exc):raise
        else:raise ValueError('failed growth free published result')
        if driver.contexts or driver.memory or driver.modules:
            raise ValueError('failed growth retained an unowned replacement allocation')
        if session.run('chosen',[(23,)])['rows'][0]['result']!='23':
            raise ValueError('clean discard failed to recover')
    if driver.contexts or driver.memory or driver.modules or driver.stack()!=[77]:
        raise ValueError('final cleanup changed caller context')
results.append({'fault':'free-old-buffer-during-growth','resources_released':True,'later_run_recovers':True})
report={'status':'PASS','scope':'Fake-driver failure isolation only; no GPU device execution claim',
    'probes':results,'source_sha256':{name:hashlib.sha256((ROOT/name).read_bytes()).hexdigest()
    for name in ('native/gpu_compiler.py','tests/test_gpu_compiler.py','simulations/chiral_gpu.py')}}
Path(__file__).with_name('review-gpu-probes-'+('optimized' if sys.flags.optimize else 'normal')+'.json').write_text(
    json.dumps(report,indent=2)+'\n')
print(json.dumps({'status':'PASS','checks':len(results),'device_execution':False}))
