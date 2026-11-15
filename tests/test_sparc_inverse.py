from dataclasses import replace
from fractions import Fraction as Q
import unittest

from simulations.sparc_inputs import BinKey, PreparationRow, TargetRow
from simulations.sparc_inverse import (InverseError, baseline_terms, baseline_projections,
    inverse_requirements, fit_effective_factors)


class SparcInverseTests(unittest.TestCase):
    def prep(self, galaxy='A', radius=1, gas=1, disk=0, bulge=0):
        key = BinKey(galaxy, Q(radius))
        return PreparationRow(key, Q(gas), Q(disk), Q(bulge), Q(0), Q(0), Q(1))

    def target(self, prep, speed, error=0):
        return TargetRow(prep.key, Q(speed), Q(error))

    def fit(self, rows, speeds):
        prep = {row.key: row for row in rows}
        targets = {row.key: self.target(row, speed) for row, speed in zip(rows, speeds)}
        return fit_effective_factors(prep, targets, source_evidence=('exact fixture',))

    def test_frozen_baseline_independent_exact_values_and_no_targets(self):
        row = self.prep(radius=3, gas=-2, disk=4, bulge=10)
        self.assertEqual(baseline_terms(row), {'u_bar': Q(82), 'base_dbf': Q(48), 'u_g0': Q(466)})
        prediction, = baseline_projections({row.key: row}, source_evidence=('fixture',))
        self.assertEqual(prediction.u_km2_s2, 466)
        self.assertEqual(prediction.unit, 'km^2/s^2')
        self.assertIsNone(prediction.absolute_error_bound)

    def test_negative_requirement_is_retained_without_clipping(self):
        row = self.prep(radius=3, gas=-2, disk=4, bulge=10)
        target = self.target(row, 1, 2)
        result = inverse_requirements({row.key: row}, {row.key: target})['rows'][0]
        self.assertEqual(result['required_dbf'], -Q(81, 8))
        self.assertEqual(result['required_effective_factor'], -Q(27, 128))
        self.assertEqual(result['factor_status'], 'requires_negative_factor')
        self.assertFalse(result['nonnegative_factor_feasible'])
        self.assertEqual(result['nominal_required_dbf_interval'], (-Q(41, 4), -Q(73, 8)))
        self.assertIsNone(result['unknown_model_and_systematic_uncertainty'])

    def test_positive_inverse_and_nominal_interval_exact(self):
        row = self.prep(radius=3, gas=-2, disk=4, bulge=10)
        target = self.target(row, 22, 1)
        result = inverse_requirements({row.key: row}, {row.key: target})['rows'][0]
        self.assertEqual(result['required_dbf'], Q(201, 4))
        self.assertEqual(result['required_effective_factor'], Q(67, 64))
        self.assertEqual(result['nominal_required_dbf_interval'], (Q(359, 8), Q(447, 8)))

    def test_zero_support_is_infeasible_and_fit_is_unidentified(self):
        row = self.prep(gas=0)
        target = self.target(row, 1)
        result = inverse_requirements({row.key: row}, {row.key: target})['rows'][0]
        self.assertEqual(result['required_dbf'], Q(1, 8))
        self.assertEqual(result['factor_status'], 'infeasible_zero_support')
        self.assertIsNone(result['required_effective_factor'])
        self.assertIsNone(result['nominal_required_factor_interval'])
        fit = self.fit([row], [1])
        self.assertIsNone(fit['factors']['A']['value'])
        self.assertEqual(fit['factors']['A']['minimizer_interval'], (0, None))
        self.assertEqual(fit['comparison']['primary_mape_percent'], 100)

    def test_weighted_median_is_exact_convex_minimizer_and_baseline_stays_fixed(self):
        rows = [self.prep(radius=1), self.prep(radius=2)]
        # A: Uobs=9,b=8,r=1,w=8/9; B: Uobs=25,b=16,r=3/2,w=16/25.
        baseline = baseline_projections({r.key: r for r in rows}, source_evidence=('fixture',))
        fit = self.fit(rows, [3, 5])
        factor = fit['factors']['A']['value']
        self.assertEqual(factor, 1)
        self.assertEqual(fit['comparison']['primary_mape_percent'], 16)
        objective = lambda f: (abs(1 + 8*f - 9) / Q(9) + abs(1 + 16*f - 25) / Q(25)) / 2
        for candidate in (Q(0), Q(1, 2), Q(1), Q(3, 2), Q(2), Q(100)):
            self.assertLessEqual(objective(factor), objective(candidate))
        self.assertEqual(baseline_projections({r.key: r for r in rows}, source_evidence=('fixture',)), baseline)
        self.assertEqual(fit['exposure'], 'target-conditioned in-sample fit')

    def test_weighted_median_tie_returns_complete_minimizer_interval(self):
        rows = [self.prep(radius=Q(9, 8)), self.prep(radius=Q(25, 8))]
        fit = self.fit(rows, [3, 5])
        factor = fit['factors']['A']
        self.assertEqual(factor['value'], Q(8, 9))
        self.assertEqual(factor['minimizer_interval'], (Q(8, 9), Q(24, 25)))
        loss = lambda f: abs(1+9*f-9)/Q(9) + abs(1+25*f-25)/Q(25)
        self.assertEqual(loss(Q(8, 9)), loss(Q(24, 25)))
        self.assertEqual(loss(Q(8, 9)), loss(Q(9, 10)))

    def test_nonnegative_constraint_and_zero_support_rows_keep_constant_loss(self):
        supported = self.prep(gas=2)
        zero = self.prep(radius=2, gas=0)
        fit = self.fit([supported, zero], [1, 1])
        self.assertEqual(fit['factors']['A']['value'], 0)
        self.assertEqual(fit['factors']['A']['minimizer_interval'], (0, 0))
        self.assertEqual(fit['factors']['A']['informative_bins'], 1)
        self.assertEqual(fit['comparison']['primary_mape_percent'], 200)

    def test_galaxy_fit_is_separate_and_types_alignment_are_strict(self):
        a, b = self.prep('A'), self.prep('B')
        fit = self.fit([a, b], [3, 5])
        self.assertEqual(fit['factors']['A']['value'], 1)
        self.assertEqual(fit['factors']['B']['value'], 3)
        self.assertEqual(fit['comparison']['primary_mape_percent'], 0)
        for value in (True, 1.0, '1'):
            with self.subTest(value=value), self.assertRaises(InverseError):
                baseline_terms(replace(a, vgas_km_s=value))
        for targets in ({}, {b.key: self.target(b, 1)}, {a.key: self.target(b, 1)}):
            with self.subTest(targets=targets), self.assertRaises(InverseError):
                inverse_requirements({a.key: a}, targets)
        for speed in (True, 1.0, 0, -1):
            with self.subTest(speed=speed), self.assertRaises(InverseError):
                inverse_requirements({a.key: a}, {a.key: TargetRow(a.key, speed, Q(0))})


if __name__ == '__main__':
    unittest.main()
