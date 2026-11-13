"""Independent finite laws and hostile controls for the chiral encounter."""
from copy import deepcopy
from dataclasses import replace
from fractions import Fraction as F
import unittest

from simulations.chiral_collision import (CollisionConfig, FACETS, INITIAL_STATE,
    State, background_record, common_ticket_denominator, conditional_flip_rows,
    core_score, distribution_rows, exact_data, exact_distribution, flip_probability,
    move_kernel, simulate_path, states, successors, validate_path)
from simulations.vendor import aod_manual_native as native


class ChiralCollisionTests(unittest.TestCase):
    def test_declared_profile_and_exact_conditional_flip_law(self):
        cfg = CollisionConfig()
        self.assertEqual(flip_probability("*0**", "0***", cfg), F(1, 4))
        self.assertEqual(flip_probability("0***", "1***", cfg), F(1, 16))
        self.assertEqual(flip_probability("*0**", "*0**", cfg), F(7, 16))
        self.assertEqual(flip_probability("0***", "0***", cfg), F(19, 64))
        self.assertEqual(common_ticket_denominator((cfg,)), 64)
        self.assertEqual([r["population_count"] for r in conditional_flip_rows(cfg)],
                         [1, 1, 4, 1, 1, 1, 1, 1])
        self.assertEqual(core_score("*0**", cfg), 1)

    def test_all_kernel_rows_and_independent_one_step_values(self):
        self.assertEqual(len(states()), 256)
        for state in states():
            row = successors(state)
            self.assertEqual(sum(row.values()), 1)
            self.assertTrue(all(p > 0 for p in row.values()))
        row = successors(INITIAL_STATE)
        # Both hold outside the core/contact: (1/4)^2 (15/16)^2.
        self.assertEqual(row[INITIAL_STATE], F(225, 4096))
        # Both move to the declared core and both flip: (1/8)^2 (7/16)^2.
        self.assertEqual(row[State("*0**", "*0**", -1, 1)], F(49, 16384))

    def test_movement_uses_exact_shared_square_support(self):
        for facet in FACETS:
            row = move_kernel(facet)
            self.assertEqual(row[facet], F(1, 4))
            self.assertEqual(len(row), 7)
            self.assertEqual(sum(row.values()), 1)
            self.assertEqual(sum(p == F(1, 8) for p in row.values()), 6)
        self.assertNotIn("1***", move_kernel("0***"))

    def test_fixed_q_analytic_parity_not_raw_flip_count(self):
        q = F(1, 4)
        cfg = CollisionConfig(steps=4, q_outer=q, q_core=q, contact_gain=0)
        for n in (0, 1, 2, 4):
            law = exact_distribution(cfg, n)
            changed_a = sum(p for state, p in law.items() if state.chi_a == -1)
            changed_b = sum(p for state, p in law.items() if state.chi_b == 1)
            expected = (1 - (1 - 2 * q)**n) / 2
            self.assertEqual(changed_a, expected)
            self.assertEqual(changed_b, expected)
            self.assertEqual(sum(law.values()), 1)
        # Two flips restore the original chirality; expected flip count is n*q.
        self.assertNotEqual(expected, 4 * q)

    def test_no_flip_and_forced_flip_parity(self):
        for q in (0, 1):
            for n in (1, 2, 3):
                cfg = CollisionConfig(steps=n, q_outer=q, q_core=q, contact_gain=0)
                path = simulate_path(cfg, 5, retain_witnesses=False)
                self.assertEqual(path["flip_counts"], (n * q, n * q))
                expected = bool(n % 2) if q else False
                self.assertEqual(path["terminal_changed"], (expected, expected))
                law = exact_distribution(cfg)
                self.assertTrue(all((s.chi_a != 1) == expected for s in law))

    def test_matched_controls_keep_tickets_and_motion(self):
        baseline = CollisionConfig(steps=8)
        uniform = replace(baseline, q_core=baseline.q_outer)
        contact_off = replace(baseline, contact_gain=0)
        d = common_ticket_denominator((baseline, uniform, contact_off))
        paths = [simulate_path(c, 19, retain_witnesses=False, ticket_denominator=d)
                 for c in (baseline, uniform, contact_off)]
        def retained_randomness(path):
            return [(s["movement_tickets"], s["flip_tickets"],
                     s["after"].facet_a, s["after"].facet_b) for s in path["steps"]]
        self.assertEqual(retained_randomness(paths[0]), retained_randomness(paths[1]))
        self.assertEqual(retained_randomness(paths[0]), retained_randomness(paths[2]))
        self.assertNotEqual(paths[0]["path_id"], paths[1]["path_id"])
        self.assertNotEqual(paths[0]["steps"][0]["return_witnesses"][0]["return_id"],
                            paths[1]["steps"][0]["return_witnesses"][0]["return_id"])
        self.assertEqual(paths[0]["carrier_identities"], paths[1]["carrier_identities"])
        with self.assertRaisesRegex(ValueError, "exactly"):
            simulate_path(baseline, 1, ticket_denominator=16)

    def test_whole_return_hinge_and_immutable_carrier_history(self):
        path = simulate_path(CollisionConfig(steps=3, q_core=1, q_outer=1, contact_gain=0), 7)
        self.assertTrue(validate_path(path))
        for i, wave in enumerate(path["final_waves"]):
            self.assertTrue(native.validate_wave(wave))
            self.assertEqual(wave.identity, path["initial_waves"][i].identity)
            self.assertEqual(wave.returns[:2], path["initial_waves"][i].returns)
            self.assertEqual(len(wave.returns), 5)
            for step in path["steps"]:
                witness = step["return_witnesses"][i]
                trace = witness["trace"]
                self.assertEqual(trace.vertices, (0, 1, 0))
                self.assertEqual(trace.roles[1], 0)
                self.assertEqual(trace.roles[-1], -trace.roles[0])
                self.assertNotEqual(witness["return_id"], witness["parent_return_id"])
        self.assertEqual(len({s["event_id"] for s in path["steps"]}), 3)

    def test_retention_is_passive_and_hostile_replay_fails(self):
        cfg = CollisionConfig(steps=3)
        full, compact = simulate_path(cfg, 7), simulate_path(cfg, 7, retain_witnesses=False)
        for name in ("initial", "final", "flip_counts", "strata", "carrier_identities"):
            self.assertEqual(full[name], compact[name])
        for a, b in zip(full["steps"], compact["steps"]):
            for name in ("before", "after", "flip_tickets", "flip_probabilities", "transition_probability"):
                self.assertEqual(a[name], b[name])
        bad = deepcopy(full)
        bad["steps"][0]["return_witnesses"][0]["parent_return_id"] = "forged"
        with self.assertRaises(ValueError):
            validate_path(bad)
        bad = deepcopy(full)
        bad["steps"][0]["flip_probabilities"] = (F(0), F(0))
        with self.assertRaises(ValueError):
            validate_path(bad)
        bad = deepcopy(full)
        bad["steps"][0]["ordinal"] = True  # Python equality aliases True and 1.
        with self.assertRaises(ValueError):
            validate_path(bad)
        bad = deepcopy(full)
        bad["steps"][0]["flip_probabilities"] = tuple(
            str(q) for q in full["steps"][0]["flip_probabilities"])
        with self.assertRaises(ValueError):
            validate_path(bad)

    def test_population_profile_admission_and_no_collision_promotion(self):
        background = background_record()
        self.assertEqual(background["population_count"], 11)
        self.assertEqual(len(set(background["field"].members)), 11)
        self.assertIsNone(background["density"])
        for wave in background["waves"]:
            self.assertTrue(native.validate_wave(wave))
        path = simulate_path(CollisionConfig(steps=8), 7)
        for row in path["steps"]:
            self.assertFalse(row["coupling_admitted"])
            self.assertFalse(row["fusion_admitted"])
        self.assertEqual(path["physical_proton_correspondence"], "NOT_INSTANTIATED")

    def test_invalid_exact_inputs_and_distribution_export(self):
        for kwargs in ({"q_outer": 0.1}, {"q_core": True}, {"contact_gain": -1},
                       {"q_core": F(3, 2)}, {"steps": True}, {"steps": 65},
                       {"population_counts": (1,) * 7}, {"population_counts": (0,) * 8}):
            with self.subTest(kwargs=kwargs), self.assertRaises((TypeError, ValueError)):
                CollisionConfig(**kwargs)
        with self.assertRaises(ValueError):
            State("0***", "1***", True, -1)
        with self.assertRaises(TypeError):
            simulate_path(seed=True)
        law = exact_distribution(CollisionConfig(steps=0))
        self.assertEqual(law, {INITIAL_STATE: F(1)})
        self.assertEqual(exact_data(distribution_rows(law))[0]["probability"], "1")
        with self.assertRaises(TypeError):
            exact_data(law)


if __name__ == "__main__":
    unittest.main()
