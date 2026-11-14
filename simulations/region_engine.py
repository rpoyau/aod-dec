"""Exact finite-cell boundary views of one immutable committed current history.

Inputs are already bound native currents or explicitly declared application
projections. This adapter supplies neither transition probabilities nor a
constitutive/force law. Crossings use AFTER membership. Moving sweep is derived
from a separately committed swept-current field, never guessed from cell count.
The initial adapter supports scale-zero current projections; higher native scale
admission requires the ancestral-support bridge of boundary_accounting.
"""
from dataclasses import asdict, dataclass, is_dataclass
from fractions import Fraction
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
from runtime.dec_core import observe_commits, _committed


ACCOUNTING_SHA256 = "5b2127b90e9527b00bfcea742b98e1bec60d13e344828a34a0c69e86ff713f78"
_sibling = Path(__file__).with_name("boundary_accounting.py")
_source = _sibling if _sibling.is_file() else (
    Path(__file__).resolve().parents[2] /
    "AOD_DEC_BOUNDARY_ACCOUNTING_2026-11-14/application/boundary_accounting.py")
if hashlib.sha256(_source.read_bytes()).hexdigest() != ACCOUNTING_SHA256:
    raise ImportError("boundary_accounting source differs from bound implementation")
_spec = importlib.util.spec_from_file_location("_aod_region_bound_accounting", _source)
accounting = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = accounting
_spec.loader.exec_module(accounting)
require, exact = accounting.require, accounting.exact


def serial(value):
    """Deterministic JSON-compatible representation, retaining exact rationals."""
    if isinstance(value, Fraction):
        return str(value)
    if is_dataclass(value):
        return serial(asdict(value))
    if isinstance(value, dict):
        return {str(k): serial(v) for k, v in value.items()}
    if isinstance(value, (tuple, list)):
        return [serial(v) for v in value]
    return value


def fingerprint(value):
    return hashlib.sha256(json.dumps(serial(value), sort_keys=True,
                                    separators=(",", ":")).encode()).hexdigest()


@dataclass(frozen=True)
class Region:
    name: str
    cells: tuple

    def __post_init__(self):
        accounting.text(self.name, "region name")
        accounting.identifiers(self.cells, "region cell")


def region_union(name, *regions):
    require(bool(regions), "union requires regions")
    for region in regions:
        require(type(region) is Region, "typed region required")
        region.__post_init__()
    return Region(name, tuple(sorted({c for r in regions for c in r.cells})))


def region_difference(name, outer, inner):
    require(type(outer) is Region and type(inner) is Region, "typed regions required")
    outer.__post_init__(); inner.__post_init__()
    require(set(inner.cells) <= set(outer.cells), "shell interior is not contained in outer region")
    return Region(name, tuple(sorted(set(outer.cells) - set(inner.cells))))


@dataclass(frozen=True)
class Channel:
    identity: str
    physical_id: str
    source: str
    target: str
    value: Fraction
    provenance: str
    kind: str = "cross"
    available: bool = True

    def __post_init__(self):
        for name in ("identity", "physical_id", "source", "target"):
            accounting.text(getattr(self, name), name)
        object.__setattr__(self, "value", exact(self.value))
        accounting.digest(self.provenance)
        require(self.kind in ("cross", "shedding"), "signed current channel kind required")
        require(type(self.available) is bool, "availability must be Boolean")


@dataclass(frozen=True)
class SweepSample:
    cell: str
    value: Fraction
    unit: str
    scale: int
    provenance: str

    def __post_init__(self):
        accounting.text(self.cell, "sweep cell"); accounting.text(self.unit, "sweep unit")
        accounting.integer(self.scale, "sweep scale"); accounting.digest(self.provenance)
        object.__setattr__(self, "value", exact(self.value))
        require(self.value >= 0, "swept-current measure must be nonnegative; membership sets its sign")


