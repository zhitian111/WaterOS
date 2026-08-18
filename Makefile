.PHONY: all clean la2k_check la2k_image la2k_uimage la2k_bootdir la2k_bootscr la2k_flashscr la2k_tftp

OS_DIR := os
KERNEL_RV := kernel-rv
KERNEL_LA := kernel-la
CARGO_CONFIG := $(OS_DIR)/.cargo/config.toml
CARGO_CONFIG_TEMPLATE := $(OS_DIR)/cargo-vendor-config.toml

$(CARGO_CONFIG): $(CARGO_CONFIG_TEMPLATE)
	@mkdir -p $(dir $@)
	@cp $< $@

all: $(CARGO_CONFIG)
	$(MAKE) -C $(OS_DIR) all
	cp $(OS_DIR)/$(KERNEL_RV) ./
	cp $(OS_DIR)/$(KERNEL_LA) ./

clean:
	$(MAKE) -C $(OS_DIR) clean
	rm -f $(KERNEL_RV) $(KERNEL_LA)

# Board-facing targets are forwarded so the repository root can be used as the
# single entry point; command-line variables are propagated by recursive make.
la2k_check la2k_image la2k_uimage la2k_bootdir la2k_bootscr la2k_flashscr la2k_tftp:
	$(MAKE) -C $(OS_DIR) $@
