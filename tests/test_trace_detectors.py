"""Source-predicate detector checks; TP-04–11, TP-13/14/19/20/26."""
from copy import deepcopy
from dataclasses import asdict, replace
from fractions import Fraction
from pathlib import Path
import tempfile
import unittest

from runtime import dec_core as core
from runtime.dec_language import run_source
from simulations.trace_store import make_event, run_manifest, TraceStore, record_commits
from simulations.trace_views import Window
from simulations.trace_detectors import (CoreDetector, DetectionError, detect_core,
    inspect_fusion, inspect_recurrence, inspect_shedding, trace_from_record, replay_boundaries)
from simulations.vendor import aod_manual_native as native
from simulations import region_engine as region

ROOT = Path(__file__).resolve().parents[1]
P = "a" * 64


def envelope(i, kind, payload, parents=(), scope="B", lineage="a"):
    return make_event("r", i, kind, payload, scope_id=scope, lineage_id=lineage,
                      source_ref="runtime/dec_core.py:finite-hinge-return-v1", causal_parents=parents)


def primitives(n=2, *, open_tail=False, dwell=False):
    rows = [envelope(0, "core.start", {"start": 0, "proposal": "", "parents": []})]
    ordinal = 0
    for _ in range(n):
        for kind, source, target, orientation in (("step", 0, 1, 1), ("hinge", 1, 1, 0), ("step", 1, 0, -1)):
            ordinal += 1
            rows.append(envelope(len(rows), "core.transition", dict(ordinal=ordinal, kind=kind,
                source=source, target=target, orientation=orientation, start=0, proposal=""), (rows[-1]["event_id"],)))
    for kind, source, target, orientation in (([("dwell", 0, 0, 0)] if dwell else []) +
            ([("step", 0, 1, 1), ("hinge", 1, 1, 0)] if open_tail else [])):
        ordinal += 1
        rows.append(envelope(len(rows), "core.transition", dict(ordinal=ordinal, kind=kind,
            source=source, target=target, orientation=orientation, start=0, proposal=""), (rows[-1]["event_id"],)))
    return rows


def closed(lineage="a", scope="B"):
    t = core.new_trace(scope, lineage, core.Q4(0))
    t = core.append_event(t, "step", core.Q4(1), 1)
    t = core.append_event(t, "hinge")
    t = core.append_event(t, "step", core.Q4(0), -1)
    return core.close_trace(t)


def recorded(n=2, *, dwell=False, open_tail=False):
    with tempfile.TemporaryDirectory() as td:
        with TraceStore(Path(td)/"trace", run_manifest("recorded", source=P, kernel="core", preparation="test")) as store:
            with record_commits(store):
                t = core.new_trace("B", "a", core.Q4(0))
                for _ in range(n):
                    t = core.append_event(t, "step", core.Q4(1), 1)
                    t = core.append_event(t, "hinge")
                    t = core.append_event(t, "step", core.Q4(0), -1)
                    t = core.close_trace(t)
                    t = core.close_trace(t)
                if dwell:
                    t = core.append_event(t, "dwell")
                if open_tail:
                    t = core.append_event(t, "step", core.Q4(1), 1)
                    t = core.append_event(t, "hinge")
            store.finish()
            return list(store.iter_events())


