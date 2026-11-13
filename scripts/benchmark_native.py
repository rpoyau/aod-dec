#!/usr/bin/env python3
"""Reproducible native ORC benchmark and retained-policy control experiment."""
from __future__ import annotations
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import random
import re
import resource
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from native.compiler import ABI_VERSION, JitSession, build
from native.learning import (RetainedPolicy, VariantRegistry, digest, host_binding,
                             host_inventory, semantic_outcome)
from runtime.dec_language import Interpreter, compile_source
from runtime.dec_core import render

SEED = 20261113
CONTROLS = ('generic', 'passive', 'frozen', 'active', 'stale', 'shuffled', 'removed')


def now():
    return datetime.now(timezone.utc).isoformat()


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def reference(source):
    output = []
    try:
        result = Interpreter(compile_source(source), emit=output.append).run()
        return {'stdout': ''.join(x+'\n' for x in output), 'result': render(result),
                'error': None, 'error_line': 0, 'exit_code': 0}
    except Exception as exc:
        message = str(exc)
        match = re.match(r'line (\d+): (.*)', message, re.S)
        return {'stdout': ''.join(x+'\n' for x in output), 'result': None,
                'error': message,
                'error_line': int(match[1]) if match else 0, 'exit_code': 1}


def workloads():
    result = {name: (ROOT/'examples'/(name+'.dec')).read_text()
              for name in ('ternary', 'weights', 'closure', 'fusion', 'coupling')}
    huge = '1'+'0'*119+'7'
    result['rational_stress'] = ('Dec main() -> result:\n    () -> ratio result:\n'
        +'        ratio value = ratio('+huge+', 999999999999999999999999999999)\n'
        +''.join('        value = (value + ratio(1, 3)) * ratio(3, 2) - ratio(1, 2)\n' for _ in range(40))
        +'        emit value\n        result = value\n')
    result['nested_fusion'] = result['fusion'].replace('        emit c\n',
        '        trace d = recur(trace("separate", "d", q4(0)))\n'
        '        fusion second = fold(c, d, "outer2", "abcd")\n'
        '        trace e = recur(outer(second, q4(0)))\n'
        '        e = fuse(second, e)\n        assert phases(e) == 1\n        emit e\n')
    return result


