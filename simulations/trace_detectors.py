"""Passive source-bound detectors over primitive committed D.E.C. events.

Core return candidates are reconstructed through runtime.dec_core's retained-start
H1, hinge, counterbranch and parent validators. A model's committed span is then
checked against those raw primitives; an earlier endpoint need not end that span.
The bounded detector stops with partial coverage instead of forgetting memory.
Manual shedding and directional recurrence use their separate canonical APIs.
"""
from dataclasses import asdict
from fractions import Fraction
import json

from runtime import dec_core as core
from .trace_views import Window, ViewError
from .vendor import aod_manual_native as native
from .trace_store import validate_event, TraceLimits, TraceIncomplete, TraceError


class DetectionError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise DetectionError(message)


def trace_from_record(record, depth=0):
    """Decode retained bytes, then perform canonical validation; no digest shortcut."""
    require(depth < 64 and type(record) is dict, "missing or excessively nested parent witness")
    require(set(record) == {"scope", "lineage", "start", "events", "closures", "parents", "proposal"},
            "incomplete retained trace fields")
    start = record["start"]
    require(type(start) is dict and set(start) == {"value"}, "typed retained q4 start required")
    require(type(record["events"]) in (list, tuple) and type(record["closures"]) in (list, tuple)
            and type(record["parents"]) in (list, tuple), "retained trace sequences required")
    events = tuple(core.Event(**event) for event in record["events"])
    closures = tuple(core.Closure(**certificate) for certificate in record["closures"])
    parents = tuple(trace_from_record(p, depth + 1) for p in record["parents"])
    trace = core.Trace(record["scope"], record["lineage"], core.Q4(start["value"]),
                       events, closures, parents, record["proposal"])
    core.validate_trace(trace)
    return trace


