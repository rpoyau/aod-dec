#!/usr/bin/env python3
"""Measure active-field scheduling separately from cold witness admission."""
import argparse
from dataclasses import asdict
from fractions import Fraction as Q
import hashlib
import json
import math
from pathlib import Path
import sys
from tempfile import TemporaryDirectory
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from simulations.field_frontier import FieldFrontier, IndexedFieldQueue, WitnessArchive
from simulations.vendor import aod_manual_native as native


def preparation(label, interior):
    if not interior:
        return native.recurrent(label)
    inner = tuple(native.recurrent(label+':inner:'+str(i)) for i in range(interior))
    records = []
    for occurrence in range(2):
        trace = native.outer_trace(inner, lineage=label, occurrence=occurrence)
        certificate = native.certify(trace, 'outer', inner=inner)
        records.append((trace, certificate, {certificate.certificate_digest:certificate}))
    return native.wave(records)


def check(condition, message):
    if not condition:
        raise ValueError(message)


def run_case(count, interior, steps):
    with TemporaryDirectory() as directory:
        started = time.perf_counter()
        archive = WitnessArchive(Path(directory))
        frontier = FieldFrontier(archive, max_fields=count)
        queue = IndexedFieldQueue(max_fields=count)
        states = {}
        for i in range(count):
            wave = preparation('field:'+str(i), interior)
            summary = frontier.admit(wave, window=(0,10000), coupling_id='fixed-fixture',
                                     pressure=Q(3), current=Q(1), field_weight=Q(1))
            states[summary.identity] = summary
            queue.register(summary.identity)
        del wave
        cold_seconds = time.perf_counter()-started
        active_bytes = sum(len(json.dumps(asdict(s), default=str, sort_keys=True).encode())
                           for s in states.values())
        reads_before = archive.metrics['archive_reads']
        source = hashlib.sha256(b'declared exact attention=1/3 in check_field_frontiers.py').hexdigest()
        started = time.perf_counter()
        events = 0
        for _ in range(steps):
            for identity, summary in states.items():
                queue.schedule(identity, summary.committed+1)
            while len(queue):
                event = queue.pop()
                old = states[event.identity]
                attention = frontier.bind_attention(old, Q(1,3), source_sha256=source)
                new = frontier.advance(old, attention, window=old.window,
                                       coupling_id=old.coupling_id, scale=old.scale)
                check(new.committed == event.cut, 'scheduler and committed response differ')
                states[event.identity] = new
                events += 1
        hot_seconds = time.perf_counter()-started
        reads_after = archive.metrics['archive_reads']
        check(reads_before == reads_after, 'hot response reopened an interior')
        check(events == count*steps and frontier.active_count == count, 'active field/event count')
        check(queue.metrics['peak_pending'] <= count, 'pending queue grew beyond active fields')
        check(all(s.current == Q(1)+Q(steps,9) for s in states.values()), 'independent response answer')
        # Exposure is deliberately outside the hot-loop timing and does not
        # claim that response-only steps generated any new native closure.
        started = time.perf_counter()
        recovered = frontier.expose(next(iter(states.values())))
        exposure_seconds = time.perf_counter()-started
        check(recovered == preparation('field:0', interior), 'interior exposure changed evidence')
        archive_bytes = sum(p.stat().st_size for p in Path(directory).rglob('*') if p.is_file())
        archive.close()
        return {'active_fields':count,'inner_waves_per_field':interior,'response_events':events,
                'cold_preparation_admission_archive_seconds':cold_seconds,
                'hot_schedule_and_response_seconds':hot_seconds,
                'cold_plus_hot_seconds':cold_seconds+hot_seconds,
                'one_field_exposure_seconds':exposure_seconds,
                'cold_hot_one_exposure_seconds':cold_seconds+hot_seconds+exposure_seconds,
                'active_summary_json_bytes':active_bytes,'archive_bytes':archive_bytes,
                'hot_archive_reads':reads_after-reads_before,'exposure_verified':True,
                'queue':dict(queue.metrics),
                'comparisons_per_event_log2_n':queue.metrics['comparisons']/(events*math.log2(max(2,count))),
                'independent_exact_response':'PASS'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--counts', type=int, nargs='+', default=(8,32,128))
    parser.add_argument('--steps', type=int, default=4)
    args = parser.parse_args()
    check(all(0<n<=2048 for n in args.counts) and 0<args.steps<=64, 'bounded benchmark counts required')
    records = [run_case(n,0,args.steps) for n in args.counts]
    interior = [run_case(8,k,args.steps) for k in (1,4,16)]
    paths = ('scripts/check_field_frontiers.py','simulations/field_frontier.py',
             'simulations/vendor/aod_manual_native.py','simulations/trace_support.py',
             'simulations/trace_history_union.py')
    report = {'status':'PASS','python':sys.version,'mode':'optimized' if sys.flags.optimize else 'normal',
              'scaling':records,'interior_variation':interior,
              'source_sha256':{p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in paths},
              'scope':'Exact supplied-attention response and indexed scheduler; n counts active fields. '
                      'Cold admission/archive and hot response measured separately and summed. '
                      'Attention generation, arbitrary coupling graphs and full response journaling are outside this fixture. '
                      'Operation counts test scheduler scaling; timings are not a whole-engine speedup claim.'}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({'status':report['status'],'cases':len(records)+len(interior),
                      'mode':report['mode']}))


if __name__ == '__main__':
    main()
