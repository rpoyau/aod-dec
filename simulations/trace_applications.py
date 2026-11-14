"""Fresh host applications with passive commit capture and recoverable witnesses.

Region/current records are application commitments, not primitive closure events.
The native particle formation history remains available in the produced witness
archive. Capturing it does not rerun or newly certify a return in every view.
"""
from base64 import b64encode, b64decode
from contextlib import ExitStack, nullcontext
from dataclasses import asdict, fields, is_dataclass
from fractions import Fraction
from hashlib import sha256
import json
from pathlib import Path

from simulations import boundary_accounting as accounting
from simulations import electron_wire_accounting as electron
from simulations import proton_accounting as proton
from simulations import run_regions as regions
from simulations.trace_store import TraceIncomplete, TraceIntegrityError, canonical_bytes
from simulations.trace_observation import observable


def _source_ref(module, operation):
    path = Path(module.__file__).resolve()
    return str(path.relative_to(Path(__file__).resolve().parents[1])) + ":" + operation + "#sha256=" + sha256(path.read_bytes()).hexdigest()


def _observation(value, *, unit, preparation, kernel, boundary, window,
                 evidence, observation_map, participation_windows=(), unresolved=False):
    """Describe the exact finite output separately from its physical calibration.

    ``result:`` locators are JSON pointers in this returned report; ``artifact:``
    locators refer to unchanged files produced alongside a particle report.
    Bounds are conditional on those declared operands, never total physical error.
    """
    def error(status, reason):
        return {"status": status, "bound": Fraction(0) if status == "exact" else None,
                "unit": unit, "reason": reason}
    errors = {
        "input_measurement": error("not_applicable", "Declared exact fixture operands; no measured input distribution supplied."),
        "sampling": error("not_applicable", "This reports the retained realization, not an ensemble estimator."),
        "model_discrepancy": error("unknown", "No measured physical discrepancy bound is supplied for this declared preparation."),
        "truncation_aggregation": error("unknown" if unresolved else "exact",
            "The routed burden has no admitted signed-current projection." if unresolved else
            "All contributions of this explicitly declared finite observable are retained; no claim outside its boundary or window."),
        "numerical": error("not_applicable" if unresolved else "exact",
            "No numerical full-current value is defined." if unresolved else "Exact integer/Fraction arithmetic on the declared operands."),
        "detection_incompleteness": error("unknown" if unresolved else "exact",
            "Full signed shedding-current coverage is unresolved." if unresolved else
            "The supplied finite commitment/channel family is complete for this view; no completeness claim for external dynamics."),
        "anchor": error("not_applicable", "Native current/count coordinates only; no temporal or SI conversion was applied.")}
    return observable(value, unit=unit, preparation=preparation, kernel=kernel,
        boundary=boundary, window=window,
        scale={"family": "retained-current-representation", "index": 0,
               "window_domain": "native-committed-cut", "interval": "[start,stop)",
               "participation_windows": tuple(participation_windows),
               "participation_domain": "native-return-count"},
        evidence=evidence, observation_map=observation_map,
        anchor={"kind": "native", "unit": unit, "coordinate": "native-committed-cut",
                "si_conversion": None}, errors=errors)


def _region_observations(results):
    observations = {}
    for index, result in enumerate(results):
        ledger = result["ledger"]
        options = dict(unit=ledger.unit, preparation=_source_ref(regions, "run_" + result["example"].replace("-", "_")),
            kernel=_source_ref(regions.n, "participation+aod_flux") + ";" + _source_ref(regions.r, "audit_boundary"),
            window=(ledger.commits[0].cut, ledger.commits[-1].cut + 1),
            participation_windows=tuple(event.window for event in ledger.commits))
        for name, audit in result["audits"].items():
            observations[result["example"] + "/" + name + "/current"] = _observation(
                sum((row["current"] for row in audit.rows), Fraction(0)),
                boundary=ledger.scope + ":" + name,
                evidence=("result:/examples/" + str(index) + "/audits/" + name,
                          "result:/examples/" + str(index) + "/complete_preparation"),
                observation_map="sum of scoped cross - sweep + signed shedding over the declared committed cuts", **options)
        if result["example"] == "maser":
            observations["maser/detected-output/outward"] = _observation(
                result["summary"]["detected_output_window_total"], boundary=ledger.scope + ":output-to-exterior",
                evidence=("result:/examples/0/summary/detected_output_outward", "result:/examples/0/channel_operands"),
                observation_map="sum of the declared detected-output oriented channel; distinct from apparatus net current", **options)
    return observations


