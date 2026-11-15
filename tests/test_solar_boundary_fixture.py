from dataclasses import replace
from copy import deepcopy
from fractions import Fraction as Q
from pathlib import Path
import tempfile
import unittest

from simulations import solar_boundary_fixture as solar
from simulations import region_engine as regions
from simulations.trace_store import TraceStore, run_manifest
from simulations.trace_support import detect_particle_support
from simulations.trace_particle_relations import detect_particle_relations
from simulations.trace_parallel import boundary_views
from simulations.trace_detectors import _decode_ratios


class SolarBoundaryFixtureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture = solar.run()

    def test_native_support_and_actual_reflection_intervals(self):
        result = self.fixture
        for wave in result["native_waves"]:
            self.assertTrue(solar.n.validate_wave(wave))
            self.assertEqual(wave.motif, "Duon")
        for item in result["channel_operands"]:
            evidence = item["native_comparison"]
            self.assertEqual(item["duration"], 5)
            self.assertEqual((item["participation"], item["pressure"]), (4, Q(12)))
            self.assertEqual(len(evidence["contacts"]), 2)
            self.assertEqual(len({event.identity for event in evidence["contacts"]}), 2)
            for contact, reflected in zip(evidence["contacts"], evidence["reflection_evidence"]):
                self.assertEqual(contact.reflection, (1, 1))
                self.assertEqual(tuple(map(len, reflected["reflected_return_ids"])), contact.reflection)
                self.assertLess(contact.committed, item["cut"])
            self.assertNotEqual(item["window"], evidence["comparison_interval"][1] - evidence["comparison_interval"][0])

    def test_whole_return_flips_preserve_hinge_and_old_prefix(self):
        result = self.fixture
        self.assertEqual([(r["committed"], r["before"], r["after"]) for r in result["chiral_flips"]],
                         [(14, 1, -1), (20, -1, 1)])
        for event in result["chiral_flips"]:
            before, after = event["previous_return"], event["current_return"]
            self.assertTrue(solar.n.validate_trace(before))
            self.assertTrue(solar.n.validate_trace(after))
            self.assertEqual(after.roles, tuple(-sign for sign in before.roles))
            self.assertEqual(after.roles[1], 0)
            self.assertLess(before.indices[-1], after.indices[0])
        snapshots = result["native_snapshots"]
        for old, new in zip(snapshots, snapshots[1:]):
            for name in ("sun", "mercury"):
                self.assertEqual(old[name].returns, new[name].returns[:len(old[name].returns)])
                self.assertEqual(old[name].certificates, new[name].certificates[:len(old[name].certificates)])
                self.assertLess(new[name].returns[-1].indices[-1], new["response_cut"])
        with self.assertRaises(ValueError):
            solar._duon("forged-boolean", (True, 1, 1))
        bad = replace(result["chiral_flips"][0]["current_return"], roles=(-1, 1))
        with self.assertRaises(solar.n.AdmissionError):
            solar.n.validate_trace(bad)

    def test_current_partition_uses_after_membership_and_signed_sweep(self):
        result = self.fixture
        self.assertEqual(result["summary"]["selected"], (Q(4), Q(-3), Q(0), Q(7), Q(6)))
        self.assertEqual(result["summary"]["complement"], tuple(-v for v in result["summary"]["selected"]))
        self.assertEqual(result["summary"]["whole"], (Q(0),) * 5)
        for name, audit in result["audits"].items():
            for commit, frame, row in zip(result["ledger"].commits, audit.frames, audit.rows):
                cells = set(frame.after.cells)
                # Independent incidence sum over the complete input family.
                direct = sum((Q(int(c.source in cells) - int(c.target in cells)) * c.value
                              for c in commit.channels if c.available), Q(0))
                moved = sum((s.value * (int(s.cell in frame.after.cells) - int(s.cell in frame.before.cells))
                             for s in frame.sweep), Q(0))
                self.assertEqual(row["current"], direct - moved, name)
        frames = list(result["frames"]["selected"])
        with self.assertRaises(ValueError):
            frames[1] = replace(frames[1], sweep=())

    def test_surplus_and_admitted_route_current_keep_separate_domains(self):
        result, n = self.fixture, solar.n
        route = result["route_evidence"]
        self.assertTrue(n.validate_shedding(route["fold"], route["shedding"]))
        self.assertEqual((route["shedding"].closure_load, route["shedding"].closure_cost,
                          route["shedding"].surplus), (Q(27), Q(24), Q(3)))
        self.assertEqual(sum(route["partition"].values()), Q(3))
        self.assertEqual(route["route_support"].motif_witnesses[0].component_traces,
                         (result["native_waves"][0].returns[-1],))
        self.assertGreater(route["route_support"].returns[0].indices[0], route["shedding"].committed)
        current = next(item for item in result["channel_operands"] if item["kind"] == "shedding")
        self.assertEqual(current["signed_current"], Q(2))
        self.assertIsNone(current["surplus_to_current_map"])
        pending, = result["ledger"].commits[-1].obligations
        self.assertEqual((pending.amount, pending.unit), (Q(3), solar.BURDEN_UNIT))
        for name in ("selected", "complement"):
            with self.assertRaisesRegex(ValueError, "unresolved signed shedding"):
                regions.audit_boundary(result["ledger"], name, result["frames"][name], scope="full")
        self.assertTrue(regions.audit_boundary(result["ledger"], "whole",
                                              result["frames"]["whole"], scope="full"))

    def test_trace_roundtrip_retains_detectable_constructions_and_relations(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "trace"
            manifest = run_manifest("solar-boundary-fixture", source=solar.SOURCE,
                                    kernel="native scoped current", preparation="declared finite Duon input")
            with TraceStore(path, manifest) as store:
                traced = solar.run(store)
            self.assertEqual(regions.serial(traced), regions.serial(self.fixture))
            with TraceStore(path, read_only=True) as store:
                support, relations = detect_particle_support(store), detect_particle_relations(store)
                self.assertEqual(support["coverage"], "complete")
                self.assertEqual(len(support["returns"]), 18)
                self.assertEqual({row["inferred_core"] for row in support["returns"]}, {"Duon"})
                self.assertEqual(relations["coverage"], "complete")
                self.assertEqual((len(relations["waves"]), len(relations["couplings"]), len(relations["folds"])), (4, 2, 1))
                self.assertEqual(relations["failures"], [])
                # The existing detector has no declared-load27 bridge. Native
                # validation and the complete operand evidence remain available.
                self.assertEqual(relations["shedding"], [])
                events = list(store.iter_events())
                retained = next(e for e in events if e["kind"] == "evidence.solar_shedding")
                self.assertEqual(retained["payload"], regions.serial(traced["route_evidence"]))
                self.assertEqual(len([e for e in events if e["kind"] == "region.commit"]), 5)
                self.assertEqual(len([e for e in events if e["kind"] == "detector.boundary_view"]), 15)
                inspected = solar.inspect_trace(store)
                self.assertEqual((inspected["status"], inspected["coverage"]), ("verified", "complete"))
                self.assertEqual(inspected["declared_routed_current"], Q(2))
                self.assertEqual(inspected["route"]["surplus"], Q(3))
                self.assertEqual(inspected["chiral_flips"], [
                    {"committed": 14, "before": 1, "after": -1},
                    {"committed": 20, "before": -1, "after": 1}])
                self.assertEqual(solar.inspect_trace(store, max_events=1)["coverage"], "partial")
                self.assertEqual(solar.inspect_trace(store, max_bytes=100)["coverage"], "partial")
            declared = boundary_views(path, coverage_scope="declared-current")
            self.assertEqual(declared["coverage"], "complete", declared["failures"])
            for name, expected in traced["summary"].items():
                rows = declared["boundaries"][solar.SCOPE + "/" + name]["rows"]
                self.assertEqual(tuple(Q(row["current"]) for row in rows), expected)
            self.assertEqual(boundary_views(path, coverage_scope="full")["coverage"], "partial")

    @staticmethod
    def _clone(source, destination, mutate):
        """Produce a valid new archive around altered scientific evidence."""
        manifest = run_manifest("solar-tamper-test", source=solar.SOURCE,
                                kernel="native", preparation="hostile witness mutation")
        ids = {}
        with TraceStore(source, read_only=True) as original, TraceStore(destination, manifest) as target:
            for saved in original.iter_events():
                event = deepcopy(saved)
                if mutate(event) is False:
                    continue
                made = target.append(event["kind"], event["payload"], scope_id=event["scope_id"],
                    lineage_id=event["lineage_id"], source_ref=event["source_ref"],
                    scale_family=event["scale_family"], scale_index=event["scale_index"],
                    causal_parents=tuple(ids[parent] for parent in event["causal_parents"]))
                ids[saved["event_id"]] = made["event_id"]

    def test_reopened_route_detector_rejects_semantic_tampering(self):
        result = self.fixture
        sun, mercury, route, receiver = result["native_waves"]
        changed_route = solar._duon(sun.lineage, (1, 1, 1),
            pivot=(sun.returns[-2], sun.certificates[-2]), start=25, occurrence=100)
        changed_mercury = solar._duon(mercury.lineage, (1, 1, 1, 1, -1, 1))
        self.assertTrue(solar.n.validate_wave(changed_route))
        self.assertTrue(solar.n.validate_wave(changed_mercury))

        def bad_pivot(event):
            if event["kind"] == "evidence.solar_shedding":
                event["payload"]["route_support"] = regions.serial(changed_route)

        def bad_flip(event):
            if event["kind"] == "evidence.solar_chiral_flips":
                event["payload"]["flips"][0]["after"] = 1

        def conflicting_history(event):
            payload = event["payload"]
            if (event["kind"] == "particle.native_return" and
                    payload["wave"]["identity"] == mercury.identity and payload["return_ordinal"] == 3):
                motif = changed_mercury.motif_witnesses[3]
                payload.update(trace=regions.serial(changed_mercury.returns[3]),
                    certificate=regions.serial(changed_mercury.certificates[3]), motif_witness=regions.serial(motif),
                    formation=regions.serial({"construction": motif.construction,
                        "component_traces": motif.component_traces,
                        "component_certificates": motif.component_certificates}))

        def missing_target(event):
            if event["kind"] == "particle.native_return" and event["payload"]["wave"]["identity"] == receiver.identity:
                return False

        def burden_as_current(event):
            if event["kind"] == "evidence.channel_preparation" and event["payload"]["kind"] == "shedding":
                event["payload"]["signed_current"] = "3"

        def bad_snapshot(event):
            if event["kind"] == "region.commit" and event["payload"]["row"]["cut"] == 35:
                event["payload"]["row"]["snapshot"] = "0" * 64

        def disabled_route(event):
            if event["kind"] == "region.commit" and event["payload"]["row"]["cut"] == 35:
                row = event["payload"]["row"]
                next(c for c in row["channels"] if c["kind"] == "shedding")["available"] = False
                decoded = _decode_ratios(row)
                # Refresh the native cut hash: detection must check semantic
                # availability, not merely catch the old immutable hash.
                row["snapshot"] = regions.fingerprint((decoded["cut"], decoded["window"],
                    tuple(regions.Channel(**c) for c in decoded["channels"]),
                    tuple(regions.PendingProjection(**o) for o in decoded["obligations"]),
                    tuple(regions.SweepSample(**s) for s in decoded["sweep_field"])))

        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source"
            manifest = run_manifest("solar-original", source=solar.SOURCE, kernel="native", preparation="D0")
            with TraceStore(source, manifest) as store:
                solar.run(store)
            for label, mutate in (("pivot", bad_pivot), ("flip", bad_flip),
                                  ("history", conflicting_history), ("target", missing_target),
                                  ("burden-current", burden_as_current), ("cut-snapshot", bad_snapshot),
                                  ("disabled-route", disabled_route)):
                with self.subTest(label=label):
                    target = Path(directory) / label
                    self._clone(source, target, mutate)
                    with TraceStore(target, read_only=True) as store:
                        self.assertEqual(store.status()["coverage"], "complete")
                        inspected = solar.inspect_trace(store)
                        self.assertEqual((inspected["status"], inspected["coverage"]), ("rejected", "partial"))
                        self.assertTrue(inspected["failures"])
                        self.assertIsNone(inspected["route"])
                        self.assertIsNone(inspected["declared_routed_current"])

    def test_claim_scope_has_no_physical_target_or_si_map(self):
        scope = self.fixture["scope"]
        self.assertFalse(scope["physical_trajectory"])
        self.assertIsNone(scope["si_conversion"])
        self.assertIsNone(scope["physical_percent_error"])


if __name__ == "__main__":
    unittest.main()
