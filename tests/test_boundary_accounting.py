"""Hostile accounting cases; native admission is exercised by the separate adapter."""
from dataclasses import replace
from fractions import Fraction as F
import unittest

from simulations.boundary_accounting import (
    ArchiveState, BoundaryCoverage, BoundaryMotion, ContentRecord, DispositionChange,
    RouteLeg, RouteLedger, RouteRecord, ScaleMap, account_slice, accumulate_current,
    archive_view, commit_archive, pending_routes, project_window, receive_route,
    register_route, validate_route_successor, validate_successor,
)

DIGEST = "a" * 64
CURRENT = "signed-native-current"
BURDEN = "closure-burden"


def record(identity, value, cut=1, **overrides):
    args = dict(identity=identity, physical_id=identity, source_digest=DIGEST,
                committed=cut, scale=0, unit=CURRENT, value=value,
                support_ids=(identity,), parent_ids=())
    args.update(overrides)
    return ContentRecord(**args)


def archive(*items, cut=1):
    return commit_archive(ArchiveState(), cut, tuple(items))


def coverage(cut=1, cross=(), shedding=(), **overrides):
    args = dict(cut=cut, boundary_id="boundary", scale=0, unit=CURRENT,
                cross=cross, shedding=shedding, scope="complete admitted boundary channel family",
                window=4, domain="native-return-count", provenance=DIGEST)
    args.update(overrides)
    return BoundaryCoverage(**args)


def motion(before=("a",), after=None, sweep=0):
    after = before if after is None else after
    return BoundaryMotion("stationary" if set(before) == set(after) else "moving",
                          before, after, sweep, CURRENT, DIGEST)


def row(state, cut=1, cross=(), shedding=(), move=None, **overrides):
    args = dict(cross=cross, shedding=shedding, motion=motion() if move is None else move,
                coverage=coverage(cut, cross, shedding))
    args.update(overrides)
    return account_slice(state, cut, "boundary", 0, CURRENT, **args)


