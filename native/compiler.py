"""Verified Dec-to-LLVM lowering, native object emission and ORC LLJIT.

Python is a compile/load host only. Generated code calls the declared native ABI;
no Dec AST or interpreter is present in generated machine code.
"""
from __future__ import annotations
import ctypes as C
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import threading
import tempfile
import time
from runtime.dec_language import compile_source, loads_ir, dumps_ir, DecError
from runtime.dec_core import BUILTIN_SIGNATURES

ROOT = Path(__file__).resolve().parents[1]
ABI_VERSION = 'aod-native-1'
COMPILER_VERSION = 'dec-llvm-1'
LLVM_PATH = os.environ.get('DEC_LLVM_LIBRARY', '/usr/lib64/llvm16/lib/libLLVM-16.so')


def sha(data): return hashlib.sha256(data).hexdigest()
def program_of(program):
    return loads_ir(dumps_ir(compile_source(program) if isinstance(program,str) else program))
def mangle(name, types, prefix='dec'):
    return prefix+'_'+name+'__'+('_'.join(types) or 'void')

DECLARATIONS = '''
declare i32 @dec_tick(ptr)
declare i32 @dec_enter(ptr)
declare void @dec_leave(ptr)
declare i32 @dec_ok(ptr)
declare ptr @dec_literal(ptr, ptr, ptr, i64, i32)
declare ptr @dec_unary(ptr, ptr, ptr, i32)
declare ptr @dec_binary(ptr, ptr, ptr, ptr, i32)
declare ptr @dec_builtin(ptr, ptr, ptr, i32, i32)
declare i32 @dec_truth(ptr, ptr)
declare void @dec_emit(ptr, ptr, i32)
declare void @dec_assert(ptr, ptr, i32)
'''