class CoreDetectionTests(unittest.TestCase):
    def test_raw_primitives_detect_returns_without_close_labels(self):
        rows = primitives()
        self.assertTrue(all(r["kind"] != "core.closure" for r in rows))
        result = detect_core(rows)
        self.assertEqual((result["phases"], result["reclosure_beats"]), (None, None))
        self.assertEqual(result["candidate_returns"], 2)
        self.assertEqual([r["ordinal_end"] for r in result["return_candidates"]], [3, 6])
        self.assertTrue(all(r["verification"] == "verified" for r in result["return_candidates"]))
        self.assertEqual(result["coverage"], "partial")
        self.assertEqual(result["directional_rcd"]["status"], "unavailable")

    def test_duplicate_ingestion_and_dwell_do_not_add_beats(self):
        rows = recorded(dwell=True)
        d = CoreDetector()
        for row in rows:
            d.consume(row); d.consume(deepcopy(row))
        result = d.inspect()
        self.assertEqual((result["phases"], result["reclosure_beats"]), (2, 1))
        self.assertEqual(result["open_candidates"], [])

    def test_conflicting_content_and_bool_envelope_ordinal_rejected(self):
        row = primitives()[0]
        d = CoreDetector(); d.consume(row)
        hostile = deepcopy(row); hostile["payload"]["start"] = 1
        with self.assertRaises(DetectionError): d.consume(hostile)
        hostile = deepcopy(row); hostile["commit_ordinal"] = True
        with self.assertRaises(DetectionError): d.consume(hostile)

    def test_endpoint_only_without_hinge_rejected(self):
        rows = primitives(1)[:2]
        rows.append(envelope(2, "core.transition", dict(ordinal=2, kind="step", source=1,
            target=0, orientation=1, start=0, proposal=""), (rows[-1]["event_id"],)))
        result = detect_core(rows)
        self.assertEqual(result["phases"], 0)
        self.assertEqual(result["returns"][-1]["status"], "rejected")
        self.assertIn("hinge", result["returns"][-1]["reason"])

    def test_missing_start_and_parent_are_unresolved(self):
        rows = primitives(1)
        result = detect_core(rows[1:])
        self.assertEqual((result["phases"], result["coverage"]), (None, "partial"))
        missing_parent = envelope(1, rows[1]["kind"], rows[1]["payload"])
        result = detect_core([rows[0], missing_parent])
        self.assertEqual(result["returns"][-1]["status"], "unresolved")

    def test_rejected_early_endpoint_does_not_erase_valid_later_path(self):
        rows = primitives(0)
        path = (("step", 0, 1, 1), ("step", 1, 0, 1), ("step", 0, 1, 1),
                ("hinge", 1, 1, 0), ("step", 1, 0, -1))
        for ordinal, (kind, source, target, orientation) in enumerate(path, 1):
            rows.append(envelope(ordinal, "core.transition", dict(ordinal=ordinal, kind=kind,
                source=source, target=target, orientation=orientation, start=0, proposal=""), (rows[-1]["event_id"],)))
        result = detect_core(rows)
        self.assertEqual([r["status"] for r in result["returns"]], ["rejected"])
        self.assertEqual(result["candidate_returns"], 1)
        self.assertEqual(result["return_candidates"][0]["ordinal_end"], 5)
        self.assertIsNone(result["phases"])

    def test_h1_orientation_and_exact_primitive_ordinal_guards(self):
        for field, value in (("target", 3), ("ordinal", True), ("orientation", 0)):
            rows = primitives(1)[:1]
            payload = deepcopy(primitives(1)[1]["payload"]); payload[field] = value
            rows.append(envelope(1, "core.transition", payload, (rows[0]["event_id"],)))
            result = detect_core(rows)
            self.assertEqual(result["phases"], 0)
            self.assertEqual(result["returns"][-1]["status"], "rejected")

    def test_left_edge_uses_retained_start_and_right_edge_censors(self):
        rows = recorded(2, open_tail=True)
        d = CoreDetector()
        for row in rows: d.consume(row)
        left = d.inspect(Window(4, 5))
        self.assertEqual((left["phases"], left["coverage"]), (1, "complete"))
        self.assertEqual(left["returns"][0]["start_event_id"], rows[0]["event_id"])
        right = d.inspect(Window(0, 3))
        self.assertEqual((right["phases"], right["coverage"]), (0, "censored"))
        self.assertEqual(d.inspect(Window(0, 11))["coverage"], "censored")
        empty = d.inspect(Window(2, 2))
        self.assertEqual((empty["phases"], empty["returns"], empty["open_candidates"]), (0, [], []))

    def test_overlapping_windows_keep_one_identity(self):
        d = CoreDetector()
        for row in recorded(2): d.consume(row)
        a, b = d.inspect(Window(0, 9)), d.inspect(Window(4, 9))
        self.assertEqual({r["event_id"] for r in a["returns"]}, {r["event_id"] for r in b["returns"]})
        self.assertEqual(d.inspect()["phases"], 2)

    def test_detector_limits_are_explicit_incomplete_not_memory_loss(self):
        result = detect_core(primitives(2), max_events=3)
        self.assertEqual(result["coverage"], "partial")
        self.assertIsNone(result["phases"])
        self.assertEqual(result["resources"]["retained_events"], 3)
        self.assertIn("budget", result["incomplete_reason"])
        result = detect_core(primitives(1), max_bytes=32)
        self.assertEqual((result["coverage"], result["resources"]["retained_events"]), ("partial", 0))

    def test_live_reference_and_offline_archive_detection_agree(self):
        with tempfile.TemporaryDirectory() as td:
            manifest = run_manifest("live", source=P, kernel="core", preparation="closure.dec")
            with TraceStore(Path(td)/"trace", manifest) as store:
                with record_commits(store):
                    run_source((ROOT/"examples/closure.dec").read_text(), emit=lambda _: None)
                store.finish()
                rows = list(store.iter_events())
                live = detect_core(rows)
            with TraceStore.open(Path(td)/"trace") as reader:
                replay = detect_core(reader.iter_events())
            self.assertEqual(live, replay)
            self.assertEqual((live["phases"], live["reclosure_beats"]), (2, 1))

    def test_fusion_requires_executed_outer_return_and_retrievable_bytes(self):
        a, b = closed("a", "inner"), closed("b", "inner")
        f = core.fold(a, b, "outer", "ab")
        outer = core.outer(f, core.Q4(0))
        self.assertEqual(inspect_fusion(a, b)["status"], "unresolved")
        self.assertEqual(inspect_fusion(a, b, outer)["status"], "rejected")
        for kind, target, orientation in (("step", core.Q4(1), 1), ("hinge", None, 0), ("step", core.Q4(0), -1)):
            outer = core.append_event(outer, kind, target, orientation)
        outer = core.close_trace(outer)
        result = inspect_fusion(a, b, outer)
        self.assertEqual(result["status"], "verified")
        recovered = trace_from_record(result["evidence"])
        self.assertEqual(recovered, outer)
        tampered = deepcopy(result["evidence"]); tampered["parents"] = []
        with self.assertRaises(ValueError): trace_from_record(tampered)

    def test_live_fusion_detected_from_outer_primitives(self):
        with tempfile.TemporaryDirectory() as td:
            with TraceStore(Path(td)/"trace", run_manifest("f", source=P, kernel="core", preparation="fusion.dec")) as store:
                with record_commits(store):
                    run_source((ROOT/"examples/fusion.dec").read_text(), emit=lambda _: None)
                store.finish()
                rows = [r for r in store.iter_events() if r["kind"] not in ("core.closure", "core.fusion")]
                result = detect_core(rows)
                self.assertIsNone(result["phases"])
                self.assertEqual(result["candidate_returns"], 3)
                outer = [r for r in result["return_candidates"] if r.get("fusion")]
                self.assertEqual(len(outer), 1)
                self.assertEqual(len(outer[0]["parent_closure_digests"]), 2)
                scoped = detect_core(rows, scope="outer")
                self.assertEqual((scoped["candidate_returns"], scoped["requested_scope"]), (1, "outer"))
                self.assertTrue(all(r["scope_id"] == "outer" for r in scoped["return_candidates"]))
                committed = detect_core(store.iter_events(), scope="outer")
                self.assertEqual((committed["phases"], committed["reclosure_beats"]), (1, 0))

    def test_earlier_return_does_not_split_lawful_extended_counterbranch(self):
        with tempfile.TemporaryDirectory() as td:
            with TraceStore(Path(td)/"trace", run_manifest("long", source=P, kernel="core", preparation="extended-counterbranch")) as store:
                with record_commits(store):
                    t = core.new_trace("B", "a", core.Q4(0))
                    for kind, target, orientation in (("step", core.Q4(1), 1), ("hinge", None, 0),
                            ("step", core.Q4(0), -1), ("step", core.Q4(2), -1), ("step", core.Q4(0), -1)):
                        t = core.append_event(t, kind, target, orientation)
                    t = core.close_trace(t)
                store.finish()
                rows = list(store.iter_events())
                result = detect_core(rows)
                self.assertEqual((core.phases(t), result["phases"], result["reclosure_beats"]), (1, 1, 0))
                self.assertEqual((result["coverage"], result["open_candidates"]), ("complete", []))
                self.assertEqual([r["ordinal_end"] for r in result["returns"]], [5])
                self.assertEqual(result["returns"][0]["closure_digest"], t.closures[0].digest)
                self.assertEqual([r["commitment"] for r in result["return_candidates"]], ["not_selected", "selected"])
                raw = detect_core([r for r in rows if r["kind"] != "core.closure"])
                self.assertIsNone(raw["phases"])
                self.assertEqual(raw["candidate_returns"], 2)
                self.assertEqual(raw["returns"], [])

    def test_candidate_parse_budget_stops_without_selecting_a_false_history(self):
        result = detect_core(primitives(1), max_parses=1)
        self.assertEqual(result["coverage"], "partial")
        self.assertIsNone(result["phases"])
        self.assertIn("parse budget", result["incomplete_reason"])

    def test_forged_close_label_cannot_supply_missing_hinge(self):
        rows = primitives(1)[:2]
        rows.append(envelope(2, "core.transition", dict(ordinal=2, kind="step", source=1,
            target=0, orientation=1, start=0, proposal=""), (rows[-1]["event_id"],)))
        rows.append(envelope(3, "core.closure", {"begin":0,"end":2,"digest":P}, (rows[-1]["event_id"],)))
        result = detect_core(rows)
        self.assertEqual(result["observed_committed_phases"], 0)
        self.assertTrue(all(r["status"] != "verified" for r in result["returns"]))

    def test_uninstrumented_event_family_is_unavailable_not_zero_closures(self):
        row = envelope(0, "application.report", {"reported_value": 2})
        result = detect_core([row])
        self.assertEqual((result["coverage"], result["phases"], result["reclosure_beats"]), ("unavailable", None, None))
        self.assertEqual(detect_core([row], window=Window(0, 0))["phases"], 0)


class ManualDetectionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        inner = (native.recurrent("a"), native.recurrent("b"))
        t = native.outer_trace(inner)
        cls.folded = native.fold(inner, native.certified(t, "outer", inner=inner))
        cls.state = native.initial_state(inner[0])
        cls.record = native.shedding(cls.folded, cls.state, 27, 24, source_event_digest=P,
            model_bridge="b"*64, committed=t.indices[-1]+1, route="exoshedding", target_scope="B0", target_scale=0)

    def test_shedding_derived_from_bound_capacity_and_route(self):
        result = inspect_shedding(self.folded, self.record)
        self.assertEqual((result["status"], result["surplus"]), ("verified", "3"))
        self.assertEqual(result["signed_current"]["status"], "unresolved")
        self.assertIsNone(result["receipts"]["pending"])

    def test_labels_eviction_and_forged_surplus_cannot_be_shedding(self):
        for record in ({"status": "shedding", "reason": "eviction"}, replace(self.record, surplus=Fraction(4)),
                       replace(self.record, route="gate-off")):
            self.assertEqual(inspect_shedding(self.folded, record)["status"], "rejected")

    def test_actual_route_pending_and_receipt_timing_are_retained(self):
        a = region.accounting
        record = self.record
        content = a.ContentRecord("surplus", "physical", record.record_digest, record.committed,
            0, "declared-closure-burden", record.surplus, ("support",))
        archive = a.commit_archive(a.ArchiveState(), record.committed, ((content, "routed"),))
        route = a.RouteRecord("route", content.identity, record.committed, record.surplus, content.unit,
            (a.RouteLeg("leg", "exoshedding", record.surplus, record.target_scope),), record.record_digest)
        ledger = a.register_route(a.RouteLedger(), archive, route)
        pending = inspect_shedding(self.folded, record, route_ledger=ledger)
        self.assertEqual(pending["receipts"]["pending"][0]["amount"], 3)
        with self.assertRaises(ValueError):
            a.receive_route(ledger, "route", "leg", record.committed, record.target_scope, "receipt", P)
        received = a.receive_route(ledger, "route", "leg", record.committed+1, record.target_scope, "receipt", P)
        result = inspect_shedding(self.folded, record, route_ledger=received)
        self.assertEqual(result["receipts"]["pending"], ())
        self.assertEqual(result["receipts"]["retained_receipts"][0]["cut"], record.committed+1)
        self.assertEqual(result["signed_current"]["status"], "unresolved")

    def test_signed_current_needs_bound_full_audit(self):
        base = region.Ledger(("a", "b"), "native-current", 0, "finite", "return-count")
        selected = region.Region("a", ("a",))
        for provenance, valid in ((P, False), (self.record.record_digest, True)):
            ledger = region.commit(base, 1, 4,
                (region.Channel("shed", "physical", "a", "b", Fraction(3), provenance, "shedding"),))
            audit = region.audit_boundary(ledger, "a", (region.Frame(selected, selected, "stationary"),), scope="full")
            if valid:
                result = inspect_shedding(self.folded, self.record, signed_projection=audit)
                self.assertEqual(result["signed_current"]["value"], "3")
            else:
                with self.assertRaises(DetectionError):
                    inspect_shedding(self.folded, self.record, signed_projection=audit)

    def test_directional_rcd_lag_and_prior_are_preserved(self):
        a, b = native.recurrent("a", n=3), native.recurrent("b", n=4, spacing=6)
        contacts = native.contacts(a, b)
        left = inspect_recurrence(a, b, contacts, producer_window=(0, 23), bias=(Fraction(1,2), Fraction(0)), prior_lag=Fraction(3))
        right = inspect_recurrence(a, b, contacts, producer_window=(0, 23), reverse=True,
                                   bias=(Fraction(1,2), Fraction(0)), prior_lag=Fraction(3))
        self.assertEqual((left["status"], right["status"]), ("verified", "verified"))
        self.assertNotEqual(left["RCD"]["reflection_counts"], right["RCD"]["reflection_counts"])
        self.assertEqual(Fraction(left["lag"]), -Fraction(right["lag"]))
        self.assertEqual(Fraction(left["drift"]), -Fraction(right["drift"]))


class CoreContactDetectionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        with tempfile.TemporaryDirectory() as td:
            with TraceStore(Path(td)/"trace", run_manifest("contacts", source=P, kernel="core", preparation="coupling.dec")) as store:
                with record_commits(store):
                    output = []
                    run_source((ROOT/"examples/coupling.dec").read_text(), emit=output.append)
                store.finish()
                cls.rows = list(store.iter_events())
                cls.output = output

    def replace_contact(self, original, *, ordinal=None, payload=None, parents=None):
        return make_event(original["run_id"], original["commit_ordinal"] if ordinal is None else ordinal,
            "core.contact", original["payload"] if payload is None else payload,
            scope_id=original["scope_id"], lineage_id=original["lineage_id"], source_ref=original["source_ref"],
            causal_parents=original["causal_parents"] if parents is None else parents)

    def test_actual_coupling_archive_derives_cadence_signed_lag_and_drift(self):
        result = detect_core(self.rows)
        self.assertEqual(self.output, ["1/1", "-1", "0"])
        self.assertEqual(len(result["couplings"]), 1)
        comparison = result["couplings"][0]
        self.assertEqual((comparison["status"], comparison["cadence"], comparison["lag"], comparison["drift"]),
                         ("verified", "1", -1, 0))
        self.assertEqual(comparison["contacts"], [[3,4],[6,7]])
        self.assertEqual(comparison["directional_rcd"]["status"], "unavailable")
        self.assertEqual((result["phases"], result["coverage"]), (4, "complete"))

    def test_one_contact_does_not_fabricate_recurrence_values(self):
        rows = self.rows[:-1]
        self.assertEqual(rows[-1]["kind"], "core.contact")
        comparison = detect_core(rows)["couplings"][0]
        self.assertEqual(comparison["status"], "unresolved")
        self.assertIsNone(comparison["cadence"]); self.assertIsNone(comparison["lag"])

    def test_repeated_out_of_order_and_missing_return_parents_fail(self):
        first, last = [row for row in self.rows if row["kind"] == "core.contact"]
        duplicate = self.replace_contact(first, ordinal=len(self.rows))
        result = detect_core(self.rows + [duplicate])
        self.assertEqual(result["couplings"][0]["status"], "rejected")
        self.assertIn("ordered", result["couplings"][0]["reason"])
        missing = self.replace_contact(last, parents=())
        result = detect_core(self.rows[:-1] + [missing])
        self.assertEqual(result["couplings"][0]["status"], "rejected")
        self.assertIn("parents", result["couplings"][0]["reason"])
        payload = dict(last["payload"], left_return=True)
        invalid = self.replace_contact(last, payload=payload)
        self.assertEqual(detect_core(self.rows[:-1]+[invalid])["couplings"][0]["status"], "rejected")

    def test_contact_without_retained_lineage_is_unresolved(self):
        contacts = [row for row in self.rows if row["kind"] == "core.contact"]
        result = detect_core(contacts)
        self.assertEqual(result["couplings"][0]["status"], "unresolved")
        self.assertIsNone(result["couplings"][0]["cadence"])
        self.assertIsNone(result["phases"])


