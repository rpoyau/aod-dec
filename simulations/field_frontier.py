"""Compact admitted field boundaries for the existing native response law.

The archive retains formation evidence. Live summaries retain only response
operands and a scoped reference to that evidence. Response updates do not create
new closures, prove new recurrence, or archive their own event history. Callers
may retain returned summaries/attention records in TraceStore.
"""
from dataclasses import asdict, dataclass, fields, replace
from fractions import Fraction
import hashlib
import json
import math
import os
from pathlib import Path
import sqlite3
import tempfile

from .trace_history_union import RetainedHistoryUnion
from .trace_support import decode_wave
from .vendor import aod_manual_native as native


class FrontierError(ValueError):
    pass


class ExposureRequired(FrontierError):
    pass


def _require(condition, message):
    if not condition:
        raise FrontierError(message)


def _integer(value, label, *, minimum=0, max_bits=16384):
    _require(type(value) is int and value >= minimum and value.bit_length() <= max_bits,
             label + " requires a bounded exact integer")
    return value


def _text(value, label):
    _require(type(value) is str and bool(value) and len(value.encode()) <= 4096,
             label + " requires bounded nonempty text")
    return value


def _digest(value):
    _require(type(value) is str and len(value) == 64 and
             all(char in "0123456789abcdef" for char in value), "SHA-256 binding required")
    return value


def _exact(value, label, max_bits, *, positive=False):
    _require(type(value) in (int, Fraction), label + " requires exact int/Fraction")
    numerator, denominator = (value, 1) if type(value) is int else (value.numerator, value.denominator)
    _require(type(numerator) is int and type(denominator) is int and denominator > 0,
             label + " requires canonical rational components")
    _require(max(numerator.bit_length(), denominator.bit_length()) <= max_bits,
             label + " exceeds exact scalar budget")
    _require(math.gcd(numerator, denominator) == 1, label + " requires a reduced rational")
    value = Fraction(numerator, denominator)
    _require(not positive or value > 0, label + " must be positive")
    return value


def _window(value):
    _require(type(value) is tuple and len(value) == 2, "immutable half-open window required")
    for cut in value:
        _integer(cut, "window cut")
    _require(value[0] < value[1], "nonempty half-open window required")
    return value


_WITNESS_TYPES = (native.Trace, native.ReflectionWitness, native.Certificate,
                  native.Wave, native.MotifWitness)

# Exact schemas close inherited Python equality aliases such as False == 0.
# Only reflected_comparison is a Boolean; numeric fields/signs never are.
_WITNESS_SCHEMA = {
    native.Trace: {"scope": str, "scale": int, "lineage": str, "event_ids": (str,),
        "vertices": (int,), "indices": (int,), "roles": (int,), "turn": int,
        "ancestor": str, "reflection": native.ReflectionWitness, "reflected_comparison": bool},
    native.ReflectionWitness: {"scope": str, "lineage": str, "turnaround_id": str,
        "outbound_ids": (str,), "return_ids": (str,), "target_start_id": str,
        "committed_at": int, "turnaround_committed_at": int},
    native.Certificate: {"motif": str, "scope": str, "scale": int, "lineage": str,
        "trace_digest": str, "return_id": str, "witness_digest": str, "status": str,
        "certificate_digest": str, "inner_waves": (native.Wave,)},
    native.Wave: {"scope": str, "scale": int, "lineage": str, "motif": str,
        "identity": str, "returns": (native.Trace,), "certificates": (native.Certificate,),
        "motif_witnesses": (native.MotifWitness,)},
    native.MotifWitness: {"identity": str, "core": str, "scope": str, "lineage": str,
        "construction": tuple, "support_trace": native.Trace, "closure": native.Certificate,
        "component_certificates": (native.Certificate,), "component_traces": (native.Trace,),
        "enclosure": int, "chirality": int, "witness_digest": str},
}


