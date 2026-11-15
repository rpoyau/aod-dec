"""Independent span expectations and hostile native-history witnesses."""

from dataclasses import asdict, replace
import unittest

from simulations.sparc_native_span import NativeSpanError, extract_native_span
from simulations.vendor import aod_manual_native as native


def recurrent(starts=(0, 5, 14), lineage="a", scale=0):
    return native.wave([native.certified(native.trace(lineage, i, start_index=k, scale=scale))
                        for i, k in enumerate(starts)])


class NativeSpanTests(unittest.TestCase):
    def test_nonuniform_cadence_is_calculated_without_a_period(self):
        w = recurrent()
        result = extract_native_span((w,))
        row = result["waves"][0]
        self.assertEqual((row["first_return_cut"], row["last_return_cut"]), (2, 16))
        self.assertEqual(row["cadence"], [5, 9])
        self.assertEqual(row["local_windows"], [
            {"left_open": 2, "right_closed": 7, "span": 5},
            {"left_open": 7, "right_closed": 16, "span": 9}])
        self.assertEqual((row["closure_count"], row["reclosure_beats"], row["reclosure_span"]),
                         (3, 2, 14))
        self.assertFalse(result["continuous_observation_claimed"])

    def test_end_cut_retains_started_return_without_future_certificate(self):
        w = recurrent()
        row = extract_native_span((w,), end_cut=15)["waves"][0]
        self.assertEqual((row["closure_count"], row["cadence"]), (2, [5]))
        self.assertEqual(row["return_status"], "pending")
        prefix = row["pending_return_prefixes"][0]
        self.assertEqual(prefix["events"], (("a:2:e0", 0, 14), ("a:2:e1", 1, 15)))
        self.assertIsNone(prefix["complete_source_trace"])
        self.assertNotIn(w.certificates[-1].certificate_digest, row["certificate_digests"])
        self.assertNotIn(w.returns[-1].return_id, row["return_ids"])

    def test_actual_incomplete_tail_survives_last_completed_return(self):
        w = recurrent()
        tail = native.trace("a", 3, vertices=(0, 1, 3), start_index=20)
        row = extract_native_span((w,), pending_traces=(tail,))["waves"][0]
        self.assertEqual((row["last_return_cut"], row["last_recorded_cut"]), (16, 22))
        self.assertEqual(row["reclosure_beats"], 2)
        self.assertEqual(row["pending_return_prefixes"][0]["events"][-1], ("a:3:e2", 3, 22))
        self.assertEqual(row["pending_return_prefixes"][0]["complete_source_trace"], asdict(tail))
        self.assertEqual(row["pending_return_prefixes"][0]["source_trace_digest"], tail.trace_digest)

    def test_no_completion_differs_from_pending_and_unobserved_gap(self):
        w = recurrent()
        initial = extract_native_span((w,), end_cut=0)["waves"][0]
        self.assertEqual(initial["closure_count"], 0)
        self.assertEqual(initial["return_status"], "pending")
        self.assertIsNone(initial["first_return_cut"])
        gap = extract_native_span((w,), end_cut=4)["waves"][0]
        self.assertEqual(gap["return_status"], "no_pending_path_supplied")
        self.assertEqual(gap["unobserved_after_last_record"], {"left_open": 2, "right_closed": 4})
        self.assertEqual(extract_native_span(())["coverage"], "unavailable")

    def test_exact_cut_domain_rejects_future_and_invalid_cuts(self):
        for cut in (True, 3.0, "3", -1, 17):
            with self.subTest(cut=cut), self.assertRaises(NativeSpanError):
                extract_native_span((recurrent(),), end_cut=cut)
        with self.assertRaises(NativeSpanError):
            extract_native_span((), end_cut=0)

    def test_forged_certificate_and_overlapping_returns_fail(self):
        w = recurrent()
        bad = replace(w, certificates=(replace(w.certificates[0], status="open"),)
                      + w.certificates[1:])
        with self.assertRaises(NativeSpanError):
            extract_native_span((bad,))
        t = native.trace("a", 1, start_index=1)
        c = native.certify(t)
        overlap = replace(w, returns=(w.returns[0], t, w.returns[2]),
                          certificates=(w.certificates[0], c, w.certificates[2]))
        with self.assertRaisesRegex(NativeSpanError, "previous completion"):
            extract_native_span((overlap,))

    def test_individually_valid_conflicting_histories_fail_together(self):
        w = recurrent()
        # Same immutable event identities, different committed coordinates.
        records = [native.certified(native.trace("a", i, vertices=(0, 2, 0), start_index=k))
                   for i, k in enumerate((0, 5, 14))]
        other = native.wave(records)
        native.validate_wave(other)
        with self.assertRaisesRegex(NativeSpanError, "conflicting"):
            extract_native_span((w, other))

    def test_same_identity_diverging_appendices_fail(self):
        w = recurrent()
        records = [(t, c, {c.certificate_digest: c})
                   for t, c in zip(w.returns[:2], w.certificates[:2])]
        records.append(native.certified(native.trace("a", 99, start_index=25)))
        fork = native.wave(records)
        self.assertEqual(w.identity, fork.identity)
        with self.assertRaisesRegex(NativeSpanError, "conflicting histories"):
            extract_native_span((w, fork))

    def test_replayed_alias_and_compatible_prefix_do_not_duplicate_beats(self):
        w = recurrent()
        prefix = native.wave([(t, c, {c.certificate_digest: c})
                              for t, c in zip(w.returns[:2], w.certificates[:2])])
        report = extract_native_span((prefix, w, w))
        self.assertEqual(len(report["waves"]), 1)
        self.assertEqual(report["waves"][0]["reclosure_beats"], 2)

    def test_actual_fold_dependencies_preserve_separate_inner_beats(self):
        inner = recurrent()
        records = [native.certified(native.outer_trace((inner,), occurrence=i, scale=7),
                                    "outer", inner=(inner,)) for i in range(2)]
        outer = native.wave(records)
        report = extract_native_span((outer,), addresses={inner.identity: (0, 0),
                                                         outer.identity: (1, 7, 0)})
        self.assertEqual(report["scale_range"], {"minimum": 0, "maximum": 7,
                                               "retained_values": [0, 7]})
        self.assertEqual(len(report["fold_dependencies"]), 2)
        self.assertEqual([r["reclosure_beats"] for r in report["waves"]], [2, 1])
        path = report["retained_scope_paths"][0]
        self.assertEqual([r["identity"] for r in path], [outer.identity, inner.identity])
        self.assertEqual([r["address"] for r in path], [[1, 7, 0], [0, 0]])
        self.assertEqual([r["recurrence_status"] for r in path], ["admitted", "admitted"])
        self.assertIsNone(report["physical_scale_multiplier"])

    def test_started_outer_support_is_not_prematurely_admitted_recurrence(self):
        inner = recurrent()
        outer = native.wave([native.certified(
            native.outer_trace((inner,), occurrence=i, scale=7), "outer", inner=(inner,))
            for i in range(2)])
        for cut, status, scales in ((17, "no_completed_return_in_projection", [0]),
                                   (18, "no_completed_return_in_projection", [0]),
                                   (19, "one_closure", [0]), (29, "admitted", [0, 7])):
            with self.subTest(cut=cut):
                report = extract_native_span((outer,), end_cut=cut)
                self.assertEqual(report["scale_range_basis"], "started_support")
                self.assertEqual(report["scale_range"]["retained_values"], [0, 7])
                self.assertEqual(report["admitted_recurrent_scale_range"]["retained_values"], scales)
                self.assertEqual(report["retained_scope_paths"][0][0]["recurrence_status"], status)
                row = next(r for r in report["waves"] if r["identity"] == outer.identity)
                self.assertEqual(row["identity_status"], "admitted_by_cut" if cut == 29 else
                                 "retrospective_wave_identity")
                if cut < 19:
                    self.assertEqual(report["fold_dependencies"], [])

    def test_different_future_suffixes_keep_same_cutoff_prefix_and_admitted_range(self):
        common = recurrent(starts=(0, 5))
        common_records = [(t, c, {c.certificate_digest: c})
                          for t, c in zip(common.returns, common.certificates)]
        reports = []
        for vertices in ((0, 1, 3, 2, 0), (0, 1, 9, 8, 0)):
            tail = native.certified(native.trace("a", 2, vertices=vertices, turn=2,
                                                 start_index=14))
            reports.append(extract_native_span((native.wave(common_records + [tail]),), end_cut=15))
        a, b = [report["waves"][0]["pending_return_prefixes"][0] for report in reports]
        self.assertEqual(a["events"], b["events"])
        self.assertEqual(a["prefix_digest"], b["prefix_digest"])
        self.assertNotEqual(a["source_trace_digest"], b["source_trace_digest"])
        self.assertEqual(a["source_trace_digest_scope"], "retrospective_full_source_not_cutoff_local")
        self.assertEqual(a["prefix_digest_scope"], "events_committed_at_or_before_end_cut")
        self.assertIsNone(a["complete_source_trace"])
        self.assertIsNone(b["complete_source_trace"])
        self.assertEqual(reports[0]["admitted_recurrent_scale_range"],
                         reports[1]["admitted_recurrent_scale_range"])
        self.assertEqual(reports[0]["waves"][0]["return_ids"], reports[1]["waves"][0]["return_ids"])

    def test_octave_binding_is_optional_and_not_a_physical_multiplier(self):
        w = recurrent(scale=9)
        row = extract_native_span((w,))["waves"][0]
        self.assertIsNone(row["address"])
        self.assertEqual(row["address_status"], "not_stored_by_native_producer")
        for address in ((), (8,), (True,), "00", (-1,)):
            with self.subTest(address=address), self.assertRaises(NativeSpanError):
                extract_native_span((w,), addresses={w.identity: address})
        with self.assertRaises(NativeSpanError):
            extract_native_span((w,), addresses={"unbound": (0, 0)})

    def test_pending_tail_cannot_alias_closed_overlap_or_other_lineage(self):
        w = recurrent()
        tails = (native.trace("a", 3, start_index=20),
                 native.trace("a", 3, vertices=(0, 1, 3), start_index=15),
                 native.trace("other", 3, vertices=(0, 1, 3), start_index=20))
        for tail in tails:
            with self.subTest(tail=tail), self.assertRaises(NativeSpanError):
                extract_native_span((w,), pending_traces=(tail,))


if __name__ == "__main__":
    unittest.main()
