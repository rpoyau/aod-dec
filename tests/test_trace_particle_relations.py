"""Native relations are independent trace-derived axes, not inferred labels."""
from copy import deepcopy
from pathlib import Path
import tempfile
import unittest

from simulations import proton_accounting as proton
from simulations.trace_particle_relations import detect_particle_relations, _json_payload, RelationError


class SnapshotStore:
    """Validated-store interface for scientific witness mutation controls."""
    def __init__(self, events, coverage="complete"):
        self.events, self.coverage = events, coverage

    def iter_events(self):
        return iter(self.events)

    def status(self):
        return {"coverage": self.coverage}


class NativeParticleRelationsTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.events = []
        def capture(kind, payload):
            context = payload.get("wave", {})
            cls.events.append({"event_id": "snapshot-" + str(len(cls.events)), "kind": kind,
                "payload": payload, "scope_id": context.get("scope", payload.get("scope", proton.SCOPE)),
                "lineage_id": context.get("lineage", payload.get("lineage", "ancillary")),
                "scale_index": context.get("scale", payload.get("scale", 0))})
        with proton.observe_commits(capture) as observed:
            proton.run(Path(cls.tmp.name) / "proton.json")
        if observed.failure_count:
            raise RuntimeError(observed.last_error)
        cls.baseline = detect_particle_relations(SnapshotStore(cls.events))

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def altered(self, predicate, edit):
        events = deepcopy(self.events)
        target = next(event for event in events if predicate(event))
        edit(target)
        return detect_particle_relations(SnapshotStore(events))

    def test_actual_coupling_shedding_and_duad_seating_are_separate(self):
        result = self.baseline
        self.assertEqual(result["coverage"], "complete", result["failures"])
        self.assertEqual(len(result["couplings"]), 1)
        self.assertEqual(result["couplings"][0]["forward"]["lag"], -1)
        self.assertEqual(result["couplings"][0]["reverse"]["lag"], 1)
        self.assertEqual(len(result["folds"]), 1)
        self.assertEqual([row["state"] for row in result["shedding"]],
                         ["deficit_open", "deficit_open", "deficit_open", "shedding"])
        self.assertEqual([row["surplus"] for row in result["shedding"]], [0, 0, 0, 6])
        self.assertFalse(result["alternative_windows_additive"])
        self.assertEqual(len(result["seating"]), 1)
        self.assertEqual(result["seating"][0]["state"], "Duad")
        self.assertEqual(result["seating"][0]["current"], 0)
        self.assertEqual(len(result["waves"]), 5)
        for wave in result["waves"].values():
            if wave["core"] == "1:2":
                self.assertEqual(wave["coupling"]["status"], "unavailable")
                self.assertEqual(wave["shedding"]["status"], "unavailable")
            if wave["core"] == "Duon":
                self.assertEqual(wave["seating"]["status"], "verified")
                self.assertEqual(wave["coupling"]["status"], "unavailable")

    def test_empty_and_missing_families_remain_unavailable(self):
        empty = detect_particle_relations(SnapshotStore([]))
        self.assertEqual(empty["coverage"], "unavailable")
        self.assertEqual(empty["waves"], {})
        events = [event for event in self.events if event["kind"] != "particle.native_state"]
        result = detect_particle_relations(SnapshotStore(events))
        self.assertEqual(result["coverage"], "partial")
        self.assertEqual(result["seating"], [])
        self.assertEqual(result["shedding"], [])
        self.assertTrue(all(w["seating"]["status"] == "unavailable" for w in result["waves"].values()))

    def test_receipt_does_not_supply_duad_seating(self):
        events = [event for event in self.events if event["kind"] != "particle.native_state"
                  or event["payload"]["operation"] != "seat_duon"]
        events = events + [{"event_id": "receipt-only", "kind": "application.accounting.receipt",
                            "payload": {"state": "Duad"}}]
        result = detect_particle_relations(SnapshotStore(events))
        self.assertEqual(result["coverage"], "complete")
        self.assertEqual(result["seating"], [])
        duon = next(w for w in result["waves"].values() if w["core"] == "Duon")
        self.assertEqual(duon["seating"]["status"], "unavailable")

    def test_reflection_scalar_cannot_replace_occurrence_history(self):
        result = self.altered(lambda e: e["kind"] == "particle.native_contacts",
            lambda e: e["payload"]["contacts"][0]["reflection"].__setitem__(0, 999))
        self.assertEqual(result["coverage"], "partial")
        self.assertEqual(result["couplings"], [])
        self.assertTrue(any("reflection" in row["reason"] for row in result["failures"]))

    def test_changed_bridge_bytes_are_rejected(self):
        result = self.altered(lambda e: e["kind"] == "particle.native_shedding",
            lambda e: e["payload"]["model_bridge"].__setitem__("capacity", "4"))
        self.assertEqual(result["coverage"], "partial")
        self.assertEqual(len(result["shedding"]), 3)
        self.assertTrue(any("bridge bytes" in row["reason"] for row in result["failures"]))

    def test_seating_cannot_invent_a_current_response(self):
        result = self.altered(lambda e: e["kind"] == "particle.native_state" and
                             e["payload"]["operation"] == "seat_duon",
            lambda e: e["payload"]["state"].__setitem__("current", "1"))
        self.assertEqual(result["coverage"], "partial")
        self.assertEqual(result["seating"], [])
        self.assertTrue(any("seated state differs" in row["reason"] for row in result["failures"]))

    def test_malformed_rational_and_ambiguous_json_are_rejected(self):
        result = self.altered(lambda e: e["kind"] == "particle.native_state" and
                             e["payload"]["operation"] == "seat_duon",
            lambda e: e["payload"]["state"].__setitem__("pressure", "1/0"))
        self.assertEqual(result["coverage"], "partial")
        self.assertTrue(any("invalid exact rational" in row["reason"] for row in result["failures"]))
        with self.assertRaisesRegex(RelationError, "duplicate JSON"):
            _json_payload(b'{"x":1,"x":2}')

    def test_exact_duplicate_is_idempotent_and_conflict_is_rejected(self):
        result = detect_particle_relations(SnapshotStore(self.events + [deepcopy(self.events[-1])]))
        self.assertEqual(result["coverage"], "complete", result["failures"])
        for field in ("couplings", "folds", "shedding", "seating", "waves"):
            self.assertEqual(result[field], self.baseline[field])
        conflict = deepcopy(self.events[-1])
        conflict["payload"]["scope"] = "wrong"
        result = detect_particle_relations(SnapshotStore(self.events + [conflict]))
        self.assertEqual(result["coverage"], "partial")
        self.assertTrue(any("conflicting duplicate" in row["reason"] for row in result["failures"]))

    def test_relation_dependencies_do_not_use_capture_order(self):
        result = detect_particle_relations(SnapshotStore(list(reversed(self.events))))
        self.assertEqual(result["coverage"], "complete", result["failures"])
        for field in ("couplings", "folds", "shedding", "seating", "states"):
            self.assertEqual(sorted(result[field], key=lambda row: row["event_id"]),
                             sorted(self.baseline[field], key=lambda row: row["event_id"]))

    def test_alternative_contact_window_does_not_overwrite_source_channel(self):
        events = deepcopy(self.events)
        index = next(index for index, event in enumerate(events)
                     if event["kind"] == "particle.native_contacts")
        extra = deepcopy(events[index])
        extra["event_id"] = "alternative-contact-window"
        first_contact = extra["payload"]["contacts"][0]["committed"]
        extra["payload"]["comparison_interval"][0] = first_contact
        events.insert(index + 1, extra)
        result = detect_particle_relations(SnapshotStore(events))
        self.assertEqual(result["coverage"], "complete", result["failures"])
        self.assertEqual(len(result["couplings"]), 2)
        self.assertEqual(result["shedding"], self.baseline["shedding"])
        self.assertNotEqual(result["couplings"][0]["forward"], result["couplings"][1]["forward"])

    def test_envelope_scope_and_unknown_kind_do_not_pass(self):
        result = self.altered(lambda e: e["kind"] == "particle.native_return",
                             lambda e: e.__setitem__("scope_id", "wrong"))
        self.assertEqual(result["coverage"], "partial")
        self.assertTrue(any("envelope" in row["reason"] for row in result["failures"]))
        result = self.altered(lambda e: e["kind"] == "particle.native_contacts",
                             lambda e: e.__setitem__("scope_id", "wrong-cavity"))
        self.assertEqual(result["coverage"], "partial")
        self.assertEqual(result["couplings"], [])
        self.assertTrue(any("envelope" in row["reason"] for row in result["failures"]))
        unknown = {"event_id": "unknown", "kind": "particle.native_invented", "payload": {}}
        result = detect_particle_relations(SnapshotStore(self.events + [unknown]))
        self.assertEqual(result["coverage"], "partial")
        self.assertTrue(any("unsupported native" in row["reason"] for row in result["failures"]))

    def test_budgets_and_partial_archive_never_claim_complete(self):
        for limits in ({"max_events": 1}, {"max_bytes": 1}, {"max_relations": 1}):
            with self.subTest(limits=limits):
                result = detect_particle_relations(SnapshotStore(self.events), **limits)
                self.assertNotEqual(result["coverage"], "complete")
                self.assertTrue(result["failures"])
        result = detect_particle_relations(SnapshotStore(self.events, "partial"))
        self.assertEqual(result["coverage"], "partial")


if __name__ == "__main__":
    unittest.main()
