"""Independent compiler/ORC contract tests; run directly to retain JSON evidence."""
from __future__ import annotations
import argparse
import copy
import ctypes
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from native import compiler as backend
from runtime.dec_language import Interpreter,compile_source,dumps_ir,DecError
from runtime.dec_core import render

SCALAR='''Dec square(n) -> r:
    (int n) -> int r:
        r = n * n
Dec main() -> r:
    () -> int r:
        emit "start"
        r = square(123456789012345678901234567890)
'''
TREE='''Dec tree(n) -> r:
    (int n) -> int r:
        r = 1 if n == 0 else tree(n - 1) + tree(n - 1)
Dec main() -> r:
    () -> int r:
        r = tree(12)
'''
RECORDS=[]

def digest(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def reference(source,fuel=100000,depth=128):
    out=[]
    try:
        value=Interpreter(compile_source(source),emit=out.append,max_steps=fuel,max_depth=depth).run()
        return {'stdout':''.join(x+'\n' for x in out),'result':render(value),'error':None,'exit_code':0}
    except Exception as e:
        return {'stdout':''.join(x+'\n' for x in out),'result':None,'error':str(e),'exit_code':1}

def observable(r):return {k:r[k] for k in ('stdout','result','error','exit_code')}

class NativeBackend(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory(prefix='dec-backend-')
        cls.work=Path(cls.temp.name); cls.program=compile_source(SCALAR)
        backend.ensure_runtime()
    @classmethod
    def tearDownClass(cls):cls.temp.cleanup()
    def record(self,name,**fields):RECORDS.append({'test':self.id(),'check':name,**fields})
    def test_cache_mutation_matrix(self):
        directory=self.work/'matrix'
        obj,base=backend.object_file(self.program,0,directory)
        _,again=backend.object_file(self.program,0,directory)
        self.assertFalse(base['cache_hit']);self.assertTrue(again['cache_hit'])
        self.record('initial-and-repeat',key=base['key'],first_hit=base['cache_hit'],second_hit=again['cache_hit'])
        mutated=compile_source(SCALAR.replace('n * n','n * n + 1'))
        _,changed=backend.object_file(mutated,0,directory)
        self.assertNotEqual(changed['key'],base['key']);self.assertFalse(changed['cache_hit'])
        self.record('source-change',key=changed['key'])
        _,optimized=backend.object_file(self.program,2,directory)
        self.assertNotEqual(optimized['key'],base['key']);self.assertFalse(optimized['cache_hit'])
        self.record('optimization-change',key=optimized['key'])
        for name in ('ABI_VERSION','COMPILER_VERSION'):
            with patch.object(backend,name,getattr(backend,name)+'-mutation'):
                _,changed=backend.object_file(self.program,0,directory)
            self.assertNotEqual(changed['key'],base['key']);self.assertFalse(changed['cache_hit'])
            self.record(name,key=changed['key'])
        real=backend.runtime_digest()
        with patch.object(backend,'runtime_digest',return_value=real+'-mutation'):
            _,changed=backend.object_file(self.program,0,directory)
        self.assertNotEqual(changed['key'],base['key']);self.assertFalse(changed['cache_hit'])
        self.record('runtime-source-binding-mutation',key=changed['key'],scope='synthetic digest mutation; runtime ABI remains fixed')
        L=backend.llvm()
        for attribute,value in [('cpu',L.cpu+'-mutation'),('features',L.features+',test-feature-mutation'),('triple','x86_64-unknown-linux-gnu')]:
            with patch.object(L,attribute,value): _,changed=backend.object_file(self.program,0,directory)
            self.assertNotEqual(changed['key'],base['key']);self.assertFalse(changed['cache_hit'])
            self.record('host-'+attribute,key=changed['key'],scope='cache invalidation; no foreign architecture support inferred')
    def test_cache_corruption_and_guard_repair(self):
        cache=self.work/'corruption';obj,base=backend.object_file(self.program,2,cache)
        orig=digest(obj); ir=Path(base['llvm_ir']); original_ir=ir.read_bytes()
        obj.write_bytes(b'not an object')
        repaired,info=backend.object_file(self.program,2,cache)
        self.assertFalse(info['cache_hit']);self.assertEqual(digest(repaired),orig)
        self.record('object-corruption-repaired',object_sha256=orig)
        ir.write_bytes(original_ir+b'\n; hostile stale IR\n')
        _,info=backend.object_file(self.program,2,cache)
        self.assertFalse(info['cache_hit']);self.assertEqual(ir.read_bytes(),original_ir)
        self.record('IR-corruption-repaired')
        with backend.JitSession(SCALAR,opt=2,cache_dir=cache) as session:
            first=session.run(); self.assertEqual(observable(first),reference(SCALAR))
            actual=Path(session.info['object'])
        for suffix in ('generic','guard'):
            p=actual.with_suffix('.'+suffix+'.o'); before=digest(p);p.write_bytes(b'corrupt '+suffix.encode())
            with backend.JitSession(SCALAR,opt=2,cache_dir=cache) as session:
                fast=session.run();slow=session.run(guard_key='invalid')
            self.assertEqual(digest(p),before);self.assertEqual(observable(fast),observable(slow));self.assertEqual(slow['dispatch'],'generic')
            self.record(suffix+'-object-corruption-repaired',object_sha256=before,dispatch=[fast['dispatch'],slow['dispatch']])
        manifest=actual.with_suffix('.json'); manifest.write_text('{broken')
        _,info=backend.object_file(self.program,2,cache)
        self.assertFalse(info['cache_hit']);self.record('malformed-manifest-repaired')
    def test_entry_guards_and_runtime_inputs(self):
        with backend.JitSession(SCALAR,opt=2,cache_dir=self.work/'entries') as session:
            for value in (0,-1,123,10**90+3):
                fast=session.run(function='square',args=[value]);slow=session.run(function='square',args=[value],guard_key='stale')
                self.assertEqual(fast['result'],str(value*value));self.assertEqual(observable(fast),observable(slow))
                self.assertEqual(fast['dispatch'],'specialized');self.assertEqual(slow['dispatch'],'generic');self.assertGreater(fast['address'],0)
                self.record('external-input',input=str(value),expected=str(value*value),address=fast['address'],paths=[fast['dispatch'],slow['dispatch']])
            with self.assertRaises(DecError):session.run(function='square',args=[True])
            with self.assertRaises(DecError):session.run(function='absent',args=[])
    def test_typed_foreign_entry(self):
        source="""Dec ident(x) -> r:
    (t1 x) -> t1 r:
        r = x
    (t32 x) -> t32 r:
        r = x
    (q4 x) -> q4 r:
        r = x
    (ratio x) -> ratio r:
        r = x
Dec main() -> r:
    () -> int r:
        r = 0
"""
        valid=[('t1','-1','t1(-1)'),('t32','926510094425920','t32(926510094425920)'),
               ('q4','15','q4(15)'),('ratio','-20/-30','2/3'),('ratio','+2/+3','2/3'),('ratio','0/-3','0/1'),
               ('ratio','7','7/1'),('ratio',str(10**100+7)+'/3',str(10**100+7)+'/3')]
        invalid=[('t1','2','t1 outside {-1,0,+1}'),('t32','926510094425921','t32 overflow'),
                 ('q4','16','q4 outside 0..15'),('ratio','1/0','zero denominator')]
        with backend.JitSession(source,opt=2,cache_dir=self.work/'typed') as session:
            for typ,text,expected in valid:
                for key in (None,'stale'):
                    result=session.run(function='ident',args=[(typ,text)],guard_key=key)
                    self.assertEqual(result['exit_code'],0);self.assertEqual(result['result'],expected)
                    self.record('typed-foreign-entry',type=typ,text=text,result=result)
            for typ,text,error in invalid:
                result=session.run(function='ident',args=[(typ,text)])
                self.assertEqual(result['error'],error);self.assertEqual(result['exit_code'],1)
                self.assertEqual(result['dispatch'],'not_entered');self.assertEqual(result['stdout'],'')
                self.record('typed-foreign-invalid-domain',type=typ,text=text,result=result)
            for typ,text in [('ratio','1/2/3'),('ratio','/2'),('ratio','1/'),('ratio','1.5/2'),('ratio',' 1/2'),('t32','1 2'),('t1','x')]:
                with self.assertRaises(DecError):session.run(function='ident',args=[(typ,text)])
                self.record('typed-foreign-malformed-rejected',type=typ,text=text)
    def test_concurrent_first_publish(self):
        shared=self.work/'parallel-cache'; parallel_root=self.work/'parallel-root';(parallel_root/'native').mkdir(parents=True)
        # Isolated empty runtime directory exercises first runtime publication,
        # while shared cache publication is independently serialized.
        for name in ('runtime.cpp','runtime.hpp'):
            (parallel_root/'native'/name).write_bytes((ROOT/'native'/name).read_bytes())
        code="""import json,sys
from pathlib import Path
from native import compiler as c
c.ROOT=Path(sys.argv[1])
source=sys.argv[3]
with c.JitSession(source,opt=2,cache_dir=sys.argv[2]) as j:
 r=j.run(); print(json.dumps({'result':r,'object':c.sha(Path(j.info['object']).read_bytes())}))
"""
        command=[sys.executable,'-c',code,str(parallel_root),str(shared),SCALAR]
        processes=[subprocess.Popen(command,cwd=ROOT,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True) for _ in range(3)]
        observations=[]
        for process in processes:
            stdout,stderr=process.communicate(timeout=90)
            self.assertEqual(process.returncode,0,stderr);observations.append(json.loads(stdout))
        expected=reference(SCALAR)
        for item in observations:self.assertEqual(observable(item['result']),expected)
        self.assertEqual(len({item['object'] for item in observations}),1)
        self.assertFalse(list(shared.glob('*.tmp')));self.assertFalse(list((parallel_root/'build/native').glob('*.tmp')))
        self.record('three-process-first-publish',observations=observations,scope='same empty runtime build directory and same empty object cache')
    def test_generated_code_permissions(self):
        with backend.JitSession(SCALAR,opt=2,cache_dir=self.work/'permissions') as session:
            result=session.run(); address=result['address']; found=[]
            for row in Path('/proc/self/maps').read_text().splitlines():
                span,perms,*_=row.split();lo,hi=[int(s,16) for s in span.split('-')]
                if lo<=address<hi:found.append(row);self.assertIn('x',perms);self.assertNotIn('w',perms)
            self.assertEqual(len(found),1);self.record('executed-address-mapping',address=address,mapping=found[0])
    def test_repeated_lifetime_and_concurrent_close(self):
        for i in range(5):
            session=backend.JitSession(SCALAR,opt=2,cache_dir=self.work/'lifetime');result=session.run();self.assertEqual(observable(result),reference(SCALAR));session.close()
            with self.assertRaisesRegex(DecError,'closed'):session.run()
            session.close();self.record('load-run-close',iteration=i,address=result['address'])
        session=backend.JitSession(TREE,opt=2,cache_dir=self.work/'concurrent');session._load()
        native_new=session.runtime.dec_context_new;entered=threading.Event();release=threading.Event();closed=threading.Event();events=[];result=[]
        def at_entry(*args):entered.set();self.assertTrue(release.wait(10));return native_new(*args)
        session.runtime.dec_context_new=at_entry
        def run():
            try: result.append(session.run(fuel=1000000));events.append(('call-returned',time.perf_counter_ns()))
            except BaseException as e:result.append(e)
        def close():session.close();events.append(('close-returned',time.perf_counter_ns()));closed.set()
        caller=threading.Thread(target=run);caller.start();self.assertTrue(entered.wait(10))
        closer=threading.Thread(target=close);closer.start();self.assertFalse(closed.wait(.03));release.set();caller.join(20);closer.join(20)
        self.assertFalse(caller.is_alive());self.assertFalse(closer.is_alive());self.assertTrue(closed.is_set());self.assertIsInstance(result[0],dict);self.assertEqual(result[0]['result'],'4096')
        # The call has completed native execution before its lock is released;
        # Python scheduling can append the diagnostic list after close acquired it.
        self.assertTrue(session.closed);self.record('close-waits-for-owned-call',events=events,result=result[0],blocked_before_release=True)
    def test_malformed_typed_IR_rejected(self):
        cases=[]
        bad=copy.deepcopy(self.program);bad['source_sha256']='0'*64;cases.append(bad)
        bad=copy.deepcopy(self.program);bad['version']=True;cases.append(bad)
        bad=copy.deepcopy(self.program);bad['functions'][0]['branches'][0]['body'][0]['value']['op']='+';cases.append(bad)
        bad=copy.deepcopy(self.program);bad['functions'][0]['branches'][0]['return_type']='bool';cases.append(bad)
        bad=copy.deepcopy(self.program);bad['extra']='forged';cases.append(bad)
        for index,bad in enumerate(cases):
            with self.assertRaises(DecError):backend.lower(bad)
            with self.assertRaises(DecError):backend.JitSession(bad)
            self.record('malformed-IR-rejected',mutation=index)
    def test_deterministic_artifacts(self):
        a,ia=backend.object_file(self.program,2,self.work/'repro-a');b,ib=backend.object_file(self.program,2,self.work/'repro-b')
        self.assertEqual(a.read_bytes(),b.read_bytes());self.assertEqual(Path(ia['llvm_ir']).read_bytes(),Path(ib['llvm_ir']).read_bytes())
        outa=self.work/'binary-a';outb=self.work/'binary-b'
        backend.build(SCALAR,outa,opt=2,cache_dir=self.work/'repro-a');backend.build(SCALAR,outb,opt=2,cache_dir=self.work/'repro-b')
        self.assertEqual(outa.read_bytes(),outb.read_bytes())
        result=subprocess.run([str(outa)],capture_output=True,text=True,check=True)
        expected=reference(SCALAR);self.assertEqual(result.stdout,expected['stdout']+expected['result']+'\n')
        self.record('repeat-build-bytes',object_sha256=digest(a),binary_sha256=digest(outa),stdout=result.stdout)
    def test_fuel_depth_and_effect_order(self):
        source='''Dec probe(n) -> r:
    (int n) -> int r:
        emit n
        r = 1 if n == 0 else probe(n - 1) + 1
Dec main() -> r:
    () -> int r:
        emit "before"
        r = probe(3)
'''
        with backend.JitSession(source,opt=2,cache_dir=self.work/'budgets') as session:
            for depth in (0,1,2,3,4,5,128):
                for fuel in (0,1,2,3,4,5,10,20,40,60,100,100000):
                    expect=reference(source,fuel,depth);got=session.run(fuel=fuel,max_depth=depth)
                    self.assertEqual(observable(got),expect,(fuel,depth,got,expect))
                    self.record('budget-parity',fuel=fuel,depth=depth,observable=observable(got),execute_ns=got['execute_ns'])
        # The invalid operands are untaken; neither error nor emit may occur.
        branches='''Dec fail() -> r:
    () -> int r:
        emit "wrong"
        r = 1 // 0
Dec main() -> r:
    () -> bool r:
        bool a = false and fail() == 0
        bool b = true or fail() == 0
        int c = 7 if b else fail()
        assert c == 7
        r = b
'''
        with backend.JitSession(branches,opt=2,cache_dir=self.work/'shortcircuit') as session:
            result=session.run();self.assertEqual(observable(result),reference(branches));self.assertEqual(result['stdout'],'');self.assertEqual(result['result'],'true');self.record('untaken-effects',result=result)
        failures='''Dec bad() -> r:
    () -> int r:
        emit "inside"
        r = 1 // 0
Dec main() -> r:
    () -> int r:
        emit "before"
        r = bad()
        emit "after"
'''
        with backend.JitSession(failures,opt=2,cache_dir=self.work/'errors') as session:
            result=session.run();self.assertEqual(observable(result),reference(failures));self.assertEqual(result['error_line'],4);self.record('first-error-and-emits',result=result)

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--evidence',type=Path);args=parser.parse_args()
    suite=unittest.defaultTestLoader.loadTestsFromTestCase(NativeBackend)
    started=time.time();result=unittest.TextTestRunner(verbosity=2).run(suite)
    if args.evidence:
        args.evidence.parent.mkdir(parents=True,exist_ok=True)
        args.evidence.write_text(json.dumps({'started_unix':started,'completed_unix':time.time(),'tests_run':result.testsRun,'failures':[(str(t),e) for t,e in result.failures],'errors':[(str(t),e) for t,e in result.errors],'status':'PASS' if result.wasSuccessful() else 'FAIL','compiler_sha256':digest(ROOT/'native/compiler.py'),'runtime_sha256':backend.runtime_digest(),'records':RECORDS},indent=2)+'\n')
    raise SystemExit(0 if result.wasSuccessful() else 1)