class Lowering:
    def __init__(self, program, prefix):
        self.program=program_of(program); self.prefix=prefix; self.constants={}; self.code=[]; self.counter=0
    def string(self, text):
        raw=text.encode('utf-8'); key=raw
        if key not in self.constants: self.constants[key]=f'@{self.prefix}_s{len(self.constants)}'
        return self.constants[key]
    def fresh(self,kind='v'):
        self.counter+=1; return f'{kind}{self.counter}'
    def emit(self,line): self.code.append('  '+line)
    def label(self,label): self.code.append(label+':'); self.block=label
    def value(self,instr):
        v='%'+self.fresh(); self.emit(v+' = '+instr); return v
    def guard(self,value):
        cond=self.value('icmp ne i32 '+value+', 0'); label=self.fresh('ok')
        self.emit(f'br i1 {cond}, label %{label}, label %failed'); self.label(label)
    def check(self): self.guard(self.value('call i32 @dec_ok(ptr %ctx)'))
    def tick(self): self.guard(self.value('call i32 @dec_tick(ptr %ctx)'))
    def expr(self,e,env,line):
        self.tick(); k=e['kind']
        if k=='name': return env[e['name']]
        if k=='literal':
            text=('true' if e['value'] else 'false') if e['type']=='bool' else str(e['value'])
            v=self.value(f"call ptr @dec_literal(ptr %ctx, ptr {self.string(e['type'])}, ptr {self.string(text)}, i64 {len(text.encode())}, i32 {line})")
        elif k=='call':
            args=[self.expr(a,env,line) for a in e['args']]
            if e['name'] in BUILTIN_SIGNATURES:
                self.tick()
                if args:
                    arr=self.value(f'alloca [{len(args)} x ptr]')
                    for i,a in enumerate(args):
                        p=self.value(f'getelementptr [{len(args)} x ptr], ptr {arr}, i64 0, i64 {i}')
                        self.emit(f'store ptr {a}, ptr {p}')
                else: arr='null'
                v=self.value(f"call ptr @dec_builtin(ptr %ctx, ptr {self.string(e['name'])}, ptr {arr}, i32 {len(args)}, i32 {line})")
            else:
                symbol=mangle(e['name'],[a['type'] for a in e['args']],self.prefix)
                tail=''.join(', ptr '+a for a in args)
                v=self.value(f'call ptr @{symbol}(ptr %ctx{tail})')
        elif k in ('binary','compare'):
            a=self.expr(e['left'],env,line); b=self.expr(e['right'],env,line)
            v=self.value(f"call ptr @dec_binary(ptr %ctx, ptr {self.string(e['op'])}, ptr {a}, ptr {b}, i32 {line})")
        elif k=='unary':
            a=self.expr(e['value'],env,line)
            v=self.value(f"call ptr @dec_unary(ptr %ctx, ptr {self.string(e['op'])}, ptr {a}, i32 {line})")
        elif k=='conditional':
            a=self.expr(e['condition'],env,line); cond=self.value(f'call i32 @dec_truth(ptr %ctx, ptr {a})')
            cond=self.value(f'icmp ne i32 {cond}, 0'); yes,no,done=[self.fresh(x) for x in ('yes','no','merge')]
            self.emit(f'br i1 {cond}, label %{yes}, label %{no}')
            self.label(yes); a=self.expr(e['yes'],env,line); ap=self.block; self.emit(f'br label %{done}')
            self.label(no); b=self.expr(e['no'],env,line); bp=self.block; self.emit(f'br label %{done}')
            self.label(done); return self.value(f'phi ptr [{a}, %{ap}], [{b}, %{bp}]')
        elif k=='boolean':
            done=self.fresh('booldone'); incoming=[]
            for i,e2 in enumerate(e['values']):
                a=self.expr(e2,env,line); pred=self.block; incoming.append((a,pred))
                if i==len(e['values'])-1: self.emit(f'br label %{done}')
                else:
                    cond=self.value(f'call i32 @dec_truth(ptr %ctx, ptr {a})'); cond=self.value(f'icmp ne i32 {cond}, 0'); nxt=self.fresh('boolnext')
                    yes,no=(nxt,done) if e['op']=='and' else (done,nxt)
                    self.emit(f'br i1 {cond}, label %{yes}, label %{no}'); self.label(nxt)
            self.label(done); return self.value('phi ptr '+', '.join(f'[{a}, %{p}]' for a,p in incoming))
        else: raise DecError('unsupported verified expression '+k)
        self.check(); return v
    def function(self,f,b):
        self.counter=0; symbol=mangle(f['name'],[p['type'] for p in b['params']],self.prefix)
        args=''.join(f', ptr %arg{i}' for i in range(len(b['params'])))
        self.code.append(f'define ptr @{symbol}(ptr %ctx{args}) {{'); self.label('entry')
        first=self.value('call i32 @dec_tick(ptr %ctx)'); cond=self.value(f'icmp ne i32 {first}, 0')
        self.emit(f'br i1 {cond}, label %enter, label %prefailed'); self.label('enter')
        self.guard(self.value('call i32 @dec_enter(ptr %ctx)'))
        env={p['name']:f'%arg{i}' for i,p in enumerate(b['params'])}; result=None
        for s in b['body']:
            self.tick(); v=self.expr(s['value'],env,s['line']); k=s['kind']
            if k=='assign': env[s['name']]=v
            elif k in ('assert','emit'):
                self.emit(f"call void @dec_{k}(ptr %ctx, ptr {v}, i32 {s['line']})"); self.check()
            elif k=='return': result=v; break
        if result is None: result=env[b['output']]
        self.emit('call void @dec_leave(ptr %ctx)'); self.emit('ret ptr '+result)
        self.label('failed'); self.emit('call void @dec_leave(ptr %ctx)'); self.emit('ret ptr null')
        self.label('prefailed'); self.emit('ret ptr null'); self.code.append('}')
    def run(self, declarations=True):
        for f in self.program['functions']:
            for b in f['branches']: self.function(f,b)
        constants=[]
        for raw,name in self.constants.items():
            escaped=''.join('\\%02X'%b for b in raw+b'\0')
            constants.append(f'{name} = private unnamed_addr constant [{len(raw)+1} x i8] c"{escaped}"')
        return '\n'.join((['; Dec semantic IR v1; executable operations, no source VM',DECLARATIONS] if declarations else [])+constants+self.code)+'\n'

def lower(program,prefix='dec'):
    return Lowering(program,prefix).run()

