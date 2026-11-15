from concurrent.futures import ThreadPoolExecutor
from dataclasses import replace
from hashlib import sha256
from pathlib import Path
import sys
import tempfile
import unittest

from native.cpu_batch import run_cpu_batch
from scripts import check_awareness_jit as check
from simulations import awareness_jit_fixture as fixture


class AwarenessJitTests(unittest.TestCase):
    def test_exact_manual_source_and_isolated_dependency_binding(self):
        before = sys.modules.get("aod_native_closure_wave")
        with ThreadPoolExecutor(max_workers=4) as pool:
            modules = tuple(pool.map(lambda _: fixture.load_manual(), range(8)))
        self.assertTrue(all(module is modules[0] for module in modules))
        self.assertIs(modules[0].n, fixture.native)
        self.assertIs(sys.modules.get("aod_native_closure_wave"), before)
        self.assertEqual(sha256(Path(modules[0].__file__).read_bytes()).hexdigest(), fixture.AWARENESS_SHA)

    def test_retention_is_the_only_changed_coordinate(self):
        data = fixture.prepare()
        retained, removed, restored = (data["lanes"][name] for name in ("retained", "removed", "restored"))
        self.assertEqual(retained.present, removed.present)
        self.assertEqual(retained.closure_memory, removed.closure_memory)
        self.assertEqual(removed.retained, ())
        self.assertEqual(restored, retained)
        self.assertEqual(data["intervention_cut"], 33)
        prior = retained.retained[0].prior
        self.assertEqual((prior.input_commit, prior.retained_at), (23, 24))
        self.assertEqual(prior.target_prefix, fixture.native.wave_prefix(data["world"].get(prior.target), 23))
        self.assertEqual(prior.source_prefix, fixture.native.wave_prefix(data["world"].get(prior.source), 23))
        self.assertEqual(fixture.response_inputs(retained, data["world"], data["model"], data["inputs"][0]), (0, 0, 0, 1))
        self.assertEqual(fixture.response_inputs(removed, data["world"], data["model"], data["inputs"][0]), (0, 0, 0, 0))

    def test_compiled_return_drives_later_native_state_and_passive_observation(self):
        with tempfile.TemporaryDirectory() as directory:
            report = check.check(directory, gpu="off", workers=2)
            self.assertEqual(report["status"], "PASS")
            self.assertEqual(report["later_currents"], {
                "retained": [1, 2, 3, 4], "removed": [0, 0, 0, 0], "restored": [1, 2, 3, 4]})
            self.assertEqual(report["query_currents"], {
                "retained": [1] * 4, "removed": [0] * 4, "restored": [1] * 4})
            self.assertEqual(report["later_edges"]["retained"], [(0, 1), (1, 0), (0, 1), (1, 0)])
            self.assertEqual(report["later_edges"]["removed"], [(0, 2), (2, 0), (0, 2), (2, 0)])
            self.assertTrue(report["passive_equal"])
            self.assertTrue(report["restoration_equal"])
            self.assertEqual(report["new_closure_cycles"], 0)
            self.assertEqual(report["compiler_selection"]["variant"], "generic")
            self.assertFalse(report["gpu"]["gpu_executed"])
            self.assertEqual(report["existing_learning"]["currents"], {
                "retained": [5, 6, 7], "removed": [4, 4, 4], "restored": [5, 6, 7]})
            self.assertEqual(report["existing_learning"]["intervention_cut"], 37)
            self.assertTrue(report["existing_learning"]["relation_persists"])
            self.assertEqual(report["existing_mindfulness"]["initial_current"], 3)
            self.assertEqual(report["existing_mindfulness"]["currents"]["retained"], [1]*6)
            self.assertEqual(report["existing_mindfulness"]["observation_cuts"], list(range(33, 39)))
            self.assertEqual(report["existing_delay_control"]["currents"]["retained"], [4, 2, -1, -2, 0, 3])
            self.assertEqual(report["existing_delay_control"]["declared_initial_history"], (32, 0))
            self.assertEqual(report["existing_delay_control"]["observation_cuts"], list(range(32, 38)))
            controls = report["pressure_controls"]
            self.assertFalse(controls["compiled"])
            self.assertFalse(controls["gpu_executed"])
            self.assertEqual([e["current"] for e in controls["cases"]["half"]["execution"]["events"]], [-1, 3]*3)
            self.assertEqual([e["current"] for e in controls["cases"]["two"]["execution"]["events"]],
                list(map(fixture.native.Q, (2, "3/2", "5/4", "9/8", "17/16", "33/32"))))

    def test_learning_requires_actual_training_and_integer_target_does_not_truncate(self):
        data = fixture.prepare()
        trained = data["reference"]["retained"]["final"]
        wrong = replace(trained, present=replace(trained.present, current=fixture.native.Q(40)))
        with self.assertRaisesRegex(ValueError, "four-step training"):
            fixture.prepare_learning(data, wrong)
        state = data["lanes"]["retained"]
        rational = replace(state, present=replace(state.present, current=fixture.native.Q(3, 2)))
        with self.assertRaisesRegex(ValueError, "cannot truncate"):
            fixture.response_inputs(rational, data["world"], data["model"], data["inputs"][0])
        changed_pressure = replace(state, present=replace(state.present, pressure=fixture.native.Q(1, 2)))
        with self.assertRaisesRegex(ValueError, "p=g=1"):
            fixture.response_inputs(changed_pressure, data["world"], data["model"], data["inputs"][0])

    def test_changed_source_changes_machine_result_and_fails_model_binding(self):
        data = fixture.prepare()
        source = (fixture.ROOT / "examples/awareness_response.dec").read_text()
        changed = source.replace("next = current + attention + query + retained",
                                 "next = current + attention + query + retained + 1")
        rows = ((0, 0, 0, 1), (0, 0, 0, 0))
        original = run_cpu_batch(source, "response", rows, workers=2)
        actual = run_cpu_batch(changed, "response", rows, workers=2)
        self.assertEqual([row["result"] for row in original["rows"]], ["1", "0"])
        self.assertEqual([row["result"] for row in actual["rows"]], ["2", "1"])
        self.assertNotEqual(original["source_binding"]["entry"]["entry_sha256"],
                            actual["source_binding"]["entry"]["entry_sha256"])
        with self.assertRaisesRegex(ValueError, "unit-pressure law"):
            fixture.apply_compiled(data["lanes"]["retained"], data["world"], data["model"],
                                   data["inputs"][0], int(actual["rows"][0]["result"]), 1)

    def test_forged_retention_future_query_and_same_cut_cannot_enter(self):
        data = fixture.prepare()
        state, world, model, inputs = data["lanes"]["retained"], data["world"], data["model"], data["inputs"][0]
        forged = replace(state, retained=(replace(state.retained[0], value=fixture.native.Q(7)),))
        with self.assertRaises(ValueError):
            fixture.response_inputs(forged, world, model, inputs)
        with self.assertRaisesRegex(ValueError, "future/stale"):
            fixture.response_inputs(state, world, model, replace(inputs, committed=inputs.committed + 1))
        with self.assertRaisesRegex(ValueError, "committed observation"):
            fixture.response_inputs(state, world, model,
                replace(inputs, query=fixture.native.Q(1), query_observed_at=inputs.committed + 1))
        with self.assertRaisesRegex(ValueError, "selector"):
            fixture.apply_compiled(state, world, model, inputs, 1, 2)
        with self.assertRaisesRegex(ValueError, "type/domain"):
            fixture.apply_compiled(state, world, model, inputs, True, 1)


if __name__ == "__main__":
    unittest.main()
