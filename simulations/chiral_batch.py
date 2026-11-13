"""Table-driven integer execution for a supplied chiral application model.

The model adapter compiles declared probabilities into exact ticket thresholds.
The scalar and multicore executors only consume those tables and supplied
tickets. Their rows are transition records, not admitted return certificates;
full D.E.C. witness construction and routing admission remain host obligations.
"""
from __future__ import annotations

from concurrent.futures import ProcessPoolExecutor
from dataclasses import asdict, dataclass
import hashlib
import json
import multiprocessing as mp
import os
from random import Random
import time

from .chiral_collision import (CollisionConfig, INITIAL_STATE, State,
    common_ticket_denominator, flip_probability, move_kernel)
from .tesseract_cells import FACETS


UINT32_MAX = (1 << 32) - 1
MAX_STEPS = 64
# Bounds also keep every flattened buffer index below signed 32-bit capacity.
MAX_PATHS = 1_000_000
MAX_TRANSITIONS = 1_000_000
OUTPUT_FIELDS = ("facet_a", "facet_b", "chi_a", "chi_b", "flip_a", "flip_b", "contact")


def _integer(value, low, high, name):
    if type(value) is not int or not low <= value <= high:
        raise ValueError(f"{name} requires an exact integer in [{low},{high}]")


def _tuple(value, length, name):
    if type(value) is not tuple or len(value) != length:
        raise ValueError(f"{name} requires an immutable tuple of length {length}")


def _digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"),
                                     ensure_ascii=True).encode()).hexdigest()


@dataclass(frozen=True, slots=True)
class TransitionTables:
    """Caller-supplied tables, without intrinsic geometry or flip probabilities.

    Movement is indexed by [previous facet][movement ticket in 0..7]. The flip
    table is indexed by [destination facet][other carrier destination]. A flip
    occurs when the supplied flip ticket is strictly below that integer entry.
    """
    move_destinations: tuple[tuple[int, ...], ...]
    flip_thresholds: tuple[tuple[int, ...], ...]
    ticket_denominator: int

    def __post_init__(self):
        _integer(self.ticket_denominator, 1, UINT32_MAX, "ticket denominator")
        for name, high in (("move_destinations", 7),
                           ("flip_thresholds", self.ticket_denominator)):
            table = getattr(self, name)
            _tuple(table, 8, name)
            for row in table:
                _tuple(row, 8, name + " row")
                for value in row:
                    _integer(value, 0, high, name + " entry")


@dataclass(frozen=True, slots=True)
class TicketBatch:
    """Exact supplied tickets and initial states; path order is significant."""
    steps: int
    ticket_denominator: int
    initial_states: tuple[tuple[int, int, int, int], ...]
    tickets: tuple[tuple[tuple[int, int, int, int], ...], ...]
    seeds: tuple[int, ...] | None = None

    def __post_init__(self):
        _integer(self.steps, 0, MAX_STEPS, "steps")
        _integer(self.ticket_denominator, 1, UINT32_MAX, "ticket denominator")
        if type(self.initial_states) is not tuple:
            raise ValueError("initial states require an immutable tuple")
        n = len(self.initial_states)
        _integer(n, 0, MAX_PATHS, "path count")
        if n * self.steps > MAX_TRANSITIONS:
            raise ValueError("batch exceeds the declared transition-buffer bound")
        _tuple(self.tickets, n, "ticket paths")
        if self.seeds is not None:
            _tuple(self.seeds, n, "seeds")
            if any(type(seed) is not int for seed in self.seeds):
                raise ValueError("seed bindings require exact integers")
        for initial, path in zip(self.initial_states, self.tickets):
            _tuple(initial, 4, "initial state")
            for facet in initial[:2]:
                _integer(facet, 0, 7, "initial facet")
            if any(type(sign) is not int or sign not in (-1, 1) for sign in initial[2:]):
                raise ValueError("initial chirality requires exact -1 or +1")
            _tuple(path, self.steps, "ticket path")
            for row in path:
                _tuple(row, 4, "ticket row")
                for ticket in row[:2]:
                    _integer(ticket, 0, 7, "movement ticket")
                for ticket in row[2:]:
                    _integer(ticket, 0, self.ticket_denominator - 1, "flip ticket")


