#!/usr/bin/env python3
"""Compare frozen and optimized traces, allowing only implementation bindings to change."""
import argparse
import gc
import gzip
import hashlib
import json
from pathlib import Path


def inspect(path):
    with gzip.open(path, 'rt') as stream:
        data = json.load(stream)
    bindings = data.pop('implementation_sha256')
    digest = hashlib.sha256()
    for piece in json.JSONEncoder(sort_keys=True, separators=(',', ':'), allow_nan=False).iterencode(data):
        digest.update(piece.encode())
    result = {'semantic_sha256': digest.hexdigest(), 'implementation_sha256': bindings,
              'compressed_sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
    del data
    gc.collect()
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before', type=Path, required=True)
    parser.add_argument('--after', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    before, after = inspect(args.before), inspect(args.after)
    changed = sorted(key for key in before['implementation_sha256'].keys() | after['implementation_sha256'].keys()
                     if before['implementation_sha256'].get(key) != after['implementation_sha256'].get(key))
    csv_equal = args.before.with_name('comparison.csv').read_bytes() == args.after.with_name('comparison.csv').read_bytes()
    passed = before['semantic_sha256'] == after['semantic_sha256'] and csv_equal
    report = {'status': 'PASS' if passed else 'FAIL', 'before': before, 'after': after,
              'comparison_csv_byte_identical': csv_equal, 'changed_implementation_paths': changed,
              'scope': 'All trace data except the top-level implementation_sha256 map must match exactly. '
                       'Production replay retains strict whole-trace equality; it has not been relaxed.'}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'status': report['status'], 'changed_implementation_paths': changed}))
    if not passed:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
