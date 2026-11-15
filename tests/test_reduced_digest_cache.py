"""Exact optional cache equivalence and retained validator checks.

Vendored byte-for-byte from AOD Manual commit
ab8359209b8f9b2ef4007403101016e3ea58cfe4,
manual/scripts/aod_trace_digest_cache.py, SHA-256
0d36febb309f6366d12da54955010e94124581739e0da70660e19192a60eac14.
The existing simulations/vendor/LICENSE preserves the source MIT notice.
"""

from dataclasses import replace
from fractions import Fraction as Q
import hashlib
from pathlib import Path
import unittest

from simulations import region_engine as region
from simulations.sparc_inputs import BinKey, PreparationRow
from simulations.sparc_reduced import simulate_galaxy
from simulations.vendor import aod_manual_native as native
from simulations.vendor import aod_trace_digest_cache as cache_module


SOURCE_SHA256 = "0d36febb309f6366d12da54955010e94124581739e0da70660e19192a60eac14"


def rows():
    return tuple(PreparationRow(BinKey("CacheFixture", Q(radius)), Q(gas), Q(disk),
                                Q(0), Q(2), Q(0), Q(4))
                 for radius, gas, disk in ((1, 2, 5), (2, -1, 8), (4, 3, 2), (7, 1, 3)))


class ReducedDigestCacheTests(unittest.TestCase):
    def test_vendor_bytes_match_bound_source(self):
        self.assertEqual(hashlib.sha256(Path(cache_module.__file__).read_bytes()).hexdigest(),
                         SOURCE_SHA256)

    def test_small_simulation_outputs_returns_and_history_hash_are_identical(self):
        descriptor = native.Trace.trace_digest
        validators = (native.validate_trace, native.validate_wave, native.admitted)
        plain = simulate_galaxy(rows())
        plain_fingerprint = region.fingerprint(plain)
        with cache_module.trace_digest_cache(native) as stats:
            self.assertIsNot(native.Trace.trace_digest, descriptor)
            self.assertEqual((native.validate_trace, native.validate_wave, native.admitted), validators)
            cached = simulate_galaxy(rows())
            self.assertGreater(stats["hits"], 0)
            self.assertGreater(stats["misses"], 0)
        self.assertIs(native.Trace.trace_digest, descriptor)
        self.assertEqual(cached, plain)
        self.assertEqual(region.fingerprint(cached), plain_fingerprint)
        self.assertEqual(cached["history_sha256"], plain["history_sha256"])
        self.assertEqual(region.fingerprint(cached["native_history"]),
                         region.fingerprint(plain["native_history"]))
        self.assertEqual([[t.return_id for t in w.returns] for w in cached["native_history"].waves],
                         [[t.return_id for t in w.returns] for w in plain["native_history"].waves])

    def test_descriptor_restored_after_body_exception(self):
        descriptor = native.Trace.trace_digest
        trace = native.trace("restore")
        expected = trace.trace_digest
        with self.assertRaisesRegex(RuntimeError, "test exit"):
            with cache_module.trace_digest_cache(native) as stats:
                self.assertEqual(trace.trace_digest, expected)
                self.assertEqual(trace.trace_digest, expected)
                self.assertGreater(stats["hits"], 0)
                raise RuntimeError("test exit")
        self.assertIs(native.Trace.trace_digest, descriptor)
        self.assertEqual(trace.trace_digest, expected)

    def test_same_forged_history_is_rejected_after_priming_valid_cache(self):
        wave = native.recurrent("forgery")
        forged = replace(wave, returns=wave.returns[:-1] +
                         (replace(wave.returns[-1], vertices=(0, 2, 0)),))
        with self.assertRaises(native.AdmissionError) as uncached:
            native.validate_wave(forged)
        descriptor = native.Trace.trace_digest
        with cache_module.trace_digest_cache(native) as stats:
            native.validate_wave(wave)
            with self.assertRaises(native.AdmissionError) as cached:
                native.validate_wave(forged)
            self.assertEqual(str(cached.exception), str(uncached.exception))
            self.assertGreater(stats["hits"], 0)
            malformed = replace(wave.returns[0], indices=(True, 1, 2))
            malformed.trace_digest  # unsupported typed keys fall back, never alias int1
            self.assertGreater(stats["fallbacks"], 0)
            with self.assertRaises(native.AdmissionError):
                native.validate_trace(malformed)
        self.assertIs(native.Trace.trace_digest, descriptor)

    def test_actual_entry_and_byte_caps_evict_without_changing_digests(self):
        traces = tuple(native.trace("bounded", occurrence=k) for k in range(20))
        expected = tuple(t.trace_digest for t in traces)
        with cache_module.trace_digest_cache(native, max_entries=2, max_key_bytes=512,
                                             max_entry_bytes=512) as stats:
            for t, digest in zip(traces, expected):
                self.assertEqual(t.trace_digest, digest)
                self.assertEqual(t.trace_digest, digest)
            self.assertGreater(stats["evictions"], 0)
            self.assertGreater(stats["hits"], 0)
            self.assertGreater(stats["peak_entries"], 0)
            self.assertLessEqual(stats["peak_entries"], 2)
            self.assertLessEqual(stats["peak_key_bytes"], 512)
        with cache_module.trace_digest_cache(native, max_entry_bytes=1) as stats:
            self.assertEqual(tuple(t.trace_digest for t in traces), expected)
            self.assertEqual(stats["peak_entries"], 0)
            self.assertEqual(stats["peak_key_bytes"], 0)
            self.assertEqual(stats["fallbacks"], len(traces))

    def test_invalid_caps_never_replace_descriptor(self):
        descriptor = native.Trace.trace_digest
        for option in ("max_entries", "max_key_bytes", "max_entry_bytes"):
            for value in (0, -1, True, 1.0):
                with self.subTest(option=option, value=value), self.assertRaises(ValueError):
                    with cache_module.trace_digest_cache(native, **{option: value}):
                        self.fail("invalid cache entered")
                self.assertIs(native.Trace.trace_digest, descriptor)


if __name__ == "__main__":
    unittest.main()
