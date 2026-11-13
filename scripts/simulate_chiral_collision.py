#!/usr/bin/env python3
"""Exact core-biased chiral encounters, retained sampling and native Dec replay."""
from __future__ import annotations

import argparse
from dataclasses import astuple
from fractions import Fraction as F
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from simulations.chiral_collision import (CollisionConfig, background_record,
    common_ticket_denominator, exact_data, exact_distribution, simulate_path)
from simulations.chiral_dec import dec_source, validate_native_output
from simulations.chiral_reference import STRATA, exact_reference, fixed_probability_checks


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def save(path, value, *, compact=False):
    path.parent.mkdir(parents=True, exist_ok=True)
    temp = path.with_suffix(path.suffix + ".tmp")
    temp.write_text(json.dumps(exact_data(value), indent=None if compact else 2,
                               ensure_ascii=False) + "\n")
    temp.replace(path)


def source_hashes():
    paths = [Path(__file__).resolve(), *ROOT.glob("simulations/chiral_*.py"),
             ROOT / "simulations/tesseract_cells.py", ROOT / "simulations/manual_dec_fixture.py",
             ROOT / "simulations/vendor/aod_manual_native.py", ROOT / "simulations/vendor/LICENSE",
             ROOT / "aodc.py", ROOT / "runtime/dec_core.py", ROOT / "runtime/dec_language.py",
             ROOT / "native/compiler.py", ROOT / "native/runtime.cpp", ROOT / "native/runtime.hpp",
             ROOT / "native/runner.cpp", ROOT / "native/toolchain-lock.json",
             ROOT / "examples/chiral_core_collision.dec"]
    return {str(p.relative_to(ROOT)): digest(p) for p in sorted(paths)}


def configs():
    return {
        "core_only": CollisionConfig(contact_gain=F(0)),
        "uniform": CollisionConfig(q_core=F(1, 16), contact_gain=F(0)),
        "contact_feedback": CollisionConfig(contact_gain=F(1, 4)),
        "no_flips": CollisionConfig(q_outer=F(0), q_core=F(0), contact_gain=F(0)),
        "forced_flips": CollisionConfig(q_outer=F(1), q_core=F(1), contact_gain=F(0)),
        "fixed_q": CollisionConfig(q_outer=F(1, 4), q_core=F(1, 4), contact_gain=F(0)),
    }


def exact_study(cases):
    reports = {}
    for name, config in cases.items():
        reference = exact_reference(steps=config.steps, shell_probability=config.q_outer,
                                    core_probability=config.q_core, contact_gain=config.contact_gain)
        actual = {astuple(s): p for s, p in exact_distribution(config).items()}
        if actual != reference["distribution"]:
            raise ArithmeticError("independent terminal law differs: " + name)
        reference["model_reference_comparison"] = {
            "status": "PASS", "terminal_states": len(actual),
            "coordinate": "probability on the same 256-state space",
            "max_absolute_residual": F(0),
            "scope": "exact finite-law identity; no physical accuracy inference"}
        if config.q_core == config.q_outer and not config.contact_gain:
            analytic = fixed_probability_checks(config.q_outer, config.steps)
            if reference["expected_flip_count"] != 2 * analytic["expected_flip_count"]:
                raise ArithmeticError("fixed-q expectation failed")
            for carrier in ("a", "b"):
                for key in ("probability_at_least_one_flip", "probability_terminal_sign_change"):
                    if reference[key][carrier] != analytic[key]:
                        raise ArithmeticError("fixed-q event/parity check failed")
            reference["fixed_q_analytic_per_carrier"] = analytic
        for field in ("distribution", "position_distribution"):
            reference[field] = [{"state": s, "probability": p}
                                for s, p in sorted(reference[field].items())]
        reports[name] = reference
    primary = reports["core_only"]
    if primary["expected_flip_count"] != F(11, 8):
        raise ArithmeticError("independent primary expectation differs from 11/8")
    for zone, opportunities, flips in (("core", F(2), F(1, 2)), ("outer", F(14), F(7, 8))):
        if sum(v for k, v in primary["expected_opportunities_by_stratum"].items()
               if k.startswith(zone)) != opportunities:
            raise ArithmeticError("primary exposure comparator failed")
        if sum(v for k, v in primary["expected_flips_by_stratum"].items()
               if k.startswith(zone)) != flips:
            raise ArithmeticError("primary flip comparator failed")
    if reports["core_only"]["position_distribution"] != reports["contact_feedback"]["position_distribution"]:
        raise ArithmeticError("flip-law intervention changed exact movement law")
    return reports


