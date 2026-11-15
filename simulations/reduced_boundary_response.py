"""Existing native response and an exact moving-boundary identity.

The application supplies pressure-bearing signed channels, positive state
pressures and boundary membership. This helper chooses none of those operands.
It calls the unchanged native updater with negative outward current. The
accounted scalar q=P*theta is a native response stock, not physical mass.

Pressure is fixed during one response. Crossing is evaluated on after-membership
and the symmetric-difference sweep uses PRE-response stock. Together these give
Delta q_R + cross_after - sweep_before + shedding_after = 0. A changed pressure,
unresolved unsigned burden or external endpoint needs a separately declared
source term; none is silently added, clipped or dropped here.
"""
from collections.abc import Mapping
from copy import deepcopy
from dataclasses import dataclass
from fractions import Fraction as Q

from simulations import region_engine as region
from simulations.vendor import aod_manual_native as native


UNIT = 'native-pressure-current'
WINDOW_DOMAIN = 'native-return-count'
STOCK_KIND = 'q=P*theta; native response stock, not physical mass'
_NO_AFTER = object()


class ResponseError(ValueError):
    pass


def _require(condition, message):
    if not condition:
        raise ResponseError(message)


def _exact(value, label):
    _require(type(value) in (int, Q), label + ' requires exact int/Fraction; bool/float excluded')
    return Q(value)


def _states(states):
    _require(isinstance(states, Mapping) and bool(states), 'nonempty cell/state mapping required')
    result = dict(states)
    cuts = set()
    for cell, state in result.items():
        _require(type(cell) is str and bool(cell.strip()), 'cell identity must be a nonempty string')
        _require(type(state) is native.WaveState, 'exact native WaveState required')
        _require(type(state.committed) is int and state.committed >= 0,
                 'nonnegative exact native commitment required')
        cuts.add(state.committed)
        pressure = _exact(state.pressure, 'state pressure')
        current = _exact(state.current, 'state current')
        _require(pressure > 0, 'positive state pressure required')
        _require(pressure * current >= 0, 'negative response stock is not clipped')
    _require(len(cuts) == 1, 'all response inputs must share one committed cut')
    return result, next(iter(cuts))


def _channels(channels, cells):
    _require(type(channels) is tuple, 'immutable signed channel tuple required')
    identities, physical = set(), set()
    for channel in channels:
        _require(type(channel) is region.Channel,
                 'signed region.Channel required; pending unsigned burden has no response projection')
        channel.__post_init__()
        _require(channel.source in cells and channel.target in cells,
                 'every channel endpoint requires a state, including explicit reservoirs')
        _require(channel.identity not in identities and channel.physical_id not in physical,
                 'duplicate channel identity or physical occurrence')
        identities.add(channel.identity)
        physical.add(channel.physical_id)
    return channels


def response_step(states, channels):
    """Apply only the existing native response to a complete endpoint family."""
    before, cut = _states(states)
    channels = _channels(channels, before)
    inputs = {cell: Q(0) for cell in before}
    for channel in channels:
        if channel.available:
            inputs[channel.source] -= channel.value
            inputs[channel.target] += channel.value
    after = {cell: native.update(state, inputs[cell], input_index=cut)
             for cell, state in before.items()}
    after, response_cut = _states(after)
    _require(response_cut == cut + 1, 'native response must be committed at the next cut')
    before_stock = {cell: Q(state.pressure) * Q(state.current) for cell, state in before.items()}
    after_stock = {cell: Q(state.pressure) * Q(state.current) for cell, state in after.items()}
    weighted = {cell: Q(before[cell].pressure) * (after[cell].current - before[cell].current)
                for cell in before}
    _require(weighted == inputs, 'native pressure-weighted response differs from bound channel incidence')
    residual = sum(after_stock.values(), Q(0)) - sum(before_stock.values(), Q(0))
    _require(residual == 0, 'complete endpoint response stock is not conserved')
    return {'before': before, 'after': after, 'cut': cut, 'response_cut': response_cut,
            'channels': channels, 'response_inputs': inputs,
            'before_stock': before_stock, 'after_stock': after_stock,
            'weighted_response': weighted, 'conservation_residual': residual,
            'unit': UNIT, 'stock_kind': STOCK_KIND}


@dataclass(frozen=True, slots=True, init=False)
class BoundaryContext:
    """One admitted response and source ledger, independent of boundary views.

    Construct with ``boundary_context``. Inputs and returned records are copied,
    so changing a caller's mappings or a prior view cannot change later views.
    The private snapshots contain no caller-owned mutable mappings.
    """
    _response: tuple
    _ledger: region.Ledger

    def __init__(self, *args, **kwargs):
        raise ResponseError('construct a validated context with boundary_context')

    def response(self):
        """Return the existing response_step record as an independent value."""
        result = {}
        maps = ('before', 'after', 'response_inputs', 'before_stock',
                'after_stock', 'weighted_response')
        for name, value in self._response:
            result[name] = deepcopy(dict(value) if name in maps else value)
        return result


