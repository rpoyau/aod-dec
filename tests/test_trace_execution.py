from pathlib import Path
import tempfile
import unittest
from simulations.trace_execution import run_core, replay
from simulations.trace_store import TraceStore,run_manifest
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

    def test_approximation_bound_for_nonstationary_future(self):
        P=((Q(2,3),Q(1,3)),(Q(1,7),Q(6,7)))
        exact=q=(Q(1),Q(0));bound=Q(0)
        for _ in range(15):
            exact=advance(exact,P)
            q,bound=approximate_step(q,P,11,bound)
            self.assertLessEqual(sum(abs(a-b) for a,b in zip(exact,q)),bound)
            self.assertEqual(sum(q),1)


if __name__=='__main__':unittest.main()
