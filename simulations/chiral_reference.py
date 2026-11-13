"""Independent exact reference for the declared two-carrier encounter model.

This module does not import the sampled model evaluator or its transitions.
It enumerates destination pairs with integer movement weights and independently
forms the four sign outcomes. Integer dynamic programming uses a common exact
denominator, while a separate position-only recursion evaluates exposure and
first-event probabilities. These are probabilities under the declared finite
law, not measured proton-collision rates or physical uncertainties.
"""
from __future__ import annotations

from collections import defaultdict
from fractions import Fraction
from itertools import product
from math import lcm

from simulations.tesseract_cells import FACETS, facet_neighbors


F = Fraction
INITIAL_STATE = ("0***", "1***", 1, -1)
STRATA = ("core_contact", "core_separate", "outer_contact", "outer_separate")


def _probability(value, name):
    if type(value) not in (int, Fraction):
        raise TypeError(name + " must be an exact integer or Fraction")
    value = F(value)
    if not 0 <= value <= 1:
        raise ValueError(name + " must lie in [0,1]")
    return value


def _steps(value):
    if type(value) is not int or value < 0:
        raise ValueError("steps must be a nonnegative integer")
    return value


def _state(value):
    if not isinstance(value, (tuple, list)) or len(value) != 4:
        raise ValueError("state requires two facets and two chirality signs")
    a, b, chi_a, chi_b = value
    if a not in FACETS or b not in FACETS:
        raise ValueError("state facet lies outside the declared tesseract shell")
    if any(type(x) is not int or x not in (-1, 1) for x in (chi_a, chi_b)):
        raise ValueError("chirality signs must be the exact integers -1 or +1")
    return a, b, chi_a, chi_b


def fixed_probability_checks(probability, steps):
    """Independent Bernoulli identities for one carrier at constant q.

    A terminal sign difference counts odd flip parity, whereas at-least-one
    flip counts every nonempty flip history. Expected flip count is a third
    quantity and need not lie below one.
    """
    q, n = _probability(probability, "probability"), _steps(steps)
    return {"expected_flip_count": n * q,
            "probability_at_least_one_flip": 1 - (1 - q) ** n,
            "probability_terminal_sign_change": (1 - (1 - 2 * q) ** n) / 2}


