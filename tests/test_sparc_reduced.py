"""Independent finite expectations for the declared radial native response."""

from dataclasses import replace
from fractions import Fraction as Q
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from simulations.sparc_inputs import BinKey, PreparationRow, load_inputs
from simulations.sparc_reduced import simulate_galaxy


def row(radius, gas, *, galaxy="Demo"):
    return PreparationRow(BinKey(galaxy, Q(radius)), Q(gas), Q(0), Q(0),
                          Q(0), Q(0), Q(1))


class SparcReducedTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.rows = (row(1, 1), row(2, 2), row(3, 3))
        cls.model_result = simulate_galaxy(cls.rows)

    def test_independent_exact_three_bin_response_and_projection(self):
        run = self.model_result
        self.assertEqual(run["initial_dbf"], (Q(1), Q(4), Q(9)))
        self.assertEqual(run["cell_pressure"], (Q(18), Q(27), Q(18)))
        self.assertEqual(run["transitions"][0]["before_theta"], (Q(1, 9), Q(4, 9), Q(1)))
        self.assertEqual(run["transitions"][0]["after_theta"], (Q(5, 18), Q(14, 27), Q(13, 18)))
        self.assertEqual(run["transitions"][1]["after_theta"], (Q(43, 108), Q(41, 81), Q(67, 108)))
        self.assertEqual(run["transitions"][2]["after_theta"], (Q(293, 648), Q(247, 486), Q(365, 648)))
        self.assertEqual(run["final_dbf"], (Q(293, 72), Q(247, 54), Q(365, 72)))
        self.assertEqual(tuple(p.u_km2_s2 for p in run["projections"]),
                         (Q(302, 9), Q(1096, 27), Q(446, 9)))
        self.assertEqual(tuple(p.key for p in run["projections"]), tuple(r.key for r in self.rows))

    def test_fixed_native_pressure_stock_and_range_are_preserved(self):
        run = self.model_result
        self.assertEqual(run["initial_total_stock"], Q(32))
        self.assertEqual(run["final_total_stock"], Q(32))
        for transition in run["transitions"]:
            self.assertEqual(transition["before_total_stock"], Q(32))
            self.assertEqual(transition["after_total_stock"], Q(32))
            self.assertEqual(transition["conservation_residual"], 0)
            self.assertTrue(all(Q(1, 9) <= value <= 1 for value in transition["after_theta"]))
        # Unweighted theta is not the declared conserved stock on this graph.
        self.assertNotEqual(sum(run["transitions"][0]["before_theta"]),
                            sum(run["transitions"][0]["after_theta"]))

    def test_moving_sweep_uses_old_stock_and_after_membership_crossing(self):
        run = self.model_result
        self.assertEqual(run["threshold"], Q(32, 63))
        selected = run["transitions"][0]["boundaries"]["selected"]
        self.assertEqual(selected["before_cells"], (run["cells"][2],))
        self.assertEqual(set(selected["after_cells"]), set(run["cells"][1:]))
        self.assertEqual(selected["regional_stock_before"], Q(18))
        self.assertEqual(selected["regional_stock_after"], Q(27))
        self.assertEqual(selected["stock_change"], Q(9))
        self.assertEqual(selected["cross"], Q(3))
        self.assertEqual(selected["sweep"], Q(12))
        self.assertEqual(selected["current"], Q(-9))
        self.assertEqual(selected["crossing_policy"], "after-membership")
        for transition in run["transitions"]:
            self.assertTrue(all(value == 0 for value in transition["partition_residuals"].values()))
            for boundary in transition["boundaries"].values():
                self.assertEqual(boundary["balance_residual"], 0)
                self.assertEqual(boundary["stock_change"] + boundary["current"], 0)
                self.assertEqual(boundary["current"], boundary["cross"] - boundary["sweep"] + boundary["shedding"])
            whole = transition["boundaries"]["whole"]
            self.assertEqual((whole["cross"], whole["sweep"], whole["stock_change"]), (0, 0, 0))
            stationary = transition["boundaries"]["stationary"]
            self.assertEqual(stationary["sweep"], 0)
            self.assertEqual(stationary["stock_change"], -stationary["cross"])

    def test_spacing_primary_and_unit_ablation_have_independent_expected_states(self):
        uneven = (row(1, 1), row(2, 2), row(4, 4))
        spacing = simulate_galaxy(uneven)
        unit = simulate_galaxy(uneven, geometry="unit")
        self.assertEqual(spacing["intensity"], (Q(1), Q(1, 2)))
        self.assertEqual(unit["intensity"], (Q(1), Q(1)))
        self.assertEqual(spacing["transitions"][0]["after_theta"], (Q(5, 32), Q(5, 16), Q(13, 16)))
        self.assertEqual(unit["transitions"][0]["after_theta"], (Q(5, 32), Q(7, 16), Q(5, 8)))
        self.assertEqual(spacing["cell_pressure"], unit["cell_pressure"])
        self.assertEqual(spacing["initial_total_stock"], Q(207, 8))
        self.assertEqual(spacing["final_total_stock"], Q(207, 8))
        self.assertEqual(unit["final_total_stock"], Q(207, 8))
        self.assertEqual(spacing["response_steps"], unit["response_steps"])
        self.assertEqual(self.model_result["intensity"], (Q(1), Q(1)))

    def test_coupling_disabled_keeps_initial_projection_with_same_pressure_and_horizon(self):
        control = simulate_galaxy(self.rows, coupled=False)
        self.assertEqual(control["cell_pressure"], self.model_result["cell_pressure"])
        self.assertEqual(control["response_steps"], self.model_result["response_steps"])
        self.assertEqual(control["computed_cadence"], self.model_result["computed_cadence"])
        self.assertEqual(control["final_dbf"], (Q(1), Q(4), Q(9)))
        self.assertEqual(tuple(p.u_km2_s2 for p in control["projections"]), (Q(9), Q(36), Q(81)))
        for transition in control["transitions"]:
            self.assertEqual(transition["after_theta"], (Q(1, 9), Q(4, 9), Q(1)))
            self.assertTrue(all(not channel.available for channel in transition["channels"]))
            self.assertTrue(all(boundary["current"] == 0 for boundary in transition["boundaries"].values()))

    def test_input_order_normalization_preserves_synchronous_history(self):
        shuffled = simulate_galaxy((self.rows[2], self.rows[0], self.rows[1]))
        self.assertEqual(shuffled, self.model_result)

    def test_targets_cannot_enter_preparation_response_or_projection(self):
        header = ("# Distance = 1 Mpc\n"
                  "# Rad Vobs errV Vgas Vdisk Vbul SBdisk SBbul\n"
                  "# kpc km/s km/s km/s km/s km/s L/pc^2 L/pc^2\n")
        with TemporaryDirectory() as directory:
            path = Path(directory) / "Demo_rotmod.dat"
            original_rows = "1 10 1 1 0 0 0 0\n2 10 1 2 0 0 0 0\n3 10 1 3 0 0 0 0\n"
            path.write_text(header + original_rows)
            original = load_inputs(directory)
            path.write_text(header + original_rows.replace(" 10 1 ", " 999 57 "))
            changed = load_inputs(directory)
        self.assertEqual(tuple(original.preparation.values()), self.rows)
        self.assertEqual(original.preparation, changed.preparation)
        self.assertNotEqual(original.targets, changed.targets)
        self.assertNotEqual(original.sources[0].sha256, changed.sources[0].sha256)
        self.assertEqual(simulate_galaxy(changed.preparation.values()), self.model_result)

    def test_constant_and_zero_input_families_do_not_manufacture_a_response(self):
        for rows, expected in (((row(1, 1), row(2, Q(1, 2))), (Q(1), Q(1))),
                               ((row(1, 0), row(2, 0)), (Q(0), Q(0)))):
            with self.subTest(initial=expected):
                result = simulate_galaxy(rows)
                self.assertEqual(result["initial_dbf"], expected)
                self.assertEqual(result["final_dbf"], expected)
                for transition in result["transitions"]:
                    self.assertTrue(all(operand["value"] == 0 for operand in transition["operands"]))
                    self.assertEqual(transition["before_theta"], transition["after_theta"])

    def test_cadence_and_pressure_come_from_committed_representative_history(self):
        run = self.model_result
        self.assertEqual((run["initial_cut"], run["final_cut"], run["computed_cadence"]), (11, 14, 3))
        self.assertEqual((run["graph_diameter"], run["response_steps"]), (2, 3))
        self.assertEqual((run["self_pressure"], run["channel_pressure"]), (Q(9), Q(9)))
        self.assertEqual(run["native_history"].committed_cut, 14)
        for transition in run["transitions"]:
            self.assertEqual(transition["response_cut"], transition["cut"] + 1)
            for operand in transition["operands"]:
                self.assertEqual((operand["capacity_positions"], operand["participation"], operand["rcd_duration"]), (3, 3, 5))
                self.assertLessEqual(operand["source_return_cut"], operand["input_cut"])
                self.assertEqual(operand["input_cut"], transition["cut"])
        self.assertIsNone(run["boundary_declaration"]["unresolved_exchange_error"])
        self.assertIn("not physical mass", run["quantity_types"]["stock"])

    def test_malformed_support_and_undeclared_controls_are_rejected(self):
        malformed = (
            (), (self.rows[0],), (self.rows[0], self.rows[0]),
            (self.rows[0], row(2, 2, galaxy="Other")),
            (self.rows[0], replace(self.rows[1], distance_mpc=Q(2))),
            (self.rows[0], replace(self.rows[1], vgas_km_s=2.0)),
            (row(0, 1), self.rows[1]), (object(), self.rows[1]),
        )
        for rows in malformed:
            with self.subTest(rows=rows):
                with self.assertRaises(ValueError):
                    simulate_galaxy(rows)
        for options in ({"coupled": 1}, {"coupled": "yes"}, {"geometry": "fit"}):
            with self.subTest(options=options):
                with self.assertRaises(ValueError):
                    simulate_galaxy(self.rows, **options)


if __name__ == "__main__":
    unittest.main()