def plain(value):
    if is_dataclass(value):
        return plain(asdict(value))
    if isinstance(value, dict):
        return {str(key): plain(item) for key, item in value.items()}
    if type(value) in (tuple, list):
        return [plain(item) for item in value]
    if type(value) in (set, frozenset):
        return [plain(item) for item in sorted(value)]
    if type(value) not in (str, int, bool, type(None), Fraction):
        raise TypeError("unsupported exact application value: " + type(value).__name__)
    return value


class AccountingRecorder:
    """A context-bound stream of validated deltas; no history-sized ID cache."""
    def __init__(self, store, application, *, alternatives=False):
        self.store, self.application = store, application
        self.alternatives, self.sequence = alternatives, 0
        self.archive_parent, self.route_parent = None, None

    def __call__(self, kind, record):
        payload = {"record": plain(record), "application": self.application,
                   "stage": "validated accounting commitment",
                   "occurrence_semantics": "non-additive alternative views" if self.alternatives else "successive committed accounting deltas"}
        # Serialization order is already recorded by the archive chain. It
        # must not create causal dependencies between alternative windows.
        parent = (None if self.alternatives else self.archive_parent) if kind == "accounting.commit" else (
            self.archive_parent if kind == "accounting.route" else self.route_parent)
        event = self.store.append("application." + kind, payload,
            scope_id=self.application, lineage_id="accounting",
            source_ref="simulations/boundary_accounting.py:" + kind,
            scale_family="retained-current-representation", scale_index=0,
            causal_parents=() if parent is None else (parent,),
            source_key=self.application + ":accounting:" + str(self.sequence))
        if kind == "accounting.commit":
            self.archive_parent = event["event_id"]
        elif kind == "accounting.route":
            self.route_parent = event["event_id"]
        self.sequence += 1


class NativeParticleRecorder:
    """Persist actual admitted formation operands for independent classification.

    These snapshots retain the producer's native indices. Archive ordering is
    transport ordering, not an invented collision clock or causal parent chain.
    """
    def __init__(self, store):
        self.store, self.sequence = store, 0
        self.source = _source_ref(proton, "observe_commits")
        self.encounter_parent = None
        self.encounter_context = None
        self.encounter_finished = False

    def __call__(self, kind, payload):
        if kind == "particle.native_return":
            wave = payload["wave"]
            scope, lineage, scale = wave["scope"], wave["lineage"], wave["scale"]
            key = [scope, lineage, kind, payload["certificate"]["return_id"]]
        else:
            scope, lineage, scale = payload["scope"], payload["lineage"], payload["scale"]
            key = [scope, kind, self.sequence]
        # The outer fold retains all inner formation histories. Preserve its
        # bytes in bounded chunks instead of exceeding one event's limits.
        parents = ()
        encounter_kind = kind.startswith("particle.encounter_")
        if encounter_kind:
            context = (scope, lineage, scale)
            if kind == "particle.encounter_init":
                if self.encounter_context is not None:
                    raise TraceIntegrityError("second encounter initialization in one recording")
                self.encounter_context = context
            elif context != self.encounter_context or self.encounter_finished:
                raise TraceIntegrityError("encounter update lacks its active initialization")
            elif self.encounter_parent is None:
                raise TraceIntegrityError("encounter update lacks its prior committed update")
            parents = () if self.encounter_parent is None else (self.encounter_parent,)
        if kind == "particle.native_fold":
            locator = retain_blob(self.store, proton.packed(payload),
                name="native-fold-" + str(self.sequence) + ".json", scope=scope)
            payload = {"schema": "native-particle-evidence-v1", "kind": kind,
                       "locator": locator, "capture": "validated-retained-snapshot"}
            kind = "evidence.native_witness"
            parents = (locator["last_event"],)
        event = self.store.append(kind, payload, scope_id=scope, lineage_id=lineage,
            source_ref=self.source, scale_family="native-particle-formation",
            scale_index=scale, source_key=json.dumps(key), causal_parents=parents)
        if encounter_kind:
            self.encounter_parent = event["event_id"]
            self.encounter_finished = kind == "particle.encounter_finish"
        self.sequence += 1


