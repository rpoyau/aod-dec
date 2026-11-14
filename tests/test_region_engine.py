"""Adversarial accounting tests over explicit finite current inputs."""
from dataclasses import replace
from fractions import Fraction as F
import copy
import json
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "simulations"))
import region_engine as r

P = "a" * 64
Q = "b" * 64
UNIT = "native-current"


def base():
    return r.Ledger(("core", "water", "surface", "moon", "outside"), UNIT, 0,
                    "declared finite interface", "completed-return")


def channel(identity="x", source="core", target="water", value=F(4), **kwargs):
    return r.Channel(identity, "physical-" + identity, source, target, value, P, **kwargs)


def stationary(ledger, cells, name="region", representation=None, scope="declared-current"):
    region = r.Region(name, tuple(cells))
    frames = tuple(r.Frame(region, region, "stationary") for _ in ledger.commits)
    return r.audit_boundary(ledger, name, frames, scope=scope, representation=representation)


class RegionTests(unittest.TestCase):
    def test_internal_transfer_retained_and_union_cancels(self):
        ledger = r.commit(base(), 1, 4, (channel(),))
        a = stationary(ledger, ("core",), "a")
        b = stationary(ledger, ("water",), "b")
        parent = stationary(ledger, ("core", "water"), "union")
        self.assertEqual((a.total["current"], b.total["current"], parent.total["current"]), (4, -4, 0))
        self.assertEqual(parent.rows[0]["internal_ids"], ("x",))
        self.assertEqual(len(r.accounting.archive_view(parent.slices[0].archive)[0]), 1)
        self.assertEqual(r.audit_partition(parent, (a, b))["total_current_residual"], 0)

    def test_shell_difference_and_internal_layer(self):
        earth = r.Region("earth", ("core",))
        outer = r.Region("earth-water", ("core", "water"))
        shell = r.region_difference("shell", outer, earth)
        self.assertEqual(shell.cells, ("water",))
        self.assertEqual(set(r.region_union("whole", earth, shell).cells), set(outer.cells))
        ledger = r.commit(base(), 2, 4, (channel(), channel("y", "water", "moon", F(7))))
        result = stationary(ledger, shell.cells)
        self.assertEqual(result.total["cross"], F(3))

    def test_noncontained_shell_rejected(self):
        with self.assertRaisesRegex(ValueError, "not contained"):
            r.region_difference("bad", r.Region("outer", ("core",)), r.Region("inner", ("moon",)))

    def test_moving_shared_field_and_complement_cancel(self):
        sample = r.SweepSample("surface", F(3, 2), UNIT, 0, Q)
        ledger = r.commit(base(), 1, 4, (channel(),), sweep_field=(sample,))
        a0, a1 = r.Region("a0", ("core",)), r.Region("a1", ("core", "surface"))
        b0, b1 = r.Region("b0", ("water", "surface", "moon", "outside")), r.Region("b1", ("water", "moon", "outside"))
        a = r.audit_boundary(ledger, "a", (r.Frame(a0, a1, "moving", (sample,)),))
        b = r.audit_boundary(ledger, "b", (r.Frame(b0, b1, "moving", (sample,)),))
        whole = stationary(ledger, ledger.cells, "all")
        self.assertEqual((a.total["current"], b.total["current"]), (F(5, 2), F(-5, 2)))
        self.assertEqual(r.audit_partition(whole, (a, b))["rows"][0]["sweep_residual"], 0)

    def test_sweep_omission_rejected(self):
        with self.assertRaisesRegex(ValueError, "sweep coverage"):
            r.Frame(r.Region("old", ("core",)), r.Region("new", ("core", "water")), "moving")

    def test_forged_sweep_field_rejected_even_same_total(self):
        sample = r.SweepSample("surface", F(3), UNIT, 0, P)
        ledger = r.commit(base(), 1, 4, (), sweep_field=(sample,))
        frame = r.Frame(r.Region("old", ("core",)), r.Region("new", ("core", "surface")), "moving", (replace(sample, provenance=Q),))
        with self.assertRaisesRegex(ValueError, "same-cut committed field"):
            r.audit_boundary(ledger, "bad", (frame,))

    def test_sweep_unit_and_scale_rejected(self):
        for sample in (r.SweepSample("water", 3, "closure-burden", 0, P),
                       r.SweepSample("water", 3, UNIT, 1, P)):
            with self.assertRaisesRegex(ValueError, "domain or scale mismatch"):
                r.commit(base(), 1, 4, (), sweep_field=(sample,))

    def test_crossing_convention_is_after_membership(self):
        sample = r.SweepSample("water", F(1), UNIT, 0, P)
        ledger = r.commit(base(), 1, 4, (channel(),), sweep_field=(sample,))
        frame = r.Frame(r.Region("before", ("core",)), r.Region("after", ("core", "water")), "moving", (sample,))
        view = r.audit_boundary(ledger, "moving", (frame,))
        self.assertEqual((view.total["cross"], view.total["sweep"], view.total["current"]), (0, 1, -1))
        self.assertEqual(view.rows[0]["internal_ids"], ("x",))

    def test_disconnected_motion_schedule_rejected(self):
        ledger = r.commit(r.commit(base(), 1, 4, (channel(),)), 2, 4, ())
        a, b = r.Region("a", ("core",)), r.Region("b", ("moon",))
        with self.assertRaisesRegex(ValueError, "disconnected"):
            r.audit_boundary(ledger, "bad", (r.Frame(a, a, "stationary"), r.Frame(b, b, "stationary")))

    def test_gate_dropout_reentry_preserves_every_occurrence(self):
        ledger = base()
        for cut, enabled in enumerate((True, False, True), 1):
            ledger = r.commit(ledger, cut, 4, (channel(str(cut), available=enabled),))
        view = stationary(ledger, ("core",))
        self.assertEqual(tuple(row["current"] for row in view.rows), (4, 0, 4))
        self.assertEqual(view.total["current"], 8)
        self.assertEqual(len(view.ledger.commits), 3)
        records, statuses, _ = r.accounting.archive_view(view.slices[-1].archive)
        self.assertEqual(len(records), 3)
        self.assertEqual(statuses["2"], "inactive")
        self.assertEqual(records["2"].value, 4)

    def test_duplicate_physical_occurrence_rejected(self):
        ledger = r.commit(base(), 1, 4, (channel(),))
        with self.assertRaisesRegex(ValueError, "duplicate identity or physical"):
            r.commit(ledger, 2, 4, (replace(channel(), identity="alias"),))

    def test_omitted_committed_event_rejected(self):
        ledger = r.commit(base(), 1, 4, (channel(), channel("two")))
        with self.assertRaisesRegex(ValueError, "snapshot was altered or omitted"):
            replace(ledger, commits=(replace(ledger.commits[0], channels=(channel(),)),))

    def test_rebuilt_truncated_history_is_not_successor(self):
        old = r.commit(base(), 1, 4, (channel(),))
        changed = r.commit(base(), 1, 4, ())
        with self.assertRaisesRegex(ValueError, "dropped or rewritten"):
            r.validate_successor(old, changed)

    def test_skipped_cut_frame_rejected(self):
        ledger = r.commit(r.commit(base(), 1, 4, (channel(),)), 2, 4, ())
        reg = r.Region("core", ("core",))
        with self.assertRaisesRegex(ValueError, "every committed input cut"):
            r.audit_boundary(ledger, "core", (r.Frame(reg, reg, "stationary"),))

    def test_tampered_cached_current_rejected(self):
        view = stationary(r.commit(base(), 1, 4, (channel(),)), ("core",))
        forged = copy.deepcopy(view)
        forged.rows[0]["current"] = F(0)
        with self.assertRaisesRegex(ValueError, "reconstruction"):
            r.validate_audit(forged)

    def test_partition_overlap_rejected(self):
        ledger = r.commit(base(), 1, 4, (channel(),))
        a = stationary(ledger, ("core",), "a")
        parent = stationary(ledger, ("core",), "parent")
        with self.assertRaisesRegex(ValueError, "overlap"):
            r.audit_partition(parent, (a, a))

    def test_float_and_bool_cached_values_rejected_despite_numeric_equality(self):
        view = stationary(r.commit(base(), 1, 4, (channel(value=F(1)),)), ("core",))
        for forged_value in (1.0, True):
            for target in ("total", "row", "view"):
                forged = copy.deepcopy(view)
                if target == "total":
                    forged.total["current"] = forged_value
                elif target == "row":
                    forged.rows[0]["current"] = forged_value
                else:
                    forged.rows[0]["views"][0]["projected_value"] = forged_value
                with self.assertRaisesRegex(ValueError, "exact committed source reconstruction"):
                    r.validate_audit(forged)

    def test_coverage_claim_is_relative_to_declared_family(self):
        ledger = r.commit(base(), 1, 4, ())
        view = stationary(ledger, ("core",), scope="full")
        self.assertIn("external producer completeness is not certified", view.coverage_claim)
        self.assertEqual(view.total["current"], 0)

    def test_partition_different_occurrence_family_rejected(self):
        one = r.commit(base(), 1, 4, (channel(),))
        two = r.commit(base(), 1, 4, (channel("independent"),))
        with self.assertRaisesRegex(ValueError, "occurrence family"):
            r.audit_partition(stationary(one, ("core",)), (stationary(two, ("core",)),))

    def test_independent_reverse_channel_is_not_bookkeeping_negation(self):
        ledger = r.commit(base(), 1, 4, (channel(), channel("reverse", "water", "core", F(5))))
        a = stationary(ledger, ("core",), "a")
        b = stationary(ledger, ("water",), "b")
        parent = stationary(ledger, ("core", "water"), "parent")
        self.assertEqual((a.total["current"], b.total["current"]), (-1, 1))
        self.assertEqual(len(parent.rows[0]["internal_ids"]), 2)
        self.assertEqual(r.audit_partition(parent, (a, b))["status"], "PASS")

    def test_exact_fraction_and_explicit_representation(self):
        ledger = r.commit(base(), 1, 4, (channel(value=F(1, 3)),))
        mapping = r.RepresentationMap(0, 2, UNIT, F(3, 7), P)
        view = stationary(ledger, ("core",), representation=mapping)
        self.assertEqual(view.total["current"], F(1, 7))
        self.assertEqual(view.total["scale"], 2)
        self.assertEqual(view.slices[0].archive.commits[0].additions[0][0].scale, 0)

    def test_representation_maps_sweep_and_channel_together(self):
        sample = r.SweepSample("surface", F(3), UNIT, 0, P)
        ledger = r.commit(base(), 1, 4, (channel(),), sweep_field=(sample,))
        frame = r.Frame(r.Region("old", ("core",)), r.Region("new", ("core", "surface")), "moving", (sample,))
        view = r.audit_boundary(ledger, "mapped", (frame,), representation=r.RepresentationMap(0, 1, UNIT, F(1, 2), P))
        self.assertEqual((view.total["cross"], view.total["sweep"], view.total["current"]), (2, F(3, 2), F(1, 2)))

    def test_wrong_quantity_or_missing_representation_rejected(self):
        ledger = r.commit(base(), 1, 4, (channel(),))
        with self.assertRaisesRegex(ValueError, "quantity unit differs"):
            stationary(ledger, ("core",), representation=r.RepresentationMap(0, 1, "metres", F(1), P))
        with self.assertRaisesRegex(ValueError, "ancestral-support bridge"):
            replace(base(), scale=1)

    def test_float_bool_and_negative_representation_rejected(self):
        for value in (0.5, True):
            with self.assertRaisesRegex(ValueError, "exact"):
                channel(value=value)
        with self.assertRaisesRegex(ValueError, "positive"):
            r.RepresentationMap(0, 1, UNIT, F(-1), P)

    def test_signed_shedding_is_separate_from_contact(self):
        ledger = r.commit(base(), 1, 4, (channel(), channel("shed", "core", "outside", F(2), kind="shedding")))
        view = stationary(ledger, ("core",))
        self.assertEqual((view.total["cross"], view.total["shedding"], view.total["current"]), (4, 2, 6))

    def test_unresolved_burden_never_silently_becomes_zero_current(self):
        obligation = r.PendingProjection("burden", "burden-physical", "core", "outside", F(6), "closure-burden", P)
        ledger = r.commit(base(), 1, 4, (channel(),), obligations=(obligation,))
        ledger = r.commit(ledger, 2, 4, ())
        view = stationary(ledger, ("core",))
        self.assertEqual(tuple(row["unresolved_projection_ids"] for row in view.rows), (("burden",), ("burden",)))
        with self.assertRaisesRegex(ValueError, "unresolved signed shedding"):
            stationary(ledger, ("core",), scope="full")

    def test_passive_audits_do_not_mutate_committed_ledger(self):
        ledger = r.commit(base(), 1, 4, (channel(),))
        before = r.fingerprint(ledger)
        stationary(ledger, ("core",)); stationary(ledger, ("water",)); stationary(ledger, ledger.cells)
        self.assertEqual(before, r.fingerprint(ledger))

    def test_internal_pending_burden_blocks_full_when_boundary_moves_across_it(self):
        obligation = r.PendingProjection("pending", "pending-physical", "core", "water", F(6), "closure-burden", P)
        sample = r.SweepSample("water", F(0), UNIT, 0, P)
        ledger = r.commit(base(), 1, 4, (), obligations=(obligation,))
        ledger = r.commit(ledger, 2, 4, (), sweep_field=(sample,))
        both, core = r.Region("both", ("core", "water")), r.Region("core", ("core",))
        frames = (r.Frame(both, both, "stationary"), r.Frame(both, core, "moving", (sample,)))
        view = r.audit_boundary(ledger, "moving", frames)
        self.assertEqual(tuple(row["unresolved_projection_ids"] for row in view.rows), ((), ("pending",)))
        with self.assertRaisesRegex(ValueError, "unresolved signed shedding"):
            r.audit_boundary(ledger, "moving", frames, scope="full")


if __name__ == "__main__":
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(RegionTests)
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    print(json.dumps({"status": "PASS" if result.wasSuccessful() else "FAIL",
                      "tests": result.testsRun, "failures": len(result.failures),
                      "errors": len(result.errors), "optimized": not __debug__}))
    raise SystemExit(0 if result.wasSuccessful() else 1)
