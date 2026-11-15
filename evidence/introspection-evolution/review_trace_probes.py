"""Rehashed hostile complete-trace records; never modify engine source."""
from dataclasses import replace
from hashlib import sha256
import json
from pathlib import Path
import sys

ROOT=Path(__file__).resolve().parents[1]/'aod-dec'
sys.path.insert(0,str(ROOT))
from simulations import awareness_engine_trace as t

def altered(trace,rows=None,witnesses=None):
    events=trace.events if rows is None else tuple(t._json(row) for row in rows)
    witnesses=trace.witnesses if witnesses is None else tuple(witnesses)
    used=sum(len(raw.encode()) for raw in events)+sum(64+len(raw.encode()) for _,raw in witnesses)
    return replace(trace,events=events,witnesses=witnesses,retained_bytes=used)

before=sha256(Path(t.__file__).read_bytes()).hexdigest()
original=t.run_engine_case('mindfulness')['trace']
manual=t.fixture.load_manual()
results={'original':t.verify_trace(original)}
def probe(name,trace):
    try:
        results[name]={'status':'ACCEPTED','result':t.verify_trace(trace)}
    except (ValueError,TypeError,KeyError) as exc:
        results[name]={'status':'REJECTED','reason':str(exc)}

rows=[json.loads(raw) for raw in original.events]
root=next(row['id'] for row in rows if row['kind']=='call' and row['parent'] is None)
for row in rows:
    if row['kind']=='return' and row['id']==root:
        row['status']='none-or-unwound'
probe('root_unwound',altered(original,rows))

rows=[json.loads(raw) for raw in original.events]
parents={row['id']:row['parent'] for row in rows if row['kind']=='call'}
removed={row['id'] for row in rows if row['function']=='context'}
kept=[]
for row in rows:
    if row['id'] in removed:continue
    if row['kind']=='call':
        while row['parent'] in removed:row['parent']=parents[row['parent']]
    kept.append(row)
probe('deleted_context',altered(original,kept))

rows=[json.loads(raw) for raw in original.events]
for row in rows:
    if row['function']=='context':row['function']='core'
probe('relabelled_context',altered(original,rows))

rows=[json.loads(raw) for raw in original.events]
call=next(row for row in rows if row['kind']=='call' and row['function']=='retain')
returned=next(row for row in rows if row['kind']=='return' and row['id']==call['id'])
prior=t._unpack(json.loads(dict(original.witnesses)[call['arguments']['prior']]),t._registry(manual))
changed_prior=replace(prior,cross=(t.fixture.native.Q(2),))
changed_retained=manual.retain(changed_prior)
names={value:key for key,value in t._registry(manual).items()}
replacements={}
for old,value in ((call['arguments']['prior'],changed_prior),(returned['result'],changed_retained)):
    raw=t._json(t._pack(value,names));digest=sha256(raw.encode()).hexdigest()
    replacements[old]=(digest,raw)
witnesses=[replacements.get(key,(key,raw)) for key,raw in original.witnesses]
call['arguments']['prior']=replacements[call['arguments']['prior']][0]
returned['result']=replacements[returned['result']][0]
probe('detached_retention',altered(original,rows,witnesses))

rows=[json.loads(raw) for raw in original.events]
rows[0]['extra_unbound_claim']='a forged source claim'
probe('extra_record_field',altered(original,rows))

# Native certificate validation historically admits bool/int equality aliases;
# source-owned world admission must preserve the actual typed witness.
mapping={};witnesses=[]
def boolean_scale(value):
    if type(value) is dict:
        if value.get('type')=='native.Certificate' and value['fields'].get('scale')==0:
            value['fields']['scale']=False
        for child in value.values():boolean_scale(child)
    elif type(value) is list:
        for child in value:boolean_scale(child)
for key,raw in original.witnesses:
    value=json.loads(raw)
    if type(value) is dict and value.get('type')=='manual.World':boolean_scale(value)
    raw=t._json(value);digest=sha256(raw.encode()).hexdigest()
    mapping[key]=digest;witnesses.append((digest,raw))
rows=[]
for raw in original.events:
    for old,new in mapping.items():raw=raw.replace(old,new)
    rows.append(json.loads(raw))
probe('boolean_certificate_scale_in_world',altered(original,rows,witnesses))

after=sha256(Path(t.__file__).read_bytes()).hexdigest()
report={'status':'PASS' if all(value['status']=='REJECTED' for key,value in results.items() if key!='original') else 'FINDINGS',
    'source_sha256':after,'sources_unchanged':before==after,'probes':results}
Path(__file__).with_name('trace-probes-'+('optimized' if sys.flags.optimize else 'normal')+'.json').write_text(
    json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
if before!=after:raise ValueError('source changed during review')
