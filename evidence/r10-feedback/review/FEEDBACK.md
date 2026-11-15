# PR #7 R10 fix feedback

Target head: 1edf97b710c0b1622db746ccd8b8638415a7e210.
Finding R10-01 (P2), also called FRONTIER-01 in the focused report.

## Required correction

In simulations/solar_boundary_fixture.py:460, derive declared_routed_current
from the channel already reconstructed and matched to the committed channel
family. The current assignment Q(2) is the original fixture's expected answer,
not the result of every history the inspector admits.

The existing code at lines 428–440 already provides the exact authoritative
channel. Publish its value after all admission checks complete. Preserve
current_unit, archive and history-union validation, pending unsigned surplus,
and the original first-error/partial-coverage behavior.

No new transition law, SI conversion, source fixture or physical premise is
required for this repair.

## Regression acceptance

Use checks/frontier/probe_solar_current.py with fresh normal/-O output paths.
It constructs and reopens real checksummed archives; do not replace the
inspector, decoder or comparator with canned values.

- Original control: route completion cuts 27, 30, 33; receiver 27, 30, 33; lag 1;
  committed and reported current 2; verified complete coverage.
- Alternate lawful history: route 27, 33, 39; receiver 24, 27, 30, 33, 36, 39; lag 0;
  committed and reported current 0; verified complete coverage.
- Preserve load 27, cost 24, unsigned surplus 3 and its unresolved projection;
  do not turn unsigned surplus into signed current.
- Preserve the actual source pivot, folded histories and whole-return flips.
- Maintain rejection/partial coverage for missing or conflicting evidence.
- Add the lawful alternate case to the regression suite, not only a test that
  directly mirrors the replacement expression.

Run affected solar/parallel trace tests under normal and optimized Python,
then the full supported workflow. Rebind checksums/evidence to the final
revision. Retain the original [4,-3,0,7,6] fixture and its stated scope.

If the intended API is restricted to exactly one fixed preparation, express
and test that restriction explicitly; it cannot accept the alternate history
as verified and then return a substituted constant.

## Nonblocking owner preference

The new reconstruction API/JSON uses mass_over_pi. Align any future public
Tau naming with the user's preference while preserving normalization:
M/pi = 2(M/tau). Do not silently rename the old coefficient to mass_over_tau
without changing its value or providing a documented compatibility mapping.

## Delivery

This is review feedback, not an applied patch. No repository/GitHub source
was changed. Current CI passes despite the counterexample, so its green
status alone is not the acceptance check for R10-01.
