"""Independent analytic and conservation checks of the continuous reference."""
from decimal import Decimal, localcontext
from fractions import Fraction as F
import unittest

from simulations.cell_transport import ModelConfig, flatten_state, interval_rates, make_schedule, simulate
from simulations.tesseract_cells import FACETS, facet_interfaces
from simulations.transport_reference import (
    compare_precisions, exponential_action, flatten_reference, generator_matrix,
    simulate_reference,
)


class ContinuousReferenceTests(unittest.TestCase):
    def assertSmall(self, value, threshold="1e-48"):
        self.assertLess(abs(value), Decimal(threshold))

    def test_isolated_emitter_matches_analytic_decay(self):
        config = ModelConfig(intervals=3, exchange=F(0), leak=F(0), absorption=F(0),
                             pump=F(0), gamma=F(1, 4))
        result = simulate_reference(config)
        with localcontext() as context:
            context.prec = 80
            expected = (-Decimal(3) / Decimal(4)).exp()
            self.assertSmall(result["final"]["emitter"] - expected)
            self.assertSmall(result["final"]["cells"][FACETS.index(config.source)] - (1 - expected))
        self.assertEqual(result["final"]["output"], 0)
        self.assertEqual(result["final"]["loss"], 0)

    def test_isolated_output_matches_analytic_leak(self):
        initial = tuple(F(1) if cell == "1***" else F(0) for cell in FACETS)
        config = ModelConfig(intervals=2, exchange=F(0), leak=F(1, 3), absorption=F(0),
                             pump=F(0), gamma=F(0), initial_emitter=F(0), initial_cells=initial)
        result = simulate_reference(config)
        with localcontext() as context:
            context.prec = 80
            retained = (-Decimal(2) / Decimal(3)).exp()
            self.assertSmall(result["final"]["cells"][FACETS.index(config.output)] - retained)
            self.assertSmall(result["final"]["output"] - (1 - retained))

    def test_affine_pump_and_injection_match_linear_solution(self):
        config = ModelConfig(intervals=3, exchange=F(0), leak=F(0), absorption=F(0),
                             pump=F(1, 7), gamma=F(0), initial_emitter=F(0))
        result = simulate_reference(config)
        with localcontext() as context:
            context.prec = 80
            expected = Decimal(3) / Decimal(7)
            self.assertSmall(result["final"]["emitter"] - expected)
            self.assertSmall(result["final"]["injected"] - expected)

    def test_generator_conserves_total_with_recorded_injection(self):
        config = ModelConfig()
        schedule = make_schedule(config.intervals, amplitude=F(1, 2), seed=41)
        weights = (F(1),) * 11 + (F(-1), F(0))
        for interval in range(config.intervals):
            matrix = generator_matrix(config, interval_rates(config, schedule, interval))
            for column in range(13):
                self.assertEqual(sum(weights[row] * matrix[row][column] for row in range(13)), 0)
        # Opposite cubic facets have no shared-square exchange.
        self.assertEqual(matrix[1 + FACETS.index("0***")][1 + FACETS.index("1***")], 0)

    def test_shared_square_rates_enter_both_directions(self):
        config = ModelConfig()
        rates = tuple(F(index + 1, 100) for index in range(24))
        matrix = generator_matrix(config, rates)
        for face, rate in zip(facet_interfaces(), rates):
            left, right = 1 + FACETS.index(face.left), 1 + FACETS.index(face.right)
            self.assertEqual(matrix[left][right], rate)
            self.assertEqual(matrix[right][left], rate)

    def test_exponential_semigroup_for_constant_generator(self):
        config = ModelConfig(intervals=1)
        matrix = generator_matrix(config, interval_rates(config, make_schedule(1), 0))
        initial = (config.initial_emitter, *config.initial_cells, 0, 0, 0, 1)
        full = exponential_action(matrix, initial, precision=80)["vector"]
        half = exponential_action(matrix, initial, duration=F(1, 2), precision=80)["vector"]
        split = exponential_action(matrix, half, duration=F(1, 2), precision=80)["vector"]
        with localcontext() as context:
            context.prec = 80
            for a, b in zip(full, split):
                self.assertSmall(a - b, "1e-68")

    def test_precision_comparison_and_euler_first_order(self):
        config = ModelConfig(intervals=2)
        schedule = make_schedule(2, amplitude=F(1, 4), seed=17)
        comparison = compare_precisions(config, schedule)
        self.assertSmall(comparison["max_absolute_difference"])
        reference = flatten_reference(comparison["higher"])
        errors = []
        with localcontext() as context:
            context.prec = 80
            for substeps in (16, 32):
                discrete = flatten_state(simulate(config, substeps, schedule, retain_steps=False))
                errors.append(sum(abs(Decimal(x.numerator) / Decimal(x.denominator) - y)
                                  for x, y in zip(discrete, reference)))
            self.assertGreater(errors[0], 0)
            self.assertLess(errors[1], errors[0])
            self.assertGreater(errors[0] / errors[1], Decimal("1.8"))
            self.assertLess(errors[0] / errors[1], Decimal("2.2"))

    def test_exponential_input_rejections_and_term_limit(self):
        with self.assertRaises(ValueError):
            exponential_action(((F(1), F(2)),), (1,))
        with self.assertRaises(ValueError):
            exponential_action(((F(1),),), (1,), duration=F(-1))
        with self.assertRaises(TypeError):
            exponential_action(((0.1,),), (1,))
        with self.assertRaises(ArithmeticError):
            exponential_action(((F(2),),), (1,), max_terms=1)
        zero = exponential_action(((F(0),),), (F(2, 3),))
        self.assertEqual(zero["tail_estimate"], 0)
        self.assertFalse(zero["rounding_included_in_tail"])


if __name__ == "__main__":
    unittest.main()
