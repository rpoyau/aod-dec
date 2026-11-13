#!/usr/bin/env python3
"""Compare exact execution backends and retain admitted collision routing traces."""
from __future__ import annotations

import argparse
from concurrent.futures import ProcessPoolExecutor
from fractions import Fraction as F
import gzip
import hashlib
import json
import multiprocessing as mp
import os
from pathlib import Path
import platform
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from simulations.chiral_collision import CollisionConfig, exact_data, simulate_path
from simulations.chiral_batch import build_ticket_batch, compile_model_tables, run_multicore, run_scalar, validate_rows
from simulations.collision_routing import build_routing_fixture, validate_routing_fixture
from simulations.tesseract_cells import FACETS
from simulations.wave_observables import WavelengthMap, wavelength_report


def encoded(value):
    return json.dumps(exact_data(value), sort_keys=True, separators=(",", ":")).encode()


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(exact_data(value), indent=2) + "\n")
    temporary.replace(path)


def compressed(path, value):
    raw = encoded(value)
    packed = gzip.compress(raw, compresslevel=9, mtime=0)
    path.write_bytes(packed)
    return {"file": path.name, "sha256": digest(path), "decoded_sha256": hashlib.sha256(raw).hexdigest(),
            "decoded_bytes": len(raw), "compression": "gzip, mtime=0"}


def source_hashes():
    names = ["scripts/simulate_routing_parallel.py", "simulations/chiral_batch.py",
             "simulations/chiral_gpu.py", "simulations/kernels/chiral_batch.cu",
             "simulations/collision_routing.py", "simulations/wave_observables.py",
             "simulations/chiral_collision.py", "simulations/manual_dec_fixture.py",
             "simulations/vendor/aod_manual_native.py", "simulations/vendor/LICENSE",
             "simulations/tesseract_cells.py", "evidence/chiral-collision/cohort.json.gz",
             "evidence/chiral-collision/results.json"]
    return {name: digest(ROOT / name) for name in names}


def retained_comparison(batch, result):
    validate_rows(batch, result["rows"])
    path = ROOT / "evidence/chiral-collision/cohort.json.gz"
    decoded = gzip.decompress(path.read_bytes())
    binding = json.loads((ROOT / "evidence/chiral-collision/results.json").read_text())
    if hashlib.sha256(decoded).hexdigest() != binding["artifacts"]["cohort.json"]:
        raise ValueError("prior retained cohort does not match its source binding")
    prior = {p["seed"]: p for p in json.loads(decoded) if p["case"] == "core_only"}
    checked = 0
    for index, seed in enumerate(batch.seeds):
        if seed not in prior:
            continue
        source = prior[seed]
        if len(source["steps"]) != batch.steps:
            raise ValueError("retained reference window differs")
        for ticket, row, old in zip(batch.tickets[index], result["rows"][index], source["steps"]):
            target = old["after"]
            expected = (FACETS.index(target["facet_a"]), FACETS.index(target["facet_b"]),
                        target["chi_a"], target["chi_b"], *map(int, old["flipped"]),
                        int(old["support_contact"]))
            if row != expected or ticket != tuple(old["movement_tickets"] + old["flip_tickets"]):
                raise ArithmeticError("candidate backend differs from the previously retained path")
        checked += 1
    return {"status": "PASS", "paths_checked": checked, "transitions_checked": checked * batch.steps,
            "prior_cohort_sha256": hashlib.sha256(decoded).hexdigest(),
            "scope": "Every selected prior core-only seed available in the bound 256-path cohort."}


def _trace_job(job):
    seed, map_operands = job
    path = simulate_path(CollisionConfig(contact_gain=F(0)), seed=seed, ticket_denominator=64)
    fixture = build_routing_fixture(path)
    validate_routing_fixture(fixture)
    wavelengths = {}
    for name in ("a", "b"):
        wave = fixture["carriers"][name]["wave"]
        mapping = None if map_operands is None else WavelengthMap(
            F(map_operands[0]), F(map_operands[1]), F(map_operands[2]), wave.scope,
            map_operands[3], "declared_calibration")
        wavelengths[name] = wavelength_report(wave.cadence, native_scope=wave.scope, observation_map=mapping)
    result = {"seed": seed, "fixture": fixture, "wavelengths": wavelengths}
    # Encoding is inside this timed work item, including on each worker.
    return result, hashlib.sha256(encoded(result)).hexdigest(), os.getpid()


