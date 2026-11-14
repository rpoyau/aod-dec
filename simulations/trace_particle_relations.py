"""Recover independent coupling, shedding and seating axes from native witnesses.

This detector reads committed snapshots. It does not evolve the model, select a
target support signature, or infer absent relations from an empty trace family.
"""
from dataclasses import asdict, fields, is_dataclass, replace
from fractions import Fraction
from hashlib import sha256
import json

from .vendor import aod_manual_native as native
from .trace_store import TraceError, TraceLimits, canonical_bytes
from .trace_support import (decode_trace, decode_certificate, decode_wave,
                            decode_motif, inspect_native_return)
from .trace_history_union import RetainedHistoryUnion


class RelationError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise RelationError(message)


def _tuple(value):
    require(type(value) in (tuple, list), "exact sequence required")
    return tuple(_tuple(x) if type(x) in (tuple, list) else x for x in value)


def _fields(value, cls):
    require(type(value) is dict and set(value) == {f.name for f in fields(cls)},
            "incomplete or unexpected " + cls.__name__ + " fields")
    return dict(value)


def _ratio(value):
    require(type(value) in (int, str, Fraction), "exact rational operand required")
    require(not isinstance(value, str) or len(value) <= 1024, "rational text budget exceeded")
    require(not isinstance(value, int) or value.bit_length() <= 8192, "integer budget exceeded")
    try:
        result = Fraction(value)
    except (ValueError, ZeroDivisionError) as exc:
        raise RelationError("invalid exact rational operand: " + str(exc)) from exc
    require(max(result.numerator.bit_length(), result.denominator.bit_length()) <= 8192,
            "rational integer budget exceeded")
    return result


def _plain(value):
    if is_dataclass(value):
        return _plain(asdict(value))
    if isinstance(value, Fraction):
        return str(value)
    if type(value) is dict:
        return {str(key): _plain(child) for key, child in value.items()}
    if type(value) in (list, tuple):
        return [_plain(child) for child in value]
    return value


def _producer_digest(value):
    return sha256(json.dumps(_plain(value), sort_keys=True, separators=(",", ":"),
                             allow_nan=False).encode()).hexdigest()


def _state(value):
    row = _fields(value, native.WaveState)
    for key in ("present", "closure_memory", "retained_rebalancing", "motif_certificates", "cadence"):
        row[key] = _tuple(row[key])
    for key in ("pressure", "current", "capacity"):
        row[key] = _ratio(row[key])
    require(all(type(row[key]) is int for key in ("committed", "orientation", "phases")),
            "exact wave state counters required")
    return native.WaveState(**row)


def _contact(value):
    row = _fields(value, native.Contact)
    row["reflection"] = _tuple(row["reflection"])
    row["support_trace"] = decode_trace(row["support_trace"])
    row["support_certificate"] = decode_certificate(row["support_certificate"])
    return native.Contact(**row)


def _shedding(value):
    row = _fields(value, native.SheddingRecord)
    for key in ("retained_lineages", "retained_certificate_digests"):
        row[key] = _tuple(row[key])
    for key in ("closure_load", "closure_cost", "residual", "surplus"):
        row[key] = _ratio(row[key])
    return native.SheddingRecord(**row)


def _fold(value):
    require(type(value) is dict and set(value) == {"outer", "phase", "inner", "proposal", "status"},
            "complete native fold required")
    require(type(value["outer"]) in (tuple, list) and len(value["outer"]) == 3,
            "outer trace, certificate and registry required")
    trace, certificate, registry = value["outer"]
    require(type(registry) is dict, "outer certificate registry required")
    outer = (decode_trace(trace), decode_certificate(certificate),
             {key: decode_certificate(cert) for key, cert in registry.items()})
    inner = tuple(decode_wave(wave) for wave in _tuple(value["inner"]))
    decoded = {"outer": outer, "phase": value["phase"], "inner": inner,
               "proposal": _tuple(value["proposal"]), "status": value["status"]}
    require(type(decoded["phase"]) is int, "exact outer phase count required")
    expected = native.fold(inner, outer)
    require(decoded == expected, "fold snapshot differs from executed closure and retained inner histories")
    return expected