def validate_batch(tables, batch):
    if type(tables) is not TransitionTables or type(batch) is not TicketBatch:
        raise TypeError("typed TransitionTables and TicketBatch required")
    # Repeat validation at the execution boundary, including after unsafe object
    # mutation or restoration. Python optimized mode must not weaken the checks.
    tables.__post_init__()
    batch.__post_init__()
    if tables.ticket_denominator != batch.ticket_denominator:
        raise ValueError("table and ticket denominators differ")
    return True


def _model_denominator(config, supplied):
    if type(config) is not CollisionConfig:
        raise TypeError("a declared CollisionConfig model is required")
    required = common_ticket_denominator((config,))
    denominator = required if supplied is None else supplied
    _integer(denominator, 1, UINT32_MAX, "ticket denominator")
    if denominator % required:
        raise ValueError("ticket denominator does not represent this model exactly")
    return denominator


def compile_model_tables(config, *, ticket_denominator=None):
    """Application adapter; compile this model's law without changing executors."""
    d = _model_denominator(config, ticket_denominator)
    movement = []
    for facet in FACETS:
        row = []
        for destination, probability in move_kernel(facet).items():
            weight = probability * 8
            if weight.denominator != 1:
                raise ValueError("model movement requires a different ticket grid")
            row.extend([FACETS.index(destination)] * weight.numerator)
        movement.append(tuple(row))
    thresholds = tuple(tuple(int(flip_probability(a, b, config) * d) for b in FACETS)
                       for a in FACETS)
    return TransitionTables(tuple(movement), thresholds, d)


def build_ticket_batch(config, seeds, *, ticket_denominator=None, initial=INITIAL_STATE):
    """Bind existing four SHA-derived streams to supplied integer ticket rows.

    Ticket generation is a host experiment-preparation step. All execution
    backends receive identical tickets; no backend-specific random sampler is
    introduced. This reproduces the existing chiral fixture's stream contract.
    """
    d = _model_denominator(config, ticket_denominator)
    if type(initial) is not State:
        raise TypeError("complete initial State required")
    seeds = tuple(seeds)
    if len(seeds) > MAX_PATHS or len(seeds) * config.steps > MAX_TRANSITIONS:
        raise ValueError("requested batch exceeds the declared buffer bound")
    if any(type(seed) is not int for seed in seeds):
        raise ValueError("seeds require exact integers")
    paths = []
    for seed in seeds:
        streams = [Random(int.from_bytes(hashlib.sha256(
            f"aod-dec-chiral-v1:{seed}:{name}".encode()).digest(), "big"))
            for name in ("move-a", "move-b", "flip-a", "flip-b")]
        paths.append(tuple((streams[0].randrange(8), streams[1].randrange(8),
                            streams[2].randrange(d), streams[3].randrange(d))
                           for _ in range(config.steps)))
    state = (FACETS.index(initial.facet_a), FACETS.index(initial.facet_b),
             initial.chi_a, initial.chi_b)
    return TicketBatch(config.steps, d, (state,) * len(seeds), tuple(paths), seeds)


def buffer_inputs(tables, batch):
    """Flatten exact validated values; tickets/tables use uint32, states int32."""
    validate_batch(tables, batch)
    return {"move_destinations": tuple(v for row in tables.move_destinations for v in row),
            "flip_thresholds": tuple(v for row in tables.flip_thresholds for v in row),
            "initial_states": tuple(v for row in batch.initial_states for v in row),
            "tickets": tuple(v for path in batch.tickets for row in path for v in row)}


def validate_rows(batch, rows):
    """Validate transport representation only, without certifying a model law."""
    _tuple(rows, len(batch.initial_states), "output paths")
    for path in rows:
        _tuple(path, batch.steps, "output path")
        for row in path:
            _tuple(row, len(OUTPUT_FIELDS), "output row")
            for facet in row[:2]:
                _integer(facet, 0, 7, "output facet")
            if any(type(sign) is not int or sign not in (-1, 1) for sign in row[2:4]):
                raise ValueError("output chirality requires exact -1 or +1")
            for bit in row[4:]:
                _integer(bit, 0, 1, "output bit")
            if row[6] != int(row[0] == row[1]):
                raise ValueError("output contact bit differs from destination equality")
    return True


