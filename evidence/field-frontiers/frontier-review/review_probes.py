"""Independent review probes; explicit checks survive Python -O."""
from dataclasses import asdict, replace
from fractions import Fraction as Q
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest.mock import patch
import argparse, hashlib, json, random, sys
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--repo',type=Path)
parser.add_argument('--output',type=Path)
args=parser.parse_args()
ROOT=Path(__file__).resolve().parents[2]
candidates=(ROOT/'aod-dec', *Path(__file__).resolve().parents, Path.cwd())
REPO=args.repo or next((p for p in candidates if (p/'simulations/field_frontier.py').is_file()),None)
if REPO is None:raise SystemExit('Provide --repo PATH_TO_AOD_DEC')
OUTPUT=args.output or Path(__file__).parent
OUTPUT.mkdir(parents=True,exist_ok=True)
sys.path.insert(0,str(REPO))
from simulations import field_frontier as f
from simulations.vendor import aod_manual_native as n


def check(condition, message):
    if not condition: raise RuntimeError(message)


def reject(call):
    try: call()
    except (ValueError,OSError) as exc: return {'rejected':True,'type':type(exc).__name__,'message':str(exc)}
    return {'rejected':False}


def malformed_wave(flag=None,certificate_scale=None):
    records=[]
    for k in range(2):
        t=n.trace('typed-probe',k)
        if flag is not None:t=replace(t,reflected_comparison=flag)
        c=n.certify(t)
        if certificate_scale is not None:c=replace(c,scale=certificate_scale)
        records.append((t,c,{c.certificate_digest:c}))
    return n.wave(records)


def admission_types():
    results={}
    for label,w in [('integer_reflection_flag',malformed_wave(flag=1)),
                    ('boolean_certificate_scale',malformed_wave(certificate_scale=False))]:
        with TemporaryDirectory() as directory:
            archive=f.WitnessArchive(directory)
            front=f.FieldFrontier(archive)
            results[label]=reject(lambda:front.admit(w,window=(0,100),coupling_id='fixed'))
            check(results[label]['rejected'],'invalid witness admitted: '+label)
            check(not tuple(Path(directory).glob('*.json')),'invalid witness persisted')
            archive.close()
    return results


def write_failure():
    first=n.recurrent('same-lineage',n=2)
    second=n.wave([n.certified(n.trace('same-lineage',k,dwell=1)) for k in range(2)])
    with TemporaryDirectory() as directory:
        archive=f.WitnessArchive(directory);front=f.FieldFrontier(archive)
        original=archive._write_validated
        def fail(wave):original(wave);raise OSError('injected post-write failure')
        with patch.object(archive,'_write_validated',fail):
            failure=reject(lambda:front.admit(first,window=(0,100),coupling_id='fixed'))
        check(failure['rejected'],'injected failure absent')
        result=reject(lambda:front.admit(second,window=(0,100),coupling_id='fixed'))
        check(result['rejected'],'conflict admitted after post-write failure')
        archive.close()
        archive=f.WitnessArchive(directory);front=f.FieldFrontier(archive)
        rebound=reject(lambda:front.admit(second,window=(0,100),coupling_id='fixed'))
        check(rebound['rejected'],'reopened union lost retained event conflict')
        archive.close()
        return {'live_rejection':result,'reopened_rejection':rebound}


