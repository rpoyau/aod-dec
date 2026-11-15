"""Shared eligibility and exact input contract for compiled scalar-entry batches.

Only the selected typed branch must belong to this initial pure int/bool target.
Each row is an independent invocation with its own existing source fuel/depth.
These checks add no field formation, observation or probability semantics.
"""
from dataclasses import dataclass
import hashlib
import json

from runtime.dec_language import DecError, compile_source, dumps_ir, loads_ir


MAX_ROWS = 65536
MAX_PARAMS = 16
MAX_INPUT_BITS = 8192
MAX_INPUT_BYTES = 16*1024*1024
MAX_FUEL = 1000000
MAX_DEPTH = 1024
MAX_TOTAL_WORK = 100000000
MAX_INTERMEDIATE_BITS = 262144
MAX_INTEGER_BIT_WORK = 64*1024*1024


class BatchError(DecError):
    pass


def require(condition, message):
    if not condition:
        raise BatchError(message)


def _digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()).hexdigest()


@dataclass(frozen=True)
class Entry:
    program: dict
    function: str
    branch: dict
    param_types: tuple
    result_type: str
    source_sha256: str
    ir_sha256: str
    entry_sha256: str
    max_steps: int


def _expression_steps(expr):
    require(expr["type"] in ("int", "bool"), "compiled batch expressions require int/bool types")
    kind = expr["kind"]
    if kind in ("literal", "name"):
        return 1
    if kind in ("binary", "compare"):
        require(expr["op"] in (("+", "-", "*") if kind == "binary" else ("==", "!=", "<", "<=", ">", ">=")),
                "unsupported compiled batch operator: " + expr["op"])
        return 1+_expression_steps(expr["left"])+_expression_steps(expr["right"])
    if kind == "unary":
        require(expr["op"] in ("+", "-", "not"), "unsupported compiled batch unary operator")
        return 1+_expression_steps(expr["value"])
    if kind == "boolean":
        return 1+sum(_expression_steps(value) for value in expr["values"])
    if kind == "conditional":
        return 1+_expression_steps(expr["condition"])+max(_expression_steps(expr["yes"]), _expression_steps(expr["no"]))
    raise BatchError("compiled batch entry rejects calls and unsupported expressions")


def select_entry(program, function, *, param_types=None):
    """Return an Entry with a canonical program and one pure typed IR branch.

    ``program`` is Dec source text or verified IR. Overloads require an explicit
    list/tuple of ``param_types`` unless the function has only one branch.
    """
    require(type(function) is str and function, "exact entry function name required")
    verified = loads_ir(dumps_ir(compile_source(program) if type(program) is str else program))
    matches = [item for item in verified["functions"] if item["name"] == function]
    require(len(matches) == 1, "compiled batch entry function is unavailable")
    branches = matches[0]["branches"]
    if param_types is not None:
        require(type(param_types) in (tuple, list) and all(type(value) is str for value in param_types),
                "explicit exact parameter type sequence required")
        wanted = tuple(param_types)
        branches = [branch for branch in branches if tuple(p["type"] for p in branch["params"]) == wanted]
    require(len(branches) == 1, "compiled batch requires one selected signature; provide param_types for overloads")
    branch = branches[0]
    kinds = tuple(p["type"] for p in branch["params"])
    require(len(kinds) <= MAX_PARAMS and all(kind in ("int", "bool") for kind in kinds),
            "compiled batch supports at most 16 int/bool parameters")
    require(branch["return_type"] in ("int", "bool"), "compiled batch result must be int or bool")
    steps = 1  # Existing native/reference source-function call tick.
    for statement in branch["body"]:
        require(statement["kind"] in ("assign", "assert", "return"),
                "compiled batch entry rejects effects including emit")
        steps += 1+_expression_steps(statement["value"])
    raw_ir = dumps_ir(verified).encode()
    return Entry(verified, function, branch, kinds, branch["return_type"], verified["source_sha256"],
                 hashlib.sha256(raw_ir).hexdigest(), _digest((function, branch)), steps)


