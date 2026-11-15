"""Declared finite solar boundary roles with actual native Duon witnesses.

Main/Manual ab8359209b8f9b2ef4007403101016e3ea58cfe4:
05_curl_closure_duon_current.tex:327-343 binds handed branch exchange;
06_field.tex:1183-1237,1320-1346 supplies nested/current/route accounting;
manual/sections/04_boundary_current_applications.tex:85-142 supplies D0
rho=5, window=4, C3=3, A=1/3 and the moving-boundary calculation;
manual/sections/00_dec_ledger.tex:337-353 supplies closure load27/cost24;
manual/sections/05_solar_system_field_tests.tex:62-98,132-153 declares the
solar/perihelion roles and the separate, unevaluated physical observable map.

This is a finite native preparation, not inferred solar formation or an orbit.
The prescribed whole-return signs, boundary membership and signed attention
are application inputs. No stochastic transition table or SI clock is used.
An admitted E2 route, unsigned closure surplus and separately prepared signed
route current retain separate units. Their missing burden-to-current map is
kept as a pending obligation rather than silently equating those quantities.
"""

from contextlib import nullcontext
from dataclasses import replace
from fractions import Fraction as Q
import json
import re

from . import region_engine as r
from .vendor import aod_manual_native as n


SCOPE = "declared:solar-boundary-duon-fixture"
UNIT = "native-pressure-current"
DOMAIN = "native-return-count"
BURDEN_UNIT = "native-closure-burden"
SOURCE = "simulations/solar_boundary_fixture.py:run"
CELLS = ("sun-core", "solar-shell", "mercury", "exterior")


def _duon(lineage, signs, *, pivot=None, start=3, occurrence=1):
    """Admit every returned branch, retaining the real earlier Monon pivot."""
    if not signs or any(type(sign) is not int or sign not in (-1, 1) for sign in signs):
        raise ValueError("whole-return chirality requires exact integer signs")
    if pivot is None:
        t = n.trace(lineage, 0, scope=SCOPE, start_index=start - 3)
        pivot = t, n.certify(t)
    records, motifs = [], []
    for index, sign in enumerate(signs):
        t = n.trace(lineage, occurrence + index, scope=SCOPE,
                    start_index=start + 3 * index, chirality=sign)
        c = n.certify(t)
        registry = {x.certificate_digest: x for x in (pivot[1], c)}
        construction = ((sign, -sign), (pivot[1].certificate_digest, sign), (-sign, sign))
        motifs.append(n.motif_witness("Duon", t, construction, (pivot,), registry,
                                      enclosure=6, chirality=sign))
        records.append((t, c, registry))
    return n.wave(tuple(records), tuple(motifs))


def _prefix(wave, count):
    registry = {c.certificate_digest: c for c in wave.certificates[:count]}
    return n.wave(tuple((t, c, registry) for t, c in
                        zip(wave.returns[:count], wave.certificates[:count])),
                  wave.motif_witnesses[:count])


def _comparison(left, right, *, bias=(Q(0), Q(0)), full=False):
    """Count real returns in disjoint intervals; preserve actual contact cuts."""
    n._validate_wave_collection((left, right))
    cuts = tuple(t.indices[-1] for t in left.returns)
    if len(cuts) < 3:
        raise ValueError("three completed returns required for local comparison")
    intervals = tuple(zip((-1,) + cuts[1:-1], cuts[1:])) if full else tuple(zip(cuts[-3:-1], cuts[-2:]))
    contacts, evidence = [], []
    for u, v in intervals:
        occurrences = tuple(tuple(t.return_id for t in w.returns if u < t.indices[-1] <= v)
                            for w in (left, right))
        reflected = tuple(map(len, occurrences))
        support = n.trace(left.identity + "|" + right.identity, v,
                          scope=SCOPE, start_index=v - 2)
        identity = ("solar-full-contact:" if full else "solar-window-contact:") + n.digest(
            (left.identity, right.identity, u, v))
        contact = n.Contact(identity, left.identity, right.identity, SCOPE, v,
            n.wave_prefix(left, v), n.wave_prefix(right, v), reflected, support, n.certify(support))
        contacts.append(contact)
        evidence.append({"contact_id": identity, "interval": (u, v),
            "reflected_return_ids": occurrences, "reflection_counts": reflected})
    window = (0, cuts[-1]) if full else (cuts[-2], cuts[-1])
    result = n.compare(left, right, tuple(contacts), window=window, bias=bias)
    return {"left": left.identity, "right": right.identity, "contacts": tuple(contacts),
            "reflection_evidence": tuple(evidence), "comparison_interval": window,
            "bias_operands": bias, "comparison": result}


