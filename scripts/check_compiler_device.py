#!/usr/bin/env python3
"""Run required actual-device compiler, trace and awareness checks in both Python modes."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True,exist_ok=False)
    records = []
    for mode,flags in (('normal',[]),('optimized',['-O'])):
        run = output/mode
        run.mkdir()
        commands = [
            ['-m','unittest','tests.test_gpu_compiler','tests.test_boundary_accelerator','-v'],
            ['scripts/check_parallel_tracing.py','--workers','4','--gpu','required','--output',str(run/'tracing')],
            ['scripts/check_compiled_boundaries.py','--trace',str(run/'tracing/trace'),
             '--workers','4','--gpu','required','--output',str(run/'compiled')],
            ['scripts/check_awareness_jit.py','--workers','2','--gpu','required','--output',str(run/'awareness')],
        ]
        for index,command in enumerate(commands):
            started = time.perf_counter()
            argv = [sys.executable,'-B',*flags,*command]
            with (run/(str(index)+'.log')).open('w') as log:
                status = subprocess.run(argv,cwd=ROOT,stdout=log,stderr=subprocess.STDOUT).returncode
            records.append({'mode':mode,'command':argv,'exit_code':status,
                            'seconds':time.perf_counter()-started,'log':str(run/(str(index)+'.log'))})
            (output/'commands.json').write_text(json.dumps(records,indent=2)+'\n')
            if status:
                raise SystemExit('required device check failed: '+str(run/(str(index)+'.log')))
    from native.gpu_compiler import lower_cuda, compile_cuda
    source = (ROOT/'examples/boundary_coefficients.dec').read_text()
    lowered = lower_cuda(source,'coefficient')
    report = json.loads((output/'normal/compiled/gpu-coefficients.json').read_text())
    compiled = compile_cuda(lowered['source'],report['execution']['compilation']['architecture'],opt=0)
    if (compiled['ptx_sha256'] != report['source_binding']['ptx_sha256'] or
        compiled['generated_cuda_sha256'] != report['source_binding']['generated_cuda_sha256']):
        raise ValueError('retained PTX differs from executed compiler artifact')
    (output/'boundary_coefficients.cu').write_text(lowered['source'])
    (output/'boundary_coefficients.ptx').write_bytes(compiled['ptx'])
    result = {'status':'PASS','commands':records,'generated_cuda_sha256':hashlib.sha256(lowered['source'].encode()).hexdigest(),
              'ptx_sha256':compiled['ptx_sha256'], 'scope':'Required NVIDIA execution; both Python modes; no silent CPU fallback'}
    (output/'results.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({'status':'PASS','output':str(output)}))


if __name__ == '__main__':
    main()