def _formation(payload):
    detected = inspect_native_return(payload)
    require(detected["status"] == "verified", detected.get("reason", "unverified native formation"))
    trace, certificate = decode_trace(payload["trace"]), decode_certificate(payload["certificate"])
    if payload.get("motif_witness") is not None:
        motif = decode_motif(payload["motif_witness"])
    else:
        raw = payload["formation"]
        traces = tuple(decode_trace(t) for t in raw["component_traces"])
        certificates = tuple(decode_certificate(c) for c in raw["component_certificates"])
        registry = {c.certificate_digest: c for c in (*certificates, certificate)}
        motif = native.motif_witness(detected["inferred_core"], trace, _tuple(raw["construction"]),
            tuple(zip(traces, certificates)), registry,
            detected["enclosure"]["grammar_validation_parameter"], trace.roles[0])
    return trace, certificate, motif, detected


def _json_payload(data):
    def integer(text):
        require(len(text.lstrip("-")) <= 128, "JSON integer budget exceeded")
        return int(text)
    def inexact(text):
        raise RelationError("inexact JSON number in native snapshot")
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, "duplicate JSON object key in native snapshot")
            result[key] = value
        return result
    value = json.loads(data, parse_int=integer, parse_float=inexact, parse_constant=inexact,
                       object_pairs_hook=pairs)
    nodes = 0
    def inspect(item, depth=0):
        nonlocal nodes
        nodes += 1
        require(nodes <= 262144 and depth <= 64, "native snapshot node/depth budget exceeded")
        if type(item) is dict:
            for child in item.values():
                inspect(child, depth + 1)
        elif type(item) is list:
            for child in item:
                inspect(child, depth + 1)
    inspect(value)
    require(type(value) is dict, "native snapshot object required")
    return value


