"""Paired inclusive CPU entry-session comparison, not an evolution-model benchmark."""
import hashlib
import json
from pathlib import Path
import random
import statistics
import sys
import time

ROOT=Path(__file__).resolve().parents[2]/'aod-dec'
sys.path.insert(0,str(ROOT))
from native.cpu_batch import CpuBatchSession,run_cpu_batch,source_bindings

source=(ROOT/'examples/awareness_response.dec').read_text()
before=source_bindings()
calls=[]
expected=[]
for step in range(8):
    calls.extend((('response',((step,-1,0,1),(-step,2,0,-1),(0,0,1,1))),
                  ('continuation',((-step,),(0,),(step,))),
                  ('reflexive_query',((-step,),(0,),(step,)))))
    expected.extend(([str(step),str(1-step),'2'],
                     ['1' if -step>0 else '2','2','1' if step>0 else '2'],
                     [str(step),'0',str(-step)]))

def execute(reusable):
    results=[]
    start=time.perf_counter_ns()
    if reusable:
        with CpuBatchSession(source,workers=2,opt=0) as session:
            for function,rows in calls:
                results.append(session.run(function,rows))
    else:
        for function,rows in calls:
            results.append(run_cpu_batch(source,function,rows,workers=2,opt=0))
    elapsed=time.perf_counter_ns()-start
    if [[r['result'] for r in result['rows']] for result in results]!=expected:
        raise ValueError('compiled response/continuation/query result differs from exact arithmetic')
    if not all(result['execution']['workers_used']==2 and
               result['execution']['compiled_entries_entered']==3 and
               all(r['exit_code']==0 for r in result['rows']) for result in results):
        raise ValueError('missing actual two-thread generated execution')
    return elapsed,results

# Both variants share existing content-addressed artifacts. Every measured
# sequence includes its own construction, first loads, all calls and close.
warm_one,_=execute(False)
warm_reuse,_=execute(True)
rng=random.Random(231115)
pairs=[]
for pair in range(7):
    order=[False,True]
    rng.shuffle(order)
    values={}
    for reusable in order:
        elapsed,latest=execute(reusable)
        values['reusable' if reusable else 'one_shot']=elapsed
    pairs.append({'order':['reusable' if value else 'one_shot' for value in order],**values,
                  'ratio':values['one_shot']/values['reusable']})
if source_bindings()!=before:
    raise ValueError('implementation changed during timing')
report={'status':'PASS','source_sha256':before,
    'source_dec_sha256':hashlib.sha256(source.encode()).hexdigest(),
    'benchmark_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    'python':sys.version,'workers':2,'rows_per_call':3,'calls_per_sequence':len(calls),
    'pairs':pairs,'median_ratio':statistics.median(p['ratio'] for p in pairs),
    'median_one_shot_ns':statistics.median(p['one_shot'] for p in pairs),
    'median_reusable_ns':statistics.median(p['reusable'] for p in pairs),
    'warmup_ns':{'one_shot':warm_one,'reusable':warm_reuse},
    'timing_scope':'Warm persistent artifact cache; each measured sequence includes owning session construction, '
        'first-use entry/dispatcher loads, all per-call source/IR/input validation, exact packing, native threads, '
        'runtime Context allocation/free, result collection and final JIT close. Report serialization and oracle checks excluded.',
    'limitation':'A call-sequence optimization for the existing three source functions; not full native evolution, '
        'cold artifact compilation, GPU, physical prediction or blanket engine speedup. Shared host load is uncontrolled.'}
Path(__file__).with_name('results.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({key:report[key] for key in ('status','median_ratio','median_one_shot_ns','median_reusable_ns')}))
