"""Infer native support shapes from retained construction evidence.

Names and diagnostic targets are not detector inputs. Ordered construction blocks
and the ancestral shell determine p:q; canonical native validators bind their
component occurrences and returned path. Record/window length, RD and Q4 path
length remain different quantities. A window view never creates a new return.
"""
from base64 import b64decode
from dataclasses import asdict, fields
import gzip
import io
import json

from .vendor import aod_manual_native as native
from .trace_store import canonical_bytes
from .trace_views import Window
from .trace_history_union import RetainedHistoryUnion


class SupportError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise SupportError(message)


def _fields(value, cls):
    require(type(value) is dict and set(value) == {f.name for f in fields(cls)},
            "incomplete or unexpected " + cls.__name__ + " fields")
    return dict(value)


def _tuple(value):
    require(type(value) in (list, tuple), "retained sequence required")
    return tuple(_tuple(v) if type(v) in (list, tuple) else v for v in value)


def _trace(value):
    row = _fields(value, native.Trace)
    for key in ("event_ids", "vertices", "indices", "roles"):
        row[key] = _tuple(row[key])
    if row["reflection"] is not None:
        reflection = _fields(row["reflection"], native.ReflectionWitness)
        for key in ("outbound_ids", "return_ids"):
            reflection[key] = _tuple(reflection[key])
        row["reflection"] = native.ReflectionWitness(**reflection)
    for key in ("scope", "lineage", "ancestor"):
        require(type(row[key]) is str and bool(row[key]), "exact native identity required")
    require(all(type(v) is int for v in row["vertices"]), "exact Q4 vertices required")
    require(all(type(v) is str and v for v in row["event_ids"]), "exact native event identities required")
    require(type(row["reflected_comparison"]) is bool, "typed reflection comparison required")
    result = native.Trace(**row)
    native.validate_trace(result)
    return result


def _certificate(value, depth=0):
    require(depth < 16, "native parent depth budget exhausted")
    row = _fields(value, native.Certificate)
    row["inner_waves"] = tuple(_wave(w, depth + 1) for w in _tuple(row["inner_waves"]))
    return native.Certificate(**row)


def _motif(value, depth=0):
    row = _fields(value, native.MotifWitness)
    row["construction"] = _tuple(row["construction"])
    row["support_trace"] = _trace(row["support_trace"])
    row["closure"] = _certificate(row["closure"], depth)
    row["component_certificates"] = tuple(_certificate(c, depth) for c in _tuple(row["component_certificates"]))
    row["component_traces"] = tuple(_trace(t) for t in _tuple(row["component_traces"]))
    require(type(row["enclosure"]) is int and type(row["chirality"]) is int,
            "exact enclosure and chirality required")
    return native.MotifWitness(**row)


def _wave(value, depth=0):
    require(depth < 16, "native parent depth budget exhausted")
    row = _fields(value, native.Wave)
    row["returns"] = tuple(_trace(t) for t in _tuple(row["returns"]))
    row["certificates"] = tuple(_certificate(c, depth + 1) for c in _tuple(row["certificates"]))
    row["motif_witnesses"] = tuple(_motif(m, depth + 1) for m in _tuple(row["motif_witnesses"]))
    result = native.Wave(**row)
    native.validate_wave(result)
    return result


def _shape(construction):
    """Infer structural syntax before checking a producer's claimed name."""
    if len(construction) == 2:
        blocks, shell = construction
        require(type(blocks) is tuple and type(shell) is tuple and blocks,
                "ordered support blocks and retained shell required")
        p, q = len(blocks), len(shell)
        require(1 <= p <= 64 and 1 <= q <= 64, "support shape budget exhausted")
        require(all(type(block) is tuple and len(block) == q for block in blocks),
                "support blocks and ancestral shell have different arities")
        return f"{p}:{q}", (p, q)
    if len(construction) == 3:
        left, pivot, right = construction
        require(all(type(pair) is tuple and len(pair) == 2 and
                    all(type(sign) is int and sign in (-1, 1) for sign in pair)
                    for pair in (left, right)) and
                type(pivot) is tuple and len(pivot) == 2 and
                type(pivot[1]) is int and pivot[1] in (-1, 0, 1),
                "exact integer Dion and pivot signs required")
        require(left in ((1, -1), (-1, 1)) and right in ((1, -1), (-1, 1))
                and type(pivot) is tuple and len(pivot) == 2,
                "unrecognized Dion-pivot-Dion construction")
        return "Duon", None
    raise SupportError("unrecognized retained construction")


