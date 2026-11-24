# AOD / AFC `Dec` Self-Hosting Compiler & Hardware-Learning Kit

Quickstart on bare-metal Linux (`x86_64` or `aarch64`, zero external dependencies):

```bash
unzip aod-dec-dev.zip
cd aod-dec-dev
sha256sum -c SHA256SUMS
make verify
```

## Documentation

- `docs/FIRST_PRINCIPLES_DERIVATION.md` — Full step-by-step first-principles derivation showing how explicit signature combinatorics, register encodings, Landauer erasure avoidance, loop memory, double-slit thread concurrency, balanced ternary trits, and exact ratios converge directly onto AFC & Alpha-Omega Dynamics across CPU, Cache, RAM, SSD, GPU, and Unified Memory.
- `docs/SPECIFICATION.md` — Architectural reference and module index for the `Dec` language and Stage-0/Stage-1/Stage-2 ELF64 bootstrap pipeline.
