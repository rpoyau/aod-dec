"""Unlabelled construction inference, observation scope and hostile witnesses."""
from dataclasses import asdict
import gzip
import json
from pathlib import Path
import tempfile
import unittest

from simulations.vendor import aod_manual_native as native
from simulations.trace_store import TraceStore, run_manifest
from simulations.trace_support import inspect_native_return, detect_particle_support, SupportError
from simulations.trace_views import Window
from simulations.trace_applications import retain_blob


def formation(p=1, q=2, *, lineage="formed", scope="cavity", occurrence=0, offset=0,
              vertices=(0, 1, 3, 7, 6, 4, 0)):
    components = tuple(native.certified(native.trace(lineage, occurrence * 10 + i,
        scope=scope, start_index=offset + i * 3)) for i in range(q))
    trace = native.trace(lineage, occurrence * 10 + q, vertices=vertices,
        turn=(len(vertices) - 1) // 2, scope=scope, start_index=offset + 3 * q)
    record = native.certified(trace)
    registry = {c.certificate_digest: c for _, c, _ in (*components, record)}
    ids = tuple(c.return_id for _, c, _ in components)
    motif = native.motif_witness(f"{p}:{q}", trace, (tuple(ids for _ in range(p)), ids),
        tuple((t, c) for t, c, _ in components), registry, enclosure=len(vertices) - 1, chirality=1)
    return {"schema": "native-particle-return-v1", "family": "test-construction", "return_ordinal": occurrence,
        "wave": {"identity": "unused-wave-identity", "scope": scope, "scale": 0,
                 "lineage": lineage, "declared_motif": f"{p}:{q}"},
        "trace": asdict(trace), "certificate": asdict(record[1]), "motif_witness": asdict(motif)}


class SupportDetectionTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.manifest = run_manifest("support-detection", source={"repository": "aod-dec"},
            kernel="canonical-native", preparation="raw-construction", backend="host")

    def tearDown(self):
        self.tmp.cleanup()

    def store(self, payloads, *, complete=True):
        path = self.root / str(len(tuple(self.root.iterdir())))
        store = TraceStore(path, self.manifest)
        for ordinal, payload in enumerate(payloads):
            trace = payload["trace"]
            store.append("particle.native_return", payload, scope_id=trace["scope"],
                lineage_id=trace["lineage"], scale_index=trace["scale"],
                source_ref="test-canonical-construction", source_key="return:" + str(ordinal))
        if complete:
            store.finish()
        self.addCleanup(store.close)
        return store

    def test_infers_pq_from_raw_blocks_without_classification_label(self):
        payload = formation()
        payload.pop("wave")
        row = inspect_native_return(payload)
        self.assertEqual((row["status"], row["signature"], row["support"]["RD"]), ("verified", "1:2", 9))
        self.assertEqual(row["detected_support_form"], "1:2:6")
        self.assertIsNone(row["physical_species"])

    def test_rd_nine_does_not_invent_nine_window_or_tau(self):
        report = detect_particle_support(self.store((formation(),)))
        self.assertEqual(report["signatures"], {"1:2": 1})
        self.assertIsNone(report["returns"][0]["record_window_view"])
        self.assertIsNone(report["application_classification"]["value"])

    def test_explicit_nine_view_preserves_detected_form_and_identity(self):
        payload = formation()
        original = inspect_native_return(payload)
        viewed = inspect_native_return(payload, record_window=9)
        self.assertEqual(viewed["record_window_view"]["signature"], "1:2:9")
        self.assertEqual(viewed["record_window_view"]["rho_window"], 9)
        self.assertEqual(viewed["detected_support_form"], "1:2:6")
        self.assertEqual(viewed["return_id"], original["return_id"])
        self.assertEqual(viewed["support"], original["support"])

    def test_three_three_rd_is_reconstructed(self):
        row = inspect_native_return(formation(3, 3))
        self.assertEqual(row["support"]["Pi"], 30)
        self.assertEqual(row["support"]["RD"], 61)
        self.assertEqual(row["detected_support_form"], "3:3:6")

    def test_three_j_shapes_are_not_special_cased_to_proton(self):
        for j in (1, 2, 4):
            row = inspect_native_return(formation(3, j))
            self.assertEqual(row["signature"], f"3:{j}")
            self.assertEqual(row["support"]["RD"], 2 * (j ** 3 + j) + 1 if j >= 2 else None)
            self.assertEqual(row["shedding"]["status"], "unavailable")
            self.assertEqual(row["coupling"]["status"], "unavailable")

    def test_enclosure_is_observed_from_actual_path_not_fixed_six(self):
        cycle = (0, 1, 3, 2, 6, 7, 5, 4, 0)
        row = inspect_native_return(formation(3, 4, vertices=cycle))
        self.assertEqual(row["detected_support_form"], "3:4:8")
        self.assertEqual(row["enclosure"]["observed_simple_cycle"], 8)

    def test_repeated_edges_do_not_make_declared_simple_enclosure(self):
        row = inspect_native_return(formation(vertices=(0, 1, 0, 1, 0, 1, 0)))
        self.assertEqual(row["signature"], "1:2")
        self.assertIsNone(row["detected_support_form"])
        self.assertEqual(row["enclosure"]["status"], "unresolved")

    def test_duon_grammar_does_not_imply_duad_or_shedding(self):
        from simulations.collision_routing import _route_support
        source = native.recurrent("source", n=2)
        wave = _route_support(source, "route-test", 20)["wave"]
        payload = {"trace": asdict(wave.returns[0]), "certificate": asdict(wave.certificates[0]),
                   "motif_witness": asdict(wave.motif_witnesses[0])}
        row = inspect_native_return(payload)
        self.assertEqual((row["status"], row["signature"]), ("verified", "Duon"))
        self.assertIsNone(row["support"])
        self.assertEqual(row["closure"]["status"], "verified")
        self.assertEqual(row["seating"]["status"], "unavailable")
        self.assertEqual(row["shedding"]["status"], "unavailable")

    def test_label_free_raw_formation_recovers_generic_shapes(self):
        for p, q in ((1, 2), (3, 1), (3, 2), (3, 4)):
            payload = formation(p, q)
            motif = payload.pop("motif_witness")
            payload.pop("wave")
            payload["formation"] = {key: motif[key] for key in
                ("construction", "component_traces", "component_certificates")}
            row = inspect_native_return(payload)
            self.assertEqual(row["status"], "verified")
            self.assertEqual(row["signature"], f"{p}:{q}")
            self.assertEqual(row["detected_support_form"], f"{p}:{q}:6")
            self.assertIsNone(row["enclosure"]["declared"])

    def test_label_free_duon_grammar_without_enclosure_claim(self):
        from simulations.collision_routing import _route_support
        wave = _route_support(native.recurrent("source", n=2), "route-test", 20)["wave"]
        motif = asdict(wave.motif_witnesses[0])
        payload = {"trace": asdict(wave.returns[0]), "certificate": asdict(wave.certificates[0]),
                   "formation": {key: motif[key] for key in
                       ("construction", "component_traces", "component_certificates")}}
        row = inspect_native_return(payload)
        self.assertEqual((row["status"], row["signature"]), ("verified", "Duon"))
        self.assertIsNone(row["detected_support_form"])
        self.assertIsNone(row["enclosure"]["observed_simple_cycle"])
        self.assertEqual(row["seating"]["status"], "unavailable")

    def test_mislabelled_motif_and_wave_are_rejected(self):
        for path in ("motif", "wave"):
            payload = formation()
            if path == "motif":
                payload["motif_witness"]["core"] = "3:3"
            else:
                payload["wave"]["declared_motif"] = "Tau"
            row = inspect_native_return(payload)
            self.assertEqual(row["status"], "rejected")
            self.assertIsNone(row["signature"])

    def test_scalar_rd_or_name_cannot_replace_construction(self):
        payload = formation()
        payload["motif_witness"] = None
        payload.update(expected="1:2:9", RD=9)
        row = inspect_native_return(payload)
        self.assertEqual(row["status"], "unresolved")
        self.assertIsNone(row["signature"])

    def test_missing_certificate_is_not_promoted(self):
        payload = formation(); payload["certificate"] = None
        self.assertEqual(inspect_native_return(payload)["status"], "unresolved")

    def test_missing_component_or_changed_chirality_rejected(self):
        for key in ("component_traces", "chirality"):
            payload = formation()
            if key == "component_traces":
                payload["motif_witness"][key] = payload["motif_witness"][key][:-1]
            else:
                payload["motif_witness"][key] = -1
            self.assertEqual(inspect_native_return(payload)["status"], "rejected")

    def test_raw_endpoint_mutation_rejected(self):
        payload = formation()
        payload["trace"]["vertices"] = (*payload["trace"]["vertices"][:-1], 5)
        self.assertEqual(inspect_native_return(payload)["status"], "rejected")

    def test_duplicate_occurrence_is_idempotent(self):
        payload = formation()
        report = detect_particle_support(self.store((payload, payload)), max_returns=1)
        self.assertEqual(report["coverage"], "complete")
        self.assertEqual(report["observed_formations"], 1)
        self.assertEqual(len(report["returns"][0]["evidence"]), 2)

    def test_conflicting_terminal_native_identity_fails(self):
        report = detect_particle_support(self.store((formation(), formation(offset=100))))
        self.assertEqual(report["coverage"], "partial")
        self.assertIn("conflicting native history", report["errors"][0])

    def test_scope_is_part_of_identity(self):
        report = detect_particle_support(self.store((formation(scope="a"), formation(scope="b"))), scope="b")
        self.assertEqual(report["coverage"], "complete")
        self.assertEqual(report["observed_formations"], 1)
        self.assertEqual(report["returns"][0]["scope"], "b")

    def test_rewindow_uses_native_cuts_and_retained_prefix(self):
        payload = formation(); final = payload["trace"]["indices"][-1]
        store = self.store((payload,))
        wide = detect_particle_support(store)
        clipped = detect_particle_support(store, window=Window(final, final + 1, "native-committed-cut"))
        self.assertEqual(clipped["returns"][0]["return_id"], wide["returns"][0]["return_id"])
        censored = detect_particle_support(store, window=Window(final - 1, final, "native-committed-cut"))
        self.assertEqual(censored["observed_formations"], 0)
        self.assertEqual(len(censored["censored"]), 1)
        empty = detect_particle_support(store, window=Window(final, final, "native-committed-cut"))
        self.assertEqual((empty["observed_formations"], empty["censored"]), (0, []))

    def test_mixed_observation_domain_rejected(self):
        with self.assertRaises(SupportError):
            detect_particle_support(self.store((formation(),)), window=Window(0, 100))

    def test_incomplete_archive_and_budget_remain_partial(self):
        report = detect_particle_support(self.store((formation(),), complete=False))
        self.assertEqual(report["coverage"], "partial")
        report = detect_particle_support(self.store((formation(),)), max_bytes=10)
        self.assertEqual(report["coverage"], "partial")
        self.assertEqual(report["observed_formations"], 0)

    def test_envelope_context_mismatch_rejected(self):
        payload = formation()
        store = TraceStore(self.root / "mismatch", self.manifest)
        self.addCleanup(store.close)
        store.append("particle.native_return", payload, scope_id="different", lineage_id="formed",
                     source_ref="test", source_key="different")
        store.finish()
        report = detect_particle_support(store)
        self.assertEqual(report["coverage"], "partial")
        self.assertEqual(report["observed_formations"], 0)

    def test_bounded_legacy_archive_replay_and_gzip_bomb(self):
        payload = formation()
        wave = {"identity": "unused", "scope": "cavity", "scale": 0, "lineage": "formed", "motif": "1:2",
                "returns": [payload["trace"]], "certificates": [payload["certificate"]],
                "motif_witnesses": [payload["motif_witness"]]}
        store = TraceStore(self.root / "legacy", self.manifest)
        self.addCleanup(store.close)
        data = json.dumps({"carrier_waves": [wave]}).encode()
        retain_blob(store, gzip.compress(data), name="proton-accounting-witnesses.json.gz", scope="particle-evidence")
        store.finish()
        report = detect_particle_support(store)
        self.assertEqual(report["coverage"], "complete")
        self.assertEqual(report["signatures"], {"1:2": 1})
        self.assertEqual(report["input_groups"]["routed-support"], {"status": "absent", "capture_records": None})
        limited = detect_particle_support(store, max_expanded_bytes=100)
        self.assertEqual(limited["coverage"], "partial")
        self.assertEqual(limited["observed_formations"], 0)

    def test_legacy_routed_support_is_detected(self):
        from simulations.collision_routing import _route_support
        wave = _route_support(native.recurrent("source", n=2), "legacy-route", 20)["wave"]
        store = TraceStore(self.root / "legacy-routed", self.manifest)
        self.addCleanup(store.close)
        data = json.dumps({"route_witnesses": [{"route_support": {"wave": asdict(wave)}}]}).encode()
        retain_blob(store, gzip.compress(data), name="proton-accounting-witnesses.json.gz", scope="particle-evidence")
        store.finish()
        report = detect_particle_support(store)
        self.assertEqual(report["coverage"], "complete")
        self.assertEqual(report["signatures"], {"Duon": 2})
        self.assertEqual(report["input_groups"]["routed-support"], {"status": "present", "capture_records": 2})
        self.assertEqual(report["input_groups"]["proton-carrier"]["status"], "absent")

    def test_malformed_present_legacy_routing_is_not_silently_omitted(self):
        for index, routes in enumerate(({}, [None], [{"route_support": {}}], [{"route_support": {"wave": {}}}])):
            with self.subTest(routes=routes):
                store = TraceStore(self.root / ("legacy-malformed-route-" + str(index)), self.manifest)
                self.addCleanup(store.close)
                data = json.dumps({"route_witnesses": routes}).encode()
                retain_blob(store, gzip.compress(data), name="proton-accounting-witnesses.json.gz", scope="particle-evidence")
                store.finish()
                report = detect_particle_support(store)
                self.assertEqual(report["coverage"], "partial")
                self.assertEqual(report["observed_formations"], 0)
                self.assertTrue(report["errors"])

    def test_record_window_exact_domain(self):
        for bad in (True, -1, 1.5, 1000001):
            with self.subTest(value=bad), self.assertRaises(SupportError):
                inspect_native_return(formation(), record_window=bad)

    def test_duplicate_legacy_json_fields_are_rejected(self):
        store = TraceStore(self.root / "duplicate-json", self.manifest)
        self.addCleanup(store.close)
        data = b'{"carrier_waves":[{"invalid":"hidden by duplicate"}],"carrier_waves":[]}'
        retain_blob(store, gzip.compress(data), name="proton-accounting-witnesses.json.gz", scope="particle-evidence")
        store.finish()
        report = detect_particle_support(store)
        self.assertEqual(report["coverage"], "partial")
        self.assertEqual(report["observed_formations"], 0)
        self.assertIn("duplicate native JSON field", report["errors"][0])

    def test_inexact_legacy_json_numbers_are_rejected(self):
        for index, number in enumerate((b'1.5', b'NaN', b'Infinity')):
            with self.subTest(number=number):
                store = TraceStore(self.root / ("inexact-json-" + str(index)), self.manifest)
                self.addCleanup(store.close)
                data = b'{"carrier_waves":[],"number":' + number + b'}'
                retain_blob(store, gzip.compress(data), name="proton-accounting-witnesses.json.gz", scope="particle-evidence")
                store.finish()
                report = detect_particle_support(store)
                self.assertEqual(report["coverage"], "partial")
                self.assertEqual(report["observed_formations"], 0)
                self.assertIn("inexact number in native witness", report["errors"][0])


if __name__ == "__main__":
    unittest.main()
