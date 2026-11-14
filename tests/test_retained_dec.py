"""Independent finite geometry and retained-update compiler conformance."""
from itertools import permutations
import unittest

from runtime.dec_language import DecError, Interpreter, compile_source
from simulations.retained_dec import geometry_source, reference_call, exact_integer_lines


class RetainedDecGeometryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.program = compile_source(geometry_source() +
            'Dec main() -> result:\n    () -> int result:\n        result = 0\n')
        cls.vm = Interpreter(cls.program, max_steps=20_000_000)

    def call(self, function, args):
        return {'result': str(self.vm.call(function, list(args)))}

    def test_all_interface_masks_match_explicit_h1_walk_inventory(self):
        for ancestor in range(4):
            for mask in range(16):
                expected = []
                for number, axes in enumerate(permutations(range(4), 3)):
                    a, b, c = axes
                    vertices = (0, 1 << a, (1 << a) ^ (1 << b),
                                (1 << a) ^ (1 << b) ^ (1 << c),
                                (1 << b) ^ (1 << c), 1 << c, 0)
                    steps = [left ^ right for left, right in zip(vertices, vertices[1:])]
                    self.assertTrue(all(step.bit_count() == 1 for step in steps))
                    self.assertEqual(len(set(vertices[:-1])), 6)
                    if steps[0] == 1 << ancestor or any(not step & mask for step in steps):
                        continue
                    expected.append(number)
                got = self.call('word_count', (ancestor, mask))
                self.assertEqual(int(got['result']), len(expected))
                for rank, word in enumerate(expected):
                    got = self.call('word_select', (ancestor, mask, rank))
                    self.assertEqual(int(got['result']), word)
                    last = self.call('word_last_axis', (word,))
                    self.assertEqual(int(last['result']), tuple(permutations(range(4), 3))[word][-1])

    def test_blocked_and_out_of_domain_geometry_cannot_choose_a_return(self):
        for function, args in [('word_select', (0, 0, 0)), ('word_select', (0, 15, 18)),
                               ('word_count', (-1, 15)), ('word_count', (0, 16)),
                               ('word_last_axis', (24,))]:
            with self.subTest(function=function, args=args), self.assertRaises(DecError):
                self.call(function, args)
        with self.assertRaises(DecError):
            self.call('word_count', (True, 15))

    def test_output_parser_rejects_lossy_or_ambiguous_numbers(self):
        self.assertEqual(exact_integer_lines('0\n-3\n4\n'), (0, -3, 4))
        for bad in ('1.0\n', 'True\n', '01\n', ' 1\n', '1/1\n'):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                exact_integer_lines(bad)


class RetainedDecUpdateTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        from simulations import retained_continuation as host
        from simulations import retained_dec as dec
        cls.host, cls.dec = host, dec
        cls.source = dec.program_source([{'initial': host.initial_state(), 'inputs': ()}], host.TICKET_DENOMINATOR)
        cls.program = compile_source(cls.source)
        cls.lines = []
        cls.vm = Interpreter(cls.program, emit=cls.lines.append, max_steps=30_000_000)

    def compiled(self, state, values):
        self.lines.clear()
        result = self.vm.call('counted_step', [self.dec.encode_state(state), *values])
        rows = self.dec.output_rows('\n'.join(self.lines) + '\n')
        self.assertEqual(len(rows), 1)
        self.assertEqual(result, rows[0]['next_state'])
        return self.dec.transition_output(rows[0])

    def test_recursive_expiry_and_new_retention_use_computed_feedback(self):
        d = self.host.TICKET_DENOMINATOR
        state = self.host.initial_state()
        masks = [(15,15),(0,15),(0,15),(15,15),(15,15),(15,15)]
        states = []
        for ma, mb in masks:
            values = (ma,mb,0,d-1)
            expected = self.host.transition(state,*values)
            got = self.compiled(state,values)
            self.assertEqual(got,expected)
            state = got['next_state']
            states.append(state)
        self.assertEqual(states[2][12:20], (0,0,0,0,0,0,0,0))
        self.assertEqual(states[3][13], 0)
        self.assertEqual(states[4][13], 1)
        self.assertNotEqual(states[4][12], 0)
        self.assertEqual(states[-1][0], 6)

    def test_variable_totals_and_all_recipe_selection_boundaries(self):
        d = self.host.TICKET_DENOMINATOR
        initial = self.host.initial_state()
        retained = self.host.transition(initial,15,15,0,d-1)['next_state']
        cancelling = list(retained)
        cancelling[12:16] = [6,1,-6,1]
        for state in (initial,retained,tuple(cancelling)):
            for masks in ((15,15),(7,11),(0,15)):
                baseline = self.host.transition(state,*masks,0,0)
                points = {0,d-1}
                for total,geometry,counts in zip(baseline['totals'],baseline['geometry_counts'],baseline['counts']):
                    if total:
                        self.assertEqual(d % total,0)
                        # First/last ticket of every handed recipe interval;
                        # these also select first/last token ranks in that word.
                        cumulative = 0
                        for multiplicity in counts:
                            for _ in range(geometry):
                                points.update((cumulative*(d//total),
                                               (cumulative+multiplicity)*(d//total)-1))
                                cumulative += multiplicity
                for ticket in sorted(points):
                    values = (*masks,ticket,d-1-ticket)
                    self.assertEqual(self.compiled(state,values),self.host.transition(state,*values))

    def test_backend_flags_cannot_coerce_noncanonical_truth(self):
        self.compiled(self.host.initial_state(),(15,15,0,0))
        row = self.dec.output_rows('\n'.join(self.lines)+'\n')[0]
        for field in ('contact','new_presence'):
            for value in (True,2,1.0):
                forged = dict(row);forged[field] = value
                with self.subTest(field=field,value=value), self.assertRaises(ValueError):
                    self.dec.transition_output(forged)

    def test_state_codec_and_exact_boundary_rejections(self):
        initial = self.host.initial_state()
        self.assertEqual(self.dec.decode_state(self.dec.encode_state(initial)),initial)
        for index,(name,low,high) in enumerate(self.dec.STATE_SCHEMA):
            for value in (low,high):
                state = list(initial);state[index] = value
                self.assertEqual(self.dec.decode_state(self.dec.encode_state(tuple(state))),tuple(state))
        with self.assertRaises(ValueError):
            self.dec.encode_state((True,*initial[1:]))
        with self.assertRaises(ValueError):
            self.dec.decode_state(10**100)
        for state,values in [(initial,(15,15,self.host.TICKET_DENOMINATOR,0)),
                             (initial,(16,15,0,0)),
                             ((6,*initial[1:]),(15,15,0,0))]:
            with self.assertRaises(DecError):
                self.compiled(state,values)
        with self.assertRaises(DecError):
            self.vm.call('counted_step',[self.dec.encode_state(initial),15,15,True,0])



if __name__ == '__main__':
    unittest.main()
