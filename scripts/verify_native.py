#!/usr/bin/env python3
"""Run repository native conformance, retaining commands and failure logs.

This checks the declared Linux x86-64 core. It does not measure speedup or certify
manuscript claims, self-hosting, or an unexecuted architecture.
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))


def run(output: Path) -> int:
    from native.compiler import LLVM_PATH, llvm

    output = output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    records = []
    environment = dict(os.environ, PYTHONDONTWRITEBYTECODE='1')
    report = {
        'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'platform': platform.platform(),
        'python': sys.version,
        'python_executable': sys.executable,
        'llvm_library': LLVM_PATH,
        'scope': 'Linux x86-64 native core conformance; no performance claim',
        'checks': records,
        'complete': False,
    }

    def persist():
        (output / 'results.json').write_text(json.dumps(report, indent=2) + '\n')

    def step(name, command):
        started = time.perf_counter_ns()
        record = {'name': name, 'command': list(map(str, command)), 'cwd': str(ROOT),
                  'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat()}
        try:
            proc = subprocess.run(command, cwd=ROOT, env=environment,
                                  capture_output=True, text=True, timeout=900)
            text = proc.stdout + '\n' + proc.stderr
            record['exit_code'] = proc.returncode
        except subprocess.TimeoutExpired as exc:
            parts = [exc.stdout or '', exc.stderr or '', str(exc)]
            text = '\n'.join(part.decode('utf-8', errors='replace') if isinstance(part, bytes) else part
                             for part in parts)
            record['exit_code'] = -1
        except OSError as exc:
            text = str(exc)
            record['exit_code'] = -1
        record.update(elapsed_ns=time.perf_counter_ns()-started,
                      status='PASS' if record['exit_code'] == 0 else 'FAIL',
                      log=name+'.log')
        (output / record['log']).write_text(text)
        records.append(record)
        persist()
        print(name + ': ' + record['status'], flush=True)
        return record['exit_code'] == 0

    persist()
    try:
        backend = llvm()  # Includes exact 16.0.6 and executed-target checks.
        report['backend'] = {'version': list(backend.version), 'target': backend.triple,
                             'cpu': backend.cpu, 'features': backend.features,
                             'library_sha256': hashlib.sha256(Path(LLVM_PATH).read_bytes()).hexdigest()}
    except Exception as exc:
        report.update(complete=True, status='FAIL', environment_error=str(exc))
        persist()
        print('Backend prerequisite failed: ' + str(exc), file=sys.stderr)
        return 1

    step('cxx-version', ['g++', '--version'])
    step('rust-version', ['rustc', '--version'])
    step('python-normal', [sys.executable, '-m', 'unittest', 'discover', '-s', 'tests', '-v'])
    step('python-optimized', [sys.executable, '-O', '-m', 'unittest', 'discover', '-s', 'tests', '-v'])
    step('legacy-rust', ['make', 'verify-rust'])
    for name, opt in [('debug', '-O0'), ('optimized', '-O3')]:
        binary = output / ('runtime-'+name)
        built = step('runtime-'+name+'-build', ['g++', '-std=c++17', opt,
                     str(ROOT/'native/test_runtime.cpp'), '-lgmpxx', '-lgmp', '-lcrypto', '-o', str(binary)])
        if built:
            step('runtime-'+name, [str(binary)])
    library = output / 'libruntime-check.so'
    if step('runtime-differential-build', ['g++', '-std=c++17', '-O2', '-fPIC', '-shared',
            str(ROOT/'native/runtime.cpp'), '-lgmpxx', '-lgmp', '-lcrypto', '-o', str(library)]):
        step('runtime-differential', [sys.executable, 'native/check_runtime.py', str(library),
                                     '--output', str(output/'runtime-differential.json')])
    step('source-conformance', [sys.executable, 'scripts/conformance_native.py',
                               '--output', str(output/'conformance')])
    report.update(complete=True, status='PASS' if all(x['status'] == 'PASS' for x in records) else 'FAIL',
                  finished_utc=datetime.datetime.now(datetime.timezone.utc).isoformat())
    persist()
    return 0 if report['status'] == 'PASS' else 1


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT/'build/verification')
    raise SystemExit(run(parser.parse_args().output))
