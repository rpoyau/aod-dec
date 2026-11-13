"""Replay declared encounter locations and exact tickets through scalar D.E.C.

The host samples movement and random tickets. The generated program computes the
population profile, local/contact probabilities, ticket decisions and selected
complete handed returns. Each return retains its outgoing step, structural
hinge and opposite return step. The short core-v1 trace is a separate declared
realization of that event pattern, not a physical proton or a six-edge motif.
"""
from __future__ import annotations

from dataclasses import asdict
from fractions import Fraction
import hashlib
import json
from pathlib import Path
import re

from . import chiral_collision as host
from .tesseract_cells import FACETS


MAX_EXPANDED_STEPS = 32
STEP_OUTPUT_FIELDS = ("q_a", "q_b", "chi_a", "chi_b", "phases_a", "phases_b",
                      "beats_a", "beats_b")


def _literal(value):
    exact = Fraction(value)
    return f"ratio({exact.numerator}, {exact.denominator})"


def _digest(value):
    return hashlib.sha256(json.dumps(host.exact_data(value), sort_keys=True,
                                    separators=(",", ":")).encode()).hexdigest()


def _validated(config, path):
    if not isinstance(config, host.CollisionConfig):
        raise TypeError("config must be CollisionConfig")
    if config.steps > MAX_EXPANDED_STEPS:
        raise ValueError("scalar D.E.C. fixture exceeds 32 expanded steps")
    if not isinstance(path, dict) or path.get("config") != config:
        raise ValueError("path configuration differs from generator configuration")
    host.validate_path(path)
    return path


def expected_numeric_output(config, path):
    """Return the host comparator prefix, eight scalar lines per declared tick.

    Ratios use ``numerator/denominator`` even for integral values. Remaining
    fields use exact decimal integers. The generated program then emits its
    two retained JSON traces and returns integer zero; those traces must be
    structurally validated independently rather than replaced by this prefix.
    """
    path = _validated(config, path)
    lines = []
    for row in path["steps"]:
        for value in row["flip_probabilities"]:
            value = Fraction(value)
            lines.append(f"{value.numerator}/{value.denominator}")
        n = row["ordinal"]
        lines.extend(str(value) for value in (row["after"].chi_a, row["after"].chi_b,
                                              n + 2, n + 2, n + 1, n + 1))
    return "".join(line + "\n" for line in lines)


def numeric_prefix(path):
    """Return the exact eight-field-per-step comparison prefix for a path."""
    if not isinstance(path, dict) or "config" not in path:
        raise ValueError("complete path record required")
    return expected_numeric_output(path["config"], path)


