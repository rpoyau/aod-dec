"""Independent values, conservation, and hostile inputs for the cell fixture."""
from dataclasses import replace
from fractions import Fraction as F
from itertools import combinations
import unittest

from runtime.dec_core import finite_afc
from simulations.cell_transport import (FACETS, ModelConfig, exact_data, flatten_state,
                                       interval_rates, make_schedule, region_audit,
                                       simulate, transition_kernel)


class CellTransportTests(unittest.TestCase):
    def test_single_step_emitter_and_no_instantaneous_new_cell_leak(self):
        result = simulate(ModelConfig(intervals=1))
        final = result["final"]
        self.assertEqual(final["emitter"], 1)
        self.assertEqual(final["cells"], (F(1, 4),) + (F(0),) * 7)
        self.assertEqual(final["output"], 0)
        self.assertEqual(final["loss"], 0)
        self.assertEqual(final["injected"], F(1, 4))

    def test_independent_isolated_emitter_geometric_solution(self):
        config = ModelConfig(intervals=3, exchange=0, leak=0, absorption=0, pump=0)
        for n in (1, 2, 4):
            result = simulate(config, n)
            expected = (1 - config.gamma / n) ** (config.intervals * n)
            self.assertEqual(result["final"]["emitter"], expected)
            self.assertEqual(result["final"]["cells"][0], 1 - expected)
            self.assertEqual(sum(flatten_state(result)[:-1]), 1)

    def test_independent_isolated_output_geometric_solution(self):
        cells = (F(0), F(1)) + (F(0),) * 6
        config = ModelConfig(intervals=2, exchange=0, absorption=0, pump=0,
                             gamma=0, initial_emitter=0, initial_cells=cells)
        for n in (1, 2, 4):
            result = simulate(config, n)
            expected = (1 - config.leak / n) ** (config.intervals * n)
            self.assertEqual(result["final"]["cells"][1], expected)
            self.assertEqual(result["final"]["output"], 1 - expected)

    def test_exact_global_balance_under_jitter_and_refinement(self):
        config = ModelConfig(intervals=2)
        schedule = make_schedule(2, F(1, 2), 19)
        for n in (1, 2, 4):
            result = simulate(config, n, schedule)
            final = result["final"]
            self.assertEqual(final["emitter"] + sum(final["cells"]) + final["output"] + final["loss"],
                             1 + config.pump * config.intervals)
            self.assertTrue(all(value >= 0 for value in flatten_state(result)))
            self.assertEqual(result["schedule"]["sha256"], schedule["sha256"])
            self.assertEqual(result["checks"]["checked_steps"], 2 * n)

    def test_all_256_regions_retain_oriented_boundary_balance(self):
        # Nonuniform retained mass produces genuinely nonzero interface flux.
        config = ModelConfig(intervals=1, initial_cells=tuple(F(i, 28) for i in range(8)))
        result = simulate(config, 2, make_schedule(1, F(1, 2), 31))
        nonzero = 0
        for count in range(9):
            for region in combinations(FACETS, count):
                audit = region_audit(result["steps"][-1], region)
                self.assertEqual(audit["residual"], 0)
                nonzero += bool(audit["interface_boundary_flux"])
        self.assertGreater(nonzero, 0)
        whole = region_audit(result["steps"][-1], FACETS)
        self.assertEqual(whole["boundary_chain"], {})
        self.assertEqual(whole["interface_boundary_flux"], 0)
        self.assertGreater(whole["output"] + whole["loss"], 0)

    def test_finite_afc_on_separately_normalized_positive_state(self):
        config = ModelConfig(intervals=1)
        schedule = make_schedule(1, F(1, 4), 8)
        kernel = transition_kernel(config, schedule["multipliers"][0], F(1, 2))
        # This p is a separate normalized probability fixture, not energy mass.
        p = tuple(F(i + 1, 66) for i in range(11))
        audit = finite_afc(p, kernel, (0, 1, 4, 9))
        self.assertEqual(sum(audit["divergence"][i] for i in (0, 1, 4, 9)), audit["boundary"])
        self.assertEqual(sum(audit["next_memory"]), 1)

    def test_zero_jitter_removes_seed_effect_and_same_path_is_retained(self):
        config = ModelConfig(intervals=2)
        a, b = make_schedule(2, 0, 1), make_schedule(2, 0, 999)
        self.assertEqual(a["multipliers"], b["multipliers"])
        self.assertEqual(simulate(config, 2, a)["final"], simulate(config, 2, b)["final"])
        perturbed = make_schedule(2, F(1, 4), 1)
        self.assertNotEqual(simulate(config, 2, perturbed)["final"], simulate(config, 2, a)["final"])
        self.assertEqual(interval_rates(config, perturbed, 1),
                         tuple(config.exchange * v for v in perturbed["multipliers"][1]))

    def test_retention_does_not_change_engine(self):
        config = ModelConfig(intervals=2)
        schedule = make_schedule(2, F(1, 2), 43)
        retained, compact = simulate(config, 4, schedule), simulate(config, 4, schedule, retain_steps=False)
        self.assertEqual(retained["final"], compact["final"])
        self.assertEqual(retained["interval_snapshots"], compact["interval_snapshots"])
        self.assertEqual(compact["steps"], ())
        self.assertIsInstance(exact_data(compact)["final"]["emitter"], str)

    def test_invalid_inputs_and_unstable_step_fail(self):
        with self.assertRaises(TypeError):
            ModelConfig(exchange=0.1)
        with self.assertRaises(ValueError):
            ModelConfig(initial_cells=(-1,) + (0,) * 7)
        with self.assertRaises(ValueError):
            make_schedule(4, F(3, 2))
        with self.assertRaises(ValueError):
            simulate(replace(ModelConfig(), exchange=1), 1)
        with self.assertRaises(ValueError):
            simulate(substeps=True)
        damaged = make_schedule(4, F(1, 4), 11)
        damaged["multipliers"] = ((F(0),) * 24,) + damaged["multipliers"][1:]
        with self.assertRaises(ValueError):
            simulate(schedule=damaged)
        stale = make_schedule(4)
        stale["sha256"] = "0" * 64
        with self.assertRaises(ValueError):
            simulate(schedule=stale)
        with self.assertRaises(ValueError):
            simulate(schedule=make_schedule(3))


if __name__ == "__main__":
    unittest.main()