def _channel(comparison, cut, identity, source, target, intensity, *, kind="cross"):
    capacity, window = Q(3), 4
    calculation = comparison["comparison"]
    participation = n.participation(calculation["rcd"], window, DOMAIN)
    pressure = capacity * participation
    attention = calculation["lag"] * intensity
    value = n.aod_flux(((source, target, pressure, attention, identity),))[source, target, identity]
    operand = {"identity": identity, "cut": cut, "scope": SCOPE,
        "source": source, "target": target, "kind": kind, "available": True,
        "native_comparison": comparison, "RCD": calculation["rcd"],
        "duration": calculation["duration"], "window": window, "window_domain": DOMAIN,
        "participation": participation, "capacity": capacity, "pressure": pressure,
        "lag": calculation["lag"], "intensity": intensity, "attention": attention,
        "signed_current": value, "effective_current": value,
        "bias_protocol": "declared signed offset; independent of recurrence count and SI time"}
    channel = r.Channel(identity, identity, source, target, value, r.fingerprint(operand), kind)
    return channel, operand


def _append(store, kind, payload, *, scope=SCOPE, lineage="solar-preparation", scale=0,
            key=None, parents=(), exact_payload=False):
    return store.append(kind, payload if exact_payload else r.serial(payload), scope_id=scope, lineage_id=lineage,
        scale_index=scale, scale_family="native-solar-fixture", source_ref=SOURCE,
        source_key=None if key is None else json.dumps([SCOPE, kind, key]), causal_parents=parents)


def _capture_wave(store, wave, family):
    for ordinal, (trace, certificate, motif) in enumerate(
            zip(wave.returns, wave.certificates, wave.motif_witnesses)):
        _append(store, "particle.native_return", {
            "schema": "native-particle-return-v1", "family": family,
            "capture": "validated-retained-snapshot",
            "wave": {"identity": wave.identity, "scope": wave.scope, "scale": wave.scale,
                     "lineage": wave.lineage, "declared_motif": wave.motif},
            "return_ordinal": ordinal, "trace": trace, "certificate": certificate,
            "motif_witness": motif, "formation": {"construction": motif.construction,
                "component_traces": motif.component_traces,
                "component_certificates": motif.component_certificates}},
            scope=wave.scope, lineage=wave.lineage, scale=wave.scale,
            key=(wave.identity, ordinal))


def _capture_contacts(store, left, right):
    evidence = _comparison(left, right, full=True)
    _append(store, "particle.native_contacts", {
        "schema": "native-particle-contacts-v1", "capture": "validated-retained-snapshot",
        "scope": SCOPE, "lineage": left.identity + "|" + right.identity, "scale": 0,
        **{k: evidence[k] for k in ("left", "right", "contacts", "reflection_evidence", "comparison_interval")}},
        lineage=left.identity + "|" + right.identity, key=(left.identity, right.identity))
    return evidence


