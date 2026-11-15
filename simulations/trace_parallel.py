"""Source-bound parallel replay of complete named boundary schedules.

Workers never write the archive, split causal histories, or replace native
particle/path admission. The coordinator retains one bounded immutable source
snapshot and checks complete per-occurrence results before deterministic merge.
"""
from concurrent.futures import FIRST_COMPLETED, ProcessPoolExecutor, wait
from fractions import Fraction as Q
import hashlib
import json
import multiprocessing
import os
from pathlib import Path
import time

from . import region_engine as region
from .trace_detectors import _decode_ratios, replay_boundaries
from .trace_store import TraceIncomplete, TraceLimits, TraceStore, canonical_bytes


ROOT = Path(__file__).resolve().parents[1]
DOMAIN = "native region cuts; not a trace-ordinal participation window"
POLICY = TraceLimits(max_event_bytes=32*1024*1024, max_storage_bytes=128*1024*1024,
                     max_nodes=1048576)


def require(condition, message):
    if not condition:
        raise TraceIncomplete(message)


def source_bindings(backend):
    paths = ["simulations/trace_parallel.py", "simulations/trace_detectors.py",
             "simulations/trace_store.py", "simulations/region_engine.py",
             "simulations/boundary_accounting.py", "runtime/dec_core.py"]
    if backend == "gpu":
        paths.extend(("simulations/boundary_accelerator.py", "simulations/kernels/boundary_incidence.cu",
                      "simulations/chiral_gpu.py"))
    result = {path: hashlib.sha256((ROOT/path).read_bytes()).hexdigest() for path in paths}
    if backend in ("compiled-cpu", "compiled-gpu"):
        from .compiled_boundaries import source_bindings as compiler_bindings
        result.update(compiler_bindings(backend))
    return result


def fingerprint(value):
    return hashlib.sha256(canonical_bytes(value, POLICY)).hexdigest()


def _archive_binding(store):
    status = store.status()
    return {"run_id": store.run_id, "manifest_sha256": fingerprint(store.manifest),
            "chain": status["chain"], "events": status["events"], "coverage": status["coverage"]}


def _frame(raw):
    row = _decode_ratios(raw)
    return region.Frame(region.Region(row["before"]["name"], tuple(row["before"]["cells"])),
        region.Region(row["after"]["name"], tuple(row["after"]["cells"])), row["kind"],
        tuple(region.SweepSample(**sample) for sample in row["sweep"]))


def _ledger(events):
    context, commits = None, []
    for event in events:
        payload = _decode_ratios(event["payload"])
        current = (tuple(payload["cells"]), payload["unit"], payload["scale"], event["scope_id"], payload["domain"])
        require(context in (None, current), "region stream changed its bound context")
        context = current
        row = payload["row"]
        commits.append(region.CutRecord(row["cut"], row["window"],
            tuple(region.Channel(**c) for c in row["channels"]),
            tuple(region.PendingProjection(**c) for c in row["obligations"]),
            tuple(region.SweepSample(**c) for c in row["sweep_field"]), row["snapshot"]))
    require(context is not None, "boundary view has no committed region context")
    return region.Ledger(*context, tuple(commits))


def _capture(path, scope, name, max_events, max_bytes, coverage_scope):
    """Read under one SQLite snapshot; retain original IDs, not new ordinals."""
    with TraceStore.open(path) as store:
        store.db.execute("BEGIN")
        try:
            before = _archive_binding(store)
            regions, views, count, used, view_bytes = {}, {}, 0, 0, 0
            for event in store.iter_events():
                if event["kind"] not in ("region.commit", "detector.boundary_view"):
                    continue
                if scope is not None and event["scope_id"] != scope:
                    continue
                if event["kind"] == "detector.boundary_view" and name is not None and event["payload"].get("boundary") != name:
                    continue
                encoded = canonical_bytes(event, store.limits)
                count += 1
                used += len(encoded)
                require(count <= max_events and used <= max_bytes, "parallel boundary snapshot budget exhausted")
                if event["kind"] == "region.commit":
                    regions.setdefault(event["scope_id"], []).append(event)
                else:
                    boundary = event["payload"]["boundary"]
                    require(type(boundary) is str and boundary, "exact boundary identity required")
                    views.setdefault((event["scope_id"], boundary), []).append(event)
                    view_bytes += len(encoded)
            require(_archive_binding(store) == before, "archive changed while capturing boundary snapshot")
            jobs, output_keys, ledgers = [], set(), {}
            for key, captured in sorted(views.items()):
                scope_id, boundary = key
                output_key = scope_id + "/" + boundary
                require(output_key not in output_keys, "ambiguous joined boundary identity")
                output_keys.add(output_key)
                events = tuple(regions.get(scope_id, ()))
                if scope_id not in ledgers:
                    ledgers[scope_id] = _ledger(events)
                ledger = ledgers[scope_id]
                require(len(captured) == len(events), "boundary frame/commit coverage differs")
                frames = []
                for source, view, cut in zip(events, captured, ledger.commits):
                    payload = view["payload"]
                    row = payload["row"]
                    require(view["causal_parents"] == [source["event_id"]], "boundary frame lost its original causal commit")
                    require(type(row["cut"]) is int and type(row["window"]) is int and
                            (row["cut"], row["window"], row["snapshot"], payload["unit"]) ==
                            (cut.cut, cut.window, cut.snapshot, ledger.unit), "boundary frame/source snapshot differs")
                    frames.append(_frame(payload["frame"]))
                job = {"key": key, "events": events, "frames": tuple(frames), "ledger": ledger,
                       "frame_evidence": tuple(view["event_id"] for view in captured),
                       "archive": before, "max_events": max_events, "max_bytes": max_bytes}
                job["coverage_scope"] = coverage_scope
                jobs.append((job, fingerprint(job)))
            return before, jobs, view_bytes, used
        finally:
            store.db.execute("ROLLBACK")


