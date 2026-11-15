"""Boundary coefficients compiled from ordinary Dec source on CPU or CUDA.

The compiler computes all occurrence and sweep coefficients. Retained-history
admission, exact rational current assembly and canonical revalidation stay on
the host. No source function name selects a handwritten implementation.
"""
import hashlib
from pathlib import Path
import time

from . import boundary_accelerator as accounting
from . import region_engine as region

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / 'examples/boundary_coefficients.dec'


def source_bindings(backend):
    paths = ['examples/boundary_coefficients.dec', 'simulations/compiled_boundaries.py',
             'simulations/boundary_accelerator.py', 'simulations/region_engine.py',
             'simulations/boundary_accounting.py', 'native/batch.py',
             'runtime/dec_language.py', 'runtime/dec_core.py']
    if backend == 'compiled-cpu':
        paths += ['native/cpu_batch.py', 'native/batch_dispatch.cpp', 'native/compiler.py',
                  'native/runtime.cpp', 'native/runtime.hpp']
    elif backend == 'compiled-gpu':
        paths += ['native/gpu_compiler.py', 'simulations/chiral_gpu.py']
    else:
        raise ValueError('explicit compiled-cpu or compiled-gpu backend required')
    return {p: hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in paths}


def profiled_audit_boundary(ledger, name, frames, scope='declared-current', representation=None,
                           *, backend='compiled-cpu', workers=1):
    started = time.perf_counter_ns()
    bindings = source_bindings(backend)
    plan = accounting._prepare(ledger, name, frames, scope, representation)
    a, cells = plan.arrays, len(plan.ledger.cells)
    event_inputs = [(0, a['after'][cut*cells+source], a['after'][cut*cells+target], available, kind)
                    for source, target, cut, available, kind in zip(
                        a['source'], a['target'], a['event_cut'], a['available'], a['kind'])]
    sweep_inputs = [(1, a['after'][cut*cells+cell], a['before'][cut*cells+cell], 1, 0)
                    for cell, cut in zip(a['sweep_cell'], a['sweep_cut'])]
    prepared = time.perf_counter_ns()
    source = SOURCE.read_text()
    accounting._require(hashlib.sha256(source.encode()).hexdigest() == bindings['examples/boundary_coefficients.dec'],
                        'compiled source text differs from bound Dec file')
    if backend == 'compiled-cpu':
        from native.cpu_batch import run_cpu_batch
        result = run_cpu_batch(source, 'coefficient', event_inputs+sweep_inputs, workers=workers)
    else:
        accounting._require(workers == 1, 'compiled GPU uses one device-owning host')
        from native.gpu_compiler import run_gpu_batch
        result = run_gpu_batch(source, 'coefficient', event_inputs+sweep_inputs)
    compiler_binding = result['source_binding']
    entry = compiler_binding['entry'] if backend == 'compiled-cpu' else compiler_binding
    accounting._require(entry['source_sha256'] == bindings['examples/boundary_coefficients.dec'],
                        'returned compiler entry differs from bound Dec source')
    accounting._require(len(result['rows']) == len(event_inputs)+len(sweep_inputs),
                        'compiled batch row count differs')
    values = []
    for row in result['rows']:
        accounting._require(row['exit_code'] == 0 and row['error'] is None and not row['stdout'],
                            'compiled boundary coefficient failed: '+str(row.get('error')))
        text = row['result']
        accounting._require(type(text) is str and str(int(text)) == text,
                            'compiled coefficient is not a canonical exact integer')
        values.append(int(text))
    coefficients = []
    for value in values[:len(event_inputs)]:
        accounting._require(0 <= value < 192, 'compiled occurrence code outside declared domain')
        coefficients.append((value%3-1, (value//3)%2, (value//6)%2,
                             (value//12)%2, (value//24)%2, value//48))
    coefficients, sweeps = tuple(coefficients), tuple(values[len(event_inputs):])
    classified = time.perf_counter_ns()
    audit = accounting._assemble(plan, coefficients, sweeps)
    accounting._require(plan.source_digest == accounting._source_digest(ledger,name,frames,scope,representation),
                        'source ledger changed during compiled audit')
    accounting._require(bindings == source_bindings(backend), 'compiled audit source changed during execution')
    completed = time.perf_counter_ns()
    return {'audit':audit, 'profile':{'backend':backend, 'source_sha256':bindings,
        'input_sha256':plan.source_digest, 'audit_sha256':region.fingerprint(audit),
        'occurrence_view_rows':len(coefficients), 'sweep_rows':len(sweeps),
        'validation_and_packing_ns':prepared-started, 'classification_ns':classified-prepared,
        'host_fraction_assembly_and_binding_ns':completed-classified,
        'elapsed_ns':completed-started, 'execution':result['execution'],
        'compiler_binding':compiler_binding,
        'scope':'Dec-source compiled coefficients and exact host assembly; downstream replay adds canonical validation'}}


def audit_boundary(*args, **kwargs):
    return profiled_audit_boundary(*args, **kwargs)['audit']
