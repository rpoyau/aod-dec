"""Independent source-bound integration probes; no production edits."""
import copy
import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]/'aod-dec'
sys.path.insert(0, str(ROOT))
import aodc
from native.cpu_batch import run_cpu_batch
from simulations import compiled_boundaries as compiled, region_engine as region
from simulations.trace_execution import boundary_views
from simulations.trace_store import TraceStore
from simulations.chiral_gpu import GPUUnavailable
from tests.test_trace_parallel import fixture

RESULTS = {}


class Probes(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.path = Path(self.temp.name)/'trace'
        self.ledger = fixture(self.path)
        selected = region.Region('left', ('left',))
        self.frames = tuple(region.Frame(selected,selected,'stationary') for _ in self.ledger.commits)

    def tearDown(self):
        self.temp.cleanup()

    def test_cpu_joined_replay_has_actual_native_threads_and_same_occurrences(self):
        before = hashlib.sha256((self.path/'trace.sqlite3').read_bytes()).hexdigest()
        scalar = boundary_views(self.path)
        report = boundary_views(self.path,backend='compiled-cpu',workers=2)
        self.assertEqual(report['coverage'],'complete',report['failures'])
        self.assertEqual(report['boundaries'],scalar['boundaries'])
        self.assertEqual(report['execution']['backend'],'compiled-cpu')
        for job in report['execution']['jobs']:
            profile = job['profile']
            self.assertEqual(profile['execution']['workers_used'],2)
            self.assertGreater(profile['execution']['compiled_entries_entered'],0)
            self.assertEqual(profile['execution']['native_dispatch_calls'],1)
            self.assertEqual(profile['compiler_binding']['entry']['source_sha256'],
                             profile['source_sha256']['examples/boundary_coefficients.dec'])
        self.assertEqual(before,hashlib.sha256((self.path/'trace.sqlite3').read_bytes()).hexdigest())
        RESULTS['native_evidence'] = report['execution']

    def test_compiled_values_are_consumed_then_joined_validation_rejects_wrong_orientation(self):
        def reversed_orientation(source,function,rows,**kwargs):
            result = run_cpu_batch(source,function,rows,**kwargs)
            for row,inputs in zip(result['rows'],rows):
                if inputs[0]==0:
                    value=int(row['result'])
                    row['result']=str(value-2*(value%3-1))
            return result
        expected=region.audit_boundary(self.ledger,'left',self.frames)
        with patch('native.cpu_batch.run_cpu_batch',side_effect=reversed_orientation):
            direct=compiled.audit_boundary(self.ledger,'left',self.frames,workers=2)
            self.assertNotEqual(direct.total['current'],expected.total['current'])
            with self.assertRaisesRegex(ValueError,'differs'):
                region.validate_audit(direct)
            replay=boundary_views(self.path,backend='compiled-cpu',workers=2)
        self.assertEqual(replay['coverage'],'partial')
        self.assertTrue(all(row['value'] is None and row['status']=='unresolved'
                            for row in replay['boundaries'].values()))
        RESULTS['altered_compiled_values']={'expected':str(expected.total['current']),
            'direct_consumed_value':str(direct.total['current']),'joined_coverage':replay['coverage']}

    def test_required_gpu_unavailable_propagates_and_is_not_scalar_success(self):
        report={'status':'UNAVAILABLE','reason':'review-isolated device','gpu_executed':False}
        with patch('native.gpu_compiler.run_gpu_batch',side_effect=GPUUnavailable(report)):
            with self.assertRaises(GPUUnavailable) as raised:
                boundary_views(self.path,backend='compiled-gpu')
        self.assertEqual(raised.exception.report,report)

    def test_lane_failure_and_wrong_row_count_cannot_publish_current(self):
        for corruption in ('failure','omitted'):
            def altered(source,function,rows,**kwargs):
                result=run_cpu_batch(source,function,rows,**kwargs)
                if corruption=='failure':
                    result['rows'][0].update(exit_code=1,result=None,error='synthetic lane failure')
                else:
                    result['rows'].pop()
                return result
            with patch('native.cpu_batch.run_cpu_batch',side_effect=altered):
                report=boundary_views(self.path,backend='compiled-cpu',workers=2)
            self.assertEqual(report['coverage'],'partial')
            self.assertTrue(all(row['value'] is None for row in report['boundaries'].values()))

    def test_returned_compiler_entry_cannot_substitute_another_source(self):
        def altered(source,function,rows,**kwargs):
            result=run_cpu_batch(source,function,rows,**kwargs)
            result['source_binding']['entry']['source_sha256']='0'*64
            return result
        with patch('native.cpu_batch.run_cpu_batch',side_effect=altered):
            report=boundary_views(self.path,backend='compiled-cpu',workers=2)
        self.assertEqual(report['coverage'],'partial')
        self.assertTrue(all(row['value'] is None for row in report['boundaries'].values()))

    def test_cli_orders_success_and_failure_and_reports_nonzero_exit(self):
        path=Path(self.temp.name)/'rows.json'
        path.write_text(json.dumps([[0,1,0,1,0],[0,2,0,1,0],[1,0,1,1,0]]))
        process=subprocess.run([sys.executable,str(ROOT/'aodc.py'),'batch',str(compiled.SOURCE),
            '--function','coefficient','--inputs',str(path),'--backend','cpu','--workers','2'],
            capture_output=True,text=True,check=False)
        self.assertEqual(process.returncode,1)
        self.assertEqual(process.stderr,'')
        report=json.loads(process.stdout)
        self.assertEqual([r['result'] for r in report['rows']],['53',None,'-1'])
        self.assertIn('assertion failed',report['rows'][1]['error'])
        self.assertEqual(report['execution']['workers_used'],2)
        RESULTS['cli_mixed_outcomes']=report['rows']

    def test_cli_malformed_json_and_required_gpu_emit_error(self):
        path=Path(self.temp.name)/'rows.json'
        path.write_text('[[true,1,0,1,0]]')
        import contextlib
        out,err=io.StringIO(),io.StringIO()
        with contextlib.redirect_stdout(out),contextlib.redirect_stderr(err):
            code=aodc.main(['batch',str(compiled.SOURCE),'--function','coefficient','--inputs',str(path),
                            '--backend','cpu'])
        self.assertEqual(code,1)
        self.assertEqual(out.getvalue(),'')
        self.assertIn('exact int',err.getvalue())
        path.write_text('[[0,1,0,1,0]]')
        out,err=io.StringIO(),io.StringIO()
        report={'status':'UNAVAILABLE','reason':'review-isolated device','gpu_executed':False}
        with patch('native.gpu_compiler.run_gpu_batch',side_effect=GPUUnavailable(report)), \
             contextlib.redirect_stdout(out),contextlib.redirect_stderr(err):
            code=aodc.main(['batch',str(compiled.SOURCE),'--function','coefficient','--inputs',str(path),
                            '--backend','gpu'])
        self.assertEqual(code,1)
        self.assertEqual(out.getvalue(),'')
        self.assertIn('review-isolated device',err.getvalue())

    def test_record_current_source_linkage_guard(self):
        changed=compiled.SOURCE.read_text()+'\n# independently simulated changed read\n'
        class AlternateRead:
            def read_text(self):
                return changed
        try:
            with patch.object(compiled,'SOURCE',AlternateRead()):
                result=compiled.profiled_audit_boundary(self.ledger,'left',self.frames)
        except (ValueError,TypeError) as exc:
            RESULTS['source_linkage']={'status':'REJECTED','reason':str(exc)}
        else:
            bound=result['profile']['source_sha256']['examples/boundary_coefficients.dec']
            actual=result['profile']['compiler_binding']['entry']['source_sha256']
            RESULTS['source_linkage']={'status':'ACCEPTED_MISMATCH' if bound!=actual else 'BOUND',
                                     'outer_sha256':bound,'compiled_sha256':actual}


if __name__=='__main__':
    paths=['aodc.py','simulations/compiled_boundaries.py','simulations/trace_parallel.py',
           'simulations/trace_detectors.py','scripts/check_compiled_boundaries.py',
           'tests/test_compiled_boundaries.py','examples/boundary_coefficients.dec']
    before={name:hashlib.sha256((ROOT/name).read_bytes()).hexdigest() for name in paths}
    suite=unittest.defaultTestLoader.loadTestsFromTestCase(Probes)
    result=unittest.TextTestRunner(verbosity=2).run(suite)
    RESULTS['tests']={'run':result.testsRun,'errors':len(result.errors),'failures':len(result.failures)}
    RESULTS['source_sha256']={name:hashlib.sha256((ROOT/name).read_bytes()).hexdigest() for name in paths}
    RESULTS['sources_unchanged_during_review']=RESULTS['source_sha256']==before
    Path(__file__).with_name('probes-'+('optimized' if sys.flags.optimize else 'normal')+'.json').write_text(
        json.dumps(RESULTS,sort_keys=True,indent=2)+'\n')
    sys.exit(not result.wasSuccessful())
