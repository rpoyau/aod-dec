#import "/bindings/aod/aod_schema.typ": *
#import "/docs/manual/figures/fig_double_slit_delta3.typ": fig-double-slit-delta3
#import "/docs/manual/figures/fig_disk_enclosure.typ": fig-disk-enclosure

= Native Dec Standard Library and Executable Manual Fixtures <sec:stdlib>

#aod-paragraph("Setup.")
Five pure Dec modules under `stdlib/aod/` expose executable Manual calculations through the `use stdlib::aod::*`
namespace, resolved with strict root precedence (`--stdlib <dir>`, then `AOD_STDLIB_DIR`, then automatic
upward discovery, with zero silent fallback on explicit override) and bundled hermetically into `DecIR v2`.
Module constants initialize in post-order DFS topological dependency order so intra-stdlib dependencies
(such as `stdlib::aod::boundary` importing `stdlib::aod::ledger`) evaluate identically across Exact VM,
AOT, and ORC LLJIT.

#figure(
  kind: table,
  caption: [Pure Dec standard library modules (`stdlib/aod/*.dec`) and bound executable Manual fixtures.],
  table(
    columns: (auto, auto, 1fr),
    align: (left, left, left),
    toprule,
    [*Stdlib Module*], [*Fixture*], [*Verified AOD Manual Scope*],
    midrule,
    [`stdlib::aod::ledger`], [`01_dec_ledger.dec`], [Isotropic `(1/4)^4`, anisotropic `(1,1,4,2)`, blocked dwell hinge slide `(3,3,1)`, field tunnelling `6/7`, ADAR/SADAR `+-4`, `1:2:9_yro` surplus `3`, Tau `31/50`, `R02` gap `1/100`.],
    [`stdlib::aod::wavelet`], [`02_closure_wavelets.dec`], [12 Duon variants, same-current Duad seating, half-open window dedup `(0,6] U (4,8] -> 3 vs 4`, opposing cancellation, 6 recipient stages, exact cyclotomic $ZZ_(12)$ DFT over $QQ[x]/(x^4 - x^2 + 1)$.],
    [`stdlib::aod::boundary`], [`03_short_window_boundary.dec`], [11-bin Dimonhexon double-slit cross-return $delta_3$, short-window clipping (`1:2:7_yro`, `1:2:9_yro`, `1:2:10`), purely native `3:3:7_yro` saddle (`+3/-3`), moving boundary `7`, Stokes cancellation `0/1`.],
    [`stdlib::aod::cells`], [`04_cell_measures_tesseract.dec`], [4-cube `(16,32,24,8,1)`, `V_4=81/16` & `B_3=27`, quarter-symmetry disk enclosures `n=1..16`, rectangle swap `10->8->10`, exact discrete-law moments (`T`, `F`, `Rf`, `U`), affine covariance, 16-step tesseract transport.],
    [`stdlib::aod::metrology`], [`05_conditional_si_metrology.dec`], [Native turn advances, `prop:m3-atomic-gain` (`lambda=0` vs `1`), strictly quarantined `SiMetrology` (`1/4596315885`, Hellwig midpoint `14204057517679/10000`, separate NIST-F2 budgets).],
    bottomrule,
  ),
)<tab:stdlib-modules>

