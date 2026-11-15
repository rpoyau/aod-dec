"""Small end-to-end checks of reduced execution, serialization and replay."""

import csv
from fractions import Fraction as Q
import gzip
import hashlib
import json
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from scripts import simulate_sparc_reduced as driver


HEADER = ("# Distance = 1 Mpc\n"
          "# Rad Vobs errV Vgas Vdisk Vbul SBdisk SBbul\n"
          "# kpc km/s km/s km/s km/s km/s L/pc^2 L/pc^2\n")


class SparcReducedDriverTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = TemporaryDirectory()
        cls.addClassCleanup(cls.temporary.cleanup)
        cls.directory = Path(cls.temporary.name)
        cls.inputs = cls.directory / "inputs"
        cls.inputs.mkdir()
        (cls.inputs / "Alpha_rotmod.dat").write_text(
            HEADER + "1 3 1 1 0 0 0 0\n2 6 1 2 0 0 0 0\n")
        (cls.inputs / "Beta_rotmod.dat").write_text(
            HEADER + "1 3 1 1 0 0 0 0\n3 9 1 3 0 0 0 0\n")
        cls.result = driver.execute(cls.inputs, workers=1)
        cls.outputs = cls.directory / "outputs"
        driver.write_outputs(cls.result, cls.outputs)

    def cli(self, output, replay):
        return subprocess.run(
            [sys.executable, *(["-O"] if sys.flags.optimize else []),
             str(driver.ROOT / "scripts/simulate_sparc_reduced.py"),
             "--input", str(self.inputs), "--output", str(output),
             "--workers", "1", "--replay", str(replay)],
            cwd=driver.ROOT, text=True, capture_output=True, timeout=60,
        )

    def test_execute_keeps_all_bins_and_reports_independent_exact_worsening(self):
        result = self.result
        self.assertEqual(result["baseline"]["bins"], 4)
        self.assertEqual(result["baseline"]["primary_mape_percent"], Q(0))
        expected = {("Alpha", Q(1)): Q(21), ("Alpha", Q(2)): Q(24),
                    ("Beta", Q(1)): Q(41), ("Beta", Q(3)): Q(49)}
        for case in ("spacing", "unit"):
            comparison = result["comparisons"][case]
            actual = {(row["key"]["galaxy"], row["key"]["radius_kpc"]): row["u_projected"]
                      for row in comparison["rows"]}
            self.assertEqual(actual, expected)
            self.assertEqual(comparison["primary_mape_percent"], Q(11375, 81))
            self.assertEqual(result["summaries"][case]["change_from_g0_percentage_points"], Q(11375, 81))
            self.assertEqual(comparison["bins"], 4)
            self.assertEqual(result["summaries"][case]["global_stock_residual"], 0)
            self.assertEqual(result["summaries"][case]["boundary_balance_residual"], 0)
        self.assertEqual(result["comparisons"]["disabled"]["primary_mape_percent"], 0)
        for source in result["source_files"]:
            self.assertEqual(source["rows"], 2)
            self.assertEqual(source["sha256"], hashlib.sha256((self.inputs / source["name"]).read_bytes()).hexdigest())

    def test_saved_trace_reopens_with_exact_states_native_returns_and_bound_hashes(self):
        compressed = (self.outputs / "trace.json.gz").read_bytes()
        raw = gzip.decompress(compressed)
        reopened = json.loads(raw)
        summary = json.loads((self.outputs / "summary.json").read_text())
        self.assertEqual(reopened, driver.encode(self.result))
        self.assertEqual(summary["trace_compressed_sha256"], hashlib.sha256(compressed).hexdigest())
        self.assertEqual(summary["trace_uncompressed_sha256"], hashlib.sha256(raw).hexdigest())
        self.assertEqual(summary["trace_uncompressed_bytes"], len(raw))
        self.assertEqual(Q(summary["summaries"]["spacing"]["mape_percent"]), Q(11375, 81))
        self.assertEqual(Q(reopened["runs"]["spacing"][0]["transitions"][0]["after_total_stock"]), Q(45, 2))
        for run, expected_theta in zip(reopened["runs"]["spacing"], (Q(5, 8), Q(5, 9))):
            self.assertEqual(tuple(map(Q, run["transitions"][0]["after_theta"])), (expected_theta, expected_theta))
            self.assertEqual(run["native_history"]["committed_cut"], 14)
            self.assertFalse(run["native_history"]["pending"])
            for wave in run["native_history"]["history"]["waves"]:
                self.assertEqual(len(wave["returns"]), 4)
                self.assertEqual(wave["returns"][-1]["indices"][-1], 14)
            for transition in run["transitions"]:
                for boundary in transition["boundaries"].values():
                    self.assertEqual(Q(boundary["stock_change"]) + Q(boundary["current"]), 0)

    def test_output_csv_and_report_use_the_actual_small_cohort(self):
        with (self.outputs / "comparison.csv").open(newline="") as stream:
            rows = list(csv.DictReader(stream))
        self.assertEqual(len(rows), 12)
        for case in ("spacing", "disabled", "unit"):
            selected = [row for row in rows if row["case"] == case]
            self.assertEqual(len(selected), 4)
            self.assertEqual(len({(r["galaxy"], r["radius_kpc"]) for r in selected}), 4)
            for row in selected:
                observed, projected = Q(row["u_observed"]), Q(row["u_projected"])
                residual = projected - observed
                self.assertEqual(Q(row["signed_residual"]), residual)
                self.assertEqual(Q(row["signed_relative_residual"]), residual / observed)
                self.assertEqual(Q(row["absolute_percentage_error"]), 100 * abs(residual) / observed)
        report = (self.outputs / "REPORT.md").read_text()
        self.assertIn("4 bins", report)
        self.assertNotIn("209 bins", report)

    def test_serialization_and_digest_cache_are_deterministic(self):
        repeated = self.directory / "written_again"
        driver.write_outputs(self.result, repeated)
        for name in ("trace.json.gz", "summary.json", "comparison.csv", "REPORT.md"):
            self.assertEqual((repeated / name).read_bytes(), (self.outputs / name).read_bytes())
        uncached = driver.execute(self.inputs, workers=1, use_cache=False)
        self.assertEqual(driver.packed(uncached), driver.packed(self.result))

    def test_worker_count_preserves_complete_exact_result(self):
        try:
            parallel = driver.execute(self.inputs, workers=2)
        except PermissionError as exc:
            self.skipTest(f"process workers unavailable in this environment: {exc}")
        self.assertEqual(driver.packed(parallel), driver.packed(self.result))

    def test_implementation_change_during_execution_is_rejected(self):
        before = driver.source_bindings()
        changed = dict(before)
        changed[driver.SOURCE_PATHS[0]] = "0" * 64
        with patch.object(driver, "source_bindings", side_effect=(before, changed)) as bindings:
            with self.assertRaisesRegex(RuntimeError, "implementation changed during execution"):
                driver.execute(self.inputs, workers=1)
        self.assertEqual(bindings.call_count, 2)

    def test_cli_replays_reopened_trace_and_rejects_valid_but_changed_json(self):
        output = self.directory / "cli_replay"
        completed = self.cli(output, self.outputs / "trace.json.gz")
        self.assertEqual(completed.returncode, 0, completed.stderr)
        execution = json.loads((output / "execution.json").read_text())
        self.assertEqual(execution["replay"], "PASS")
        self.assertEqual((output / "trace.json.gz").read_bytes(), (self.outputs / "trace.json.gz").read_bytes())
        altered = json.loads(gzip.decompress((self.outputs / "trace.json.gz").read_bytes()))
        altered["runs"]["spacing"][0]["transitions"][0]["after_theta"][0] = "7/8"
        tampered = self.directory / "tampered.json.gz"
        tampered.write_bytes(gzip.compress(json.dumps(altered).encode(), mtime=0))
        refused_output = self.directory / "refused_replay"
        rejected = self.cli(refused_output, tampered)
        self.assertNotEqual(rejected.returncode, 0)
        self.assertIn("reopened trace differs", rejected.stderr)
        self.assertFalse(refused_output.exists())

    def test_invalid_worker_counts_are_rejected_before_execution(self):
        for count in (0, -1, True, 1.0, "2"):
            with self.subTest(workers=count):
                with self.assertRaisesRegex(ValueError, "positive exact worker count"):
                    driver.execute(self.inputs, workers=count)


if __name__ == "__main__":
    unittest.main()
