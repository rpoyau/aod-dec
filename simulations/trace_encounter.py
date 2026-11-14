"""Replay a captured source encounter, including pending and retained updates.

This verifies the declared finite continuation model. The source encounter is
executed before the current producer materializes its proton carriers; replay
does not relabel that model as populated proton/cavity dynamics.
"""
from pathlib import Path
from hashlib import sha256

from . import retained_continuation as model
from . import proton_accounting as producer
from .trace_store import TraceError, canonical_bytes, validate_event


class EncounterTraceError(ValueError):
    pass


def require(value, message):
    if not value:
        raise EncounterTraceError(message)


def equal(actual, expected, message):
    # JSON preserves the distinction between exact integers and Boolean aliases.
    require(producer.packed(actual) == producer.packed(expected), message)


def source_bindings():
    return {name: sha256(Path(module.__file__).read_bytes()).hexdigest()
            for name, module in (("simulations/retained_continuation.py", model),
                                 ("simulations/vendor/aod_manual_native.py", model.native))}


def retained_history(world):
    """Independently enumerate retained identities from the reconstructed world."""
    return {
        "population": [{"identity": w.identity, "scope": w.scope, "lineage": w.lineage,
            "scale": w.scale, "declared_motif": w.motif,
            "return_ids": [t.return_id for t in w.returns],
            "trace_digests": [t.trace_digest for t in w.returns],
            "certificate_digests": [c.certificate_digest for c in w.certificates]}
            for w in world.waves],
        "component_return_ids": [[t.return_id for t, _ in records] for records in world.components],
        "contacts": [{"identity": p.identity, "tick": p.tick, "committed": p.committed,
            "contact_ids": [c.identity for c in p.contacts],
            "reflection_occurrences": p.reflection_occurrences, "prefix_counts": p.prefix_counts}
            for p in world.contacts],
        "relations": [{"identity": r.identity, "target": r.target, "created_tick": r.created_tick,
            "input_cut": r.input_cut, "retained_at": r.retained_at, "contact_ids": r.contact_ids,
            "value": r.value, "RCD": r.rcd, "support_units": r.support_units,
            "unmatched_returns": r.unmatched_returns, "token_count": len(r.tokens),
            "tokens_digest": producer.digest(r.tokens),
            "active": r.created_tick in (world.state[0] - 1, world.state[0] - 2)
                      and r.identity not in world.disabled_relations}
            for r in world.relations],
        "disabled_relations": world.disabled_relations, "interventions": world.interventions}


COMMON = {"schema", "capture", "scope", "lineage", "scale", "source_core",
          "population_role", "source_bindings"}
FIELDS = {
    "init": {"requested_steps", "state_fields", "ticket_denominator", "state", "world_digest", "history"},
    "update": {"index", "masks", "tickets", "before_state", "after_state", "before_world_digest",
               "after_world_digest", "attempt", "pending_carriers", "before_history", "after_history"},
    "finish": {"requested_steps", "completed_attempts", "final_state", "final_world_digest", "history"}}


