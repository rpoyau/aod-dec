#!/usr/bin/env python3
"""Conformance of the counted retained update through reference, AOT and ORC.

Programs contain initial operands and exogenous tickets only. Complete Manual
witnesses are reconstructed from emitted compiler decisions by the host model.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import gzip
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from runtime.dec_core import render
from runtime.dec_language import Interpreter, compile_source, dumps_ir, loads_ir
from simulations import retained_dec as dec


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def require(condition, message):
    if not condition:
        raise ArithmeticError(message)


def scenarios(host):
    d = host.TICKET_DENOMINATOR
    schedules = {
        "expiry_reentry": [(15, 15), (0, 15), (0, 15), (15, 15), (15, 15), (15, 15)],
        "both_pending_release": [(0, 0), (0, 0), (15, 15), (15, 15), (15, 15), (15, 15)],
        "partial_interfaces": [(7, 11), (13, 14), (15, 7), (11, 15), (14, 13), (15, 15)],
        "open_boundaries": [(15, 15)] * 6,
    }
    addressed = [(0, d - 1), (d // 2, 0), (d - 1, d // 2),
                 (d // 3, 2 * (d // 3)), (0, d - 1), (d - 1, 0)]
    cases = [{"name": name, "initial": host.initial_state(),
             "inputs": tuple((*masks, *tickets) for masks, tickets in zip(schedule, addressed)),
             "world": host.initialize_world()}
            for name, schedule in schedules.items()]
    seed = host.advance_world(host.initialize_world(),(15,15),(0,d-1))
    zero = host.advance_world(host.initialize_world(),(0,15),(0,0))
    zero = host.advance_world(zero,(15,15),(0,0))
    cancel = host.initialize_world()
    for tickets in ((0,0),(d-1,d-1),(0,0)):
        cancel = host.advance_world(cancel,(15,15),tickets)
    for family,world,masks in (("retention",seed,(15,15)), ("closed",seed,(0,15)),
                               ("zero",zero,(15,15)), ("cancellation",cancel,(15,15))):
        relation = next(r for r in reversed(world.relations) if r.target == 0)
        removed = host.remove_relation(world,relation.identity)
        restored = host.restore_relation(removed,relation.identity)
        require(world.waves == removed.waves == restored.waves and
                world.contacts == removed.contacts == restored.contacts and
                world.components == removed.components == restored.components and
                world.relations == removed.relations == restored.relations,
                "intervention changed immutable preparation/history")
        require(host.extract_state(restored) == host.extract_state(world), "restoration did not restore exact state")
        future = ((*masks,3*d//4,0), (*masks,d//4,d-1))
        for variant,start in (("retained",world),("removed",removed),("restored",restored)):
            cases.append({"name": family+"_"+variant, "initial": host.extract_state(start),
                          "inputs": future, "world": start, "intervention_family": family,
                          "intervention_variant": variant, "relation_id": relation.identity})
    return cases


def execute_reference(program):
    lines = []
    result = Interpreter(program, emit=lines.append, max_steps=4_000_000).run()
    return "".join(line + "\n" for line in lines), render(result)


def emitted_contract(row):
    return dec.transition_output(row)

def compare_step(row, expected):
    actual = emitted_contract(row)
    for name in actual:
        require(actual[name] == expected[name], "compiled retained update differs: " + name)
    require(row["total_a"] == row["geometry_a"] * sum(actual["counts"][0]), "A inventory total differs")
    require(row["total_b"] == row["geometry_b"] * sum(actual["counts"][1]), "B inventory total differs")
    return actual


def run(output):
    sources = ["simulations/retained_dec.py", "simulations/retained_continuation.py",
               "scripts/check_retained_continuation.py", "runtime/dec_language.py", "runtime/dec_core.py",
               "native/compiler.py", "native/runtime.cpp", "native/runtime.hpp", "native/runner.cpp",
               "native/toolchain-lock.json", "simulations/vendor/aod_manual_native.py", "simulations/manual_dec_fixture.py"]
    source_snapshot = {name: digest(ROOT / name) for name in sources}
    from simulations import retained_continuation as host
    from native import compiler as native
    require(source_snapshot == {name: digest(ROOT / name) for name in sources}, "operative source changed during import")

    output = Path(output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    report = {"format": "aod-dec.retained-continuation-conformance", "version": 1,
              "started_utc": datetime.now(timezone.utc).isoformat(), "status": "RUNNING",
              "scope": "Same counted scalar update from retained operands through reference/typed IR/LLVM AOT/ORC; full Manual witnesses checked by host. No performance or physical mapping claim.",
              "python": sys.version, "python_executable": sys.executable,
              "commands": [], "backends": [], "artifacts": {}, "scenarios": [],
              "source_hashes": source_snapshot}
    def persist():
        (output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    persist()
    try:
        cases = scenarios(host)
        source = dec.program_source(cases, host.TICKET_DENOMINATOR)
        program = compile_source(source)
        (output / "retained-update.dec").write_text(source)
        (output / "retained-update.json").write_text(dumps_ir(program))
        stdout, result = execute_reference(program)
        require(result == "0", "reference main result differs")
        expected_native_stdout = stdout + result + "\n"
        (output / "reference.stdout").write_text(expected_native_stdout)
        rows = dec.output_rows(stdout)
        cursor = 0
        for case in cases:
            state = case["initial"]
            trajectory = []
            for row_input in case["inputs"]:
                expected = host.transition(state, *row_input)
                actual = compare_step(rows[cursor], expected)
                trajectory.append(actual)
                state = actual["next_state"]
                cursor += 1
            report["scenarios"].append({"name": case["name"], "steps": len(trajectory),
                                        "initial_state": case["initial"], "inputs": case["inputs"],
                                        "initial_preparation_attempts": len(case["world"].attempts),
                                        "final_state": state, "rows": trajectory,
                                        "intervention_family": case.get("intervention_family"),
                                        "intervention_variant": case.get("intervention_variant"),
                                        "relation_id": case.get("relation_id")})
        require(cursor == len(rows), "unexpected extra compiler rows")
        report["matched_controls"] = matched_controls(report["scenarios"])
        ir_stdout, ir_result = execute_reference(loads_ir((output / "retained-update.json").read_text()))
        require((ir_stdout, ir_result) == (stdout, result), "typed IR replay differs")
        report["backends"].extend([{"name": "reference", "status": "PASS"},
                                    {"name": "typed-ir", "status": "PASS"}])
        persist()
        for opt in (0, 2):
            binary = output / f"retained-update-O{opt}"
            build = native.build(program, binary, opt=opt, cache_dir=output / "cache")
            command = [str(binary), "4000000", "128"]
            started = time.perf_counter_ns()
            proc = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, timeout=180,
                                  env=dict(os.environ, PYTHONDONTWRITEBYTECODE="1"))
            record = {"name": f"aot-O{opt}", "command": command, "returncode": proc.returncode,
                      "elapsed_ns": time.perf_counter_ns() - started,
                      "stdout_sha256": hashlib.sha256(proc.stdout.encode()).hexdigest(), "stderr": proc.stderr}
            report["commands"].append(record)
            (output / f"aot-O{opt}.stdout").write_text(proc.stdout)
            require(proc.returncode == 0 and proc.stdout == expected_native_stdout, f"AOT O{opt} differs")
            report["backends"].append({"name": f"aot-O{opt}", "status": "PASS", "build": build})
            with native.JitSession(program, opt=opt, cache_dir=output / "cache") as session:
                main = session.run(fuel=4_000_000)
                require(main["exit_code"] == 0 and (main["stdout"], main["result"]) == (stdout, result),
                        f"ORC main O{opt} differs")
                dynamic = []
                for case in cases:
                    state = case["initial"]
                    for values in case["inputs"]:
                        entry = session.run(function="counted_step", args=[dec.encode_state(state), *values], fuel=2_000_000)
                        require(entry["exit_code"] == 0, "dynamic ORC update failed: " + str(entry["error"]))
                        emitted = dec.output_rows(entry["stdout"])
                        require(len(emitted) == 1, "dynamic ORC output row count differs")
                        compare_step(emitted[0], host.transition(state, *values))
                        require(int(entry["result"]) == emitted[0]["next_state"], "dynamic state return differs")
                        state = dec.decode_state(int(entry["result"]))
                        dynamic.append({"state": state, "dispatch": entry["dispatch"]})
                report["backends"].append({"name": f"orc-O{opt}", "status": "PASS",
                    "dynamic_steps": len(dynamic), "runtime_input_feedback": True,
                    "binding": session.info, "dynamic_state_sha256": hashlib.sha256(json.dumps(dynamic,sort_keys=True).encode()).hexdigest()})
            persist()
        # Full-witness reconstruction is completed below once the host supplies
        # its frozen admission adapter; scalar agreement alone is insufficient.
        report["witness_reconstruction"] = reconstruct_witnesses(host, cases, rows, output)
        final_hashes = {name: digest(ROOT / name) for name in sources}
        report["sources_unchanged"] = final_hashes == source_snapshot
        require(report["sources_unchanged"], "operative source changed during conformance")
        report["artifacts"] = {p.name: digest(p) for p in output.iterdir()
                               if p.is_file() and p.name != "results.json"}
        report["status"] = "PASS"
    except Exception as error:
        report["status"] = "FAIL"
        report["error"] = {"type": type(error).__name__, "message": str(error)}
        raise
    finally:
        report["completed_utc"] = datetime.now(timezone.utc).isoformat()
        persist()
    return report


def matched_controls(scenarios):
    result = []
    for family in ("retention","closed","zero","cancellation"):
        cases = {row["intervention_variant"]: row for row in scenarios if row["intervention_family"] == family}
        base,removed,restored = (cases[name] for name in ("retained","removed","restored"))
        require(base["inputs"] == removed["inputs"] == restored["inputs"], "matched tickets or gates changed")
        require(base["rows"] == restored["rows"], "restored compiled trajectory differs")
        # Outcomes exclude retained coordinates and intervention labels.
        observable = lambda case: [(row["words"],row["hands"],row["completed"],row["contact"]) for row in case["rows"]]
        changed = observable(base) != observable(removed)
        require(changed == (family in ("retention","cancellation")), "unexpected compiled intervention operativity: " + family)
        result.append({"family":family,"status":"PASS","restoration_equal":True,
                       "realized_outcome_changed":changed,"matched_inputs":True,
                       "preserved_immutable_history":True})
    return result


def reconstruct_witnesses(host, cases, rows, output):
    from simulations.manual_dec_fixture import exact_data
    cursor = 0
    records = []
    for case in cases:
        world = case["world"]
        for values in case["inputs"]:
            world = host.advance_world(world, values[:2], values[2:], computed_row=emitted_contract(rows[cursor]))
            cursor += 1
        host.validate_world(world)
        raw = json.dumps(exact_data(world),sort_keys=True,separators=(",", ":")).encode()
        path = output / (case["name"] + "-witness.json.gz")
        path.write_bytes(gzip.compress(raw,mtime=0))
        records.append({"name": case["name"], "steps": len(case["inputs"]), "status": "PASS",
            "initial_preparation_attempts": len(case["world"].attempts),
            "total_attempts": len(world.attempts),
            "file": path.name, "sha256": digest(path), "decoded_sha256": hashlib.sha256(raw).hexdigest(),
            "final_state": host.extract_state(world),
            "wave_identities": [wave.identity for wave in world.waves],
            "closure_counts": [len(wave.returns) for wave in world.waves],
            "relation_count": len(world.relations), "contact_count": len(world.contacts)})
    return {"status": "PASS", "scenarios": records,
            "scope": "Host reconstructs complete admitted witnesses from emitted compiler selections and independently extracts updated state."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "build/retained-continuation")
    args = parser.parse_args()
    try:
        result = run(args.output)
    except Exception as error:
        print(f"retained conformance: {error}", file=sys.stderr)
        return 1
    print(json.dumps({"status": result["status"], "backends": [b["name"] for b in result["backends"]]}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
