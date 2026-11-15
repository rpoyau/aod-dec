# SPARC reconstruction and calculated native spans

This change implements data reconstruction under the owner's instruction to
keep AOD fixed and infer missing galaxy data. Main/Manual is bound to
`ab8359209b8f9b2ef4007403101016e3ea58cfe4`; D.E.C. starts at
`dbf79dcc14d5a36a68969212c617bb8d486fee2d`. Upstream manuscripts are unchanged.

The stellar input reconstruction uses the five official SPARC profiles and
catalog disk sizes. It computes a positive axisymmetric exponential vertical
density, exact column projection, radial annular mass, physical magnitude spans,
and explicit vertical tails. The empirical height relation estimates an input;
column projection cannot uniquely identify thickness. Tabulated zeros do not
establish absent physical outer support. Both rounded-endpoint continuation and
a separately inferred positive-anchor extension are retained.

`sparc_native_span.extract_native_span` calculates beat separations, local
windows, reclosure spans and actual fold dependencies from validated histories.
Started support and admitted recurrent scales are distinct. Open tails and
unobserved gaps remain visible. It rejects conflicting histories and does not
invent native octave/SI conversions. The current native Wave representation has
no octal address field; optional address bindings remain supplied metadata.

The unchanged G0 control contains all 209 bins: exact-rational aggregation
displays **26.570553423860424%** equal-bin and **28.44053183847862%** equal-galaxy.
The first is one binary64 ULP below the earlier floating-point aggregation;
the underlying frozen formula/data are unchanged. An explicitly target-conditioned
inverse calculation preserves three negative required contributions and unknown
physical uncertainty. One effective factor per galaxy minimizes the in-sample
square-speed MAPE at **20.270217992958536%**. This diagnostic is not a reconstructed
density, native current result or independent prediction.

## Four-stage coverage

| Requested stage | Executed result |
|---|---|
| Field inputs and scales | Stellar density family and physical spans; native span extractor uses actual histories. Central/gas/bulge/background and temporal input binding remain incomplete. |
| AOD evolution and complete current accounting | Existing response is unchanged. No galaxy native formation/current history has been fabricated; G2/G3 evolution is not run. |
| Projection | Full analytic stellar column projection; no evolved-galaxy rotation projection. |
| Same-cohort comparison | Frozen G0 control and separately labelled inverse requirements/in-sample diagnostic across all 209 bins. |

The remaining task is the acquisition/reconstruction binding from physical
mass/size, density and temporal support to retained native operands. The Manual
Appendix C interface requires that binding; its fixed `beta=5` fixture is not
used as galaxy data. This is not a request for a replacement evolution law.

## Trace correction and validation

The interleaved sequence route A, route B, receipt A previously attached the
receipt to B. The recorder now resolves the actual route's durable identity,
including its independent branch origin. Reopened archives and alternative
windows are regression-tested; the underlying model result is unchanged.

- 49 SPARC tests pass in normal and optimized Python, including the actual
  209-bin pipeline, exact physical spans, negative/infeasible inverse values,
  independent density quadrature, tail budgets, catalog alignment, conflicting
  histories, nonuniform cadence, pending folds and future-prefix controls.
- 25 trace-application tests pass in each mode, including the two new routing
  regressions. Canonical chiral regeneration is byte-identical.
- Pinned aod-lint scans ten Typst manuscripts; its scope is manuscript diagnostics.
  The new two-page Typst PDF builds and both pages were visually inspected.
- Full CI runs normal/optimized suites and all existing native/application
  stages, plus a fresh reconstruction and Typst build. Remote completion status
  belongs to the PR checks; these local results do not claim a remote outcome.

The source-bound review files, exact JSON, density/current CSVs and PDF are
retained here. `validation.json` binds commands, inspected scope and file hashes.
Reproduce with `python3 scripts/reconstruct_sparc.py --output build/sparc` and
the Typst command in `docs/sparc-reconstruction.typ`.
