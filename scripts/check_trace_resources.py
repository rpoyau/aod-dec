#!/usr/bin/env python3
"""Measure an open, unsummarized core history under explicit execution budgets.

The driver stops before creating an event beyond its retained graph budget.
It does not claim that arbitrary callers of the immutable core API are bounded.
"""
from pathlib import Path
from dataclasses import asdict
import argparse
import hashlib
import json
import resource
import sys
import time
import tracemalloc
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from runtime import dec_core as core
from simulations.trace_store import TraceStore,TraceLimits,TraceIncomplete,run_manifest,record_commits
from simulations.trace_detectors import CoreDetector


def run_case(path, cap, requested=10000):
    if type(cap) is not int or not 4<=cap<=512 or type(requested) is not int or not cap<requested<=100000:
        raise ValueError('bounded campaign domain required')
    detector=CoreDetector(max_events=cap//2,max_bytes=131072,max_lineages=1)
    start=time.perf_counter_ns();tracemalloc.start()
    store=TraceStore(path,run_manifest('open-history-'+str(cap),source={'path':'runtime/dec_core.py'},
        kernel='lawful-open-core-branch',preparation={'retained_node_cap':cap,'requested_events':requested}),
        limits=TraceLimits(max_event_bytes=262144,max_storage_bytes=8*1024*1024,cache_bytes=65536))
    try:
        with record_commits(store):
            t=core.new_trace('open','A',core.Q4(0))
            t=core.append_event(t,'step',core.Q4(1),1)
            while len(t.events)<requested:
                if len(t.events)+1>cap:
                    store.checkpoint({'trace':asdict(t),'next_event':len(t.events)+1,
                                      'reason':'no admitted summary; increase declared execution budget to continue'})
                    store._failure(TraceIncomplete('active retained-history node budget reached before next allocation'))
                    break
                t=core.append_event(t,'dwell')
        for event in store.iter_events():detector.consume(event)
        report=detector.inspect();status=store.status()
        store.close();store=None
        _,peak=tracemalloc.get_traced_memory();elapsed=time.perf_counter_ns()-start
    finally:
        if store is not None:store.close()
        tracemalloc.stop()
    with TraceStore.open(path) as reopened:
        checkpoint=reopened.load_checkpoint()
        restored=checkpoint['state']['trace']
        recoverable_events=len(restored['events'])
        if recoverable_events!=cap or reopened.status()['coverage']!='partial':
            raise RuntimeError('incomplete history was lost or falsely completed')
    if status['coverage']!='partial' or report['coverage']!='partial' or len(t.events)!=cap:
        raise RuntimeError('budget did not stop with explicit incomplete result')
    return {'cap':cap,'requested_events':requested,'executed_events':len(t.events),
        'canonical_trace_events':status['events'],'not_executed':requested-len(t.events),
        'retained_graph_nodes':len(t.events)+1,'graph_node_definition':'one retained start plus capped transition records',
        'node_cap_including_start':cap+1,'graph_edges':len(t.events),'unique_q4_vertices':2,
        'allocator_high_water_bytes':peak,'rss_process_high_water_bytes':resource.getrusage(resource.RUSAGE_SELF).ru_maxrss*1024,
        'total_ns':elapsed,'store':status,'detector':report,
        'recovered_checkpoint_event_count':recoverable_events,
        'result':'explicit_incomplete_no_admissible_summary','discarded_required_witnesses':0}


def run(output):
    output=Path(output).resolve();output.mkdir(parents=True,exist_ok=True)
    rows=[run_case(output/f'cap-{cap}',cap) for cap in (16,64,256)]
    report={'status':'PASS','cases':rows,
            'scope':'bounded open-history driver with no admissible summary; arbitrary caller graph and hard process RSS remain outside this driver guarantee',
            'source_sha256':{str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in
                [Path(__file__),ROOT/'runtime/dec_core.py',ROOT/'simulations/trace_store.py',ROOT/'simulations/trace_detectors.py']}}
    (output/'results.json').write_text(json.dumps(report,indent=2)+'\n')
    return report


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path,required=True)
    r=run(parser.parse_args().output);print(json.dumps({'status':r['status'],'cases':len(r['cases'])}))
