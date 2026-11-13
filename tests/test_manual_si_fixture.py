"""Independent exact expectations for the Manual's conditional phase/SI map."""

from copy import deepcopy
from fractions import Fraction as Q
import unittest

from simulations.manual_si_fixture import (
    HYDROGEN_HZ, build_fixture, conditional_prediction, phase_advance,
)


class ManualSIFixtureTests(unittest.TestCase):
    def test_observed_advance_and_split_have_independent_exact_answers(self):
        record = build_fixture()
        self.assertEqual(record["prediction"]["compared_advance_turns"], Q(7, 2))
        self.assertEqual(record["prediction"]["reference_advance_turns"], Q(17, 2))
        self.assertEqual(record["split_window"]["advances_turns"], [Q(13, 6), Q(4, 3)])
        self.assertEqual(record["split_window"]["sum_turns"], Q(7, 2))

    def test_conditional_si_results_have_independent_exact_answers(self):
        prediction = build_fixture()["prediction"]
        self.assertEqual(prediction["ratio"], Q(7, 17))
        self.assertEqual(prediction["duration_seconds"], Q(17, 18_385_263_540))
        self.assertEqual(prediction["mean_frequency_hz"], Q(64_348_422_390, 17))
        self.assertEqual(prediction["physical_correspondence"], "unestablished")
        self.assertIsNone(prediction["complete_physical_uncertainty"])

    def test_actual_targets_preserve_definition_measurement_and_uncertainty_roles(self):
        targets = build_fixture()["targets"]
        self.assertEqual(targets["caesium_definition"]["value"], Q(9_192_631_770))
        self.assertEqual(targets["caesium_definition"]["definition_uncertainty_hz"], 0)
        self.assertIsNone(targets["caesium_definition"]["realization_uncertainty_hz"])
        hydrogen = targets["hydrogen"]
        self.assertEqual(hydrogen["value"], Q(177_550_718_971, 125))
        self.assertEqual(hydrogen["reported_standard_uncertainty"]["value"], Q(1, 500))
        self.assertIsNone(hydrogen["reported_standard_uncertainty"]["finite_distribution"])
        self.assertEqual(hydrogen["role"], "measured_comparison_target")

    def test_target_only_change_preserves_prediction_and_does_not_create_residual(self):
        original = build_fixture()
        changed = build_fixture(hydrogen_target_override=HYDROGEN_HZ + Q(1, 10))
        self.assertEqual(original["prediction"], changed["prediction"])
        self.assertEqual(original["split_window"], changed["split_window"])
        self.assertEqual(original["hydrogen_reduction"], changed["hydrogen_reduction"])
        self.assertNotEqual(original["targets"], changed["targets"])
        self.assertEqual(changed["targets"]["hydrogen"]["role"], "synthetic_target_control")
        for record in (original, changed):
            self.assertIsNone(record["transport_to_hydrogen"]["prediction_hz"])
            self.assertIsNone(record["transport_to_hydrogen"]["residual_hz"])
            self.assertFalse(record["transport_to_hydrogen"]["manual_fixture_scored_against_hydrogen"])

    def test_hydrogen_printed_reduction_and_reporting_difference_are_exact(self):
        reduction = build_fixture()["hydrogen_reduction"]
        tables = reduction["tables"]
        self.assertEqual(tables["table_iv"]["corrected_frequency_hz"], Q(14_204_057_517_691, 10_000))
        self.assertEqual(tables["table_vi"]["corrected_frequency_hz"], Q(14_204_057_517_667, 10_000))
        self.assertEqual(reduction["printed_input_mean_hz"], Q(14_204_057_517_679, 10_000))
        self.assertEqual(reduction["published_rounded_mean_hz"], Q(1_420_405_751_768, 1_000))
        self.assertEqual(reduction["published_minus_printed_mean_hz"], Q(1, 10_000))
        self.assertEqual(reduction["display_difference_class"], "source_reporting_precision")
        uncertainty = reduction["reported_mean_uncertainty"]
        self.assertEqual(uncertainty["value_hz"], Q(1, 500))
        self.assertEqual(uncertainty["category"], "published_standard_uncertainty")
        self.assertEqual(uncertainty["coverage"], "one_sigma")
        self.assertIsNone(uncertainty["finite_distribution"])

    def test_hydrogen_reduction_preserves_correction_ownership_and_missing_inputs(self):
        reduction = build_fixture()["hydrogen_reduction"]
        table_vi = reduction["tables"]["table_vi"]
        self.assertEqual(table_vi["corrections_already_in_input"], ["NP3_reference_offset", "wall"])
        self.assertEqual(table_vi["already_in_input_owner"], "NP3_corrected_zero_wall_beat")
        added = {term["name"] for term in table_vi["printed_added_terms"]}
        self.assertNotIn("NP3_reference_offset", added)
        self.assertNotIn("wall", added)
        self.assertFalse(table_vi["raw_observations_available"])
        self.assertFalse(reduction["independent_atomic_prediction"])
        self.assertIsNone(reduction["complete_physical_uncertainty"])
        self.assertIn("raw microwave phase stream", reduction["missing_raw_inputs"])

    def test_invalid_phase_domains_and_nonphase_quantities_are_rejected(self):
        valid = build_fixture()["prediction"]["compared_record"]
        for key, value in (("phase_start", "1"), ("phase_end", "-1/8"),
                           ("phase_start", 0.5), ("winding", True),
                           ("window", [10, 10]), ("kind", "transport_mass"),
                           ("unit", "count"), ("unwrapping", "unsigned")):
            record = deepcopy(valid)
            record[key] = value
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                phase_advance(record)

    def test_nonpositive_reference_and_mismatched_windows_are_rejected(self):
        p = build_fixture()["prediction"]
        for winding, start, end in ((0, "0", "0"), (-1, "0", "0")):
            reference = deepcopy(p["reference_record"])
            reference.update(winding=winding, phase_start=start, phase_end=end)
            with self.assertRaises(ValueError):
                conditional_prediction(p["compared_record"], reference, p["reference_realization"])
        for key, value in (("window", [11, 22]), ("coordinate", "other"),
                           ("epoch", "other"), ("orientation", "reverse")):
            reference = deepcopy(p["reference_record"])
            reference[key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                conditional_prediction(p["compared_record"], reference, p["reference_realization"])

    def test_reference_realization_is_separate_and_required(self):
        p = build_fixture()["prediction"]
        for realization in (None, {}, {**p["reference_realization"], "reference_id": "wrong"},
                            {**p["reference_realization"], "kind": "native_event_count"},
                            {**p["reference_realization"], "physical_correspondence": "measured"}):
            with self.subTest(realization=realization), self.assertRaises(ValueError):
                conditional_prediction(p["compared_record"], p["reference_record"], realization)
        for target in (0, -1, True, 1.5):
            with self.subTest(target=target), self.assertRaises(ValueError):
                build_fixture(hydrogen_target_override=target)


if __name__ == "__main__":
    unittest.main()