def sampled_study(cases, references, count):
    names = ("core_only", "uniform", "contact_feedback")
    denominator = common_ticket_denominator([cases[n] for n in names])
    summaries = {name: {"strata": {s: {"opportunities": 0, "flips": 0} for s in STRATA},
                        "flips": 0, "terminal_changed": 0, "any_flip": 0}
                 for name in names}
    cohort, exemplars = [], {}
    for seed in range(count):
        paired = {name: simulate_path(cases[name], seed=seed, retain_witnesses=False,
                                      ticket_denominator=denominator) for name in names}
        baseline = paired["core_only"]
        for name, path in paired.items():
            summary = summaries[name]
            summary["flips"] += sum(path["flip_counts"])
            summary["terminal_changed"] += sum(path["terminal_changed"])
            summary["any_flip"] += sum(path["any_flip"])
            for primary, step in zip(baseline["steps"], path["steps"]):
                for key in ("movement_tickets", "flip_tickets", "crossed_faces"):
                    if primary[key] != step[key]:
                        raise ArithmeticError("paired intervention changed shared tickets/support")
                after = step["after"]
                if (primary["after"].facet_a, primary["after"].facet_b) != (after.facet_a, after.facet_b):
                    raise ArithmeticError("paired intervention changed destination")
                for facet, flipped in zip((after.facet_a, after.facet_b), step["flipped"]):
                    stratum = ("core" if facet == "*0**" else "outer") + (
                        "_contact" if step["support_contact"] else "_separate")
                    summary["strata"][stratum]["opportunities"] += 1
                    summary["strata"][stratum]["flips"] += int(flipped)
            # Source metadata is common; keep one binding in results, every event here.
            cohort.append({"case": name, **{k: v for k, v in path.items() if k != "source"}})
    for name, summary in summaries.items():
        summary.update(realizations=count, carrier_histories=2 * count,
                       completed_return_opportunities=16 * count,
                       mean_flip_count_per_two_carrier_path=F(summary["flips"], count),
                       terminal_changed_frequency=F(summary["terminal_changed"], 2 * count),
                       any_flip_frequency=F(summary["any_flip"], 2 * count))
        for stratum, row in summary["strata"].items():
            q = references[name]["q_by_stratum"][stratum]
            row.update(declared_probability=q,
                       observed_frequency=F(row["flips"], row["opportunities"]) if row["opportunities"] else None,
                       conditional_expected_flips=q * row["opportunities"],
                       residual=row["flips"] - q * row["opportunities"],
                       residual_type="exact signed sample discrepancy; not a bound or standard uncertainty")
    for name in ("core_only", "contact_feedback"):
        for seed in (7, 19):
            exemplars[f"{name}-{seed}"] = simulate_path(cases[name], seed=seed,
                                                        ticket_denominator=denominator)
    return {"seed_range": [0, count - 1], "flip_ticket_denominator": denominator,
            "paired_tickets_and_movement": "PASS", "summaries": summaries,
            "sampling_scope": "Deterministic seeded pseudorandom realizations; no empirical confidence claim."}, cohort, exemplars


def execute(argv, commands, expected=None):
    argv = list(map(str, argv))
    row = {"command": argv, "status": "RUNNING"}
    commands.append(row)
    proc = subprocess.run(argv, cwd=ROOT, capture_output=True, text=True, timeout=300,
                          env=dict(os.environ, PYTHONDONTWRITEBYTECODE="1"))
    row.update(returncode=proc.returncode, stderr=proc.stderr,
               stdout_sha256=hashlib.sha256(proc.stdout.encode()).hexdigest())
    if proc.returncode or (expected is not None and proc.stdout != expected):
        row.update(status="FAIL", stdout=proc.stdout)
        raise RuntimeError("command or exact stdout comparison failed: " + repr(argv))
    row["status"] = "PASS"
    return proc.stdout


