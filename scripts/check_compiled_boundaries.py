#!/usr/bin/env python3
"""Verify source-compiled CPU/CUDA tracing against retained boundary occurrences."""
import argparse
import hashlib
import itertools
import json
from pathlib import Path
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from native.cpu_batch import run_cpu_batch
from runtime.dec_core import render
from runtime.dec_language import Interpreter, compile_source
from simulations.compiled_boundaries import SOURCE, source_bindings
from simulations.trace_execution import boundary_views
from simulations.trace_store import TraceStore
from simulations.chiral_gpu import GPUUnavailable


def require(value, message):
    if not value:
        raise ValueError(message)


def dump(path, value):
    path.write_text(json.dumps(value, sort_keys=True, indent=2, allow_nan=False)+'\n')


def semantic(result):
    return {key:result[key] for key in ('boundaries','archive_coverage','coverage','domain','coverage_scope')}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--trace',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--workers',type=int,default=4)
    p.add_argument('--gpu',choices=('auto','required','off'),default='auto')
    args = p.parse_args()
    require(2 <= args.workers <= 32, 'compiled multicore check requires 2..32 workers')
    args.output.mkdir(parents=True,exist_ok=True)
    bindings = {b:source_bindings(b) for b in ('compiled-cpu','compiled-gpu')}
    source = SOURCE.read_text()
    require(hashlib.sha256(source.encode()).hexdigest() == bindings['compiled-cpu']['examples/boundary_coefficients.dec'],
            'truth-table source differs from bound Dec file')
    program = compile_source(source)
    rows = list(itertools.product(range(2),range(2),range(2),range(2),range(3)))
    expected = []
    for mode,left,right,available,kind in rows:
        orientation = left-right
        selected = int(available == 1 and left != right)
        code = orientation+1+3*selected+6*int(left == right == 1)+12*int(left == right == 0)+24*(1-available)+48*((kind+1) if selected else 0)
        expected.append(str(code if mode == 0 else orientation))
    reference = [render(Interpreter(program).call('coefficient',list(row))) for row in rows]
    require(reference == expected, 'Dec source differs from independent occurrence truth table')
    cpu = run_cpu_batch(source,'coefficient',rows*32,workers=args.workers)
    require(all(row['exit_code'] == 0 for row in cpu['rows']) and
            [row['result'] for row in cpu['rows']] == expected*32,
            'compiled CPU differs from the independent truth table')
    require(cpu['execution']['backend'] == 'compiled-cpu' and
            cpu['execution']['native_dispatch_calls'] == 1 and
            cpu['execution']['compiled_entries_entered'] == len(rows)*32 and
            len(set(cpu['execution']['worker_ids'])) >= 2,
            'compiler CPU check requires observed native threads entering generated code')
    dump(args.output/'cpu-coefficients.json',cpu)
    with TraceStore.open(args.trace) as store:
        before = store.status()
    timings = {}
    reports = {}
    for backend,workers in (('scalar',1),('compiled-cpu',args.workers)):
        started = time.perf_counter()
        reports[backend] = boundary_views(args.trace,backend=backend,workers=workers,coverage_scope='declared-current')
        timings[backend] = time.perf_counter()-started
        dump(args.output/(backend+'.json'),reports[backend])
    expected_views = semantic(reports['scalar'])
    require(expected_views['coverage'] == 'complete' and len(expected_views['boundaries']) == 16,
            'expected complete 16-boundary retained fixture')
    require(semantic(reports['compiled-cpu']) == expected_views,
            'compiled CPU changed retained occurrence/current/sweep/burden evidence')
    gpu = {'status':'NOT_REQUESTED','gpu_executed':False}
    if args.gpu != 'off':
        from native.gpu_compiler import run_gpu_batch
        started = time.perf_counter()
        try:
            device = run_gpu_batch(source,'coefficient',rows)
            reports['compiled-gpu'] = boundary_views(args.trace,backend='compiled-gpu',coverage_scope='declared-current')
        except GPUUnavailable as exc:
            gpu = exc.report
            if args.gpu == 'required':
                raise
        else:
            require(all(row['exit_code'] == 0 for row in device['rows']) and
                    [row['result'] for row in device['rows']] == expected,
                    'generated CUDA differs from independent coefficient truth table')
            require(semantic(reports['compiled-gpu']) == expected_views,
                    'generated CUDA changed retained occurrence/current/sweep/burden evidence')
            executions = [device['execution']] + [job['profile']['execution']
                for job in reports['compiled-gpu']['execution']['jobs']]
            require(len(executions) == 17 and all(e.get('gpu_executed') is True and
                    e.get('launch_count',0) > 0 and e.get('backend') == 'cuda-generated' for e in executions),
                    'compiled GPU check requires generated CUDA launches for coefficients and all trace groups')
            dump(args.output/'gpu-coefficients.json',device)
            dump(args.output/'compiled-gpu.json',reports['compiled-gpu'])
            gpu = {'status':'PASS','execution':device['execution'],'gpu_executed':True}
        timings['compiled-gpu-attempt'] = time.perf_counter()-started
    full = boundary_views(args.trace,backend='compiled-cpu',workers=args.workers,coverage_scope='full')
    unresolved = {key for key,row in full['boundaries'].items() if row['coverage'] != 'complete'}
    require(full['coverage'] == 'partial' and len(unresolved) == 2 and
            all(key.endswith(('/selected','/complement')) for key in unresolved),
            'compiled current replay lost the two unresolved signed projection obligations')
    dump(args.output/'full-current.json',full)
    with TraceStore.open(args.trace) as store:
        after = store.status()
    require((before['events'],before['chain']) == (after['events'],after['chain']),
            'compiled replay mutated its trace archive')
    require(bindings == {b:source_bindings(b) for b in bindings}, 'compiler source changed during validation')
    result = {'status':'PASS','python':sys.version,'source_sha256':bindings,
        'source_dec_sha256':hashlib.sha256(source.encode()).hexdigest(),
        'truth_table_rows':len(rows),'cpu_batch_rows':len(cpu['rows']),
        'cpu_execution':cpu['execution'],'gpu':gpu,'boundary_groups':16,
        'archive_before':before,'archive_after':after,'timing_seconds':timings,
        'full_current_unresolved_groups':sorted(unresolved),
        'scope':'Compiler-owned generated LLVM/native-thread and generated CUDA coefficient execution; '
                'exact host current assembly and joined trace validation. JIT batch targets; no GPU AOT, '
                'generic GPU Dec/GMP, empirical prediction or speedup assertion.'}
    dump(args.output/'results.json',result)
    print(json.dumps({'status':'PASS','boundary_groups':16,'gpu':gpu['status'],'output':str(args.output)}))


if __name__ == '__main__':
    main()
