"""Exact two-carrier, core-biased Markov encounter fixture.

The owner proposes enhanced flip propensity in field cores. This application
declares the core profile and rational law; neither follows from tesseract
incidence. A chirality update selects a whole admitted return recipe, not an
atomic branch-to-counterbranch jump. Co-residence is a projected encounter,
not automatically coupling, fusion, or a physical proton-proton event.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass, fields, is_dataclass, replace
from fractions import Fraction
from functools import lru_cache
import hashlib
from itertools import product
from math import lcm
from random import Random

from .manual_dec_fixture import source_binding
from .tesseract_cells import FACETS, facet_interfaces, facet_neighbors
from .vendor import aod_manual_native as native


F = Fraction
SCOPE = "chiral-encounter:declared-support"
SIGNS = (-1, 1)


def _exact_probability(value, name):
    if type(value) not in (int, Fraction):
        raise TypeError(name + " requires an exact integer or Fraction")
    value = F(value)
    if not 0 <= value <= 1:
        raise ValueError(name + " must lie in [0,1]")
    return value


@dataclass(frozen=True, slots=True)
class CollisionConfig:
    steps: int = 8
    q_outer: Fraction = F(1, 16)
    q_core: Fraction = F(1, 4)
    contact_gain: Fraction = F(1, 4)
    population_counts: tuple[int, ...] = (1, 1, 4, 1, 1, 1, 1, 1)

    def __post_init__(self):
        if type(self.steps) is not int or not 0 <= self.steps <= 64:
            raise ValueError("steps must be an integer from 0 through 64")
        for name in ("q_outer", "q_core", "contact_gain"):
            object.__setattr__(self, name, _exact_probability(getattr(self, name), name))
        if (not isinstance(self.population_counts, tuple) or len(self.population_counts) != 8
                or any(type(n) is not int or n not in range(1, 5)
                       for n in self.population_counts)):
            raise ValueError("eight declared integer populations in [1,4] required")


@dataclass(frozen=True, slots=True)
class State:
    facet_a: str
    facet_b: str
    chi_a: int
    chi_b: int

    def __post_init__(self):
        if self.facet_a not in FACETS or self.facet_b not in FACETS:
            raise ValueError("state positions must be declared tesseract facets")
        if any(type(x) is not int or x not in SIGNS for x in (self.chi_a, self.chi_b)):
            raise ValueError("chirality is exactly -1 or +1")


INITIAL_STATE = State("0***", "1***", 1, -1)


def states():
    return tuple(State(a, b, ca, cb) for a, b, ca, cb in product(FACETS, FACETS, SIGNS, SIGNS))


@lru_cache(maxsize=8)
def _move_weights(facet):
    if facet not in FACETS:
        raise ValueError("unknown movement facet")
    neighbors = facet_neighbors(facet)
    return tuple((target, 2 if target == facet else 1)
                 for target in FACETS if target == facet or target in neighbors)


def move_kernel(facet):
    """Hold weight 2 and six shared-square moves of weight 1, normalized by 8."""
    return {target: F(weight, 8) for target, weight in _move_weights(facet)}


def core_score(facet, config=CollisionConfig()):
    if facet not in FACETS:
        raise ValueError("unknown profile facet")
    return F(config.population_counts[FACETS.index(facet)] - 1, 3)


def zone(facet, config=CollisionConfig()):
    score = core_score(facet, config)
    return "core" if score == 1 else ("outer" if score == 0 else "intermediate")


def flip_probability(facet, other_facet, config=CollisionConfig()):
    """Exact per-carrier flip probability conditional on destination positions."""
    if other_facet not in FACETS:
        raise ValueError("unknown other-carrier facet")
    q = config.q_outer + (config.q_core - config.q_outer) * core_score(facet, config)
    if facet == other_facet:
        q += config.contact_gain * (1 - q)
    return q


def conditional_flip_rows(config=CollisionConfig()):
    return [{"facet": facet, "population_count": config.population_counts[i],
             "core_score": core_score(facet, config), "zone": zone(facet, config),
             "without_contact": flip_probability(facet, FACETS[(i + 1) % 8], config),
             "with_contact": flip_probability(facet, facet, config)}
            for i, facet in enumerate(FACETS)]


def common_ticket_denominator(configs):
    """One declared exact ticket grid for paired probability interventions."""
    configs = tuple(configs)
    if not configs or any(not isinstance(c, CollisionConfig) for c in configs):
        raise ValueError("one or more CollisionConfig records required")
    return lcm(*(flip_probability(a, b, c).denominator
                 for c in configs for a, b in product(FACETS, FACETS)))


def _ticket_denominator(config, supplied):
    required = common_ticket_denominator((config,))
    if supplied is None:
        return required
    if type(supplied) is not int or supplied <= 0 or supplied % required:
        raise ValueError("ticket denominator must represent every probability exactly")
    return supplied


@lru_cache(maxsize=16)
def _integer_kernel(config):
    """Common-denominator kernel; integer DP is exactly equal to rational DP."""
    d = common_ticket_denominator((config,))
    denominator = 64 * d * d
    kernel = {}
    for before in states():
        rows = []
        for a, wa in _move_weights(before.facet_a):
            for b, wb in _move_weights(before.facet_b):
                ta = int(flip_probability(a, b, config) * d)
                tb = int(flip_probability(b, a, config) * d)
                for fa, fb in product((False, True), repeat=2):
                    numerator = wa * wb * (ta if fa else d - ta) * (tb if fb else d - tb)
                    if numerator:
                        after = State(a, b, -before.chi_a if fa else before.chi_a,
                                      -before.chi_b if fb else before.chi_b)
                        rows.append((after, numerator))
        if sum(n for _, n in rows) != denominator:
            raise ArithmeticError("transition row failed exact normalization")
        kernel[before] = tuple(rows)
    return denominator, kernel


def successors(state, config=CollisionConfig()):
    if not isinstance(state, State):
        raise TypeError("complete State required")
    denominator, kernel = _integer_kernel(config)
    return {after: F(numerator, denominator) for after, numerator in kernel[state]}


def exact_distribution(config=CollisionConfig(), steps=None, initial=INITIAL_STATE):
    """Exact joint terminal law, not an empirical frequency or calibrated law."""
    if not isinstance(initial, State):
        raise TypeError("complete initial State required")
    count = config.steps if steps is None else steps
    if type(count) is not int or not 0 <= count <= 64:
        raise ValueError("exact step count must be an integer from 0 through 64")
    denominator, kernel = _integer_kernel(config)
    weights, scale = {initial: 1}, 1
    for _ in range(count):
        next_weights = {}
        for before, weight in weights.items():
            for after, numerator in kernel[before]:
                next_weights[after] = next_weights.get(after, 0) + weight * numerator
        weights, scale = next_weights, scale * denominator
        if sum(weights.values()) != scale:
            raise ArithmeticError("probability population was not conserved")
    return {state: F(weight, scale) for state, weight in weights.items()}


def distribution_rows(distribution):
    return [{"state": state, "probability": probability}
            for state, probability in sorted(distribution.items(),
                key=lambda item: (FACETS.index(item[0].facet_a), FACETS.index(item[0].facet_b),
                                  item[0].chi_a, item[0].chi_b))]


def _stream(seed, name):
    if type(seed) is not int:
        raise TypeError("seed must be an integer")
    binding = f"aod-dec-chiral-v1:{seed}:{name}".encode()
    return Random(int.from_bytes(hashlib.sha256(binding).digest(), "big"))


def _move(facet, ticket):
    remaining = ticket
    for target, weight in _move_weights(facet):
        if remaining < weight:
            return target, F(weight, 8)
        remaining -= weight
    raise ArithmeticError("movement ticket outside its support")


def _crossed_face(before, after):
    if before == after:
        return None
    for interface in facet_interfaces():
        if {before, after} == {interface.left, interface.right}:
            return interface.face
    raise ArithmeticError("movement crossed no declared shared square")


def _initial_carrier(lineage, chirality):
    records = [native.certified(native.trace(lineage, k, scope=SCOPE,
                                            chirality=chirality)) for k in range(2)]
    return records, native.wave(records)


def _continued_return(lineage, occurrence, chirality, path_id):
    """Bind new counterfactual events without rewriting the shared initial prefix."""
    trace = native.trace(lineage, occurrence, scope=SCOPE, chirality=chirality)
    event_ids = tuple(path_id + ":" + event_id for event_id in trace.event_ids)
    witness = replace(trace.reflection,
        turnaround_id=event_ids[trace.turn], outbound_ids=event_ids[1:trace.turn + 1],
        return_ids=event_ids[trace.turn + 1:], target_start_id=event_ids[0])
    return replace(trace, event_ids=event_ids, reflection=witness)


@lru_cache(maxsize=16)
def _background(config):
    waves, placement = [], {}
    for facet, count in zip(FACETS, config.population_counts):
        for slot in range(count):
            wave = native.recurrent(f"background:{facet}:{slot}", n=2, scope=SCOPE)
            waves.append(wave)
            placement[wave.identity] = facet
    field = native.field(SCOPE, "declared-core-profile", waves)
    if len(field.members) != sum(config.population_counts):
        raise ArithmeticError("background aliases changed the declared profile")
    return tuple(waves), field, placement


def background_record(config=CollisionConfig()):
    waves, field, placement = _background(config)
    return {"waves": waves, "field": field, "support_map": dict(placement),
            "population_counts": dict(zip(FACETS, config.population_counts)),
            "population_count": len(field.members),
            "profile_status": "declared frozen background, separate from the two carriers",
            "density": None, "cell_measure": None,
            "core_definition": "score=(population_count-1)/3; designated core has score one",
            "geometric_center_inferred": False}


def simulate_path(config=CollisionConfig(), seed=7, *, retain_witnesses=True,
                  ticket_denominator=None, initial=INITIAL_STATE):
    """Sample with exact integer tickets and preserve completed-return histories.

    Setting retain_witnesses=False omits large witness payloads only. It still
    constructs/certifies all selected returns and validates final recurrence.
    Separate move streams guarantee unchanged movement under a flip-law change;
    paired flip draws additionally require the same explicit ticket denominator.
    """
    if type(retain_witnesses) is not bool or not isinstance(initial, State):
        raise TypeError("Boolean retention mode and complete initial State required")
    source = source_binding()
    d = _ticket_denominator(config, ticket_denominator)
    path_id = native.digest({"config": asdict(config), "seed": seed,
                             "initial": asdict(initial), "ticket_denominator": d})
    rng = {name: _stream(seed, name) for name in ("move-a", "move-b", "flip-a", "flip-b")}
    records_a, wave_a = _initial_carrier("carrier-A", initial.chi_a)
    records_b, wave_b = _initial_carrier("carrier-B", initial.chi_b)
    initial_waves = (wave_a, wave_b)
    before, steps, flip_counts = initial, [], [0, 0]
    strata = {"core": {"opportunities": 0, "flips": 0},
              "outer": {"opportunities": 0, "flips": 0},
              "intermediate": {"opportunities": 0, "flips": 0}}
    contacts = 0
    for k in range(config.steps):
        move_tickets = (rng["move-a"].randrange(8), rng["move-b"].randrange(8))
        a, pa = _move(before.facet_a, move_tickets[0])
        b, pb = _move(before.facet_b, move_tickets[1])
        qa, qb = flip_probability(a, b, config), flip_probability(b, a, config)
        thresholds = (int(qa * d), int(qb * d))
        flip_tickets = (rng["flip-a"].randrange(d), rng["flip-b"].randrange(d))
        flipped = tuple(ticket < threshold for ticket, threshold in zip(flip_tickets, thresholds))
        after = State(a, b, -before.chi_a if flipped[0] else before.chi_a,
                      -before.chi_b if flipped[1] else before.chi_b)
        witnesses = []
        for index, (records, lineage, chi) in enumerate(((records_a, "carrier-A", after.chi_a),
                                                       (records_b, "carrier-B", after.chi_b))):
            previous_return_id = records[-1][0].return_id
            trace = _continued_return(lineage, k + 2, chi, path_id)
            record = native.certified(trace)
            records.append(record)
            witnesses.append({"carrier_identity": initial_waves[index].identity,
                "parent_return_id": previous_return_id, "return_id": trace.return_id,
                "trace_digest": trace.trace_digest,
                "trace": trace if retain_witnesses else None,
                "certificate": record[1] if retain_witnesses else None})
        for index, facet in enumerate((a, b)):
            flip_counts[index] += int(flipped[index])
            strata[zone(facet, config)]["opportunities"] += 1
            strata[zone(facet, config)]["flips"] += int(flipped[index])
        contacts += int(a == b)
        transition_probability = pa * pb * (qa if flipped[0] else 1 - qa) * (qb if flipped[1] else 1 - qb)
        steps.append({"event_id": f"encounter:{path_id}:completed-return:{k + 1}",
            "ordinal": k + 1, "before": before, "after": after,
            "movement_tickets": move_tickets, "movement_denominator": 8,
            "movement_probabilities": (pa, pb),
            "crossed_faces": (_crossed_face(before.facet_a, a), _crossed_face(before.facet_b, b)),
            "flip_tickets": flip_tickets, "flip_denominator": d,
            "flip_thresholds": thresholds, "flip_probabilities": (qa, qb),
            "flipped": flipped, "transition_probability": transition_probability,
            "support_contact": a == b, "classification": "contact_candidate" if a == b else "separate_support",
            "coupling_admitted": False, "fusion_admitted": False,
            "return_witnesses": witnesses})
        before = after
    final_waves = native.wave(records_a), native.wave(records_b)
    if tuple(w.identity for w in final_waves) != tuple(w.identity for w in initial_waves):
        raise ArithmeticError("a mutable chirality selection changed retained carrier identity")
    return {"format": "aod-dec.chiral-encounter-path", "version": 1, "status": "PASS",
            "config": config, "seed": seed, "path_id": path_id, "source": source,
            "initial": initial, "final": before, "steps": steps,
            "ticket_denominator": d, "flip_counts": tuple(flip_counts),
            "contact_count": contacts, "strata": strata,
            "terminal_changed": (before.chi_a != initial.chi_a, before.chi_b != initial.chi_b),
            "any_flip": (flip_counts[0] > 0, flip_counts[1] > 0),
            "carrier_identities": tuple(w.identity for w in initial_waves),
            "initial_waves": initial_waves if retain_witnesses else None,
            "final_waves": final_waves if retain_witnesses else None,
            "witness_payloads_retained": retain_witnesses,
            "physical_proton_correspondence": "NOT_INSTANTIATED",
            "SI_clock_energy_momentum_map": "NOT_INSTANTIATED"}


def exact_data(value):
    if isinstance(value, Fraction):
        return str(value)
    if is_dataclass(value):
        return exact_data(asdict(value))
    if isinstance(value, dict):
        if any(not isinstance(key, (str, int)) for key in value):
            raise TypeError("use distribution_rows for structured state keys")
        return {str(key): exact_data(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [exact_data(item) for item in value]
    if isinstance(value, float):
        raise TypeError("binary floats are display-only, not exact fixture records")
    return value


def _same_exact_record(left, right):
    """Preserve input types: True is not ticket 1, nor is '1/4' a Fraction."""
    if type(left) is not type(right):
        return False
    if is_dataclass(left):
        return all(_same_exact_record(getattr(left, f.name), getattr(right, f.name))
                   for f in fields(left))
    if isinstance(left, dict):
        if {(type(k), k) for k in left} != {(type(k), k) for k in right}:
            return False
        return all(_same_exact_record(v, right[k]) for k, v in left.items())
    if isinstance(left, (tuple, list)):
        return len(left) == len(right) and all(_same_exact_record(a, b)
                                             for a, b in zip(left, right))
    return left == right


def validate_path(path):
    """Replay the declared seed/config/ticket experiment and compare every row.

    This binds the internal fixture, including its actual return witnesses. It
    does not authenticate external collision data or certify a proton model.
    The in-memory dataclass representation is required; JSON is an output view.
    """
    if not isinstance(path, dict) or not isinstance(path.get("config"), CollisionConfig):
        raise TypeError("in-memory path with a bound CollisionConfig required")
    rebuilt = simulate_path(path["config"], path["seed"],
        retain_witnesses=path["witness_payloads_retained"],
        ticket_denominator=path["ticket_denominator"], initial=path["initial"])
    if not _same_exact_record(rebuilt, path):
        raise ValueError("path differs from its declared exact replay, including retained witnesses")
    return True