P=C.c_void_p; S=C.c_char_p; I=C.c_int; U64=C.c_uint64
class LLVM:
    def __init__(self):
        if platform.machine()!='x86_64' or platform.system()!='Linux': raise DecError('only executed Linux x86-64 target is supported')
        self.lib=C.CDLL(LLVM_PATH)
        version=self.lib.LLVMGetVersion; version.restype=None; version.argtypes=[C.POINTER(C.c_uint)]*3
        major,minor,patch=C.c_uint(),C.c_uint(),C.c_uint(); version(C.byref(major),C.byref(minor),C.byref(patch))
        self.version=(major.value,minor.value,patch.value)
        if self.version != (16,0,6): raise DecError('backend requires pinned LLVM 16.0.6, found '+str(self.version))
        sig={
          'LLVMContextCreate':(P,[]),'LLVMContextDispose':(None,[P]),
          'LLVMCreateMemoryBufferWithMemoryRangeCopy':(P,[S,C.c_size_t,S]),
          'LLVMDisposeMemoryBuffer':(None,[P]),'LLVMParseIRInContext':(I,[P,P,C.POINTER(P),C.POINTER(P)]),
          'LLVMVerifyModule':(I,[P,I,C.POINTER(P)]),'LLVMDisposeModule':(None,[P]),
          'LLVMDisposeMessage':(None,[P]),'LLVMGetDefaultTargetTriple':(P,[]),
          'LLVMGetHostCPUName':(P,[]),'LLVMGetHostCPUFeatures':(P,[]),
          'LLVMGetTargetFromTriple':(I,[S,C.POINTER(P),C.POINTER(P)]),
          'LLVMCreateTargetMachine':(P,[P,S,S,S,I,I,I]),'LLVMDisposeTargetMachine':(None,[P]),
          'LLVMSetTarget':(None,[P,S]),'LLVMCreateTargetDataLayout':(P,[P]),
          'LLVMCopyStringRepOfTargetData':(P,[P]),'LLVMSetDataLayout':(None,[P,S]),'LLVMDisposeTargetData':(None,[P]),
          'LLVMTargetMachineEmitToFile':(I,[P,P,S,I,C.POINTER(P)]),
          'LLVMCreatePassBuilderOptions':(P,[]),'LLVMDisposePassBuilderOptions':(None,[P]),
          'LLVMRunPasses':(P,[P,S,P,P]),'LLVMGetErrorMessage':(P,[P]),'LLVMDisposeErrorMessage':(None,[P]),
          'LLVMOrcCreateLLJIT':(P,[C.POINTER(P),P]),'LLVMOrcDisposeLLJIT':(P,[P]),
          'LLVMOrcLLJITGetMainJITDylib':(P,[P]),'LLVMOrcLLJITAddObjectFile':(P,[P,P,P]),
          'LLVMOrcLLJITLookup':(P,[P,C.POINTER(U64),S]),'LLVMOrcLLJITMangleAndIntern':(P,[P,S]),
          'LLVMOrcAbsoluteSymbols':(P,[P,C.c_size_t]),'LLVMOrcJITDylibDefine':(P,[P,P]),
          'LLVMOrcReleaseSymbolStringPoolEntry':(None,[P]),
        }
        for name,(ret,args) in sig.items():
            fn=getattr(self.lib,name); fn.restype=ret; fn.argtypes=args; setattr(self,name,fn)
        for name in ('TargetInfo','Target','TargetMC','AsmPrinter','AsmParser'):
            fn=getattr(self.lib,'LLVMInitializeX86'+name); fn.restype=None; fn.argtypes=[]; fn()
        self.triple=self.message(self.LLVMGetDefaultTargetTriple()); self.cpu=self.message(self.LLVMGetHostCPUName()); self.features=self.message(self.LLVMGetHostCPUFeatures())
    def message(self,p):
        if not p: return ''
        result=C.string_at(p).decode(); self.LLVMDisposeMessage(p); return result
    def error(self,p):
        if p:
            s=self.LLVMGetErrorMessage(p); message=C.string_at(s).decode(); self.LLVMDisposeErrorMessage(s); raise DecError(message)
    def emit(self,ir,path,opt=0):
        context=self.LLVMContextCreate(); module=P(); message=P(); tm=None
        try:
            raw=ir.encode(); buf=self.LLVMCreateMemoryBufferWithMemoryRangeCopy(raw,len(raw),b'dec.ll')
            if self.LLVMParseIRInContext(context,buf,C.byref(module),C.byref(message)): raise DecError(self.message(message))
            if self.LLVMVerifyModule(module,2,C.byref(message)): raise DecError(self.message(message))
            if message.value: self.message(message); message=P()
            target=P()
            if self.LLVMGetTargetFromTriple(self.triple.encode(),C.byref(target),C.byref(message)): raise DecError(self.message(message))
            tm=self.LLVMCreateTargetMachine(target,self.triple.encode(),b'x86-64',b'',opt,2,0)
            if not tm: raise DecError('LLVM target-machine construction failed')
            self.LLVMSetTarget(module,self.triple.encode()); dl=self.LLVMCreateTargetDataLayout(tm)
            layout=self.message(self.LLVMCopyStringRepOfTargetData(dl)); self.LLVMSetDataLayout(module,layout.encode()); self.LLVMDisposeTargetData(dl)
            if opt:
                options=self.LLVMCreatePassBuilderOptions()
                try: self.error(self.LLVMRunPasses(module,('default<O%d>'%opt).encode(),tm,options))
                finally: self.LLVMDisposePassBuilderOptions(options)
            if self.LLVMVerifyModule(module,2,C.byref(message)): raise DecError(self.message(message))
            if message.value: self.message(message); message=P()
            if self.LLVMTargetMachineEmitToFile(tm,module,str(path).encode(),1,C.byref(message)): raise DecError(self.message(message))
        finally:
            if tm: self.LLVMDisposeTargetMachine(tm)
            if module: self.LLVMDisposeModule(module)
            self.LLVMContextDispose(context)