def _run_job(job, binding, backend, native_workers=1):
    started = time.perf_counter_ns()
    require(source_bindings(backend) == binding, "worker implementation differs from source binding")
    profile, auditor = {}, None
    if backend == "gpu":
        from .boundary_accelerator import profiled_audit_boundary
        def auditor(*args, **kwargs):
            answer = profiled_audit_boundary(*args, **kwargs, backend="gpu")
            profile.update(answer["profile"])
            return answer["audit"]
    elif backend in ("compiled-cpu", "compiled-gpu"):
        from .compiled_boundaries import profiled_audit_boundary
        def auditor(*args, **kwargs):
            answer = profiled_audit_boundary(*args, **kwargs, backend=backend, workers=native_workers)
            profile.update(answer["profile"])
            return answer["audit"]
    scope, name = job["key"]
    result = replay_boundaries(job["events"], name=name, cells=job["frames"][0].before.cells,
        frames=job["frames"], scope=scope, max_events=job["max_events"],
        max_bytes=job["max_bytes"], auditor=auditor, coverage_scope=job["coverage_scope"])
    require(source_bindings(backend) == binding, "worker implementation changed during replay")
    return {"key": job["key"], "input_sha256": fingerprint(job), "sources": binding,
            "result": result, "pid": os.getpid(), "profile": profile,
            "replay_ns": time.perf_counter_ns()-started}


def _verify_result(job, result):
    """Independent linear merge oracle for full source-bound occurrence rows.

    This does not redo the worker's archive construction and native accounting
    validation. It checks every returned row, not merely an aggregate label.
    """
    require(result["evidence"] == [event["event_id"] for event in job["events"]],
            "worker evidence omitted, reordered or changed a source commit")
    require(result["scope"] == region.BoundaryAudit.__dataclass_fields__["coverage_claim"].default,
            "worker changed the declared source-coverage claim")
    ledger, rows, totals, contributions = job["ledger"], [], {key: Q() for key in ("cross", "sweep", "shedding", "current")}, []
    pending, previous = [], None
    for cut, frame in zip(ledger.commits, job["frames"]):
        before, after = set(frame.before.cells), set(frame.after.cells)
        require(previous is None or before == previous, "disconnected merged boundary schedule")
        previous = after
        require(before <= set(ledger.cells) and after <= set(ledger.cells), "merged boundary outside source universe")
        field = {sample.cell: sample for sample in cut.sweep_field}
        require(all(field.get(sample.cell) == sample for sample in frame.sweep), "merged sweep lost same-cut source field")
        pending.extend(cut.obligations)
        unresolved = [item.identity for item in pending if (item.source in after) != (item.target in after)]
        require(job["coverage_scope"] != "full" or not unresolved,
                "complete full-current worker result dropped unresolved signed projection")
        values = {key: Q() for key in totals}
        values["sweep"] = sum((sample.value*(1 if sample.cell in after else -1) for sample in frame.sweep), Q())
        views, internal, exterior, inactive, cross_ids, shed_ids = [], [], [], [], [], []
        for event in cut.channels:
            orientation = int(event.source in after)-int(event.target in after)
            projected = event.value*orientation if event.available else Q()
            views.append({"identity": event.identity, "physical_id": event.physical_id,
                "source": event.source, "target": event.target, "orientation": orientation,
                "input_value": str(event.value), "source_projected_value": str(projected),
                "projected_value": str(projected), "kind": event.kind, "available": event.available})
            if orientation and event.available:
                values[event.kind] += projected
                (cross_ids if event.kind == "cross" else shed_ids).append(event.identity)
            if event.source in after and event.target in after:
                internal.append(event.identity)
            elif event.source not in after and event.target not in after:
                exterior.append(event.identity)
            if not event.available:
                inactive.append(event.identity)
        contributions.extend(cross_ids+shed_ids)
        values["current"] = values["cross"]-values["sweep"]+values["shedding"]
        rows.append({"cut": cut.cut, "window": cut.window, "snapshot": cut.snapshot,
            **{key: str(value) for key, value in values.items()}, "internal_ids": internal,
            "exterior_ids": exterior, "inactive_ids": inactive, "unresolved_projection_ids": unresolved,
            "views": views, "stock_balance_status": "NOT_ASSERTED"})
        for key in totals:
            totals[key] += values[key]
    expected_total = {"boundary_id": job["key"][1], "scale": ledger.scale, "unit": ledger.unit,
        "cuts": [cut.cut for cut in ledger.commits], **{key: str(value) for key, value in totals.items()},
        "contribution_ids": contributions, "stock_balance_status": "NOT_ASSERTED"}
    require(canonical_bytes(result["rows"], POLICY) == canonical_bytes(rows, POLICY),
            "worker per-occurrence rows differ from exact joined source projection")
    require(canonical_bytes(result["total"], POLICY) == canonical_bytes(expected_total, POLICY) and
            result["value"] == str(totals["current"]) and result["unit"] == ledger.unit,
            "worker total/context differs from complete occurrence rows")


