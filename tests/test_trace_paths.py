"""Cross-dialect lineage, conservation and terminal-observation path checks."""
from copy import deepcopy
from dataclasses import replace
from pathlib import Path
import tempfile
import unittest

from simulations.trace_store import TraceStore, run_manifest
from simulations.trace_applications import run_particles
from simulations.trace_paths import detect_sheddic_paths
from simulations.trace_particle_relations import _shedding
from simulations.vendor import aod_manual_native as native


class EvidenceView:
    def __init__(self, original, events, coverage="complete"):
        self.original, self.events, self.coverage = original, events, coverage

    def iter_events(self):
        return iter(self.events)

    def get_by_key(self, key):
        return self.original.get_by_key(key)

    def status(self):
        return {"coverage": self.coverage}


class SheddicPathTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        root = Path(cls.tmp.name)
        manifest = run_manifest("path-tests", source={"repository": "aod-dec"},
            kernel="existing-source-bound", preparation="particle-encounter", backend="host")
        with TraceStore(root / "trace", manifest) as store:
            run_particles(root / "artifacts", store)
        cls.store = TraceStore.open(root / "trace")
        cls.events = tuple(cls.store.iter_events())
        cls.baseline = detect_sheddic_paths(cls.store)

    @classmethod
    def tearDownClass(cls):
        cls.store.close()
        cls.tmp.cleanup()

    def query(self, events, **kwargs):
        return detect_sheddic_paths(EvidenceView(self.store, events), **kwargs)

    def mutation(self, kind, edit, *, without_terminals=False):
        events = deepcopy(list(self.events))
        target = next(event for event in events if event["kind"] == kind)
        edit(target)
        if without_terminals:
            events = [event for event in events if event["kind"] != "particle.routing_finish"]
        return self.query(events)

    def test_actual_population_to_received_support_chain(self):
        report = self.baseline
        self.assertEqual(report["coverage"], "complete", report["failures"])
        self.assertEqual(report["population_to_path_coverage"], "complete", report["encounter_ancestry"])
        self.assertEqual(report["terminal_routing"]["status"], "verified")
        self.assertEqual(report["routing_plan"]["status"], "verified")
        self.assertEqual(len(report["terminal_routing"]["alternatives"]), 4)
        self.assertEqual(report["counts"]["received"], 1)
        path = report["paths"][0]
        self.assertEqual((path["amount"], path["unit"]), ("6", "closure-burden"))
        self.assertEqual(path["nodes"]["routed_support"]["signature"], "Duon")
        self.assertEqual(path["nodes"]["seating"]["state"], "Duad")
        self.assertFalse(path["seating_is_sensor_induced"])
        self.assertEqual(path["source_history"]["materialization"]["status"], "verified")
        cuts = path["causal_cuts"]
        self.assertEqual(cuts["shed"], cuts["route"])
        self.assertLess(cuts["route"], cuts["support_completed"])
        self.assertLess(cuts["support_completed"], cuts["receipt"])

    def test_alternatives_remain_separate_not_summed(self):
        alternatives = self.baseline["alternatives"]
        self.assertEqual([a["participation_window"] for a in alternatives], [2, 4, 6, 12])
        self.assertEqual([a["surplus"] for a in alternatives], ["0", "0", "0", "6"])
        self.assertEqual(len({a["source_history_digest"] for a in alternatives}), 1)
        self.assertTrue(all(a["source_history"]["status"] == "verified" for a in alternatives))
        self.assertFalse(self.baseline["alternative_windows_additive"])

    def test_hidden_positive_branch_cannot_reseal_a_complete_zero_output_view(self):
        branch_id = self.baseline["paths"][0]["nodes"]["archived_surplus"]["event_id"]
        positive_digest = self.baseline["paths"][0]["nodes"]["native_shedding"]["record_digest"]
        events = deepcopy([event for event in self.events
            if event["event_id"] != branch_id
            and event["kind"] not in ("application.accounting.route", "application.accounting.receipt")
            and not (event["kind"] == "particle.native_shedding"
                     and event["payload"]["record"]["record_digest"] == positive_digest)
            and not (event["kind"] == "particle.routing_finish"
                     and event["payload"]["source_native_shedding_digest"] == positive_digest)])
        for event in events:
            if event["kind"] == "particle.routing_finish":
                event["payload"]["alternative_count"] = 3
        report = self.query(events)
        self.assertEqual(report["counts"]["received"], 0)
        self.assertEqual(report["coverage"], "partial")
        self.assertEqual(report["terminal_routing"]["status"], "rejected")
        self.assertNotEqual(report["population_to_path_coverage"], "complete")

    def test_forged_routing_plan_cannot_change_bound_input_windows(self):
        report = self.mutation("particle.routing_plan",
            lambda event: event["payload"].__setitem__("windows", [2, 4, 6]))
        self.assertEqual(report["routing_plan"]["status"], "rejected")
        self.assertEqual(report["coverage"], "partial")
        self.assertIn("window schedule", report["routing_plan"]["reason"])

    def test_routing_plan_requires_actual_executed_producer_hash(self):
        def forge(event):
            bindings = event["payload"]["source_bindings"]
            key = next(key for key in bindings if key.endswith("proton_accounting.py"))
            bindings[key] = "0" * 64
        report = self.mutation("particle.routing_plan", forge)
        self.assertEqual(report["routing_plan"]["status"], "rejected")
        self.assertNotEqual(report["population_to_path_coverage"], "complete")

    def test_missing_plan_preserves_path_but_not_exhaustive_population_claim(self):
        report = self.query([event for event in self.events if event["kind"] != "particle.routing_plan"])
        self.assertEqual(report["coverage"], "complete", report["failures"])
        self.assertEqual(report["counts"]["received"], 1)
        self.assertEqual(report["routing_plan"]["status"], "unavailable")
        self.assertEqual(report["population_to_path_coverage"], "partial")

    def test_nonpositive_branch_requires_executed_population_ancestry(self):
        # Preserve the native record's internal exact digest checks while
        # changing its history. The cross-dialect source replay must reject it,
        # independently of the new plan comparison.
        events = deepcopy([event for event in self.events if event["kind"] != "particle.routing_plan"])
        event = next(event for event in events if event["kind"] == "particle.native_shedding"
                     and _shedding(event["payload"]["record"]).surplus == 0)
        record = _shedding(event["payload"]["record"]); original_digest = record.record_digest
        record = replace(record, source_event_digest="0" * 64)
        record = replace(record, operand_digest=native.digest((record.source_state_digest,
            record.source_event_digest, record.outer_return_id, record.outer_certificate_digest,
            record.closure_load, record.closure_cost, record.model_bridge)))
        record = replace(record, record_digest=native.digest(native._shedding_payload(record)))
        for key in ("source_event_digest", "operand_digest", "record_digest"):
            event["payload"]["record"][key] = getattr(record, key)
        for event in events:
            if event["kind"] == "particle.routing_finish" and event["payload"]["source_native_shedding_digest"] == original_digest:
                event["payload"]["source_native_shedding_digest"] = record.record_digest
        report = self.query(events)
        self.assertEqual(report["coverage"], "partial")
        self.assertNotEqual(report["population_to_path_coverage"], "complete")
        self.assertTrue(any(f["stage"] == "native_to_accounting" and "source digest" in f["reason"]
                            for f in report["failures"]))

    def test_wrong_native_source_digest_cannot_hide_in_valid_accounting(self):
        events = deepcopy(list(self.events))
        for event in events:
            if event["kind"] == "application.accounting.commit":
                for content, _ in event["payload"]["record"]["additions"]:
                    if content["identity"].startswith("surplus:"):
                        content["source_digest"] = "0" * 64
        report = self.query(events)
        self.assertEqual(report["coverage"], "partial")
        self.assertTrue(any("not this native" in f["reason"] for f in report["failures"]))

    def test_wrong_receipt_provenance_is_not_an_observed_response(self):
        report = self.mutation("application.accounting.receipt",
            lambda event: event["payload"]["record"].__setitem__("provenance", "0" * 64), without_terminals=True)
        self.assertEqual(report["coverage"], "partial")
        self.assertEqual(report["paths"][0]["status"], "partial")
        self.assertIn("receipt provenance differs", report["paths"][0]["reason"])

    def test_wrong_route_kind_is_not_just_a_label(self):
        report = self.mutation("application.accounting.route",
            lambda event: event["payload"]["record"]["legs"][0].__setitem__("kind", "redirected_support"),
            without_terminals=True)
        self.assertEqual(report["coverage"], "partial")
        self.assertIn("route kind differs", report["paths"][0]["reason"])

    def test_receipt_without_actual_routed_formation_is_partial(self):
        dropped = False; events = []
        for event in self.events:
            if event["kind"] == "particle.native_return" and event["payload"].get("family") == "routed-support" and not dropped:
                dropped = True
            else:
                events.append(event)
        report = self.query(events)
        self.assertEqual(report["coverage"], "partial")
        self.assertEqual(report["counts"]["received"], 0)

    def test_dropped_receipt_conflicts_with_terminal_producer_disposition(self):
        report = self.query([event for event in self.events if event["kind"] != "application.accounting.receipt"])
        self.assertEqual(report["coverage"], "partial")
        self.assertEqual(report["terminal_routing"]["status"], "rejected")
        self.assertEqual(report["population_to_path_coverage"], "partial")
        self.assertEqual(report["paths"][0]["status"], "pending_or_unobserved")

    def test_no_receipt_or_terminal_is_unknown_not_physical_nonreceipt(self):
        report = self.query([event for event in self.events if event["kind"] not in
            ("application.accounting.receipt", "particle.routing_finish")])
        self.assertEqual(report["paths"][0]["status"], "pending_or_unobserved")
        self.assertEqual(report["paths"][0]["nodes"]["receipt"]["status"], "unobserved")
        self.assertEqual(report["population_to_path_coverage"], "partial")

    def test_witnessed_pending_allocation_has_complete_accounting(self):
        # A declared terminal prefix retains the leg but has not consumed it.
        events = deepcopy([event for event in self.events if event["kind"] != "application.accounting.receipt"])
        finish = next(event["payload"] for event in events
                      if event["kind"] == "particle.routing_finish" and event["payload"]["routes"])
        route = finish["routes"][0]; leg = route["legs"][0]
        finish["receipts"] = []
        finish["pending_routes"] = [{"route_id": route["identity"], "leg_id": leg["identity"],
            "destination": leg["destination"], "amount": leg["amount"], "unit": route["unit"],
            "committed": route["committed"]}]
        report = self.query(events)
        self.assertEqual(report["coverage"], "complete", report["failures"])
        self.assertEqual(report["paths"][0]["status"], "pending_receipt")
        self.assertEqual(report["population_to_path_coverage"], "complete")

    def test_missing_terminal_alternative_does_not_pass(self):
        events = [event for event in self.events if event["kind"] != "particle.routing_finish"
                  or event["payload"]["alternative_index"] != 0]
        report = self.query(events)
        self.assertEqual(report["coverage"], "partial")
        self.assertEqual(report["terminal_routing"]["status"], "rejected")

    def test_missing_encounter_step_keeps_native_path_but_not_population_claim(self):
        events = [event for event in self.events if event["kind"] != "particle.encounter_update"
                  or event["payload"]["index"] != 0]
        report = self.query(events)
        self.assertEqual(report["counts"]["received"], 1)
        self.assertEqual(report["population_to_path_coverage"], "partial")

    def test_boolean_terminal_coordinate_cannot_alias_zero(self):
        report = self.mutation("particle.routing_finish",
            lambda event: event["payload"].__setitem__("scale", False))
        self.assertEqual(report["coverage"], "partial")
        self.assertEqual(report["terminal_routing"]["status"], "rejected")

    def test_legacy_snapshots_keep_ancestry_unavailable(self):
        events = [event for event in self.events if not event["kind"].startswith("particle.encounter_")
                  and event["kind"] != "particle.routing_finish"]
        report = self.query(events)
        self.assertEqual(report["coverage"], "complete", report["failures"])
        self.assertEqual(report["population_to_path_coverage"], "unavailable")
        self.assertEqual(report["counts"]["received"], 1)

    def test_orphan_route_is_not_silently_dropped(self):
        events = [event for event in self.events if event["kind"] != "particle.native_shedding"]
        report = self.query(events)
        self.assertEqual(report["coverage"], "partial")
        self.assertTrue(any(f["stage"] == "orphan_route" for f in report["failures"]))

    def test_duplicate_capture_preserves_path_identity(self):
        report = self.query(list(self.events) + [self.events[-1]])
        self.assertEqual(report["coverage"], "complete", report["failures"])
        self.assertEqual([p["path_id"] for p in report["paths"]], [p["path_id"] for p in self.baseline["paths"]])

    def test_shedding_evidence_alias_does_not_duplicate_emission(self):
        alias = deepcopy(next(event for event in self.events if event["kind"] == "particle.native_shedding"
                              and event["payload"]["record"]["status"] == "shedding"))
        alias["event_id"] = "second-capture-of-same-native-shedding"
        report = self.query(list(self.events) + [alias])
        self.assertEqual(report["coverage"], "complete", report["failures"])
        self.assertEqual(report["counts"]["received"], 1)
        self.assertEqual(len(report["alternatives"]), 4)

    def test_unknown_scope_does_not_borrow_global_coverage(self):
        report = self.query(self.events, scope="unobserved-cavity")
        self.assertEqual(report["coverage"], "unavailable")
        self.assertNotEqual(report["population_to_path_coverage"], "complete")

    def test_budget_and_archive_failure_are_partial(self):
        for limits in ({"max_events": 1}, {"max_bytes": 1}):
            report = self.query(self.events, **limits)
            self.assertEqual(report["coverage"], "partial")
            self.assertTrue(report["failures"])
        report = detect_sheddic_paths(EvidenceView(self.store, self.events, coverage="partial"))
        self.assertEqual(report["coverage"], "partial")
        self.assertEqual(report["population_to_path_coverage"], "partial")


if __name__ == "__main__":
    unittest.main()