_LLVM=None
_LLVM_LOCK=threading.RLock()
def llvm():
    global _LLVM
    with _LLVM_LOCK:
        if _LLVM is None: _LLVM=LLVM()
        return _LLVM

RUNTIME_SYMBOLS=['dec_tick','dec_enter','dec_leave','dec_ok','dec_literal','dec_unary','dec_binary','dec_builtin','dec_truth','dec_emit','dec_assert']
def runtime_sources(): return [ROOT/'native/runtime.cpp',ROOT/'native/runtime.hpp']
def runtime_digest(): return sha(b''.join(p.read_bytes() for p in runtime_sources()))

_FILE_DIGESTS={}
def file_digest(path):
    path=Path(path); stat=path.stat(); key=(str(path.resolve()),stat.st_size,stat.st_mtime_ns)
    if key not in _FILE_DIGESTS: _FILE_DIGESTS[key]=sha(path.read_bytes())
    return _FILE_DIGESTS[key]
@contextmanager
def cache_lock(directory, name='.cache.lock'):
    """Serialize cooperating publishers across both threads and processes."""
    directory=Path(directory); directory.mkdir(parents=True,exist_ok=True)
    with (directory/name).open('a+b') as stream:
        fcntl.flock(stream.fileno(),fcntl.LOCK_EX)
        try: yield
        finally: fcntl.flock(stream.fileno(),fcntl.LOCK_UN)

@contextmanager
def staged_path(destination):
    destination=Path(destination)
    fd,name=tempfile.mkstemp(prefix='.'+destination.name+'.',suffix='.tmp',dir=destination.parent)
    os.close(fd)
    try:
        yield Path(name)
        with open(name,'rb') as stream: os.fsync(stream.fileno())
        os.replace(name,destination)
        directory=os.open(destination.parent,os.O_RDONLY|os.O_DIRECTORY)
        try: os.fsync(directory)
        finally: os.close(directory)
    finally:
        if os.path.exists(name): os.unlink(name)

def atomic_text(destination,text):
    with staged_path(destination) as temporary: temporary.write_text(text)

def ensure_runtime():
    builddir=ROOT/'build/native'; builddir.mkdir(parents=True,exist_ok=True)
    digest=runtime_digest(); so=builddir/('libdec-'+digest[:16]+'.so'); obj=builddir/('runtime-'+digest[:16]+'.o')
    manifest=builddir/('runtime-'+digest[:16]+'.json')
    with cache_lock(builddir,'.runtime.lock'):
        valid=False
        if obj.is_file() and so.is_file() and manifest.is_file():
            try: valid=json.loads(manifest.read_text())=={'source':digest,'object':file_digest(obj),'library':file_digest(so)}
            except (ValueError,OSError): pass
        if not valid:
            with staged_path(obj) as temporary:
                subprocess.run(['g++','-std=c++17','-O2','-fPIC','-c',str(ROOT/'native/runtime.cpp'),'-o',str(temporary)],check=True)
            with staged_path(so) as temporary:
                subprocess.run(['g++','-shared',str(obj),'-lgmpxx','-lgmp','-lcrypto','-o',str(temporary)],check=True)
            atomic_text(manifest,json.dumps({'source':digest,'object':file_digest(obj),'library':file_digest(so)},sort_keys=True)+'\n')
    return so,obj

