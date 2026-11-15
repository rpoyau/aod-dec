#!/usr/bin/env python3
"""Execute the bound Manual response and introspection example through Dec JIT batches."""
from dataclasses import replace
import argparse
import hashlib
import json
from pathlib import Path
import re
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from native.cpu_batch import CpuBatchSession, run_cpu_batch
from native.gpu_compiler import GpuBatchSession, run_gpu_batch, GPUUnavailable
from simulations import awareness_jit_fixture as fixture


def require(condition, message):
    if not condition:
        raise ValueError(message)


def source_binding():
    files = ("examples/awareness_response.dec", "scripts/check_awareness_jit.py",
        "simulations/awareness_jit_fixture.py", "simulations/vendor/aod_awareness.py",
        "simulations/vendor/aod_manual_native.py", "native/batch.py", "native/cpu_batch.py",
        "native/batch_dispatch.cpp", "native/gpu_compiler.py", "native/compiler.py",
        "native/runtime.cpp", "native/runtime.hpp", "runtime/dec_language.py", "runtime/dec_core.py")
    return {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest() for name in files}


def _integers(result, count):
    require(type(result) is dict and type(result.get("rows")) is list and len(result["rows"]) == count,
            "compiled batch row coverage differs")
    values = []
    for row in result["rows"]:
        require(row["exit_code"] == 0 and row["error"] is None and row["error_line"] == 0 and row["stdout"] == "",
                "compiled awareness response failed: " + str(row))
        value = row["result"]
        require(type(value) is str and len(value) <= 32 and re.fullmatch(r"-?[0-9]+", value) is not None,
                "compiled response must render one bounded exact integer")
        values.append(int(value))
    return tuple(values)


def execute_compiled(preparation, *, backend="cpu", workers=2, passive=False, reflexive=False,
                     extra_delay=False, session=None, reuse=True):
    """Reuse generated entries while retaining every native state admission."""
    require(backend in ("cpu", "gpu"), "explicit compiled backend required")
    require(type(reuse) is bool, "explicit reusable-session mode required")
    require(session is None or reuse, "external session requires reuse")
    if session is not None:
        require(isinstance(session, CpuBatchSession if backend == "cpu" else GpuBatchSession),
                "compiled session belongs to another backend")
    started = time.perf_counter_ns()
    options = dict(backend=backend, workers=workers, passive=passive,
                   reflexive=reflexive, extra_delay=extra_delay)
    if reuse and session is None:
        source = (ROOT / "examples/awareness_response.dec").read_text()
        owner = (CpuBatchSession(source, workers=workers, opt=0) if backend == "cpu"
                 else GpuBatchSession(source, opt=0))
        with owner:
            result = _execute_compiled(preparation, session=owner, **options)
        timing_scope = "complete execution including session construction, first loads, and close"
    else:
        result = _execute_compiled(preparation, session=session, **options)
        timing_scope = ("execution within caller-owned session; caller accounts for construction and close"
                        if session is not None else "complete execution including each one-shot runtime lifecycle")
    result.update(elapsed_ns=time.perf_counter_ns()-started, timing_scope=timing_scope,
                  runtime_reuse=reuse)
    return result


