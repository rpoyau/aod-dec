"""Passive call/return tracing of the existing, unchanged Manual evolution.

Main08:6-97,154; Manual00:219-230,315-372; executable aod_awareness.py
transition:219-249, execute:257-262, learning:354-362, mindfulness:451-466.
The source functions remain the engine. This observer neither supplies operands
nor implements their response. Replay is an explicit later verification cost.
"""
from dataclasses import dataclass, fields, is_dataclass, replace
from fractions import Fraction
from hashlib import sha256
import json
from pathlib import Path
import sys
import time
from types import MappingProxyType

from . import awareness_jit_fixture as fixture
from .trace_history_union import RetainedHistoryUnion

SCHEMA = "aod-existing-awareness-trace/v1"
CASES = MappingProxyType({"learning": ("learning", ()), "mindfulness": ("mindfulness", ()),
                         "delayed": ("mindfulness", (("extra_delay", True),))})


def require(condition, message):
    if not condition:
        raise ValueError(message)


def _json(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False)


def _registry(manual):
    return {prefix+"."+name: value for prefix, module in (("manual", manual), ("native", fixture.native))
            for name, value in vars(module).items() if isinstance(value, type) and is_dataclass(value)
            and value.__module__ == module.__name__}


def _pack(value, names, depth=0):
    require(depth < 100, "witness nesting budget exceeded")
    kind = type(value)
    if value is None or kind in (str, int, bool):
        return value
    if kind is Fraction:
        return {"q": [value.numerator, value.denominator]}
    if kind in names:
        return {"type": names[kind], "fields": {f.name: _pack(getattr(value, f.name), names, depth+1) for f in fields(value)}}
    if kind in (tuple, list):
        return {"tuple" if kind is tuple else "list": [_pack(v, names, depth+1) for v in value]}
    if kind is dict:
        return {"dict": [[_pack(k, names, depth+1), _pack(v, names, depth+1)] for k, v in value.items()]}
    raise ValueError("unsupported exact witness type: "+kind.__name__)


def _unpack(value, registry, depth=0):
    require(depth < 100, "witness nesting budget exceeded")
    if value is None or type(value) in (str, int, bool):
        return value
    require(type(value) is dict, "typed witness object required")
    if set(value) == {"q"}:
        pair = value["q"]
        require(type(pair) is list and len(pair) == 2 and all(type(v) is int for v in pair) and pair[1] > 0,
                "exact rational witness required")
        result = Fraction(*pair)
        require([result.numerator, result.denominator] == pair, "canonical rational witness required")
        return result
    if set(value) in ({"tuple"}, {"list"}):
        key = next(iter(value)); require(type(value[key]) is list, "sequence witness required")
        values = [_unpack(v, registry, depth+1) for v in value[key]]
        return tuple(values) if key == "tuple" else values
    if set(value) == {"dict"}:
        require(type(value["dict"]) is list, "mapping witness required")
        result = {}
        for pair in value["dict"]:
            require(type(pair) is list and len(pair) == 2, "mapping pair required")
            key, item = (_unpack(v, registry, depth+1) for v in pair)
            require(key not in result, "duplicate witness mapping key")
            result[key] = item
        return result
    require(set(value) == {"type", "fields"} and type(value["type"]) is str and value["type"] in registry,
            "unknown witness type")
    cls = registry[value["type"]]
    require(type(value["fields"]) is dict and set(value["fields"]) == {f.name for f in fields(cls)},
            "witness fields differ")
    return cls(**{k: _unpack(v, registry, depth+1) for k, v in value["fields"].items()})


@dataclass(frozen=True)
class EngineTrace:
    schema: str
    case: str
    events: tuple[str, ...]
    witnesses: tuple[tuple[str, str], ...]
    source_binding: str
    complete: bool
    reasons: tuple[str, ...]
    retained_bytes: int
    max_events: int
    max_bytes: int


