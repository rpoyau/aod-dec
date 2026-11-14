#!/usr/bin/env python3
"""Exact D0 region applications of the shared native scoped-current equations.

The declared preparation tables supply channels, coupling, windows and boundary
motion. The region engine supplies accounting, never an application probability
or force law. The fixtures do not claim certified maser or Earth/Moon dynamics.
"""
import argparse
from dataclasses import asdict, is_dataclass, replace
from fractions import Fraction as Q
import hashlib
import importlib.util
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
PROJECT = ROOT.parent


def load(name, candidates):
    path = next((p for p in candidates if p.is_file()), None)
    if path is None:
        raise FileNotFoundError("No bound module found: " + name)
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


r = load("region_engine", (HERE / "region_engine.py", ROOT / "application/region_engine.py"))
n = load("region_example_native", (HERE / "vendor/aod_manual_native.py",
    PROJECT / "AOD_DEC_R04_IMPLEMENTATION_2026-11-14/downstream/simulations/vendor/aod_manual_native.py"))
UNIT = "native-pressure-current"
DOMAIN = "native-return-count"
EXPECTED_NATIVE = "562b3eb53ad6dd13c0801c43bd6e918110083ab4d12592c024b3007b634e2554"


def encode(value):
    if is_dataclass(value):
        return encode(asdict(value))
    if isinstance(value, Q):
        return str(value)
    if isinstance(value, dict):
        return {str(k): encode(v) for k, v in value.items()}
    if isinstance(value, (tuple, list, set, frozenset)):
        return [encode(v) for v in value]
    return value


