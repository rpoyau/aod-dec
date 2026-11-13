#!/usr/bin/env python3
"""Dec core compiler to deterministic typed JSON IR and exact reference VM."""
import argparse
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
    args=parser.parse_args(argv)
    try:
        if args.command=='exec': program=loads_ir(read_limited(args.ir,MAX_IR))
        else: program=compile_source(read_limited(args.source,MAX_SOURCE))
        if args.command=='build':
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
    except (DecError,OSError,UnicodeError,ValueError,TypeError,ArithmeticError,RecursionError) as e:
        print(f'aodc: {e}',file=sys.stderr); return 1

if __name__=='__main__': raise SystemExit(main())
