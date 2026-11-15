#!/usr/bin/env python3
"""Trace existing Manual evolution and measure reusable compiler execution."""
import argparse
from hashlib import sha256
import json
from pathlib import Path
import platform
import statistics
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from native.gpu_compiler import GPUUnavailable
from scripts import check_awareness_jit as compiled
from simulations import awareness_jit_fixture as fixture
from simulations.awareness_engine_trace import run_engine_case, trace_to_dict, verify_trace


def require(condition, message):
    if not condition:
        raise ValueError(message)


def encoded(value):
    return json.dumps(fixture.load_manual().encode(value), sort_keys=True,
                      separators=(",", ":"), allow_nan=False).encode()


def measure(call):
    started = time.perf_counter_ns()
    value = call()
    raw = encoded(value)
    return value, raw, time.perf_counter_ns()-started


def summary(samples, before, after):
    ratios = [sample[before]/sample[after] for sample in samples]
    return {"samples": samples, "before": before, "after": after,
            "median_before_ns": statistics.median(s[before] for s in samples),
            "median_after_ns": statistics.median(s[after] for s in samples),
            "median_paired_time_ratio": statistics.median(ratios),
            "all_pairs_faster": all(ratio > 1 for ratio in ratios)}


def check(output, *, repeats=3, gpu="auto", workers=2):
    require(type(repeats) is int and 1 <= repeats <= 20, "repeats must be1..20")
    require(gpu in ("off", "auto", "required"), "explicit GPU mode required")
    require(type(workers) is int and 2 <= workers <= 32, "workers must be2..32")
    output = Path(output); output.mkdir(parents=True, exist_ok=True)
    before = compiled.source_binding()
    before.update({name: sha256((ROOT/name).read_bytes()).hexdigest() for name in
                   ("scripts/check_introspection_evolution.py", "simulations/awareness_engine_trace.py")})
    report = {"status": "PASS", "source_sha256": before,
              "python": sys.version, "platform": platform.platform(), "optimized": not __debug__,
              "engine": {}, "compiler": {}, "repeats": repeats,
              "scope": "Unchanged existing Manual execution, plus a matched old/new compiled adapter comparison. "
                       "A compiler-session ratio is not a speedup of the original Manual engine. "
                       "Tracing is observation, not a new evolution law or a speedup premise."}
    for case in ("learning", "mindfulness", "delayed"):
        samples, reference, kept, trace_reference = [], None, None, None
        for repeat in range(repeats):
            sample = {}
            order = ("off", "profile", "auto")
            order = order[repeat % 3:]+order[:repeat % 3]
            for observer in order:
                tracing = observer != "off"
                def invoke():
                    run = run_engine_case(case, trace=tracing, observer="auto" if observer == "off" else observer)
                    return {**run, "trace": trace_to_dict(run["trace"]) if tracing else None}
                run, raw, elapsed = measure(invoke)
                result = encoded(run["result"])
                if reference is None:
                    reference = result
                require(result == reference and run["error"] is None, "passive trace changed original engine outcome")
                sample[{"off": "trace_off_ns", "profile": "trace_profile_ns", "auto": "trace_on_ns"}[observer]] = elapsed
                if tracing:
                    trace_bytes = encoded(run["trace"])
                    if trace_reference is None:
                        trace_reference = trace_bytes
                    require(trace_bytes == trace_reference, "observer mechanism changed exact trace records")
                    if observer == "auto":
                        kept = run
            samples.append(sample)
        # Verification is deliberately later and has its own cost.
        from simulations.awareness_engine_trace import EngineTrace
        trace = EngineTrace(**kept["trace"])
        started = time.perf_counter_ns(); verification = verify_trace(trace)
        verification_ns = time.perf_counter_ns()-started
        require(trace.complete, "original engine trace is incomplete")
        (output/(case+"-trace.json")).write_bytes(encoded(kept)+b"\n")
        report["engine"][case] = {**summary(samples, "trace_off_ns", "trace_on_ns"),
            "interpretation": "trace-off time divided by trace-on time; below1 is tracing overhead",
            "timing_scope": "source checks, original source execution, observer lifecycle, retained witnesses and full report serialization",
            "observer_reuse": summary(samples, "trace_profile_ns", "trace_on_ns"),
            "observer_backend": kept["observer_backend"],
            "observer_mechanisms_equal": True,
            "result_sha256": sha256(reference).hexdigest(), "verification": verification,
            "verification_ns": verification_ns, "events": len(trace.events),
            "retained_bytes": trace.retained_bytes, "passive_equal": True}
    # Both adapters have exactly the same initial states, validation calls,
    # native updates, queries, complete output comparison and passive records.
    preparation = fixture.prepare_mindfulness(fixture.prepare())
    reference = compiled._query_reference(preparation)
    for backend in (("cpu", "gpu") if gpu != "off" else ("cpu",)):
        samples, kept = [], None
        try:
            for repeat in range(repeats):
                sample = {}
                for reuse in ((False, True) if repeat % 2 == 0 else (True, False)):
                    run, raw, elapsed = measure(lambda: compiled.execute_compiled(preparation,
                        backend=backend, workers=workers, passive=True, reflexive=True, reuse=reuse))
                    require(compiled._same(run["lanes"], reference), "compiler changed original complete native outcomes")
                    sample["reusable_ns" if reuse else "one_shot_ns"] = elapsed
                    if reuse:
                        kept = run
                samples.append(sample)
        except GPUUnavailable as exc:
            require(backend == "gpu", "CPU unexpectedly reported GPU unavailability")
            report["compiler"][backend] = {"status": "UNAVAILABLE", "reason": exc.report}
            if gpu == "required":
                (output/"results.json").write_bytes(encoded(report)+b"\n")
                raise
        else:
            (output/(backend+"-execution.json")).write_bytes(encoded(kept)+b"\n")
            calls = kept["calls"]
            report["compiler"][backend] = {"status": "PASS", **summary(samples, "one_shot_ns", "reusable_ns"),
                "calls_per_run": len(calls), "compiled_entries": sum(len(c["rows"]) for c in calls),
                "full_native_outcomes_equal": True,
                "timing_scope": "same prepared18-transition conformance adapter, runtime construction, compile/cache lookup, "
                    "first loads, execution, all original admissions, passive records, cleanup and full report serialization; "
                    "shared initial fixture construction and later reference comparison excluded from both",
                "artifact_cache": "existing content-addressed CPU disk cache; GPU NVRTC compile included. First measured calls retained, no warmup samples discarded",
                "baseline": "current one-shot public API, including session-per-call GPU implementation; not the historical pre-session binary",
                "execution": calls[-1]["execution"]}
    after = compiled.source_binding()
    after.update({name: sha256((ROOT/name).read_bytes()).hexdigest() for name in
                  ("scripts/check_introspection_evolution.py", "simulations/awareness_engine_trace.py")})
    require(before == after, "source changed during evolution check")
    (output/"results.json").write_bytes(encoded(report)+b"\n")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--workers", type=int, default=2)
    parser.add_argument("--gpu", choices=("auto", "required", "off"), default="auto")
    args = parser.parse_args()
    report = check(args.output, repeats=args.repeats, gpu=args.gpu, workers=args.workers)
    print(json.dumps({"status": report["status"], "compiler": {
        key: {k: v for k, v in value.items() if k in ("status", "median_paired_time_ratio")}
        for key, value in report["compiler"].items()}, "output": str(args.output)}))


if __name__ == "__main__":
    main()
