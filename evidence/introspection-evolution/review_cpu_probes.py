"""Independent reusable CPU lifetime/failure probes; no production edits."""
import hashlib
import json
from pathlib import Path
import sys
import threading

ROOT=Path(__file__).resolve().parents[1]/'aod-dec'
sys.path.insert(0,str(ROOT))
from native.cpu_batch import CpuBatchSession
from runtime.dec_language import DecError

source='''Dec chosen(x) -> r:
    (int x) -> int r:
        r = x * x
Dec main() -> r:
    () -> int r:
        r = 0
'''
evidence={}
with CpuBatchSession(source,workers=2) as session:
    session.run('chosen',[(2,),(3,)])
    library,binding=session._dispatcher
    freed=[]
    class CollectionFailure:
        def __getattr__(self,name):
            return getattr(library,name)
        def dec_batch_get(self,*args):
            raise ValueError('injected collection failure')
        def dec_batch_free(self,owner):
            freed.append(owner)
            return library.dec_batch_free(owner)
    session._dispatcher=(CollectionFailure(),binding)
    try:
        session.run('chosen',[(5,),(7,)])
    except ValueError as exc:
        if str(exc)!='injected collection failure':
            raise
    else:
        raise ValueError('injected collection failure was swallowed')
    if len(freed)!=1 or not freed[0]:
        raise ValueError('native result owner was not freed exactly once')
    session._dispatcher=(library,binding)
    recovered=session.run('chosen',[(11,),(13,)])
    if [r['result'] for r in recovered['rows']]!=['121','169']:
        raise ValueError('collection failure affected later contexts')
    evidence['collection_failure']={'native_owner_frees':len(freed),'recovered_rows':recovered['rows']}
    key=next(iter(session._loaded))
    address,thunk=session._loaded[key]
    session._loaded[key]=(address+1,thunk)
    try:
        session.run('chosen',[(1,)])
    except DecError as exc:
        if 'not owned' not in str(exc):
            raise
        evidence['wrong_entry_address']='rejected before native invocation'
    else:
        raise ValueError('unowned cached executable address admitted')
    session._loaded[key]=(address,thunk)
    outcomes=[]
    def migrated_call():
        outcomes.append(session.run('chosen',[(17,),(19,)])['rows'])
    thread=threading.Thread(target=migrated_call)
    thread.start();thread.join(10)
    if thread.is_alive() or not outcomes or [r['result'] for r in outcomes[0]]!=['289','361']:
        raise ValueError('reused CPU owner failed after host thread migration')
    evidence['thread_migration']='exact output and joined native lane contexts'
evidence['source_sha256']={name:hashlib.sha256((ROOT/name).read_bytes()).hexdigest()
    for name in ('native/cpu_batch.py','native/batch_dispatch.cpp','tests/test_cpu_batch.py')}
evidence['status']='PASS'
Path(__file__).with_name('review-cpu-probes-'+('optimized' if sys.flags.optimize else 'normal')+'.json').write_text(
    json.dumps(evidence,indent=2)+'\n')
print(json.dumps({'status':'PASS','checks':3}))