class BoundaryAccountingTests(unittest.TestCase):
    def test_native_manual_signed_example_and_cancellation_channels(self):
        state = archive((record("plus", 12), "active"), (record("minus", -4), "active"),
                        (record("shed", 2), "routed"))
        result = row(state, cross=("plus", "minus"), shedding=("shed",),
                     move=motion(after=("a", "b"), sweep=3))
        self.assertEqual((result.cross_total, result.sweep_total, result.shedding_total,
                          result.moving_total), (8, 3, 2, 7))
        total = accumulate_current((result,))
        self.assertEqual(total["current"], 7)
        self.assertEqual(total["contribution_ids"], ("plus", "minus", "shed"))
        self.assertEqual(total["stock_balance_status"], "NOT_ASSERTED")

    def test_dropout_recoupling_preserves_archive_without_automatic_activation(self):
        source = record("relation", 4)
        first = archive((source, "active"))
        second = commit_archive(first, 2, changes=(DispositionChange("relation", "active", "inactive", "dropout"),))
        self.assertEqual(archive_view(second)[0]["relation"], source)
        with self.assertRaisesRegex(ValueError, "not explicitly active"):
            row(second, 2, cross=("relation",))
        third = commit_archive(second, 3, changes=(DispositionChange("relation", "inactive", "active", "recoupling"),))
        self.assertEqual(row(third, 3, cross=("relation",)).moving_total, 4)
        removed = commit_archive(third, 4, changes=(DispositionChange("relation", "active", "removed", "structural control"),))
        with self.assertRaisesRegex(ValueError, "explicit restoration"):
            DispositionChange("relation", "removed", "active", "archive lookup")
        restored = commit_archive(removed, 5, changes=(DispositionChange("relation", "removed", "active", "matched restoration", True),))
        self.assertEqual(archive_view(restored)[0]["relation"], source)

    def test_dropped_rewritten_history_and_stale_dispositions_fail(self):
        first = archive((record("a", 1), "active"))
        with self.assertRaisesRegex(ValueError, "dropped or rewritten"):
            validate_successor(first, ArchiveState())
        rewritten = archive((record("a", 99), "active"))
        with self.assertRaisesRegex(ValueError, "dropped or rewritten"):
            validate_successor(first, rewritten)
        with self.assertRaisesRegex(ValueError, "stale old disposition"):
            commit_archive(first, 2, changes=(DispositionChange("a", "pending", "active", "wrong prior"),))

    def test_explicit_motion_and_explicit_coverage_are_required(self):
        state = ArchiveState()
        with self.assertRaisesRegex(ValueError, "motion/sweep"):
            account_slice(state, 1, "boundary", 0, CURRENT, coverage=coverage())
        with self.assertRaisesRegex(ValueError, "coverage"):
            account_slice(state, 1, "boundary", 0, CURRENT, motion=motion())
        with self.assertRaisesRegex(ValueError, "stationary"):
            BoundaryMotion("stationary", ("a",), ("b",), 0, CURRENT, DIGEST)
        with self.assertRaisesRegex(ValueError, "stationary"):
            BoundaryMotion("stationary", ("a",), ("a",), 1, CURRENT, DIGEST)
        self.assertEqual(row(state, move=motion(after=("a", "b"))).moving_total, 0)

    def test_dropped_expected_channels_and_wrong_coverage_context_fail(self):
        state = archive((record("a", 4), "active"))
        with self.assertRaisesRegex(ValueError, "selected channels"):
            row(state, coverage=coverage(cross=("a",)))
        with self.assertRaisesRegex(ValueError, "context differs"):
            row(state, coverage=coverage(cut=2))

    def test_same_crossing_shedding_and_alias_physical_content_fail(self):
        with self.assertRaisesRegex(ValueError, "duplicates crossing"):
            coverage(cross=("a",), shedding=("a",))
        state = archive((record("a", 4, physical_id="one"), "active"),
                        (record("b", 4, physical_id="one"), "routed"))
        with self.assertRaisesRegex(ValueError, "duplicate physical"):
            row(state, cross=("a",), shedding=("b",))
        with self.assertRaisesRegex(ValueError, "missing source"):
            row(state, cross=("missing",))

    def test_repeated_projection_occurrences_fail_but_fresh_history_projections_pass(self):
        first = archive((record("a:1", 4), "active"))
        one = row(first, cross=("a:1",))
        with self.assertRaisesRegex(ValueError, "repeated across slices"):
            accumulate_current((one, row(first, 2, cross=("a:1",))))
        second = commit_archive(first, 2, ((record("a:2", -4, cut=2), "active"),),
                                (DispositionChange("a:1", "active", "inactive", "past projection"),))
        two = row(second, 2, cross=("a:2",))
        self.assertEqual(accumulate_current((one, two))["current"], 0)
        self.assertEqual(one.archive.commits[0].additions[0][0].source_digest,
                         two.archive.commits[-1].additions[0][0].source_digest)

    def test_motion_discontinuity_and_archive_rewrite_across_slices_fail(self):
        first = row(ArchiveState(), move=motion(after=("a", "b")))
        second = row(ArchiveState(), 2, move=motion(before=("b",)))
        with self.assertRaisesRegex(ValueError, "disconnected"):
            accumulate_current((first, second))
        original = archive((record("a", 1), "inactive"))
        rewritten = archive((record("a", 9), "inactive"))
        with self.assertRaisesRegex(ValueError, "dropped or rewritten"):
            accumulate_current((row(original), row(rewritten, 2)))

    def test_forged_consistent_totals_float_totals_and_dropped_ids_fail(self):
        state = archive((record("a", 4), "active"))
        original = row(state, cross=("a",))
        for forged in (replace(original, cross_total=F(99), moving_total=F(99)),
                       replace(original, cross_total=4.0),
                       replace(original, physical_ids=()),
                       replace(original, cross=(), cross_total=F(0), moving_total=F(0))):
            with self.subTest(forged=forged):
                with self.assertRaises(ValueError):
                    accumulate_current((forged,))

    def test_incompatible_boundary_or_units_fail(self):
        first = row(ArchiveState())
        with self.assertRaisesRegex(ValueError, "incompatible boundary"):
            accumulate_current((first, replace(row(ArchiveState(), 2), boundary_id="different")))
        pressure = archive((record("pressure", 12, unit="path-pressure"), "active"))
        conversion = ScaleMap("pressure", 0, CURRENT, F(1, 3), DIGEST)
        with self.assertRaisesRegex(ValueError, "quantity domains"):
            row(pressure, cross=("pressure",), scale_maps=(conversion,))

    def test_nested_support_requires_ancestry_and_cannot_be_summed_twice(self):
        first = archive((record("inner", 3, support_ids=("physical",)), "active"))
        outer = record("outer", 1, cut=2, scale=1, support_ids=("physical",), parent_ids=("inner",))
        second = commit_archive(first, 2, ((outer, "active"),))
        conversion = ScaleMap("outer", 0, CURRENT, 3, DIGEST)
        self.assertEqual(row(second, 2, cross=("outer",), scale_maps=(conversion,)).moving_total, 3)
        with self.assertRaisesRegex(ValueError, "overlapping"):
            row(second, 2, cross=("inner", "outer"), scale_maps=(conversion,))
        with self.assertRaisesRegex(ValueError, "unconverted scale"):
            row(second, 2, cross=("outer",))
        with self.assertRaisesRegex(ValueError, "missing parent"):
            archive((replace(outer, committed=1), "active"))
        with self.assertRaisesRegex(ValueError, "parent union"):
            commit_archive(first, 2, ((replace(outer, support_ids=("physical", "unbound")), "active"),))
        with self.assertRaisesRegex(ValueError, "support occurrence repeated"):
            accumulate_current((row(first, cross=("inner",)),
                                row(second, 2, cross=("outer",), scale_maps=(conversion,))))

    def test_window_accumulation_requires_same_coverage_universe_and_domain(self):
        first = row(ArchiveState())
        for different in (coverage(2, domain="attempt-count"), coverage(2, scope="different channel family")):
            with self.assertRaisesRegex(ValueError, "coverage universe or count domain"):
                accumulate_current((first, row(ArchiveState(), 2, coverage=different)))
        changed_window = row(ArchiveState(), 2, coverage=coverage(2, window=6))
        self.assertEqual(accumulate_current((first, changed_window))["current"], 0)

    def test_window_preserves_duration_and_has_separate_ttl_domain(self):
        result = project_window(3, 7, 4, "native-return", "native-return", ttl=2,
                                retention_window=3, ttl_domain="seat-cut", retention_domain="seat-cut")
        self.assertEqual((result["RD"], result["RCD_duration"], result["participation"], result["seat_retained"]),
                         (3, 7, 4, False))
        with self.assertRaisesRegex(ValueError, "incompatible RCD"):
            project_window(3, 7, 4, "native-return", "attempt")
        with self.assertRaises(ValueError):
            project_window(3, 7, 4, "native-return", "native-return", ttl=2)
        for bad in (True, 2.0):
            with self.assertRaises(ValueError):
                project_window(3, 7, bad, "native-return", "native-return")

    def test_exact_values_and_provenance_are_required(self):
        for bad in (True, 4.0):
            with self.assertRaises(ValueError):
                record("a", bad)
        with self.assertRaisesRegex(ValueError, "SHA256"):
            record("a", 4, source_digest="unbound")


