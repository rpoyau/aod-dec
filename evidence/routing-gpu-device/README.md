# NVIDIA hardware validation

This run executes the existing application backend on the laptop's NVIDIA RTX
A1000 6GB GPU through direct host access. The sandboxed Driver API had reported
`CUDA_ERROR_NO_DEVICE`; `nvidia-smi` also failed inside the sandbox. A read-only
host probe found the GPU, after which the existing CUDA tests and required-GPU
study succeeded. No driver, system configuration or implementation was changed.

The executed source commit is `2bcce6cf9b833b0da0945054a0b3435f16895ad2`.
The source-bound Main/Manual review remains in `../routing-parallel/`; its
definitions and declared model are unchanged. This addition establishes device
execution and exact finite output comparisons, not a new scientific model.

## Recorded results

- GPU: NVIDIA RTX A1000 6GB Laptop GPU, 6,144 MiB, compute capability 8.6;
  NVIDIA driver package 620.70.06. The CUDA Driver API version integer is 13050.
- NVRTC 12.9 targets the actual device as `compute_86`. The study's separate
  offline compiler check still targets `compute_75`. `chiral_batch.ptx` belongs
  to that offline check; `chiral_batch-compute_86.ptx` matches the compilation
  hash recorded by the actual device execution.
- The 11-test CUDA suite passes normally and under Python `-O`, with no skips.
  Actual device tests compare supplied-model outputs and uint32 threshold/
  equality boundaries. Other tests cover compilation, validation and cleanup.
- The `--gpu required` study executes 256 paths, eight updates each. All 2,048
  GPU candidate rows equal both CPU backends and match the prior retained cohort.
  Model-table, ticket and semantic hashes agree across backends.
- All 16 complete host traces agree between scalar and four-process execution;
  their 128 transitions join directly to the supplied candidate records.

The GPU executes candidate transitions. Full return, preparation and route
witness construction/admission remain on the CPU. The retained timing includes
NVRTC compilation, allocation, transfers, kernel execution and cleanup; it is
not a kernel-only benchmark or a general speedup claim. Physical proton
preparation, collision dynamics and SI observation correspondence remain unset.

## Reproduction

Run in an environment with direct access to the NVIDIA device and the installed
CUDA Driver API/NVRTC libraries:

```sh
nvidia-smi --query-gpu=name,driver_version,memory.total,compute_cap --format=csv
python3 -m unittest tests.test_chiral_gpu -v
python3 -O -m unittest tests.test_chiral_gpu -v
python3 scripts/simulate_routing_parallel.py --output build/routing-gpu-device \
  --workers 4 --paths 256 --traced-paths 16 --gpu required --plots
typst compile --root . --creation-timestamp 1794528000 \
  --input results=../evidence/routing-gpu-device/results.json \
  --input chart=../evidence/routing-gpu-device/routing-parallel.png \
  docs/routing-parallel.typ build/routing-gpu-device/routing-gpu-device.pdf
```

The PDF uses the same Typst source with this run's actual status and timing data.
The previous CPU/sandbox PDF and evidence are preserved separately. GPU rows
are in `results.json`; supplied inputs and CPU rows are in
`candidate-records.json.gz`; complete host witnesses are in `traces.json.gz`.
`validation.json` binds their encoded/decoded bytes and the inspected PDF.
