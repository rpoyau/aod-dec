"""Frozen G0 and target-conditioned inverse diagnostics for SPARC preparation.

The optional effective-factor fit is an in-sample scalar diagnostic. Its factors
are neither a reconstructed density nor a native AOD field/beat history. The G0
baseline remains frozen; no target enters baseline_projections.
"""
from fractions import Fraction as Q
from collections.abc import Mapping

from simulations.sparc_inputs import BinKey, PreparationRow, TargetRow
from simulations.sparc_comparison import Projection, compare


G0_VERSION = 'sparc-g0-frozen-square-gas-v1'
INVERSE_VERSION = 'sparc-target-conditioned-effective-factor-v1'
DISK_ML = Q(1, 2)
BULGE_ML = Q(7, 10)
K8 = Q(8)


class InverseError(ValueError):
    pass


def _require(condition, message):
    if not condition:
        raise InverseError(message)


def _exact(value, label):
    _require(type(value) in (int, Q), label + ' requires exact int/Fraction; bool/float excluded')
    return Q(value)


def _key(key):
    _require(type(key) is BinKey, 'typed BinKey required')
    _require(type(key.galaxy) is str and bool(key.galaxy.strip()), 'nonempty galaxy identity required')
    _require(_exact(key.radius_kpc, 'radius') > 0, 'positive radius required')


def baseline_terms(row):
    """Frozen G0 law, with the gas sign squared exactly as in its baseline."""
    _require(type(row) is PreparationRow, 'typed preparation row required')
    _key(row.key)
    gas = _exact(row.vgas_km_s, 'gas component')
    disk = _exact(row.vdisk_km_s, 'disk component')
    bulge = _exact(row.vbul_km_s, 'bulge component')
    ubar = gas * gas + DISK_ML * disk * disk + BULGE_ML * bulge * bulge
    base = Q(row.key.radius_kpc) * (abs(gas) + abs(disk) + abs(bulge))
    return {'u_bar': ubar, 'base_dbf': base, 'u_g0': ubar + K8 * base}


def _preparation(preparation):
    _require(isinstance(preparation, Mapping) and bool(preparation), 'nonempty preparation mapping required')
    bound = dict(preparation)
    for key, row in bound.items():
        _key(key)
        _require(type(row) is PreparationRow and row.key == key, 'preparation key/row identity mismatch')
        baseline_terms(row)
    return bound


def _aligned(preparation, targets):
    prep = _preparation(preparation)
    _require(isinstance(targets, Mapping) and bool(targets), 'nonempty target mapping required')
    retained = dict(targets)
    _require(prep.keys() == retained.keys(), 'preparation/target key family differs')
    for key, row in retained.items():
        _require(type(row) is TargetRow and row.key == key, 'target key/row identity mismatch')
        _require(_exact(row.vobs_km_s, 'observed speed') > 0, 'positive observed speed required')
        _require(_exact(row.errv_km_s, 'observed speed error') >= 0, 'nonnegative observed speed error required')
    return prep, retained


def baseline_projections(preparation, *, source_evidence):
    """Project the unchanged G0 law from preparation alone; targets are absent."""
    prep = _preparation(preparation)
    return tuple(Projection(key, baseline_terms(prep[key])['u_g0'], G0_VERSION, source_evidence)
                 for key in sorted(prep))


