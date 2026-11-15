"""Independent selected-code observer review on actual Python monitoring APIs."""
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
from hashlib import sha256
import json
from pathlib import Path
import sys
import threading
import unittest
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[1]/'aod-dec'
sys.path.insert(0,str(ROOT))
from simulations import awareness_engine_trace as t
RESULTS={}


class Review(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not hasattr(sys,'monitoring'):
            raise unittest.SkipTest('actual sys.monitoring unavailable')
        cls.manual=t.fixture.load_manual()
        cls.baseline={case:t.run_engine_case(case,observer='profile') for case in t.CASES}

    def test_exact_record_parity_and_owned_mask_cleanup(self):
        evidence=[]
        for case,expected in self.baseline.items():
            actual=t.run_engine_case(case,observer='monitoring')
            self.assertEqual(actual['observer_backend'],'sys.monitoring')
            self.assertEqual(actual['trace'],expected['trace'])
            self.assertEqual(self.manual.packed(actual['result']),self.manual.packed(expected['result']))
            identity=actual['observer_tool_id']
            self.assertIsNone(sys.monitoring.get_tool(identity))
            self.assertEqual(sys.monitoring.get_events(identity),0)
            collector=t._Collector(self.manual,case,{},4096,8*1024*1024)
            self.assertTrue(all(sys.monitoring.get_local_events(identity,code)==0 for code in collector.codes))
            evidence.append({'case':case,'tool_id':identity,'events':len(actual['trace'].events),'exact_records':True})
        RESULTS['parity']=evidence

    def test_existing_python_profiler_retains_compatibility_and_identity(self):
        previous=sys.getprofile(); seen=[]
        def profiler(frame,event,value):
            if event=='call' and frame.f_code is self.manual.mindfulness.__code__:
                seen.append(True)
        try:
            sys.setprofile(profiler)
            actual=t.run_engine_case('mindfulness',observer='monitoring')
            self.assertIs(sys.getprofile(),profiler)
        finally:
            sys.setprofile(previous)
        self.assertTrue(seen)
        self.assertEqual(actual['observer_backend'],'sys.setprofile')
        self.assertEqual(actual['trace'],self.baseline['mindfulness']['trace'])
        self.assertTrue(actual['observer_fallback'])

    def test_two_simultaneous_tools_keep_thread_records_separate(self):
        gate=threading.Barrier(2)
        original=t._MonitoringObserver.install
        def overlap(observer):
            installed=original(observer)
            if not installed:raise ValueError('monitoring setup unavailable')
            gate.wait(15)
            return installed
        with patch.object(t._MonitoringObserver,'install',overlap):
            with ThreadPoolExecutor(max_workers=2) as pool:
                futures=[pool.submit(t.run_engine_case,case,observer='monitoring') for case in ('mindfulness','delayed')]
                actual=[future.result(timeout=30) for future in futures]
        self.assertEqual(len({run['observer_tool_id'] for run in actual}),2)
        for run in actual:
            self.assertEqual(run['observer_backend'],'sys.monitoring')
            self.assertEqual(run['trace'],self.baseline[run['case']]['trace'])
            self.assertIsNone(sys.monitoring.get_tool(run['observer_tool_id']))
        RESULTS['simultaneous_tool_ids']=[run['observer_tool_id'] for run in actual]

    def test_occupied_slots_fall_back_without_changing_owners(self):
        owned=[]
        before={identity:sys.monitoring.get_tool(identity) for identity in range(6)}
        try:
            for identity,name in before.items():
                if name is None:
                    sys.monitoring.use_tool_id(identity,'review-owned-'+str(identity))
                    owned.append(identity)
            claimed={identity:sys.monitoring.get_tool(identity) for identity in range(6)}
            actual=t.run_engine_case('mindfulness',observer='monitoring')
            self.assertEqual({i:sys.monitoring.get_tool(i) for i in range(6)},claimed)
            self.assertEqual(actual['observer_backend'],'sys.setprofile')
            self.assertEqual(actual['trace'],self.baseline['mindfulness']['trace'])
        finally:
            for identity in owned:sys.monitoring.free_tool_id(identity)
        self.assertEqual({i:sys.monitoring.get_tool(i) for i in range(6)},before)

    def test_dirty_unowned_callbacks_are_preserved_and_not_reused(self):
        owned=[]; event=sys.monitoring.events.PY_START
        def prior_callback(*args):return None
        try:
            for identity in range(6):
                if sys.monitoring.get_tool(identity) is None:
                    sys.monitoring.use_tool_id(identity,'review-dirty-'+str(identity))
                    old=sys.monitoring.register_callback(identity,event,prior_callback)
                    if old is not None:raise ValueError('review expected initially clean callback')
                    sys.monitoring.free_tool_id(identity)
                    owned.append(identity)
            actual=t.run_engine_case('mindfulness',observer='monitoring')
            self.assertEqual(actual['observer_backend'],'sys.setprofile')
            self.assertEqual(actual['trace'],self.baseline['mindfulness']['trace'])
            for identity in owned:
                self.assertIsNone(sys.monitoring.get_tool(identity))
                sys.monitoring.use_tool_id(identity,'review-cleanup-'+str(identity))
                old=sys.monitoring.register_callback(identity,event,None)
                self.assertIs(old,prior_callback)
                sys.monitoring.free_tool_id(identity)
        finally:
            for identity in owned:
                if sys.monitoring.get_tool(identity) is None:
                    sys.monitoring.use_tool_id(identity,'review-cleanup-'+str(identity))
                sys.monitoring.register_callback(identity,event,None)
                sys.monitoring.free_tool_id(identity)

    def test_original_error_and_partial_unwind_records_match_profile(self):
        def broken(s,attention,input_index):
            raise RuntimeError('review original engine error')
        with patch.object(t.fixture.native,'update',broken):
            profile=t.run_engine_case('mindfulness',observer='profile',capture_errors=True)
            monitor=t.run_engine_case('mindfulness',observer='monitoring',capture_errors=True)
        self.assertEqual(profile['error'],{'type':'RuntimeError','message':'review original engine error'})
        self.assertEqual(monitor['error'],profile['error'])
        self.assertEqual(monitor['trace'],profile['trace'])
        self.assertFalse(monitor['trace'].complete)
        RESULTS['error_parity']={'events':len(monitor['trace'].events),'original_error':monitor['error']}

    def test_install_and_cleanup_failures_preserve_engine_outcome(self):
        with patch.object(t._MonitoringObserver,'install',side_effect=RuntimeError('review setup failure')):
            run=t.run_engine_case('mindfulness',observer='monitoring')
        self.assertEqual(self.manual.packed(run['result']),self.manual.packed(self.baseline['mindfulness']['result']))
        self.assertFalse(run['trace'].complete)
        retained=[]; original_close=t._MonitoringObserver.close
        def failed_close(observer):
            retained.append(observer)
            raise RuntimeError('review cleanup failure')
        def broken(s,attention,input_index):
            raise RuntimeError('review original engine error')
        try:
            with patch.object(t._MonitoringObserver,'close',failed_close),patch.object(t.fixture.native,'update',broken):
                with self.assertRaisesRegex(RuntimeError,'^review original engine error$') as captured:
                    t.run_engine_case('mindfulness',observer='monitoring')
            self.assertFalse(captured.exception.engine_trace.complete)
        finally:
            for observer in retained:original_close(observer)


if __name__=='__main__':
    paths=['simulations/awareness_engine_trace.py','simulations/trace_history_union.py',
           'tests/test_awareness_engine_trace.py']
    before={name:sha256((ROOT/name).read_bytes()).hexdigest() for name in paths}
    tests=unittest.defaultTestLoader.loadTestsFromTestCase(Review)
    result=unittest.TextTestRunner(verbosity=2).run(tests)
    after={name:sha256((ROOT/name).read_bytes()).hexdigest() for name in paths}
    RESULTS.update(status='PASS' if result.wasSuccessful() and before==after else 'FAIL',
        tests=result.testsRun,errors=len(result.errors),failures=len(result.failures),source_sha256=after,
        sources_unchanged=before==after,python=sys.version)
    Path(__file__).with_name('review-monitoring-'+('optimized' if sys.flags.optimize else 'normal')+'.json').write_text(
        json.dumps(RESULTS,indent=2)+'\n')
    sys.exit(RESULTS['status']!='PASS')
