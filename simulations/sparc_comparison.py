"""Exact SPARC target comparison after an explicitly declared projection.

This module aligns externally projected square-speed values to the entire frozen
target family. It does not generate fields, choose an observation map, fit model
parameters, or infer a physical clock. Fractional losses and percentages are
separate output fields. Nominal Vobs +/- errV intervals are diagnostic intervals,
not a confidence or systematic-uncertainty claim.
"""
from dataclasses import dataclass
from fractions import Fraction
from collections.abc import Mapping

from simulations.sparc_inputs import BinKey, TargetRow


SQUARE_SPEED_UNIT = 'km^2/s^2'
SCORE_VERSION = 'sparc-square-speed-mape-v1'


class ComparisonError(ValueError):
    pass


def _require(condition, message):
    if not condition:
        raise ComparisonError(message)


def _exact(value, label):
    _require(type(value) in (int, Fraction), label + ' requires an exact int/Fraction; bool/float excluded')
    return Fraction(value)


def _text(value, label):
    _require(type(value) is str and bool(value.strip()), label + ' requires a nonempty string')


def _key(value):
    _require(type(value) is BinKey, 'typed BinKey required')
    _text(value.galaxy, 'galaxy')
    _require(_exact(value.radius_kpc, 'radius') > 0, 'radius must be positive')


@dataclass(frozen=True)
class Projection:
    """One projected U with its declared map, evidence and optional exact bound.

    A nonnegative absolute error bound is supplied only when justified by the
    producer. None means unknown and is preserved as such in the score output.
    Source evidence identifies provenance; its presence is not native admission.
    """
    key: BinKey
    u_km2_s2: Fraction
    observation_map: str
    source_evidence: tuple
    unit: str = SQUARE_SPEED_UNIT
    absolute_error_bound: Fraction | None = None

    def __post_init__(self):
        _key(self.key)
        value = _exact(self.u_km2_s2, 'projected square speed')
        _require(value >= 0, 'projected square speed must be nonnegative')
        object.__setattr__(self, 'u_km2_s2', value)
        _require(type(self.unit) is str and self.unit == SQUARE_SPEED_UNIT, 'square-speed unit must be ' + SQUARE_SPEED_UNIT)
        _text(self.observation_map, 'observation map')
        _require(type(self.source_evidence) is tuple and bool(self.source_evidence), 'source evidence must be a nonempty tuple')
        for source in self.source_evidence:
            _text(source, 'source evidence')
        if self.absolute_error_bound is not None:
            bound = _exact(self.absolute_error_bound, 'projection absolute error bound')
            _require(bound >= 0, 'projection absolute error bound must be nonnegative')
            object.__setattr__(self, 'absolute_error_bound', bound)


def compare(targets, predictions, *, large_relative_error_threshold=Fraction(1, 2)):
    """Score each frozen target exactly once, rejecting any alignment mismatch.

    The primary mean weights every bin equally. The companion mean weights
    galaxy means equally. Neither ignores a bin for uncertainty flags. Targets
    must be held separately from field preparation and projected predictions.
    """
    threshold = _exact(large_relative_error_threshold, 'relative-error threshold')
    _require(threshold > 0, 'relative-error threshold must be positive')
    _require(isinstance(targets, Mapping) and bool(targets), 'nonempty target mapping required')
    retained_targets = dict(targets)
    for key, target in retained_targets.items():
        _key(key)
        _require(type(target) is TargetRow and target.key == key, 'target key/row identity mismatch')
        _require(_exact(target.vobs_km_s, 'observed speed') > 0, 'observed speed must be positive')
        _require(_exact(target.errv_km_s, 'observed speed error') >= 0, 'observed speed error must be nonnegative')
    projected = {}
    for prediction in predictions:
        _require(type(prediction) is Projection, 'typed Projection required')
        prediction.__post_init__()
        _require(prediction.key not in projected, 'duplicate projected target key')
        _require(prediction.key in retained_targets, 'unexpected projected target key')
        projected[prediction.key] = prediction
    _require(projected.keys() == retained_targets.keys(), 'missing projected target key')

    rows, by_galaxy = [], {}
    for key in sorted(retained_targets):
        target, prediction = retained_targets[key], projected[key]
        vobs, errv = Fraction(target.vobs_km_s), Fraction(target.errv_km_s)
        observed = vobs * vobs
        residual = prediction.u_km2_s2 - observed
        relative = residual / observed
        nominal_speed = (max(Fraction(0), vobs - errv), vobs + errv)
        flags = []
        if errv / vobs >= threshold:
            flags.append('large_relative_speed_error')
        if nominal_speed[0] == 0:
            flags.append('nominal_speed_interval_reaches_zero')
        bound = prediction.absolute_error_bound
        row = {
            'key': {'galaxy': key.galaxy, 'radius_kpc': Fraction(key.radius_kpc)},
            'u_observed': observed, 'u_projected': prediction.u_km2_s2,
            'unit': SQUARE_SPEED_UNIT,
            'signed_residual': residual,
            'signed_relative_residual': relative,
            'signed_percentage_residual': 100 * relative,
            'absolute_relative_error': abs(relative),
            'absolute_percentage_error': 100 * abs(relative),
            'observed_speed': vobs, 'observed_speed_error': errv,
            'relative_speed_error': errv / vobs,
            'nominal_speed_interval': nominal_speed,
            'nominal_square_speed_interval': tuple(value * value for value in nominal_speed),
            'sigma_u_linear': 2 * vobs * errv,
            'linear_error_status': 'flagged' if flags else 'first_order_only',
            'uncertainty_flags': tuple(flags),
            'projection_uncertainty': {'status': 'unknown' if bound is None else 'bounded', 'bound': bound},
            'inclination_systematics': {'status': 'unknown', 'bound': None},
            'model_discrepancy': {'status': 'unknown', 'bound': None},
            'observation_map': prediction.observation_map,
            'source_evidence': prediction.source_evidence,
        }
        rows.append(row)
        by_galaxy.setdefault(key.galaxy, []).append(row)

    galaxies = {}
    for galaxy, members in sorted(by_galaxy.items()):
        count = len(members)
        galaxies[galaxy] = {
            'bins': count,
            'mape_percent': sum((row['absolute_percentage_error'] for row in members), Fraction(0)) / count,
            'mean_signed_percentage_residual': sum((row['signed_percentage_residual'] for row in members), Fraction(0)) / count,
            'underpredicted_bins': sum(row['signed_residual'] < 0 for row in members),
            'overpredicted_bins': sum(row['signed_residual'] > 0 for row in members),
            'equal_bins': sum(row['signed_residual'] == 0 for row in members),
            'flagged_bins': sum(bool(row['uncertainty_flags']) for row in members),
        }
    return {
        'format': SCORE_VERSION,
        'observable': 'square speed', 'unit': SQUARE_SPEED_UNIT,
        'bins': len(rows), 'galaxy_count': len(galaxies),
        'primary_aggregation': 'equal-bin',
        'primary_mape_percent': sum((row['absolute_percentage_error'] for row in rows), Fraction(0)) / len(rows),
        'equal_galaxy_mape_percent': sum((row['mape_percent'] for row in galaxies.values()), Fraction(0)) / len(galaxies),
        'large_relative_error_threshold': threshold,
        'uncertainty_note': 'Nominal measurement intervals and first-order square propagation exclude unsupplied systematics; flags do not remove targets.',
        'galaxies': galaxies, 'rows': tuple(rows),
    }
