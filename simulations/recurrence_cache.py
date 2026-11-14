"""Bounded exact recurrence search for an already admitted finite quotient.

Repeated *complete quotient states* supply a cycle, including a transient prefix.
Equal boundary values alone do not. The cache retains the prefix and one cycle;
reuse checks the actual reducer, partition and complete update-domain binding.
This layer adds no orbital heuristic, physical octave or new transition law.
"""
from dataclasses import dataclass
from fractions import Fraction
import sys

from .scoped_reduction import (ExactReduction, UpdateDomain, ReductionError,
                              bounded, bounded_sum, digest, integer)


def _state(reducer, values):
    if type(values) not in (tuple, list) or len(values) != len(reducer.quotient):
        raise ReductionError("complete finite quotient state required")
    values = tuple(bounded(v, reducer.max_bits) for v in values)
    if any(v < 0 for v in values) or bounded_sum(values, reducer.max_bits) != 1:
        raise ReductionError("normalized nonnegative recurrence state required")
    return values


def _state_bytes(values):
    # Shared small integers are deliberately counted per reference. This is
    # conservative owned-object accounting, not an allocator/RSS measurement.
    return sys.getsizeof(values) + sum(sys.getsizeof(v) + sys.getsizeof(v.numerator)
                                      + sys.getsizeof(v.denominator) for v in values)


def _domain_bytes(domain, max_bits):
    total = sys.getsizeof(domain)
    for value in vars(domain).values():
        total += sys.getsizeof(value)
        for item in value if type(value) is tuple else ():
            if type(item) is int and item.bit_length() > max_bits:
                raise ReductionError("incomplete: domain index exceeds bigint budget")
            total += sys.getsizeof(item)
    return total


@dataclass(frozen=True, init=False)
class CycleReuse:
    """A checked exact prefix/cycle witness; no fine kernel is retained."""
    def __init__(self, reducer, states, transient, period, *, max_bytes=1024 * 1024):
        if type(reducer) is not ExactReduction:
            raise ReductionError("admitted immutable ExactReduction required")
        integer(transient, "transient"); integer(period, "period", 1)
        integer(max_bytes, "cache byte cap", 1)
        if type(states) is not tuple or len(states) != transient + period or len(states) > 100000:
            raise ReductionError("complete transient and one-cycle states required")
        reducer.domain.__post_init__()
        accounted_bytes = _domain_bytes(reducer.domain, reducer.max_bits) + 2048
        for values in states:
            if type(values) is not tuple or len(values) != len(reducer.quotient):
                raise ReductionError("complete typed quotient state required")
            for value in values:
                bounded(value, reducer.max_bits)
            accounted_bytes += 2 * _state_bytes(values) + 512
            if accounted_bytes > max_bytes:
                raise ReductionError("incomplete: retained cycle witness exceeds byte budget")
        checked = tuple(_state(reducer, q) for q in states)
        # This is a validation of every retained transition, including the wrap.
        # Public construction cannot promote an unexecuted state table to a cycle.
        for index, state in enumerate(checked):
            following = checked[index + 1] if index + 1 < len(checked) else checked[transient]
            if reducer.step(state, domain=reducer.domain, kernel_sha256=reducer.kernel_sha256) != following:
                raise ReductionError("recurrence witness does not follow the bound update")
        for key, value in dict(states=checked, transient=transient, period=period,
                domain=reducer.domain, kernel_sha256=reducer.kernel_sha256,
                partition_sha256=reducer.partition_sha256,
                quotient_sha256=digest(reducer.quotient), max_bits=reducer.max_bits,
                accounted_bytes=accounted_bytes).items():
            object.__setattr__(self, key, value)

    def state_at(self, step, *, reducer, domain):
        """Exact update offset from the admitted window's start, within that window."""
        integer(step, "reuse step")
        if step.bit_length() > self.max_bits:
            raise ReductionError("incomplete: reuse index exceeds bigint budget")
        if type(reducer) is not ExactReduction or type(domain) is not UpdateDomain:
            raise ReductionError("typed reducer and update domain required for reuse")
        domain.__post_init__()
        if (domain != self.domain or reducer.domain != self.domain or
                reducer.kernel_sha256 != self.kernel_sha256 or
                reducer.partition_sha256 != self.partition_sha256 or
                digest(reducer.quotient) != self.quotient_sha256 or reducer.max_bits != self.max_bits):
            raise ReductionError("reuse invalidated by kernel/partition/boundary/window/phase/coupling dependencies")
        if step >= self.domain.window[1] - self.domain.window[0]:
            raise ReductionError("reuse query is outside the admitted half-open window")
        index = step if step < self.transient else self.transient + (step - self.transient) % self.period
        return self.states[index]


