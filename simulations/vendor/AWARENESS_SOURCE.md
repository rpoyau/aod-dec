# Bound awareness fixture

`aod_awareness.py` is copied byte for byte from
`manual/scripts/aod_awareness.py` at Main/Manual commit
`ab8359209b8f9b2ef4007403101016e3ea58cfe4`.

- Awareness source SHA-256: `6846300c020aca4af2c7fc51c0727dcd0ce7e302453e0a514904ce8aa5331d7b`.
- Original native dependency SHA-256: `562b3eb53ad6dd13c0801c43bd6e918110083ab4d12592c024b3007b634e2554`.
- Bound downstream native dependency SHA-256: `918c6c0bd242e7ed0bff6f4d1a4ff7bf2cc840b3da41b6888c218d34eddf5be0`.

The downstream dependency differs only by the existing R07 exact-integer
chirality, Dion and pivot admission checks. Its response, recurrence, contact
and FRG implementations are unchanged. `awareness_jit_fixture.load_manual`
binds this dependency through a module-local import function; it does not
replace the global `aod_native_closure_wave` module name. The original script
is unchanged. The upstream MIT notice is retained in [LICENSE](LICENSE).
