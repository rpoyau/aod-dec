import unittest
from fractions import Fraction as Q
from simulations.trace_observation import (affine_interval, linear_variance,
    symbolic_norm, angle, observable, ERROR_CLASSES, ReductionError)


class TraceObservationTests(unittest.TestCase):
    def test_TP21_shared_input_cancels_without_independence(self):
        inputs = {'x': {'unit':'m','interval':(Q(1),Q(2))}}
        self.assertEqual(affine_interval([(1,'x'),(-1,'x')], inputs, 'm')['interval'], (0,0))
        self.assertEqual(linear_variance([1,-1], [[1,1],[1,1]], input_unit='m')['variance'], 0)

    def test_TP21_unknown_and_units(self):
        inputs = {'x': {'unit':'m','interval':None}}
        self.assertIsNone(affine_interval([(1,'x')], inputs, 'm')['interval'])
        with self.assertRaises(ReductionError): affine_interval([(1,'x')], inputs, 's')
        for sigma in ([[1,2],[2,1]], [[1,0],[1,1]], [[0,1],[1,1]]):
            with self.assertRaises(ReductionError): linear_variance([1,1], sigma, input_unit='m')

    def test_TP22_symbolic_norm_proved_interval_tau(self):
        result = symbolic_norm((1,1), decimal_places=20)
        lo, hi = result['interval']
        self.assertLessEqual(lo*lo, 2); self.assertGreaterEqual(hi*hi, 2)
        self.assertIsNone(result['exact_rational'])
        self.assertEqual(symbolic_norm((3,4))['exact_rational'], 5)
        self.assertEqual(angle(Q(1,2))['symbol'], 'tau')
        with self.assertRaises(ReductionError): symbolic_norm((1.0,1))
        with self.assertRaises(ReductionError): angle(True)

    def sample(self):
        return dict(unit='closure_count', preparation='input-1', kernel='model-1',
            boundary='R',window=(0,8),scale={'family':'core','index':0},
            evidence=('trace://run/chunk/event',),observation_map='count',anchor={'kind':'native'},
            errors={name:{'status':'unknown','unit':'closure_count','reason':'no bound supplied'} for name in ERROR_CLASSES})

    def test_TP21_complete_error_accounting_and_no_host_anchor(self):
        args = self.sample()
        self.assertEqual(observable(2, **args)['value'], 2)
        args['anchor'] = {'kind':'host_timer','value':1}
        with self.assertRaises(ReductionError): observable(2, **args)
        args = self.sample(); args['errors']['model_discrepancy']['bound'] = 0
        with self.assertRaises(ReductionError): observable(2, **args)
        args = self.sample(); del args['errors']['sampling']
        with self.assertRaises(ReductionError): observable(2, **args)


if __name__ == '__main__': unittest.main()
