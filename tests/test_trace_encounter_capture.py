"""Capture every actual finite encounter update, including pending histories."""
import hashlib
import gzip
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from simulations import proton_accounting as proton
from simulations import retained_continuation as encounter
from simulations.trace_store import canonical_bytes


class EncounterCaptureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.plain_world, cls.plain_report = proton.encounter_source()
        cls.events = []
        with proton.observe_commits(lambda kind, payload: cls.events.append((kind, payload))) as observed:
            cls.world, cls.report = proton.encounter_source()
        cls.observed = observed
        cls.captured = [(kind, payload) for kind, payload in cls.events
                        if kind.startswith("particle.encounter_")]
        cls.initial = cls.captured[0][1]
        cls.updates = [payload for kind, payload in cls.captured if kind == "particle.encounter_update"]
        cls.finished = cls.captured[-1][1]

    def test_passive_capture_preserves_exact_model_and_report_bytes(self):
        self.assertEqual(proton.packed(self.world), proton.packed(self.plain_world))
        self.assertEqual(proton.packed(self.report), proton.packed(self.plain_report))
        self.assertEqual(self.observed.failure_count, 0)

    def test_init_six_real_updates_and_terminal_outcome_are_distinct(self):
        self.assertEqual([kind for kind, payload in self.captured],
            ["particle.encounter_init"] + ["particle.encounter_update"] * 6 + ["particle.encounter_finish"])
        self.assertEqual(self.initial["requested_steps"], 6)
        self.assertEqual(self.initial["state_fields"], list(encounter.STATE_FIELDS))
        self.assertEqual(self.initial["ticket_denominator"], encounter.TICKET_DENOMINATOR)
        self.assertEqual(self.initial["state"], list(encounter.initial_state()))
        self.assertEqual(self.initial["world_digest"], self.report["preparation_digest"])
        self.assertEqual(self.finished["requested_steps"], 6)
        self.assertEqual(self.finished["completed_attempts"], len(self.world.attempts))
        self.assertEqual(self.finished["final_state"], list(self.world.state))
        self.assertEqual(self.finished["final_world_digest"], proton.digest(self.world))
        self.assertEqual(sum(kind == "particle.native_return" for kind, payload in self.events), 15)

    def test_each_actual_attempt_preserves_operands_selection_and_digest_chain(self):
        previous = self.initial["world_digest"]
        for index, (captured, actual, report) in enumerate(zip(self.updates, self.world.attempts, self.report["rows"])):
            with self.subTest(index=index):
                self.assertEqual(captured["index"], index)
                self.assertEqual(captured["masks"], list(actual.masks))
                self.assertEqual(captured["tickets"], list(actual.tickets))
                self.assertEqual(captured["attempt"], proton.encode(actual))
                self.assertEqual(captured["before_state"], list(report["before_state"]))
                self.assertEqual(captured["after_state"], list(report["after_state"]))
                self.assertEqual(captured["before_world_digest"], previous)
                self.assertEqual(captured["after_world_digest"], report["archive_digest"])
                self.assertEqual(captured["after_state"], captured["attempt"]["row"]["next_state"])
                previous = captured["after_world_digest"]
        self.assertEqual(previous, self.finished["final_world_digest"])

    def test_pending_updates_are_not_erased_or_counted_as_returns(self):
        self.assertEqual([row["pending_carriers"] for row in self.updates], [[], [0], [0], [], [], []])
        for index in (1, 2):
            row = self.updates[index]
            before, after = row["before_history"]["population"], row["after_history"]["population"]
            self.assertEqual(before[0]["return_ids"], after[0]["return_ids"])
            self.assertEqual(len(after[1]["return_ids"]), len(before[1]["return_ids"]) + 1)
            self.assertIsNone(row["attempt"]["selected_opportunities"][0])
            self.assertEqual(row["after_state"][0], row["before_state"][0] + 1)

    def test_expired_relations_survive_with_inactive_dispositions_and_reentry(self):
        first = self.updates[0]["after_history"]["relations"]
        expired = self.updates[2]["after_history"]["relations"]
        reentry = self.updates[3]["after_history"]
        later = self.updates[4]["after_history"]["relations"]
        self.assertEqual([row["identity"] for row in first], [row["identity"] for row in expired])
        self.assertTrue(all(row["active"] for row in first))
        self.assertTrue(all(not row["active"] for row in expired))
        self.assertTrue(all(a["tokens_digest"] == b["tokens_digest"] for a, b in zip(first, expired)))
        self.assertEqual(len(reentry["contacts"]), len(self.updates[2]["after_history"]["contacts"]) + 1)
        self.assertEqual([row["identity"] for row in reentry["relations"]], [row["identity"] for row in first])
        self.assertEqual([row["created_tick"] for row in later], [0, 0, 4, 4])
        self.assertEqual([row["active"] for row in later], [False, False, True, True])
        final_ids = {row["identity"] for row in self.finished["history"]["relations"]}
        self.assertTrue({row["identity"] for row in first} <= final_ids)

    def test_context_source_binding_and_default_record_budget(self):
        expected = {"simulations/retained_continuation.py": hashlib.sha256(Path(encounter.__file__).read_bytes()).hexdigest(),
            "simulations/vendor/aod_manual_native.py": hashlib.sha256(Path(proton.native.__file__).read_bytes()).hexdigest()}
        for kind, payload in self.captured:
            with self.subTest(kind=kind, index=payload.get("index")):
                self.assertEqual(payload["source_bindings"], expected)
                self.assertEqual((payload["scope"], payload["lineage"], payload["scale"]),
                                 (encounter.SCOPE, "retained-continuation-world", 0))
                self.assertEqual(payload["source_core"], "1:2")
                self.assertEqual(payload["population_role"], "source encounter before proton materialization")
                self.assertLess(len(canonical_bytes(payload)), 262144)


