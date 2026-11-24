#!/usr/bin/env python3
"""Stdlib-only zero-raster, unresolved-reference, page-count, and determinism auditor for Typst PDFs.

Zero bare `assert` statements so every check remains active under `python3 -O` (`P1-1`).
"""

from __future__ import annotations

import argparse
import hashlib
import re
import sys
import zlib
from pathlib import Path

STREAM_RE = re.compile(rb"stream\r?\n(.*?)\r?\n?endstream", re.DOTALL)
SUBTYPE_IMAGE_RE = re.compile(rb"/Subtype\s*/Image(?![A-Za-z0-9_])")
INLINE_IMAGE_RE = re.compile(
    rb"(?<![A-Za-z0-9_/])BI\s.*?[\s/]ID[\s\r\n].*?(?<![A-Za-z0-9_/])EI(?![A-Za-z0-9_])",
    re.DOTALL,
)
PAGE_OBJ_RE = re.compile(rb"/Type\s*/Page(?![A-Za-z0-9_])")
PAGES_COUNT_RE = re.compile(rb"/Type\s*/Pages\b[^>]{0,200}/Count\s+(\d+)", re.DOTALL)

UNRESOLVED_MARKERS = (
    "Unresolved Main reference",
    "Unresolved internal reference",
    "Unresolved citation",
    "[unresolved]",
    "PANIC",
)


def strip_pdf_strings(data: bytes) -> bytes:
    """Replace PDF literal string `(...)` and hex string `<...>` contents with spaces.

    Preserves dictionary delimiters `<< ... >>` while preventing prose text like
    `(/Subtype /Image is forbidden)` from triggering false-positive raster detections.
    """
    out = bytearray()
    n = len(data)
    i = 0
    while i < n:
        b = data[i]
        if b == 0x28:  # '(' literal string with nested parens and escapes
            depth = 1
            out.append(0x20)
            i += 1
            while i < n and depth > 0:
                c = data[i]
                if c == 0x5C and i + 1 < n:  # backslash escape
                    i += 2
                    continue
                if c == 0x28:
                    depth += 1
                elif c == 0x29:
                    depth -= 1
                i += 1
            continue
        if b == 0x3C:  # '<'
            if i + 1 < n and data[i + 1] == 0x3C:  # '<<' dict start
                out.extend(b"<<")
                i += 2
                continue
            # Single '<...>' hex string
            out.append(0x20)
            i += 1
            while i < n and data[i] != 0x3E:
                i += 1
            if i < n:
                i += 1
            continue
        out.append(b)
        i += 1
    return bytes(out)


def extract_pdf_literal_strings(data: bytes) -> list[str]:
    """Extract decoded literal string contents `(...)` for unresolved-reference marker checks."""
    found: list[str] = []
    n = len(data)
    i = 0
    while i < n:
        if data[i] == 0x28:  # '('
            depth = 1
            i += 1
            buf = bytearray()
            while i < n and depth > 0:
                c = data[i]
                if c == 0x5C and i + 1 < n:
                    buf.append(data[i + 1])
                    i += 2
                    continue
                if c == 0x28:
                    depth += 1
                    buf.append(c)
                elif c == 0x29:
                    depth -= 1
                    if depth > 0:
                        buf.append(c)
                else:
                    buf.append(c)
                i += 1
            found.append(buf.decode("latin-1", errors="ignore"))
        else:
            i += 1
    return found


def extract_decompressed_streams(raw: bytes) -> list[bytes]:
    """Return all raw stream payloads plus zlib-decompressed payloads for `/FlateDecode` streams."""
    streams: list[bytes] = []
    for match in STREAM_RE.finditer(raw):
        payload = match.group(1)
        streams.append(payload)
        for cand in (payload, payload.strip(b"\r\n")):
            try:
                dec = zlib.decompress(cand)
                streams.append(dec)
                break
            except zlib.error:
                continue
    return streams


def count_pdf_pages(raw: bytes, streams: list[bytes]) -> int:
    """Count PDF pages across uncompressed objects and decompressed `/ObjStm` object streams."""
    total = len(PAGE_OBJ_RE.findall(strip_pdf_strings(raw)))
    for s in streams:
        total += len(PAGE_OBJ_RE.findall(strip_pdf_strings(s)))
    if total > 0:
        return total
    best = 0
    for blob in [raw, *streams]:
        for m in PAGES_COUNT_RE.finditer(blob):
            best = max(best, int(m.group(1)))
    return best


