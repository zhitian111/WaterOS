SHELL := /bin/bash
.PHONY: version stat
VERSION_BASE := 0.1.0
STAGE := prototype
BRANCH=$(shell git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown")
BUILD_NUM = $(shell git rev-list --count HEAD 2>/dev/null || echo "0")

.PHONY: clean check all all_start_info kernel configure fmt apply_features
.PHONY: rv_check rv_elf_info rv_obj_dump rv_qemu_run rv_qemu_run_with_log rv_qemu_gdb rv_gdb
.PHONY: rv_user_mode_bin
COLOR_ANSI_RED := \033[31m
COLOR_ANSI_GREEN := \033[32m
COLOR_ANSI_YELLOW := \033[33m
COLOR_ANSI_BLUE := \033[34m
COLOR_ANSI_PURPLE := \033[35m
COLOR_ANSI_CYAN := \033[36m
COLOR_ANSI_WHITE := \033[37m

COLOR_ANSI_CLEAR := \033[0m

VERSION := v$(COLOR_ANSI_GREEN)$(VERSION_BASE)$(COLOR_ANSI_CLEAR)-$(COLOR_ANSI_PURPLE)$(STAGE)$(COLOR_ANSI_CLEAR).$(BUILD_NUM)+$(COLOR_ANSI_YELLOW)$(BRANCH)$(COLOR_ANSI_CLEAR)

user_mode_program_root_dir = ../wateros_user_mode_program

common_dependencies = $(shell find . -name "*.rs") \
					  $(shell find . -name "*.ld")  \
					  $(shell find . -name "*.S") \
					  $(shell find . -name "*.toml")

kernel_rv_dependencies = $(common_dependencies)

features_flags = $(shell bash ./scripts/config-to-features-make.bash || echo "unknown")

common_flags = --features $(features_flags)

kernel_rv_flag := --release --target riscv64gc-unknown-none-elf	\
				  $(common_flags)

kernel_rv_build_artifact_path := ./target/riscv64gc-unknown-none-elf/release

define TRACE
	@echo -e "$(COLOR_ANSI_PURPLE)[MAKEFILE]$(COLOR_ANSI_CLEAR)$(COLOR_ANSI_BLUE)\t[TRACE]\t$(1)$(COLOR_ANSI_CLEAR)"
endef
define INFO
	@echo -e "$(COLOR_ANSI_PURPLE)[MAKEFILE]$(COLOR_ANSI_CLEAR)$(COLOR_ANSI_GREEN)\t[INFO]\t$(1)$(COLOR_ANSI_CLEAR)"
endef
define WARNING
	@echo -e "$(COLOR_ANSI_PURPLE)[MAKEFILE]$(COLOR_ANSI_CLEAR)$(COLOR_ANSI_YELLOW)\t[WARNING]\t$(1)$(COLOR_ANSI_CLEAR)"
endef
define ERROR
	@echo -e "$(COLOR_ANSI_PURPLE)[MAKEFILE]$(COLOR_ANSI_CLEAR)$(COLOR_ANSI_RED)\t[ERROR]\t$(1)	错误代码:$(2)$(COLOR_ANSI_CLEAR)"
	@exit $$2
endef


version:
	$(call INFO, "当前版本信息如下：")
	@echo -e "$(COLOR_ANSI_CYAN)WaterOS\t$(COLOR_ANSI_WHITE)--version\t$(VERSION)"

stat: version
	$(call INFO, "开始统计项目状态信息...")
	$(call INFO, "开始统计项目文本文件字符数...")
	@bash ./scripts/stat_texts.bash
	$(call INFO, "项目文本文件字符数统计完成！")
	$(call INFO, "开始统计项目贡献度...")
	@bash ./scripts/stat_contribute.bash
	$(call INFO, "项目贡献度统计完成！")

configure: ./config.conf ./feature-tree.txt

./config.conf ./feature-tree.txt:
	$(call INFO, "正在生成配置文件...")
	@bash ./scripts/configure.bash 2> /dev/null
	$(call INFO, "配置文件生成完毕！")

kernel:

$(kernel_rv_build_artifact_path)/wateros: $(kernel_rv_dependencies) kernel
	$(call INFO, "开始构建 riscv64 内核...")
	@cargo build $(kernel_rv_flag)
	$(call INFO,"构建成功！");

kernel-rv:	version apply_features $(kernel_rv_build_artifact_path)/wateros
	@cp $(kernel_rv_build_artifact_path)/wateros ./kernel-rv

kernel-rv-strip-image: $(kernel_rv_build_artifact_path)/wateros
	$(call INFO,"清除文件元数据...");
	@rust-objcopy --strip-all $(kernel_rv_build_artifact_path)/wateros -O binary target/riscv64gc-unknown-none-elf/release/wateros.bin
	$(call INFO, "清除完成！请见target/riscv64gc-unknown-none-elf/release/wateros.bin")

rv_elf_info: kernel-rv
	$(call INFO, "内核文件 elf 头部信息")
	@readelf -h ./kernel-rv
	$(call INFO, "内核文件 elf 段信息")
	@readelf -S ./kernel-rv
	$(call INFO, "内核文件 elf 符号表")
	@readelf -s ./kernel-rv

rv_qemu_run: kernel-rv
	$(call INFO, "开始运行 riscv64 内核...")
	@bash ./scripts/rv_qemu_run.sh
	$(call INFO, "riscv64 内核运行结束")

rv_qemu_run_with_log: kernel-rv
	$(call INFO, "开始运行 riscv64 内核...")
	@bash ./scripts/rv_qemu_run_with_log.sh
	$(call INFO, "riscv64 内核运行结束, 调试信息请见qemu.log")

rv_qemu_gdb: kernel-rv
	$(call INFO, "开始运行 riscv64 内核和 QEMU 的 GDB 服务...")
	@bash ./scripts/rv_qemu_gdb.sh
	$(call INFO, "riscv64 内核和 QEMU 的 GDB 服务运行已结束")
rv_gdb: kernel-rv
	$(call INFO, "开始运行 GDB 客户端调试 riscv64 内核...")
	@riscv64-elf-gdb \
		-ex 'file kernel-rv' \
		-ex 'set arch riscv:riscv64' \
		-ex 'target remote localhost:1234'
	$(call INFO, "GDB 客户端及 riscv64 内核调试已结束...")


rv_check:
	$(call INFO, "开始对 riscv 平台进行 cargo check ...")
	@-cargo check $(kernel_rv_flag)
	$(call INFO, "对 riscv 平台的 cargo check 完成！")

# rv_link_app_asm:
# 	$(call INFO, "开始生成批处理系统的 asm 文件...")
# 	@bash ./scripts/rv_link_app_gen.bash
# 	$(call INFO, "批处理系统的 asm 文件生成完成！详见./src/riscv/assembly_code/linke_app.asm")

check: version rv_check
	$(call INFO, "check 完成！")

rv_user_mode_program:
	$(call INFO, "开始为 riscv 平台生成用户态测试用程序...")
	@$(MAKE) -C $(user_mode_program_root_dir) all
	$(call INFO, "riscv 平台用户态测试用程序生成完成！")


clean:
	$(call INFO, "开始清理构建产物...")
	@cargo clean
	@bash ./scripts/cargo_clean_all.sh ./components/
	@rm -f ./kernel-rv >/dev/null 2>/dev/null
	$(call INFO, "构建产物清理完成！")
	$(call INFO, "开始清理备份文件...")
	$(call INFO, "以下文件将被删除！")
	$(call INFO, "$(shell find . -name "*.wosbak")")
	@rm -f $(shell find . -name "*.wosbak")
	$(call INFO, "备份文件清理完成！")
	$(call INFO, "开始清理 feature 配置文件...")
	@rm -f ./config.conf ./feature-tree.txt
	$(call INFO, "feature 配置文件清理完成！")
	$(call INFO, "清理完成！")

all_start_info:
	$(call INFO, "开始构建完整项目...")
all: all_start_info kernel-rv kernel-rv-strip-image

fmt:
	$(call INFO, "开始格式化项目文件...")
	@taplo fmt **/Cargo.toml
	@rustfmt **/*.rs
	$(call INFO, "项目文件格式化完成！")
apply_features: ./config.conf
	$(call INFO, "开始写入 features...")
	@bash ./scripts/apply-config-as-default-features.bash
	$(call INFO, "features 写入完成！")
