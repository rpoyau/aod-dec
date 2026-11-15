from dataclasses import replace
from fractions import Fraction as Q
import unittest

from simulations.reduced_boundary_response import (ResponseError, response_step,
    boundary_step, region, native, UNIT, WINDOW_DOMAIN)


def states():
    result = {}
    for cell, pressure, current in (('a', 2, 3), ('b', 4, 1), ('reservoir', 3, 2)):
        wave = native.recurrent(cell, n=2, spacing=5)
        result[cell] = replace(native.initial_state(wave, Q(pressure)), current=Q(current))
    return result


def channel(name='a-b', source='a', target='b', value=2, **options):
    return region.Channel(name, 'physical:' + name, source, target, Q(value), '1' * 64, **options)


def view(response, before, after):
    return boundary_step(response['before'], response['after'], response['channels'],
        region.Region('before', tuple(before)), region.Region('after', tuple(after)),
        scope='declared-response-region', window=3, window_domain=WINDOW_DOMAIN)


class ReducedBoundaryResponseTests(unittest.TestCase):
    def test_independent_three_cell_weighted_response(self):
        initial = states()
        response = response_step(initial, (channel(), channel('b-r', 'b', 'reservoir', 1)))
        self.assertEqual(response['response_inputs'], {'a': -2, 'b': 1, 'reservoir': 1})
        self.assertEqual({k: s.current for k, s in response['after'].items()},
                         {'a': Q(2), 'b': Q(5, 4), 'reservoir': Q(7, 3)})
        self.assertEqual(response['before_stock'], {'a': 6, 'b': 4, 'reservoir': 6})
        self.assertEqual(response['after_stock'], {'a': 4, 'b': 5, 'reservoir': 7})
        self.assertEqual(response['conservation_residual'], 0)
        self.assertTrue(all(s.committed == 8 for s in response['after'].values()))
        self.assertEqual(initial, states())

    def test_after_membership_and_pre_stock_are_necessary_for_moving_identity(self):
        response = response_step(states(), (channel(),))
        expanding = view(response, ('a',), ('a', 'b'))
        row = expanding['audit'].rows[0]
        self.assertEqual((row['cross'], row['sweep'], row['current']), (0, 4, -4))
        self.assertEqual(expanding['stock_change'], 4)
        self.assertEqual(expanding['balance_residual'], 0)
        # Using b's post-stock (6) or before-membership crossing (2) fails.
        self.assertNotEqual(expanding['stock_change'] - response['after_stock']['b'], 0)
        self.assertNotEqual(expanding['stock_change'] + 2 - row['sweep'], 0)
        shrinking = view(response, ('a', 'b'), ('a',))
        self.assertEqual(shrinking['stock_change'], -6)
        self.assertEqual((shrinking['audit'].rows[0]['cross'],
                          shrinking['audit'].rows[0]['sweep'],
                          shrinking['audit'].rows[0]['current']), (2, -4, 6))

    def test_internal_reverse_and_whole_universe_cancel(self):
        initial = states()
        forward = response_step(initial, (channel(),))
        reverse = response_step(initial, (channel('reverse', 'b', 'a', -2),))
        self.assertEqual(forward['after'], reverse['after'])
        all_cells = tuple(initial)
        whole = view(forward, all_cells, all_cells)
        self.assertEqual(whole['audit'].rows[0]['current'], 0)
        self.assertEqual(whole['stock_change'], 0)
        self.assertEqual(whole['audit'].rows[0]['internal_ids'], ('a-b',))

    def test_shedding_and_unavailable_channels_are_retained_without_double_response(self):
        channels = (channel('shed', 'a', 'reservoir', 1, kind='shedding'),
                    channel('inactive', 'b', 'a', 100, available=False))
        response = response_step(states(), channels)
        result = view(response, ('a',), ('a',))
        row = result['audit'].rows[0]
        self.assertEqual((row['cross'], row['shedding'], row['current']), (0, 1, 1))
        self.assertEqual(row['inactive_ids'], ('inactive',))
        self.assertEqual(result['stock_change'], -1)
        self.assertEqual(len(result['ledger'].commits[0].channels), 2)
        pending = region.PendingProjection('pending', 'physical-pending', 'a', 'reservoir',
                                          Q(1), 'native-burden', '1' * 64)
        with self.assertRaisesRegex(ResponseError, 'pending unsigned burden'):
            response_step(states(), (pending,))

    def test_negative_stock_is_rejected_without_clipping(self):
        with self.assertRaisesRegex(ResponseError, 'negative response stock'):
            response_step(states(), (channel(value=7),))
        bad = states()
        bad['a'] = replace(bad['a'], current=Q(-1))
        with self.assertRaisesRegex(ResponseError, 'negative response stock'):
            response_step(bad, ())

    def test_exact_shared_state_domain_and_complete_endpoint_family(self):
        for field, value in (('pressure', 0), ('pressure', -1), ('pressure', True),
                             ('pressure', 1.0), ('current', True), ('current', 1.0),
                             ('committed', True), ('committed', 8)):
            bad = states()
            bad['a'] = replace(bad['a'], **{field: value})
            with self.subTest(field=field, value=value), self.assertRaises(ResponseError):
                response_step(bad, ())
        with self.assertRaisesRegex(ResponseError, 'every channel endpoint'):
            response_step(states(), (channel(target='untracked'),))
        with self.assertRaisesRegex(ResponseError, 'duplicate'):
            response_step(states(), (channel(), channel()))

    def test_boundary_rejects_future_response_changed_pressure_and_cross_domain(self):
        response = response_step(states(), (channel(),))
        before, after = response['before'], response['after']
        for field, value in (('committed', 9), ('pressure', Q(9)), ('current', Q(99))):
            bad = dict(after)
            bad['a'] = replace(bad['a'], **{field: value})
            with self.subTest(field=field), self.assertRaises(ResponseError):
                boundary_step(before, bad, response['channels'], region.Region('old', ('a',)),
                              region.Region('new', ('a',)), scope='test', window=3,
                              window_domain=WINDOW_DOMAIN)
        for window, domain in ((True, WINDOW_DOMAIN), (3.0, WINDOW_DOMAIN), (3, 'seconds')):
            with self.subTest(window=window, domain=domain), self.assertRaises(ResponseError):
                boundary_step(before, after, response['channels'], region.Region('old', ('a',)),
                              region.Region('new', ('a',)), scope='test', window=window,
                              window_domain=domain)

    def test_multiple_passive_views_preserve_states_and_return_exact_operands(self):
        response = response_step(states(), (channel(),))
        before, after = dict(response['before']), dict(response['after'])
        first = view(response, ('a',), ('b',))
        second = view(response, ('b', 'reservoir'), ('a', 'reservoir'))
        self.assertEqual(first['audit'].rows[0]['current'] + second['audit'].rows[0]['current'], 0)
        self.assertEqual(response['before'], before)
        self.assertEqual(response['after'], after)
        self.assertEqual(first['unit'], UNIT)
        self.assertEqual({s.cell: s.value for s in first['frame'].sweep}, {'a': 6, 'b': 4})
        self.assertEqual({s.cell: s.value for s in first['ledger'].commits[0].sweep_field},
                         {'a': 6, 'b': 4, 'reservoir': 6})
        self.assertEqual(first['ledger'], second['ledger'])

    def test_boundary_derived_from_response_partitions_same_committed_history(self):
        response = response_step(states(), (channel(value=3),))
        # Fixed classifier applied to actual states; no prescribed motion schedule.
        selected = lambda state_map: tuple(k for k, s in state_map.items() if s.current >= Q(7, 4))
        before, after = selected(response['before']), selected(response['after'])
        self.assertEqual((before, after), (('a', 'reservoir'), ('b', 'reservoir')))
        moving = view(response, before, after)
        complement = view(response, ('b',), ('a',))
        universe = tuple(response['before'])
        whole = view(response, universe, universe)
        self.assertEqual(moving['ledger'], complement['ledger'])
        self.assertEqual(moving['ledger'], whole['ledger'])
        self.assertEqual(whole['frame'].sweep, ())
        result = region.audit_partition(whole['audit'], (moving['audit'], complement['audit']))
        self.assertEqual(result['total_current_residual'], 0)
        self.assertEqual(moving['stock_change'], 1)
        self.assertEqual(moving['audit'].rows[0]['current'], -1)


if __name__ == '__main__':
    unittest.main()
