"""Passive native particle capture preserves actual formation witnesses."""
import gzip
import json
from pathlib import Path
import tempfile
import unittest

from simulations import proton_accounting as proton


class NativeParticleCaptureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.root = Path(cls.tmp.name)
        cls.plain = proton.run(cls.root / "plain/proton.json")
        cls.events = []
        with proton.observe_commits(lambda kind, payload: cls.events.append((kind, payload))) as observed:
            cls.traced = proton.run(cls.root / "traced/proton.json")
        cls.observed = observed
        cls.witness = json.loads(gzip.decompress((cls.root / "traced/proton-witnesses.json.gz").read_bytes()))

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def test_capture_preserves_model_outputs_and_witness_bytes(self):
        strip = lambda value: {k: v for k, v in value.items() if k != "output"}
        self.assertEqual(strip(self.plain), strip(self.traced))
        for name in ("proton.json", "proton-witnesses.json.gz"):
            self.assertEqual((self.root / "plain" / name).read_bytes(),
                             (self.root / "traced" / name).read_bytes())
        self.assertEqual(self.observed.failure_count, 0)

    def test_each_snapshot_contains_the_executed_formation_not_an_output_target(self):
        snapshots = [payload for kind, payload in self.events if kind == "particle.native_return"]
        families = {
            "source-encounter": self.witness["encounter_world"]["waves"],
            "proton-carrier": self.witness["carrier_waves"],
            "routed-support": [row["route_support"]["wave"] for row in self.witness["route_witnesses"]],
        }
        expected = {(family, wave["identity"], ordinal): (wave, ordinal)
                    for family, waves in families.items() for wave in waves
                    for ordinal in range(len(wave["returns"]))}
        observed = {(row["family"], row["wave"]["identity"], row["return_ordinal"]): row
                    for row in snapshots}
        self.assertEqual(len(snapshots), 32)
        self.assertEqual(len(observed), len(snapshots))
        self.assertEqual(set(observed), set(expected))
        for key, (wave, ordinal) in expected.items():
            with self.subTest(key=key):
                row = observed[key]
                self.assertEqual(row["schema"], "native-particle-return-v1")
                self.assertEqual(row["capture"], "validated-retained-snapshot")
                self.assertEqual(row["trace"], wave["returns"][ordinal])
                self.assertEqual(row["certificate"], wave["certificates"][ordinal])
                self.assertEqual(row["motif_witness"], wave["motif_witnesses"][ordinal])
                self.assertEqual(set(row["formation"]),
                                 {"construction", "component_traces", "component_certificates"})
                for field, value in row["formation"].items():
                    self.assertEqual(value, row["motif_witness"][field])
                self.assertEqual(row["wave"]["scope"], row["trace"]["scope"])
                self.assertEqual(row["wave"]["lineage"], row["trace"]["lineage"])
                self.assertNotIn("detected_signature", row)
                self.assertNotIn("expected_signature", row)

    def test_contacts_fold_and_shedding_preserve_native_operands(self):
        grouped = {}
        for kind, payload in self.events:
            grouped.setdefault(kind, []).append(payload)
        self.assertEqual(len(grouped["particle.native_contacts"]), 1)
        self.assertEqual(grouped["particle.native_contacts"][0]["contacts"], self.witness["contacts"])
        self.assertEqual(grouped["particle.native_contacts"][0]["reflection_evidence"],
                         self.witness["reflection_occurrences"])
        self.assertEqual(len(grouped["particle.native_fold"]), 1)
        self.assertEqual(grouped["particle.native_fold"][0]["fold"], self.witness["outer_fold"])
        shedding = grouped["particle.native_shedding"]
        report = json.loads((self.root / "traced/proton.json").read_text())
        self.assertEqual(len(shedding), len(report["rows"]))
        for captured, actual in zip(shedding, report["rows"]):
            self.assertEqual(captured["record"], actual["native_shedding"])
            self.assertEqual(captured["participation_window"], actual["window"])
            self.assertEqual(captured["occurrence_semantics"], "non-additive alternative views")
        states = grouped["particle.native_state"]
        self.assertEqual(len(states), 2)
        self.assertEqual(states[0]["operation"], "initial_with_terminal_orientation")
        self.assertEqual(states[0]["state"], shedding[0]["source_state"])
        self.assertEqual(states[1]["operation"], "seat_duon")
        self.assertEqual(states[1]["state"], self.witness["route_witnesses"][0]["receipt"]["seated_state"])
        self.assertEqual(states[1]["before"]["current"], states[1]["state"]["current"])


class NativeObserverIsolationTests(unittest.TestCase):
    def test_callback_cannot_mutate_the_model_input(self):
        original = {"rows": [{"count": 2}], "ratio": proton.Q(1, 3)}
        def mutate(kind, snapshot):
            snapshot["rows"][0]["count"] = 999
            snapshot["ratio"] = "wrong"
        with proton.observe_commits(mutate) as observed:
            proton._committed("test", original)
        self.assertEqual(original, {"rows": [{"count": 2}], "ratio": proton.Q(1, 3)})
        self.assertEqual(observed.failure_count, 0)

    def test_observer_error_is_separate_and_stops_capture(self):
        called = []
        def broken(kind, snapshot):
            called.append(kind)
            raise OSError("trace full")
        with proton.observe_commits(broken) as observed:
            proton._committed("first", {"value": 1})
            proton._committed("second", {"value": 2})
            with self.assertRaisesRegex(ValueError, "original model failure"):
                proton.require(False, "original model failure")
        self.assertEqual(called, ["first"])
        self.assertEqual(observed.failure_count, 1)
        self.assertEqual(observed.last_error, "OSError: trace full")

    def test_contexts_restore_and_none_disables_capture(self):
        outer, inner = [], []
        with proton.observe_commits(lambda kind, value: outer.append(kind)):
            proton._committed("outer-1", {})
            with proton.observe_commits(lambda kind, value: inner.append(kind)):
                proton._committed("inner", {})
            with proton.observe_commits(None) as disabled:
                proton._committed("disabled", {})
            proton._committed("outer-2", {})
        proton._committed("outside", {})
        self.assertEqual(outer, ["outer-1", "outer-2"])
        self.assertEqual(inner, ["inner"])
        self.assertEqual(disabled.failure_count, 0)

    def test_noncallable_observer_is_rejected(self):
        with self.assertRaises(TypeError):
            with proton.observe_commits(5):
                pass


if __name__ == "__main__":
    unittest.main()