class RoutingTests(unittest.TestCase):
    def fixture(self):
        state = archive((record("surplus", 5, unit=BURDEN), "routed"))
        route = RouteRecord("route", "surplus", 1, 5, BURDEN,
                            (RouteLeg("retained", "exoshedding", 3, "outer"),
                             RouteLeg("loss", "open_loss", 2, None)), DIGEST)
        return state, route

    def test_partition_pending_destination_and_one_later_consumption(self):
        state, route = self.fixture()
        ledger = register_route(RouteLedger(), state, route)
        self.assertEqual([(p["destination"], p["amount"]) for p in pending_routes(ledger)], [("outer", 3)])
        received = receive_route(ledger, "route", "retained", 2, "outer", "receipt", DIGEST)
        self.assertEqual(pending_routes(received), ())
        self.assertEqual(received.routes, ledger.routes)
        self.assertTrue(validate_route_successor(ledger, received))
        with self.assertRaisesRegex(ValueError, "more than once"):
            receive_route(received, "route", "retained", 3, "outer", "second-receipt", DIGEST)
        with self.assertRaisesRegex(ValueError, "dropped or rewritten"):
            validate_route_successor(received, ledger)

    def test_same_cut_wrong_destination_and_open_loss_receipts_fail(self):
        state, route = self.fixture()
        ledger = register_route(RouteLedger(), state, route)
        with self.assertRaisesRegex(ValueError, "must follow"):
            receive_route(ledger, "route", "retained", 1, "outer", "receipt", DIGEST)
        with self.assertRaisesRegex(ValueError, "destination differs"):
            receive_route(ledger, "route", "retained", 2, "wrong", "receipt", DIGEST)
        with self.assertRaises(ValueError):
            receive_route(ledger, "route", "loss", 2, None, "receipt", DIGEST)

    def test_route_partition_and_destination_cannot_disappear(self):
        _, route = self.fixture()
        with self.assertRaisesRegex(ValueError, "partition"):
            replace(route, surplus=6)
        with self.assertRaisesRegex(ValueError, "pending destination"):
            RouteLeg("lost", "exoshedding", 1, None)

    def test_unbound_or_changed_route_source_fails(self):
        state, route = self.fixture()
        with self.assertRaisesRegex(ValueError, "immutable source archive"):
            pending_routes(RouteLedger((route,), ()))
        with self.assertRaisesRegex(ValueError, "missing route source"):
            register_route(RouteLedger(), ArchiveState(), route)
        with self.assertRaisesRegex(ValueError, "domain mismatch"):
            register_route(RouteLedger(), state, replace(route, unit=CURRENT))
        ledger = register_route(RouteLedger(), state, route)
        wrong = archive((record("surplus", 7, unit=BURDEN), "routed"))
        with self.assertRaisesRegex(ValueError, "domain mismatch"):
            pending_routes(replace(ledger, source_archives=(wrong,)))

    def test_same_physical_or_support_surplus_cannot_route_twice_under_aliases(self):
        for alias in (dict(physical_id="same", support_ids=("second",)),
                      dict(physical_id="other", support_ids=("support",))):
            first = record("r1", 5, unit=BURDEN, physical_id="same", support_ids=("support",))
            second = record("r2", 5, unit=BURDEN, **alias)
            state = archive((first, "routed"), (second, "routed"))
            r1 = RouteRecord("route1", "r1", 1, 5, BURDEN, (RouteLeg("leg1", "exoshedding", 5, "outer"),), DIGEST)
            r2 = RouteRecord("route2", "r2", 1, 5, BURDEN, (RouteLeg("leg2", "exoshedding", 5, "outer"),), DIGEST)
            with self.subTest(alias=alias):
                ledger = register_route(RouteLedger(), state, r1)
                with self.assertRaisesRegex(ValueError, "routed more than once"):
                    register_route(ledger, state, r2)


if __name__ == "__main__":
    unittest.main()
