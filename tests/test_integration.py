"""End-to-end language regressions; invoke the public command line."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
CLI = ROOT / 'aodc.py'

def run(*args):
    return subprocess.run([sys.executable, str(CLI), *map(str, args)],
                          cwd=ROOT, capture_output=True, timeout=15)

class EndToEnd(unittest.TestCase):
    def test_all_worked_examples(self):
        for source in sorted((ROOT / 'examples').glob('*.dec')):
            with self.subTest(source=source.name):
                r = run('run', source)
                self.assertEqual(r.returncode, 0, r.stderr)
                self.assertTrue(r.stdout)

    def test_source_and_ir_have_identical_behavior(self):
        with tempfile.TemporaryDirectory() as d:
            for source in sorted((ROOT / 'examples').glob('*.dec')):
                with self.subTest(source=source.name):
                    target = Path(d) / (source.stem + '.json')
                    r = run('compile', source, '-o', target)
                    self.assertEqual(r.returncode, 0, r.stderr)
                    direct, compiled = run('run', source), run('exec', target)
                    self.assertEqual(compiled.returncode, 0, compiled.stderr)
                    self.assertEqual(direct.stdout, compiled.stdout)

    def test_repeated_compilation_is_byte_identical(self):
        with tempfile.TemporaryDirectory() as d:
            blobs = []
            for i in range(20):
                target = Path(d) / f'{i}.json'
                r = run('compile', ROOT / 'examples/closure.dec', '-o', target)
                self.assertEqual(r.returncode, 0, r.stderr)
                blobs.append(target.read_bytes())
            self.assertEqual(len(set(blobs)), 1)

    def test_body_changes_change_executed_result(self):
        with tempfile.TemporaryDirectory() as d:
            outputs = []
            for n in (1, 99):
                f = Path(d) / f'{n}.dec'
                f.write_text(f'Dec main() -> result:\n    () -> t32 result:\n        emit t32({n})\n        result = t32(0)\n')
                r = run('run', f)
                self.assertEqual(r.returncode, 0, r.stderr)
                outputs.append(r.stdout)
            self.assertNotEqual(*outputs)

    def test_missing_source_and_output_directory_fail(self):
        with tempfile.TemporaryDirectory() as d:
            missing = Path(d) / 'absent.dec'
            self.assertNotEqual(run('check', missing).returncode, 0)
            self.assertNotEqual(run('run', missing).returncode, 0)
            self.assertNotEqual(run('compile', ROOT / 'examples/ternary.dec', '-o', Path(d)/'absent/out.json').returncode, 0)

    def test_invalid_ir_fails_without_execution(self):
        with tempfile.TemporaryDirectory() as d:
            f = Path(d)/'bad.json'
            for text in ('not json', '{}', '{"format":"dec-ir","functions":[]}'):
                f.write_text(text)
                self.assertNotEqual(run('exec', f).returncode, 0)

    def test_unsupported_specification_is_not_certified(self):
        for f in sorted((ROOT / 'dec').glob('*.dec')):
            with self.subTest(source=f.name):
                self.assertNotEqual(run('check', f).returncode, 0)

    def test_no_arguments_is_a_normal_usage_error(self):
        r = run()
        self.assertGreater(r.returncode, 0)
        self.assertNotIn(b'Traceback', r.stderr)

if __name__ == '__main__':
    unittest.main()
