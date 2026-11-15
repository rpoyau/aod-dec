"""Public compiler batch CLI: native execution and ordered typed outcomes."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
SOURCE = """Dec chosen(x) -> result:
    (int x) -> int result:
        assert x >= 0
        result = x * x
Dec main() -> result:
    () -> int result:
        result = 0
"""
OVERLOADED = """Dec chosen(x) -> result:
    (int x) -> int result:
        result = x + 1
    (bool x) -> bool result:
        result = not x
Dec main() -> result:
    () -> int result:
        result = 0
"""


class BatchCLITests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.directory = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def invoke(self, rows, *, source=SOURCE, options=()):
        source_path, inputs_path = self.directory/'entry.dec', self.directory/'rows.json'
        source_path.write_text(source)
        inputs_path.write_text(json.dumps(rows))
        return subprocess.run([sys.executable, *(['-O'] if sys.flags.optimize else []),
            str(ROOT/'aodc.py'), 'batch', str(source_path), '--function', 'chosen',
            '--inputs', str(inputs_path), '--backend', 'cpu', '--workers', '2', *options],
            capture_output=True, text=True, check=False, timeout=60)

    def test_real_native_threads_preserve_mixed_lane_success_and_assertion_order(self):
        process = self.invoke([[3], [-1], [11], [0]])
        self.assertEqual(process.returncode, 1, process.stderr)
        self.assertEqual(process.stderr, '')
        report = json.loads(process.stdout)
        self.assertEqual([row['result'] for row in report['rows']], ['9', None, '121', '0'])
        self.assertEqual([row['exit_code'] for row in report['rows']], [0, 1, 0, 0])
        self.assertEqual(report['rows'][1]['error'], 'line 3: assertion failed')
        self.assertEqual(report['rows'][1]['error_line'], 3)
        self.assertTrue(all(row['stdout'] == '' for row in report['rows']))
        execution = report['execution']
        self.assertEqual(execution['backend'], 'compiled-cpu')
        self.assertEqual(execution['native_dispatch_calls'], 1)
        self.assertEqual(execution['compiled_entries_entered'], 4)
        self.assertEqual(execution['workers_used'], 2)
        self.assertEqual(len(set(execution['worker_ids'])), 2)

    def test_overloaded_entry_requires_and_uses_explicit_parameter_types(self):
        missing = self.invoke([[3]], source=OVERLOADED)
        self.assertEqual(missing.returncode, 1)
        self.assertEqual(missing.stdout, '')
        self.assertIn('provide param_types', missing.stderr)
        for kind, rows, expected in (('int', [[3], [10]], ['4', '11']),
                                     ('bool', [[True], [False]], ['false', 'true'])):
            with self.subTest(kind=kind):
                process = self.invoke(rows, source=OVERLOADED, options=('--param-types', kind))
                self.assertEqual(process.returncode, 0, process.stderr)
                self.assertEqual(process.stderr, '')
                report = json.loads(process.stdout)
                self.assertEqual([row['result'] for row in report['rows']], expected)
                self.assertEqual(report['source_binding']['entry']['param_types'], [kind])

    def test_boolean_as_integer_and_malformed_row_shapes_are_rejected(self):
        for rows, expected in (([[True]], 'exact int'), ([[1, 2]], 'arity differs'),
                               ({'x': 1}, 'list/tuple')):
            with self.subTest(rows=rows):
                process = self.invoke(rows)
                self.assertEqual(process.returncode, 1)
                self.assertEqual(process.stdout, '')
                self.assertIn(expected, process.stderr)

    def test_zero_fuel_returns_ordered_json_row_errors_and_failure_exit(self):
        process = self.invoke([[3], [4]], options=('--fuel', '0'))
        self.assertEqual(process.returncode, 1, process.stderr)
        self.assertEqual(process.stderr, '')
        report = json.loads(process.stdout)
        self.assertEqual(report['rows'], [
            {'result': None, 'stdout': '', 'error': 'execution step budget exhausted',
             'error_line': 0, 'exit_code': 1} for _ in range(2)])
        self.assertEqual(report['execution']['fuel_per_lane'], 0)
        self.assertEqual(report['execution']['compiled_entries_entered'], 2)


if __name__ == '__main__':
    unittest.main()
