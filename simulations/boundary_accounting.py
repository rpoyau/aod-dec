"""Exact, source-bound boundary accounting; no physical transition law.

Support IDs identify additive accounting occurrences, not shared construction
vertices. Source digests bind the separately admitted complete native witnesses.
Every per-cut projected contribution needs its own physical_id. Archive presence
never makes a record active. All checks remain enabled under Python -O.
"""
from dataclasses import dataclass
from fractions import Fraction
from contextlib import contextmanager
from contextvars import ContextVar


_commit_observer = ContextVar("aod_boundary_commit_observer", default=None)


@contextmanager
def observe_commits(observer):
    """Observe validated immutable deltas without adding model operations.

    The callback receives ``(kind, record)`` after validation. Storage failures
    propagate before a successor is returned, allowing an explicit incomplete
    run/checkpoint. Observer state is local to the active execution context.
    """
    if observer is not None and not callable(observer):
        raise TypeError("commit observer must be callable or None")
    token = _commit_observer.set(observer)
    try:
        yield
    finally:
        _commit_observer.reset(token)


def _observe(kind, record):
    observer = _commit_observer.get()
    if observer is not None:
        observer(kind, record)


STATUSES = frozenset(("active", "inactive", "pending", "routed", "removed"))
ROUTE_KINDS = frozenset(("endoshedding", "exoshedding", "redirected_support", "open_loss"))


def require(condition, message):
    if not condition:
        raise ValueError(message)


def exact(value):
    require(type(value) in (int, Fraction), "exact int/Fraction required; bool/float excluded")
    return Fraction(value)


def text(value, label):
    require(type(value) is str and bool(value.strip()), label + " must be a nonempty string")


def integer(value, label):
    require(type(value) is int and value >= 0, label + " must be a nonnegative exact integer")


def digest(value):
    require(type(value) is str and len(value) == 64 and all(c in "0123456789abcdef" for c in value),
            "bound SHA256 provenance required")


def identifiers(values, label, nonempty=False):
    require(type(values) is tuple and (bool(values) or not nonempty), label + " must be a tuple")
    for value in values:
        text(value, label)
    require(len(values) == len(set(values)), "duplicate " + label)


@dataclass(frozen=True)
class ContentRecord:
    identity: str
    physical_id: str
    source_digest: str
    committed: int
    scale: int
    unit: str
    value: Fraction
    support_ids: tuple
    parent_ids: tuple = ()

    def __post_init__(self):
        text(self.identity, "identity"); text(self.physical_id, "physical occurrence")
        digest(self.source_digest); integer(self.committed, "commitment"); integer(self.scale, "scale")
        text(self.unit, "unit"); object.__setattr__(self, "value", exact(self.value))
        identifiers(self.support_ids, "additive support", True)
        identifiers(self.parent_ids, "parent identity")
        require(self.scale == 0 or bool(self.parent_ids), "higher-scale content needs parent support provenance")


@dataclass(frozen=True)
class DispositionChange:
    identity: str
    before: str
    after: str
    reason: str
    restoration: bool = False

    def __post_init__(self):
        text(self.identity, "identity"); text(self.reason, "disposition reason")
        require(self.before in STATUSES and self.after in STATUSES, "unknown disposition")
        require(type(self.restoration) is bool, "restoration flag must be Boolean")
        require(self.before != self.after, "disposition change must change status")
        require(not (self.before == "removed" and self.after != "removed") or self.restoration,
                "structural removal requires explicit restoration; archive lookup is insufficient")
        require(not self.restoration or self.before == "removed", "restoration requires structural removal")


@dataclass(frozen=True)
class ArchiveCommit:
    cut: int
    additions: tuple
    changes: tuple


@dataclass(frozen=True)
class ArchiveState:
    commits: tuple = ()


