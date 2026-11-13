"""Source-bound exact Manual D.E.C. fixture, separate from reservoir transport.

The producer admits the actual return/wave/contact records. This application
declares finite support placement, channel attention and a later response. It
does not infer a physical maser, a volume measure, or a closed Markov reduction
from present population counts. Every reported native value is integer/rational.
"""
from __future__ import annotations

from dataclasses import asdict, is_dataclass, replace
from fractions import Fraction
import hashlib
from pathlib import Path

from .vendor import aod_manual_native as native


F = Fraction
SOURCE_COMMIT = "8c3149b9d3d924b80b9385b178da4ba2c39175e8"
SOURCE_TREE = "b2b55cbcf18354828f7b34418ab3fa26d747a927"
SOURCE_PATH = "manual/scripts/aod_native_closure_wave.py"
SOURCE_SHA256 = "3ba98c7097bc104aeb6218265b8531f94f2ca5a35239d0df097aba096675dfdf"
LICENSE_SHA256 = "bcaee0778a5a742463fae8d3bf66d1dfc37d45b2a92962ad415b730b310d5bf7"
SCOPE = "manual-dec:declared-field"


def exact_data(value):
    """Serialize exact native records without decimal conversion or lost fields."""
    if isinstance(value, Fraction):
        return str(value)
    if is_dataclass(value):
        return exact_data(asdict(value))
    if isinstance(value, dict):
        if any(not isinstance(key, (str, int)) for key in value):
            raise TypeError("structured keys require an explicit row representation")
        return {str(key): exact_data(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [exact_data(item) for item in value]
    if isinstance(value, float):
        raise TypeError("binary floats are not native fixture records")
    return value


def source_binding():
    path = Path(native.__file__)
    actual = hashlib.sha256(path.read_bytes()).hexdigest()
    license_digest = hashlib.sha256(path.with_name("LICENSE").read_bytes()).hexdigest()
    native.require(actual == SOURCE_SHA256, "vendored producer differs from bound source")
    native.require(license_digest == LICENSE_SHA256, "vendored license differs from bound source")
    return {"repository": "https://github.com/rpoyau/aod", "commit": SOURCE_COMMIT,
            "tree": SOURCE_TREE, "path": SOURCE_PATH, "sha256": actual,
            "license_sha256": license_digest,
            "role": "delivered local Manual I candidate; remote acceptance not inferred"}


def ledger_kernel(rows):
    """Normalize only the declared admissible H1 rows through the producer API.

    Each row is (slot, neighbor, ternary role, admissible, integer weight).
    The current role is an already-entered hinge. An admissible zero-weight
    row remains in the structural family; a blocked row is removed from it.
    """
    rows = tuple(rows)
    native.require(len({(r[0], r[2]) for r in rows}) == len(rows), "duplicate ledger row")
    native.require(len({(r[1], r[2]) for r in rows}) == len(rows),
                   "edge aliases do not create distinct kernel candidates")
    for slot, neighbor, role, admitted, weight in rows:
        native.require(isinstance(slot, str) and bool(slot), "unbound edge slot")
        native.require(type(neighbor) is int and native.hamming(0, neighbor) == 1,
                       "ledger neighbor must be Hamming-1")
        native.require(type(role) is int and role in (-1, 0, 1), "invalid ternary role")
        native.require(type(admitted) is int and admitted in (0, 1), "invalid admissibility")
        native.require(type(weight) is int and weight >= 0, "weight must be nonnegative integer")
    candidates = tuple((0, neighbor, 0, role) for _, neighbor, role, adm, _ in rows if adm)
    weights = {(0, neighbor, 0, role): weight for _, neighbor, role, adm, weight in rows if adm}
    raw = native.kernel(candidates, weights)
    z = sum(weight for _, _, _, adm, weight in rows if adm)
    output = []
    for slot, neighbor, role, admitted, weight in rows:
        key = (0, neighbor, 0, role)
        output.append({"scope": SCOPE, "v": 0, "edge": slot, "v_e": neighbor,
                       "sigma": role, "admissible": admitted, "weight": weight,
                       "probability": raw["probabilities"].get(key, F(0)) if admitted else F(0),
                       "route": "blocked" if not admitted else {-1: "returned", 0: "hinge", 1: "outgoing"}[role]})
    return {"status": raw["status"], "normalizer": z, "rows": output,
            "structural_rows": len(candidates),
            "positive_weight_rows": sum(weight > 0 for _, _, _, adm, weight in rows if adm)}


def make_population():
    """Four distinct recurring lineages with retained certified H1 returns."""
    return tuple(native.recurrent(name, n=count, spacing=spacing, scope=SCOPE)
                 for name, count, spacing in (("A", 3, 10), ("B", 4, 6),
                                              ("C", 3, 10), ("D", 4, 6)))


def bound_contacts(left, right, reflection=(1, 2)):
    return tuple(replace(event, identity=f"{left.lineage}|{right.lineage}:{event.identity}",
                         reflection=reflection) for event in native.contacts(left, right))


def channel(left, right, contacts, capacity, attention, *, reverse=False, window_count=6):
    """Construct directional RCD first, then project and clip its duration."""
    capacity, attention = native.Q(capacity), native.Q(attention)
    native.require(capacity >= 0, "negative capacity")
    native.require(type(window_count) is int and window_count >= 0,
                   "window participation requires a nonnegative integer count")
    comparison = native.compare(left, right, contacts, window=(0, 22), reverse=reverse)
    rcd = comparison["rcd"]
    participation = native.participation(rcd, window_count, "native-return-count")
    return {"id": f"{rcd.left}|{rcd.right}", "left": rcd.left, "right": rcd.right,
            "contacts": contacts, "comparison": comparison, "RD": rcd.support,
            "RCD": rcd, "duration": rcd.duration(), "window_count": window_count,
            "window_domain": "native-return-count", "participation": participation,
            "capacity": capacity, "pressure": capacity * participation,
            "attention": attention, "aod_flux": capacity * participation * attention}


def history_reduction_check(left, right, contacts):
    """A counterexample to discarding directional reflected-contact history.

    Alternative input histories share wave records and their first contact.
    The last contact is a separately identified counterfactual record; the
    original committed event is retained unchanged. A prescribed response reads
    it at cut 22 and updates only at cut 23.
    """
    changed = (*contacts[:-1], replace(contacts[-1],
               identity=contacts[-1].identity + ":reflection-alternative",
               reflection=(contacts[-1].reflection[0] + 2, contacts[-1].reflection[1])))
    before = native.initial_state(left, pressure=1)
    first = channel(left, right, contacts, 3, F(1, 3))
    second = channel(left, right, changed, 3, F(1, 3))
    outputs = tuple(native.update(before, item["aod_flux"], input_index=22)
                    for item in (first, second))
    native.require(outputs[0].current != outputs[1].current,
                   "declared history intervention did not change the later response")
    return {"status": "COUNTEREXAMPLE_TO_SCALAR_REDUCTION",
            "reduction": "present vertex, closure-cycle count, population and current; omit coupling history",
            "same_wave_prefixes": [left, right], "same_reduced_state": before,
            "common_contact_prefix": contacts[:-1],
            "alternatives": [{"channel": item, "response": output}
                             for item, output in zip((first, second), outputs)],
            "declared_response": "producer.update: next_current = current + channel_flux / state_pressure",
            "input_cut": 22, "response_cut": 23,
            "conclusion": "This reduced state is insufficient for this declared response. No universal non-Markov claim follows."}


def build_fixture():
    """Return a JSON-ready exact D0 calculation card and all retained witnesses."""
    binding = source_binding()
    initial = ledger_kernel((("e1", 1, 1, 1, 1), ("e2", 2, -1, 1, 1),
                             ("e3", 4, 0, 1, 4), ("e4", 8, 1, 1, 2)))
    slide = ledger_kernel((("e1", 1, -1, 1, 3), ("e2", 2, -1, 1, 3),
                           ("e3", 4, 1, 1, 1)))
    waves = make_population()
    a, b, c, d = waves
    field = native.field(SCOPE, "four-member-field", waves)
    alias = native.field(SCOPE, "alias-containing-field", (a, a))
    unique = native.resolve_fields((field, alias))
    native.require(len(unique) == 4, "aliases changed population")
    placement = {a.identity: "cell-left", c.identity: "cell-left",
                 b.identity: "cell-right", d.identity: "cell-right"}
    counts = {cell: sum(placement[identity] == cell for identity in unique)
              for cell in ("cell-left", "cell-right")}
    contacts_ab, contacts_cd = bound_contacts(a, b), bound_contacts(c, d, (2, 3))
    channels = (channel(a, b, contacts_ab, 3, F(1, 3)),
                channel(c, d, contacts_cd, 2, F(-1, 2)))
    reverse = (channel(a, b, contacts_ab, 3, F(-1, 3), reverse=True),
               channel(c, d, contacts_cd, 2, F(1, 2), reverse=True))
    pressure = sum((item["pressure"] for item in channels), F(0))
    signed = sum((item["aod_flux"] for item in channels), F(0))
    flux_rows = [(0, 1, item["pressure"], item["attention"], item["id"]) for item in channels]
    flux = native.aod_flux(flux_rows)
    boundary_audit = native.stokes(flux, {0})
    normalized_memory = (F(counts["cell-left"], 4), F(counts["cell-right"], 4))
    afc_matrix = ((F(3, 4), F(1, 4)), (F(1, 2), F(1, 2)))
    afc_flux = native.afc_flux(normalized_memory, afc_matrix)
    cancellation_inputs = [{"path_history_id": channels[0]["id"], "pressure": 15,
                            "attention": F(1, 3)},
                           {"path_history_id": channels[1]["id"], "pressure": 12,
                            "attention": F(-5, 12)}]
    cancelled = native.aod_flux([(0, 1, row["pressure"], row["attention"], row["path_history_id"])
                                for row in cancellation_inputs])
    cancellation = native.stokes(cancelled, {0})
    reduction = history_reduction_check(a, b, contacts_ab)
    report = {
        "format": "aod.dec.manual-fixture", "version": 1, "status": "PASS",
        "source": binding,
        "calculation_card": {
            "support_class": "D0 exact internal fixture",
            "question": "Do exact admitted kernels, histories, directional RCD and scoped boundary values retain their declared types?",
            "scope": SCOPE, "window": [0, 22],
            "operator_chain": ["declared admissibility", "exact kernel", "retained certified returns",
                               "recurring population", "bound contacts", "directional RCD",
                               "duration projection", "same-domain window clip", "path-history pressure",
                               "oriented AOD flux", "boundary audit"],
            "spatial_support": "two declared bookkeeping cells grouping admitted identities; no metric cell volume",
            "temporal_support": "native integer cuts; no seconds per cut",
            "SI_observation_map": "NOT_INSTANTIATED", "physical_target": "NOT_JOINED"},
        "dec_kernel": {"one_node": initial, "slide": slide,
                       "blocked_additional_dwell": {"edge": "e3", "sigma": 0,
                           "event_kind": "fibre-only additional dwell", "admissible": 0,
                           "weight": 4, "probability": F(0), "route": "blocked",
                           "retained_structural_hinge": native.fibre_step(1, 0, "enter_hinge")},
                       "slide_role_probabilities": {"returned": F(6, 7), "outgoing": F(1, 7)}},
        "population": {"waves": waves, "field": field, "alias_field": alias,
                       "unique_member_ids": unique, "population_count": len(unique),
                       "support_map": placement, "cell_population_counts": counts,
                       "normalized_occupancy": {cell: F(count, 4) for cell, count in counts.items()},
                       "population_density": {"status": "NOT_INSTANTIATED", "reason": "No positive cell measure is declared."},
                       "typing": "Counts refer to unique recurring members; neither transported mass nor RCD duration."},
        "path_histories": channels,
        "pressure_and_flux": {"path_pressure_sum": pressure, "sum_of_oriented_path_fluxes": signed,
                              "compression": {"rule": "A_eff = sum_h(p_h A_h) / sum_h p_h; positive denominator",
                                              "pressure": pressure, "effective_attention": signed / pressure,
                                              "reconstructed_flux": pressure * (signed / pressure)},
                              "oriented_rows": [{"left": i, "right": j, "lineage": lineage, "value": value}
                                                for (i, j, lineage), value in flux.items()],
                              "boundary_audit": boundary_audit,
                              "separately_constructed_reverse_channels": reverse,
                              "reverse_physical_flux_sum": sum((item["aod_flux"] for item in reverse), F(0)),
                              "bookkeeping_reverse_flux_sum": -signed},
        "afc": {"memory": normalized_memory, "kernel": afc_matrix, "flux": afc_flux,
                "typing": "Declared normalized finite memory/kernel; distinct from AOD pressure-bearing flux."},
        "cancellation": {"inputs": cancellation_inputs,
                         "control": "Declared attention-only projection control on the same retained path histories: +5 and -5.",
                         "audit": cancellation},
        "history_reduction": reduction,
        "integer_fixture_comparators": [
            {"name": "unique-population", "computed": len(unique), "expected": 4,
             "delta_Z": len(unique) - 4, "delta3": [0, 0]},
            {"name": "path-pressure-sum", "computed": int(pressure), "expected": 27,
             "delta_Z": int(pressure) - 27, "delta3": [0, 0]}],
        "scope_limits": ["Exact conditional fixture; no empirical uncertainty is set to zero.",
                         "Admitted population and coupling histories are preserved, not inferred from cell mass.",
                         "The counterexample rejects only the displayed scalar reduction for the declared response."]}
    native.require(pressure == 27 and signed == -1, "independent path-pressure/flux comparator failed")
    native.require(boundary_audit["boundary"] == -1, "independent boundary comparator failed")
    return exact_data(report)