#aod-paragraph("Derivation.")
Each module in #tabref("tab:stdlib-modules") binds directly to canonical Main equations (#aod-cite("aod-manual-v41")):
1. *D.E.C. Ledger (`01_dec_ledger.dec`):* Implements normalized kernels, blocked hinge slide, window-clipped
   tunnelling (#mainequation("main:eq:window-clipped-rho")), SADAR orientation antisymmetry
   (#mainequation("main:eq:sadar-orientation-antisymmetry")), and shedding surplus (#mainequation("main:eq:shedding-excess")).
2. *Closure Wavelets and Exact Cyclotomic DFT (`02_closure_wavelets.dec`):* Instantiates return identity
   (#mainequation("main:eq:return-identity")), beat reclosure (#mainequation("main:eq:beat-reclosure")),
   phase windows (#mainequation("main:eq:closure-phase-window")), wave identity (#mainequation("main:eq:wave-identity")),
   duon current (#mainequation("main:eq:duon-current")), branch-hinge RD (#mainequation("main:eq:branch-hinge-rd")),
   same-current duad seating (#mainequation("main:eq:duad-same-current")), continuation and overlap dedup
   (#mainequation("main:eq:aod-wavelet-continuation"), #mainequation("main:eq:wavelet-overlap-dedup")),
   cancellation retention (#mainequation("main:eq:wavelet-cancellation-retention")), cadence ratio and
   reference timing (#mainequation("main:eq:cadence-ratio"), #mainequation("main:eq:wavelet-reference-timing")),
   and recipient event lattice (#mainequation("main:eq:recipient-event-lattice")). Its exact $ZZ_(12)$ DFT
   computes in $QQ[x]/(x^4 - x^2 + 1)$ without floats, yielding $hat(v)(3) = 1/4$ and Parseval sum $1/4$
   on pulse `{0, 4, 8}`.
3. *Boundary Currents and Double-Slit Cross-Return (`03_short_window_boundary.dec`):* Computes the 11-bin
   ring-gate tally (#mainequation("main:eq:frg-cross-return-contribution"), #mainequation("main:eq:frg-identity"))
   rendered in #figref("fig:double-slit-delta3"), alongside moving-boundary flux (#mainequation("main:eq:moving-boundary"))
   and scoped Stokes cancellation (#mainequation("main:eq:b-scoped-stokes")). The `3:3:7_yro` saddle fixture
   (`Pi=30`, `RD=61`, `P^D=21`, `Q^D=162`, `+3/-3`) stays strictly native with zero external mass assignment.

#figure(
  fig-double-slit-delta3(),
  caption: [Pure-vector plot of the 11-bin Dimonhexon double-slit cross-return fixture loaded directly from `/docs/manual/data/double_slit_delta3.json` and live-verified against `03_short_window_boundary.dec`.],
)<fig:double-slit-delta3>

4. *Cell Measures and Discrete-Law Moments (`04_cell_measures_tesseract.dec`):* Instantiates the M3 cubical
   complex (#mainsection("main:sec:m3-cells"), #mainequation("main:eq:m3-support-measure"),
   #mainequation("main:eq:m3-cubical-boundary")), computing quarter-symmetry unit-disk enclosures at
   $n in \{1, 2, 4, 8, 16\}$ (#figref("fig:disk-enclosure")) where axial perimeter $C_n = 8$ remains fixed
   while $[A_n^-, A_n^+]$ tightens to $[183/64, 107/32]$. Exact finite-law moments (#mainequation("main:eq:m3-finite-law"))
   and linear covariance diagnostics (#mainequation("main:eq:m3-linear-covariance")) confirm exact rational
   means and variances for $T$, $F$, $R f$, and $U$.

#figure(
  fig-disk-enclosure(),
  caption: [Pure-vector convergence of exact rational unit-disk enclosures $[A_n^-, A_n^+]$ against constant axial perimeter $C_n = 8$, rendered from `/docs/manual/data/disk_enclosure.json`.],
)<fig:disk-enclosure>

5. *Conditional SI Metrology (`05_conditional_si_metrology.dec`):* Keeps native turn advances and the
   atomic-gain identifiability family (#mainsection("main:prop:m3-atomic-gain")) separate from the quarantined
   `SiMetrology` submodule holding $Delta nu_"Cs" = 9192631770$, Hellwig H-maser reductions
   (#aod-cite("hellwig-hydrogen-1970")), and separate NIST-F2 Aug 2013 vs Table 1 uncertainty budgets
   (#aod-cite("heavner-nist-f2-2014, bipm-si-2019")).

An inline check against `stdlib::aod::ledger` confirms exact tunnelling clip preservation:

```dec
use stdlib::aod::ledger::{block_dwell_slide, tunnel_clip}

Dec main() -> int:
    let slide = block_dwell_slide(3, 3, 1)
    let tun = tunnel_clip(5, 2, 3, slide.tunnel_prob)
    emit tun.rd_unclipped
    emit tun.tunnel_contrib
    return 0
```

```dec-expect
5
6/7
0
```

#aod-paragraph("Falsification.")
All five executable Manual fixtures (`01`..`05`) are live-verified across Exact VM, AOT `-O2`, and ORC LLJIT
`-O2` against their frozen `.stdout` files and live JSON data invariants:

// @dec-file: manual_examples/01_dec_ledger.dec
// @dec-file: manual_examples/02_closure_wavelets.dec
// @dec-file: manual_examples/03_short_window_boundary.dec
// @dec-file: manual_examples/04_cell_measures_tesseract.dec
// @dec-file: manual_examples/05_conditional_si_metrology.dec
