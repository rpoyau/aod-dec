"""Declared support materialization and surplus routing for retained encounters.

The application supplies the count-to-load bridge, support constructions and
retention operands. The bound producer checks their witnesses. These choices
are neither an intrinsic D.E.C. engine law nor calibrated proton dynamics.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass, fields, is_dataclass, replace
from fractions import Fraction as F

from . import chiral_collision as chiral
from .vendor import aod_manual_native as native


ROUTES = ("endoshedding", "exoshedding", "redirected_support", "open_loss")
IMPLEMENTED_ROUTES = ("redirected_support", "exoshedding")


@dataclass(frozen=True)
class RoutingConfig:
    closure_cost: F = F(1)
    route: str = "redirected_support"
    seat_compatible: bool = True
    retention_ticks: int = 2
    window_ticks: int = 2

    def __post_init__(self):
        if type(self.closure_cost) not in (int, F) or self.closure_cost < 0:
            raise ValueError("closure cost requires a nonnegative exact integer or Fraction")
        object.__setattr__(self, "closure_cost", F(self.closure_cost))
        if self.route not in IMPLEMENTED_ROUTES:
            raise ValueError("this fixture implements redirected_support and exoshedding routes")
        if type(self.seat_compatible) is not bool:
            raise TypeError("seat compatibility requires a declared Boolean")
        if type(self.retention_ticks) is not int or self.retention_ticks < 0:
            raise ValueError("available retention must be a nonnegative integer count")
        if type(self.window_ticks) is not int or self.window_ticks <= 0:
            raise ValueError("route window must have a positive integer count")


def _bound_trace(lineage, occurrence, start, sign, namespace, *, scope, scale=0):
    """Bind immutable event identities while retaining a compatible ancestor."""
    trace = native.trace(lineage, occurrence, start_index=start, chirality=sign,
                         scope=scope, scale=scale)
    identifiers = tuple(namespace + ":" + event for event in trace.event_ids)
    reflection = replace(trace.reflection, turnaround_id=identifiers[trace.turn],
        outbound_ids=identifiers[1:trace.turn + 1], return_ids=identifiers[trace.turn + 1:],
        target_start_id=identifiers[0])
    return replace(trace, event_ids=identifiers, reflection=reflection)


def trace_address_records(trace):
    """Explicit finite octal address map bound to immutable trace events.

    Main supplies retained octal addressability, not this index encoding. Here
    a scoped trace namespace plus octal committed-cut digits identifies the
    mapped event. Q4 vertex bits and fibre roles remain separate coordinates.
    """
    native.validate_trace(trace)
    if any(cut < 0 for cut in trace.indices):
        raise ValueError("declared octal address encoding is unbound below 00_8")
    namespace = {"scope": trace.scope, "scale": trace.scale, "lineage": trace.lineage,
                 "trace_digest": trace.trace_digest}
    rows = tuple({"event_id": event, "event_ordinal": ordinal,
                  "committed_cut": cut,
                  "retained_address": {"base": 8, "digits": format(cut, "02o"),
                                       "namespace": namespace},
                  "q4_vertex": vertex, "q4_vertex_bits": format(vertex, "04b")}
                 for ordinal, (event, cut, vertex) in enumerate(
                     zip(trace.event_ids, trace.indices, trace.vertices)))
    return {"mapping": "declared trace namespace plus base-8 encoding of committed native cut",
            "source_anchor": "eq:first-anchor", "namespace": namespace, "rows": rows,
            "fibre_roles": trace.roles, "return_id": trace.return_id,
            "retained_start_event_id": trace.event_ids[0],
            "terminal_event_id": trace.event_ids[-1],
            "retained_start_address": rows[0]["retained_address"],
            "terminal_address": rows[-1]["retained_address"],
            "closure_witness_digest": native.digest(asdict(trace.reflection)),
            "scope_note": "Address, Q4 vertex, fibre role, event ordinal, scope and beat count are distinct; equal endpoint vertices do not erase event/address identity."}


def wave_observables(wave):
    """Counts on this admitted wave; no frequency or wavelength inference."""
    native.validate_wave(wave)
    closures = len(wave.returns)
    addresses = tuple(trace_address_records(trace) for trace in wave.returns)
    return {"identity": wave.identity, "scope": wave.scope, "scale": wave.scale,
            "core": wave.motif, "closures": closures, "beats": closures - 1,
            "octal_address_records": addresses,
            "beat_records": tuple({"beat_ordinal": ordinal, "return_id": trace.return_id,
                "completion_event_id": trace.event_ids[-1],
                "completion_address": address["terminal_address"]}
                for ordinal, (trace, address) in enumerate(zip(wave.returns, addresses)) if ordinal > 0),
            "completion_cuts": tuple(t.indices[-1] for t in wave.returns),
            "cadence": wave.cadence, "cadence_domain": "declared native construction-cut count",
            "atomic_traversals_per_return": tuple(native.phase_step_count(t) for t in wave.returns),
            "atomic_traversals": sum(native.phase_step_count(t) for t in wave.returns),
            "wavelength": None, "wavelength_status": "NOT_INSTANTIATED",
            "physical_proton_correspondence": "NOT_INSTANTIATED"}


def _carrier(source, path_id, carrier):
    """Materialize the supplied orientation sequence as declared 3:3 supports.

    Each support return has three distinct prior Monon component occurrences.
    Three ordered blocks reference that same bound triple, as in the Matter I
    producer. This is p:q support syntax, not three traversals on each leg.
    """
    native.validate_wave(source)
    lineage = "proton-facing:" + carrier
    records, motifs, maps = [], [], []
    cursor = 0
    for index, (source_trace, source_certificate) in enumerate(zip(source.returns, source.certificates)):
        sign = source_trace.roles[0]
        start = max(cursor, source_trace.indices[-1] + 1)
        namespace = ("routing-formation-v1:" + carrier + ":" + source.identity if index < 2
                     else "routing-continuation-v1:" + path_id + ":" + carrier)
        components = tuple(native.certified(_bound_trace(lineage, 4 * index + part,
            start + 3 * part, sign, namespace, scope=source.scope, scale=source.scale))
            for part in range(3))
        trace = _bound_trace(lineage, 4 * index + 3, start + 9, sign, namespace,
                             scope=source.scope, scale=source.scale)
        record = native.certified(trace)
        registry = {certificate.certificate_digest: certificate
                    for _, certificate, _ in (*components, record)}
        identifiers = tuple(t.return_id for t, _, _ in components)
        construction = (tuple(identifiers for _ in range(3)), identifiers)
        motif = native.motif_witness("3:3", trace, construction,
            tuple((t, c) for t, c, _ in components), registry, enclosure=6, chirality=sign)
        records.append(record)
        motifs.append(motif)
        maps.append({"ordinal": index, "role": "formation" if index < 2 else "selected_return",
            "source_identity": source.identity, "source_return_id": source_trace.return_id,
            "source_trace_digest": source_trace.trace_digest,
            "source_certificate_digest": source_certificate.certificate_digest,
            "source_committed": source_trace.indices[-1], "selected_chirality": sign,
            "construction_start": start, "component_records": components,
            "component_address_records": tuple(trace_address_records(t) for t, _, _ in components),
            "materialized_return_id": trace.return_id,
            "materialized_certificate_digest": record[1].certificate_digest,
            "motif_witness_digest": motif.witness_digest})
        cursor = trace.indices[-1] + 1
    wave = native.wave(records, motifs)
    return {"source_identity": source.identity, "materialized_identity": wave.identity,
            "wave": wave, "return_maps": tuple(maps),
            "projection_kind": "Declared orientation/history map into separately constructed 3:3 support",
            "core": "3:3", "facing_label": "proton-facing",
            "physical_proton_correspondence": "NOT_INSTANTIATED",
            "formation_returns": 2, "selected_returns": len(records) - 2,
            "component_atomic_traversals": sum(native.phase_step_count(t)
                for mapping in maps for t, _, _ in mapping["component_records"]),
            "observables": wave_observables(wave)}


def _route_support(source, path_id, start):
    """A separate admitted recurring Duon/E2 form from a retained Monon pivot."""
    pivot = (source.returns[-1], source.certificates[-1])
    sign = source.returns[-1].roles[0]
    records, motifs = [], []
    for index in range(2):
        trace = _bound_trace(source.lineage, 1000 + index, start + 3 * index,
            sign, "routing-form-v1:" + path_id, scope=source.scope, scale=source.scale)
        record = native.certified(trace)
        registry = {c.certificate_digest: c for c in (pivot[1], record[1])}
        construction = ((1, -1), (pivot[1].certificate_digest, sign), (-1, 1))
        motif = native.motif_witness("Duon", trace, construction, (pivot,), registry,
                                      enclosure=6, chirality=sign)
        records.append(record)
        motifs.append(motif)
    wave = native.wave(records, motifs)
    return {"wave": wave, "form": "E2", "core": "Duon", "enclosure": 6,
            "source_identity": source.identity, "pivot_return_id": pivot[0].return_id,
            "pivot_certificate_digest": pivot[1].certificate_digest,
            "status": "ADMITTED_DECLARED_FORM", "observables": wave_observables(wave)}


def _build(path, config):
    chiral.validate_path(path)
    if not path["witness_payloads_retained"]:
        path = chiral.simulate_path(path["config"], seed=path["seed"], retain_witnesses=True,
            ticket_denominator=path["ticket_denominator"], initial=path["initial"])
    carriers = {name: _carrier(wave, path["path_id"], name)
                for name, wave in zip(("a", "b"), path["final_waves"])}
    inner = tuple(carriers[name]["wave"] for name in ("a", "b"))
    source_state = replace(native.initial_state(inner[0]),
                           orientation=inner[0].returns[-1].roles[0])
    load = F(sum(path["flip_counts"]))
    residual = load - config.closure_cost
    surplus = max(F(0), residual)
    cursor = max(w.returns[-1].indices[-1] for w in inner) + 1
    support = None
    if surplus:
        if not config.seat_compatible or config.retention_ticks < config.window_ticks:
            raise ValueError("positive surplus requires compatible seat and sufficient declared retention")
        support = _route_support(inner[0], path["path_id"], cursor)
        cursor = support["wave"].returns[-1].indices[-1] + 1
    experiment_id = native.digest(chiral.exact_data({"path_id": path["path_id"], "routing": config}))
    outer_trace = native.outer_trace(inner, lineage="encounter-fold:" + experiment_id,
        scope="encounter-outer:" + experiment_id, scale=1, start_index=cursor)
    outer_record = native.certified(outer_trace, "outer", inner=inner)
    folded = native.fold(inner, outer_record)
    committed = outer_trace.indices[-1] + 1
    event_payload = {"path_id": path["path_id"], "carrier_identities": path["carrier_identities"],
        "window": {"start_exclusive": 0, "end_inclusive": len(path["steps"]),
                   "domain": "completed-return update index", "empty": not path["steps"]},
        "flip_counts": path["flip_counts"],
        "selected_events": tuple({"event_id": row["event_id"], "ordinal": row["ordinal"],
            "flipped": row["flipped"], "flip_tickets": row["flip_tickets"],
            "return_ids": tuple(w["return_id"] for w in row["return_witnesses"])}
            for row in path["steps"])}
    bridge_payload = {"name": "retained-flip-count-routing-v1", "load_rule": "L=F_A+F_B",
        "load_coordinate": "dimensionless count-derived model load; not pressure or energy",
        "cost_rule": "C=declared exact closure_cost", "closure_cost": config.closure_cost,
        "source_state_digest": native.digest(asdict(source_state)),
        "source_event_digest": native.digest(chiral.exact_data(event_payload)),
        "carrier_projection_ids": tuple(carriers[x]["materialized_identity"] for x in ("a", "b")),
        "routing_parameters": config}
    route = config.route if surplus else None
    target_scope = support["wave"].scope if support else None
    target_scale = support["wave"].scale if support else None
    record = native.shedding(folded, source_state, load, config.closure_cost,
        source_event_digest=native.digest(chiral.exact_data(event_payload)),
        model_bridge=native.digest(chiral.exact_data(bridge_payload)), committed=committed,
        route=route, target_scope=target_scope, target_scale=target_scale)
    partition = {name: surplus if name == route else F(0) for name in ROUTES}
    if sum(partition.values()) != surplus:
        raise ArithmeticError("surplus routing partition is not conserved")
    if route == "exoshedding" and native.expose(folded, record) != inner:
        raise ArithmeticError("exposure changed retained inner identities")
    routing = {"record": record, "source_state": source_state,
        "source_state_orientation_rule": "Orientation is the last retained selected return's chirality; formation orientation does not overwrite mutable state.",
        "source_event_payload": event_payload, "model_bridge_payload": bridge_payload,
        "route_support": support, "partition": partition, "partition_residual": F(0),
        "retention": {"status": "DECLARED_COMPATIBLE" if surplus else "NOT_APPLICABLE",
            "window": (committed, committed + config.window_ticks),
            "window_ticks": config.window_ticks, "available_ticks": config.retention_ticks,
            "domain": "declared native construction-cut count",
            "seat_compatible": config.seat_compatible,
            "rule": "The admitted route form remains available for the declared count after route commitment; this input is not derived from RCD or cadence."},
        "physical_energy_balance": "NOT_INSTANTIATED"}
    report = {"format": "aod-dec.collision-routing", "version": 1, "status": "PASS",
        "config": config, "source_path": path, "path_id": path["path_id"],
        "routing_experiment_id": experiment_id,
        "carriers": carriers, "observables": {name: carriers[name]["observables"] for name in ("a", "b")},
        "outer_fold": folded, "outer_address_records": trace_address_records(outer_trace), "routing": routing,
        "model_execution_boundary": "Application supplies support, load and route declarations; the unchanged producer checks witnesses.",
        "physical_proton_collision": "NOT_INSTANTIATED"}
    report["record_digest"] = native.digest(chiral.exact_data(report))
    return report


def build_routing_fixture(path, *, closure_cost=F(1), route="redirected_support",
                          seat_compatible=True, retention_ticks=2, window_ticks=2):
    """Materialize an exact sampled path; every returned payload is deterministic."""
    return _build(path, RoutingConfig(closure_cost, route, seat_compatible,
                                     retention_ticks, window_ticks))


def _same(left, right):
    if type(left) is not type(right):
        return False
    if is_dataclass(left):
        return all(_same(getattr(left, item.name), getattr(right, item.name)) for item in fields(left))
    if isinstance(left, dict):
        return left.keys() == right.keys() and all(_same(value, right[key]) for key, value in left.items())
    if isinstance(left, (list, tuple)):
        return len(left) == len(right) and all(_same(a, b) for a, b in zip(left, right))
    return left == right


def validate_routing_fixture(report):
    """Rebuild all source/payload/form/partition bindings, not just digest shapes."""
    if not isinstance(report, dict) or not isinstance(report.get("config"), RoutingConfig):
        raise TypeError("in-memory routing fixture and typed configuration required")
    rebuilt = _build(report["source_path"], report["config"])
    if not _same(report, rebuilt):
        raise ValueError("routing record differs from its retained path, model bridge or admission witnesses")
    return True