def run(store=None):
    """Return typed ledger/audits and canonical native evidence; capture optionally."""
    sun = _duon("solar-role:sun", (1, 1, 1, 1, 1, 1))
    mercury = _duon("solar-role:mercury", (1, 1, 1, -1, -1, 1))
    n._validate_wave_collection((sun, mercury))
    outer = n.outer_trace((sun, mercury), lineage="solar-role:outer", scope=SCOPE + ":outer",
                          scale=1, start_index=21)
    fold = n.fold((sun, mercury), n.certified(outer, "outer", inner=(sun, mercury)))
    source_state = n.initial_state(sun)
    bridge = {"law": "declared Manual D0 closure-load fixture", "load": Q(27), "cost": Q(24),
              "unit": BURDEN_UNIT, "source_anchor": "manual:dec-ledger:boundary-audit"}
    shedding = n.shedding(fold, source_state, Q(27), Q(24), source_event_digest=n.digest((sun, mercury)),
        model_bridge=r.fingerprint(bridge), committed=24, route="exoshedding", target_scope=SCOPE, target_scale=0)
    n.expose(fold, shedding)
    route = _duon(sun.lineage, (1, 1, 1), pivot=(sun.returns[-1], sun.certificates[-1]),
                  start=25, occurrence=100)
    receiver = _duon("solar-role:route-receiver", (1, 1, 1), start=25)
    n._validate_wave_collection((sun, mercury, route, receiver))
    route_evidence = {"fold": fold, "source_state": source_state, "model_bridge": bridge,
        "shedding": shedding, "route_support": route, "receiver": receiver,
        "form": "E2", "core": "Duon", "route_relation": "declared retained-source pivot",
        "surplus_unit": BURDEN_UNIT, "partition": {"exoshedding": Q(3), "endoshedding": Q(0),
            "redirected_support": Q(0), "open_loss": Q(0)},
        "signed_projection": None,
        "current_preparation": "independent routed-Duon contact/attention current; not a surplus conversion"}
    if store is not None:
        for wave, family in ((sun, "solar-role"), (mercury, "mercury-role"),
                             (route, "sheddic-E2-role"), (receiver, "route-receiver-role")):
            _capture_wave(store, wave, family)
        _capture_contacts(store, sun, mercury)
        _capture_contacts(store, route, receiver)
        # The canonical relation detector handles folds, but its load bridge is
        # proton-specific. Keep this distinct D0 bridge recoverable as evidence.
        _append(store, "particle.native_fold", {"schema": "native-particle-fold-v1",
            "capture": "validated-retained-snapshot", "scope": outer.scope, "lineage": outer.lineage,
            "scale": outer.scale, "fold": fold}, scope=outer.scope, lineage=outer.lineage,
            scale=1, key=outer.return_id)
        _append(store, "evidence.solar_shedding", route_evidence, key=shedding.record_digest)

    ledger = r.Ledger(CELLS, UNIT, 0, SCOPE, DOMAIN)
    universe = r.Region("whole", CELLS)
    small = r.Region("selected", ("sun-core", "solar-shell"))
    large = r.Region("selected", ("sun-core", "solar-shell", "mercury"))
    memberships = ((small, small), (small, large), (large, large), (large, small), (small, small))
    frames = {name: [] for name in ("selected", "complement", "whole")}
    operands, preparations, snapshots, flips = [], [], [], []
    observer = nullcontext() if store is None else r.observe_commits(store.core_recorder)
    with observer as observation:
        for ordinal, (before, after) in enumerate(memberships):
            count = min(ordinal + 3, 6)
            a, b = _prefix(sun, count), _prefix(mercury, count)
            source_cut = b.returns[-1].indices[-1]
            cut = source_cut + 1 if ordinal < 4 else 35
            sign = b.returns[-1].roles[0]
            comparison = _comparison(a, b, bias=(Q(sign), Q(0)))
            channel, operand = _channel(comparison, cut, "solar-cross:" + str(cut),
                                        "sun-core", "mercury", Q(1, 3))
            operand["bias_protocol"] = "latest committed Mercury-role whole-return orientation supplies left offset; right offset0"
            # Bind the final, explicit protocol text along with the operands.
            channel = replace(channel, provenance=r.fingerprint(operand))
            channels, obligations = [channel], ()
            operands.append(operand)
            snapshots.append({"source_cut": source_cut, "response_cut": cut, "sun": a, "mercury": b})
            if ordinal < 4 and b.returns[-2].roles[0] != sign:
                flips.append({"wave_identity": b.identity, "previous_return": b.returns[-2],
                    "current_return": b.returns[-1], "before": b.returns[-2].roles[0], "after": sign,
                    "committed": source_cut, "basis": "whole-return branch-order exchange with retained hinge"})
            if ordinal == 4:
                routed = _comparison(route, receiver, bias=(Q(1), Q(0)))
                route_channel, route_operand = _channel(routed, cut, "solar-routed-current:35",
                    "solar-shell", "exterior", Q(1, 6), kind="shedding")
                route_operand["route_record"] = shedding.record_digest
                route_operand["surplus_to_current_map"] = None
                route_channel = replace(route_channel, provenance=r.fingerprint(route_operand))
                channels.append(route_channel); operands.append(route_operand)
                obligations = (r.PendingProjection("solar-surplus-unmapped", "solar-surplus:" + shedding.record_digest,
                    "solar-shell", "exterior", shedding.surplus, BURDEN_UNIT, shedding.record_digest),)
            changed = before.cells != after.cells
            sweep = (r.SweepSample("mercury", Q(3), UNIT, 0,
                r.fingerprint({"declared_swept_content": Q(3), "cut": cut, "unit": UNIT})),) if changed else ()
            ledger = r.commit(ledger, cut, 4, tuple(channels), obligations=obligations, sweep_field=sweep)
            preparations.append({"cut": cut, "window": 4, "channels": tuple(channels), "obligations": obligations})
            motion = "moving" if changed else "stationary"
            frames["selected"].append(r.Frame(before, after, motion, sweep))
            frames["complement"].append(r.Frame(r.region_difference("complement", universe, before),
                r.region_difference("complement", universe, after), motion, sweep))
            frames["whole"].append(r.Frame(universe, universe, "stationary", ()))
    if store is not None and observation.failure_count:
        from .trace_store import TraceIncomplete
        raise TraceIncomplete("solar region capture incomplete: " + str(observation.last_error))
    frames = {name: tuple(rows) for name, rows in frames.items()}
    audits = {name: r.audit_boundary(ledger, name, rows) for name, rows in frames.items()}
    partition = r.audit_partition(audits["whole"], (audits["selected"], audits["complement"]))
    if store is not None:
        for operand in operands:
            parent = store.get_by_key(json.dumps(["region", SCOPE, operand["cut"]]))
            _append(store, "evidence.channel_preparation", operand, key=operand["identity"],
                    parents=(parent["event_id"],))
        for name, audit in audits.items():
            for row, frame in zip(audit.rows, audit.frames):
                parent = store.get_by_key(json.dumps(["region", SCOPE, row["cut"]]))
                _append(store, "detector.boundary_view", {"boundary": name, "row": row, "frame": frame,
                    "unit": UNIT, "coverage": audit.coverage_claim}, key=(name, row["cut"]),
                    parents=(parent["event_id"],), exact_payload=True)
        _append(store, "evidence.solar_chiral_flips", {"flips": flips,
            "law": "prescribed whole-return signs; no transition probability"}, key="whole-return-flips")
    return {"example": "solar-boundary-duon", "evidence_class": "D0 declared finite native boundary application",
        "ledger": ledger, "frames": frames, "audits": audits, "partitions": {"whole": partition},
        "channel_operands": tuple(operands), "complete_preparation": tuple(preparations),
        "native_waves": (sun, mercury, route, receiver), "native_snapshots": tuple(snapshots),
        "native_contacts": tuple(item["native_comparison"] for item in operands),
        "chiral_flips": tuple(flips), "route_evidence": route_evidence,
        "summary": {name: tuple(row["current"] for row in audit.rows) for name, audit in audits.items()},
        "scope": {"solar_names": "boundary roles only", "physical_trajectory": False,
            "si_conversion": None, "physical_percent_error": None,
            "shedding_current_coverage": "pending surplus-to-current projection",
            "participation_window": "declared4 native return-count slots; distinct from comparison cut endpoints",
            "boundary_motion": "declared include/exclude Mercury-role cell; no orbital motion inference"}}