def _immutable_witness(value, *, max_nodes=200000):
    """Bound typed input work before serialization; no object-identity trust."""
    nodes = 0

    def walk(item, depth=0, wave_depth=0, boolean=False):
        nonlocal nodes
        nodes += 1
        _require(nodes <= max_nodes and depth <= 128, "witness graph budget exceeded")
        kind = type(item)
        if kind is native.Wave:
            wave_depth += 1
            # The shared strict decoder has an explicit depth-16 parent cap.
            _require(wave_depth <= 7, "recoverable parent-depth budget exceeded")
        if kind in _WITNESS_TYPES:
            schema = _WITNESS_SCHEMA[kind]
            _require(set(schema) == {field.name for field in fields(item)}, "native witness schema changed")
            for field in fields(item):
                child, expected = getattr(item, field.name), schema[field.name]
                if type(expected) is tuple:
                    _require(type(child) is tuple and all(type(v) is expected[0] for v in child),
                             "exact native sequence required: " + field.name)
                else:
                    _require(type(child) is expected, "exact native field required: " + field.name)
                walk(child, depth + 1, wave_depth, expected is bool)
        elif kind is tuple:
            for child in item:
                walk(child, depth + 1, wave_depth)
        elif kind is int:
            _require(item.bit_length() <= 16384, "witness integer budget exceeded")
        elif kind is str:
            _require(len(item.encode()) <= 65536, "witness text budget exceeded")
        else:
            _require(boolean and kind is bool, "immutable exact native witness required")

    walk(value)


@dataclass(frozen=True)
class WitnessRef:
    sha256: str
    producer_sha256: str