def _execute_compiled(preparation, *, backend, workers, passive, reflexive, extra_delay, session):
    """Generated values drive the next native state; host checks their source law."""
    require(backend in ("cpu", "gpu"), "explicit compiled backend required")
    require(not extra_delay or reflexive, "delayed query requires the explicit perturbation family")
    manual = fixture.load_manual()
    source = (ROOT / "examples/awareness_response.dec").read_text()
    source_sha = hashlib.sha256(source.encode()).hexdigest()
    states = dict(preparation["lanes"])
    names = tuple(states)
    previous = {name: (states[name].present.committed-1, fixture.native.Q(0)) for name in names}
    results = {name: {"states": [], "events": [], "observations": []} for name in names}
    calls = []
    def batch(function, rows):
        answer = (session.run(function, rows) if session is not None else
                  run_cpu_batch(source, function, rows, workers=workers, opt=0) if backend == "cpu"
                  else run_gpu_batch(source, function, rows, opt=0))
        binding = answer["source_binding"]
        entry = binding["entry"] if backend == "cpu" else binding
        require(entry["source_sha256"] == source_sha,
                "returned compiler entry differs from bound awareness source")
        if backend == "gpu":
            require(answer["execution"].get("gpu_executed") is True,
                    "nonempty GPU awareness batch did not execute on device")
        else:
            require(answer["execution"]["compiled_entries_entered"] == len(rows),
                    "CPU awareness batch skipped a generated entry")
        calls.append({"function": function, "inputs": rows, **answer})
        return _integers(answer, len(rows))
    for declared in preparation["inputs"]:
        inputs = {name: declared for name in names}
        if reflexive:
            observed = {name: (states[name].present.committed, states[name].present.current) for name in names}
            consumed = previous if extra_delay else observed
            require(all(value.denominator == 1 for _, value in consumed.values()),
                    "compiled query cannot truncate a rational observation")
            queries = batch("reflexive_query", tuple((int(consumed[name][1]),) for name in names))
            inputs = {name: replace(declared, query=fixture.native.Q(query),
                                    query_observed_at=consumed[name][0])
                      for name, query in zip(names, queries)}
            previous = observed
        rows = tuple(fixture.response_inputs(states[name], preparation["world"], preparation["model"], inputs[name])
                     for name in names)
        currents = batch("response", rows)
        axes = batch("continuation", tuple((current,) for current in currents))
        for name, current, axis in zip(names, currents, axes):
            state, event = fixture.apply_compiled(states[name], preparation["world"], preparation["model"],
                                                  inputs[name], current, axis)
            states[name] = state
            results[name]["states"].append(state)
            results[name]["events"].append(event)
            if passive:
                results[name]["observations"].append({"state_digest": manual.digest(state), "event": manual.encode(event)})
    for name in names:
        results[name].update(final=states[name], states=tuple(results[name]["states"]), events=tuple(results[name]["events"]))
    return {"lanes": results, "calls": calls, "backend": backend,
            "observation_mode": "passive" if passive else "off",
            "declared_delay_initial_history": {name: (state.present.committed-1, fixture.native.Q(0))
                for name, state in preparation["lanes"].items()} if extra_delay else None,
            "query_delay": int(extra_delay),
            "perturbation_family": ("committed delayed query q_k=-J_(k-1)" if extra_delay
                else "committed reflexive query q_k=-J_k" if reflexive else "unqueried")}


def _query_reference(preparation, *, extra_delay=False):
    manual = fixture.load_manual()
    results = {}
    for name, initial in preparation["lanes"].items():
        state, states, events = initial, [], []
        previous = initial.present.committed-1, fixture.native.Q(0)
        for declared in preparation["inputs"]:
            observed = state.present.committed, state.present.current
            cut, value = previous if extra_delay else observed
            inputs = replace(declared, query=-value, query_observed_at=cut)
            previous = observed
            state, event = manual.transition(state, preparation["world"], preparation["model"], inputs)
            states.append(state); events.append(event)
        results[name] = {"final": state, "states": tuple(states), "events": tuple(events), "observations": []}
    return results


def _same(left, right):
    manual = fixture.load_manual()
    return set(left) == set(right) and all(manual.execution_bytes(left[name]) == manual.execution_bytes(right[name]) for name in left)


def check(output, *, gpu="auto", workers=2):
    started = time.perf_counter_ns()
    before = source_binding()
    source = (ROOT / "examples/awareness_response.dec").read_text()
    require(hashlib.sha256(source.encode()).hexdigest() == before["examples/awareness_response.dec"],
            "awareness source changed before session construction")
    with CpuBatchSession(source, workers=workers, opt=0) as session:
        report = _check(output, gpu=gpu, workers=workers, cpu_session=session, before=before)
    require(source_binding() == before, "source changed during awareness session lifetime")
    report["complete_check_ns"] = time.perf_counter_ns()-started
    report["runtime_reuse"] = "one CPU session across six families; one GPU session across five families"
    report["timing_scope"] = "preparation, admissions, compilation/loading, execution, checking, artifact writes and runtime close"
    (Path(output) / "results.json").write_text(json.dumps(fixture.load_manual().encode(report), indent=2) + "\n")
    return report


