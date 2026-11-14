"""Dion/pivot operands have exact integer domains at admission and decoding."""
from copy import deepcopy
from dataclasses import asdict, replace
from pathlib import Path
import tempfile
import unittest

from simulations.collision_routing import _route_support
from simulations.trace_store import TraceStore, run_manifest
from simulations.trace_support import inspect_native_return, detect_particle_support
from simulations.vendor import aod_manual_native as native


class IntegerAlias(int):
    pass


class DuonSignTypeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        wave = _route_support(native.recurrent("source", n=2), "sign-types", 20)["wave"]
        cls.motif = wave.motif_witnesses[0]
        cls.registry = {c.certificate_digest: c for c in
                        (*cls.motif.component_certificates, cls.motif.closure)}
        witness = asdict(cls.motif)
        cls.payload = {
            "schema": "native-particle-return-v1", "family": "duon-sign-controls",
            "trace": asdict(wave.returns[0]), "certificate": asdict(wave.certificates[0]),
            "formation": {key: witness[key] for key in
                          ("construction", "component_traces", "component_certificates")},
            "diagnostic_flag": True,
        }

    def construct(self, construction, *, chirality=1):
        return native.motif_witness("Duon", self.motif.support_trace, construction,
            tuple(zip(self.motif.component_traces, self.motif.component_certificates)),
            self.registry, self.motif.enclosure, chirality)

    def test_valid_integer_dion_patterns_and_ternary_pivots_admit(self):
        self.assertEqual(self.construct(self.motif.construction), self.motif)
        pivot_id = self.motif.construction[1][0]
        for left in ((1, -1), (-1, 1)):
            for right in ((1, -1), (-1, 1)):
                for state in (-1, 0, 1):
                    with self.subTest(left=left, right=right, state=state):
                        motif = self.construct((left, (pivot_id, state), right))
                        self.assertTrue(native.validate_motif(motif))
        # The producer already accepted JSON-style lists before this repair.
        # Preserve that container domain while rejecting aliases in its signs.
        construction = tuple(list(part) for part in self.motif.construction)
        self.assertTrue(native.validate_motif(self.construct(construction)))

    def test_boolean_float_and_subclass_dion_signs_reject_at_admission(self):
        for side in (0, 2):
            for position in (0, 1):
                for value in (True, 1.0, IntegerAlias(1)):
                    with self.subTest(side=side, position=position, value=repr(value),
                                      value_type=type(value).__name__):
                        construction = list(self.motif.construction)
                        pattern = [1, -1] if position == 0 else [-1, 1]
                        pattern[position] = value
                        construction[side] = tuple(pattern)
                        with self.assertRaisesRegex(native.AdmissionError, "exact integers"):
                            self.construct(tuple(construction))

    def test_boolean_float_and_subclass_pivot_signs_reject_at_admission(self):
        for value in (True, False, 1.0, 0.0, -1.0,
                      IntegerAlias(1), IntegerAlias(0), IntegerAlias(-1)):
            with self.subTest(value=repr(value), value_type=type(value).__name__):
                left, pivot, right = self.motif.construction
                with self.assertRaisesRegex(native.AdmissionError, "exact ternary integer"):
                    self.construct((left, (pivot[0], value), right))

    def test_boolean_float_and_subclass_chirality_reject_at_admission(self):
        for value in (True, 1.0, -1.0, IntegerAlias(1), IntegerAlias(-1)):
            with self.subTest(value=repr(value), value_type=type(value).__name__):
                with self.assertRaisesRegex(native.AdmissionError, "enclosure/chirality"):
                    self.construct(self.motif.construction, chirality=value)

    def test_out_of_domain_integer_signs_and_same_sign_dions_reject(self):
        left, pivot, right = self.motif.construction
        for pattern in ((2, -1), (0, -1), (1, 1), (-1, -1)):
            with self.subTest(pattern=pattern):
                with self.assertRaises(native.AdmissionError):
                    self.construct((pattern, pivot, right))
        for value in (-2, 2):
            with self.subTest(pivot=value):
                with self.assertRaises(native.AdmissionError):
                    self.construct((left, (pivot[0], value), right))

    def test_retained_motif_cannot_reintroduce_boolean_sign_alias(self):
        left, pivot, right = self.motif.construction
        forged = replace(self.motif, construction=((True, -1), pivot, right))
        with self.assertRaisesRegex(native.AdmissionError, "exact integers"):
            native.validate_motif(forged)

    def test_raw_decoder_rejects_aliases_before_classifying_duon(self):
        for side, position in ((0, 0), (1, 1), (2, 1)):
            for value in (True, 1.0, IntegerAlias(1)):
                with self.subTest(side=side, position=position, value=repr(value),
                                  value_type=type(value).__name__):
                    payload = deepcopy(self.payload)
                    construction = [list(part) for part in payload["formation"]["construction"]]
                    construction[side][position] = value
                    payload["formation"]["construction"] = construction
                    row = inspect_native_return(payload)
                    self.assertEqual(row["status"], "rejected")
                    self.assertIsNone(row["signature"])
                    self.assertNotIn("inferred_core", row)

    def test_valid_integer_construction_and_boolean_metadata_survive_reopen(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            manifest = run_manifest("exact-signs", source={"test": "R07-04"},
                kernel="canonical-native", preparation="integer-construction", backend="host")
            with TraceStore(path, manifest) as writer:
                trace = self.payload["trace"]
                writer.append("particle.native_return", self.payload,
                    scope_id=trace["scope"], lineage_id=trace["lineage"],
                    scale_index=trace["scale"], source_ref="sign-test", source_key="valid")
                writer.finish()
            with TraceStore.open(path) as reader:
                report = detect_particle_support(reader)
                self.assertEqual(report["coverage"], "complete")
                self.assertEqual(report["signatures"], {"Duon": 1})
                row = inspect_native_return(self.payload)
                self.assertEqual(row["status"], "verified")
                self.assertIs(self.payload["diagnostic_flag"], True)
                self.assertIs(self.payload["trace"]["reflected_comparison"], True)

    def test_boolean_construction_cannot_verify_after_reopening_archive(self):
        for side, position, value in ((0, 0, True), (1, 1, False)):
            with self.subTest(side=side), tempfile.TemporaryDirectory() as directory:
                payload = deepcopy(self.payload)
                construction = [list(part) for part in payload["formation"]["construction"]]
                construction[side][position] = value
                payload["formation"]["construction"] = construction
                path = Path(directory)
                manifest = run_manifest("hostile-signs", source={"test": "R07-04"},
                    kernel="canonical-native", preparation="boolean-sign", backend="host")
                with TraceStore(path, manifest) as writer:
                    trace = payload["trace"]
                    writer.append("particle.native_return", payload,
                        scope_id=trace["scope"], lineage_id=trace["lineage"],
                        scale_index=trace["scale"], source_ref="sign-test", source_key="hostile")
                    writer.finish()
                with TraceStore.open(path) as reader:
                    report = detect_particle_support(reader)
                    self.assertEqual(report["coverage"], "partial")
                    self.assertEqual(report["observed_formations"], 0)
                    self.assertEqual(report["signatures"], {})
                    self.assertEqual(report["returns"][0]["status"], "rejected")


if __name__ == "__main__":
    unittest.main()
