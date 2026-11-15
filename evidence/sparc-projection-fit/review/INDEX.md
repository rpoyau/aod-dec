# Projection-fit implementation and review

User instruction: simulate and fit a three-dimensional distribution whose projection matches the acquired observations. This authorizes implementation and an observational inverse fit. No new ontology or galactic time-transition law is introduced.

Bound DEC base: 99035b0cd7bad3103e55fe0cf82fffc6b3145b8d. Main/Manual: ab8359209b8f9b2ef4007403101016e3ea58cfe4. Source-specific review bindings are in model-scope-sources.json. Current AOD/AF skills and local Main/Manual ownership amendment govern this application; pinned repository skill integration is recorded in docs/SKILLS.md.

Source/claim review is kept separate from applied source changes. The source-bound model-scope.md documents actual inspected coverage, the finite observational model choices and falsifiers. Numerical review and executed controls follow in model-numerics.md. Results are empirical in-sample fits of projected moment maps. No native formation-history or time-evolution claim is attached to numerical optimizer steps.

The fit uses the acquired HALOGAS moment0/1 data for NGC2403 and NGC3198, the fixed catalog HI mass at its stated distance, a normalized finite radial density basis, exponential vertical distributions, line-of-sight projection and measured beam convolution. Alternative geometry evaluations and residuals are retained. The zero-filled exterior of the source moment mask is not treated as a measured physical vacuum.

The retained stellar observations are a different tracer and galaxy. They are not inserted into these HI fits. The full spectral cubes, stellar and bulge distributions, total galactic matter, line-profile shapes, independent validation and physical evolution remain outside this declared fit.
