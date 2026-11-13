"""Bounded retained-cost selection over previously validated native variants.

This is an explicit compiler control model. It supplies no AOD transition law.
"""
from __future__ import annotations
from dataclasses import dataclass
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import random
import shutil
import statistics
import subprocess
import tempfile
import time

POLICY = 'dec-cost-table-v1'
PROFILE_VERSION = 1
MAX_BYTES = 1_000_000
MAX_SAMPLES = 32
MIN_SAMPLES = 5
VARIANTS = ('generic', 'optimized')
THRESHOLD = 0.05


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True, allow_nan=False)


def digest(value):
    return hashlib.sha256(canonical(value).encode()).hexdigest()


def semantic_outcome(result):
    """Exclude timing/code metadata; retain every observable execution field."""
    return {key: result.get(key) for key in ('stdout', 'result', 'error', 'error_line', 'exit_code')}


def paired_interval(generic, optimized, samples=2000, seed=20261113):
    """Paired median fractional savings and percentile bootstrap interval."""
    pairs = [(a-b)/a for a, b in zip(generic, optimized) if a > 0 and b > 0]
    if len(pairs) < MIN_SAMPLES:
        return {'pairs': len(pairs), 'median_gain': None, 'lower': None, 'upper': None}
    rng = random.Random(seed)
    boot = sorted(statistics.median(rng.choices(pairs, k=len(pairs))) for _ in range(samples))
    return {'pairs': len(pairs), 'median_gain': statistics.median(pairs),
            'lower': boot[int(0.025 * samples)], 'upper': boot[min(samples-1, int(0.975*samples))]}


def host_inventory():
    def probe(command):
        try:
            proc = subprocess.run(command, capture_output=True, text=True, timeout=10)
            return {'command': command, 'returncode': proc.returncode, 'stdout': proc.stdout,
                    'stderr': proc.stderr, 'available': proc.returncode == 0}
        except (OSError, subprocess.TimeoutExpired) as exc:
            return {'command': command, 'available': False, 'error': str(exc)}
    cpuinfo = Path('/proc/cpuinfo').read_text() if Path('/proc/cpuinfo').exists() else ''
    rows = {}
    for line in cpuinfo.splitlines():
        if ':' in line:
            key, value = map(str.strip, line.split(':', 1))
            if key in ('model name', 'vendor_id', 'cpu family', 'model', 'stepping', 'flags', 'Features'):
                rows.setdefault(key, value)
    clock = time.get_clock_info('perf_counter')
    return {'recorded_utc': datetime.now(timezone.utc).isoformat(), 'platform': platform.platform(),
            'machine': platform.machine(), 'cpu_count': os.cpu_count(), 'cpu': rows,
            'loadavg': Path('/proc/loadavg').read_text().strip() if Path('/proc/loadavg').exists() else None,
            'memory': Path('/proc/meminfo').read_text() if Path('/proc/meminfo').exists() else None,
            'affinity': sorted(os.sched_getaffinity(0)) if hasattr(os, 'sched_getaffinity') else None,
            'topology': probe(['lscpu', '--json']), 'perf_counter': {'implementation': clock.implementation,
                'monotonic': clock.monotonic, 'adjustable': clock.adjustable, 'resolution_seconds': clock.resolution},
            'hardware_counters': probe(['perf', 'stat', '-e', 'cycles,instructions', '--', 'true']),
            'counter_interpretation': 'Unavailable counters remain unavailable, never numeric zero.'}


def host_binding(inventory):
    return digest({k: inventory[k] for k in ('platform', 'machine', 'cpu_count', 'cpu', 'affinity')})


@dataclass(frozen=True)
class Variant:
    name: str
    source_sha256: str
    artifact_sha256: str
    abi: str
    outcome_sha256: str


class VariantRegistry:
    """Only exact, prevalidated variant identities may be dispatched."""
    def __init__(self, source_sha256, abi, expected):
        self.source_sha256 = source_sha256
        self.abi = abi
        self.expected = semantic_outcome(expected)
        self.variants = {}

    def validate(self, name, artifact_sha256, actual):
        if name not in VARIANTS:
            raise ValueError('unknown variant')
        if semantic_outcome(actual) != self.expected:
            raise ValueError('native variant differs from reference outcome')
        self.variants[name] = Variant(name, self.source_sha256, artifact_sha256, self.abi, digest(self.expected))

    def permits(self, name):
        return name in self.variants

    def manifest(self):
        return {name: vars(value) for name, value in self.variants.items()}


