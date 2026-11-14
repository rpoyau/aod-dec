"""Raw encounter replay must account for every requested committed attempt."""
from copy import deepcopy
from pathlib import Path
import tempfile
import unittest

from simulations import proton_accounting as producer
from simulations.trace_applications import NativeParticleRecorder
from simulations.trace_encounter import reconstruct_encounter, replay_encounter
from simulations.trace_store import TraceStore, run_manifest, digest


class RecordedEvents:
    def __init__(self, events, coverage='complete'):
        self.events, self.coverage = events, coverage

    def iter_events(self):
        return iter(self.events)

    def status(self):
        return {'coverage': self.coverage}


def rebind(events, change):
    """Mutate scientific data while keeping the transport hash chain valid."""
    values, remap = deepcopy(events), {}
    change(values)
    for event in values:
        old = event['event_id']
        event['causal_parents'] = [remap.get(parent, parent) for parent in event['causal_parents']]
        body = {key: value for key, value in event.items() if key != 'event_id'}
        event['event_id'] = digest(body)
        remap[old] = event['event_id']
    return values


class EncounterReplayTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        with TraceStore(Path(cls.tmp.name) / 'trace', run_manifest('raw-encounter',
                source={'purpose': 'regression'}, kernel='retained-continuation', preparation='expiry-reentry')) as store:
            with producer.observe_commits(NativeParticleRecorder(store)) as observation:
                cls.world, cls.original = producer.encounter_source()
            if observation.failure_count:
                raise RuntimeError(observation.last_error)
            store.finish()
            cls.events = list(store.iter_events())
        cls.baseline, cls.rebuilt = reconstruct_encounter(RecordedEvents(cls.events))

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def test_reconstructs_actual_six_step_world_and_population(self):
        self.assertEqual(self.baseline['coverage'], 'complete', self.baseline['errors'])
        self.assertEqual(self.baseline['requested_steps'], 6)
        self.assertEqual(self.baseline['verified_steps'], 6)
        self.assertEqual(self.baseline['final_return_counts'], [7, 8])
        self.assertEqual(producer.packed(self.rebuilt), producer.packed(self.world))
        self.assertEqual(self.baseline['final_world_digest'], self.original['rows'][-1]['archive_digest'])

    def test_pending_updates_are_retained_without_inventing_returns(self):
        steps = self.baseline['steps']
        self.assertEqual([row['index'] for row in steps if row['pending_carriers']], [1, 2])
        for index in (1, 2):
            self.assertEqual(steps[index]['pending_carriers'], [0])
            self.assertEqual(steps[index]['new_returns'][0], [])
        self.assertEqual([sum(len(row['new_returns'][i]) for row in steps) for i in (0, 1)], [4, 6])

    def test_expired_relations_survive_in_later_histories(self):
        old = {row['identity'] for row in self.baseline['steps'][0]['relations']}
        later = {row['identity']: row for row in self.baseline['steps'][2]['relations']}
        self.assertTrue(old)
        self.assertTrue(old <= later.keys())
        self.assertTrue(all(not later[key]['active'] for key in old))
        self.assertTrue(self.baseline['steps'][3]['contact'])

    def test_missing_initial_middle_last_or_terminal_never_completes(self):
        for part, index in [('init', None), ('update', 2), ('update', 5), ('finish', None)]:
            events = [e for e in self.events if not (e['kind'] == 'particle.encounter_' + part
                and (index is None or e['payload'].get('index') == index))]
            with self.subTest(part=part, index=index):
                report, world = reconstruct_encounter(RecordedEvents(events))
                self.assertEqual(report['coverage'], 'partial')
                self.assertIsNone(world)

    def test_self_consistent_transport_cannot_replace_model_transition(self):
        def mutate(events):
            row = next(e['payload'] for e in events if e['kind'] == 'particle.encounter_update')
            row['after_state'][3] *= -1
        report = replay_encounter(RecordedEvents(rebind(self.events, mutate)))
        self.assertEqual(report['coverage'], 'partial')
        self.assertTrue(any('successor state' in error for error in report['errors']))

    def test_dropped_pending_disposition_is_rejected(self):
        def mutate(events):
            next(e['payload'] for e in events if e['kind'] == 'particle.encounter_update'
                 and e['payload']['index'] == 1)['pending_carriers'] = []
        report = replay_encounter(RecordedEvents(rebind(self.events, mutate)))
        self.assertEqual(report['coverage'], 'partial')
        self.assertTrue(any('pending' in error for error in report['errors']))

    def test_changed_source_binding_or_boolean_count_is_rejected(self):
        for field in ('source', 'count'):
            def mutate(events):
                row = next(e['payload'] for e in events if e['kind'] == 'particle.encounter_init')
                if field == 'source':
                    row['source_bindings']['simulations/retained_continuation.py'] = '0' * 64
                else:
                    row['requested_steps'] = True
            with self.subTest(field=field):
                report = replay_encounter(RecordedEvents(rebind(self.events, mutate)))
                self.assertEqual(report['coverage'], 'partial')
                self.assertEqual(report['verified_steps'], 0)

    def test_causal_predecessor_is_required(self):
        def mutate(events):
            next(e for e in events if e['kind'] == 'particle.encounter_update')['causal_parents'] = []
        report = replay_encounter(RecordedEvents(rebind(self.events, mutate)))
        self.assertEqual(report['coverage'], 'partial')
        self.assertTrue(any('predecessor' in error for error in report['errors']))

    def test_duplicate_transport_and_reordering_preserve_world(self):
        events = list(reversed(self.events)) + [deepcopy(self.events[0])]
        report, world = reconstruct_encounter(RecordedEvents(events))
        self.assertEqual(report['coverage'], 'complete', report['errors'])
        self.assertEqual(producer.packed(world), producer.packed(self.world))

    def test_snapshot_only_partial_archive_and_resource_limit_are_not_full_replay(self):
        snapshots = [e for e in self.events if not e['kind'].startswith('particle.encounter_')]
        self.assertEqual(replay_encounter(RecordedEvents(snapshots))['coverage'], 'unavailable')
        self.assertEqual(replay_encounter(RecordedEvents(self.events, 'partial'))['coverage'], 'partial')
        self.assertEqual(replay_encounter(RecordedEvents(self.events), max_events=1)['coverage'], 'partial')


if __name__ == '__main__':
    unittest.main()