def binding(program,opt):
    L=llvm()
    return {'compiler':COMPILER_VERSION,'compiler_sha256':sha(Path(__file__).read_bytes()),'semantic_version':1,'abi':ABI_VERSION,'source_sha256':program['source_sha256'],'ir_sha256':sha(dumps_ir(program).encode()),'runtime_sha256':runtime_digest(),'llvm_version':list(L.version),'llvm_library':str(Path(LLVM_PATH).resolve()),'llvm_sha256':file_digest(LLVM_PATH),'target':L.triple,'cpu':L.cpu,'features':L.features,'opt':opt}

def _object_file(program,opt=0,cache_dir=None):
    if opt not in (0,1,2,3): raise DecError('optimization level must be 0..3')
    program=program_of(program); started=time.perf_counter_ns(); meta=binding(program,opt); key=sha(json.dumps(meta,sort_keys=True).encode())
    directory=Path(cache_dir) if cache_dir else ROOT/'build/native/cache'; directory.mkdir(parents=True,exist_ok=True)
    obj=directory/(key+'.o'); manifest=directory/(key+'.json'); irpath=directory/(key+'.ll'); hit=False
    if obj.is_file() and manifest.is_file() and irpath.is_file():
        try:
            old=json.loads(manifest.read_text()); hit=old['binding']==meta and old['object_sha256']==sha(obj.read_bytes()) and old['llvm_ir_sha256']==sha(irpath.read_bytes())
        except (ValueError,KeyError,OSError): pass
    if not hit:
        ir=lower(program)
        with staged_path(obj) as temporary:
            with _LLVM_LOCK: llvm().emit(ir,temporary,opt)
        atomic_text(irpath,ir)
        atomic_text(manifest,json.dumps({'binding':meta,'key':key,'object_sha256':sha(obj.read_bytes()),'llvm_ir_sha256':sha(ir.encode())},indent=2)+'\n')
    return obj,{'compile_ns':time.perf_counter_ns()-started,'cache_hit':hit,'key':key,'object':str(obj),'llvm_ir':str(irpath),'binding':meta}

def object_file(program,opt=0,cache_dir=None):
    directory=Path(cache_dir) if cache_dir else ROOT/'build/native/cache'
    with cache_lock(directory): return _object_file(program,opt,directory)

def build(program,output,opt=0,cache_dir=None):
    program=program_of(program); started=time.perf_counter_ns(); so,runtime=ensure_runtime(); obj,info=object_file(program,opt,cache_dir)
    output=Path(output).resolve(); output.parent.mkdir(parents=True,exist_ok=True)
    runner=ROOT/'native/runner.cpp'
    command=['g++','-std=c++17','-O2',str(runner),str(obj),str(runtime),'-lgmpxx','-lgmp','-lcrypto','-o',str(output)]
    linkstart=time.perf_counter_ns(); subprocess.run(command,check=True)
    info.update(output=str(output),link_ns=time.perf_counter_ns()-linkstart,total_ns=time.perf_counter_ns()-started,command=command)
    output.with_suffix(output.suffix+'.build.json').write_text(json.dumps(info,indent=2)+'\n'); return info

def guard_ir(program,key):
    """Identity-bound optimized entry with generic fallback before source effects."""
    expected=int(key[:16],16); lines=[]
    for f in program['functions']:
        for b in f['branches']:
            types=[p['type'] for p in b['params']]
            sig=''.join(', ptr' for _ in types)
            actual=''.join(f', ptr %a{i}' for i in range(len(types)))
            declaration=''.join(f', ptr %a{i}' for i in range(len(types)))
            fast=mangle(f['name'],types); slow=mangle(f['name'],types,'generic'); wrapper=mangle(f['name'],types,'guard')
            lines.extend([f'declare ptr @{fast}(ptr{sig})',f'declare ptr @{slow}(ptr{sig})',
                f'define ptr @{wrapper}(ptr %ctx{declaration}, i64 %identity, ptr %path) {{',
                'entry:',f'  %valid = icmp eq i64 %identity, {expected}',
                '  br i1 %valid, label %specialized, label %fallback',
                'specialized:','  store i32 1, ptr %path',f'  %s = call ptr @{fast}(ptr %ctx{actual})','  ret ptr %s',
                'fallback:','  store i32 0, ptr %path',f'  %g = call ptr @{slow}(ptr %ctx{actual})','  ret ptr %g','}'])
    return '\n'.join(lines)+'\n'

