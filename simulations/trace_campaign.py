"""Paired finite-workload profiles; resident state and durable history separated."""
from fractions import Fraction as Q
from pathlib import Path
import gc
import json
import platform
import resource
import sys
import time
import tracemalloc
from native.learning import paired_interval
from simulations.scoped_reduction import (ExactReduction, UpdateDomain, advance,
    aggregate, approximate_step, digest)
from simulations.trace_store import TraceStore, TraceLimits, TraceIncomplete, run_manifest


def json_default(value):
    if type(value) is Q: return {'$ratio':[value.numerator,value.denominator]}
    raise TypeError(type(value).__name__)


def workload(output, *, detail, steps, mode, run_id):
    if type(detail) is not int or not 1 <= detail <= 32 or type(steps) is not int or not 1 <= steps <= 10000:
        raise ValueError('campaign finite graph/step cap exceeded')
    if mode not in ('full','passive','streaming','reduced','bounded_approximate'):
        raise ValueError('unknown execution mode')
    gc.collect(); tracemalloc.start(); start = time.perf_counter_ns()
    store = None
    try:
        setup_start = time.perf_counter_ns()
        row = (Q(1,2*detail),)*detail + (Q(1,2),)
        P = (row,)*(detail+1)
        blocks = (tuple(range(detail)), (detail,))
        domain = UpdateDomain('finite-interior/sensor', (0,steps+1), 'finite-partition', (0,0), 'fixed', (0,), ())
        reducer = ExactReduction(P, blocks, domain)
        setup_ns = time.perf_counter_ns()-setup_start
        kernel_sha = reducer.kernel_sha256
        p = (Q(1),)+(Q(0),)*detail
        q = aggregate(p, blocks)
        retained = []
        l1_bound = Q(0)
        graph_nodes, graph_edges = detail+1, (detail+1)**2
        if mode in ('reduced','bounded_approximate'):
            # Complete finite admission was executed and charged above.
            # No fine kernel or fine distribution remains in the update loop.
            P = None; p = None; row = None; blocks = None
            graph_nodes, graph_edges = 2, 4
        limits = TraceLimits(cache_bytes=65536,max_event_bytes=8192,max_storage_bytes=32*1024*1024,max_integer_bits=1024)
        if mode != 'full':
            store = TraceStore(output, run_manifest(run_id, source={'kernel_sha256':kernel_sha},
                kernel='finite-interior/sensor-lumpable', preparation={'detail':detail,'steps':steps},
                backend='host-exact-rational', scope={'octave_range':[0,0],'family':'finite-partition'}),limits=limits)
        update_ns=0
        for tick in range(steps):
            t0=time.perf_counter_ns()
            if mode == 'bounded_approximate':
                q,l1_bound=approximate_step(q,reducer.quotient,3,l1_bound)
            elif mode == 'reduced':
                q=reducer.step(q,domain=domain,kernel_sha256=kernel_sha)
            else:
                p=advance(p,P); q=aggregate(p,blocks)
            update_ns+=time.perf_counter_ns()-t0
            if mode in ('full','passive'): retained.append(p)
            if store is not None:
                store.append('model.finite_update',{'tick':tick,'block_memory':q,'l1_error_bound':l1_bound},
                    scope_id='finite-interior/sensor',lineage_id='memory',source_ref='simulations/scoped_reduction.py',
                    scale_family='finite-partition')
        # Independent closed-form target for this chosen row-identical kernel.
        target=(Q(1,2),Q(1,2)); observed_l1=sum(abs(a-b) for a,b in zip(q,target))
        if (mode != 'bounded_approximate' and q != target) or observed_l1 > l1_bound:
            raise RuntimeError('output disagreement or violated approximation bound')
        if store is not None:
            store.checkpoint({'tick':steps,'block_memory':q,'domain':vars(domain),'l1_error_bound':l1_bound})
            store.finish(); status=store.status()
        else: status={'coverage':'not_recorded','archive_bytes':0,'metrics':{}}
        if store is not None:
            store.close();store=None
        elapsed=time.perf_counter_ns()-start; _,peak=tracemalloc.get_traced_memory()
        return {'run_id':run_id,'detail':detail,'steps':steps,'mode':mode,'outcome':q,
            'observed_l1':observed_l1,'certified_l1_bound':l1_bound,'total_ns':elapsed,
            'kernel_setup_validation_ns':setup_ns,'update_ns':update_ns,
            'trace_io_ns':status['metrics'].get('io_ns'),
            'python_allocator_high_water_bytes':peak,
            'process_lifetime_rss_high_water_bytes':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss*1024,
            'active_graph_nodes':graph_nodes,'active_graph_edges':graph_edges,
            'retained_full_snapshots':len(retained),'archive':status,
            'detector_state':{'kind':'finite-update-no-closure-detector','nodes':0},
            'index_residency':'SQLite configured page-cache target (not a hard RSS cap); durable B-tree grows on disk',
            'compilation':{'availability':'not_applicable','reason':'host rational specialization'},
            'device_counters':{'availability':'unavailable','value':None},
            'warmup':{'count':0,'protocol':'each repeat includes construction, admission and storage'},
            'scope':'finite nested-state mechanism; no physical octave or celestial labels'}
    finally:
        if store is not None: store.close()
        tracemalloc.stop()


def campaign(output, repetitions=5, sizes=((2,16),(8,32),(16,64))):
    output=Path(output);output.mkdir(parents=True,exist_ok=True)
    import hashlib
    sources={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in
        [Path(__file__),Path(__file__).with_name('scoped_reduction.py'),Path(__file__).with_name('trace_store.py')]}
    rows=[]
    modes=('full','passive','streaming','reduced','bounded_approximate')
    for detail,steps in sizes:
        for repeat in range(repetitions):
            # Rotate paired mode order to reduce order/temperature bias.
            order=modes[repeat%len(modes):]+modes[:repeat%len(modes)]
            for mode in order:
                name=f'd{detail}-n{steps}-r{repeat}-{mode}'
                row=workload(output/name,detail=detail,steps=steps,mode=mode,run_id=name)
                row['repeat']=repeat;rows.append(row)
                (output/'raw.json').write_text(json.dumps(rows,default=json_default,indent=2)+'\n')
    comparisons=[]
    for detail,steps in sizes:
        def times(mode):return [r['total_ns'] for r in rows if r['detail']==detail and r['steps']==steps and r['mode']==mode]
        for mode in modes[1:]:
            comparisons.append({'detail':detail,'steps':steps,'baseline':'full','mode':mode,
                                'inclusive_paired_interval':paired_interval(times('full'),times(mode))})
    report={'format':'aod-dec-profile-campaign-v1','rows':rows,'comparisons':comparisons,
        'source_sha256_at_start':sources,
        'python':sys.version,'platform':platform.platform(),'repetitions':repetitions,
        'rss_scope':'process-lifetime high water, not per-mode incremental RSS',
        'python_memory_scope':'tracemalloc restarted per workload; native allocator/OS caches excluded',
        'caps_scope':'finite graph <=33 nodes, exact integer guard, bounded store/cache; unrestricted core trace/model has separate finite caps and may stop incomplete',
        'octave_campaign':'only admitted computational level 0; source-bound view rejection/refinement tests cover unsupported ranges; no higher-scale dynamics claimed',
        'performance_claim':'measured workload-specific paired total costs only; no universal speedup/memory bound'}
    (output/'results.json').write_text(json.dumps(report,default=json_default,indent=2)+'\n')
    return report