def native_replay(output, exemplars, enabled, commands):
    reports, artifacts = [], {}

    def retain(path):
        relative = str(path.relative_to(output))
        artifacts[relative] = digest(path)
        return relative

    for name, path in exemplars.items():
        folder = output / "dec"
        folder.mkdir(parents=True, exist_ok=True)
        source = folder / (name + ".dec")
        source.write_text(dec_source(path["config"], path))
        retain(source)
        if name == "core_only-7" and (ROOT / "examples/chiral_core_collision.dec").read_text() != source.read_text():
            raise ArithmeticError("published canonical Dec example differs from generated replay")
        execute([sys.executable, "aodc.py", "check", source], commands, "OK: Dec core v1\n")
        expected = execute([sys.executable, "aodc.py", "run", source], commands)
        validation = validate_native_output(expected, path)
        stdout = folder / (name + ".stdout")
        stdout.write_text(expected)
        retain(stdout)
        ir = folder / (name + ".json")
        execute([sys.executable, "aodc.py", "compile", source, "-o", ir], commands)
        retain(ir)
        execute([sys.executable, "aodc.py", "exec", ir], commands, expected)
        builds = []
        if enabled:
            for level in (0, 2):
                binary = folder / (name + f"-O{level}")
                execute([sys.executable, "aodc.py", "build", source, "-O", level, "-o", binary], commands)
                execute([binary], commands, expected)
                execute([sys.executable, "aodc.py", "jit", source, "-O", level], commands, expected)
                manifest = binary.with_suffix(binary.suffix + ".build.json")
                metadata = json.loads(manifest.read_text())
                binary_path, manifest_path = retain(binary), retain(manifest)
                builds.append({"optimization": level,
                               "binary": binary_path, "binary_sha256": artifacts[binary_path],
                               "manifest": manifest_path, "manifest_sha256": artifacts[manifest_path],
                               "binding": metadata["binding"],
                               "scope": "AOT compiler/runtime and LLVM binding from this build; JIT stdout is compared separately."})
        reports.append({"case": name, "program_sha256": digest(source), "witness_validation": validation,
                        "execution_modes": 6 if enabled else 2, "aot_builds": builds, "status": "PASS"})
    return {"status": "PASS", "native_requested": enabled, "programs": reports,
            "artifact_sha256": artifacts,
            "scope": "Dec replays supplied motion and tickets, computes q/sign decisions and executes whole returns. Python enumerates and samples the joint law."}


