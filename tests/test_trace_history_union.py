"""Independent records cannot give one native occurrence two histories."""
from copy import deepcopy
from dataclasses import asdict, replace
from pathlib import Path
import tempfile
import unittest

from simulations import proton_accounting as proton
from simulations.vendor import aod_manual_native as native
from simulations.trace_applications import NativeParticleRecorder
from simulations.trace_history_union import RetainedHistoryUnion, HistoryUnionError
from simulations.trace_particle_relations import detect_particle_relations, _fold, _state, _shedding
from simulations.trace_store import TraceStore, run_manifest
from simulations.trace_support import decode_trace, decode_motif, inspect_native_return, detect_particle_support
from tests.test_trace_support import formation


def shared_interior(left, right):
    """Share one raw event name, leaving both individual certificates valid."""
    trace = decode_trace(right["trace"])
    old, new = trace.event_ids[1], left["trace"]["event_ids"][1]
    trace = replace(trace, event_ids=tuple(new if v == old else v for v in trace.event_ids),
                    reflection=replace(trace.reflection, outbound_ids=tuple(
                        new if v == old else v for v in trace.reflection.outbound_ids)))
    old_motif = decode_motif(right["motif_witness"])
    certificate = native.certify(trace)
    registry = {c.certificate_digest: c for c in (*old_motif.component_certificates, certificate)}
    motif = native.motif_witness(old_motif.core, trace, old_motif.construction,
        tuple(zip(old_motif.component_traces, old_motif.component_certificates)), registry,
        old_motif.enclosure, old_motif.chirality)
    result = deepcopy(right)
    result.update(trace=asdict(trace), certificate=asdict(certificate), motif_witness=asdict(motif))
    return result


def manifest():
    return run_manifest("union-controls", source={"test": "R07-01"},
        kernel="canonical-native", preparation="separately-admitted-records", backend="host")


class SupportUnionTests(unittest.TestCase):
    def assert_no_verified_axis(self, value):
        if type(value) is dict:
            self.assertNotEqual(value.get("status"), "verified")
            if "detected_support_form" in value:
                self.assertIsNone(value["detected_support_form"])
            for child in value.values():
                self.assert_no_verified_axis(child)
        elif type(value) in (list, tuple):
            for child in value:
                self.assert_no_verified_axis(child)

    def detect(self, payloads, **budgets):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "trace"
            with TraceStore(path, manifest()) as store:
                for index, payload in enumerate(payloads):
                    t = payload["trace"]
                    store.append("particle.native_return", payload, scope_id=t["scope"],
                        lineage_id=t["lineage"], scale_index=t["scale"],
                        source_ref="union-test", source_key=str(index))
            with TraceStore.open(path) as reopened:
                self.assertEqual(reopened.status()["coverage"], "complete")
                return detect_particle_support(reopened, **budgets)

    def test_conflicting_nonterminal_bindings_reject_reopened_union(self):
        left = formation()
        right = shared_interior(left, formation(occurrence=1, offset=100))
        self.assertEqual([inspect_native_return(p)["status"] for p in (left, right)], ["verified"] * 2)
        with self.assertRaisesRegex(native.AdmissionError, "conflicting coordinates"):
            native.validate_record_collection(tuple(native.certified(decode_trace(p["trace"]))
                                                    for p in (left, right)))
        result = self.detect((left, right))
        self.assertEqual((result["coverage"], result["observed_formations"]), ("partial", 0))
        self.assertTrue(all(row["status"] != "verified" for row in result["returns"]))
        self.assert_no_verified_axis(result["returns"])
        self.assertIn("conflicting coordinates", result["history_union"]["reason"])

    def test_identical_shared_event_is_not_a_conflict(self):
        left = formation()
        right = shared_interior(left, formation(occurrence=1))
        result = self.detect((left, right))
        self.assertEqual((result["coverage"], result["observed_formations"]), ("complete", 2))

    def test_same_raw_event_in_distinct_lineage_is_not_a_conflict(self):
        left = formation()
        right = shared_interior(left, formation(occurrence=1, offset=100, lineage="other"))
        result = self.detect((left, right))
        self.assertEqual((result["coverage"], result["observed_formations"]), ("complete", 2))

    def test_history_budgets_do_not_verify_a_truncated_collection(self):
        for budget in ({"max_history_bindings": 1}, {"max_history_nodes": 1}):
            with self.subTest(budget=budget):
                result = self.detect((formation(),), **budget)
                self.assertEqual((result["coverage"], result["observed_formations"]), ("partial", 0))
                self.assertIn("budget exhausted", result["history_union"]["reason"])
                self.assert_no_verified_axis(result["returns"])

    def test_exhaustion_after_first_valid_row_revokes_nested_verdicts(self):
        first = formation()
        required = self.detect((first,))["history_union"]["nodes"]
        result = self.detect((first, formation(occurrence=1, offset=100)),
                             max_history_nodes=required + 1)
        self.assertEqual((result["coverage"], result["observed_formations"]), ("partial", 0))
        self.assertEqual(len(result["returns"]), 2)
        self.assert_no_verified_axis(result["returns"])


