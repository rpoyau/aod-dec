"""Independent continuous reference for the declared cell transport fixture.

The affine generator is assembled from model parameters and the retained face
rate schedule. No Euler update or stochastic-kernel constructor is called.
Each dimensionless unit interval uses a Decimal matrix-exponential action.

The Taylor remainder majorant is exact rational arithmetic for the exact
matrix and the represented input vector. Decimal arithmetic in the actual
matrix action is rounded; the reported decimal tail and precision comparison
are estimates, not a rigorous enclosure of the complete numerical solution.
This evaluates the declared transport equations, not a physical maser model.
"""
from __future__ import annotations

from decimal import Decimal, localcontext
from fractions import Fraction
import hashlib
import json

from simulations.tesseract_cells import FACETS, facet_interfaces


STATE_NAMES = ("emitter", *FACETS, "output", "loss", "injected", "constant")
OUTPUT_INDEX = 9
LOSS_INDEX = 10
INJECTED_INDEX = 11
CONSTANT_INDEX = 12


def _fraction(value):
    if isinstance(value, bool) or not isinstance(value, (int, Fraction, Decimal)):
        raise TypeError("reference values must be exact integers, fractions, or Decimal")
    if isinstance(value, Decimal) and not value.is_finite():
        raise ValueError("reference values must be finite")
    return Fraction(value)


def _decimal(value):
    exact = _fraction(value)
    return Decimal(exact.numerator) / Decimal(exact.denominator)


def _state(vector):
    return {
        "emitter": vector[0], "cells": tuple(vector[1:9]),
        "output": vector[OUTPUT_INDEX], "loss": vector[LOSS_INDEX],
        "injected": vector[INJECTED_INDEX],
    }


def flatten_reference(result):
    """Return the twelve reported state components in transport state order."""
    state = result["final"]
    return (state["emitter"], *state["cells"], state["output"],
            state["loss"], state["injected"])


def generator_matrix(config, face_rates):
    """Assemble x'=A x independently; the last coordinate is constant one.

    Rows are destination derivatives and columns are source state coordinates.
    Symmetric exchange is attached to each retained shared square, while pump,
    emission, output coupling and absorption retain their declared directions.
    """
    rates = tuple(_fraction(rate) for rate in face_rates)
    interfaces = facet_interfaces()
    if len(rates) != len(interfaces) or any(rate < 0 for rate in rates):
        raise ValueError("one nonnegative rate is required per shared square")
    if config.source not in FACETS or config.output not in FACETS:
        raise ValueError("source and output must name cubic boundary facets")
    pump, gamma, leak, absorption = map(
        _fraction, (config.pump, config.gamma, config.leak, config.absorption))
    if any(value < 0 for value in (pump, gamma, leak, absorption)):
        raise ValueError("pump, gamma, leak and absorption must be nonnegative")
    matrix = [[Fraction(0) for _ in STATE_NAMES] for _ in STATE_NAMES]
    matrix[0][0] = -gamma
    matrix[0][CONSTANT_INDEX] = pump
    matrix[1 + FACETS.index(config.source)][0] = gamma
    matrix[INJECTED_INDEX][CONSTANT_INDEX] = pump
    for interface, rate in zip(interfaces, rates):
        left, right = 1 + FACETS.index(interface.left), 1 + FACETS.index(interface.right)
        matrix[left][left] -= rate
        matrix[right][right] -= rate
        matrix[left][right] += rate
        matrix[right][left] += rate
    for cell in range(1, 9):
        matrix[cell][cell] -= absorption
        matrix[LOSS_INDEX][cell] += absorption
    output_cell = 1 + FACETS.index(config.output)
    matrix[output_cell][output_cell] -= leak
    matrix[OUTPUT_INDEX][output_cell] += leak
    return tuple(tuple(row) for row in matrix)


