"""Meaningful scope and arithmetic checks for numerical/SI report records."""
from copy import deepcopy
from decimal import Decimal, localcontext
from fractions import Fraction as F
import unittest

from simulations.cell_transport import ModelConfig, make_schedule, simulate
from simulations.report_contract import (
    ERROR_COMPONENTS, integer_delta3, numerical_comparison, si_comparison,
    terminal_report, transport_error_components,
)
from simulations.transport_reference import simulate_reference


def si_fixture():
    common = {"unit": "Hz", "kind": "frequency", "dimension": (0, -1, 0)}
    mapping = {"id": "declared-frequency-map", "source_id": "map-source",
               "source_sha256": "a" * 64, "output_unit": "Hz",
               "output_kind": "frequency", "output_dimension": (0, -1, 0),
               "dependencies": []}
    prediction = {**common, "id": "model-output", "role": "prediction",
                  "value": F(9, 2), "source_id": "model-source",
                  "source_sha256": "b" * 64, "map_id": mapping["id"],
                  "dependencies": [{"id": "independent-input", "role": "model"}]}
    target = {**common, "id": "measured-target", "role": "evaluation",
              "value": F(5), "source_id": "measurement-source",
              "source_sha256": "c" * 64, "standard_uncertainty": F(1, 4),
              "uncertainty_category": "source_reported_standard_uncertainty"}
    return prediction, target, mapping