class _Collector:
    def __init__(self, manual, case, source_binding, max_events, max_bytes):
        self.manual, self.case = manual, case
        self.names = {v: k for k, v in _registry(manual).items()}
        self.codes = {getattr(manual, name).__code__: name for name in
            ("context", "core", "matched", "execute", "transition", "retain", "learning", "mindfulness")}
        self.codes[fixture.native.update.__code__] = "native.update"
        self.source_binding = source_binding
        self.max_events, self.max_bytes = max_events, max_bytes
        self.events, self.witnesses, self.identities, self.active = [], {}, {}, {}
        self.next_id, self.used, self.reasons = 1, 0, []
        self.stopped = False

    def fail(self, reason):
        self.stopped = True
        if len(self.reasons) < 8:
            self.reasons.append(str(reason)[:300])
        self.active.clear()
        self.identities.clear()

    def witness(self, value):
        old = self.identities.get(id(value))
        if old is not None and old[0] is value:
            return old[1]
        raw = _json(_pack(value, self.names))
        digest = sha256(raw.encode()).hexdigest()
        if digest not in self.witnesses:
            require(self.used+len(raw.encode())+64 <= self.max_bytes, "trace byte budget exhausted")
            self.witnesses[digest] = raw
            self.used += len(raw.encode())+64
        self.identities[id(value)] = (value, digest)
        return digest

    def emit(self, record):
        require(len(self.events) < self.max_events, "trace event budget exhausted")
        raw = _json(record); size = len(raw.encode())
        require(self.used+size <= self.max_bytes, "trace byte budget exhausted")
        self.events.append(raw); self.used += size

    def observe(self, frame, event, value):
        if self.stopped or event not in ("call", "return"):
            return
        name = self.codes.get(frame.f_code)
        if name is None:
            return
        if event == "call":
            parent = frame.f_back
            while parent is not None and id(parent) not in self.active:
                parent = parent.f_back
            parent_id = self.active.get(id(parent)) if parent is not None else None
            identity = self.next_id; self.next_id += 1
            record = {"kind": "call", "id": identity, "parent": parent_id, "function": name}
            local = frame.f_locals
            if name == "transition":
                record["arguments"] = {key: self.witness(local[key]) for key in ("s", "world", "model", "inp")}
                record["native_cut"] = local["s"].present.committed
            elif name == "native.update":
                record["arguments"] = {key: self.witness(local[key]) for key in ("s", "attention", "input_index")}
                record["native_cut"] = local["s"].committed
            elif name == "retain":
                record["arguments"] = {"prior": self.witness(local["prior"])}
            self.emit(record); self.active[id(frame)] = identity
        else:
            identity = self.active.pop(id(frame), None)
            require(identity is not None, "return lacks captured call")
            record = {"kind": "return", "id": identity, "function": name,
                      "status": "returned" if value is not None else "none-or-unwound"}
            if name in ("transition", "native.update", "retain") and value is not None:
                record["result"] = self.witness(value)
            self.emit(record)

    def freeze(self):
        if self.active:
            self.fail("unfinished observed calls")
        return EngineTrace(SCHEMA, self.case, tuple(self.events), tuple(self.witnesses.items()),
            _json(self.source_binding), not self.reasons, tuple(self.reasons), self.used, self.max_events, self.max_bytes)


def _sources(manual):
    paths = (Path(manual.__file__), Path(fixture.native.__file__), Path(__file__), Path(fixture.__file__),
             Path(__file__).with_name("trace_history_union.py"))
    return {str(p.relative_to(fixture.ROOT)): sha256(p.read_bytes()).hexdigest() for p in paths}