def _auxiliary_objects(program,main_info):
    base=Path(main_info['object']); objects=[]
    for suffix,ir in [('generic',lower(program,'generic')),('guard',guard_ir(program,main_info['key']))]:
        obj=base.with_suffix('.'+suffix+'.o'); meta=obj.with_suffix(obj.suffix+'.json'); digest=sha(ir.encode()); valid=False
        if obj.exists() and meta.exists():
            try:
                old=json.loads(meta.read_text()); valid=old=={'ir':digest,'object':sha(obj.read_bytes())}
            except (OSError,ValueError): pass
        if not valid:
            with staged_path(obj) as temporary:
                with _LLVM_LOCK: llvm().emit(ir,temporary,0)
            atomic_text(obj.with_suffix('.ll'),ir); atomic_text(meta,json.dumps({'ir':digest,'object':sha(obj.read_bytes())})+'\n')
        objects.append(obj)
    return objects

def auxiliary_objects(program,main_info):
    with cache_lock(Path(main_info['object']).parent): return _auxiliary_objects(program,main_info)

class _Flags(C.Structure): _fields_=[('GenericFlags',C.c_uint8),('TargetFlags',C.c_uint8)]
class _Symbol(C.Structure): _fields_=[('Address',U64),('Flags',_Flags)]
class _Pair(C.Structure): _fields_=[('Name',P),('Sym',_Symbol)]