@dataclass(frozen=True)
class CycleSearch:
    status: str
    cache: CycleReuse | None
    checked_steps: int
    validation_steps: int
    retained_states: int
    accounted_high_water_bytes: int
    max_steps: int
    max_bytes: int
    reason: str
    byte_scope: str = "Conservative owned-state/index/container accounting plus bounded arithmetic workspace reserve; not process RSS"


def find_cycle(reducer, initial, *, max_steps=1024, max_bytes=1024 * 1024):
    """Search exact state equality with explicit finite work and owned-memory caps.

    ``not_established`` means no cycle was reached within the search/window cap;
    it is not a proof of aperiodicity. ``incomplete`` names a resource limitation.
    The reducer's max_bits bounds input, update and query arithmetic.
    """
    if type(reducer) is not ExactReduction:
        raise ReductionError("admitted immutable ExactReduction required")
    integer(max_steps, "search update cap"); integer(max_bytes, "search byte cap", 1)
    if max_steps > 100000:
        raise ReductionError("search implementation supports at most 100000 updates")
    reducer.domain.__post_init__()
    window_span = reducer.domain.window[1] - reducer.domain.window[0]
    if window_span <= 0:
        raise ReductionError("recurrence search requires a nonempty admitted update window")
    history, index, used, checked_steps = [], {}, 0, 0
    # Three full bounded rational vectors plus temporary integer arithmetic.
    # This reserve is charged before executing any update or allocating an index.
    int_bytes = sys.getsizeof(0) + 4 * ((2 * reducer.max_bits + 1 + 29) // 30)
    vector_reserve = sys.getsizeof(()) + len(reducer.quotient) * (8 + sys.getsizeof(Fraction(0)) + 2 * int_bytes)
    entry_allowance = 2 * (sys.getsizeof({0: 0}) + sys.getsizeof([None]) + sys.getsizeof(0))
    try:
        fixed = _domain_bytes(reducer.domain, reducer.max_bits) + 3 * vector_reserve + 2048
        if fixed > max_bytes:
            return CycleSearch("incomplete", None, 0, 0, 0, 0, max_steps, max_bytes,
                               "byte cap cannot reserve one bounded update workspace")
        used = fixed
        current = _state(reducer, initial)
        limit = min(max_steps, window_span - 1)
        while True:
            old = index.get(current)
            if old is not None:
                witness = CycleReuse(reducer, tuple(history), old, checked_steps - old,
                                     max_bytes=max_bytes)
                return CycleSearch("verified_cycle", witness, checked_steps, len(history),
                    len(history), used, max_steps, max_bytes,
                    "Every prefix/cycle transition and the closing state equality were executed exactly")
            if checked_steps == limit:
                return CycleSearch("not_established", None, checked_steps, 0, len(history),
                    used, max_steps, max_bytes, "No exact repeated complete state within search/window cap")
            # Covers both search history/index and the immutable cache's copy
            # during admission, including container resize allowances.
            additional = 2 * _state_bytes(current) + entry_allowance
            if used + additional > max_bytes:
                return CycleSearch("incomplete", None, checked_steps, 0, len(history), used,
                                   max_steps, max_bytes, "retained recurrence state/index byte cap exhausted")
            used += additional
            index[current] = checked_steps
            history.append(current)
            current = reducer.step(current, domain=reducer.domain, kernel_sha256=reducer.kernel_sha256)
            checked_steps += 1
    except ReductionError as exc:
        if "budget" not in str(exc) and "incomplete" not in str(exc):
            raise
        return CycleSearch("incomplete", None, checked_steps, 0, len(history), used,
                           max_steps, max_bytes, str(exc))
