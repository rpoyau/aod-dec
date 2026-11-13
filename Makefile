PYTHON ?= python3
RUSTC ?= rustc
TYPST ?= typst
RUSTFLAGS := -C opt-level=3 -C debuginfo=0 -C strip=symbols -C codegen-units=1
BUILD_DIR := build

.PHONY: all verify verify-python verify-python-optimized verify-rust verify-native verify-ci docs docs-native simulate-cells docs-cells examples clean
all: $(BUILD_DIR)/aod_stage0_seed $(BUILD_DIR)/aod_self_host_compiler $(BUILD_DIR)/aod_stage2_bootstrap

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