def _partial(reason, evidence=()):
    return {"status": "unresolved", "coverage": "partial", "value": None,
            "reason": str(reason), "evidence": list(evidence)}


def _packets(jobs, binding, backend, workers, metrics):
    if workers == 1:
        for job, _ in jobs:
            try:
                packet = (_run_job(job, binding, backend, metrics["native_workers_requested"])
                          if backend in ("compiled-cpu", "compiled-gpu") else _run_job(job, binding, backend))
                yield job, packet, None
            except Exception as exc:
                if backend in ("gpu", "compiled-gpu"):
                    from .chiral_gpu import GPUUnavailable
                    if isinstance(exc, GPUUnavailable):
                        raise
                yield job, None, exc
        return
    # Bound outstanding work to the number of worker processes. Complete frame
    # schedules remain intact; no worker assigns new global archive ordinals.
    with ProcessPoolExecutor(max_workers=workers, mp_context=multiprocessing.get_context("spawn")) as executor:
        pending, iterator = {}, iter(jobs)
        def submit():
            try:
                job, _ = next(iterator)
            except StopIteration:
                return
            pending[executor.submit(_run_job, job, binding, backend)] = job
            metrics["peak_pending_jobs"] = max(metrics["peak_pending_jobs"], len(pending))
        for _ in range(workers):
            submit()
        while pending:
            completed, _ = wait(pending, return_when=FIRST_COMPLETED)
            for future in completed:
                job = pending.pop(future)
                try:
                    yield job, future.result(), None
                except Exception as exc:
                    yield job, None, exc
                submit()


