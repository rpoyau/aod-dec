import itertools
import unittest
from dataclasses import replace
from fractions import Fraction as Q
from simulations.scoped_reduction import (ExactReduction, FixedPointReuse, UpdateDomain, ReductionError,
    kernel, advance, aggregate, boundary, finite_scope)


class ScopedReductionTests(unittest.TestCase):
    def setUp(self):
        self.P = ((Q(1,4), Q(1,4), Q(1,2)), (Q(1,8), Q(3,8), Q(1,2)),
                  (Q(1,4), Q(1,4), Q(1,2)))
        self.blocks = ((0, 1), (2,))
        self.domain = UpdateDomain('R', (0, 8), 'declared', (0, 0), 'coupled', (0,), ('pending-x',))

    def test_TP15_all_basis_memories_seven_future_steps(self):
        r = ExactReduction(self.P, self.blocks, self.domain)
        for i in range(3):
            p = tuple(Q(int(j == i)) for j in range(3)); q = aggregate(p, self.blocks)
            for _ in range(7):
                p = advance(p, self.P)
                q = r.step(q, domain=self.domain, kernel_sha256=r.kernel_sha256)
                self.assertEqual(aggregate(p, self.blocks), q)

    def test_TP15_equal_present_boundary_different_future(self):
        P = ((0,0,1,0), (0,1,0,0), (0,0,0,1), (0,0,0,1))
        blocks = ((0,1,2), (3,))
        p = (1,0,0,0); other = (0,1,0,0)
        self.assertEqual(boundary(p, P, blocks[0])['boundary'], 0)
        self.assertEqual(boundary(other, P, blocks[0])['boundary'], 0)
        self.assertEqual(aggregate(p, blocks), aggregate(other, blocks))
        next_p, next_other = advance(p, P), advance(other, P)
        self.assertNotEqual(boundary(next_p, P, blocks[0])['boundary'], boundary(next_other, P, blocks[0])['boundary'])
        self.assertNotEqual(aggregate(advance(next_p, P), blocks), aggregate(advance(next_other, P), blocks))
        with self.assertRaises(ReductionError): ExactReduction(P, blocks, self.domain)

    def test_TP13_all_regions_partition_and_shell(self):
        p = (Q(1,2), Q(1,3), Q(1,6))
        currents = {}
        for mask in range(8):
            R = tuple(i for i in range(3) if mask & (1 << i))
            record = boundary(p, self.P, R)
            self.assertEqual(record['boundary'], record['divergence'])
            currents[R] = record['boundary']
        self.assertEqual(currents[()], 0); self.assertEqual(currents[(0,1,2)], 0)
        self.assertEqual(currents[(0,1)], currents[(0,)]+currents[(1,)])
        self.assertEqual(currents[(0,1)]-currents[(0,)], currents[(1,)])

    def test_TP16_invalidate_every_dependency(self):
        r = ExactReduction(self.P, self.blocks, self.domain)
        changed = dict(boundary_id='other', window=(0,9), octave_family='other',
                       octave_range=(0,1), coupling_id='off', phase_state=(1,), pending_obligations=())
        for key, value in changed.items():
            with self.subTest(key=key), self.assertRaises(ReductionError):
                r.step((Q(1),Q(0)), domain=replace(self.domain, **{key:value}), kernel_sha256=r.kernel_sha256)
        with self.assertRaises(ReductionError):
            r.step((Q(1),Q(0)), domain=self.domain, kernel_sha256='stale')

    def test_TP16_recurrence_is_computed(self):
        r = ExactReduction(self.P, self.blocks, self.domain)
        self.assertIsNone(r.recurrence((Q(1),Q(0)))['period'])
        self.assertEqual(r.recurrence((Q(1,2),Q(1,2)))['period'], 1)
        with self.assertRaises(ReductionError):FixedPointReuse(r,(Q(1),Q(0)))
        cache=FixedPointReuse(r,(Q(1,2),Q(1,2)))
        for _ in range(20):
            self.assertEqual(cache.advance(domain=self.domain,kernel_sha256=r.kernel_sha256)['state'],(Q(1,2),Q(1,2)))
        with self.assertRaises(ReductionError):cache.advance(domain=replace(self.domain,coupling_id='off'),kernel_sha256=r.kernel_sha256)

    def test_TP17_kernel_and_bigint_caps(self):
        with self.assertRaises(ReductionError): kernel(self.P, max_states=2)
        with self.assertRaises(ReductionError): kernel(((Q(1,2**100), 1-Q(1,2**100)),(Q(0),Q(1))), max_bits=16)
        for alias in (True, 1.0):
            with self.assertRaises(ReductionError): kernel(((alias,),))
        r=ExactReduction(self.P,self.blocks,self.domain,max_bits=16)
        huge=Q(1,2**10000)
        with self.assertRaises(ReductionError):r.step((huge,1-huge),domain=self.domain,kernel_sha256=r.kernel_sha256)
        with self.assertRaises(ReductionError):r.recurrence((huge,1-huge))

    def test_reducer_state_cannot_change_after_admission(self):
        r=ExactReduction(self.P,self.blocks,self.domain)
        with self.assertRaises(AttributeError):r.quotient=((Q(1),Q(0)),(Q(0),Q(1)))
        with self.assertRaises(ReductionError):replace(self.domain,octave_range=(-3,100))

    def test_TP28_interior_refinement_preserves_scope(self):
        results = [finite_scope(n) for n in (1, 2, 5, 16)]
        for r in results[1:]: self.assertEqual(r['trajectory'], results[0]['trajectory'])
        self.assertTrue(all(r['reduced_nodes'] == 2 for r in results))

    def test_bad_partition_and_kernel(self):
        for blocks in (((0,1),(1,2)), ((0,), (2,)), ((), (0,1,2))):
            with self.assertRaises(ReductionError): ExactReduction(self.P, blocks, self.domain)
        with self.assertRaises(ReductionError): kernel(((Q(1,2),),))
        with self.assertRaises(ReductionError): kernel(((Q(-1),Q(2)),(Q(0),Q(1))))


if __name__ == '__main__': unittest.main()
