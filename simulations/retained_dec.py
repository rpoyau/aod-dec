"""Scalar D.E.C. execution of the declared retained-continuation update.

The generated program computes geometry/counts/selection/update from retained
operands. Complete Manual formation/certificate witnesses remain host records.
"""
from __future__ import annotations

from itertools import permutations
import hashlib
import json
import re

from runtime.dec_language import Interpreter, compile_source, dumps_ir, loads_ir
from runtime.dec_core import render


WORD_AXES = tuple(permutations(range(4), 3))


def _function(name, parameters, result_type, body):
    names = ", ".join(name for _, name in parameters)
    typed = ", ".join(typ + " " + name for typ, name in parameters)
    return "\n".join([f"Dec {name}({names}) -> result:",
                       f"    ({typed}) -> {result_type} result:",
                       *("        " + line for line in body)]) + "\n\n"


def geometry_source():
    """Enumerate the fixed 24 three-axis C6 recipes inside generated code."""
    source = _function("gate_open", (("int", "mask"), ("int", "axis")), "bool", [
        "assert mask >= 0 and mask < 16",
        "assert axis >= 0 and axis < 4",
        "int divisor = 1 if axis == 0 else (2 if axis == 1 else (4 if axis == 2 else 8))",
        "result = (mask // divisor) % 2 == 1",
    ])
    params = (("int", "ancestor"), ("int", "mask"))
    predicates = [f"ancestor != {a} and gate_open(mask, {a}) and gate_open(mask, {b}) and gate_open(mask, {c})"
                  for a, b, c in WORD_AXES]
    body = ["assert ancestor >= 0 and ancestor < 4", "assert mask >= 0 and mask < 16"]
    body += [f"int e{n} = 1 if {predicate} else 0" for n, predicate in enumerate(predicates)]
    body += ["result = " + " + ".join(f"e{n}" for n in range(24))]
    source += _function("word_count", params, "int", body)
    body = ["int total = word_count(ancestor, mask)", "assert rank >= 0 and rank < total"]
    body += [f"int e{n} = 1 if {predicate} else 0" for n, predicate in enumerate(predicates)]
    for n in range(24):
        body.append(f"int c{n} = " + ("e0" if n == 0 else f"c{n-1} + e{n}"))
    chosen = "23"
    for n in reversed(range(23)):
        chosen = f"{n} if rank < c{n} else ({chosen})"
    body += ["result = " + chosen]
    source += _function("word_select", (*params, ("int", "rank")), "int", body)
    source += _function("word_last_axis", (("int", "word"),), "int", [
        "assert word >= 0 and word < 24",
        "result = " + _choice("word", [axes[-1] for axes in WORD_AXES]),
    ])
    return source


def _choice(name, values):
    result = str(values[-1])
    for n in reversed(range(len(values) - 1)):
        result = f"{values[n]} if {name} == {n} else ({result})"
    return result


def reference_call(program, function, args, *, fuel=2_000_000):
    """Run a typed source function using actual runtime operands."""
    emitted = []
    vm = Interpreter(program, emit=emitted.append, max_steps=fuel)
    value = vm.call(function, list(args))
    return {"stdout": "".join(line + "\n" for line in emitted), "result": render(value)}


def exact_integer_lines(stdout):
    if not isinstance(stdout, str):
        raise TypeError("backend output must be text")
    rows = stdout.splitlines()
    if any(re.fullmatch(r"-?(?:0|[1-9][0-9]*)", row) is None for row in rows):
        raise ValueError("backend output is not canonical exact integer rows")
    return tuple(int(row) for row in rows)


# Finite H=6, two-field application domain. Sign coordinates have two values;
# all other entries use the inclusive integer interval recorded here.
STATE_SCHEMA = (
    ("tick", 0, 6), ("incoming_a", 0, 3), ("incoming_b", 0, 3),
    ("chi_a", -1, 1), ("chi_b", -1, 1),
    ("count_a", 3, 9), ("count_b", 2, 8),
    ("since_a", 0, 6), ("since_b", 0, 6), ("contact_age", 0, 3),
    ("previous_ref_a", 0, 6), ("previous_ref_b", 0, 6),
    ("r0a", -42, 42), ("p0a", 0, 1), ("r1a", -42, 42), ("p1a", 0, 1),
    ("r0b", -42, 42), ("p0b", 0, 1), ("r1b", -42, 42), ("p1b", 0, 1),
)
STATE_FIELDS = tuple(row[0] for row in STATE_SCHEMA)
OUTPUT_FIELDS = ("geometry_a", "geometry_b", "negative_a", "positive_a",
    "negative_b", "positive_b", "total_a", "total_b", "rank_a", "rank_b",
    "word_a", "word_b", "new_relation_a", "new_relation_b", "new_presence",
    "contact", "next_state")


def _radix(field):
    name, low, high = field
    return 2 if name.startswith("chi_") else high - low + 1