def batch(session, expected, count):
    started = time.perf_counter_ns()
    values = [session.run() for _ in range(count)]
    elapsed = time.perf_counter_ns()-started
    validated = time.perf_counter_ns()
    equal = all(semantic_outcome(value) == expected for value in values)
    return {'elapsed_ns': elapsed, 'per_invocation_ns': max(1, elapsed//count),
            'generated_code_ns': [value['execute_ns'] for value in values],
            'addresses': sorted({value['address'] for value in values}),
            'native_dispatch': [value.get('dispatch') for value in values],
            'invocations_per_second': count*1_000_000_000/elapsed,
            'validation_ns': time.perf_counter_ns()-validated,
            'invocations': count, 'semantic_equal': equal,
            'outcome_sha256': digest([semantic_outcome(value) for value in values]),
            'failures': [value for value in values if semantic_outcome(value) != expected]}


def execute(output):
    output.mkdir(parents=True, exist_ok=False)
    started_all = time.perf_counter_ns()
    protocol = ROOT/'docs/BENCHMARK_PROTOCOL.md'
    shutil.copyfile(protocol, output/'BENCHMARK_PROTOCOL.md')
    run = {'protocol_sha256': sha(protocol), 'started_utc': now(), 'seed': SEED,
           'warmup': 3, 'tuning_pairs': 12, 'heldout_pairs': 20, 'batch_tuning': 5,
           'batch_heldout': 10, 'workloads': {}, 'failures': [],
           'implementation_binding': {str(p.relative_to(ROOT)): sha(p) for p in
               [Path(__file__).resolve(), ROOT/'native/learning.py', ROOT/'native/compiler.py',
                ROOT/'native/runtime.cpp', ROOT/'native/runtime.hpp']}}
    (output/'run-start.json').write_text(json.dumps(run, indent=2)+'\n')
    inventory = host_inventory()
    (output/'host.json').write_text(json.dumps(inventory, indent=2)+'\n')
    rng = random.Random(SEED)
    raw = (output/'observations.jsonl').open('w')
    def record(obj):
        raw.write(json.dumps(dict(recorded_utc=now(), **obj), sort_keys=True)+'\n')
        raw.flush()
    for name, source in workloads().items():
        work = output/name
        work.mkdir()
        (work/'source.dec').write_text(source)
        expected = reference(source)
        (work/'reference.json').write_text(json.dumps(expected, indent=2)+'\n')
        sessions, builds, timings = {}, {}, {}
        summary = {'prevalidation': {}, 'raw_records': '../observations.jsonl', 'controls': {},
                   'train_costs_ns': {'generic': [], 'optimized': []},
                   'heldout_costs_ns': {'generic': [], 'optimized': []}}
        run['workloads'][name] = summary
        try:
            registry = VariantRegistry(hashlib.sha256(source.encode()).hexdigest(), ABI_VERSION, expected)
            for variant, opt in [('generic', 0), ('optimized', 2)]:
                builds[variant] = build(source, work/variant, opt=opt, cache_dir=work/'cache')
                sessions[variant] = JitSession(source, opt=opt, cache_dir=work/'cache')
                wall = time.perf_counter_ns()
                actual = sessions[variant].run()
                timings[variant] = {'first_invocation_wall_ns': time.perf_counter_ns()-wall,
                    'compile_ns': actual['compile_ns'], 'load_ns': actual['load_ns'],
                    'native_ns': actual['execute_ns'], 'cache_hit': actual['cache_hit']}
                if sessions[variant].info['binding'] != builds[variant]['binding']:
                    raise ValueError('compiler/runtime binding changed during variant construction')
                obj = Path(sessions[variant].info['object'])
                artifacts = {path.name: sha(path) for path in (obj, obj.with_suffix('.generic.o'), obj.with_suffix('.guard.o'))}
                registry.validate(variant, digest(artifacts), actual)
                fallback = sessions[variant].run(guard_key='deliberately stale identity')
                if semantic_outcome(fallback) != expected or fallback['dispatch'] != 'generic':
                    raise ValueError('native entry guard fallback failed')
                proc = subprocess.run([str(work/variant)], capture_output=True, text=True, timeout=30)
                aot_ok = proc.returncode == expected['exit_code'] and proc.stdout == expected['stdout']+expected['result']+'\n'
                summary['prevalidation'][variant] = {'jit': actual, 'aot_stdout': proc.stdout,
                    'aot_stderr': proc.stderr, 'aot_exit_code': proc.returncode, 'aot_equal': aot_ok,
                    'binary_sha256': sha(work/variant), 'jit_artifacts': artifacts, 'guard_fallback': fallback}
                if not aot_ok:
                    raise ValueError('AOT prevalidation failed: '+name+'/'+variant)
            (work/'variant-registry.json').write_text(json.dumps(registry.manifest(), indent=2)+'\n')
            summary['builds'], summary['initial_load'] = builds, timings
            software = digest({key: builds['generic']['binding'][key] for key in builds['generic']['binding']
                               if key not in ('ir_sha256', 'source_sha256', 'optimization')})
            binding = {'host': host_binding(inventory), 'software': software, 'workload': name+'-v1'}
            policy = RetainedPolicy(work/'trained-profile.json', binding, registry)
            warmup_start = time.perf_counter_ns()
            for variant in sessions:
                record({'workload': name, 'phase': 'warmup', 'variant': variant,
                        **batch(sessions[variant], expected, 3)})
            summary['warmup_ns'] = time.perf_counter_ns()-warmup_start
            profiling_start = time.perf_counter_ns()
            for pair in range(12):
                order = list(sessions)
                rng.shuffle(order)
                costs = {}
                for position, variant in enumerate(order):
                    measured = batch(sessions[variant], expected, 5)
                    record({'workload': name, 'phase': 'tune', 'pair': pair, 'position': position,
                            'variant': variant, **measured})
                    if not measured['semantic_equal']:
                        raise ValueError('semantic mismatch in tuning')
                    costs[variant] = measured['per_invocation_ns']
                    summary['train_costs_ns'][variant].append(costs[variant])
                policy.observe_pair(costs['generic'], costs['optimized'])
                policy.save()
            summary['profiling_ns'] = time.perf_counter_ns()-profiling_start
            summary['trained_decision'] = policy.choose()
            for control in ('frozen', 'active', 'stale', 'shuffled'):
                shutil.copyfile(work/'trained-profile.json', work/(control+'-profile.json'))
            corrupted = json.loads((work/'shuffled-profile.json').read_text())
            corrupted['payload']['costs']['generic'], corrupted['payload']['costs']['optimized'] = (
                corrupted['payload']['costs']['optimized'], corrupted['payload']['costs']['generic'])
            (work/'shuffled-profile.json').write_text(json.dumps(corrupted))
            control_overhead = 0
            for pair in range(20):
                order = list(sessions)
                rng.shuffle(order)
                costs = {}
                for position, variant in enumerate(order):
                    measured = batch(sessions[variant], expected, 10)
                    record({'workload': name, 'phase': 'heldout_pair', 'pair': pair, 'position': position,
                            'variant': variant, **measured})
                    if not measured['semantic_equal']:
                        raise ValueError('semantic mismatch in held-out pair')
                    costs[variant] = measured['per_invocation_ns']
                    summary['heldout_costs_ns'][variant].append(costs[variant])
                controls = list(CONTROLS)
                rng.shuffle(controls)
                for position, control in enumerate(controls):
                    select_start = time.perf_counter_ns()
                    if control in ('generic', 'passive'):
                        selected = {'variant': 'generic', 'reason': control+' control', 'generation': 0}
                        p = None
                    else:
                        binding_control = dict(binding, host='deliberately stale') if control == 'stale' else binding
                        p = RetainedPolicy(work/(control+'-profile.json'), binding_control, registry)
                        selected = p.choose('frozen' if control == 'frozen' else 'active')
                    selection_ns = time.perf_counter_ns()-select_start
                    measured = batch(sessions[selected['variant']], expected, 10)
                    if not measured['semantic_equal']:
                        raise ValueError('semantic mismatch in '+control)
                    update_start = time.perf_counter_ns()
                    if control == 'active':
                        p.observe_pair(costs['generic'], costs['optimized'])
                        p.save()
                    elif control == 'passive':
                        p = RetainedPolicy(work/'passive-profile.json', binding, registry)
                        p.observe_pair(costs['generic'], costs['optimized'])
                        p.save()
                    update_ns = time.perf_counter_ns()-update_start
                    control_overhead += selection_ns+update_ns
                    record({'workload': name, 'phase': 'control', 'pair': pair, 'position': position,
                            'control': control, 'selection': selected, 'selection_ns': selection_ns,
                            'profile_update_ns': update_ns, **measured})
                    summary['controls'].setdefault(control, {'dispatches': [], 'elapsed_ns': [],
                        'selection_ns': [], 'profile_update_ns': []})
                    item = summary['controls'][control]
                    item['dispatches'].append(selected)
                    item['elapsed_ns'].append(measured['elapsed_ns'])
                    item['selection_ns'].append(selection_ns)
                    item['profile_update_ns'].append(update_ns)
            # Causal fixture is separate from performance and uses labelled synthetic costs.
            causal = RetainedPolicy(work/'causal-fixture-profile.json', binding, registry)
            before = causal.choose()
            for _ in range(5):
                causal.observe_pair(100, 70)
            causal.save()
            reload = RetainedPolicy(work/'causal-fixture-profile.json', binding, registry)
            after = reload.choose()
            before_result = sessions[before['variant']].run()
            after_result = sessions[after['variant']].run()
            removed = RetainedPolicy(work/'removed-profile.json', binding, registry).choose()
            summary['causal_fixture'] = {'synthetic_costs_only': True, 'before': before, 'after_reload': after,
                'after_removal': removed, 'before_native_address': before_result['address'],
                'after_native_address': after_result['address'], 'semantic_equal': semantic_outcome(before_result)==semantic_outcome(after_result),
                'influence': before['variant'] != after['variant'] and removed['variant']==before['variant']}
            # Actual restart: load committed evidence in a new process and invoke selected native code.
            restart_request = {'profile': str(work/'causal-fixture-profile.json'), 'binding': binding,
                'registry': registry.manifest(), 'expected': expected, 'source': str(work/'source.dec'),
                'cache': str(work/'cache')}
            (work/'restart-request.json').write_text(json.dumps(restart_request))
            restarted = subprocess.run([sys.executable, str(Path(__file__).resolve()), '--restart-check',
                str(work/'restart-request.json')], capture_output=True, text=True, timeout=60)
            restart_result = {'exit_code': restarted.returncode, 'stdout': restarted.stdout, 'stderr': restarted.stderr}
            if restarted.returncode != 0:
                raise ValueError('restart dispatch failed: '+repr(restart_result))
            restarted_value = json.loads(restarted.stdout)
            if restarted_value['selection']['variant'] != 'optimized' or not restarted_value['semantic_equal']:
                raise ValueError('restart selection/equality failed')
            summary['causal_fixture']['process_restart'] = restarted_value
            # Actual horizon: 20 held-out pairs * 10 invocations; charge preparation once.
            compile_ns = sum(v['total_ns'] for v in builds.values())
            load_ns = sum(v['first_invocation_wall_ns'] for v in timings.values())
            overhead = compile_ns+load_ns+summary['warmup_ns']+summary['profiling_ns']+control_overhead
            held_generic = [v*10 for v in summary['heldout_costs_ns']['generic']]
            held_opt = [v*10 for v in summary['heldout_costs_ns']['optimized']]
            summary['heldout_analysis'] = policy.evaluate_heldout(held_generic, held_opt, overhead)
            policy.save()
            summary['costs'] = {'compilation_ns': compile_ns, 'loading_and_initial_invocation_ns': load_ns,
                'warmup_ns': summary['warmup_ns'], 'profiling_ns': summary['profiling_ns'],
                'profile_control_overhead_ns': control_overhead, 'heldout_generic_ns': sum(held_generic),
                'heldout_optimized_ns': sum(held_opt), 'charged_overhead_ns': overhead,
                'heldout_invocation_horizon': 200}
            summary['post_heldout_decision'] = policy.choose()
            summary['status'] = 'PASS'
        except BaseException as exc:
            summary['status'] = 'FAIL'
            summary['failure'] = repr(exc)
            run['failures'].append({'workload': name, 'error': repr(exc)})
            record({'workload': name, 'phase': 'failure', 'error': repr(exc)})
        finally:
            for session in sessions.values():
                session.close()
            (work/'summary.json').write_text(json.dumps(summary, indent=2)+'\n')
            (output/'results.json').write_text(json.dumps(run, indent=2)+'\n')
    run['finished_utc'] = now()
    run['implementation_binding_unchanged'] = all(sha(ROOT/name)==value for name,value in run['implementation_binding'].items())
    if not run['implementation_binding_unchanged']:
        run['failures'].append({'workload': 'binding', 'error': 'implementation changed during experiment; all observations retained'})
    run['total_experiment_ns'] = time.perf_counter_ns()-started_all
    run['peak_process_rss_kib'] = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    run['functional_controls_pass'] = bool(run['workloads']) and not run['failures'] and all(
        w.get('causal_fixture', {}).get('influence') and w.get('causal_fixture', {}).get('semantic_equal')
        for w in run['workloads'].values())
    run['established_cost_inclusive_benefit'] = run['functional_controls_pass'] and all(
        w['heldout_analysis']['established_benefit'] for w in run['workloads'].values())
    run['conclusion'] = ('Cost-inclusive benefit established on tested host/workloads.'
        if run['established_cost_inclusive_benefit'] else
        'No cost-inclusive performance benefit established; retained policy restores generic.')
    (output/'results.json').write_text(json.dumps(run, indent=2)+'\n')
    raw.close()
    print(json.dumps({'results': str(output/'results.json'), 'functional_controls_pass': run['functional_controls_pass'],
                      'established_cost_inclusive_benefit': run['established_cost_inclusive_benefit'], 'failures': run['failures']}))
    return 0 if run['functional_controls_pass'] else 1

def restart_check(request_path):
    request = json.loads(Path(request_path).read_text())
    source = Path(request['source']).read_text()
    registry = VariantRegistry(hashlib.sha256(source.encode()).hexdigest(), ABI_VERSION, request['expected'])
    sessions = {}
    try:
        for name, opt in [('generic', 0), ('optimized', 2)]:
            session = JitSession(source, opt=opt, cache_dir=request['cache'])
            sessions[name] = session
            actual = session.run()
            obj = Path(session.info['object'])
            artifacts = {path.name: sha(path) for path in (obj, obj.with_suffix('.generic.o'), obj.with_suffix('.guard.o'))}
            registry.validate(name, digest(artifacts), actual)
        policy = RetainedPolicy(request['profile'], request['binding'], registry)
        selection = policy.choose()
        result = sessions[selection['variant']].run()
        response = {'selection': selection, 'result': result,
                    'semantic_equal': semantic_outcome(result) == request['expected']}
        print(json.dumps(response))
        return 0 if response['semantic_equal'] else 1
    finally:
        for session in sessions.values():
            session.close()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--restart-check', type=Path)
    args = parser.parse_args()
    if args.restart_check:
        raise SystemExit(restart_check(args.restart_check))
    if not args.output:
        parser.error('--output or --restart-check required')
    raise SystemExit(execute(args.output.resolve()))
