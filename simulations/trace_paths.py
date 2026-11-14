"""Join retained shedding, route, support, seating and receipt evidence.

The query checks cross-dialect bindings which neither native relation validation
nor accounting validation establishes alone. It adds no propagation or cavity
law. Alternative observation windows remain separate branches of one history.
"""
from dataclasses import asdict
from fractions import Fraction
from hashlib import sha256
from pathlib import Path

from .vendor import aod_manual_native as native
from . import boundary_accounting as accounting_model
from .trace_store import canonical_bytes
from .trace_applications import replay_accounting, retrieve_blob, _ratio as accounting_ratio, _restore_record
from .trace_particle_relations import (detect_particle_relations, _formation, _state,
    _shedding, _contact, _producer_digest, _json_payload, _ratio as native_ratio)


class PathError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise PathError(message)


class _View:
    def __init__(self, store, events):
        self.store, self.events = store, events

    def iter_events(self):
        return iter(self.events)

    def get_by_key(self, key):
        return self.store.get_by_key(key)

    def status(self):
        return self.store.status()


def _route_record(value, number):
    return _restore_record(accounting_model.RouteRecord, value, surplus=number,
        legs=lambda rows: tuple(_restore_record(accounting_model.RouteLeg, row, amount=number) for row in rows))


def _accounting_number(value):
    return value if type(value) is Fraction else accounting_ratio(value)