def _check(output, *, gpu, workers, cpu_session, before):
    require(gpu in ("auto", "required", "off"), "explicit GPU mode required")
    require(type(workers) is int and 2 <= workers <= 32, "multicore example requires 2..32 workers")
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    require(source_binding() == before, "source changed during awareness session construction")
    manual = fixture.load_manual()
    preparation = fixture.prepare()
    started = time.perf_counter_ns()
    cpu = execute_compiled(preparation, workers=workers, session=cpu_session)
    passive = execute_compiled(preparation, workers=workers, passive=True, session=cpu_session)
    queried = execute_compiled(preparation, workers=workers, reflexive=True, session=cpu_session)
    learning_preparation = fixture.prepare_learning(preparation, cpu["lanes"]["retained"]["final"])
    learning = execute_compiled(learning_preparation, workers=workers, session=cpu_session)
    mindfulness_preparation = fixture.prepare_mindfulness(preparation)
    mindfulness = execute_compiled(mindfulness_preparation, workers=workers, reflexive=True, session=cpu_session)
    delayed = execute_compiled(mindfulness_preparation, workers=workers, reflexive=True, extra_delay=True, session=cpu_session)
    cpu_runs = {"ordinary": cpu, "passive": passive, "query": queried,
                "learning": learning, "mindfulness": mindfulness, "delayed": delayed}
    require(_same(cpu["lanes"], preparation["reference"]), "compiled CPU changed the complete Manual outcome")
    require(_same(passive["lanes"], cpu["lanes"]), "passive inspection changed compiled native execution")
    require(_same(queried["lanes"], _query_reference(preparation)), "compiled query changed the declared Manual feedback law")
    require(_same(learning["lanes"], learning_preparation["reference"]),
            "compiled continuation changed existing Manual learning()")
    require(_same(mindfulness["lanes"], _query_reference(mindfulness_preparation)),
            "compiled current-three query changed complete native states")
    require(_same(delayed["lanes"], _query_reference(mindfulness_preparation, extra_delay=True)),
            "compiled delay changed complete native states")
    for key, run in (("manual_mindfulness", mindfulness), ("manual_delayed", delayed)):
        require(run["lanes"]["retained"]["events"] == tuple(mindfulness_preparation[key]["events"]),
                "compiled events differ from existing Manual " + key)
    for run in cpu_runs.values():
        require(manual.execution_bytes(run["lanes"]["retained"]) == manual.execution_bytes(run["lanes"]["restored"]),
                "restored relation changed a matched later execution")
        require(all(event["new_phase"] == 0 for lane in run["lanes"].values() for event in lane["events"]),
                "unwitnessed closure in existing awareness continuation")
    lanes = cpu["lanes"]
    require(manual.outcome(lanes["retained"]["events"]) != manual.outcome(lanes["removed"]["events"]),
            "retained relation produced no later realized outcome difference")
    require(manual.execution_bytes(lanes["retained"]) == manual.execution_bytes(lanes["restored"]),
            "exact restoration did not restore native execution")
    require(all(event["at"] > preparation["intervention_cut"] and event["new_phase"] == 0
                for lane in lanes.values() for event in lane["events"]), "same-cut effect or unwitnessed closure")
    pids = sorted({identifier for call in cpu["calls"] for identifier in call["execution"]["worker_ids"]})
    require(any(call["execution"]["workers_used"] >= 2 for call in cpu["calls"]), "CPU example used no concurrent native worker pair")
    gpu_record = {"status": "NOT_REQUESTED", "gpu_executed": False}
    if gpu != "off":
        try:
            with GpuBatchSession((ROOT / "examples/awareness_response.dec").read_text(), opt=0) as device_session:
                device = execute_compiled(preparation, backend="gpu", workers=workers, session=device_session)
                device_query = execute_compiled(preparation, backend="gpu", workers=workers, reflexive=True, session=device_session)
                device_learning_preparation = fixture.prepare_learning(preparation, device["lanes"]["retained"]["final"])
                device_runs = {"ordinary": device, "query": device_query,
                    "learning": execute_compiled(device_learning_preparation, backend="gpu", workers=workers, session=device_session),
                    "mindfulness": execute_compiled(mindfulness_preparation, backend="gpu", workers=workers, reflexive=True, session=device_session),
                    "delayed": execute_compiled(mindfulness_preparation, backend="gpu", workers=workers, reflexive=True, extra_delay=True, session=device_session)}
        except GPUUnavailable as exc:
            gpu_record = {"status": "UNAVAILABLE", "gpu_executed": False, "reason": exc.report}
            if gpu == "required":
                (output / "gpu-unavailable.json").write_text(json.dumps(gpu_record, indent=2) + "\n")
                raise
        else:
            for name, run in device_runs.items():
                require(_same(run["lanes"], cpu_runs[name]["lanes"]), "CUDA changed existing Manual " + name)
            gpu_record = {"status": "PASS", "gpu_executed": True,
                "launches": sum(c["execution"]["launch_count"] for run in device_runs.values() for c in run["calls"]),
                "verified_families": list(device_runs),
                "device": device["calls"][0]["execution"]["device"]}
            (output / "gpu-execution.json").write_text(json.dumps(manual.encode(device_runs), indent=2) + "\n")
    require(source_binding() == before, "source changed during awareness check")
    report = {"status": "PASS", "source_sha256": before, "source_binding": preparation["source_binding"],
        "intervention_cut": preparation["intervention_cut"],
        "later_currents": {name: [event["current"] for event in lane["events"]] for name, lane in lanes.items()},
        "later_edges": {name: [event["edge"] for event in lane["events"]] for name, lane in lanes.items()},
        "query_currents": {name: [event["current"] for event in lane["events"]] for name, lane in queried["lanes"].items()},
        "existing_learning": {"training_backend": "compiled-cpu", "intervention_cut": learning_preparation["intervention_cut"],
            "currents": {name: [e["current"] for e in lane["events"]] for name, lane in learning["lanes"].items()},
            "relation_persists": learning_preparation["manual_learning"]["acquisition"] ==
                learning_preparation["manual_learning"]["retained_after_training"], "restoration_equal": True},
        "existing_mindfulness": {"initial_current": 3, "currents": {
            name: [e["current"] for e in lane["events"]] for name, lane in mindfulness["lanes"].items()},
            "observation_cuts": [e["query_observed_at"] for e in mindfulness["lanes"]["retained"]["events"]]},
        "existing_delay_control": {"declared_initial_history": mindfulness_preparation["manual_delayed"]["declared_delay_initial_history"],
            "currents": {name: [e["current"] for e in lane["events"]] for name, lane in delayed["lanes"].items()},
            "observation_cuts": [e["query_observed_at"] for e in delayed["lanes"]["retained"]["events"]]},
        "pressure_controls": {"execution": "unchanged Manual host reference only; outside declared p=1 integer batch",
            "compiled": False, "gpu_executed": False, "cases": mindfulness_preparation["manual_pressure_controls"]},
        "retention_effect": True, "restoration_equal": True, "passive_equal": True,
        "new_closure_cycles": 0, "compiled_cpu_worker_ids": pids, "gpu": gpu_record,
        "execution_and_checks_ns": time.perf_counter_ns() - started,
        "compiler_selection": {"variant": "generic", "opt": 0,
            "reason": "No retained performance evidence was supplied; no cost-control policy change inferred."},
        "scope": "Existing Manual p=g=1 operative-retention and later-query model; native admission on host; response/query/selector executed as compiled Dec. No physical awareness, new probability law, or speedup claim."}
    (output / "results.json").write_text(json.dumps(manual.encode(report), indent=2) + "\n")
    (output / "witnesses.json").write_text(json.dumps(manual.encode({"ordinary": preparation,
        "learning": learning_preparation, "mindfulness": mindfulness_preparation}), indent=2) + "\n")
    (output / "cpu-execution.json").write_text(json.dumps(manual.encode(cpu_runs), indent=2) + "\n")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--gpu", choices=("auto", "required", "off"), default="auto")
    parser.add_argument("--workers", type=int, default=2)
    args = parser.parse_args()
    report = check(args.output, gpu=args.gpu, workers=args.workers)
    print(json.dumps({"status": report["status"], "gpu": report["gpu"]["status"], "output": str(args.output)}))


if __name__ == "__main__":
    main()