def encode_state(state):
    if type(state) is not tuple or len(state) != len(STATE_SCHEMA):
        raise ValueError("retained state requires the declared immutable 20-tuple")
    result, multiplier = 0, 1
    for value, field in zip(state, STATE_SCHEMA):
        name, low, high = field
        if type(value) is not int or not low <= value <= high:
            raise ValueError("invalid exact retained state field: " + name)
        if name.startswith("chi_"):
            if value not in (-1, 1):
                raise ValueError("chirality must be -1 or +1")
            digit = (value + 1) // 2
        else:
            digit = value - low
        result += multiplier * digit
        multiplier *= _radix(field)
    return result


def decode_state(encoded):
    if type(encoded) is not int or encoded < 0:
        raise ValueError("encoded retained state must be a nonnegative exact integer")
    values = []
    for field in STATE_SCHEMA:
        name, low, _ = field
        encoded, digit = divmod(encoded, _radix(field))
        values.append(2 * digit - 1 if name.startswith("chi_") else digit + low)
    if encoded:
        raise ValueError("encoded state exceeds the declared domain")
    return tuple(values)


def _encode_expression(names):
    expression = "0"
    for field, name in reversed(tuple(zip(STATE_SCHEMA, names))):
        field_name, low, _ = field
        digit = f"(({name} + 1) // 2)" if field_name.startswith("chi_") else f"({name} - ({low}))"
        expression = f"({digit} + {_radix(field)} * {expression})"
    return expression


def kernel_source(denominator):
    """Compile the agreed unit-opportunity rule, not an evaluated lookup table."""
    if type(denominator) is not int or denominator <= 0:
        raise ValueError("ticket denominator must be a positive exact integer")
    body = ["assert state >= 0", "int rest = state"]
    for field in STATE_SCHEMA:
        name, low, _ = field
        digit = f"rest % {_radix(field)}"
        expr = f"2 * ({digit}) - 1" if name.startswith("chi_") else f"({digit}) + ({low})"
        body.extend([f"int {name} = {expr}", f"rest = rest // {_radix(field)}"])
    body.extend(["assert rest == 0", "assert tick < 6",
        f"assert ticket_a >= 0 and ticket_a < {denominator}",
        f"assert ticket_b >= 0 and ticket_b < {denominator}"])
    for carrier in ("a", "b"):
        for age in (0, 1):
            body.append(f"assert p{age}{carrier} == 1 or r{age}{carrier} == 0")
        neg = " + ".join(f"(-r{age}{carrier} if p{age}{carrier} == 1 and r{age}{carrier} < 0 else 0)" for age in (0, 1))
        pos = " + ".join(f"(r{age}{carrier} if p{age}{carrier} == 1 and r{age}{carrier} > 0 else 0)" for age in (0, 1))
        body.extend([
            f"int geometry_{carrier} = word_count(incoming_{carrier}, mask_{carrier})",
            f"int negative_{carrier} = 1 + {neg}", f"int positive_{carrier} = 1 + {pos}",
            f"int total_{carrier} = geometry_{carrier} * (negative_{carrier} + positive_{carrier})",
            f"assert total_{carrier} == 0 or {denominator} % total_{carrier} == 0",
            f"int rank_{carrier} = (ticket_{carrier} * total_{carrier}) // {denominator} if total_{carrier} > 0 else -1",
            f"int selected_chi_{carrier} = (-1 if rank_{carrier} < geometry_{carrier} * negative_{carrier} else 1) if total_{carrier} > 0 else chi_{carrier}",
            f"int word_rank_{carrier} = (rank_{carrier} // negative_{carrier} if selected_chi_{carrier} == -1 else (rank_{carrier} - geometry_{carrier} * negative_{carrier}) // positive_{carrier}) if total_{carrier} > 0 else -1",
            f"int word_{carrier} = word_select(incoming_{carrier}, mask_{carrier}, word_rank_{carrier}) if total_{carrier} > 0 else -1",
            f"int next_incoming_{carrier} = word_last_axis(word_{carrier}) if total_{carrier} > 0 else incoming_{carrier}",
            f"int completed_{carrier} = 1 if total_{carrier} > 0 else 0",
            f"int next_count_{carrier} = count_{carrier} + completed_{carrier}",
            f"int advanced_since_{carrier} = since_{carrier} + completed_{carrier}",
        ])
    body.extend([
        "int contact = 1 if completed_a == 1 and completed_b == 1 else 0",
        "int new_presence = 1 if contact == 1 and contact_age + 1 <= 2 else 0",
        "int difference = next_count_a - next_count_b",
        "int units_a = 3 + previous_ref_b + advanced_since_b",
        "int units_b = 3 + previous_ref_a + advanced_since_a",
        "units_a = 6 if units_a > 6 else units_a", "units_b = 6 if units_b > 6 else units_b",
        "int new_relation_a = -selected_chi_b * units_a * difference if new_presence == 1 else 0",
        "int new_relation_b = selected_chi_a * units_b * difference if new_presence == 1 else 0",
        "int next_age = 0 if contact == 1 else (3 if contact_age >= 2 else contact_age + 1)",
        "int next_since_a = 0 if contact == 1 else advanced_since_a",
        "int next_since_b = 0 if contact == 1 else advanced_since_b",
        "int next_ref_a = advanced_since_a if contact == 1 else previous_ref_a",
        "int next_ref_b = advanced_since_b if contact == 1 else previous_ref_b",
    ])
    next_names = ("tick + 1", "next_incoming_a", "next_incoming_b", "selected_chi_a", "selected_chi_b",
        "next_count_a", "next_count_b", "next_since_a", "next_since_b", "next_age", "next_ref_a", "next_ref_b",
        "new_relation_a", "new_presence", "r0a", "p0a", "new_relation_b", "new_presence", "r0b", "p0b")
    body.extend(["assert next_count_a <= 9 and next_count_b <= 8",
        "assert next_since_a <= 6 and next_since_b <= 6",
        "assert next_ref_a <= 6 and next_ref_b <= 6",
        "assert new_relation_a >= -42 and new_relation_a <= 42",
        "assert new_relation_b >= -42 and new_relation_b <= 42"])
    body.append("int next_state = " + _encode_expression(next_names))
    body.extend("emit " + name for name in OUTPUT_FIELDS)
    body.append("result = next_state")
    return geometry_source() + _function("counted_step", tuple(("int", name) for name in (
        "state", "mask_a", "mask_b", "ticket_a", "ticket_b")), "int", body)


