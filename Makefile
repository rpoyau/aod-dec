PYTHON ?= python3
RUSTC ?= rustc
TYPST ?= typst
RUSTFLAGS := -C opt-level=3 -C debuginfo=0 -C strip=symbols -C codegen-units=1
BUILD_DIR := build

.PHONY: all verify verify-python verify-python-optimized verify-rust verify-native verify-ci docs docs-native simulate-cells docs-cells simulate-chiral docs-chiral simulate-routing docs-routing examples clean
.PHONY: verify-retained docs-retained
.PHONY: simulate-regions simulate-particles verify-region-current docs-scoped
all: $(BUILD_DIR)/aod_stage0_seed $(BUILD_DIR)/aod_self_host_compiler $(BUILD_DIR)/aod_stage2_bootstrap

verify-retained:
	$(PYTHON) scripts/check_retained_continuation.py --output $(BUILD_DIR)/retained-continuation

docs-retained:
	mkdir -p $(BUILD_DIR)/retained-continuation
	$(TYPST) compile --root . docs/retained-continuation.typ $(BUILD_DIR)/retained-continuation/retained-continuation.pdf

simulate-regions:
	$(PYTHON) -B simulations/run_regions.py --example all --output $(BUILD_DIR)/scoped-current/regions.json

simulate-particles:
	$(PYTHON) -B simulations/proton_accounting.py --output $(BUILD_DIR)/scoped-current/proton-accounting.json
	$(PYTHON) -B simulations/electron_wire_accounting.py --output $(BUILD_DIR)/scoped-current/electron-wire.json

verify-region-current:
	$(PYTHON) -B scripts/check_region_current.py --output $(BUILD_DIR)/scoped-current/dec

docs-scoped: docs-retained
	mkdir -p $(BUILD_DIR)/scoped-current/docs
	$(TYPST) compile --root . docs/boundary-accounting.typ $(BUILD_DIR)/scoped-current/docs/boundary-accounting.pdf
	$(TYPST) compile --root . docs/particle-examples.typ $(BUILD_DIR)/scoped-current/docs/particle-examples.pdf
	$(TYPST) compile --root . docs/region-current.typ $(BUILD_DIR)/scoped-current/docs/region-current.pdf

$(BUILD_DIR):
	mkdir -p $(BUILD_DIR)

$(BUILD_DIR)/%: bootstrap/%.rs $(wildcard bootstrap/*.rs) | $(BUILD_DIR)
	$(RUSTC) $(RUSTFLAGS) $< -o $@

verify-python:
	$(PYTHON) -m unittest discover -s tests -p 'test_*.py' -v

verify-python-optimized:
	$(PYTHON) -O -m unittest discover -s tests -p 'test_*.py' -v

# Includes reference/native Python tests, retained Rust checks, native witness
# tests and both exact differential corpora; all command logs are retained.
verify-native:
	$(PYTHON) scripts/verify_native.py --output $(BUILD_DIR)/verification

verify-ci: verify-native

# Compile the manual and execute its published Dec examples through the
# reference VM and typed IR. The native variant also checks AOT/JIT at O0/O2.
docs:
	$(PYTHON) scripts/build_docs.py --typst "$(TYPST)" --output $(BUILD_DIR)/docs

docs-native:
	$(PYTHON) scripts/build_docs.py --typst "$(TYPST)" --output $(BUILD_DIR)/docs --native

simulate-cells:
	$(PYTHON) scripts/simulate_cell_jitter.py --output $(BUILD_DIR)/cell-jitter

docs-cells:
	mkdir -p $(BUILD_DIR)/cell-jitter
	$(TYPST) compile --root . --creation-timestamp 1794528000 docs/tesseract-cell-jitter.typ $(BUILD_DIR)/cell-jitter/tesseract-cell-jitter.pdf

simulate-chiral:
	$(PYTHON) scripts/simulate_chiral_collision.py --plots --output $(BUILD_DIR)/chiral-collision

# The local companion uses retained evidence; CI compiles fresh study results.
docs-chiral:
	mkdir -p $(BUILD_DIR)/chiral-collision
	$(TYPST) compile --root . --creation-timestamp 1794528000 \
		--input results=../evidence/chiral-collision/results.json \
		--input chart=../evidence/chiral-collision/chiral-study.png \
		docs/chiral-core-collision.typ $(BUILD_DIR)/chiral-collision/chiral-core-collision.pdf

simulate-routing:
	$(PYTHON) scripts/simulate_routing_parallel.py --output $(BUILD_DIR)/routing-parallel \
		--workers 4 --paths 256 --traced-paths 16 --gpu auto --plots

docs-routing:
	mkdir -p $(BUILD_DIR)/routing-parallel
	$(TYPST) compile --root . --creation-timestamp 1794528000 \
		--input results=../evidence/routing-parallel/results.json \
		--input chart=../evidence/routing-parallel/routing-parallel.png \
		docs/routing-parallel.typ $(BUILD_DIR)/routing-parallel/routing-parallel.pdf

$(BUILD_DIR)/bootstrap_tests: tests/bootstrap_regressions.rs $(wildcard bootstrap/*.rs) | $(BUILD_DIR)
	$(RUSTC) --test tests/bootstrap_regressions.rs -o $@

$(BUILD_DIR)/bootstrap_tests_opt: tests/bootstrap_regressions.rs $(wildcard bootstrap/*.rs) | $(BUILD_DIR)
	$(RUSTC) --test -O tests/bootstrap_regressions.rs -o $@

verify-rust: all $(BUILD_DIR)/bootstrap_tests $(BUILD_DIR)/bootstrap_tests_opt
	./$(BUILD_DIR)/bootstrap_tests
	./$(BUILD_DIR)/bootstrap_tests_opt

examples: | $(BUILD_DIR)
	$(PYTHON) aodc.py run examples/ternary.dec
	$(PYTHON) aodc.py run examples/closure.dec
	$(PYTHON) aodc.py run examples/fusion.dec
	$(PYTHON) aodc.py run examples/coupling.dec
	$(PYTHON) aodc.py run examples/weights.dec
	$(PYTHON) aodc.py compile examples/closure.dec -o $(BUILD_DIR)/closure.dec.json
	$(PYTHON) aodc.py exec $(BUILD_DIR)/closure.dec.json

verify: verify-python verify-rust examples

clean:
	rm -rf $(BUILD_DIR)
