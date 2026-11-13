"""Adversarial support/routing checks for the declared collision application."""
from copy import deepcopy
from dataclasses import replace
from fractions import Fraction as F
import unittest

from simulations.chiral_collision import CollisionConfig, simulate_path
from simulations.collision_routing import build_routing_fixture, validate_routing_fixture, trace_address_records
from simulations.vendor import aod_manual_native as native


class CollisionRoutingTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        config = CollisionConfig(contact_gain=F(0))
        cls.paths = {seed: simulate_path(config, seed=seed, ticket_denominator=64) for seed in (0, 1, 2, 8)}
        cls.positive = build_routing_fixture(cls.paths[8])

    def test_actual_33_support_retains_every_source_return_and_component(self):
        report = self.positive
        for name in ("a", "b"):
            carrier = report["carriers"][name]
            wave = carrier["wave"]
            self.assertTrue(native.validate_wave(wave))
            self.assertEqual(wave.motif, "3:3")
            self.assertEqual(len(wave.returns), 10)
            self.assertEqual(len(wave.motif_witnesses), 10)
            self.assertEqual((carrier["formation_returns"], carrier["selected_returns"]), (2, 8))
            for trace, motif, row in zip(wave.returns, wave.motif_witnesses, carrier["return_maps"]):
                self.assertEqual(trace.roles[0], row["selected_chirality"])
                self.assertEqual(len(row["component_records"]), 3)
                self.assertLess(row["source_committed"], row["construction_start"])
                self.assertTrue(native.validate_motif(motif))
                self.assertEqual(tuple(map(len, motif.construction[0])), (3, 3, 3))
                self.assertEqual(len(set(motif.construction[1])), 3)
        self.assertTrue(validate_routing_fixture(report))

    def test_exact_zero_one_and_multiple_flip_controls(self):
        for seed, load, residual, status in ((1, 0, -1, "deficit_open"),
                                             (0, 1, 0, "no_shedding"),
                                             (8, 3, 2, "shedding")):
            result = build_routing_fixture(self.paths[seed])
            route = result["routing"]
            self.assertEqual((route["record"].closure_load, route["record"].residual,
                              route["record"].status), (F(load), F(residual), status))
            self.assertEqual(sum(route["partition"].values()), max(F(0), F(residual)))
            if residual <= 0:
                self.assertIsNone(route["record"].route)
                self.assertIsNone(route["route_support"])

    def test_positive_route_has_actual_duon_form_and_causal_order(self):
        report = self.positive
        routing = report["routing"]
        route = routing["record"]
        form = routing["route_support"]
        outer = report["outer_fold"]["outer"][0]
        self.assertEqual((form["form"], form["wave"].motif, form["enclosure"]), ("E2", "Duon", 6))
        self.assertTrue(native.validate_wave(form["wave"]))
        self.assertLess(form["wave"].returns[-1].indices[-1], outer.indices[0])
        self.assertLess(route.source_committed, outer.indices[0])
        self.assertLess(outer.indices[-1], route.committed)
        self.assertEqual(route.route, "redirected_support")
        self.assertEqual(routing["partition"]["redirected_support"], F(2))

    def test_exoshedding_exposes_the_actual_retained_inner_pair(self):
        report = build_routing_fixture(self.paths[8], route="exoshedding")
        exposed = native.expose(report["outer_fold"], report["routing"]["record"])
        self.assertEqual(exposed, tuple(report["carriers"][name]["wave"] for name in ("a", "b")))
        self.assertTrue(validate_routing_fixture(report))

    def test_same_formation_prefix_and_different_continuation_bindings(self):
        left = build_routing_fixture(self.paths[0])
        right = self.positive
        for name in ("a", "b"):
            a, b = (r["carriers"][name]["wave"] for r in (left, right))
            self.assertEqual(a.identity, b.identity)
            self.assertEqual(a.returns[:2], b.returns[:2])
            self.assertNotEqual(a.returns[2].event_ids, b.returns[2].event_ids)

    def test_routing_counterfactuals_do_not_reuse_outer_events_with_changed_payloads(self):
        surplus = self.positive
        neutral = build_routing_fixture(self.paths[8], closure_cost=F(3))
        a, b = (r["outer_fold"]["outer"][0] for r in (surplus, neutral))
        self.assertNotEqual(a.indices, b.indices)
        self.assertNotEqual(a.event_ids, b.event_ids)
        self.assertEqual(surplus["carriers"], neutral["carriers"])

    def test_empty_selected_window_retains_formation_without_fabricated_events(self):
        path = simulate_path(CollisionConfig(steps=0, contact_gain=F(0)), seed=8, ticket_denominator=64)
        result = build_routing_fixture(path)
        self.assertTrue(result["routing"]["source_event_payload"]["window"]["empty"])
        self.assertEqual(result["routing"]["record"].closure_load, F(0))
        for row in result["observables"].values():
            self.assertEqual((row["closures"], row["beats"]), (2, 1))

    def test_source_state_orientation_tracks_last_return_not_formation(self):
        result = build_routing_fixture(self.paths[2])
        self.assertEqual(result["routing"]["source_state"].orientation, -1)
        self.assertEqual(result["carriers"]["a"]["wave"].returns[0].roles[0], 1)

    def test_octal_encoding_rejects_an_address_below_the_declared_origin(self):
        with self.assertRaises(ValueError):
            trace_address_records(native.trace("negative-address", start_index=-3))

    def test_octal_addresses_bind_events_without_changing_beat_or_wavelength_types(self):
        for row in self.positive["observables"].values():
            self.assertEqual((row["closures"], row["beats"]), (10, 9))
            self.assertNotIn("octaves", row)
            self.assertEqual(len(row["beat_records"]), 9)
            for record in row["octal_address_records"]:
                first, last = record["rows"][0], record["rows"][-1]
                self.assertEqual(first["q4_vertex"], last["q4_vertex"])
                self.assertNotEqual(first["retained_address"], last["retained_address"])
                self.assertNotEqual(first["event_id"], last["event_id"])
                for event in record["rows"]:
                    self.assertEqual(int(event["retained_address"]["digits"], 8), event["committed_cut"])
                    self.assertEqual(int(event["q4_vertex_bits"], 2), event["q4_vertex"])
            self.assertEqual(row["cadence"], (12,) * 9)
            self.assertEqual(row["atomic_traversals_per_return"], (2,) * 10)
            self.assertIsNone(row["wavelength"])
            self.assertEqual(row["wavelength_status"], "NOT_INSTANTIATED")

    def test_rejects_missing_form_wrong_partition_and_forged_bridge(self):
        for mutate in (
            lambda r: r["routing"].update(route_support=None),
            lambda r: r["routing"]["partition"].update(open_loss=F(2)),
            lambda r: r["routing"]["model_bridge_payload"].update(load_rule="L=99"),
            lambda r: r["routing"]["source_event_payload"].update(flip_counts=(99, 99)),
            lambda r: r["routing"].update(record=replace(r["routing"]["record"], source_event_digest="a" * 64)),
            lambda r: r["routing"]["retention"].update(available_ticks=0),
            lambda r: r["carriers"]["a"]["observables"].update(beats=True),
        ):
            forged = deepcopy(self.positive)
            mutate(forged)
            with self.assertRaises(ValueError):
                validate_routing_fixture(forged)

    def test_rejects_route_and_retention_precondition_failures(self):
        for kwargs in ({"route": "gpu"}, {"route": "endoshedding"}, {"seat_compatible": False},
                       {"retention_ticks": 1, "window_ticks": 2}, {"retention_ticks": True},
                       {"closure_cost": 1.0}, {"closure_cost": -1}, {"window_ticks": 0}):
            with self.assertRaises((ValueError, TypeError)):
                build_routing_fixture(self.paths[8], **kwargs)

    def test_rejects_changed_source_tickets_and_preserves_compact_input(self):
        forged = deepcopy(self.paths[8])
        forged["steps"][0]["flip_tickets"] = (64, 64)
        with self.assertRaises(ValueError):
            build_routing_fixture(forged)
        compact = simulate_path(self.paths[8]["config"], seed=8, ticket_denominator=64,
                                retain_witnesses=False)
        self.assertEqual(build_routing_fixture(compact), self.positive)


if __name__ == "__main__":
    unittest.main()