def inspect_native_return(payload, *, record_window=None, evidence=(), _history=None):
    """Inspect an actual native return snapshot without a requested motif label.

    ``record_window`` is an optional declared *view* in the Manual additive
    native-count specialization. It is not measured from serialized events or
    from this trace's Q4 path. Its qualifier is never a detected physical form.
    """
    require(record_window is None or (type(record_window) is int and 0 <= record_window <= 1000000),
            "record window must be a bounded exact nonnegative native count")
    row = {"status": "unresolved", "evidence": list(evidence), "support": None,
           "signature": None, "record_window_view": None, "physical_species": None,
           "closure": {"status": "unavailable"},
           "coupling": {"status": "unavailable", "reason": "requires bound repeated contact evidence"},
           "shedding": {"status": "unavailable", "reason": "requires bound burden/route evidence"},
           "seating": {"status": "unavailable", "reason": "Duad state requires bound Duon seating evidence"}}
    try:
        require(type(payload) is dict, "native return payload required")
        canonical_bytes(payload)  # Same exact depth/node/byte limits as a stored event.
        trace = _trace(payload.get("trace"))
        row.update(return_id=trace.return_id, scope=trace.scope, lineage=trace.lineage,
                   scale=trace.scale, first_cut=trace.indices[0], committed_cut=trace.indices[-1],
                   native_event_ids=list(trace.event_ids), trace_digest=trace.trace_digest,
                   family=payload.get("family", "archived-native-evidence"),
                   capture=payload.get("capture", "retained-witness-input"))
        # An open/censored record or missing certificate cannot be promoted by
        # matching p:q alone. _trace has already validated the full returned path.
        require(payload.get("certificate") is not None, "committed closure certificate unavailable")
        certificate = _certificate(payload["certificate"])
        native.admitted(trace, certificate, {certificate.certificate_digest: certificate})
        row["closure"] = {"status": "verified", "return_id": trace.return_id,
                          "certificate_digest": certificate.certificate_digest}
        claim = _motif(payload["motif_witness"]) if payload.get("motif_witness") is not None else None
        raw_formation = payload.get("formation")
        if raw_formation is not None:
            require(type(raw_formation) is dict and set(raw_formation) ==
                    {"construction", "component_traces", "component_certificates"},
                    "raw formation requires construction and bound component records")
            construction = _tuple(raw_formation["construction"])
            component_traces = tuple(_trace(t) for t in _tuple(raw_formation["component_traces"]))
            component_certificates = tuple(_certificate(c) for c in _tuple(raw_formation["component_certificates"]))
        else:
            require(claim is not None, "retained higher-support construction unavailable")
            construction, component_traces, component_certificates = (
                claim.construction, claim.component_traces, claim.component_certificates)
        inferred, shape = _shape(construction)
        row["inferred_core"] = inferred
        require(len(component_traces) == len(component_certificates),
                "component trace/certificate mismatch")
        traversals = native.phase_step_count(trace)
        simple_cycle = len(set(trace.vertices[:-1])) == traversals
        # Native grammar validation carries an enclosure parameter. Where a
        # complete outer cycle is absent (e.g. a Duon E2 path), its canonical
        # minimum is only an admission parameter, never an observed enclosure.
        validation_enclosure = traversals if simple_cycle and traversals >= 6 and traversals % 2 == 0 else 6
        registry = {c.certificate_digest: c for c in (*component_certificates, certificate)}
        motif = native.motif_witness(inferred, trace, construction,
            tuple(zip(component_traces, component_certificates)), registry,
            validation_enclosure if claim is None else claim.enclosure, trace.roles[0])
        if claim is not None:
            require(claim == motif, "claimed motif fields disagree with inferred construction and canonical witness")
        wave_claim = payload.get("wave")
        if wave_claim is not None:
            require(type(wave_claim) is dict and
                    (wave_claim.get("scope"), wave_claim.get("scale"), wave_claim.get("lineage")) ==
                    (trace.scope, trace.scale, trace.lineage), "wave claim changed native context")
            require(wave_claim.get("declared_motif") == inferred,
                    "wave label differs from inferred construction")
        native.validate_motif(motif)
        if _history is not None:
            _history.include(trace, certificate, motif)
        hinges = len(trace.roles) - 2
        enclosure_verified = traversals == motif.enclosure and simple_cycle
        row.update(status="verified", reason="shape inferred from bound component construction; closure independently validated",
                   signature=inferred, chirality=trace.roles[0], component_return_ids=[c.return_id for c in motif.component_certificates],
                   witness_digest=motif.witness_digest, certificate_digest=certificate.certificate_digest,
                   atomic_traversals=traversals, hinge_roles=hinges,
                   construction_origin="raw formation operands" if raw_formation is not None else "legacy retained witness operands",
                   enclosure={"declared": claim.enclosure if claim is not None else None,
                              "grammar_validation_parameter": motif.enclosure,
                              "observed_simple_cycle": traversals if enclosure_verified else None,
                              "status": "verified" if enclosure_verified else "unresolved"})
        row["detected_support_form"] = inferred + ":" + str(traversals) if enclosure_verified else None
        if shape is not None:
            p, q = shape
            rd = native.rd(p, q, hinges=hinges) if q >= 2 else None
            row["support"] = {"p": p, "q": q, "Pi": q ** p + q, "RD": rd,
                "RD_status": "verified" if rd is not None else "unavailable: canonical RD accessor requires q >= 2",
                "domain": "Main finite support model; symmetric additive native count",
                "RCD": None, "RCD_status": "requires separately bound repeated directional contacts"}
            if record_window is not None:
                row["record_window_view"] = {"signature": f"{p}:{q}:{record_window}",
                    "classification": "declared support-window view, not detected new motif",
                    "record_window": record_window, "domain": "additive-native-count",
                    "rho": rd, "rho_window": min(rd, record_window) if rd is not None else None, "RD": rd,
                    "RCD": None, "event_identity": trace.return_id}
        return row
    except (ValueError, TypeError, KeyError, AttributeError, OverflowError, RecursionError) as exc:
        row.update(status="unresolved" if "unavailable" in str(exc) else "rejected", reason=str(exc),
                   signature=None, record_window_view=None, support=None)
        return row


