"""Exact finite boundary reduction with a checked future-update contract.

This is a declared normalized AFC specialization. It is not an orbital law or
the pressure-weighted AOD current. Strong lumpability is checked on the complete
finite kernel; Stokes alone is never used as an evolution certificate.
"""
from dataclasses import dataclass
from fractions import Fraction as Q
import hashlib
import json


class ReductionError(ValueError):
    pass


def exact(value):
    if type(value) not in (int, Q):
        raise ReductionError('exact int/Fraction required; Boolean/float excluded')
    return Q(value)


def integer(value, name, minimum=0):
    if type(value) is not int or value < minimum:
        raise ReductionError(name + ' must be an exact integer in its domain')
    return value


def bounded(value, max_bits):
    if type(value) not in (int, Q):
        raise ReductionError('exact bounded value required')
    if type(value) is int:
        bits = value.bit_length()
    else:
        bits = max(value.numerator.bit_length(), value.denominator.bit_length())
    if bits > max_bits:
        raise ReductionError('incomplete: exact input/state exceeds bigint budget')
    return Q(value)


def bounded_sum(values, max_bits):
    result = Q(0)
    for value in values:
        value = bounded(value, max_bits)
        # Inputs <= B bits: addition allocates at most 2B+1 bit intermediates.
        # Check every partial sum before allowing another arithmetic operation.
        result = bounded(result + value, max_bits)
    return result


def digest(value):
    def encode(x):
        if type(x) is Q:
            return {'numerator': x.numerator, 'denominator': x.denominator}
        raise TypeError(type(x).__name__)
    return hashlib.sha256(json.dumps(value, default=encode, sort_keys=True,
                                    separators=(',', ':')).encode()).hexdigest()


def kernel(rows, *, max_states=4096, max_bits=4096):
    integer(max_states, 'max states', 1); integer(max_bits, 'max bits', 1)
    if not isinstance(rows, (tuple, list)) or not 0 < len(rows) <= max_states:
        raise ReductionError('kernel state budget/domain exceeded')
    n = len(rows)
    out = []
    for row in rows:
        if not isinstance(row, (tuple, list)) or len(row) != n:
            raise ReductionError('square kernel required')
        values = tuple(bounded(x, max_bits) for x in row)
        if any(x < 0 or max(x.numerator.bit_length(), x.denominator.bit_length()) > max_bits for x in values):
            raise ReductionError('negative weight or integer byte budget exceeded')
        if bounded_sum(values, max_bits) != 1:
            raise ReductionError('row must sum to exactly one')
        out.append(values)
    return tuple(out)


def memory(values, n):
    result = tuple(exact(x) for x in values)
    if len(result) != n or any(x < 0 for x in result) or sum(result) != 1:
        raise ReductionError('normalized nonnegative memory required')
    return result


def partition(blocks, n):
    if not isinstance(blocks, (tuple, list)) or not blocks:
        raise ReductionError('partition required')
    result = tuple(tuple(integer(i, 'state index') for i in block) for block in blocks)
    if any(not block for block in result) or sorted(i for block in result for i in block) != list(range(n)):
        raise ReductionError('partition must cover each state exactly once')
    return result


def aggregate(p, blocks):
    p = memory(p, len(p)); blocks = partition(blocks, len(p))
    return tuple(sum((p[i] for i in block), Q(0)) for block in blocks)


def advance(p, P):
    P = kernel(P); p = memory(p, len(P))
    return tuple(sum((p[i] * P[i][j] for i in range(len(p))), Q(0)) for j in range(len(p)))


def boundary(p, P, region):
    """Return both sides plus constituents; empty/full regions are admitted."""
    P = kernel(P); p = memory(p, len(P))
    region = tuple(integer(i, 'region index') for i in region)
    if len(set(region)) != len(region) or any(i >= len(P) for i in region):
        raise ReductionError('invalid region membership')
    R = frozenset(region)
    phi = tuple(tuple(p[i]*P[i][j] - p[j]*P[j][i] for j in range(len(P))) for i in range(len(P)))
    divergence = sum((sum(phi[i], Q(0)) for i in R), Q(0))
    crossing = sum((phi[i][j] for i in R for j in range(len(P)) if j not in R), Q(0))
    return {'divergence': divergence, 'boundary': crossing, 'phi': phi,
            'unit': 'normalized_memory_per_declared_update', 'region': region,
            'exact': True, 'constituents_preserved': True}


