"""Reporting failure modes independent of the chiral transition implementation."""
from contextlib import redirect_stdout
import hashlib
from io import StringIO
import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from scripts import simulate_chiral_collision as study


class ChiralStudyTests(unittest.TestCase):
    def test_reused_output_does_not_certify_an_unrequested_stale_plot(self):
        # A preceding --plots run can leave this file in a reused destination.
        # Stubbing the independent computations isolates manifest provenance.
        with TemporaryDirectory() as temporary:
            output = Path(temporary)
            stale = output / "chiral-study.png"
            stale.write_bytes(b"plot from a previous experiment")
            with patch.object(study.sys, "argv", ["study", "--output", str(output), "--samples", "1"]), \
                    patch.object(study, "source_hashes", return_value={"fixture": "unchanged"}), \
                    patch.object(study, "exact_study", return_value={}), \
                    patch.object(study, "sampled_study", return_value=({}, [], {})), \
                    patch.object(study, "background_record", return_value={}), \
                    patch.object(study, "native_replay", return_value={"status": "PASS", "artifact_sha256": {}}), \
                    redirect_stdout(StringIO()):
                study.main()
            report = json.loads((output / "results.json").read_text())
            self.assertEqual(report["status"], "PASS")
            self.assertIn("cohort.json", report["artifacts"])
            self.assertIn("exemplars.json", report["artifacts"])
            self.assertNotIn("chiral-study.png", report["artifacts"])

    def test_evidence_binds_executed_local_compiler_and_runtime_sources(self):
        hashes = study.source_hashes()
        for relative in ("aodc.py", "runtime/dec_core.py", "runtime/dec_language.py",
                         "native/compiler.py", "native/runtime.cpp", "native/runtime.hpp",
                         "native/runner.cpp", "native/toolchain-lock.json"):
            with self.subTest(source=relative):
                expected = hashlib.sha256((study.ROOT / relative).read_bytes()).hexdigest()
                self.assertEqual(hashes.get(relative), expected)

    def test_native_inventory_binds_current_builds_and_excludes_stale_files(self):
        with TemporaryDirectory() as temporary:
            output = Path(temporary)
            (output / "dec").mkdir()
            (output / "dec/stale.build.json").write_text('{"old": true}')

            def fake_execute(argv, commands, expected=None):
                args = list(map(str, argv))
                if len(args) > 2 and args[2] == "compile":
                    Path(args[-1]).write_text("declared fixture IR")
                if len(args) > 2 and args[2] == "build":
                    binary = Path(args[-1])
                    level = int(args[args.index("-O") + 1])
                    binary.write_bytes(b"executed fixture binary " + str(level).encode())
                    binary.with_suffix(".build.json").write_text(json.dumps({
                        "binding": {"compiler": "fixture", "opt": level, "llvm_sha256": "bound-library"}}))
                return expected if expected is not None else "declared fixture output\n"

            with patch.object(study, "dec_source", return_value="declared fixture source"), \
                    patch.object(study, "validate_native_output", return_value={"status": "PASS"}), \
                    patch.object(study, "execute", side_effect=fake_execute):
                report = study.native_replay(output, {"fixture": {"config": None}}, True, [])
            expected_names = {"dec/fixture.dec", "dec/fixture.stdout", "dec/fixture.json",
                              "dec/fixture-O0", "dec/fixture-O2",
                              "dec/fixture-O0.build.json", "dec/fixture-O2.build.json"}
            self.assertEqual(set(report["artifact_sha256"]), expected_names)
            for relative, actual in report["artifact_sha256"].items():
                self.assertEqual(actual, hashlib.sha256((output / relative).read_bytes()).hexdigest())
            builds = report["programs"][0]["aot_builds"]
            self.assertEqual([b["binding"]["opt"] for b in builds], [0, 2])
            self.assertTrue(all(b["binding"]["llvm_sha256"] == "bound-library" for b in builds))


if __name__ == "__main__":
    unittest.main()