class RecursiveUnionTests(unittest.TestCase):
    def test_component_trace_is_included(self):
        motif = decode_motif(formation()["motif_witness"])
        component = motif.component_traces[0]
        altered = native.trace(component.lineage, 0, scope=component.scope, start_index=100)
        native.admitted(altered, native.certify(altered),
                        {native.certify(altered).certificate_digest: native.certify(altered)})
        union = RetainedHistoryUnion()
        union.include(motif)
        with self.assertRaisesRegex(HistoryUnionError, "immutable terminal"):
            union.include(altered)

    def test_nested_outer_certificate_retains_inner_histories(self):
        left, right = native.recurrent("left", n=2), native.recurrent("right", n=2)
        outer = native.outer_trace((left, right), lineage="outer", scope="B1", scale=1)
        record = native.certified(outer, "outer", inner=(left, right))
        changed = native.trace("right", 1, start_index=100)
        union = RetainedHistoryUnion()
        union.include(record[1])
        with self.assertRaisesRegex(HistoryUnionError, "immutable terminal"):
            union.include(changed)

    def test_legitimate_wave_prefixes_accept_in_either_order(self):
        longer = native.recurrent("prefix", n=3)
        shorter = native.wave([native.certified(t) for t in longer.returns[:2]])
        for pair in ((shorter, longer), (longer, shorter)):
            union = RetainedHistoryUnion()
            union.include(*pair)
            self.assertIsNone(union.failure)

    def test_same_wave_identity_cannot_replace_an_overlapping_ordinal(self):
        original = native.recurrent("prefix", n=3)
        replacement = native.trace("prefix", 99, start_index=20)
        changed = native.wave([native.certified(t) for t in (*original.returns[:2], replacement)])
        self.assertEqual(changed.identity, original.identity)
        # Distinct last event names evade event-coordinate checking alone.
        native.field("B0", "union-event-control", (original, changed))
        union = RetainedHistoryUnion()
        union.include(original)
        with self.assertRaisesRegex(HistoryUnionError, "wave prefix"):
            union.include(changed)

    def test_exhaustion_remains_sticky_and_indexes_stay_bounded(self):
        for budgets in ({"max_bindings": 1}, {"max_nodes": 1}):
            union = RetainedHistoryUnion(**budgets)
            with self.assertRaisesRegex(HistoryUnionError, "budget exhausted"):
                union.include(native.trace())
            with self.assertRaisesRegex(HistoryUnionError, "budget exhausted"):
                union.include()
            self.assertLessEqual(len(union.bindings), union.max_bindings)
            self.assertLessEqual(union.nodes, union.max_nodes)


class FoldUnionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.events = []
        def capture(kind, payload):
            cls.events.append((kind, payload))
        with tempfile.TemporaryDirectory() as tmp:
            with proton.observe_commits(capture):
                proton.run(Path(tmp) / "proton.json")

    def detect(self, events, **budgets):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "trace"
            with TraceStore(path, manifest()) as store:
                recorder = NativeParticleRecorder(store)
                for kind, payload in events:
                    recorder(kind, payload)
            with TraceStore.open(path) as reopened:
                self.assertEqual(reopened.status()["coverage"], "complete")
                return detect_particle_relations(reopened, **budgets)

    def test_valid_real_archive_and_history_limits(self):
        result = self.detect(self.events)
        self.assertEqual(result["coverage"], "complete", result["failures"])
        self.assertEqual((len(result["folds"]), len(result["shedding"])), (1, 4))
        for budget in ({"max_history_bindings": 1}, {"max_history_nodes": 1}):
            result = self.detect(self.events, **budget)
            self.assertEqual(result["coverage"], "partial")
            self.assertEqual((result["folds"], result["shedding"]), ([], []))
            self.assertIn("budget exhausted", result["history_union"]["reason"])

    def test_valid_altered_fold_conflicts_with_captured_carrier(self):
        events = deepcopy(self.events)
        payload = next(p for kind, p in events if kind == "particle.native_fold")
        fold = _fold(payload["fold"])
        left, right = fold["inner"]
        old = right.returns[-1]
        vertices = tuple((v & ~3) | ((v & 1) << 1) | ((v & 2) >> 1) for v in old.vertices)
        altered = replace(old, vertices=vertices)
        self.assertNotEqual(vertices, old.vertices)
        prior = right.motif_witnesses[-1]
        certificate = native.certify(altered)
        motif = native.motif_witness(prior.core, altered, prior.construction,
            tuple(zip(prior.component_traces, prior.component_certificates)),
            {c.certificate_digest: c for c in (*prior.component_certificates, certificate)},
            prior.enclosure, prior.chirality)
        traces, certificates = right.returns[:-1] + (altered,), right.certificates[:-1] + (certificate,)
        registry = {c.certificate_digest: c for c in certificates}
        changed = native.wave(tuple((t, c, registry) for t, c in zip(traces, certificates)),
                              right.motif_witnesses[:-1] + (motif,))
        self.assertEqual(changed.identity, right.identity)
        with self.assertRaises(native.AdmissionError):
            native.field(left.scope, "joint-control", (left, right, changed))
        original_outer = fold["outer"][0]
        outer = native.outer_trace((left, changed), lineage=original_outer.lineage,
            scope=original_outer.scope, scale=original_outer.scale, start_index=original_outer.indices[0])
        new_fold = native.fold((left, changed), native.certified(outer, "outer", inner=(left, changed)))
        payload["fold"] = proton.encode(new_fold)
        for kind, payload in events:
            if kind == "particle.native_shedding":
                record = _shedding(payload["record"])
                replaced = native.shedding(new_fold, _state(payload["source_state"]),
                    record.closure_load, record.closure_cost, source_event_digest=record.source_event_digest,
                    model_bridge=record.model_bridge, committed=record.committed, route=record.route,
                    target_scope=record.target_scope, target_scale=record.target_scale)
                payload["record"] = proton.encode(replaced)
        result = self.detect(events)
        self.assertEqual(result["coverage"], "partial")
        self.assertEqual((result["folds"], result["shedding"], result["couplings"]), ([], [], []))
        self.assertIsNotNone(result["history_union"]["reason"])
        self.assertTrue(all(w["closure"]["status"] != "verified" for w in result["waves"].values()))