@dataclass(frozen=True)
class UpdateDomain:
    boundary_id: str
    window: tuple
    octave_family: str
    octave_range: tuple
    coupling_id: str
    phase_state: tuple
    pending_obligations: tuple = ()

    def __post_init__(self):
        for x in (self.boundary_id, self.octave_family, self.coupling_id):
            if type(x) is not str or not x:
                raise ReductionError('named update dependency required')
        for x in (self.window, self.octave_range):
            if type(x) is not tuple or len(x) != 2 or any(type(v) is not int for v in x) or x[0] > x[1]:
                raise ReductionError('ordered exact range required')
        if self.octave_range != (0, 0):
            raise ReductionError('finite reducer supports declared level 0 only; native octave formation is a separate contract')
        if type(self.phase_state) is not tuple or any(type(v) is not int for v in self.phase_state):
            raise ReductionError('exact retained phase state required')
        if type(self.pending_obligations) is not tuple or any(type(v) is not str for v in self.pending_obligations):
            raise ReductionError('immutable pending identities required')


@dataclass(frozen=True, init=False)
class ExactReduction:
    """Validated quotient retaining only executable block state after admission.

    For each source block B, sum(P[i,j], j in C) is identical for every i in B.
    Therefore summing p' over C equals sum_B q[B] Q[B,C]. Induction gives equality
    at every future update under this fixed kernel/domain. This says nothing
    about finer observables, changed coupling, or history-dependent kernels.
    """
    def __init__(self, P, blocks, domain, *, max_states=4096, max_bits=4096):
        if type(domain) is not UpdateDomain:
            raise ReductionError('declared update domain required')
        P = kernel(P, max_states=max_states, max_bits=max_bits)
        blocks = partition(blocks, len(P))
        quotient = []
        for block in blocks:
            reference = tuple(bounded_sum((P[block[0]][j] for j in target), max_bits) for target in blocks)
            for i in block[1:]:
                actual = tuple(bounded_sum((P[i][j] for j in target), max_bits) for target in blocks)
                if actual != reference:
                    raise ReductionError('not lumpable: equal present boundary does not determine future')
            quotient.append(reference)
        for key, value in dict(quotient=tuple(quotient), kernel_sha256=digest(P),
                               partition_sha256=digest(blocks), domain=domain,
                               fine_states=len(P),block_sizes=tuple(map(len, blocks)),max_bits=max_bits).items():
            object.__setattr__(self,key,value)
        object.__setattr__(self,'proof',{'kind': 'complete_finite_blockwise_equality',
                      'checked_rows': len(P), 'checked_target_blocks': len(blocks),
                      'kernel_sha256': self.kernel_sha256,
                      'partition_sha256': self.partition_sha256,
                      'observable': 'block memory and quotient boundary current',
                      'arithmetic_bits': {'retained':max_bits,'temporary_upper_bound':2*max_bits+1}})

    def step(self, q, *, domain, kernel_sha256):
        if domain != self.domain or type(domain) is not UpdateDomain or kernel_sha256 != self.kernel_sha256:
            raise ReductionError('reuse invalidated: boundary/window/scale/phase/coupling/kernel changed')
        if not isinstance(q,(tuple,list)) or len(q)!=len(self.quotient):
            raise ReductionError('bounded quotient state required')
        q=tuple(bounded(x,self.max_bits) for x in q)
        if any(x<0 for x in q) or bounded_sum(q,self.max_bits)!=1:
            raise ReductionError('normalized quotient memory required')
        values=tuple(bounded_sum((bounded(q[i]*self.quotient[i][j],self.max_bits)
                                 for i in range(len(q))),self.max_bits) for j in range(len(q)))
        return values

    def recurrence(self, q):
        """One-update recurrence is computed, never inferred from a body label."""
        future = self.step(q, domain=self.domain, kernel_sha256=self.kernel_sha256)
        q = tuple(bounded(x,self.max_bits) for x in q)
        return {'period': 1 if q == future else None,
                'status': 'verified_fixed_state' if q == future else 'not_established',
                'state': q, 'future': future,
                'reuse_dependencies': self.proof | {'domain': vars(self.domain)}}