def digest(value):
    return hashlib.sha256(json.dumps(encode(value), sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def file_sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def computed_channel(example, cut, window, identity, source, target, multiplier,
                     *, kind="cross", available=True):
    """Use bound native participation and signed-flux helpers on declared D0 inputs."""
    scope = "declared:" + example
    rcd = n.RCD(source, target, scope, ("D0:declared-contact:" + identity,), 3, (1, 1))
    participation = n.participation(rcd, window, DOMAIN)
    capacity, intensity, lag = Q(3), Q(1, 3), Q(multiplier)
    pressure, attention = capacity * participation, lag * intensity
    occurrence = example + ":" + identity + ":" + str(cut)
    flux = n.aod_flux(((source, target, pressure, attention, occurrence),))
    value = flux[source, target, occurrence]
    operand = {"source_class": "D0 declared scoped-flux preparation", "example": example,
        "cut": cut, "scope": scope, "source": source, "target": target, "identity": identity,
        "RCD": rcd, "duration": rcd.duration(), "window": window, "capacity": capacity,
        "lag": lag, "intensity": intensity, "attention": attention, "participation": participation,
        "pressure": pressure, "signed_current": value, "kind": kind, "available": available,
        "native_helper_sha256": EXPECTED_NATIVE,
        "effective_current": value if available else Q(0),
        "source_anchors": ["manual:boundary-current:shared-setup", "manual:boundary-current:worked-substitution"]}
    channel = r.Channel(occurrence, occurrence, source, target, value, digest(operand), kind, available)
    return channel, operand


def stationary(region):
    return r.Frame(region, region, "stationary", ())


def currents(audit):
    return tuple(row["current"] for row in audit.rows)


def validate_preparation(ledger, preparations):
    """Bind complete interface inventory to immutable source preparation, not a selected view."""
    require(len(ledger.commits) == len(preparations), "preparation cut omitted")
    for event, prepared in zip(ledger.commits, preparations):
        require((event.cut, event.window) == (prepared["cut"], prepared["window"]),
                "preparation cut/window replaced")
        expected = prepared["channels"]
        require(event.channels == expected, "channel omitted, replaced or added relative to complete preparation")
    return True


def native_stokes(ledger, frames):
    rows = []
    for event, frame in zip(ledger.commits, frames):
        channels = tuple((c.source, c.target, Q(1), c.value if c.available else Q(0), c.identity)
                         for c in event.channels)
        values = n.stokes(n.aod_flux(channels), set(frame.after.cells))
        rows.append({"cut": event.cut, **values})
    return tuple(rows)


def check_boundary_against_native(ledger, frames, audit):
    values = native_stokes(ledger, frames)
    require(all(saved["boundary"] == row["cross"] + row["shedding"]
                for saved, row in zip(values, audit.rows)), "native boundary differs from region projection")
    require(all(row["current"] == row["cross"] - row["sweep"] + row["shedding"]
                for row in audit.rows), "moving-current split mismatch")
    return values


def run_maser():
    name = "maser"
    cells = ("emitter", "cavity", "output", "exterior")
    ledger = r.Ledger(cells, UNIT, 0, "declared:" + name, DOMAIN)
    schedule = ((100, 4, True), (101, 4, False), (102, 4, True), (103, 6, True))
    interfaces = (("pump", "exterior", "emitter", 2),
                  ("emission", "emitter", "cavity", 2),
                  ("output-coupling", "cavity", "output", 1),
                  ("detected-output", "output", "exterior", 1),
                  ("cavity-return", "cavity", "exterior", 1))
    operands, preparations, prefixes = [], [], []
    for cut, window, enabled in schedule:
        made = tuple(computed_channel(name, cut, window, identity, source, target, multiplier,
                                      available=enabled)
                     for identity, source, target, multiplier in interfaces)
        channels = tuple(pair[0] for pair in made)
        operands.extend(pair[1] for pair in made)
        before = ledger
        ledger = r.commit(ledger, cut, window, channels)
        require(ledger.commits[:-1] == before.commits, "coupling changed committed prefix")
        prefixes.append({"cut": cut, "previous_commit_count": len(before.commits),
                         "retained_prefix_sha256": digest(before.commits)})
        preparations.append({"cut": cut, "window": window, "channels": channels})
    validate_preparation(ledger, preparations)
    regions = {cell: r.Region(cell, (cell,)) for cell in cells}
    regions["apparatus"] = r.region_union("apparatus", regions["emitter"], regions["cavity"], regions["output"])
    regions["universe"] = r.region_union("universe", regions["apparatus"], regions["exterior"])
    frames = {key: tuple(stationary(region) for _ in schedule) for key, region in regions.items()}
    audits = {key: r.audit_boundary(ledger, key, frames[key]) for key in regions}
    comparisons = {key: check_boundary_against_native(ledger, frames[key], audit)
                   for key, audit in audits.items()}
    require(all(all(value == 0 for value in currents(audit)) for audit in audits.values()),
            "balanced maser preparation does not balance its region current")
    partitions = {"apparatus": r.audit_partition(audits["apparatus"],
        tuple(audits[key] for key in ("emitter", "cavity", "output"))),
        "universe": r.audit_partition(audits["universe"], (audits["apparatus"], audits["exterior"]))}
    pump = tuple(next(c.value if c.available else Q(0) for c in e.channels if ":pump:" in c.identity)
                 for e in ledger.commits)
    output = tuple(next(c.value if c.available else Q(0) for c in e.channels if ":detected-output:" in c.identity)
                   for e in ledger.commits)
    require(pump == (Q(8), Q(0), Q(8), Q(10)) and output == (Q(4), Q(0), Q(4), Q(5)),
            "maser declared window/gate values mismatch")
    require(len(ledger.commits[1].channels) == 5 and all(not c.available for c in ledger.commits[1].channels),
            "dropout erased its channel declarations")
    return {"example": name, "evidence_class": "D0 declared finite accounting application",
        "preparation": {"schedule": schedule, "interfaces": interfaces,
            "interpretation": "Declared matched pump and two outward branches; the boundary current is net, not absence of transport.",
            "not_supplied": ["hyperfine preparation/transition law", "cavity geometry and electromagnetic response", "SI power/phase observation map"]},
        "ledger": ledger, "channel_operands": tuple(operands), "complete_preparation": tuple(preparations),
        "prefix_checks": tuple(prefixes), "audits": audits, "native_stokes": comparisons, "partitions": partitions,
        "summary": {"pump_inward_magnitude": pump, "detected_output_outward": output,
            "cavity_return_outward": output, "pump_window_total": sum(pump),
            "detected_output_window_total": sum(output), "cavity_return_window_total": sum(output),
            "apparatus_net": currents(audits["apparatus"]), "whole_universe_net": currents(audits["universe"]),
            "retained_channel_occurrences": sum(len(e.channels) for e in ledger.commits),
            "gate_off_declared_occurrences": len(ledger.commits[1].channels)}}


def run_earth_water_moon():
    name = "earth-water-moon"
    cells = ("earth-core", "water-inner", "water-outer", "surface", "moon", "exterior")
    ledger = r.Ledger(cells, UNIT, 0, "declared:" + name, DOMAIN)
    earth = r.Region("earth-core", ("earth-core",))
    moon = r.Region("moon", ("moon",))
    outer0 = r.Region("earth-water-outer", ("earth-core", "water-inner", "water-outer"))
    outer1 = r.Region("earth-water-outer", outer0.cells + ("surface",))
    shell0 = r.region_difference("water-shell", outer0, earth)
    shell1 = r.region_difference("water-shell", outer1, earth)
    universe = r.Region("universe", cells)
    before_shell = (shell0, shell0, shell1, shell1)
    after_shell = (shell0, shell1, shell1, shell0)
    schedule = ((200, True), (201, True), (202, False), (203, True))
    frames = {key: [] for key in ("earth", "water-shell", "earth-water", "moon", "earth-water-moon", "exterior", "universe")}
    operands, preparations, sweep_records = [], [], []
    for i, (cut, moon_coupled) in enumerate(schedule):
        made = tuple(computed_channel(name, cut, 4, identity, source, target, multiplier,
                                      kind=kind, available=available)
             for identity, source, target, multiplier, kind, available in (
                ("core-water", "earth-core", "water-inner", 1, "cross", True),
                ("fluid-internal", "water-inner", "water-outer", 1, "cross", True),
                ("water-moon", "water-outer", "moon", 3, "cross", moon_coupled),
                ("signed-interface-shedding", "water-outer", "exterior", Q(1, 2), "shedding", True)))
        channels = tuple(pair[0] for pair in made)
        operands.extend(pair[1] for pair in made)
        changing = before_shell[i].cells != after_shell[i].cells
        swept = {"cut": cut, "cell": "surface", "declared_measure": Q(3),
            "measure_unit": "native-swept-content", "mapped_current_unit": UNIT,
            "map_factor": Q(1), "mapped_value": Q(3),
            "scope": "D0 finite moving-interface preparation; no volume, density or SI time inferred"}
        sample = r.SweepSample("surface", Q(3), UNIT, 0, digest(swept))
        sweep = (sample,) if changing else ()
        before = ledger
        ledger = r.commit(ledger, cut, 4, channels, sweep_field=sweep)
        require(ledger.commits[:-1] == before.commits, "moving boundary changed committed prefix")
        preparations.append({"cut": cut, "window": 4, "channels": channels})
        if changing:
            sweep_records.append(swept)
        parent_before = r.region_union("earth-water-moon", earth, before_shell[i], moon)
        parent_after = r.region_union("earth-water-moon", earth, after_shell[i], moon)
        outside_before = r.region_difference("exterior", universe, parent_before)
        outside_after = r.region_difference("exterior", universe, parent_after)
        kind = "moving" if changing else "stationary"
        frames["earth"].append(stationary(earth))
        frames["moon"].append(stationary(moon))
        frames["universe"].append(stationary(universe))
        frames["water-shell"].append(r.Frame(before_shell[i], after_shell[i], kind, sweep))
        frames["earth-water"].append(r.Frame(
            r.region_union("earth-water", earth, before_shell[i]),
            r.region_union("earth-water", earth, after_shell[i]), kind, sweep))
        frames["earth-water-moon"].append(r.Frame(parent_before, parent_after, kind, sweep))
        frames["exterior"].append(r.Frame(outside_before, outside_after, kind, sweep))
    frames = {key: tuple(value) for key, value in frames.items()}
    validate_preparation(ledger, preparations)
    audits = {key: r.audit_boundary(ledger, key, value) for key, value in frames.items()}
    comparisons = {key: check_boundary_against_native(ledger, frames[key], audit)
                   for key, audit in audits.items()}
    partitions = {"earth-water": r.audit_partition(audits["earth-water"],
        (audits["earth"], audits["water-shell"])),
        "earth-water-moon": r.audit_partition(audits["earth-water-moon"],
        tuple(audits[key] for key in ("earth", "water-shell", "moon"))),
        "universe": r.audit_partition(audits["universe"],
                                      (audits["earth-water-moon"], audits["exterior"]))}
    require(currents(audits["water-shell"]) == (Q(10), Q(7), Q(-2), Q(13)), "water-shell arithmetic")
    require(currents(audits["earth-water-moon"]) == (Q(2), Q(-1), Q(2), Q(5)), "combined-region arithmetic")
    require(currents(audits["exterior"]) == (Q(-2), Q(1), Q(-2), Q(-5)), "complement arithmetic")
    require(all(value == 0 for value in currents(audits["universe"])), "whole-universe cancellation")
    return {"example": name, "evidence_class": "D0 declared finite accounting application",
        "preparation": {"schedule": schedule, "water_shell": "earth-water-outer minus earth-core",
            "boundary_motion": "stationary, include surface, stationary with Moon coupling disabled, exclude surface and recouple",
            "signed_shedding": "Explicit signed current input 2; not an inferred burden-to-current conversion.",
            "not_supplied": ["Earth/Moon gravitational field solution", "water constitutive/viscous response", "surface motion derived from forcing", "SI tide height or fluid velocity observation map"]},
        "ledger": ledger, "channel_operands": tuple(operands), "complete_preparation": tuple(preparations),
        "swept_measure_and_map": tuple(sweep_records), "audits": audits, "native_stokes": comparisons,
        "partitions": partitions,
        "summary": {key: currents(audit) for key, audit in audits.items()}}


def hostile_checks(results):
    checks = []
    def reject(name, operation):
        try:
            operation()
        except (ValueError, TypeError):
            checks.append({"name": name, "status": "REJECTED"})
        else:
            raise ValueError("Hostile control was accepted: " + name)
    def changed_preparation(ledger, first_channels):
        # Recompute valid commit snapshots: the independent complete preparation
        # binding must still reject a changed source interface family.
        changed = replace(ledger, commits=())
        for i, event in enumerate(ledger.commits):
            changed = r.commit(changed, event.cut, event.window,
                first_channels if i == 0 else event.channels,
                obligations=event.obligations, sweep_field=event.sweep_field)
        return changed
    for result in results:
        ledger = result["ledger"]
        first = ledger.commits[0]
        name = result["example"]
        omitted = changed_preparation(ledger, first.channels[1:])
        reject(name + ": omitted source interface", lambda omitted=omitted, result=result:
               validate_preparation(omitted, result["complete_preparation"]))
        replaced = replace(first.channels[0], value=first.channels[0].value + 1)
        substituted = changed_preparation(ledger, (replaced,) + first.channels[1:])
        reject(name + ": substituted current", lambda substituted=substituted, result=result:
               validate_preparation(substituted, result["complete_preparation"]))
        reject(name + ": deleted cut", lambda ledger=ledger, result=result:
               validate_preparation(replace(ledger, commits=ledger.commits[:-1]), result["complete_preparation"]))
        reject(name + ": reused physical occurrence", lambda ledger=ledger, first=first:
               r.commit(ledger, ledger.commits[-1].cut + 1, 4, (first.channels[0],)))
    return tuple(checks)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--example", choices=("maser", "earth-water-moon", "all"), default="all")
    parser.add_argument("--output", type=Path, default=Path("region-examples.json"))
    args = parser.parse_args()
    require(file_sha(n.__file__) == EXPECTED_NATIVE, "Native helper bytes differ from bound source")
    results = []
    if args.example in ("maser", "all"):
        results.append(run_maser())
    if args.example in ("earth-water-moon", "all"):
        results.append(run_earth_water_moon())
    hostile = hostile_checks(results)
    report = {"status": "PASS", "examples": results, "hostile_controls": hostile,
        "source_sha256": {"native_helper": file_sha(n.__file__), "region_engine": file_sha(r.__file__),
                          "runner": file_sha(__file__)},
        "scope": "Shared exact current accounting on explicit D0 finite preparations; no additional engine transition law",
        "physical_percent_error": None}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(encode(report), indent=2, sort_keys=True) + "\n")
    print(json.dumps({"status": "PASS", "examples": [x["example"] for x in results],
                      "hostile_rejections": len(hostile), "output_sha256": file_sha(args.output)}))


if __name__ == "__main__":
    main()
