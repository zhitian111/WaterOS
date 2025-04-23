# 定义目标平台
TARGET_RISCV = riscv64gc-unknown-none-elf
TARGET_LOONGARCH = loongarch64-unknown-none

# 定义输出文件名
OUTPUT_RISCV = kernel-rv
OUTPUT_LOONGARCH = kernel-la

# 设置汇编器
RISCV_ASM_FILES_PATH = ./src/asm/riscv
AS_RISCV_TARGET_PATH = ./target/objects/kernel-rv
RISCV_ASM_FILES = $(RISCV_ASM_FILES_PATH)/wateros_platform_riscv64_gcc.S
RISCV_OBJ_FILES = $(RISCV_TARGET_PATH)/wateros_platform_riscv64_gcc.o
AS_RISCV = riscv64-unknown-elf-gcc
AS_RISCV_FLAG = -nostdlib -nostartfiles -ffreestanding -c -o $(AS_RISCV_TARGET_PATH)/wateros_platform_riscv64_gcc.o
# 默认目标：编译所有平台
all: build-riscv build-loongarch
# 编译 riscv 平台
build-riscv:$(RISCV_OBJ_FILES)
	@echo "Building for RISCV..."
	cargo build --target $(TARGET_RISCV) --release --bin $(OUTPUT_RISCV)
	cp target/$(TARGET_RISCV)/release/$(OUTPUT_RISCV) .
	@echo "RISCV build complete: $(OUTPUT_RISCV)"
# 编译 riscv 平台的汇编文件
# 以及编译 riscv 平台 rust 代码至目标文件
$(RISCV_OBJ_FILES):$(RISCV_ASM_FILES)
	@echo "Building for RISCV..."
	mkdir -p $(AS_RISCV_TARGET_PATH)
	$(AS_RISCV) $(AS_RISCV_FLAG) $(RISCV_ASM_FILES)
	@echo "RISCV build complete: $(RISCV_OBJ_FILES)"

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
