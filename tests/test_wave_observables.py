import unittest
from dataclasses import replace
from fractions import Fraction as F

from simulations.wave_observables import WavelengthMap, wavelength_report


class WavelengthObservationTests(unittest.TestCase):
    def setUp(self):
        self.mapping = WavelengthMap(F(1, 5), F(1), F(12), "fixture",
                                     "synthetic dimensional control", "synthetic_control")

    def test_missing_map_keeps_physical_length_unknown(self):
        report = wavelength_report((10, 10), native_scope="fixture")
        self.assertEqual(report["status"], "NOT_INSTANTIATED")
        self.assertIsNone(report["wavelength_m"])

    def test_declared_map_has_independent_exact_dimensional_values(self):
        report = wavelength_report((10, 20), native_scope="fixture", observation_map=self.mapping)
        self.assertEqual([(r["elapsed_s"], r["phase_frequency_hz"], r["wavelength_m"])
                          for r in report["rows"]], [(F(2), F(1, 2), F(24)), (F(4), F(1, 4), F(48))])
        self.assertIsNone(report["measured_target"])
        self.assertIsNone(report["uncertainty"]["standard_uncertainty_m"])

    def test_phase_cycles_are_explicit_and_not_inferred_from_coordinates(self):
        report = wavelength_report((10,), native_scope="fixture",
                                   observation_map=replace(self.mapping, phase_cycles_per_beat=F(2)))
        self.assertEqual(report["rows"][0]["wavelength_m"], 12)
        self.assertEqual(report["rows"][0]["phase_frequency_hz"], 1)

    def test_wrong_scope_and_inexact_or_nonpositive_operands_fail(self):
        with self.assertRaises(ValueError):
            wavelength_report((10,), native_scope="another", observation_map=self.mapping)
        for value in (True, 0.2, "1/5", 0, -1):
            with self.assertRaises(ValueError):
                replace(self.mapping, seconds_per_native_index=value)
        for cadence in ((True,), (1.0,), (0,), (-1,)):
            with self.assertRaises(ValueError):
                wavelength_report(cadence, native_scope="fixture")


if __name__ == "__main__":
    unittest.main()