def detect_particle_relations(store, *, scope=None, max_events=10000,
                              max_bytes=32 * 1024 * 1024, max_relations=1000,
                              max_history_bindings=100000, max_history_nodes=500000):
    """Validate actual native relations and retain unknown axes as unavailable.

    Scope filters result rows after their retained dependencies are read. Budgets
    bound serialized operands/record counts, not whole-process memory or time.
    Alternative shedding windows stay separate records and are never summed.
    """
    require(scope is None or (type(scope) is str and bool(scope)), "nonempty scope required")
    require(all(type(value) is int and value > 0 for value in (max_events, max_bytes, max_relations)),
            "positive exact detector budgets required")
    result = {"detector": "native-particle-relations-v1", "coverage": "unavailable",
        "couplings": [], "folds": [], "shedding": [], "seating": [], "states": [],
        "waves": {}, "failures": [], "retained_bytes": 0, "inspected_events": 0,
        "coverage_claim": "recorded native relations and retained formation witnesses; no cavity evolution completeness claim",
        "absence_semantics": "missing relation evidence is unavailable, never false",
        "alternative_windows_additive": False}
    groups, wave_errors, folds, pending = {}, {}, {}, []
    contacts_by_pair, observed_states, seen_events = {}, {}, {}
    source_events = set()
    history = RetainedHistoryUnion(max_bindings=max_history_bindings, max_nodes=max_history_nodes)

    def inconsistent_union():
        result["history_union"] = history.status()
        if history.failure is None:
            return False
        # The query has one inspected witness union. Do not publish dependent
        # relations from a contradictory or truncated union as verified.
        result["failures"].append({"status": "rejected", "reason": history.failure,
                                   "scope": "inspected retained history union"})
        for category in ("couplings", "folds", "shedding", "seating", "states"):
            result[category] = []
        for wave in result["waves"].values():
            for axis in ("closure", "coupling", "shedding", "seating"):
                wave[axis] = {"status": "unresolved", "reason": "retained history union rejected"}
        result["archive_coverage"] = store.status()["coverage"]
        result["coverage"] = "partial"
        return True
    kinds = {"particle.native_return", "particle.native_contacts", "particle.native_fold",
             "particle.native_shedding", "particle.native_state"}
    serialization = TraceLimits(max_event_bytes=max_bytes, max_storage_bytes=max(65536, max_bytes * 4),
                                max_nodes=262144)
    try:
        for event in store.iter_events():
            result["inspected_events"] += 1
            encoded = canonical_bytes(event, serialization)
            result["retained_bytes"] += len(encoded)
            require(result["inspected_events"] <= max_events and result["retained_bytes"] <= max_bytes,
                    "native relation operand budget exhausted")
            event_id = event["event_id"]
            require(type(event_id) is str and event_id, "retained event identity required")
            if event_id in seen_events:
                require(seen_events[event_id] == sha256(encoded).digest(), "conflicting duplicate event identity")
                continue
            seen_events[event_id] = sha256(encoded).digest()
            kind, payload = event["kind"], event["payload"]
            if kind == "evidence.native_witness":
                require(payload.get("schema") == "native-particle-evidence-v1", "unknown native evidence wrapper")
                from .trace_applications import retrieve_blob
                raw = retrieve_blob(store, payload["locator"], max_bytes=max_bytes - result["retained_bytes"])
                result["retained_bytes"] += len(raw)
                kind, payload = payload["kind"], _json_payload(raw)
            if not kind.startswith("particle.native_"):
                continue
            require(kind in kinds, "unsupported native relation event kind: " + kind)
            if kind != "particle.native_return":
                require(type(payload.get("scale")) is int and type(payload.get("lineage")) is str and
                        (event["scope_id"], event["lineage_id"], event["scale_index"]) ==
                        (payload.get("scope"), payload["lineage"], payload["scale"]),
                        "native relation envelope disagrees with witness context")
            source_events.add(event["event_id"])
            require(len(source_events) <= max_relations, "native relation record budget exhausted")
            if kind == "particle.native_return":
                try:
                    trace, cert, motif, detected = _formation(payload)
                    history.include(trace, cert, motif)
                    require((event["scope_id"], event["lineage_id"], event["scale_index"]) ==
                            (trace.scope, trace.lineage, trace.scale), "native event envelope disagrees with witness context")
                    metadata = payload.get("wave")
                    require(type(metadata) is dict and type(metadata.get("identity")) is str,
                            "retained wave identity required for recurrence relations")
                    identity, ordinal = metadata["identity"], payload["return_ordinal"]
                    require(type(ordinal) is int and ordinal >= 0, "exact return ordinal required")
                    group = groups.setdefault(identity, {})
                    record = (trace, cert, motif, detected, event["event_id"])
                    require(ordinal not in group or group[ordinal][:3] == record[:3],
                            "conflicting retained return ordinal")
                    group[ordinal] = record
                except (ValueError, TypeError, KeyError, AttributeError) as exc:
                    identity = payload.get("wave", {}).get("identity", event["event_id"])
                    wave_errors[identity] = str(exc)
                    result["failures"].append({"event_id": event["event_id"], "kind": kind,
                                               "status": "rejected", "reason": str(exc)})
            else:
                pending.append((event["event_id"], kind, payload))
    except (TraceError, ValueError, TypeError, KeyError, AttributeError) as exc:
        result["failures"].append({"status": "incomplete", "reason": str(exc)})

    waves = {}
    if inconsistent_union():
        return result
    for identity, group in groups.items():
        try:
            require(identity not in wave_errors, wave_errors.get(identity, "rejected retained formation"))
            require(sorted(group) == list(range(len(group))), "retained wave prefix has missing returns")
            values = [group[index] for index in range(len(group))]
            registry = {value[1].certificate_digest: value[1] for value in values}
            wave = native.wave(tuple((v[0], v[1], registry) for v in values), tuple(v[2] for v in values))
            require(wave.identity == identity, "wave metadata identity differs from reconstructed recurrence")
            history.include(wave)
            waves[identity] = wave
            if scope is None or wave.scope == scope:
                result["waves"][identity] = {"scope": wave.scope, "lineage": wave.lineage,
                    "core": wave.motif, "return_ids": [t.return_id for t in wave.returns],
                    "closure": {"status": "verified", "phases": len(wave.returns), "beats": wave.bip},
                    "coupling": {"status": "unavailable", "records": []},
                    "shedding": {"status": "unavailable", "records": []},
                    "seating": {"status": "unavailable", "records": []}}
        except (ValueError, TypeError, KeyError, AttributeError) as exc:
            wave_errors[identity] = str(exc)
            result["failures"].append({"wave_identity": identity, "status": "unresolved", "reason": str(exc)})

    if inconsistent_union():
        return result

    def publish(category, row, identities=()):
        if scope is None or row.get("scope") == scope:
            result[category].append(row)
            axis = {"couplings": "coupling", "shedding": "shedding", "seating": "seating"}.get(category)
            if axis:
                for identity in identities:
                    if identity in result["waves"]:
                        result["waves"][identity][axis]["status"] = "verified"
                        result["waves"][identity][axis]["records"].append(row["event_id"])

    # Fold dependencies are reconstructed before dependent shedding records,
    # irrespective of storage serialization order.
    priorities = {"particle.native_contacts": 0, "particle.native_state": 1,
                  "particle.native_shedding": 2}
    for event_id, kind, payload in pending:
        if kind != "particle.native_fold":
            continue
        try:
            require(payload.get("schema") == "native-particle-fold-v1", "unknown fold schema")
            fold = _fold(payload["fold"])
            history.include(fold["outer"][0], fold["outer"][1], *fold["inner"])
            require((payload["scope"], payload["lineage"], payload["scale"]) ==
                    (fold["outer"][0].scope, fold["outer"][0].lineage, fold["outer"][0].scale),
                    "fold snapshot context differs from its outer closure")
            identity = fold["outer"][0].return_id
            require(identity not in folds or folds[identity] == fold, "conflicting outer fold witness")
            folds[identity] = fold
            publish("folds", {"event_id": event_id, "status": "verified", "scope": fold["outer"][0].scope,
                "outer_return_id": identity, "inner_wave_ids": [wave.identity for wave in fold["inner"]]})
        except (ValueError, TypeError, KeyError, AttributeError) as exc:
            result["failures"].append({"event_id": event_id, "kind": kind, "status": "rejected", "reason": str(exc)})

    if inconsistent_union():
        return result

    for event_id, kind, payload in sorted(pending, key=lambda row: priorities.get(row[1], -1)):
        try:
            if kind == "particle.native_contacts":
                require(payload.get("schema") == "native-particle-contacts-v1", "unknown contact schema")
                require(payload["left"] in waves and payload["right"] in waves,
                        "coupling lacks complete admitted recurrent prefixes")
                left, right = waves[payload["left"]], waves[payload["right"]]
                contacts = tuple(_contact(row) for row in payload["contacts"])
                interval = _tuple(payload["comparison_interval"])
                forward = native.compare(left, right, contacts, window=interval)
                reverse = native.compare(left, right, contacts, window=interval, reverse=True)
                for contact in contacts:
                    history.include(contact.support_trace, contact.support_certificate)
                require(payload["scope"] == left.scope, "contact snapshot scope differs")
                require(payload["scale"] == left.scale and payload["lineage"] == left.identity + "|" + right.identity,
                        "contact snapshot lineage/scale differs")
                evidence = payload["reflection_evidence"]
                require(type(evidence) in (list, tuple) and len(evidence) == len(contacts),
                        "contact reflection occurrence evidence unavailable")
                previous = -1
                for contact, reflected in zip(contacts, evidence):
                    occurrences = tuple(tuple(t.return_id for t in wave.returns
                        if previous < t.indices[-1] <= contact.committed) for wave in (left, right))
                    counts = tuple(map(len, occurrences))
                    require(contact.reflection == counts and reflected == {
                        "contact_id": contact.identity, "interval": [previous, contact.committed],
                        "reflected_return_ids": _plain(occurrences), "reflection_counts": list(counts)},
                        "directional reflection counts differ from actual retained occurrences")
                    previous = contact.committed
                contacts_by_pair[(left.identity, right.identity, _producer_digest(forward))] = forward
                publish("couplings", {"event_id": event_id, "status": "verified", "scope": left.scope,
                    "left": left.identity, "right": right.identity, "contacts": [c.identity for c in contacts],
                    "comparison_interval": list(interval), "forward": forward, "reverse": reverse},
                    (left.identity, right.identity))
            elif kind == "particle.native_shedding":
                require(payload.get("schema") == "native-particle-shedding-v1", "unknown shedding schema")
                record, state = _shedding(payload["record"]), _state(payload["source_state"])
                require(record.outer_return_id in folds, "shedding lacks retained outer closure")
                fold = folds[record.outer_return_id]
                native.validate_shedding(fold, record)
                require(native.digest(asdict(state)) == record.source_state_digest,
                        "shedding source state witness differs")
                state_key = (record.source_identity, record.source_state_digest)
                require(state_key in observed_states and observed_states[state_key] == state,
                        "shedding source lacks its validated recorded state")
                bridge = payload["model_bridge"]
                require(type(bridge) is dict and _producer_digest(bridge) == record.model_bridge,
                        "shedding model bridge bytes differ from bound digest")
                require(bridge.get("law") == "P=sum C3*rho_window*abs(lag), one declared directional channel",
                        "unsupported shedding load bridge")
                channel = bridge["channel"]
                pair = (channel["rcd"]["left"], channel["rcd"]["right"], _producer_digest(channel))
                require(pair in contacts_by_pair and channel == _plain(contacts_by_pair[pair]),
                        "shedding bridge lacks matching recomputed directional contact")
                require(pair[0] == record.source_identity, "shedding bridge source differs")
                require(type(bridge["window"]) is int and bridge["window"] >= 0 and
                        payload["participation_window"] == bridge["window"] and
                        payload["participation_domain"] == contacts_by_pair[pair]["rcd"].domain,
                        "shedding participation window/domain differs")
                participation = native.participation(contacts_by_pair[pair]["rcd"], bridge["window"],
                                                     payload["participation_domain"])
                capacity, lag = _ratio(bridge["capacity"]), contacts_by_pair[pair]["lag"]
                require(capacity >= 0 and bridge["participation"] == participation and
                        _ratio(bridge["lag"]) == lag and
                        record.closure_load == capacity * participation * abs(lag) and
                        record.closure_cost == _ratio(bridge["closure_cost"]),
                        "shedding operands differ from source-bound participation and lag")
                rebuilt = native.shedding(fold, state, record.closure_load, record.closure_cost,
                    source_event_digest=record.source_event_digest, model_bridge=record.model_bridge,
                    committed=record.committed, route=record.route,
                    target_scope=record.target_scope, target_scale=record.target_scale)
                require(rebuilt == record, "shedding witness does not regenerate its exact residual")
                require(payload["scope"] == record.source_scope, "shedding scope differs")
                source = next(wave for wave in fold["inner"] if wave.identity == record.source_identity)
                require((payload["lineage"], payload["scale"]) == (source.lineage, source.scale),
                        "shedding snapshot lineage/scale differs")
                publish("shedding", {"event_id": event_id, "status": "verified", "scope": record.source_scope,
                    "source_identity": record.source_identity, "state": record.status,
                    "closure_load": record.closure_load, "closure_cost": record.closure_cost,
                    "residual": record.residual, "surplus": record.surplus, "route": record.route,
                    "participation_window": payload["participation_window"],
                    "participation_domain": payload["participation_domain"],
                    "occurrence_semantics": payload["occurrence_semantics"],
                    "outer_return_id": record.outer_return_id}, (record.source_identity,))
            elif kind == "particle.native_state":
                require(payload.get("schema") == "native-particle-state-v1", "unknown state schema")
                identity = payload["wave_identity"]
                require(identity in waves, "state lacks complete admitted wave history")
                wave, state = waves[identity], _state(payload["state"])
                require(payload["scope"] == wave.scope, "state snapshot scope differs")
                require((payload["lineage"], payload["scale"]) == (wave.lineage, wave.scale),
                        "state snapshot lineage/scale differs")
                if payload["operation"] == "initial_with_terminal_orientation":
                    expected = replace(native.initial_state(wave), orientation=wave.returns[-1].roles[0])
                    require(state == expected, "initial state differs from actual retained wave")
                    observed_states[(identity, native.digest(asdict(state)))] = state
                    publish("states", {"event_id": event_id, "status": "verified", "scope": wave.scope,
                        "wave_identity": identity, "operation": payload["operation"], "state": state})
                elif payload["operation"] == "seat_duon":
                    before = _state(payload["before"])
                    require(type(payload["compatible"]) is bool, "typed seating compatibility required")
                    require(before == native.initial_state(wave), "seating input is not the recorded initial current")
                    expected = native.seat_duon(wave, before, payload["compatible"])
                    require(state == expected, "seated state differs from executed Duon seating")
                    observed_states[(identity, native.digest(asdict(state)))] = state
                    publish("seating", {"event_id": event_id, "status": "verified", "scope": wave.scope,
                        "wave_identity": identity, "state": "Duad", "current": state.current,
                        "before_current": before.current, "native_committed": state.committed,
                        "receipt_required_for_this_claim": False}, (identity,))
                else:
                    raise RelationError("unknown native state operation")
        except (ValueError, TypeError, KeyError, AttributeError) as exc:
            result["failures"].append({"event_id": event_id, "kind": kind, "status": "unresolved", "reason": str(exc)})
    if inconsistent_union():
        return result
    coverage = store.status()["coverage"]
    result["archive_coverage"] = coverage
    result["coverage"] = ("unavailable" if not source_events else "partial"
        if result["failures"] or coverage != "complete" else "complete")
    return result
