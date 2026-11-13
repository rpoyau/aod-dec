#!/usr/bin/env python3
"""Recompute paired intervals from saved raw observations; do not rerun workloads."""
import argparse
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from native.learning import paired_interval


def analyze(directory):
    directory = Path(directory)
    report = json.loads((directory/'results.json').read_text())
    rows = [json.loads(line) for line in (directory/'observations.jsonl').read_text().splitlines()]
    result = {'input': str(directory), 'workloads': {}, 'failures_retained': report['failures']}
    for name, summary in report['workloads'].items():
        if summary.get('status') != 'PASS':
            result['workloads'][name] = {'status': summary.get('status'), 'failure': summary.get('failure')}
            continue
        records = [row for row in rows if row.get('workload') == name and row.get('phase') == 'heldout_pair']
        costs = {variant: [row['elapsed_ns'] for row in sorted(records, key=lambda r:r['pair'])
                          if row['variant'] == variant] for variant in ('generic', 'optimized')}
        if any(len(values) != report['heldout_pairs'] for values in costs.values()):
            raise ValueError('unexpected heldout count for '+name)
        if not all(row['semantic_equal'] and row['addresses'] for row in records):
            raise ValueError('missing semantics or native addresses for '+name)
        # Harness quantizes each per-invocation integer cost before summing a batch.
        costs = {variant: [row['per_invocation_ns']*row['invocations']
                          for row in sorted(records,key=lambda r:r['pair']) if row['variant']==variant]
                 for variant in ('generic','optimized')}
        interval = paired_interval(costs['generic'], costs['optimized'])
        for key in ('pairs', 'median_gain', 'lower', 'upper'):
            if interval[key] != summary['heldout_analysis'][key]:
                raise ValueError('analysis mismatch '+name+'/'+key)
        overhead = summary['costs']['charged_overhead_ns']
        gain = sum(costs['generic'])-sum(costs['optimized'])-overhead
        controls = [row for row in rows if row.get('workload')==name and row.get('phase')=='control']
        if len(controls) != 7*report['heldout_pairs'] or not all(row['semantic_equal'] for row in controls):
            raise ValueError('control count/equivalence mismatch '+name)
        for row in controls:
            if row['control'] in ('generic','passive','stale','shuffled','removed') and row['selection']['variant'] != 'generic':
                raise ValueError('incorrect guarded fallback '+name+'/'+row['control'])
        result['workloads'][name] = dict(interval, total_gain_ns=gain,
            established_benefit=interval['lower'] > 0.05 and gain>0,
            controls_equal=True, status='PASS')
    result['status'] = 'PASS' if not result['failures_retained'] else 'FAIL'
    return result

if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory',type=Path)
    parser.add_argument('--output',type=Path)
    args=parser.parse_args()
    result=analyze(args.directory)
    encoded=json.dumps(result,indent=2)+'\n'
    if args.output: args.output.write_text(encoded)
    print(encoded,end='')