def program_source(cases, denominator):
    """Embed initial operands/exogenous inputs only; every next state is computed.

    Each case is {'initial': state20tuple, 'inputs': [(mask_a,mask_b,u_a,u_b), ...]}.
    The compiled counted_step body is identical for every scenario and backend.
    """
    cases = tuple(cases)
    if not cases:
        raise ValueError("at least one scenario is required")
    body = []
    manifest = []
    for index, case in enumerate(cases):
        initial = case["initial"]
        encoded = encode_state(initial)
        inputs = tuple(tuple(row) for row in case["inputs"])
        if initial[0] + len(inputs) > 6:
            raise ValueError("scenario exceeds the six-attempt domain")
        body.append(f"int state{index} = {encoded}")
        for row in inputs:
            if len(row) != 4 or any(type(value) is not int for value in row):
                raise ValueError("input rows require four exact integers")
            ma, mb, ua, ub = row
            if not (0 <= ma < 16 and 0 <= mb < 16 and 0 <= ua < denominator and 0 <= ub < denominator):
                raise ValueError("input row lies outside the declared mask/ticket domain")
            body.append(f"state{index} = counted_step(state{index}, {ma}, {mb}, {ua}, {ub})")
        manifest.append({"initial": initial, "inputs": inputs})
    body.append("result = 0")
    encoded = json.dumps(manifest, sort_keys=True, separators=(",", ":"))
    return ("# Retained unit-opportunity update; no host-selected future state.\n"
            + "# Initial operands and addressed inputs SHA-256: " + hashlib.sha256(encoded.encode()).hexdigest() + "\n"
            + f"# Exact ticket grid denominator: {denominator}\n"
            + kernel_source(denominator) + _function("main", (), "int", body))


def output_rows(stdout):
    values = exact_integer_lines(stdout)
    width = len(OUTPUT_FIELDS)
    if len(values) % width:
        raise ValueError("backend output has an incomplete retained-step row")
    rows = []
    for offset in range(0, len(values), width):
        row = dict(zip(OUTPUT_FIELDS, values[offset:offset + width]))
        row["state"] = decode_state(row["next_state"])
        rows.append(row)
    return rows


def transition_output(row):
    """Translate exact compiler fields to the host's frozen transition interface."""
    for field in ("contact", "new_presence"):
        if type(row[field]) is not int or row[field] not in (0, 1):
            raise ValueError("compiled flag is not an exact zero/one: " + field)
    completed = (row["word_a"] >= 0, row["word_b"] >= 0)
    hands = tuple(row["state"][3+i] if done else 0 for i, done in enumerate(completed))
    token_ranks = []
    for carrier, hand in zip(("a", "b"), hands):
        rank = row["rank_" + carrier]
        if hand == -1:
            token_ranks.append(rank % row["negative_" + carrier])
        elif hand == 1:
            offset = row["geometry_" + carrier] * row["negative_" + carrier]
            token_ranks.append((rank - offset) % row["positive_" + carrier])
        else:
            token_ranks.append(-1)
    return {"next_state": row["state"],
            "counts": ((row["negative_a"], row["positive_a"]),
                       (row["negative_b"], row["positive_b"])),
            "geometry_counts": (row["geometry_a"], row["geometry_b"]),
            "totals": (row["total_a"], row["total_b"]),
            "words": (row["word_a"], row["word_b"]), "hands": hands,
            "token_ranks": tuple(token_ranks), "completed": completed,
            "contact": bool(row["contact"]),
            "new_relations": ((row["new_relation_a"], row["new_presence"]),
                              (row["new_relation_b"], row["new_presence"]))}