def run_engine_case(case="learning", *, trace=True, max_events=4096, max_bytes=8*1024*1024, capture_errors=False):
    """Run the existing source function; observation cannot replace its outcome.

    Immutable records may be persisted by the caller. Limits bound retained
    observer payload, not engine RAM or transient serialization. A prior profiler
    is chained and restored; observer failures only make trace coverage partial.
    Engine errors propagate unchanged by default. Explicit capture_errors=True
    returns their type/message with partial trace coverage for archival callers.
    """
    require(type(case) is str and case in CASES, "unknown existing engine case")
    require(type(trace) is bool, "exact trace switch required")
    require(type(capture_errors) is bool, "exact error-capture switch required")
    require(type(max_events) is int and 1 <= max_events <= 100000, "event bound must be in1..100000")
    require(type(max_bytes) is int and 1 <= max_bytes <= 64*1024*1024, "byte bound must be in1..64MiB")
    started = time.perf_counter_ns()
    manual = fixture.load_manual(); source_binding = _sources(manual)
    name, fixed_kwargs = CASES[case]
    kwargs = dict(fixed_kwargs)
    collector = _Collector(manual, case, source_binding, max_events, max_bytes) if trace else None
    previous = sys.getprofile()
    previous_active = previous is not None
    def observe(frame, event, value):
        nonlocal previous_active
        if previous_active:
            try:
                previous(frame, event, value)
            except Exception as exc:
                previous_active = False
                collector.fail("prior observer failed: "+type(exc).__name__+": "+str(exc))
        if not collector.stopped:
            try:
                collector.observe(frame, event, value)
            except Exception as exc:
                collector.fail("observer failed: "+type(exc).__name__+": "+str(exc))
    result, error, original_error = None, None, None
    before_call = time.perf_counter_ns()
    installed = False
    try:
        if trace:
            if previous is not None and not callable(previous):
                collector.fail("existing native profiler retained; passive hook unavailable")
            else:
                try:
                    sys.setprofile(observe)
                    installed = True
                except Exception as exc:
                    collector.fail("observer installation failed: "+str(exc))
        try:
            result = getattr(manual, name)(**kwargs)
        except Exception as exc:
            original_error = exc
            error = {"type": type(exc).__name__, "message": str(exc)}
    finally:
        if installed:
            try:
                sys.setprofile(previous)
            except Exception as exc:
                collector.fail("observer restoration failed: "+str(exc))
    execution_ns = time.perf_counter_ns()-before_call
    if error is not None and collector is not None:
        collector.fail("engine error ended nominal continuation")
    try:
        if _sources(manual) != source_binding and collector is not None:
            collector.fail("source changed during observed engine execution")
    except Exception as exc:
        if collector is not None:
            collector.fail("source verification failed: "+str(exc))
    captured = collector.freeze() if collector is not None else None
    if original_error is not None and not capture_errors:
        try:
            original_error.engine_trace = captured
        except Exception:
            pass
        raise original_error
    return {"case": case, "result": result, "error": error, "trace": captured,
            "source_binding": source_binding, "engine_execution_ns": execution_ns,
            "total_ns": time.perf_counter_ns()-started,
            "execution": "unchanged Manual source function", "passive": trace}


def trace_to_dict(trace):
    require(type(trace) is EngineTrace, "typed engine trace required")
    return {f.name: getattr(trace, f.name) for f in fields(trace)}