class CoreDetector:
    """Finite host detector with explicit retained-history caps.

    ``max_bytes`` caps canonical input bytes retained by this detector, not total
    Python allocator/RSS. Those are measured separately by the profile campaign.
    Full history remains in the external trace store when this detector stops.
    """
    def __init__(self, *, max_events=10000, max_bytes=8000000, max_lineages=128, max_parses=64):
        for value in (max_events, max_bytes, max_lineages, max_parses):
            require(type(value) is int and value > 0, "positive exact detector limits required")
        self.max_events, self.max_bytes, self.max_lineages = max_events, max_bytes, max_lineages
        self.max_parses = max_parses
        self._events, self._seen, self._states, self._results = [], {}, {}, []
        self._couplings, self._contact_results = {}, []
        self._bytes = 0
        self._last_ordinal = None
        self._run_id = None
        self._parses_high_water = 0
        self.incomplete_reason = None

    def _result(self, event, status, reason, **fields):
        row = {"namespace": "detector", "detector": "dec-core-return-v1", "status": status,
               "event_id": event["event_id"], "commit_ordinal": event["commit_ordinal"],
               "scope_id": event["scope_id"], "lineage_id": event["lineage_id"],
               "reason": reason, **fields}
        self._results.append(row)
        return row

    def consume(self, event):
        try:
            event = validate_event(event, TraceLimits(max_event_bytes=min(self.max_bytes, 262144)))
        except TraceIncomplete as exc:
            self.incomplete_reason = "detector event budget exhausted: " + str(exc)
            return ()
        except TraceError as exc:
            raise DetectionError(str(exc)) from exc
        require(type(event) is dict, "event envelope required")
        required = {"schema_version", "run_id", "event_id", "commit_ordinal", "causal_parents",
                    "scope_id", "lineage_id", "kind", "payload"}
        require(required <= set(event), "incomplete event envelope")
        require(type(event["schema_version"]) is int and event["schema_version"] == 1,
                "unsupported exact schema version")
        require(type(event["commit_ordinal"]) is int, "exact commit ordinal required")
        for key in ("run_id", "event_id", "scope_id", "lineage_id", "kind"):
            require(type(event[key]) is str and bool(event[key]), "nonempty " + key + " required")
        require(type(event["causal_parents"]) in (list, tuple) and
                all(type(v) is str and bool(v) for v in event["causal_parents"]), "invalid causal parent identities")
        require(len(set(event["causal_parents"])) == len(event["causal_parents"]), "duplicate causal parents")
        require(type(event["payload"]) is dict, "typed primitive payload required")
        encoded = json.dumps(event, sort_keys=True, separators=(",", ":"), allow_nan=False)
        previous = self._seen.get(event["event_id"])
        if previous is not None:
            require(previous == encoded, "conflicting duplicate event identity")
            return ()
        if self.incomplete_reason is not None:
            return ()
        if len(self._events) >= self.max_events or self._bytes + len(encoded.encode()) > self.max_bytes:
            self.incomplete_reason = "detector history budget exhausted; replay with sufficient state/budget"
            return ()
        require(self._run_id in (None, event["run_id"]), "detector cannot combine occurrence families")
        require(self._last_ordinal is None or event["commit_ordinal"] > self._last_ordinal,
                "new event serialization ordinals must strictly increase")
        self._run_id = event["run_id"]
        self._last_ordinal = event["commit_ordinal"]
        self._seen[event["event_id"]] = encoded
        self._bytes += len(encoded.encode())
        # Keep an independent immutable-by-serialization copy, not the caller's dict.
        event = json.loads(encoded)
        self._events.append(event)
        count = len(self._results)
        contact_count = len(self._contact_results)
        kind, payload = event["kind"], event["payload"]
        key = (event["scope_id"], event["lineage_id"])
        if kind == "core.start":
            if key in self._states:
                self._result(event, "rejected", "second retained start for one lineage")
                self._states[key]["blocked"] = "conflicting retained start"
            elif len(self._states) >= self.max_lineages:
                self.incomplete_reason = "detector lineage budget exhausted"
            else:
                try:
                    parents = tuple(trace_from_record(p) for p in payload.get("parents", ()))
                    t = core.Trace(key[0], key[1], core.Q4(payload["start"]),
                                   parents=parents, proposal=payload.get("proposal", ""))
                    core.validate_trace(t)
                    self._states[key] = {"trace": t, "parses": (t,), "raw_events": (), "raw_ids": (), "committed": (),
                                         "last_event": event, "last_id": event["event_id"],
                                         "start_id": event["event_id"], "blocked": None,
                                         "last_ordinal": event["commit_ordinal"], "last_kind": "start"}
                except (ValueError, TypeError, KeyError) as exc:
                    self._result(event, "unresolved", "retained start/parent witness unavailable or invalid: " + str(exc))
        elif kind == "core.transition":
            state = self._states.get(key)
            if state is None:
                self._result(event, "unresolved", "retained start is unavailable")
            elif state["blocked"]:
                self._result(event, "unresolved", "earlier primitive failed admission: " + state["blocked"])
            elif state["last_id"] not in event["causal_parents"]:
                state["blocked"] = "missing causal predecessor"
                self._result(event, "unresolved", state["blocked"])
            else:
                try:
                    t = state["trace"]
                    require(payload.get("start") == t.start.value and type(payload.get("start")) is int,
                            "primitive changed its retained start")
                    require(payload.get("proposal", "") == t.proposal, "primitive changed fusion ancestry")
                    primitive = core.Event(**{k: payload[k] for k in ("ordinal", "kind", "source", "target", "orientation")})
                    raw = state["raw_events"] + (primitive,)
                    alternatives, candidates, errors = {}, {}, []
                    for parse in state["parses"]:
                        out = core.Trace(t.scope, t.lineage, t.start, raw, parse.closures, t.parents, t.proposal)
                        try:
                            core.validate_trace(out)
                        except core.DecError as exc:
                            errors.append(str(exc)); continue
                        alternatives[out.closures] = out
                        if primitive.kind == "step" and primitive.target == t.start.value:
                            begin = parse.closures[-1].end if parse.closures else 0
                            try:
                                core._segment(out, begin, len(raw), True)
                            except core.DecError as exc:
                                errors.append(str(exc)); continue
                            certificate = core.Closure(begin, len(raw), core.digest(core.trace_prefix(out, len(raw))))
                            closed = core.Trace(t.scope, t.lineage, t.start, raw,
                                                out.closures + (certificate,), t.parents, t.proposal)
                            core.validate_trace(closed)
                            alternatives[closed.closures] = closed
                            candidates[certificate] = closed
                    require(bool(alternatives), errors[0] if errors else "no lawful retained primitive parse")
                    if len(alternatives) > self.max_parses:
                        self.incomplete_reason = "detector candidate-parse budget exhausted; no interpretation discarded as physical history"
                        return ()
                    state.update(trace=next(iter(alternatives.values())), parses=tuple(alternatives.values()),
                                 raw_events=raw, raw_ids=state["raw_ids"] + (event["event_id"],),
                                 last_event=event, last_id=event["event_id"],
                                 last_ordinal=event["commit_ordinal"], last_kind=primitive.kind)
                    for certificate, closed in candidates.items():
                        self._result(event, "candidate", "raw path satisfies canonical return predicate; committed span not yet selected",
                                     verification="verified", closure_digest=certificate.digest,
                                     start_event_id=state["start_id"], ordinal_begin=certificate.begin,
                                     ordinal_end=certificate.end, fusion=bool(closed.parents),
                                     parent_closure_digests=[p.closures[-1].digest for p in closed.parents])
                    if primitive.kind == "step" and primitive.target == t.start.value and not candidates:
                        self._result(event, "rejected", "return candidate failed: " + (errors[0] if errors else "no complete branch"), candidate=True)
                except (ValueError, TypeError, KeyError) as exc:
                    state["blocked"] = str(exc)
                    self._result(event, "rejected", "primitive return admission failed: " + str(exc), candidate=True)
        elif kind == "core.closure":
            state = self._states.get(key)
            if state is None or state["blocked"]:
                self._result(event, "unresolved", "certificate label has no detected raw return")
            else:
                try:
                    require(set(payload) == {"begin", "end", "digest"} and type(payload["begin"]) is int
                            and type(payload["end"]) is int and type(payload["digest"]) is str,
                            "exact committed closure fields required")
                    if any(payload == asdict(certificate) for certificate in state["committed"]):
                        return ()  # Replaying a certificate never creates a phase.
                    t = state["trace"]
                    raw = core.Trace(t.scope, t.lineage, t.start, state["raw_events"],
                                     state["committed"], t.parents, t.proposal)
                    core.validate_trace(raw)
                    begin = state["committed"][-1].end if state["committed"] else 0
                    core._segment(raw, begin, len(raw.events), True)
                    certificate = core.Closure(begin, len(raw.events), core.digest(core.trace_prefix(raw, len(raw.events))))
                    require(payload == asdict(certificate), "certificate span/digest differs from retained raw history")
                    require(state["last_id"] in event["causal_parents"], "closure lacks its retained terminal event")
                    closed = core.Trace(t.scope, t.lineage, t.start, raw.events,
                                        raw.closures + (certificate,), t.parents, t.proposal)
                    core.validate_trace(closed)
                    state.update(trace=closed, parses=(closed,), committed=closed.closures)
                    self._result(event, "verified", "committed span independently verified against complete raw primitives",
                                 event_id=state["last_id"], certificate_event_id=event["event_id"],
                                 return_commit_ordinal=state["last_ordinal"], closure_digest=certificate.digest,
                                 start_event_id=state["start_id"], phase=len(closed.closures),
                                 reclosure_beat=max(0, len(closed.closures)-1),
                                 ordinal_begin=certificate.begin, ordinal_end=certificate.end,
                                 fusion=bool(closed.parents),
                                 parent_closure_digests=[p.closures[-1].digest for p in closed.parents])
                except (ValueError, TypeError, KeyError) as exc:
                    self._result(event, "rejected", "committed closure admission failed: " + str(exc))
        elif kind == "core.fusion":
            state = self._states.get(key)
            if state is None or not state["trace"].parents or not state["committed"]:
                self._result(event, "unresolved", "fusion label has no independent outer return")
        elif kind == "core.contact":
            self._contact(event)
        self._parses_high_water = max(self._parses_high_water,
            sum(len(state["parses"]) for state in self._states.values()))
        return tuple(self._results[count:]) + tuple(self._contact_results[contact_count:])

    def _contact(self, event):
        payload = event["payload"]
        result = {"detector": "dec-core-contact-v1", "event_id": event["event_id"],
                  "commit_ordinal": event["commit_ordinal"], "scope_id": event["scope_id"],
                  "left_lineage": payload.get("left_lineage"), "right_lineage": payload.get("right_lineage"),
                  "window": payload.get("window"), "cadence": None, "lag": None, "drift": None,
                  "directional_rcd": {"status": "unavailable", "reason": "core contact law is not the Manual RCD constructor"}}
        try:
            required = {"left_lineage", "right_lineage", "window", "left_return", "right_return",
                        "left_closure_digest", "right_closure_digest", "event_span"}
            require(set(payload) == required, "contact payload fields differ")
            for field in ("left_lineage", "right_lineage", "window", "left_closure_digest", "right_closure_digest"):
                require(type(payload[field]) is str and bool(payload[field]), "contact string fields required")
            for field in ("left_return", "right_return", "event_span"):
                require(type(payload[field]) is int and payload[field] > 0, "contact exact positive index required")
            left_state = self._states.get((event["scope_id"], payload["left_lineage"]))
            right_state = self._states.get((event["scope_id"], payload["right_lineage"]))
            if left_state is None or right_state is None or left_state["blocked"] or right_state["blocked"]:
                result.update(status="unresolved", reason="contact lacks retained admitted lineage histories")
                self._contact_results.append(result); return
            traces, parent_ids = [], []
            for state, side in ((left_state, "left"), (right_state, "right")):
                span, returned = payload["event_span"], payload[side + "_return"]
                require(span <= len(state["raw_events"]) and returned <= span, "contact reads unavailable future primitives")
                t = state["trace"]
                retained = core.Trace(t.scope, t.lineage, t.start, state["raw_events"][:span],
                    tuple(c for c in state["committed"] if c.end <= span), t.parents, t.proposal)
                core.validate_trace(retained)
                matches = [c for c in retained.closures if c.end == returned]
                require(len(matches) == 1 and matches[0].digest == payload[side + "_closure_digest"],
                        "contact lacks its matching committed return certificate")
                parent_ids.append(state["raw_ids"][returned - 1])
                traces.append(retained)
            require(tuple(event["causal_parents"]) == tuple(parent_ids), "contact causal return parents differ or are missing")
            key = (event["scope_id"], payload["left_lineage"], payload["right_lineage"], payload["window"])
            prior = self._couplings.get(key)
            if prior is None:
                if len(self._couplings) >= self.max_lineages:
                    self.incomplete_reason = "detector coupling budget exhausted"
                    result.update(status="unresolved", reason=self.incomplete_reason)
                    self._contact_results.append(result); return
                contacts = ()
            else:
                require((prior.left, prior.right) == tuple(traces), "coupling preparation changed under one retained window")
                contacts = prior.contacts
            coupling = core.Coupling(traces[0], traces[1], payload["window"],
                                     contacts + ((payload["left_return"], payload["right_return"]),))
            core.validate_coupling(coupling)
            self._couplings[key] = coupling
            result["contact_count"] = len(coupling.contacts)
            result["contacts"] = [list(pair) for pair in coupling.contacts]
            result["event_span"] = payload["event_span"]
            try:
                result.update(status="verified", cadence=str(core.cadence(coupling)),
                              lag=core.lag(coupling), drift=core.drift(coupling),
                              reason="canonical repeated-contact comparison of retained raw/closed traces")
            except core.DecError as exc:
                result.update(status="unresolved", reason=str(exc))
        except (ValueError, TypeError, KeyError, IndexError) as exc:
            result.update(status="rejected", reason=str(exc))
        self._contact_results.append(result)

    def _report(self, window, scope=None):
        selected = [r for r in self._results if (window is None or window.contains(r["commit_ordinal"]))
                    and (scope is None or r["scope_id"] == scope)]
        comparisons = {}
        for row in self._contact_results:
            if (window is None or window.contains(row["commit_ordinal"])) and (scope is None or row["scope_id"] == scope):
                key = json.dumps((row["scope_id"], row["left_lineage"], row["right_lineage"], row["window"]), sort_keys=True)
                comparisons[key] = row
        all_verified = [r for r in self._results if r["status"] == "verified"]
        verified = [r for r in selected if r["status"] == "verified"]
        candidates = []
        for row in selected:
            if row["status"] != "candidate":
                continue
            commitment = "unconfirmed"
            for closed in all_verified:
                if (closed["scope_id"], closed["lineage_id"]) != (row["scope_id"], row["lineage_id"]):
                    continue
                if (closed["ordinal_begin"], closed["ordinal_end"], closed["closure_digest"]) == (
                        row["ordinal_begin"], row["ordinal_end"], row["closure_digest"]):
                    commitment = "selected"; break
                if closed["ordinal_begin"] < row["ordinal_end"] <= closed["ordinal_end"]:
                    commitment = "not_selected"
            candidates.append(dict(row, commitment=commitment))
        selected = [r for r in selected if r["status"] != "candidate"]
        censored = []
        for (state_scope, lineage), state in self._states.items():
            if scope is not None and state_scope != scope:
                continue
            t = state["trace"]
            begin = state["committed"][-1].end if state["committed"] else 0
            raw = state["raw_events"]
            if raw and raw[-1].target != t.start.value and len(raw) > begin and not state["blocked"]:
                censored.append({"status": "candidate", "coverage": "censored", "scope_id": state_scope,
                                 "lineage_id": lineage, "start_event_id": state["start_id"],
                                 "last_event_id": state["last_id"], "reason": "right-edge return remains open"})
        closure_unresolved = any(r["status"] == "unresolved" for r in selected)
        unresolved = closure_unresolved or any(r["status"] == "unresolved" for r in comparisons.values())
        unconfirmed = any(r["commitment"] == "unconfirmed" for r in candidates)
        coverage = "partial" if self.incomplete_reason or unresolved or unconfirmed else ("censored" if censored else "complete")
        relevant = any(event["kind"] in ("core.start", "core.transition", "core.closure", "core.fusion")
                       and (scope is None or event["scope_id"] == scope)
                       for event in self._events)
        phases = len(verified) if relevant and not closure_unresolved and not unconfirmed and not self.incomplete_reason else None
        reclosures = sum(r["phase"] > 1 for r in verified) if phases is not None else None
        if not relevant and not self.incomplete_reason:
            coverage = "unavailable"
        if window is not None and window.start == window.stop:
            selected, verified, censored, candidates, coverage = [], [], [], [], "complete"
            comparisons = {}
            phases, reclosures = 0, 0
        return {"detector": "dec-core-return-v1", "coverage": coverage,
                "incomplete_reason": self.incomplete_reason,
                "window": None if window is None else asdict(window),
                "requested_scope": scope,
                "returns": selected, "open_candidates": censored,
                "return_candidates": candidates,
                "couplings": list(comparisons.values()),
                "candidate_returns": len({r["event_id"] for r in candidates}),
                "observed_committed_phases": len(verified),
                "phases": phases, "reclosure_beats": reclosures,
                "count_scope": "Model-committed spans validated from raw history; unconfirmed candidates do not create phases or beats",
                "event_ordinal_rate": None if not self._events or phases is None else str(Fraction(phases, len(self._events))),
                "rate_unit": "detected-return/serialized-event (not physical bip rate)",
                "directional_rcd": {"status": "unavailable", "reason": "core stream alone has no Manual repeated-contact RCD"},
                "resources": {"retained_events": len(self._events), "retained_input_bytes": self._bytes,
                              "retained_lineages": len(self._states), "max_events": self.max_events,
                              "max_bytes": self.max_bytes, "max_lineages": self.max_lineages,
                              "max_parses_per_lineage": self.max_parses,
                              "candidate_parses": sum(len(s["parses"]) for s in self._states.values()),
                              "retained_couplings": len(self._couplings),
                              "candidate_parses_high_water": self._parses_high_water}}

    def inspect(self, window=None, *, scope=None):
        require(scope is None or (type(scope) is str and bool(scope)), "scope selection must be nonempty")
        if window is None:
            return self._report(None, scope)
        require(type(window) is Window and window.role == "observation" and window.domain == "commit-ordinal",
                "typed observation window in commit-ordinal domain required")
        prefix = CoreDetector(max_events=self.max_events, max_bytes=self.max_bytes,
                              max_lineages=self.max_lineages, max_parses=self.max_parses)
        for event in self._events:
            if event["commit_ordinal"] >= window.stop:
                break
            prefix.consume(event)
        if self.incomplete_reason:
            prefix.incomplete_reason = self.incomplete_reason
        return prefix._report(window, scope)


