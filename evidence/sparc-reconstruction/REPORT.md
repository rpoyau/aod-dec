# SPARC reconstruction result

The 5-galaxy, 209-bin stellar reconstruction and its analytic column projection ran. The stellar profile is constrained by the input data. It does not supply native return histories. AOD evolution of these galaxies has not run.

| Comparison | Equal-bin square-speed MAPE | Equal-galaxy MAPE |
|---|---:|---:|
| Frozen G0 control | 26.570553423860% | 28.440531838479% |
| Target-conditioned scalar diagnostic | 20.270217992959% | 18.509468393497% |

The second row fits one effective factor per galaxy to these same targets. It is an inverse diagnostic; its improvement is not a G2/G3 prediction or evidence that a recovered density produced that current.

The vertical scale is estimated from catalog disk size using the SPARC empirical relation. The full exponential vertical tails remain accounted for. Positive heights produce the same column, so this projected profile alone cannot identify thickness.

Native beat separations, windows and scale ranges are computed from certified return histories by `extract_native_span`. With no galaxy histories, that output remains unavailable. Physical radii, mass density and cadence are kept as distinct quantities.

Exact rational values and all signed inverse requirements are in results.json and required-current.csv. Unknown central/gas/bulge volume density and unsupplied physical uncertainty are retained as unknown.
