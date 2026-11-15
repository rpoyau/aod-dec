# AOD Dec PR #7 — R10 review and feedback bundle

Start with REVIEW.md, FEEDBACK.md and FINDINGS.json.

Reviewed head: 1edf97b710c0b1622db746ccd8b8638415a7e210.
Reviewed stacked base/unchanged PR6: dbf79dcc14d5a36a68969212c617bb8d486fee2d.
Result: one P2 reporting defect, R10-01. Source/GitHub unchanged.

The decisive probe is checks/frontier/probe_solar_current.py. Complete original
and alternative lawful TraceStore archives are included, with independent
normal/optimized parent reruns. The inspector validates committed current 0
but publishes constant 2; the original current 2 control remains correct.

Contents:
- checks/: six focused reviews, independent probes, complete small witnesses,
  exact oracle records and raw test/audit logs.
- evidence/: complete Git-tree/PR/source bindings, raw workflow logs,
  artifact identities and upstream awareness source binding.
- evidence/publisher-device/: source-bound retained current device records.
- ci_artifacts/: five original downloaded current CI ZIPs under 1.7 MB each.
- ci_selected/: current full-cohort SPARC summaries, comparison CSV and
  validation/oracle records, without duplicating its 29 MB compressed traces.
- prior-review/: R09 report/findings for continuity.
- SHA256SUMS: all bundle members except the manifest itself.

Reproduction:
Place the exact reviewed repository snapshot at source/ adjacent to checks/.
The externally git-backed repository is not duplicated in this review bundle.
Probes and reports state exact commands and import/source bindings.

For current CI audits, extract each included ci_artifacts/NAME.zip into
ci_artifacts/NAME/. The complete SPARC audits additionally need the two original
archives referenced below; both were downloaded, digest-checked and inspected
during this review but excluded here to keep the feedback download compact.

Normal full SPARC artifact:
https://github.com/rpoyau/aod-dec/actions/runs/43728633037/artifacts/14167397692
SHA-256: 303346868d002b748bbf845aa39eb6ed0cb275ed436bbbdb7650d6966b295a93

Optimized full SPARC artifact:
https://github.com/rpoyau/aod-dec/actions/runs/43728633037/artifacts/14167393786
SHA-256: d4950221788d7d99e950e83477c90de6842629fd6f82586cb63a41140efd1b9f

Extract those into ci_artifacts/sparc-reduced-normal/ and
ci_artifacts/sparc-reduced-optimized/. Publisher device files normally live at
source/evidence/introspection-evolution/device/; the evidence/publisher-device/
copy in this bundle preserves their bytes if you need to restore that path.

Current full CI is green. Its exact method accounting is 771 discovered =
758 passes + 8 individual skips + 5 methods blocked by one class setup skip,
per Python mode. See checks/ci/REVIEW.md for the raw-summary distinction.
The solar inspector counterexample is independent of those fixed examples.

Verify ZIP:
    python -m zipfile -t AOD_DEC_PR7_REVIEW_R10.zip
After extraction:
    sha256sum -c SHA256SUMS
