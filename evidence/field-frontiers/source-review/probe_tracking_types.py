"""Bounded source-mapping probes; no source edits or physical dynamics claim."""
from dataclasses import asdict
import json
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "aod-dec"))
from simulations.vendor import aod_manual_native as n
from simulations.trace_support import inspect_native_return
from simulations.trace_history_union import RetainedHistoryUnion


def must(value, message):
    if not value:
        raise RuntimeError(message)


def rejected(operation):
    try:
        operation()
    except (ValueError, TypeError) as exc:
        return str(exc)
    raise RuntimeError("operation unexpectedly accepted")


pivot = n.trace("type-probe", 0, start_index=0)
pc = n.certify(pivot)
primitive = inspect_native_return({"trace": asdict(pivot), "certificate": asdict(pc)})
must(primitive["closure"]["status"] == "verified", "primitive closure not verified")
must(primitive["status"] == "unresolved", "primitive detector coverage changed")
outer_path = n.trace("type-probe", 1, vertices=(0,1,3,2,0), turn=2, start_index=3)
oc = n.certify(outer_path)
registry = {pc.certificate_digest:pc, oc.certificate_digest:oc}
construction = (((pc.return_id,), (pc.return_id,), (pc.return_id,)), (pc.return_id,))
trio = n.motif_witness("3:1", outer_path, construction, ((pivot,pc),), registry)
detected = inspect_native_return({"trace": asdict(outer_path), "certificate": asdict(oc),
                                 "motif_witness": asdict(trio)})
must(detected["status"] == "verified" and detected["signature"] == "3:1", "3:1 grammar changed")
must(detected["enclosure"]["status"] == "unresolved", "four-cycle became E4 admission")
must(detected["support"]["RD"] is None, "q=1 duration was invented")
tetron_rejection = rejected(lambda:n.motif_witness("Tetron", outer_path, construction, ((pivot,pc),), registry))
e4_rejection = rejected(lambda:n.motif_witness("3:1", outer_path, construction, ((pivot,pc),), registry, enclosure=4))
wave = n.recurrent("field-probe",n=2)
field = n.field("B0","retained-field",(wave,wave))
must(len(field.members)==1,"aliases were counted as distinct waves")
nonwave_rejection = rejected(lambda:n.field("B0","not-a-field",(pivot,)))
one = n.trace("conflict",0,vertices=(0,1,0))
two = n.trace("conflict",0,vertices=(0,2,0))
n.certify(one);n.certify(two)
union=RetainedHistoryUnion();union.include(one)
conflict = rejected(lambda:union.include(two))
must(union.status()["status"] == "rejected", "union failed to retain rejection")
print(json.dumps({"primitive":{"closure":primitive["closure"]["status"],"detector":primitive["status"],"reason":primitive["reason"]},"threefold":{"signature":detected["signature"],"status":detected["status"],"enclosure_status":detected["enclosure"]["status"],"RD":detected["support"]["RD"]},"tetron_name_rejected":tetron_rejection,"enclosure_four_rejected":e4_rejection,"field_alias_member_count":len(field.members),"closure_not_field_rejection":nonwave_rejection,"cross_history_conflict_rejection":conflict},sort_keys=True,indent=2))