def detect_core(events, *, window=None, scope=None, **budgets):
    detector = CoreDetector(**budgets)
    for event in events:
        detector.consume(event)
    return detector.inspect(window, scope=scope)


def _decode_ratios(value):
    if type(value) is dict:
        if set(value) == {"$ratio"}:
            return Fraction(*value["$ratio"])
        return {k: _decode_ratios(v) for k, v in value.items()}
    if type(value) in (tuple, list):
        return tuple(_decode_ratios(v) for v in value)
    return value


def replay_boundaries(events, *, name, cells, frames=None, scope=None, max_events=10000, max_bytes=8000000,
                      auditor=None, coverage_scope="full"):
    """Reconstruct region commits and derive crossings from endpoint membership.

    Motion frames are independent query operands; their sweep must match the
    committed field. Unprojected burden prevents a complete signed-current result.
    """
    from . import region_engine as region
    require(type(max_events) is int and max_events > 0 and type(max_bytes) is int and max_bytes > 0,
            "positive replay budgets required")
    require(coverage_scope in ("full", "declared-current"), "explicit current coverage scope required")
    commits, locators, retained_bytes, context = [], [], 0, None
    seen = {}
    try:
        for raw in events:
            event = validate_event(raw, TraceLimits(max_event_bytes=min(max_bytes, 262144)))
            if event["kind"] != "region.commit" or (scope is not None and event["scope_id"] != scope):
                continue
            encoded = json.dumps(event, sort_keys=True, separators=(",", ":"))
            if event["event_id"] in seen:
                require(seen[event["event_id"]] == encoded, "conflicting region event identity")
                continue
            if len(commits) >= max_events or retained_bytes + len(encoded.encode()) > max_bytes:
                return {"status": "unresolved", "coverage": "partial", "value": None,
                        "reason": "boundary replay budget exhausted", "evidence": locators}
            retained_bytes += len(encoded.encode())
            seen[event["event_id"]] = encoded
            payload = _decode_ratios(event["payload"])
            current = (tuple(payload["cells"]), payload["unit"], payload["scale"], event["scope_id"], payload["domain"])
            require(context in (None, current), "region stream changed its bound context")
            context = current
            row = payload["row"]
            channels = tuple(region.Channel(**c) for c in row["channels"])
            obligations = tuple(region.PendingProjection(**c) for c in row["obligations"])
            sweep = tuple(region.SweepSample(**c) for c in row["sweep_field"])
            commits.append(region.CutRecord(row["cut"], row["window"], channels, obligations, sweep, row["snapshot"]))
            locators.append(event["event_id"])
        if context is None:
            return {"status": "unresolved", "coverage": "unavailable", "value": None,
                    "reason": "no committed region context", "evidence": []}
        ledger = region.Ledger(*context, tuple(commits))
        if frames is None:
            selected = region.Region(name, tuple(cells))
            frames = tuple(region.Frame(selected, selected, "stationary") for _ in commits)
        requested = region.fingerprint((ledger, name, tuple(frames), coverage_scope, None))
        audit = (region.audit_boundary if auditor is None else auditor)(ledger, name, tuple(frames), scope=coverage_scope)
        require(type(audit) is region.BoundaryAudit and
                (audit.ledger, audit.name, audit.frames, audit.scope, audit.representation) ==
                (ledger, name, tuple(frames), coverage_scope, None) and
                region.fingerprint((audit.ledger, audit.name, audit.frames, audit.scope, audit.representation)) == requested,
                "boundary auditor returned another requested source/context")
        region.validate_audit(audit)
        return {"status": "verified", "coverage": "complete", "value": str(audit.total["current"]),
                "unit": ledger.unit, "rows": region.serial(audit.rows), "total": region.serial(audit.total),
                "evidence": locators, "retained_input_bytes": retained_bytes,
                "scope": audit.coverage_claim}
    except (ValueError, TypeError, KeyError) as exc:
        return {"status": "unresolved", "coverage": "partial", "value": None,
                "reason": str(exc), "evidence": locators}


