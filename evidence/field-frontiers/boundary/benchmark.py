"""Cost-inclusive shared-boundary comparison against the frozen baseline helper."""
from dataclasses import replace
from fractions import Fraction as Q
import hashlib
import importlib.util
import json
from pathlib import Path
import platform
import statistics
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
REPO = ROOT / "aod-dec"
sys.path.insert(0, str(REPO))
from simulations import reduced_boundary_response as new
from simulations import region_engine as region
from simulations.vendor import aod_manual_native as native
spec = importlib.util.spec_from_file_location("frozen_boundary_baseline", Path(__file__).with_name("baseline_reduced_boundary_response.py"))
old = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = old
spec.loader.exec_module(old)


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def fixture(count):
    cells = tuple(f"cell:{i:03d}" for i in range(count))
    states = {}
    for i, cell in enumerate(cells):
        wave = native.recurrent(cell, n=12, spacing=5)
        pressure = Q(18 if i in (0, count-1) else 27)
        states[cell] = replace(native.initial_state(wave, pressure), current=Q(1+i%7, 7))
    channels = tuple(region.Channel(f"edge:{i}", f"physical:{i}", a, b,
        9 * Q(1, 1+i%3) * (states[a].current-states[b].current), "1"*64)
        for i,(a,b) in enumerate(zip(cells,cells[1:])))
    response = old.response_step(states, channels)
    threshold = sum(response["before_stock"].values()) / sum(s.pressure for s in states.values())
    select = lambda values: tuple(cell for cell in cells if values[cell].current >= threshold)
    before, after = select(states), select(response["after"])
    frames = ((region.Region("selected", before), region.Region("selected", after)),
              (region.Region("complement", tuple(c for c in cells if c not in before)),
               region.Region("complement", tuple(c for c in cells if c not in after))),
              (region.Region("whole", cells), region.Region("whole", cells)),
              (region.Region("stationary", before), region.Region("stationary", before)))
    return states, channels, response, frames


def bench(count, repeats=11):
    states, channels, expected_response, frames = fixture(count)
    options = dict(scope="medium-response-benchmark", window=3, window_domain=new.WINDOW_DOMAIN)
    def baseline():
        return tuple(old.boundary_step(states, expected_response["after"], channels, a, b, **options)
                     for a,b in frames)
    def shared():
        context = new.boundary_context(states, channels, **options)
        result = context.response()  # Include the copy needed by the actual caller.
        if result != expected_response:
            raise ValueError("response differs")
        return tuple(new.boundary_view(context, a, b) for a,b in frames)
    expected = baseline()
    actual = shared()
    if expected != actual or region.fingerprint(expected) != region.fingerprint(actual):
        raise ValueError("complete boundary records differ")
    partition = region.audit_partition(actual[2]["audit"], (actual[0]["audit"], actual[1]["audit"]))
    times = {"original_four_boundary_steps": [], "context_creation_response_copy_four_views": []}
    functions = (("original_four_boundary_steps",baseline),
                 ("context_creation_response_copy_four_views",shared))
    for repetition in range(repeats):
        for name, function in functions if repetition % 2 == 0 else reversed(functions):
            start = time.perf_counter()
            records = function()
            times[name].append(time.perf_counter()-start)
            if records != expected:
                raise ValueError("repeated records differ")
    medians = {k:statistics.median(v) for k,v in times.items()}
    return {"cells":count,"returns_per_cell":12,"channels":len(channels),"repeats":repeats,
            "timing_seconds":times,"median_seconds":medians,
            "ratio_original_to_shared":medians["original_four_boundary_steps"] / medians["context_creation_response_copy_four_views"],
            "exact_full_record_equality":True,"record_sha256":region.fingerprint(actual),
            "partition_status":partition["status"],"partition_total_residual":str(partition["total_current_residual"]),
            "moving_selected_membership":frames[0][0].cells != frames[0][1].cells}

if __name__ == "__main__":
    sources = {str(Path(path).relative_to(ROOT)):sha(path) for path in
               (new.__file__, old.__file__, region.__file__, native.__file__, __file__)}
    result = {"python":sys.version,"platform":platform.platform(),"source_sha256":sources,
              "baseline_commit":"430fb14","mode":"optimized" if sys.flags.optimize else "normal",
              "measurement_scope":"Includes creation/validation, response copy and all four views; input preparation shared and excluded; full equality checked outside timing; no full-engine speedup claim",
              "benchmarks":[bench(24),bench(48)]}
    if any(sha(ROOT/path) != value for path,value in sources.items()):
        raise ValueError("sources changed during benchmark")
    target = Path(__file__).with_name("benchmark-"+result["mode"]+".json")
    target.write_text(json.dumps(result,indent=2,sort_keys=True)+"\n")
    print(json.dumps({"output":str(target),"ratios":[r["ratio_original_to_shared"] for r in result["benchmarks"]]},indent=2))
