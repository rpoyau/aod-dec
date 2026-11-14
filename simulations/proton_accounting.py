#!/usr/bin/env python3
"""Proton-associated 3:3:6 accounting with actual retained encounter witnesses.

The source continuation law is unchanged. Its 1:2 histories supply explicitly
mapped preparation orientations for new admitted 3:3 C6 carriers. Main/Manual
species association, contact-channel accounting and mass metrology stay typed.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
from contextvars import ContextVar
from dataclasses import asdict, dataclass, is_dataclass, replace
from fractions import Fraction as Q
import gzip
import hashlib
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from simulations import retained_continuation as encounter
from simulations import collision_routing as support
from simulations.vendor import aod_manual_native as native
from simulations import boundary_accounting as accounting

MANUSCRIPT_BINDING = {'source_base_commit': '8c3149b9d3d924b80b9385b178da4ba2c39175e8',
 'candidate': 'R04 Main/Manual working source; recorded base and candidate bytes are distinct',
 'verification': 'Recorded by the original source-bound review; manuscript files are not reopened '
                 'by this portable runner',
 'files': {'manual/sections/01_rest_energy_prediction.tex': '40e6af142bfca8d4d97f8f48c50337f7c7d0099954d4e349dea09a9a76c40469',
           'manual/sections/04_boundary_current_applications.tex': '6d517127cb599c502cbeb06deed09451495e4a824f2c93eaaa5a7076a72c37b6',
           'sections/06_field.tex': '74f2b76195fe5dd760540cdcf46a72f615022950ab07e38e4585ad77b643de88',
           'sections/06_wave_state_relational_time.tex': '2add34566c97406939bfa6c67b1d6acef99ad13c22422ec5edc0800df7e52d2b'}}
NATIVE_SHA = "562b3eb53ad6dd13c0801c43bd6e918110083ab4d12592c024b3007b634e2554"

ACCOUNTING_SHA = "5b2127b90e9527b00bfcea742b98e1bec60d13e344828a34a0c69e86ff713f78"
ENCOUNTER_SHA = "5f4ec3ea81a0fc0b7b5f36dcc27bb3faf60a5894ef12eb74cc0a929c75364472"
SCOPE = "manual-proton-pair:declared-preparation"
CURRENT_UNIT = "native-pressure-current"
BURDEN_UNIT = "closure-burden"
WINDOWS = (2, 4, 6, 12)
CAPACITY = Q(3)
CLOSURE_COST = Q(24)
C6 = (0, 1, 3, 7, 6, 4, 0)


@dataclass
class NativeObservation:
    callback: object
    failure_count: int = 0
    last_error: str | None = None


_native_observation = ContextVar("proton_native_observation", default=None)


@contextmanager
def observe_commits(callback):
    """Passively capture admitted native witnesses with isolated observer errors.

    These are retained formation snapshots, not a new physical event at capture
    time. Native event identities and committed indices remain in every trace.
    A callback receives a fresh serialization tree and cannot alter the model's
    immutable witnesses or mutable preparation reports through that tree.
    """
    if callback is not None and not callable(callback):
        raise TypeError("native observer must be callable or None")
    observation = NativeObservation(callback)
    token = _native_observation.set(observation)
    try:
        yield observation
    finally:
        _native_observation.reset(token)


def _committed(kind, value):
    observation = _native_observation.get()
    if observation is not None and observation.callback is not None and not observation.failure_count:
        try:
            observation.callback(kind, encode(value))
        except Exception as exc:
            observation.failure_count += 1
            observation.last_error = type(exc).__name__ + ": " + str(exc)[:2048]


def _observe_native_wave(wave, family):
    """Expose each actual return once, retaining its bound construction evidence."""
    observation = _native_observation.get()
    if observation is None or observation.callback is None or observation.failure_count:
        return
    metadata = {"identity": wave.identity, "scope": wave.scope, "scale": wave.scale,
                "lineage": wave.lineage, "declared_motif": wave.motif}
    for ordinal, (trace, certificate) in enumerate(zip(wave.returns, wave.certificates)):
        motif = wave.motif_witnesses[ordinal] if wave.motif_witnesses else None
        _committed("particle.native_return", {
            "schema": "native-particle-return-v1", "family": family,
            "capture": "validated-retained-snapshot", "wave": metadata,
            "return_ordinal": ordinal, "trace": trace,
            "certificate": certificate, "motif_witness": motif,
            "formation": None if motif is None else {
                "construction": motif.construction, "component_traces": motif.component_traces,
                "component_certificates": motif.component_certificates}})


def _encounter_history(world):
    """Retained identities and dispositions, including inactive/pending history."""
    return {
        "population": tuple({"identity": wave.identity, "scope": wave.scope,
            "lineage": wave.lineage, "scale": wave.scale, "declared_motif": wave.motif,
            "return_ids": tuple(trace.return_id for trace in wave.returns),
            "trace_digests": tuple(trace.trace_digest for trace in wave.returns),
            "certificate_digests": tuple(cert.certificate_digest for cert in wave.certificates)}
            for wave in world.waves),
        "component_return_ids": tuple(tuple(trace.return_id for trace, cert in records)
                                      for records in world.components),
        "contacts": tuple({"identity": pair.identity, "tick": pair.tick,
            "committed": pair.committed, "contact_ids": tuple(contact.identity for contact in pair.contacts),
            "reflection_occurrences": pair.reflection_occurrences, "prefix_counts": pair.prefix_counts}
            for pair in world.contacts),
        "relations": tuple({"identity": relation.identity, "target": relation.target,
            "created_tick": relation.created_tick, "input_cut": relation.input_cut,
            "retained_at": relation.retained_at, "contact_ids": relation.contact_ids,
            "value": relation.value, "RCD": relation.rcd,
            "support_units": relation.support_units, "unmatched_returns": relation.unmatched_returns,
            "token_count": len(relation.tokens), "tokens_digest": digest(relation.tokens),
            "active": (relation.created_tick in (world.state[0]-1, world.state[0]-2)
                       and relation.identity not in world.disabled_relations)}
            for relation in world.relations),
        "disabled_relations": world.disabled_relations, "interventions": world.interventions}


def _encounter_context(schema):
    return {"schema": schema, "capture": "validated-source-execution",
        "scope": encounter.SCOPE, "lineage": "retained-continuation-world", "scale": 0,
        "source_core": "1:2", "population_role": "source encounter before proton materialization",
        "source_bindings": {"simulations/retained_continuation.py": file_sha(encounter.__file__),
                            "simulations/vendor/aod_manual_native.py": file_sha(native.__file__)}}


def _observe_encounter(kind, build_payload):
    """Isolate snapshot construction as well as callback failures from the model."""
    observation = _native_observation.get()
    if observation is None or observation.callback is None or observation.failure_count:
        return
    try:
        _committed(kind, build_payload())
    except Exception as exc:
        observation.failure_count += 1
        observation.last_error = type(exc).__name__ + ": " + str(exc)[:2048]


def require(ok, message):
    if not ok:
        raise ValueError(message)


def encode(value):
    if is_dataclass(value):
        return encode(asdict(value))
    if isinstance(value, Q):
        return str(value)
    if isinstance(value, dict):
        return {str(k): encode(v) for k, v in value.items()}
    if isinstance(value, (tuple, list)):
        return [encode(v) for v in value]
    if isinstance(value, float):
        raise TypeError("binary floating-point value in exact example")
    return value


def packed(value):
    return json.dumps(encode(value), sort_keys=True, separators=(",", ":")).encode()


def digest(value):
    return hashlib.sha256(packed(value)).hexdigest()


def file_sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def rejected(name, action):
    try:
        action()
    except (ValueError, TypeError) as error:
        return {"name": name, "rejected": True, "reason": str(error)}
    raise ValueError("hostile control accepted: " + name)


def requested_full_current(row):
    """A known contact projection cannot silently complete an unmapped route."""
    require(not row["unresolved_signed_projection"],
            "full current unresolved: routed burden has no admitted signed-current projection")
    return row["boundary_slice"].moving_total


def encounter_source():
    """Reuse the already declared expiry/reentry scenario and exact addresses."""
    d = encounter.TICKET_DENOMINATOR
    masks = ((15, 15), (0, 15), (0, 15), (15, 15), (15, 15), (15, 15))
    tickets = ((0, d-1), (d//2, 0), (d-1, d//2),
               (d//3, 2*(d//3)), (0, d-1), (d-1, 0))
    world = encounter.initialize_world()
    preparation_digest = digest(world)
    _observe_encounter("particle.encounter_init", lambda: {
        **_encounter_context("retained-encounter-init-v1"),
        "requested_steps": len(masks), "state_fields": encounter.STATE_FIELDS,
        "ticket_denominator": d, "state": world.state, "world_digest": preparation_digest,
        "history": _encounter_history(world)})
    rows = []
    for index, (mask, ticket) in enumerate(zip(masks, tickets)):
        before = world
        world = encounter.advance_world(world, mask, ticket)
        row = world.attempts[-1]
        classifications = tuple({"identity": r.identity, "target": r.target,
            "created_tick": r.created_tick, "value": r.value,
            "status": "active" if r.created_tick in (world.state[0]-1, world.state[0]-2) else "inactive",
            "RD": r.rcd.support, "RCD_duration": r.rcd.duration(),
            "clipped_support_count": len(r.support_units), "token_count": len(r.tokens)}
            for r in world.relations)
        rows.append({"attempt": index, "masks": mask, "tickets": ticket,
            "before_state": before.state, "after_state": world.state,
            "counted_update": row.row, "selected_opportunities": row.selected_opportunities,
            "pending_carriers": tuple(i for i, complete in enumerate(row.row["completed"]) if not complete),
            "retained_relation_dispositions": classifications,
            "archive_digest": digest(world)})
        _observe_encounter("particle.encounter_update", lambda: {
            **_encounter_context("retained-encounter-update-v1"),
            "index": index, "masks": mask, "tickets": ticket,
            "before_state": before.state, "after_state": world.state,
            "before_world_digest": digest(before), "after_world_digest": rows[-1]["archive_digest"],
            "attempt": row, "pending_carriers": rows[-1]["pending_carriers"],
            "before_history": _encounter_history(before), "after_history": _encounter_history(world)})
    require(encounter.validate_world(world), "encounter witness replay failed")
    require(tuple(len(w.returns) for w in world.waves) == (7, 8), "fixed encounter schedule changed")
    _observe_encounter("particle.encounter_finish", lambda: {
        **_encounter_context("retained-encounter-finish-v1"),
        "requested_steps": len(masks), "completed_attempts": len(world.attempts),
        "final_state": world.state, "final_world_digest": digest(world),
        "history": _encounter_history(world)})
    for wave in world.waves:
        _observe_native_wave(wave, "source-encounter")
    return world, {"scenario": "existing expiry_reentry", "masks": masks, "tickets": tickets,
        "ticket_denominator": d, "preparation_returns": (3, 2),
        "preparation_digest": preparation_digest, "rows": rows,
        "source_core": "1:2", "scope": encounter.SCOPE,
        "law": "Unit formation and retained-relation occurrences count admissible C6 continuations; no flip-rate coefficient.",
        "map_to_proton_preparation": "Completed source return chirality is copied into a separately admitted 3:3 C6 return; the source law is not relabelled as a proton law."}


def materialize_carrier(source, label, prepared_count):
    """Three earlier Monon components, then one actual six-edge 3:3 return."""
    native.validate_wave(source)
    lineage = "proton-3:3:" + label + ":" + source.identity
    records, motifs, maps = [], [], []
    cursor = 0
    for ordinal, (source_trace, source_certificate) in enumerate(zip(source.returns, source.certificates)):
        hand = source_trace.roles[0]
        start = max(cursor, source_trace.indices[-1]+1)
        components = tuple(native.certified(native.trace(lineage, 4*ordinal+i,
            start_index=start+3*i, scope=SCOPE, chirality=hand)) for i in range(3))
        trace = native.trace(lineage, 4*ordinal+3, vertices=C6, turn=3,
                             start_index=start+9, scope=SCOPE, chirality=hand)
        record = native.certified(trace)
        registry = {c.certificate_digest: c for _, c, _ in (*components, record)}
        ids = tuple(t.return_id for t, _, _ in components)
        motif = native.motif_witness("3:3", trace, (tuple(ids for _ in range(3)), ids),
            tuple((t, c) for t, c, _ in components), registry, enclosure=6, chirality=hand)
        require(len(trace.vertices)-1 == 6 and len(set(trace.vertices[:-1])) == 6,
                "target must traverse actual complete C6")
        require(all(t.indices[-1] < trace.indices[0] for t, _, _ in components),
                "proton construction component is not earlier")
        records.append(record); motifs.append(motif)
        maps.append({"ordinal": ordinal,
            "source_role": "preparation" if ordinal < prepared_count else "counted_continuation",
            "source_return_id": source_trace.return_id,
            "source_trace_digest": source_trace.trace_digest,
            "source_certificate_digest": source_certificate.certificate_digest,
            "source_committed": source_trace.indices[-1], "copied_chirality": hand,
            "construction_start": start, "component_records": components,
            "materialized_return_id": trace.return_id,
            "materialized_certificate_digest": record[1].certificate_digest,
            "motif_witness_digest": motif.witness_digest})
        cursor = trace.indices[-1]+1
    wave = native.wave(records, motifs)
    require(native.validate_wave(wave), "3:3 carrier admission failed")
    require(all(m.chirality == t.roles[0] for m,t in zip(wave.motif_witnesses,wave.returns)),
            "mapped chirality differs from trace")
    _observe_native_wave(wave, "proton-carrier")
    return wave, {"label": label, "source_identity": source.identity,
        "identity": wave.identity, "core": "3:3", "enclosure": 6,
        "species_map": "Manual p+ -> 3:3:6", "return_maps": tuple(maps),
        "source_preparation_returns": prepared_count,
        "source_continuation_returns": len(records)-prepared_count,
        "target_atomic_traversals": tuple(native.phase_step_count(t) for t in wave.returns),
        "target_vertices": C6, "turn": 3,
        "observables": support.wave_observables(wave)}


def admitted_contacts(left, right):
    contacts, evidence, previous = [], [], -1
    for event in native.contacts(left, right):
        occurrences = tuple(tuple(t.return_id for t in w.returns if previous < t.indices[-1] <= event.committed)
                            for w in (left, right))
        reflected = tuple(map(len, occurrences))
        identity = digest({"source": event, "reflection_occurrences": occurrences})
        contacts.append(replace(event, identity=identity, reflection=reflected))
        evidence.append({"contact_id": identity, "interval": (previous, event.committed),
                         "reflected_return_ids": occurrences, "reflection_counts": reflected})
        previous = event.committed
    interval = (0, max(w.returns[-1].indices[-1] for w in (left, right)))
    forward = native.compare(left, right, tuple(contacts), window=interval)
    backward = native.compare(left, right, tuple(contacts), window=interval, reverse=True)
    _committed("particle.native_contacts", {
        "schema": "native-particle-contacts-v1", "capture": "validated-retained-snapshot",
        "scope": left.scope, "left": left.identity, "right": right.identity,
        "scale": left.scale, "lineage": left.identity + "|" + right.identity,
        "contacts": tuple(contacts), "reflection_evidence": tuple(evidence),
        "comparison_interval": interval})
    return tuple(contacts), tuple(evidence), interval, forward, backward


def run(output):
    sources = [Path(encounter.__file__), Path(support.__file__), Path(native.__file__),
               Path(accounting.__file__), Path(__file__)]
    before_hashes = {str(p.relative_to(ROOT)): file_sha(p) for p in sources}
    require(file_sha(native.__file__) == NATIVE_SHA, "native producer differs from bound R04 source")
    require(file_sha(accounting.__file__) == ACCOUNTING_SHA, "accounting dependency changed")
    require(file_sha(encounter.__file__) == ENCOUNTER_SHA, "encounter dependency changed")
    world, encounter_report = encounter_source()
    materialized = tuple(materialize_carrier(w, label, prep) for w,label,prep in
                         zip(world.waves, ("A", "B"), (3, 2)))
    carriers = tuple(item[0] for item in materialized)
    carrier_reports = tuple(item[1] for item in materialized)
    # The helper's generic legacy status is superseded by the source-bound
    # species association here; it is not an empirical observable prediction.
    for item in carrier_reports:
        item["observables"].pop("physical_proton_correspondence", None)
        item["observables"]["species_association"] = "Manual p+ -> 3:3:6"
    contacts, reflection_evidence, interval, channel, reverse = admitted_contacts(*carriers)
    expected_lag = Q((len(carriers[0].returns)-1)-(len(carriers[1].returns)-1))
    require(channel["lag"] == expected_lag == -1, "native recurrence lag differs from full counts")
    fixed_history = {"source_world": world, "carriers": carriers, "contacts": contacts,
                     "reflection_evidence": reflection_evidence, "interval": interval}
    history_digest = digest(fixed_history)
    outer = native.outer_trace(carriers, lineage="proton-pair:outer",
        scope="proton-pair:outer", scale=1, start_index=interval[1]+1)
    folded = native.fold(carriers, native.certified(outer, "outer", inner=carriers))
    _committed("particle.native_fold", {
        "schema": "native-particle-fold-v1", "capture": "validated-retained-snapshot",
        "scope": outer.scope, "lineage": outer.lineage, "scale": outer.scale, "fold": folded})
    first_cut = outer.indices[-1]+1
    source_state = replace(native.initial_state(carriers[0]),
                           orientation=carriers[0].returns[-1].roles[0])
    _committed("particle.native_state", {
        "schema": "native-particle-state-v1", "capture": "validated-retained-snapshot",
        "scope": carriers[0].scope, "operation": "initial_with_terminal_orientation",
        "lineage": carriers[0].lineage, "scale": carriers[0].scale,
        "wave_identity": carriers[0].identity, "state": source_state})
    _observe_encounter("particle.routing_plan", lambda: {
        "schema": "native-routing-plan-v1", "capture": "validated-routing-setup",
        "scope": SCOPE, "lineage": "proton-routing", "scale": 0,
        "windows": WINDOWS, "capacity": CAPACITY, "closure_cost": CLOSURE_COST,
        "burden_unit": BURDEN_UNIT, "current_unit": CURRENT_UNIT,
        "source_history_digest": history_digest, "outer_return_id": outer.return_id,
        "source_bindings": before_hashes})
    slices, rows, route_witnesses, controls = [], [], [], []
    for ordinal, window in enumerate(WINDOWS):
        # Matched alternatives read the SAME physical history at the SAME cut.
        # A new window is not a new physical flow occurrence or elapsed step.
        cut = first_cut
        archive = accounting.ArchiveState()
        route_ledger = accounting.RouteLedger()
        projected = accounting.project_window(channel["rcd"].support, channel["duration"], window,
            channel["rcd"].domain, "native-return-count", ttl=10, retention_window=2,
            ttl_domain="native-committed-cut", retention_domain="native-committed-cut")
        part = native.participation(channel["rcd"], window, "native-return-count")
        require(projected["participation"] == part, "accounting/native participation differ")
        pressure, attention = CAPACITY*part, channel["lag"]
        flux = pressure*attention
        burden = pressure*abs(channel["lag"])
        surplus = max(Q(0), burden-CLOSURE_COST)
        bridge = {"law": "P=sum C3*rho_window*abs(lag), one declared directional channel",
                  "capacity": CAPACITY, "participation": part, "lag": channel["lag"],
                  "channel": channel, "window": window, "closure_cost": CLOSURE_COST}
        sheddic = native.shedding(folded, source_state, burden, CLOSURE_COST,
            source_event_digest=history_digest, model_bridge=digest(bridge), committed=cut,
            route="exoshedding" if surplus else None,
            target_scope=SCOPE if surplus else None, target_scale=0 if surplus else None)
        require(native.validate_shedding(folded, sheddic), "computed shedding did not admit")
        require(sheddic.surplus == surplus, "native surplus differs from exact burden residual")
        _committed("particle.native_shedding", {
            "schema": "native-particle-shedding-v1", "capture": "validated-retained-snapshot",
            "scope": SCOPE, "occurrence_semantics": "non-additive alternative views",
            "lineage": carriers[0].lineage, "scale": carriers[0].scale,
            "participation_window": window, "participation_domain": "native-return-count",
            "record": sheddic, "model_bridge": bridge, "source_state": source_state})
        additions = []
        for label,wave in zip(("A", "B"),carriers):
            additions.append((accounting.ContentRecord("carrier:"+label, "carrier:"+wave.identity,
                digest(wave), wave.returns[-1].indices[-1], 0, "native-carrier-support", Q(1),
                ("carrier-support:"+label,)), "active"))
        additions.append((accounting.ContentRecord("outer-fold", "fold:"+outer.return_id,
            digest(folded), outer.indices[-1], 1, "native-carrier-support", Q(1),
            ("carrier-support:A", "carrier-support:B"), ("carrier:A", "carrier:B")), "active"))
        for name,comparison in (("AB",channel),("BA",reverse)):
            additions.append((accounting.ContentRecord("RCD:"+name, "contact-history:"+name,
                digest(comparison["rcd"]), interval[1], 0, "native-return-count",
                Q(comparison["duration"]), ("contact-family:"+name,)), "active"))
        flow_id = "AB:flux:"+str(cut)
        additions.append((accounting.ContentRecord(flow_id, "AB:projection:"+str(cut),
            digest({"channel":channel,"window":window,"capacity":CAPACITY,"attention":attention}),
            cut, 0, CURRENT_UNIT, flux, ("AB:occurrence:"+str(cut),)), "active"))
        surplus_id = "surplus:"+str(cut)
        if surplus:
            additions.append((accounting.ContentRecord(surplus_id, "native-surplus:"+sheddic.record_digest,
                digest(sheddic), cut, 0, BURDEN_UNIT, surplus,
                ("surplus-occurrence:"+sheddic.record_digest,)), "routed"))
        archive = accounting.commit_archive(archive, cut, tuple(additions))
        motion = accounting.BoundaryMotion("stationary", ("carrier-A-cell",), ("carrier-A-cell",),
            Q(0), CURRENT_UNIT, digest({"cut":cut,"region":("carrier-A-cell",),"motion":"stationary"}))
        coverage = accounting.BoundaryCoverage(cut, "proton-A-contact-boundary", 0, CURRENT_UNIT,
            (flow_id,), (), SCOPE+":contact-channel-projection", window, "native-return-count",
            digest({"placement":{carriers[0].identity:"inside",carriers[1].identity:"outside"},
                    "channel":channel,"window":window,"cross":(flow_id,),
                    "shedding_current":(),"burden_route_has_no_signed_current_bridge":True}))
        boundary = accounting.account_slice(archive, cut, "proton-A-contact-boundary", 0, CURRENT_UNIT,
            cross=(flow_id,), shedding=(), motion=motion, coverage=coverage)
        require(boundary.moving_total == flux, "stationary boundary current differs from admitted channel")
        pending_before, pending_after, receipt = (), (), None
        route_form = None
        if surplus:
            require(native.expose(folded, sheddic) == carriers, "exoshedding lost inner carriers")
            route_form = support._route_support(carriers[0], "proton-accounting:"+str(cut), cut+1)
            route_wave = route_form["wave"]
            require(native.validate_wave(route_wave), "route support did not admit")
            _observe_native_wave(route_wave, "routed-support")
            route_state = native.initial_state(route_wave)
            seated = native.seat_duon(route_wave, route_state, compatible=True)
            require(seated.current == route_state.current, "seating unexpectedly generated feedback")
            _committed("particle.native_state", {
                "schema": "native-particle-state-v1", "capture": "validated-retained-snapshot",
                "scope": route_wave.scope, "operation": "seat_duon",
                "lineage": route_wave.lineage, "scale": route_wave.scale,
                "wave_identity": route_wave.identity, "compatible": True,
                "before": route_state, "state": seated})
            destination = route_wave.identity
            leg_id, route_id = "exo-leg:"+str(cut), "exo-route:"+str(cut)
            route_record = accounting.RouteRecord(route_id, surplus_id, cut, surplus, BURDEN_UNIT,
                (accounting.RouteLeg(leg_id, "exoshedding", surplus, destination),), digest(sheddic))
            route_ledger = accounting.register_route(route_ledger, archive, route_record)
            pending_before = accounting.pending_routes(route_ledger)
            receipt_cut = route_wave.returns[-1].indices[-1]+1
            require(receipt_cut > cut and receipt_cut-cut <= 10,
                    "receipt outside declared retention availability")
            receipt = {"route_id":route_id,"leg_id":leg_id,"destination":destination,"cut":receipt_cut,
                       "admitted_route_wave_digest":digest(route_wave),"seated_state":seated,
                       "current_before_seating":route_state.current,"current_after_seating":seated.current}
            controls.append(rejected("same-cut route receipt", lambda: accounting.receive_route(
                route_ledger, route_id, leg_id, cut, destination, "bad-same-cut", digest(receipt))))
            route_ledger = accounting.receive_route(route_ledger, route_id, leg_id, receipt_cut,
                destination, "receipt:"+str(receipt_cut), digest(receipt))
            pending_after = accounting.pending_routes(route_ledger)
            controls.append(rejected("duplicate route consumption", lambda: accounting.receive_route(
                route_ledger, route_id, leg_id, receipt_cut+1, destination, "duplicate-receipt", digest(receipt))))
            route_witnesses.append({"shedding":sheddic,"bridge":bridge,"route_support":route_form,
                                    "route_record":route_record,"receipt":receipt})
        one_slice_current = accounting.accumulate_current((boundary,))
        unresolved = ({"status":"SIGNED_SHEDDING_CURRENT_UNRESOLVED",
            "physical_source":sheddic.record_digest,"amount":surplus,"known_unit":BURDEN_UNIT,
            "required_projection":"bound burden-route to signed-current map",
            "route_accounted":True,"receipt_accounted":bool(receipt)},) if surplus else ()
        oriented = native.aod_flux((("carrier-A-cell", "carrier-B-cell", pressure, attention,
                                     "retained-AB:"+history_digest),))
        a_view = native.stokes(oriented, {"carrier-A-cell"})
        b_view = native.stokes(oriented, {"carrier-B-cell"})
        union_view = native.stokes(oriented, {"carrier-A-cell", "carrier-B-cell"})
        stokes = {"same_channel_A_outward":a_view["boundary"],"same_channel_B_outward":b_view["boundary"],
            "union_boundary":union_view["boundary"],"internal_pair_sum":union_view["internal"],
            "native_audits":{"A":a_view,"B":b_view,"union":union_view},
            "reverse_view":"fixed AB channel orientation reversal; independent native BA diagnostic remains separate"}
        require(stokes["internal_pair_sum"] == stokes["union_boundary"], "same-channel Stokes cancellation failed")
        rows.append({"alternative":"participation-window-"+str(window),
            "cut":cut,"window":window,"projection":projected,"channel_pressure":pressure,
            "attention":attention,"signed_crossing":flux,"closure_burden":burden,
            "closure_cost":CLOSURE_COST,"residual":burden-CLOSURE_COST,"surplus":surplus,
            "native_shedding":sheddic,"boundary_slice":boundary,"one_slice_current":one_slice_current,
            "current_scope":"contact-channel projection only",
            "unresolved_signed_projection":unresolved,"full_current_total":None,
            "same_channel_stokes_views":stokes,
            "pending_before_receipt":pending_before,"pending_after_receipt":pending_after,"receipt":receipt})
        _observe_encounter("particle.routing_finish", lambda: {
            "schema": "native-routing-finish-v1", "capture": "validated-branch-terminal",
            "scope": SCOPE, "lineage": "proton-routing", "scale": 0,
            "occurrence_semantics": "non-additive alternative views",
            "alternative_count": len(WINDOWS), "alternative_index": ordinal,
            "participation_window": window, "cut": cut,
            "source_native_shedding_digest": sheddic.record_digest,
            "routes": route_ledger.routes, "receipts": route_ledger.receipts,
            "pending_routes": accounting.pending_routes(route_ledger)})
        slices.append(boundary)
        require(digest(fixed_history) == history_digest, "window projection changed retained native histories")
    controls.append(rejected("alternative windows are non-additive", lambda:
                             accounting.accumulate_current(tuple(slices))))
    controls.append(rejected("unmapped routed burden cannot complete full current", lambda:
                             requested_full_current(rows[-1])))
    controls.append(rejected("omitted admitted crossing", lambda: accounting.account_slice(
        archive, slices[-1].cut, "proton-A-contact-boundary", 0, CURRENT_UNIT,
        cross=(), motion=slices[-1].motion, coverage=slices[-1].coverage)))
    controls.append(rejected("missing explicit sweep", lambda: accounting.account_slice(
        archive, slices[-1].cut, "proton-A-contact-boundary", 0, CURRENT_UNIT,
        cross=slices[-1].cross, coverage=slices[-1].coverage)))
    controls.append(rejected("same physical projection counted twice", lambda: accounting.accumulate_current(
        (slices[-1], accounting.account_slice(archive, slices[-1].cut+1, "proton-A-contact-boundary", 0,
        CURRENT_UNIT, cross=slices[-1].cross, motion=slices[-1].motion,
        coverage=replace(slices[-1].coverage,cut=slices[-1].cut+1))))))
    bad_motif = replace(carriers[0].motif_witnesses[0],
                        component_traces=carriers[0].motif_witnesses[0].component_traces[:-1])
    controls.append(rejected("missing proton formation component", lambda: native.validate_motif(bad_motif)))
    require(all(file_sha(p)==before_hashes[str(p.relative_to(ROOT))] for p in sources), "bound source changed during example")
    witnesses = {"encounter_world":world,"carrier_waves":carriers,"carrier_preparation_maps":carrier_reports,
        "contacts":contacts,"reflection_occurrences":reflection_evidence,"outer_fold":folded,
        "route_witnesses":route_witnesses,"route_ledger":route_ledger,"final_archive":archive}
    output.parent.mkdir(parents=True,exist_ok=True)
    witness_path = output.with_name(output.stem+"-witnesses.json.gz")
    witness_path.write_bytes(gzip.compress(packed(witnesses),mtime=0))
    report = {"status":"PASS","example":"proton-proton retained encounter and boundary accounting",
        "support_class":"D0 exact declared native preparation, projection and accounting example",
        "source_sha256":before_hashes,"sources_unchanged":True,
        "recorded_manuscript_binding":MANUSCRIPT_BINDING,
        "encounter":encounter_report,"carriers":tuple({k:v for k,v in item.items() if k!="return_maps"} for item in carrier_reports),
        "species_observation_map":{"species":"p+","native_support":"3:3:6",
            "source":"manual/sections/01_rest_energy_prediction.tex",
            "motif_accessors":{"Pi":30,"RD":native.rd(3,3),"window":6,"participation":6,
                               "motif_capacity":6,"P":36,"Q":330},
            "historical_mass_target_MeV":"938.27208943","mass_prediction_MeV":None,
            "mass_error":None,"mass_target_role":"retained Manual comparison target; this accounting run scores exact native invariants",
            "scored_observables":"native admission, exact boundary identity, route partition and receipt causality"},
        "contacts":{"comparison_interval":interval,"events":contacts,"reflection_evidence":reflection_evidence,
                    "forward":channel,"reverse_diagnostic":reverse,
                    "reverse_role":"independently reconstructed direction; not another outward AB contribution"},
        "fixed_history_sha256":history_digest,"choices":{"capacity":CAPACITY,"closure_cost":CLOSURE_COST,
            "windows":WINDOWS,"boundary":"carrier A inside, carrier B outside; stationary",
            "orientation":"positive bookkeeping orientation A to B; attention equals signed native lag",
            "load_rule":"capacity * compatible RCD participation * abs(lag)",
            "route":"positive native surplus -> exoshedding -> admitted Duon/E2 support -> later receipt",
            "receipt_TTL":10,"seat_window":2,"retention_domain":"native-committed-cut",
            "source_kernel_changed":False},
        "rows":rows,"comparison_semantics":{"kind":"matched window alternatives",
            "same_history":True,"same_input_cut":first_cut,"additive":False,
            "cumulative_current":None,"reason":"Alternative projections of one physical occurrence are not successive flow slices."},
        "negative_controls":controls,
        "witness_payload":{"file":witness_path.name,"sha256":file_sha(witness_path),
                           "uncompressed_sha256":hashlib.sha256(packed(witnesses)).hexdigest()},
        "scope_notes":["Actual six-edge C6 targets have three prior Monon component witnesses each.",
            "Motif RD61 and contact-channel RD/RCD are separate typed accessors.",
            "The 1:2 counted encounter law is mapped into a declared 3:3 preparation; no implicit change to the engine or law.",
            "Each signed slice covers the declared contact-channel projection only. For positive surplus, routed burden remains an explicit unresolved signed-shedding projection obligation; an empty contact-projection shedding family does not establish physical shedding current zero or a complete full current.",
            "The route receipt is later than admitted support completion; seating retains current and does not itself generate feedback.",
            "All native histories survive clipping, source pending attempts and route exposure.",
            "The four window alternatives are non-additive. Each has its own one-slice current; no repeated-response experiment or cumulative current is invented."]}
    output.write_text(json.dumps(encode(report),indent=2,sort_keys=True)+"\n")
    return {"status":"PASS","output":str(output),"source_returns":tuple(len(w.returns) for w in world.waves),
        "carrier_returns":tuple(len(w.returns) for w in carriers),"contact_RD":channel["rcd"].support,
        "contact_RCD":channel["duration"],"reverse_RCD":reverse["duration"],"lag":channel["lag"],
        "windows":WINDOWS,"crossing":tuple(r["signed_crossing"] for r in rows),
        "burden":tuple(r["closure_burden"] for r in rows),"surplus":tuple(r["surplus"] for r in rows),
        "alternatives_additive":False,"negative_controls":len(controls),"witness_file":witness_path.name}


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--output",type=Path,default=ROOT/"outputs/proton-accounting.json")
    args=parser.parse_args()
    print(json.dumps(encode(run(args.output)),indent=2))


if __name__ == "__main__":
    main()
