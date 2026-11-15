#!/usr/bin/env python3
"""Execute native boundary fixtures and compare complete serial, CPU and CUDA trace views."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from simulations import region_engine as region
from simulations import solar_boundary_fixture as solar
from simulations.chiral_gpu import GPUUnavailable
from simulations.trace_applications import run_regions
from simulations.trace_execution import boundary_views
from simulations.trace_store import TraceStore, run_manifest
from simulations.trace_support import detect_particle_support
from simulations.trace_particle_relations import detect_particle_relations
from simulations.trace_paths import detect_sheddic_paths


def require(condition, message):
    if not condition:
        raise ValueError(message)


def dump(path, value):
    path.write_text(json.dumps(region.serial(value), sort_keys=True, indent=2, allow_nan=False)+'\n')


def source_bindings():
    paths = ('scripts/check_parallel_tracing.py','simulations/solar_boundary_fixture.py',
        'simulations/trace_execution.py','simulations/trace_parallel.py','simulations/trace_detectors.py',
        'simulations/boundary_accelerator.py','simulations/kernels/boundary_incidence.cu',
        'simulations/chiral_gpu.py','simulations/region_engine.py','simulations/boundary_accounting.py',
        'simulations/trace_store.py','simulations/trace_applications.py','simulations/run_regions.py',
        'simulations/trace_support.py','simulations/trace_particle_relations.py',
        'simulations/trace_paths.py','simulations/trace_history_union.py',
        'runtime/dec_core.py','simulations/vendor/aod_manual_native.py')
    return {p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in paths}


def semantic(result):
    return {key:result[key] for key in ('boundaries','archive_coverage','coverage','domain','coverage_scope')}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--workers', type=int, default=4)
    parser.add_argument('--gpu', choices=('auto','required','off'), default='auto')
    args = parser.parse_args()
    require(type(args.workers) is int and 2 <= args.workers <= 32, 'multicore check requires 2..32 workers')
    binding = source_bindings()
    args.output.mkdir(parents=True, exist_ok=True)
    archive = args.output/'trace'
    manifest = run_manifest('parallel-native-boundaries', source=binding,
        kernel='existing-native-pressure-current-and-boundary-accounting',
        preparation='declared maser, Earth-water-Moon and Sun-Mercury native boundary fixtures',
        backend='host-native-formation-and-single-trace-writer')
    started = time.perf_counter()
    with TraceStore(archive, manifest) as store:
        regions = run_regions(store)
        solar_result = solar.run(store)
        store.finish()
        before = store.status()
        native = {'support':detect_particle_support(store),
                  'relations':detect_particle_relations(store), 'paths':detect_sheddic_paths(store)}
    preparation_seconds = time.perf_counter()-started
    dump(args.output/'preparation.json', {'regions':regions,'solar':solar_result})
    dump(args.output/'native-detection.json', native)
    expected_keys = {item['ledger']['scope']+'/'+name
                     for item in regions['examples'] for name in item['audits']}
    expected_keys.update(solar_result['ledger'].scope+'/'+name for name in solar_result['audits'])
    with TraceStore.open(archive) as store:
        solar_detection = solar.inspect_trace(store)
    dump(args.output/'solar-detection.json', solar_detection)
    require(solar_detection['status'] == 'verified' and solar_detection['coverage'] == 'complete',
            'reopened solar route/current/flip evidence did not verify')
    reports, timings = {}, {}
    for mode, workers in (('scalar',1),('multicore',args.workers)):
        started = time.perf_counter()
        reports[mode] = boundary_views(archive, workers=workers, backend='scalar',
                                      coverage_scope='declared-current')
        timings[mode] = time.perf_counter()-started
        dump(args.output/(mode+'.json'), reports[mode])
    expected = semantic(reports['scalar'])
    require(set(expected['boundaries']) == expected_keys, 'boundary views omitted or added a prepared group')
    require(expected['coverage_scope'] == 'declared-current', 'declared-current query changed its scope')
    require(expected == semantic(reports['multicore']), 'multicore changed complete boundary trace views')
    execution = reports['multicore']['execution']
    pids = execution['worker_pids']
    require(execution['backend'] == 'multicore' and len(set(pids)) >= 2 and
            all(type(pid) is int and pid > 0 and pid != os.getpid() for pid in pids),
            'multicore check requires observed participation by distinct child processes')
    require(expected['coverage'] == 'complete', 'declared signed-current family lacks complete boundary coverage')
    full = boundary_views(archive, workers=1, backend='scalar', coverage_scope='full')
    require(set(full['boundaries']) == expected_keys and full['coverage_scope'] == 'full'
            and full['coverage'] == 'partial', 'full query changed group coverage or scope')
    unresolved_keys = {key for key,row in full['boundaries'].items() if row['coverage'] != 'complete'}
    require(unresolved_keys == {solar.SCOPE+'/selected',solar.SCOPE+'/complement'},
            'full query did not preserve exactly the two known solar projection gaps')
    unresolved = [r for r in full['boundaries'].values() if r['coverage'] != 'complete']
    require(bool(unresolved) and all('unresolved signed shedding projection' in r.get('reason','')
                                    for r in unresolved), 'full coverage must retain the known burden/current gap')
    dump(args.output/'full-current.json', full)
    gpu = {'status':'NOT_REQUESTED','gpu_executed':False}
    if args.gpu != 'off':
        started = time.perf_counter()
        try:
            reports['gpu'] = boundary_views(archive, workers=1, backend='gpu',
                                           coverage_scope='declared-current')
        except GPUUnavailable as exc:
            gpu = exc.report
            if args.gpu == 'required':
                dump(args.output/'gpu-unavailable.json', gpu)
                raise
        else:
            require(expected == semantic(reports['gpu']), 'CUDA changed complete boundary trace views')
            device_jobs = [job['profile'] for job in reports['gpu']['execution']['jobs']
                           if job['profile'].get('execution',{}).get('gpu_executed')]
            require(bool(device_jobs), 'GPU backend returned no device-executed boundary job')
            gpu = {'status':'PASS','gpu_executed':True, 'executed_jobs':len(device_jobs),
                'launch_count':sum(p['execution']['launch_count'] for p in device_jobs),
                'device':device_jobs[0]['execution']['device'],
                'scope':'device boundary classification with exact host assembly and validation'}
            dump(args.output/'gpu.json', reports['gpu'])
        timings['gpu_attempt'] = time.perf_counter()-started
    with TraceStore.open(archive) as store:
        after = store.status()
    require((before['events'],before['chain']) == (after['events'],after['chain']),
            'parallel replay changed the authoritative trace')
    require(source_bindings() == binding, 'source changed during tracing check')
    result = {'status':'PASS','python':sys.version,'source_sha256':binding,
        'archive_before':before,'archive_after':after,
        'boundary_groups':len(expected['boundaries']),
        'coverage_scope':'declared-current', 'full_current_coverage':full['coverage'],
        'full_current_unresolved_groups':len(unresolved),
        'serial_multicore_complete_equality':True,'gpu':gpu,'workers':args.workers,
        'multicore_participating_pids':reports['multicore']['execution']['worker_pids'],
        'multicore_participating_processes':len(reports['multicore']['execution']['worker_pids']),
        'timing_seconds':{'formation_and_trace_and_native_detection':preparation_seconds,**timings},
        'solar_summary':solar_result['summary'],
        'solar_detection':solar_detection,
        'scope':'Actual retained boundary trace replay. Hardware execution does not add a physical clock, '
                'a flip probability or an orbital observable map. Timings include the measured pipeline; '
                'no speedup threshold or whole-engine acceleration claim.'}
    dump(args.output/'results.json', result)
    print(json.dumps({'status':'PASS','boundary_groups':result['boundary_groups'],
                      'gpu':gpu['status'],'output':str(args.output)}))


if __name__ == '__main__':
    main()
