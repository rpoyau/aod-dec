# Shared boundary response: implementation and cost-inclusive check

The helper now exposes `boundary_context(before, channels, *, scope, window, window_domain, after=...)`, `context.response()` and `boundary_view(context, before_region, after_region)`. The optional supplied after-state is verified against the actual unchanged native updater, including pressure, cut and closure metadata. Context creation performs one response and commits one complete pre-response swept field. Each boundary view still calls the unchanged `region.audit_boundary(..., scope='full')` validator; shared admission does not bypass exact source-family checks.

`boundary_step` remains the external compatibility entry point. It validates the supplied after-state and rejects invalid regions before emitting a ledger commit. Both APIs return the existing dictionaries, field values, ledger snapshots and audit records. The context is frozen and holds copied private snapshots. Caller mappings, state/channel objects, returned response records, returned ledgers and mutable audit rows do not alias the private snapshots used by subsequent views.

All 14 focused tests pass in normal and optimized Python. Tests retain the independent stock/sweep expectations, reject mismatched pressure/current/commitment/closure identity/context, include explicit unavailable and shedding channels, reject attempted context replacement, and deliberately mutate caller and returned frozen objects through Python's low-level mutation escape hatch to verify isolation. Four views perform exactly one native update per state and one ledger commit while still invoking four full boundary audits. The selected/complement/whole partition passes the existing validator.

The benchmark loads the original helper bytes from commit `430fb14` as an independent baseline. The exact fixtures have 24 or 48 cells, twelve retained native returns per cell, rational state/current/pressure operands and four selected/complement/whole/stationary views. The 48-cell selection actually changes membership. Eleven alternating timed repetitions include new context creation, all validation, copying the response for the caller, and all four audits. Input preparation is shared and excluded. The baseline times only its four original boundary_step calls, not the model's additional original response_step; the comparison is therefore conservative for that omitted work. Complete returned records and their canonical fingerprints match the original, not merely their scalar totals.

| Python mode | Cells | Original four steps, median | Context + response copy + four views, median | Ratio |
|---|---:|---:|---:|---:|
| normal | 24 | 0.149280 s | 0.097397 s | 1.533× |
| normal | 48 | 0.292439 s | 0.188202 s | 1.554× |
| optimized | 24 | 0.151161 s | 0.098021 s | 1.542× |
| optimized | 48 | 0.295516 s | 0.190554 s | 1.551× |

These are local helper measurements with raw timings retained in the JSON artifacts. They establish a cost-inclusive saving for these fixtures, not a universal complexity bound or a full-application speedup. Retained-history work, global application tracing, serialization, archive I/O and other engine costs are outside this measured helper interval. The source-bound proof and patch identify the exact final implementation; parent integration and full 209-bin trace equivalence are separate checks.
