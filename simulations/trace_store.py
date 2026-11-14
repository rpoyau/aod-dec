"""Versioned exact commit traces with bounded records and durable retrieval.

Each SQLite row is a checksummed chunk. The disk index is never mirrored in a
Python identity set. SQLite transactions commit the chunk, identity and chain
head together. The page cap reserves an equal allowance for rollback journaling;
cache, event, checkpoint and source-key sizes are separately bounded. These are
store budgets, not a bound on the caller's model or detector memory.
"""
from contextlib import contextmanager
from dataclasses import asdict, dataclass, fields, is_dataclass
from fractions import Fraction
from hashlib import sha256
import json
from pathlib import Path
import sqlite3
import time


class TraceError(ValueError):
    pass


class TraceIntegrityError(TraceError):
    pass


class TraceIncomplete(TraceError):
    """An execution/evidence budget or storage failure prevents completeness."""


def require(condition, message):
    if not condition:
        raise TraceIntegrityError(message)


@dataclass(frozen=True)
class TraceLimits:
    max_event_bytes: int = 262144
    max_storage_bytes: int = 64 * 1024 * 1024
    cache_bytes: int = 1024 * 1024
    max_integer_bits: int = 8192
    max_depth: int = 64
    max_nodes: int = 32768

    def __post_init__(self):
        for name, value in asdict(self).items():
            require(type(value) is int and value > 0, name + " must be a positive exact integer")
        require(self.max_storage_bytes >= 65536, "storage budget too small for durable index")
        require(self.max_event_bytes <= self.max_storage_bytes // 4, "event budget exceeds storage reserve")
        require(self.cache_bytes >= 4096, "SQLite cache requires at least one page")


def exact_json(value, limits=None):
    """Copy a bounded exact value to JSON form before serializing it.

    A preflight traversal bounds bigint/string/container size before JSON's
    allocation. Ratios use a normalized numerator/positive denominator pair.
    Booleans remain a separate payload type; envelope indices forbid them.
    """
    limits = limits or TraceLimits()
    nodes, estimated = 0, 0

    def visit(v, depth):
        nonlocal nodes, estimated
        nodes += 1
        if nodes > limits.max_nodes or depth > limits.max_depth:
            raise TraceIncomplete("exact value exceeds node/depth budget")
        if type(v) is int:
            if v.bit_length() > limits.max_integer_bits:
                raise TraceIncomplete("integer exceeds exact-integer byte budget")
            estimated += max(1, (v.bit_length() * 30103) // 100000 + 2)
            out = v
        elif type(v) is str:
            estimated += len(v) * 6 + 2
            out = v
        elif v is None or type(v) is bool:
            estimated += 5
            out = v
        elif type(v) is Fraction:
            out = {"$ratio": [visit(v.numerator, depth + 1), visit(v.denominator, depth + 1)]}
            estimated += 14
        elif type(v) in (tuple, list):
            estimated += len(v) + 2
            if len(v) > limits.max_nodes:
                raise TraceIncomplete("sequence exceeds node budget")
            out = [visit(x, depth + 1) for x in v]
        elif type(v) is dict:
            estimated += len(v) * 2 + 2
            if len(v) > limits.max_nodes:
                raise TraceIncomplete("mapping exceeds node budget")
            out = {}
            for key, item in v.items():
                require(type(key) is str, "JSON keys must be strings")
                visit(key, depth + 1)
                out[key] = visit(item, depth + 1)
            if "$ratio" in out:
                pair = out["$ratio"]
                require(len(out) == 1 and type(pair) is list and len(pair) == 2 and
                        all(type(x) is int for x in pair), "malformed exact ratio")
                require(pair[1] > 0 and Fraction(*pair).numerator == pair[0] and
                        Fraction(*pair).denominator == pair[1], "ratio must be normalized")
        elif is_dataclass(v) and not isinstance(v, type):
            # Enforce budgets before copying a long retained parent history.
            out = {}
            for field in fields(v):
                visit(field.name, depth + 1)
                out[field.name] = visit(getattr(v, field.name), depth + 1)
        else:
            raise TraceIntegrityError("unsupported or inexact scientific payload type: " + type(v).__name__)
        if estimated > limits.max_event_bytes * 6:
            raise TraceIncomplete("exact value exceeds serialization budget")
        return out

    return visit(value, 0)


def canonical_bytes(value, limits=None):
    limits = limits or TraceLimits()
    try:
        result = json.dumps(exact_json(value, limits), sort_keys=True, separators=(",", ":"),
                            ensure_ascii=True, allow_nan=False).encode("ascii")
    except (ValueError, OverflowError) as exc:
        if isinstance(exc, TraceError):
            raise
        raise TraceIncomplete("exact value exceeds JSON encoder integer/resource limits") from exc
    if len(result) > limits.max_event_bytes:
        raise TraceIncomplete("encoded value exceeds event/checkpoint byte budget")
    return result


def digest(value, limits=None):
    return sha256(canonical_bytes(value, limits)).hexdigest()


def chunk_chain(previous, body_hash, source_key, limits):
    # The source-key index is part of the integrity binding, not an unsealed
    # route to otherwise valid but unrelated witness bytes.
    return sha256((previous + body_hash + digest(source_key, limits)).encode()).hexdigest()


FIELDS = frozenset(("schema_version", "run_id", "event_id", "commit_ordinal", "causal_parents",
                    "scope_id", "lineage_id", "fractal_coordinate", "scale_family", "scale_index",
                    "kind", "payload", "source_ref"))


def validate_event(event, limits=None):
    event = exact_json(event, limits)
    require(type(event) is dict and set(event) == FIELDS, "trace envelope fields mismatch")
    for name in ("run_id", "scope_id", "lineage_id", "scale_family", "kind", "source_ref", "event_id"):
        require(type(event[name]) is str and bool(event[name].strip()), name + " must be nonempty")
    require(type(event["schema_version"]) is int and event["schema_version"] == 1, "unsupported trace schema")
    require(type(event["commit_ordinal"]) is int and 0 <= event["commit_ordinal"] < 2**63,
            "ordinal must be exact nonnegative int in the declared storage domain")
    require(type(event["scale_index"]) is int, "scale index must be exact int")
    require(type(event["fractal_coordinate"]) in (tuple, list) and
            all(type(x) is int for x in event["fractal_coordinate"]), "coordinate requires exact integer components")
    require(type(event["causal_parents"]) in (tuple, list), "parent references must be a sequence")
    parents = event["causal_parents"]
    require(all(type(x) is str and len(x) == 64 for x in parents) and len(parents) == len(set(parents)),
            "causal parents must be distinct content IDs")
    require(type(event["payload"]) is dict, "event payload must be a mapping")
    body = {k: v for k, v in event.items() if k != "event_id"}
    require(event["event_id"] == digest(body, limits), "content-bound event identity mismatch")
    return json.loads(canonical_bytes(event, limits))


def make_event(run_id, commit_ordinal, kind, payload, *, scope_id, lineage_id,
               source_ref, causal_parents=(), fractal_coordinate=(),
               scale_family="core-q4", scale_index=0, limits=None):
    event = exact_json(dict(schema_version=1, run_id=run_id, commit_ordinal=commit_ordinal,
                           kind=kind, payload=payload, scope_id=scope_id, lineage_id=lineage_id,
                           source_ref=source_ref, causal_parents=causal_parents,
                           fractal_coordinate=fractal_coordinate, scale_family=scale_family,
                           scale_index=scale_index), limits)
    event["event_id"] = digest(event, limits)
    return validate_event(event, limits)


def run_manifest(run_id, *, source, kernel, preparation, backend="reference",
                 model="declared-finite", rng=None, scope=None):
    return dict(format="aod-dec-trace", version=1, semantic_version=1,
                detector_version=1, run_id=run_id, source=source, kernel=kernel,
                preparation=preparation, backend=backend, model=model,
                rng=rng if rng is not None else {"algorithm": "none", "streams": []},
                scope=scope if scope is not None else {})


class TraceStore:
    """Append-only chunk store; use finish() to attest a complete producer run.

    Opening an existing archive read-only does not change its state. Resuming a
    writer requires resume=True and explicitly changes coverage to recording.
    Each append is durable before it returns. No writer thread/queue is hidden.
    """
    def __init__(self, path, manifest=None, *, limits=None, resume=False, read_only=False):
        self.path = Path(path).resolve()
        self.limits = limits or TraceLimits()
        self.read_only = read_only
        self.failure = None
        self.metrics = {"append_count": 0, "io_ns": 0, "replay_ns": 0,
                        "event_bytes_high_water": 0, "checkpoint_bytes_high_water": 0,
                        "archive_bytes_high_water": 0, "queue_high_water": 0,
                        "resident_identity_entries": 0, "cache_budget_bytes": self.limits.cache_bytes}
        self.db = None
        database = self.path / "trace.sqlite3"
        existed = database.exists()
        require(not read_only or existed, "trace archive is unavailable")
        require(not existed or resume or read_only, "archive exists; select resume or read-only")
        if not read_only:
            self.path.mkdir(parents=True, exist_ok=True)
        try:
            self.db = sqlite3.connect((database.as_uri() + "?mode=ro") if read_only else str(database),
                                      uri=read_only)
            self.db.execute("PRAGMA temp_store=FILE")
            self.db.execute("PRAGMA mmap_size=0")
            self.db.execute("PRAGMA cache_size=-" + str(max(1, self.limits.cache_bytes // 1024)))
            self.db.execute("PRAGMA cache_spill=ON")
            if not read_only:
                self.db.execute("PRAGMA journal_mode=DELETE")
                self.db.execute("PRAGMA synchronous=FULL")
                self.db.execute("PRAGMA page_size=4096")
                pages = max(8, (self.limits.max_storage_bytes - 16384) // (2 * 4096))
                self.db.execute("PRAGMA max_page_count=" + str(pages))
            if not existed:
                require(type(manifest) is dict, "new store needs a run manifest")
                for key in ("run_id", "source", "model", "kernel", "preparation", "backend", "rng"):
                    require(key in manifest, "manifest missing " + key)
                require(type(manifest["run_id"]) is str and manifest["run_id"], "run ID required")
                encoded = canonical_bytes(manifest, self.limits)
                with self.db:
                    self.db.execute("CREATE TABLE metadata(key TEXT PRIMARY KEY, value BLOB NOT NULL)")
                    self.db.execute("CREATE TABLE chunks(ordinal INTEGER PRIMARY KEY, event_id TEXT UNIQUE NOT NULL, source_key TEXT UNIQUE, body BLOB NOT NULL, body_hash TEXT NOT NULL, chain TEXT NOT NULL)")
                    for key, value in (("manifest", encoded.decode()), ("manifest_hash", sha256(encoded).hexdigest()),
                                       ("limits", canonical_bytes(asdict(self.limits)).decode()),
                                       ("count", "0"), ("chain", "0" * 64), ("state", "recording")):
                        self.db.execute("INSERT INTO metadata VALUES (?,?)", (key, value))
            self.manifest = json.loads(self._meta("manifest"))
            require(digest(self.manifest, self.limits) == self._meta("manifest_hash"), "manifest checksum mismatch")
            self.run_id = self.manifest["run_id"]
            if manifest is not None:
                require(canonical_bytes(manifest, self.limits) == canonical_bytes(self.manifest, self.limits),
                        "run manifest changed on restart")
            stored_limits = json.loads(self._meta("limits"))
            if read_only and limits is None:
                self.limits = TraceLimits(**stored_limits)
                self.db.execute("PRAGMA cache_size=-" + str(max(1, self.limits.cache_bytes // 1024)))
                self.metrics["cache_budget_bytes"] = self.limits.cache_bytes
            if existed and not read_only:
                require(stored_limits == asdict(self.limits), "resource limits changed on restart")
            self.verify()
            if existed and not read_only:
                self._set_state("recording")
            self._disk_measure()
        except TraceError:
            if self.db is not None:
                self.db.close()
                self.db = None
            raise
        except (sqlite3.Error, OSError, json.JSONDecodeError) as exc:
            if self.db is not None:
                self.db.close()
                self.db = None
            raise TraceIncomplete("trace storage unavailable: " + str(exc)) from exc

    @classmethod
    def open(cls, path, *, limits=None):
        return cls(path, limits=limits, read_only=True)

    def _meta(self, key):
        row = self.db.execute("SELECT substr(value,1,?),length(value) FROM metadata WHERE key=?",
                              (self.limits.max_event_bytes + 1, key)).fetchone()
        require(row is not None, "trace metadata missing: " + key)
        require(row[1] <= self.limits.max_event_bytes, "metadata exceeds read budget")
        return row[0]

    def _set_state(self, state):
        with self.db:
            self.db.execute("UPDATE metadata SET value=? WHERE key='state'", (state,))

    def _disk_measure(self):
        total = sum(p.stat().st_size for p in self.path.iterdir() if p.is_file())
        self.metrics["archive_bytes_high_water"] = max(self.metrics["archive_bytes_high_water"], total)
        if total > self.limits.max_storage_bytes:
            raise TraceIncomplete("archive byte budget exceeded")
        return total

    def _failure(self, exc):
        self.failure = str(exc)
        if not self.read_only:
            try:
                self._set_state("incomplete")
            except (sqlite3.Error, OSError):
                pass  # recording was durable before the attempted commit

    def get(self, event_id):
        row = self.db.execute("SELECT substr(body,1,?),substr(body_hash,1,65),length(body),ordinal,substr(source_key,1,?),length(source_key),substr(chain,1,65) FROM chunks WHERE event_id=?",
                              (self.limits.max_event_bytes + 1, self.limits.max_event_bytes + 1, event_id)).fetchone()
        if row is None:
            raise TraceIncomplete("requested event witness is unavailable")
        require(row[2] <= self.limits.max_event_bytes, "retrieved chunk exceeds read budget")
        require(row[5] is None or row[5] <= self.limits.max_event_bytes, "source key exceeds read budget")
        require(sha256(row[0]).hexdigest() == row[1], "retrieved chunk checksum mismatch")
        event = validate_event(json.loads(row[0]), self.limits)
        require(event["event_id"] == event_id and event["commit_ordinal"] == row[3] and
                event["run_id"] == self.run_id, "retrieved chunk/index binding mismatch")
        previous = "0" * 64 if row[3] == 0 else self.db.execute(
            "SELECT substr(chain,1,65) FROM chunks WHERE ordinal=?", (row[3] - 1,)).fetchone()
        if row[3] != 0:
            require(previous is not None, "retrieved witness prefix unavailable")
            previous = previous[0]
        require(chunk_chain(previous, row[1], row[4], self.limits) == row[6], "retrieved source-key chain mismatch")
        for parent in event["causal_parents"]:
            found = self.db.execute("SELECT ordinal FROM chunks WHERE event_id=?", (parent,)).fetchone()
            require(found is not None and found[0] < row[3], "retrieved causal witness unavailable")
        return event

    def get_by_key(self, source_key):
        row = self.db.execute("SELECT substr(event_id,1,65) FROM chunks WHERE source_key=?", (source_key,)).fetchone()
        return None if row is None else self.get(row[0])

    def append(self, kind, payload, *, scope_id, lineage_id, source_ref,
               causal_parents=(), fractal_coordinate=(), scale_family="core-q4",
               scale_index=0, source_key=None):
        require(not self.read_only, "cannot append to read-only archive")
        if self.failure is not None:
            raise TraceIncomplete("trace already incomplete: " + self.failure)
        try:
            if source_key is not None:
                require(type(source_key) is str and bool(source_key), "source key must be nonempty")
                canonical_bytes(source_key, self.limits)
                old = self.get_by_key(source_key)
                if old is not None:
                    candidate = make_event(self.run_id, old["commit_ordinal"], kind, payload,
                                           scope_id=scope_id, lineage_id=lineage_id, source_ref=source_ref,
                                           causal_parents=causal_parents, fractal_coordinate=fractal_coordinate,
                                           scale_family=scale_family, scale_index=scale_index, limits=self.limits)
                    require(candidate == old, "conflicting immutable source event")
                    return old
            event = make_event(self.run_id, int(self._meta("count")), kind, payload,
                               scope_id=scope_id, lineage_id=lineage_id, source_ref=source_ref,
                               causal_parents=causal_parents, fractal_coordinate=fractal_coordinate,
                               scale_family=scale_family, scale_index=scale_index, limits=self.limits)
            return self.ingest(event, source_key=source_key)
        except (sqlite3.Error, OSError, TraceError) as exc:
            self._failure(exc)
            if isinstance(exc, TraceError):
                raise
            raise TraceIncomplete("trace append failed: " + str(exc)) from exc

    def ingest(self, event, *, source_key=None):
        require(not self.read_only, "cannot ingest into read-only archive")
        if self.failure is not None:
            raise TraceIncomplete("trace already incomplete: " + self.failure)
        if source_key is not None:
            require(type(source_key) is str and bool(source_key), "source key must be nonempty")
            canonical_bytes(source_key, self.limits)
        event = validate_event(event, self.limits)
        require(event["run_id"] == self.run_id, "event belongs to a different run")
        encoded = canonical_bytes(event, self.limits)
        start = time.perf_counter_ns()
        try:
            old = self.db.execute("SELECT substr(body,1,?),substr(source_key,1,?) FROM chunks WHERE event_id=?",
                                  (self.limits.max_event_bytes + 1, self.limits.max_event_bytes + 1, event["event_id"])).fetchone()
            if old is not None:
                require(old[0] == encoded, "conflicting duplicate event")
                require(source_key is None or source_key == old[1], "conflicting duplicate source key")
                return event
            require(event["commit_ordinal"] == int(self._meta("count")), "out-of-order or missing commitment")
            for parent in event["causal_parents"]:
                found = self.db.execute("SELECT ordinal FROM chunks WHERE event_id=?", (parent,)).fetchone()
                require(found is not None and found[0] < event["commit_ordinal"], "unavailable or noncausal parent")
            body_hash = sha256(encoded).hexdigest()
            chain = chunk_chain(self._meta("chain"), body_hash, source_key, self.limits)
            with self.db:
                self.db.execute("INSERT INTO chunks VALUES (?,?,?,?,?,?)", (event["commit_ordinal"],
                                event["event_id"], source_key, encoded, body_hash, chain))
                self.db.execute("UPDATE metadata SET value=? WHERE key='count'", (str(event["commit_ordinal"] + 1),))
                self.db.execute("UPDATE metadata SET value=? WHERE key='chain'", (chain,))
                self.db.execute("UPDATE metadata SET value='recording' WHERE key='state'")
            self.metrics["append_count"] += 1
            self.metrics["event_bytes_high_water"] = max(self.metrics["event_bytes_high_water"], len(encoded))
            self._disk_measure()
            return event
        except (sqlite3.Error, OSError, TraceError) as exc:
            self._failure(exc)
            if isinstance(exc, TraceError):
                raise
            raise TraceIncomplete("trace chunk transaction failed: " + str(exc)) from exc
        finally:
            self.metrics["io_ns"] += time.perf_counter_ns() - start

    def iter_events(self):
        start = time.perf_counter_ns()
        ordinal, chain = 0, "0" * 64
        try:
            cursor = self.db.execute("SELECT ordinal,substr(event_id,1,65),substr(body,1,?),substr(body_hash,1,65),substr(chain,1,65),length(body),substr(source_key,1,?),length(source_key) FROM chunks ORDER BY ordinal",
                                     (self.limits.max_event_bytes + 1, self.limits.max_event_bytes + 1))
            for index, event_id, body, checksum, row_chain, length, source_key, key_length in cursor:
                require(index == ordinal, "missing/truncated chunk sequence")
                require(length <= self.limits.max_event_bytes, "chunk exceeds read budget")
                require(key_length is None or key_length <= self.limits.max_event_bytes, "source key exceeds read budget")
                require(sha256(body).hexdigest() == checksum, "chunk checksum mismatch")
                event = validate_event(json.loads(body), self.limits)
                require(event["commit_ordinal"] == index and event["event_id"] == event_id and
                        event["run_id"] == self.run_id, "chunk/index binding mismatch")
                chain = chunk_chain(chain, checksum, source_key, self.limits)
                require(chain == row_chain, "chunk chain mismatch")
                for parent in event["causal_parents"]:
                    found = self.db.execute("SELECT ordinal FROM chunks WHERE event_id=?", (parent,)).fetchone()
                    require(found is not None and found[0] < index, "broken causal parent reference")
                ordinal += 1
                yield event
            require(ordinal == int(self._meta("count")) and chain == self._meta("chain"),
                    "archive tail missing or altered")
        except sqlite3.Error as exc:
            raise TraceIntegrityError("unreadable trace archive: " + str(exc)) from exc
        finally:
            self.metrics["replay_ns"] += time.perf_counter_ns() - start

    def verify(self):
        count = sum(1 for _ in self.iter_events())
        return {"events": count, "chain": self._meta("chain"),
                "coverage": "complete" if self._meta("state") == "complete" else "partial"}

    def checkpoint(self, state, rng_state=None):
        require(not self.read_only, "cannot checkpoint read-only archive")
        try:
            data = canonical_bytes({"next_ordinal": int(self._meta("count")), "chain": self._meta("chain"),
                                    "state": state, "rng_state": rng_state}, self.limits)
            with self.db:
                for key, value in (("checkpoint", data.decode()), ("checkpoint_hash", sha256(data).hexdigest())):
                    self.db.execute("INSERT OR REPLACE INTO metadata VALUES (?,?)", (key, value))
            self.metrics["checkpoint_bytes_high_water"] = max(self.metrics["checkpoint_bytes_high_water"], len(data))
            self._disk_measure()
        except (sqlite3.Error, OSError, TraceError) as exc:
            self._failure(exc)
            if isinstance(exc, TraceError):
                raise
            raise TraceIncomplete("checkpoint storage failed: " + str(exc)) from exc

    def load_checkpoint(self):
        data = self._meta("checkpoint")
        require(sha256(data.encode()).hexdigest() == self._meta("checkpoint_hash"), "checkpoint checksum mismatch")
        value = json.loads(data)
        require(value["next_ordinal"] <= int(self._meta("count")), "checkpoint exceeds available history")
        if value["next_ordinal"]:
            row = self.db.execute("SELECT chain FROM chunks WHERE ordinal=?", (value["next_ordinal"] - 1,)).fetchone()
            require(row is not None and row[0] == value["chain"], "checkpoint history mismatch")
        else:
            require(value["chain"] == "0" * 64, "empty checkpoint chain mismatch")
        return value

    def finish(self):
        if not self.read_only and self.failure is None:
            self.verify()
            self._set_state("complete")

    def status(self):
        return {"coverage": "complete" if self._meta("state") == "complete" and self.failure is None else "partial",
                "state": self._meta("state"), "failure": self.failure, "events": int(self._meta("count")),
                "chain": self._meta("chain"), "archive_bytes": self._disk_measure(),
                "limits": asdict(self.limits), "metrics": dict(self.metrics),
                "scope": "store allocation caps; caller model, detector and OS RSS are measured separately"}

    def close(self):
        if self.db is not None:
            self.db.close()
            self.db = None

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        try:
            if exc is None:
                self.finish()
            elif not self.read_only:
                self._failure(exc)
        finally:
            self.close()

    def core_recorder(self, kind, value):
        """Observer for runtime.dec_core.observe_commits; never draws an RNG."""
        if kind == "core.contact":
            coupling = value
            left_return, right_return = coupling.contacts[-1]
            parent_ids, certificates = [], []
            for trace, ordinal in ((coupling.left, left_return), (coupling.right, right_return)):
                source_key = json.dumps(["core", trace.scope, trace.lineage, ordinal], separators=(",", ":"))
                parent = self.get_by_key(source_key)
                if parent is None:
                    raise TraceIncomplete("contact requires recoverable raw return witnesses")
                require(parent["kind"] == "core.transition" and parent["payload"]["ordinal"] == ordinal
                        and parent["payload"]["target"] == trace.start.value,
                        "contact parent is not its declared raw return")
                parent_ids.append(parent["event_id"])
                certificates.append(next(c.digest for c in trace.closures if c.end == ordinal))
            left, right = coupling.left, coupling.right
            payload = {"left_lineage": left.lineage, "right_lineage": right.lineage,
                       "window": coupling.window, "left_return": left_return, "right_return": right_return,
                       "left_closure_digest": certificates[0], "right_closure_digest": certificates[1],
                       "event_span": len(left.events)}
            return self.append(kind, payload, scope_id=left.scope,
                               # This groups a directed relation; it does not
                               # claim a newly formed wave lineage.
                               lineage_id="coupling:" + json.dumps([left.lineage, right.lineage], separators=(",", ":")),
                               source_ref="runtime/dec_core.py:contact",
                               causal_parents=tuple(parent_ids),
                               source_key=json.dumps(["core-contact", left.scope, left.lineage, right.lineage,
                                                      coupling.window, left_return, right_return], separators=(",", ":")))
        if kind == "region.commit":
            ledger, row = value
            payload = {"cells": ledger.cells, "unit": ledger.unit, "scale": ledger.scale,
                       "domain": ledger.domain, "row": row}
            return self.append(kind, payload, scope_id=ledger.scope, lineage_id="region-current",
                               source_ref="simulations/region_engine.py:commit",
                               scale_family="declared-current", scale_index=ledger.scale,
                               source_key=json.dumps(["region", ledger.scope, row.cut]))
        t = value
        prefix = ["core", t.scope, t.lineage]
        key = lambda ordinal: json.dumps(prefix + [ordinal], separators=(",", ":"))
        source = "runtime/dec_core.py:finite-hinge-return-v1"
        if kind == "core.start":
            payload = {"start": t.start.value, "proposal": t.proposal,
                       "parents": t.parents}
            parent_ids = []
            for parent in t.parents:
                parent_key = json.dumps(["core", parent.scope, parent.lineage, len(parent.events)], separators=(",", ":"))
                previous = self.get_by_key(parent_key)
                if previous is not None:
                    parent_ids.append(previous["event_id"])
            return self.append(kind, payload, scope_id=t.scope, lineage_id=t.lineage,
                               source_ref=source, causal_parents=tuple(parent_ids), source_key=key(0))
        if kind == "core.transition":
            event = t.events[-1]
            previous = self.get_by_key(key(event.ordinal - 1))
            if previous is None:
                raise TraceIncomplete("recording began after required retained start/previous event")
            payload = dict(asdict(event), start=t.start.value, proposal=t.proposal)
            return self.append(kind, payload, scope_id=t.scope, lineage_id=t.lineage,
                               source_ref=source, causal_parents=(previous["event_id"],), source_key=key(event.ordinal))
        if kind in ("core.closure", "core.fusion"):
            previous = self.get_by_key(key(len(t.events)))
            if previous is None:
                raise TraceIncomplete("closure/fusion witness transitions unavailable")
            payload = asdict(t.closures[-1]) if kind == "core.closure" else {"proposal": t.proposal, "parents": t.parents}
            return self.append(kind, payload, scope_id=t.scope, lineage_id=t.lineage,
                               source_ref=source, causal_parents=(previous["event_id"],),
                               source_key=json.dumps(prefix + [kind, len(t.events)]))
        raise TraceIntegrityError("unsupported core commit hook: " + kind)


@contextmanager
def record_commits(store):
    """Connect a store to actual core/region commits and retain observer failures."""
    from runtime.dec_core import observe_commits
    with observe_commits(store.core_recorder) as observation:
        try:
            yield observation
        finally:
            if observation.failure_count:
                store._failure(TraceIncomplete(observation.last_error))
