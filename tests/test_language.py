import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from runtime import dec_core as core
from runtime.dec_language import DecError, Interpreter, compile_source, dumps_ir, loads_ir, run_source

ROOT = Path(__file__).resolve().parents[1]

def program(body, result='t32'):
    return f'Dec main() -> result:\n    () -> {result} result:\n'+''.join('        '+s+'\n' for s in body.splitlines())

class LanguageTests(unittest.TestCase):
    def test_executes_body_and_distinguishes_literals(self):
        a=program('result = t32(1)'); b=program('result = t32(99)')
        self.assertEqual(core.render(run_source(a)), 't32(1)')
        self.assertEqual(core.render(run_source(b)), 't32(99)')
        self.assertNotEqual(dumps_ir(compile_source(a)),dumps_ir(compile_source(b)))

    def test_dispatches_exact_overload(self):
        s='''Dec twice(x) -> y:
    (int x) -> int y:
        y = x + x
    (t32 x) -> t32 y:
        y = x + x
Dec main() -> result:
    () -> t32 result:
        int a = twice(3)
        assert a == 6
        result = twice(t32(4))
'''
        self.assertEqual(core.render(run_source(s)),'t32(8)')

    def test_forward_calls_are_checked(self):
        s=program('result = later(t32(7))')+'Dec later(x) -> y:\n    (t32 x) -> t32 y:\n        y = x\n'
        self.assertEqual(core.render(run_source(s)),'t32(7)')

    def test_exact_ratio_expression(self):
        s=program('ratio a = ratio(1, 4000000000)\nratio b = a + a\nassert b == ratio(1, 2000000000)\nresult = t32(0)')
        self.assertEqual(core.render(run_source(s)),'t32(0)')

    def test_emit_and_conditional(self):
        output=[]
        s=program('t1 branch = t1(-1)\nemit branch\nresult = t32(2) if branch < t1(0) else t32(3)')
        self.assertEqual(core.render(run_source(s,emit=output.append)),'t32(2)')
        self.assertEqual(output,['t1(-1)'])

    def test_comments_preserve_hash_in_string(self):
        output=[]
        s=program('emit "scope#name" # comment\nresult = t32(0)')
        run_source(s,emit=output.append)
        self.assertEqual(output,['scope#name'])

    def test_boolean_short_circuit(self):
        s=program('assert true or (1 / 0 == ratio(0, 1))\nassert not false\nresult = t32(0)')
        self.assertEqual(core.render(run_source(s)),'t32(0)')

    def test_static_assert_false_executes_failure(self):
        p=compile_source(program('assert false\nresult = t32(0)'))
        with self.assertRaisesRegex(DecError,'assertion failed'): Interpreter(p).run()

    def test_unknown_types_fail(self):
        with self.assertRaisesRegex(DecError,'unknown type'): compile_source(program('result = 1','Missing'))

    def test_undefined_names_fail(self):
        with self.assertRaisesRegex(DecError,'undefined'): compile_source(program('result = t32(unbound)'))

    def test_unlisted_dispatch_fails(self):
        s='Dec f(a) -> r:\n    (t32 a) -> t32 r:\n        r = a\n'+program('result = f(1)')
        with self.assertRaisesRegex(DecError,'unlisted signature'): compile_source(s)

    def test_no_implicit_numeric_coercion(self):
        with self.assertRaisesRegex(DecError,'matching explicit numeric types'): compile_source(program('result = t32(1) + 1'))
        with self.assertRaisesRegex(DecError,'assignment'): compile_source(program('result = 1'))

    def test_return_type_cannot_disambiguate_inputs(self):
        s='Dec f(a) -> r:\n    (int a) -> int r:\n        r = a\n    (int a) -> t32 r:\n        r = t32(a)\n'+program('result = t32(0)')
        with self.assertRaisesRegex(DecError,'duplicate input'): compile_source(s)

    def test_unsupported_code_never_silently_passes(self):
        for code in ['struct secret { value: int }\n',program('for x in 0..4:\nresult = t32(0)'),program('result = t32(1.5)'),program('result = t32((1).__class__)'),program('result = t32([1][0])')]:
            with self.subTest(code=code),self.assertRaises(DecError): compile_source(code)

    def test_unreachable_function_checked(self):
        s=program('result = t32(0)')+'Dec unused(a) -> b:\n    (int a) -> int b:\n        b = typo(a)\n'
        with self.assertRaisesRegex(DecError,'undefined function'): compile_source(s)

    def test_unassigned_result_fails(self):
        with self.assertRaisesRegex(DecError,'never assigned'): compile_source(program('emit 1'))

    def test_missing_main_fails(self):
        with self.assertRaisesRegex(DecError,'main'): compile_source('Dec f(a) -> b:\n    (int a) -> int b:\n        b = a\n')

    def test_duplicate_local_fails(self):
        with self.assertRaisesRegex(DecError,'duplicate local'): compile_source(program('int a = 1\nint a = 2\nresult = t32(a)'))

    def test_reserved_bindings_fail(self):
        for name in ['true', 'false', 'True', 'if']:
            with self.subTest(name=name), self.assertRaises(DecError):
                compile_source(program('int '+name+' = 1\nresult = t32(0)'))

    def test_explicit_return(self):
        self.assertEqual(core.render(run_source(program('return t32(8)'))),'t32(8)')
        with self.assertRaisesRegex(DecError,'unreachable'): compile_source(program('return t32(8)\nemit 7'))

    def test_ir_reproducible_roundtrip(self):
        source=program('result = t32(6)')
        a=dumps_ir(compile_source(source)); b=dumps_ir(compile_source(source))
        self.assertEqual(a,b)
        self.assertEqual(core.render(Interpreter(loads_ir(a)).run()),'t32(6)')

    def test_ir_mutations_rejected(self):
        p=compile_source(program('result = t32(6)'))
        for kind in ('digest','body','boolint','unknown'):
            bad=copy.deepcopy(p)
            if kind=='digest': bad['source_sha256']='0'*64
            if kind=='body': bad['functions'][0]['branches'][0]['body'][0]['value']['args'][0]['value']=9
            if kind=='boolint': bad['functions'][0]['branches'][0]['body'][0]['value']['args'][0]['value']=True
            if kind=='unknown': bad['unsafe']='execute me'
            with self.subTest(kind=kind),self.assertRaises(DecError): loads_ir(json.dumps(bad))
        with self.assertRaisesRegex(DecError,'duplicate IR key'): loads_ir('{"format":"aod.dec.ir","format":"aod.dec.ir"}')

    def test_direct_interpreter_cannot_bypass_ir_validation(self):
        p=compile_source(program('result = t32(1)')); p['source_sha256']='bogus'
        with self.assertRaises(DecError): Interpreter(p)

    def test_recursion_has_execution_limit(self):
        s='Dec recur(x) -> y:\n    (t32 x) -> t32 y:\n        y = recur(x)\n'+program('result = recur(t32(0))')
        with self.assertRaisesRegex(DecError,'depth|budget|nesting'): run_source(s,max_depth=8)

    def test_cli_missing_input_fails_without_artifact(self):
        with tempfile.TemporaryDirectory() as d:
            out=Path(d)/'out.json'
            p=subprocess.run([sys.executable,str(ROOT/'aodc.py'),'compile',str(Path(d)/'missing.dec'),'-o',str(out)],capture_output=True)
            self.assertNotEqual(p.returncode,0); self.assertFalse(out.exists())

    def test_cli_compile_execute_and_errors(self):
        with tempfile.TemporaryDirectory() as d:
            src=Path(d)/'main.dec'; ir=Path(d)/'main.json';src.write_text(program('result = t32(42)'))
            p=subprocess.run([sys.executable,str(ROOT/'aodc.py'),'compile',str(src),'-o',str(ir)],capture_output=True)
            self.assertEqual(p.returncode,0,p.stderr)
            p=subprocess.run([sys.executable,str(ROOT/'aodc.py'),'exec',str(ir)],capture_output=True,text=True)
            self.assertEqual((p.returncode,p.stdout),(0,'t32(42)\n'))
            p=subprocess.run([sys.executable,str(ROOT/'aodc.py'),'compile',str(src),'-o',str(Path(d)/'absent/out')],capture_output=True)
            self.assertNotEqual(p.returncode,0)
            p=subprocess.run([sys.executable,str(ROOT/'aodc.py'),'compile',str(src),'-o',str(src)],capture_output=True)
            self.assertNotEqual(p.returncode,0)

if __name__=='__main__': unittest.main()