class WitnessArchive:
    """Content-addressed durable native-wave evidence, revalidated on exposure."""
    def __init__(self, directory, *, max_witness_bytes=8 * 1024 * 1024):
        self.directory = Path(directory)
        self.directory.mkdir(parents=True, exist_ok=True)
        self.max_witness_bytes = _integer(max_witness_bytes, "witness byte cap", minimum=1)
        self.producer_sha256 = hashlib.sha256(Path(native.__file__).read_bytes()).hexdigest()
        self.metrics = {"archive_reads": 0, "archive_writes": 0, "written_bytes": 0}
        self._failed = False
        self._db = sqlite3.connect(self.directory / ".bindings.sqlite3", isolation_level=None)
        self._db.execute("PRAGMA cache_size=-64")
        self._db.execute("PRAGMA temp_store=FILE")
        self._db.execute("CREATE TABLE IF NOT EXISTS bindings (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
        # This index is derived acceleration, never admission authority. Reopen
        # revalidates durable evidence and reconstructs all union bindings.
        try:
            self._db.execute("BEGIN IMMEDIATE")
            self._db.execute("DELETE FROM bindings")
            for path in sorted(self.directory.glob("*.json")):
                ref = WitnessRef(path.stem, self.producer_sha256)
                self._merge_bindings(self.expose(ref))
            self._db.execute("COMMIT")
        except Exception:
            self._db.execute("ROLLBACK")
            self._db.close()
            raise

    def close(self):
        self._db.close()

    def _merge_bindings(self, wave):
        union = RetainedHistoryUnion(max_bindings=200000, max_nodes=500000)
        union.include(wave)
        for key, value in union.bindings.items():
            encoded_key = json.dumps(key, separators=(",", ":"))
            encoded_value = json.dumps(value, separators=(",", ":"))
            existing = self._db.execute("SELECT value FROM bindings WHERE key=?", (encoded_key,)).fetchone()
            _require(existing is None or existing[0] == encoded_value,
                     "conflicting retained history binding: " + key[0])
            if existing is None:
                self._db.execute("INSERT INTO bindings(key,value) VALUES (?,?)", (encoded_key, encoded_value))

    def _path(self, ref):
        _require(type(ref) is WitnessRef, "typed witness reference required")
        _digest(ref.sha256)
        _require(_digest(ref.producer_sha256) == self.producer_sha256,
                 "witness producer version changed; explicit migration required")
        return self.directory / (ref.sha256 + ".json")

    def _store_validated(self, wave):
        _require(not self._failed, "archive write failed; reopen and rebuild retained bindings")
        self._db.execute("BEGIN IMMEDIATE")
        write_started = False
        try:
            self._merge_bindings(wave)
            write_started = True
            result = self._write_validated(wave)
            self._db.execute("COMMIT")
            return result
        except Exception:
            # A rename/fsync/commit failure can leave a valid witness file even
            # after the derived index rolls back. Reopen must rebuild that union
            # before any further admission; otherwise conflicting histories
            # could be accepted by this still-open connection.
            self._failed = write_started
            self._db.execute("ROLLBACK")
            raise

    def _write_validated(self, wave):
        body = {"schema": "aod-native-wave-v1", "producer_sha256": self.producer_sha256,
                "wave": asdict(wave)}
        raw = json.dumps(body, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()
        _require(len(raw) <= self.max_witness_bytes, "witness byte cap exceeded")
        ref = WitnessRef(hashlib.sha256(raw).hexdigest(), self.producer_sha256)
        path = self._path(ref)
        if path.exists():
            self.metrics["archive_reads"] += 1
            with path.open("rb") as stream:
                existing = stream.read(self.max_witness_bytes + 1)
            _require(existing == raw, "conflicting content-addressed witness")
            return ref
        temporary = None
        try:
            with tempfile.NamedTemporaryFile(dir=self.directory, prefix=".witness-", delete=False) as stream:
                temporary = Path(stream.name)
                stream.write(raw)
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(temporary, path)
            temporary = None
            descriptor = os.open(self.directory, os.O_RDONLY)
            try:
                os.fsync(descriptor)
            finally:
                os.close(descriptor)
        finally:
            if temporary is not None:
                temporary.unlink(missing_ok=True)
        self.metrics["archive_writes"] += 1
        self.metrics["written_bytes"] += len(raw)
        return ref

    def expose(self, ref):
        """Recover the historical admitted Wave, not response-created returns."""
        path = self._path(ref)
        self.metrics["archive_reads"] += 1
        with path.open("rb") as stream:
            raw = stream.read(self.max_witness_bytes + 1)
        _require(len(raw) <= self.max_witness_bytes, "witness byte cap exceeded")
        _require(hashlib.sha256(raw).hexdigest() == ref.sha256, "archived witness hash mismatch")

        def unique(pairs):
            result = {}
            for key, value in pairs:
                _require(key not in result, "duplicate archived field")
                result[key] = value
            return result

        def no_float(value):
            raise FrontierError("inexact archived operand: " + value)

        try:
            payload = json.loads(raw, object_pairs_hook=unique,
                                 parse_float=no_float, parse_constant=no_float)
            _require(type(payload) is dict and set(payload) == {"schema", "producer_sha256", "wave"},
                     "complete archived wave envelope required")
            _require(payload["schema"] == "aod-native-wave-v1" and
                     payload["producer_sha256"] == ref.producer_sha256, "archive schema/producer mismatch")
            wave = decode_wave(payload["wave"])
            _immutable_witness(wave)
            return wave
        except (TypeError, KeyError, AttributeError, RecursionError) as exc:
            raise FrontierError("invalid archived witness: " + str(exc)) from exc


@dataclass(frozen=True)
class FieldSummary:
    identity: str
    motif: str
    scope: str
    scale: int
    window: tuple
    coupling_id: str
    committed: int
    pressure: Fraction
    current: Fraction
    closure_cycles: int
    reclosure_beats: int
    latest_return_cut: int
    latest_cadence: int
    provenance: WitnessRef
    field_weight: Fraction
    pending_obligations: tuple = ()
    generation: int = 0


@dataclass(frozen=True)
class BoundAttention:
    identity: str
    committed: int
    generation: int
    scope: str
    scale: int
    window: tuple
    coupling_id: str
    value: Fraction
    source_sha256: str


class FieldFrontier:
    """Own a finite family of admitted summaries and unchanged-domain updates.

    Registration snapshots are copied scalar values, not a caller-supplied
    certificate or hash. A forged/stale summary cannot acquire admission by
    merely matching its name. The archive's cold consistency index checks the
    union with all previously retained fields, including nested histories.
    """
    def __init__(self, archive, *, max_fields=100000, max_scalar_bits=16384):
        _require(type(archive) is WitnessArchive, "typed witness archive required")
        self.archive = archive
        self.max_fields = _integer(max_fields, "field cap", minimum=1)
        self.max_scalar_bits = _integer(max_scalar_bits, "scalar bit cap", minimum=1)
        self._active = {}
        self._inputs = {}

    @property
    def active_count(self):
        return len(self._active)

    def _snapshot(self, summary):
        _require(type(summary) is FieldSummary, "typed compact field summary required")
        for name in ("identity", "motif", "scope", "coupling_id"):
            _text(getattr(summary, name), name)
        for name in ("scale", "committed", "closure_cycles", "reclosure_beats",
                     "latest_return_cut", "latest_cadence", "generation"):
            _integer(getattr(summary, name), name)
        _window(summary.window)
        _require(summary.window[0] <= summary.committed < summary.window[1], "state outside declared window")
        _require(summary.closure_cycles >= 2 and summary.reclosure_beats == summary.closure_cycles-1
                 and summary.latest_return_cut <= summary.committed and summary.latest_cadence > 0,
                 "invalid historical closure summary")
        for name in ("pressure", "current", "field_weight"):
            _exact(getattr(summary, name), name, self.max_scalar_bits, positive=name != "current")
        _require(type(summary.pending_obligations) is tuple and
                 len(summary.pending_obligations) <= 1024 and
                 len(set(summary.pending_obligations)) == len(summary.pending_obligations),
                 "bounded distinct pending identities required")
        for item in summary.pending_obligations:
            _text(item, "pending identity")
        self.archive._path(summary.provenance)  # Validate reference, without I/O.
        # Nested objects are copied to scalars; retaining an alias would permit
        # object.__setattr__ to mutate the registered comparison itself.
        def scalar(field):
            value = getattr(summary, field.name)
            if field.name == "provenance":
                return (value.sha256, value.producer_sha256)
            if field.name in ("pressure", "current", "field_weight"):
                return (value.numerator, value.denominator)
            return value
        return tuple(scalar(field) for field in fields(summary))

    def _check(self, summary):
        snapshot = self._snapshot(summary)
        _require(self._active.get(summary.identity) == snapshot, "unregistered, stale or altered field summary")
        return snapshot

    def admit(self, wave, *, window, coupling_id, pressure=1, current=0,
              field_weight=1, pending_obligations=()):
        _require(type(wave) is native.Wave, "native recurring Wave required")
        _require(self.active_count < self.max_fields, "active field cap exceeded")
        _require(wave.identity not in self._active, "field identity already active")
        _immutable_witness(wave)
        native.validate_wave(wave)
        pressure = _exact(pressure, "pressure", self.max_scalar_bits, positive=True)
        current = _exact(current, "current", self.max_scalar_bits)
        weight = _exact(field_weight, "field weight", self.max_scalar_bits, positive=True)
        # Validate all compact values before creating a durable witness file.
        placeholder = WitnessRef("0"*64, self.archive.producer_sha256)
        summary = FieldSummary(wave.identity, wave.motif, wave.scope, wave.scale,
            _window(window), _text(coupling_id, "coupling binding"), wave.returns[-1].indices[-1],
            pressure, current, len(wave.returns), len(wave.returns)-1,
            wave.returns[-1].indices[-1], wave.cadence[-1], placeholder, weight,
            pending_obligations)
        self._snapshot(summary)
        summary = replace(summary, provenance=self.archive._store_validated(wave))
        self._active[summary.identity] = self._snapshot(summary)
        return summary

    def bind_attention(self, summary, value, *, source_sha256):
        self._check(summary)
        attention = BoundAttention(summary.identity, summary.committed, summary.generation,
            summary.scope, summary.scale, summary.window, summary.coupling_id,
            _exact(value, "attention", self.max_scalar_bits), _digest(source_sha256))
        self._inputs[summary.identity] = self._attention_snapshot(attention)
        return attention

    def _attention_snapshot(self, attention):
        _require(type(attention) is BoundAttention, "bound exact attention required")
        for name in ("identity", "scope", "coupling_id"):
            _text(getattr(attention, name), "attention " + name)
        for name in ("committed", "generation", "scale"):
            _integer(getattr(attention, name), "attention " + name)
        _window(attention.window)
        _digest(attention.source_sha256)
        value = _exact(attention.value, "attention", self.max_scalar_bits)
        return tuple((value.numerator, value.denominator) if field.name == "value"
                     else getattr(attention, field.name) for field in fields(attention))

    def advance(self, summary, attention, *, window, coupling_id, scale):
        self._check(summary)
        _window(window); _text(coupling_id, "coupling binding"); _integer(scale, "scale")
        if (window, coupling_id, scale) != (summary.window, summary.coupling_id, summary.scale):
            raise ExposureRequired("window/coupling/scale changed; expose and explicitly rebind the model")
        if summary.pending_obligations:
            raise ExposureRequired("pending or routing obligations require an explicit resolved response binding")
        _require(self._attention_snapshot(attention) == self._inputs.get(summary.identity),
                 "unregistered, stale or altered attention binding")
        _require((attention.identity, attention.committed, attention.generation,
                  attention.scope, attention.scale, attention.window, attention.coupling_id) ==
                 (summary.identity, summary.committed, summary.generation,
                  summary.scope, summary.scale, summary.window, summary.coupling_id),
                 "attention changed its committed field/domain binding")
        _window(attention.window); _digest(attention.source_sha256)
        value = _exact(attention.value, "attention", self.max_scalar_bits)
        if summary.committed + 1 >= summary.window[1]:
            raise ExposureRequired("next response leaves the admitted half-open window")
        # native.update only consumes committed, pressure and current, and uses
        # dataclasses.replace. This is its exact existing law on a compact state.
        updated = native.update(summary, value, input_index=summary.committed)
        updated = replace(updated, generation=summary.generation + 1)
        snapshot = self._snapshot(updated)
        self._active[summary.identity] = snapshot
        del self._inputs[summary.identity]
        return updated

    def expose(self, summary):
        self._check(summary)
        wave = self.archive.expose(summary.provenance)
        _require((wave.identity, wave.motif, wave.scope, wave.scale, len(wave.returns),
                  len(wave.returns)-1, wave.returns[-1].indices[-1], wave.cadence[-1]) ==
                 (summary.identity, summary.motif, summary.scope, summary.scale,
                  summary.closure_cycles, summary.reclosure_beats,
                  summary.latest_return_cut, summary.latest_cadence),
                 "exposed formation evidence differs from registered summary")
        return wave

    def rebind(self, summary, *, window, coupling_id):
        """Expose the same evidence and change response context without rewind.

        This explicit cold operation preserves the advanced response state and
        every historical closure claim. It resolves no pending obligations and
        creates no new scale, formation, pressure or physical coupling law.
        """
        self._check(summary)
        window = _window(window)
        coupling_id = _text(coupling_id, "coupling binding")
        _require(window[0] <= summary.committed < window[1],
                 "rebound window must contain the current committed cut")
        self.expose(summary)
        updated = replace(summary, window=window, coupling_id=coupling_id,
                          generation=summary.generation+1)
        snapshot = self._snapshot(updated)
        self._active[summary.identity] = snapshot
        self._inputs.pop(summary.identity, None)
        return updated

    def remove(self, summary):
        self._check(summary)
        del self._active[summary.identity]
        self._inputs.pop(summary.identity, None)


@dataclass(frozen=True)
class ScheduledEvent:
    identity: str
    cut: int


class IndexedFieldQueue:
    """At most one pending scheduling entry per registered active field.

    Indexed replacement/removal never retains stale heap entries. Heap work is
    O(log n) per schedule/pop/cancel; actual interaction edges are separate work.
    Scheduled cuts are ordering coordinates, not newly certified native events.
    """
    def __init__(self, *, max_fields=100000):
        self.max_fields = _integer(max_fields, "queue field cap", minimum=1)
        self._active, self._heap, self._positions = set(), [], {}
        self.metrics = {"comparisons": 0, "peak_pending": 0, "schedules": 0, "pops": 0}

    def __len__(self):
        return len(self._heap)

    @property
    def active_count(self):
        return len(self._active)

    def register(self, identity):
        _text(identity, "field identity")
        _require(identity not in self._active, "field already registered")
        _require(self.active_count < self.max_fields, "queue field cap exceeded")
        self._active.add(identity)

    def _less(self, left, right):
        self.metrics["comparisons"] += 1
        return self._heap[left] < self._heap[right]

    def _swap(self, left, right):
        self._heap[left], self._heap[right] = self._heap[right], self._heap[left]
        self._positions[self._heap[left][1]] = left
        self._positions[self._heap[right][1]] = right

    def _up(self, index):
        while index:
            parent = (index-1)//2
            if not self._less(index, parent):
                break
            self._swap(index, parent)
            index = parent

    def _down(self, index):
        while 2*index+1 < len(self._heap):
            child = 2*index+1
            if child+1 < len(self._heap) and self._less(child+1, child):
                child += 1
            if not self._less(child, index):
                break
            self._swap(index, child)
            index = child

    def schedule(self, identity, cut):
        _text(identity, "field identity"); _integer(cut, "scheduled cut")
        _require(identity in self._active, "cannot schedule an inactive field")
        entry = (cut, identity)
        if identity in self._positions:
            index = self._positions[identity]
            previous = self._heap[index]
            self._heap[index] = entry
            self.metrics["comparisons"] += 1
            if entry < previous:
                self._up(index)
            else:
                self._down(index)
        else:
            index = len(self._heap)
            self._heap.append(entry)
            self._positions[identity] = index
            self._up(index)
        self.metrics["schedules"] += 1
        self.metrics["peak_pending"] = max(self.metrics["peak_pending"], len(self._heap))

    def cancel(self, identity):
        _text(identity, "field identity")
        if identity not in self._positions:
            return False
        index = self._positions.pop(identity)
        last = self._heap.pop()
        if index < len(self._heap):
            self._heap[index] = last
            self._positions[last[1]] = index
            if index and self._less(index, (index-1)//2):
                self._up(index)
            else:
                self._down(index)
        return True

    def pop(self):
        if not self._heap:
            return None
        cut, identity = self._heap[0]
        self.cancel(identity)
        self.metrics["pops"] += 1
        return ScheduledEvent(identity, cut)

    def remove(self, identity):
        _text(identity, "field identity")
        self.cancel(identity)
        self._active.discard(identity)
