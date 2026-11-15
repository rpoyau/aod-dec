#!/usr/bin/env python3
"""Run the predeclared reduced SPARC response, controls and exact boundary audits."""
import argparse
from concurrent.futures import ProcessPoolExecutor
from contextlib import nullcontext
import csv
from dataclasses import asdict, is_dataclass
from fractions import Fraction as Q
import gzip
import hashlib
import json
from pathlib import Path
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from simulations.sparc_inputs import load_inputs
from simulations.sparc_inverse import baseline_projections
from simulations.sparc_comparison import compare
from simulations.sparc_reduced import simulate_galaxy, MODEL, MAIN_MANUAL
from simulations.vendor import aod_manual_native as native

SOURCE_PATHS = ('scripts/simulate_sparc_reduced.py','simulations/sparc_reduced.py',
    'simulations/reduced_duon_history.py','simulations/reduced_boundary_response.py',
    'simulations/sparc_inputs.py','simulations/sparc_inverse.py','simulations/sparc_comparison.py',
    'simulations/sparc_native_span.py','simulations/region_engine.py',
    'simulations/boundary_accounting.py','simulations/vendor/aod_manual_native.py',
    'simulations/vendor/aod_trace_digest_cache.py')


def source_bindings():
    return {path:hashlib.sha256((ROOT/path).read_bytes()).hexdigest() for path in SOURCE_PATHS}


def encode(value):
    if is_dataclass(value):
        return encode(asdict(value))
    if isinstance(value, Q):
        return str(value)
    if isinstance(value, dict):
        return {str(k):encode(v) for k,v in value.items()}
    if isinstance(value,(tuple,list)):
        return [encode(v) for v in value]
    return value


def packed(value):
    return json.dumps(encode(value),sort_keys=True,separators=(',',':'),allow_nan=False).encode()


def _galaxy_task(item):
    rows, use_cache = item
    if use_cache:
        from simulations.vendor.aod_trace_digest_cache import trace_digest_cache
        context = trace_digest_cache(native)
    else:
        context = nullcontext()
    with context:
        result = {}
        for case, options in (('spacing', {}), ('disabled', {'coupled':False}),
                              ('unit', {'geometry':'unit'})):
            started = time.monotonic()
            print(f"{rows[0].key.galaxy} {case}: starting", file=sys.stderr, flush=True)
            result[case] = simulate_galaxy(rows, **options)
            print(f"{rows[0].key.galaxy} {case}: completed in {time.monotonic()-started:.1f}s",
                  file=sys.stderr, flush=True)
        return result


def execute(input_directory, *, workers=1, use_cache=True):
    if type(workers) is not int or workers < 1:
        raise ValueError('positive exact worker count required')
    binding = source_bindings()
    data = load_inputs(input_directory)
    galaxies = sorted({key.galaxy for key in data.preparation})
    tasks = [(tuple(row for key,row in sorted(data.preparation.items()) if key.galaxy==galaxy),use_cache)
             for galaxy in galaxies]
    if workers == 1:
        results = list(map(_galaxy_task,tasks))
    else:
        with ProcessPoolExecutor(max_workers=min(workers,len(tasks))) as executor:
            results = list(executor.map(_galaxy_task,tasks))
    runs = {case:tuple(item[case] for item in results) for case in ('spacing','disabled','unit')}
    source_evidence = tuple(f'{Path(s.path).name}:sha256:{s.sha256}' for s in data.sources)
    baseline = compare(data.targets,baseline_projections(data.preparation,source_evidence=source_evidence))
    comparisons = {case:compare(data.targets,tuple(p for run in family for p in run['projections']))
                   for case,family in runs.items()}
    if any(a['u_projected'] != b['u_projected'] for a,b in
           zip(baseline['rows'],comparisons['disabled']['rows'])):
        raise ValueError('uncoupled control failed to preserve every frozen G0 value')
    summaries = {}
    for case,family in runs.items():
        comparison = comparisons[case]
        summaries[case] = {'mape_percent':comparison['primary_mape_percent'],
            'equal_galaxy_mape_percent':comparison['equal_galaxy_mape_percent'],
            'change_from_g0_percentage_points':comparison['primary_mape_percent']-baseline['primary_mape_percent'],
            'bins':comparison['bins'],'galaxies':comparison['galaxies'],
            'moving_boundary_cuts':sum(run['moving_boundary_cuts'] for run in family),
            'response_steps':{run['galaxy']:run['response_steps'] for run in family},
            'boundary_balance_residual':max(abs(row['boundaries'][name]['balance_residual'])
                for run in family for row in run['transitions'] for name in row['boundaries']),
            'global_stock_residual':max(abs(run['final_total_stock']-run['initial_total_stock']) for run in family),
            'total_reclosures_in_retained_history':sum(len(wave.returns)-1 for run in family for wave in run['native_history'].waves),
            'execution_reclosures':sum(len(wave.returns)-3 for run in family for wave in run['native_history'].waves),
            'channel_occurrences':sum(len(row['channels']) for run in family for row in run['transitions'])}
    if source_bindings() != binding:
        raise RuntimeError('implementation changed during execution; refusing mixed-source evidence')
    return {'format':MODEL,'main_manual_commit':MAIN_MANUAL,'implementation_sha256':binding,
        'source_files':tuple({'name':Path(s.path).name,'sha256':s.sha256,'rows':len(s.rows)} for s in data.sources),
        'case_roles':{'spacing':'predeclared primary, no target fitting',
                      'disabled':'matched response-off control',
                      'unit':'geometry ablation, not selected by residual'},
        'baseline':baseline,'comparisons':comparisons,'summaries':summaries,'runs':runs,
        'scope':'executed reduced fixed-support native response with moving control boundaries; no complete G2/G3 formation claim',
        'unknowns':('unobserved radial/out-of-plane exchange','physical current/density/clock calibration',
                    'formation/decoupling/shedding outside the fixed-support specialization',
                    'reduction and observation-map discrepancy')}