def archive_view(state):
    require(type(state) is ArchiveState and type(state.commits) is tuple, "typed immutable archive required")
    records, statuses, previous_cut = {}, {}, -1
    for event in state.commits:
        require(type(event) is ArchiveCommit, "typed archive commitment required")
        integer(event.cut, "archive cut")
        require(event.cut > previous_cut, "archive cuts must increase strictly")
        require(type(event.additions) is tuple and type(event.changes) is tuple, "tuple archive operations required")
        touched = set()
        for item in event.additions:
            require(type(item) is tuple and len(item) == 2, "addition requires record and explicit disposition")
            record, status = item
            require(type(record) is ContentRecord, "typed source content required")
            record.__post_init__()
            require(status in STATUSES and record.identity not in records, "duplicate identity or unknown disposition")
            require(previous_cut < record.committed <= event.cut, "new record must belong to the new committed interval")
            for parent in record.parent_ids:
                require(parent in records, "missing parent support record")
                ancestor = records[parent]
                require(ancestor.committed < record.committed and ancestor.scale < record.scale,
                        "parent support must precede the higher-scale representation")
                require(set(ancestor.support_ids) <= set(record.support_ids), "higher-scale record dropped parent support")
            if record.parent_ids:
                inherited = {key for parent in record.parent_ids for key in records[parent].support_ids}
                require(set(record.support_ids) == inherited, "higher-scale support must equal the declared parent union")
            records[record.identity], statuses[record.identity] = record, status
            touched.add(record.identity)
        for change in event.changes:
            require(type(change) is DispositionChange, "typed disposition change required")
            change.__post_init__()
            require(change.identity in records, "disposition references missing archived content")
            require(change.identity not in touched, "multiple disposition operations at one cut")
            require(statuses[change.identity] == change.before, "stale old disposition")
            statuses[change.identity] = change.after
            touched.add(change.identity)
        previous_cut = event.cut
    return records, statuses, previous_cut


def commit_archive(state, cut, additions=(), changes=()):
    archive_view(state)
    result = ArchiveState(state.commits + (ArchiveCommit(cut, additions, changes),))
    validate_successor(state, result)
    _observe("accounting.commit", result.commits[-1])
    return result


def validate_successor(old, new):
    archive_view(old); archive_view(new)
    require(new.commits[:len(old.commits)] == old.commits and len(new.commits) >= len(old.commits),
            "archive history was dropped or rewritten")
    return True


@dataclass(frozen=True)
class BoundaryMotion:
    kind: str
    before: tuple
    after: tuple
    sweep: Fraction
    unit: str
    provenance: str

    def __post_init__(self):
        require(self.kind in ("stationary", "moving"), "explicit boundary motion declaration required")
        identifiers(self.before, "old region cell"); identifiers(self.after, "new region cell")
        text(self.unit, "sweep unit"); digest(self.provenance)
        object.__setattr__(self, "sweep", exact(self.sweep))
        if self.kind == "stationary":
            require(set(self.before) == set(self.after) and self.sweep == 0,
                    "stationary declaration requires unchanged region and explicit zero sweep")
        else:
            require(set(self.before) != set(self.after), "moving declaration requires changed finite region")


@dataclass(frozen=True)
class ScaleMap:
    record_id: str
    target_scale: int
    target_unit: str
    factor: Fraction
    provenance: str

    def __post_init__(self):
        text(self.record_id, "mapped record"); integer(self.target_scale, "target scale")
        text(self.target_unit, "target unit"); digest(self.provenance)
        object.__setattr__(self, "factor", exact(self.factor))
        require(self.factor > 0, "representation conversion factor must be positive")


@dataclass(frozen=True)
class BoundaryCoverage:
    cut: int
    boundary_id: str
    scale: int
    unit: str
    cross: tuple
    shedding: tuple
    scope: str
    window: int
    domain: str
    provenance: str

    def __post_init__(self):
        integer(self.cut, "coverage cut"); integer(self.scale, "coverage scale")
        text(self.boundary_id, "coverage boundary"); text(self.unit, "coverage unit")
        identifiers(self.cross, "expected crossing"); identifiers(self.shedding, "expected shedding")
        require(not set(self.cross).intersection(self.shedding), "coverage duplicates crossing as shedding")
        text(self.scope, "coverage scope"); integer(self.window, "coverage window")
        text(self.domain, "coverage count domain"); digest(self.provenance)


