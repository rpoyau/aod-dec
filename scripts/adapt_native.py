#!/usr/bin/env python3
"""Execute Dec with prior retained native selection; optionally profile later calls."""
import argparse
import hashlib
import json
from pathlib import Path
import random
import re
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from native.compiler import ABI_VERSION, JitSession
from native.learning import RetainedPolicy, VariantRegistry, digest, host_inventory, host_binding, semantic_outcome
from runtime.dec_language import compile_source, Interpreter
from runtime.dec_core import render


def reference(source):
    lines = []
    try:
        value = Interpreter(compile_source(source), emit=lines.append).run()
        return {'stdout': ''.join(x+'\n' for x in lines), 'result': render(value), 'error': None,
                'error_line': 0, 'exit_code': 0}
    except Exception as exc:
        message = str(exc)
        match = re.match(r'line (\d+):', message)
        return {'stdout': ''.join(x+'\n' for x in lines), 'result': None, 'error': message,
                'error_line': int(match[1]) if match else 0, 'exit_code': 1}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('--profile', required=True, type=Path)
    parser.add_argument('--mode', choices=('generic','passive','frozen','active'), default='active')
    parser.add_argument('--train-pairs', type=int, default=0)
    parser.add_argument('--cache', type=Path)
    parser.add_argument('--json-report', type=Path)
    args = parser.parse_args(argv)
    if not 0 <= args.train_pairs <= 32:
        parser.error('--train-pairs must be in 0..32')
    if args.train_pairs and args.mode not in ('active','passive'):
        parser.error('profiling requires --mode active or passive')
    started = time.perf_counter_ns()
    source = args.source.read_text()
    expected = reference(source)
    sessions = {}
    report = {'mode': args.mode, 'source_sha256': hashlib.sha256(source.encode()).hexdigest(),
              'profile': str(args.profile), 'observations': [],
              'performance_claim': 'No speedup claim; use held-out benchmark with all overheads.'}
    try:
        registry = VariantRegistry(report['source_sha256'], ABI_VERSION, expected)
        for name, opt in [('generic',0), ('optimized',2)]:
            session = JitSession(source, opt=opt, cache_dir=args.cache)
            sessions[name] = session
            value = session.run()
            obj = Path(session.info['object'])
            artifacts = {path.name: hashlib.sha256(path.read_bytes()).hexdigest()
                         for path in (obj, obj.with_suffix('.generic.o'), obj.with_suffix('.guard.o'))}
            registry.validate(name, digest(artifacts), value)
        inventory = host_inventory()
        binding = {'host': host_binding(inventory), 'software': digest(sessions['generic'].info['binding']),
                   'workload': report['source_sha256']}
        report['binding'] = binding
        report['variant_registry'] = registry.manifest()
        report['preparation_ns'] = time.perf_counter_ns()-started
        select_start = time.perf_counter_ns()
        policy = RetainedPolicy(args.profile, binding, registry)
        selection = policy.choose(args.mode)
        report['selection_ns'] = time.perf_counter_ns()-select_start
        report['selection'] = selection
        before = time.perf_counter_ns()
        value = sessions[selection['variant']].run()
        report['invocation_wall_ns'] = time.perf_counter_ns()-before
        if semantic_outcome(value) != expected:
            raise ValueError('selected native execution changed semantic outcome')
        report['executed'] = value
        # Completed invocation k precedes profiling and any generation changes.
        profiling_start = time.perf_counter_ns()
        rng = random.Random(20261113)
        for pair in range(args.train_pairs):
            order = ['generic','optimized']
            rng.shuffle(order)
            costs = {}
            for name in order:
                before = time.perf_counter_ns()
                observed = sessions[name].run()
                elapsed = time.perf_counter_ns()-before
                if semantic_outcome(observed) != expected:
                    raise ValueError('profiling changed semantic outcome')
                costs[name] = max(1,elapsed)
                report['observations'].append({'pair': pair, 'variant': name, 'wall_ns': elapsed,
                    'generated_code_ns': observed['execute_ns'], 'address': observed['address'],
                    'outcome_sha256': digest(semantic_outcome(observed))})
            policy.observe_pair(costs['generic'],costs['optimized'])
        if args.train_pairs:
            policy.save()
        report['profiling_ns'] = time.perf_counter_ns()-profiling_start
        report['next_invocation_preview'] = policy.choose(args.mode)
        report['total_ns'] = time.perf_counter_ns()-started
        if args.json_report:
            args.json_report.parent.mkdir(parents=True,exist_ok=True)
            args.json_report.write_text(json.dumps(report,indent=2)+'\n')
        print(value['stdout'], end='')
        if value['exit_code']:
            print('aodc: '+value['error'],file=sys.stderr)
        else:
            print(value['result'])
        return value['exit_code']
    finally:
        for session in sessions.values():
            session.close()

if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError,ValueError,TypeError,RuntimeError) as exc:
        print('adapt_native: '+str(exc),file=sys.stderr)
        raise SystemExit(1)
