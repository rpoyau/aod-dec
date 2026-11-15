"""Exact scalar/CUDA incidence with unchanged host boundary-accounting records.

Host admission binds complete source occurrences, identities and swept fields.
The device computes bounded integer membership/incidence coefficients only.
Host Fraction arithmetic consumes those coefficients in source order. Neither
backend constructs a native closure, invents a route, or resolves unsigned burden.
The original region_engine.audit_boundary remains an independent oracle.
"""
from dataclasses import dataclass
from fractions import Fraction
import ctypes as C
import hashlib
from pathlib import Path
import sys
import time

from . import chiral_gpu as cuda
from . import region_engine as region


GPUUnavailable = cuda.GPUUnavailable
KERNEL = Path(__file__).with_name('kernels') / 'boundary_incidence.cu'
MAX_ROWS = 1_000_000
MAX_MEMBERSHIPS = 16_000_000
MAX_CELLS = 1_000_000
FIELDS = ('orientation', 'selected', 'internal', 'exterior', 'inactive', 'bucket')


class BoundaryAccelerationError(ValueError):
    pass


def _require(condition, message):
    if not condition:
        raise BoundaryAccelerationError(message)


@dataclass(frozen=True)
class _Plan:
    ledger: region.Ledger
    name: str
    frames: tuple
    scope: str
    representation: object
    factor: Fraction
    target_scale: int
    arrays: dict
    blocks: tuple
    source_digest: str


def _source_digest(ledger, name, frames, scope, representation):
    return region.fingerprint((ledger, name, frames, scope, representation))


def _source_bindings():
    root = Path(__file__).resolve().parents[1]
    paths = (Path(__file__), KERNEL, Path(region.__file__), Path(cuda.__file__), Path(region.accounting.__file__))
    return {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest() for path in paths}


def _prepare(ledger, name, frames, scope, representation):
    # These are the same source and motion preconditions as the original audit.
    region.require(type(ledger) is region.Ledger, 'typed ledger required')
    ledger.__post_init__()
    region.accounting.text(name, 'boundary name')
    region.require(scope in ('declared-current', 'full'), 'explicit current coverage scope required')
    region.require(type(frames) is tuple and len(frames) == len(ledger.commits) and bool(frames),
                   'exactly one frame for every committed input cut required')
    factor, target_scale = Fraction(1), ledger.scale
    if representation is not None:
        region.require(type(representation) is region.RepresentationMap,
                       'typed explicit representation map required')
        representation.__post_init__()
        region.require((representation.source_scale, representation.unit) == (ledger.scale, ledger.unit),
                       'representation source scale or quantity unit differs')
        factor, target_scale = representation.factor, representation.target_scale
    cells = {cell: index for index, cell in enumerate(ledger.cells)}
    _require(len(cells) <= MAX_CELLS and len(cells)*len(frames) <= MAX_MEMBERSHIPS,
             'boundary membership buffer budget exceeded')
    arrays = {key: [] for key in ('source', 'target', 'event_cut', 'available', 'kind',
                                  'before', 'after', 'sweep_cell', 'sweep_cut')}
    previous, pending, blocks = None, [], []
    universe = set(cells)
    for index, (cut, frame) in enumerate(zip(ledger.commits, frames)):
        region.require(type(frame) is region.Frame, 'typed frame required')
        frame.__post_init__()
        before, after = set(frame.before.cells), set(frame.after.cells)
        region.require(before <= universe and after <= universe, 'region outside declared universe')
        if previous is not None:
            region.require(before == previous, 'disconnected boundary motion schedule')
        previous = after
        field = {sample.cell: sample for sample in cut.sweep_field}
        for sample in frame.sweep:
            region.require(sample.cell in field and sample == field[sample.cell],
                           'sweep sample differs from same-cut committed field')
        pending.extend(cut.obligations)
        _require(len(arrays['source'])+len(cut.channels)+len(pending) <= MAX_ROWS and
                 len(arrays['sweep_cell'])+len(frame.sweep) <= MAX_ROWS,
                 'boundary occurrence buffer budget exceeded')
        arrays['before'].extend(int(cell in before) for cell in ledger.cells)
        arrays['after'].extend(int(cell in after) for cell in ledger.cells)
        event_start, sweep_start = len(arrays['source']), len(arrays['sweep_cell'])
        for event in (*cut.channels, *pending):
            is_channel = type(event) is region.Channel
            arrays['source'].append(cells[event.source])
            arrays['target'].append(cells[event.target])
            arrays['event_cut'].append(index)
            arrays['available'].append(int(event.available) if is_channel else 1)
            arrays['kind'].append((0 if event.kind == 'cross' else 1) if is_channel else 2)
        for sample in frame.sweep:
            arrays['sweep_cell'].append(cells[sample.cell])
            arrays['sweep_cut'].append(index)
        blocks.append((event_start, sweep_start, tuple(pending)))
    return _Plan(ledger, name, frames, scope, representation, factor, target_scale,
                 {key: tuple(value) for key, value in arrays.items()}, tuple(blocks),
                 _source_digest(ledger, name, frames, scope, representation))


