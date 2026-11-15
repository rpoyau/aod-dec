"""Bind compiled unit-pressure response to the unchanged Manual awareness fixture.

Main/Manual ab835920: sections/08_awareness_learning_mindfulness.tex:6-97;
manual/sections/00_awareness_fixtures.tex:39-108,128-173,225-230,315-372.
Retained operands are admitted on the host; compiled Dec response and selector
outputs drive later H1/fibre continuations. These do not create new closure
certificates. Cost-control policy and host performance are separate quantities.
"""
import builtins
from dataclasses import replace
import importlib.util
from pathlib import Path
import sys
import threading
from hashlib import sha256

from .vendor import aod_manual_native as native


ROOT = Path(__file__).resolve().parents[1]
MANUAL_COMMIT = "ab8359209b8f9b2ef4007403101016e3ea58cfe4"
AWARENESS_SHA = "6846300c020aca4af2c7fc51c0727dcd0ce7e302453e0a514904ce8aa5331d7b"
NATIVE_SHA = "918c6c0bd242e7ed0bff6f4d1a4ff7bf2cc840b3da41b6888c218d34eddf5be0"
_LOCK = threading.RLock()
_MANUAL = None


def require(condition, message):
    if not condition:
        raise ValueError(message)


def load_manual():
    """Load exact source with an isolated dependency import, without aliases.

    Ordinary fully qualified module registration supports its dataclasses.
    No process-wide replacement of the upstream dependency name is performed.
    The lock covers module registration/execution and every reuse hash check.
    """
    global _MANUAL
    with _LOCK:
        source = ROOT / "simulations/vendor/aod_awareness.py"
        require(sha256(source.read_bytes()).hexdigest() == AWARENESS_SHA, "bound awareness source changed")
        require(sha256(Path(native.__file__).read_bytes()).hexdigest() == NATIVE_SHA, "bound native producer changed")
        if _MANUAL is not None:
            require(_MANUAL.n is native, "awareness dependency binding changed")
            return _MANUAL
        name = "simulations.vendor.aod_awareness"
        require(name not in sys.modules, "awareness module was loaded outside its bound adapter")
        spec = importlib.util.spec_from_file_location(name, source)
        module = importlib.util.module_from_spec(spec)
        original_import = builtins.__import__
        def local_import(requested, globals=None, locals=None, fromlist=(), level=0):
            if requested == "aod_native_closure_wave" and level == 0:
                return native
            return original_import(requested, globals, locals, fromlist, level)
        module.__dict__["__builtins__"] = dict(vars(builtins), __import__=local_import)
        sys.modules[name] = module
        try:
            spec.loader.exec_module(module)
            require(module.n is native, "wrong awareness native dependency")
        except BaseException:
            sys.modules.pop(name, None)
            raise
        _MANUAL = module
        return module


def prepare():
    """Actual admitted Manual context; only retained relation membership differs."""
    manual = load_manual()
    state, world, model, inputs = manual.context(pressure=1)
    removed = replace(state, retained=())
    restored = replace(removed, retained=state.retained)
    lanes = {"retained": state, "removed": removed, "restored": restored}
    for current in lanes.values():
        manual.validate_state(current, world)
        require((current.present, current.closure_memory) == (state.present, state.closure_memory),
                "matched intervention changed non-retained coordinates")
    reference = {name: manual.execute(current, world, model, inputs)
                 for name, current in lanes.items()}
    return {"lanes": lanes, "world": world, "model": model, "inputs": inputs,
            "reference": reference, "intervention_cut": state.present.committed,
            "source_binding": {"manual_commit": MANUAL_COMMIT, "awareness_sha256": AWARENESS_SHA,
                "native_sha256": NATIVE_SHA, "pressure": 1, "gain": 1,
                "outcome": "later current, actual H1 edge and new closure count; excludes retention and labels"}}


def prepare_learning(preparation, trained):
    """Reuse Manual learning() after the four actually compiled training steps."""
    manual = load_manual()
    require(trained == preparation["reference"]["retained"]["final"],
            "learning boundary differs from the admitted four-step training")
    existing = manual.learning()
    removed = replace(trained, retained=())
    return {**preparation,
        "lanes": {"retained": trained, "removed": removed,
                  "restored": replace(removed, retained=trained.retained)},
        "inputs": existing["future"], "intervention_cut": trained.present.committed,
        "reference": {"retained": existing["pair"]["left"],
                      "removed": existing["pair"]["right"],
                      "restored": existing["pair"]["left"]},
        "manual_learning": existing}


