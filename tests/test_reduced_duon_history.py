"""Actual retained-event checks for the representative finite Duon pool."""

from dataclasses import asdict, replace, FrozenInstanceError
from fractions import Fraction as Q
import unittest
from unittest.mock import patch

from simulations.reduced_duon_history import (HistoryError, PendingReturn, prepare,
                                              advance, channel_comparison)
from simulations.vendor import aod_manual_native as native


class ReducedDuonHistoryTests(unittest.TestCase):
    def test_preparation_is_certified_duon_with_computed_native_cadence(self):
        pool = prepare(("a", "b", "c"), "radial")
        self.assertEqual(pool.committed_cut, 11)
        self.assertEqual(pool.pending, ())
        for wave, pivot in zip(pool.waves, pool.pivots):
            native.validate_wave(wave)
            self.assertEqual(wave.motif, "Duon")
            self.assertEqual(pivot.trace.indices, (0, 1, 2))
            self.assertEqual([t.indices for t in wave.returns], [(3, 4, 5), (6, 7, 8), (9, 10, 11)])
            self.assertEqual(wave.cadence, (3, 3))
            self.assertTrue(all(m.component_traces == (pivot.trace,) for m in wave.motif_witnesses))
        self.assertEqual(pool.metadata["cadence_native_cuts"], 3)
        self.assertIsNone(pool.metadata["si_time_map"])

    def test_one_event_advance_retains_prefix_without_future_trace(self):
        initial = prepare(("a", "b"))
        with patch.object(native, "certify", side_effect=AssertionError("premature certification")):
            first = advance(initial)
            second = advance(first)
        self.assertIs(first.history, initial.history)
        self.assertIs(second.history, initial.history)
        self.assertEqual(first.pending[0].events, (("a:4:e0", 0, 12),))
        self.assertEqual(second.pending[0].events, (("a:4:e0", 0, 12), ("a:4:e1", 1, 13)))
        self.assertNotIn("reflection", asdict(first.pending[0]))
        complete = advance(second)
        self.assertEqual((complete.committed_cut, complete.pending), (14, ()))
        trace = complete.wave("a").returns[-1]
        self.assertEqual(tuple(zip(trace.event_ids, trace.vertices, trace.indices))[:2],
                         second.pending[0].events)
        self.assertEqual(complete.wave("a").returns[:-1], initial.wave("a").returns)
        self.assertEqual(initial.committed_cut, 11)
        self.assertEqual(len(initial.wave("a").returns), 3)

    def test_later_cycles_add_one_return_without_replay_or_jitter(self):
        pool = prepare(("a",))
        counts = []
        for _ in range(9):
            pool = advance(pool)
            counts.append(len(pool.wave("a").returns))
        self.assertEqual(counts, [3, 3, 4, 4, 4, 5, 5, 5, 6])
        self.assertEqual([t.indices[-1] for t in pool.wave("a").returns], [5, 8, 11, 14, 17, 20])
        self.assertEqual(pool.wave("a").cadence, (3,) * 5)
        self.assertEqual(len({t.return_id for t in pool.wave("a").returns}), 6)
        native.validate_wave(pool.wave("a"))

    def test_contacts_use_distinct_committed_cuts_and_counted_reflections(self):
        pool = prepare(("a", "b"))
        evidence = channel_comparison(pool, "a", "b")
        self.assertEqual(evidence.window, (8, 11))
        self.assertEqual([c.committed for c in evidence.contacts], [8, 11])
        self.assertEqual(len({c.identity for c in evidence.contacts}), 2)
        self.assertEqual([c.reflection for c in evidence.contacts], [(1, 1), (1, 1)])
        self.assertEqual([(r.left_open, r.right_closed, r.left_returns, r.right_returns)
                          for r in evidence.reflection_intervals], [(5, 8, 1, 1), (8, 11, 1, 1)])
        self.assertEqual((evidence.rcd.support, evidence.duration, evidence.participation), (3, 5, 3))
        self.assertEqual((evidence.ratio, evidence.bias, evidence.lag), (1, 0, 0))
        for contact in evidence.contacts:
            self.assertEqual(contact.left_prefix, native.wave_prefix(pool.wave("a"), contact.committed))
            self.assertLessEqual(contact.support_trace.indices[-1], contact.committed)
        self.assertEqual(channel_comparison(pool, "a", "b"), evidence)

    def test_partial_advance_keeps_source_comparison_until_next_actual_return(self):
        pool = prepare(("a", "b"))
        original = channel_comparison(pool, "a", "b")
        for _ in range(2):
            pool = advance(pool)
            self.assertEqual(channel_comparison(pool, "a", "b"), original)
        pool = advance(pool)
        later = channel_comparison(pool, "a", "b")
        self.assertEqual(later.window, (11, 14))
        self.assertEqual(later.contacts[0], original.contacts[1])
        self.assertNotEqual(later.contacts[1].identity, original.contacts[1].identity)

    def test_bias_never_changes_phase_returns_contacts_or_cadence(self):
        pool = prepare(("a", "b"))
        before = asdict(pool)
        base = channel_comparison(pool, "a", "b")
        biased = channel_comparison(pool, "a", "b", bias=(Q(3, 4), Q(1, 5)))
        self.assertEqual((biased.bias, biased.lag), (Q(11, 20), Q(11, 20)))
        self.assertEqual(base.contacts, biased.contacts)
        self.assertEqual(base.rcd, biased.rcd)
        self.assertEqual(asdict(pool), before)

    def test_identity_and_exact_domain_rejections(self):
        for lineages in ((), "a", ("a", "a"), (True,), ("",)):
            with self.subTest(lineages=lineages), self.assertRaises(HistoryError):
                prepare(lineages)
        pool = prepare(("a", "b"))
        for left, right in (("a", "a"), ("a", "missing"), (True, "b")):
            with self.subTest(left=left, right=right), self.assertRaises(HistoryError):
                channel_comparison(pool, left, right)
        for bias in ((True, 0), (0.5, 0), (0,), "00"):
            with self.subTest(bias=bias), self.assertRaises(HistoryError):
                channel_comparison(pool, "a", "b", bias=bias)
        for cut in (True, 12, 14):
            with self.subTest(cut=cut), self.assertRaises(HistoryError):
                replace(pool, committed_cut=cut)
        with self.assertRaises(HistoryError):
            advance({})
        with self.assertRaises(FrozenInstanceError):
            pool.committed_cut = 99

    def test_forged_new_history_and_future_pending_events_fail(self):
        pool = prepare(("a", "b"))
        wave = pool.wave("a")
        forged = replace(wave, identity="forged")
        with self.assertRaises(HistoryError):
            replace(pool.history, waves=(forged, pool.wave("b")))
        partial = advance(pool)
        future = PendingReturn("a", 4, (("a:4:e0", 0, 12), ("a:4:e1", 1, 13)))
        with self.assertRaises(HistoryError):
            replace(partial, pending=(future, partial.pending[1]))
        for events in ((("a:4:e0", False, 12),), (("a:4:e0", 0, True),)):
            with self.subTest(events=events), self.assertRaises(HistoryError):
                PendingReturn("a", 4, events)


if __name__ == "__main__":
    unittest.main()
