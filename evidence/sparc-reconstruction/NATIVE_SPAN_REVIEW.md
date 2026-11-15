# Independent native-span review

Reviewed `simulations/sparc_native_span.py`, its tests, and the bound native collection/trace/certificate validators. This bounded review concerns admission, cutoff projection, nested history, and pending/started support status. It does not establish physical galaxy preparation.

## Findings

The extractor checks the full retained union and individually validates wave/certificate collections before computing any count. It separately checks same-lineage origin conflicts and divergent appendices for an immutable wave identity. Compatible repeated prefixes deduplicate. Supplied pending tails must share retained ancestry, start strictly after the last certified return, remain open, and not overlap another pending tail. Native union validation includes nested folded histories and motif component witnesses.

At `end_cut`, the closure count, certificate digests and return identities include only completed returns. Recurrence is labelled admitted only after two completed returns. Sparse gaps are exposed through last-recorded-cut and unobserved intervals; the report explicitly disclaims continuous observation. Completed outer closure certificates bind only earlier inner histories, and folded dependency records preserve those exact inner certificate identities.

One output ambiguity was reproduced and referred to the owning agent for correction: `scale_range` initially included any started support, and an outer path could appear before its first closure or recurrent admission. For inner returns completing at 2, 7 and 16, an outer scale-7 path starts at 17, first closes at 19, and becomes recurrent at 29. The prior report already said `maximum=7` at cut 17, while the wave row correctly said pending with zero closures. This must be labelled started support and separated from the admitted recurrent range, with path-node recurrence status retained. Probe: `native-span-cutoff-probe.json`.

The same probe compares two later returns with identical events through cut 15 and different future continuations. Their cutoff-local `prefix_digest` agrees; full `source_trace_digest` differs, while raw future bytes are absent. The full-source digest is retrospective archive binding and needs an explicit role distinct from cutoff-local operands. The owning agent implemented this distinction alongside the range correction. No extractor source edits were made by this reviewer.

The correction was independently rechecked: `scale_range_basis=started_support` now labels the original range, while `admitted_recurrent_scale_range` remains `[0]` at cuts 17, 18 and 19, becoming `[0,7]` only at cut 29. Path nodes carry recurrence status and pre-admission row identities are explicitly retrospective. Full-source and cutoff-local digests have separate scope labels. The report itself is labelled a retrospective validated-source prefix rather than an online admission computation. See `native-span-cutoff-postfix.json`. The source owner reports 14 focused tests passing in normal and optimized Python; this review's independent cutoff probe also passes.

## Canonical fixture impact

The trace causal-parent repair does not change the chiral generator, host evaluator or native producer hashes. Independently regenerated the actual canonical recipe: `core_only`, seed 7, common paired-study ticket denominator 64. The generated and retained `examples/chiral_core_collision.dec` match byte-for-byte at SHA-256 `e7483a2b5274b523e9246a24a0bb7f25034933069484aacb05ba396fa5fc2ba0`. See `canonical-chiral-check.json`. Generating the API's different default configuration is not the canonical recipe and is not a fixture regression.

## Coverage and disposition

Direct source review plus exact cutoff probes and canonical regeneration were performed. The root agent runs the full test suite. The started-versus-admitted range and retrospective-digest output clarification are corrected and rechecked; the counts and retained union checks inspected here did not require repair. No actionable issue remains within this bounded review. No broader claim about arbitrary hostile inputs, unbounded recursion, physical cadence or a complete galaxy run follows from this review.
