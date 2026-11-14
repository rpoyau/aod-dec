"""Typed observation domains over retained events, without a scale evolution law.

The octal address follows supplied AOD sections/03_afc_basis.tex and appendix A.
A scale family enumerates declared formation evidence; it does not manufacture
that evidence, a metric, or a power-of-two relationship.
"""
from dataclasses import dataclass
from fractions import Fraction
import json


class ViewError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise ViewError(message)


def label(value, name):
    require(type(value) is str and bool(value.strip()), name + " must be nonempty")


def integer(value, name):
    require(type(value) is int, name + " must be an exact integer")


@dataclass(frozen=True)
class Window:
    start: int
    stop: int
    domain: str = "commit-ordinal"
    role: str = "observation"

    def __post_init__(self):
        integer(self.start, "start"); integer(self.stop, "stop")
        label(self.domain, "window domain")
        require(self.start <= self.stop, "window endpoints are reversed")
        require(self.role in ("observation", "acquisition", "comparison", "retention"),
                "unknown window role")

    def contains(self, value, domain="commit-ordinal"):
        integer(value, "window coordinate")
        require(domain == self.domain, "incompatible window coordinate domain")
        return self.start <= value < self.stop

    @classmethod
    def from_right_closed(cls, start, stop, domain="commit-ordinal", role="observation"):
        """Exact integer conversion of a producer interval (start, stop]."""
        integer(start, "start"); integer(stop, "stop")
        require(start <= stop, "window endpoints are reversed")
        return cls(start + 1, stop + 1, domain, role)


@dataclass(frozen=True)
class Duration:
    value: Fraction
    domain: str

    def __post_init__(self):
        require(type(self.value) in (int, Fraction), "duration must be exact")
        require(self.value >= 0, "duration must be nonnegative")
        label(self.domain, "duration domain")
        object.__setattr__(self, "value", Fraction(self.value))


def clip_participation(rd, rcd, rho, window):
    """Clip only the declared scalar participation, preserving original RD/RCD."""
    require(type(rho) is Duration and type(window) is Duration, "typed durations required")
    rho.__post_init__(); window.__post_init__()
    require(rho.domain == window.domain, "incompatible participation domains")
    return {"RD": rd, "RCD": rcd, "rho": rho,
            "rho_window": Duration(min(rho.value, window.value), rho.domain), "window": window}


@dataclass(frozen=True)
class FractalCoordinate:
    digits: tuple
    source_ref: str = "AOD:sections/03_afc_basis.tex#subsec:afc-fractal-address"
    family: str = "aod-retained-octal"

    def __post_init__(self):
        require(type(self.digits) is tuple and bool(self.digits), "retained octal digits required")
        require(all(type(d) is int and 0 <= d <= 7 for d in self.digits), "invalid octal digit")
        label(self.source_ref, "coordinate source")
        require(self.family == "aod-retained-octal", "undeclared coordinate family")

    @property
    def display(self):
        return "".join(map(str, self.digits)) + "_8"


@dataclass(frozen=True)
class ScaleLevel:
    index: int
    formation_locator: str
    parent_indices: tuple = ()

    def __post_init__(self):
        integer(self.index, "scale index")
        require(self.index >= 0, "negative formation level")
        label(self.formation_locator, "formation evidence locator")
        require(type(self.parent_indices) is tuple, "immutable parent indices required")
        require(all(type(i) is int and 0 <= i < self.index for i in self.parent_indices),
                "parent scales must precede outer scale")
        require(len(set(self.parent_indices)) == len(self.parent_indices), "repeated parent index")
        require(self.index == 0 or bool(self.parent_indices), "higher scale requires retained parents")


@dataclass(frozen=True)
class ScaleFamily:
    name: str
    source_ref: str
    levels: tuple

    def __post_init__(self):
        label(self.name, "scale family"); label(self.source_ref, "scale family source")
        require(type(self.levels) is tuple and bool(self.levels), "declared formation levels required")
        for level in self.levels:
            require(type(level) is ScaleLevel, "typed formation level required")
            level.__post_init__()
        indices = tuple(level.index for level in self.levels)
        require(indices == tuple(sorted(set(indices))), "formation levels must be distinct and ordered")
        require(all(p in indices for level in self.levels for p in level.parent_indices),
                "missing declared parent level")


