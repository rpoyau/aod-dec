#!/usr/bin/env python3
"""Declared electron-facing C6 / wire-current E2 native accounting example.

Actual native witnesses feed contacts, counted reflections, scoped flux and
the existing next-cut current response. Placement is a finite side-of-interface
declaration. External charge and mass labels do not supply a force law.
"""
import argparse
from dataclasses import asdict, is_dataclass, replace
from fractions import Fraction as Q
import hashlib
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from simulations.vendor import aod_manual_native as n
from simulations import boundary_accounting as b

MANUSCRIPT_BINDING = {'source_base_commit': '8c3149b9d3d924b80b9385b178da4ba2c39175e8',
 'candidate': 'R04 Main/Manual working source; recorded base and candidate bytes are distinct',
 'verification': 'Recorded by the original source-bound review; manuscript files are not reopened '
                 'by this portable runner',
 'files': {'manual/appendices/B_quarantined_external_comparison.tex': '2a4730702a7f1aa7889f297ef58d3f15573ade2b4ce4b89a3631720b5ee10cb1',
           'manual/sections/01_rest_energy_prediction.tex': '40e6af142bfca8d4d97f8f48c50337f7c7d0099954d4e349dea09a9a76c40469',
           'manual/sections/04_boundary_current_applications.tex': '6d517127cb599c502cbeb06deed09451495e4a824f2c93eaaa5a7076a72c37b6',
           'sections/06_field.tex': '74f2b76195fe5dd760540cdcf46a72f615022950ab07e38e4585ad77b643de88'}}
NATIVE_SHA = "918c6c0bd242e7ed0bff6f4d1a4ff7bf2cc840b3da41b6888c218d34eddf5be0"
ACCOUNTING_SHA = "5b2127b90e9527b00bfcea742b98e1bec60d13e344828a34a0c69e86ff713f78"

SCOPE = 'declared:electron-above-wire-interface'
UNIT = 'native-pressure-current'
PLACEMENT = {'wire': 'wire-core-cell', 'electron': 'above-wire-cell'}
INTERVAL = (0, 52)
SCHEDULE = ((4, True), (4, False), (4, True), (6, True))


def encode(value):
    if is_dataclass(value):
        return encode(asdict(value))
    if isinstance(value, Q):
        return str(value)
    if isinstance(value, dict):
        return {str(k): encode(v) for k, v in value.items()}
    if isinstance(value, (tuple, list)):
        return [encode(v) for v in value]
    return value


