#!/usr/bin/env bash
set -euo pipefail

echo "=== Building Deterministic AOD/AFC Bootstrap Pipeline ==="
make verify
echo "=== All Stage-0 / Stage-1 / ELF64 Ring-Gate Verifications Passed ==="
