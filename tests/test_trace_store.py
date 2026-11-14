"""Adversarial storage and actual-runtime passive trace integration checks."""
from contextlib import nullcontext
from dataclasses import replace
from fractions import Fraction
from hashlib import sha256
import json
import os
from pathlib import Path
import random
import sqlite3
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from runtime import dec_core as core
from simulations import region_engine as regions
from simulations.trace_store import (TraceStore, TraceLimits, TraceIntegrityError,
                                    TraceIncomplete, canonical_bytes, make_event,
                                    record_commits, run_manifest)


def manifest(name="fixture"):
    return run_manifest(name, source="runtime/dec_core.py:finite-hinge-return-v1",
                        kernel="declared deterministic hinge-return", preparation="start at q4(0)")


def event_args():
    return dict(scope_id="B", lineage_id="A", source_ref="test exact finite stream")


def one_return(lineage="A"):
    trace = core.new_trace("B", lineage, core.Q4(0))
    trace = core.append_event(trace, "step", core.Q4(1), 1)
    trace = core.append_event(trace, "hinge")
    trace = core.append_event(trace, "step", core.Q4(0), -1)
    return core.close_trace(trace)


class TraceIdentityTests(unittest.TestCase):
    def test_exact_json_and_ratio(self):
        self.assertEqual(json.loads(canonical_bytes({"n": 2**100, "r": Fraction(2, 6)})),
                         {"n": 2**100, "r": {"$ratio": [1, 3]}})
        for value in (1.0, float("nan"), {"$ratio": [2, 4]}, {"$ratio": [True, 1]}):
            with self.assertRaises(TraceIntegrityError):
                canonical_bytes(value)

    def test_exact_envelope_indices(self):
        for ordinal in (True, 0.0, -1):
            with self.assertRaises(TraceIntegrityError):
                make_event("r", ordinal, "raw", {}, **event_args())
        for scale in (True, 1.0):
            with self.assertRaises(TraceIntegrityError):
                make_event("r", 0, "raw", {}, scale_index=scale, **event_args())
        with self.assertRaises(TraceIntegrityError):
            make_event("r", 0, "raw", {}, fractal_coordinate=(True,), **event_args())

    def test_idempotent_duplicate_and_content_conflict(self):
        with TemporaryDirectory() as directory, TraceStore(directory, manifest()) as store:
            event = store.append("raw", {"value": 1}, source_key="A:1", **event_args())
            self.assertEqual(store.ingest(event), event)
            self.assertEqual(store.append("raw", {"value": 1}, source_key="A:1", **event_args()), event)
            self.assertEqual(store.status()["events"], 1)
            with self.assertRaises(TraceIntegrityError):
                store.append("raw", {"value": 2}, source_key="A:1", **event_args())
            self.assertEqual(store.status()["coverage"], "partial")

    def test_external_mutation_cannot_change_stored_event(self):
        with TemporaryDirectory() as directory, TraceStore(directory, manifest()) as store:
            payload = {"nested": [1, 2]}
            event = store.append("raw", payload, **event_args())
            identity = event["event_id"]
            payload["nested"][0] = 4
            event["payload"]["nested"][0] = 8
            self.assertEqual(store.get(identity)["payload"], {"nested": [1, 2]})
            with self.assertRaises(TraceIntegrityError):
                store.ingest(event)

    def test_missing_causal_parent_rejected(self):
        with TemporaryDirectory() as directory, TraceStore(directory, manifest()) as store:
            with self.assertRaises(TraceIntegrityError):
                store.append("raw", {}, causal_parents=("1" * 64,), **event_args())
            self.assertEqual(store.status()["events"], 0)

    def test_causal_order_and_same_named_local_event_in_other_lineage(self):
        with TemporaryDirectory() as directory, TraceStore(directory, manifest()) as store:
            first = store.append("raw", {}, source_key="A:0", **event_args())
            args = event_args(); args["lineage_id"] = "B"
            second = store.append("raw", {}, source_key="B:0", causal_parents=(first["event_id"],), **args)
            self.assertEqual(second["commit_ordinal"], 1)
            self.assertNotEqual(second["event_id"], first["event_id"])

    def test_missing_ordinal_rejected(self):
        with TemporaryDirectory() as directory, TraceStore(directory, manifest()) as store:
            event = make_event("fixture", 1, "raw", {}, **event_args())
            with self.assertRaises(TraceIntegrityError):
                store.ingest(event)