def retain_blob(store, data, *, name, scope, chunk_bytes=16384):
    """Store actual bounded chunks, not only a witness hash."""
    if type(data) is not bytes or type(chunk_bytes) is not int or chunk_bytes <= 0:
        raise TypeError("bytes and positive exact chunk length required")
    digest = sha256(data).hexdigest()
    total = (len(data) + chunk_bytes - 1) // chunk_bytes
    parent = None
    for part in range(total):
        event = store.append("evidence.blob_chunk", {
            "name": name, "sha256": digest, "part": part, "parts": total,
            "encoding": "base64", "data": b64encode(data[part*chunk_bytes:(part+1)*chunk_bytes]).decode("ascii")},
            scope_id=scope, lineage_id="durable-witness", source_ref="simulations/trace_applications.py:retain_blob",
            scale_family="evidence-storage", causal_parents=() if parent is None else (parent,),
            source_key=json.dumps([scope, "blob", name, digest, part]))
        parent = event["event_id"]
    return {"name": name, "sha256": digest, "bytes": len(data), "parts": total,
            "scope": scope, "last_event": parent, "coverage": "recoverable"}


def retrieve_blob(store, locator, *, max_bytes=8 * 1024 * 1024):
    """Recover one witness with an explicit query allocation cap."""
    if type(max_bytes) is not int or max_bytes < 0:
        raise TypeError("retrieval byte cap must be an exact nonnegative integer")
    if type(locator.get("bytes")) is not int or locator["bytes"] < 0:
        raise TraceIntegrityError("invalid witness byte size")
    if (type(locator.get("parts")) is not int or locator["parts"] < 0 or
            locator["parts"] > locator["bytes"] or
            any(type(locator.get(k)) is not str or not locator[k] for k in ("scope", "name", "sha256"))):
        raise TraceIntegrityError("invalid witness locator")
    if locator["bytes"] > max_bytes:
        raise TraceIncomplete("witness exceeds requested retrieval byte budget")
    data = bytearray()
    for part in range(locator["parts"]):
        key = json.dumps([locator["scope"], "blob", locator["name"], locator["sha256"], part])
        event = store.get_by_key(key)
        if event is None:
            raise TraceIncomplete("witness chunk unavailable")
        payload = event["payload"]
        if (event["kind"] != "evidence.blob_chunk" or payload["part"] != part or
                payload["parts"] != locator["parts"] or payload["sha256"] != locator["sha256"] or
                payload["name"] != locator["name"] or payload["encoding"] != "base64"):
            raise TraceIntegrityError("wrong witness chunk")
        try:
            chunk = b64decode(payload["data"], validate=True)
        except ValueError as exc:
            raise TraceIntegrityError("invalid witness encoding") from exc
        if len(data) + len(chunk) > max_bytes or len(data) + len(chunk) > locator["bytes"]:
            raise TraceIncomplete("witness byte budget exceeded")
        data.extend(chunk)
    if len(data) != locator["bytes"] or sha256(data).hexdigest() != locator["sha256"]:
        raise TraceIntegrityError("witness content differs from locator")
    return bytes(data)


def _restore_record(cls, data, **conversions):
    if type(data) is not dict or set(data) != {field.name for field in fields(cls)}:
        raise TraceIntegrityError("accounting record fields differ: " + cls.__name__)
    return cls(**{key: conversions.get(key, lambda x: x)(value) for key, value in data.items()})


def _ratio(value):
    if type(value) is int:
        return Fraction(value)
    if (type(value) is dict and set(value) == {"$ratio"} and
            type(value["$ratio"]) is list and len(value["$ratio"]) == 2 and
            all(type(v) is int for v in value["$ratio"]) and value["$ratio"][1] > 0):
        result = Fraction(*value["$ratio"])
        if [result.numerator, result.denominator] == value["$ratio"]:
            return result
    raise TraceIntegrityError("canonical exact accounting number required")


def _sequence(value):
    if type(value) is not list:
        raise TraceIntegrityError("canonical accounting array required")
    return tuple(value)