@dataclass(frozen=True)
class BoundarySlice:
    cut: int
    boundary_id: str
    scale: int
    unit: str
    cross: tuple
    shedding: tuple
    motion: BoundaryMotion
    scale_maps: tuple
    physical_ids: tuple
    cross_total: Fraction
    sweep_total: Fraction
    shedding_total: Fraction
    moving_total: Fraction
    archive: ArchiveState
    coverage: BoundaryCoverage
    stock_balance_status: str = "NOT_ASSERTED"


def account_slice(state, cut, boundary_id, scale, unit, *, cross=(), shedding=(), motion=None,
                  scale_maps=(), coverage=None):
    records, statuses, archived_cut = archive_view(state)
    integer(cut, "slice cut"); integer(scale, "boundary scale"); text(boundary_id, "boundary identity"); text(unit, "current unit")
    require(archived_cut <= cut, "slice cannot consume a future archive")
    require(type(motion) is BoundaryMotion, "explicit motion/sweep input required, including stationary zero")
    motion.__post_init__(); require(motion.unit == unit, "sweep/current domains differ")
    identifiers(cross, "crossing identity"); identifiers(shedding, "shedding identity")
    require(type(coverage) is BoundaryCoverage, "explicit source-bound boundary coverage required")
    coverage.__post_init__()
    require((coverage.cut, coverage.boundary_id, coverage.scale, coverage.unit) ==
            (cut, boundary_id, scale, unit), "coverage boundary context differs")
    require(cross == coverage.cross and shedding == coverage.shedding,
            "selected channels differ from source-bound boundary coverage")
    chosen = cross + shedding
    require(len(set(chosen)) == len(chosen), "same contribution classified as crossing and shedding")
    require(type(scale_maps) is tuple, "tuple scale maps required")
    maps = {}
    for mapping in scale_maps:
        require(type(mapping) is ScaleMap, "typed scale map required"); mapping.__post_init__()
        require(mapping.record_id in chosen and mapping.record_id not in maps, "unused or duplicate scale map")
        require(mapping.target_scale == scale and mapping.target_unit == unit, "scale map target differs from boundary")
        maps[mapping.record_id] = mapping
    supports, physical, values = set(), set(), {}
    for identity in chosen:
        require(identity in records, "missing source content: " + identity)
        record = records[identity]
        require(record.committed <= cut, "future source content")
        expected = "active" if identity in cross else "routed"
        require(statuses[identity] == expected, "content is not explicitly " + expected + ": " + identity)
        require(record.physical_id not in physical, "duplicate physical contribution across boundary classes")
        require(not supports.intersection(record.support_ids), "overlapping inner/outer additive support")
        physical.add(record.physical_id); supports.update(record.support_ids)
        factor = Fraction(1)
        require(record.unit == unit, "different quantity domains cannot be converted by a scale map")
        if identity in maps:
            factor = maps[identity].factor
        else:
            require(record.scale == scale and record.unit == unit, "unconverted scale or unit")
        values[identity] = record.value * factor
    crossing = sum((values[k] for k in cross), Fraction(0))
    shed = sum((values[k] for k in shedding), Fraction(0))
    return BoundarySlice(cut, boundary_id, scale, unit, cross, shedding, motion, scale_maps,
                         tuple(records[k].physical_id for k in chosen), crossing, motion.sweep,
                         shed, crossing - motion.sweep + shed, state, coverage)