class DurableTraceTests(unittest.TestCase):
    def test_relative_path_read_only_reopen(self):
        with TemporaryDirectory(dir=Path.cwd()) as directory:
            relative = os.path.relpath(directory, Path.cwd())
            with TraceStore(relative, manifest()) as store:
                expected = store.append("raw", {"n": 1}, **event_args())
            with TraceStore.open(relative) as archive:
                self.assertEqual(list(archive.iter_events()), [expected])

    def test_restart_checkpoint_and_replay(self):
        with TemporaryDirectory() as directory:
            with TraceStore(directory, manifest()) as store:
                a = store.append("raw", {"v": Fraction(3, 7)}, **event_args())
                store.checkpoint({"next": 1}, {"algorithm": "fixture", "state": [3]})
            with TraceStore(directory, manifest(), resume=True) as store:
                self.assertEqual(store.load_checkpoint()["state"], {"next": 1})
                self.assertEqual(store.status()["coverage"], "partial")
                b = store.append("raw", {"v": Fraction(4, 7)}, causal_parents=(a["event_id"],), **event_args())
            with TraceStore.open(directory) as store:
                self.assertEqual(list(store.iter_events()), [a, b])
                self.assertEqual(store.verify()["coverage"], "complete")

    def test_restarted_and_uninterrupted_scientific_digest_equal(self):
        with TemporaryDirectory() as base:
            roots = []
            for mode in ("full", "restart"):
                directory = Path(base) / mode
                store = TraceStore(directory, manifest())
                for i in range(8):
                    if mode == "restart" and i == 4:
                        store.checkpoint({"next": i}); store.close()
                        store = TraceStore(directory, manifest(), resume=True)
                    store.append("raw", {"i": i}, **event_args())
                store.finish(); roots.append(store.status()["chain"]); store.close()
            self.assertEqual(roots[0], roots[1])

    def test_manifest_rebind_required(self):
        with TemporaryDirectory() as directory:
            with TraceStore(directory, manifest()):
                pass
            changed = manifest(); changed["kernel"] = "other law"
            with self.assertRaises(TraceIntegrityError):
                TraceStore(directory, changed, resume=True)

    def test_tampered_chunk_reported(self):
        with TemporaryDirectory() as directory:
            with TraceStore(directory, manifest()) as store:
                store.append("raw", {"n": 1}, **event_args())
            with sqlite3.connect(str(Path(directory) / "trace.sqlite3")) as db:
                db.execute("UPDATE chunks SET body=?", (b"{}",))
            db.close()
            with self.assertRaises(TraceIntegrityError):
                TraceStore.open(directory)

    def test_missing_tail_cannot_report_complete(self):
        with TemporaryDirectory() as directory:
            with TraceStore(directory, manifest()) as store:
                for i in range(3):
                    store.append("raw", {"n": i}, **event_args())
            with sqlite3.connect(str(Path(directory) / "trace.sqlite3")) as db:
                db.execute("DELETE FROM chunks WHERE ordinal=2")
            db.close()
            with self.assertRaises(TraceIntegrityError):
                TraceStore.open(directory)

    def test_source_key_index_tampering_rejected(self):
        with TemporaryDirectory() as directory:
            with TraceStore(directory, manifest()) as store:
                event = store.append("raw", {"n": 1}, source_key="original", **event_args())
                store.db.execute("UPDATE chunks SET source_key='misdirected'")
                store.db.commit()
                with self.assertRaises(TraceIntegrityError):
                    store.get_by_key("misdirected")
                with self.assertRaises(TraceIntegrityError):
                    list(store.iter_events())
                store._failure(TraceIntegrityError("hostile mutation"))

    def test_corrupt_archive_reported(self):
        with TemporaryDirectory() as directory:
            (Path(directory) / "trace.sqlite3").write_bytes(b"truncated database")
            with self.assertRaises((TraceIntegrityError, TraceIncomplete)):
                TraceStore.open(directory)

    def test_interrupted_transaction_rolls_back_no_phantom_event(self):
        with TemporaryDirectory() as directory:
            with TraceStore(directory, manifest()) as store:
                expected = store.append("raw", {"n": 0}, **event_args())
            db = sqlite3.connect(str(Path(directory) / "trace.sqlite3"))
            db.execute("BEGIN")
            db.execute("DELETE FROM chunks")
            db.close()  # uncommitted write is rolled back
            with TraceStore.open(directory) as store:
                self.assertEqual(list(store.iter_events()), [expected])

    def test_digest_is_not_retrievable_witness(self):
        with TemporaryDirectory() as directory, TraceStore(directory, manifest()) as store:
            with self.assertRaises(TraceIncomplete):
                store.get("9" * 64)

    def test_unfinished_writer_stays_partial(self):
        with TemporaryDirectory() as directory:
            store = TraceStore(directory, manifest())
            store.append("raw", {}, **event_args()); store.close()
            with TraceStore.open(directory) as store:
                self.assertEqual(store.verify()["coverage"], "partial")

    def test_disk_cap_stops_without_dropping_prefix(self):
        limits = TraceLimits(max_event_bytes=4096, max_storage_bytes=98304, cache_bytes=8192)
        with TemporaryDirectory() as directory:
            store = TraceStore(directory, manifest(), limits=limits)
            count = 0
            try:
                for i in range(1000):
                    store.append("raw", {"i": i, "witness": "x" * 1000}, **event_args())
                    count += 1
            except TraceIncomplete:
                pass
            else:
                self.fail("storage budget did not stop producer")
            self.assertGreater(count, 0)
            self.assertEqual(len(list(store.iter_events())), count)
            self.assertLessEqual(store.status()["archive_bytes"], limits.max_storage_bytes)
            self.assertEqual(store.status()["coverage"], "partial")
            store.close()
            with TraceStore.open(directory, limits=limits) as archive:
                self.assertEqual(archive.verify()["events"], count)
                self.assertEqual(archive.status()["coverage"], "partial")

    def test_bigint_and_event_allocation_are_bounded(self):
        limits = TraceLimits(max_event_bytes=2048, max_integer_bits=128)
        with TemporaryDirectory() as directory, TraceStore(directory, manifest(), limits=limits) as store:
            with self.assertRaises(TraceIncomplete):
                store.append("raw", {"huge": 2**1000}, **event_args())
            self.assertEqual(store.status()["events"], 0)
        with self.assertRaises(TraceIncomplete):
            canonical_bytes({"blob": "x" * 10000}, limits)

    def test_nested_container_and_checkpoint_budgets(self):
        limits = TraceLimits(max_event_bytes=2048, max_nodes=64, max_depth=8)
        nested = [0]
        for _ in range(10):
            nested = [nested]
        with self.assertRaises(TraceIncomplete):
            canonical_bytes(nested, limits)
        with self.assertRaises(TraceIncomplete):
            canonical_bytes([0] * 100, limits)
        with TemporaryDirectory() as directory, TraceStore(directory, manifest(), limits=limits) as store:
            with self.assertRaises(TraceIncomplete):
                store.checkpoint({"history": "x" * 10000})
            self.assertEqual(store.status()["coverage"], "partial")


