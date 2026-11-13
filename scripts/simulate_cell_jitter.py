#!/usr/bin/env python3
"""Run the declared tesseract-cell experiment and retain reproducible results.

The exact Euler engine, independent continuous reference, and generated Dec
programs are separate checks. None is a calibrated quantum maser simulation.
"""
from __future__ import annotations

import argparse
from dataclasses import asdict, is_dataclass
from decimal import Decimal, localcontext
from fractions import Fraction
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from simulations.cell_transport import (ModelConfig, STATE_NAMES, flatten_state,
                                       make_schedule, region_audit, simulate)
from simulations.tesseract_cells import (FACETS, TESSERACT, boundary, boundary_chain,
                                        cells, facet_interfaces)
from simulations.transport_reference import (compare_precisions, flatten_reference,
                                             simulate_reference)
from simulations.report_contract import terminal_report
from simulations.manual_dec_fixture import build_fixture as manual_dec_fixture
from simulations.manual_si_fixture import build_fixture as manual_si_fixture


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def serial(value):
    if isinstance(value, (Fraction, Decimal)):
        return str(value)
    if is_dataclass(value):
        return serial(asdict(value))
    if isinstance(value, dict):
        return {str(k): serial(v) for k, v in value.items()}
    if isinstance(value, (tuple, list)):
        return [serial(v) for v in value]
    return value