def accumulate_current(slices):
    require(type(slices) is tuple and bool(slices), "nonempty tuple of committed slices required")
    first = slices[0]
    require(type(first) is BoundarySlice, "typed boundary slice required")
    previous, seen, seen_support, previous_region, previous_archive = -1, set(), set(), None, None
    for row in slices:
        require(type(row) is BoundarySlice and (row.boundary_id, row.scale, row.unit) ==
                (first.boundary_id, first.scale, first.unit), "incompatible boundary, scale or unit")
        integer(row.cut, "slice cut"); require(row.cut > previous, "window slices must be committed once in order")
        rebuilt = account_slice(row.archive, row.cut, row.boundary_id, row.scale, row.unit,
                                cross=row.cross, shedding=row.shedding, motion=row.motion,
                                scale_maps=row.scale_maps, coverage=row.coverage)
        require(type(row.cross_total) is Fraction and type(row.sweep_total) is Fraction
                and type(row.shedding_total) is Fraction and type(row.moving_total) is Fraction
                and row == rebuilt, "slice differs from recomputed exact source accounting")
        require((row.coverage.scope, row.coverage.domain) == (first.coverage.scope, first.coverage.domain),
                "incompatible boundary coverage universe or count domain")
        if previous_region is not None:
            require(set(row.motion.before) == previous_region, "disconnected boundary motion schedule")
            validate_successor(previous_archive, row.archive)
        require(not seen.intersection(row.physical_ids), "physical projection occurrence repeated across slices")
        require(len(row.physical_ids) == len(set(row.physical_ids)), "duplicate slice physical occurrence")
        records = archive_view(row.archive)[0]
        row_support = {support for identity in row.cross + row.shedding for support in records[identity].support_ids}
        require(not seen_support.intersection(row_support), "additive support occurrence repeated across slices")
        require(row.moving_total == row.cross_total - row.sweep_total + row.shedding_total,
                "moving-boundary accounting changed")
        require(row.stock_balance_status == "NOT_ASSERTED", "no stock-balance bridge supplied")
        seen.update(row.physical_ids); previous = row.cut
        seen_support.update(row_support)
        previous_region, previous_archive = set(row.motion.after), row.archive
    return {"boundary_id": first.boundary_id, "scale": first.scale, "unit": first.unit,
            "cuts": tuple(r.cut for r in slices),
            "cross": sum((r.cross_total for r in slices), Fraction(0)),
            "sweep": sum((r.sweep_total for r in slices), Fraction(0)),
            "shedding": sum((r.shedding_total for r in slices), Fraction(0)),
            "current": sum((r.moving_total for r in slices), Fraction(0)),
            "contribution_ids": tuple(k for r in slices for k in r.cross + r.shedding),
            "stock_balance_status": "NOT_ASSERTED"}


def project_window(rd, rcd, window, domain, window_domain, *, ttl=None, retention_window=None,
                   ttl_domain=None, retention_domain=None):
    for value, name in ((rd, "RD"), (rcd, "RCD duration"), (window, "participation window")):
        integer(value, name)
    text(domain, "RCD domain"); text(window_domain, "window domain")
    require(domain == window_domain, "incompatible RCD/window count domain")
    retained = None
    if any(x is not None for x in (ttl, retention_window, ttl_domain, retention_domain)):
        integer(ttl, "TTL"); integer(retention_window, "retention window")
        text(ttl_domain, "TTL domain"); text(retention_domain, "retention domain")
        require(ttl_domain == retention_domain, "incompatible TTL/retention-window domain")
        retained = ttl >= retention_window
    return {"RD": rd, "RCD_duration": rcd, "domain": domain, "window": window,
            "participation": min(rcd, window), "TTL": ttl, "retention_window": retention_window,
            "TTL_domain": ttl_domain, "retention_domain": retention_domain, "seat_retained": retained}


@dataclass(frozen=True)
class RouteLeg:
    identity: str
    kind: str
    amount: Fraction
    destination: str | None

    def __post_init__(self):
        text(self.identity, "route leg"); require(self.kind in ROUTE_KINDS, "unknown route kind")
        object.__setattr__(self, "amount", exact(self.amount)); require(self.amount > 0, "route legs carry positive amounts")
        if self.kind == "open_loss":
            require(self.destination is None, "open loss has no retained receiving destination")
        else:
            text(self.destination, "pending destination")


@dataclass(frozen=True)
class RouteRecord:
    identity: str
    source_id: str
    committed: int
    surplus: Fraction
    unit: str
    legs: tuple
    provenance: str

    def __post_init__(self):
        text(self.identity, "route"); text(self.source_id, "route source"); integer(self.committed, "route commitment")
        text(self.unit, "route burden unit"); digest(self.provenance)
        object.__setattr__(self, "surplus", exact(self.surplus)); require(self.surplus > 0, "route requires positive surplus")
        require(type(self.legs) is tuple and bool(self.legs), "retained route partition required")
        for leg in self.legs:
            require(type(leg) is RouteLeg, "typed route leg required"); leg.__post_init__()
        identifiers(tuple(leg.identity for leg in self.legs), "route leg")
        require(sum((leg.amount for leg in self.legs), Fraction(0)) == self.surplus,
                "route amounts do not partition surplus")