def exact_reference(*, steps=8, core_facet="*0**",
                    shell_probability=F(1, 16), core_probability=F(1, 4),
                    contact_gain=F(1, 4), initial_state=INITIAL_STATE):
    """Enumerate the finite joint law and exact opportunity-conditioned rewards.

    On each completed-return tick each carrier holds with weight 2 or moves to
    one of six neighboring facets with weight 1, total 8. Flip probabilities
    are evaluated at the destinations. Co-residence applies the separately
    declared law q -> q + contact_gain*(1-q). Sign draws are conditionally
    independent given those positions. The fixed core tag is an explicit
    profile declaration, not an intrinsic center supplied by cell incidence.

    ``distribution`` has tuple-state keys and Fraction values. ``step_reports``
    and cumulative rewards distinguish core/outer and contact/separate
    opportunities. Complement recursions provide exact probabilities of any
    flip and any co-residence. No Monte Carlo confidence interval is implied.
    """
    n = _steps(steps)
    initial = _state(initial_state)
    if core_facet not in FACETS:
        raise ValueError("core facet must be explicitly selected from the shell")
    shell = _probability(shell_probability, "shell_probability")
    core = _probability(core_probability, "core_probability")
    gain = _probability(contact_gain, "contact_gain")
    if core < shell:
        raise ValueError("the core-enhancement model requires core >= shell")

    q_by_stratum = {"core_contact": core + gain * (1 - core),
                    "core_separate": core,
                    "outer_contact": shell + gain * (1 - shell),
                    "outer_separate": shell}
    denominator = lcm(*(q.denominator for q in q_by_stratum.values()))
    joint_denominator = 64 * denominator ** 2

    def stratum(facet, contact):
        return ("core" if facet == core_facet else "outer") + (
            "_contact" if contact else "_separate")

    # The movement relation is read from incidence; no sampled-model helper
    # supplies either normalization or transition probabilities.
    moves = {facet: ((facet, 2), *((x, 1) for x in facet_neighbors(facet)))
             for facet in FACETS}
    if any(len(row) != 7 or sum(w for _, w in row) != 8 for row in moves.values()):
        raise ArithmeticError("independent movement weights failed normalization")
    operators = {}
    for old_a, old_b in product(FACETS, repeat=2):
        row = []
        for (new_a, wa), (new_b, wb) in product(moves[old_a], moves[old_b]):
            contact = new_a == new_b
            sa, sb = stratum(new_a, contact), stratum(new_b, contact)
            qa, qb = q_by_stratum[sa], q_by_stratum[sb]
            ia, ib = int(qa * denominator), int(qb * denominator)
            outcomes = ((1, 1, (denominator - ia) * (denominator - ib)),
                        (-1, 1, ia * (denominator - ib)),
                        (1, -1, (denominator - ia) * ib),
                        (-1, -1, ia * ib))
            row.append((new_a, new_b, wa * wb, ia, ib, outcomes))
        row_mass = sum(w * sum(t[2] for t in outcomes)
                       for _, _, w, _, _, outcomes in row)
        if row_mass != joint_denominator:
            raise ArithmeticError("independent joint row is not normalized")
        operators[(old_a, old_b)] = tuple(row)

    joint = {initial: 1}
    position = {initial[:2]: 1}
    no_contact = {initial[:2]: 1}
    no_flip = {name: {initial[:2]: 1} for name in ("either", "a", "b")}
    joint_scale = position_scale = 1
    cumulative_opportunities = {s: F(0) for s in STRATA}
    cumulative_flips = {s: F(0) for s in STRATA}
    cumulative_carrier_flips = {"a": F(0), "b": F(0)}
    step_reports = []

    for index in range(1, n + 1):
        next_joint = defaultdict(int)
        for (a, b, ca, cb), mass in joint.items():
            for da, db, movement, _, _, outcomes in operators[(a, b)]:
                for sign_a, sign_b, flips in outcomes:
                    if flips:
                        next_joint[(da, db, ca * sign_a, cb * sign_b)] += mass * movement * flips
        joint = dict(next_joint)
        joint_scale *= joint_denominator
        if sum(joint.values()) != joint_scale:
            raise ArithmeticError("independent state recursion lost probability mass")

        next_position = defaultdict(int)
        for pair, mass in position.items():
            for da, db, movement, _, _, _ in operators[pair]:
                next_position[(da, db)] += mass * movement
        position = dict(next_position)
        position_scale *= 64
        if sum(position.values()) != position_scale:
            raise ArithmeticError("independent position recursion lost probability mass")

        for name, surviving in no_flip.items():
            following = defaultdict(int)
            for pair, mass in surviving.items():
                for da, db, movement, ia, ib, _ in operators[pair]:
                    survival = ((denominator - ia) * (denominator - ib) if name == "either"
                                else (denominator - (ia if name == "a" else ib)) * denominator)
                    if survival:
                        following[(da, db)] += mass * movement * survival
            no_flip[name] = dict(following)
        following_contact = defaultdict(int)
        for pair, mass in no_contact.items():
            for da, db, movement, _, _, _ in operators[pair]:
                if da != db:
                    following_contact[(da, db)] += mass * movement
        no_contact = dict(following_contact)

        opportunities = {s: F(0) for s in STRATA}
        carrier_flips = {"a": F(0), "b": F(0)}
        contact_probability = core_contact_probability = F(0)
        for (a, b), mass in position.items():
            probability = F(mass, position_scale)
            contact = a == b
            if contact:
                contact_probability += probability
                if a == core_facet:
                    core_contact_probability += probability
            for carrier, facet in (("a", a), ("b", b)):
                group = stratum(facet, contact)
                opportunities[group] += probability
                carrier_flips[carrier] += probability * q_by_stratum[group]
        flips = {s: opportunities[s] * q_by_stratum[s] for s in STRATA}
        if sum(opportunities.values()) != 2 or sum(flips.values()) != sum(carrier_flips.values()):
            raise ArithmeticError("reward accounting differs from two carrier opportunities")
        for s in STRATA:
            cumulative_opportunities[s] += opportunities[s]
            cumulative_flips[s] += flips[s]
        for name in ("a", "b"):
            cumulative_carrier_flips[name] += carrier_flips[name]
        sign_change = {
            "a": F(sum(m for (_, _, ca, _), m in joint.items() if ca != initial[2]), joint_scale),
            "b": F(sum(m for (_, _, _, cb), m in joint.items() if cb != initial[3]), joint_scale)}
        step_reports.append({
            "step": index, "normalization": F(sum(joint.values()), joint_scale),
            "expected_opportunities_by_stratum": opportunities,
            "expected_flips_by_stratum": flips,
            "expected_flips_by_carrier": carrier_flips,
            "expected_flip_count": sum(flips.values()),
            "contact_probability": contact_probability,
            "core_contact_probability": core_contact_probability,
            "probability_terminal_sign_change": sign_change,
            "probability_ever_contact": 1 - F(sum(no_contact.values()), position_scale),
            "probability_at_least_one_flip": {
                name: 1 - F(sum(counts.values()), joint_scale)
                for name, counts in no_flip.items()},
        })

    terminal_sign_change = {
        "a": F(sum(m for (_, _, ca, _), m in joint.items() if ca != initial[2]), joint_scale),
        "b": F(sum(m for (_, _, _, cb), m in joint.items() if cb != initial[3]), joint_scale)}
    return {
        "format": "aod.dec.chiral-independent-reference", "version": 1, "status": "PASS",
        "scope": "Exact conditional finite law; no physical collision calibration or confidence level.",
        "config": {"steps": n, "core_facet": core_facet, "shell_probability": shell,
                   "core_probability": core, "contact_gain": gain, "initial_state": initial},
        "q_by_stratum": q_by_stratum, "flip_ticket_denominator": denominator,
        "joint_step_denominator": joint_denominator,
        "distribution": {state: F(mass, joint_scale) for state, mass in joint.items()},
        "position_distribution": {pair: F(mass, position_scale) for pair, mass in position.items()},
        "normalization": F(sum(joint.values()), joint_scale),
        "population_count": 2, "step_reports": step_reports,
        "expected_opportunities_by_stratum": cumulative_opportunities,
        "expected_flips_by_stratum": cumulative_flips,
        "expected_flips_by_carrier": cumulative_carrier_flips,
        "expected_flip_count": sum(cumulative_flips.values()),
        "conditional_flip_probability_by_stratum": {
            s: cumulative_flips[s] / cumulative_opportunities[s]
            if cumulative_opportunities[s] else None for s in STRATA},
        "probability_terminal_sign_change": terminal_sign_change,
        "probability_ever_contact": 1 - F(sum(no_contact.values()), position_scale),
        "probability_at_least_one_flip": {
            name: 1 - F(sum(counts.values()), joint_scale) for name, counts in no_flip.items()},
        "first_event_window": "completed-return updates 1 through steps; the initial state is excluded",
        "method": "Independent integer-weight joint recursion and position-only complement recursions; no sampled-model transitions are called.",
    }
