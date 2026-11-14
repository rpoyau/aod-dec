#!/usr/bin/env python3
"""Actual reference/AOT/ORC core examples, raw inclusive costs, passive host trace."""
from pathlib import Path
from fractions import Fraction
import argparse
import hashlib
import json
import subprocess
import sys
import time
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from native import compiler
from native.learning import VariantRegistry, RetainedPolicy, semantic_outcome
from simulations.trace_execution import reference, run_core
from simulations.trace_store import exact_json


def run(output, repetitions=5):
    output = Path(output).resolve(); output.mkdir(parents=True, exist_ok=True)
    source_paths=[Path(__file__),ROOT/'native/compiler.py',ROOT/'native/learning.py',ROOT/'runtime/dec_core.py',
                  ROOT/'simulations/trace_execution.py',ROOT/'simulations/trace_store.py',
                  ROOT/'simulations/trace_detectors.py',ROOT/'simulations/trace_views.py']
    source_binding={str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in source_paths}
    rows = []
    for name in ('closure','fusion','coupling'):
        total_start=time.perf_counter_ns()
        source = (ROOT/'examples'/f'{name}.dec').read_text()
        ref = reference(source)
        traced = run_core(source, output/f'{name}-trace', run_id=name)
        start = time.perf_counter_ns()
        executable = output/(name+'-aot')
        build = compiler.build(source, executable, opt=2, cache_dir=output/'cache')
        aot_compile = time.perf_counter_ns()-start
        start = time.perf_counter_ns()
        session = compiler.JitSession(source, opt=2, cache_dir=output/'cache')
        jit_load_compile = time.perf_counter_ns()-start
        try:
            warmup_start = time.perf_counter_ns(); warmup = session.run()
            warmup_ns = time.perf_counter_ns()-warmup_start
            records=[]
            registry = VariantRegistry(hashlib.sha256(source.encode()).hexdigest(), compiler.ABI_VERSION, dict(ref,error_line=0))
            fallback = session.run(guard_key='deliberately-stale')
            objects = [Path(session.info['object'])] + compiler.auxiliary_objects(session.program, session.info)
            artifact_digest = hashlib.sha256(b''.join(p.read_bytes() for p in objects)).hexdigest()
            registry.validate('generic', artifact_digest, fallback)
            registry.validate('optimized', artifact_digest, warmup)
            policy = RetainedPolicy(output/(name+'-policy.json'),
                                    {'source':hashlib.sha256(source.encode()).hexdigest()}, registry)
            for repeat in range(2*repetitions):
                phase='training' if repeat<repetitions else 'heldout'
                decision=policy.choose('active')
                chosen=session.run(guard_key=None if decision['variant']=='optimized' else 'deliberately-stale')
                if any(chosen[k]!=ref[k] for k in ('stdout','result','error','exit_code')):
                    raise RuntimeError('adaptive dispatch changed semantic output')
                start = time.perf_counter_ns()
                aot = subprocess.run([str(executable)], capture_output=True, text=True, timeout=30)
                aot_ns = time.perf_counter_ns()-start
                start = time.perf_counter_ns(); jit=session.run(); jit_ns=time.perf_counter_ns()-start
                start = time.perf_counter_ns(); generic=session.run(guard_key='deliberately-stale'); generic_ns=time.perf_counter_ns()-start
                equal = (aot.returncode == ref['exit_code'] and aot.stdout == ref['stdout']+ref['result']+'\n'
                         and all(jit[k] == ref[k] == generic[k] for k in ('stdout','result','error','exit_code'))
                         and jit['address'] > 0 and generic['address'] > 0)
                if not equal: raise RuntimeError('actual backend mismatch: '+name)
                if phase=='training':policy.observe_pair(generic_ns, jit_ns)
                records.append({'repeat':repeat,'phase':phase,'prior_decision':decision,'chosen_dispatch':chosen['dispatch'],
                                'aot_ns':aot_ns,'jit_ns':jit_ns,'generic_ns':generic_ns,
                                'equal':equal,'jit':jit,'generic':generic,'aot_stdout':aot.stdout})
            heldout=[r for r in records if r['phase']=='heldout']
            adaptation=policy.evaluate_heldout([r['generic_ns'] for r in heldout],
                [r['jit_ns'] for r in heldout],overhead_ns=jit_load_compile+warmup_ns)
            policy.save()
            all_cost = aot_compile+jit_load_compile+warmup_ns+sum(r['aot_ns']+r['jit_ns']+r['generic_ns'] for r in records)
            rows.append({'name':name,'reference':ref,'passive_trace':traced,'build':build,
                         'jit_binding':session.info,'records':records,'aot_compile_ns':aot_compile,
                         'session_construction_ns':jit_load_compile,'first_call_inclusive_ns':warmup_ns,
                         'jit_compile_ns':session.compile_ns,'jit_load_ns':session.load_ns,
                         'warmup_protocol':'one first call includes lazy compile/load; component timings overlap inclusive first call',
                         'artifact_bundle_sha256':artifact_digest,
                         'selected_backend_phase_sum_ns':all_cost,
                         'adaptation':adaptation,
                         'saved_final_policy':policy.choose('active'),
                         'adaptation_scope':'existing guarded compiler policy only; profile never updates scientific kernel'})
        finally:
            session.close()
        rows[-1]['total_inclusive_ns']=time.perf_counter_ns()-total_start
        rows[-1]['total_scope']='source read, reference, trace, builds, warmup, all executions, hashing, policy evaluation/save, teardown'
    if source_binding!={str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in source_paths}:
        raise RuntimeError('source changed during backend campaign; rerun against a frozen candidate')
    report={'status':'PASS','results':rows,'repetitions':repetitions,
            'source_sha256':source_binding,
            'scope':'actual reference/AOT/ORC core execution with host reference commit tracing; no native commit-hook or full native region claim',
            'performance_claim':'raw inclusive costs only; no established cost-inclusive speedup'}
    def encode(value):
        if type(value) is Fraction: return {'$ratio':[value.numerator,value.denominator]}
        raise TypeError(type(value).__name__)
    (output/'results.json').write_text(json.dumps(report,default=encode,indent=2,allow_nan=False)+'\n')
    return report


if __name__ == '__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args(); result=run(args.output)
    print(json.dumps({'status':result['status'],'examples':len(result['results'])}))
