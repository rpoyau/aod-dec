"""Exact backend equivalence and hostile inputs at the table/ticket boundary."""
from dataclasses import replace
from fractions import Fraction as F
import os
import unittest

from simulations.chiral_batch import (TransitionTables, TicketBatch, UINT32_MAX,
    buffer_inputs, build_ticket_batch, compile_model_tables, result_record,
    run_multicore, run_scalar, validate_batch)
from simulations.chiral_collision import CollisionConfig, simulate_path
from simulations.tesseract_cells import FACETS


class ChiralBatchTests(unittest.TestCase):
    def test_model_adapter_and_independent_threshold_values(self):
        tables = compile_model_tables(CollisionConfig(), ticket_denominator=64)
        outer, core = FACETS.index("0***"), FACETS.index("*0**")
        self.assertEqual(tables.flip_thresholds[outer][core], 4)
        self.assertEqual(tables.flip_thresholds[core][outer], 16)
        self.assertEqual(tables.flip_thresholds[core][core], 28)
        self.assertEqual(tables.flip_thresholds[outer][outer], 19)
        self.assertEqual(tables.move_destinations[outer].count(outer), 2)
        self.assertNotIn(FACETS.index("1***"), tables.move_destinations[outer])
        self.assertEqual(len(set(tables.move_destinations[outer])), 7)

    def test_supplied_tickets_match_every_full_host_decision(self):
        for gain in (F(0), F(1, 4)):
            config = CollisionConfig(contact_gain=gain)
            tables = compile_model_tables(config, ticket_denominator=64)
            batch = build_ticket_batch(config, (7, 19, -2), ticket_denominator=64)
            result = run_scalar(tables, batch)
            for index, seed in enumerate(batch.seeds):
                path = simulate_path(config, seed=seed, ticket_denominator=64)
                for k, (actual, original) in enumerate(zip(result["rows"][index], path["steps"])):
                    after = original["after"]
                    self.assertEqual(actual, (FACETS.index(after.facet_a), FACETS.index(after.facet_b),
                        after.chi_a, after.chi_b, int(original["flipped"][0]),
                        int(original["flipped"][1]), int(original["support_contact"])))
                    self.assertEqual(batch.tickets[index][k],
                                     original["movement_tickets"] + original["flip_tickets"])

    def test_executor_uses_supplied_tables_without_intrinsic_core_rule(self):
        config = CollisionConfig(steps=3)
        batch = build_ticket_batch(config, (7,), ticket_denominator=64)
        stationary = tuple((facet,) * 8 for facet in range(8))
        zero = TransitionTables(stationary, ((0,) * 8,) * 8, 64)
        forced = replace(zero, flip_thresholds=((64,) * 8,) * 8)
        a = run_scalar(zero, batch)
        b = run_scalar(forced, batch)
        self.assertEqual(a["rows"][0], ((0, 1, 1, -1, 0, 0, 0),) * 3)
        self.assertEqual(b["rows"][0], ((0, 1, -1, 1, 1, 1, 0),
                                         (0, 1, 1, -1, 1, 1, 0),
                                         (0, 1, -1, 1, 1, 1, 0)))
        self.assertNotEqual(a["tables_sha256"], b["tables_sha256"])
        self.assertEqual(a["ticket_batch_sha256"], b["ticket_batch_sha256"])

    def test_multicore_preserves_order_and_uses_distinct_processes(self):
        config = CollisionConfig(steps=12, contact_gain=F(0))
        tables = compile_model_tables(config, ticket_denominator=64)
        batch = build_ticket_batch(config, range(24), ticket_denominator=64)
        baseline = run_scalar(tables, batch)
        for chunk in (1, 7):
            parallel = run_multicore(tables, batch, workers=2, chunksize=chunk)
            self.assertEqual(parallel["rows"], baseline["rows"])
            self.assertEqual(parallel["semantic_sha256"], baseline["semantic_sha256"])
            self.assertEqual(parallel["tables_sha256"], baseline["tables_sha256"])
            self.assertEqual(parallel["ticket_batch_sha256"], baseline["ticket_batch_sha256"])
            self.assertEqual(parallel["execution"]["start_method"], "spawn")
            self.assertEqual(parallel["execution"]["effective_workers"], 2)
            pids = parallel["execution"]["participating_worker_pids"]
            self.assertEqual(len(set(pids)), 2)
            self.assertNotIn(os.getpid(), pids)

    def test_zero_steps_and_empty_batches_do_not_claim_work(self):
        config = CollisionConfig(steps=0)
        tables = compile_model_tables(config)
        empty = build_ticket_batch(config, ())
        result = run_multicore(tables, empty, workers=4)
        self.assertEqual(result["rows"], ())
        self.assertEqual(result["execution"]["effective_workers"], 0)
        self.assertEqual(result["execution"]["participating_worker_pids"], ())
        nonempty = build_ticket_batch(config, (1, 2))
        self.assertEqual(run_scalar(tables, nonempty)["rows"], ((), ()))
        self.assertEqual(buffer_inputs(tables, empty)["tickets"], ())

    def test_exact_uint32_edge_and_strict_threshold_inequality(self):
        stationary = tuple((facet,) * 8 for facet in range(8))
        tables = TransitionTables(stationary, ((UINT32_MAX,) * 8,) * 8, UINT32_MAX)
        batch = TicketBatch(1, UINT32_MAX, ((0, 1, 1, -1),),
                            (((0, 0, UINT32_MAX - 1, UINT32_MAX - 1),),))
        self.assertEqual(run_scalar(tables, batch)["rows"][0][0][4:6], (1, 1))
        equal = replace(tables, flip_thresholds=((UINT32_MAX - 1,) * 8,) * 8)
        self.assertEqual(run_scalar(equal, batch)["rows"][0][0][4:6], (0, 0))
        with self.assertRaises(ValueError):
            compile_model_tables(CollisionConfig(q_outer=F(1, 1 << 32)),
                                 ticket_denominator=1 << 32)

    def test_hostile_aliases_and_inconsistent_buffers_are_rejected(self):
        cfg = CollisionConfig(steps=1)
        tables = compile_model_tables(cfg)
        batch = build_ticket_batch(cfg, (7,))
        for value in (True, 64.0, F(64), 0, UINT32_MAX + 1):
            with self.subTest(value=value), self.assertRaises(ValueError):
                replace(tables, ticket_denominator=value)
        for fields in ({"steps": True}, {"initial_states": ((0, 1, True, -1),)},
                       {"tickets": (((8, 0, 0, 0),),)},
                       {"tickets": (((0, 0, 64, 0),),)},
                       {"tickets": (((0, 0, False, 0),),)},
                       {"tickets": (((0, 0, 1.0, 0),),)},
                       {"tickets": ((),)}, {"seeds": (True,)}):
            with self.subTest(fields=fields), self.assertRaises(ValueError):
                replace(batch, **fields)
        with self.assertRaises(ValueError):
            validate_batch(tables, replace(batch, ticket_denominator=128))
        with self.assertRaises(ValueError):
            build_ticket_batch(cfg, (True,))
        with self.assertRaises(ValueError):
            compile_model_tables(cfg, ticket_denominator=16)
        for kwargs in ({"workers": True}, {"workers": 0}, {"chunksize": 0}):
            with self.subTest(kwargs=kwargs), self.assertRaises(ValueError):
                run_multicore(tables, batch, **kwargs)

    def test_hashes_bind_tickets_and_rows_but_exclude_execution_metadata(self):
        cfg = CollisionConfig(steps=1)
        tables = compile_model_tables(cfg)
        batch = build_ticket_batch(cfg, (7,))
        a = run_scalar(tables, batch)
        b = result_record(tables, batch, a["rows"], "other-backend", {"pid": 1234})
        self.assertEqual(a["semantic_sha256"], b["semantic_sha256"])
        self.assertEqual(a["status"], "EXECUTED")
        old = batch.tickets[0][0]
        changed = replace(batch, tickets=(((old[0], old[1], (old[2] + 1) % 64, old[3]),),))
        c = run_scalar(tables, changed)
        self.assertNotEqual(a["ticket_batch_sha256"], c["ticket_batch_sha256"])
        self.assertNotEqual(a["semantic_sha256"], c["semantic_sha256"])
        row = a["rows"][0][0]
        with self.assertRaises(ValueError):
            result_record(tables, batch, ((row[:4] + (True,) + row[5:],),), "forged", {})


if __name__ == "__main__":
    unittest.main()