def write_outputs(result,output):
    output.mkdir(parents=True,exist_ok=True)
    raw = packed(result)
    (output/'trace.json.gz').write_bytes(gzip.compress(raw,compresslevel=6,mtime=0))
    summary = {key:result[key] for key in ('format','main_manual_commit','implementation_sha256',
        'source_files','case_roles','summaries','scope','unknowns')}
    summary['baseline_mape_percent'] = result['baseline']['primary_mape_percent']
    summary['display'] = {case:{'mape_percent':f"{float(row['mape_percent']):.12f}",
        'equal_galaxy_mape_percent':f"{float(row['equal_galaxy_mape_percent']):.12f}",
        'change_percentage_points':f"{float(row['change_from_g0_percentage_points']):+.12f}"}
        for case,row in result['summaries'].items()}
    summary['display']['baseline_mape_percent'] = f"{float(summary['baseline_mape_percent']):.12f}"
    summary['trace_uncompressed_sha256'] = hashlib.sha256(raw).hexdigest()
    summary['trace_compressed_sha256'] = hashlib.sha256((output/'trace.json.gz').read_bytes()).hexdigest()
    summary['trace_uncompressed_bytes'] = len(raw)
    (output/'summary.json').write_text(json.dumps(encode(summary),indent=2,sort_keys=True)+'\n')
    with (output/'comparison.csv').open('w',newline='') as stream:
        fields=('case','galaxy','radius_kpc','u_observed','u_projected','signed_residual',
                'signed_relative_residual','absolute_percentage_error')
        writer=csv.DictWriter(stream,fieldnames=fields,lineterminator='\n');writer.writeheader()
        for case,comparison in result['comparisons'].items():
            for row in comparison['rows']:
                writer.writerow(encode({'case':case,**row['key'],**{k:row[k] for k in fields[3:]}}))
    report=['# Reduced SPARC native-current execution','',result['scope']+'.','',
        '| Case | Equal-bin square-speed MAPE | Change from G0 |','|---|---:|---:|',
        f"| Frozen G0 | {float(summary['baseline_mape_percent']):.12f}% | 0 pp |"]
    for case,row in summary['display'].items():
        if case=='baseline_mape_percent':continue
        report.append(f"| {case} | {row['mape_percent']}% | {row['change_percentage_points']} pp |")
    report += ['', f"Every case retains all {result['baseline']['bins']} bins. The primary case was fixed before scoring; the other cases are controls.",
        '', 'The native response, actual representative returns, directional contact evidence, state trajectories, '
        'current occurrences and moving-boundary audit operands are recoverable from trace.json.gz. '
        'No observed speed enters the evolution. The disabled case must reproduce every G0 prediction exactly.',
        '', 'The closed radial and out-of-plane ports are explicit reduction assumptions. Their actual exchange is unknown. '
        'There is no separate shedding channel in the fixed-support case; no physical absence of shedding is inferred. '
        'Physical mass, native response stock and the projected support coefficient retain separate types.',
        '', 'An error reduction would be conditional on this finite specialization and external map. '
        'It would not establish a complete three-dimensional galaxy model or a measured physical time evolution.','']
    (output/'REPORT.md').write_text('\n'.join(report))
    return summary


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input',type=Path,default=ROOT/'data/sparc')
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--workers',type=int,default=1)
    parser.add_argument('--no-digest-cache',action='store_true')
    parser.add_argument('--replay',type=Path,help='recompute and compare the complete reopened compressed trace')
    args=parser.parse_args();started=time.monotonic()
    result=execute(args.input,workers=args.workers,use_cache=not args.no_digest_cache)
    if args.replay:
        prior=json.loads(gzip.decompress(args.replay.read_bytes()))
        if prior!=encode(result):raise SystemExit('reopened trace differs from complete deterministic recomputation')
    summary=write_outputs(result,args.output)
    execution={'workers':args.workers,'digest_cache':not args.no_digest_cache,
        'elapsed_seconds':time.monotonic()-started,'replay':'PASS' if args.replay else 'not_requested'}
    (args.output/'execution.json').write_text(json.dumps(execution,indent=2)+'\n')
    print(json.dumps({'display':summary['display'],'execution':execution}),flush=True)


if __name__=='__main__':
    main()