def audit_pdf(
    pdf_path: Path,
    min_pages: int = 1,
    compare_pdf: Path | None = None,
) -> tuple[bool, list[str], dict[str, object]]:
    errors: list[str] = []
    if not pdf_path.is_file():
        return False, [f"PDF file not found: {pdf_path}"], {}

    raw = pdf_path.read_bytes()
    if not raw.startswith(b"%PDF-"):
        errors.append(f"{pdf_path}: missing %PDF- header")

    sha = hashlib.sha256(raw).hexdigest()
    streams = extract_decompressed_streams(raw)
    all_blobs = [raw, *streams]

    raster_hits: list[str] = []
    for idx, blob in enumerate(all_blobs):
        stripped = strip_pdf_strings(blob)
        tag = "root" if idx == 0 else f"stream[{idx}]"
        if SUBTYPE_IMAGE_RE.search(stripped):
            raster_hits.append(f"{tag}: /Subtype /Image XObject detected")
        if idx > 0 and INLINE_IMAGE_RE.search(stripped):
            raster_hits.append(f"{tag}: inline raster image BI..ID..EI detected")

    for hit in raster_hits:
        errors.append(f"raster violation: {hit}")

    # Check unresolved-reference markers across literal strings & stream text
    unresolved_hits: list[str] = []
    for idx, blob in enumerate(all_blobs):
        tag = "root" if idx == 0 else f"stream[{idx}]"
        for lit in extract_pdf_literal_strings(blob):
            for marker in UNRESOLVED_MARKERS:
                if marker in lit:
                    unresolved_hits.append(f"{tag}: unresolved marker {marker!r} in text")
            if "??" in lit:
                unresolved_hits.append(f"{tag}: unresolved marker '??' in text")
        if idx > 0:
            decoded = blob.decode("latin-1", errors="ignore")
            for marker in UNRESOLVED_MARKERS:
                if marker in decoded and not any(marker in h for h in unresolved_hits):
                    unresolved_hits.append(f"{tag}: unresolved marker {marker!r}")

    for hit in unresolved_hits:
        errors.append(f"unresolved reference violation: {hit}")

    pages = count_pdf_pages(raw, streams)
    if pages < min_pages:
        errors.append(f"page count {pages} < required --min-pages {min_pages}")

    compare_sha: str | None = None
    if compare_pdf is not None:
        if not compare_pdf.is_file():
            errors.append(f"--compare-pdf file not found: {compare_pdf}")
        else:
            compare_sha = hashlib.sha256(compare_pdf.read_bytes()).hexdigest()
            if compare_sha != sha:
                errors.append(
                    f"non-deterministic PDF build: {pdf_path} ({sha}) != {compare_pdf} ({compare_sha})"
                )

    summary: dict[str, object] = {
        "pdf": str(pdf_path),
        "sha256": sha,
        "compare_sha256": compare_sha,
        "pages": pages,
        "streams_inspected": len(streams),
        "raster_violations": len(raster_hits),
        "unresolved_violations": len(unresolved_hits),
        "status": "PASS" if not errors else "FAIL",
    }
    return len(errors) == 0, errors, summary


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Audit PDF for zero raster images, zero unresolved refs, page count, and byte determinism."
    )
    parser.add_argument("pdf", type=Path, help="Path to PDF file to audit")
    parser.add_argument(
        "--min-pages", type=int, default=1, help="Minimum required page count (default: 1)"
    )
    parser.add_argument(
        "--compare-pdf",
        type=Path,
        default=None,
        help="Optional second PDF build path to verify byte-identical SHA-256 determinism",
    )
    args = parser.parse_args(argv)

    ok, errors, summary = audit_pdf(
        args.pdf, min_pages=args.min_pages, compare_pdf=args.compare_pdf
    )
    print(
        f"audit_pdf_zero_raster: {summary.get('status', 'FAIL')} "
        f"(pages={summary.get('pages', 0)}, streams={summary.get('streams_inspected', 0)}, "
        f"rasters={summary.get('raster_violations', 0)}, sha256={summary.get('sha256', '')})"
    )
    for err in errors:
        print(f"ERROR: {err}", file=sys.stderr)
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
