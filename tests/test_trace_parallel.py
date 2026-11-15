"""Spawned full-boundary replay preserves exact joined occurrence evidence."""
from copy import deepcopy
from fractions import Fraction as Q
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from simulations import region_engine as region
from simulations import trace_parallel as parallel
from simulations.trace_applications import plain, run_regions
from simulations.trace_execution import boundary_views
from simulations.trace_detectors import replay_boundaries
from simulations.trace_store import TraceStore, run_manifest


ROOT = Path(__file__).resolve().parents[1]


def semantic(report):
    return {key: value for key, value in report.items() if key != "execution"}


def fixture(path, *, pending=False, complete=True, wrong_frame_parent=False):
    manifest = run_manifest("parallel-fixture", source="exact source fixture",
        kernel="existing-region-current", preparation="two cuts, three full boundary schedules")
    ledger = region.Ledger(("left", "right", "outside"), "current", 0, "fixture", "native-current")
    with TraceStore(path, manifest) as store:
        with region.observe_commits(store.core_recorder):
            for cut in (1, 2):
                channels = (
                    region.Channel("cross:"+str(cut), "physical:cross:"+str(cut), "left", "right", Q(3, 2), "1"*64),
                    region.Channel("shed:"+str(cut), "physical:shed:"+str(cut), "right", "outside", Q(-2, 3), "2"*64, "shedding"),
                    region.Channel("off:"+str(cut), "physical:off:"+str(cut), "left", "outside", Q(7), "3"*64, available=False))
                obligation = (region.PendingProjection("pending", "physical:pending", "left", "outside", Q(3), "closure-burden", "4"*64),) if pending and cut == 1 else ()
                sweep = (region.SweepSample("right", Q(5, 2), "current", 0, "5"*64),)
                ledger = region.commit(ledger, cut, 3, channels, obligation, sweep)
        for name, members in (("left", ("left",)), ("right", ("right",)), ("union", ("left", "right"))):
            selected = region.Region(name, members)
            frames = tuple(region.Frame(selected, selected, "stationary") for _ in ledger.commits)
            audit = region.audit_boundary(ledger, name, frames, scope="declared-current")
            for row, frame in zip(audit.rows, frames):
                source = store.get_by_key(json.dumps(["region", ledger.scope, row["cut"]]))
                if wrong_frame_parent:
                    source = store.get_by_key(json.dumps(["region", ledger.scope, 1]))
                store.append("detector.boundary_view", {"boundary": name, "row": plain(row),
                    "frame": plain(frame), "unit": ledger.unit, "coverage": audit.coverage_claim},
                    scope_id=ledger.scope, lineage_id="boundary-view", source_ref="fixture",
                    causal_parents=(source["event_id"],), scale_family="declared-current",
                    source_key=json.dumps([ledger.scope, "boundary", name, row["cut"]]))
        if not complete:
            store._failure(ValueError("deliberate partial fixture"))
    return ledger


class ParallelBoundaryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.path = Path(cls.tmp.name)/"regions"
        with TraceStore(cls.path, run_manifest("regions-reopened", source="existing examples",
                kernel="exact-current", preparation="maser and earth-water-moon")) as store:
            cls.original = run_regions(store)
        cls.scalar = boundary_views(cls.path)

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def test_real_spawned_workers_match_reopened_scalar_and_preserve_archive(self):
        before = hashlib.sha256((self.path/"trace.sqlite3").read_bytes()).hexdigest()
        report = boundary_views(self.path, workers=2, backend="multicore")
        self.assertEqual(report["coverage"], "complete", report["failures"])
        self.assertEqual(semantic(report), semantic(self.scalar))
        execution = report["execution"]
        self.assertEqual((execution["workers_used"], execution["start_method"]), (2, "spawn"))
        self.assertEqual(len(execution["worker_pids"]), 2)
        self.assertNotIn(os.getpid(), execution["worker_pids"])
        self.assertLessEqual(execution["peak_pending_jobs"], 2)
        self.assertEqual(execution["pool_width"], 2)
        self.assertEqual(hashlib.sha256((self.path/"trace.sqlite3").read_bytes()).hexdigest(), before)
        with TraceStore.open(self.path) as reopened:
            events = {event["event_id"]: event for event in reopened.iter_events()}
        for result in report["boundaries"].values():
            self.assertEqual(len(result["rows"]), len(result["evidence"]))
            for row, identity in zip(result["rows"], result["evidence"]):
                source = events[identity]["payload"]["row"]
                self.assertEqual((row["cut"], row["snapshot"]), (source["cut"], source["snapshot"]))
                self.assertEqual([view["identity"] for view in row["views"]], [item["identity"] for item in source["channels"]])

    def test_existing_moving_example_preserves_sweep_and_all_occurrences(self):
        self.assertEqual(self.scalar["coverage"], "complete", self.scalar["failures"])
        rows = [row for result in self.scalar["boundaries"].values() for row in result["rows"]]
        self.assertTrue(any(Q(row["sweep"]) != 0 for row in rows))
        self.assertTrue(any(view["kind"] == "shedding" for row in rows for view in row["views"]))
        self.assertTrue(any(not view["available"] for row in rows for view in row["views"]))
        for row in rows:
            self.assertEqual(Q(row["current"]), Q(row["cross"])-Q(row["sweep"])+Q(row["shedding"]))

    def test_one_job_reports_inline_actual_participation(self):
        scope, name = self.scalar["source_binding"]["jobs"][0]["key"]
        report = boundary_views(self.path, scope=scope, name=name, workers=2, backend="multicore")
        self.assertEqual(report["coverage"], "complete")
        metrics = report["execution"]
        self.assertEqual((metrics["pool_width"], metrics["workers_used"], metrics["start_method"]), (1, 1, "inline"))
        self.assertEqual(metrics["worker_pids"], [os.getpid()])

    def test_small_exact_partition_and_pending_burden_coverage(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/"pending"
            fixture(path, pending=True)
            full = boundary_views(path)
            self.assertEqual(full["coverage"], "partial")
            declared = boundary_views(path, workers=2, backend="multicore", coverage_scope="declared-current")
            self.assertEqual(declared["coverage"], "complete", declared["failures"])
            self.assertEqual(declared["coverage_scope"], "declared-current")
            boundaries = declared["boundaries"]
            for index in (0, 1):
                left, right, union = (boundaries["fixture/"+name]["rows"][index] for name in ("left", "right", "union"))
                self.assertEqual((Q(left["cross"]), Q(right["cross"]), Q(union["cross"])), (Q(3, 2), Q(-3, 2), Q()))
                self.assertEqual((Q(right["shedding"]), Q(union["shedding"])), (Q(-2, 3), Q(-2, 3)))
                self.assertEqual(left["unresolved_projection_ids"], ["pending"])
                self.assertEqual(union["unresolved_projection_ids"], ["pending"])
                for field in ("cross", "sweep", "shedding", "current"):
                    self.assertEqual(Q(left[field])+Q(right[field]), Q(union[field]))

    def test_declared_partial_archive_is_never_upgraded_by_workers(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/"partial"
            fixture(path, complete=False)
            report = boundary_views(path, workers=2)
            self.assertEqual((report["archive_coverage"], report["coverage"]), ("partial", "partial"))

    def test_tampered_trace_is_explicit_partial_with_no_verified_boundaries(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/"tampered"
            fixture(path)
            with sqlite3.connect(path/"trace.sqlite3") as db:
                db.execute("UPDATE chunks SET body_hash=? WHERE ordinal=0", ("f"*64,))
            report = boundary_views(path, workers=2)
            self.assertEqual((report["coverage"], report["boundaries"]), ("partial", {}))
            self.assertTrue(report["failures"])

    def test_valid_archive_with_wrong_frame_parent_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/"wrong-parent"
            fixture(path, wrong_frame_parent=True)
            with TraceStore.open(path) as store:
                self.assertEqual(store.verify()["coverage"], "complete")
            report = boundary_views(path, workers=2)
            self.assertEqual(report["coverage"], "partial")
            self.assertIn("original causal commit", report["failures"][0]["reason"])

    def test_worker_packet_row_corruption_is_not_accepted_by_matching_total(self):
        original = parallel._run_job
        def corrupt(job, binding, backend):
            packet = original(job, binding, backend)
            packet["result"]["rows"][0]["views"][0]["physical_id"] = "another-physical-occurrence"
            return packet
        with patch.object(parallel, "_run_job", side_effect=corrupt):
            report = boundary_views(self.path)
        self.assertEqual(report["coverage"], "partial")
        self.assertTrue(all(result["coverage"] == "partial" for result in report["boundaries"].values()))

    def test_auditor_cannot_substitute_an_independently_valid_boundary(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/"foreign-audit"
            ledger = fixture(path)
            selected = region.Region("left", ("left",))
            frames = tuple(region.Frame(selected, selected, "stationary") for _ in ledger.commits)
            foreign = region.Region("outside", ("outside",))
            other_frames = tuple(region.Frame(foreign, foreign, "stationary") for _ in ledger.commits)
            valid = region.audit_boundary(ledger, "foreign", other_frames, scope="full")
            self.assertTrue(region.validate_audit(valid))
            with TraceStore.open(path) as store:
                result = replay_boundaries(store.iter_events(), name="left", cells=selected.cells,
                    frames=frames, scope="fixture", auditor=lambda *args, **kwargs: valid)
            self.assertEqual((result["status"], result["coverage"], result["value"]), ("unresolved", "partial", None))
            self.assertIn("requested source/context", result["reason"])

    def test_partial_worker_packet_cannot_publish_unverified_numeric_claim(self):
        original = parallel._run_job
        def unverified(job, binding, backend):
            packet = original(job, binding, backend)
            packet["result"].update(coverage="partial", value="999", status="verified")
            return packet
        with patch.object(parallel, "_run_job", side_effect=unverified):
            report = boundary_views(self.path)
        self.assertEqual(report["coverage"], "partial")
        for row in report["boundaries"].values():
            self.assertEqual((row["status"], row["coverage"], row["value"]), ("unresolved", "partial", None))
            self.assertNotIn("rows", row)
            self.assertNotIn("total", row)

    def test_missing_duplicate_wrong_source_and_reordered_packets(self):
        original = parallel._packets
        def reordered(*args):
            yield from reversed(list(original(*args)))
        with patch.object(parallel, "_packets", side_effect=reordered):
            self.assertEqual(semantic(boundary_views(self.path)), semantic(self.scalar))
        def missing(*args):
            yield from list(original(*args))[1:]
        with patch.object(parallel, "_packets", side_effect=missing):
            self.assertEqual(boundary_views(self.path)["coverage"], "partial")
        def duplicate(*args):
            items = list(original(*args))
            yield from items
            yield items[0]
        with patch.object(parallel, "_packets", side_effect=duplicate):
            self.assertEqual(boundary_views(self.path)["coverage"], "partial")
        def wrong(*args):
            for job, packet, error in original(*args):
                packet["input_sha256"] = "f"*64
                yield job, packet, error
        with patch.object(parallel, "_packets", side_effect=wrong):
            self.assertEqual(boundary_views(self.path)["coverage"], "partial")

    def test_worker_failure_and_source_change_are_partial(self):
        with patch.object(parallel, "_run_job", side_effect=RuntimeError("worker stopped")):
            report = boundary_views(self.path)
        self.assertEqual(report["coverage"], "partial")
        self.assertEqual(set(report["boundaries"]), set(self.scalar["boundaries"]))
        original = parallel.source_bindings
        calls = 0
        def changed(backend):
            nonlocal calls
            calls += 1
            result = original(backend)
            if calls > 1:
                result["simulations/trace_parallel.py"] = "f"*64
            return result
        with patch.object(parallel, "source_bindings", side_effect=changed):
            self.assertEqual(boundary_views(self.path)["coverage"], "partial")

    def test_archive_change_after_snapshot_invalidates_completed_claims(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/"changed"
            fixture(path)
            original = parallel._run_job
            changed = False
            def advance_archive(job, binding, backend):
                nonlocal changed
                packet = original(job, binding, backend)
                if not changed:
                    with TraceStore(path, resume=True) as store:
                        store.append("extra", {}, scope_id="fixture", lineage_id="extra", source_ref="test")
                    changed = True
                return packet
            with patch.object(parallel, "_run_job", side_effect=advance_archive):
                report = boundary_views(path)
            self.assertEqual(report["coverage"], "partial")
            self.assertTrue(all(result["coverage"] == "partial" for result in report["boundaries"].values()))

    def test_cli_workers_reopens_real_archive(self):
        result = subprocess.run([sys.executable, str(ROOT/"scripts/trace_simulation.py"), "boundaries",
            str(self.path), "--workers", "2", "--backend", "multicore"], capture_output=True, text=True, check=True)
        report = json.loads(result.stdout)
        self.assertEqual(semantic(report), semantic(self.scalar))

    def test_solar_cli_runs_native_fixture_and_reopens_all_boundary_currents(self):
        from simulations import solar_boundary_fixture as solar
        from simulations.trace_applications import application_gaps
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/"solar"
            command = [sys.executable, str(ROOT/"scripts/trace_simulation.py"), "run",
                       "--example", "solar-boundaries", "--trace", str(path)]
            execution = subprocess.run(command, capture_output=True, text=True, check=True, timeout=90)
            result = json.loads(execution.stdout)
            self.assertEqual(result["trace"]["coverage"], "complete")
            self.assertEqual(result["native_support"]["coverage"], "complete")
            self.assertEqual(result["native_relations"]["coverage"], "complete")
            inspected = result["solar_trace"]
            self.assertEqual((inspected["status"], inspected["coverage"]), ("verified", "complete"))
            self.assertEqual(inspected["route"]["surplus"], {"$ratio": [3, 1]})
            self.assertEqual(inspected["declared_routed_current"], {"$ratio": [2, 1]})
            self.assertEqual(inspected["chiral_flips"], [
                {"committed": 14, "before": 1, "after": -1}, {"committed": 20, "before": -1, "after": 1}])
            declared = result["declared_boundaries"]
            self.assertEqual(declared["coverage"], "complete")
            self.assertEqual(result["full_boundaries"]["coverage"], "partial")
            for name, currents in result["outcome"]["summary"].items():
                rows = declared["boundaries"][solar.SCOPE+"/"+name]["rows"]
                self.assertEqual([Q(row["current"]) for row in rows], [Q(*value["$ratio"]) for value in currents])
            with TraceStore.open(path) as store:
                self.assertNotIn("base", store.manifest["source"])
                for module in (solar, solar.n):
                    relative = str(Path(module.__file__).relative_to(ROOT))
                    self.assertEqual(store.manifest["source"][relative], hashlib.sha256(Path(module.__file__).read_bytes()).hexdigest())
            replayed = boundary_views(path, workers=2, backend="multicore", coverage_scope="declared-current")
            self.assertEqual(replayed["boundaries"], declared["boundaries"])
        gaps = application_gaps()["sun_mercury"]
        self.assertEqual(gaps["status"], "native_boundary_fixture")
        self.assertFalse(gaps["physical_trajectory"])
        self.assertIn("solar-boundaries", gaps["cli"])

    def test_parameter_and_capture_budgets_are_explicit(self):
        for options in ({"workers": True}, {"workers": 0}, {"workers": 33}, {"backend": "automatic"},
                        {"backend": "multicore"}, {"backend": "gpu", "workers": 2}, {"coverage_scope": "physical-full"}):
            with self.subTest(options=options), self.assertRaises(ValueError):
                boundary_views(self.path, **options)
        report = parallel.boundary_views(self.path, max_events=1)
        self.assertEqual(report["coverage"], "partial")
        self.assertEqual(report["boundaries"], {})

    def test_gpu_unavailable_propagates_without_scalar_fallback(self):
        from simulations.chiral_gpu import GPUUnavailable
        unavailable = GPUUnavailable({"status": "UNAVAILABLE", "gpu_executed": False, "reason": "test device absent"})
        with patch.object(parallel, "_run_job", side_effect=unavailable), \
             patch.object(parallel, "source_bindings", return_value={}):
            with self.assertRaises(GPUUnavailable) as caught:
                boundary_views(self.path, backend="gpu")
        self.assertEqual(caught.exception.report["status"], "UNAVAILABLE")


if __name__ == "__main__":
    unittest.main()