def archive_bound_and_nested_union():
    with TemporaryDirectory() as directory:
        cap=8192;archive=f.WitnessArchive(directory,max_witness_bytes=cap)
        wave=n.recurrent('bounded-duplicate',n=2)
        body={'schema':'aod-native-wave-v1','producer_sha256':archive.producer_sha256,'wave':asdict(wave)}
        raw=json.dumps(body,sort_keys=True,separators=(',',':'),allow_nan=False).encode()
        check(len(raw)<cap,'fixture larger than declared cap')
        target=Path(directory)/(hashlib.sha256(raw).hexdigest()+'.json')
        target.write_bytes(raw+b' '*(cap*2))
        requests=[];original_open=Path.open
        class Tracked:
            def __init__(self,stream):self.stream=stream
            def __enter__(self):return self
            def __exit__(self,*details):self.stream.close()
            def read(self,size=-1):requests.append(size);return self.stream.read(size)
        def tracked(path,*arguments,**options):
            stream=original_open(path,*arguments,**options)
            return Tracked(stream) if path==target else stream
        with patch.object(Path,'open',tracked):
            refused=reject(lambda:f.FieldFrontier(archive).admit(wave,window=(0,100),coupling_id='fixed'))
        check(refused['rejected'] and requests and all(0<=size<=cap+1 for size in requests),
              'existing witness read bypassed byte cap')
        archive.close()
    def outer(inner,lineage):
        return n.wave([n.certified(n.outer_trace((inner,),lineage=lineage,occurrence=k),'outer',inner=(inner,)) for k in range(2)])
    first=n.recurrent('shared-nested',n=2)
    second=n.wave([n.certified(n.trace('shared-nested',k,dwell=1)) for k in range(2)])
    first,second=outer(first,'outer-first'),outer(second,'outer-second')
    with TemporaryDirectory() as directory:
        archive=f.WitnessArchive(directory);front=f.FieldFrontier(archive)
        summary=front.admit(first,window=(0,1000),coupling_id='fixed')
        check(front.expose(summary)==first,'nested exposure differs')
        front.remove(summary)
        nested=reject(lambda:front.admit(second,window=(0,1000),coupling_id='fixed'))
        check(nested['rejected'],'removed field lost nested union bindings')
        archive.close();archive=f.WitnessArchive(directory)
        nested_reopen=reject(lambda:f.FieldFrontier(archive).admit(second,window=(0,1000),coupling_id='fixed'))
        check(nested_reopen['rejected'],'reopening lost nested union bindings')
        archive.close()
    return {'duplicate_read_requests':requests,'byte_cap':cap,'nested_conflict':nested,'nested_reopened_conflict':nested_reopen}


