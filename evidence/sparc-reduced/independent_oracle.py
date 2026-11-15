#!/usr/bin/env python3
"""Independent rational oracle for the declared reduced radial recurrence.

Only Python's standard library is imported. This checks the numerical reduction;
it does not execute or independently admit native Duon formation histories.
"""

import argparse
import csv
from decimal import Decimal, localcontext
from fractions import Fraction as Q
import hashlib
import json
from pathlib import Path
import re
import sys


CASES = ("spacing", "disabled", "unit")
COLUMNS = ("Rad", "Vobs", "errV", "Vgas", "Vdisk", "Vbul", "SBdisk", "SBbul")
UNITS = ("kpc", "km/s", "km/s", "km/s", "km/s", "km/s", "L/pc^2", "L/pc^2")
DECIMAL = re.compile(r"[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?\Z")


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def read_tables(directory):
    preparation, targets, sources = {}, {}, []
    for path in sorted(directory.glob("*_rotmod.dat")):
        galaxy = path.name.removesuffix("_rotmod.dat")
        raw = path.read_bytes()
        lines = raw.decode("ascii").splitlines()
        column_header = any(tuple(line.lstrip("# ").split()) == COLUMNS for line in lines if line.startswith("#"))
        unit_header = any(tuple(line.lstrip("# ").split()) == UNITS for line in lines if line.startswith("#"))
        if not column_header or not unit_header:
            raise ValueError(f"missing radial schema/units: {path}")
        rows, previous = [], Q(0)
        for line_number, line in enumerate(lines, 1):
            if not line.strip() or line.lstrip().startswith("#"):
                continue
            tokens = line.split()
            if len(tokens) != 8 or not all(DECIMAL.fullmatch(token) for token in tokens):
                raise ValueError(f"invalid finite eight-column row: {path}:{line_number}")
            radius, vobs, errv, gas, disk, bulge, sdisk, sbulge = map(Q, tokens)
            if radius <= previous or vobs <= 0 or errv <= 0 or min(sdisk, sbulge) < 0:
                raise ValueError(f"invalid radial order or data domain: {path}:{line_number}")
            key = (galaxy, radius)
            if key in targets:
                raise ValueError(f"duplicate source identity: {key}")
            rows.append({"radius": radius,
                         "c": radius * (abs(gas) + abs(disk) + abs(bulge)),
                         "b": gas**2 + Q(1, 2)*disk**2 + Q(7, 10)*bulge**2})
            targets[key] = {"u_observed": vobs**2, "errv": errv}
            previous = radius
        if len(rows) < 2:
            raise ValueError(f"at least two radial rows required: {path}")
        preparation[galaxy] = tuple(rows)
        sources.append({"path": str(path.resolve()), "name": path.name,
                        "sha256": hashlib.sha256(raw).hexdigest(), "rows": len(rows)})
    if not preparation:
        raise ValueError("no radial input tables")
    return preparation, targets, sources


