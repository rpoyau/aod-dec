"""Committed representative Duon histories for a reduced radial calculation.

This target-free finite preparation is not inferred galaxy particle formation.
One pivot closes at cut 2. Duon returns close at 5, 8 and 11; subsequent
events are committed individually. The resulting cadence is 3 native cuts.
Main/Manual ab8359209b8f9b2ef4007403101016e3ea58cfe4 anchors:
sections/04_cut_running_fractal_tesseract.tex:75-114 (returns and cadence),
sections/05_curl_closure_duon_current.tex:34-88 (Duon and support), and
sections/06_wave_state_relational_time.tex:53-152 (directional RCD/comparison).

Reflection counts are completed returns in two disjoint intervals ending at
the actual contact cuts. One native cut per participation slot is an explicit
reduced-model choice. Neither cadence nor this interface introduces SI time.
The frozen admitted-history container validates new closures through the
canonical producer. Advancing an open prefix reuses that immutable container;
it does not re-admit old closures or construct uncommitted future Trace bytes.
"""

from dataclasses import dataclass, replace
from fractions import Fraction as Q

from .vendor import aod_manual_native as native


PARTICIPATION_MAP = "one native cut per participation slot; declared finite reduction; no SI time"


class HistoryError(ValueError):
    pass


def _require(condition, message):
    if not condition:
        raise HistoryError(message)


def _text(value, name):
    _require(type(value) is str and bool(value.strip()), name + " must be a nonempty string")


@dataclass(frozen=True)
class Pivot:
    trace: native.Trace
    certificate: native.Certificate


@dataclass(frozen=True)
class PendingReturn:
    lineage: str
    occurrence: int
    # Only (immutable event identity, vertex, committed cut) triples exist here.
    events: tuple

    def __post_init__(self):
        _text(self.lineage, "lineage")
        _require(type(self.occurrence) is int and self.occurrence > 0,
                 "positive exact return occurrence required")
        _require(type(self.events) is tuple and len(self.events) in (1, 2),
                 "one or two actually committed pending events required")
        for event in self.events:
            _require(type(event) is tuple and len(event) == 3 and
                     type(event[0]) is str and bool(event[0]) and
                     type(event[1]) is int and type(event[2]) is int,
                     "pending event identity, vertex and cut require exact types")


@dataclass(frozen=True)
class _AdmittedHistory:
    scope: str
    lineages: tuple
    pivots: tuple
    waves: tuple

    def __post_init__(self):
        _text(self.scope, "scope")
        _require(type(self.lineages) is tuple and bool(self.lineages), "finite lineage tuple required")
        for lineage in self.lineages:
            _text(lineage, "lineage")
        _require(len(set(self.lineages)) == len(self.lineages), "duplicate lineage identity")
        _require(type(self.pivots) is tuple and type(self.waves) is tuple and
                 len(self.lineages) == len(self.pivots) == len(self.waves),
                 "one pivot and recurring wave per lineage required")
        try:
            native._validate_wave_collection(self.waves)
            for lineage, pivot, wave in zip(self.lineages, self.pivots, self.waves):
                _require(type(pivot) is Pivot, "typed retained pivot required")
                native.admitted(pivot.trace, pivot.certificate,
                                {pivot.certificate.certificate_digest: pivot.certificate})
                _require(pivot.trace == native.trace(lineage, 0, scope=self.scope, start_index=0),
                         "pivot differs from declared finite preparation")
                _require((wave.scope, wave.lineage, wave.scale, wave.motif) ==
                         (self.scope, lineage, 0, "Duon"), "wave scope/lineage/motif mismatch")
                _require(len(wave.returns) >= 3, "three committed Duon returns required")
                for occurrence, (trace, motif) in enumerate(zip(wave.returns, wave.motif_witnesses), 1):
                    _require(trace == native.trace(lineage, occurrence, scope=self.scope,
                                                   start_index=3 * occurrence),
                             "return differs from declared uniform finite preparation")
                    _require(motif.component_traces == (pivot.trace,) and
                             motif.component_certificates == (pivot.certificate,),
                             "Duon changed its carried pivot")
            _require(len({w.returns[-1].indices[-1] for w in self.waves}) == 1,
                     "representative histories must share a completed cut")
        except (native.AdmissionError, AttributeError, TypeError, IndexError) as exc:
            raise HistoryError(str(exc)) from exc

    @property
    def completed_cut(self):
        return self.waves[0].returns[-1].indices[-1]


