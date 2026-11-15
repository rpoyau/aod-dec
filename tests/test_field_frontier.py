"""Exact compact response, recoverable closure evidence and bounded scheduling."""
from dataclasses import fields, replace
from fractions import Fraction as Q
import hashlib
import json
import math
from pathlib import Path
import random
import tempfile
import unittest
from unittest.mock import patch

from simulations.field_frontier import (
    ExposureRequired, FieldFrontier, FieldSummary, FrontierError,
    IndexedFieldQueue, WitnessArchive, WitnessRef,
)
from simulations.trace_support import decode_certificate, decode_motif, decode_trace
from simulations.vendor import aod_manual_native as native
from tests.test_trace_support import formation


SOURCE = hashlib.sha256(b"independent supplied attention fixture").hexdigest()


def outer_wave():
    inner = tuple(native.recurrent("inner:" + str(i), n=2) for i in range(2))
    records = [native.certified(native.outer_trace(inner, lineage="outer", occurrence=k,
                start_index=30+10*k), "outer", inner=inner) for k in range(2)]
    return native.wave(records)


def generic_support():
    payloads = [formation(3, 1, lineage="three-one", occurrence=k, offset=30*k)
                for k in range(2)]
    traces = tuple(decode_trace(p["trace"]) for p in payloads)
    certificates = tuple(decode_certificate(p["certificate"]) for p in payloads)
    registry = {c.certificate_digest: c for c in certificates}
    return native.wave(tuple((t, c, registry) for t, c in zip(traces, certificates)),
                       tuple(decode_motif(p["motif_witness"]) for p in payloads))


def conflicting_waves():
    original = native.recurrent("overlap", n=2)
    changed = replace(original.returns[0], vertices=(0, 2, 0))
    conflicting = native.wave([native.certified(changed), native.certified(original.returns[1])])
    # Both are individually canonical; their retained event union is not.
    native.validate_wave(original)
    native.validate_wave(conflicting)
    return original, conflicting


class FieldFrontierTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.path = Path(self.tmp.name)
        self.archive = WitnessArchive(self.path)
        self.addCleanup(self.archive.close)
        self.frontier = FieldFrontier(self.archive)

    def admit(self, wave=None, **kwargs):
        options = {"window": (0, 1000), "coupling_id": "fixed", "pressure": Q(9), "current": Q(1)}
        options.update(kwargs)
        return self.frontier.admit(native.recurrent() if wave is None else wave, **options)

    def advance(self, summary, value=Q(1), **kwargs):
        attention = self.frontier.bind_attention(summary, value, source_sha256=SOURCE)
        options = {"window": summary.window, "coupling_id": summary.coupling_id, "scale": summary.scale}
        options.update(kwargs)
        return self.frontier.advance(summary, attention, **options)

    def test_exact_trajectory_matches_expanded_native_update_without_hot_history(self):
        wave = native.recurrent()
        state = self.admit(wave)
        expanded = replace(native.initial_state(wave, pressure=Q(9)), current=Q(1))
        expected = Q(1)
        reads = self.archive.metrics["archive_reads"]
        with patch.object(native, "validate_wave", side_effect=AssertionError("hot validation")), \
             patch.object(self.archive, "expose", side_effect=AssertionError("hot exposure")), \
             patch.object(self.archive, "_merge_bindings", side_effect=AssertionError("hot history walk")):
            for attention in (Q(1), Q(-2, 3), Q(0), Q(13, 7), Q(-5)):
                old_cut = expanded.committed
                expanded = native.update(expanded, attention, input_index=old_cut)
                state = self.advance(state, attention)
                expected += attention / 9
                self.assertEqual((state.current, state.committed), (expected, old_cut+1))
                self.assertEqual((state.current, state.committed), (expanded.current, expanded.committed))
        self.assertEqual(self.archive.metrics["archive_reads"], reads)
        self.assertEqual((state.closure_cycles, state.reclosure_beats, state.latest_return_cut), (3, 2, 22))
        self.assertEqual(self.frontier.expose(state), wave)

    def test_field_weight_is_metadata_and_active_state_has_no_inner_history(self):
        first = self.admit(native.recurrent("unit"), field_weight=1)
        second = self.admit(native.recurrent("mapped"), field_weight=Q(7, 3))
        self.assertEqual(self.advance(first).current, self.advance(second).current)
        names = {field.name for field in fields(FieldSummary)}
        self.assertFalse(names.intersection({"returns", "certificates", "inner_waves", "vertices", "cadence"}))
        def copied_scalars(value):
            if type(value) is tuple:
                return all(copied_scalars(item) for item in value)
            return type(value) in (int, str)
        self.assertTrue(all(copied_scalars(v) for v in self.frontier._active.values()))

    def test_monon_duon_generic_support_outer_archive_roundtrip(self):
        from simulations.collision_routing import _route_support
        waves = (native.recurrent("monon"),
                 _route_support(native.recurrent("source", n=2), "duon", 20)["wave"],
                 generic_support(), outer_wave())
        summaries = [self.admit(w) for w in waves]
        self.assertEqual([s.motif for s in summaries], ["Monon", "Duon", "3:1", "outer"])
        for summary, wave in zip(summaries, waves):
            self.assertEqual(self.frontier.expose(summary), wave)
        updated = self.advance(summaries[-1])
        self.assertEqual(self.frontier.expose(updated), waves[-1])
        self.archive.close()
        reopened = WitnessArchive(self.path)
        self.addCleanup(reopened.close)
        for summary, wave in zip(summaries, waves):
            self.assertEqual(reopened.expose(summary.provenance), wave)
        recovered = reopened.expose(updated.provenance)
        self.assertEqual(len(recovered.certificates[0].inner_waves), 2)
        self.assertEqual(recovered.returns[-1].indices[-1], updated.latest_return_cut)
        self.assertGreater(updated.committed, updated.latest_return_cut)

    def test_joint_conflict_stays_rejected_after_remove_and_reopen(self):
        original, conflicting = conflicting_waves()
        summary = self.admit(original)
        with self.assertRaisesRegex(FrontierError, "conflicting retained"):
            self.admit(conflicting)
        self.frontier.remove(summary)
        with self.assertRaisesRegex(FrontierError, "conflicting retained"):
            self.admit(conflicting)
        self.archive.close()
        reopened = WitnessArchive(self.path)
        self.addCleanup(reopened.close)
        fresh = FieldFrontier(reopened)
        with self.assertRaisesRegex(FrontierError, "conflicting retained"):
            fresh.admit(conflicting, window=(0, 1000), coupling_id="fixed")
        self.assertEqual(fresh.active_count, 0)

    def test_reopen_rebuilds_index_and_rejects_conflicting_files(self):
        original, conflicting = conflicting_waves()
        self.admit(original)
        self.archive._db.execute("DELETE FROM bindings")
        self.archive._write_validated(conflicting)  # Simulate external archive tampering.
        self.archive.close()
        with self.assertRaisesRegex(FrontierError, "conflicting retained"):
            WitnessArchive(self.path)

    def test_durable_write_failure_blocks_new_admission_until_reopen(self):
        original, conflicting = conflicting_waves()
        write = self.archive._write_validated
        def failed_after_rename(wave):
            write(wave)
            raise OSError("simulated fsync failure after rename")
        with patch.object(self.archive, "_write_validated", side_effect=failed_after_rename):
            with self.assertRaises(OSError):
                self.admit(original)
        self.assertEqual(self.frontier.active_count, 0)
        with self.assertRaisesRegex(FrontierError, "reopen"):
            self.admit(conflicting)
        self.archive.close()
        reopened = WitnessArchive(self.path)
        self.addCleanup(reopened.close)
        with self.assertRaisesRegex(FrontierError, "conflicting retained"):
            FieldFrontier(reopened).admit(conflicting, window=(0, 1000), coupling_id="fixed")

    def test_integer_reflection_flag_and_boolean_certificate_scale_rejected_cold(self):
        plain = native.recurrent()
        reflected = native.wave([native.certified(replace(t, reflected_comparison=1)) for t in plain.returns])
        certificates = tuple(replace(c, scale=False) for c in plain.certificates)
        registry = {c.certificate_digest: c for c in certificates}
        alias = native.wave([(t, c, registry) for t, c in zip(plain.returns, certificates)])
        for wave in (reflected, alias):
            with self.subTest(wave=wave.identity), self.assertRaisesRegex(FrontierError, "exact native field"):
                self.admit(wave)
        self.assertEqual(list(self.path.glob("*.json")), [])

    def test_one_closure_failed_closure_and_unbound_names_cannot_be_compacted(self):
        wave = native.recurrent()
        failures = (replace(wave, returns=wave.returns[:1], certificates=wave.certificates[:1]),
                    replace(wave, motif="Tetron"), replace(wave, motif="Trion"),
                    replace(wave, certificates=(replace(wave.certificates[0], status="pending"),)
                            + wave.certificates[1:]))
        for failed in failures:
            with self.subTest(motif=failed.motif), self.assertRaises(ValueError):
                self.admit(failed)
        self.assertEqual(self.frontier.active_count, 0)

    def test_changed_scope_window_coupling_scale_and_pending_require_exposure(self):
        state = self.admit()
        for change in ({"window": (0, 1001)}, {"coupling_id": "changed"}, {"scale": 1}):
            with self.subTest(change=change), self.assertRaises(ExposureRequired):
                self.advance(state, **change)
        with self.assertRaises(FrontierError):
            self.advance(replace(state, scope="new"))
        pending = self.admit(native.recurrent("pending"), pending_obligations=("unresolved-route",))
        with self.assertRaises(ExposureRequired):
            self.advance(pending)
        expiring = self.admit(native.recurrent("expiry"), window=(0, 23))
        with self.assertRaises(ExposureRequired):
            self.advance(expiring)
        self.assertEqual(self.frontier.expose(state), native.recurrent())

    def test_summary_and_attention_forgery_staleness_are_rejected(self):
        state = self.admit()
        for changes in ({"current": Q(2)}, {"pressure": Q(3)}, {"committed": 23},
                        {"scale": False}, {"closure_cycles": 4}, {"field_weight": Q(2)},
                        {"provenance": WitnessRef("f"*64, state.provenance.producer_sha256)}):
            with self.subTest(changes=changes), self.assertRaises(FrontierError):
                self.advance(replace(state, **changes))
        attention = self.frontier.bind_attention(state, Q(1), source_sha256=SOURCE)
        for changes in ({"value": Q(2)}, {"source_sha256": "f"*64}, {"scale": False}, {"generation": 1}):
            with self.subTest(changes=changes), self.assertRaises(FrontierError):
                self.frontier.advance(state, replace(attention, **changes), window=state.window,
                                      coupling_id=state.coupling_id, scale=state.scale)
        successor = self.frontier.advance(state, attention, window=state.window,
                                         coupling_id=state.coupling_id, scale=state.scale)
        with self.assertRaises(FrontierError):
            self.advance(state)
        with self.assertRaises(FrontierError):
            self.frontier.advance(successor, attention, window=state.window,
                                  coupling_id=state.coupling_id, scale=state.scale)

    def test_rebind_exposes_without_rewinding_response_or_recertifying_closure(self):
        state = self.advance(self.advance(self.admit(), Q(2)), Q(-1, 2))
        attention = self.frontier.bind_attention(state, Q(1), source_sha256=SOURCE)
        reads = self.archive.metrics["archive_reads"]
        rebound = self.frontier.rebind(state, window=(24, 60), coupling_id="recoupled")
        self.assertEqual(self.archive.metrics["archive_reads"], reads+1)
        self.assertEqual((rebound.committed, rebound.current, rebound.pressure), (24, Q(7, 6), Q(9)))
        self.assertEqual((rebound.closure_cycles, rebound.reclosure_beats, rebound.latest_return_cut,
                          rebound.latest_cadence, rebound.scale, rebound.field_weight, rebound.provenance),
                         (state.closure_cycles, state.reclosure_beats, state.latest_return_cut,
                          state.latest_cadence, state.scale, state.field_weight, state.provenance))
        self.assertEqual(rebound.generation, state.generation+1)
        self.assertEqual((rebound.window, rebound.coupling_id), ((24, 60), "recoupled"))
        with self.assertRaises(FrontierError):
            self.advance(state)
        with self.assertRaises(FrontierError):
            self.frontier.advance(rebound, attention, window=rebound.window,
                                  coupling_id=rebound.coupling_id, scale=rebound.scale)
        successor = self.advance(rebound, Q(3))
        self.assertEqual((successor.current, successor.committed), (Q(3, 2), 25))
        self.assertEqual(successor.latest_return_cut, 22)

    def test_rebind_failed_exposure_and_excluded_cut_do_not_commit_or_clear_pending(self):
        state = self.admit(pending_obligations=("unresolved-route",))
        with self.assertRaisesRegex(FrontierError, "current committed cut"):
            self.frontier.rebind(state, window=(23, 100), coupling_id="new")
        with patch.object(self.archive, "expose", side_effect=OSError("unavailable witness")):
            with self.assertRaises(OSError):
                self.frontier.rebind(state, window=(0, 200), coupling_id="new")
        rebound = self.frontier.rebind(state, window=(0, 200), coupling_id="new")
        self.assertEqual(rebound.pending_obligations, ("unresolved-route",))
        self.assertEqual((rebound.current, rebound.committed), (state.current, state.committed))
        with self.assertRaises(ExposureRequired):
            self.advance(rebound)

    def test_mutable_fraction_and_reference_aliases_do_not_mutate_admission(self):
        for name in ("pressure", "current", "field_weight"):
            state = self.admit(native.recurrent("alias:" + name))
            value = getattr(state, name)
            object.__setattr__(value, "_numerator", value.numerator+1)
            with self.subTest(name=name), self.assertRaises(FrontierError):
                self.advance(state)
        state = self.admit(native.recurrent("alias:reference"))
        object.__setattr__(state.provenance, "sha256", "f"*64)
        with self.assertRaises(FrontierError):
            self.advance(state)
        state = self.admit(native.recurrent("alias:attention"))
        attention = self.frontier.bind_attention(state, Q(1), source_sha256=SOURCE)
        object.__setattr__(attention.value, "_numerator", 2)
        with self.assertRaises(FrontierError):
            self.frontier.advance(state, attention, window=state.window,
                                  coupling_id=state.coupling_id, scale=state.scale)

    def test_archive_hash_and_canonical_validation_are_both_required(self):
        summary = self.admit()
        path = self.archive._path(summary.provenance)
        raw = path.read_bytes()
        path.write_bytes(raw + b" ")
        with self.assertRaisesRegex(FrontierError, "hash mismatch"):
            self.frontier.expose(summary)
        path.write_bytes(raw)
        body = json.loads(raw)
        body["wave"]["certificates"][0]["status"] = "pending"
        altered = json.dumps(body, sort_keys=True, separators=(",", ":")).encode()
        digest = hashlib.sha256(altered).hexdigest()
        (self.path / (digest + ".json")).write_bytes(altered)
        with self.assertRaises(ValueError):
            self.archive.expose(WitnessRef(digest, self.archive.producer_sha256))

    def test_budgets_and_exact_types_fail_without_committing_response(self):
        self.frontier = FieldFrontier(self.archive, max_fields=1, max_scalar_bits=3)
        state = self.admit(pressure=1, current=7)
        with self.assertRaisesRegex(FrontierError, "scalar budget"):
            self.advance(state, 1)
        successor = self.advance(state, -1)
        self.assertEqual(successor.current, 6)
        with self.assertRaisesRegex(FrontierError, "field cap"):
            self.admit(native.recurrent("extra"), pressure=1)
        for invalid in (True, 1.0, Q(1, 16)):
            with self.subTest(invalid=invalid), self.assertRaises(FrontierError):
                self.advance(successor, invalid)
        self.frontier.remove(successor)
        self.assertEqual((self.frontier.active_count, len(self.frontier._inputs)), (0, 0))


