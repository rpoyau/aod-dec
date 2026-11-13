# Executed feature inventory

| Feature | Reference VM | Native AOT | ORC JIT |
|---|---|---|---|
| Exact int/ratio; checked t1/t32/q4; bool/string | Yes | Yes | Yes |
| Explicit overloads, direct calls, bounded recursion | Yes | Yes | Yes |
| Conditional/Boolean branches with ordered effects | Yes | Yes | Yes |
| Source-located operation errors and assertions | Yes | Yes | Yes |
| Logical expression/statement/call fuel | Yes | Yes | Yes |
| Immutable closure/fusion/coupling witnesses | Yes | Yes | Yes |
| All five repaired examples | Yes | Yes | Yes |
| Runtime scalar inputs to named function | Embedding | Native ABI; tested harness | Embedding |
| Generated native entry guard and fallback | Not applicable | Generic compiled path | Yes |
| Persistent measured O0/O2 selection | Not applicable | Validation comparator | Adaptive host and benchmark |
| General `finite_afc` collection helper | Python API only | No language surface | No language surface |
| Structs/arrays/slices/tuples/loops/modules/I/O | Unsupported | Unsupported | Unsupported |
| Literal-pattern dispatch | Unsupported | Unsupported | Unsupported |
| Genuine Dec compiler bootstrap stages | Not implemented | Not implemented | Not implemented |

The original core grammar is frozen at v1. No extensions were required to execute
the five examples or the M0–M5 compiler-control model, and none are silently
accepted. `dec/*.dec` remains rejected design material. General folding/RCD,
physical awareness/FRG equivalence, GPU/UMA and parallel execution require separate
models and language increments.

Linux x86-64 is the executed target. AArch64 and other operating systems remain
unsupported. AOT needs its listed native shared libraries, but no Python runtime.
Compiler frontend, JIT control and adaptive policy currently use Python. Retained
profiles are single-writer; concurrent first native builds share locked atomically
published caches. Cache directories are trusted local build storage.

Performance is separately evaluated: functional profile influence is demonstrated,
but the delivered held-out experiment does not establish cost-inclusive speedup.
The source-bound execution dispositions are in
`evidence/native-r01/ACCEPTANCE_RESULTS.json`; `evidence/README.md` explains their
archived paths and the separate fresh repository checks.