def replay_accounting(store, *, scope=None, max_events=10000, max_bytes=8000000):
    """Revalidate actual application routes and receipts from durable commits.

    Retained-current accounting is its own event dialect. Alternative proton
    windows remain separate branches, never an additive sequence. The byte cap
    bounds retained serialized operands, not total Python allocator/RSS usage.
    """
    if any(type(value) is not int or value <= 0 for value in (max_events, max_bytes)):
        raise TraceIntegrityError("positive exact accounting replay budgets required")
    validator = sha256(Path(accounting.__file__).read_bytes()).hexdigest()
    if validator != proton.ACCOUNTING_SHA or validator != electron.ACCOUNTING_SHA:
        raise TraceIntegrityError("accounting validator requires explicit source rebind")
    states, transitions, used, count = {}, [], 0, 0
    alternatives = "non-additive alternative views"
    successive = "successive committed accounting deltas"
    try:
        # Replay validation must never emit new model commitments into an active
        # caller's observer context.
        with accounting.observe_commits(None):
            for event in store.iter_events():
                kind = event["kind"]
                if not kind.startswith("application.accounting.") or (scope is not None and event["scope_id"] != scope):
                    continue
                count += 1
                used += len(canonical_bytes(event))
                if count > max_events or used > max_bytes:
                    raise TraceIncomplete("accounting replay operand budget exceeded")
                payload = event["payload"]
                application = payload["application"]
                semantics = payload["occurrence_semantics"]
                accounting.require(application == event["scope_id"] and semantics in (alternatives, successive),
                                   "accounting scope or occurrence semantics differ")
                state = states.setdefault(application, {"archive": accounting.ArchiveState(),
                    "ledger": accounting.RouteLedger(), "archive_parent": None, "route_ids": {},
                    "semantics": semantics, "branches": [], "branch": None})
                accounting.require(state["semantics"] == semantics, "accounting branch semantics changed")
                record = payload["record"]
                parents = event["causal_parents"]
                if kind == "application.accounting.commit":
                    expected = [] if semantics == alternatives or state["archive_parent"] is None else [state["archive_parent"]]
                    accounting.require(parents == expected, "archive commitment causal parent differs")
                    if semantics == alternatives or state["branch"] is None:
                        if state["branch"] is not None:
                            state["branch"]["pending_routes"] = plain(accounting.pending_routes(state["ledger"]))
                        state["archive"] = accounting.ArchiveState()
                        state["ledger"] = accounting.RouteLedger()
                        state["route_ids"] = {}
                        state["branch"] = {"commit_event": event["event_id"], "pending_routes": []}
                        state["branches"].append(state["branch"])
                    def addition(pair):
                        accounting.require(type(pair) is list and len(pair) == 2, "typed addition pair required")
                        return (_restore_record(accounting.ContentRecord, pair[0], value=_ratio,
                            support_ids=_sequence, parent_ids=_sequence), pair[1])
                    commit = _restore_record(accounting.ArchiveCommit, record,
                        additions=lambda rows: tuple(addition(row) for row in _sequence(rows)),
                        changes=lambda rows: tuple(_restore_record(accounting.DispositionChange, row) for row in _sequence(rows)))
                    state["archive"] = accounting.commit_archive(state["archive"], commit.cut, commit.additions, commit.changes)
                    state["archive_parent"] = event["event_id"]
                elif kind == "application.accounting.route":
                    accounting.require(parents == [state["archive_parent"]], "route lacks its actual source archive")
                    route = _restore_record(accounting.RouteRecord, record, surplus=_ratio,
                        legs=lambda rows: tuple(_restore_record(accounting.RouteLeg, row, amount=_ratio) for row in _sequence(rows)))
                    state["ledger"] = accounting.register_route(state["ledger"], state["archive"], route)
                    state["route_ids"][route.identity] = event["event_id"]
                    transitions.append({"kind": "validated_route", "event_id": event["event_id"],
                        "application": application, "branch_commit": state["branch"]["commit_event"],
                        "record": plain(route), "pending_routes": plain(accounting.pending_routes(state["ledger"]))})
                elif kind == "application.accounting.receipt":
                    receipt = _restore_record(accounting.Receipt, record)
                    accounting.require(receipt.route_id in state["route_ids"] and
                        parents == [state["route_ids"][receipt.route_id]], "receipt lacks its actual route causal parent")
                    state["ledger"] = accounting.receive_route(state["ledger"], receipt.route_id, receipt.leg_id,
                        receipt.cut, receipt.destination, receipt.identity, receipt.provenance)
                    transitions.append({"kind": "validated_receipt", "event_id": event["event_id"],
                        "application": application, "branch_commit": state["branch"]["commit_event"],
                        "record": plain(receipt), "pending_routes": plain(accounting.pending_routes(state["ledger"]))})
                else:
                    raise TraceIntegrityError("unsupported application accounting event")
    except (KeyError, TypeError, ValueError) as exc:
        if isinstance(exc, (TraceIncomplete, TraceIntegrityError)):
            raise
        raise TraceIntegrityError("invalid accounting replay: " + str(exc)) from exc
    applications = {}
    for application, state in states.items():
        if state["branch"] is not None:
            state["branch"]["pending_routes"] = plain(accounting.pending_routes(state["ledger"]))
        applications[application] = {"occurrence_semantics": state["semantics"], "branches": state["branches"]}
    status = store.status()
    return {"format": "aod-application-accounting-replay-v1", "applications": applications,
        "transitions": transitions, "validated_accounting_events": count, "retained_query_bytes": used,
        "validator_sha256": validator, "archive_coverage": status["coverage"],
        "coverage": "unavailable" if not count else "complete" if status["coverage"] == "complete" else "partial",
        "scope": "source-validated retained accounting routes/receipts; no inferred burden-to-signed-current map"}