def result_record(tables, batch, rows, backend, metadata):
    """Common report; EXECUTED does not imply witness or empirical validation."""
    validate_batch(tables, batch)
    validate_rows(batch, rows)
    table_hash, batch_hash = _digest(asdict(tables)), _digest(asdict(batch))
    semantic_hash = _digest({"tables": table_hash, "ticket_batch": batch_hash, "rows": rows})
    return {"format": "aod-dec.table-batch-execution", "version": 1, "status": "EXECUTED",
            "backend": backend, "path_count": len(rows), "steps": batch.steps,
            "output_fields": OUTPUT_FIELDS, "rows": rows,
            "tables_sha256": table_hash, "ticket_batch_sha256": batch_hash,
            "semantic_sha256": semantic_hash, "execution": metadata,
            "scope": "Integer transition-table execution; no full return certificates or routing admission are constructed here."}


def _evaluate_path(move, thresholds, initial, tickets):
    a, b, ca, cb = initial
    rows = []
    for ma, mb, fa, fb in tickets:
        a, b = move[a][ma], move[b][mb]
        flip_a, flip_b = int(fa < thresholds[a][b]), int(fb < thresholds[b][a])
        ca = -ca if flip_a else ca
        cb = -cb if flip_b else cb
        rows.append((a, b, ca, cb, flip_a, flip_b, int(a == b)))
    return tuple(rows)


def run_scalar(tables, batch):
    started = time.perf_counter_ns()
    validate_batch(tables, batch)
    rows = tuple(_evaluate_path(tables.move_destinations, tables.flip_thresholds, initial, tickets)
                 for initial, tickets in zip(batch.initial_states, batch.tickets))
    metadata = {"elapsed_ns": time.perf_counter_ns() - started,
                "requested_workers": 1, "participating_worker_pids": (os.getpid(),) if rows else (),
                "timing_scope": "Input validation and transition evaluation; excludes ticket/model preparation, result encoding and full witness construction."}
    return result_record(tables, batch, rows, "scalar-cpu", metadata)


_START_BARRIER = None
_FIRST_TASK = True


def _worker_initialize(barrier):
    global _START_BARRIER, _FIRST_TASK
    _START_BARRIER, _FIRST_TASK = barrier, True


def _worker_chunk(job):
    global _FIRST_TASK
    if _FIRST_TASK:
        # Every selected worker receives a first job before any first job runs.
        # This observes actual participation even for tiny finite test batches;
        # synchronization and spawn overhead remain part of the measured cost.
        _START_BARRIER.wait(timeout=30)
        _FIRST_TASK = False
    offset, move, thresholds, initials, paths = job
    rows = tuple(_evaluate_path(move, thresholds, initial, tickets)
                 for initial, tickets in zip(initials, paths))
    return offset, rows, os.getpid()


def run_multicore(tables, batch, *, workers=2, chunksize=16):
    started = time.perf_counter_ns()
    validate_batch(tables, batch)
    _integer(workers, 1, 64, "workers")
    _integer(chunksize, 1, MAX_PATHS, "chunksize")
    jobs = [(offset, tables.move_destinations, tables.flip_thresholds,
             batch.initial_states[offset:offset + chunksize], batch.tickets[offset:offset + chunksize])
            for offset in range(0, len(batch.initial_states), chunksize)]
    effective = min(workers, len(jobs))
    parts = []
    if effective:
        context = mp.get_context("spawn")
        barrier = context.Barrier(effective)
        with ProcessPoolExecutor(max_workers=effective, mp_context=context,
                                 initializer=_worker_initialize, initargs=(barrier,)) as pool:
            parts = list(pool.map(_worker_chunk, jobs, chunksize=1))
    parts.sort(key=lambda part: part[0])
    if [part[0] for part in parts] != [job[0] for job in jobs]:
        raise RuntimeError("worker result offsets differ from the submitted partition")
    rows = tuple(row for _, chunk, _ in parts for row in chunk)
    pids = tuple(sorted({pid for _, _, pid in parts}))
    if len(pids) != effective:
        raise RuntimeError("observed workers differ from the selected executor pool")
    metadata = {"elapsed_ns": time.perf_counter_ns() - started, "start_method": "spawn",
                "requested_workers": workers, "effective_workers": effective,
                "participating_worker_pids": pids, "chunksize": chunksize, "jobs": len(jobs),
                "timing_scope": "Input validation, spawn, first-job synchronization, table/ticket IPC, evaluation, result IPC, ordered merge and pool shutdown; excludes ticket/model preparation, report encoding and full witness construction."}
    return result_record(tables, batch, rows, "multicore-cpu", metadata)