def _scalar_coefficients(plan):
    a, cells = plan.arrays, len(plan.ledger.cells)
    events = []
    for source, target, cut, available, kind in zip(
            a['source'], a['target'], a['event_cut'], a['available'], a['kind']):
        left, right = a['after'][cut*cells+source], a['after'][cut*cells+target]
        orientation = left-right
        selected = int(bool(available and orientation))
        events.append((orientation, selected, int(bool(left and right)),
                       int(not left and not right), int(not available),
                       kind+1 if selected else 0))
    sweeps = tuple(a['after'][cut*cells+cell]-a['before'][cut*cells+cell]
                   for cell, cut in zip(a['sweep_cell'], a['sweep_cut']))
    return tuple(events), sweeps, {'gpu_executed': False, 'launch_count': 0}


def compile_kernel(architecture, *, source_path=KERNEL):
    """Compile this explicit CUDA source; compilation is not device execution."""
    import re
    _require(type(architecture) is str and re.fullmatch(r'compute_[0-9]{2,3}', architecture),
             'an explicit CUDA compute architecture is required')
    lib = cuda._load('nvrtc')
    pointer, integer, size = C.c_void_p, C.c_int, C.c_size_t
    for name, arguments in {
        'nvrtcVersion': [C.POINTER(integer), C.POINTER(integer)],
        'nvrtcCreateProgram': [C.POINTER(pointer), C.c_char_p, C.c_char_p, integer, pointer, pointer],
        'nvrtcCompileProgram': [pointer, integer, C.POINTER(C.c_char_p)],
        'nvrtcGetProgramLogSize': [pointer, C.POINTER(size)], 'nvrtcGetProgramLog': [pointer, pointer],
        'nvrtcGetPTXSize': [pointer, C.POINTER(size)], 'nvrtcGetPTX': [pointer, pointer],
        'nvrtcDestroyProgram': [C.POINTER(pointer)],
    }.items():
        cuda._function(lib, name, arguments)
    cuda._function(lib, 'nvrtcGetErrorString', [integer], C.c_char_p)

    def check(code, operation):
        if code:
            raise RuntimeError(operation+': '+lib.nvrtcGetErrorString(code).decode())

    major, minor, program = integer(), integer(), pointer()
    check(lib.nvrtcVersion(C.byref(major), C.byref(minor)), 'nvrtcVersion')
    source_path = Path(source_path)
    source = source_path.read_bytes()
    check(lib.nvrtcCreateProgram(C.byref(program), source, source_path.name.encode(), 0, None, None),
          'nvrtcCreateProgram')
    try:
        options = (C.c_char_p*2)(b'--std=c++11', ('--gpu-architecture='+architecture).encode())
        status = lib.nvrtcCompileProgram(program, len(options), options)
        log_size = size()
        check(lib.nvrtcGetProgramLogSize(program, C.byref(log_size)), 'nvrtcGetProgramLogSize')
        log = C.create_string_buffer(log_size.value)
        check(lib.nvrtcGetProgramLog(program, log), 'nvrtcGetProgramLog')
        if status:
            raise RuntimeError('nvrtcCompileProgram: '+lib.nvrtcGetErrorString(status).decode()+'\n'+log.value.decode())
        ptx_size = size()
        check(lib.nvrtcGetPTXSize(program, C.byref(ptx_size)), 'nvrtcGetPTXSize')
        ptx = C.create_string_buffer(ptx_size.value)
        check(lib.nvrtcGetPTX(program, ptx), 'nvrtcGetPTX')
        return {'status': 'COMPILED', 'gpu_executed': False, 'architecture': architecture,
                'nvrtc_version': [major.value, minor.value], 'ptx': ptx.raw,
                'source_sha256': hashlib.sha256(source).hexdigest(),
                'ptx_sha256': hashlib.sha256(ptx.raw).hexdigest(), 'compiler_log': log.value.decode()}
    finally:
        cuda._finish_cleanup([('nvrtcDestroyProgram', lambda:
            check(lib.nvrtcDestroyProgram(C.byref(program)), 'nvrtcDestroyProgram'))], sys.exc_info()[1])


