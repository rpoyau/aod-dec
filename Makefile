PYTHON ?= python3
RUSTC ?= rustc
RUSTFLAGS := -C opt-level=3 -C debuginfo=0 -C strip=symbols -C codegen-units=1
BUILD_DIR := build

.PHONY: all verify verify-python verify-rust examples clean
all: $(BUILD_DIR)/aod_stage0_seed $(BUILD_DIR)/aod_self_host_compiler $(BUILD_DIR)/aod_stage2_bootstrap

$(BUILD_DIR):
	mkdir -p $(BUILD_DIR)

$(BUILD_DIR)/%: bootstrap/%.rs $(wildcard bootstrap/*.rs) | $(BUILD_DIR)
	$(RUSTC) $(RUSTFLAGS) $< -o $@

verify-python:
	$(PYTHON) -m unittest discover -s tests -p 'test_*.py' -v

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
