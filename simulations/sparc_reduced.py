"""Declared radial AOD response on fixed support with moving control boundaries.

Source: Main/Manual ab8359209b8f9b2ef4007403101016e3ea58cfe4,
Main06 field-fluid regime, pressure/current and inverse-pressure response;
Main06_wave count-comparator realization; Manual06 G0/G1 and slice support.

This reduced application fixes its input/response/projection choices before
target comparison. It does not infer a full galaxy from a scalar fit. Native
Duon representatives are computational clustering choices, not measured galaxy
formations. Physical mass is never treated as native capacity or RCD.
"""
from dataclasses import replace
from fractions import Fraction as Q

from simulations import reduced_duon_history as history
from simulations import reduced_boundary_response as response
from simulations import region_engine as region
from simulations.vendor import aod_manual_native as native
from simulations.sparc_inputs import PreparationRow
from simulations.sparc_inverse import baseline_terms
from simulations.sparc_comparison import Projection
from simulations.sparc_native_span import extract_native_span


MODEL = 'sparc-radial-native-response-v1'
MAIN_MANUAL = 'ab8359209b8f9b2ef4007403101016e3ea58cfe4'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def _rows(rows):
    rows = tuple(rows)
    require(len(rows) >= 2 and all(type(row) is PreparationRow for row in rows),
            'at least two typed radial preparation rows required')
    galaxies = {row.key.galaxy for row in rows}
    require(len(galaxies) == 1, 'one galaxy per reduced run')
    require(len({row.distance_mpc for row in rows}) == 1, 'mixed distance preparation')
    for row in rows:
        baseline_terms(row)  # exact component and identity validation
    rows = tuple(sorted(rows, key=lambda row: row.key))
    require(len({row.key for row in rows}) == len(rows), 'duplicate radial support')
    return rows


def _compact_boundary(record):
    row = record['audit'].rows[0]
    return {key: record[key] for key in ('regional_stock_before', 'regional_stock_after',
                                        'stock_change', 'balance_residual', 'sweep_policy')} | {
        'before_cells': record['frame'].before.cells,
        'after_cells': record['frame'].after.cells,
        'kind': record['frame'].kind,
        'cross': row['cross'], 'sweep': row['sweep'], 'shedding': row['shedding'],
        'current': row['current'], 'snapshot': row['snapshot'],
        'crossing_policy': record['crossing_policy']}


