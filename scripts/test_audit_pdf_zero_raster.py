#!/usr/bin/env python3
"""Unit tests for scripts/audit_pdf_zero_raster.py (passes under both `python3` and `python3 -O`)."""

from __future__ import annotations

import sys
import tempfile
import unittest
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import audit_pdf_zero_raster as auditor


def make_synthetic_pdf(
    pages: int = 2,
    extra_uncompressed: bytes = b"",
    flate_streams: list[bytes] | None = None,
) -> bytes:
    parts = [b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n"]
    obj_id = 1
    for _ in range(pages):
        parts.append(
            f"{obj_id} 0 obj\n<< /Type /Page /Parent 99 0 R /Resources << /ProcSet [/PDF /Text] >> >>\nendobj\n".encode(
                "ascii"
            )
        )
        obj_id += 1
    if extra_uncompressed:
        parts.append(
            f"{obj_id} 0 obj\n".encode("ascii") + extra_uncompressed + b"\nendobj\n"
        )
        obj_id += 1
    for payload in flate_streams or []:
        comp = zlib.compress(payload)
        parts.append(
            f"{obj_id} 0 obj\n<< /Filter /FlateDecode /Length {len(comp)} >>\nstream\n".encode(
                "ascii"
            )
            + comp
            + b"\nendstream\nendobj\n"
        )
        obj_id += 1
    parts.append(b"%%EOF\n")
    return b"".join(parts)


class AuditPdfZeroRasterTests(unittest.TestCase):
    def test_clean_vector_pdf_and_string_literal_no_false_positive(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "clean.pdf"
            content = b"BT /F1 11 Tf (/Subtype /Image is forbidden in prose) Tj ET"
            p.write_bytes(make_synthetic_pdf(pages=6, flate_streams=[content]))
            ok, errs, summary = auditor.audit_pdf(p, min_pages=6)
            self.assertTrue(ok, f"expected PASS, got errors: {errs}")
            self.assertEqual(summary["pages"], 6)
            self.assertEqual(summary["raster_violations"], 0)
            self.assertEqual(summary["unresolved_violations"], 0)

    def test_uncompressed_subtype_image_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "img.pdf"
            p.write_bytes(
                make_synthetic_pdf(
                    pages=2,
                    extra_uncompressed=b"<< /Type /XObject /Subtype /Image /Width 8 /Height 8 >>",
                )
            )
            ok, errs, summary = auditor.audit_pdf(p, min_pages=1)
            self.assertFalse(ok)
            self.assertGreater(summary["raster_violations"], 0)
            self.assertTrue(any("/Subtype /Image" in e for e in errs))

    def test_flate_objstm_hidden_subtype_image_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "objstm_img.pdf"
            objstm = b"10 0 << /Type /XObject /Subtype /Image /Width 16 /Height 16 >>"
            p.write_bytes(make_synthetic_pdf(pages=2, flate_streams=[objstm]))
            ok, errs, summary = auditor.audit_pdf(p, min_pages=1)
            self.assertFalse(ok)
            self.assertGreater(summary["raster_violations"], 0)
            self.assertTrue(any("/Subtype /Image" in e for e in errs))

    def test_flate_inline_image_rejected_and_boilerplate_procset_allowed(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            p1 = Path(td) / "inline.pdf"
            inline_stream = b"q BI /W 2 /H 2 /BPC 8 /CS /G ID \x00\xff\xff\x00 EI Q"
            p1.write_bytes(make_synthetic_pdf(pages=2, flate_streams=[inline_stream]))
            ok1, errs1, _ = auditor.audit_pdf(p1, min_pages=1)
            self.assertFalse(ok1)
            self.assertTrue(any("inline raster" in e for e in errs1))

            # Typst 0.15.1 (pdf-writer) emits the deprecated ISO 32000-1 /ProcSet boilerplate
            # array even on pure-vector PDFs with zero /Subtype /Image XObjects.
            p2 = Path(td) / "procset.pdf"
            p2.write_bytes(
                make_synthetic_pdf(
                    pages=2,
                    extra_uncompressed=b"<< /ProcSet [/PDF /Text /ImageB /ImageC /ImageI] >>",
                )
            )
            ok2, errs2, summary2 = auditor.audit_pdf(p2, min_pages=1)
            self.assertTrue(ok2, f"boilerplate /ProcSet without /Subtype /Image must pass, got {errs2}")
            self.assertEqual(summary2["raster_violations"], 0)

    def test_unresolved_reference_markers_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            for idx, marker in enumerate(
                [b"(See ??)", b"(Unresolved Main reference: main:eq:missing)", b"(Unresolved citation: bad)"]
            ):
                p = Path(td) / f"unres_{idx}.pdf"
                p.write_bytes(make_synthetic_pdf(pages=2, flate_streams=[b"BT " + marker + b" Tj ET"]))
                ok, errs, summary = auditor.audit_pdf(p, min_pages=1)
                self.assertFalse(ok, f"marker {marker!r} must fail")
                self.assertGreater(summary["unresolved_violations"], 0)
                self.assertTrue(any("unresolved" in e for e in errs))

    def test_min_pages_and_determinism_comparison(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            p1 = Path(td) / "a.pdf"
            p2 = Path(td) / "b.pdf"
            p3 = Path(td) / "c.pdf"
            raw = make_synthetic_pdf(pages=3)
            p1.write_bytes(raw)
            p2.write_bytes(raw)
            p3.write_bytes(raw + b" ")

            ok_short, errs_short, _ = auditor.audit_pdf(p1, min_pages=6)
            self.assertFalse(ok_short)
            self.assertTrue(any("page count" in e for e in errs_short))

            ok_same, _, _ = auditor.audit_pdf(p1, min_pages=3, compare_pdf=p2)
            self.assertTrue(ok_same)

            ok_diff, errs_diff, _ = auditor.audit_pdf(p1, min_pages=3, compare_pdf=p3)
            self.assertFalse(ok_diff)
            self.assertTrue(any("non-deterministic" in e for e in errs_diff))


if __name__ == "__main__":
    unittest.main()
