# Radial resolution amendment for the inferred 3D fit

The initial uniform radial mesh extended to every sampled positive pixel. Its NGC3198 intensity fit concentrated all mass in one broad basis. The run was stopped and preserved by the parent. This is a declared numerical representation failure, not evidence that NGC3198 has one homogeneous radial component.

I independently loaded both original moment maps with the observation adapter and computed positive-intensity-weighted radius quantiles using the intensity-moment seed center/position angle and the upper catalog inclination. Exact values, rules and source bindings are in `radial-resolution-data.json`. The nonzero observational masks remain intact throughout this diagnostic.

| Galaxy | R99.5%, full source / arcsec | R99.9%, full source / arcsec | Furthest positive sample / arcsec |
|---|---:|---:|---:|
| NGC2403 | 1639.523049 | 1775.207375 | 3080.682155 |
| NGC3198 | 787.878857 | 6873.148323 | 11318.102239 |

The stride-four values show the same distinction: NGC3198 R99.5%=787.532459 arcsec, R99.9%=6869.668193 arcsec. The jump is observed in the input; it is not an optimizer result. Therefore a 99.9% concentration rule would still spread the dense radial grid over the distant component and fail to resolve the principal emission.

Use the fixed **99.5% intensity-concentration radius plus one beam** for dense inner radial knots, and retain outer knots through the full declared endpoint. This is a data-adaptive resolution rule for an explicitly in-sample inverse fit. It is not a physical galaxy boundary, a confidence region or permission to discard the remaining 0.5% of positive intensity. All nonzero finite observations—including negative noise—remain in their declared loss masks. Keep the fixed intrinsic mass normalization and record the exact final nonuniform knots. A fallback to a strictly increasing grid is necessary if the selected concentration radius reaches the endpoint.

The primary [NGC3198 study, section 3](https://arxiv.org/html/1304.4232v1#S3) reports a distinct HI galaxy pair at approximately 120 kpc. That report is a reason not to assign every distant map feature to the main galaxy; the present radial-quantile calculation does not independently identify which pixels belong to that pair. Describe distant fitted components as unclassified observed support. Their possible outer basis weights do not establish a galactic halo, and residuals there remain valid model discrepancy.

A uniform 65-cube visualization over the full several-hundred-kpc deprojected support would erase the newly resolved central structure. Export a dense inner Cartesian view over the concentration radius, retain the full parametric density/knots and outer mass accounting, and identify the display crop explicitly. This changes display resolution, not the data-support accounting.

The existing independent nonuniform-knot area control already exercises the required cylindrical normalization; no new implementation-mirroring test is needed solely for a recorded knot-placement policy. The 11 model controls remain relevant. Actual post-change fit residuals, quadrature/spatial sensitivity and any concentration/basis sensitivity still determine the adequacy of the evaluated candidate. This review approves the scoped numerical amendment, not a unique recovered density or time-evolution claim.