def inspect_fusion(left, right, outer=None):
    """A scale number or proposal alone cannot satisfy canonical outer closure."""
    if outer is None:
        return {"status": "unresolved", "reason": "independent outer closure unavailable"}
    try:
        for t in (left, right, outer):
            core.validate_trace(t)
        require(left.closures and right.closures, "inner closure witnesses required")
        require((left.scope, left.lineage) != (right.scope, right.lineage), "distinct inner lineages required")
        require(outer.parents == (left, right), "outer trace lost retained inner histories")
        require(outer.proposal == core.fusion_digest(left, right, outer.scope, outer.lineage), "wrong outer proposal")
        require(outer.closures and outer.closures[-1].end == len(outer.events), "outer return is still open")
        return {"status": "verified", "outer_digest": outer.closures[-1].digest,
                "inner_digests": [left.closures[-1].digest, right.closures[-1].digest],
                "evidence": asdict(outer)}
    except (ValueError, TypeError, AttributeError) as exc:
        return {"status": "rejected", "reason": str(exc)}


def inspect_shedding(folded, record, *, route_ledger=None, signed_projection=None):
    """Use the existing burden/capacity/route predicate, preserving missing maps."""
    try:
        native.validate_shedding(folded, record)
    except (ValueError, TypeError, KeyError, AttributeError) as exc:
        return {"status": "rejected", "reason": str(exc)}
    result = {"status": "verified", "physical_kind": record.status,
              "source_identity": record.source_identity, "event_digest": record.source_event_digest,
              "record_digest": record.record_digest, "closure_load": str(record.closure_load),
              "closure_cost": str(record.closure_cost), "surplus": str(record.surplus),
              "unit": "declared-closure-burden", "route": record.route,
              "target_scope": record.target_scope, "committed": record.committed,
              "signed_current": {"status": "unresolved", "value": None,
                                 "reason": "burden has no supplied signed-current projection"},
              "receipts": {"status": "unavailable", "pending": None}}
    if signed_projection is not None:
        # A supplied scalar is insufficient. Revalidate the complete boundary audit.
        from . import region_engine
        region_engine.validate_audit(signed_projection)
        require(signed_projection.scope == "full", "complete signed-current audit required")
        require(any(channel.kind == "shedding" and channel.provenance == record.record_digest
                    for cut in signed_projection.ledger.commits for channel in cut.channels),
                "signed current has no declared mapping from this shedding record")
        result["signed_current"] = {"status": "verified", "value": str(signed_projection.total["current"]),
                                    "unit": signed_projection.ledger.unit,
                                    "boundary_digest": region_engine.fingerprint(signed_projection)}
    if route_ledger is not None:
        from . import region_engine
        accounting = region_engine.accounting
        accounting._route_view(route_ledger)
        require(any(r.provenance == record.record_digest and r.committed >= record.committed and
                    r.surplus == record.surplus for r in route_ledger.routes),
                "route ledger does not bind this shedding record")
        result["receipts"] = {"status": "verified", "pending": accounting.pending_routes(route_ledger),
                              "retained_receipts": tuple(asdict(r) for r in route_ledger.receipts)}
    return result


def inspect_recurrence(left, right, contacts, *, producer_window, reverse=False, bias=(Fraction(0), Fraction(0)), prior_lag=Fraction(0)):
    """Manual comparison preserves directional RCD, signed bias and unwrapped lag."""
    try:
        result = native.compare(left, right, contacts, window=producer_window,
                                reverse=reverse, bias=bias, prior_lag=prior_lag)
        return {"status": "verified", "producer_window": list(producer_window),
                "return_count_interval": "(start, stop]", "contact_admission_interval": "[start, stop]",
                "ratio": str(result["ratio"]), "bias": str(result["bias"]),
                "lag": str(result["lag"]), "drift": str(result["drift"]),
                "RCD": asdict(result["rcd"]), "duration": result["duration"]}
    except (ValueError, TypeError, KeyError, AttributeError) as exc:
        return {"status": "unresolved", "reason": str(exc)}