# Shared exact decoders for relation detectors; no model state is changed.
decode_trace = _trace
decode_certificate = _certificate
decode_wave = _wave
decode_motif = _motif


def _json(data, max_nodes):
    def integer(value):
        require(len(value.lstrip("-")) <= 128, "native JSON integer budget exhausted")
        return int(value)
    def no_float(value):
        raise SupportError("inexact number in native witness")
    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate native JSON field")
            result[key] = value
        return result
    result = json.loads(data, parse_int=integer, parse_float=no_float, parse_constant=no_float,
                        object_pairs_hook=unique_object)
    stack, nodes = [(result, 0)], 0
    while stack:
        value, depth = stack.pop(); nodes += 1
        require(nodes <= max_nodes and depth <= 64, "native witness node/depth budget exhausted")
        if type(value) is dict:
            stack.extend((v, depth + 1) for v in value.values())
        elif type(value) is list:
            stack.extend((v, depth + 1) for v in value)
    return result


def _legacy_returns(data, input_groups):
    require(type(data) is dict, "native witness archive must be an object")
    source_present = "encounter_world" in data
    if source_present:
        require(type(data["encounter_world"]) is dict and "waves" in data["encounter_world"],
                "malformed archived encounter world")
    routed = []
    route_present = "route_witnesses" in data
    if route_present:
        require(type(data["route_witnesses"]) in (list, tuple), "archived route witness collection required")
        for route in data["route_witnesses"]:
            require(type(route) is dict and type(route.get("route_support")) is dict
                    and "wave" in route["route_support"], "malformed archived routed support")
            routed.append(route["route_support"]["wave"])
    groups = (("source-encounter", data["encounter_world"]["waves"] if source_present else (), source_present),
              ("proton-carrier", data.get("carrier_waves", ()), "carrier_waves" in data),
              ("routed-support", routed, route_present))
    for family, waves, present in groups:
        description = input_groups.setdefault(family, {"status": "absent", "capture_records": None})
        if present:
            description["status"] = "present"
            if description["capture_records"] is None:
                description["capture_records"] = 0
        require(type(waves) in (list, tuple), "native wave collection required")
        for wave in waves:
            require(type(wave) is dict and {"returns", "certificates", "motif_witnesses"} <= set(wave),
                    "native wave object with retained histories required")
            returns, certificates, motifs = (wave[k] for k in ("returns", "certificates", "motif_witnesses"))
            require(all(type(values) in (list, tuple) for values in (returns, certificates, motifs)),
                    "native retained histories must be sequences")
            require(len(returns) == len(certificates) == len(motifs), "archived formation histories have different lengths")
            for ordinal, (trace, certificate, motif) in enumerate(zip(returns, certificates, motifs)):
                description["capture_records"] += 1
                yield {"schema": "native-particle-return-v1", "family": family, "return_ordinal": ordinal,
                    "wave": {"identity": wave.get("identity"), "scope": wave.get("scope"),
                             "scale": wave.get("scale"), "lineage": wave.get("lineage"),
                             "declared_motif": wave.get("motif")},
                    "trace": trace, "certificate": certificate, "motif_witness": motif}