def finite_scope(detail=2, steps=7):
    """Nested interior/sensor finite demo; no Sun/Mercury physical assignment.

    Each of detail interior cells sends half its memory to the sensor and half
    uniformly within the interior. Sensor does likewise. The closed two-block
    kernel is checked before fine data can be discarded.
    """
    integer(detail, 'detail', 1); integer(steps, 'steps')
    if detail > 256 or steps > 10000:
        raise ReductionError('demonstration budget exceeded')
    row = (Q(1, 2*detail),)*detail + (Q(1, 2),)
    P = (row,)*(detail+1)
    blocks = (tuple(range(detail)), (detail,))
    domain = UpdateDomain('interior/sensor', (0, steps+1), 'finite-partition-not-physical-octave',
                          (0, 0), 'fixed-coupling', (0,), ())
    reducer = ExactReduction(P, blocks, domain)
    p = (Q(1),) + (Q(0),)*detail
    q = aggregate(p, blocks)
    trajectory = []
    for tick in range(steps+1):
        b = boundary(p, P, blocks[0])
        reduced_boundary = boundary(q, reducer.quotient, (0,))['boundary']
        if aggregate(p, blocks) != q or b['boundary'] != reduced_boundary:
            raise ReductionError('fine/reduced execution disagreement')
        trajectory.append({'tick': tick, 'block_memory': q, 'fine_boundary': b['boundary'],
                           'reduced_boundary': reduced_boundary, 'equal': True})
        if tick < steps:
            p = advance(p, P)
            q = reducer.step(q, domain=domain, kernel_sha256=reducer.kernel_sha256)
    return {'kind': 'exact_finite_scope_demonstration', 'detail': detail, 'steps': steps,
            'trajectory': trajectory, 'proof': reducer.proof, 'domain': vars(domain),
            'recurrence': reducer.recurrence(q), 'fine_nodes': detail+1,
            'reduced_nodes': 2, 'reduced_edges': 4,
            'physical_orbit': 'unavailable: no bound celestial evolution/preparation in this fixture',
            'exactness': 'exact_within_declared_finite_kernel'}


def approximate_step(q, P, denominator, previous_l1_bound=Q(0)):
    """Optional rational grid projection with an explicit L1 error bound.

    A stochastic matrix contracts L1. Round the first n-1 coordinates down and
    put the remainder in the last: added L1 error <= 2(n-1)/denominator. The
    accumulated bound concerns this finite model, not model/anchor uncertainty.
    """
    integer(denominator, 'grid denominator', 1)
    old = exact(previous_l1_bound)
    if old < 0: raise ReductionError('negative prior approximation bound')
    full = advance(q, P)
    rounded = tuple(Q((x.numerator*denominator)//x.denominator, denominator) for x in full[:-1])
    rounded += (1-sum(rounded),)
    bound = min(Q(2), old + Q(2*(len(full)-1), denominator))
    return rounded, bound


@dataclass(frozen=True, init=False)
class FixedPointReuse:
    """One retained fixed point may skip updates only under the bound dependencies.

    Qq=q is calculated at admission. Induction then preserves q for arbitrarily
    many updates of that same fixed kernel; any boundary/coupling/phase/window
    change invalidates this cache. This is not a slow-body heuristic.
    """
    def __init__(self, reducer, state):
        if type(reducer) is not ExactReduction:
            raise ReductionError('validated exact reduction required')
        record=reducer.recurrence(state)
        if record['period']!=1:
            raise ReductionError('no established recurrence for reuse')
        object.__setattr__(self,'state',record['state'])
        object.__setattr__(self,'domain',reducer.domain)
        object.__setattr__(self,'kernel_sha256',reducer.kernel_sha256)

    def advance(self, *, domain, kernel_sha256):
        if type(domain) is not UpdateDomain or domain!=self.domain or kernel_sha256!=self.kernel_sha256:
            raise ReductionError('reuse invalidated by changed dependencies')
        return {'state':self.state,'cache_reused':True,'refresh_required':False,
                'law':'verified fixed-point induction under unchanged finite kernel'}
