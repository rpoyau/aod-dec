#!/usr/bin/env python3
"""Build the pinned Typst manual and replay its literal Dec/expected-output pairs."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parents[1]
MANUAL = ROOT / "docs/language-and-compiler.typ"
TYPST_VERSION = "0.13.1"
CREATION_TIMESTAMP = "1794528000"  # 2026-11-13 00:00:00 UTC


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def example_pairs(source):
    """Read literal fenced blocks, allowing other tagged blocks between a pair."""
    pairs = []
    pending = None
    tag = None
    opened = None
    contents = []
    for number, line in enumerate(source.splitlines(keepends=True), 1):
        marker = re.fullmatch(r"```([^`\r\n]*)\r?\n?", line)
        if tag is None:
            if marker:
                tag = marker[1].strip()
                opened = number
                contents = []
            continue
        if marker and not marker[1].strip():
            body = "".join(contents)
            if tag == "dec":
                if pending is not None:
                    raise ValueError(f"Dec block at line {pending[0]} has no expected-output block")
                if not body.strip():
                    raise ValueError(f"Empty Dec block at line {opened}")
                pending = (opened, body)
            elif tag == "expect":
                if pending is None:
                    raise ValueError(f"Expected-output block at line {opened} has no Dec block")
                pairs.append({"source_line": pending[0], "expect_line": opened,
                              "source": pending[1], "expected_stdout": body})
                pending = None
            tag = None
            opened = None
            contents = []
        else:
            contents.append(line)
    if tag is not None:
        raise ValueError(f"Unclosed {tag or 'raw'} block at line {opened}")
    if pending is not None:
        raise ValueError(f"Dec block at line {pending[0]} has no expected-output block")
    if not pairs:
        raise ValueError("The manual contains no literal Dec/expected-output pairs")
    return pairs


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--typst", default="typst", help="Typst 0.13.1 executable or path")
    parser.add_argument("--output", type=Path, default=ROOT / "build/docs",
                        help="PDF, report, and generated-example directory")
    parser.add_argument("--native", action="store_true", help="Also replay AOT and ORC at O0/O2")
    args = parser.parse_args(argv)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    report_path = output / "report.json"
    report = {"format": "aod.dec.documentation-build", "version": 1,
              "status": "RUNNING", "started_utc": datetime.now(timezone.utc).isoformat(),
              "root": str(ROOT), "source": str(MANUAL), "source_sha256": None,
              "driver_sha256": sha256(__file__), "creation_timestamp": CREATION_TIMESTAMP,
              "native_requested": args.native, "commands": [], "examples": []}

    def save():
        temporary = report_path.with_name(report_path.name + ".tmp")
        temporary.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        temporary.replace(report_path)

    def run(command, *, expected_stdout=None):
        command = [str(part) for part in command]
        entry = {"command": command, "cwd": str(ROOT), "status": "RUNNING"}
        report["commands"].append(entry)
        save()
        started = time.perf_counter()
        try:
            result = subprocess.run(command, cwd=ROOT, capture_output=True, timeout=300,
                                    env=dict(os.environ, PYTHONDONTWRITEBYTECODE="1"))
            stdout = result.stdout.decode("utf-8")
            stderr = result.stderr.decode("utf-8")
            entry.update(returncode=result.returncode, stdout=stdout, stderr=stderr,
                         elapsed_seconds=time.perf_counter() - started)
            if result.returncode:
                raise RuntimeError(f"Command exited {result.returncode}: {command!r}")
            if expected_stdout is not None:
                entry["expected_stdout_sha256"] = hashlib.sha256(expected_stdout.encode()).hexdigest()
                if stdout != expected_stdout:
                    entry["expected_stdout"] = expected_stdout
                    raise RuntimeError(f"Output differs from the documented bytes: {command!r}")
            entry["status"] = "PASS"
            save()
            return stdout
        except subprocess.TimeoutExpired as exc:
            entry.update(status="FAIL", error="command exceeded 300 seconds",
                         elapsed_seconds=time.perf_counter() - started)
            for name in ("stdout", "stderr"):
                value = getattr(exc, name)
                entry[name] = value.decode("utf-8", errors="replace") if isinstance(value, bytes) else value
            save()
            raise RuntimeError(f"Command timed out: {command!r}") from exc
        except (Exception, KeyboardInterrupt) as exc:
            entry.update(status="FAIL", error=str(exc), elapsed_seconds=time.perf_counter() - started)
            save()
            raise

    try:
        source_bytes = MANUAL.read_bytes()
        report["source_sha256"] = hashlib.sha256(source_bytes).hexdigest()
        pairs = example_pairs(source_bytes.decode("utf-8"))
        report["example_count"] = len(pairs)
        executable = shutil.which(args.typst)
        if executable is None:
            raise FileNotFoundError(f"Typst {TYPST_VERSION} is required; executable not found: {args.typst}")
        executable = str(Path(executable).resolve())
        version = run([executable, "--version"]).strip()
        report["typst"] = {"path": executable, "version": version, "sha256": sha256(executable)}
        if not re.fullmatch(r"typst 0\.13\.1(?: \([^\r\n]*\))?", version):
            raise RuntimeError(f"Expected Typst {TYPST_VERSION}, found {version!r}")
        pdf = output / "aod-dec-language-and-compiler.pdf"
        run([executable, "compile", "--root", ROOT, "--creation-timestamp", CREATION_TIMESTAMP,
             MANUAL, pdf])
        if not pdf.is_file() or not pdf.read_bytes().startswith(b"%PDF-"):
            raise RuntimeError("Typst did not create a PDF")
        report["pdf"] = {"path": str(pdf), "sha256": sha256(pdf), "bytes": pdf.stat().st_size}
        examples = output / "examples"
        examples.mkdir(parents=True, exist_ok=True)
        cli = [sys.executable, ROOT / "aodc.py"]
        for index, pair in enumerate(pairs, 1):
            source = examples / f"example-{index:02d}.dec"
            source.write_bytes(pair["source"].encode("utf-8"))
            expected = pair["expected_stdout"]
            record = {"number": index, "source_line": pair["source_line"],
                      "expect_line": pair["expect_line"], "path": str(source),
                      "source_sha256": sha256(source), "expected_stdout": expected,
                      "status": "RUNNING", "modes": []}
            report["examples"].append(record)
            save()
            run([*cli, "check", source], expected_stdout="OK: Dec core v1\n")
            run([*cli, "run", source], expected_stdout=expected)
            record["modes"].append("reference")
            ir = source.with_suffix(".json")
            run([*cli, "compile", source, "-o", ir])
            run([*cli, "exec", ir], expected_stdout=expected)
            record["modes"].append("typed-ir")
            record["ir_sha256"] = sha256(ir)
            if args.native:
                for opt in (0, 2):
                    binary = examples / f"example-{index:02d}-O{opt}"
                    run([*cli, "build", source, "-O", opt, "-o", binary])
                    run([binary], expected_stdout=expected)
                    record["modes"].append(f"aot-O{opt}")
                    run([*cli, "jit", source, "-O", opt], expected_stdout=expected)
                    record["modes"].append(f"jit-O{opt}")
            record["status"] = "PASS"
            save()
        if sha256(MANUAL) != report["source_sha256"]:
            raise RuntimeError("Manual source changed during the build; rerun against stable source")
        report["status"] = "PASS"
    except (Exception, KeyboardInterrupt) as exc:
        report["status"] = "FAIL"
        report["error"] = f"{type(exc).__name__}: {exc}"
        for record in report["examples"]:
            if record["status"] == "RUNNING":
                record["status"] = "FAIL"
    report["finished_utc"] = datetime.now(timezone.utc).isoformat()
    save()
    if report["status"] == "PASS":
        modes = "reference, typed IR, AOT/ORC O0/O2" if args.native else "reference and typed IR"
        print(f"PASS: PDF and {len(pairs)} examples ({modes}); report: {report_path}")
        return 0
    print(f"FAIL: {report['error']}; report: {report_path}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