@dataclass(frozen=True)
class DuonPool:
    history: _AdmittedHistory
    committed_cut: int
    pending: tuple = ()

    def __post_init__(self):
        _require(type(self.history) is _AdmittedHistory, "validated admitted history required")
        _require(type(self.committed_cut) is int, "exact integer committed cut required")
        phase = self.committed_cut - self.history.completed_cut
        _require(phase in (0, 1, 2), "cut extends beyond the supplied committed prefix")
        _require(type(self.pending) is tuple, "immutable pending tuple required")
        if phase == 0:
            _require(not self.pending, "completed cut cannot retain an unfinished return")
            return
        _require(len(self.pending) == len(self.lineages), "one pending prefix per lineage required")
        for lineage, wave, pending in zip(self.lineages, self.waves, self.pending):
            occurrence = len(wave.returns) + 1
            expected = tuple((f"{lineage}:{occurrence}:e{j}", j, 3 * occurrence + j)
                             for j in range(phase))
            _require(type(pending) is PendingReturn and type(pending.occurrence) is int and
                     (pending.lineage, pending.occurrence, pending.events) ==
                     (lineage, occurrence, expected), "pending prefix contains altered or future events")

    @property
    def scope(self):
        return self.history.scope

    @property
    def lineages(self):
        return self.history.lineages

    @property
    def waves(self):
        return self.history.waves

    @property
    def pivots(self):
        return self.history.pivots

    @property
    def metadata(self):
        return {"preparation": "representative uniform finite Duon reduction",
                "formation_claim": "certified finite preparation, not inferred galaxy microhistory",
                "participation_map": PARTICIPATION_MAP,
                "cadence_native_cuts": self.waves[0].cadence[0],
                "native_scale": 0, "si_time_map": None}

    def wave(self, lineage):
        _text(lineage, "lineage")
        _require(lineage in self.lineages, "unknown lineage identity")
        return self.waves[self.lineages.index(lineage)]


def _duon_return(pivot, lineage, occurrence, scope):
    trace = native.trace(lineage, occurrence, scope=scope, start_index=3 * occurrence)
    certificate = native.certify(trace)
    registry = {c.certificate_digest: c for c in (pivot.certificate, certificate)}
    construction = ((1, -1), (pivot.certificate.certificate_digest, 0), (-1, 1))
    motif = native.motif_witness("Duon", trace, construction,
        ((pivot.trace, pivot.certificate),), registry, enclosure=6, chirality=1)
    return trace, certificate, motif


def prepare(lineages, scope="B0"):
    """Construct actual pivot and three recurring Duon return witnesses."""
    _text(scope, "scope")
    _require(type(lineages) in (tuple, list) and bool(lineages), "nonempty finite lineage family required")
    lineages = tuple(lineages)
    for lineage in lineages:
        _text(lineage, "lineage")
    _require(len(set(lineages)) == len(lineages), "duplicate lineage identity")
    pivots, waves = [], []
    for lineage in lineages:
        trace = native.trace(lineage, 0, scope=scope, start_index=0)
        pivot = Pivot(trace, native.certify(trace))
        returns = [_duon_return(pivot, lineage, n, scope) for n in (1, 2, 3)]
        registry = {c.certificate_digest: c for _, c, _ in returns}
        wave = native.wave([(t, c, registry) for t, c, _ in returns],
                           tuple(m for _, _, m in returns))
        pivots.append(pivot)
        waves.append(wave)
    return DuonPool(_AdmittedHistory(scope, lineages, tuple(pivots), tuple(waves)), 11)


