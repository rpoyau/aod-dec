"""Portable strict admission regressions; every stable case is discoverable.

Copy this file and admission_feedback_cases.py into repository tests/.
Historical unsafe-acceptance modes are intentionally absent.
"""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from simulations.vendor import aod_manual_native as producer
from admission_feedback_cases import cases


class TestAdmissionRegressions(unittest.TestCase):
    """Six valid controls and thirteen mandatory AdmissionError rejections."""


def _method(case_id, kind, action):
    def test(self):
        if kind == 'positive':
            action()
        else:
            with self.assertRaises(producer.AdmissionError):
                action()
    test.__doc__ = case_id
    test.feedback_case_id = case_id
    return test


_cases = cases(producer)
if len(_cases) != 19 or len({case_id for case_id, _, _ in _cases}) != 19:
    raise RuntimeError('strict admission inventory must contain 19 unique cases')
for _id, _kind, _action in _cases:
    _name = 'test_' + _id.replace('-', '_').lower()
    if hasattr(TestAdmissionRegressions, _name):
        raise RuntimeError('duplicate test method')
    setattr(TestAdmissionRegressions, _name, _method(_id, _kind, _action))

if __name__ == '__main__':
    unittest.main()