@dataclass(frozen=True)
class Receipt:
    identity: str
    route_id: str
    leg_id: str
    cut: int
    destination: str
    provenance: str


@dataclass(frozen=True)
class RouteLedger:
    routes: tuple = ()
    receipts: tuple = ()
    source_archives: tuple = ()


def _route_source(archive, route):
    records, statuses, cut = archive_view(archive)
    require(route.source_id in records, "missing route source record")
    source = records[route.source_id]
    require(statuses[route.source_id] == "routed", "route source is not explicitly routed")
    require(source.committed <= route.committed and cut <= route.committed, "route consumes a future archive")
    require(source.unit == route.unit and source.value == route.surplus, "surplus source/value domain mismatch")
    return source


def _route_view(ledger):
    require(type(ledger) is RouteLedger and type(ledger.routes) is tuple and type(ledger.receipts) is tuple,
            "immutable route ledger required")
    require(type(ledger.source_archives) is tuple and len(ledger.source_archives) == len(ledger.routes),
            "each route requires its immutable source archive")
    routes, legs, consumed, receipts = {}, {}, set(), set()
    sources, physical, supports = set(), set(), set()
    for route, archive in zip(ledger.routes, ledger.source_archives):
        require(type(route) is RouteRecord, "typed route record required"); route.__post_init__()
        require(route.identity not in routes and route.source_id not in sources, "route or surplus source reused")
        source = _route_source(archive, route)
        require(source.physical_id not in physical and not supports.intersection(source.support_ids),
                "physical/additive surplus source routed more than once")
        physical.add(source.physical_id); supports.update(source.support_ids)
        routes[route.identity] = route; sources.add(route.source_id)
        for leg in route.legs:
            require(leg.identity not in legs, "route leg identity reused")
            legs[leg.identity] = (route, leg)
    for receipt in ledger.receipts:
        require(type(receipt) is Receipt, "typed receipt required")
        text(receipt.identity, "receipt"); digest(receipt.provenance); integer(receipt.cut, "receipt cut")
        require(receipt.identity not in receipts and receipt.leg_id not in consumed, "receipt consumed more than once")
        require(receipt.leg_id in legs, "receipt lacks a retained route leg")
        route, leg = legs[receipt.leg_id]
        require(receipt.route_id == route.identity and receipt.destination == leg.destination
                and leg.kind != "open_loss", "receipt route or destination differs")
        require(receipt.cut > route.committed, "receipt must follow route commitment")
        consumed.add(receipt.leg_id); receipts.add(receipt.identity)
    return routes, legs, consumed


def register_route(ledger, archive, route):
    _route_view(ledger)
    require(type(route) is RouteRecord, "typed route record required"); route.__post_init__()
    _route_source(archive, route)
    result = RouteLedger(ledger.routes + (route,), ledger.receipts, ledger.source_archives + (archive,))
    validate_route_successor(ledger, result)
    _observe("accounting.route", route)
    return result


def receive_route(ledger, route_id, leg_id, cut, destination, receipt_id, provenance):
    _route_view(ledger)
    receipt = Receipt(receipt_id, route_id, leg_id, cut, destination, provenance)
    result = RouteLedger(ledger.routes, ledger.receipts + (receipt,), ledger.source_archives)
    validate_route_successor(ledger, result)
    _observe("accounting.receipt", receipt)
    return result


def pending_routes(ledger):
    _, legs, consumed = _route_view(ledger)
    return tuple({"route_id": route.identity, "leg_id": leg.identity, "destination": leg.destination,
                  "amount": leg.amount, "unit": route.unit, "committed": route.committed}
                 for identity, (route, leg) in legs.items() if identity not in consumed and leg.kind != "open_loss")


def validate_route_successor(old, new):
    _route_view(old); _route_view(new)
    require(new.routes[:len(old.routes)] == old.routes and len(new.routes) >= len(old.routes)
            and new.receipts[:len(old.receipts)] == old.receipts and len(new.receipts) >= len(old.receipts)
            and new.source_archives[:len(old.source_archives)] == old.source_archives,
            "route/receipt history was dropped or rewritten")
    return True
