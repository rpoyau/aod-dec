# Source-bound compiled awareness example

Status: implemented and CPU-verified. The Manual source is vendored byte for byte; source paths and hashes, owner amendment and review skill are bound in AWARENESS_BINDINGS.json. Main and Manual were not edited.

## Governing operational claim

Main `sections/08_awareness_learning_mindfulness.tex:6–43` separates present state, closure memory and retained operative rebalancing, with prior source/target/boundary/scale/commit/contact binding. Lines 59–97 define matched structural removal of retained rebalancing, an outcome taken only from a later realized transition, and exact restoration. Lines 103–149 bound the self graph and distinguish source-wise operativity from cancellation. Lines 154–196 require later causal use rather than retained bytes alone.

Manual `manual/sections/00_awareness_fixtures.tex:15–36` makes executable inputs operative and keeps expected labels and resource proxies out of the engine. Lines 39–107 give J_next = J + (A + q + g sum(r))/p, with p=g=1, source input committed at 23, retention at 24, intervention at 33 and response at 34. Its existing two H1/fibre continuations are selected by the sign of the computed current. Lines 128–173 require the matched law/state/source conditions and structural removal. Lines 312–349 separate passive post-transition inspection from an explicit later query q=-J formed from a committed observation.

## Executed specialization

`examples/awareness_response.dec` implements three ordinary typed functions: integer unit-pressure response, its sign selector, and later reflexive query. CPU batches enter the generated LLVM function; GPU batches, when requested and available, enter generated IR-derived CUDA. No source-name dispatch and no Python engine result is substituted for generated output.

`simulations/awareness_jit_fixture.py` loads the exact Manual executable using a module-local import adapter to the existing DEC native producer. It does not replace a process-wide import alias. The dependency differs from the pinned Manual producer only by the three already accepted R07 exact-int validation repairs; AWARENESS_NATIVE_COMPATIBILITY.diff records these hunks. Response, update, retained provenance, contact and continuation mathematics are unchanged.

The host admits the actual Manual waves, contact/prior records and augmented states. It supplies integer operands only after checking pressure, gain, threshold, input cut, query cut, source availability and exact integrality. Compiled response and selector values form the next native state; full state/events must independently equal unchanged Manual execution. Source mutation producing a different generated result is demonstrated and rejected by the model binding.

All three lanes share X, closure memory, world, model and future input. Only R is removed and restored. The retained/restored currents are [1,2,3,4], while removal gives [0,0,0,0]. Retained/restored actual continuation edges alternate (0,1),(1,0); removal alternates (0,2),(2,0). The later query control gives retained/restored [1,1,1,1], and removal remains zero. Events occur at cuts 34–37. No new closure cycle is asserted or certified.

## Checks and limits

Six focused tests pass in normal and optimized Python: exact source/import binding; actual retained provenance and matched coordinates; real multicore JIT controls and passive equality; changed Dec source changes the machine result and fails the operational law; forged retention, future/stale query and invalid selector/type rejection. CPU smoke output is in awareness-cpu-smoke/. NVIDIA execution is deliberately recorded separately by the required-device runner; the tests do not infer it from host compilation.

The generic O0 compiler variant remains selected because no prior performance evidence was supplied. `scripts/adapt_native.py` and `native/learning.py` are a separate retained compiler-cost policy: selection precedes the current invocation's new profiling data, and its 0.05 relative-savings threshold means 5%, not 0.05%. Timing, backend labels, retained bytes and an intervention verdict are not used as the AOD outcome. This example makes no speedup, physical awareness, complete self-hosting or new probability-engine claim.

The final extension reuses existing learning() after the actual fourth compiled update, and the existing six-step current-three mindfulness() and delayed-query controls. Exact rational pressure controls stay on the host. See EXISTING_AWARENESS_EXAMPLES.md/json for source lineage and prior replay evidence. Final focused logs: awareness-expanded-normal.log and awareness-expanded-optimized.log (six passes each).
