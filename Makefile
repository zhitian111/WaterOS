# 定义目标平台
TARGET_RISCV = riscv64gc-unknown-none-elf
TARGET_LOONGARCH = loongarch64-unknown-none

# 定义输出文件名
OUTPUT_RISCV = kernel-rv
OUTPUT_LOONGARCH = kernel-la

# 默认目标：编译所有平台
all: build-riscv build-loongarch
# 编译 riscv 平台
build-riscv:
	@echo "Building for RISCV..."
	cargo build --target $(TARGET_RISCV) --release --bin $(OUTPUT_RISCV)
	cp target/$(TARGET_RISCV)/release/$(OUTPUT_RISCV) .
	@echo "RISCV build complete: $(OUTPUT_RISCV)"

# 编译 LoongArch 平台
build-loongarch:
	@echo "Building for LoongArch..."
	cargo build --target $(TARGET_LOONGARCH) --release --bin $(OUTPUT_LOONGARCH)
	cp target/$(TARGET_LOONGARCH)/release/$(OUTPUT_LOONGARCH) .
	@echo "LoongArch build complete: $(OUTPUT_LOONGARCH)"
# 清理生成的文件
clean:
	rm -f $(OUTPUT_RISCV) $(OUTPUT_LOONGARCH)
	cargo clean
	@echo "Clean complete."

# 伪目标声明
.PHONY: all build-riscv build-loongarch clean
