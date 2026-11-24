// Alpha-Omega Dynamics (AOD) — Dec v2 Reference Engine & Manual (M4-Aligned Typst Source)
// Compile from repository root via:
//   typst compile --root . --creation-timestamp 0 docs/manual/main.typ docs/manual/aod-dec-v2-manual.pdf

#import "/bindings/aod/aod_schema.typ": *

#show: aod-document.with(
  title: "AOD-DEC v2: Native Reference Engine, Standard Library, and Verification Manual",
  author: AODDocumentAuthor,
  subject: "Exact Dec v2 compiler, std-only Rust core, LLVM 18 AOT/ORC LLJIT backend, native AOD standard library, and hardware-informed policy learner",
  keywords: ("AOD", "Dec", "AFC", "exact arithmetic", "LLVM 18", "ORC LLJIT", "Typst"),
  bib-path: "/docs/manual/refs.bib",
  is-manual: true,
  main-registry-path: "/bindings/aod/main_registry.json",
  expected-main-digest: "a05a787d75633c95e09c3b15fa9900a270d4a4bb31418a2423c7e8121fac9289",
)

#align(center)[
  #text(size: 17pt, weight: "bold")[AOD-DEC v2 Reference Engine and Verification Manual] \
  #v(0.35em)
  #text(size: 11.5pt)[Exact Dec v2 Language, Compile-Time Firewalls, Native Standard Library, LLVM 18 Backend, and Hardware Policy Learner] \
  #v(0.4em)
  #text(size: 10.5pt)[#AODDocumentAuthor --- Release Line `v2/native-reference-engine` (Bound to AOD `v41.02`)]
]

#v(0.5em)

#aod-noteblock("Scope")[
  This manual documents the downstream executable reference compiler `rpoyau/aod-dec` v2
  (#aod-cite("aod-main-v41, aod-manual-v41, afc-foundation, general-mechanics-style")).
  Every mathematical construct is exact (`BigInt`, canonical `Ratio`, checked balanced ternary `t1`/`t32`,
  finite `Q_4`, leading-zero-preserving `addr8`, and exact cyclotomic arithmetic), with zero IEEE-754
  floating-point types across all four `std`-only Rust crates. All chamber apparatus models remain explicitly
  tagged `DECLARED_SANDBOX_DEC_MODEL_NOT_S3_AUTHENTICATED`, and all JIT hardware-learning policies remain
  explicitly tagged `COMPILER_EXECUTION_POLICY_NOT_PHYSICAL_AOD_LAW`.
]

#aod-provenance[
  Upstream read-only bindings verified against `SOURCE_BINDING.json`: AOD Main `S0@98ddfa0` (`v41.02`),
  M4 Typst registry `S1b@cea71a7` (`455` Main labels, digest `a05a787d75633c95e09c3b15fa9900a270d4a4bb31418a2423c7e8121fac9289`),
  and Stage 2 golden vectors `S2@3122c94` (`30` vendored vectors, status `DRAFT_CANDIDATE_UNDER_S2_REVIEW`, PR `#51`).
  Zero upstream mutations are permitted in `rpoyau/aod`.
]

#outline(title: "Contents", depth: 2)
#pagebreak()

#include "chapters/00_quickstart.typ"
#include "chapters/01_language_and_types.typ"
#include "chapters/02_compile_time_firewalls.typ"
#include "chapters/03_aod_stdlib_and_manual_fixtures.typ"
#include "chapters/04_unbiased_apparatus.typ"
#include "chapters/05_llvm_and_hw_learner.typ"
#include "chapters/06_validation_and_limits.typ"

#pagebreak()
#aod-render-bibliography(title: "References")