def trace_candidate_comparison(batch, result, traces):
    """Join each materialized source history to the corresponding candidate path."""
    validate_rows(batch, result["rows"])
    if len(traces) > len(batch.tickets) or batch.seeds is None:
        raise ValueError("traces require a bound prefix of the candidate seed batch")
    checked = 0
    for index, trace in enumerate(traces):
        source = trace["fixture"]["source_path"]
        if trace["seed"] != batch.seeds[index] or source["seed"] != trace["seed"]:
            raise ValueError("full trace belongs to another candidate seed")
        if len(source["steps"]) != batch.steps or source["ticket_denominator"] != batch.ticket_denominator:
            raise ValueError("full trace window or ticket grid differs from candidate")
        initial = source["initial"]
        if batch.initial_states[index] != (FACETS.index(initial.facet_a), FACETS.index(initial.facet_b), initial.chi_a, initial.chi_b):
            raise ValueError("full trace has a different initial state")
        for ticket, candidate, event in zip(batch.tickets[index], result["rows"][index], source["steps"]):
            target = event["after"]
            expected = (FACETS.index(target.facet_a), FACETS.index(target.facet_b),
                        target.chi_a, target.chi_b, *map(int, event["flipped"]), int(event["support_contact"]))
            if candidate != expected or ticket != (*event["movement_tickets"], *event["flip_tickets"]):
                raise ArithmeticError("full admitted trace differs from candidate execution")
            checked += 1
    return {"status": "PASS", "paths_checked": len(traces), "transitions_checked": checked,
            "scope": "Every retained full trace joined to its supplied seed, initial state, tickets and candidate states/signs, including seeds beyond the prior cohort."}


def trace_study(count, workers, map_operands):
    jobs = [(seed, map_operands) for seed in range(count)]
    start = time.perf_counter_ns()
    scalar = [_trace_job(job) for job in jobs]
    scalar_ns = time.perf_counter_ns() - start
    start = time.perf_counter_ns()
    with ProcessPoolExecutor(max_workers=min(workers, count), mp_context=mp.get_context("spawn")) as pool:
        parallel = list(pool.map(_trace_job, jobs, chunksize=1))
    parallel_ns = time.perf_counter_ns() - start
    if [r[1] for r in scalar] != [r[1] for r in parallel]:
        raise ArithmeticError("parallel full witness/routing traces differ from scalar results")
    rows = [row[0] for row in scalar]
    summary = []
    for row in rows:
        fixture = row["fixture"]
        shedding = fixture["routing"]["record"]
        for name in ("a", "b"):
            obs = fixture["observables"][name]
            if obs["closures"] != 10 or obs["beats"] != 9 or len(obs["beat_records"]) != 9:
                raise ArithmeticError("independent formation/return/beat comparator failed")
        summary.append({"seed": row["seed"], "path_id": fixture["path_id"],
            "record_digest": fixture["record_digest"], "load": shedding.closure_load,
            "cost": shedding.closure_cost, "residual": shedding.residual, "surplus": shedding.surplus,
            "status": shedding.status, "route": shedding.route,
            "partition": fixture["routing"]["partition"],
            "observables": fixture["observables"], "wavelengths": row["wavelengths"]})
    return {"status": "PASS", "paths": count, "summary": summary,
            "scalar_ns": scalar_ns, "multicore_ns": parallel_ns,
            "scalar_over_multicore_time": F(scalar_ns, parallel_ns),
            "requested_workers": workers, "participating_worker_pids": sorted({r[2] for r in parallel}),
            "semantic_sha256": hashlib.sha256(encoded([r[1] for r in scalar])).hexdigest(),
            "timing_scope": "One paired run: seeded path generation, host support materialization, full witness/routing validation and canonical encoding; multicore includes spawn, IPC, merge and shutdown. Excludes model/backend preparation, final artifact compression and PDF/plot writing. No general speedup claim."}, rows


def gpu_study(tables, batch, output, mode):
    if mode == "off":
        return {"execution": {"status": "NOT_REQUESTED"}, "compilation": {"status": "NOT_REQUESTED"}}
    from simulations.chiral_gpu import GPUUnavailable, compile_kernel, probe_gpu, run_gpu
    compilation = {}
    try:
        compiled = compile_kernel()
        ptx = compiled.pop("ptx")
        (output / "chiral_batch.ptx").write_bytes(ptx)
        compilation = compiled
    except GPUUnavailable as exc:
        compilation = exc.report
    availability = probe_gpu()
    if availability["status"] != "AVAILABLE":
        return {"execution": availability, "compilation": compilation}
    try:
        executed = run_gpu(tables, batch)
    except GPUUnavailable as exc:
        return {"execution": exc.report, "compilation": compilation}
    return {"execution": executed, "compilation": compilation}


