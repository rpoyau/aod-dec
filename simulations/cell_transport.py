"""Exact conservative transport on a declared eight-facet tesseract shell.

This application model uses an upper-state reservoir, cell storage, an output
port and absorption. It omits stimulated gain, coherence and a physical clock.
Jitter changes declared exchange weights while cell incidence stays fixed.
Forward Euler resolution error is separate from that model perturbation.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass, is_dataclass
from fractions import Fraction
import hashlib
import json
import random

from .tesseract_cells import FACETS, TESSERACT, boundary, boundary_chain, facet_interfaces


F = Fraction
INTERFACES = facet_interfaces()
CELL_INDEX = {cell: index for index, cell in enumerate(FACETS)}
STATE_NAMES = ("emitter", *FACETS, "output", "loss", "injected")


def exact(value):
    if isinstance(value, bool) or not isinstance(value, (int, Fraction)):
        raise TypeError("model inputs must be exact integers or Fraction values")
    return Fraction(value)


def exact_data(value):
    """Return JSON-ready exact data without silently rounding fractions."""
    if isinstance(value, Fraction):
        return str(value)
    if is_dataclass(value):
        return exact_data(asdict(value))
    if isinstance(value, dict):
        return {str(key): exact_data(item) for key, item in value.items()}
    if isinstance(value, (tuple, list)):
        return [exact_data(item) for item in value]
    return value


@dataclass(frozen=True)
class ModelConfig:
    intervals: int = 4
    exchange: Fraction = F(1, 16)
    leak: Fraction = F(1, 4)
    absorption: Fraction = F(1, 32)
    pump: Fraction = F(1, 4)
    gamma: Fraction = F(1, 4)
    initial_emitter: Fraction = F(1)
    initial_cells: tuple[Fraction, ...] = (F(0),) * 8
    source: str = "0***"
    output: str = "1***"

    def __post_init__(self):
        if type(self.intervals) is not int or not 1 <= self.intervals <= 64:
            raise ValueError("intervals must be an integer from 1 through 64")
        for name in ("exchange", "leak", "absorption", "pump", "gamma", "initial_emitter"):
            value = exact(getattr(self, name))
            if value < 0:
                raise ValueError(f"{name} must be nonnegative")
            object.__setattr__(self, name, value)
        if len(self.initial_cells) != 8:
            raise ValueError("initial_cells must contain exactly eight cell values")
        initial = tuple(exact(value) for value in self.initial_cells)
        if any(value < 0 for value in initial):
            raise ValueError("initial cell values must be nonnegative")
        object.__setattr__(self, "initial_cells", initial)
        if self.source not in FACETS or self.output not in FACETS:
            raise ValueError("source and output must name cubic boundary facets")


def _schedule_hash(schedule):
    payload = {key: schedule[key] for key in ("version", "seed", "amplitude", "intervals", "multipliers")}
    return hashlib.sha256(json.dumps(exact_data(payload), sort_keys=True,
                                    separators=(",", ":")).encode()).hexdigest()


def make_schedule(intervals, amplitude=F(0), seed=1729):
    """Use one fixed multiplier per macro-interval/interface at every refinement.

    Each multiplier is 1 + amplitude*z, with z in {-1,-3/4,...,1} from a local
    seeded PRNG. This bounded distribution is a declared application choice.
    Stored multipliers, rather than PRNG replay alone, identify the realized path.
    """
    if type(intervals) is not int or not 1 <= intervals <= 64:
        raise ValueError("intervals must be an integer from 1 through 64")
    if type(seed) is not int:
        raise TypeError("seed must be an integer")
    amplitude = exact(amplitude)
    if not 0 <= amplitude <= 1:
        raise ValueError("jitter amplitude must lie between zero and one")
    rng = random.Random(seed)
    multipliers = tuple(tuple(1 + amplitude * F(rng.randint(-4, 4), 4)
                              for _ in INTERFACES) for _ in range(intervals))
    result = {"version": 1, "seed": seed, "amplitude": amplitude,
              "intervals": intervals, "multipliers": multipliers}
    result["sha256"] = _schedule_hash(result)
    return result


def validate_schedule(config, schedule):
    if not isinstance(schedule, dict) or set(schedule) != {
        "version", "seed", "amplitude", "intervals", "multipliers", "sha256"
    }:
        raise ValueError("invalid jitter schedule fields")
    if schedule["version"] != 1 or type(schedule["version"]) is not int:
        raise ValueError("unsupported jitter schedule version")
    if type(schedule["seed"]) is not int:
        raise TypeError("schedule seed must be an integer")
    amplitude = exact(schedule["amplitude"])
    if not 0 <= amplitude <= 1:
        raise ValueError("invalid jitter amplitude")
    if type(schedule["intervals"]) is not int or schedule["intervals"] != config.intervals:
        raise ValueError("schedule interval count differs from model")
    rows = schedule["multipliers"]
    if len(rows) != config.intervals or any(len(row) != len(INTERFACES) for row in rows):
        raise ValueError("schedule shape differs from interval/interface domain")
    for row in rows:
        for value in row:
            multiplier = exact(value)
            if not 1 - amplitude <= multiplier <= 1 + amplitude:
                raise ValueError("jitter multiplier exceeds its declared bound")
    if schedule["sha256"] != _schedule_hash(schedule):
        raise ValueError("jitter schedule checksum mismatch")


def interval_rates(config, schedule, interval):
    validate_schedule(config, schedule)
    if type(interval) is not int or not 0 <= interval < config.intervals:
        raise ValueError("interval outside schedule")
    return tuple(config.exchange * exact(value) for value in schedule["multipliers"][interval])


def transition_kernel(config, multipliers, h):
    """Return a row-stochastic kernel on emitter, eight cells, output and loss.

    The external pump is an additive input after this kernel acts. Output and
    loss are absorbing states. The unnormalized energy-like state is not itself
    an AFC probability memory; normalizing a positive total is a separate step.
    """
    h = exact(h)
    if h <= 0:
        raise ValueError("step size must be positive")
    if len(multipliers) != len(INTERFACES):
        raise ValueError("one exchange multiplier is required for each interface")
    matrix = [[F(0) for _ in range(11)] for _ in range(11)]
    matrix[0][1 + CELL_INDEX[config.source]] = h * config.gamma
    for interface, multiplier in zip(INTERFACES, multipliers):
        rate = config.exchange * exact(multiplier)
        if rate < 0:
            raise ValueError("exchange rates must be nonnegative")
        left, right = 1 + CELL_INDEX[interface.left], 1 + CELL_INDEX[interface.right]
        matrix[left][right] = matrix[right][left] = h * rate
    for index, cell in enumerate(FACETS, 1):
        matrix[index][10] = h * config.absorption
        if cell == config.output:
            matrix[index][9] = h * config.leak
    for index, row in enumerate(matrix):
        departure = sum(row)
        if departure > 1:
            raise ValueError("Euler step violates nonnegative-kernel stability bound")
        row[index] = 1 - departure
        if any(value < 0 for value in row) or sum(row) != 1:
            raise ArithmeticError("non-stochastic transition kernel")
    return tuple(tuple(row) for row in matrix)


def region_audit(step, region):
    """Check a cell-region balance using its actual oriented cubical boundary."""
    selected = frozenset(region)
    if not selected <= set(FACETS):
        raise ValueError("region contains a non-facet cell")
    orientation = boundary(TESSERACT)
    chain = {cell: orientation[cell] for cell in selected}
    faces = boundary_chain(chain)
    face_flux = {entry["face"]: entry["oriented_face_flux"] for entry in step["interfaces"]}
    crossing = sum((coefficient * face_flux[face] for face, coefficient in faces.items()), F(0))
    direct = F(0)
    for entry in step["interfaces"]:
        if entry["left"] in selected and entry["right"] not in selected:
            direct += entry["net_left_to_right"]
        if entry["right"] in selected and entry["left"] not in selected:
            direct -= entry["net_left_to_right"]
    if crossing != direct:
        raise ArithmeticError("cubical boundary and retained pair flux disagree")
    indices = [CELL_INDEX[cell] for cell in selected]
    change = sum((step["cells_after"][i] - step["cells_before"][i] for i in indices), F(0))
    output = sum((step["output_by_cell"][i] for i in indices), F(0))
    loss = sum((step["loss_by_cell"][i] for i in indices), F(0))
    incoming = step["emission"] if step["source"] in selected else F(0)
    residual = change - incoming + crossing + output + loss
    if residual:
        raise ArithmeticError("cell-region conservation failed")
    return {"region": tuple(cell for cell in FACETS if cell in selected),
            "boundary_chain": faces, "interface_boundary_flux": crossing,
            "cell_mass_change": change, "emitter_input": incoming,
            "output": output, "loss": loss, "residual": residual}


def flatten_state(result):
    final = result["final"] if "final" in result else result
    return (final["emitter"], *final["cells"], final["output"], final["loss"], final["injected"])


def simulate(config=ModelConfig(), substeps=1, schedule=None, *, retain_steps=True):
    """Execute exact Euler steps; resolution is a dimensionless model parameter."""
    if not isinstance(config, ModelConfig):
        raise TypeError("config must be ModelConfig")
    if type(substeps) is not int or not 1 <= substeps <= 1024:
        raise ValueError("substeps must be an integer from 1 through 1024")
    if type(retain_steps) is not bool:
        raise TypeError("retain_steps must be Boolean")
    schedule = make_schedule(config.intervals) if schedule is None else schedule
    validate_schedule(config, schedule)
    h = F(1, substeps)
    emitter, current = config.initial_emitter, config.initial_cells
    output = loss = injected = F(0)
    initial_total = emitter + sum(current)
    steps, snapshots = [], []
    audit_count = 0
    for macro in range(config.intervals):
        rates = interval_rates(config, schedule, macro)
        kernel = transition_kernel(config, schedule["multipliers"][macro], h)
        for local in range(substeps):
            previous = current
            emitter_before = emitter
            emission = h * config.gamma * emitter_before
            pump = h * config.pump
            emitter = emitter_before + pump - emission
            updated = list(previous)
            updated[CELL_INDEX[config.source]] += emission
            pairs = []
            for interface, rate in zip(INTERFACES, rates):
                left, right = CELL_INDEX[interface.left], CELL_INDEX[interface.right]
                forward, backward = h * rate * previous[left], h * rate * previous[right]
                net = forward - backward
                updated[left] -= net
                updated[right] += net
                pairs.append({"face": interface.face, "left": interface.left, "right": interface.right,
                              "rate": rate, "forward": forward, "backward": backward,
                              "net_left_to_right": net,
                              "oriented_face_flux": interface.left_sign * net})
            losses = tuple(h * config.absorption * value for value in previous)
            outputs = tuple(h * config.leak * value if cell == config.output else F(0)
                            for cell, value in zip(FACETS, previous))
            for index in range(8):
                updated[index] -= losses[index] + outputs[index]
            current = tuple(updated)
            old_state = (emitter_before, *previous, output, loss)
            output += sum(outputs)
            loss += sum(losses)
            injected += pump
            new_state = (emitter, *current, output, loss)
            # A second assembly verifies the row-stochastic interpretation.
            predicted = tuple(sum((old_state[i] * kernel[i][j] for i in range(11)), F(0))
                              + (pump if j == 0 else F(0)) for j in range(11))
            if predicted != new_state or any(value < 0 for value in new_state):
                raise ArithmeticError("positive transport update differs from its kernel")
            residual = emitter + sum(current) + output + loss - initial_total - injected
            if residual:
                raise ArithmeticError("global energy-accounting balance failed")
            step = {"ordinal": macro * substeps + local + 1, "macro_interval": macro,
                    "substep": local, "h": h, "source": config.source,
                    "emitter_before": emitter_before, "emitter_after": emitter,
                    "cells_before": previous, "cells_after": current,
                    "pump": pump, "emission": emission, "interfaces": tuple(pairs),
                    "output_by_cell": outputs, "loss_by_cell": losses,
                    "output_total": output, "loss_total": loss,
                    "injected_total": injected, "balance_residual": residual}
            # A nontrivial four-cell region is checked every step. Exhaustive
            # 256-region checks are exposed via region_audit and run in tests.
            step["region_audit"] = region_audit(step, FACETS[:4])
            audit_count += 1
            if retain_steps:
                steps.append(step)
        snapshots.append({"model_parameter": F(macro + 1), "emitter": emitter,
                          "cells": current, "output": output, "loss": loss, "injected": injected})
    final = {"emitter": emitter, "cells": current, "output": output, "loss": loss, "injected": injected}
    return {"model": "tesseract-cell-reservoir-transport-v1", "config": asdict(config),
            "state_names": STATE_NAMES, "substeps": substeps, "h": h,
            "schedule": {**schedule, "multipliers": tuple(tuple(row) for row in schedule["multipliers"])},
            "initial_total": initial_total, "final": final,
            "steps": tuple(steps), "interval_snapshots": tuple(snapshots),
            "checks": {"global_balance": "PASS", "nonnegative_states": "PASS",
                       "stochastic_kernel": "PASS", "cubical_region_balance": "PASS",
                       "checked_steps": audit_count, "exact_balance_residual": F(0)}}