@dataclass(frozen=True)
class PendingProjection:
    identity: str
    physical_id: str
    source: str
    target: str
    amount: Fraction
    unit: str
    provenance: str

    def __post_init__(self):
        for name in ("identity", "physical_id", "source", "target", "unit"):
            accounting.text(getattr(self, name), name)
        accounting.digest(self.provenance)
        object.__setattr__(self, "amount", exact(self.amount))
        require(self.amount > 0, "unresolved burden must be positive")


@dataclass(frozen=True)
class CutRecord:
    cut: int
    window: int
    channels: tuple
    obligations: tuple
    sweep_field: tuple
    snapshot: str


def _payload(row):
    return (row.cut, row.window, row.channels, row.obligations, row.sweep_field)


@dataclass(frozen=True)
class Ledger:
    cells: tuple
    unit: str
    scale: int
    scope: str
    domain: str
    commits: tuple = ()

    def __post_init__(self):
        accounting.identifiers(self.cells, "declared universe cell", True)
        for name in ("unit", "scope", "domain"):
            accounting.text(getattr(self, name), name)
        accounting.integer(self.scale, "current scale")
        require(self.scale == 0, "higher native scales require an explicit ancestral-support bridge")
        require(type(self.commits) is tuple, "immutable commit tuple required")
        last, identities, physical = -1, set(), set()
        for row in self.commits:
            require(type(row) is CutRecord, "typed cut record required")
            accounting.integer(row.cut, "cut"); accounting.integer(row.window, "participation window")
            require(row.cut > last, "cuts must increase strictly")
            require(type(row.channels) is tuple and type(row.obligations) is tuple and
                    type(row.sweep_field) is tuple, "immutable cut inputs required")
            accounting.digest(row.snapshot)
            require(row.snapshot == fingerprint(_payload(row)), "committed source snapshot was altered or omitted")
            for event in row.channels + row.obligations:
                require(type(event) in (Channel, PendingProjection), "typed current or unresolved burden required")
                event.__post_init__()
                require(event.identity not in identities and event.physical_id not in physical,
                        "duplicate identity or physical occurrence in committed history")
                require(event.source in self.cells and event.target in self.cells,
                        "channel endpoint outside declared universe")
                identities.add(event.identity); physical.add(event.physical_id)
            require(all(type(event) is Channel for event in row.channels) and
                    all(type(event) is PendingProjection for event in row.obligations),
                    "burden and signed-current families must remain distinct")
            seen = set()
            for sample in row.sweep_field:
                require(type(sample) is SweepSample, "typed swept-current field required")
                sample.__post_init__()
                require(sample.cell in self.cells and sample.cell not in seen, "unknown or duplicate sweep cell")
                require((sample.unit, sample.scale) == (self.unit, self.scale), "sweep/current domain or scale mismatch")
                seen.add(sample.cell)
            last = row.cut


def validate_successor(old, new):
    require(type(old) is Ledger and type(new) is Ledger, "typed immutable ledgers required")
    old.__post_init__(); new.__post_init__()
    require((old.cells, old.unit, old.scale, old.scope, old.domain) ==
            (new.cells, new.unit, new.scale, new.scope, new.domain), "ledger context changed")
    require(new.commits[:len(old.commits)] == old.commits and len(new.commits) >= len(old.commits),
            "committed current history dropped or rewritten")
    return True


def commit(ledger, cut, window, channels, obligations=(), sweep_field=()):
    require(type(ledger) is Ledger, "typed ledger required")
    ledger.__post_init__()
    row = CutRecord(cut, window, channels, obligations, sweep_field,
                    fingerprint((cut, window, channels, obligations, sweep_field)))
    new = Ledger(ledger.cells, ledger.unit, ledger.scale, ledger.scope, ledger.domain,
                 ledger.commits + (row,))
    validate_successor(ledger, new)
    _committed("region.commit", (new, row))
    return new