class IndexedFieldQueueTests(unittest.TestCase):
    def test_order_replace_cancel_remove_and_single_slot(self):
        queue = IndexedFieldQueue(max_fields=3)
        for name in ("a", "b", "c"):
            queue.register(name)
        queue.schedule("b", 2)
        queue.schedule("a", 2)
        for cut in range(1000):
            queue.schedule("c", cut)
            self.assertEqual(len(queue), 3)
        queue.schedule("c", 1)
        self.assertEqual((queue.pop().identity, queue.pop().identity), ("c", "a"))
        self.assertTrue(queue.cancel("b"))
        self.assertFalse(queue.cancel("b"))
        self.assertIsNone(queue.pop())
        queue.schedule("a", 5)
        queue.remove("a")
        self.assertEqual((len(queue), queue.active_count, queue.metrics["peak_pending"]), (0, 2, 3))
        with self.assertRaises(FrontierError):
            queue.schedule("a", 6)
        with self.assertRaises(FrontierError):
            queue.schedule("b", True)

    def test_randomized_indexed_heap_matches_dict_oracle_and_log_bound(self):
        rng = random.Random(10407)
        count, steps = 32, 3000
        names = ["field:" + str(i) for i in range(count)]
        queue, expected = IndexedFieldQueue(max_fields=count), {}
        for name in names:
            queue.register(name)
        for _ in range(steps):
            name, action = rng.choice(names), rng.randrange(4)
            if action <= 1:
                cut = rng.randrange(10000)
                queue.schedule(name, cut)
                expected[name] = cut
            elif action == 2:
                self.assertEqual(queue.cancel(name), name in expected)
                expected.pop(name, None)
            else:
                event = queue.pop()
                if expected:
                    cut, identity = min((cut, identity) for identity, cut in expected.items())
                    self.assertEqual((event.cut, event.identity), (cut, identity))
                    del expected[identity]
                else:
                    self.assertIsNone(event)
            self.assertEqual(len(queue), len(expected))
            self.assertEqual(len(queue._positions), len(expected))
            self.assertEqual({identity: cut for cut, identity in queue._heap}, expected)
        self.assertLessEqual(queue.metrics["peak_pending"], count)
        self.assertLessEqual(queue.metrics["comparisons"], steps * (4*math.ceil(math.log2(count))+4))


if __name__ == "__main__":
    unittest.main()