def plot(output, report, exemplars):
    config_directory = output / ".matplotlib"
    config_directory.mkdir(parents=True, exist_ok=True)
    os.environ["MPLCONFIGDIR"] = str(config_directory)
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    import numpy as np
    plt.rcParams.update({"font.size": 10, "axes.spines.top": False, "axes.spines.right": False})
    fig, axes = plt.subplots(2, 2, figsize=(10, 7), layout="constrained")
    ax = axes[0, 0]
    labels = ["Outer", "Core"]
    ax.bar(labels, [1/16, 1/4], color=["#507c96", "#b85239"])
    ax.set(title="Primary rule: flip probability per opportunity", ylabel="Probability", ylim=(0, .3))
    for i, value in enumerate((F(1,16), F(1,4))):
        ax.text(i, float(value)+.01, str(value), ha="center")
    ax = axes[0, 1]
    ax.bar(labels, [7/8, 1/2], color=["#507c96", "#b85239"])
    ax.set(title="Eight returns: expected flip counts", ylabel="Expected count", ylim=(0, 1.1))
    for i, value, exposure in ((0,F(7,8),14),(1,F(1,2),2)):
        ax.text(i, float(value)+.04, f"{value}\n{exposure} expected opportunities", ha="center", fontsize=9)
    ax = axes[1, 0]
    n = np.arange(9)
    ax.plot(n, [float(fixed_probability_checks(F(1,4), int(k))["probability_at_least_one_flip"]) for k in n],
            "o-", label="At least one flip")
    ax.plot(n, [float(fixed_probability_checks(F(1,4), int(k))["probability_terminal_sign_change"]) for k in n],
            "s-", label="Final sign changed")
    ax.set(title="Fixed q = 1/4 control, one carrier", xlabel="Completed returns", ylabel="Probability", ylim=(0,1.03))
    ax.legend(fontsize=9)
    ax = axes[1, 1]
    path = exemplars["core_only-7"]
    for index, carrier in enumerate(("a", "b")):
        states = [path["initial"], *(s["after"] for s in path["steps"])]
        signs = [getattr(s, "chi_" + carrier) for s in states]
        yy = [index * 3 + sign / 2 for sign in signs]
        ax.step(range(9), yy, where="post", label="Carrier " + carrier.upper())
        for k, state in enumerate(states):
            core = getattr(state, "facet_" + carrier) == "*0**"
            ax.scatter(k, yy[k], color="#b85239" if core else "#507c96", marker="D" if core else "o", s=32, zorder=3)
    ax.set(title="Retained primary path, seed 7", xlabel="Completed returns",
           yticks=[-.5,.5,2.5,3.5], yticklabels=["A −1","A +1","B −1","B +1"], ylim=(-1,4))
    ax.text(.02, .46, "Diamond = core; circle = outer", transform=ax.transAxes, fontsize=9)
    fig.savefig(output / "chiral-study.png", dpi=180)
    plt.close(fig)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "build/chiral-collision")
    parser.add_argument("--samples", type=int, default=256)
    parser.add_argument("--native", action="store_true")
    parser.add_argument("--plots", action="store_true")
    args = parser.parse_args()
    if args.samples < 1:
        parser.error("--samples must be positive")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    report = {"format": "aod-dec.chiral-collision-study", "version": 1, "status": "RUNNING",
              "source_sha256": source_hashes(), "commands": [],
              "model_execution_boundary": {
                  "probabilistic_model": "Application-declared profile, movement kernel, flip law and conditional factorization.",
                  "host_evaluation": "Exact finite-law enumeration and seeded sampling of that supplied model.",
                  "DEC_engine": "Unchanged compiler/runtime executes supplied expressions and checks trace operations/witnesses; no intrinsic core-bias or collision law."},
              "scope": "Declared stochastic chiral encounter; physical proton-proton correspondence and SI calibration absent."}
    try:
        cases = configs()
        report["cases"] = exact_study(cases)
        report["sampling"], cohort, exemplars = sampled_study(cases, report["cases"], args.samples)
        save(output / "cohort.json", cohort, compact=True)
        save(output / "exemplars.json", exemplars)
        report["background"] = background_record(cases["core_only"])
        report["native_replay"] = native_replay(output, exemplars, args.native, report["commands"])
        report["error_ledger"] = {
            "support": "D0 typed finite fixture; no physical G2/G3 dataset",
            "arithmetic": {"status": "exact", "roundoff": 0},
            "terminal_law_comparison": {"status": "exact", "max_absolute_residual": F(0),
                "comparator": "independent integer recursion for the same law"},
            "sampling": {"status": "reported", "coordinate": "flips minus q times opportunities per stratum",
                "bound": None, "standard_uncertainty": None},
            "projection": {"status": "declared", "assumption": "frozen profile and q depend only on current destination pair; archived histories do not feed back"},
            "discretization": {"status": "not_applicable", "reason": "declared discrete law; no continuous generator limit claimed"},
            "physical_model": {"status": "unknown", "bound": None},
            "SI_map": {"status": "not_instantiated", "seconds_per_return": None,
                "energy": None, "momentum": None, "cross_section": None}}
        if args.plots:
            plot(output, report, exemplars)
        if source_hashes() != report["source_sha256"]:
            raise RuntimeError("study sources changed during execution")
        generated = [output / "cohort.json", output / "exemplars.json"]
        if args.plots:
            generated.append(output / "chiral-study.png")
        report["artifacts"] = {str(p.relative_to(output)): digest(p) for p in generated}
        report["artifacts"].update(report["native_replay"]["artifact_sha256"])
        report["status"] = "PASS"
    except Exception as exc:
        report.update(status="FAIL", error=f"{type(exc).__name__}: {exc}")
        raise
    finally:
        save(output / "results.json", report)
    print(json.dumps({"status": report["status"], "primary_expected_flips": "11/8",
                      "samples_per_case": args.samples, "output": str(output)}))


if __name__ == "__main__":
    main()