def exponential_action(matrix, vector, *, duration=Fraction(1), precision=60,
                       tolerance=None, max_terms=512):
    """Compute exp(duration*A) vector with a scalar Taylor-tail majorant.

    After terms 0..n, let q=||duration*A||_infinity and M=||vector||_infinity.
    The omitted series norm is at most

      M q**(n+1)/(n+1)! / (1 - q/(n+2)),  when n+2 > q.

    This bound covers the exact series for the represented inputs. It does not
    bound Decimal rounding, input conversion error, or propagation of earlier
    interval errors. A different precision run diagnoses rounding sensitivity.
    """
    if type(precision) is not int or precision < 25:
        raise ValueError("reference precision must be an integer of at least 25 digits")
    if type(max_terms) is not int or max_terms < 1:
        raise ValueError("max_terms must be a positive integer")
    exact_matrix = tuple(tuple(_fraction(value) for value in row) for row in matrix)
    size = len(exact_matrix)
    if not size or any(len(row) != size for row in exact_matrix):
        raise ValueError("matrix must be nonempty and square")
    exact_vector = tuple(_fraction(value) for value in vector)
    if len(exact_vector) != size:
        raise ValueError("vector size must match matrix")
    span = _fraction(duration)
    if span < 0:
        raise ValueError("duration must be nonnegative")
    tolerance = (Fraction(1, 10 ** (precision - 10)) if tolerance is None
                 else _fraction(tolerance))
    if tolerance <= 0:
        raise ValueError("tolerance must be positive")
    scaled = tuple(tuple(span * value for value in row) for row in exact_matrix)
    norm = max(sum(abs(value) for value in row) for row in scaled)
    majorant = max(abs(value) for value in exact_vector)
    with localcontext() as context:
        context.prec = precision
        decimal_matrix = tuple(tuple(_decimal(value) for value in row) for row in scaled)
        term = tuple(_decimal(value) for value in exact_vector)
        total = list(term)
        # The zero matrix has no omitted terms, including for a nonzero vector.
        if norm == 0 or majorant == 0:
            return {"vector": tuple(total), "terms": 1, "precision": precision,
                    "matrix_infinity_norm": str(norm), "tail_estimate": Decimal(0),
                    "rounding_included_in_tail": False}
        for order in range(1, max_terms + 1):
            term = tuple(sum((coefficient * value for coefficient, value in zip(row, term)),
                             Decimal(0)) / Decimal(order) for row in decimal_matrix)
            total = [accumulated + value for accumulated, value in zip(total, term)]
            majorant *= norm / order
            if order + 2 > norm:
                tail = majorant * norm / (order + 1) / (1 - norm / (order + 2))
                if tail <= tolerance:
                    return {"vector": tuple(total), "terms": order + 1,
                            "precision": precision, "matrix_infinity_norm": str(norm),
                            "tail_estimate": _decimal(tail),
                            "rounding_included_in_tail": False}
        raise ArithmeticError("matrix exponential did not reach the declared tail tolerance")


def simulate_reference(config, schedule=None, *, precision=60):
    """Evolve the same piecewise-constant jitter path in continuous model time."""
    # Shared helpers provide inputs only; the generator and solver are independent.
    from simulations.cell_transport import ModelConfig, interval_rates, make_schedule

    if not isinstance(config, ModelConfig):
        raise TypeError("config must be ModelConfig")
    if type(precision) is not int or precision < 25:
        raise ValueError("reference precision must be an integer of at least 25 digits")
    if schedule is None:
        schedule = make_schedule(config.intervals)
    with localcontext() as context:
        context.prec = precision
        vector = tuple(_decimal(value) for value in
                       (config.initial_emitter, *config.initial_cells, 0, 0, 0, 1))
        if len(vector) != len(STATE_NAMES) or any(value < 0 for value in vector):
            raise ValueError("initial state must contain an emitter and eight nonnegative cells")
        trajectory = [_state(vector)]
        intervals = []
        for interval in range(config.intervals):
            matrix = generator_matrix(config, interval_rates(config, schedule, interval))
            identity = json.dumps([[str(value) for value in row] for row in matrix],
                                  separators=(",", ":")).encode()
            record = exponential_action(matrix, vector, precision=precision)
            vector = record.pop("vector")
            record.update(interval=interval,
                          generator_sha256=hashlib.sha256(identity).hexdigest())
            intervals.append(record)
            trajectory.append(_state(vector))
    return {
        "method": "independently assembled affine generator; Decimal Taylor exponential action",
        "precision": precision, "schedule_sha256": schedule["sha256"],
        "final": _state(vector), "trajectory": tuple(trajectory),
        "intervals": tuple(intervals),
        "error_scope": "Taylor tail estimates exclude Decimal rounding and accumulated earlier-interval errors; precision comparison is diagnostic, not a rigorous enclosure.",
    }


def compare_precisions(config, schedule=None, *, lower=60, higher=80):
    """Retain both independently executed precision settings and their difference."""
    if type(lower) is not int or type(higher) is not int or lower >= higher:
        raise ValueError("higher precision must exceed lower precision")
    low = simulate_reference(config, schedule, precision=lower)
    high = simulate_reference(config, schedule, precision=higher)
    with localcontext() as context:
        context.prec = higher
        differences = tuple(abs(a - b) for a, b in zip(flatten_reference(low), flatten_reference(high)))
    return {"lower": low, "higher": high, "max_absolute_difference": max(differences),
            "interpretation": "Precision sensitivity only; this is not a certified global error bound."}