def validate_native_output(stdout, path):
    """Check numeric values and independent complete-return event structure.

    This is shared by source-VM and native-execution evidence. It checks each
    explicit event against the declared short return, including retained starts,
    opposite orientations, all hinge positions, lineage, phase ranges and unique
    certificate identifiers. It does not substitute for the compiler/runtime's
    full certificate validation or claim equivalence to the Manual producer's
    differently represented return certificates.
    """
    prefix = numeric_prefix(path)
    if not isinstance(stdout, str):
        raise TypeError("stdout must be text")
    lines = stdout.splitlines()
    count = len(path["steps"]) * len(STEP_OUTPUT_FIELDS)
    if len(lines) != count + 3 or lines[-1] != "0":
        raise ValueError("output must contain the numeric prefix, two traces and result zero")
    if "".join(line + "\n" for line in lines[:count]) != prefix:
        raise ValueError("numeric output differs from exact path decisions")
    seen_certificates = set()
    traces = []
    for carrier, line in enumerate(lines[count:count + 2]):
        try:
            trace = json.loads(line)
        except json.JSONDecodeError as error:
            raise ValueError("retained trace output is not JSON") from error
        if not isinstance(trace, dict) or set(trace) != {
            "scope", "lineage", "start", "events", "closures", "parents", "proposal"
        }:
            raise ValueError("retained trace fields differ from the core-v1 contract")
        if (trace["scope"] != host.SCOPE or trace["lineage"] != ("carrier-A", "carrier-B")[carrier]
                or json.dumps(trace["start"], sort_keys=True) != '{"value": 0}'
                or trace["parents"] != [] or trace["proposal"] != ""):
            raise ValueError("retained trace scope, lineage, start or parent binding differs")
        attr = "chi_a" if carrier == 0 else "chi_b"
        signs = [getattr(path["initial"], attr)] * 2 + [
            getattr(row["after"], attr) for row in path["steps"]]
        if (not isinstance(trace["events"], list) or len(trace["events"]) != 3 * len(signs)
                or not isinstance(trace["closures"], list) or len(trace["closures"]) != len(signs)):
            raise ValueError("trace must retain every initial and selected complete return")
        for n, sign in enumerate(signs):
            expected = [
                {"ordinal": 3*n + 1, "kind": "step", "source": 0, "target": 1, "orientation": sign},
                {"ordinal": 3*n + 2, "kind": "hinge", "source": 1, "target": 1, "orientation": 0},
                {"ordinal": 3*n + 3, "kind": "step", "source": 1, "target": 0, "orientation": -sign}]
            # Canonical JSON distinguishes a forged Boolean from its integer
            # comparison alias (True == 1 in Python).
            if json.dumps(trace["events"][3*n:3*n+3], sort_keys=True) != json.dumps(expected, sort_keys=True):
                raise ValueError("selected return lacks its exact step-hinge-opposite-return history")
            certificate = trace["closures"][n]
            if (not isinstance(certificate, dict) or set(certificate) != {"begin", "end", "digest"}
                    or type(certificate["begin"]) is not int or certificate["begin"] != 3*n
                    or type(certificate["end"]) is not int or certificate["end"] != 3*n + 3
                    or not isinstance(certificate["digest"], str)
                    or re.fullmatch(r"[0-9a-f]{64}", certificate["digest"]) is None
                    or certificate["digest"] in seen_certificates):
                raise ValueError("invalid or replayed complete-return certificate range/identity")
            seen_certificates.add(certificate["digest"])
        traces.append(trace)
    return {"status": "PASS", "numeric_lines": count, "trace_count": 2,
            "returns_per_trace": len(path["steps"]) + 2,
            "events_per_trace": 3 * (len(path["steps"]) + 2),
            "hinges_per_trace": len(path["steps"]) + 2,
            "scope": "exact path decisions and declared core-v1 return event structure"}