@dataclass(frozen=True)
class OctaveRange:
    family: ScaleFamily
    low: int
    high: int

    def __post_init__(self):
        require(type(self.family) is ScaleFamily, "source-bound family required")
        self.family.__post_init__(); integer(self.low, "low octave"); integer(self.high, "high octave")
        require(0 <= self.low <= self.high, "invalid inclusive octave range")
        indices = {v.index for v in self.family.levels}
        require(self.low in indices and self.high in indices, "octave endpoints lack formation declarations")

    def contains(self, family, index):
        integer(index, "scale index")
        require(family == self.family.name, "incompatible octave family")
        return self.low <= index <= self.high and index in {v.index for v in self.family.levels}


@dataclass(frozen=True)
class Span:
    support_ids: tuple
    domain: str = "support-cells"

    def __post_init__(self):
        require(self.domain == "support-cells", "span is declared support extent, not duration")
        require(type(self.support_ids) is tuple, "immutable spatial support required")
        for value in self.support_ids:
            label(value, "support identity")
        require(len(set(self.support_ids)) == len(self.support_ids), "repeated support identity")


def select_events(events, *, window=None, coordinates=None, octaves=None, span=None, scope=None):
    """Streaming conjunction of distinct view dimensions; no mutation or replay."""
    if window is not None:
        require(type(window) is Window and window.role == "observation", "observation window required")
    if coordinates is not None:
        require(type(coordinates) is tuple and all(type(c) is FractalCoordinate for c in coordinates),
                "typed coordinate selection required")
    if octaves is not None:
        require(type(octaves) is OctaveRange, "typed octave range required")
    if span is not None:
        require(type(span) is Span, "typed support span required")
    for event in events:
        if window is not None and not window.contains(event["commit_ordinal"]):
            continue
        if scope is not None and event["scope_id"] != scope:
            continue
        if coordinates is not None and tuple(event["fractal_coordinate"]) not in {c.digits for c in coordinates}:
            continue
        if octaves is not None and not octaves.contains(event["scale_family"], event["scale_index"]):
            continue
        if span is not None:
            support = event.get("payload", {}).get("support_ids")
            require(type(support) in (tuple, list), "event has no declared support extent")
            if not set(support).intersection(span.support_ids):
                continue
        yield event


def union_views(*views, max_events=10000, max_bytes=8000000):
    """Occurrence union, never arithmetic addition of overlapping alternatives."""
    integer(max_events, "view capacity"); require(max_events > 0, "positive view capacity required")
    integer(max_bytes, "view byte capacity"); require(max_bytes > 0, "positive view byte capacity required")
    from .trace_store import canonical_bytes, TraceLimits, TraceError
    by_id = {}
    run_id = None
    retained_bytes = 0
    for view in views:
        for event in view:
            require(type(event) is dict, "event envelope required")
            if run_id is None:
                run_id = event["run_id"]
            require(event["run_id"] == run_id, "views belong to different occurrence families")
            key = event["event_id"]
            try:
                canonical = canonical_bytes(event, TraceLimits(max_event_bytes=min(max_bytes, 262144))).decode()
            except TraceError as exc:
                raise ViewError("view input exceeds exact byte/type budget: " + str(exc)) from exc
            if key in by_id:
                require(by_id[key][0] == canonical, "conflicting duplicate event")
            else:
                require(len(by_id) < max_events, "view capacity exceeded; refine or stream the query")
                require(retained_bytes + len(canonical.encode()) <= max_bytes, "view byte capacity exceeded")
                retained_bytes += len(canonical.encode())
                by_id[key] = (canonical, json.loads(canonical))
    return tuple(v[1] for v in sorted(by_id.values(), key=lambda item: item[1]["commit_ordinal"]))
