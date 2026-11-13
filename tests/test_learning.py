import hashlib
import json
import runpy
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from native.learning import (MAX_SAMPLES, RetainedPolicy, VariantRegistry, canonical,
                             digest, paired_interval, semantic_outcome)

RESULT = {'stdout': '2\n', 'result': 't32(0)', 'error': None, 'error_line': 0, 'exit_code': 0}


def registry(optimized=True):
    reg = VariantRegistry('source', 'abi1', RESULT)
    reg.validate('generic', 'obj-O0', RESULT)
    if optimized:
        reg.validate('optimized', 'obj-O2', RESULT)
    return reg


class LearningPolicyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / 'profile.json'
        self.binding = {'host': 'host1', 'software': 'sw1', 'workload': 'work1'}

    def trained(self):
        p = RetainedPolicy(self.path, self.binding, registry())
        for _ in range(5):
            p.observe_pair(100, 70)
        p.save()
        return p

    def test_semantic_gate_keeps_outputs_errors_and_location(self):
        for key, value in [('stdout', '3\n'), ('result', 't32(1)'), ('error', 'overflow'),
                           ('error_line', 5), ('exit_code', 1)]:
            r = dict(RESULT, **{key: value})
            with self.assertRaises(ValueError):
                registry(False).validate('optimized', 'obj-O2', r)
        registry(False).validate('optimized', 'obj-O2', dict(RESULT, execute_ns=999, address=1234))

    def test_later_selection_causally_depends_on_retention(self):
        p = RetainedPolicy(self.path, self.binding, registry())
        self.assertEqual(p.choose()['variant'], 'generic')
        self.trained()
        # Current instance has not read later observations; next instance has.
        self.assertEqual(p.choose()['variant'], 'generic')
        resumed = RetainedPolicy(self.path, self.binding, registry())
        self.assertEqual(resumed.choose()['variant'], 'optimized')
        self.path.unlink()
        self.assertEqual(RetainedPolicy(self.path, self.binding, registry()).choose()['variant'], 'generic')

    def test_restart_reads_committed_profile(self):
        self.trained()
        code = '''import json,sys
from native.learning import *
r=VariantRegistry('source','abi1',json.loads(sys.argv[2]))
r.validate('generic','obj-O0',json.loads(sys.argv[2]))
r.validate('optimized','obj-O2',json.loads(sys.argv[2]))
p=RetainedPolicy(sys.argv[1],{'host':'host1','software':'sw1','workload':'work1'},r)
print(json.dumps(p.choose()))'''
        proc = subprocess.run([sys.executable, '-c', code, str(self.path), json.dumps(RESULT)],
                              cwd=Path(__file__).resolve().parents[1], capture_output=True, text=True)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(json.loads(proc.stdout)['variant'], 'optimized')

    def test_generic_passive_frozen_active(self):
        p = self.trained()
        self.assertEqual(p.choose('generic')['variant'], 'generic')
        self.assertEqual(p.choose('passive')['variant'], 'generic')
        self.assertEqual(p.choose('frozen')['variant'], 'optimized')
        self.assertEqual(p.choose('active')['variant'], 'optimized')
        self.assertEqual(p.generation, 5)

    def test_binding_version_registry_and_corruption_fallback(self):
        self.trained()
        for key in self.binding:
            binding = dict(self.binding, **{key: 'incompatible'})
            p = RetainedPolicy(self.path, binding, registry())
            self.assertTrue(p.load_reason.startswith('rejected'))
            self.assertEqual(p.choose()['variant'], 'generic')
        different = registry()
        different.validate('optimized', 'changed-object', RESULT)
        self.assertEqual(RetainedPolicy(self.path, self.binding, different).choose()['variant'], 'generic')
        original = self.path.read_text()
        obj = json.loads(original)
        obj['payload']['version'] = 99
        obj['sha256'] = digest(obj['payload'])
        self.path.write_text(canonical(obj))
        self.assertEqual(RetainedPolicy(self.path, self.binding, registry()).choose()['variant'], 'generic')
        for raw in ('{', 'null', '[]', '{"payload":1,"sha256":"a"}', 'x' * 1000001):
            self.path.write_text(raw)
            p = RetainedPolicy(self.path, self.binding, registry())
            self.assertTrue(p.load_reason.startswith('rejected'), raw[:10])
            self.assertEqual(p.choose()['variant'], 'generic')

    def test_checksum_valid_unpaired_or_inconsistent_generation_rejected(self):
        self.trained()
        original = self.path.read_text()
        for generation, generic_count, optimized_count in ((32,32,5), (5,2,2), (0,5,5), (5,0,0)):
            obj = json.loads(original)
            obj['payload']['generation'] = generation
            obj['payload']['costs'] = {'generic':[100]*generic_count, 'optimized':[70]*optimized_count}
            obj['sha256'] = digest(obj['payload'])
            self.path.write_text(canonical(obj))
            p = RetainedPolicy(self.path, self.binding, registry())
            self.assertIn('paired counts', p.load_reason)
            self.assertEqual(p.choose()['variant'], 'generic')
        # A rolled, fully paired window is lawful when generation exceeds the cap.
        obj = json.loads(original)
        obj['payload']['generation'] = MAX_SAMPLES+1
        obj['payload']['costs'] = {'generic':[100]*MAX_SAMPLES, 'optimized':[70]*MAX_SAMPLES}
        obj['sha256'] = digest(obj['payload'])
        self.path.write_text(canonical(obj))
        p = RetainedPolicy(self.path, self.binding, registry())
        self.assertEqual(p.load_reason, 'valid')
        self.assertEqual(p.choose()['variant'], 'optimized')

    def test_shuffled_costs_without_integrity_update_rejected(self):
        self.trained()
        obj = json.loads(self.path.read_text())
        obj['payload']['costs']['generic'][0] = 1
        self.path.write_text(canonical(obj))
        p = RetainedPolicy(self.path, self.binding, registry())
        self.assertIn('checksum', p.load_reason)
        self.assertEqual(p.choose()['variant'], 'generic')

    def test_atomic_write_ignores_interrupted_temporary(self):
        self.trained()
        (self.path.parent / ('.'+self.path.name+'.interrupted.tmp')).write_text('{"partial":')
        p = RetainedPolicy(self.path, self.binding, registry())
        self.assertEqual(p.load_reason, 'valid')
        self.assertEqual(p.choose()['variant'], 'optimized')

    def test_bounded_counts_invalid_costs_and_registry(self):
        p = RetainedPolicy(self.path, self.binding, registry())
        for _ in range(100):
            p.observe_pair(100, 70)
        self.assertEqual(len(p.costs['generic']), MAX_SAMPLES)
        for value in (0, -1, True, 1.5, 2**63):
            with self.assertRaises(ValueError):
                p.observe_pair(value, 10)
        with self.assertRaises(ValueError):
            RetainedPolicy(self.path, self.binding, registry(False)).observe_pair(100, 70)
        self.assertEqual(RetainedPolicy(self.path, self.binding, registry(False)).choose()['variant'], 'generic')

    def test_zero_negative_gain_and_overhead_roll_back(self):
        for generic, optimized, overhead in [([100]*10,[100]*10,0), ([100]*10,[110]*10,0),
                                             ([100]*10,[70]*10,1000)]:
            p = self.trained()
            result = p.evaluate_heldout(generic, optimized, overhead)
            self.assertFalse(result['established_benefit'])
            self.assertTrue(p.rollback)
            self.assertEqual(p.choose()['variant'], 'generic')
        p = self.trained()
        self.assertTrue(p.evaluate_heldout([100]*10, [70]*10, 0)['established_benefit'])

    def test_benchmark_reference_preserves_error_location_and_effects(self):
        helper = runpy.run_path(str(Path(__file__).resolve().parents[1] / 'scripts/benchmark_native.py'))['reference']
        source = ('Dec main() -> result:\n    () -> t32 result:\n'
                  '        emit 123\n        result = t32(1000000000000000000000000000000)\n')
        self.assertEqual(helper(source), {'stdout': '123\n', 'result': None,
            'error': 'line 4: t32 overflow', 'error_line': 4, 'exit_code': 1})

    def test_independent_paired_interval(self):
        result = paired_interval([100]*10, [75]*10)
        self.assertEqual((result['median_gain'], result['lower'], result['upper']), (0.25, 0.25, 0.25))

if __name__ == '__main__':
    unittest.main()