def run_regions(store=None):
    """Execute the original D0 applications, optionally observing real commits."""
    context = nullcontext() if store is None else regions.r.observe_commits(store.core_recorder)
    with context as observation:
        results = (regions.run_maser(), regions.run_earth_water_moon())
    if store is not None:
        if observation.failure_count:
            error = TraceIncomplete("region commit capture incomplete: " + str(observation.last_error))
            store._failure(error)
            raise error
        for result in results:
            scope = result["ledger"].scope
            for item in result["channel_operands"]:
                committed = store.get_by_key(json.dumps(["region", scope, item["cut"]]))
                if committed is None:
                    error = TraceIncomplete("region preparation lacks its committed trace evidence")
                    store._failure(error)
                    raise error
                store.append("evidence.channel_preparation", plain(item), scope_id=scope,
                    lineage_id="declared-channel-preparation",
                    source_ref="simulations/run_regions.py:computed_channel",
                    scale_family="declared-current",
                    causal_parents=(committed["event_id"],),
                    source_key=json.dumps([scope, "preparation", item["identity"], item["cut"]]))
            for name, audit in result["audits"].items():
                for row, frame in zip(audit.rows, audit.frames):
                    committed = store.get_by_key(json.dumps(["region", scope, row["cut"]]))
                    if committed is None:
                        error = TraceIncomplete("boundary view lacks its committed trace evidence")
                        store._failure(error)
                        raise error
                    # A derived view keeps its original occurrence identities;
                    # this record is not a new source crossing or closure.
                    store.append("detector.boundary_view", {
                        "boundary": name, "row": plain(row), "frame": plain(frame),
                        "unit": audit.ledger.unit, "coverage": audit.coverage_claim},
                        scope_id=scope, lineage_id="boundary-view",
                        source_ref="simulations/region_engine.py:audit_boundary",
                        scale_family="declared-current",
                        causal_parents=(committed["event_id"],),
                        source_key=json.dumps([scope, "boundary", name, row["cut"]]))
    return {"support_class": "D0 declared current preparation", "engine": "host-region-adapter",
            "examples": [regions.encode(result) for result in results],
            "observations": regions.encode(_region_observations(results)),
            "trace_scope": "validated region commits and derived current views; no new native transition law",
            "physical_percent_error": None}


