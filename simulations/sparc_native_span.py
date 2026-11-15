"""Calculate native spans from validated returns, without a physical scale map.

Main/Manual ab8359209b8f9b2ef4007403101016e3ea58cfe4 anchors:
* sections/04_cut_running_fractal_tesseract.tex:75-114: distinct returns,
  ordered cadence, and the (u, v] origin convention;
* sections/05_curl_closure_duon_current.tex:255-278: actual folded inner
  witnesses; inner beats are not additional outer returns;
* sections/06_field.tex:1293-1317: address range, section grade and beats
  are separate quantities.

The native producer does not store octal addresses in Wave. Optional address
bindings are therefore identified as supplied metadata, never inferred from
scale or certified by validate_wave. Sparse return traces do not establish
continuous observation between returns. An end_cut selects a committed prefix;
it cannot extend the supplied histories or manufacture future recurrence.
Started support scales and admitted recurrent scales are reported separately.
Full-source hashes are retrospective provenance, not cutoff-local operands.
"""

from collections.abc import Mapping
from dataclasses import asdict

from .vendor import aod_manual_native as native


class NativeSpanError(ValueError):
    pass


def _require(condition, message):
    if not condition:
        raise NativeSpanError(message)


def _collect_waves(roots):
    """Retain the longest compatible prefix for each immutable wave identity."""
    found = {}
    lineages = {}

    def visit(w):
        native.validate_wave(w)
        key = (w.scope, w.scale, w.lineage)
        _require(lineages.get(key, w.identity) == w.identity,
                 "conflicting origins for one retained lineage")
        lineages[key] = w.identity
        prior = found.get(w.identity)
        if prior is not None:
            shorter, longer = sorted((prior, w), key=lambda x: len(x.returns))
            n = len(shorter.returns)
            _require(shorter.returns == longer.returns[:n] and
                     shorter.certificates == longer.certificates[:n] and
                     shorter.motif_witnesses == longer.motif_witnesses[:n],
                     "conflicting histories for one wave identity")
            if prior == longer:
                return
        found[w.identity] = w
        for c in w.certificates:
            for inner in c.inner_waves:
                visit(inner)
        for motif in w.motif_witnesses:
            for c in motif.component_certificates:
                for inner in c.inner_waves:
                    visit(inner)

    for root in roots:
        visit(root)
    return found


def _prefix(t, cut):
    events = tuple((event, vertex, k) for event, vertex, k in
                   zip(t.event_ids, t.vertices, t.indices) if k <= cut)
    return {"scope": t.scope, "scale": t.scale, "lineage": t.lineage,
            "ancestor": t.ancestor, "events": events,
            "source_trace_digest": t.trace_digest,
            "source_trace_digest_scope": "retrospective_full_source_not_cutoff_local",
            # A supplied open tail is retained in full, including hinge roles
            # and reflection. An earlier projection never reveals later bytes.
            "complete_source_trace": asdict(t) if t.indices[-1] <= cut else None,
            "prefix_digest": native.digest((t.scope, t.scale, t.lineage,
                                             t.ancestor, events)),
            "prefix_digest_scope": "events_committed_at_or_before_end_cut",
            "last_committed_cut": events[-1][2]}


