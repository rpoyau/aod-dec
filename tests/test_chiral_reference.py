"""Independent exact values for core-biased encounter probabilities."""
from fractions import Fraction as F
import unittest

from simulations.chiral_reference import exact_reference, fixed_probability_checks


class ChiralReferenceTests(unittest.TestCase):
    def test_one_step_contact_and_core_exposure_from_six_common_neighbors(self):
        result = exact_reference(steps=1)
        row = result["step_reports"][0]
        self.assertEqual(row["contact_probability"], F(3, 32))
        self.assertEqual(row["core_contact_probability"], F(1, 64))
        self.assertEqual(row["probability_ever_contact"], F(3, 32))
        self.assertEqual(row["expected_opportunities_by_stratum"], {
            "core_contact": F(1, 32), "core_separate": F(7, 32),
            "outer_contact": F(5, 32), "outer_separate": F(51, 32)})
        # Sum opportunities times independently substituted q values.
        self.assertEqual(row["expected_flip_count"], F(439, 2048))
        self.assertEqual(row["expected_flips_by_carrier"], {"a": F(439, 4096), "b": F(439, 4096)})

    def test_declared_exact_core_and_contact_probabilities(self):
        result = exact_reference(steps=3)
        expected = {"core_contact": F(7, 16), "core_separate": F(1, 4),
                    "outer_contact": F(19, 64), "outer_separate": F(1, 16)}
        self.assertEqual(result["q_by_stratum"], expected)
        self.assertEqual(result["conditional_flip_probability_by_stratum"], expected)
        self.assertEqual(sum(result["expected_opportunities_by_stratum"].values()), 6)
        self.assertEqual(result["normalization"], 1)
        self.assertEqual(sum(result["distribution"].values()), 1)

    def test_zero_flip_preserves_initial_signs_despite_motion_and_contact(self):
        result = exact_reference(steps=3, shell_probability=0, core_probability=0, contact_gain=0)
        self.assertTrue(all(state[2:] == (1, -1) for state in result["distribution"]))
        self.assertEqual(result["expected_flip_count"], 0)
        self.assertEqual(result["probability_at_least_one_flip"], {"either": 0, "a": 0, "b": 0})
        self.assertGreater(result["probability_ever_contact"], 0)

    def test_forced_flip_even_and_odd_parity(self):
        for steps in (1, 2, 3):
            with self.subTest(steps=steps):
                result = exact_reference(steps=steps, shell_probability=1, core_probability=1)
                sign = (-1) ** steps
                self.assertTrue(all(state[2:] == (sign, -sign) for state in result["distribution"]))
                self.assertEqual(result["expected_flip_count"], 2 * steps)
                self.assertEqual(result["probability_at_least_one_flip"], {"either": 1, "a": 1, "b": 1})
                self.assertEqual(result["probability_terminal_sign_change"], {"a": steps % 2, "b": steps % 2})

    def test_homogeneous_law_matches_binomial_parity_and_first_event(self):
        q, steps = F(1, 4), 4
        result = exact_reference(steps=steps, shell_probability=q, core_probability=q, contact_gain=0)
        exact = fixed_probability_checks(q, steps)
        self.assertEqual(exact, {"expected_flip_count": 1,
                                "probability_at_least_one_flip": F(175, 256),
                                "probability_terminal_sign_change": F(15, 32)})
        for carrier in ("a", "b"):
            self.assertEqual(result["expected_flips_by_carrier"][carrier], exact["expected_flip_count"])
            self.assertEqual(result["probability_at_least_one_flip"][carrier], exact["probability_at_least_one_flip"])
            self.assertEqual(result["probability_terminal_sign_change"][carrier], exact["probability_terminal_sign_change"])
        self.assertEqual(result["probability_at_least_one_flip"]["either"], 1 - F(3, 4) ** 8)

    def test_chirality_kernel_changes_do_not_change_movement_law(self):
        enhanced = exact_reference(steps=2)
        uniform = exact_reference(steps=2, core_probability=F(1, 16))
        self.assertEqual(enhanced["position_distribution"], uniform["position_distribution"])
        self.assertEqual(enhanced["expected_opportunities_by_stratum"], uniform["expected_opportunities_by_stratum"])
        self.assertEqual(enhanced["probability_ever_contact"], uniform["probability_ever_contact"])
        self.assertGreater(enhanced["expected_flip_count"], uniform["expected_flip_count"])

    def test_initial_state_is_not_an_event_in_the_requested_future_window(self):
        result = exact_reference(steps=0, initial_state=("*0**", "*0**", -1, 1))
        self.assertEqual(result["distribution"], {("*0**", "*0**", -1, 1): F(1)})
        self.assertEqual(result["probability_ever_contact"], 0)
        self.assertEqual(result["expected_flip_count"], 0)
        self.assertTrue(all(x is None for x in result["conditional_flip_probability_by_stratum"].values()))

    def test_invalid_probabilities_states_and_step_counts_reject(self):
        for kwargs in ({"steps": True}, {"steps": -1}, {"core_facet": "****"},
                       {"shell_probability": .125}, {"contact_gain": F(9, 8)},
                       {"shell_probability": F(1, 2), "core_probability": F(1, 4)},
                       {"initial_state": ("0***", "1***", True, -1)}):
            with self.subTest(kwargs=kwargs), self.assertRaises((TypeError, ValueError)):
                exact_reference(**kwargs)


if __name__ == "__main__":
    unittest.main()