def plot(output, report):
    os.environ["MPLCONFIGDIR"] = str(output / ".matplotlib")
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    rows = report["tracing"]["summary"]
    fig, axes = plt.subplots(1, 2, figsize=(10, 3.8), layout="constrained")
    axes[0].bar([r["seed"] for r in rows], [float(r["residual"]) for r in rows],
                color=["#b3523c" if r["surplus"] else "#416a83" for r in rows])
    axes[0].axhline(0, color="black", linewidth=.7)
    axes[0].set(title="Declared load minus cost", xlabel="Retained path seed", ylabel="Exact count-derived residual")
    t = report["tracing"]
    axes[1].bar(["Scalar CPU", "Multicore CPU"], [t["scalar_ns"]/1e9, t["multicore_ns"]/1e9], color=["#416a83", "#368b82"])
    axes[1].set(title="Complete host tracing and admission", ylabel="Wall time (seconds, display conversion)")
    axes[1].text(.03,.94, f"{t['paths']} paths; one paired run\nSpawn / IPC included; final files excluded", transform=axes[1].transAxes, va="top", fontsize=9,
                 bbox={"facecolor": "white", "alpha": .9, "edgecolor": "none", "pad": 3})
    fig.savefig(output / "routing-parallel.png", dpi=180)
    plt.close(fig)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "build/routing-parallel")
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--paths", type=int, default=256)
    parser.add_argument("--traced-paths", type=int, default=16)
    parser.add_argument("--gpu", choices=("auto", "required", "off"), default="auto")
    parser.add_argument("--plots", action="store_true")
    parser.add_argument("--seconds-per-index")
    parser.add_argument("--cycles-per-beat")
    parser.add_argument("--phase-speed-m-s")
    parser.add_argument("--map-provenance")
    args = parser.parse_args()
    if not 1 <= args.workers <= 64 or not 1 <= args.traced_paths <= args.paths <= 125000:
        parser.error("require workers in 1..64 and 1 <= traced-paths <= paths <= 125000")
    operands = (args.seconds_per_index, args.cycles_per_beat, args.phase_speed_m_s, args.map_provenance)
    if any(x is not None for x in operands) and not all(x is not None for x in operands):
        parser.error("wavelength mapping requires all four explicit map operands")
    map_operands = operands if operands[0] is not None else None
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    report = {"format": "aod-dec.routing-parallel-study", "version": 1, "status": "RUNNING",
              "source_sha256": source_hashes(), "platform": platform.platform(), "python": sys.version,
              "logical_cpus": os.cpu_count(), "model_execution_boundary":
              "Application supplies transition tables, tickets, supports, load bridge and routing/retention declarations. Execution backends apply supplied tables. Full host producer checks formation, closures, lineage and route witnesses."}
    try:
        start = time.perf_counter_ns()
        config = CollisionConfig(contact_gain=F(0))
        tables = compile_model_tables(config, ticket_denominator=64)
        batch = build_ticket_batch(config, range(args.paths), ticket_denominator=64)
        report["preparation_ns"] = time.perf_counter_ns() - start
        scalar = run_scalar(tables, batch)
        parallel = run_multicore(tables, batch, workers=args.workers, chunksize=max(1, args.paths // (4 * args.workers)))
        if scalar["semantic_sha256"] != parallel["semantic_sha256"]:
            raise ArithmeticError("scalar and multicore transition records differ")
        report["retained_reference"] = retained_comparison(batch, scalar)
        gpu = gpu_study(tables, batch, output, args.gpu)
        if gpu["execution"]["status"] == "EXECUTED" and gpu["execution"]["semantic_sha256"] != scalar["semantic_sha256"]:
            raise ArithmeticError("GPU transition records differ from the exact scalar reference")
        report["gpu"] = gpu
        executions = {"scalar": scalar, "multicore": parallel}
        report["candidate_execution"] = {name: {k: v for k, v in result.items() if k != "rows"}
                                         for name, result in executions.items()}
        report["candidate_execution"]["scalar_over_multicore_time"] = F(scalar["execution"]["elapsed_ns"], parallel["execution"]["elapsed_ns"])
        report["tracing"], traces = trace_study(args.traced_paths, args.workers, map_operands)
        report["full_trace_candidate_comparison"] = trace_candidate_comparison(batch, scalar, traces)
        report["artifacts"] = [compressed(output / "traces.json.gz", traces),
                               compressed(output / "candidate-records.json.gz", {"tables": tables, "batch": batch, **executions})]
        if gpu["compilation"]["status"] == "COMPILED":
            report["artifacts"].append({"file": "chiral_batch.ptx", "sha256": digest(output / "chiral_batch.ptx"),
                                        "scope": "Offline CUDA compilation artifact; execution status is separate."})
        if args.plots:
            plot(output, report)
            report["artifacts"].append({"file": "routing-parallel.png", "sha256": digest(output / "routing-parallel.png")})
        report["limits"] = {"physical_proton_collision": "NOT_INSTANTIATED",
                            "GPU_full_witness_admission": "NOT_IMPLEMENTED; host validates all materialized witnesses",
                            "wavelength_map": "NOT_INSTANTIATED" if map_operands is None else "CONDITIONAL_MAP",
                            "general_GPU_Dec_compiler_target": "NOT_IMPLEMENTED"}
        if source_hashes() != report["source_sha256"]:
            raise RuntimeError("bound study sources changed during execution")
        if args.gpu == "required" and gpu["execution"]["status"] != "EXECUTED":
            raise RuntimeError("required GPU execution unavailable; no CPU fallback is reported as GPU execution")
        report["status"] = "PASS"
    except Exception as exc:
        report.update(status="FAIL", error=f"{type(exc).__name__}: {exc}")
        raise
    finally:
        save(output / "results.json", report)
    print(json.dumps({"status": report["status"], "GPU": report["gpu"]["execution"]["status"],
                      "paths": args.paths, "traced_paths": args.traced_paths, "output": str(output)}))


if __name__ == "__main__":
    main()