def extract_native_span(waves, *, end_cut=None, pending_traces=(), addresses=None):
    """Return JSON-compatible count/window evidence from native Wave objects.

    ``pending_traces`` may carry an unfinished returned path admitted by
    ``validate_trace(require_closed=False)`` after its wave's last closure.
    A future completed trace cut by ``end_cut`` also supplies only its actual
    prefix. Neither is counted as a closure. Pending status describes return
    coverage; it is not a claim about pressure, RCD or a shedding route.

    ``addresses`` optionally maps wave identities to finite tuples of octal
    digits. These bindings supplement the native source and retain that status.
    No component of this function performs SI conversion or time extrapolation.
    """
    _require(isinstance(waves, (list, tuple)) and
             isinstance(pending_traces, (list, tuple)), "finite history sequences required")
    _require(end_cut is None or type(end_cut) is int,
             "end_cut must be an exact integer native cut")
    _require(addresses is None or isinstance(addresses, Mapping),
             "address bindings must be a mapping")
    try:
        native._validate_wave_collection(waves)
        collected = _collect_waves(waves)
        tails = {}
        by_lineage = {(w.scope, w.scale, w.lineage): w for w in collected.values()}
        for t in pending_traces:
            _require(isinstance(t, native.Trace), "pending path must be a native Trace")
            native.validate_trace(t, require_closed=False)
            key = (t.scope, t.scale, t.lineage)
            _require(key in by_lineage, "pending path has no retained wave lineage")
            w = by_lineage[key]
            _require(w.identity not in tails, "overlapping pending histories")
            _require(t.vertices[-1] != t.vertices[0],
                     "closed path requires its certificate, not a pending label")
            _require(t.ancestor == w.returns[-1].ancestor,
                     "pending path changed retained ancestry")
            _require(t.indices[0] > w.returns[-1].indices[-1],
                     "pending path overlaps a certified return")
            tails[w.identity] = t
        all_traces = tuple(native._wave_retained_traces(waves)) + tuple(pending_traces)
        native._check_retained_bindings(all_traces)
    except (native.AdmissionError, AttributeError, TypeError, IndexError) as exc:
        raise NativeSpanError(str(exc)) from exc

    address_map = {} if addresses is None else dict(addresses)
    _require(set(address_map) <= set(collected), "address binding has unknown wave identity")
    for digits in address_map.values():
        _require(isinstance(digits, (tuple, list)) and len(digits) > 0 and
                 all(type(d) is int and 0 <= d < 8 for d in digits),
                 "address requires a nonempty finite sequence of octal digits")

    if not all_traces:
        _require(end_cut is None, "no committed source supports the requested end_cut")
        return {"schema": "aod-native-span-v1", "coverage": "unavailable",
                "continuous_observation_claimed": False,
                "source_end_cut": None, "end_cut": None, "waves": [],
                "fold_dependencies": [], "retained_scope_paths": [],
                "scale_range": None, "scale_range_basis": "started_support",
                "admitted_recurrent_scale_range": None,
                "physical_scale_multiplier": None}
    first_source_cut = min(t.indices[0] for t in all_traces)
    source_end_cut = max(t.indices[-1] for t in all_traces)
    cut = source_end_cut if end_cut is None else end_cut
    _require(first_source_cut <= cut <= source_end_cut,
             "end_cut lies outside the committed source extent")
    rows = []
    folds = []
    visible = set()
    recurrence_status = {}
    for w in collected.values():
        complete = [(t, c) for t, c in zip(w.returns, w.certificates)
                    if t.indices[-1] <= cut]
        traces = w.returns + ((tails[w.identity],) if w.identity in tails else ())
        started = [t for t in traces if t.indices[0] <= cut]
        prefixes = [_prefix(t, cut) for t in started
                    if t.indices[-1] > cut or t is tails.get(w.identity)]
        if started:
            visible.add(w.identity)
        completions = [t.indices[-1] for t, _ in complete]
        windows = [{"left_open": a, "right_closed": b, "span": b - a}
                   for a, b in zip(completions, completions[1:])]
        recurrence_status[w.identity] = ("admitted" if len(complete) >= 2 else
                                        "one_closure" if complete else
                                        "no_completed_return_in_projection")
        known_cut = max((k for t in started for k in t.indices if k <= cut), default=None)
        rows.append({
            "identity": w.identity, "scope": w.scope, "lineage": w.lineage,
            "scale": w.scale, "address": list(address_map[w.identity])
            if w.identity in address_map else None,
            "address_status": "supplied_binding" if w.identity in address_map
            else "not_stored_by_native_producer",
            "closure_count": len(complete), "reclosure_beats": max(len(complete) - 1, 0),
            "recurrence_status": recurrence_status[w.identity],
            "identity_status": "admitted_by_cut" if len(complete) >= 2 else
            "retrospective_wave_identity",
            "first_return_cut": completions[0] if complete else None,
            "last_return_cut": completions[-1] if complete else None,
            "return_ids": [t.return_id for t, _ in complete],
            "certificate_digests": [c.certificate_digest for _, c in complete],
            "cadence": [window["span"] for window in windows],
            "local_windows": windows,
            "reclosure_span": completions[-1] - completions[0] if complete else None,
            "pending_return_prefixes": prefixes,
            "return_status": "pending" if prefixes else "no_source_events_at_cut"
            if not started else "no_pending_path_supplied",
            "last_recorded_cut": known_cut,
            "unobserved_after_last_record": None if known_cut is None or known_cut == cut
            else {"left_open": known_cut, "right_closed": cut},
            "recorded_support_intervals": [
                {"first_cut": t.indices[0], "last_cut": min(t.indices[-1], cut)}
                for t in started],
        })
        for t, c in complete:
            for inner in c.inner_waves:
                folds.append({"outer_identity": w.identity, "inner_identity": inner.identity,
                              "outer_scale": w.scale, "inner_scale": inner.scale,
                              "outer_return_id": t.return_id,
                              "outer_certificate_digest": c.certificate_digest,
                              "inner_certificate_digests": [x.certificate_digest
                                                            for x in inner.certificates]})

    # Paths use actual folded certificates, not inferred steps between scale numbers.
    adjacency = {}
    for f in folds:
        adjacency.setdefault(f["outer_identity"], set()).add(f["inner_identity"])
    paths = []

    def path_from(identity, path):
        path = path + [identity]
        children = adjacency.get(identity, ())
        if not children:
            paths.append([{"identity": i, "scope": collected[i].scope,
                           "scale": collected[i].scale,
                           "recurrence_status": recurrence_status[i],
                           "address": list(address_map[i]) if i in address_map else None}
                          for i in path])
        for child in sorted(children):
            path_from(child, path)

    for identity in dict.fromkeys(w.identity for w in waves):
        if identity in visible:
            path_from(identity, [])
    scales = sorted({collected[i].scale for i in visible})
    recurrent_scales = sorted({collected[i].scale for i in visible
                              if recurrence_status[i] == "admitted"})
    return {"schema": "aod-native-span-v1", "coverage": "supplied_return_histories",
            "continuous_observation_claimed": False,
            "projection_mode": "retrospective_validated_source_prefix",
            "source_end_cut": source_end_cut, "end_cut": cut,
            "waves": sorted(rows, key=lambda r: (r["scale"], r["scope"], r["identity"])),
            "fold_dependencies": folds, "retained_scope_paths": paths,
            "scale_range": None if not scales else
            {"minimum": scales[0], "maximum": scales[-1], "retained_values": scales},
            "scale_range_basis": "started_support",
            "admitted_recurrent_scale_range": None if not recurrent_scales else
            {"minimum": recurrent_scales[0], "maximum": recurrent_scales[-1],
             "retained_values": recurrent_scales},
            "physical_scale_multiplier": None}