class RetainedPolicy:
    """Read old evidence, dispatch, then observe: k can affect only k+1.

    Each instance exclusively owns its profile. Atomic rename prevents partial-file
    acceptance, but concurrent writers require distinct profile paths.
    """
    def __init__(self, path, binding, registry):
        self.path = Path(path)
        self.binding = dict(binding, policy=POLICY)
        self.registry = registry
        self.generation = 0
        self.costs = {name: [] for name in VARIANTS}
        self.rollback = False
        self.load_reason = 'missing'
        self._load()

    def _load(self):
        try:
            with self.path.open('rb') as stream:
                raw = stream.read(MAX_BYTES + 1)
            if len(raw) > MAX_BYTES:
                raise ValueError('profile exceeds bound')
            def pairs(values):
                out = {}
                for key, value in values:
                    if key in out:
                        raise ValueError('duplicate profile key')
                    out[key] = value
                return out
            obj = json.loads(raw, object_pairs_hook=pairs, parse_constant=lambda v: (_ for _ in ()).throw(ValueError(v)))
            payload = obj['payload']
            if digest(payload) != obj['sha256']:
                raise ValueError('profile checksum mismatch')
            if payload['version'] != PROFILE_VERSION or type(payload['version']) is not int:
                raise ValueError('profile version mismatch')
            if payload['binding'] != self.binding:
                raise ValueError('profile binding mismatch')
            if payload['registry'] != self.registry.manifest():
                raise ValueError('profile variant registry mismatch')
            if type(payload['generation']) is not int or not 0 <= payload['generation'] < 2**63:
                raise ValueError('invalid generation')
            costs = payload['costs']
            if set(costs) != set(VARIANTS):
                raise ValueError('invalid variant table')
            for values in costs.values():
                if type(values) is not list or len(values) > MAX_SAMPLES:
                    raise ValueError('invalid observation count')
                if any(type(v) is not int or not 0 < v < 2**63 for v in values):
                    raise ValueError('invalid observation')
            expected_count = min(payload['generation'], MAX_SAMPLES)
            if any(len(values) != expected_count for values in costs.values()):
                raise ValueError('profile paired counts do not match generation')
            if type(payload['rollback']) is not bool:
                raise ValueError('invalid rollback state')
            self.costs = costs
            self.generation = payload['generation']
            self.rollback = payload['rollback']
            self.load_reason = 'valid'
        except FileNotFoundError:
            self.load_reason = 'missing'
        except (OSError, KeyError, ValueError, TypeError, RecursionError, OverflowError) as exc:
            self.load_reason = 'rejected: ' + str(exc)

    def choose(self, mode='active'):
        if not self.registry.permits('generic'):
            raise ValueError('generic variant has not been validated')
        if mode not in ('generic', 'passive', 'frozen', 'active'):
            raise ValueError('unknown control mode')
        selected, reason = 'generic', 'generic control'
        if mode in ('frozen', 'active'):
            reason = 'insufficient evidence'
            if self.rollback:
                reason = 'held-out rollback'
            elif self.registry.permits('optimized') and min(map(len, self.costs.values())) >= MIN_SAMPLES:
                interval = paired_interval(self.costs['generic'], self.costs['optimized'])
                ratio = statistics.median(self.costs['optimized']) / statistics.median(self.costs['generic'])
                if ratio < 1-THRESHOLD and interval['lower'] > 0:
                    selected, reason = 'optimized', 'validated retained improvement'
                else:
                    reason = 'no established practical improvement'
        return {'variant': selected, 'reason': reason, 'generation': self.generation,
                'load_reason': self.load_reason, 'mode': mode}

    def observe_pair(self, generic_ns, optimized_ns):
        for name, value in zip(VARIANTS, (generic_ns, optimized_ns)):
            if not self.registry.permits(name):
                raise ValueError('cannot profile an unvalidated variant')
            if type(value) is not int or not 0 < value < 2**63:
                raise ValueError('cost must be positive bounded integer nanoseconds')
        for name, value in zip(VARIANTS, (generic_ns, optimized_ns)):
            self.costs[name] = (self.costs[name] + [value])[-MAX_SAMPLES:]
        self.generation += 1
        if self.generation >= 2**63:
            raise ValueError('profile generation exhausted')

    def evaluate_heldout(self, generic, optimized, overhead_ns=0):
        interval = paired_interval(generic, optimized)
        total_gain = sum(generic) - sum(optimized) - overhead_ns
        benefit = bool(interval['lower'] is not None and interval['lower'] > THRESHOLD and total_gain > 0)
        regression = bool(interval['median_gain'] is not None and interval['median_gain'] < -THRESHOLD)
        self.rollback = not benefit
        return dict(interval, measured_overhead_ns=overhead_ns, total_gain_ns=total_gain,
                    established_benefit=benefit, regression=regression,
                    action='retain optimized' if benefit else 'restore generic')

    def save(self):
        self.path.parent.mkdir(parents=True, exist_ok=True)
        payload = {'version': PROFILE_VERSION, 'binding': self.binding,
                   'registry': self.registry.manifest(), 'generation': self.generation,
                   'costs': self.costs, 'rollback': self.rollback}
        encoded = (canonical({'payload': payload, 'sha256': digest(payload)}) + '\n').encode()
        if len(encoded) > MAX_BYTES:
            raise ValueError('encoded profile exceeds bound')
        fd, temporary = tempfile.mkstemp(prefix='.'+self.path.name+'.', suffix='.tmp', dir=self.path.parent)
        try:
            with os.fdopen(fd, 'wb') as stream:
                stream.write(encoded)
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(temporary, self.path)
            directory = os.open(self.path.parent, os.O_RDONLY | getattr(os, 'O_DIRECTORY', 0))
            try:
                os.fsync(directory)
            finally:
                os.close(directory)
        finally:
            if os.path.exists(temporary):
                os.unlink(temporary)
