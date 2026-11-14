"""Exact observation dependencies and bounded representation geometry.

These are downstream report operations, never an inferred physical model/clock.
Intervals are deterministic enclosures, not standard deviations. A covariance
matrix is accepted only as an explicitly supplied joint input assumption.
"""
from fractions import Fraction as Q
from math import isqrt
from simulations.scoped_reduction import exact, bounded, bounded_sum, ReductionError

ERROR_CLASSES = ('input_measurement', 'sampling', 'model_discrepancy',
                 'truncation_aggregation', 'numerical', 'detection_incompleteness', 'anchor')


def interval(lo, hi):
    lo, hi = exact(lo), exact(hi)
    if lo > hi:
        raise ReductionError('reversed interval')
    return lo, hi


def affine_interval(terms, inputs, unit):
    """Same named input cancels before interval propagation; no independence guess.

    terms = [(coefficient, input_id), ...], inputs[id] = {unit, interval|None}.
    A truly unknown dependency stays unknown unless its combined coefficient is zero.
    """
    if type(unit) is not str or not unit:
        raise ReductionError('output unit required')
    combined = {}
    for coefficient, name in terms:
        if type(name) is not str or name not in inputs or inputs[name]['unit'] != unit:
            raise ReductionError('missing dependency or incompatible unit')
        combined[name] = combined.get(name, Q(0)) + exact(coefficient)
    lo = hi = Q(0)
    unknown = []
    for name, coefficient in combined.items():
        if coefficient == 0:
            continue
        limits = inputs[name]['interval']
        if limits is None:
            unknown.append(name)
            continue
        a, b = interval(*limits)
        pair = coefficient*a, coefficient*b
        lo += min(pair); hi += max(pair)
    return {'interval': None if unknown else (lo, hi), 'unit': unit,
            'unknown_dependencies': unknown, 'combined_coefficients': combined,
            'method': 'affine_dependency_preserving_interval', 'independence_assumed': False}


def linear_variance(weights, covariance, *, input_unit):
    """J Sigma J^T after exact symmetric PSD validation, including singular inputs."""
    weights = tuple(exact(x) for x in weights)
    n = len(weights)
    if not 0 < n <= 32 or len(covariance) != n or any(len(row) != n for row in covariance):
        raise ReductionError('bounded square joint covariance required')
    if type(input_unit) is not str or not input_unit:
        raise ReductionError('joint input unit required')
    sigma = tuple(tuple(exact(x) for x in row) for row in covariance)
    if any(sigma[i][j] != sigma[j][i] for i in range(n) for j in range(n)):
        raise ReductionError('covariance must be symmetric')
    work = [list(row) for row in sigma]
    for i in range(n):
        diagonal = work[i][i]
        if diagonal < 0 or (diagonal == 0 and any(work[i][j] != 0 for j in range(i+1, n))):
            raise ReductionError('covariance is not positive semidefinite')
        if diagonal:
            for j in range(i+1, n):
                for k in range(i+1, n):
                    work[j][k] -= work[j][i]*work[i][k]/diagonal
    return {'variance': sum((weights[i]*sigma[i][j]*weights[j] for i in range(n) for j in range(n)), Q(0)),
            'unit': input_unit+'^2', 'method': 'exact_affine_joint_covariance',
            'covariance': sigma, 'weights': weights}


def norm_squared(coordinates):
    if type(coordinates) not in (tuple,list) or len(coordinates)>64:
        raise ReductionError('finite coordinate vector of at most 64 entries required')
    coordinates=tuple(bounded(x,4096) for x in coordinates)
    return bounded_sum((bounded(x*x,4096) for x in coordinates),4096)


def symbolic_norm(coordinates, *, decimal_places=12):
    """sqrt(sum x²) remains symbolic, alongside a rational proved enclosure."""
    if type(decimal_places) is not int or not 0 <= decimal_places <= 1000:
        raise ReductionError('bounded exact decimal-place request required')
    square = norm_squared(coordinates)
    scale = 10**decimal_places
    floor = isqrt(square.numerator * scale*scale // square.denominator)
    lo = Q(floor, scale)
    hi = lo if lo*lo == square else Q(floor+1, scale)
    if not lo*lo <= square <= hi*hi:
        raise ReductionError('invalid square-root enclosure')
    return {'expression': {'operation': 'sqrt', 'radicand': square},
            'interval': (lo, hi), 'width_bound': hi-lo,
            'exact_rational': lo if lo == hi else None,
            'metric': 'declared_downstream_Euclidean_representation'}


def angle(turns):
    return {'coefficient': exact(turns), 'symbol': 'tau', 'unit': 'radian',
            'definition': 'one full turn = tau radians', 'exactness': 'symbolic'}


def observable(value, *, unit, preparation, kernel, boundary, window, scale,
               evidence, observation_map, anchor, errors):
    """Enforce explicit report dependencies; source authenticity is separately bound."""
    value = None if value is None else exact(value)
    for name, operand in [('unit', unit), ('preparation', preparation), ('kernel', kernel),
                          ('boundary', boundary), ('observation_map', observation_map)]:
        if type(operand) is not str or not operand:
            raise ReductionError(name + ' required')
    if type(window) is not tuple or len(window) != 2 or any(type(x) is not int for x in window) or window[0] > window[1]:
        raise ReductionError('exact half-open report window required')
    if not isinstance(evidence, (tuple, list)) or not evidence or any(type(x) is not str or not x for x in evidence):
        raise ReductionError('retrievable evidence locators required')
    if not isinstance(anchor, dict) or anchor.get('kind') not in ('native', 'explicit_calibration'):
        raise ReductionError('host timer is not a temporal/SI anchor')
    if anchor['kind'] == 'explicit_calibration' and not all(anchor.get(k) for k in ('source_ref', 'map_id', 'unit')):
        raise ReductionError('calibration needs provenance, map and units')
    if set(errors) != set(ERROR_CLASSES):
        raise ReductionError('all error classes must be reported')
    checked = {}
    for name, row in errors.items():
        if row.get('unit') != unit or row.get('status') not in ('exact', 'bounded', 'unknown', 'not_applicable'):
            raise ReductionError('error unit/status mismatch')
        status = row['status']
        if status in ('unknown', 'not_applicable') and row.get('bound') is not None:
            raise ReductionError('unknown/not-applicable error is not zero')
        if status in ('bounded', 'exact'):
            bound = exact(row.get('bound'))
            if bound < 0 or (status == 'exact' and bound != 0):
                raise ReductionError('invalid error bound')
        if not row.get('reason'):
            raise ReductionError('error interpretation required')
        checked[name] = dict(row)
    return {'value': value, 'unit': unit, 'preparation': preparation, 'kernel': kernel,
            'boundary': boundary, 'window': window, 'scale': scale,
            'evidence': tuple(evidence), 'observation_map': observation_map,
            'anchor': dict(anchor), 'errors': checked,
            'status': 'unresolved' if value is None else 'reported'}