def _gpu_coefficients(plan):
    probe = cuda.probe_gpu()
    if probe['status'] != 'AVAILABLE':
        raise GPUUnavailable(probe)
    a = plan.arrays
    events, sweeps = len(a['source']), len(a['sweep_cell'])
    device_info = probe['devices'][0]
    if not (events or sweeps):
        return (), (), {'gpu_executed': False, 'launch_count': 0, 'status': 'NO_WORK', 'device': device_info}
    compiled = compile_kernel('compute_'+''.join(map(str, device_info['compute_capability'])))
    driver, device = cuda._driver(), C.c_int()
    cuda._check(driver, driver.cuDeviceGet(C.byref(device), 0), 'cuDeviceGet')
    context, module, function = C.c_void_p(), C.c_void_p(), C.c_void_p()
    allocated = []
    buffer_names = ('source', 'target', 'event_cut', 'available', 'kind',
                    'before', 'after', 'sweep_cell', 'sweep_cut')
    inputs = [(C.c_int32*max(1, len(a[name])))(*(a[name] or (0,))) for name in buffer_names]
    event_output, sweep_output = (C.c_int32*max(1, events*6))(), (C.c_int32*max(1, sweeps))()
    try:
        cuda._check(driver, driver.cuCtxCreate_v2(C.byref(context), 0, device), 'cuCtxCreate')
        ptx = C.create_string_buffer(compiled['ptx'])
        cuda._check(driver, driver.cuModuleLoadData(C.byref(module), ptx), 'cuModuleLoadData')
        cuda._check(driver, driver.cuModuleGetFunction(C.byref(function), module, b'boundary_incidence'),
                    'cuModuleGetFunction')
        for host in (*inputs, event_output, sweep_output):
            pointer = C.c_uint64()
            cuda._check(driver, driver.cuMemAlloc_v2(C.byref(pointer), C.sizeof(host)), 'cuMemAlloc')
            allocated.append(pointer)
        for host, pointer in zip(inputs, allocated):
            cuda._check(driver, driver.cuMemcpyHtoD_v2(pointer, host, C.sizeof(host)), 'cuMemcpyHtoD')
        parameters = [*allocated, C.c_uint(events), C.c_uint(sweeps), C.c_uint(len(plan.ledger.cells))]
        arguments = (C.c_void_p*len(parameters))(*[C.cast(C.byref(x), C.c_void_p) for x in parameters])
        cuda._check(driver, driver.cuLaunchKernel(function, (max(events, sweeps)+127)//128, 1, 1,
                    128, 1, 1, 0, None, arguments, None), 'cuLaunchKernel')
        cuda._check(driver, driver.cuCtxSynchronize(), 'cuCtxSynchronize')
        for host, pointer in zip((event_output, sweep_output), allocated[-2:]):
            cuda._check(driver, driver.cuMemcpyDtoH_v2(host, pointer, C.sizeof(host)), 'cuMemcpyDtoH')
        event_rows = tuple(tuple(event_output[row*6+column] for column in range(6)) for row in range(events))
        sweep_rows = tuple(sweep_output[:sweeps])
    finally:
        cuda._release_driver_resources(driver, allocated, module, context, sys.exc_info()[1])
    return event_rows, sweep_rows, {'gpu_executed': True, 'launch_count': 1, 'device': device_info,
        'driver_version': probe['driver_version'],
        'compilation': {key: value for key, value in compiled.items() if key != 'ptx'},
        'host_to_device_bytes': sum(C.sizeof(host) for host in inputs),
        'device_to_host_bytes': C.sizeof(event_output)+C.sizeof(sweep_output)}