def inspect_trace(store, *, max_events=10000, max_bytes=16 * 1024 * 1024,
                  max_history_bindings=100000, max_history_nodes=500000):
    """Recompute this declared route/current witness from a bounded real archive.

    General native relation detection has a different load bridge. This adapter
    admits the specifically declared Manual27/24 operands, never a generic
    physical shedding or solar observation law. A partial query publishes no
    verified dependent route, current, or chirality result.
    """
    from .trace_history_union import RetainedHistoryUnion
    from .trace_particle_relations import _formation, _fold, _state, _shedding
    from .trace_support import decode_wave
    from .trace_store import TraceError, TraceLimits, canonical_bytes
    from .trace_detectors import _decode_ratios

    if any(type(limit) is not int or limit <= 0 for limit in
           (max_events, max_bytes, max_history_bindings, max_history_nodes)):
        raise ValueError("positive exact solar inspection budgets required")
    history = RetainedHistoryUnion(max_bindings=max_history_bindings, max_nodes=max_history_nodes)
    result = {"status": "unavailable", "coverage": "partial",
        "scope": "declared fixture route/current witness", "physical_trajectory": False,
        "route": None, "chiral_flips": [], "declared_routed_current": None,
        "full_current": "unresolved surplus-to-current map", "failures": [],
        "inspected_events": 0, "inspected_bytes": 0}
    groups, folds, routes, flip_records, preparations, commits = {}, [], [], [], [], {}

    def require(condition, message):
        if not condition:
            raise ValueError(message)

    def quantity(value):
        if type(value) is dict:
            require(set(value) == {"$ratio"} and type(value["$ratio"]) is list and
                    len(value["$ratio"]) == 2 and all(type(x) is int for x in value["$ratio"]),
                    "exact stored rational required")
            a, b = value["$ratio"]
            require(b > 0 and (Q(a, b).numerator, Q(a, b).denominator) == (a, b),
                    "normalized stored rational required")
            return Q(a, b)
        require(type(value) in (int, str, Q), "exact rational operand required")
        if type(value) is str:
            require(len(value) <= 1024 and re.fullmatch(r"-?[0-9]+(?:/[1-9][0-9]*)?", value) is not None,
                    "bounded integer-ratio text required")
        result = Q(value)
        require(max(result.numerator.bit_length(), result.denominator.bit_length()) <= 8192,
                "rational operand budget exhausted")
        return result

    policy = TraceLimits(max_event_bytes=max(4096, max_bytes),
                         max_storage_bytes=max(65536, 4 * max_bytes), max_nodes=262144)
    try:
        for event in store.iter_events():
            result["inspected_events"] += 1
            require(result["inspected_events"] <= max_events, "solar event budget exhausted")
            result["inspected_bytes"] += len(canonical_bytes(event, policy))
            require(result["inspected_bytes"] <= max_bytes, "solar byte budget exhausted")
            if event["scope_id"] not in (SCOPE, SCOPE + ":outer"):
                continue
            kind, payload = event["kind"], event["payload"]
            if kind == "particle.native_return":
                trace, cert, motif, detected = _formation(payload)
                require(detected["inferred_core"] == "Duon", "solar fixture requires admitted Duon support")
                require((event["scope_id"], event["lineage_id"], event["scale_index"]) ==
                        (trace.scope, trace.lineage, trace.scale), "return envelope differs from witness")
                history.include(trace, cert, motif)
                identity, ordinal = payload["wave"]["identity"], payload["return_ordinal"]
                require(type(identity) is str and type(ordinal) is int and ordinal >= 0,
                        "exact retained wave identity and ordinal required")
                group = groups.setdefault(identity, {})
                record = (trace, cert, motif)
                require(ordinal not in group or group[ordinal] == record,
                        "conflicting retained wave ordinal")
                group[ordinal] = record
            elif kind == "particle.native_fold":
                require(payload.get("schema") == "native-particle-fold-v1", "unknown fold schema")
                folded = _fold(payload["fold"])
                t = folded["outer"][0]
                require((event["scope_id"], event["lineage_id"], event["scale_index"]) ==
                        (t.scope, t.lineage, t.scale), "fold envelope differs from witness")
                history.include(t, folded["outer"][1], *folded["inner"])
                folds.append(folded)
            elif kind == "evidence.solar_shedding":
                routes.append(payload)
            elif kind == "evidence.solar_chiral_flips":
                flip_records.append(payload)
            elif kind == "evidence.channel_preparation":
                preparations.append((event, payload))
            elif kind == "region.commit":
                cut = payload["row"]["cut"]
                require(type(cut) is int and cut not in commits, "duplicate or invalid committed cut")
                commits[cut] = event
        waves = {}
        for identity, group in groups.items():
            require(sorted(group) == list(range(len(group))), "missing retained wave ordinal")
            rows = [group[i] for i in range(len(group))]
            registry = {row[1].certificate_digest: row[1] for row in rows}
            wave = n.wave(tuple((row[0], row[1], registry) for row in rows), tuple(row[2] for row in rows))
            require(wave.identity == identity, "wave identity differs from admitted recurrence")
            history.include(wave)
            waves[identity] = wave
        require(len(routes) == len(folds) == len(flip_records) == 1,
                "one complete route, fold and flip evidence family required")
        retained_cuts = []
        for event in commits.values():
            payload = _decode_ratios(event["payload"])
            require(set(payload) == {"cells", "unit", "scale", "domain", "row"} and
                    (tuple(payload["cells"]), payload["unit"], payload["scale"], event["scope_id"], payload["domain"]) ==
                    (CELLS, UNIT, 0, SCOPE, DOMAIN), "committed current context differs from declared solar fixture")
            row = dict(payload["row"])
            row["channels"] = tuple(r.Channel(**c) for c in row["channels"])
            row["obligations"] = tuple(r.PendingProjection(**o) for o in row["obligations"])
            row["sweep_field"] = tuple(r.SweepSample(**s) for s in row["sweep_field"])
            retained_cuts.append(r.CutRecord(**row))
        # Validate immutable snapshots, monotone cuts, complete typed families,
        # and global physical-occurrence uniqueness before inspecting one route.
        ledger = r.Ledger(CELLS, UNIT, 0, SCOPE, DOMAIN, tuple(retained_cuts))
        raw = routes[0]
        for field in ("pressure", "current", "capacity"):
            quantity(raw["source_state"][field])
        for field in ("closure_load", "closure_cost", "residual", "surplus"):
            quantity(raw["shedding"][field])
        folded, state, record = _fold(raw["fold"]), _state(raw["source_state"]), _shedding(raw["shedding"])
        route, receiver = decode_wave(raw["route_support"]), decode_wave(raw["receiver"])
        history.include(folded["outer"][0], folded["outer"][1], *folded["inner"], route, receiver)
        require(folded == folds[0], "route has another retained outer fold")
        require(len(folded["inner"]) == 2, "two declared inner solar roles required")
        source, mercury = folded["inner"]
        require(all(w.identity in waves and waves[w.identity] == w
                    for w in (source, mercury, route, receiver)), "route target or source formation unavailable")
        require(state == n.initial_state(source), "source state differs from admitted source wave")
        bridge = {"law": "declared Manual D0 closure-load fixture", "load": Q(27), "cost": Q(24),
                  "unit": BURDEN_UNIT, "source_anchor": "manual:dec-ledger:boundary-audit"}
        require(raw["model_bridge"] == r.serial(bridge) and record.model_bridge == r.fingerprint(bridge),
                "declared Manual closure-load bridge differs")
        require(record.source_event_digest == n.digest(tuple(folded["inner"])),
                "source event digest differs from actual retained source family")
        require(record.closure_load == Q(27) and record.closure_cost == Q(24), "closure operands differ")
        n.validate_shedding(folded, record)
        rebuilt = n.shedding(folded, state, Q(27), Q(24), source_event_digest=record.source_event_digest,
            model_bridge=r.fingerprint(bridge), committed=record.committed, route="exoshedding",
            target_scope=SCOPE, target_scale=0)
        require(record == rebuilt, "route record differs from regenerated closure residual")
        require(n.expose(folded, record) == folded["inner"], "exoshedding lost retained inner family")
        require(raw["surplus_unit"] == BURDEN_UNIT and raw["signed_projection"] is None,
                "unsigned surplus acquired an unestablished current map")
        require(raw["partition"] == r.serial({"exoshedding": Q(3), "endoshedding": Q(0),
                    "redirected_support": Q(0), "open_loss": Q(0)}), "route partition differs from surplus")
        require(raw["form"] == "E2" and raw["core"] == route.motif == "Duon",
                "E2 route claim lacks Duon construction")
        require((route.scope, route.scale, route.lineage) == (source.scope, source.scale, source.lineage) and
                (receiver.scope, receiver.scale) == (route.scope, route.scale) and receiver.identity != route.identity,
                "route/target context differs")
        pivot = source.returns[-1], source.certificates[-1]
        require(all(m.component_traces == (pivot[0],) and m.component_certificates == (pivot[1],)
                    for m in route.motif_witnesses), "routed Duon lost the actual terminal source pivot")
        require(record.committed < route.returns[0].indices[0] and
                all(n.phase_step_count(t) == 2 for t in route.returns), "route E2 support or chronology differs")
        routed = [(e, p) for e, p in preparations if p.get("kind") == "shedding"]
        require(len(routed) == 1, "one independently prepared routed current required")
        event, prepared = routed[0]
        require(type(prepared["cut"]) is int and prepared["cut"] >
                max(route.returns[-1].indices[-1], receiver.returns[-1].indices[-1]),
                "routed current reads future native returns")
        comparison = _comparison(route, receiver, bias=(Q(1), Q(0)))
        channel, expected = _channel(comparison, prepared["cut"], prepared["identity"],
            "solar-shell", "exterior", Q(1, 6), kind="shedding")
        expected.update(route_record=record.record_digest, surplus_to_current_map=None)
        channel = replace(channel, provenance=r.fingerprint(expected))
        require(prepared == r.serial(expected), "routed current differs from reconstructed contact/attention operands")
        require(prepared["cut"] in commits, "routed current commitment unavailable")
        committed = commits[prepared["cut"]]
        require(event["causal_parents"] == [committed["event_id"]], "routed current lost its actual commitment")
        row = next(cut for cut in ledger.commits if cut.cut == prepared["cut"])
        matches = [c for c in row.channels if c.identity == channel.identity]
        require(matches == [channel],
                "declared route current is absent from its committed channel family")
        pending = [o for o in row.obligations if o.provenance == record.record_digest]
        expected_pending = r.PendingProjection("solar-surplus-unmapped", "solar-surplus:" + record.record_digest,
            "solar-shell", "exterior", Q(3), BURDEN_UNIT, record.record_digest)
        require(pending == [expected_pending],
                "unmapped unsigned surplus obligation is missing or changed")
        flips = [{"wave_identity": mercury.identity, "previous_return": before, "current_return": after,
                  "before": before.roles[0], "after": after.roles[0], "committed": after.indices[-1],
                  "basis": "whole-return branch-order exchange with retained hinge"}
                 for before, after in zip(mercury.returns, mercury.returns[1:])
                 if before.roles[0] != after.roles[0]]
        require(flip_records[0] == r.serial({"flips": flips,
            "law": "prescribed whole-return signs; no transition probability"}),
            "chiral flip markers differ from consecutive admitted whole-return histories")
        require(store.status()["coverage"] == "complete", "archive coverage is partial")
        result.update(status="verified", coverage="complete",
            route={"status": "verified", "core": route.motif, "form": "E2", "source": source.identity,
                "target": route.identity, "receiver": receiver.identity, "surplus": record.surplus,
                "surplus_unit": BURDEN_UNIT, "record_digest": record.record_digest},
            chiral_flips=[{"committed": f["committed"], "before": f["before"], "after": f["after"]} for f in flips],
            declared_routed_current=Q(2), current_unit=UNIT,
            source_waves=len(waves), verified_route_current_occurrences=1)
    except (TraceError, ValueError, TypeError, KeyError, AttributeError, IndexError, ZeroDivisionError) as exc:
        result.update(status="rejected", coverage="partial")
        result["failures"].append(str(exc))
    result["history_union"] = history.status()
    return result