def trace_sheddic_paths(store, *, scope=None, max_events=10000,
                        max_bytes=32 * 1024 * 1024, max_paths=1000):
    """Traverse actual evidence links with exact amount and identity checks.

    ``coverage`` concerns recorded native-shedding-to-receipt evidence. The
    separate ``population_to_path_coverage`` requires a validated encounter
    initialization, transition stream and finish, when that dialect is present.
    Missing receipts are unobserved unless a terminal disposition witnesses a
    pending allocation. Missing formation cannot be replaced by a destination
    label or by an accounting receipt.
    """
    require(scope is None or (type(scope) is str and scope), "nonempty native scope required")
    require(all(type(x) is int and x > 0 for x in (max_events, max_bytes, max_paths)),
            "positive exact path-query budgets required")
    result = {"detector": "native-sheddic-evidence-paths-v1", "coverage": "unavailable",
        "population_to_path_coverage": "unavailable", "paths": [], "alternatives": [],
        "failures": [], "alternative_windows_additive": False,
        "coverage_claim": "recorded native shedding through checked accounting and recipient witnesses",
        "encounter_ancestry": {"status": "unavailable", "reason": "encounter replay not inspected"},
        "encounter": {"coverage": "unavailable", "reason": "encounter replay not inspected"},
        "terminal_routing": {"status": "unavailable", "reason": "terminal route disposition not inspected"},
        "routing_plan": {"status": "unavailable", "reason": "routing setup not inspected"},
        "sensor_interaction": {"status": "unavailable", "reason": "receipt and seating do not establish sensor collision dynamics"},
        "resources": {"events": 0, "serialized_bytes": 0, "expanded_bytes": 0}}
    events, seen, payloads = [], {}, {}
    try:
        for event in store.iter_events():
            data = canonical_bytes(event)
            identity = event["event_id"]
            digest = sha256(data).digest()
            if identity in seen:
                require(seen[identity] == digest, "conflicting duplicate trace event")
                continue
            require(len(events) < max_events and result["resources"]["serialized_bytes"] + len(data) <= max_bytes,
                    "path-query event/byte budget exhausted")
            seen[identity] = digest
            events.append(event)
            result["resources"]["serialized_bytes"] += len(data)
            kind, payload = event["kind"], event["payload"]
            if kind == "evidence.native_witness":
                require(payload.get("schema") == "native-particle-evidence-v1", "unknown native evidence wrapper")
                remaining = max_bytes - result["resources"]["serialized_bytes"] - result["resources"]["expanded_bytes"]
                require(remaining > 0, "expanded native path-evidence budget exhausted")
                raw = retrieve_blob(store, payload["locator"], max_bytes=remaining)
                result["resources"]["expanded_bytes"] += len(raw)
                kind, payload = payload["kind"], _json_payload(raw)
            payloads[identity] = (kind, payload)
        result["resources"]["events"] = len(events)
        require(sum(result["resources"][k] for k in ("serialized_bytes", "expanded_bytes")) <= max_bytes,
                "combined path-evidence byte budget exhausted")
    except (ValueError, TypeError, KeyError, AttributeError, OSError) as exc:
        result["coverage"] = "partial"
        result["failures"].append({"stage": "capture", "reason": str(exc)})
        return result

    view = _View(store, events)
    relations = detect_particle_relations(view, max_events=max_events, max_bytes=max_bytes)
    result["native_relation_coverage"] = relations["coverage"]
    if relations["coverage"] == "partial":
        result["failures"].append({"stage": "native_relations", "reason": "native prerequisite validation incomplete",
                                   "details": relations["failures"]})
    try:
        accounting = replay_accounting(view, max_events=max_events, max_bytes=max_bytes)
    except (ValueError, TypeError, KeyError, AttributeError) as exc:
        result["coverage"] = "partial"
        result["failures"].append({"stage": "accounting", "reason": str(exc)})
        return result
    result["accounting_coverage"] = accounting.get("coverage", "validated recorded accounting")
    native_rows, native_occurrences = {}, {}
    for row in relations["shedding"]:
        digest = _shedding(payloads[row["event_id"]][1]["record"]).record_digest
        # Several captures of one immutable shedding record are evidence aliases,
        # not additional emitted burden or alternative physical occurrences.
        if digest not in native_occurrences:
            native_rows[row["event_id"]] = row
            native_occurrences[digest] = []
        native_occurrences[digest].append(row["event_id"])
    contact_rows = {row["event_id"]: row for row in relations["couplings"]}
    seat_rows = {row["wave_identity"]: row for row in relations["seating"]}
    route_rows = [row for row in accounting["transitions"] if row["kind"] == "validated_route"]
    receipt_rows = [row for row in accounting["transitions"] if row["kind"] == "validated_receipt"]
    plans = [(identity, payload) for identity, (kind, payload) in payloads.items() if kind == "particle.routing_plan"]
    planned_windows = None
    if plans:
        try:
            from . import proton_accounting as producer
            require(len(plans) == 1, "exactly one routing setup commitment required")
            plan_id, plan = plans[0]
            require(plan.get("schema") == "native-routing-plan-v1", "unknown routing setup schema")
            require(type(plan["scale"]) is int and
                    (plan["scope"], plan["lineage"], plan["scale"]) == (producer.SCOPE, "proton-routing", 0),
                    "routing setup context differs from bound driver")
            envelope = next(event for event in events if event["event_id"] == plan_id)
            require((envelope["scope_id"], envelope["lineage_id"], envelope["scale_index"]) ==
                    (plan["scope"], plan["lineage"], plan["scale"]), "routing setup envelope differs")
            modules = (producer.encounter, producer.support, producer.native, producer.accounting, producer)
            bindings = {str(Path(module.__file__).relative_to(producer.ROOT)):
                        sha256(Path(module.__file__).read_bytes()).hexdigest() for module in modules}
            require(plan["source_bindings"] == bindings, "routing setup source bindings differ from executed driver")
            require(type(plan["windows"]) is list and all(type(w) is int and w >= 0 for w in plan["windows"])
                    and tuple(plan["windows"]) == producer.WINDOWS,
                    "routing setup window schedule differs from bound driver input")
            require(native_ratio(plan["capacity"]) == producer.CAPACITY and
                    native_ratio(plan["closure_cost"]) == producer.CLOSURE_COST and
                    plan["burden_unit"] == producer.BURDEN_UNIT and plan["current_unit"] == producer.CURRENT_UNIT,
                    "routing setup capacity, closure cost or units differ from bound driver")
            require(all(_shedding(payloads[identity][1]["record"]).source_event_digest == plan["source_history_digest"]
                        and _shedding(payloads[identity][1]["record"]).outer_return_id == plan["outer_return_id"]
                        for identity in native_rows), "routing setup history or outer closure differs from native alternatives")
            planned_windows = tuple(plan["windows"])
            result["routing_plan"] = {"status": "verified", "event_id": plan_id, "windows": list(planned_windows),
                "source_bindings": bindings, "source_history_digest": plan["source_history_digest"],
                "scope": "actual finite driver routing inputs; no target structure or output probability"}
        except (ValueError, TypeError, KeyError, AttributeError) as exc:
            result["routing_plan"] = {"status": "rejected", "reason": str(exc)}
            result["failures"].append({"stage": "routing_plan", "reason": str(exc)})
    else:
        result["routing_plan"] = {"status": "unavailable", "reason": "no source-bound routing setup commitment"}
    terminal_pending = {}
    finishes = [(event_id, payload) for event_id, (kind, payload) in payloads.items()
                if kind == "particle.routing_finish"]
    if finishes:
        try:
            indices, counts, terminal_rows, used_branches = [], set(), [], set()
            native_digests = {_shedding(payloads[event_id][1]["record"]).record_digest: event_id for event_id in native_rows}
            for finish_id, finish in finishes:
                require(finish.get("schema") == "native-routing-finish-v1"
                        and finish.get("occurrence_semantics") == "non-additive alternative views",
                        "unknown terminal routing schema or alternative semantics")
                require(type(finish["alternative_count"]) is int and type(finish["alternative_index"]) is int
                        and 0 <= finish["alternative_index"] < finish["alternative_count"] <= max_events,
                        "invalid exact alternative terminal index/count")
                require(all(type(finish[key]) is int and finish[key] >= 0
                            for key in ("scale", "cut", "participation_window"))
                        and all(type(finish[key]) is str and finish[key] for key in ("scope", "lineage")),
                        "terminal routing context requires exact typed coordinates")
                if planned_windows is not None:
                    require(finish["alternative_count"] == len(planned_windows)
                            and finish["participation_window"] == planned_windows[finish["alternative_index"]],
                            "terminal alternative schedule differs from committed routing setup")
                indices.append(finish["alternative_index"]); counts.add(finish["alternative_count"])
                envelope = next(event for event in events if event["event_id"] == finish_id)
                require((envelope["scope_id"], envelope["lineage_id"], envelope["scale_index"]) ==
                        (finish["scope"], finish["lineage"], finish["scale"]), "terminal routing envelope context differs")
                digest = finish["source_native_shedding_digest"]
                require(digest in native_digests and digest not in terminal_pending, "terminal routing lacks unique validated native source")
                source_payload = payloads[native_digests[digest]][1]
                source_record = _shedding(source_payload["record"])
                require(finish["scope"] == source_record.source_scope and finish["cut"] == source_record.committed
                        and finish["participation_window"] == source_payload["participation_window"],
                        "terminal alternative differs from its native shedding context")
                bridge = source_payload["model_bridge"]
                flow_digest = _producer_digest({"channel": bridge["channel"], "window": bridge["window"],
                    "capacity": native_ratio(bridge["capacity"]), "attention": native_ratio(bridge["lag"])})
                branches = [(identity, payload) for identity, (kind, payload) in payloads.items()
                            if kind == "application.accounting.commit" and payload.get("application") == "proton-window-alternatives"
                            and payload["record"]["cut"] == source_record.committed
                            and any(pair[0]["source_digest"] == flow_digest for pair in payload["record"]["additions"])]
                require(len(branches) == 1 and branches[0][0] not in used_branches,
                        "terminal alternative has no unique matching accounting branch")
                branch_id = branches[0][0]; used_branches.add(branch_id)
                declared_routes = tuple(_route_record(row, native_ratio) for row in finish["routes"])
                declared_receipts = tuple(_restore_record(accounting_model.Receipt, row) for row in finish["receipts"])
                observed_routes = tuple(_route_record(row["record"], _accounting_number) for row in route_rows if row["branch_commit"] == branch_id)
                observed_receipts = tuple(_restore_record(accounting_model.Receipt, row["record"]) for row in receipt_rows if row["branch_commit"] == branch_id)
                require(_producer_digest(declared_routes) == _producer_digest(observed_routes),
                        "captured routes differ from terminal producer routing disposition")
                require(_producer_digest(declared_receipts) == _producer_digest(observed_receipts),
                        "captured receipts differ from terminal producer routing disposition")
                consumed = {receipt.leg_id for receipt in declared_receipts}
                pending = tuple({"route_id": route.identity, "leg_id": leg.identity, "destination": leg.destination,
                    "amount": leg.amount, "unit": route.unit, "committed": route.committed}
                    for route in declared_routes for leg in route.legs
                    if leg.identity not in consumed and leg.kind != "open_loss")
                require(_producer_digest(pending) == _producer_digest(finish["pending_routes"]),
                        "terminal pending disposition differs from route and receipt records")
                terminal_pending[digest] = {(row["route_id"], row["leg_id"]) for row in pending}
                terminal_rows.append({"event_id": finish_id, "branch_commit": branch_id,
                    "native_shedding_digest": digest, "alternative_index": finish["alternative_index"],
                    "routes": len(declared_routes), "receipts": len(declared_receipts), "pending": len(pending)})
            require(len(counts) == 1 and sorted(indices) == list(range(next(iter(counts)))),
                    "missing, repeated or inconsistent terminal alternative markers")
            if planned_windows is not None:
                require(len(finishes) == len(planned_windows), "committed routing setup has missing terminal alternatives")
            require(len(terminal_pending) == len(native_rows), "native alternatives lack terminal disposition coverage")
            result["terminal_routing"] = {"status": "verified", "alternatives": terminal_rows,
                "scope": "actual producer terminal routing snapshots; no statement about later unobserved evolution"}
        except (ValueError, TypeError, KeyError, AttributeError) as exc:
            result["terminal_routing"] = {"status": "rejected", "reason": str(exc)}
            result["failures"].append({"stage": "terminal_routing", "reason": str(exc)})
    else:
        result["terminal_routing"] = {"status": "unavailable", "reason": "no independent terminal routing snapshot"}

    # Reconstruct the same native waves consumed by the relation validator.
    wave_records, waves, wave_events = {}, {}, {}
    for event_id, (kind, payload) in payloads.items():
        if kind != "particle.native_return":
            continue
        identity = payload.get("wave", {}).get("identity")
        if identity not in relations["waves"]:
            continue
        try:
            trace, cert, motif, _ = _formation(payload)
            group = wave_records.setdefault(identity, {})
            group[payload["return_ordinal"]] = (trace, cert, motif)
            wave_events.setdefault(identity, []).append(event_id)
        except (ValueError, TypeError, KeyError, AttributeError) as exc:
            result["failures"].append({"stage": "formation", "event_id": event_id, "reason": str(exc)})
    for identity, group in wave_records.items():
        try:
            require(sorted(group) == list(range(len(group))), "routed wave has a missing return")
            records = [group[i] for i in range(len(group))]
            registry = {cert.certificate_digest: cert for _, cert, _ in records}
            wave = native.wave(tuple((trace, cert, registry) for trace, cert, _ in records),
                               tuple(motif for _, _, motif in records))
            require(wave.identity == identity, "reconstructed path wave identity differs")
            waves[identity] = wave
        except (ValueError, TypeError, KeyError, AttributeError) as exc:
            result["failures"].append({"stage": "formation", "wave_identity": identity, "reason": str(exc)})

    world = None
    encounter_present = any(kind.startswith("particle.encounter_") for kind, _ in payloads.values())
    if encounter_present:
        try:
            from .trace_encounter import reconstruct_encounter
            encounter, world = reconstruct_encounter(view, max_events=max_events, max_bytes=max_bytes)
            result["encounter"] = encounter
            result["encounter_ancestry"] = {"status": "verified" if world is not None else encounter["coverage"],
                "final_world_digest": encounter.get("final_world_digest"), "coverage": encounter["coverage"]}
        except (ImportError, ValueError, TypeError, KeyError, AttributeError) as exc:
            result["encounter_ancestry"] = {"status": "unavailable", "reason": str(exc)}
            result["encounter"] = {"coverage": "partial", "errors": [str(exc)]}
    else:
        result["encounter_ancestry"] = {"status": "unavailable",
            "reason": "legacy retained snapshots contain no initialization/update/finish stream"}
        result["encounter"] = {"coverage": "unavailable", "reason": result["encounter_ancestry"]["reason"]}

    ancestry_cache = {}
    mapped_population = None

    def source_ancestry(record, payload):
        nonlocal mapped_population
        channel = payload["model_bridge"]["channel"]
        key = (record.source_event_digest, record.source_identity, _producer_digest(channel))
        if key in ancestry_cache:
            return ancestry_cache[key]
        coupling = [row for row in contact_rows.values() if row["left"] == record.source_identity
                    and _producer_digest(row["forward"]) == _producer_digest(channel)]
        require(coupling, "shedding has no matching directional contact evidence")
        ancestry = {"status": "unavailable", "digest": record.source_event_digest,
                    "reason": "retained snapshot only; encounter replay unavailable"}
        if world is not None:
            matching_history = []
            for candidate in coupling:
                candidate_payload = payloads[candidate["event_id"]][1]
                candidate_history = {"source_world": world,
                    "carriers": (waves[candidate["left"]], waves[candidate["right"]]),
                    "contacts": tuple(_contact(c) for c in candidate_payload["contacts"]),
                    "reflection_evidence": candidate_payload["reflection_evidence"],
                    "interval": tuple(candidate_payload["comparison_interval"])}
                if _producer_digest(candidate_history) == record.source_event_digest:
                    matching_history.append(candidate)
            require(matching_history, "no directional contact history matches the executed population source digest")
            coupling = matching_history
            from . import proton_accounting as producer
            # Reuse only within this exact immutable population/query. This is
            # the existing driver preparation map, not a new proton update law.
            if mapped_population is None:
                with producer.observe_commits(None):
                    mapped_population = tuple(producer.materialize_carrier(source_wave, label, prepared)[0]
                        for source_wave, label, prepared in zip(world.waves, ("A", "B"), (3, 2)))
            require(mapped_population == (waves[coupling[0]["left"]], waves[coupling[0]["right"]]),
                    "proton carriers differ from the bound source-population materialization map")
            source_ids = {trace.return_id for wave in world.waves for trace in wave.returns}
            observed_ids = {trace.return_id for identity, wave in waves.items()
                            if any(payloads[e][1].get("family") == "source-encounter" for e in wave_events[identity])
                            for trace in wave.returns}
            require(source_ids == observed_ids, "captured source formations differ from replayed population history")
            ancestry = {"status": "verified", "digest": record.source_event_digest,
                "source_return_ids": sorted(source_ids),
                "materialization": {"status": "verified", "operation": "proton_accounting.materialize_carrier",
                    "source_sha256": sha256(Path(producer.__file__).read_bytes()).hexdigest(),
                    "scope": "existing finite driver maps source orientations into separately admitted 3:3 C6 carriers"}}
        result_value = coupling[0], [row["event_id"] for row in coupling], ancestry
        ancestry_cache[key] = result_value
        return result_value

    matched_routes = set()
    for event_id, native_row in native_rows.items():
        payload = payloads[event_id][1]
        try:
            record = _shedding(payload["record"])
            if scope is not None and record.source_scope != scope:
                continue
            alternative = {"event_id": event_id, "record_digest": record.record_digest,
                "evidence": native_occurrences[record.record_digest],
                "source_history_digest": record.source_event_digest, "model_bridge": record.model_bridge,
                "participation_window": payload["participation_window"], "state": record.status,
                "surplus": str(record.surplus), "additive_with_other_windows": False}
            result["alternatives"].append(alternative)
            coupling, coupling_evidence, ancestry = source_ancestry(record, payload)
            alternative["source_history"] = ancestry
            if record.surplus <= 0:
                continue
            candidates = [row for row in route_rows if row["record"]["provenance"] == _producer_digest(record)]
            require(candidates, "positive native surplus has no matching accounting route provenance")
            require(len(candidates) == 1, "one native surplus has multiple accounting route claims")
            route_row = candidates[0]
            route = route_row["record"]
            matched_routes.add(route_row["event_id"])
            branch_id = route_row["branch_commit"]
            branch_kind, branch = payloads[branch_id]
            require(branch_kind == "application.accounting.commit", "route branch has no source archive commitment")
            require(branch["occurrence_semantics"] == payload["occurrence_semantics"] == "non-additive alternative views",
                    "native/accounting alternative semantics differ")
            additions = [pair for pair in branch["record"]["additions"] if pair[0]["identity"] == route["source_id"]]
            require(len(additions) == 1 and additions[0][1] == "routed", "route lacks its unique routed source content")
            source = additions[0][0]
            require(source["physical_id"] == "native-surplus:" + record.record_digest
                    and source["source_digest"] == _producer_digest(record), "accounting source is not this native shedding record")
            require(_accounting_number(source["value"]) == _accounting_number(route["surplus"]) == record.surplus
                    and source["unit"] == route["unit"] == "closure-burden", "native/accounting surplus or unit differs")
            require(source["committed"] == route["committed"] == record.committed,
                    "native and accounting source cuts differ")
            require(source["support_ids"] == ["surplus-occurrence:" + record.record_digest],
                    "accounting source occurrence support differs")
            fold = [row for row in relations["folds"] if row["outer_return_id"] == record.outer_return_id]
            require(len(fold) == 1 and record.source_identity in waves, "shedding source or outer closure unavailable")

            for leg in route["legs"]:
                require(len(result["paths"]) < max_paths, "sheddic path budget exhausted")
                path_id = native.digest((record.source_scope, record.record_digest, route["identity"], leg["identity"]))
                path = {"path_id": path_id, "status": "partial", "source_scope": record.source_scope,
                    "source_history": ancestry, "alternative": alternative,
                    "amount": str(_accounting_number(leg["amount"])), "unit": route["unit"],
                    "route_id": route["identity"], "leg_id": leg["identity"], "destination": leg["destination"],
                    "nodes": {"source_wave": {"identity": record.source_identity, "evidence": wave_events[record.source_identity]},
                        "coupling": {"event_id": coupling["event_id"], "evidence": coupling_evidence, "contact_ids": coupling["contacts"]},
                        "outer_closure": fold[0], "native_shedding": {"event_id": event_id, "record_digest": record.record_digest},
                        "archived_surplus": {"event_id": branch_id, "content_identity": source["identity"], "physical_id": source["physical_id"]},
                        "route_allocation": {"event_id": route_row["event_id"], "route_id": route["identity"], "leg_id": leg["identity"]},
                        "routed_support": {"status": "unavailable"}, "seating": {"status": "unavailable"},
                        "receipt": {"status": "pending"}},
                    "causal_cuts": {"shed": record.committed, "route": route["committed"]},
                    "seating_is_sensor_induced": False,
                    "signed_current": {"status": "unavailable", "reason": "closure burden has no bound signed-current projection"}}
                result["paths"].append(path)
                try:
                    require(leg["kind"] == record.route, "allocated route kind differs from native shedding route")
                    receipts = [row for row in receipt_rows if row["record"]["route_id"] == route["identity"]
                                and row["record"]["leg_id"] == leg["identity"] and row["branch_commit"] == branch_id]
                    require(len(receipts) <= 1, "route leg has multiple receipts")
                    if leg["kind"] == "open_loss":
                        require(not receipts and leg["destination"] is None, "open loss cannot have a retained receipt")
                        path.update(status="unobserved_open_loss")
                        path["nodes"]["receipt"] = {"status": "unobserved", "reason": "declared open-loss route"}
                        continue
                    destination = leg["destination"]
                    wave = waves.get(destination)
                    if wave is not None:
                        path["nodes"]["routed_support"] = {"status": "verified", "identity": destination,
                            "signature": wave.motif, "return_ids": [t.return_id for t in wave.returns],
                            "evidence": wave_events[destination]}
                        require(wave.returns[0].indices[0] > record.committed,
                                "routed support was constructed before source shedding")
                        path["causal_cuts"]["support_completed"] = wave.returns[-1].indices[-1]
                    seat = seat_rows.get(destination)
                    if seat is not None:
                        path["nodes"]["seating"] = {**seat, "sensor_induced": False}
                        path["causal_cuts"]["seated"] = seat["native_committed"]
                    if not receipts:
                        witnessed_pending = (route["identity"], leg["identity"]) in terminal_pending.get(record.record_digest, set())
                        path["status"] = "pending_receipt" if witnessed_pending else "pending_or_unobserved"
                        path["nodes"]["receipt"] = {"status": "pending" if witnessed_pending else "unobserved",
                            "reason": "terminal routing snapshot retains allocation" if witnessed_pending else
                                      "allocation is retained; absent receipt does not prove physical non-receipt"}
                        continue
                    require(wave is not None, "receipt destination lacks admitted routed formation witnesses")
                    require(seat is not None, "receipt provenance lacks its recorded seated state")
                    receipt_row = receipts[0]; receipt = receipt_row["record"]
                    require(receipt["destination"] == destination and receipt["cut"] > wave.returns[-1].indices[-1],
                            "receipt destination or causal completion cut differs")
                    seat_payload = payloads[seat["event_id"]][1]
                    before, seated = _state(seat_payload["before"]), _state(seat_payload["state"])
                    receipt_evidence = {"route_id": route["identity"], "leg_id": leg["identity"],
                        "destination": destination, "cut": receipt["cut"],
                        "admitted_route_wave_digest": _producer_digest(wave), "seated_state": seated,
                        "current_before_seating": before.current, "current_after_seating": seated.current}
                    require(receipt["provenance"] == _producer_digest(receipt_evidence),
                            "receipt provenance differs from actual route, wave and seated-state witnesses")
                    path["nodes"]["receipt"] = {"status": "verified", "event_id": receipt_row["event_id"],
                        "identity": receipt["identity"], "provenance": receipt["provenance"]}
                    path["causal_cuts"]["receipt"] = receipt["cut"]
                    path["status"] = "received"
                except (ValueError, TypeError, KeyError, AttributeError) as exc:
                    path["status"] = "partial"
                    path["reason"] = str(exc)
                    result["failures"].append({"stage": "route_path", "path_id": path_id, "reason": str(exc)})
        except (ValueError, TypeError, KeyError, AttributeError, StopIteration) as exc:
            result["failures"].append({"stage": "native_to_accounting", "event_id": event_id, "reason": str(exc)})

    if scope is None:
        for row in route_rows:
            if row["event_id"] not in matched_routes:
                result["failures"].append({"stage": "orphan_route", "event_id": row["event_id"],
                    "reason": "accounting route has no matching validated native shedding evidence"})
    available = bool(result["alternatives"] or (route_rows if scope is None else ()))
    result["coverage"] = ("partial" if result["failures"] or store.status()["coverage"] != "complete"
                          else "complete" if available else "unavailable")
    result["population_to_path_coverage"] = ("complete" if result["coverage"] == "complete" and world is not None
        and result["routing_plan"]["status"] == "verified"
        and result["terminal_routing"]["status"] == "verified"
        and all(row.get("source_history", {}).get("status") == "verified" for row in result["alternatives"])
        and all(path["source_history"]["status"] == "verified" and
                path["status"] in ("received", "pending_receipt", "unobserved_open_loss")
                for path in result["paths"])
        else "partial" if encounter_present else "unavailable")
    result["counts"] = {"paths": len(result["paths"]),
        **{state: sum(path["status"] == state for path in result["paths"])
           for state in ("received", "pending_receipt", "pending_or_unobserved", "unobserved_open_loss", "partial")}}
    return result


detect_sheddic_paths = trace_sheddic_paths
