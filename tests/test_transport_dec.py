"""Check scalar Dec translation against exact values and the pair-transfer engine."""
from fractions import Fraction as F
import unittest

from runtime.dec_core import render
from runtime.dec_language import Interpreter, compile_source
from simulations.cell_transport import ModelConfig, flatten_state, make_schedule, simulate
from simulations.transport_dec import MAX_EXPANDED_STEPS, dec_source


def execute(source):
    emitted = []
    result = Interpreter(compile_source(source), emit=emitted.append).run()
    return "\n".join((*emitted, render(result))) + "\n"


def expected_stdout(values):
    return "".join(f"{F(value).numerator}/{F(value).denominator}\n" for value in values) + "0\n"


class TransportDecTests(unittest.TestCase):
    def test_independent_one_step_requires_old_state_updates(self):
        # Source and output are the same cell. Newly emitted mass must not also
        # leak or be absorbed until the next step; all updates read old values.
        config = ModelConfig(intervals=1, output="0***")
        source = dec_source(config)
        expected = (F(1), F(1, 4), *((F(0),) * 7), F(0), F(0), F(1, 4))
        self.assertEqual(execute(source), expected_stdout(expected))

    def test_independent_isolated_emitter_geometric_solution(self):
        config = ModelConfig(intervals=3, exchange=0, leak=0, absorption=0, pump=0)
        remaining = (1 - F(1, 4) / 2) ** 6
        expected = (remaining, 1 - remaining, *((F(0),) * 7), F(0), F(0), F(0))
        self.assertEqual(execute(dec_source(config, 2)), expected_stdout(expected))

    def test_jittered_matrix_translation_matches_pair_transfer_engine(self):
        config = ModelConfig(intervals=2, initial_cells=tuple(F(i, 28) for i in range(8)))
        for amplitude, substeps in ((F(0), 1), (F(1, 4), 2)):
            schedule = make_schedule(config.intervals, amplitude, 7)
            expected = flatten_state(simulate(config, substeps, schedule))
            source = dec_source(config, substeps, schedule)
            with self.subTest(amplitude=amplitude, substeps=substeps):
                self.assertEqual(execute(source), expected_stdout(expected))
                self.assertIn(schedule["sha256"], source)

    def test_zero_jitter_ignores_seed_and_generation_is_deterministic(self):
        config = ModelConfig(intervals=1)
        a, b = make_schedule(1, 0, 2), make_schedule(1, 0, 999)
        first = dec_source(config, 2, a)
        self.assertEqual(first, dec_source(config, 2, a))
        self.assertEqual(execute(first), execute(dec_source(config, 2, b)))

    def test_invalid_and_unstable_generation_fails(self):
        with self.assertRaises(TypeError):
            dec_source(config={})
        for value in (0, -1, True, F(1, 2)):
            with self.subTest(substeps=value), self.assertRaises(ValueError):
                dec_source(substeps=value)
        with self.assertRaisesRegex(ValueError, "expanded steps"):
            dec_source(ModelConfig(intervals=1), MAX_EXPANDED_STEPS + 1)
        with self.assertRaisesRegex(ValueError, "stability"):
            dec_source(ModelConfig(exchange=1))
        with self.assertRaisesRegex(ValueError, "checksum"):
            schedule = make_schedule(4)
            schedule["sha256"] = "0" * 64
            dec_source(schedule=schedule)


if __name__ == "__main__":
    unittest.main()
