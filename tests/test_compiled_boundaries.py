import itertools
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

from runtime.dec_language import Interpreter, compile_source
from simulations import compiled_boundaries as compiled
from simulations import region_engine as region
from simulations.solar_boundary_fixture import run


class CompiledBoundaryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture = run()

    def test_source_truth_table(self):
        program = compile_source(compiled.SOURCE.read_text())
        for mode,left,right,available,kind in itertools.product(range(2),range(2),range(2),range(2),range(3)):
            value = Interpreter(program).call('coefficient',[mode,left,right,available,kind])
            if mode:
                self.assertEqual(value,left-right)
            else:
                self.assertEqual(value%3-1,left-right)
                self.assertEqual((value//3)%2,int(bool(available and left != right)))
                self.assertEqual((value//6)%2,int(left == right == 1))
                self.assertEqual((value//12)%2,int(left == right == 0))
                self.assertEqual((value//24)%2,1-available)
                self.assertEqual(value//48,kind+1 if available and left != right else 0)

    def test_compiled_current_preserves_exact_solar_rows(self):
        f = self.fixture
        for name,frames in f['frames'].items():
            audit = compiled.audit_boundary(f['ledger'],name,frames,workers=2)
            self.assertEqual(audit,region.audit_boundary(f['ledger'],name,frames,scope='declared-current'))
        with self.assertRaisesRegex(ValueError,'unresolved signed shedding projection'):
            compiled.audit_boundary(f['ledger'],'selected',f['frames']['selected'],scope='full',workers=2)

    def test_failed_compiled_lane_cannot_publish_current(self):
        f = self.fixture
        binding = {'entry':{'source_sha256':compiled.source_bindings('compiled-cpu')['examples/boundary_coefficients.dec']}}
        with patch('native.cpu_batch.run_cpu_batch',return_value={'source_binding':binding,'rows':[{'exit_code':1,'error':'failed','stdout':'','result':None}]}):
            with self.assertRaisesRegex(ValueError,'row count differs|coefficient failed'):
                compiled.audit_boundary(f['ledger'],'selected',f['frames']['selected'],workers=2)

    def test_source_read_must_match_bound_file(self):
        f = self.fixture
        source = Mock()
        source.read_text.return_value = '# transient change\n'+compiled.SOURCE.read_text()
        with patch.object(compiled,'SOURCE',source):
            with self.assertRaisesRegex(ValueError,'source text differs'):
                compiled.audit_boundary(f['ledger'],'selected',f['frames']['selected'],workers=2)

    def test_changed_source_cannot_reuse_named_classifier(self):
        # Ordinary Dec arithmetic, not the function's name, controls the result.
        from native.cpu_batch import run_cpu_batch
        text = compiled.SOURCE.read_text().replace('result = orientation + 1 +','result = orientation + 2 +')
        original = run_cpu_batch(compiled.SOURCE.read_text(),'coefficient',[[0,1,0,1,0]],workers=1)
        changed = run_cpu_batch(text,'coefficient',[[0,1,0,1,0]],workers=1)
        self.assertEqual(int(changed['rows'][0]['result']),int(original['rows'][0]['result'])+1)


if __name__ == '__main__':
    unittest.main()
