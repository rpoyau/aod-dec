# Source-supported stellar reconstruction inputs

The official SPARC catalog supplies the five galaxies' geometry and photometric scales needed to construct a data-conditioned stellar density family. The existing inclination-corrected `SBdisk` rows provide its radial column constraint. This is constructive additional input beyond the original eight-column files; it does not require inventing an evolution law.

The local catalog is `reconstruction_sources/SPARC_Lelli2016c.mrt`, SHA-256 `5aa0501f6b0d881fa579030e315e7b5b6ef561a5bd3a07472f9929c7e5728243`. Its data rows use 19 whitespace-separated fields; the later field positions drift from the nominal fixed-width header. Selected rows are 104, 143, 145, 152 and 187. Exact decimal strings, rational values, source rows, references and unavailable uncertainty fields are preserved in `RECONSTRUCTION_INPUTS.json`.

| Galaxy | D ± error [Mpc] | Inclination ± error [deg] | R_d [kpc] | R_eff [kpc] | L_3.6 ± error [10^9 Lsun] | R_HI [kpc] |
|---|---:|---:|---:|---:|---:|---:|
| DDO154 | 4.04 ± 0.20 | 64 ± 3 | 0.37 | 0.65 | 0.053 ± 0.002 | 4.96 |
| NGC2403 | 3.16 ± 0.16 | 63 ± 3 | 1.39 | 2.16 | 10.041 ± 0.028 | 15.11 |
| NGC2841 | 14.10 ± 1.40 | 76 ± 10 | 3.64 | 5.51 | 188.121 ± 0.520 | 45.12 |
| NGC3198 | 13.80 ± 1.40 | 73 ± 3 | 3.14 | 5.84 | 38.279 ± 0.212 | 35.66 |
| NGC6503 | 6.26 ± 0.31 | 74 ± 2 | 2.16 | 1.62 | 12.845 ± 0.059 | 14.05 |

The table supplies mean errors for distance, inclination and luminosity. It supplies no corresponding per-galaxy error columns for R_d, R_eff or R_HI. R_HI denotes the radius at an H I column of 1 solar mass/pc², not an outer material boundary. [Official SPARC Table 1](https://astroweb.case.edu/SPARC/SPARC_Lelli2016c.mrt)

The SPARC mass model adopts an exponential vertical stellar profile with `h/kpc = 0.196 (R_d/kpc)^0.633`, and spherical bulges. These are geometric assumptions. The catalog inclination represents an outer-disk mean; a varying warp requires additional information. [Lelli et al. 2016, §§2 and 3.3](https://astroweb.case.edu/ssm/papers/AJv152n157.pdf)

Applying that empirical relation gives h = 0.104455, 0.241426, 0.444051, 0.404401 and 0.319128 kpc in the table order. These are calculated estimates; no measured height or zero uncertainty is implied.

For the implemented reconstruction, the normalized family is

```
rho_star(R,z) = Upsilon_disk Sigma_L(R) exp(-|z|/h) / (2h).
```

This formula is the normalized density implied by the stated exponential family. Unit conversion must accompany h: with Sigma in Lsun/pc² and h in kpc, the denominator is `2000*h` pc. Integrating through all z returns `Upsilon_disk Sigma_L(R)` for every positive h. Consequently, a fit to these face-on columns alone cannot select h. The empirical size relation supplies that otherwise unidentifiable quantity; it is not a result of fitting the column projection.

The root implementation uses linear interpolation of tabulated stellar columns, axisymmetry, a declared mass-to-light ratio and this height estimate. It retains analytic vertical tails. Central stellar support, gas volume density and bulge volume density remain unfilled. Optional radial extrapolations are separately reported and do not establish measured outer mass. The physical density has not been identified with native support or an AOD formation history.

The present projection reconstructs the already inclination-corrected column. A future on-sky image projection would also require viewing orientation, spatial sampling and instrumental effects; it would integrate through tilted sight lines. Table 1 does not supply position angle, a radial warp map or azimuthal structure. Axisymmetric radial averages cannot establish the actual arrangement of bars, arms or individual stars. Global H I mass and radius constrain possible gas profiles but do not uniquely determine a gas density field.

SPARC publicly links [on-sky photometric profiles](https://astroweb.case.edu/SPARC/sfb_LTG.zip) and [bulge/disk decompositions](https://astroweb.case.edu/SPARC/BulgeDiskDec_LTG.zip). The former are uncorrected for inclination/extinction; the latter include declared extrapolations. These provide a route to constrain radial profiles at their own photometric sampling, instead of relying only on rotation-bin samples. [SPARC data description](https://astroweb.case.edu/SPARC/)

The separate [bulge table](https://astroweb.case.edu/SPARC/Bulges.mrt) gives NGC2841 a nonparametric bulge luminosity of 46.0617 × 10^9 Lsun and zero assigned bulge luminosity for the other four. Its extrapolation warning must accompany any total bulge constraint. The profile archives and bulge table were identified through the official website but were not downloaded in this child task; the existing rotmod surface columns are sufficient for the implemented initial stellar family.

Exact recovery of the supplied column is an interpolation/reconstruction check. It is not a held-out prediction of galaxy dynamics. Geometry may be fitted against photometric observations while rotation targets remain separate. If geometry, thickness, mass-to-light ratio or effective current factors are optimized against Vobs, report those results as target-conditioned fits and assess predictive performance on a separately declared held-out set. A fitted inverse family does not by itself select a unique three-dimensional history or prove that AOD evolution produced its density.

Independent local checks compared 55 exact geometry/metadata fields and empirical-height calculations with `outputs/results.json`; all matched. Its reviewed SHA-256 and individual checks are in `RECONSTRUCTION_INPUTS.json`. No root source code was changed by this review.

The delayed child response resulted from a direct download's sandbox DNS failure followed by a pending escalation that was aborted after 653.8 seconds. Queued parent messages became available after that tool returned. The parent subsequently downloaded Table 1, which supplied the local artifact used here; no further network requests were made.