class JitSession:
    """An owning ORC code lifetime. Calls and close are serialized per session.

    No raw callable escapes this owner. The generated address is diagnostic only.
    Closed sessions reject calls rather than touching reclaimed executable pages.
    """
    def __init__(self,program,opt=0,cache_dir=None):
        self.program=program_of(program); self.opt=opt; self.cache_dir=cache_dir; self.lock=threading.RLock(); self.closed=False; self.jit=None; self.compile_ns=0; self.load_ns=0; self.cache_hit=False; self.addresses={}; self.info={}
    def _load(self):
        if self.jit: return
        runtime_start=time.perf_counter_ns(); so,_=ensure_runtime(); runtime_ns=time.perf_counter_ns()-runtime_start; obj,info=object_file(self.program,self.opt,self.cache_dir); extra_start=time.perf_counter_ns(); extra=auxiliary_objects(self.program,info); self.info=info; self.compile_ns=runtime_ns+info['compile_ns']+time.perf_counter_ns()-extra_start; self.cache_hit=info['cache_hit']; start=time.perf_counter_ns()
        self.runtime=C.CDLL(str(so)); rt=self.runtime
        sig={'dec_context_new':(P,[C.c_int64,I]),'dec_context_free':(None,[P]),'dec_ok':(I,[P]),'dec_error':(S,[P]),'dec_error_line':(I,[P]),'dec_output':(P,[P]),'dec_output_size':(U64,[P]),'dec_render':(P,[P,P]),'dec_render_size':(U64,[P]),'dec_literal':(P,[P,S,S,U64,I]),'dec_builtin':(P,[P,S,C.POINTER(P),I,I])}
        for name,(r,a) in sig.items(): getattr(rt,name).restype=r; getattr(rt,name).argtypes=a
        L=llvm(); jit=P(); L.error(L.LLVMOrcCreateLLJIT(C.byref(jit),None)); self.jit=jit
        try:
            jd=L.LLVMOrcLLJITGetMainJITDylib(jit); pairs=(_Pair*len(RUNTIME_SYMBOLS))()
            for pair,name in zip(pairs,RUNTIME_SYMBOLS):
                pair.Name=L.LLVMOrcLLJITMangleAndIntern(jit,name.encode()); pair.Sym=_Symbol(C.cast(getattr(rt,name),P).value,_Flags(5,0))
            unit=L.LLVMOrcAbsoluteSymbols(pairs,len(pairs)); L.error(L.LLVMOrcJITDylibDefine(jd,unit))
            for path in [obj]+extra:
                raw=path.read_bytes(); buf=L.LLVMCreateMemoryBufferWithMemoryRangeCopy(raw,len(raw),b'dec.o'); L.error(L.LLVMOrcLLJITAddObjectFile(jit,jd,buf))
        except BaseException:
            L.error(L.LLVMOrcDisposeLLJIT(jit)); self.jit=None; raise
        self.load_ns=time.perf_counter_ns()-start
    def _literal(self,ctx,value):
        if type(value) is bool: typ,text='bool','true' if value else 'false'
        elif type(value) is int: typ,text='int',str(value)
        elif type(value) is str: typ,text='str',value
        elif isinstance(value,tuple) and len(value)==2: typ,text=value
        else: raise DecError('entry arguments must be int/bool/str or (type, text)')
        if typ not in ('int','bool','str','t1','t32','q4','ratio'): raise DecError('unsupported external argument type')
        text=str(text)
        if typ in ('int','t1','t32','q4') and not re.fullmatch(r'[+-]?[0-9]+',text):
            raise DecError('external integer text must contain one exact decimal integer')
        def literal(kind,spelling):
            if kind=='int' and spelling.startswith('+'): spelling=spelling[1:]
            raw=spelling.encode(); return self.runtime.dec_literal(ctx,kind.encode(),raw,len(raw),0)
        if typ in ('int','bool','str'): return typ,literal(typ,text)
        if typ=='ratio':
            match=re.fullmatch(r'([+-]?[0-9]+)(?:/([+-]?[0-9]+))?',text)
            if not match: raise DecError('external ratio text must be N/D or an exact decimal integer')
            handles=[literal('int',match[1])]
            if match[2] is not None: handles.append(literal('int',match[2]))
        else:
            handles=[literal('int',text)]
        array=(P*len(handles))(*handles)
        # Construct source-level numeric types through the same checked native
        # conversions used by compiled code, before entering the source function.
        return typ,self.runtime.dec_builtin(ctx,typ.encode(),array,len(handles),0)
    def run(self,fuel=100000,max_depth=128,function='main',args=None,guard_key=None):
        with self.lock:
            if self.closed: raise DecError('JIT session is closed')
            self._load(); rt=self.runtime; ctx=rt.dec_context_new(fuel,max_depth)
            if not ctx: raise DecError('native context allocation failed')
            try:
                args=[] if args is None else args; types=[]; handles=[]
                for value in args:
                    typ,handle=self._literal(ctx,value); types.append(typ); handles.append(handle)
                symbol=mangle(function,types,'guard')
                if not any(f['name']==function and any([p['type'] for p in b['params']]==types for b in f['branches']) for f in self.program['functions']): raise DecError('unlisted native entry signature')
                if symbol not in self.addresses:
                    start=time.perf_counter_ns(); address=U64(); llvm().error(llvm().LLVMOrcLLJITLookup(self.jit,C.byref(address),symbol.encode())); self.addresses[symbol]=address.value; self.load_ns+=time.perf_counter_ns()-start
                address=self.addresses[symbol]; call=C.CFUNCTYPE(P,P,*([P]*len(handles)),U64,C.POINTER(I))(address)
                identity=int(self.info['key'][:16],16) if guard_key is None or guard_key==self.info['key'] else int(sha(str(guard_key).encode())[:16],16); dispatch=I(-1)
                started=time.perf_counter_ns(); result=call(ctx,*handles,identity,C.byref(dispatch)) if rt.dec_ok(ctx) else None; elapsed=time.perf_counter_ns()-started
                output=C.string_at(rt.dec_output(ctx),rt.dec_output_size(ctx)).decode('utf-8'); ok=rt.dec_ok(ctx)
                rendered=C.string_at(rt.dec_render(ctx,result),rt.dec_render_size(ctx)).decode('utf-8') if ok else None
                return {'stdout':output,'result':rendered,'error':None if ok else rt.dec_error(ctx).decode(),'error_line':0 if ok else rt.dec_error_line(ctx),'exit_code':0 if ok else 1,'execute_ns':elapsed,'address':address,'cache_hit':self.cache_hit,'compile_ns':self.compile_ns,'load_ns':self.load_ns,'key':self.info['key'],'dispatch':'specialized' if dispatch.value==1 else 'generic' if dispatch.value==0 else 'not_entered'}
            finally: rt.dec_context_free(ctx)
    def close(self):
        with self.lock:
            if self.jit: llvm().error(llvm().LLVMOrcDisposeLLJIT(self.jit)); self.jit=None
            self.closed=True; self.addresses.clear()
    def __enter__(self): return self
    def __exit__(self,*args): self.close()
