import unittest
from fractions import Fraction as Q

from simulations.sparc_inputs import BinKey, TargetRow
from simulations.sparc_comparison import ComparisonError, Projection, compare


class SparcComparisonTests(unittest.TestCase):
    def target(self, galaxy, radius, speed, error=0):
        key = BinKey(galaxy, Q(radius))
        return TargetRow(key, Q(speed), Q(error))

    def projection(self, target, value, **kwargs):
        return Projection(target.key, value, 'declared radial projection', ('fixture-source',), **kwargs)

    def test_independent_toy_exact_primary_and_galaxy_companion(self):
        # A losses 0%, 100%; B loss 25%. Bin mean = 125/3%, galaxy mean = 75/2%.
        targets = [self.target('A', 1, 2), self.target('A', 2, 2), self.target('B', 1, 4)]
        predictions = [self.projection(t, value) for t, value in zip(targets, (4, 0, 20))]
        result = compare({t.key: t for t in targets}, reversed(predictions))
        self.assertEqual(result['primary_mape_percent'], Q(125, 3))
        self.assertEqual(result['equal_galaxy_mape_percent'], Q(75, 2))
        self.assertEqual(result['galaxies']['A']['underpredicted_bins'], 1)
        self.assertEqual(result['galaxies']['B']['overpredicted_bins'], 1)
        self.assertEqual(result['rows'][1]['signed_percentage_residual'], -100)
        self.assertEqual(result['rows'][1]['absolute_percentage_error'], 100)
        self.assertEqual(result['rows'][2]['signed_residual'], 4)

    def test_alignment_rejects_missing_extra_duplicate_and_mismatched_row(self):
        a, b = self.target('A', 1, 2), self.target('B', 1, 2)
        projection = self.projection(a, 4)
        for predictions in ([], [projection, projection], [projection, self.projection(b, 4)]):
            with self.subTest(predictions=predictions), self.assertRaises(ComparisonError):
                compare({a.key: a}, predictions)
        with self.assertRaises(ComparisonError):
            compare({a.key: b}, [projection])
        with self.assertRaises(ComparisonError):
            compare({}, [])

    def test_projection_requires_exact_nonnegative_value_and_squared_units(self):
        target = self.target('A', 1, 2)
        for value in (True, False, 1.0, '1', -1):
            with self.subTest(value=value), self.assertRaises(ComparisonError):
                self.projection(target, value)
        for unit in ('km/s', 'm^2/s^2', None):
            with self.subTest(unit=unit), self.assertRaises(ComparisonError):
                self.projection(target, 4, unit=unit)
        for bound in (True, 0.0, -1):
            with self.subTest(bound=bound), self.assertRaises(ComparisonError):
                self.projection(target, 4, absolute_error_bound=bound)

    def test_map_and_evidence_are_explicit(self):
        target = self.target('A', 1, 2)
        for observation_map, evidence in (('', ('source',)), ('map', ()), ('map', ['source']), ('map', ('',))):
            with self.subTest(map=observation_map, evidence=evidence), self.assertRaises(ComparisonError):
                Projection(target.key, 4, observation_map, evidence)

    def test_large_measurement_error_keeps_target_and_unknown_uncertainties(self):
        target = self.target('A', 1, 2, 3)
        result = compare({target.key: target}, [self.projection(target, 4)])
        row = result['rows'][0]
        self.assertEqual(result['bins'], 1)
        self.assertEqual(result['primary_mape_percent'], 0)
        self.assertEqual(row['relative_speed_error'], Q(3, 2))
        self.assertEqual(row['sigma_u_linear'], 12)
        self.assertEqual(row['nominal_square_speed_interval'], (0, 25))
        self.assertEqual(row['uncertainty_flags'], ('large_relative_speed_error', 'nominal_speed_interval_reaches_zero'))
        for name in ('projection_uncertainty', 'inclination_systematics', 'model_discrepancy'):
            self.assertEqual(row[name], {'status': 'unknown', 'bound': None})

    def test_error_threshold_is_inclusive_and_exact_and_interval_flag_independent(self):
        target = self.target('A', 1, 2, 1)
        prediction = self.projection(target, 4, absolute_error_bound=Q(1, 3))
        row = compare({target.key: target}, [prediction])['rows'][0]
        self.assertEqual(row['uncertainty_flags'], ('large_relative_speed_error',))
        self.assertEqual(row['projection_uncertainty'], {'status': 'bounded', 'bound': Q(1, 3)})
        self.assertEqual(row['nominal_square_speed_interval'], (1, 9))
        for threshold in (True, 0.5, 0, -1):
            with self.subTest(threshold=threshold), self.assertRaises(ComparisonError):
                compare({target.key: target}, [prediction], large_relative_error_threshold=threshold)
        target = self.target('A', 1, 1, 1)
        row = compare({target.key: target}, [self.projection(target, 1)], large_relative_error_threshold=2)['rows'][0]
        self.assertEqual(row['uncertainty_flags'], ('nominal_speed_interval_reaches_zero',))


if __name__ == '__main__':
    unittest.main()