def save(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    temp = path.with_suffix(path.suffix + ".tmp")
    temp.write_text(json.dumps(serial(data), indent=2, ensure_ascii=False) + "\n")
    temp.replace(path)


def decimal(value):
    return Decimal(value.numerator) / Decimal(value.denominator)


def expected_stdout(result):
    return "".join(f"{v.numerator}/{v.denominator}\n" for v in flatten_state(result)) + "0\n"


def execute(argv, commands, *, expected=None):
    command = list(map(str, argv))
    row = {"command": command, "status": "RUNNING"}
    commands.append(row)
    try:
        proc = subprocess.run(command, cwd=ROOT, capture_output=True, timeout=300,
                              env=dict(os.environ, PYTHONDONTWRITEBYTECODE="1"))
        row.update(returncode=proc.returncode, stdout=proc.stdout.decode(),
                   stderr=proc.stderr.decode())
        if proc.returncode:
            raise RuntimeError(f"command failed: {command}")
        if expected is not None and row["stdout"] != expected:
            row["expected_stdout"] = expected
            raise ArithmeticError(f"observable output differs: {command}")
        row["status"] = "PASS"
        return row["stdout"]
    except Exception as exc:
        row.update(status="FAIL", error=str(exc))
        raise


def topology_checks():
    inventory = [len(cells(d)) for d in range(5)]
    if inventory != [16, 32, 24, 8, 1]:
        raise ArithmeticError("incorrect cubical inventory")
    for d in range(5):
        for cell in cells(d):
            if boundary_chain(boundary(cell)):
                raise ArithmeticError("boundary of boundary is not zero")
    interfaces = facet_interfaces()
    if len(interfaces) != 24 or any(f.left_sign + f.right_sign for f in interfaces):
        raise ArithmeticError("shared-square incidence failed")
    return {"status": "PASS", "cell_inventory_by_dimension": inventory,
            "cell_ids_by_dimension": {d: cells(d) for d in range(5)},
            "boundary_of_boundary_checked_cells": sum(inventory),
            "shared_squares": interfaces, "whole_shell_boundary": boundary_chain(boundary(TESSERACT))}


def studies(config, seed, ensemble_size):
    convergence, references = [], []
    for amplitude in (Fraction(0), Fraction(1, 4), Fraction(1, 2)):
        schedule = make_schedule(config.intervals, amplitude, seed)
        precision = compare_precisions(config, schedule)
        if precision["max_absolute_difference"] > Decimal("1e-45"):
            raise ArithmeticError("reference precision crosscheck exceeded tolerance")
        reference = precision["higher"]
        references.append({"amplitude": amplitude, "schedule": schedule,
                           "precision_check": precision})
        previous = None
        for substeps in (1, 2, 4, 8, 16, 32):
            result = simulate(config, substeps, schedule, retain_steps=False)
            with localcontext() as context:
                context.prec = 80
                differences = [abs(decimal(a) - b) for a, b in
                               zip(flatten_state(result), flatten_reference(reference))]
                error = sum(differences, Decimal(0))
                total = decimal(result["initial_total"] + result["final"]["injected"])
                relative = error / total
            if previous is not None and error >= previous:
                raise ArithmeticError("refinement did not reduce error in the declared default experiment")
            convergence.append({"amplitude": amplitude, "seed": seed, "substeps": substeps,
                                "h": Fraction(1, substeps), "schedule_sha256": schedule["sha256"],
                                "terminal": result["final"], "l1_error": error,
                                "relative_l1_error": relative,
                                "observed_order": None if previous is None else math.log2(float(previous / error)),
                                "checks": result["checks"],
                                "output_report": terminal_report(result, reference,
                                    reference_id="independent-generator:" + schedule["sha256"])})
            previous = error
    ensemble = []
    # Use the continuous reference here to avoid mixing coarse Euler error into
    # the reported response to different jitter paths.
    for amplitude in (Fraction(0), Fraction(1, 4), Fraction(1, 2)):
        rows = []
        for realization in range(ensemble_size if amplitude else 1):
            schedule = make_schedule(config.intervals, amplitude, realization)
            result = simulate_reference(config, schedule, precision=60)
            final = result["final"]
            with localcontext() as context:
                context.prec = 60
                residual = (final["emitter"] + sum(final["cells"]) + final["output"]
                            + final["loss"] - decimal(config.initial_emitter + sum(config.initial_cells))
                            - final["injected"])
                stored_cells = sum(final["cells"])
            if abs(residual) > Decimal("1e-45"):
                raise ArithmeticError("continuous reference energy residual exceeded tolerance")
            rows.append({"seed": realization, "schedule": schedule,
                         "output": final["output"], "loss": final["loss"],
                         "stored_cells": stored_cells, "balance_residual": residual,
                         "reference_intervals": result["intervals"]})
        values = [row["output"] for row in rows]
        with localcontext() as context:
            context.prec = 60
            mean_output = sum(values) / len(values)
        ensemble.append({"amplitude": amplitude, "realizations": rows,
                         "aggregate_precision": 60, "output_mean": mean_output,
                         "output_min": min(values), "output_max": max(values)})
    return convergence, references, ensemble


def verify_dec(config, output, seed, native, commands):
    from simulations.transport_dec import dec_source
    records = []
    for amplitude in (Fraction(0), Fraction(1, 4)):
        schedule = make_schedule(config.intervals, amplitude, seed)
        for substeps in (1, 2):
            label = f"jitter-{amplitude.numerator}-{amplitude.denominator}-steps-{substeps}"
            path = output / "dec" / (label + ".dec")
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(dec_source(config, substeps, schedule))
            result = simulate(config, substeps, schedule, retain_steps=False)
            expected = expected_stdout(result)
            cli = [sys.executable, ROOT / "aodc.py"]
            execute([*cli, "check", path], commands, expected="OK: Dec core v1\n")
            execute([*cli, "run", path], commands, expected=expected)
            ir = path.with_suffix(".json")
            execute([*cli, "compile", path, "-o", ir], commands)
            execute([*cli, "exec", ir], commands, expected=expected)
            record = {"case": label, "source_sha256": digest(path), "ir_sha256": digest(ir),
                      "expected_stdout": expected, "modes": ["reference", "typed-ir"],
                      "schedule_sha256": schedule["sha256"], "native_artifacts": []}
            if native:
                for opt in (0, 2):
                    binary = path.with_name(label + f"-O{opt}")
                    execute([*cli, "build", path, "-O", opt, "-o", binary], commands)
                    execute([binary], commands, expected=expected)
                    execute([*cli, "jit", path, "-O", opt], commands, expected=expected)
                    record["modes"].extend([f"aot-O{opt}", f"jit-O{opt}"])
                    record["native_artifacts"].append({"sha256": digest(binary),
                        "build": json.loads(binary.with_name(binary.name + ".build.json").read_text())})
            record["status"] = "PASS"
            records.append(record)
    # The checked-in example must be reproducible from its stated inputs.
    checked_in = ROOT / "examples/tesseract_cell_jitter.dec"
    wanted = dec_source(config, 1, make_schedule(config.intervals, Fraction(1, 4), seed))
    if checked_in.read_text() != wanted:
        raise ArithmeticError("checked-in cell example differs from its generator and stated inputs")
    return records


def plots(report, output):
    os.environ.setdefault("MPLCONFIGDIR", str(output / "matplotlib-cache"))
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    plt.rcParams.update({"font.size": 10, "axes.spines.top": False,
                         "axes.spines.right": False, "figure.dpi": 160})
    fig, axes = plt.subplots(1, 2, figsize=(11, 4.2), layout="constrained")
    colors = ("#17607e", "#aa6b12", "#835aa1")
    for amp, color in zip((Fraction(0), Fraction(1, 4), Fraction(1, 2)), colors):
        rows = [r for r in report["convergence"] if r["amplitude"] == amp]
        axes[0].loglog([r["substeps"] for r in rows],
                       [float(abs(r["output_report"]["outputs"]["output"]["fractional_discrepancy"]))
                        for r in rows],
                       "o-", label=f"jitter amplitude {amp}", color=color)
    axes[0].set(xlabel="Substeps per fixed interval",
                ylabel="Absolute fractional boundary-output discrepancy",
                title="Solver comparison with declared numerical reference")
    axes[0].legend(fontsize=8)
    axes[0].grid(True, alpha=.2, which="both")
    for index, entry in enumerate(report["jitter_ensemble"]):
        values = [float(r["output"]) for r in entry["realizations"]]
        offsets = [(i - (len(values) - 1) / 2) * .012 for i in range(len(values))]
        axes[1].scatter([index + x for x in offsets], values, color=colors[index], s=20)
        axes[1].plot([index - .18, index + .18], [float(entry["output_mean"])] * 2,
                     color="black", lw=1.5)
    axes[1].set_xticks(range(3), ["0", "1/4", "1/2"])
    axes[1].set(xlabel="Declared jitter amplitude", ylabel="Accumulated output (model units)",
                title="Jitter sensitivity: continuous reference")
    axes[1].grid(True, axis="y", alpha=.2)
    fig.savefig(output / "accuracy-and-jitter.png")
    fig.savefig(output / "accuracy-and-jitter.pdf")
    plt.close(fig)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "build/cell-jitter")
    parser.add_argument("--native", action="store_true")
    parser.add_argument("--plots", action="store_true", help="Require matplotlib and export PNG/PDF figures")
    args = parser.parse_args(argv)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    report = {"format": "aod.dec.cell-jitter-study", "version": 2, "status": "RUNNING",
              "scope": "Auxiliary cubical transport solver control plus separate source-bound D.E.C. history/RCD and conditional observed-cycle SI fixtures.",
              "classification": {"transport": "declared numerical control; not an admitted AOD field or SI maser model",
                  "manual_dec": "D0 exact internal fixture; recurrent identities and contacts retained",
                  "manual_si": "conditional observed-cycle map; measured hydrogen target separately bound"},
              "historical_metric": "relative_l1_error is a supplied-mass-normalized aggregate solver diagnostic, not a kernel probability, output-relative error, or physical accuracy percentage.",
              "jitter_interpretation": "Bounded transition-weight proxy for unresolved successor possibilities; incidence remains fixed.",
              "physical_accuracy": "NOT_ESTABLISHED; no stimulated gain, coherence, Hamiltonian, SI clock, or empirical data.",
              "native_requested": args.native, "commands": [],
              "source_sha256": {str(p.relative_to(ROOT)): digest(p) for p in
                  sorted([*ROOT.glob('simulations/**/*.py'), Path(__file__).resolve(),
                          ROOT / 'simulations/vendor/LICENSE',
                          ROOT / 'examples/tesseract_cell_jitter.dec'])}}
    path = output / "results.json"
    try:
        config, seed = ModelConfig(), 7
        report.update(config=asdict(config), state_names=STATE_NAMES, fixed_seed=seed,
                      topology=topology_checks())
        report["manual_dec"] = manual_dec_fixture()
        report["manual_si"] = manual_si_fixture()
        schedule = make_schedule(config.intervals, Fraction(1, 4), seed)
        demo = simulate(config, 1, schedule)
        for step in demo["steps"]:
            for mask in range(256):
                region_audit(step, [cell for i, cell in enumerate(FACETS) if mask & (1 << i)])
        report["retained_example"] = demo
        report["exhaustive_region_checks"] = len(demo["steps"]) * 256
        report["convergence"], report["references"], report["jitter_ensemble"] = studies(config, seed, 16)
        save(path, report)
        report["dec_checks"] = verify_dec(config, output, seed, args.native, report["commands"])
        if args.plots:
            plots(report, output)
            report["figures"] = {p.name: digest(p) for p in
                                 (output / "accuracy-and-jitter.png", output / "accuracy-and-jitter.pdf")}
        if any(digest(ROOT / p) != expected for p, expected in report["source_sha256"].items()):
            raise RuntimeError("simulation source changed during execution; rerun against stable files")
        report["status"] = "PASS"
    except Exception as exc:
        report.update(status="FAIL", error=f"{type(exc).__name__}: {exc}")
    save(path, report)
    print(f"{report['status']}: {path}")
    if report["status"] == "FAIL":
        print(report["error"], file=sys.stderr)
        return 1
    for row in report["jitter_ensemble"]:
        print(f"jitter={row['amplitude']}: mean output={float(row['output_mean']):.9g}; "
              f"range=[{float(row['output_min']):.9g}, {float(row['output_max']):.9g}]")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