def evolve(rows, case):
    """Target-free finite recurrence, implemented directly from its equation."""
    n = len(rows)
    initial = tuple(row["c"] for row in rows)
    scale = max(initial) or Q(1)
    theta = tuple(value / scale for value in initial)
    lower, upper = min(theta), max(theta)
    gaps = tuple(right["radius"] - left["radius"] for left, right in zip(rows, rows[1:]))
    intensities = tuple(min(gaps)/gap if case == "spacing" else Q(1) for gap in gaps)
    pressures = tuple(Q(18) if index in (0, n-1) else Q(27) for index in range(n))
    steps = ((n-1+2)//3)*3
    initial_stock = sum((p*t for p, t in zip(pressures, theta)), Q(0))
    for _step in range(steps):
        incoming_minus_outgoing = [Q(0)] * n
        if case != "disabled":
            for edge, intensity in enumerate(intensities):
                transfer = 9*intensity*(theta[edge]-theta[edge+1])
                incoming_minus_outgoing[edge] -= transfer
                incoming_minus_outgoing[edge+1] += transfer
        following = tuple(t + net/p for t, net, p in zip(theta, incoming_minus_outgoing, pressures))
        if not all(lower <= value <= upper for value in following):
            raise ArithmeticError("oracle escaped initial convex interval")
        if sum((p*t for p, t in zip(pressures, following)), Q(0)) != initial_stock:
            raise ArithmeticError("oracle lost pressure-weighted stock")
        theta = following
    final = tuple(scale*t for t in theta)
    return {"initial_dbf": initial, "final_dbf": final,
            "predictions": tuple(row["b"] + 8*d for row, d in zip(rows, final)),
            "response_steps": steps, "pressure_stock": initial_stock,
            "stock_residual": Q(0), "intensities": intensities,
            "initial_theta_range": (lower, upper), "final_theta_range": (min(theta), max(theta))}


def score(projected, targets):
    scored = []
    for (galaxy, radius), prediction in sorted(projected.items()):
        observed = targets[(galaxy, radius)]["u_observed"]
        residual = prediction-observed
        scored.append({"galaxy": galaxy, "radius_kpc": radius,
                       "u_observed": observed, "u_projected": prediction,
                       "signed_residual": residual, "signed_relative_residual": residual/observed,
                       "absolute_percentage_error": 100*abs(residual)/observed})
    groups = {}
    for row in scored:
        groups.setdefault(row["galaxy"], []).append(row["absolute_percentage_error"])
    per_galaxy = {name: {"bins": len(errors), "mape_percent": sum(errors, Q(0))/len(errors)}
                  for name, errors in sorted(groups.items())}
    return {"bins": len(scored), "galaxies": len(groups), "rows": scored,
            "mape_percent": sum((row["absolute_percentage_error"] for row in scored), Q(0))/len(scored),
            "equal_galaxy_mape_percent": sum((row["mape_percent"] for row in per_galaxy.values()), Q(0))/len(groups),
            "per_galaxy": per_galaxy}


def encode(value):
    if isinstance(value, Q):
        return str(value)
    if isinstance(value, dict):
        return {key: encode(item) for key, item in value.items()}
    if isinstance(value, (tuple, list)):
        return [encode(item) for item in value]
    return value


def display(value):
    with localcontext() as context:
        context.prec = 35
        return str(Decimal(value.numerator)/Decimal(value.denominator))


def verify_csv(expected_cases, comparison):
    if not comparison.exists():
        return {"status": "not_yet_present", "path": str(comparison.resolve())}
    expected = {(case, row["galaxy"], row["radius_kpc"]): row
                for case, result in expected_cases.items() for row in result["rows"]}
    seen, mismatches = set(), []
    fields = ("u_observed", "u_projected", "signed_residual", "signed_relative_residual", "absolute_percentage_error")
    with comparison.open(newline="") as stream:
        for row in csv.DictReader(stream):
            key = (row["case"], row["galaxy"], Q(row["radius_kpc"]))
            if key in seen or key not in expected:
                raise ValueError(f"duplicate or unexpected implementation comparison row: {key}")
            seen.add(key)
            for field in fields:
                if Q(row[field]) != expected[key][field]:
                    mismatches.append({"case": key[0], "galaxy": key[1], "radius_kpc": str(key[2]),
                                       "field": field, "oracle": str(expected[key][field]), "implementation": row[field]})
    if seen != expected.keys():
        raise ValueError("implementation comparison cohort does not match oracle")
    return {"status": "PASS" if not mismatches else "FAIL", "path": str(comparison.resolve()),
            "sha256": digest(comparison), "rows_checked": len(seen),
            "exact_numeric_field_checks": len(seen)*len(fields), "mismatches": mismatches}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--comparison", type=Path)
    args = parser.parse_args()
    preparation, targets, sources = read_tables(args.input)
    # Finish all target-free state calculations before joining comparison data.
    numerical = {case: {name: evolve(rows, case) for name, rows in preparation.items()} for case in CASES}
    baseline = score({(name, row["radius"]): row["b"]+8*row["c"]
                      for name, rows in preparation.items() for row in rows}, targets)
    comparisons = {}
    for case in CASES:
        projected = {(name, row["radius"]): prediction
                     for name, rows in preparation.items()
                     for row, prediction in zip(rows, numerical[case][name]["predictions"])}
        comparisons[case] = score(projected, targets)
        comparisons[case]["change_from_g0_percentage_points"] = comparisons[case]["mape_percent"] - baseline["mape_percent"]
    result = {"format": "independent-reduced-rational-oracle-v1",
              "scope": "numerical recurrence check; no native engine imports or independent admission replay",
              "model": {"pressure": "P_i=9*(degree_i+1)",
                        "edge": "phi=9*i*(theta_left-theta_right)",
                        "spacing_intensity": "min_neighbor_gap/gap", "cadence": 3,
                        "horizon": "ceil((N-1)/3)*3", "initialization": "theta=Ccap/max(Ccap)",
                        "projection": "Ubar + 8*max(Ccap)*theta",
                        "all_zero_convention": "normalization=1; theta and projected contribution remain zero"},
              "numerical": numerical, "baseline": baseline, "comparisons": comparisons}
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output/"results.json").write_text(json.dumps(encode(result), indent=2, sort_keys=True)+"\n")
    binding = {"command": [sys.executable, *sys.argv], "input_sources": sources,
               "oracle_path": str(Path(__file__).resolve()), "oracle_sha256": digest(__file__),
               "results_sha256": digest(args.output/"results.json")}
    (args.output/"source_bindings.json").write_text(json.dumps(binding, indent=2)+"\n")
    with (args.output/"comparison.csv").open("w", newline="") as stream:
        fields = ("case", "galaxy", "radius_kpc", "u_observed", "u_projected", "signed_residual", "signed_relative_residual", "absolute_percentage_error")
        writer = csv.DictWriter(stream, fieldnames=fields, lineterminator="\n")
        writer.writeheader()
        for case, comparison in comparisons.items():
            for row in comparison["rows"]:
                writer.writerow({"case": case, **encode(row)})
    verification = verify_csv(comparisons, args.comparison) if args.comparison else {"status": "not_requested"}
    (args.output/"comparison_verification.json").write_text(json.dumps(verification, indent=2)+"\n")
    report = {"bins": baseline["bins"], "baseline_mape_percent": display(baseline["mape_percent"]),
              "cases": {case: {"mape_percent": display(values["mape_percent"]),
                                "equal_galaxy_mape_percent": display(values["equal_galaxy_mape_percent"]),
                                "change_percentage_points": display(values["change_from_g0_percentage_points"]),
                                "per_galaxy_mape_percent": {name: display(row["mape_percent"]) for name, row in values["per_galaxy"].items()}}
                        for case, values in comparisons.items()}, "comparison_verification": verification}
    (args.output/"display.json").write_text(json.dumps(report, indent=2)+"\n")
    print(json.dumps(report, indent=2))
    if verification["status"] == "FAIL":
        raise SystemExit(1)


if __name__ == "__main__":
    main()
