# Deterministic Bare-Metal Linux Build Pipeline for AOD / AFC Dec Compiler
RUSTC ?= rustc
RUSTFLAGS := -C opt-level=3 -C debuginfo=0 -C strip=symbols -C codegen-units=1
BUILD_DIR := build

.PHONY: all verify clean

all: $(BUILD_DIR)/aod_stage0_seed $(BUILD_DIR)/aod_self_host_compiler $(BUILD_DIR)/aod_stage2_bootstrap

$(BUILD_DIR):
	mkdir -p $(BUILD_DIR)

$(BUILD_DIR)/aod_stage0_seed: bootstrap/aod_stage0_seed.rs | $(BUILD_DIR)
	$(RUSTC) $(RUSTFLAGS) $< -o $@

$(BUILD_DIR)/aod_self_host_compiler: bootstrap/aod_self_host_compiler.rs | $(BUILD_DIR)
	$(RUSTC) $(RUSTFLAGS) $< -o $@

$(BUILD_DIR)/aod_stage2_bootstrap: bootstrap/aod_stage2_bootstrap.rs | $(BUILD_DIR)
	$(RUSTC) $(RUSTFLAGS) $< -o $@

verify: all
	./$(BUILD_DIR)/aod_stage0_seed
	./$(BUILD_DIR)/aod_self_host_compiler dec/kernel.dec $(BUILD_DIR)/aodc_kernel_bin
	./$(BUILD_DIR)/aodc_kernel_bin
	./$(BUILD_DIR)/aod_stage2_bootstrap $(BUILD_DIR)/aodc_stage1
	./$(BUILD_DIR)/aodc_stage1 dec/kernel.dec

clean:
	rm -rf $(BUILD_DIR) aodc_stage1