class BoundaryDetectionTests(unittest.TestCase):
    def test_boundary_replay_derives_internal_cancellation_and_motion(self):
        ledger = region.Ledger(("a", "b", "outside"), "native-current", 0, "finite", "return-count")
        with tempfile.TemporaryDirectory() as td:
            with TraceStore(Path(td)/"t", run_manifest("boundary", source=P, kernel="region", preparation="declared")) as store:
                sweep = region.SweepSample("b", Fraction(3), "native-current", 0, P)
                with record_commits(store):
                    region.commit(ledger, 1, 4, (
                        region.Channel("cross", "p-cross", "a", "outside", Fraction(8), P),
                        region.Channel("shed", "p-shed", "a", "outside", Fraction(2), P, "shedding"),
                        region.Channel("internal", "p-internal", "a", "b", Fraction(7), P)), sweep_field=(sweep,))
                store.finish()
                rows = list(store.iter_events())
                before, after = region.Region("before", ("a",)), region.Region("after", ("a", "b"))
                result = replay_boundaries(rows, name="moving", cells=after.cells,
                    frames=(region.Frame(before, after, "moving", (sweep,)),))
                self.assertEqual((result["status"], result["value"]), ("verified", "7"))
                self.assertIn("internal", result["rows"][0]["internal_ids"])
                whole = replay_boundaries(rows, name="whole", cells=ledger.cells)
                self.assertEqual(whole["value"], "0")
                with self.assertRaises(ValueError):
                    region.Frame(before, after, "moving")
                wrong = replay_boundaries(rows, name="bad", cells=after.cells,
                    frames=(region.Frame(before, after, "moving", (replace(sweep, value=Fraction(4)),)),))
                self.assertEqual(wrong["status"], "unresolved")

    def test_pending_burden_cannot_be_contact_only_complete_current(self):
        ledger = region.Ledger(("a", "b"), "native-current", 0, "finite", "return-count")
        with tempfile.TemporaryDirectory() as td:
            with TraceStore(Path(td)/"t", run_manifest("pending", source=P, kernel="region", preparation="declared")) as store:
                with record_commits(store):
                    region.commit(ledger, 1, 4, (), obligations=(region.PendingProjection("pending", "physical", "a", "b", Fraction(3), "burden", P),))
                store.finish()
                result = replay_boundaries(store.iter_events(), name="a", cells=("a",))
                self.assertEqual((result["status"], result["value"]), ("unresolved", None))


if __name__ == "__main__":
    unittest.main()
