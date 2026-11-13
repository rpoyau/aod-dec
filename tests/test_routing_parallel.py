"""Coverage and failure semantics of the parallel routing study controller."""
from contextlib import ExitStack, redirect_stdout
from copy import deepcopy
from dataclasses import replace
from fractions import Fraction as F
import io
import json
import os
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from scripts import simulate_routing_parallel as study
from simulations import chiral_gpu
from simulations.chiral_batch import build_ticket_batch, compile_model_tables, run_scalar
from simulations.chiral_collision import CollisionConfig, simulate_path


class RoutingParallelStudyTests(unittest.TestCase):
    def test_all_256_prior_paths_retain_every_ticket_and_transition(self):
        config = CollisionConfig(contact_gain=F(0))
        batch = build_ticket_batch(config, range(256), ticket_denominator=64)
        scalar = run_scalar(compile_model_tables(config, ticket_denominator=64), batch)
        report = study.retained_comparison(batch, scalar)
        self.assertEqual(report["status"], "PASS")
        self.assertEqual(report["paths_checked"], 256)
        self.assertEqual(report["transitions_checked"], 2048)

    def test_truncated_backend_trace_cannot_claim_full_reference_coverage(self):
        config = CollisionConfig(contact_gain=F(0))
        batch = build_ticket_batch(config, (7,), ticket_denominator=64)
        scalar = run_scalar(compile_model_tables(config, ticket_denominator=64), batch)
        truncated = {**scalar, "rows": (scalar["rows"][0][:-1],)}
        with self.assertRaises(ValueError):
            study.retained_comparison(batch, truncated)

    def test_new_seed_source_history_matches_candidate_beyond_archived_cohort(self):
        config = CollisionConfig(contact_gain=F(0))
        batch = build_ticket_batch(config, (300,), ticket_denominator=64)
        scalar = run_scalar(compile_model_tables(config, ticket_denominator=64), batch)
        source = simulate_path(config, seed=300, ticket_denominator=64)
        trace = {"seed": 300, "fixture": {"source_path": source}}
        report = study.trace_candidate_comparison(batch, scalar, [trace])
        self.assertEqual(report["status"], "PASS")
        self.assertEqual(report["paths_checked"], 1)
        self.assertEqual(report["transitions_checked"], 8)

    def test_fresh_source_join_rejects_changed_seed_sign_and_ticket(self):
        config = CollisionConfig(contact_gain=F(0))
        batch = build_ticket_batch(config, (300,), ticket_denominator=64)
        scalar = run_scalar(compile_model_tables(config, ticket_denominator=64), batch)
        source = simulate_path(config, seed=300, ticket_denominator=64)
        baseline = {"seed": 300, "fixture": {"source_path": source}}
        for mutation in ("seed", "sign", "ticket"):
            altered = deepcopy(baseline)
            step = altered["fixture"]["source_path"]["steps"][0]
            if mutation == "seed":
                altered["seed"] = 301
            elif mutation == "sign":
                step["after"] = replace(step["after"], chi_a=-step["after"].chi_a)
            else:
                a, b = step["flip_tickets"]
                step["flip_tickets"] = ((a + 1) % 64, b)
            with self.subTest(mutation=mutation), self.assertRaises((ValueError, ArithmeticError)):
                study.trace_candidate_comparison(batch, scalar, [altered])

    def test_absent_gpu_is_never_dispatched_or_substituted(self):
        absent = {"status": "UNAVAILABLE", "backend": "cuda-gpu",
                  "reason": "test: no accessible GPU", "gpu_executed": False}
        with TemporaryDirectory() as folder, \
                patch.object(chiral_gpu, "compile_kernel", side_effect=chiral_gpu.GPUUnavailable(absent)), \
                patch.object(chiral_gpu, "probe_gpu", return_value=absent), \
                patch.object(chiral_gpu, "run_gpu") as execute:
            report = study.gpu_study(None, None, Path(folder), "auto")
        self.assertEqual(report["execution"]["status"], "UNAVAILABLE")
        self.assertFalse(report["execution"]["gpu_executed"])
        execute.assert_not_called()

    def test_full_witness_and_route_materialization_matches_spawned_workers(self):
        # trace_study compares independently materialized scalar/worker record
        # digests, including every retained return, preparation and route witness.
        report, traces = study.trace_study(3, 2, None)
        self.assertEqual(report["status"], "PASS")
        self.assertEqual([row["seed"] for row in traces], [0, 1, 2])
        self.assertTrue(report["participating_worker_pids"])
        self.assertNotIn(os.getpid(), report["participating_worker_pids"])
        for row in traces:
            for wavelength in row["wavelengths"].values():
                self.assertEqual(wavelength["status"], "NOT_INSTANTIATED")

    def _run_unavailable_gpu_mode(self, mode):
        absent = {"execution": {"status": "UNAVAILABLE", "backend": "cuda-gpu",
                  "reason": "test: no accessible GPU", "gpu_executed": False},
                  "compilation": {"status": "UNAVAILABLE"}}
        with TemporaryDirectory() as folder, ExitStack() as stack:
            stack.enter_context(patch.object(study.sys, "argv", ["study", "--output", folder,
                "--workers", "2", "--paths", "1", "--traced-paths", "1", "--gpu", mode]))
            stack.enter_context(patch.object(study, "source_hashes", return_value={"test-input": "bound"}))
            stack.enter_context(patch.object(study, "run_multicore",
                side_effect=lambda tables, batch, **kwargs: run_scalar(tables, batch)))
            stack.enter_context(patch.object(study, "retained_comparison",
                return_value={"status": "PASS", "paths_checked": 1, "transitions_checked": 8}))
            stack.enter_context(patch.object(study, "gpu_study", return_value=absent))
            stack.enter_context(patch.object(study, "trace_study",
                return_value=({"status": "PASS", "paths": 1, "summary": []}, [])))
            stack.enter_context(redirect_stdout(io.StringIO()))
            if mode == "required":
                with self.assertRaisesRegex(RuntimeError, "required GPU"):
                    study.main()
            else:
                study.main()
            return json.loads((Path(folder) / "results.json").read_text())

    def test_required_gpu_failure_is_persisted_as_failure(self):
        report = self._run_unavailable_gpu_mode("required")
        self.assertEqual(report["status"], "FAIL")
        self.assertEqual(report["gpu"]["execution"]["status"], "UNAVAILABLE")
        self.assertFalse(report["gpu"]["execution"]["gpu_executed"])
        self.assertIn("required GPU", report["error"])

    def test_auto_gpu_can_complete_without_claiming_gpu_execution(self):
        report = self._run_unavailable_gpu_mode("auto")
        self.assertEqual(report["status"], "PASS")
        self.assertEqual(report["gpu"]["execution"]["status"], "UNAVAILABLE")
        self.assertFalse(report["gpu"]["execution"]["gpu_executed"])


if __name__ == "__main__":
    unittest.main()
