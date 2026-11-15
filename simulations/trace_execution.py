"""Actual reference execution with passive commit recording and host profiles."""
from contextlib import contextmanager
from dataclasses import asdict
from pathlib import Path
import hashlib
import json
import resource
import sqlite3
import time
import tracemalloc

from runtime.dec_language import compile_source, Interpreter, DecError
from runtime.dec_core import render
from simulations.trace_store import TraceStore, run_manifest, record_commits, canonical_bytes, TraceError, TraceIncomplete
from simulations.trace_detectors import detect_core, replay_boundaries, _decode_ratios


class Profile:
    """Host cost only. Inclusive nesting is labelled and must not be summed twice."""
    def __init__(self):
        self.phases = {}

    @contextmanager
    def phase(self, name):
        start = time.perf_counter_ns()
        try:
            yield
        finally:
            elapsed = time.perf_counter_ns() - start
            row = self.phases.setdefault(name, {'nanoseconds': 0, 'samples': 0})
            row['nanoseconds'] += elapsed; row['samples'] += 1

    def report(self):
        return {'phases': self.phases, 'clock': 'time.perf_counter_ns',
                'clock_scope': 'host profiling only; never an AOD/SI anchor',
                'phase_accounting': 'inclusive; total includes child phases; do not add total again',
                'device_counters': {'availability':'unavailable','value':None,'unit':'device_counter'},
                'rss_high_water_bytes': resource.getrusage(resource.RUSAGE_SELF).ru_maxrss * 1024,
                'rss_method': 'Linux process-lifetime ru_maxrss; includes interpreter/toolchain baseline'}


def reference(source):
    emitted = []
    try:
        value = Interpreter(compile_source(source), emit=emitted.append).run()
        return {'stdout': ''.join(s+'\n' for s in emitted), 'result': render(value),
                'error': None, 'exit_code': 0}
    except DecError as error:
        return {'stdout': ''.join(s+'\n' for s in emitted), 'result': None,
                'error': str(error), 'exit_code': 1}


def run_core(source, trace_path, *, run_id='core-example', limits=None):
    """All model execution is the existing reference VM, not trace playback."""
    profile = Profile()
    with profile.phase('untraced_reference'):
        untraced = reference(source)
    manifest = run_manifest(run_id, source={'sha256':hashlib.sha256(source.encode()).hexdigest(),
                            'runtime_sha256':hashlib.sha256((Path(__file__).resolve().parents[1]/'runtime/dec_core.py').read_bytes()).hexdigest()},
                            kernel='dec-core-hinge-return-v1', preparation='source-program',
                            backend='reference-vm', scope={'scale_family':'core-q4','scale_index':0,
                            'window_domain':'commit-ordinal','interval':'[start,stop)',
                            'native_commit_tracing':'unsupported'})
    tracemalloc.start()
    try:
        with profile.phase('total_traced_execution_and_detection'):
            with profile.phase('trace_storage_open'):
                store = TraceStore(trace_path, manifest, limits=limits)
            trace_errors = []

            def trace_failure(stage, error):
                trace_errors.append({'stage':stage, 'error':type(error).__name__, 'message':str(error)})
                if store.failure is None:
                    store._failure(error)

            storage_errors = (TraceError, OSError, sqlite3.Error)
            try:
                with profile.phase('model_with_trace_transport_storage'), record_commits(store):
                    traced = reference(source)
                if store.failure is None:
                    try:
                        store.checkpoint({'outcome':traced,'rng':{'algorithm':'none'}})
                    except storage_errors as error:
                        # A checkpoint is observation of an already computed
                        # result, including any original first model error.
                        trace_failure('checkpoint', error)
                try:
                    store.finish()
                except storage_errors as error:
                    trace_failure('finish', error)
                try:
                    with profile.phase('detector_and_archive_read'):
                        detection = detect_core(store.iter_events())
                except storage_errors as error:
                    trace_failure('detection', error)
                    detection = {'coverage':'partial', 'reason':'trace archive unavailable for detection'}
                try:
                    status = store.status()
                except storage_errors as error:
                    trace_failure('status', error)
                    # Do not invent an event count, chain or disk measurement
                    # when the durable status cannot be recovered.
                    status = {'coverage':'partial', 'state':'unavailable', 'failure':store.failure,
                              'events':None, 'chain':None, 'archive_bytes':None,
                              'limits':asdict(store.limits), 'metrics':dict(store.metrics),
                              'scope':'trace status unavailable; model outcome retained separately'}
                if trace_errors:
                    status = dict(status, coverage='partial', failure=store.failure, errors=trace_errors)
                if status['coverage']!='complete':
                    detection=dict(detection,coverage='partial',archive_coverage=status['coverage'])
            finally:
                store.close()
        _, peak = tracemalloc.get_traced_memory()
    finally:
        tracemalloc.stop()
    if traced != untraced:
        raise RuntimeError('passive instrumentation changed the model outcome')
    return {'format':'aod-dec-traced-execution-v1', 'outcome':traced,
            'passive_equivalence':True, 'manifest':manifest, 'trace':status,
            'detection':detection, 'profile':profile.report() | {
                'python_allocator_high_water_bytes':peak,
                'compilation':{'availability':'not_applicable','reason':'reference VM run'},
                'warmup':{'count':0,'protocol':'cold run; no warmup subtraction'}},
            'backend_scope':'reference VM commits; native core output conformance is a separate check'}


def replay(path, *, window=None, scope=None, record_window=None):
    with TraceStore.open(path) as store:
        result = detect_core(store.iter_events(), window=window, scope=scope)
        archive = store.status()
        if archive['coverage'] != 'complete':
            result = dict(result, coverage='partial', archive_coverage=archive['coverage'])
        from simulations.trace_applications import replay_accounting
        from simulations.trace_support import detect_particle_support
        from simulations.trace_particle_relations import detect_particle_relations
        from simulations.trace_paths import detect_sheddic_paths
        native_support = detect_particle_support(store, scope=scope,
                                                  record_window=record_window) if window is None else {
            'coverage':'unavailable',
            'reason':'trace serialization ordinals are not native construction cuts; use the support query with an explicit native window'}
        native_relations = detect_particle_relations(store, scope=scope) if window is None else {
            'coverage':'unavailable',
            'reason':'native relations require their complete retained prefixes and native comparison windows'}
        paths = detect_sheddic_paths(store, scope=scope) if window is None else {
            'coverage':'unavailable',
            'reason':'complete source and route histories are required; serialization slicing is not a native path window'}
        accounting=replay_accounting(store,scope=scope) if window is None else {
            'coverage':'unavailable','reason':'accounting uses native application windows; a trace ordinal slice is not its participation window'}
        has_checkpoint=store.db.execute("SELECT 1 FROM metadata WHERE key='checkpoint'").fetchone() is not None
        checkpoint=store.load_checkpoint() if has_checkpoint else {
            'status':'unavailable','reason':'this producer did not save an execution checkpoint'}
        return {'detection':result, 'archive':archive, 'manifest':store.manifest,
                'checkpoint':checkpoint, 'accounting':accounting,
                'native_support':native_support, 'native_relations':native_relations,
                'paths':paths}


def boundary_views(path, *, scope=None, name=None, workers=1, backend='scalar', coverage_scope='full'):
    """Reload committed geometry operands and re-execute each full boundary audit.

    These are named native-cut views, separate from the trace-ordinal timeline.
    A stored aggregate number is never accepted as its own replay proof.
    """
    from simulations.trace_parallel import boundary_views as parallel_views
    return parallel_views(path, scope=scope, name=name, workers=workers, backend=backend,
                          coverage_scope=coverage_scope)
