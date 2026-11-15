# Bound SPARC inputs

The five `*_rotmod.dat` files are the retained 209-bin cohort, byte-identical to
the official [rotation-model archive](https://astroweb.cwru.edu/SPARC/Rotmod_LTG.zip).
`SPARC_Lelli2016c.mrt` is the official
[galaxy catalog](https://astroweb.cwru.edu/SPARC/SPARC_Lelli2016c.mrt), downloaded
15 November 2026. File hashes are in the runner results and repository manifest.

Cite Lelli, McGaugh & Schombert (2016), *The Astronomical Journal* 152, 157,
[doi:10.3847/0004-6256/152/6/157](https://doi.org/10.3847/0004-6256/152/6/157),
and the original rotation sources recorded in the catalog where applicable.

The rotation table's SBdisk/SBbul are inclination-corrected profiles. They
include decomposition and extrapolation; they are not all direct measurements.
Component velocities are mass-model proxies, not 3D tracer velocity vectors.
Vobs and errV remain comparison targets except in explicitly labelled inverse
fits. Their quoted errors exclude inclination systematics.

Catalog disk size supplies the empirical height estimate used by the stellar
reconstruction. Tabulated zero surface brightness is zero at published precision;
it does not establish empty physical support. The runner retains the analytic
vertical tails and reports a separate radial exponential extrapolation from the
last positive tabulated value. Neither is an admitted native AOD history.

Run `python3 scripts/reconstruct_sparc.py --output build/sparc` from the repository.
The full catalog's documented byte offsets omit separator spaces present in its
data rows; the loader binds the actual 19-column token layout and preserves the
original row and line number. Selected distances must match the radial files.
