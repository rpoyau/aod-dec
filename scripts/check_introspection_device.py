#!/usr/bin/env python3
"""Retain actual-device tests and cost-inclusive introspection comparisons."""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repeats", type=int, default=3)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    report = {"status": "RUNNING", "steps": []}
    for name, flags in (("normal", ["-B"]), ("optimized", ["-B", "-O"])):
        jobs = (
            ("tests", ["-m", "unittest", "tests.test_gpu_compiler", "-v"]),
            ("awareness", ["scripts/check_awareness_jit.py", "--gpu", "required", "--workers", "2",
                           "--output", str(output/(name+"-awareness"))]),
            ("evolution", ["scripts/check_introspection_evolution.py", "--gpu", "required", "--workers", "2",
                           "--repeats", str(args.repeats), "--output", str(output/(name+"-evolution"))]),
        )
        for label, command in jobs:
            argv = [sys.executable, *flags, *command]
            started = time.perf_counter_ns()
            with (output/(name+"-"+label+".log")).open("w") as stream:
                result = subprocess.run(argv, cwd=ROOT, stdout=stream, stderr=subprocess.STDOUT)
            report["steps"].append({"mode": name, "check": label, "command": argv,
                                    "exit_code": result.returncode, "elapsed_ns": time.perf_counter_ns()-started})
            (output/"steps.json").write_text(json.dumps(report, indent=2)+"\n")
            print(name, label, result.returncode, flush=True)
            if result.returncode:
                report["status"] = "FAIL"
                (output/"steps.json").write_text(json.dumps(report, indent=2)+"\n")
                return result.returncode
    report["status"] = "PASS"
    (output/"steps.json").write_text(json.dumps(report, indent=2)+"\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