def reconstruct_encounter(store, *, max_events=10000, max_bytes=16 * 1024 * 1024):
    """Return a scoped report and the verified final world, or ``None``.

    Required update count comes from the recorded preparation, independently
    checked against terminal execution evidence. A partial archive never returns
    a final world for downstream path claims. Exact duplicate transport is safe.
    """
    require(all(type(v) is int and v > 0 for v in (max_events, max_bytes)), "positive exact replay budgets required")
    result = {"format": "retained-encounter-replay-v1", "coverage": "unavailable", "steps": [],
        "errors": [], "evidence": [], "requested_steps": None, "verified_steps": 0,
        "final_world_digest": None, "final_return_counts": None,
        "formation_return_ids": [], "resources": {"events": 0, "bytes": 0},
        "population_role": "source encounter before proton materialization",
        "coverage_claim": "recorded initialization, every declared finite update, and terminal state",
        "populated_proton_cavity_evolution": "unimplemented"}
    events, seen, world = [], {}, None
    bindings = source_bindings()
    try:
        for raw in store.iter_events():
            encoded = canonical_bytes(raw)
            result["resources"]["events"] += 1
            result["resources"]["bytes"] += len(encoded)
            require(result["resources"]["events"] <= max_events and result["resources"]["bytes"] <= max_bytes,
                    "encounter replay input budget exhausted")
            if not raw["kind"].startswith("particle.encounter_"):
                continue
            event = validate_event(raw)
            previous = seen.get(event["event_id"])
            if previous is not None:
                require(previous == encoded, "conflicting duplicate encounter event")
                continue
            seen[event["event_id"]] = encoded
            part = event["kind"].removeprefix("particle.encounter_")
            require(part in FIELDS, "unknown encounter event kind")
            payload = event["payload"]
            require(set(payload) == COMMON | FIELDS[part], "encounter event fields differ")
            expected = {"schema": "retained-encounter-" + part + "-v1",
                "capture": "validated-source-execution", "scope": model.SCOPE,
                "lineage": "retained-continuation-world", "scale": 0, "source_core": "1:2",
                "population_role": result["population_role"], "source_bindings": bindings}
            equal({key: payload[key] for key in COMMON}, expected, "encounter model/context binding differs")
            equal((event["scope_id"], event["lineage_id"], event["scale_index"]),
                  (model.SCOPE, expected["lineage"], 0), "encounter envelope context differs")
            events.append(event)
        if not events:
            result["errors"].append("no raw encounter update stream; formation snapshots alone do not supply it")
            return result, None
        require(len({e["run_id"] for e in events}) == 1, "mixed encounter occurrence families")
        inits = [e for e in events if e["kind"] == "particle.encounter_init"]
        finishes = [e for e in events if e["kind"] == "particle.encounter_finish"]
        require(len(inits) == 1, "exactly one encounter initialization required")
        require(len(finishes) == 1, "exactly one encounter terminal record required")
        initial, finish = inits[0], finishes[0]
        data = initial["payload"]
        count = data["requested_steps"]
        require(type(count) is int and 0 <= count <= model.HORIZON, "requested update count outside model horizon")
        result["requested_steps"] = count
        updates = [e for e in events if e["kind"] == "particle.encounter_update"]
        require(all(type(e["payload"]["index"]) is int for e in updates), "exact update indices required")
        updates.sort(key=lambda e: e["payload"]["index"])
        equal([e["payload"]["index"] for e in updates], list(range(count)), "missing, repeated or excess encounter update")
        require(not initial["causal_parents"], "encounter initialization has unexpected causal parent")
        equal(data["state_fields"], model.STATE_FIELDS, "compact state coordinate declaration differs")
        equal(data["ticket_denominator"], model.TICKET_DENOMINATOR, "exact ticket grid differs")
        world = model.initialize_world()
        equal(data["state"], world.state, "recorded initial population state differs")
        equal(data["history"], retained_history(world), "recorded initial population histories differ")
        equal(data["world_digest"], producer.digest(world), "initial world witness digest differs")
        parent = initial["event_id"]
        result["evidence"].append(parent)
        for event in updates:
            row = event["payload"]
            equal(event["causal_parents"], [parent], "encounter update lost its actual predecessor")
            equal(row["before_state"], world.state, "encounter state continuity differs")
            equal(row["before_world_digest"], producer.digest(world), "encounter retained-history continuity differs")
            equal(row["before_history"], retained_history(world), "retained history before update differs")
            require(type(row["masks"]) is list and type(row["tickets"]) is list,
                    "exact recorded masks and tickets required")
            before = world
            world = model.advance_world(world, tuple(row["masks"]), tuple(row["tickets"]))
            actual = world.attempts[-1]
            equal(row["attempt"], actual, "recorded opportunity selection or update differs from execution")
            equal(row["after_state"], world.state, "recorded successor state differs from execution")
            equal(row["after_world_digest"], producer.digest(world), "successor retained-world digest differs")
            equal(row["after_history"], retained_history(world), "retained history after update differs")
            pending = [i for i, complete in enumerate(actual.row["completed"]) if not complete]
            equal(row["pending_carriers"], pending, "pending attempts were lost or counted as returns")
            added = [[t.return_id for t in new.returns[len(old.returns):]]
                     for old, new in zip(before.waves, world.waves)]
            history = retained_history(world)
            result["steps"].append({"index": row["index"], "event_id": event["event_id"],
                "before_world_digest": row["before_world_digest"], "after_world_digest": row["after_world_digest"],
                "pending_carriers": pending, "new_returns": added,
                "contact": actual.row["contact"],
                "relations": [{"identity": r["identity"], "active": r["active"]} for r in history["relations"]]})
            result["verified_steps"] += 1
            parent = event["event_id"]
            result["evidence"].append(parent)
        terminal = finish["payload"]
        equal(finish["causal_parents"], [parent], "terminal lost its actual final update")
        equal((terminal["requested_steps"], terminal["completed_attempts"]), (count, len(world.attempts)),
              "terminal update count differs from declared execution")
        equal(terminal["final_state"], world.state, "terminal state differs from replay")
        equal(terminal["final_world_digest"], producer.digest(world), "terminal world digest differs from replay")
        equal(terminal["history"], retained_history(world), "terminal retained population differs")
        result["evidence"].append(finish["event_id"])
        require(store.status()["coverage"] == "complete", "archive is incomplete")
        require(source_bindings() == bindings, "bound model sources changed during replay")
        result.update(coverage="complete", final_world_digest=producer.digest(world),
            final_return_counts=[len(w.returns) for w in world.waves],
            formation_return_ids=[t.return_id for w in world.waves for t in w.returns],
            final_state=list(world.state))
        return result, world
    except (TraceError, ValueError, TypeError, KeyError, AttributeError, OSError) as exc:
        result["coverage"] = "partial"
        result["errors"].append(str(exc))
        return result, None


def replay_encounter(store, **budgets):
    return reconstruct_encounter(store, **budgets)[0]