def _assemble(plan, coefficients, sweep_coefficients):
    _require(type(coefficients) is tuple and len(coefficients) == len(plan.arrays['source']),
             'device occurrence row count differs')
    _require(type(sweep_coefficients) is tuple and len(sweep_coefficients) == len(plan.arrays['sweep_cell']),
             'device sweep row count differs')
    for row, available, kind in zip(coefficients, plan.arrays['available'], plan.arrays['kind']):
        _require(type(row) is tuple and len(row) == 6 and all(type(x) is int for x in row)
                 and row[0] in (-1, 0, 1) and all(x in (0, 1) for x in row[1:5])
                 and row[5] in (0, 1, 2, 3), 'invalid integer incidence output')
        orientation, selected, internal, exterior, inactive, bucket = row
        _require(selected == int(bool(orientation and available)) and inactive == 1-available
                 and bucket == (kind+1 if selected else 0)
                 and internal+exterior == int(orientation == 0),
                 'inconsistent incidence selection, availability or boundary class')
    _require(all(type(value) is int and value in (-1, 1) for value in sweep_coefficients),
             'sweep membership difference must be exact +1 or -1')
    accounting, factor = region.accounting, plan.factor
    archive, slices, rows = accounting.ArchiveState(), [], []
    for cut, frame, block in zip(plan.ledger.commits, plan.frames, plan.blocks):
        event_start, sweep_start, pending = block
        event_rows = coefficients[event_start:event_start+len(cut.channels)]
        pending_rows = coefficients[event_start+len(cut.channels):event_start+len(cut.channels)+len(pending)]
        unresolved = tuple(event.identity for event, coefficient in zip(pending, pending_rows) if coefficient[0])
        region.require(plan.scope != 'full' or not unresolved, 'full current has unresolved signed shedding projection')
        sweep = factor*sum((sample.value*coefficient for sample, coefficient in zip(
            frame.sweep, sweep_coefficients[sweep_start:sweep_start+len(frame.sweep)])), Fraction())
        additions, cross, shed, internal, exterior, inactive, views = [], [], [], [], [], [], []
        for event, coefficient in zip(cut.channels, event_rows):
            orientation, selected, is_internal, is_exterior, is_inactive, bucket = coefficient
            projected = orientation*event.value
            status = ('routed' if event.kind == 'shedding' else 'active') if selected else 'inactive'
            record = accounting.ContentRecord(event.identity, event.physical_id, event.provenance,
                cut.cut, plan.ledger.scale, plan.ledger.unit, projected, (event.physical_id,))
            additions.append((record, status))
            if selected:
                (cross if bucket == 1 else shed).append(event.identity)
            if is_internal:
                internal.append(event.identity)
            if is_exterior:
                exterior.append(event.identity)
            if is_inactive:
                inactive.append(event.identity)
            views.append({'identity': event.identity, 'physical_id': event.physical_id,
                'source': event.source, 'target': event.target, 'orientation': orientation,
                'input_value': event.value, 'source_projected_value': projected if event.available else Fraction(),
                'projected_value': factor*projected if event.available else Fraction(),
                'kind': event.kind, 'available': event.available})
        archive = accounting.commit_archive(archive, cut.cut, tuple(additions))
        coverage = accounting.BoundaryCoverage(cut.cut, plan.name, plan.target_scale, plan.ledger.unit,
            tuple(cross), tuple(shed), plan.ledger.scope, cut.window, plan.ledger.domain,
            region.fingerprint((cut.snapshot, frame, plan.scope, plan.representation)))
        motion = accounting.BoundaryMotion(frame.kind, frame.before.cells, frame.after.cells,
            sweep, plan.ledger.unit, region.fingerprint((cut.snapshot, frame.sweep, plan.representation)))
        maps = () if plan.representation is None else tuple(
            accounting.ScaleMap(identity, plan.target_scale, plan.ledger.unit, factor, plan.representation.provenance)
            for identity in cross+shed)
        row = accounting.account_slice(archive, cut.cut, plan.name, plan.target_scale, plan.ledger.unit,
            cross=tuple(cross), shedding=tuple(shed), motion=motion, coverage=coverage, scale_maps=maps)
        slices.append(row)
        rows.append({'cut': cut.cut, 'window': cut.window, 'snapshot': cut.snapshot,
            'cross': row.cross_total, 'sweep': row.sweep_total, 'shedding': row.shedding_total,
            'current': row.moving_total, 'internal_ids': tuple(internal), 'exterior_ids': tuple(exterior),
            'inactive_ids': tuple(inactive), 'unresolved_projection_ids': unresolved,
            'views': tuple(views), 'stock_balance_status': 'NOT_ASSERTED'})
    _require(_source_digest(plan.ledger, plan.name, plan.frames, plan.scope, plan.representation)
             == plan.source_digest, 'boundary source changed during backend execution')
    return region.BoundaryAudit(plan.name, plan.ledger, plan.frames, plan.scope, tuple(slices), tuple(rows),
                                accounting.accumulate_current(tuple(slices)), plan.representation)