def simulate_galaxy(rows, *, coupled=True, geometry='spacing'):
    """Execute the frozen reduced model using preparation only, never targets.

    Spacing intensity is min(adjacent gap)/gap, a declared dimensionless
    coupling specialization. The unit-intensity control removes that geometry.
    One self-return pressure plus incident channel pressures makes the native
    inverse-pressure update a convex combination. The stopping cut is the first
    certified recurrence completion with at least graph-diameter response steps;
    no observation residual selects that cut.
    """
    rows = _rows(rows)
    require(type(coupled) is bool, 'explicit Boolean coupling control required')
    require(geometry in ('spacing', 'unit'), 'unknown coupling geometry')
    galaxy = rows[0].key.galaxy
    cells = tuple(f'{galaxy}:radial:{row.key.radius_kpc}' for row in rows)
    scope = MODEL + ':' + galaxy
    pool = history.prepare(cells, scope=scope)
    initial_cut = pool.committed_cut
    initial_span = extract_native_span(pool.waves)
    cadences = {sep for wave in pool.waves for sep in wave.cadence}
    require(len(cadences) == 1, 'this reduced specialization requires a common certified cadence')
    cadence = next(iter(cadences))
    require(type(cadence) is int and cadence > 0, 'positive certified cadence required')
    diameter = len(cells)-1
    steps = ((diameter+cadence-1)//cadence)*cadence
    gaps = tuple(b.key.radius_kpc-a.key.radius_kpc for a,b in zip(rows, rows[1:]))
    min_gap = min(gaps)
    intensity = tuple(min_gap/gap if geometry == 'spacing' else Q(1) for gap in gaps)
    initial_dbf = tuple(baseline_terms(row)['base_dbf'] for row in rows)
    scale = max(initial_dbf)
    # A zero input family is a zero-response control, not an undefined ratio.
    normalization = scale if scale > 0 else Q(1)
    theta = tuple(value/normalization for value in initial_dbf)
    preparation_digest = region.fingerprint(rows)

    def coupling_evidence(current_pool):
        return tuple(history.channel_comparison(current_pool, a, b)
                     for a,b in zip(cells, cells[1:]))

    comparisons = coupling_evidence(pool)
    capacities = tuple(len(item.contacts[-1].support_trace.roles) for item in comparisons)
    pressures = tuple(Q(cap)*item.participation for cap,item in zip(capacities,comparisons))
    require(len(set(pressures)) == 1, 'representative channel pressures must match')
    p = pressures[0]
    own = pool.waves[0].returns[-1]
    self_rd = (len(own.vertices)-1)+len(own.roles)-2
    self_pressure = Q(len(own.roles))*min(self_rd, cadence)
    require(p == self_pressure and p > 0, 'self/channel pressure mismatch')
    degree = tuple(1 if index in (0,len(cells)-1) else 2 for index in range(len(cells)))
    cell_pressure = tuple(self_pressure+degree[index]*p for index in range(len(cells)))
    states = {cell: replace(native.initial_state(pool.wave(cell), pressure=cell_pressure[index]),
                            current=theta[index], committed=pool.committed_cut)
              for index,cell in enumerate(cells)}
    initial_stock = sum((states[cell].pressure*states[cell].current for cell in cells), Q(0))
    threshold = initial_stock/sum(cell_pressure, Q(0))
    initial_selection = tuple(cell for cell in cells if states[cell].current >= threshold)
    whole = region.Region('whole', cells)
    static_region = region.Region('initial-threshold-fixed', initial_selection)
    native_records, transitions = [], []
    history_chain = region.fingerprint({'model':MODEL,'preparation':preparation_digest,
        'coupled':coupled,'geometry':geometry,'steps':steps,'initial_states':states})
    evidence_cache_cut = None

    for step in range(steps):
        # All channel operands use this already committed state. Boundary
        # membership is derived only after the synchronous native response.
        cut = pool.committed_cut
        latest_completion = pool.waves[0].returns[-1].indices[-1]
        if latest_completion != evidence_cache_cut:
            comparisons = coupling_evidence(pool)
            require(all(Q(cap)*item.participation == p
                        for cap,item in zip(capacities,comparisons)),
                    'changed pressure requires a different response-stock model')
            require(all(item.lag == 0 and item.window[1]-item.window[0] == cadence
                        for item in comparisons), 'unexpected native count lag/window')
            evidence_digest = region.fingerprint(comparisons)
            native_records.append({'at_cut':cut,'comparison_sha256':evidence_digest,
                                   'comparisons':comparisons})
            evidence_cache_cut = latest_completion
        channels, operands = [], []
        for index,(left,right,item) in enumerate(zip(cells,cells[1:],comparisons)):
            bias = (states[left].current, states[right].current)
            lag = item.lag+bias[0]-bias[1]
            current = p*lag*intensity[index]
            operands.append({'left':left,'right':right,'pressure':p,
                'capacity_positions':capacities[index], 'participation':item.participation,
                'rcd_duration':item.duration,'native_count_lag':item.lag,
                'committed_offsets':bias,'lag':lag,'intensity':intensity[index],
                'value':current,'comparison_sha256':evidence_digest,
                'input_cut':cut,'source_return_cut':latest_completion})
            identity = f'{scope}:cut:{cut}:edge:{index}'
            channels.append(region.Channel(identity, identity, left, right, current,
                region.fingerprint(operands[-1]), available=coupled))
        channels = tuple(channels)
        result = response.response_step(states, channels)
        after = result['after']
        before_selection = tuple(cell for cell in cells if states[cell].current >= threshold)
        after_selection = tuple(cell for cell in cells if after[cell].current >= threshold)
        selected_before = region.Region('above-initial-mean', before_selection)
        selected_after = region.Region('above-initial-mean', after_selection)
        complement_before = region.region_difference('below-initial-mean',whole,selected_before)
        complement_after = region.region_difference('below-initial-mean',whole,selected_after)
        frames = {'selected':(selected_before,selected_after),
                  'complement':(complement_before,complement_after),
                  'whole':(whole,whole), 'stationary':(static_region,static_region)}
        audits = {name:response.boundary_step(states,after,channels,before,following,
                    scope=scope,window=cadence,window_domain=response.WINDOW_DOMAIN)
                  for name,(before,following) in frames.items()}
        partition = region.audit_partition(audits['whole']['audit'],
            (audits['selected']['audit'],audits['complement']['audit']))
        partition_residuals = {name:audits['selected']['audit'].rows[0][name]
            +audits['complement']['audit'].rows[0][name]-audits['whole']['audit'].rows[0][name]
            for name in ('cross','sweep','shedding','current')}
        require(all(value == 0 for value in partition_residuals.values()), 'partition loses current')
        next_theta = tuple(after[cell].current for cell in cells)
        require(all(min(theta) <= value <= max(theta) for value in next_theta),
                'response escaped the input range; no clipping is permitted')
        row = {'cut':cut,'response_cut':cut+1,'before_theta':tuple(states[cell].current for cell in cells),
            'after_theta':next_theta, 'channels':channels,'operands':tuple(operands),
            'boundaries':{name:_compact_boundary(value) for name,value in audits.items()},
            'partition_residuals':partition_residuals,'conservation_residual':result['conservation_residual'],
            'partition_status':partition['status'],
            'before_total_stock':sum(result['before_stock'].values(),Q(0)),
            'after_total_stock':sum(result['after_stock'].values(),Q(0)),
            'previous_history_sha256':history_chain}
        history_chain = region.fingerprint(row)
        row['history_sha256'] = history_chain
        transitions.append(row)
        pool = history.advance(pool)
        require(pool.committed_cut == cut+1, 'native history and response cut differ')
        if pool.waves[0].returns[-1].indices[-1] == pool.committed_cut:
            states = {cell:replace(native.initial_state(pool.wave(cell),pressure=after[cell].pressure),
                                  current=after[cell].current,committed=pool.committed_cut) for cell in cells}
        else:
            states = after

    final_dbf = tuple(normalization*states[cell].current for cell in cells)
    source_evidence = ('preparation:'+preparation_digest, 'history:'+history_chain,
                       'model:'+MODEL+':'+geometry+':coupled='+str(coupled))
    projections = tuple(Projection(row.key, baseline_terms(row)['u_bar']+8*value,
                          MODEL+':fixed-Ubar-plus-8-DBF',source_evidence)
                        for row,value in zip(rows,final_dbf))
    boundary_totals = {name:{field:sum((row['boundaries'][name][field] for row in transitions),Q(0))
                            for field in ('cross','sweep','shedding','current','stock_change')}
                       for name in ('selected','complement','whole','stationary')}
    return {'model':MODEL,'galaxy':galaxy,'support_class':'declared radial reduction; G0/G1 inputs',
        'preparation_sha256':preparation_digest,'rows':rows,'cells':cells,
        'coupled':coupled,'geometry':geometry,'intensity':intensity,
        'normalization_kpc_km_s':normalization,'initial_dbf':initial_dbf,'final_dbf':final_dbf,
        'cell_pressure':cell_pressure,'self_pressure':self_pressure,'channel_pressure':p,
        'threshold':threshold,'threshold_rule':'initial pressure-weighted mean; fixed during evolution',
        'initial_cut':initial_cut,'final_cut':pool.committed_cut,'response_steps':steps,
        'graph_diameter':diameter,'computed_cadence':cadence,
        'stopping_rule':'first recurrence completion after at least graph-diameter response steps',
        'initial_native_span':initial_span,'final_native_span':extract_native_span(pool.waves),
        'native_history':pool,'native_comparisons':tuple(native_records),
        'transitions':tuple(transitions),'boundary_totals':boundary_totals,
        'moving_boundary_cuts':sum(row['boundaries']['selected']['kind']=='moving' for row in transitions),
        'initial_total_stock':initial_stock,
        'final_total_stock':sum((states[cell].pressure*states[cell].current for cell in cells),Q(0)),
        'projections':projections,'history_sha256':history_chain,
        'boundary_declaration':{'radial_ports':'closed in this reduction',
            'out_of_plane_ports':'closed-slice ablation; physical exchange remains unknown',
            'unresolved_exchange_error':None,
            'shedding':'no separate shedding channel in this fixed-support reduction; not a claim of absent physical shedding'},
        'quantity_types':{'theta':'dimensionless committed orientation offset',
            'stock':response.STOCK_KIND,'DBF':'evolving projected support coefficient [kpc km/s], not lambda_ret',
            'cuts':'native committed indices, no SI-time calibration'},
        'status':'executed reduced native response and moving control-boundary accounting'}
