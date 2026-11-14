"""Bounded consistency admission across independently retained native witnesses.

Individual objects must already pass their owning native validators. This index
adds the finite-union check across query records, including recursively retained
components and inner waves. It never evolves or modifies the source objects.
"""
from .vendor import aod_manual_native as native


class HistoryUnionError(ValueError):
    pass


class RetainedHistoryUnion:
    def __init__(self, *, max_bindings=100000, max_nodes=500000):
        if any(type(n) is not int or n <= 0 for n in (max_bindings, max_nodes)):
            raise HistoryUnionError("positive exact history-union budgets required")
        self.max_bindings, self.max_nodes = max_bindings, max_nodes
        self.bindings, self.nodes, self.failure = {}, 0, None

    def _step(self):
        if self.nodes >= self.max_nodes:
            raise HistoryUnionError("native history union node budget exhausted")
        self.nodes += 1

    def _bind(self, key, value, message):
        if key in self.bindings:
            if self.bindings[key] != value:
                raise HistoryUnionError(message)
        else:
            if len(self.bindings) >= self.max_bindings:
                raise HistoryUnionError("native history union binding budget exhausted")
            self.bindings[key] = value

    def include(self, *objects):
        if self.failure is not None:
            raise HistoryUnionError(self.failure)
        try:
            for value in objects:
                self._walk(value, 0)
        except HistoryUnionError as exc:
            # No later member may turn a conflicted or truncated union complete.
            self.failure = str(exc)
            raise

    def _walk(self, value, depth):
        self._step()
        if depth > 64:
            raise HistoryUnionError("native history union depth budget exhausted")
        if type(value) is native.Trace:
            context = (value.scope, value.scale, value.lineage)
            self._bind(("terminal", *context, value.event_ids[-1]), value.trace_digest,
                       "conflicting trace for immutable terminal return event")
            for event, vertex, cut in zip(value.event_ids, value.vertices, value.indices):
                self._step()
                self._bind(("event", *context, event), (vertex, cut),
                           "conflicting coordinates for immutable retained event")
        elif type(value) is native.Certificate:
            for wave in value.inner_waves:
                self._walk(wave, depth + 1)
        elif type(value) is native.MotifWitness:
            self._walk(value.support_trace, depth + 1)
            self._walk(value.closure, depth + 1)
            for trace in value.component_traces:
                self._walk(trace, depth + 1)
            for certificate in value.component_certificates:
                self._walk(certificate, depth + 1)
        elif type(value) is native.Wave:
            context = (value.scope, value.scale, value.lineage, value.identity)
            self._bind(("wave", *context), value.motif,
                       "conflicting retained wave type")
            for index, (trace, certificate) in enumerate(zip(value.returns, value.certificates)):
                self._step()
                motif = value.motif_witnesses[index] if value.motif_witnesses else None
                binding = (trace.trace_digest, certificate.certificate_digest,
                           None if motif is None else motif.witness_digest)
                self._bind(("prefix", *context, index), binding,
                           "conflicting retained wave prefix")
                self._walk(trace, depth + 1)
                self._walk(certificate, depth + 1)
                if motif is not None:
                    self._walk(motif, depth + 1)
        else:
            raise HistoryUnionError("unsupported native history union member")

    def status(self):
        return {"status": "rejected" if self.failure else "consistent",
                "reason": self.failure, "bindings": len(self.bindings), "nodes": self.nodes,
                "max_bindings": self.max_bindings, "max_nodes": self.max_nodes}
