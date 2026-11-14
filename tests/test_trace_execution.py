from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from simulations.trace_execution import run_core, replay, reference
from simulations.trace_store import TraceStore, TraceLimits, TraceIncomplete, TraceIntegrityError, run_manifest
from simulations.trace_views import Window
from simulations.scoped_reduction import approximate_step, advance
from fractions import Fraction as Q

ROOT=Path(__file__).resolve().parents[1]


class TraceExecutionTests(unittest.TestCase):
    def test_archive_without_optional_checkpoint_replays(self):
        with tempfile.TemporaryDirectory() as d:
            with TraceStore(d,run_manifest('empty',source={'id':'fixture'},kernel='none',preparation='empty')):
                pass
            result=replay(d)
            self.assertEqual(result['archive']['coverage'],'complete')
            self.assertEqual(result['checkpoint']['status'],'unavailable')

    def test_live_and_offline_example_execution(self):
        with tempfile.TemporaryDirectory() as d:
            result=run_core((ROOT/'examples/closure.dec').read_text(),Path(d)/'trace')
            self.assertTrue(result['passive_equivalence'])
            self.assertEqual(result['detection'],replay(Path(d)/'trace')['detection'])
            self.assertEqual(result['detection']['phases'],2)

    def test_passive_first_error_and_prefix(self):
        source='Dec main() -> out:\n    () -> int out:\n        emit 2\n        out = 1 // 0\n'
        with tempfile.TemporaryDirectory() as d:
            result=run_core(source,Path(d)/'trace')
            self.assertEqual(result['outcome']['stdout'],'2\n')
            self.assertIn('zero divisor',result['outcome']['error'])
            self.assertTrue(result['passive_equivalence'])

    def check_checkpoint_budget_outcome(self, final_statement):
        source = ('Dec main() -> out:\n    () -> int out:\n' +
                  '        emit 123456789\n' * 200 + '        ' + final_statement + '\n')
        expected = reference(source)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'trace'
            result = run_core(source, path, limits=TraceLimits(max_event_bytes=2048))
            self.assertEqual(result['outcome'], expected)
            self.assertEqual(len(result['outcome']['stdout']), 2000)
            self.assertTrue(result['passive_equivalence'])
            self.assertEqual(result['trace']['coverage'], 'partial')
            self.assertEqual(result['detection']['coverage'], 'partial')
            self.assertEqual(result['trace']['errors'][0]['stage'], 'checkpoint')
            self.assertIn('budget', result['trace']['failure'])
            with TraceStore.open(path) as archive:
                self.assertEqual(archive.status()['coverage'], 'partial')
                with self.assertRaisesRegex(TraceIntegrityError, 'metadata missing: checkpoint'):
                    archive.load_checkpoint()
            self.assertEqual(replay(path)['checkpoint']['status'], 'unavailable')

    def test_successful_model_survives_checkpoint_budget_failure(self):
        self.check_checkpoint_budget_outcome('out = 7')

    def test_first_model_error_survives_checkpoint_budget_failure(self):
        self.check_checkpoint_budget_outcome('out = 1 // 0')

    def test_checkpoint_and_status_storage_failure_preserve_model_outcome(self):
        source = 'Dec main() -> out:\n    () -> int out:\n        emit 2\n        out = 1 // 0\n'
        expected = reference(source)
        original_checkpoint = TraceStore.checkpoint

        def checkpoint_with_failed_disk_measure(store, *args, **kwargs):
            def fail_measure():
                raise TraceIncomplete('forced archive byte budget failure')
            store._disk_measure = fail_measure
            return original_checkpoint(store, *args, **kwargs)

        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'trace'
            with patch.object(TraceStore, 'checkpoint', checkpoint_with_failed_disk_measure):
                result = run_core(source, path)
            self.assertEqual(result['outcome'], expected)
            self.assertEqual(result['trace']['coverage'], 'partial')
            self.assertEqual(result['trace']['state'], 'unavailable')
            self.assertIsNone(result['trace']['events'])
            self.assertIsNone(result['trace']['archive_bytes'])
            self.assertEqual(result['trace']['failure'], 'forced archive byte budget failure')
            self.assertEqual([error['stage'] for error in result['trace']['errors']], ['checkpoint', 'status'])
            with TraceStore.open(path) as archive:
                self.assertEqual(archive.status()['coverage'], 'partial')
                self.assertEqual(archive.load_checkpoint()['state']['outcome'], expected)

    def test_failed_trace_diagnostics_do_not_replace_completed_outcome(self):
        source = 'Dec main() -> out:\n    () -> int out:\n        emit 2\n        out = 7\n'
        expected = reference(source)
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(TraceStore, 'checkpoint', side_effect=OSError('checkpoint disk failure')), \
                 patch.object(TraceStore, 'finish', side_effect=TraceIncomplete('finish unavailable')), \
                 patch('simulations.trace_execution.detect_core', side_effect=TraceIntegrityError('detector unavailable')):
                result = run_core(source, Path(directory) / 'trace')
            self.assertEqual(result['outcome'], expected)
            self.assertEqual(result['trace']['coverage'], 'partial')
            self.assertEqual(result['detection']['coverage'], 'partial')
            self.assertEqual(result['trace']['failure'], 'checkpoint disk failure')
            self.assertEqual([error['stage'] for error in result['trace']['errors']],
                             ['checkpoint', 'finish', 'detection'])

    def test_approximation_bound_for_nonstationary_future(self):
        P=((Q(2,3),Q(1,3)),(Q(1,7),Q(6,7)))
        exact=q=(Q(1),Q(0));bound=Q(0)
        for _ in range(15):
            exact=advance(exact,P)
            q,bound=approximate_step(q,P,11,bound)
            self.assertLessEqual(sum(abs(a-b) for a,b in zip(exact,q)),bound)
            self.assertEqual(sum(q),1)


if __name__=='__main__':unittest.main()