@dataclass(frozen=True)
class Frame:
    before: Region
    after: Region
    kind: str
    sweep: tuple = ()

    def __post_init__(self):
        require(type(self.before) is Region and type(self.after) is Region, "typed before/after region required")
        self.before.__post_init__(); self.after.__post_init__()
        require(self.kind in ("stationary", "moving"), "explicit motion declaration required")
        require(type(self.sweep) is tuple, "explicit sweep tuple required")
        changed = set(self.before.cells) ^ set(self.after.cells)
        require((self.kind == "moving") == bool(changed), "motion declaration disagrees with region membership")
        require(all(type(s) is SweepSample for s in self.sweep), "typed sweep samples required")
        for sample in self.sweep:
            sample.__post_init__()
        require(len(self.sweep) == len({s.cell for s in self.sweep}) and
                {s.cell for s in self.sweep} == changed,
                "sweep coverage must equal exactly the entering and leaving cells")


@dataclass(frozen=True)
class RepresentationMap:
    source_scale: int
    target_scale: int
    unit: str
    factor: Fraction
    provenance: str

    def __post_init__(self):
        accounting.integer(self.source_scale, "representation source scale")
        accounting.integer(self.target_scale, "representation target scale")
        accounting.text(self.unit, "representation quantity unit")
        accounting.digest(self.provenance)
        object.__setattr__(self, "factor", exact(self.factor))
        require(self.factor > 0, "representation factor must be positive")


@dataclass(frozen=True)
class BoundaryAudit:
    name: str
    ledger: Ledger
    frames: tuple
    scope: str
    slices: tuple
    rows: tuple
    total: dict
    representation: RepresentationMap | None = None
    coverage_claim: str = "Complete relative to the supplied committed occurrence family; external producer completeness is not certified"


def _orientation(channel, cells):
    return int(channel.source in cells) - int(channel.target in cells)


def audit_boundary(ledger, name, frames, scope="declared-current", representation=None):
    """Project all declared channels; 'full' also requires no pending crossing map.

    Completeness is relative to this committed source family, not an attestation
    that an external native producer supplied every physical interaction.
    """
    require(type(ledger) is Ledger, "typed ledger required")
    ledger.__post_init__(); accounting.text(name, "boundary name")
    require(scope in ("declared-current", "full"), "explicit current coverage scope required")
    require(type(frames) is tuple and len(frames) == len(ledger.commits) and bool(frames),
            "exactly one frame for every committed input cut required")
    factor, target_scale = Fraction(1), ledger.scale
    if representation is not None:
        require(type(representation) is RepresentationMap, "typed explicit representation map required")
        representation.__post_init__()
        require((representation.source_scale, representation.unit) == (ledger.scale, ledger.unit),
                "representation source scale or quantity unit differs")
        factor, target_scale = representation.factor, representation.target_scale
    archive, slices, rows, previous, pending = accounting.ArchiveState(), [], [], None, []
    for cut, frame in zip(ledger.commits, frames):
        require(type(frame) is Frame, "typed frame required"); frame.__post_init__()
        before, after = set(frame.before.cells), set(frame.after.cells)
        require(before <= set(ledger.cells) and after <= set(ledger.cells), "region outside declared universe")
        if previous is not None:
            require(before == previous, "disconnected boundary motion schedule")
        previous = after
        field = {s.cell: s for s in cut.sweep_field}
        for sample in frame.sweep:
            require(sample.cell in field and sample == field[sample.cell],
                    "sweep sample differs from same-cut committed field")
        sweep = factor * sum((s.value * (1 if s.cell in after else -1) for s in frame.sweep), Fraction())
        pending.extend(cut.obligations)
        unresolved = tuple(o.identity for o in pending if _orientation(o, after))
        require(scope != "full" or not unresolved, "full current has unresolved signed shedding projection")
        additions, cross, shed, internal, exterior, inactive, views = [], [], [], [], [], [], []
        for event in cut.channels:
            orientation = _orientation(event, after)
            projected = orientation * event.value
            selected = bool(orientation) and event.available
            status = ("routed" if event.kind == "shedding" else "active") if selected else "inactive"
            record = accounting.ContentRecord(event.identity, event.physical_id, event.provenance,
                                              cut.cut, ledger.scale, ledger.unit, projected,
                                              (event.physical_id,))
            additions.append((record, status))
            if selected:
                (cross if event.kind == "cross" else shed).append(event.identity)
            if event.source in after and event.target in after:
                internal.append(event.identity)
            elif event.source not in after and event.target not in after:
                exterior.append(event.identity)
            if not event.available:
                inactive.append(event.identity)
            views.append({"identity": event.identity, "physical_id": event.physical_id,
                          "source": event.source, "target": event.target,
                          "orientation": orientation, "input_value": event.value,
                          "source_projected_value": projected if event.available else Fraction(),
                          "projected_value": factor * projected if event.available else Fraction(),
                          "kind": event.kind, "available": event.available})
        archive = accounting.commit_archive(archive, cut.cut, tuple(additions))
        coverage = accounting.BoundaryCoverage(cut.cut, name, target_scale, ledger.unit,
            tuple(cross), tuple(shed), ledger.scope, cut.window, ledger.domain,
            fingerprint((cut.snapshot, frame, scope, representation)))
        motion = accounting.BoundaryMotion(frame.kind, frame.before.cells, frame.after.cells,
                                            sweep, ledger.unit,
                                            fingerprint((cut.snapshot, frame.sweep, representation)))
        maps = () if representation is None else tuple(
            accounting.ScaleMap(identity, target_scale, ledger.unit, factor, representation.provenance)
            for identity in cross + shed)
        row = accounting.account_slice(archive, cut.cut, name, target_scale, ledger.unit,
            cross=tuple(cross), shedding=tuple(shed), motion=motion, coverage=coverage, scale_maps=maps)
        slices.append(row)
        rows.append({"cut": cut.cut, "window": cut.window, "snapshot": cut.snapshot,
                     "cross": row.cross_total, "sweep": row.sweep_total,
                     "shedding": row.shedding_total, "current": row.moving_total,
                     "internal_ids": tuple(internal), "exterior_ids": tuple(exterior),
                     "inactive_ids": tuple(inactive), "unresolved_projection_ids": unresolved,
                     "views": tuple(views), "stock_balance_status": "NOT_ASSERTED"})
    return BoundaryAudit(name, ledger, frames, scope, tuple(slices), tuple(rows),
                         accounting.accumulate_current(tuple(slices)), representation)