def dec_source(config=host.CollisionConfig(), path=None):
    """Generate a bounded deterministic scalar replay, with no native sampling.

    ``path`` is an in-memory result of ``host.simulate_path``. It is replay
    validated before generation. When omitted, a seed-7 host path is declared.
    Destinations and tickets are inputs; D.E.C. computes probabilities and signs
    rather than embedding host decisions. Every carrier starts with two returns
    of its initial handedness, followed by one complete return per selected tick.

    Output is eight scalar lines per step in ``STEP_OUTPUT_FIELDS`` order, then
    the full carrier-A and carrier-B JSON traces, followed by integer result 0.
    Positions are coarse facet-support labels; the fixed Q4 closure trace is not
    the geometric motion between those facets. No coupling/fusion is inferred.
    """
    if not isinstance(config, host.CollisionConfig):
        raise TypeError("config must be CollisionConfig")
    if config.steps > MAX_EXPANDED_STEPS:
        raise ValueError("scalar D.E.C. fixture exceeds 32 expanded steps")
    if path is None:
        path = host.simulate_path(config, seed=7)
    path = _validated(config, path)
    initial = path["initial"]
    population = str(config.population_counts[-1])
    for facet, count in reversed(tuple(zip(FACETS[:-1], config.population_counts[:-1]))):
        population = f"{count} if facet == {json.dumps(facet)} else ({population})"
    facet_domain = " or ".join(f"facet == {json.dumps(facet)}" for facet in FACETS)
    helper = f'''Dec population_count(facet) -> count:
    (str facet) -> int count:
        assert {facet_domain}
        count = {population}

Dec q_local(facet) -> q:
    (str facet) -> ratio q:
        ratio score = ratio(population_count(facet) - 1, 3)
        q = {_literal(config.q_outer)} + ({_literal(config.q_core)} - {_literal(config.q_outer)}) * score

Dec q_contact(base, present) -> q:
    (ratio base, bool present) -> ratio q:
        q = base + {_literal(config.contact_gain)} * (ratio(1, 1) - base) if present else base

Dec recur_plus(t) -> out:
    (trace t) -> trace out:
        t = step(t, q4(1), t1(1))
        t = hinge(t)
        t = step(t, q4(0), t1(-1))
        out = close(t)

Dec recur_minus(t) -> out:
    (trace t) -> trace out:
        t = step(t, q4(1), t1(-1))
        t = hinge(t)
        t = step(t, q4(0), t1(1))
        out = close(t)

'''
    body = [f"int chi_a = {initial.chi_a}", f"int chi_b = {initial.chi_b}",
            f"str facet_a = {json.dumps(initial.facet_a)}",
            f"str facet_b = {json.dumps(initial.facet_b)}",
            f'trace a = trace({json.dumps(host.SCOPE)}, "carrier-A", q4(0))',
            f'trace b = trace({json.dumps(host.SCOPE)}, "carrier-B", q4(0))']
    for carrier in ("a", "b"):
        body.extend([f"{carrier} = recur_plus({carrier}) if chi_{carrier} == 1 else recur_minus({carrier})"] * 2)
        body.extend((f"assert phases({carrier}) == 2", f"assert beats({carrier}) == 1"))
    body.extend(("ratio q_a = ratio(0, 1)", "ratio q_b = ratio(0, 1)",
                 "bool present = false", "bool flip_a = false", "bool flip_b = false"))
    for row in path["steps"]:
        n, after = row["ordinal"], row["after"]
        ta, tb = row["flip_tickets"]
        denominator = row["flip_denominator"]
        body.extend((f"# Tick {n}: declared post-move facets; movement tickets {row['movement_tickets']} / 8.",
                     f"facet_a = {json.dumps(after.facet_a)}", f"facet_b = {json.dumps(after.facet_b)}",
                     "present = facet_a == facet_b",
                     "q_a = q_contact(q_local(facet_a), present)",
                     "q_b = q_contact(q_local(facet_b), present)",
                     f"flip_a = ratio({ta}, {denominator}) < q_a",
                     f"flip_b = ratio({tb}, {denominator}) < q_b",
                     "chi_a = -chi_a if flip_a else chi_a", "chi_b = -chi_b if flip_b else chi_b",
                     "a = recur_plus(a) if chi_a == 1 else recur_minus(a)",
                     "b = recur_plus(b) if chi_b == 1 else recur_minus(b)",
                     f"assert phases(a) == {n + 2}", f"assert phases(b) == {n + 2}",
                     f"assert beats(a) == {n + 1}", f"assert beats(b) == {n + 1}",
                     "emit q_a", "emit q_b", "emit chi_a", "emit chi_b",
                     "emit phases(a)", "emit phases(b)", "emit beats(a)", "emit beats(b)"))
    body.extend(("emit a", "emit b", "result = 0"))
    tickets = [{key: row[key] for key in ("ordinal", "before", "after", "movement_tickets",
                "movement_denominator", "flip_tickets", "flip_denominator")} for row in path["steps"]]
    comments = ["# Generated scalar D.E.C. completed-return replay; no native random sampling.",
                "# Coarse facet motion and Q4 return realizations are separately declared maps.",
                f"# Generator source SHA-256: {hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}",
                f"# Host model evaluator source SHA-256: {hashlib.sha256(Path(host.__file__).read_bytes()).hexdigest()}",
                f"# Configuration SHA-256: {_digest(asdict(config))}",
                f"# Retained path SHA-256: {_digest(path)}",
                f"# Declared locations/tickets SHA-256: {_digest(tickets)}",
                f"# Manual producer SHA-256: {path['source']['sha256']}",
                "# Scalar output: " + ", ".join(STEP_OUTPUT_FIELDS) + "; then A trace, B trace, result."]
    return "\n".join(comments) + "\n" + helper + "Dec main() -> result:\n    () -> int result:\n" + \
        "".join(f"        {line}\n" for line in body)