def prepare_mindfulness(preparation):
    """Reuse Manual mindfulness(): initial current three, six later queries."""
    manual = load_manual()
    initial = preparation["lanes"]["retained"]
    initial = replace(initial, present=replace(initial.present, current=native.Q(3)))
    removed = replace(initial, retained=())
    inputs = tuple(manual.Input(initial.present.committed+i,
                   available=preparation["inputs"][0].available) for i in range(6))
    lanes = {"retained": initial, "removed": removed,
             "restored": replace(removed, retained=initial.retained)}
    return {**preparation, "lanes": lanes, "inputs": inputs,
        "reference": {name: manual.execute(state, preparation["world"], preparation["model"], inputs)
                      for name, state in lanes.items()},
        "manual_mindfulness": manual.mindfulness(),
        "manual_delayed": manual.mindfulness(extra_delay=True),
        "manual_pressure_controls": {
            "half": {"pressure": native.Q(1, 2), "execution": manual.mindfulness(pressure=native.Q(1, 2))},
            "two": {"pressure": native.Q(2), "execution": manual.mindfulness(pressure=native.Q(2))}}}


def response_inputs(state, world, model, inputs):
    """Project admitted operands into the declared integer specialization."""
    manual = load_manual()
    manual.validate_state(state, world)
    require(state.present.pressure == 1 and model.gain == 1 and model.selector_threshold == 0,
            "this compiled example requires the declared p=g=1 response and zero selector threshold")
    require(type(inputs.committed) is int and inputs.committed == state.present.committed,
            "future/stale input differs from matched native cut")
    require(inputs.query_observed_at is None or (type(inputs.query_observed_at) is int and
            0 <= inputs.query_observed_at <= inputs.committed), "query lacks committed observation")
    require(not inputs.query or inputs.query_observed_at is not None, "query requires prior observation")
    require(len(set(inputs.available)) == len(inputs.available) and
            set(inputs.available) <= {wave.identity for wave in world.waves}, "invalid available source family")
    retained = sum((r.value for r in state.retained if r.prior.source in inputs.available), native.Q(0))
    values = tuple(native.Q(value) for value in (state.present.current, inputs.attention, inputs.query, retained))
    require(all(value.denominator == 1 for value in values), "integer compiled example cannot truncate rational operands")
    return tuple(int(value) for value in values)


def apply_compiled(state, world, model, inputs, current, axis):
    """Consume actual compiled values while retaining canonical native support."""
    manual = load_manual()
    operands = response_inputs(state, world, model, inputs)
    require(type(current) is int and type(axis) is int and axis in (1, 2), "compiled result type/domain differs")
    require(current == sum(operands), "compiled response differs from the declared unit-pressure law")
    require(axis == (1 if current > 0 else 2), "compiled selector differs from the declared sign law")
    x = state.present
    vertex, role = x.present
    candidates = [(vertex, vertex ^ 1, role, 0 if role else 1),
                  (vertex, vertex ^ 2, role, 0 if role else -1)]
    support = native.kernel(candidates, {edge: native.Q(1) for edge in candidates})
    edge = vertex, vertex ^ axis
    require(native.hamming(*edge) == 1, "compiled continuation left admitted H1 support")
    next_role = candidates[axis - 1][3]
    native.fibre_step(role, next_role, "enter_hinge" if role else "leave_hinge")
    # Native update binds the existing causal index/state interface. Its scalar
    # current is replaced with the value actually returned by generated Dec.
    updated = native.update(replace(x, closure_memory=state.closure_memory), inputs.attention, inputs.committed)
    updated = replace(updated, present=(edge[1], next_role), current=native.Q(current), closure_memory=(),
                      retained_rebalancing=(), closure_status="pending")
    following = manual.State(updated, state.closure_memory, state.retained)
    terms = tuple((r.prior.source, native.Q(r.value)) for r in state.retained if r.prior.source in inputs.available)
    event = {"at": updated.committed, "current": native.Q(current), "edge": edge,
        "attention": native.Q(inputs.attention), "query": native.Q(inputs.query),
        "query_observed_at": inputs.query_observed_at, "retained_terms": terms, "support": support,
        "new_phase": 0, "native_update_at": updated.committed}
    manual.validate_state(following, world)
    return following, event
