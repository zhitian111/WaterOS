.PHONY: all clean

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
