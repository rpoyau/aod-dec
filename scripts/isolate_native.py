#!/usr/bin/env python3
"""Construct a Python-free minimal root, inspect dependencies and run via bwrap."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))

def prepare(out):
    from native.compiler import build,object_file,ensure_runtime
    out.mkdir(parents=True,exist_ok=True)
    cage=out/'root';cage.mkdir(exist_ok=True)
    binaries=[]
    for source in sorted((ROOT/'examples').glob('*.dec')):
        target=out/source.stem
        info=build(source.read_text(),target,opt=2)
        binaries.append((target,[],None))
    source='Dec square(x) -> y:\n    (int x) -> int y:\n        y = x * x + 7\nDec main() -> result:\n    () -> int result:\n        result = 0\n'
    (out/'runtime-input.dec').write_text(source)
    obj,_=object_file(source,opt=2);_,runtime=ensure_runtime()
    harness=out/'runtime-input.cpp'
    harness.write_text('''#include "runtime.hpp"
#include <cstdio>
#include <cstring>
extern "C" void* dec_square__int(void*,void*);
int main(int argc,char** argv){
  if(argc!=2)return 2;
  void* c=dec_context_new(100000,128);if(!c)return 3;
  void* a=dec_literal(c,"int",argv[1],std::strlen(argv[1]),0);
  void* r=dec_square__int(c,a);
  if(!dec_ok(c)){std::fprintf(stderr,"%s\\n",dec_error(c));dec_context_free(c);return 1;}
  const char* p=dec_render(c,r);std::fwrite(p,1,dec_render_size(c),stdout);std::fputc('\\n',stdout);
  dec_context_free(c);return 0;
}
''')
    input_bin=out/'runtime-input'
    subprocess.run(['g++','-std=c++17','-O2','-I',str(ROOT/'native'),str(harness),str(obj),str(runtime),'-lgmpxx','-lgmp','-lcrypto','-o',str(input_bin)],check=True)
    for n in ('0','-11','12345678901234567890123456789012345678901234567890'):
        binaries.append((input_bin,[n],str(int(n)**2+7)+'\n'))
    manifest={};runs=[]
    for binary,args,expected in binaries:
        binary=binary.resolve()
        ldd=subprocess.run(['ldd',str(binary)],capture_output=True,text=True,check=True).stdout
        (out/(binary.name+'.ldd.txt')).write_text(ldd)
        readelf=subprocess.run(['readelf','-d',str(binary)],capture_output=True,text=True,check=True).stdout
        (out/(binary.name+'.dynamic.txt')).write_text(readelf)
        disassembly=subprocess.run(['objdump','-d',str(binary)],capture_output=True,text=True,check=True).stdout
        (out/(binary.name+'.disassembly.txt')).write_text(disassembly)
        deps=re.findall(r'(/[^\s()]+)',ldd)
        if any('python' in s.lower() for s in deps):raise RuntimeError('Unexpected Python dependency')
        for dep in deps:
            src=Path(dep);dest=cage/dep.lstrip('/')
            dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(src,dest);dest.chmod(0o755)
            manifest[str(dest.relative_to(cage))]=hashlib.sha256(dest.read_bytes()).hexdigest()
        dest=cage/'bin'/binary.name;dest.parent.mkdir(exist_ok=True)
        shutil.copyfile(binary,dest);dest.chmod(0o755)
        manifest[str(dest.relative_to(cage))]=hashlib.sha256(dest.read_bytes()).hexdigest()
        normal=subprocess.run([str(binary)]+args,capture_output=True,text=True,check=True)
        if expected is not None and expected!=normal.stdout:raise RuntimeError('Independent runtime-input expectation failed')
        runs.append(dict(binary='/bin/'+binary.name,args=args,expected=expected or normal.stdout,dependencies=deps))
    (cage/'proc').mkdir(exist_ok=True);(cage/'dev').mkdir(exist_ok=True)
    spec={'root':str(cage.resolve()),'files':manifest,'runs':runs,'python_files':[],'claim':'Only listed executable and ELF dependency bytes exist in isolated root; no frontend, Python executable/library, source or transcript'}
    (out/'isolation-spec.json').write_text(json.dumps(spec,indent=2)+'\n')
    return spec

def execute(out):
    spec=json.loads((out/'isolation-spec.json').read_text());rows=[]
    for run in spec['runs']:
        command=['bwrap','--unshare-all','--die-with-parent','--ro-bind',spec['root'],'/','--proc','/proc','--dev','/dev','--clearenv',run['binary']]+run['args']
        p=subprocess.run(command,capture_output=True,text=True,timeout=30)
        rows.append(dict(command=command,stdout=p.stdout,stderr=p.stderr,exit_code=p.returncode,pass_=p.returncode==0 and p.stdout==run['expected']))
    result={'pass':all(r['pass_'] for r in rows),'runs':rows,'spec_sha256':hashlib.sha256((out/'isolation-spec.json').read_bytes()).hexdigest()}
    (out/'results.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2));return 0 if result['pass'] else 1

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--execute',action='store_true');a=p.parse_args()
    if a.execute:raise SystemExit(execute(a.output))
    prepare(a.output);print(a.output/'isolation-spec.json')
