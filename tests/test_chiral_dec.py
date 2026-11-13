"""Execute generated encounter replay and inspect complete retained returns."""
from copy import deepcopy
from dataclasses import replace
from fractions import Fraction as F
import json
import unittest

from runtime.dec_core import render
from runtime.dec_language import DecError, Interpreter, compile_source
from simulations.chiral_collision import CollisionConfig, State, simulate_path
from simulations.chiral_dec import (MAX_EXPANDED_STEPS, STEP_OUTPUT_FIELDS,
                                    dec_source, expected_numeric_output,
                                    numeric_prefix, validate_native_output)


def execute(source):
    emitted = []
    result = Interpreter(compile_source(source), emit=emitted.append).run()
    return emitted, render(result)


def trace_output(emitted, steps):
    assert len(emitted) == steps * len(STEP_OUTPUT_FIELDS) + 2
    return tuple(json.loads(line) for line in emitted[-2:])


class ChiralDecTests(unittest.TestCase):
    def check_replay(self, config, path):
        source = dec_source(config, path)
        output, result = execute(source)
        self.assertEqual(result, "0")
        self.assertEqual("".join(line + "\n" for line in output[:-2]),
                         expected_numeric_output(config, path))
        self.assertEqual(numeric_prefix(path), expected_numeric_output(config, path))
        validation = validate_native_output("\n".join((*output, result)) + "\n", path)
        self.assertEqual(validation["hinges_per_trace"], config.steps + 2)
        traces = trace_output(output, config.steps)
        for index, trace in enumerate(traces):
            attr = "chi_a" if index == 0 else "chi_b"
            signs = [getattr(path["initial"], attr)] * 2 + [
                getattr(row["after"], attr) for row in path["steps"]]
            self.assertEqual(trace["lineage"], ("carrier-A", "carrier-B")[index])
            self.assertEqual(trace["start"], {"value": 0})
            self.assertEqual(len(trace["closures"]), config.steps + 2)
            self.assertEqual(len(trace["events"]), 3 * (config.steps + 2))
            for n, sign in enumerate(signs):
                events = trace["events"][3 * n:3 * (n + 1)]
                self.assertEqual([e["ordinal"] for e in events], list(range(3*n + 1, 3*n + 4)))
                self.assertEqual([e["kind"] for e in events], ["step", "hinge", "step"])
                self.assertEqual([e["orientation"] for e in events], [sign, 0, -sign])
                self.assertEqual([(e["source"], e["target"]) for e in events], [(0, 1), (1, 1), (1, 0)])
                self.assertEqual((trace["closures"][n]["begin"], trace["closures"][n]["end"]),
                                 (3*n, 3*n + 3))
        return source, output, traces

    def test_sampled_host_decisions_execute_and_preserve_lineages(self):
        config = CollisionConfig(steps=8)
        for seed in (7, 19):
            with self.subTest(seed=seed):
                path = simulate_path(config, seed=seed)
                source, _, traces = self.check_replay(config, path)
                self.assertNotEqual(traces[0]["closures"][0]["digest"],
                                    traces[1]["closures"][0]["digest"])
                self.assertIn("Declared locations/tickets SHA-256:", source)
                self.assertIn(path["source"]["sha256"], source)
                self.assertEqual(source, dec_source(config, path))

    def test_declared_population_interpolation_and_contact_boost_execute(self):
        config = CollisionConfig(steps=8, q_outer=F(1, 12), q_core=F(1, 3),
                                 contact_gain=F(1, 5), population_counts=(1, 2, 3, 4, 1, 2, 3, 4))
        path = simulate_path(config, seed=11)
        _, output, _ = self.check_replay(config, path)
        by_facet = dict(zip(("0***", "1***", "*0**", "*1**", "**0*", "**1*", "***0", "***1"),
                           (F(1, 12), F(1, 6), F(1, 4), F(1, 3)) * 2))
        for n, row in enumerate(path["steps"]):
            for offset, facet in enumerate((row["after"].facet_a, row["after"].facet_b)):
                expected = by_facet[facet]
                if row["after"].facet_a == row["after"].facet_b:
                    expected = F(1, 5) + F(4, 5) * expected
                self.assertEqual(F(output[8*n + offset]), expected)

    def test_zero_and_unit_flip_laws_have_independent_parity(self):
        for q in (0, 1):
            for steps in (1, 2, 3):
                config = CollisionConfig(steps=steps, q_outer=q, q_core=q, contact_gain=0)
                with self.subTest(q=q, steps=steps):
                    path = simulate_path(config, seed=7)
                    _, output, traces = self.check_replay(config, path)
                    sign = (-1) ** (q * steps)
                    self.assertEqual(tuple(int(value) for value in output[8*(steps-1)+2:8*(steps-1)+4]),
                                     (sign, -sign))
                    self.assertEqual([traces[0]["events"][-3]["orientation"],
                                      traces[1]["events"][-3]["orientation"]], [sign, -sign])

    def test_exact_ticket_equality_is_no_flip(self):
        config = CollisionConfig(steps=8, q_outer=F(1, 2), q_core=F(1, 2), contact_gain=0)
        path = simulate_path(config, seed=7, ticket_denominator=2)
        self.assertEqual({ticket for row in path["steps"] for ticket in row["flip_tickets"]}, {0, 1})
        _, output, _ = self.check_replay(config, path)
        previous = (1, -1)
        for n, row in enumerate(path["steps"]):
            expected = tuple(-sign if ticket == 0 else sign
                             for sign, ticket in zip(previous, row["flip_tickets"]))
            actual = tuple(int(value) for value in output[8*n+2:8*n+4])
            self.assertEqual(actual, expected)
            previous = actual

    def test_zero_steps_retains_two_formation_returns_with_explicit_initial_state(self):
        config = CollisionConfig(steps=0)
        path = simulate_path(config, initial=State("*0**", "*0**", -1, 1))
        _, output, _ = self.check_replay(config, path)
        self.assertEqual(len(output), 2)
        self.assertEqual(expected_numeric_output(config, path), "")

    def test_removing_structural_hinges_fails_actual_vm_execution(self):
        config = CollisionConfig(steps=1)
        source = dec_source(config, simulate_path(config))
        bad = source.replace("        t = hinge(t)\n", "")
        self.assertNotEqual(bad, source)
        with self.assertRaisesRegex(DecError, "structural hinge"):
            execute(bad)

    def test_rejects_invalid_config_and_oversized_expansion(self):
        with self.assertRaises(TypeError):
            dec_source({})
        with self.assertRaisesRegex(ValueError, "expanded steps"):
            dec_source(CollisionConfig(steps=MAX_EXPANDED_STEPS + 1))
        config = CollisionConfig(steps=1)
        with self.assertRaisesRegex(ValueError, "configuration"):
            dec_source(config, simulate_path(replace(config, q_core=F(1, 2))))

    def test_rejects_tampered_paths_and_ticket_metadata(self):
        config = CollisionConfig(steps=2)
        path = simulate_path(config)
        cases = []
        bad = deepcopy(path)
        bad["steps"][0]["flip_tickets"] = (bad["ticket_denominator"], 0)
        cases.append(bad)
        bad = deepcopy(path)
        bad["steps"][0]["flip_denominator"] = 3
        cases.append(bad)
        bad = deepcopy(path)
        bad["steps"][0]["movement_tickets"] = (True, 0)
        cases.append(bad)
        bad = deepcopy(path)
        bad["steps"][0]["after"] = replace(bad["steps"][0]["after"],
                                           chi_a=-bad["steps"][0]["after"].chi_a)
        cases.append(bad)
        bad = deepcopy(path)
        bad["steps"].pop()
        cases.append(bad)
        for index, bad in enumerate(cases):
            with self.subTest(index=index), self.assertRaises((TypeError, ValueError)):
                dec_source(config, bad)

    def test_output_validator_rejects_projected_or_forged_histories(self):
        config = CollisionConfig(steps=1)
        path = simulate_path(config)
        output, result = execute(dec_source(config, path))
        for mutation in ("omit_hinge", "wrong_orientation", "wrong_lineage", "reuse_certificate",
                         "bool_ordinal", "numeric"):
            lines = list(output)
            trace = json.loads(lines[-2])
            if mutation == "omit_hinge":
                trace["events"].pop(1)
            elif mutation == "wrong_orientation":
                trace["events"][-1]["orientation"] *= -1
            elif mutation == "wrong_lineage":
                trace["lineage"] = "carrier-B"
            elif mutation == "reuse_certificate":
                trace["closures"][-1]["digest"] = trace["closures"][0]["digest"]
            elif mutation == "bool_ordinal":
                trace["events"][0]["ordinal"] = True
            else:
                lines[0] = "0/1"
            lines[-2] = json.dumps(trace)
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                validate_native_output("\n".join((*lines, result)) + "\n", path)


if __name__ == "__main__":
    unittest.main()
