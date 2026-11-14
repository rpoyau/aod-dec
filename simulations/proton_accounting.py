#!/usr/bin/env python3
"""Proton-associated 3:3:6 accounting with actual retained encounter witnesses.

The source continuation law is unchanged. Its 1:2 histories supply explicitly
mapped preparation orientations for new admitted 3:3 C6 carriers. Main/Manual
species association, contact-channel accounting and mass metrology stay typed.
"""
from __future__ import annotations

import argparse
from dataclasses import asdict, is_dataclass, replace
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

ACCOUNTING_SHA = "7bdf7a33664bf275debecf51a53c8768d18915102739d9715a6e4e6db4def88f"
ENCOUNTER_SHA = "5f4ec3ea81a0fc0b7b5f36dcc27bb3faf60a5894ef12eb74cc0a929c75364472"
SCOPE = "manual-proton-pair:declared-preparation"
CURRENT_UNIT = "native-pressure-current"
BURDEN_UNIT = "closure-burden"
WINDOWS = (2, 4, 6, 12)
CAPACITY = Q(3)
CLOSURE_COST = Q(24)
C6 = (0, 1, 3, 7, 6, 4, 0)


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
    require(encounter.validate_world(world), "encounter witness replay failed")
    require(tuple(len(w.returns) for w in world.waves) == (7, 8), "fixed encounter schedule changed")
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
    first_cut = outer.indices[-1]+1
    source_state = replace(native.initial_state(carriers[0]),
                           orientation=carriers[0].returns[-1].roles[0])
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
            route_state = native.initial_state(route_wave)
            seated = native.seat_duon(route_wave, route_state, compatible=True)
            require(seated.current == route_state.current, "seating unexpectedly generated feedback")
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