def trace_from_dict(value):
    """Reopen the exact bounded JSON record format without dynamic imports."""
    require(type(value) is dict and set(value) == {f.name for f in fields(EngineTrace)},
            "engine trace fields differ")
    require(type(value["max_events"]) is int and 1 <= value["max_events"] <= 100000 and
            type(value["max_bytes"]) is int and 1 <= value["max_bytes"] <= 64*1024*1024 and
            type(value["retained_bytes"]) is int and 0 <= value["retained_bytes"] <= value["max_bytes"],
            "trace bounds differ")
    require(type(value["schema"]) is str and type(value["case"]) is str and
            type(value["source_binding"]) is str and len(value["source_binding"]) <= 16384 and
            type(value["complete"]) is bool, "trace metadata types differ")
    for name in ("events", "witnesses", "reasons"):
        require(type(value[name]) in (tuple, list), "trace sequence required")
    require(len(value["events"]) <= value["max_events"] and
            len(value["witnesses"]) <= value["max_events"]*8 and len(value["reasons"]) <= 8,
            "trace collection bounds differ")
    require(all(type(v) is str for v in value["events"]) and
            all(type(v) is str and len(v) <= 300 for v in value["reasons"]), "trace text required")
    require(all(type(pair) in (tuple, list) and len(pair) == 2 and
                all(type(v) is str for v in pair) for pair in value["witnesses"]), "witness pair required")
    result = EngineTrace(**{**value, "events": tuple(value["events"]), "reasons": tuple(value["reasons"]),
                            "witnesses": tuple(tuple(pair) for pair in value["witnesses"])})
    used = sum(len(v.encode()) for v in result.events)+sum(64+len(v.encode()) for _, v in result.witnesses)
    require(used == result.retained_bytes, "trace byte accounting differs")
    return result