def advance(pool):
    """Commit one event per lineage; certify only after its return completes."""
    _require(type(pool) is DuonPool, "typed Duon pool required")
    pool.__post_init__()
    cut = pool.committed_cut + 1
    phase = cut - pool.history.completed_cut
    if phase < 3:
        pending = []
        for lineage, wave in zip(pool.lineages, pool.waves):
            occurrence = len(wave.returns) + 1
            events = tuple((f"{lineage}:{occurrence}:e{j}", j, 3 * occurrence + j)
                           for j in range(phase))
            pending.append(PendingReturn(lineage, occurrence, events))
        return DuonPool(pool.history, cut, tuple(pending))
    waves = []
    for lineage, wave, pivot, pending in zip(pool.lineages, pool.waves, pool.pivots, pool.pending):
        actual = pending.events + ((f"{lineage}:{pending.occurrence}:e2", 0, cut),)
        t, c, m = _duon_return(pivot, lineage, pending.occurrence, pool.scope)
        _require(actual == tuple(zip(t.event_ids, t.vertices, t.indices)),
                 "certificate differs from the actual committed prefix")
        # The new immutable history container performs full canonical admission.
        waves.append(replace(wave, returns=wave.returns + (t,),
                             certificates=wave.certificates + (c,),
                             motif_witnesses=wave.motif_witnesses + (m,)))
    return DuonPool(_AdmittedHistory(pool.scope, pool.lineages, pool.pivots, tuple(waves)), cut)


@dataclass(frozen=True)
class ReflectionInterval:
    left_open: int
    right_closed: int
    left_returns: int
    right_returns: int


@dataclass(frozen=True)
class ComparisonEvidence:
    left: str
    right: str
    source_cut: int
    window: tuple
    contacts: tuple
    reflection_intervals: tuple
    ratio: Q
    bias: Q
    lag: Q
    drift: Q
    rcd: native.RCD
    duration: int
    participation: int
    participation_map: str = PARTICIPATION_MAP


def channel_comparison(pool, left, right, bias=(Q(0), Q(0))):
    """Pure comparison; callers retain returned contact evidence per cycle.

    Contacts at u and v belong to [u,v]; reclosures are counted in (u,v].
    Reflection operands count returns in the two disjoint completed intervals
    ending at those contacts. Repeated calls preserve each contact identity.
    """
    _require(type(pool) is DuonPool, "typed Duon pool required")
    pool.__post_init__()
    a, b = pool.wave(left), pool.wave(right)
    _require(left != right, "distinct retained lineages required")
    _require(type(bias) in (tuple, list) and len(bias) == 2,
             "two exact oriented bias coordinates required")
    try:
        offsets = tuple(native.Q(value) for value in bias)
        cuts = tuple(t.indices[-1] for t in a.returns[-3:])
        intervals, contacts = [], []
        for u, v in zip(cuts, cuts[1:]):
            counts = tuple(sum(u < t.indices[-1] <= v for t in w.returns[1:]) for w in (a, b))
            intervals.append(ReflectionInterval(u, v, *counts))
            support = native.trace(f"{a.identity}|{b.identity}", v, scope=pool.scope,
                                   start_index=v - 2)
            contacts.append(native.Contact("reduced-contact:" + native.digest((a.identity, b.identity, v)),
                a.identity, b.identity, pool.scope, v, native.wave_prefix(a, v),
                native.wave_prefix(b, v), counts, support, native.certify(support)))
        window = (cuts[-2], cuts[-1])
        result = native.compare(a, b, tuple(contacts), window=window, bias=offsets)
        participation = native.participation(result["rcd"], window[1] - window[0],
                                           result["rcd"].domain)
    except (native.AdmissionError, TypeError, AttributeError, IndexError) as exc:
        raise HistoryError(str(exc)) from exc
    return ComparisonEvidence(left, right, cuts[-1], window, tuple(contacts), tuple(intervals),
        result["ratio"], result["bias"], result["lag"], result["drift"], result["rcd"],
        result["duration"], participation)