def detect_particle_support(store, *, window=None, scope=None, record_window=None,
                            max_events=10000, max_bytes=8000000, max_expanded_bytes=16000000,
                            max_returns=10000, max_nodes=500000,
                            max_history_bindings=100000, max_history_nodes=500000):
    """Detect support from typed native commits, or replay their legacy archive.

    ``window`` selects native construction cuts, not host serialization ordinals.
    ``record_window`` is a separate declared participation view. Complete means
    the inspected retained-witness collection was covered, not full collision
    dynamics, sensor acceptance, or Tau production was established.
    """
    for limit in (max_events, max_bytes, max_expanded_bytes, max_returns, max_nodes):
        require(type(limit) is int and limit > 0, "positive exact detector budgets required")
    require(scope is None or (type(scope) is str and bool(scope)), "nonempty native scope required")
    require(window is None or (type(window) is Window and window.role == "observation"
            and window.domain == "native-committed-cut"), "native committed-cut observation window required")
    require(record_window is None or (type(record_window) is int and 0 <= record_window <= 1000000),
            "record window must be a bounded exact nonnegative native count")
    result = {"detector": "native-support-from-construction-v1", "coverage": "unavailable",
        "scope": scope, "window": None if window is None else asdict(window),
        "record_window": record_window, "returns": [], "censored": [], "errors": [],
        "signatures": {},
        "application_classification": {"status": "unavailable", "value": None,
            "reason": "structural detection alone does not classify an application or species"},
        "input_capture": {"status": "unavailable", "mode": None},
        "input_groups": {},
        "coverage_claim": "retained native formation witnesses only; no complete cavity evolution claim",
        "resources": {"scanned_events": 0, "scanned_bytes": 0, "expanded_bytes": 0}}
    seen, identities, event_seen, archives = {}, {}, {}, {}
    history = RetainedHistoryUnion(max_bindings=max_history_bindings, max_nodes=max_history_nodes)
    typed = False

    def consume(payload, locator, envelope=None):
        row = inspect_native_return(payload, record_window=record_window, evidence=(locator,), _history=history)
        raw_trace = payload.get("trace", {})
        if envelope is not None and type(raw_trace) is dict:
            require((envelope["scope_id"], envelope["lineage_id"], envelope["scale_index"]) ==
                    (raw_trace.get("scope"), raw_trace.get("lineage"), raw_trace.get("scale")),
                    "native event envelope disagrees with witness context")
        if row.get("return_id") is not None:
            identity = (row["scope"], row["scale"], row["lineage"], row["native_event_ids"][0], row["native_event_ids"][-1])
            fingerprint = native.digest(payload.get("trace"))
            require(identity not in identities or identities[identity] == fingerprint,
                    "conflicting native history under one retained terminal identity")
            identities[identity] = fingerprint
            key = row["return_id"]
            binding = native.digest({k: payload.get(k) for k in ("trace", "certificate", "motif_witness", "formation")})
            if key in seen:
                require(seen[key][0] == binding, "conflicting duplicate native formation witness")
                if locator not in seen[key][1]["evidence"]:
                    seen[key][1]["evidence"].append(locator)
                return
            require(len(seen) < max_returns, "native return budget exhausted")
            seen[key] = (binding, row)
            if scope is not None and row["scope"] != scope:
                return
            if window is not None and not window.contains(row["committed_cut"], window.domain):
                if row["first_cut"] < window.stop <= row["committed_cut"] and window.start < window.stop:
                    result["censored"].append({"return_id": key, "status": "right_censored", "evidence": [locator],
                        "reason": "return completion is outside the observation window"})
                return
        result["returns"].append(row)

    try:
        for event in store.iter_events():
            encoded = canonical_bytes(event)
            previous = event_seen.get(event["event_id"])
            if previous is not None:
                require(previous == encoded, "conflicting repeated trace event")
                continue
            require(result["resources"]["scanned_events"] < max_events and
                    result["resources"]["scanned_bytes"] + len(encoded) <= max_bytes,
                    "native detector event/byte budget exhausted")
            event_seen[event["event_id"]] = encoded
            result["resources"]["scanned_events"] += 1
            result["resources"]["scanned_bytes"] += len(encoded)
            if event["kind"] in ("particle.native_return", "native.formation"):
                typed = True
                require(event["payload"].get("schema") == "native-particle-return-v1", "unknown native return schema")
                family = event["payload"].get("family")
                require(type(family) is str and family, "native return capture family required")
                description = result["input_groups"].setdefault(family, {"status": "present", "capture_records": 0})
                description["capture_records"] += 1
                consume(event["payload"], event["event_id"], event)
            elif event["kind"] == "evidence.blob_chunk" and event["payload"].get("name") == "proton-accounting-witnesses.json.gz":
                payload = event["payload"]
                key = (event["scope_id"], payload["name"], payload["sha256"])
                require(len(archives) < 4 or key in archives, "native archive count budget exhausted")
                group = archives.setdefault(key, {"parts": payload["parts"], "chunks": {}, "last": None})
                require(type(payload["part"]) is int and type(payload["parts"]) is int and
                        0 <= payload["part"] < payload["parts"] <= max_events and
                        group["parts"] == payload["parts"] and payload["encoding"] == "base64",
                        "invalid native archive chunk metadata")
                size = len(b64decode(payload["data"], validate=True))
                require(payload["part"] not in group["chunks"], "repeated native archive chunk")
                group["chunks"][payload["part"]] = size
                if payload["part"] == payload["parts"] - 1:
                    group["last"] = event["event_id"]
        if not typed:
            from .trace_applications import retrieve_blob
            for (archive_scope, name, digest), group in archives.items():
                require(set(group["chunks"]) == set(range(group["parts"])), "native archive has missing chunks")
                locator = {"name": name, "sha256": digest, "bytes": sum(group["chunks"].values()),
                    "parts": group["parts"], "scope": archive_scope, "last_event": group["last"]}
                compressed = retrieve_blob(store, locator, max_bytes=max_bytes)
                remaining = max_expanded_bytes - result["resources"]["expanded_bytes"]
                with gzip.GzipFile(fileobj=io.BytesIO(compressed)) as stream:
                    expanded = stream.read(remaining + 1)
                require(len(expanded) <= remaining, "native archive expanded-byte budget exhausted")
                result["resources"]["expanded_bytes"] += len(expanded)
                for payload in _legacy_returns(_json(expanded, max_nodes), result["input_groups"]):
                    consume(payload, group["last"])
        result["coverage"] = "complete" if typed or archives else "unavailable"
        result["input_capture"] = {"status": "available" if typed or archives else "unavailable",
            "mode": "typed-native-return-snapshots" if typed else ("legacy-witness-archive" if archives else None),
            "scope": "retained committed return evidence; no claim to observe all open collision paths"}
        if any(row["status"] != "verified" for row in result["returns"]):
            result["coverage"] = "partial"
        if hasattr(store, "status") and store.status()["coverage"] != "complete":
            result["coverage"] = "partial"
            result["errors"].append("trace archive is incomplete")
    except (ValueError, TypeError, KeyError, AttributeError, OSError, EOFError, RecursionError) as exc:
        result["coverage"] = "partial"
        result["errors"].append(str(exc))
    result["history_union"] = history.status()
    if history.failure is not None:
        result["coverage"] = "partial"
        result["errors"].append(history.failure)
        for row in result["returns"]:
            row.update(status="rejected", reason="retained history union: " + history.failure,
                       signature=None, support=None, record_window_view=None, detected_support_form=None,
                       closure={"status": "unresolved", "reason": "retained history union rejected"})
            if "enclosure" in row:
                row["enclosure"].update(status="unresolved", observed_simple_cycle=None,
                                        reason="retained history union rejected")
    for row in result["returns"]:
        if row["status"] == "verified":
            result["signatures"][row["signature"]] = result["signatures"].get(row["signature"], 0) + 1
    result["observed_formations"] = sum(result["signatures"].values())
    result["event_identity_rule"] = "native retained return identity; view parameters never create a new occurrence"
    return result