def digest(value):
    return hashlib.sha256(json.dumps(encode(value), sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def electron_wave():
    lineage = 'electron-facing-C6'
    components = tuple(n.trace(lineage, j, scope=SCOPE, start_index=10*j) for j in range(2))
    component_certs = tuple(n.certify(t) for t in components)
    ids = tuple(c.return_id for c in component_certs)
    construction = ((ids,), ids)
    records, motifs = [], []
    for j, start in enumerate((20, 30, 40), 2):
        trace = n.trace(lineage, j, vertices=(0,1,3,7,6,4,0), turn=3,
                        start_index=start, scope=SCOPE)
        cert = n.certify(trace)
        registry = {c.certificate_digest: c for c in (*component_certs, cert)}
        motif = n.motif_witness('1:2', trace, construction,
                    tuple(zip(components, component_certs)), registry, enclosure=6)
        records.append((trace, cert, registry)); motifs.append(motif)
    return n.wave(tuple(records), tuple(motifs))


def wire_wave():
    lineage = 'wire-representative-Duon-E2'
    component = n.trace(lineage, 0, scope=SCOPE, start_index=0)
    cc = n.certify(component)
    construction = ((1, -1), (cc.certificate_digest, 0), (-1, 1))
    records, motifs = [], []
    for j, start in enumerate((20, 30, 40, 50), 1):
        trace = n.trace(lineage, j, vertices=(0,1,0), turn=1, start_index=start, scope=SCOPE)
        cert = n.certify(trace); registry = {c.certificate_digest: c for c in (cc, cert)}
        motif = n.motif_witness('Duon', trace, construction, ((component, cc),), registry)
        records.append((trace, cert, registry)); motifs.append(motif)
    return n.wave(tuple(records), tuple(motifs))


def counted_contacts(wire, electron):
    """Count completed-return hinge IDs in disjoint intervals after formation.

    This is an explicit application projection. Native compare validates its
    integer/domain/prefix inputs; this adapter additionally recomputes the count
    witnesses instead of accepting contacts()'s illustrative default counts.
    """
    contacts, witnesses = [], []
    previous = {w.identity: w.returns[0].indices[-1] for w in (wire, electron)}
    for base in n.contacts(wire, electron):
        sides = []
        for wave in (wire, electron):
            lower = previous[wave.identity]
            selected = tuple(t for t in wave.returns[1:] if lower < t.indices[-1] <= base.committed)
            sides.append({'wave_identity': wave.identity, 'interval': (lower, base.committed),
                          'first_formation_return_excluded': wave.returns[0].return_id,
                          'completed_returns': tuple({'return_id': t.return_id,
                              'completed': t.indices[-1], 'hinge_id': t.event_ids[t.turn],
                              'hinge_committed': t.indices[t.turn]} for t in selected),
                          'count': len(selected)})
            previous[wave.identity] = base.committed
        actual = replace(base, reflection=tuple(side['count'] for side in sides))
        contacts.append(actual)
        witnesses.append({'contact_identity': actual.identity, 'committed': actual.committed,
                          'sides': tuple(sides), 'projection': 'completed-return hinge count after initial formation'})
    return tuple(contacts), tuple(witnesses)


def channel(wire, electron, contacts, window, available):
    expected, witnesses = counted_contacts(wire, electron)
    require(contacts == expected, 'reflection/contact substitution differs from enumerated native return history')
    comparison = n.compare(wire, electron, contacts, window=INTERVAL)
    reverse = n.compare(wire, electron, contacts, window=INTERVAL, reverse=True)
    rcd = comparison['rcd']
    participation = n.participation(rcd, window, rcd.domain)
    capacity, intensity, closure_cost = Q(3), Q(1, 3), Q(24)
    magnitude = capacity * participation
    orientation = comparison['lag'] * intensity
    flux = magnitude * orientation if available else Q(0)
    burden = magnitude * abs(comparison['lag']) if available else Q(0)
    residual = burden - closure_cost
    return {'comparison': comparison, 'reverse_comparison': reverse,
            'reflection_witnesses': witnesses, 'RCD': rcd, 'RD_contact_support': rcd.support,
            'RCD_duration': rcd.duration(), 'window': window, 'available': available,
            'participation': participation, 'capacity': capacity, 'intensity': intensity,
            'pressure_magnitude': magnitude, 'active_pressure_magnitude': magnitude if available else Q(0),
            'orientation': orientation, 'wire_outward_flux': flux,
            'electron_outward_flux': -flux, 'active_closure_burden': burden,
            'closure_cost': closure_cost, 'closure_residual': residual,
            'positive_surplus': max(Q(0), residual)}


def synchronize(wave):
    state = n.initial_state(wave, pressure=Q(1))
    updates = []
    while state.committed < INTERVAL[1]:
        before = state
        state = n.update(before, Q(0), input_index=before.committed)
        updates.append({'input_cut': before.committed, 'input': Q(0), 'before': before, 'after': state})
    require(state.committed == INTERVAL[1], 'synchronization exceeds shared committed cut')
    return state, tuple(updates)


def execute(wire, electron, contacts, schedule):
    states, synchronization = {}, {}
    for name, wave in (('wire', wire), ('electron', electron)):
        states[name], synchronization[name] = synchronize(wave)
    archive = b.ArchiveState(); previous_flows = (); previous_status = 'active'
    slices = {'wire': [], 'electron': [], 'union': []}; rows = []
    history = {'wire': wire, 'electron': electron, 'contacts': contacts, 'interval': INTERVAL}
    history_digest = digest(history)
    lineage = 'wire-electron-same-physical-channel'
    for step, (window, available) in enumerate(schedule):
        cut = INTERVAL[1] + step
        ch = channel(wire, electron, contacts, window, available)
        flux = ch['wire_outward_flux']
        oriented = n.aod_flux(((0, 1, ch['active_pressure_magnitude'], ch['orientation'], lineage),))
        stokes = {name: n.stokes(oriented, region) for name, region in
                  (('wire', {0}), ('electron', {1}), ('union', {0,1}))}
        require(stokes['wire']['boundary'] == flux and stokes['electron']['boundary'] == -flux
                and stokes['union']['boundary'] == stokes['union']['internal'] == 0,
                'native scoped orientation or Stokes cancellation mismatch')
        additions, changes, flow_ids = [], [], {}
        status = 'active' if available else 'inactive'
        if step == 0:
            retained = b.ContentRecord('retained:wire-electron', 'retained:contact-family',
                digest({'RCD': ch['RCD'], 'history': history_digest}), cut, 0,
                'native-return-count', Q(ch['RCD_duration']), ('retained:contact-family',))
            additions.append((retained, status))
        else:
            changes.extend(b.DispositionChange(identity, 'active', 'inactive',
                'past projection retained; later cut gets a fresh additive occurrence') for identity in previous_flows)
            if status != previous_status:
                changes.append(b.DispositionChange('retained:wire-electron', previous_status, status,
                    'declared coupling gate; retained history is not erased'))
        if available:
            physical = 'channel-occurrence:' + str(cut)
            for name, value in (('wire', flux), ('electron', -flux)):
                identity = 'view:' + name + ':' + str(cut)
                record = b.ContentRecord(identity, physical,
                    digest({'history': history_digest, 'channel': ch, 'oriented_view': name, 'cut': cut}),
                    cut, 0, UNIT, value, (physical,))
                additions.append((record, 'active')); flow_ids[name] = identity
        next_archive = b.commit_archive(archive, cut, tuple(additions), tuple(changes))
        b.validate_successor(archive, next_archive)
        scoped = {}
        for name, region in (('wire', (PLACEMENT['wire'],)), ('electron', (PLACEMENT['electron'],)),
                             ('union', (PLACEMENT['wire'], PLACEMENT['electron']))):
            cross = (flow_ids[name],) if available and name != 'union' else ()
            boundary = name + ':boundary'
            source = {'history': history_digest, 'channel': ch, 'placement': PLACEMENT,
                      'region': region, 'cut': cut, 'cross': cross,
                      'shedding': (), 'internal_oriented_pairs': oriented if name == 'union' else {},
                      'native_stokes': stokes[name]}
            motion = b.BoundaryMotion('stationary', region, region, Q(0), UNIT,
                                      digest({'cut': cut, 'region': region, 'sweep': Q(0)}))
            coverage = b.BoundaryCoverage(cut, boundary, 0, UNIT, cross, (), SCOPE,
                window, 'native-return-count', digest(source))
            result = b.account_slice(next_archive, cut, boundary, 0, UNIT, cross=cross,
                                      shedding=(), motion=motion, coverage=coverage)
            require(result.moving_total == stokes[name]['boundary'], 'accounting differs from native Stokes boundary')
            scoped[name] = {'slice': result, 'coverage_operands': source}
            slices[name].append(result)
        before = dict(states); response_inputs = {}
        for name in ('wire', 'electron'):
            # Explicit application bridge: an outward boundary contribution is
            # negative input to this region's retained native current coordinate.
            response_inputs[name] = -scoped[name]['slice'].moving_total
            states[name] = n.update(states[name], response_inputs[name], input_index=cut)
            require(states[name].committed == cut+1, 'response is not on the next committed cut')
        require(states['wire'].current + states['electron'].current == 0,
                'paired declared response increments do not balance')
        require(digest(history) == history_digest, 'coupling or projection changed retained histories')
        rows.append({'input_cut': cut, 'window': window, 'available': available, 'channel': ch,
                     'oriented_flux': tuple({'from': i, 'to': j, 'lineage': line, 'value': value}
                                             for (i,j,line), value in oriented.items()),
                     'native_stokes': stokes, 'scoped_accounting': scoped,
                     'archive': next_archive, 'before': before, 'response_inputs': response_inputs,
                     'after': dict(states), 'retained_history_sha256': history_digest})
        archive, previous_flows, previous_status = next_archive, tuple(flow_ids.values()), status
    totals = {name: b.accumulate_current(tuple(items)) for name, items in slices.items()}
    return {'history': history, 'history_sha256': history_digest, 'synchronization': synchronization,
            'schedule': schedule, 'rows': rows, 'window_totals': totals,
            'final_states': states, 'final_archive': archive}


def main():
    parser = argparse.ArgumentParser(); parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    sources = [Path(n.__file__), Path(b.__file__)]
    bindings = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}
    require(hashlib.sha256(Path(n.__file__).read_bytes()).hexdigest() == NATIVE_SHA,
            'native producer differs from bound R04 source')
    require(hashlib.sha256(Path(b.__file__).read_bytes()).hexdigest() == ACCOUNTING_SHA,
            'accounting dependency changed')
    checks, hostile = [], {}

    def check(name, condition):
        require(condition, 'check failed: ' + name)
        checks.append({'name': name, 'status': 'PASS'})

    def reject(name, candidate, call):
        try:
            call()
        except ValueError as exc:
            hostile[name] = {'candidate': candidate, 'error': str(exc), 'error_type': type(exc).__name__}
            checks.append({'name': name, 'status': 'REJECTED_AS_REQUIRED'}); return
        raise RuntimeError('hostile candidate admitted: ' + name)

    electron, wire = electron_wave(), wire_wave()
    field = n.field(SCOPE, 'declared-electron-wire-field', (wire, electron))
    contacts, reflections = counted_contacts(wire, electron)
    check('actual_electron_1_2_C6', electron.motif == '1:2' and
          tuple(n.phase_step_count(t) for t in electron.returns) == (6,6,6))
    check('actual_wire_Duon_E2', wire.motif == 'Duon' and
          tuple(n.phase_step_count(t) for t in wire.returns) == (2,2,2,2))
    check('electron_components_before_targets', tuple(t.indices[-1] for t in electron.motif_witnesses[0].component_traces) == (2,12))
    check('actual_completed_return_cuts', tuple(t.indices[-1] for t in electron.returns) == (26,36,46)
          and tuple(t.indices[-1] for t in wire.returns) == (22,32,42,52))
    check('contacts_at36_and46', tuple(c.committed for c in contacts) == (36,46))
    check('enumerated_reflection_counts', tuple(c.reflection for c in contacts) == ((1,1),(1,1)))
    baseline = execute(wire, electron, contacts, SCHEDULE)
    gate_off = execute(wire, electron, contacts, tuple((window, False) for window, _ in SCHEDULE))
    rows = baseline['rows']; comparison = rows[0]['channel']['comparison']
    check('native_ratio3_over2_and_lag1', comparison['ratio'] == Q(3,2) and comparison['lag'] == 1)
    check('contact_RD3_RCD5', comparison['rcd'].support == 3 and comparison['rcd'].duration() == 5)
    check('reverse_ratio_and_lag', rows[0]['channel']['reverse_comparison']['ratio'] == Q(2,3)
          and rows[0]['channel']['reverse_comparison']['lag'] == -1)
    check('clipped_participation4_4_4_5', tuple(r['channel']['participation'] for r in rows) == (4,4,4,5))
    check('computed_wire_flux4_0_4_5', tuple(r['channel']['wire_outward_flux'] for r in rows) == (4,0,4,5))
    check('electron_outward_opposite', all(r['channel']['electron_outward_flux'] == -r['channel']['wire_outward_flux'] for r in rows))
    check('paired_boundary_totals13_minus13_zero', tuple(baseline['window_totals'][name]['current'] for name in ('wire','electron','union')) == (13,-13,0))
    check('later_wire_currents', tuple(r['after']['wire'].current for r in rows) == (-4,-4,-8,-13))
    check('later_electron_currents', tuple(r['after']['electron'].current for r in rows) == (4,4,8,13))
    check('all_responses_next_cut', tuple(r['after']['wire'].committed for r in rows) == (53,54,55,56)
          and tuple(r['after']['electron'].committed for r in rows) == (53,54,55,56))
    check('zero_input_synchronization', len(baseline['synchronization']['wire']) == 0
          and len(baseline['synchronization']['electron']) == 6
          and all(x['after'].current == 0 for x in baseline['synchronization']['electron']))
    check('burden_not_signed_cancellation', tuple(r['channel']['active_closure_burden'] for r in rows) == (12,0,12,15))
    check('closure_deficits_no_surplus', tuple(r['channel']['closure_residual'] for r in rows) == (-12,-24,-12,-9)
          and all(r['channel']['positive_surplus'] == 0 for r in rows))
    check('stationary_zero_sweep_zero_shedding', all(v['slice'].sweep_total == v['slice'].shedding_total == 0
          for r in rows for v in r['scoped_accounting'].values()))
    check('gateoff_same_native_history', baseline['history_sha256'] == gate_off['history_sha256'])
    check('gateoff_no_response', all(r['after']['wire'].current == r['after']['electron'].current == 0 for r in gate_off['rows']))
    check('dropout_retains_history_and_current', rows[1]['after']['electron'].current == rows[0]['after']['electron'].current
          and rows[1]['scoped_accounting']['electron']['slice'].cross == ())

    changed_contacts = (replace(contacts[0], reflection=(99,1)),) + contacts[1:]
    reject('reflection_count_substitution', changed_contacts, lambda: channel(wire, electron, changed_contacts, 4, True))
    short_motif = replace(electron.motif_witnesses[0], component_traces=electron.motif_witnesses[0].component_traces[:-1])
    reject('missing_electron_component', short_motif, lambda: n.validate_motif(short_motif))
    first = rows[0]['scoped_accounting']['wire']['slice']
    reject('boundary_coverage_omission', first, lambda: b.account_slice(first.archive, first.cut,
        first.boundary_id, first.scale, first.unit, cross=(), shedding=(), motion=first.motion, coverage=first.coverage))
    both = ('view:wire:52', 'view:electron:52')
    coverage = replace(first.coverage, cross=both, provenance=digest({'hostile': 'same physical occurrence twice', 'views': both}))
    reject('same_occurrence_oriented_views_doublecounted', {'archive': first.archive, 'coverage': coverage},
        lambda: b.account_slice(first.archive, first.cut, first.boundary_id, first.scale, first.unit,
                               cross=both, shedding=(), motion=first.motion, coverage=coverage))
    lost = b.ArchiveState(baseline['final_archive'].commits[:-1])
    reject('archive_history_lost', lost, lambda: b.validate_successor(baseline['final_archive'], lost))
    wrong_unit = replace(first, unit='metres', coverage=replace(first.coverage, unit='metres'),
                         motion=replace(first.motion, unit='metres'))
    reject('native_current_relabelled_as_length', wrong_unit, lambda: b.accumulate_current((wrong_unit,)))
    forged = replace(first, cross_total=Q(400), moving_total=Q(400))
    reject('computed_slice_substitution', forged, lambda: b.accumulate_current((forged,)))
    reject('incompatible_RCD_window_domain', {'RCD': comparison['rcd'], 'window': 4, 'domain': 'seconds'},
        lambda: n.participation(comparison['rcd'], 4, 'seconds'))
    future = rows[0]['before']['electron']
    reject('future_response_input', {'state': future, 'input_cut': future.committed+1},
           lambda: n.update(future, Q(4), future.committed+1))
    check('source_bytes_unchanged', all(hashlib.sha256(p.read_bytes()).hexdigest() == bindings[str(p.relative_to(ROOT))] for p in sources))
    report = {
        'status': 'PASS', 'support_class': 'D0 new declared native electron-facing support / wire-current accounting application',
        'source_sha256': bindings, 'recorded_manuscript_binding': MANUSCRIPT_BINDING, 'runner_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        'preparation': {'scope': SCOPE, 'field': field, 'placement': PLACEMENT,
            'electron_native_chirality': 1, 'wire_native_chirality': 1,
            'native_chirality_relation_to_external_charge': 'No identity inferred; external charge is a separately retained Manual map.',
            'electron_support': '1:2:6 with actual six-edge C6 target paths',
            'wire_support': 'Duon construction with elementary two-edge E2 target paths; motif API enclosure metadata6 is not a six-edge traversal claim',
            'comparison_interval': INTERVAL, 'reflection_count_witnesses': reflections,
            'capacity': Q(3), 'intensity': Q(1,3), 'closure_cost': Q(24), 'response_pressure': Q(1),
            'response_bridge': 'Minus each region outward accounted moving_total feeds native current update at its next cut.'},
        'external_electron_record': {'source': 'manual/sections/01_rest_energy_prediction.tex:223-247',
            'mapped_support': '00|- 1:2:6', 'z': -1, 'Q_ext': '-e',
            'mass_energy_target_MeV': '0.51099895069', 'standard_uncertainty_MeV': '0.00000000016',
            'role': 'Stored comparison target, not input to native dynamics; no mass/force prediction is computed.'},
        'checks': checks, 'baseline': baseline, 'all_gate_off_control': gate_off, 'hostile': hostile,
        'summary': {'checks': len(checks), 'positive_checks': sum(c['status']=='PASS' for c in checks),
            'hostile_rejections': len(hostile), 'contacts': tuple(c.committed for c in contacts),
            'reflection_counts': tuple(c.reflection for c in contacts), 'RCD_duration': comparison['rcd'].duration(),
            'wire_flux': tuple(r['channel']['wire_outward_flux'] for r in rows),
            'electron_later_current': tuple(r['after']['electron'].current for r in rows),
            'wire_later_current': tuple(r['after']['wire'].current for r in rows),
            'closure_surplus': tuple(r['channel']['positive_surplus'] for r in rows),
            'wire_window_current': baseline['window_totals']['wire']['current'],
            'electron_window_current': baseline['window_totals']['electron']['current'],
            'union_window_current': baseline['window_totals']['union']['current']},
        'limits': ['Above-wire is a declared finite side-of-interface label, not a metre coordinate.',
            'The finite preparation, comparison projection, coupling schedule and outward-sign response bridge are explicit application choices.',
            'The engine admits and executes native traces; no stochastic probability law is substituted for it.',
            'No physical electron force, SI trajectory, magnetic field, wire current in amperes or mass prediction is claimed.',
            'This coupling-gate control is not a retention-only matched awareness intervention.',
            'No positive surplus occurs under closure cost24; no shedding or new-layer certificate is invented.']}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(encode(report), indent=2, sort_keys=True)+'\n')
    print(json.dumps(encode({'status': 'PASS', **report['summary'],
        'output_sha256': hashlib.sha256(args.output.read_bytes()).hexdigest()}), sort_keys=True))


if __name__ == '__main__':
    main()