def boundary_views(path, *, scope=None, name=None, workers=1, backend="scalar",
                   max_events=10000, max_bytes=8000000, coverage_scope="full"):
    for label, value in (("scope", scope), ("name", name)):
        require(value is None or (type(value) is str and value), "nonempty " + label + " required")
    require(type(workers) is int and 1 <= workers <= 32, "workers must be an exact integer in 1..32")
    require(backend in ("scalar", "multicore", "gpu", "compiled-cpu", "compiled-gpu"), "unknown boundary replay backend")
    require(backend != "multicore" or workers >= 2, "multicore backend requires at least two workers")
    require(backend not in ("gpu", "compiled-gpu") or workers == 1, "GPU boundary replay uses one device-owning worker")
    require(coverage_scope in ("full", "declared-current"), "explicit current coverage scope required")
    require(type(max_events) is int and max_events > 0 and type(max_bytes) is int and
            0 < max_bytes <= POLICY.max_event_bytes//2, "bounded exact snapshot budgets required")
    compiled = backend in ("compiled-cpu", "compiled-gpu")
    actual_backend = backend if compiled or backend == "gpu" else "scalar"
    started = time.perf_counter_ns()
    report = {"boundaries": {}, "archive_coverage": "partial", "coverage": "partial", "domain": DOMAIN,
              "retained_query_bytes": 0, "failures": [], "coverage_scope": coverage_scope}
    metrics = {"backend": "multicore" if workers > 1 else actual_backend, "workers_requested": workers,
               "workers_used": 0, "pool_width": 0, "worker_pids": [], "peak_pending_jobs": 0,
               "start_method": "spawn" if workers > 1 else "inline", "jobs": []}
    report["execution"] = metrics
    metrics["native_workers_requested"] = workers if backend == "compiled-cpu" else 1
    expected_jobs = {}
    try:
        binding = source_bindings(actual_backend)
        capture_start = time.perf_counter_ns()
        archive, jobs, view_bytes, snapshot_bytes = _capture(path, scope, name, max_events, max_bytes, coverage_scope)
        metrics["capture_ns"] = time.perf_counter_ns()-capture_start
        metrics["snapshot_bytes"] = snapshot_bytes
        report.update(archive_coverage=archive["coverage"], retained_query_bytes=view_bytes,
                      source_binding={"archive": archive, "implementation_sha256": binding})
        expected = {job["key"]: digest for job, digest in jobs}
        expected_jobs = {job["key"]: job for job, _ in jobs}
        report["source_binding"]["jobs"] = [{"key": list(job["key"]), "input_sha256": digest,
            "source_evidence": [event["event_id"] for event in job["events"]],
            "frame_evidence": list(job["frame_evidence"])} for job, digest in jobs]
        metrics["pool_width"] = min(1 if compiled else workers, len(jobs))
        metrics["backend_requested"] = backend
        metrics["backend"] = backend if compiled or backend == "gpu" else "multicore" if metrics["pool_width"] > 1 else "scalar"
        metrics["start_method"] = "native-threads" if backend == "compiled-cpu" else "spawn" if metrics["pool_width"] > 1 else "inline"
        if jobs:
            metrics["peak_pending_jobs"] = 1
        completed, pids, results = set(), set(), {}
        for job, packet, failure in _packets(jobs, binding, actual_backend, max(1, metrics["pool_width"]), metrics):
            key = job["key"]
            try:
                require(key not in completed, "duplicate boundary job completion")
                completed.add(key)
                if failure is not None:
                    raise TraceIncomplete("boundary worker failed: " + str(failure))
                require(type(packet) is dict and packet["key"] == key and packet["input_sha256"] == expected[key]
                        and packet["sources"] == binding, "worker result belongs to another source/job")
                result = packet["result"]
                require(result["coverage"] in ("complete", "partial", "unavailable"), "invalid worker coverage")
                if result["coverage"] == "complete":
                    require(result["status"] == "verified", "complete worker result is not verified")
                    _verify_result(job, result)
                else:
                    # A partial worker packet is not a proof of any numeric
                    # row/value. Retain only its reason and checked source IDs.
                    evidence = result.get("evidence", [])
                    source_ids = [event["event_id"] for event in job["events"]]
                    require(type(evidence) is list and evidence == source_ids[:len(evidence)],
                            "partial worker evidence changed source commitment order")
                    result = _partial(result.get("reason", "boundary worker coverage incomplete"), evidence)
                pids.add(packet["pid"])
                metrics["jobs"].append({"key": list(key), "input_sha256": expected[key],
                    "frame_evidence": list(job["frame_evidence"]), "pid": packet["pid"],
                    "replay_ns": packet["replay_ns"], "profile": packet["profile"]})
                results[key] = result
            except (ValueError, TypeError, KeyError, AttributeError) as exc:
                results[key] = _partial(exc)
                report["failures"].append({"job": list(key), "reason": str(exc)})
        for job, _ in jobs:
            key = job["key"]
            if key not in completed:
                results[key] = _partial("missing boundary worker result")
                report["failures"].append({"job": list(key), "reason": "missing boundary worker result"})
        report["boundaries"] = {scope_id+"/"+boundary: results[(scope_id, boundary)] for scope_id, boundary in sorted(results)}
        metrics["jobs"].sort(key=lambda item: item["key"])
        metrics["worker_pids"] = sorted(pids)
        metrics["workers_used"] = len(pids)
        with TraceStore.open(path) as store:
            require(_archive_binding(store) == archive, "archive changed during boundary replay")
        require(source_bindings(actual_backend) == binding, "implementation changed during boundary replay")
        report["coverage"] = ("complete" if results and archive["coverage"] == "complete" and not report["failures"]
            and all(result["coverage"] == "complete" for result in results.values()) else "partial" if results else "unavailable")
    except (ValueError, OSError, KeyError, TypeError, AttributeError, RuntimeError) as exc:
        if actual_backend in ("gpu", "compiled-gpu"):
            from .chiral_gpu import GPUUnavailable
            if isinstance(exc, GPUUnavailable):
                raise
        report["coverage"] = "partial"
        report["failures"].append({"stage": "capture_or_replay", "reason": str(exc)})
        # Stale or unbound results must not retain per-boundary verified claims.
        for key, job in expected_jobs.items():
            report["boundaries"].setdefault(key[0]+"/"+key[1],
                _partial(exc, [event["event_id"] for event in job["events"]]))
        report["boundaries"] = {key: _partial(exc, value.get("evidence", ()))
                                for key, value in report["boundaries"].items()}
    metrics["total_ns"] = time.perf_counter_ns()-started
    metrics["scope"] = "complete boundary audit jobs; native formation/path admission remains joined on the host"
    return report
