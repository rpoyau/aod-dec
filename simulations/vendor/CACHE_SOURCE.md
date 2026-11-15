# Native trace digest cache

`aod_trace_digest_cache.py` is copied byte for byte from
`rpoyau/aod-manual`, commit `ab8359209b8f9b2ef4007403101016e3ea58cfe4`,
path `manual/scripts/aod_trace_digest_cache.py`.

SHA-256: `0d36febb309f6366d12da54955010e94124581739e0da70660e19192a60eac14`.
The accompanying `LICENSE` preserves the upstream MIT notice.

The optional context caches only trace digests using bounded, typed value keys.
It retains the native validators and restores the original descriptor on exit.
The reduced application tests cache-on/cache-off result equality, malformed
history rejection, key/residency limits and restoration after exceptions.
This is a separate import from the previously retained native producer; that
producer's bytes and historical provenance remain unchanged.