def inverse_requirements(preparation, targets):
    """Retain even negative required contributions and all frozen target bins."""
    prep, retained = _aligned(preparation, targets)
    rows = []
    for key in sorted(prep):
        target = retained[key]
        terms = baseline_terms(prep[key])
        observed = Q(target.vobs_km_s) ** 2
        required = (observed - terms['u_bar']) / K8
        base = terms['base_dbf']
        speed_interval = (max(Q(0), Q(target.vobs_km_s) - target.errv_km_s),
                          Q(target.vobs_km_s) + target.errv_km_s)
        required_interval = tuple((value * value - terms['u_bar']) / K8 for value in speed_interval)
        if base == 0:
            factor, interval = None, None
            status = 'unidentified_zero_support' if required == 0 else 'infeasible_zero_support'
        else:
            factor, interval = required / base, tuple(value / base for value in required_interval)
            status = 'requires_negative_factor' if required < 0 else 'identified'
        rows.append({'key': {'galaxy': key.galaxy, 'radius_kpc': Q(key.radius_kpc)},
            **terms, 'u_observed': observed, 'required_dbf': required,
            'required_effective_factor': factor, 'factor_status': status,
            'nonnegative_factor_feasible': required >= 0 if base > 0 else required == 0,
            'nominal_speed_interval': speed_interval,
            'nominal_required_dbf_interval': required_interval,
            'nominal_required_factor_interval': interval,
            'unknown_model_and_systematic_uncertainty': None})
    return {'format': INVERSE_VERSION, 'baseline': G0_VERSION,
            'exposure': 'target-conditioned inverse diagnostic',
            'interpretation': 'Required effective contributions are not inferred density, native histories, or an independent prediction.',
            'law': 'required_DBF=(Vobs^2-Ubar)/8; required_factor=required_DBF/base_DBF',
            'interval_kind': 'nominal Vobs +/- errV mapped exactly; no confidence or systematic coverage claim',
            'rows': tuple(rows)}


def _median_minimizers(samples):
    """Exact weighted-median minimizer interval, intersected with f >= 0."""
    if not samples:
        return None, (Q(0), None)
    combined = {}
    for ratio, weight in samples:
        combined[ratio] = combined.get(ratio, Q(0)) + weight
    ordered = sorted(combined.items())
    total = sum(combined.values(), Q(0))
    cumulative = Q(0)
    for index, (ratio, weight) in enumerate(ordered):
        cumulative += weight
        if 2 * cumulative >= total:
            upper = ordered[index + 1][0] if 2 * cumulative == total else ratio
            lower, upper = max(Q(0), ratio), max(Q(0), upper)
            return lower, (lower, upper)
    raise InverseError('positive weighted-median family required')


def fit_effective_factors(preparation, targets, *, source_evidence):
    """Fit one nonnegative diagnostic factor per galaxy using all its targets.

    For b=8*base_DBF>0, each fractional loss is (b/Uobs)*|f-r|,
    r=(Uobs-Ubar)/b. The weighted median minimizes their convex sum exactly.
    Zero-support rows remain constant losses and stay in the reported score.
    """
    prep, retained = _aligned(preparation, targets)
    groups = {}
    for key in sorted(prep):
        groups.setdefault(key.galaxy, []).append(key)
    factors, projections = {}, []
    for galaxy, keys in sorted(groups.items()):
        samples = []
        for key in keys:
            terms = baseline_terms(prep[key])
            observed = Q(retained[key].vobs_km_s) ** 2
            b = K8 * terms['base_dbf']
            if b > 0:
                samples.append(((observed - terms['u_bar']) / b, b / observed))
        factor, minimizers = _median_minimizers(samples)
        factors[galaxy] = {'value': factor, 'minimizer_interval': minimizers,
            'status': 'unidentified_zero_support' if factor is None else 'target_conditioned_fit',
            'informative_bins': len(samples), 'total_bins': len(keys)}
        for key in keys:
            terms = baseline_terms(prep[key])
            contribution = Q(0) if factor is None else K8 * terms['base_dbf'] * factor
            projections.append(Projection(key, terms['u_bar'] + contribution,
                INVERSE_VERSION + ':in-sample-weighted-median-fit', source_evidence))
    projections = tuple(projections)
    return {'format': INVERSE_VERSION, 'baseline': G0_VERSION,
            'exposure': 'target-conditioned in-sample fit',
            'interpretation': 'Effective factors have no density or native-field interpretation; the frozen G0 baseline is unchanged.',
            'objective': 'equal-bin square-speed MAPE with one nonnegative effective factor per galaxy',
            'factors': factors, 'projections': projections,
            'comparison': compare(retained, projections)}
