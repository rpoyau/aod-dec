#!/usr/bin/env python3
"""Dec core compiler to deterministic typed JSON IR and exact reference VM."""
import argparse
import json
from pathlib import Path
import sys
from runtime.dec_language import DecError, Interpreter, compile_source, dumps_ir, loads_ir, MAX_SOURCE, MAX_IR
from runtime.dec_core import render

def read_limited(path,limit):
    with Path(path).open('rb') as f:
        raw=f.read(limit+1)
    if len(raw)>limit: raise DecError(f"input exceeds {limit} byte limit")
    return raw.decode('utf-8')

def main(argv=None):
    parser=argparse.ArgumentParser(description=__doc__)
    sub=parser.add_subparsers(dest='command',required=True)
    for name in ('check','run','compile','build','jit'):
        p=sub.add_parser(name); p.add_argument('source')
        if name in ('compile','build'): p.add_argument('-o','--output',required=True)
        if name in ('build','jit'): p.add_argument('-O',type=int,default=0,choices=range(4))
    p=sub.add_parser('exec'); p.add_argument('ir')
    p=sub.add_parser('batch',help='compile a pure int/bool entry and execute independent ordered lanes')
    p.add_argument('source'); p.add_argument('--function',required=True)
    p.add_argument('--inputs',required=True,help='JSON array of exact typed argument rows')
    p.add_argument('--backend',choices=('cpu','gpu'),required=True)
    p.add_argument('--workers',type=int,default=1)
    p.add_argument('--fuel',type=int,default=100000)
    p.add_argument('--max-depth',type=int,default=128)
    p.add_argument('--param-types',nargs='*',choices=('int','bool'))
    p.add_argument('-O',type=int,default=0,choices=range(4))
    args=parser.parse_args(argv)
    try:
        if args.command=='exec': program=loads_ir(read_limited(args.ir,MAX_IR))
        else:
            source=read_limited(args.source,MAX_SOURCE)
            program=compile_source(source)
        if args.command=='batch':
            rows=json.loads(read_limited(args.inputs,8*1024*1024))
            options=dict(fuel=args.fuel,max_depth=args.max_depth,opt=args.O,param_types=args.param_types)
            if args.backend=='cpu':
                from native.cpu_batch import run_cpu_batch
                result=run_cpu_batch(source,args.function,rows,workers=args.workers,**options)
            else:
                if args.workers!=1: raise DecError('GPU uses one device-owning host; --workers must be 1')
                from native.gpu_compiler import run_gpu_batch
                result=run_gpu_batch(source,args.function,rows,**options)
            print(json.dumps(result,sort_keys=True,allow_nan=False))
            return int(any(row['exit_code'] for row in result['rows']))
        elif args.command=='build':
            from native.compiler import build
            if Path(args.output).resolve()==Path(args.source).resolve(): raise DecError('output must differ from source')
            build(program,args.output,opt=args.O)
        elif args.command=='jit':
            from native.compiler import JitSession
            with JitSession(program,opt=args.O) as session: result=session.run()
            print(result['stdout'],end='')
            if result['exit_code']: raise DecError(result['error'])
            print(result['result'])
        elif args.command=='compile':
            if Path(args.output).resolve()==Path(args.source).resolve(): raise DecError('output must differ from source')
            # Encode before creating the output; all static checks have completed.
            encoded=dumps_ir(program)
            Path(args.output).write_text(encoded,encoding='utf-8')
        elif args.command=='check': print('OK: Dec core v1')
        else: print(render(Interpreter(program).run()))
        return 0
    except (DecError,OSError,UnicodeError,ValueError,TypeError,ArithmeticError,RecursionError,RuntimeError) as e:
        print(f'aodc: {e}',file=sys.stderr); return 1

if __name__=='__main__': raise SystemExit(main())
