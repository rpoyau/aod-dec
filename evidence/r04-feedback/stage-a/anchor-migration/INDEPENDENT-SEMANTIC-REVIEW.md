# Independent review of the formation-migration comparison

14 November 2026. Read-only source and artifact review; no checker tests or
simulation replay were executed for this review. No implementation was edited.

Bound checker SHA-256:
`e34afad1e4736f278160524d1cf5e67ffa047755921f05a63a2707af859efffd`.
Final comparison SHA-256:
`bec09c25637bc2a6e5b3ba1c886d623e75d958a6a94e0f0079dd247b3a68014a`.
Preserved initial failed comparison SHA-256:
`a97ce198cf7109b4fdccb66004c0db90c8b15face1b38a9bceca8fe53b51a0d6`.
The archived `baseline/input/population.py` read directly from the bound original
archive has SHA-256
`240a517cad639beb4afc919ef77b7ea973e4d7960092c4325d092c3e3cdf631f`.

The new exception is narrowly justified. Archived source lines 290–291 log
`[cut, "certified_reclosure", node, occurrence, target_return_id,
motif_witness_digest]`. Formation namespace migration legitimately changes the
last field because the motif witness includes component formation identities.
It does not change the actual target return or its execution cut.

`compare_semantics.py:87–114` accepts that last-field difference only for this
six-field event schema. It compares the first five fields with exact typed
equality, identifies the specific node and occurrence in both wave archives,
checks the recorded target cut and return identity against that motif, checks
each logged witness digest against its corresponding motif, and requires the
old-to-new digest map already established by the wave/formation comparison.
All other event rows remain strictly compared. The formation comparison at
lines 46–85 removes only the declared namespace from component event references
and checks the remaining geometry, indices, roles and certificate payloads.
Execution field membership and other complete-execution values are checked at
lines 162–183. No numerical, target or schedule exception was added.

The preserved failure lists 597, 464, 686 and 649 event-digest differences across
the four cases. The final report classifies exactly those counts as specific
reclosure motif references and records zero unexpected differences in every
case; both auxiliary inputs/report comparisons pass. The reviewed final report
binds the current checker hash, unlike the preserved initial failure.

Scope qualification: this is semantic comparison with declared provenance
exceptions, not independent cryptographic re-admission of every regenerated
digest. Lines 138–142 permit consistent source/context hash substitutions by
field convention rather than recomputing every referent. The top-level case
loop at lines 188–189 compares existing keys but does not itself reject newly
added top-level case fields; complete-execution field membership is explicitly
checked. These bounds do not undermine the specific event exception or the
reported equality of compared numerical/target/schedule values. Avoid expanding
the result into a claim of universal archive authentication or unrestricted
schema equivalence. No substantive defect was found in the new event-reference
exception.
