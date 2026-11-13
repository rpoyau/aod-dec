#!/usr/bin/env python3
"""Independent expectations plus reference/AOT/ORC differential conformance.

Seed and all source/outputs are persisted. This checks the declared finite core,
not scientific claims about physical hardware or completeness of AOD ontology.
"""
import argparse
import copy
import datetime
from fractions import Fraction
import hashlib
import json
import os
from pathlib import Path
import random
import subprocess
import sys
import traceback

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from runtime.dec_language import compile_source, Interpreter, dumps_ir, loads_ir, DecError
from runtime.dec_core import render

SEED = 20261113
LIMIT = (3**32-1)//2

def program(body, result='int'):
    return f'Dec main() -> result:\n    () -> {result} result:\n'+''.join('        '+s+'\n' for s in body.splitlines())

def corpus():
    cases=[]
    def add(name, body, result='int', expected=None, error=None, emits=None, source=None):
        cases.append(dict(name=name,source=source or program(body,result),expected=expected,
                          error=error,emits=emits or []))
    add('body_one','result = 1',expected='1')
    add('body_mutated','result = 99',expected='99')
    add('exact_big','result = (100000000000000000000000000000000000000000000000001 * 99999999999999999999999999999999999999999999999999)',
        expected=str((10**50+1)*(10**50-1)))
    add('ratio_reduction','result = ratio(100000000000000000000000000000000000000, 300000000000000000000000000000000000000)', 'ratio',expected='1/3')
    add('floor_negative','emit -7 // 3\nemit 7 // -3\nemit -7 % 3\nemit 7 % -3\nresult = -7 // -3',expected='2',emits=['-3','-3','2','-2'])
    add('boundaries',f'emit t32({LIMIT})\nemit t32({-LIMIT})\nassert unpack(pack(t32({LIMIT}))) == t32({LIMIT})\nassert unpack(pack(t32({-LIMIT}))) == t32({-LIMIT})\nresult = int(unpack(0))',expected='0',emits=[f't32({LIMIT})',f't32({-LIMIT})'])
    add('overflow_intermediate',f'emit 7\nresult = int((t32({LIMIT}) + t32(1)) - t32(1))',error='t32 overflow',emits=['7'])
    add('t1_carry','result = int(t1(1) + t1(1))',error='t1 outside {-1,0,+1}')
    add('invalid_lane','result = int(unpack(3))',error='11 is invalid trit encoding; it is not Null')
    add('invalid_u64',f'result = int(unpack({2**64}))',error='packed t32 outside u64')
    add('nonintegral','result = int(ratio(1, 2))',error='ratio-to-int would lose a remainder')
    add('zero_denominator','emit 2\nratio bad = ratio(1, 0)\nemit 3\nresult = 4',error='zero denominator',emits=['2'])
    add('zero_divisor','result = 1 // 0',error='zero divisor')
    add('assert_location','emit 12\nassert false\nemit 19\nresult = 0',error='line 4: assertion failed',emits=['12'])
    add('unicode','emit "Ω\\u0000line\\nend"\nassert "é" != "e"\nresult = 1',expected='1',emits=['Ω\x00line\nend'])
    add('short_circuit','assert true or (1 / 0 == ratio(0))\nassert not (false and (1 / 0 == ratio(0)))\nresult = 4 if true else 1 // 0',expected='4')
    add('overload', '',expected='18',source='Dec twice(x) -> y:\n    (int x) -> int y:\n        y = x + x\n    (ratio x) -> ratio y:\n        y = x + x\n'+program('assert twice(ratio(1, 3)) == ratio(2, 3)\nresult = twice(9)'))
    add('recursion','',expected='479001600',source='Dec fact(n) -> r:\n    (int n) -> int r:\n        r = 1 if n == 0 else n * fact(n - 1)\n'+program('result = fact(12)'))
    add('effect_order','',expected='7',emits=['3','4'],source='Dec mark(x) -> y:\n    (int x) -> int y:\n        emit x\n        y = x\n'+program('result = mark(3) + mark(4)'))
    add('effect_first_failure','',error='zero divisor',emits=['3'],source='Dec mark(x) -> y:\n    (int x) -> int y:\n        emit x\n        y = x\n'+program('result = (mark(3) // 0) + mark(4)'))
    add('nonnegative_weights','result = probability(-1, 2, 3, t1(0))','ratio',error='weights must be exact and nonnegative')
    add('zero_weights','result = probability(0, 0, 0, t1(0))','ratio',error='normalization requires positive total weight')
    add('q4_bounds','result = int(q4(16))',error='q4 outside 0..15')
    add('empty_scope','result = phases(trace(" ", "x", q4(0)))',error='scope, lineage and window names must be nonempty')
    add('empty_rate','result = rate(trace("s", "x", q4(0)))','ratio',error='rate requires a positive committed event span')
    add('no_hinge','trace t = trace("s", "x", q4(0))\nt = step(t,q4(1),t1(1))\nt = step(t,q4(0),t1(-1))\nresult = phases(close(t))',error='counterbranch orientation requires structural hinge')
    add('hinge_first','result = phases(hinge(trace("s", "x", q4(0))))',error='one structural hinge must follow outgoing branch')
    add('hamming_two','result = phases(step(trace("s", "x", q4(0)), q4(3), t1(1)))',error='atomic move must be Hamming-1')
    add('null_orientation','result = phases(step(trace("s", "x", q4(0)), q4(1), t1(0)))',error='step requires nonzero orientation')
    add('open_return','trace t = trace("s", "x", q4(0))\nt = step(t, q4(1), t1(1))\nt = hinge(t)\nt = step(t, q4(3), t1(-1))\nresult = phases(close(t))',error='open trace does not return to retained start')
    add('dwell_return','trace t = trace("s", "x", q4(0))\nt = step(t, q4(1), t1(1))\nt = hinge(t)\nt = step(t, q4(0), t1(-1))\nt = dwell(t)\nresult = phases(close(t))',error='closure must identify a return event, not dwell')
    recur='Dec cycle(t) -> out:\n    (trace t) -> trace out:\n        t = step(t, q4(1), t1(1))\n        t = hinge(t)\n        out = close(step(t, q4(0), t1(-1)))\n'
    base='trace a = cycle(trace("s", "a", q4(0)))\ntrace b = cycle(trace("s", "b", q4(0)))\n'
    add('closure_replay','',expected='1',source=recur+program(base+'assert phases(close(a)) == 1\nassert beats(a) == 0\nassert phases(cycle(a)) == 2\nresult = phases(a)'))
    for name, body, error in [
        ('same_parents','fusion f = fold(a, a, "o", "ab")','fold requires distinct inner lineages'),
        ('same_outer_scope','fusion f = fold(a, b, "s", "ab")','outer scope must differ from inner scopes'),
        ('unclosed_outer','fusion f = fold(a,b,"o","ab")\ntrace t = fuse(f,outer(f,q4(0)))','fusion requires independently witnessed outer closure'),
        ('wrong_outer','fusion f = fold(a,b,"o","ab")\ntrace t = fuse(f,cycle(trace("o","ab",q4(0))))','outer trace belongs to a different proposal'),
        ('contact_fabricated','coupling c = coupling(a,b,"w")\nc = contact(c,1,3)','contact must bind certified return events'),
        ('contact_replay','coupling c = contact(coupling(a,b,"w"),3,3)\nc = contact(c,3,3)','contacts must be distinct, ordered and unreplayed'),
        ('comparison_once','coupling c = contact(coupling(a,b,"w"),3,3)\nratio r = cadence(c)','comparison needs recurrent lineages and repeated bound contacts'),
        ('same_lineage','coupling c = coupling(a,a,"w")','coupling needs distinct lineages in common scope'),
        ('unequal_spans','coupling c = coupling(a,dwell(b),"w")','coupling needs equal positive committed event spans')]:
        add(name,'',error=error,source=recur+program(base+body+'\nresult = 0'))
    rng=random.Random(SEED); body=[]; expected=[]
    for _ in range(64):
        a=rng.randrange(-2**256,2**256); b=rng.randrange(1,2**160)
        if rng.randrange(2): b=-b
        c=rng.randrange(1,2**128); d=rng.randrange(1,2**128)
        body.extend([f'emit {a} // {b}',f'emit {a} % {b}',f'emit ratio({a},{b}) + ratio({c},{d})'])
        q=Fraction(a,b)+Fraction(c,d)
        expected.extend([str(a//b),str(a%b),f'{q.numerator}/{q.denominator}'])
    body.append('result = 64')
    add('seeded_exact','\n'.join(body),expected='64',emits=expected)
    for f in sorted((ROOT/'examples').glob('*.dec')):
        add('example_'+f.stem,'',source=f.read_text())
    return cases

def reference(source, **kwargs):
    out=[]
    try:
        v=Interpreter(compile_source(source),emit=out.append,**kwargs).run()
        return dict(stdout=''.join(x+'\n' for x in out),result=render(v),error=None,exit_code=0)
    except DecError as e:
        return dict(stdout=''.join(x+'\n' for x in out),result=None,error=str(e),exit_code=1)

def run(output):
    from native.compiler import build, JitSession, lower
    output.mkdir(parents=True,exist_ok=True)
    rows=[]; cases=corpus()
    (output/'corpus.json').write_text(json.dumps({'seed':SEED,'cases':cases},indent=2)+'\n')
    for case in cases:
        name=case['name']; source=case['source']; ref=reference(source)
        (output/(name+'.dec')).write_text(source)
        independent=True
        if case['expected'] is not None or case['error'] is not None:
            import re
            actual_error=re.sub(r'^line [0-9]+: ', '', ref['error'] or '')
            expected_error=re.sub(r'^line [0-9]+: ', '', case['error'] or '')
            independent=ref['stdout']==''.join(x+'\n' for x in case['emits']) and actual_error==expected_error and (case['error'] is not None or ref['result']==case['expected'])
        for opt in (0,2):
            row=dict(name=name,opt=opt,source_sha256=hashlib.sha256(source.encode()).hexdigest(),reference=ref,independent_expected=independent)
            try:
                artifact=output/f'{name}-O{opt}'
                row['build']=build(source,artifact,opt=opt)
                p=subprocess.run([str(artifact.resolve())],capture_output=True,text=True,timeout=30)
                row['aot']={'stdout':p.stdout,'stderr':p.stderr,'exit_code':p.returncode}
                session=JitSession(source,opt=opt,cache_dir=output/'cache')
                try: row['jit']=session.run()
                finally: session.close()
                expected_out=ref['stdout']+(ref['result']+'\n' if ref['exit_code']==0 else '')
                jit=row['jit']
                # Exact diagnostic prose remains an extra check, separate from located metadata.
                aot_err=not ref['error'] or ref['error'] in p.stderr
                row['pass']=independent and p.returncode==ref['exit_code'] and p.stdout==expected_out and aot_err and all(jit[k]==ref[k] for k in ('stdout','result','error','exit_code'))
            except Exception as exc:
                row.update(pass_=False,exception=repr(exc),traceback=traceback.format_exc()); row['pass']=False
            rows.append(row)
            (output/'results.json').write_text(json.dumps({'seed':SEED,'complete':False,'results':rows},indent=2,default=str)+'\n')
            print(name,opt,'PASS' if row['pass'] else 'FAIL',flush=True)
    malformed=[]
    original=compile_source(program('result = 7'))
    for mutation in ('hash','type','literal','extra','source','control'):
        bad=copy.deepcopy(original)
        if mutation=='hash': bad['source_sha256']='0'*64
        elif mutation=='source': bad['source']=program('result = 8')
        elif mutation=='extra': bad['unsafe']=True
        else:
            expr=bad['functions'][0]['branches'][0]['body'][0]['value']
            if mutation=='type': expr['type']='bool'
            elif mutation=='literal': expr['value']=True
            else: expr['kind']='native_intrinsic'
        rejected=False
        try: lower(bad)
        except (DecError,ValueError): rejected=True
        malformed.append(dict(mutation=mutation,rejected=rejected))
    result={'seed':SEED,'complete':True,'created_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'results':rows,'malformed_ir':malformed,'pass':all(r['pass'] for r in rows) and all(r['rejected'] for r in malformed)}
    (output/'results.json').write_text(json.dumps(result,indent=2,default=str)+'\n')
    return 0 if result['pass'] else 1

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True)
    raise SystemExit(run(p.parse_args().output))
