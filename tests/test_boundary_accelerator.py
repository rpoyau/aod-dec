"""Exact independent region-audit comparison, including actual CUDA when visible."""
import ctypes as C
from fractions import Fraction as Q
from pathlib import Path
import shutil
import subprocess
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from simulations import boundary_accelerator as accelerated
from simulations import region_engine as region


def channel(identity, source, target, value, **options):
    return region.Channel(identity, 'physical:'+identity, source, target, Q(value), '1'*64, **options)


def fixture():
    cells = ('a', 'b', 'c', 'reservoir')
    ledger = region.Ledger(cells, 'native-pressure-current', 0, 'fixture', 'native-return-count')
    channels = (
        channel('forward', 'a', 'b', Q(2, 7)), channel('reverse', 'b', 'a', Q(-3, 5)),
        channel('shed', 'b', 'c', Q(7, 11), kind='shedding'),
        channel('off', 'c', 'a', 99, available=False),
        channel('outer', 'c', 'reservoir', Q(-13, 17)), channel('self', 'a', 'a', Q(5, 2)),
    )
    first = tuple(region.SweepSample(cell, Q(value), ledger.unit, 0, '2'*64)
                  for cell, value in zip(cells, (5, 4, 3, 6)))
    ledger = region.commit(ledger, 7, 3, channels, sweep_field=first)
    before = region.Region('R', ('a',))
    middle = region.Region('R', ('a', 'b'))
    frame0 = region.Frame(before, middle, 'moving', (first[1],))
    second = tuple(region.SweepSample(cell, Q(value), ledger.unit, 0, '3'*64)
                   for cell, value in zip(cells, (7, 2, 1, 5)))
    ledger = region.commit(ledger, 8, 4,
        (channel('next-cross', 'a', 'c', Q(-5, 13)),
         channel('next-shed', 'b', 'reservoir', Q(2, 19), kind='shedding')), sweep_field=second)
    after = region.Region('R', ('b', 'c'))
    frame1 = region.Frame(middle, after, 'moving', (second[0], second[2]))
    return ledger, (frame0, frame1)


def pending_fixture():
    ledger = region.Ledger(('a', 'b'), 'u', 0, 'pending', 'native-return-count')
    obligation = region.PendingProjection('pending', 'physical:pending', 'a', 'b', Q(3), 'burden', '4'*64)
    ledger = region.commit(ledger, 0, 3, (), obligations=(obligation,))
    whole = region.Region('R', ('a', 'b'))
    ledger = region.commit(ledger, 1, 3, (), sweep_field=(region.SweepSample('b', Q(2), 'u', 0, '5'*64),))
    part = region.Region('R', ('a',))
    frames = (region.Frame(whole, whole, 'stationary'),
              region.Frame(whole, part, 'moving', ledger.commits[-1].sweep_field))
    return ledger, frames


