#!/usr/bin/env python3
"""Run pinned AOD manuscript tooling and report its actual coverage.

The upstream tool parses Typst/LaTeX, not Dec, Python, C++ or Markdown. An empty
manuscript inventory is NOT_APPLICABLE, never compiler or scientific certification.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

PIN = '68bc4795e982bbdeb266e346d694b08c7c49c844'
REPOSITORY = 'https://github.com/rpoyau/aod-lint'
ROOT = Path(__file__).resolve().parents[1]


def command(argv, cwd=None):
    proc = subprocess.run(argv, cwd=cwd, capture_output=True, text=True)
    return dict(command=list(map(str, argv)), exit_code=proc.returncode,
                stdout=proc.stdout, stderr=proc.stderr)


def run(checkout, binary, root):
    checkout = checkout.resolve()
    binary = binary.resolve()
    root = root.resolve()
    head = command(['git', 'rev-parse', 'HEAD'], checkout)
    if head['exit_code'] or head['stdout'].strip() != PIN:
        raise ValueError('aod-lint checkout must be pinned to ' + PIN)
    clean = command(['git', 'diff', '--quiet', 'HEAD', '--'], checkout)
    if clean['exit_code']:
        raise ValueError('aod-lint tracked source differs from the pinned commit')
    if not root.is_dir():
        raise ValueError('target root must be an existing directory')
    digest = command([str(binary), 'digest', str(root), '--json'])
    if not digest['stdout'].strip():
        raise ValueError('aod-lint digest returned no manifest: ' + digest['stderr'])
    manifest = json.loads(digest['stdout'])
    files = manifest['files']
    checks = []
    for file in files:
        path = (root / file['path']).resolve()
        if path.suffix not in ('.typ', '.tex') or not path.is_relative_to(root):
            raise ValueError('Unexpected manuscript path from aod-lint digest')
        checks.append(command([str(binary), 'check', str(path), '--strict', '--json']))
    failed = digest['exit_code'] != 0 or any(x['exit_code'] for x in checks)
    lock = checkout / 'Cargo.lock'
    return {
        'repository': REPOSITORY,
        'revision': PIN,
        'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
        'cargo_lock_sha256': hashlib.sha256(lock.read_bytes()).hexdigest() if lock.exists() else None,
        'target': str(root),
        'status': 'FAIL' if failed else ('PASS' if files else 'NOT_APPLICABLE'),
        'scope': 'Typst/LaTeX manuscript diagnostics only',
        'manuscript_files': [f['path'] for f in files],
        'manuscript_file_count': len(files),
        'uncovered_source_types': ['Dec', 'Python', 'C++', 'Rust compiler runtime code', 'Markdown'],
        'interpretation': (
            'No manuscript files were found. No compiler or scientific correctness claim follows.'
            if not files else
            'Only the listed manuscript files were scanned. Lint diagnostics are not a proof of scientific or compiler correctness.'
        ),
        'digest': digest,
        'manifest': manifest,
        'checks': checks,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--checkout', type=Path, required=True)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, default=ROOT)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    try:
        report = run(args.checkout, args.binary, args.root)
        encoded = json.dumps(report, indent=2) + '\n'
        if args.output:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(encoded)
        print(encoded, end='')
        return 1 if report['status'] == 'FAIL' else 0
    except (OSError, ValueError, KeyError, TypeError) as exc:
        print('aod-lint integration: ' + str(exc), file=sys.stderr)
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
