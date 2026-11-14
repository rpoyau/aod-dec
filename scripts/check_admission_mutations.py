#!/usr/bin/env python3
"""Check integrated discovered tests against isolated in-memory mutations.

No repository bytes are changed. A zero parent exit means the baseline passed
and the declared hostile cases failed under each mutation. Mutant-suite
failures are expected sensitivity evidence, not baseline test failures.
"""
from pathlib import Path
import argparse
import hashlib
import importlib.util
import inspect
import io
import json
import subprocess
import sys
import unittest

EXPECTED_SHA = '918c6c0bd242e7ed0bff6f4d1a4ff7bf2cc840b3da41b6888c218d34eddf5be0'
IDENTITY = {'R04-01-recurrence-event-conflict', 'FIX-01-contact-collection-conflict',
            'R04-02-same-contact-records-accessor'}
REFLECTION = {'FIX-02-boolean-turnaround-cut', 'FIX-03-float-turnaround-cut',
              'FIX-04-boolean-terminal-cut', 'FIX-05-float-terminal-cut'}
EXPECTED = {'baseline': set(), 'identity': IDENTITY, 'reflection': REFLECTION,
            'combined': IDENTITY | REFLECTION}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def child(args):
    repo = args.repo.resolve()
    test_file = args.tests_file.resolve()
    sys.path.insert(0, str(repo))
    sys.path.insert(0, str(test_file.parent))
    from simulations.vendor import aod_manual_native as producer
    actual = Path(producer.__file__).resolve()
    expected = repo / 'simulations/vendor/aod_manual_native.py'
    if actual != expected or sha(actual) != EXPECTED_SHA:
        raise RuntimeError('reviewed producer binding changed; rebind explicitly before running this harness')
    before = sha(actual)
    if args.child in ('reflection', 'combined'):
        guard = ("    require(type(w.turnaround_committed_at) is int and type(w.committed_at) is int,\n"
                 "            'reflection commitments must be exact integer native cuts')\n")
        source = inspect.getsource(producer.validate_trace)
        if source.count(guard) != 1:
            raise RuntimeError('reflection mutation target is not unique')
        exec(source.replace(guard, ''), producer.__dict__)
    if args.child in ('identity', 'combined'):
        producer._check_retained_bindings = lambda traces: None
    spec = importlib.util.spec_from_file_location('review_ported_admission_tests', test_file)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    if module.producer is not producer:
        raise RuntimeError('tests did not use the mutated producer instance')
    suite = unittest.defaultTestLoader.loadTestsFromModule(module)
    transcript = io.StringIO()
    result = unittest.TextTestRunner(stream=transcript, verbosity=2).run(suite)
    def identifier(test):
        return getattr(getattr(test, test._testMethodName), 'feedback_case_id', test.id())
    failures = sorted(identifier(test) for test, _ in result.failures)
    errors = sorted(identifier(test) for test, _ in result.errors)
    after = sha(actual)
    passed = (result.testsRun == 19 and set(failures) == EXPECTED[args.child]
              and not errors and not result.skipped and before == after)
    record = {'mutation': args.child, 'optimized': not __debug__,
              'producer_sha256': before, 'source_bytes_unchanged': before == after,
              'same_producer_instance': True, 'tests_run': result.testsRun,
              'suite_passed': result.wasSuccessful(), 'failure_ids': failures,
              'error_ids': errors, 'skips': len(result.skipped),
              'expected_failure_ids': sorted(EXPECTED[args.child]),
              'acceptance_passed': passed}
    args.output.mkdir(parents=True, exist_ok=True)
    stem = ('optimized' if not __debug__ else 'normal') + '-' + args.child
    (args.output / (stem + '.log')).write_text(transcript.getvalue())
    (args.output / (stem + '.json')).write_text(json.dumps(record, indent=2) + '\n')
    print(json.dumps(record))
    return 0 if passed else 1


def main(args):
    if args.child:
        return child(args)
    args.output = args.output.resolve()
    records = []
    for optimized in (False, True):
        for variant in EXPECTED:
            command = [sys.executable, '-B'] + (['-O'] if optimized else [])
            command += [str(Path(__file__).resolve()), '--repo', str(args.repo.resolve()),
                        '--tests-file', str(args.tests_file.resolve()),
                        '--output', str(args.output), '--child', variant]
            completed = subprocess.run(command, capture_output=True, text=True, timeout=120)
            if completed.returncode:
                print(completed.stdout + completed.stderr, file=sys.stderr)
                return 1
            records.append(json.loads(completed.stdout))
    report = {'all_passed': all(row['acceptance_passed'] for row in records),
              'run_count': len(records), 'records': records,
              'scope': 'Admission regression sensitivity; no compiler or physical accuracy claim'}
    (args.output / 'summary.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'all_passed': report['all_passed'], 'run_count': len(records),
                      'output': str(args.output)}))
    return 0 if report['all_passed'] else 1


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, required=True)
    parser.add_argument('--tests-file', type=Path,
                        default=Path(__file__).resolve().parents[1] / 'tests/test_admission_regressions.py')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--child', choices=tuple(EXPECTED))
    raise SystemExit(main(parser.parse_args()))