def profiled_audit_boundary(ledger, name, frames, scope='declared-current', representation=None, *, backend='scalar'):
    """Return exact audit plus separate execution provenance; GPU never falls back."""
    started = time.perf_counter_ns()
    _require(type(backend) is str and backend in ('scalar', 'gpu'), 'explicit scalar or gpu backend required')
    bindings = _source_bindings()
    plan = _prepare(ledger, name, frames, scope, representation)
    prepared = time.perf_counter_ns()
    coefficients, sweep_coefficients, execution = (
        _scalar_coefficients(plan) if backend == 'scalar' else _gpu_coefficients(plan))
    classified = time.perf_counter_ns()
    audit = _assemble(plan, coefficients, sweep_coefficients)
    completed = time.perf_counter_ns()
    _require(_source_bindings() == bindings, 'backend source changed during execution')
    if execution.get('gpu_executed'):
        _require(execution['compilation']['source_sha256'] == bindings['simulations/kernels/boundary_incidence.cu'],
                 'compiled CUDA source differs from bound kernel')
    coefficient_digest = region.fingerprint((coefficients, sweep_coefficients))
    audit_digest = region.fingerprint(audit)
    finished = time.perf_counter_ns()
    profile = {'format': 'aod-dec.boundary-incidence-execution-v1', 'backend': backend,
        'status': 'EXECUTED' if execution.get('status') != 'NO_WORK' else 'NO_WORK',
        'input_sha256': plan.source_digest, 'coefficient_fields': FIELDS,
        'coefficient_sha256': coefficient_digest, 'audit_sha256': audit_digest,
        'source_sha256': bindings,
        'cells': len(ledger.cells), 'cuts': len(ledger.commits),
        'occurrence_view_rows': len(coefficients), 'sweep_rows': len(sweep_coefficients),
        'validation_and_packing_ns': prepared-started, 'classification_ns': classified-prepared,
        'host_fraction_assembly_ns': completed-classified,
        'provenance_and_result_hashes_ns': finished-completed, 'elapsed_ns': finished-started,
        'execution': execution,
        'timing_scope': 'Source bindings/admission/validation, packing, device probe, compilation, allocation, transfers, kernel, synchronization, cleanup, exact host audit assembly and result hashes; excludes prior witness construction, downstream canonical revalidation and final report serialization.',
        'interpretation': 'Integer incidence and swept-membership classification; host preserves exact rational values and source provenance. No native witness/route admission or physical model is inferred on the GPU.'}
    return {'audit': audit, 'profile': profile}


def audit_boundary(ledger, name, frames, scope='declared-current', representation=None, *, backend='scalar'):
    return profiled_audit_boundary(ledger, name, frames, scope, representation, backend=backend)['audit']
