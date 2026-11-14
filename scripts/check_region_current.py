#!/usr/bin/env python3
"""Exercise the unchanged compiler with exact runtime boundary operands.

Collection coverage, histories, source validation and region construction remain
host responsibilities. This checks the scalar formulas in generated machine code.
"""
from pathlib import Path
import argparse
from fractions import Fraction as F
import hashlib
import itertools
import json
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from native import compiler
from runtime.dec_language import compile_source, Interpreter, DecError
from runtime.dec_core import render


def need(condition, message):
    if not condition:
        raise RuntimeError(message)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(output):
    output = Path(output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    source_path = ROOT / 'examples/region_current.dec'
    source = source_path.read_text()
    program = compile_source(source)
    work = output / 'native'

    cases = []
    for value in [F(4), F(-5, 3), F(0), F(10**35 + 1, 7)]:
        for inside_source, inside_target, available in itertools.product((0, 1), repeat=3):
            cases.append(('oriented_current', [value, inside_source, inside_target, available],
                          value * (inside_source - inside_target) * available))
    for before, after in itertools.product((0, 1), repeat=2):
        cases.append(('swept_current', [F(3), before, after], F(3) * (after - before)))
    for window in [0, 2, 4, 6, 12]:
        cases.append(('scoped_flux', [3, 5, window, 1, F(1, 3)], F(min(5, window))))
    for operands, expected in [([F(8), F(3), F(2)], F(7)),
                               ([F(-8), F(-3), F(-2)], F(-7)),
                               ([F(0), F(3), F(0)], F(-3))]:
        cases.append(('moving_current', operands, expected))
    bad_cases = [('oriented_current', [F(4), 2, 0, 1]),
                 ('oriented_current', [F(4), 1, 0, -1]),
                 ('swept_current', [F(3), 0, 2]),
                 ('swept_current', [F(-3), 0, 1]),
                 ('scoped_flux', [3, 5, -1, 1, F(1, 3)]),
                 ('scoped_flux', [3, 5, 4, 1, F(-1, 3)])]


    def foreign(args):
        return [('ratio', f'{x.numerator}/{x.denominator}') if isinstance(x, F) else x for x in args]


    records = []
    with compiler.JitSession(source, opt=2, cache_dir=work / 'cache') as session:
        for function, args, expected in cases:
            vm = Interpreter(program, emit=lambda _: None)
            actual = vm.call(function, args)
            need(actual == expected, f'Reference mismatch: {function}')
            fast = session.run(function=function, args=foreign(args))
            fallback = session.run(function=function, args=foreign(args), guard_key='stale')
            for execution in [fast, fallback]:
                need(execution['exit_code'] == 0 and execution['result'] == render(expected),
                     f'Machine-code mismatch: {function}')
                need(execution['address'] > 0, 'Generated machine code not entered')
            records.append({'function': function, 'args': [str(x) for x in args],
                            'expected': str(expected), 'reference': render(actual),
                            'jit': fast, 'generic_fallback': fallback})
        hostile = []
        for function, args in bad_cases:
            try:
                Interpreter(program, emit=lambda _: None).call(function, args)
            except DecError as exc:
                reference_error = str(exc)
            else:
                raise RuntimeError('Reference admitted invalid operands')
            fast = session.run(function=function, args=foreign(args))
            slow = session.run(function=function, args=foreign(args), guard_key='stale')
            need(fast['exit_code'] != 0 and slow['exit_code'] != 0, 'Invalid operands entered successfully')
            hostile.append({'function': function, 'args': [str(x) for x in args],
                            'reference_error': reference_error, 'jit': fast, 'generic_fallback': slow})
        jit_binding = session.info

    executable = work / 'region-current'
    build = compiler.build(program, executable, opt=2, cache_dir=work / 'cache')
    run = subprocess.run([str(executable)], capture_output=True, text=True, check=False)
    need(run.returncode == 0 and run.stdout == '4/1\n-4/1\n0/1\n0/1\n7/1\n0\n', 'AOT independent expected values')
    record = {'status': 'PASS', 'positive_runtime_cases': len(cases),
              'hostile_runtime_cases': len(bad_cases), 'records': records, 'hostile': hostile,
              'jit_binding': jit_binding, 'aot_build': build,
              'aot': {'exit_code': run.returncode, 'stdout': run.stdout, 'stderr': run.stderr,
                      'executable_sha256': digest(executable)},
              'scope': 'Exact scalar projection/clipping/sweep/moving-current expressions through reference VM, LLVM ORC specialized and fallback, and AOT. No native collection/history/geometry admission inferred.',
              'source_sha256': {str(p): digest(p) for p in [Path(__file__), source_path,
                  ROOT / 'native/compiler.py', ROOT / 'native/runtime.cpp',
                  ROOT / 'native/runtime.hpp', ROOT / 'native/runner.cpp',
                  ROOT / 'runtime/dec_language.py', ROOT / 'runtime/dec_core.py']}}
    (output / 'results.json').write_text(json.dumps(record, indent=2) + '\n')
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / 'build/region-current',
                        help='Directory for scalar compiler results and native artifacts')
    args = parser.parse_args()
    record = run(args.output)
    print(json.dumps({'status': record['status'],
                      'positive_runtime_cases': record['positive_runtime_cases'],
                      'hostile_runtime_cases': record['hostile_runtime_cases'],
                      'aot_stdout': record['aot']['stdout']}))


if __name__ == '__main__':
    main()