def verify_trace(trace):
    """Replay captured native transitions; partial coverage never verifies full execution."""
    require(type(trace) is EngineTrace and trace.schema == SCHEMA and trace.case in CASES,
            "typed supported engine trace required")
    trace_from_dict(trace_to_dict(trace))
    require(type(trace.events) is tuple and len(trace.events) <= trace.max_events and
            type(trace.witnesses) is tuple, "trace collection bound differs")
    manual = fixture.load_manual(); registry = _registry(manual)
    names = {value: name for name, value in registry.items()}
    exact_cache = {}
    def typed_bytes(value):
        saved = exact_cache.get(id(value))
        if saved is None or saved[0] is not value:
            saved = value, _json(_pack(value, names))
            exact_cache[id(value)] = saved
        return saved[1]
    def exact(left, right):
        # Dataclass/Python equality aliases False with0 and True with1.
        return typed_bytes(left) == typed_bytes(right)
    require(json.loads(trace.source_binding) == _sources(manual), "engine trace source binding differs")
    used = sum(len(v.encode()) for v in trace.events)+sum(64+len(v.encode()) for _, v in trace.witnesses)
    require(used == trace.retained_bytes and used <= trace.max_bytes, "trace byte accounting differs")
    decoded = {}; history = RetainedHistoryUnion(max_bindings=200000, max_nodes=500000)
    for digest, raw in trace.witnesses:
        require(type(digest) is str and type(raw) is str and sha256(raw.encode()).hexdigest() == digest,
                "witness digest mismatch")
        require(digest not in decoded, "duplicate witness digest")
        packed = json.loads(raw)
        require(_json(packed) == raw, "noncanonical witness JSON")
        value = _unpack(packed, registry)
        decoded[digest] = value
        if type(value) is manual.World:
            manual.validate_world(value)
            for wave in value.waves:
                history.include(wave)
        elif type(value) is manual.State:
            for relation in value.retained:
                for contact in relation.prior.contacts:
                    history.include(contact.support_trace, contact.support_certificate)
    def witness(key):
        require(type(key) is str and key in decoded, "missing witness")
        return decoded[key]
    canonical_initial = canonical_world = canonical_model = None
    branch_plan = ()
    if trace.complete:
        canonical_initial, canonical_world, canonical_model, initial_inputs = manual.context()
        if trace.case == "learning":
            learned = manual.execute(canonical_initial, canonical_world, canonical_model, initial_inputs)["final"]
            future = tuple(manual.Input(learned.present.committed+i, available=initial_inputs[0].available) for i in range(3))
            absent = replace(canonical_initial, retained=())
            branch_plan = tuple((state, initial_inputs) for state in
                (canonical_initial, absent, canonical_initial, canonical_initial, canonical_initial))
            branch_plan += tuple((state, future) for state in (learned, replace(learned, retained=()), learned))
        else:
            canonical_initial = replace(canonical_initial,
                present=replace(canonical_initial.present, current=Fraction(3)))
    active, calls, outcomes, branch_last, observations, child_updates = [], {}, {}, {}, {}, {}
    execute_branches, branch_steps, child_calls = {}, {}, {}
    transitions = updates = retentions = 0
    for raw in trace.events:
        row = json.loads(raw)
        require(type(row) is dict and _json(row) == raw and type(row.get("id")) is int and
                row["id"] > 0 and type(row.get("function")) is str, "invalid canonical call record")
        if row.get("kind") == "call":
            keys = {"kind", "id", "parent", "function"}
            if row["function"] in ("transition", "native.update"):
                keys |= {"arguments", "native_cut"}
            elif row["function"] == "retain":
                keys.add("arguments")
            require(set(row) == keys and (row["parent"] is None or type(row["parent"]) is int),
                    "source call fields/types differ")
            if "native_cut" in row:
                require(type(row["native_cut"]) is int, "exact native cut required")
            if "arguments" in row:
                expected_arguments = {"transition": {"s", "world", "model", "inp"},
                    "native.update": {"s", "attention", "input_index"}, "retain": {"prior"}}[row["function"]]
                require(type(row["arguments"]) is dict and set(row["arguments"]) == expected_arguments,
                        "source argument fields differ")
        else:
            keys = {"kind", "id", "function", "status"}
            if row.get("status") == "returned" and row["function"] in ("transition", "native.update", "retain"):
                keys.add("result")
            require(set(row) == keys, "source return fields differ")
        identity = row["id"]
        if row.get("kind") == "call":
            require(identity not in calls and row.get("parent") == (active[-1] if active else None), "call-tree identity/order differs")
            require(row.get("function") in ("context", "core", "matched", "execute", "transition", "retain",
                    "learning", "mindfulness", "native.update"), "unknown source function")
            calls[identity] = row; active.append(identity)
            child_calls.setdefault(row["parent"], []).append(identity)
            if row["function"] == "execute":
                execute_branches[identity] = len(execute_branches)
            if row["function"] == "transition":
                args = row["arguments"]; s, world, model, inp = (witness(args[k]) for k in ("s", "world", "model", "inp"))
                require(type(s) is manual.State and type(world) is manual.World and type(model) is manual.Model and type(inp) is manual.Input,
                        "transition operand types differ")
                require(row.get("native_cut") == s.present.committed, "transition cut snapshot differs")
                parent = row["parent"]
                step = branch_steps.get(parent, 0)
                if trace.complete:
                    require(exact(world, canonical_world) and exact(model, canonical_model),
                            "source-owned world/model differs")
                    if trace.case == "learning":
                        branch = execute_branches.get(parent)
                        require(branch is not None and branch < len(branch_plan), "source-owned learning branch differs")
                        expected_initial, expected_inputs = branch_plan[branch]
                        require(step < len(expected_inputs) and exact(inp, expected_inputs[step]),
                                "source-owned learning input differs")
                    else:
                        expected_initial = canonical_initial
                        require(calls[parent]["function"] == "mindfulness" and step < 6 and
                                exact(inp.attention, Fraction(0)) and exact(inp.available, initial_inputs[0].available),
                                "source-owned query input differs")
                    if step == 0:
                        require(exact(s, expected_initial), "source-owned initial state differs")
                branch_steps[parent] = step+1
                if parent in branch_last:
                    require(exact(s, branch_last[parent]), "continuation branch skipped or rewrote state")
                if parent is not None and calls[parent]["function"] == "mindfulness":
                    observed = observations.setdefault(parent, {})
                    observed[s.present.committed] = s.present.current
                    cut = inp.query_observed_at
                    if trace.case == "delayed" and not branch_last.get(parent):
                        observed[s.present.committed-1] = Fraction(0)
                    require(type(cut) is int and cut in observed and exact(inp.query, -observed[cut]),
                            "query detached from committed observation")
                    expected_cut = s.present.committed-(trace.case == "delayed")
                    require(cut == expected_cut, "query delay/source cut differs")
                outcomes[identity] = manual.transition(s, world, model, inp)
            elif row["function"] == "native.update":
                args = row["arguments"]; s, attention, input_index = (witness(args[k]) for k in ("s", "attention", "input_index"))
                require(type(s) is fixture.native.WaveState and row.get("native_cut") == s.committed,
                        "native update cut/type differs")
                parent = calls.get(row["parent"])
                require(parent is not None and parent["function"] == "transition", "native update lacks owning transition")
                original = witness(parent["arguments"]["s"])
                inp = witness(parent["arguments"]["inp"])
                require(exact(s, replace(original.present, closure_memory=original.closure_memory)) and
                        exact(attention, fixture.native.Q(inp.attention)) and exact(input_index, inp.committed),
                        "native update detached from owning transition")
                outcomes[identity] = fixture.native.update(s, attention, input_index)
            elif row["function"] == "retain":
                prior = witness(row["arguments"]["prior"])
                if trace.complete:
                    require(exact(prior, canonical_initial.retained[0].prior),
                            "source-owned retention operands differ")
                outcomes[identity] = manual.retain(prior)
        else:
            require(row.get("kind") == "return" and active and active.pop() == identity and
                    row.get("function") == calls[identity]["function"], "return/call tree differs")
            require(row.get("status") in ("returned", "none-or-unwound"), "unknown source return status")
            if trace.complete:
                require(row["status"] == "returned", "complete source call did not return normally")
            if identity in outcomes:
                if not trace.complete and row.get("status") == "none-or-unwound":
                    continue
                require(row.get("status") == "returned" and exact(witness(row.get("result")), outcomes[identity]),
                        "captured source result differs from native replay")
                kind = row["function"]
                if kind == "transition":
                    require(child_updates.get(identity, 0) == 1, "transition lacks its actual native update")
                    branch_last[calls[identity]["parent"]] = outcomes[identity][0]; transitions += 1
                elif kind == "native.update":
                    updates += 1
                    parent = calls[identity]["parent"]
                    child_updates[parent] = child_updates.get(parent, 0)+1
                else:
                    retentions += 1
    if trace.complete:
        require(not active and not trace.reasons, "complete trace has unfinished coverage")
        roots = [r for r in calls.values() if r["parent"] is None]
        require(len(roots) == 1 and roots[0]["function"] == CASES[trace.case][0], "wrong source-owned root execution")
        expected = 29 if trace.case == "learning" else 6
        require(transitions == updates == expected and retentions == 1,
                "complete existing-case call coverage differs")
        if trace.case == "learning":
            require(len(execute_branches) == len(branch_plan) and all(
                branch_steps.get(identity) == len(branch_plan[index][1])
                for identity, index in execute_branches.items()), "source-owned learning horizon differs")
        fixed_children = {"learning": ["core", "execute", "matched", "execute"],
            "core": ["context", "matched", "execute", "execute"], "context": ["retain"],
            "matched": ["execute", "execute"], "mindfulness": ["context"]+["transition"]*6,
            "transition": ["native.update"], "native.update": [], "retain": []}
        for identity, call in calls.items():
            if call["function"] == "execute":
                expected_children = ["transition"]*len(branch_plan[execute_branches[identity]][1])
            else:
                expected_children = fixed_children[call["function"]]
            actual_children = [calls[child]["function"] for child in child_calls.get(identity, ())]
            require(actual_children == expected_children, "source-owned child-call coverage differs")
    return {"status": "VERIFIED" if trace.complete else "PARTIAL", "transitions": transitions,
            "native_updates": updates, "retentions": retentions, "calls": len(calls),
            "scope": "captured existing-engine calls, native transitions, retention and committed-query causality"}