def validate_audit(audit):
    require(type(audit) is BoundaryAudit, "typed boundary audit required")
    rebuilt = audit_boundary(audit.ledger, audit.name, audit.frames, audit.scope, audit.representation)
    require(audit == rebuilt and fingerprint(audit) == fingerprint(rebuilt),
            "boundary audit differs from exact committed source reconstruction")
    return True


def audit_partition(parent, children):
    """Check additive disjoint regions against a parent using exactly one history."""
    validate_audit(parent)
    require(type(children) is tuple and bool(children), "nonempty child audit tuple required")
    for child in children:
        validate_audit(child)
        require(child.ledger == parent.ledger and child.scope == parent.scope
                and child.representation == parent.representation,
                "partition views do not share exact committed occurrence family and scope")
    residuals = []
    for index, parent_frame in enumerate(parent.frames):
        for side in ("before", "after"):
            seen = set()
            for child in children:
                cells = set(getattr(child.frames[index], side).cells)
                require(not seen.intersection(cells), "partition children overlap")
                seen.update(cells)
            require(seen == set(getattr(parent_frame, side).cells), "partition does not cover parent")
        row = {"cut": parent.rows[index]["cut"], "snapshot": parent.rows[index]["snapshot"]}
        for field in ("cross", "sweep", "shedding", "current"):
            value = sum((child.rows[index][field] for child in children), Fraction()) - parent.rows[index][field]
            require(value == 0, "partition boundary identity failed: " + field)
            row[field + "_residual"] = value
        residuals.append(row)
    return {"status": "PASS", "parent": parent.name, "children": tuple(c.name for c in children),
            "rows": tuple(residuals), "total_current_residual": Fraction(),
            "source_history_sha256": fingerprint(parent.ledger),
            "interpretation": "Same-occurrence internal cancellation; independent reverse channels remain separate"}
