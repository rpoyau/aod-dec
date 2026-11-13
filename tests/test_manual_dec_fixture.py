"""Exact independent Manual values and hostile retained-history checks."""
from dataclasses import replace
from fractions import Fraction as F
import json
import unittest

from simulations.manual_dec_fixture import (bound_contacts, build_fixture, channel,
    exact_data, history_reduction_check, ledger_kernel, make_population, source_binding)
from simulations.vendor import aod_manual_native as n


class ManualDecFixtureTests(unittest.TestCase):
    def test_source_binding_and_json_exactness(self):
        binding = source_binding()
        self.assertEqual(binding["sha256"], "3ba98c7097bc104aeb6218265b8531f94f2ca5a35239d0df097aba096675dfdf")
        record = build_fixture()
        self.assertEqual(json.loads(json.dumps(record)), record)
        with self.assertRaises(TypeError):
            exact_data(0.5)

    def test_manual_kernel_values_and_blocked_fibre(self):
        rows = build_fixture()["dec_kernel"]
        self.assertEqual([r["probability"] for r in rows["one_node"]["rows"]],
                         ["1/8", "1/8", "1/2", "1/4"])
        self.assertEqual([r["probability"] for r in rows["slide"]["rows"]],
                         ["3/7", "3/7", "1/7"])
        self.assertEqual(rows["blocked_additional_dwell"]["admissible"], 0)
        self.assertEqual(rows["blocked_additional_dwell"]["retained_structural_hinge"]["atomic_traversals"], 0)
        with self.assertRaises(n.AdmissionError):
            n.fibre_step(1, -1, "leave_hinge")

    def test_zero_weight_is_not_zero_admissibility(self):
        admitted = ledger_kernel((("e1", 1, 1, 1, 1), ("e2", 2, 0, 1, 0)))
        blocked = ledger_kernel((("e1", 1, 1, 1, 1), ("e2", 2, 0, 0, 0)))
        self.assertEqual([r["probability"] for r in admitted["rows"]], [F(1), F(0)])
        self.assertEqual([r["probability"] for r in blocked["rows"]], [F(1), F(0)])
        self.assertEqual((admitted["structural_rows"], blocked["structural_rows"]), (2, 1))
        self.assertEqual(ledger_kernel((("e1", 1, 1, 1, 0),))["status"], "EMPTY_POSITIVE_WEIGHT_SUPPORT")
        self.assertEqual(ledger_kernel((("e1", 1, 1, 0, 0),))["status"], "EMPTY_ADMISSIBLE_SUPPORT")
        with self.assertRaisesRegex(n.AdmissionError, "aliases"):
            ledger_kernel((("e1", 1, 1, 1, 1), ("alias-e1", 1, 1, 1, 1)))

    def test_population_aliases_and_density_types(self):
        population = build_fixture()["population"]
        self.assertEqual(population["population_count"], 4)
        self.assertEqual(population["cell_population_counts"], {"cell-left": 2, "cell-right": 2})
        self.assertEqual(population["normalized_occupancy"], {"cell-left": "1/2", "cell-right": "1/2"})
        self.assertEqual(len(population["alias_field"]["members"]), 1)
        self.assertEqual(population["population_density"]["status"], "NOT_INSTANTIATED")
        one = n.certified(n.trace("single"))
        with self.assertRaises(n.AdmissionError):
            n.wave([one])

    def test_directional_rcd_and_clipping_preserve_support(self):
        a, b, _, _ = make_population()
        contacts = bound_contacts(a, b)
        direct = channel(a, b, contacts, 3, F(1, 3))
        reverse = channel(a, b, contacts, 3, F(-1, 3), reverse=True)
        self.assertEqual((direct["RD"], direct["duration"], direct["participation"]), (3, 5, 5))
        self.assertEqual((reverse["RD"], reverse["duration"], reverse["participation"]), (3, 7, 6))
        self.assertEqual((direct["aod_flux"], reverse["aod_flux"]), (F(5), F(-6)))
        self.assertEqual(reverse["RCD"].reflection_counts, (2, 2))
        with self.assertRaises(n.AdmissionError):
            n.participation(reverse["RCD"], 6, "seconds")
        with self.assertRaises(n.AdmissionError):
            channel(a, b, contacts, 3, F(1, 3), window_count=True)

    def test_path_pressure_aggregation_and_typed_fluxes(self):
        record = build_fixture()
        row = record["pressure_and_flux"]
        self.assertEqual([h["pressure"] for h in record["path_histories"]], ["15", "12"])
        self.assertEqual(row["path_pressure_sum"], "27")
        self.assertEqual(row["sum_of_oriented_path_fluxes"], "-1")
        self.assertEqual(row["compression"]["effective_attention"], "-1/27")
        self.assertEqual(row["reverse_physical_flux_sum"], "0")
        self.assertEqual(row["bookkeeping_reverse_flux_sum"], "1")
        self.assertEqual(record["afc"]["flux"], [["0", "-1/8"], ["1/8", "0"]])
        self.assertEqual(record["cancellation"]["audit"]["boundary"], "0")
        self.assertEqual(len(record["cancellation"]["audit"]["lineages"]), 2)
        self.assertEqual(set(record["cancellation"]["audit"]["lineages"]),
                         {h["id"] for h in record["path_histories"]})

    def test_same_summary_different_history_changes_later_response(self):
        a, b, _, _ = make_population()
        contacts = bound_contacts(a, b)
        before = contacts[-1]
        result = history_reduction_check(a, b, contacts)
        self.assertEqual(contacts[-1], before)
        alternatives = result["alternatives"]
        self.assertEqual([x["channel"]["duration"] for x in alternatives], [5, 7])
        self.assertEqual([x["response"].current for x in alternatives], [F(5), F(6)])
        self.assertTrue(all(x["response"].committed == 23 for x in alternatives))
        self.assertNotEqual(alternatives[0]["channel"]["contacts"][-1].identity,
                            alternatives[1]["channel"]["contacts"][-1].identity)

    def test_hostile_contacts_and_return_replay_rejected(self):
        a, b, _, _ = make_population()
        contacts = bound_contacts(a, b)
        for events in ((contacts[0], contacts[0]),
                       (contacts[0], replace(contacts[1], left_prefix="forged")),
                       (contacts[0], replace(contacts[1], left=b.identity))):
            with self.subTest(events=events), self.assertRaises(n.AdmissionError):
                channel(a, b, events, 3, F(1, 3))
        single = n.certified(n.trace("replay"))
        with self.assertRaises(n.AdmissionError):
            n.wave([single, single])

    def test_endpoint_projection_loses_distinct_return_support(self):
        short = n.trace("short", vertices=(0, 1, 0))
        long = n.trace("long", vertices=(0, 1, 3, 1, 0), turn=2)
        self.assertTrue(n.validate_trace(short) and n.validate_trace(long))
        self.assertEqual(short.vertices[-1], long.vertices[-1])
        self.assertNotEqual(short.return_id, long.return_id)
        self.assertEqual([len(t.vertices) + len(t.roles) - 3 for t in (short, long)], [3, 5])


if __name__ == "__main__":
    unittest.main()