def boundary_context(before, channels, *, scope, window, window_domain, after=_NO_AFTER):
    """Validate one response and admit its complete swept field only once.

    An optional externally computed ``after`` must equal the native response,
    including all pressure and closure context. Without it, callers obtain the
    computed states from ``context.response()`` before selecting passive views.
    """
    _require(type(scope) is str and bool(scope.strip()), 'explicit scope required')
    _require(type(window) is int and window >= 0, 'nonnegative exact participation window required')
    _require(type(window_domain) is str and window_domain == WINDOW_DOMAIN,
             'participation window requires the native-return-count domain')
    # Own the state/channel objects as well as the mappings. WaveState is frozen,
    # but copying also isolates any mutable ancillary metadata supplied by callers.
    checked, _ = _states(before)
    _channels(channels, checked)
    response = response_step(deepcopy(checked), deepcopy(channels))
    if after is not _NO_AFTER:
        actual, actual_cut = _states(after)
        _require(actual == response['after'] and actual_cut == response['response_cut'],
                 'after states differ from the bound native response or change pressure/context')
    cells = tuple(sorted(response['before']))
    sweep_field = tuple(region.SweepSample(cell, response['before_stock'][cell], UNIT, 0,
        region.fingerprint({'policy': 'pre-response-stock', 'cell': cell,
                            'cut': response['cut'], 'state': response['before'][cell], 'unit': UNIT}))
        for cell in cells)
    ledger = region.Ledger(cells, UNIT, 0, scope, window_domain)
    ledger = region.commit(ledger, response['cut'], window, response['channels'],
                           sweep_field=sweep_field)
    context = object.__new__(BoundaryContext)
    snapshot = tuple((name, tuple(value.items()) if type(value) is dict else value)
                     for name, value in response.items())
    object.__setattr__(context, '_response', snapshot)
    object.__setattr__(context, '_ledger', ledger)
    return context


def _region_cells(before_region, after_region, cells):
    _require(type(before_region) is region.Region and type(after_region) is region.Region,
             'typed before/after regions required')
    before_region.__post_init__()
    after_region.__post_init__()
    old_cells, new_cells = set(before_region.cells), set(after_region.cells)
    _require(old_cells <= cells and new_cells <= cells, 'region includes an unknown response cell')
    return old_cells, new_cells


def boundary_view(context, before_region, after_region):
    """Audit a passive region against the context's unchanged source family."""
    _require(type(context) is BoundaryContext, 'validated BoundaryContext required')
    old_cells, new_cells = _region_cells(before_region, after_region, set(context._ledger.cells))
    response = dict(context._response)
    before_stock, after_stock = dict(response['before_stock']), dict(response['after_stock'])
    # Keep the private admitted ledger isolated from output objects. This copy
    # does not reconstruct or re-commit the global field; the unchanged audit
    # validator still rechecks every view's exact immutable source family.
    ledger = deepcopy(context._ledger)
    changed = old_cells ^ new_cells
    sweep = tuple(sample for sample in ledger.commits[0].sweep_field if sample.cell in changed)
    frame = region.Frame(before_region, after_region, 'moving' if changed else 'stationary', sweep)
    audit = region.audit_boundary(ledger, ledger.scope, (frame,), scope='full')
    stock_before = sum((before_stock[cell] for cell in old_cells), Q(0))
    stock_after = sum((after_stock[cell] for cell in new_cells), Q(0))
    stock_change = stock_after - stock_before
    residual = stock_change + audit.rows[0]['current']
    _require(residual == 0, 'moving response-stock identity failed')
    return {'ledger': ledger, 'frame': frame, 'audit': audit,
            'cut': response['cut'], 'response_cut': response['response_cut'],
            'before_stock': before_stock, 'after_stock': after_stock,
            'regional_stock_before': stock_before, 'regional_stock_after': stock_after,
            'stock_change': stock_change, 'balance_residual': residual,
            'sweep_policy': 'pre-response-stock on entering/leaving cells',
            'crossing_policy': 'after-membership', 'unit': UNIT, 'stock_kind': STOCK_KIND}


def boundary_step(before, after, channels, before_region, after_region, *,
                  scope, window, window_domain):
    """Verify an external response and audit one supplied moving region.

    The one-cut ledger can later be assembled into a persistent history by the
    caller. This passive view never changes either state mapping. The explicit
    window-domain declaration must be supplied by the application's native
    participation-slot map; a bare integer cadence does not establish it.
    """
    checked, _ = _states(before)
    _region_cells(before_region, after_region, set(checked))
    context = boundary_context(before, channels, scope=scope, window=window,
                               window_domain=window_domain, after=after)
    return boundary_view(context, before_region, after_region)