def queue():
    rng=random.Random(176503);q=f.IndexedFieldQueue(max_fields=100)
    active=set();pending={}
    for i in range(20000):
        key='field:'+str(rng.randrange(100));op=rng.randrange(5)
        if op==0 and key not in active:q.register(key);active.add(key)
        elif op==1 and key in active:
            cut=rng.randrange(10000);q.schedule(key,cut);pending[key]=cut
        elif op==2:
            actual=q.pop();expected=min(((cut,key) for key,cut in pending.items()),default=None)
            check((None if actual is None else (actual.cut,actual.identity))==expected,'queue pop differs')
            if expected:del pending[expected[1]]
        elif op==3:
            check(q.cancel(key)==(key in pending),'cancel differs');pending.pop(key,None)
        elif op==4:q.remove(key);active.discard(key);pending.pop(key,None)
        check(len(q)==len(pending) and q.active_count==len(active),'queue sizes differ')
        check(len(q._heap)==len(q._positions)==len(pending),'stale heap entry')
        check({key:cut for cut,key in q._heap}==pending,'heap contents differ')
        for index,entry in enumerate(q._heap):
            check(q._positions[entry[1]]==index,'index differs')
            if index:check(q._heap[(index-1)//2]<=entry,'heap order differs')
    return {'operations':20000,'result':'PASS'}


def hot_response():
    with TemporaryDirectory() as directory:
        archive=f.WitnessArchive(directory);front=f.FieldFrontier(archive)
        wave=n.recurrent('hot',n=3);summary=front.admit(wave,window=(0,100),coupling_id='fixed',pressure=Q(7,3),current=Q(2,5))
        original=summary;expected=Q(2,5)
        with patch.object(archive,'expose',side_effect=RuntimeError('hot archive access')), \
             patch.object(n,'validate_wave',side_effect=RuntimeError('hot history validation')), \
             patch.object(Path,'open',side_effect=RuntimeError('hot file access')):
            for attention in (Q(1,3),Q(-2,7),Q(5,11)):
                bound=front.bind_attention(summary,attention,source_sha256='1'*64)
                forged=reject(lambda:front.advance(summary,replace(bound,value=attention+1),window=summary.window,coupling_id='fixed',scale=summary.scale))
                check(forged['rejected'],'forged attention accepted')
                changed=reject(lambda:front.advance(summary,bound,window=(0,101),coupling_id='fixed',scale=summary.scale))
                check(changed['rejected'],'changed window silently accepted')
                summary=front.advance(summary,bound,window=summary.window,coupling_id='fixed',scale=summary.scale)
                expected+=attention/Q(7,3)
                check(summary.current==expected,'native response differs from independent rational answer')
        check(summary.committed==original.committed+3,'commit count differs')
        for name in ('closure_cycles','reclosure_beats','latest_return_cut','latest_cadence','provenance'):
            check(getattr(summary,name)==getattr(original,name),'response invented history: '+name)
        check(front.expose(summary)==wave,'formation recovery differs')
        archive.close()
        return {'exact_current':str(expected),'no_hot_archive_access':True,'unchanged_formation':True}


def rebind_recovery():
    with TemporaryDirectory() as directory:
        archive=f.WitnessArchive(directory);front=f.FieldFrontier(archive)
        wave=n.recurrent('rebind',n=3)
        state=front.admit(wave,window=(0,100),coupling_id='first',pressure=Q(7,3),current=Q(2,5))
        for value in (Q(1,3),Q(-2,7)):
            attention=front.bind_attention(state,value,source_sha256='1'*64)
            state=front.advance(state,attention,window=state.window,coupling_id=state.coupling_id,scale=state.scale)
        check(state.committed==24 and state.current==Q(103,245),'independent pre-rebind answer differs')
        previous=state;attention=front.bind_attention(state,Q(1),source_sha256='1'*64)
        registration=dict(front._active);inputs=dict(front._inputs)
        with patch.object(archive,'expose',side_effect=OSError('injected exposure failure')):
            failed=reject(lambda:front.rebind(state,window=(24,50),coupling_id='second'))
        check(failed['rejected'] and front._active==registration and front._inputs==inputs,'failed exposure changed state')
        excluded=reject(lambda:front.rebind(state,window=(25,50),coupling_id='second'))
        check(excluded['rejected'] and front._active==registration,'excluded-cut rebind changed state')
        reads=archive.metrics['archive_reads']
        state=front.rebind(state,window=(24,50),coupling_id='second')
        check(archive.metrics['archive_reads']==reads+1,'rebind did not expose exactly once')
        for name in ('identity','current','pressure','committed','field_weight','closure_cycles','reclosure_beats','latest_return_cut','latest_cadence','scale','scope','pending_obligations','provenance'):
            check(getattr(state,name)==getattr(previous,name),'rebind changed preserved '+name)
        check(state.generation==previous.generation+1,'rebind did not change generation')
        old_attention=reject(lambda:front.advance(state,attention,window=state.window,coupling_id=state.coupling_id,scale=state.scale))
        stale=reject(lambda:front.bind_attention(previous,Q(1),source_sha256='1'*64))
        check(old_attention['rejected'] and stale['rejected'],'old binding survived rebind')
        attention=front.bind_attention(state,Q(2,13),source_sha256='1'*64)
        state=front.advance(state,attention,window=state.window,coupling_id=state.coupling_id,scale=state.scale)
        check(state.committed==25 and state.current==Q(103,245)+Q(6,91),'rebound response differs')
        pending=front.admit(n.recurrent('pending'),window=(0,100),coupling_id='first',pending_obligations=('unresolved',))
        pending=front.rebind(pending,window=(0,200),coupling_id='second')
        check(pending.pending_obligations==('unresolved',),'rebind cleared pending')
        attention=front.bind_attention(pending,Q(1),source_sha256='1'*64)
        held=reject(lambda:front.advance(pending,attention,window=pending.window,coupling_id=pending.coupling_id,scale=pending.scale))
        check(held['rejected'],'pending obligation allowed hot advance')
        archive.close()
        return {'advanced_committed_cut':state.committed,'current':str(state.current),'old_attention_rejected':True,'failed_exposure_atomic':True,'pending_preserved':True}


if __name__=='__main__':
    before=hashlib.sha256(Path(f.__file__).read_bytes()).hexdigest()
    result={'source_sha256':before,'mode':'optimized' if sys.flags.optimize else 'normal',
            'admission_types':admission_types(),'write_failure':write_failure(),
            'queue':queue(),'hot_response':hot_response(),'archive_and_nested_union':archive_bound_and_nested_union(),'rebind':rebind_recovery()}
    check(hashlib.sha256(Path(f.__file__).read_bytes()).hexdigest()==before,'source changed during review probes')
    result['probe_sha256']=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    target=OUTPUT/('review-probes-'+result['mode']+'.json')
    target.write_text(json.dumps(result,indent=2,sort_keys=True)+'\n')
    print(json.dumps({'status':'PASS','output':str(target),'source_sha256':before},indent=2))