class PassiveCommitTests(unittest.TestCase):
    def test_actual_core_raw_commit_stream(self):
        expected = one_return()
        with TemporaryDirectory() as directory:
            with TraceStore(directory, manifest()) as store:
                with record_commits(store) as observer:
                    actual = one_return()
                    for _ in range(3):
                        core.validate_trace(actual); core.close_trace(actual); core.phases(actual)
                self.assertEqual(observer.failure_count, 0)
                rows = list(store.iter_events())
                self.assertEqual([row["kind"] for row in rows],
                                 ["core.start", "core.transition", "core.transition", "core.transition", "core.closure"])
                self.assertEqual([row["payload"]["kind"] for row in rows if row["kind"] == "core.transition"],
                                 ["step", "hinge", "step"])
                self.assertEqual(actual, expected)
            with TraceStore.open(directory) as archive:
                self.assertEqual(list(archive.iter_events()), rows)

    def test_storage_failure_preserves_model_value_and_first_error(self):
        expected = one_return()
        with TemporaryDirectory() as directory, TraceStore(directory, manifest()) as store:
            with patch.object(store, "append", side_effect=OSError("disk full")):
                with record_commits(store) as observation:
                    actual = one_return()
                    with self.assertRaises(core.DecError) as caught:
                        core.append_event(actual, "step", core.Q4(3), 1)
            self.assertEqual(actual, expected)
            self.assertEqual(str(caught.exception), "atomic move must be Hamming-1")
            self.assertEqual(observation.failure_count, 1)
            self.assertEqual(store.status()["coverage"], "partial")

    def test_missing_left_memory_marks_incomplete(self):
        initial = core.new_trace("B", "A", core.Q4(0))
        with TemporaryDirectory() as directory, TraceStore(directory, manifest()) as store:
            with record_commits(store) as observation:
                core.append_event(initial, "step", core.Q4(1), 1)
            self.assertEqual(observation.failure_count, 1)
            self.assertEqual(store.status()["coverage"], "partial")

    def test_fusion_keeps_complete_inner_witnesses_after_reopen(self):
        left, right = one_return("A"), one_return("B")
        with TemporaryDirectory() as directory:
            with TraceStore(directory, manifest()) as store:
                with record_commits(store):
                    proposal = core.fold(left, right, "outer", "C")
                    outer = core.outer(proposal, core.Q4(0))
                    outer = core.append_event(outer, "step", core.Q4(1), 1)
                    outer = core.append_event(outer, "hinge")
                    outer = core.append_event(outer, "step", core.Q4(0), -1)
                    outer = core.close_trace(outer)
                    core.fuse(proposal, outer)
            with TraceStore.open(directory) as archive:
                start = next(archive.iter_events())
                self.assertEqual(len(start["payload"]["parents"]), 2)
                self.assertEqual(start["payload"]["parents"][0]["closures"][0]["digest"], left.closures[0].digest)
                self.assertEqual(start["payload"]["parents"][1]["events"][-1]["orientation"], -1)

    def test_region_hook_carries_only_committed_cut_and_pending(self):
        provenance = "a" * 64
        ledger = regions.Ledger(("in", "out"), "pressure-current", 0, "B", "declared")
        channel = regions.Channel("a", "physical-a", "in", "out", Fraction(2), provenance)
        pending = regions.PendingProjection("p", "physical-p", "in", "out", Fraction(3), "burden", provenance)
        with TemporaryDirectory() as directory, TraceStore(directory, manifest()) as store:
            with record_commits(store):
                new = regions.commit(ledger, 0, 4, (channel,), (pending,))
            rows = list(store.iter_events())
            self.assertEqual(len(rows), 1)
            self.assertEqual(rows[0]["kind"], "region.commit")
            self.assertEqual(rows[0]["payload"]["row"]["obligations"][0]["amount"], {"$ratio": [3, 1]})
            self.assertEqual(new.commits[0].channels[0].value, 2)

    def test_core_contacts_commit_causal_return_references_without_copying_history(self):
        with TemporaryDirectory() as directory, TraceStore(directory, manifest()) as store:
            with record_commits(store) as observation:
                left, right = one_return("left"), one_return("right")
                coupling = core.coupling(left, right, "native-event-span")
                actual = core.contact(coupling, 3, 3)
                core.validate_coupling(actual)
                self.assertEqual(core.contact(coupling, 3, 3), actual)  # idempotent replay at recorder
                with self.assertRaises(core.DecError):
                    core.contact(actual, 3, 3)
            self.assertEqual(observation.failure_count, 0)
            rows = [event for event in store.iter_events() if event["kind"] == "core.contact"]
            self.assertEqual(len(rows), 1)
            row = rows[0]
            self.assertEqual(row["payload"], {"left_lineage": "left", "right_lineage": "right",
                "window": "native-event-span", "left_return": 3, "right_return": 3,
                "left_closure_digest": left.closures[0].digest,
                "right_closure_digest": right.closures[0].digest, "event_span": 3})
            self.assertEqual([store.get(parent)["lineage_id"] for parent in row["causal_parents"]], ["left", "right"])
            self.assertTrue(all(store.get(parent)["payload"]["ordinal"] == 3 for parent in row["causal_parents"]))
            self.assertFalse(any(name in row["payload"] for name in ("events", "parents", "closures")))

    def test_core_contact_observer_failure_preserves_successor_and_first_error(self):
        left, right = one_return("left"), one_return("right")
        coupling = core.coupling(left, right, "native-event-span")
        expected = core.contact(coupling, 3, 3)
        with TemporaryDirectory() as directory, TraceStore(directory, manifest()) as store:
            with record_commits(store) as observation:
                actual = core.contact(coupling, 3, 3)
                with self.assertRaises(core.DecError) as caught:
                    core.contact(actual, 3, 3)
            self.assertEqual(expected, actual)
            self.assertEqual(str(caught.exception), "contacts must be distinct, ordered and unreplayed")
            self.assertEqual(observation.failure_count, 1)
            self.assertEqual(store.status()["coverage"], "partial")

    def test_two_distinct_contacts_survive_reopen_and_no_validation_replay(self):
        def second(trace):
            trace = core.append_event(trace, "step", core.Q4(1), 1)
            trace = core.append_event(trace, "hinge")
            trace = core.append_event(trace, "step", core.Q4(0), -1)
            return core.close_trace(trace)
        with TemporaryDirectory() as directory:
            with TraceStore(directory, manifest()) as store:
                with record_commits(store):
                    left, right = second(one_return("left")), second(one_return("right"))
                    coupling = core.coupling(left, right, "native-event-span")
                    coupling = core.contact(coupling, 3, 3)
                    coupling = core.contact(coupling, 6, 6)
                    self.assertEqual(core.cadence(coupling), 1)
                    self.assertEqual(core.lag(coupling), 0)
                    self.assertEqual(core.drift(coupling), 0)
            with TraceStore.open(directory) as archive:
                contacts = [row for row in archive.iter_events() if row["kind"] == "core.contact"]
                self.assertEqual([row["payload"]["left_return"] for row in contacts], [3, 6])
                self.assertEqual([row["payload"]["event_span"] for row in contacts], [6, 6])

    def test_stochastic_passive_values_rng_and_first_error(self):
        def run(store=None, hostile=False):
            rng = random.Random(741)
            values, first_error = [], None
            callback = (lambda kind, value: rng.random()) if hostile else (store.core_recorder if store else None)
            context = core.observe_commits(callback) if callback else nullcontext()
            with context:
                for index in range(6):
                    before = rng.getstate()
                    choice = rng.randrange(3)
                    values.append(choice)
                    if store is not None:
                        store.append("kernel.choice", {"weights": [Fraction(1, 3)] * 3,
                                     "choice": choice, "rng_before": before, "rng_after": rng.getstate()},
                                     **event_args())
                    one_return(str(index))
                try:
                    core.append_event(core.new_trace("B", "bad", core.Q4(0)), "step", core.Q4(3), 1)
                except core.DecError as exc:
                    first_error = str(exc)
            return values, rng.getstate(), first_error

        expected = run()
        with TemporaryDirectory() as directory, TraceStore(directory, run_manifest(
                "fixture", source="test_stochastic_passive_values_rng_and_first_error", kernel="uniform 3 choices",
                preparation="six draws seed 741", rng={"algorithm": "Python random.Random MT19937",
                                                       "seed": 741, "streams": ["model-owned"]})) as store:
            self.assertEqual(run(store), expected)
            choices = [e for e in store.iter_events() if e["kind"] == "kernel.choice"]
            self.assertEqual(len(choices), 6)
            self.assertEqual(choices[0]["payload"]["weights"], [{"$ratio": [1, 3]}] * 3)
            store.checkpoint({"draws": 6}, expected[1])
            self.assertEqual(store.load_checkpoint()["rng_state"], json.loads(canonical_bytes(expected[1])))
        self.assertNotEqual(run(hostile=True)[:2], expected[:2])


if __name__ == "__main__":
    unittest.main()
