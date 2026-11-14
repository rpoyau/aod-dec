"""Typed view contracts: TP-07/08/09/12; no substituted scale dynamics."""
from dataclasses import replace
from fractions import Fraction
import unittest

from simulations.trace_views import (Window, Duration, FractalCoordinate, Span, ScaleLevel,
    ScaleFamily, OctaveRange, ViewError, clip_participation, select_events, union_views)


def event(i, **changes):
    value = {"run_id": "r", "event_id": str(i), "commit_ordinal": i,
             "scope_id": "B", "fractal_coordinate": [0, i % 8], "scale_family": "declared-core",
             "scale_index": 0, "payload": {"support_ids": ["a", "b"]}}
    value.update(changes)
    return value


class TraceViewTests(unittest.TestCase):
    def test_half_open_empty_and_integer_endpoint_conversion(self):
        self.assertEqual([i for i in range(6) if Window(2, 4).contains(i)], [2, 3])
        self.assertEqual(list(select_events([event(2)], window=Window(2, 2))), [])
        w = Window.from_right_closed(2, 4)
        self.assertEqual((w.start, w.stop), (3, 5))
        self.assertEqual([i for i in range(6) if w.contains(i)], [3, 4])

    def test_invalid_endpoint_aliases_and_domain_rejected(self):
        for value in (True, 1.0):
            with self.assertRaises(ViewError):
                Window(value, 2)
        with self.assertRaises(ViewError):
            Window(2, 1)
        with self.assertRaises(ViewError):
            Window(0, 2, "native-return").contains(1)
        with self.assertRaises(ViewError):
            list(select_events([], window=Window(0, 2, role="retention")))

    def test_participation_clips_only_scalar_keeps_rd_rcd(self):
        rd, rcd = ("RD", 7), ("directional-RCD", 5, 9)
        for window, expected in ((0, 0), (2, 2), (4, 4), (6, 5), (12, 5)):
            result = clip_participation(rd, rcd, Duration(5, "native-return"), Duration(window, "native-return"))
            self.assertIs(result["RD"], rd); self.assertIs(result["RCD"], rcd)
            self.assertEqual(result["rho_window"].value, expected)
        self.assertEqual(clip_participation(rd, rcd, Duration(Fraction(3, 2), "D"), Duration(1, "D"))["rho_window"].value, 1)

    def test_participation_incompatible_types_not_coerced(self):
        for value in (True, 1.0, -1):
            with self.assertRaises(ViewError): Duration(value, "D")
        with self.assertRaises(ViewError):
            clip_participation(1, 2, Duration(3, "RD"), Duration(4, "host-second"))

    def test_octal_coordinate_is_not_numeric_scale(self):
        self.assertEqual(FractalCoordinate((0, 0)).display, "00_8")
        self.assertEqual(FractalCoordinate((1, 0)).display, "10_8")
        for digits in ((8,), (True,), (1.0,), ()):
            with self.assertRaises(ViewError): FractalCoordinate(digits)
        with self.assertRaises(ViewError): FractalCoordinate((1,), family="binary-powers")

    def test_declared_formation_family_requires_ancestry(self):
        family = ScaleFamily("declared-core", "AOD:outer-close", (
            ScaleLevel(0, "archive:inner"), ScaleLevel(1, "archive:outer", (0,))))
        span = OctaveRange(family, 0, 1)
        self.assertTrue(span.contains("declared-core", 1))
        self.assertFalse(span.contains("declared-core", 2))
        with self.assertRaises(ViewError): ScaleLevel(1, "multiplied-current")
        with self.assertRaises(ViewError): OctaveRange(family, 0, 8)
        with self.assertRaises(ViewError): span.contains("frequency-doubling", 1)

    def test_span_is_separate_support_extent(self):
        events = [event(i) for i in range(5)]
        result = list(select_events(events, window=Window(1, 4),
                     coordinates=(FractalCoordinate((0, 2)),), span=Span(("a",))))
        self.assertEqual([e["event_id"] for e in result], ["2"])
        with self.assertRaises(ViewError): Span(("a",), "duration")
        with self.assertRaises(ViewError):
            list(select_events([event(1, payload={})], span=Span(("a",))))

    def test_overlapping_alternative_views_are_occurrence_union(self):
        events = [event(i) for i in range(7)]
        a = tuple(select_events(events, window=Window(1, 5)))
        b = tuple(select_events(events, window=Window(3, 7)))
        self.assertEqual(len(a) + len(b), 8)
        self.assertEqual([e["event_id"] for e in union_views(a, b)], [str(i) for i in range(1, 7)])

    def test_overlap_rejects_conflicts_families_and_unbounded_query(self):
        with self.assertRaises(ViewError): union_views([event(1)], [event(1, scope_id="different")])
        with self.assertRaises(ViewError): union_views([event(1)], [event(2, run_id="other")])
        with self.assertRaises(ViewError): union_views([event(1), event(2)], max_events=1)
        with self.assertRaises(ViewError): union_views([event(1)], max_bytes=10)


if __name__ == "__main__":
    unittest.main()
