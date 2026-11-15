from dataclasses import replace
import cProfile
from hashlib import sha256
import json
import sys
import threading
import unittest
from unittest import mock

from simulations import awareness_engine_trace as t


class ExistingEngineTraceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.manual = t.fixture.load_manual()
        cls.runs = {case: t.run_engine_case(case) for case in t.CASES}

    def test_passive_source_execution_equals_original_in_every_case(self):
        for case, traced in self.runs.items():
            with self.subTest(case=case):
                ordinary = t.run_engine_case(case, trace=False)
                self.assertIsNone(traced["error"])
                self.assertEqual(self.manual.packed(traced["result"]), self.manual.packed(ordinary["result"]))
                self.assertTrue(traced["trace"].complete)
                verified = t.verify_trace(traced["trace"])
                self.assertEqual(verified["status"], "VERIFIED")
                self.assertEqual(verified["transitions"], 29 if case == "learning" else 6)
                self.assertEqual(verified["native_updates"], verified["transitions"])
                calls = [json.loads(raw) for raw in traced["trace"].events if json.loads(raw)["kind"] == "call"]
                self.assertEqual(calls[0]["function"], t.CASES[case][0])
                if case == "learning":
                    branches = [row["id"] for row in calls if row["function"] == "execute"]
                    self.assertEqual(len(branches), 8)
                    self.assertEqual(len(set(branches)), 8)

    def test_reopened_trace_preserves_typed_native_evidence(self):
        original = self.runs["delayed"]["trace"]
        reopened = t.trace_from_dict(json.loads(json.dumps(t.trace_to_dict(original))))
        self.assertEqual(reopened, original)
        self.assertEqual(t.verify_trace(reopened)["status"], "VERIFIED")

    def test_observer_budgets_preserve_completed_engine_outcome(self):
        for bounds in ({"max_events": 1}, {"max_bytes": 1}):
            with self.subTest(bounds=bounds):
                actual = t.run_engine_case("mindfulness", **bounds)
                self.assertEqual(self.manual.packed(actual["result"]), self.manual.packed(self.runs["mindfulness"]["result"]))
                self.assertFalse(actual["trace"].complete)
                self.assertLessEqual(actual["trace"].retained_bytes, actual["trace"].max_bytes)
                self.assertEqual(t.verify_trace(actual["trace"])["status"], "PARTIAL")

    def test_observer_failure_isolated_and_existing_profiler_restored(self):
        old = sys.getprofile(); seen = []
        def previous(frame, event, arg):
            if event == "call" and frame.f_code is self.manual.mindfulness.__code__:
                seen.append(True)
        try:
            sys.setprofile(previous)
            with mock.patch.object(t._Collector, "observe", side_effect=RuntimeError("observer broke")):
                actual = t.run_engine_case("mindfulness")
            self.assertIs(sys.getprofile(), previous)
        finally:
            sys.setprofile(old)
        self.assertTrue(seen)
        self.assertFalse(actual["trace"].complete)
        self.assertEqual(self.manual.packed(actual["result"]), self.manual.packed(self.runs["mindfulness"]["result"]))

    def test_engine_error_remains_original_when_trace_also_exhausted(self):
        def broken_update(s, attention, input_index):
            raise RuntimeError("original engine failure")
        with mock.patch.object(t.fixture.native, "update", broken_update):
            with self.assertRaisesRegex(RuntimeError, "^original engine failure$") as caught:
                t.run_engine_case("mindfulness", max_events=1)
            self.assertFalse(caught.exception.engine_trace.complete)
            captured = t.run_engine_case("mindfulness", max_events=1, capture_errors=True)
        self.assertIsNone(captured["result"])
        self.assertEqual(captured["error"], {"type": "RuntimeError", "message": "original engine failure"})
        self.assertFalse(captured["trace"].complete)

    def test_existing_native_profiler_is_left_active(self):
        profiler = cProfile.Profile()
        profiler.enable()
        try:
            previous = sys.getprofile()
            actual = t.run_engine_case("mindfulness")
            self.assertIs(sys.getprofile(), previous)
        finally:
            profiler.disable()
        self.assertTrue(any(entry.code is self.manual.mindfulness.__code__ for entry in profiler.getstats()))
        self.assertEqual(actual["trace"].complete, previous is None or callable(previous))
        self.assertEqual(self.manual.packed(actual["result"]), self.manual.packed(self.runs["mindfulness"]["result"]))

    @staticmethod
    def changed(trace, events=None, witnesses=None):
        events = trace.events if events is None else tuple(events)
        witnesses = trace.witnesses if witnesses is None else tuple(witnesses)
        used = sum(len(x.encode()) for x in events)+sum(64+len(x.encode()) for _, x in witnesses)
        return replace(trace, events=events, witnesses=witnesses, retained_bytes=used)

    def replace_witness(self, trace, old, value):
        names = {v:k for k,v in t._registry(self.manual).items()}
        raw = t._json(t._pack(value, names)); new = sha256(raw.encode()).hexdigest()
        witnesses = [(new, raw) if key == old else (key, data) for key, data in trace.witnesses]
        events = [event.replace(old, new) for event in trace.events]
        return self.changed(trace, events, witnesses)

    def test_rehashed_query_and_cut_forgery_rejected(self):
        original = self.runs["mindfulness"]["trace"]
        rows = [json.loads(raw) for raw in original.events]
        call = next(row for row in rows if row["kind"] == "call" and row["function"] == "transition")
        key = call["arguments"]["inp"]
        decoded = t._unpack(json.loads(dict(original.witnesses)[key]), t._registry(self.manual))
        forged = self.replace_witness(original, key, replace(decoded, query=t.fixture.native.Q(7)))
        with self.assertRaisesRegex(ValueError, "query detached"):
            t.verify_trace(forged)
        call["native_cut"] += 1
        with self.assertRaisesRegex(ValueError, "cut snapshot"):
            t.verify_trace(self.changed(original, [t._json(row) for row in rows]))

    def test_missing_execution_and_rehashed_result_cannot_verify(self):
        original = self.runs["mindfulness"]["trace"]
        rows = [json.loads(raw) for raw in original.events]
        dropped = [r for r in rows if r["function"] not in ("transition", "native.update")]
        with self.assertRaisesRegex(ValueError, "coverage differs"):
            t.verify_trace(self.changed(original, [t._json(row) for row in dropped]))
        row = next(r for r in rows if r["kind"] == "return" and r["function"] == "transition")
        key = row["result"]
        state, event = t._unpack(json.loads(dict(original.witnesses)[key]), t._registry(self.manual))
        forged_state = replace(state, present=replace(state.present, current=t.fixture.native.Q(19)))
        forged_event = dict(event, current=t.fixture.native.Q(19))
        with self.assertRaisesRegex(ValueError, "native replay"):
            t.verify_trace(self.replace_witness(original, key, (forged_state, forged_event)))

    def test_coherently_rehashed_different_initial_model_is_rejected(self):
        original = self.runs["mindfulness"]["trace"]
        rows = [json.loads(raw) for raw in original.events]
        call = next(r for r in rows if r["kind"] == "call" and r["function"] == "transition")
        update = next(r for r in rows if r["kind"] == "call" and r["function"] == "native.update")
        returned = next(r for r in rows if r["kind"] == "return" and r["id"] == call["id"])
        updated = next(r for r in rows if r["kind"] == "return" and r["id"] == update["id"])
        registry = t._registry(self.manual)
        def value(key):
            return t._unpack(json.loads(dict(original.witnesses)[key]), registry)
        forged = original
        state = value(call["arguments"]["s"])
        forged = self.replace_witness(forged, call["arguments"]["s"],
            replace(state, present=replace(state.present, current=t.fixture.native.Q(99))))
        inp = value(call["arguments"]["inp"])
        forged = self.replace_witness(forged, call["arguments"]["inp"], replace(inp, query=t.fixture.native.Q(-99)))
        for key in (update["arguments"]["s"], updated["result"]):
            forged = self.replace_witness(forged, key, replace(value(key), current=t.fixture.native.Q(99)))
        state, event = value(returned["result"])
        forged = self.replace_witness(forged, returned["result"], (state, dict(event, query=t.fixture.native.Q(-99))))
        with self.assertRaisesRegex(ValueError, "source-owned initial state"):
            t.verify_trace(forged)

    def test_complete_root_unwind_and_deleted_context_are_rejected(self):
        original = self.runs["mindfulness"]["trace"]
        rows = [json.loads(raw) for raw in original.events]
        root = next(row for row in rows if row["kind"] == "call" and row["parent"] is None)
        root_return = next(row for row in rows if row["kind"] == "return" and row["id"] == root["id"])
        root_return["status"] = "none-or-unwound"
        with self.assertRaisesRegex(ValueError, "did not return normally"):
            t.verify_trace(self.changed(original, [t._json(row) for row in rows]))
        rows = [json.loads(raw) for raw in original.events]
        context = next(row for row in rows if row["kind"] == "call" and row["function"] == "context")
        trimmed = [row for row in rows if row["id"] != context["id"]]
        for row in trimmed:
            if row["kind"] == "call" and row["parent"] == context["id"]:
                row["parent"] = context["parent"]
        with self.assertRaisesRegex(ValueError, "child-call coverage differs"):
            t.verify_trace(self.changed(original, [t._json(row) for row in trimmed]))
        self.assertIn("simulations/trace_history_union.py", json.loads(original.source_binding))

    def test_rehashed_unrelated_retention_cannot_replace_preparation(self):
        original = self.runs["mindfulness"]["trace"]
        rows = [json.loads(raw) for raw in original.events]
        call = next(row for row in rows if row["kind"] == "call" and row["function"] == "retain")
        returned = next(row for row in rows if row["kind"] == "return" and row["id"] == call["id"])
        prior = t._unpack(json.loads(dict(original.witnesses)[call["arguments"]["prior"]]), t._registry(self.manual))
        changed_prior = replace(prior, cross=(t.fixture.native.Q(7),))
        changed_retention = self.manual.retain(changed_prior)
        forged = self.replace_witness(original, call["arguments"]["prior"], changed_prior)
        forged = self.replace_witness(forged, returned["result"], changed_retention)
        with self.assertRaisesRegex(ValueError, "source-owned retention operands"):
            t.verify_trace(forged)

    def test_extra_record_fields_and_noncanonical_json_are_rejected(self):
        original = self.runs["mindfulness"]["trace"]
        rows = [json.loads(raw) for raw in original.events]
        rows[0]["unbound_observation"] = "not source evidence"
        with self.assertRaisesRegex(ValueError, "fields/types differ"):
            t.verify_trace(self.changed(original, [t._json(row) for row in rows]))
        events = list(original.events)
        events[0] = json.dumps(json.loads(events[0]), indent=2)
        with self.assertRaisesRegex(ValueError, "canonical call record"):
            t.verify_trace(self.changed(original, events))

    def test_boolean_certificate_scale_cannot_alias_canonical_integer(self):
        original = self.runs["mindfulness"]["trace"]
        call = next(json.loads(raw) for raw in original.events if json.loads(raw)["kind"] == "call"
                    and json.loads(raw)["function"] == "transition")
        key = call["arguments"]["world"]
        world = t._unpack(json.loads(dict(original.witnesses)[key]), t._registry(self.manual))
        changed = replace(world, waves=tuple(replace(wave,
            certificates=tuple(replace(certificate, scale=False) for certificate in wave.certificates))
            for wave in world.waves))
        # Python equality and the inherited validator alone admit this alias.
        self.assertEqual(changed, world)
        self.manual.validate_world(changed)
        with self.assertRaisesRegex(ValueError, "source-owned world/model"):
            t.verify_trace(self.replace_witness(original, key, changed))

    @unittest.skipUnless(hasattr(sys, "monitoring"), "Python3.12 monitoring required")
    def test_monitoring_and_profile_capture_identical_complete_trace(self):
        for case, actual in self.runs.items():
            with self.subTest(case=case):
                profiled = t.run_engine_case(case, observer="profile")
                self.assertEqual(actual["observer_backend"], "sys.monitoring")
                self.assertEqual(profiled["observer_backend"], "sys.setprofile")
                self.assertEqual(actual["trace"], profiled["trace"])
                self.assertEqual(self.manual.packed(actual["result"]), self.manual.packed(profiled["result"]))

    @unittest.skipUnless(hasattr(sys, "monitoring"), "Python3.12 monitoring required")
    def test_occupied_tool_ids_fall_back_without_changing_foreign_ownership(self):
        api = sys.monitoring; owned = []
        try:
            for identity in range(6):
                if api.get_tool(identity) is None:
                    api.use_tool_id(identity, "aod-test-foreign"); owned.append(identity)
            before = tuple(api.get_tool(i) for i in range(6))
            actual = t.run_engine_case("mindfulness", observer="monitoring")
            self.assertEqual(actual["observer_backend"], "sys.setprofile")
            self.assertEqual(tuple(api.get_tool(i) for i in range(6)), before)
            self.assertEqual(actual["trace"], self.runs["mindfulness"]["trace"])
        finally:
            for identity in owned:
                api.free_tool_id(identity)

    @unittest.skipUnless(hasattr(sys, "monitoring"), "Python3.12 monitoring required")
    def test_monitoring_thread_filter_and_complete_resource_cleanup(self):
        collector = t._Collector(self.manual, "mindfulness", t._sources(self.manual), 4096, 8*1024*1024)
        observer = t._MonitoringObserver(collector)
        before = tuple(sys.monitoring.get_tool(i) for i in range(6))
        self.assertTrue(observer.install())
        identity = observer.used_tool_id
        try:
            thread = threading.Thread(target=self.manual.mindfulness)
            thread.start(); thread.join()
            self.assertEqual(collector.events, [])
        finally:
            observer.close()
        self.assertEqual(tuple(sys.monitoring.get_tool(i) for i in range(6)), before)
        sys.monitoring.use_tool_id(identity, "aod-test-cleanup")
        try:
            self.assertEqual(sys.monitoring.get_events(identity), 0)
            for code in collector.codes:
                self.assertEqual(sys.monitoring.get_local_events(identity, code), 0)
            for event in (sys.monitoring.events.PY_START, sys.monitoring.events.PY_RETURN, sys.monitoring.events.PY_UNWIND):
                self.assertIsNone(sys.monitoring.register_callback(identity, event, None))
        finally:
            sys.monitoring.free_tool_id(identity)

    def test_monitoring_lifecycle_errors_never_replace_engine_outcome(self):
        for method in ("install", "close"):
            with self.subTest(method=method):
                if method == "close":
                    # No resource is acquired in this cleanup-failure injection.
                    first = mock.patch.object(t._MonitoringObserver, "install", return_value=False)
                else:
                    first = mock.patch.object(t._MonitoringObserver, "close", return_value=None)
                with first, mock.patch.object(t._MonitoringObserver, method, side_effect=RuntimeError("observer lifecycle failed")):
                    actual = t.run_engine_case("mindfulness", observer="monitoring")
                self.assertFalse(actual["trace"].complete)
                self.assertEqual(self.manual.packed(actual["result"]), self.manual.packed(self.runs["mindfulness"]["result"]))

    def test_profile_and_monitoring_preserve_original_engine_error(self):
        def broken_update(s, attention, input_index):
            raise RuntimeError("original engine failure")
        traces = []
        with mock.patch.object(t.fixture.native, "update", broken_update):
            for observer in ("profile", "monitoring"):
                actual = t.run_engine_case("mindfulness", observer=observer, capture_errors=True)
                self.assertEqual(actual["error"], {"type": "RuntimeError", "message": "original engine failure"})
                traces.append(actual["trace"])
        self.assertEqual(traces[0], traces[1])


if __name__ == "__main__":
    unittest.main()