class EncounterCaptureIsolationTests(unittest.TestCase):
    def test_snapshot_builder_failure_does_not_change_model(self):
        plain_world, plain_report = proton.encounter_source()
        with proton.observe_commits(lambda kind, payload: None) as observed:
            with patch.object(proton, "_encounter_history", side_effect=OSError("snapshot unavailable")):
                world, report = proton.encounter_source()
        self.assertEqual(proton.packed(world), proton.packed(plain_world))
        self.assertEqual(proton.packed(report), proton.packed(plain_report))
        self.assertEqual(observed.failure_count, 1)
        self.assertEqual(observed.last_error, "OSError: snapshot unavailable")

    def test_callback_failure_does_not_replace_original_model_error(self):
        def broken(kind, payload):
            raise OSError("trace full")
        with proton.observe_commits(broken) as observed:
            with patch.object(encounter, "advance_world", side_effect=ValueError("original update failure")):
                with self.assertRaisesRegex(ValueError, "original update failure"):
                    proton.encounter_source()
        self.assertEqual(observed.failure_count, 1)
        self.assertEqual(observed.last_error, "OSError: trace full")

    def test_mutating_callback_does_not_modify_actual_world_or_report(self):
        plain_world, plain_report = proton.encounter_source()
        def mutate(kind, payload):
            if kind == "particle.encounter_update":
                payload["after_state"][0] = 999
                payload["attempt"]["row"]["completed"][0] = True
                payload["after_history"]["population"][0]["return_ids"].clear()
        with proton.observe_commits(mutate) as observed:
            world, report = proton.encounter_source()
        self.assertEqual(proton.packed(world), proton.packed(plain_world))
        self.assertEqual(proton.packed(report), proton.packed(plain_report))
        self.assertEqual(observed.failure_count, 0)


class RoutingTerminalCaptureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cls.root = Path(cls.tmp.name)
        cls.events = []
        with proton.observe_commits(lambda kind, payload: cls.events.append((kind, payload))) as observed:
            proton.run(cls.root / "proton.json")
        if observed.failure_count:
            raise RuntimeError(observed.last_error)
        cls.report = json.loads((cls.root / "proton.json").read_text())
        cls.witness = json.loads(gzip.decompress((cls.root / "proton-witnesses.json.gz").read_bytes()))
        cls.terminals = [payload for kind, payload in cls.events if kind == "particle.routing_finish"]

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def test_every_alternative_has_its_own_bound_terminal(self):
        self.assertEqual(len(self.terminals), len(self.report["rows"]))
        for index, (terminal, row) in enumerate(zip(self.terminals, self.report["rows"])):
            with self.subTest(index=index):
                self.assertEqual(terminal["schema"], "native-routing-finish-v1")
                self.assertEqual(terminal["alternative_index"], index)
                self.assertEqual(terminal["alternative_count"], len(self.report["rows"]))
                self.assertEqual(terminal["participation_window"], row["window"])
                self.assertEqual(terminal["cut"], row["cut"])
                self.assertEqual(terminal["source_native_shedding_digest"], row["native_shedding"]["record_digest"])
                self.assertEqual(terminal["pending_routes"], row["pending_after_receipt"])
                self.assertEqual(terminal["occurrence_semantics"], "non-additive alternative views")
                self.assertLess(len(canonical_bytes(terminal)), 262144)

    def test_routing_plan_binds_actual_inputs_before_alternative_execution(self):
        plans = [(index, payload) for index, (kind, payload) in enumerate(self.events)
                 if kind == "particle.routing_plan"]
        self.assertEqual(len(plans), 1)
        index, plan = plans[0]
        first_alternative = next(i for i, (kind, payload) in enumerate(self.events)
                                 if kind == "particle.native_shedding")
        self.assertLess(index, first_alternative)
        self.assertEqual(plan["schema"], "native-routing-plan-v1")
        self.assertEqual(plan["windows"], self.report["choices"]["windows"])
        self.assertEqual(plan["capacity"], self.report["choices"]["capacity"])
        self.assertEqual(plan["closure_cost"], self.report["choices"]["closure_cost"])
        self.assertEqual(plan["source_history_digest"], self.report["fixed_history_sha256"])
        self.assertEqual(plan["source_bindings"], self.report["source_sha256"])
        self.assertEqual(plan["outer_return_id"], self.witness["outer_fold"]["outer"][1]["return_id"])
        self.assertEqual((plan["burden_unit"], plan["current_unit"]),
                         (proton.BURDEN_UNIT, proton.CURRENT_UNIT))
        self.assertEqual([row["participation_window"] for row in self.terminals], plan["windows"])
        self.assertEqual([row["alternative_index"] for row in self.terminals], list(range(len(plan["windows"]))))
        self.assertTrue(all(row["alternative_count"] == len(plan["windows"]) for row in self.terminals))
        self.assertNotIn("expected_signature", plan)
        self.assertNotIn("expected_surplus", plan)
        self.assertLess(len(canonical_bytes(plan)), 262144)

    def test_terminal_records_retain_actual_routes_and_receipts(self):
        self.assertEqual([len(row["routes"]) for row in self.terminals], [0, 0, 0, 1])
        self.assertEqual([len(row["receipts"]) for row in self.terminals], [0, 0, 0, 1])
        self.assertEqual(self.terminals[-1]["routes"], self.witness["route_ledger"]["routes"])
        self.assertEqual(self.terminals[-1]["receipts"], self.witness["route_ledger"]["receipts"])
        self.assertEqual(self.terminals[-1]["receipts"][0]["cut"], self.report["rows"][-1]["receipt"]["cut"])


if __name__ == "__main__":
    unittest.main()