def entry_binding(entry):
    """Copy provenance after checking that the selected IR was not altered."""
    require(type(entry) is Entry, "typed compiled entry required")
    require(hashlib.sha256(dumps_ir(entry.program).encode()).hexdigest() == entry.ir_sha256 and
            _digest((entry.function, entry.branch)) == entry.entry_sha256 and
            entry.program["source_sha256"] == entry.source_sha256,
            "compiled batch entry changed after selection")
    require(tuple(p["type"] for p in entry.branch["params"]) == entry.param_types and
            entry.branch["return_type"] == entry.result_type, "compiled entry signature changed")
    require(type(entry.max_steps) is int and entry.max_steps ==
            1+sum(1+_expression_steps(statement["value"]) for statement in entry.branch["body"]),
            "compiled entry source-work bound changed")
    return {"function": entry.function, "param_types": list(entry.param_types), "result_type": entry.result_type,
            "source_sha256": entry.source_sha256, "ir_sha256": entry.ir_sha256,
            "entry_sha256": entry.entry_sha256, "max_steps_per_lane": entry.max_steps,
            "domain": "pure nonrecursive int/bool entry; independent per-lane source fuel/depth"}


def _integer_work(entry, inputs):
    """Conservative bit-growth bound, not execution or a claimed RSS bound.

    Both branches count toward the bound. Addition/subtraction may add one bit;
    multiplication adds operand widths. Bounds use the largest admitted input
    width per parameter, so this deliberately rejects some inexpensive special
    cases rather than risking unbounded GMP work in the batch target.
    """
    environment = {parameter["name"]: max((abs(row[index]).bit_length() for row in inputs), default=0)
                   for index, parameter in enumerate(entry.branch["params"])}
    work = sum(environment.values())
    def expression(value):
        nonlocal work
        kind = value["kind"]
        if kind == "name":
            return environment[value["name"]]
        if kind == "literal":
            width = abs(value["value"]).bit_length()
        elif kind == "binary":
            left, right = expression(value["left"]), expression(value["right"])
            width = left+right if value["op"] == "*" else max(left, right)+1
        elif kind == "unary":
            width = expression(value["value"])
        elif kind == "compare":
            expression(value["left"]); expression(value["right"])
            width = 1
        elif kind == "boolean":
            for child in value["values"]:
                expression(child)
            width = 1
        elif kind == "conditional":
            expression(value["condition"])
            width = max(expression(value["yes"]), expression(value["no"]))
        else:
            raise BatchError("unsupported integer-work expression")
        require(width <= MAX_INTERMEDIATE_BITS, "compiled batch intermediate integer growth budget exceeded")
        work += width+1
        require(work*len(inputs) <= MAX_INTEGER_BIT_WORK, "compiled batch integer-work budget exceeded")
        return width
    for statement in entry.branch["body"]:
        width = expression(statement["value"])
        if statement["kind"] == "assign":
            environment[statement["name"]] = width
    return work


def validate_inputs(entry, rows, *, fuel=100000, max_depth=128):
    """Return immutable exact rows; CPU integer width is distinct from GPU width.

    Fuel zero and depth zero are valid execution requests, producing the same
    budget/depth failures as the source runtime rather than an eligibility error.
    The aggregate work cap uses a conservative static source tick upper bound.
    """
    entry_binding(entry)
    require(type(fuel) is int and 0 <= fuel <= MAX_FUEL, "fuel must be an exact integer in 0..1000000")
    require(type(max_depth) is int and 0 <= max_depth <= MAX_DEPTH, "max_depth must be an exact integer in 0..1024")
    require(type(rows) in (tuple, list), "bounded list/tuple of input rows required")
    rows = tuple(rows)
    require(len(rows) <= MAX_ROWS, "bounded list/tuple of input rows required")
    require(len(rows)*min(fuel, entry.max_steps) <= MAX_TOTAL_WORK, "compiled batch total source-work budget exceeded")
    copied, size = [], 0
    for row in rows:
        require(type(row) in (tuple, list), "compiled batch input arity differs")
        row = tuple(row)
        require(len(row) == len(entry.param_types), "compiled batch input arity differs")
        for value, kind in zip(row, entry.param_types):
            require(type(value) is (int if kind == "int" else bool), "compiled batch input requires exact " + kind)
            if kind == "int":
                require(value.bit_length() <= MAX_INPUT_BITS, "compiled batch integer input budget exceeded")
                size += len(str(value))
            else:
                size += 5
            size += 16
        require(size <= MAX_INPUT_BYTES, "compiled batch input byte budget exceeded")
        copied.append(row)
    result = tuple(copied)
    _integer_work(entry, result)
    return result
