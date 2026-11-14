"""TP-16 finite recurrence/reuse beyond a fixed point, with hostile bounds."""
from dataclasses import replace, FrozenInstanceError
from fractions import Fraction as Q
import unittest

from simulations.scoped_reduction import ExactReduction, UpdateDomain, ReductionError, advance
from simulations.recurrence_cache import CycleReuse, find_cycle


def reducer(P, *, bits=64, window=(0, 1000), domain=None, blocks=None):
    domain = domain or UpdateDomain("finite-R", window, "finite-partition", (0, 0), "coupled", (0,), ("pending",))
    blocks = blocks or tuple((i,) for i in range(len(P)))
    return ExactReduction(P, blocks, domain, max_bits=bits)


class RecurrenceCacheTests(unittest.TestCase):
    def test_period_two_executes_exact_cycle_and_reuses_later_steps(self):
        r = reducer(((0, 1), (1, 0)))
        result = find_cycle(r, (1, 0), max_steps=2)
        self.assertEqual((result.status, result.cache.transient, result.cache.period), ("verified_cycle", 0, 2))
        self.assertEqual((result.checked_steps, result.validation_steps), (2, 2))
        for step in (0, 1, 2, 33, 998, 999):
            self.assertEqual(result.cache.state_at(step, reducer=r, domain=r.domain),
                             (Q(1), Q(0)) if step % 2 == 0 else (Q(0), Q(1)))
        self.assertLessEqual(result.accounted_high_water_bytes, result.max_bytes)

    def test_transient_then_period_two_matches_fine_updates(self):
        P = ((0,1,0,0), (0,0,1,0), (0,0,0,1), (0,0,1,0))
        r = reducer(P)
        found = find_cycle(r, (1,0,0,0), max_steps=8)
        self.assertEqual((found.cache.transient, found.cache.period, found.retained_states), (2, 2, 4))
        state = (Q(1),Q(0),Q(0),Q(0))
        for tick in range(31):
            self.assertEqual(found.cache.state_at(tick, reducer=r, domain=r.domain), state)
            state = advance(state, P)

    def test_fixed_point_still_supported_without_changing_existing_api(self):
        r = reducer(((Q(1,2),Q(1,2)),)*2)
        found = find_cycle(r, (1,0), max_steps=3)
        self.assertEqual((found.cache.transient, found.cache.period), (1,1))
        self.assertEqual(found.cache.state_at(999, reducer=r, domain=r.domain), (Q(1,2),Q(1,2)))

    def test_stochastic_convergence_is_not_a_finite_cycle(self):
        r = reducer(((Q(1,2),Q(1,2)),(0,1)))
        found = find_cycle(r, (1,0), max_steps=12)
        self.assertEqual((found.status, found.checked_steps), ("not_established", 12))
        self.assertIsNone(found.cache)
        self.assertIn("No exact repeated", found.reason)

    def test_kernel_partition_and_every_scoped_dependency_invalidate(self):
        r = reducer(((0,1),(1,0)))
        cache = find_cycle(r, (1,0), max_steps=2).cache
        for change in ({"window":(0,2000)}, {"coupling_id":"off"}, {"phase_state":(1,)},
                       {"boundary_id":"other"}, {"pending_obligations":()}, {"octave_family":"other"}):
            with self.subTest(change=change), self.assertRaises(ReductionError):
                cache.state_at(3, reducer=r, domain=replace(r.domain, **change))
        changed = reducer(((1,0),(0,1)), domain=r.domain)
        with self.assertRaises(ReductionError): cache.state_at(3, reducer=changed, domain=r.domain)
        repartitioned = reducer(((0,1),(1,0)), domain=r.domain, blocks=((0,1),))
        with self.assertRaises(ReductionError): cache.state_at(3, reducer=repartitioned, domain=r.domain)

    def test_queries_and_search_respect_window_domain(self):
        r = reducer(((0,1),(1,0)), window=(10,20))
        cache = find_cycle(r, (1,0), max_steps=20).cache
        self.assertEqual(cache.state_at(9, reducer=r, domain=r.domain), (Q(0),Q(1)))
        for value in (-1, 10, True, 1.0):
            with self.assertRaises(ReductionError): cache.state_at(value, reducer=r, domain=r.domain)
        narrow = reducer(((0,1),(1,0)), window=(10,12))
        found = find_cycle(narrow, (1,0), max_steps=100)
        self.assertEqual((found.status, found.checked_steps), ("not_established",1))

    def test_work_byte_and_bigint_caps_have_explicit_status(self):
        r = reducer(((0,1),(1,0)))
        self.assertEqual(find_cycle(r,(1,0),max_steps=0).status, "not_established")
        tiny = find_cycle(r,(1,0),max_bytes=16)
        self.assertEqual((tiny.status,tiny.retained_states), ("incomplete",0))
        self.assertIsNone(tiny.cache)
        convergence = reducer(((Q(1,2),Q(1,2)),(0,1)),bits=8,window=(0,100))
        bounded = find_cycle(convergence,(1,0),max_steps=50)
        self.assertEqual(bounded.status, "incomplete")
        self.assertIn("bigint budget", bounded.reason)
        self.assertEqual(bounded.checked_steps, 7)
        huge = Q(1,2**100)
        self.assertEqual(find_cycle(convergence,(huge,1-huge)).status,"incomplete")

    def test_retained_state_cap_stops_before_index_growth(self):
        P = ((Q(1,2),Q(1,2)),(0,1)); r = reducer(P)
        baseline = find_cycle(r,(1,0),max_steps=2)
        cap = baseline.accounted_high_water_bytes
        bounded = find_cycle(r,(1,0),max_steps=100,max_bytes=cap)
        self.assertEqual(bounded.status,"incomplete")
        self.assertLessEqual(bounded.accounted_high_water_bytes,cap)
        self.assertEqual(bounded.retained_states,2)

    def test_forged_cycle_and_mutation_cannot_enable_reuse(self):
        r = reducer(((0,1),(1,0)))
        with self.assertRaises(ReductionError): CycleReuse(r,((Q(1),Q(0)),),0,1)
        with self.assertRaises(ReductionError): CycleReuse(r,((Q(1),Q(0)),(Q(0),Q(1))),0,2,max_bytes=16)
        cache = find_cycle(r,(1,0),max_steps=2).cache
        with self.assertRaises(FrozenInstanceError): cache.period = 1
        for alias in (True,1.0):
            with self.assertRaises(ReductionError): find_cycle(r,(1,0),max_steps=alias)


if __name__ == "__main__": unittest.main()