class BoundaryAcceleratorTests(unittest.TestCase):
    def test_scalar_complete_records_equal_independent_original_and_exact_terms(self):
        ledger, frames = fixture()
        expected = region.audit_boundary(ledger, 'R', frames, scope='full')
        result = accelerated.profiled_audit_boundary(ledger, 'R', frames, scope='full')
        actual = result['audit']
        self.assertEqual(actual, expected)
        self.assertEqual(region.fingerprint(actual), region.fingerprint(expected))
        self.assertEqual(actual.rows[0]['cross'], 0)
        self.assertEqual(actual.rows[0]['shedding'], Q(7, 11))
        self.assertEqual(actual.rows[0]['sweep'], 4)
        self.assertEqual(actual.rows[0]['current'], Q(-37, 11))
        self.assertEqual(actual.rows[0]['internal_ids'], ('forward', 'reverse', 'self'))
        self.assertEqual(actual.rows[0]['exterior_ids'], ('outer',))
        self.assertEqual(actual.rows[0]['inactive_ids'], ('off',))
        self.assertEqual(actual.rows[1]['sweep'], -6)
        self.assertEqual(actual.rows[1]['current'], Q(5, 13)+Q(2, 19)+6)
        self.assertTrue(region.validate_audit(actual))
        self.assertEqual(result['profile']['occurrence_view_rows'], 8)
        self.assertEqual(result['profile']['sweep_rows'], 3)
        self.assertFalse(result['profile']['execution']['gpu_executed'])

    def test_representation_and_whole_partition_keep_all_source_occurrences(self):
        ledger, frames = fixture()
        representation = region.RepresentationMap(0, 3, ledger.unit, Q(5, 3), '6'*64)
        actual = accelerated.audit_boundary(ledger, 'R', frames, 'full', representation)
        self.assertEqual(actual, region.audit_boundary(ledger, 'R', frames, 'full', representation))
        self.assertEqual(actual.rows[0]['current'], Q(-185, 33))
        whole = region.Region('whole', ledger.cells)
        whole_frames = tuple(region.Frame(whole, whole, 'stationary') for _ in frames)
        complement = tuple(region.Frame(
            region.region_difference('complement', whole, frame.before),
            region.region_difference('complement', whole, frame.after), frame.kind, frame.sweep)
            for frame in frames)
        parent = accelerated.audit_boundary(ledger, 'whole', whole_frames, 'full')
        children = (accelerated.audit_boundary(ledger, 'R', frames, 'full'),
                    accelerated.audit_boundary(ledger, 'complement', complement, 'full'))
        self.assertEqual(region.audit_partition(parent, children)['total_current_residual'], 0)

    def test_pending_burden_survives_later_cut_and_never_becomes_signed_zero(self):
        ledger, frames = pending_fixture()
        actual = accelerated.audit_boundary(ledger, 'R', frames)
        self.assertEqual(actual, region.audit_boundary(ledger, 'R', frames))
        self.assertEqual(actual.rows[0]['unresolved_projection_ids'], ())
        self.assertEqual(actual.rows[1]['unresolved_projection_ids'], ('pending',))
        for auditor in (accelerated.audit_boundary, region.audit_boundary):
            with self.assertRaisesRegex(ValueError, 'unresolved signed shedding'):
                auditor(ledger, 'R', frames, scope='full')

    def test_invalid_and_oversized_source_is_rejected_before_device_execution(self):
        ledger, frames = fixture()
        with patch.object(accelerated, '_gpu_coefficients', side_effect=AssertionError('device called')):
            with patch.object(accelerated, 'MAX_ROWS', 1):
                with self.assertRaisesRegex(ValueError, 'buffer budget'):
                    accelerated.audit_boundary(ledger, 'R', frames, backend='gpu')
            with patch.object(accelerated, 'MAX_MEMBERSHIPS', 1):
                with self.assertRaisesRegex(ValueError, 'buffer budget'):
                    accelerated.audit_boundary(ledger, 'R', frames, backend='gpu')
            object.__setattr__(ledger.commits[0].channels[0], 'available', 1)
            with self.assertRaises(ValueError):
                accelerated.audit_boundary(ledger, 'R', frames, backend='gpu')

    def test_disconnected_frame_wrong_sweep_and_unknown_backend_are_rejected(self):
        ledger, frames = fixture()
        wrong = region.Frame(region.Region('other', ('a',)), region.Region('other', ('a',)), 'stationary')
        with self.assertRaisesRegex(ValueError, 'disconnected'):
            accelerated.audit_boundary(ledger, 'R', (frames[0], wrong))
        sample = region.SweepSample('b', Q(999), ledger.unit, 0, '7'*64)
        wrong = region.Frame(frames[0].before, frames[0].after, 'moving', (sample,))
        with self.assertRaisesRegex(ValueError, 'same-cut'):
            accelerated.audit_boundary(ledger, 'R', (wrong, frames[1]))
        for backend in ('auto', None, True):
            with self.subTest(backend=backend), self.assertRaises(ValueError):
                accelerated.audit_boundary(ledger, 'R', frames, backend=backend)

    def test_required_gpu_does_not_fall_back(self):
        ledger, frames = fixture()
        unavailable = {'status': 'UNAVAILABLE', 'reason': 'isolated device', 'gpu_executed': False}
        with patch.object(accelerated.cuda, 'probe_gpu', return_value=unavailable), \
             patch.object(accelerated, '_scalar_coefficients', side_effect=AssertionError('fallback')):
            with self.assertRaises(accelerated.GPUUnavailable) as raised:
                accelerated.audit_boundary(ledger, 'R', frames, backend='gpu')
        self.assertEqual(raised.exception.report, unavailable)

    def test_host_assembly_consumes_returned_coefficients_instead_of_rerunning_oracle(self):
        ledger, frames = fixture()
        plan = accelerated._prepare(ledger, 'R', frames, 'full', None)
        events, sweeps, _ = accelerated._scalar_coefficients(plan)
        changed = list(events)
        changed[2] = (-1, *changed[2][1:])  # Alter the signed shedding incidence.
        metadata = {'gpu_executed': True, 'compilation': {
            'source_sha256': accelerated._source_bindings()['simulations/kernels/boundary_incidence.cu']}}
        with patch.object(accelerated, '_gpu_coefficients', return_value=(tuple(changed), sweeps, metadata)), \
             patch.object(region, 'audit_boundary', side_effect=AssertionError('oracle called')):
            actual = accelerated.audit_boundary(ledger, 'R', frames, 'full', backend='gpu')
        self.assertEqual(actual.rows[0]['shedding'], Q(-7, 11))
        with self.assertRaises(ValueError):
            region.validate_audit(actual)

    def test_malformed_coefficients_and_source_mutation_fail(self):
        ledger, frames = fixture()
        plan = accelerated._prepare(ledger, 'R', frames, 'full', None)
        events, sweeps, _ = accelerated._scalar_coefficients(plan)
        with self.assertRaises(ValueError):
            accelerated._assemble(plan, ((True, *events[0][1:]), *events[1:]), sweeps)
        with self.assertRaises(ValueError):
            accelerated._assemble(plan, events, (0, *sweeps[1:]))
        for row in ((0, 1, 1, 0, 0, 1), (0, 0, 1, 1, 0, 0), (0, 0, 1, 0, 1, 0)):
            with self.subTest(row=row), self.assertRaisesRegex(ValueError, 'inconsistent'):
                accelerated._assemble(plan, (row, *events[1:]), sweeps)
        object.__setattr__(ledger.commits[-1], 'window', 99)
        with self.assertRaisesRegex(ValueError, 'source changed'):
            accelerated._assemble(plan, events, sweeps)

    def test_backend_source_change_is_rejected_before_reporting_execution(self):
        ledger, frames = fixture()
        original = accelerated._source_bindings()
        altered = dict(original)
        altered['simulations/kernels/boundary_incidence.cu'] = '0'*64
        with patch.object(accelerated, '_source_bindings', side_effect=(original, altered)):
            with self.assertRaisesRegex(ValueError, 'backend source changed'):
                accelerated.audit_boundary(ledger, 'R', frames)

    def test_empty_work_is_not_claimed_as_device_execution(self):
        ledger = region.commit(region.Ledger(('a',), 'u', 0, 'empty', 'native-return-count'), 0, 3, ())
        view = region.Region('R', ('a',))
        frames = (region.Frame(view, view, 'stationary'),)
        probe = {'status': 'AVAILABLE', 'devices': [{'index': 0, 'name': 'test probe', 'compute_capability': [8, 6]}]}
        with patch.object(accelerated.cuda, 'probe_gpu', return_value=probe):
            result = accelerated.profiled_audit_boundary(ledger, 'R', frames, backend='gpu')
        self.assertEqual(result['audit'], region.audit_boundary(ledger, 'R', frames))
        self.assertEqual(result['profile']['status'], 'NO_WORK')
        self.assertFalse(result['profile']['execution']['gpu_executed'])

    def test_host_compiled_cuda_body_matches_exact_coefficients_without_gpu_claim(self):
        compiler = shutil.which('g++')
        if compiler is None:
            self.skipTest('g++ unavailable for kernel-body conformance')
        source = ('#define __global__\nstruct Dim { unsigned int x; };\n'
                  'Dim blockIdx, blockDim, threadIdx;\n#include "'+accelerated.KERNEL.as_posix()+'"\n'
                  'extern "C" void launch(const int* a,const int* b,const int* c,const int* d,const int* e,'
                  'const int* f,const int* g,const int* h,const int* i,int* j,int* k,'
                  'unsigned int n,unsigned int s,unsigned int cells) {'
                  'blockDim.x=128;for(unsigned int t=0;t<(n>s?n:s);++t){'
                  'blockIdx.x=t/128;threadIdx.x=t%128;boundary_incidence(a,b,c,d,e,f,g,h,i,j,k,n,s,cells);}}')
        with TemporaryDirectory() as directory:
            path = Path(directory)
            (path/'kernel.cpp').write_text(source)
            subprocess.run([compiler, '-shared', '-fPIC', '-std=c++11', str(path/'kernel.cpp'), '-o', str(path/'kernel.so')],
                           check=True, capture_output=True, timeout=30)
            lib = C.CDLL(str(path/'kernel.so'))
            lib.launch.argtypes = [C.POINTER(C.c_int32)]*11+[C.c_uint]*3
            lib.launch.restype = None
            for ledger, frames in (fixture(), pending_fixture()):
                plan = accelerated._prepare(ledger, 'R', frames, 'declared-current', None)
                events, sweeps, _ = accelerated._scalar_coefficients(plan)
                names = ('source', 'target', 'event_cut', 'available', 'kind', 'before', 'after', 'sweep_cell', 'sweep_cut')
                buffers = [(C.c_int32*max(1, len(plan.arrays[name])))(*(plan.arrays[name] or (0,))) for name in names]
                event_out = (C.c_int32*max(1, len(events)*6))()
                sweep_out = (C.c_int32*max(1, len(sweeps)))()
                lib.launch(*buffers, event_out, sweep_out, len(events), len(sweeps), len(ledger.cells))
                self.assertEqual(tuple(event_out[:len(events)*6]), tuple(x for row in events for x in row))
                self.assertEqual(tuple(sweep_out[:len(sweeps)]), sweeps)

    def test_actual_nvrtc_compilation_is_not_gpu_execution(self):
        try:
            compiled = accelerated.compile_kernel('compute_75')
        except accelerated.GPUUnavailable as exc:
            self.skipTest(exc.report['reason'])
        self.assertEqual(compiled['status'], 'COMPILED')
        self.assertFalse(compiled['gpu_executed'])
        self.assertIn(b'boundary_incidence', compiled['ptx'])

    def test_actual_gpu_records_equal_original_for_signed_current_sweep_and_representation(self):
        probe = accelerated.cuda.probe_gpu()
        if probe['status'] != 'AVAILABLE':
            self.skipTest(probe['reason'])
        ledger, frames = fixture()
        representation = region.RepresentationMap(0, 2, ledger.unit, Q(17, 23), '8'*64)
        result = accelerated.profiled_audit_boundary(ledger, 'R', frames, 'full', representation, backend='gpu')
        self.assertTrue(result['profile']['execution']['gpu_executed'])
        self.assertEqual(result['audit'], region.audit_boundary(ledger, 'R', frames, 'full', representation))

    def test_actual_gpu_pending_classification_matches_original_without_resolving_burden(self):
        probe = accelerated.cuda.probe_gpu()
        if probe['status'] != 'AVAILABLE':
            self.skipTest(probe['reason'])
        ledger, frames = pending_fixture()
        result = accelerated.profiled_audit_boundary(ledger, 'R', frames, backend='gpu')
        self.assertTrue(result['profile']['execution']['gpu_executed'])
        self.assertEqual(result['audit'], region.audit_boundary(ledger, 'R', frames))
        with self.assertRaisesRegex(ValueError, 'unresolved signed shedding'):
            accelerated.audit_boundary(ledger, 'R', frames, 'full', backend='gpu')


if __name__ == '__main__':
    unittest.main()