def run_particles(output_dir, store=None):
    """Execute native preparations and preserve exact application outputs.

    Proton windows are alternative projections of one occurrence family. The
    electron baseline is recorded; its all-gate-off control is executed separately.
    No report-layer event is represented as a fresh primitive transition.
    """
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    proton_path = output_dir / "proton-accounting.json"
    capture = None
    with ExitStack() as contexts:
        if store is not None:
            contexts.enter_context(accounting.observe_commits(
                AccountingRecorder(store, "proton-window-alternatives", alternatives=True)))
            capture = contexts.enter_context(proton.observe_commits(NativeParticleRecorder(store)))
        proton_summary = proton.run(proton_path)
    if capture is not None and capture.failure_count:
        error = TraceIncomplete("native particle capture incomplete: " + str(capture.last_error))
        store._failure(error)
        raise error
    electron_wave, wire = electron.electron_wave(), electron.wire_wave()
    contacts, reflections = electron.counted_contacts(wire, electron_wave)
    context = nullcontext() if store is None else accounting.observe_commits(
        AccountingRecorder(store, "electron-wire-baseline"))
    with context:
        baseline = electron.execute(wire, electron_wave, contacts, electron.SCHEDULE)
    gate_off = electron.execute(wire, electron_wave, contacts, tuple((w, False) for w, _ in electron.SCHEDULE))
    electron_report = electron.encode({"baseline": baseline, "all_gate_off_control": gate_off,
        "reflections": reflections, "coverage": "native preparation and baseline accounting; no SI force prediction"})
    electron_path = output_dir / "electron-wire.json"
    electron_path.write_text(json.dumps(electron_report, sort_keys=True, indent=2) + "\n")
    witness_path = proton_path.with_name(proton_path.stem + "-witnesses.json.gz")
    locators = []
    if store is not None:
        for path in (proton_path, witness_path, electron_path):
            locators.append(retain_blob(store, path.read_bytes(), name=path.name, scope="particle-evidence"))
    summary = {key: value for key, value in proton_summary.items() if key != "output"}
    observations = {}
    kernel = _source_ref(proton.native, "compare+participation+aod_flux") + ";" + _source_ref(accounting, "account_slice")
    proton_report = json.loads(proton_path.read_text())
    for index, row in enumerate(proton_report["rows"]):
        options = dict(unit=proton.CURRENT_UNIT, preparation=_source_ref(proton, "run"), kernel=kernel,
            boundary=row["boundary_slice"]["boundary_id"], window=(row["cut"], row["cut"] + 1),
            participation_windows=(row["window"],),
            evidence=("artifact:proton-accounting.json#/rows/" + str(index),
                      "artifact:" + witness_path.name + "#sha256=" + sha256(witness_path.read_bytes()).hexdigest()))
        observations["proton/" + row["alternative"] + "/contact-current"] = _observation(
            Fraction(row["one_slice_current"]["current"]),
            observation_map="one contact-channel projection cross - sweep + signed shedding; alternatives are non-additive", **options)
        observations["proton/" + row["alternative"] + "/full-current"] = _observation(None,
            observation_map="full signed current is unresolved; no burden-route to signed-current map supplied",
            unresolved=True, **options)
    cuts = tuple(row["input_cut"] for row in baseline["rows"])
    for name, total in baseline["window_totals"].items():
        observations["electron-wire/" + name + "/current"] = _observation(total["current"], unit=total["unit"],
            preparation=_source_ref(electron, "execute"), kernel=kernel, boundary=total["boundary_id"],
            window=(cuts[0], cuts[-1] + 1), participation_windows=tuple(row["window"] for row in baseline["rows"]),
            evidence=("artifact:electron-wire.json#/baseline/window_totals/" + name,
                      "artifact:electron-wire.json#/baseline/rows"),
            observation_map="sum of the distinct committed scoped contact currents; union cancels oriented internal views")
    return {"support_class": "D0 exact native particle preparations", "engine": "host-native-producer-and-accounting",
        "proton": proton.encode(summary),
        "electron": {"wire_flux": [str(row["channel"]["wire_outward_flux"]) for row in baseline["rows"]],
                     "later_current": [str(row["after"]["electron"].current) for row in baseline["rows"]],
                     "union_current": str(baseline["window_totals"]["union"]["current"]),
                     "retained_history_sha256": baseline["history_sha256"]},
        "trace_scope": "source encounter initialization/updates/finish, native formation and relation witnesses, routing plan/terminal ledgers, and validated accounting commits/routes/receipts",
        "observations": proton.encode(observations),
        "witnesses": locators, "proton_alternatives_additive": False,
        "proton_full_signed_shedding_current": {"status": "unresolved", "value": None}}


def application_gaps():
    """Explicitly scoped source gaps; these do not block finite applications."""
    return {
        "tau": {"status": "implementation_incomplete", "live_run": False,
            "source": "manual/sections/00_dec_ledger.tex:manual:dec-ledger",
            "reason": "Collision, cavity and sensor setup are inputs; support names are detected from execution. The current particle generator prepares fixed cores and does not execute configurable cavity/sensor collision evolution.",
            "missing": ["configurable collision cavity and sensor execution",
                        "formation changes driven by the executed collision history",
                        "end-to-end emitted, pending and sensor-received detection coverage"],
            "detector": "retained native construction and path inference; no expected Tau label",
            "historical_score_provenance": "The original diagnostic calculation and compact/depth-indexed adapter are unavailable; this does not gate fresh collision execution.",
            "historical_missing_burden": {"numerator": 31, "denominator": 50, "role": "historical comparator, not generated output"}},
        "sun_mercury": {"status": "finite_scope_example_only", "physical_trajectory": False,
            "source": "manual/sections/05_solar_system_field_tests.tex:manual:solar:observable-export-bridge",
            "missing": ["executable solar and Mercury recurrence preparations", "initial state and coupling evolution", "boundary-current update and observation maps"]}}