class ReportContractTests(unittest.TestCase):
    def test_integer_delta3_sign_magnitude_and_strict_type(self):
        self.assertEqual(integer_delta3(4, 7), {
            "predicted": 4, "comparator": 7, "signed_residual": -3,
            "ternary_sign": -1, "integer_magnitude": 3, "unit": "declared_integer_bin"})
        self.assertEqual(integer_delta3(2, 2)["ternary_sign"], 0)
        for bad in (True, F(1), F(1, 2), 1.0, Decimal(1)):
            with self.assertRaises(TypeError):
                integer_delta3(bad, 1)

    def test_per_output_discrepancy_and_declared_denominator(self):
        row = numerical_comparison(F(1, 8), Decimal("0.25"), name="output",
            quantity_kind="cumulative_boundary_output", reference_id="independent-reference",
            denominator=Decimal("0.25"), denominator_kind="absolute_numerical_reference")
        self.assertEqual(row["signed_discrepancy"], Decimal("-0.125"))
        self.assertEqual(row["fractional_discrepancy"], Decimal("-0.5"))
        self.assertEqual(row["exact_value"], F(1, 8))
        self.assertNotIn("percent_display", row)
        for bad in (0, F(-1)):
            with self.assertRaises(ValueError):
                numerical_comparison(F(1), Decimal(1), name="x", quantity_kind="mass",
                    reference_id="r", denominator=bad, denominator_kind="declared")

    def test_conditional_budgets_preserve_unknown_and_inapplicable(self):
        budget = transport_error_components()
        self.assertEqual(set(budget), set(ERROR_COMPONENTS))
        self.assertEqual(budget["arithmetic"]["value"], F(0))
        for name in ("resolution", "model_discrepancy", "calibration", "solver"):
            self.assertEqual(budget[name]["kind"], "unknown")
            self.assertNotIn("value", budget[name])
        for name in ("window", "propagation", "comparison"):
            self.assertEqual(budget[name]["kind"], "not_applicable")
            self.assertNotIn("value", budget[name])

    def test_terminal_report_keeps_output_error_separate_from_aggregate(self):
        config = ModelConfig(intervals=1, exchange=0, absorption=0, pump=0,
            gamma=0, initial_emitter=0, initial_cells=(F(0), F(1)) + (F(0),) * 6)
        result = simulate(config)
        reference = simulate_reference(config, precision=60)
        report = terminal_report(result, reference, reference_id="isolated-leak-reference")
        output = report["outputs"]["output"]
        self.assertEqual(output["exact_value"], F(1, 4))
        self.assertEqual(output["quantity_kind"], "cumulative_boundary_output")
        self.assertEqual(report["aggregate"]["normalizer"]["exact_value"], 1)
        self.assertNotEqual(output["fractional_discrepancy"],
                            report["aggregate"]["normalized_l1_diagnostic"])
        self.assertIsNone(report["outputs"]["emitter"]["fractional_discrepancy"])

    def test_mismatched_schedule_is_rejected(self):
        config = ModelConfig(intervals=1)
        result = simulate(config, schedule=make_schedule(1, F(1, 4), 1))
        reference = simulate_reference(config, make_schedule(1, F(1, 4), 2))
        with self.assertRaises(ValueError):
            terminal_report(result, reference, reference_id="wrong-path")

    def test_reference_denominator_does_not_use_ambient_decimal_precision(self):
        config = ModelConfig(intervals=1)
        result = simulate(config)
        reference = simulate_reference(config, precision=60)
        with localcontext() as context:
            context.prec = 5
            report = terminal_report(result, reference, reference_id="reference", precision=60)
        denominator = report["outputs"]["0***"]["relative_denominator"]["value"]
        self.assertEqual(denominator, reference["final"]["cells"][0])

    def test_zero_mass_has_no_fabricated_normalized_diagnostic(self):
        config = ModelConfig(intervals=1, initial_emitter=0, pump=0)
        report = terminal_report(simulate(config), simulate_reference(config), reference_id="zero")
        self.assertEqual(report["aggregate"]["status"], "undefined_zero_normalizer")
        self.assertIsNone(report["aggregate"]["normalized_l1_diagnostic"])

    def test_si_residual_fraction_percent_and_target_only_pull_are_separate(self):
        prediction, target, mapping = si_fixture()
        report = si_comparison(prediction, target, mapping=mapping, include_percent=True)
        self.assertEqual(report["signed_residual"], F(-1, 2))
        self.assertEqual(report["fractional_residual"], F(-1, 10))
        self.assertEqual(report["percent_display"]["value"], -10)
        self.assertEqual(report["target_only_pull"]["value"], -2)
        self.assertEqual(report["target_only_pull"]["label"], "z_target")
        self.assertEqual(report["joint_standardized_residual"]["status"], "unavailable")
        self.assertNotIn("percent_display", si_comparison(prediction, target, mapping=mapping))

    def test_si_missing_map_zero_target_and_unit_relabelling_reject(self):
        prediction, target, mapping = si_fixture()
        with self.assertRaises(ValueError):
            si_comparison(prediction, target, mapping=None)
        for field, value in (("value", F(0)), ("unit", "s"),
                             ("kind", "transport_storage"), ("dimension", (False, -1, 0))):
            altered = {**target, field: value}
            with self.assertRaises(ValueError):
                si_comparison(prediction, altered, mapping=mapping)
        bad_map = {**mapping, "output_unit": "model_mass"}
        with self.assertRaises(ValueError):
            si_comparison(prediction, target, mapping=bad_map)

    def test_evaluation_leakage_through_nested_map_dependencies_rejects(self):
        prediction, target, mapping = si_fixture()
        altered = deepcopy(mapping)
        altered["dependencies"].append({"id": "intermediate", "role": "model",
                                       "parents": [{"id": target["id"], "role": "model"}]})
        with self.assertRaises(ValueError):
            si_comparison(prediction, target, mapping=altered)
        prediction["dependencies"].append({"id": "another-target", "role": "evaluation"})
        with self.assertRaises(ValueError):
            si_comparison(prediction, target, mapping=mapping)

    def test_target_mutation_changes_comparison_without_mutating_prediction(self):
        prediction, target, mapping = si_fixture()
        before = deepcopy(prediction)
        original = si_comparison(prediction, target, mapping=mapping)
        changed = si_comparison(prediction, {**target, "value": F(6)}, mapping=mapping)
        self.assertEqual(prediction, before)
        self.assertEqual(original["prediction"], changed["prediction"])
        self.assertNotEqual(original["signed_residual"], changed["signed_residual"])

    def test_bound_is_not_accepted_as_target_standard_uncertainty(self):
        prediction, target, mapping = si_fixture()
        target["uncertainty_category"] = "deterministic_bound"
        with self.assertRaises(ValueError):
            si_comparison(prediction, target, mapping=mapping)
        target["uncertainty_category"] = "exact_defining_constant"
        target["standard_uncertainty"] = F(0)
        self.assertEqual(si_comparison(prediction, target, mapping=mapping)
                         ["target_only_pull"]["status"], "unavailable")


if __name__ == "__main__":
    unittest.main()
