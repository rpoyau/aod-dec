#!/usr/bin/env python3
"""Run, inspect and replay source-bound D.E.C. host traces."""
from pathlib import Path
from dataclasses import asdict, is_dataclass
import argparse
import hashlib
import json
import sys
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT))
from simulations.trace_execution import run_core, replay, boundary_views
from simulations.trace_store import TraceStore, run_manifest, TraceError, canonical_bytes
from simulations.trace_views import Window, select_events, Span, FractalCoordinate
from simulations.trace_campaign import campaign, json_default
from simulations.scoped_reduction import finite_scope
from simulations.trace_applications import run_regions, run_particles, application_gaps
from simulations.trace_support import detect_particle_support
from simulations.trace_particle_relations import detect_particle_relations
from simulations.trace_encounter import replay_encounter
from simulations.trace_paths import detect_sheddic_paths
from simulations.chiral_gpu import GPUUnavailable


def write(value, output=None):
    def encode(item):
        return asdict(item) if is_dataclass(item) else json_default(item)
    encoded=json.dumps(value,default=encode,indent=2,allow_nan=False)+'\n'
    if output:
        path=Path(output);path.parent.mkdir(parents=True,exist_ok=True);path.write_text(encoded)
    else: print(encoded,end='')


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    sub=parser.add_subparsers(dest='command',required=True)
    run=sub.add_parser('run');run.add_argument('--example',choices=['closure','fusion','coupling','regions','particles','solar-boundaries'],required=True)
    run.add_argument('--trace',type=Path,required=True);run.add_argument('--output',type=Path);run.add_argument('--profile',type=Path)
    replay_parser=sub.add_parser('replay');replay_parser.add_argument('trace',type=Path);replay_parser.add_argument('--output',type=Path)
    support=sub.add_parser('support',help='infer native constructions from retained particle trace evidence')
    support.add_argument('trace',type=Path);support.add_argument('--scope');support.add_argument('--output',type=Path)
    support.add_argument('--record-window',type=int,help='optional native count window view; does not prescribe a detected motif')
    support.add_argument('--start',type=int,help='native committed-cut start, inclusive')
    support.add_argument('--stop',type=int,help='native committed-cut stop, exclusive')
    relations=sub.add_parser('relations',help='inspect coupling, shedding and seating as separate witnessed relations')
    relations.add_argument('trace',type=Path);relations.add_argument('--scope');relations.add_argument('--output',type=Path)
    encounter=sub.add_parser('encounter',help='replay initialization and every recorded source update, including pending attempts')
    encounter.add_argument('trace',type=Path);encounter.add_argument('--output',type=Path)
    paths=sub.add_parser('paths',help='join source, shedding, route, support, seating and receipt evidence')
    paths.add_argument('trace',type=Path);paths.add_argument('--scope');paths.add_argument('--output',type=Path)
    boundaries=sub.add_parser('boundaries');boundaries.add_argument('trace',type=Path);boundaries.add_argument('--scope');boundaries.add_argument('--name');boundaries.add_argument('--output',type=Path)
    boundaries.add_argument('--workers',type=int,default=1,help='CPU processes for scalar replay, or native threads for compiled-cpu')
    boundaries.add_argument('--backend',choices=['scalar','multicore','gpu','compiled-cpu','compiled-gpu'],default='scalar',help='GPU is required when selected; no automatic fallback')
    boundaries.add_argument('--coverage-scope',choices=['full','declared-current'],default='full',help='full also requires every crossing burden to have a signed-current map')
    inspect=sub.add_parser('inspect');inspect.add_argument('trace',type=Path);inspect.add_argument('--start',type=int,required=True)
    inspect.add_argument('--stop',type=int,required=True);inspect.add_argument('--scope');inspect.add_argument('--output',type=Path)
    finite=sub.add_parser('finite-scope');finite.add_argument('--detail',type=int,default=2);finite.add_argument('--steps',type=int,default=7);finite.add_argument('--output',type=Path)
    profile=sub.add_parser('campaign');profile.add_argument('--output',type=Path,required=True);profile.add_argument('--repetitions',type=int,default=5)
    gaps=sub.add_parser('application-gaps');gaps.add_argument('--output',type=Path)
    args=parser.parse_args()
    try:
        if args.command=='run':
            if args.example in ('closure','fusion','coupling'):
                result=run_core((ROOT/'examples'/f'{args.example}.dec').read_text(),args.trace,run_id=args.example)
            elif args.example=='solar-boundaries':
                from simulations import solar_boundary_fixture as solar
                modules=(solar,solar.n,solar.r,solar.r.accounting)
                source={str(Path(module.__file__).relative_to(ROOT)):
                    hashlib.sha256(Path(module.__file__).read_bytes()).hexdigest() for module in modules}
                manifest=run_manifest(args.example,source=source,
                    kernel='existing-native-duon-and-declared-boundary-current',
                    preparation='solar-boundary-fixture',backend='host-native-accounting')
                with TraceStore(args.trace,manifest) as store:
                    outcome=solar.run(store)
                    store.finish()
                    result={'outcome':outcome,'trace':store.status(),'backend':'host-native-accounting',
                            'native_support':detect_particle_support(store),
                            'native_relations':detect_particle_relations(store),
                            'solar_trace':solar.inspect_trace(store),
                            'profile':{'trace_storage':store.metrics,
                                'scope':'native fixture formation, declared-current commitments and recoverable witnesses'}}
                result['declared_boundaries']=boundary_views(args.trace,coverage_scope='declared-current')
                result['full_boundaries']=boundary_views(args.trace,coverage_scope='full')
                if any(hashlib.sha256((ROOT/path).read_bytes()).hexdigest()!=digest for path,digest in source.items()):
                    raise ValueError('solar fixture sources changed during execution')
            else:
                manifest=run_manifest(args.example,source={'base':'5a9368f6de286cde822963bac5f8de2620f08d5f'},
                    kernel='existing-source-bound-application',preparation=args.example,backend='host-accounting')
                with TraceStore(args.trace,manifest) as store:
                    artifacts=args.trace.parent/(args.trace.name+'-application-files')
                    outcome=run_regions(store) if args.example=='regions' else run_particles(artifacts,store)
                    store.finish()
                    result={'outcome':outcome,'trace':store.status(),'backend':'host-accounting',
                            'profile':{'trace_storage':store.metrics,'scope':'accounting hook and report/witness archive; native formation runtime separate'}}
                    if args.example=='particles':
                        result['native_support']=detect_particle_support(store)
                        result['native_relations']=detect_particle_relations(store)
                        result['paths']=detect_sheddic_paths(store)
                        result['external_application_artifacts']={'directory':str(artifacts),
                            'bytes':sum(p.stat().st_size for p in artifacts.iterdir() if p.is_file()),
                            'budget_scope':'separate application outputs; trace store cap applies to recoverable archived copies'}
            write(result,args.output)
            if args.profile:write(result['profile'],args.profile)
        elif args.command=='replay':write(replay(args.trace),args.output)
        elif args.command=='support':
            if (args.start is None) != (args.stop is None):
                raise ValueError('native support observation requires both --start and --stop')
            window=None if args.start is None else Window(args.start,args.stop,'native-committed-cut')
            with TraceStore.open(args.trace) as store:
                write(detect_particle_support(store,window=window,scope=args.scope,record_window=args.record_window),args.output)
        elif args.command=='relations':
            with TraceStore.open(args.trace) as store:
                write(detect_particle_relations(store,scope=args.scope),args.output)
        elif args.command=='encounter':
            with TraceStore.open(args.trace) as store:
                write(replay_encounter(store),args.output)
        elif args.command=='paths':
            with TraceStore.open(args.trace) as store:
                write(detect_sheddic_paths(store,scope=args.scope),args.output)
        elif args.command=='boundaries':write(boundary_views(args.trace,scope=args.scope,name=args.name,
            workers=args.workers,backend=args.backend,coverage_scope=args.coverage_scope),args.output)
        elif args.command=='inspect':
            window=Window(args.start,args.stop)
            result=replay(args.trace,window=window,scope=args.scope)
            with TraceStore.open(args.trace) as store:
                # Bound output materialization; full durable history remains queryable.
                events=[];encoded_bytes=0
                for event in store.iter_events():
                    if window.contains(event['commit_ordinal']) and (args.scope is None or args.scope==event['scope_id']):
                        encoded_bytes+=len(canonical_bytes(event))
                        if len(events)>=10000 or encoded_bytes>8000000:raise TraceError('inspect output cap exceeded; select a smaller window')
                        events.append(event)
                result['timeline']=events
            write(result,args.output)
        elif args.command=='finite-scope':write(finite_scope(args.detail,args.steps),args.output)
        elif args.command=='campaign':
            if not 1<=args.repetitions<=20:raise ValueError('repetitions must be 1..20')
            report=campaign(args.output,repetitions=args.repetitions)
            print(json.dumps({'rows':len(report['rows']),'results':str(args.output/'results.json')}))
        else:write(application_gaps(),args.output)
        return 0
    except GPUUnavailable as exc:
        print(json.dumps(exc.report),file=sys.stderr)
        return 3
    except (ValueError,OSError) as exc:
        print(json.dumps({'status':'incomplete','error':str(exc)}),file=sys.stderr)
        return 2


if __name__=='__main__':raise SystemExit(main())
