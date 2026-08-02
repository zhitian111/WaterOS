SHELL := /bin/bash
.DEFAULT_GOAL := help

VERSION_BASE := 0.1.0
STAGE := prototype
BRANCH=$(shell git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown")
BUILD_NUM = $(shell git rev-list --count HEAD 2>/dev/null || echo "0")

RV_RUN_TARGETS := rv_pre_run rv_final_run rv_final_run_log \
				  rv_final_debug_run rv_final_smp_test
LA_RUN_TARGETS := la_pre_run la_final_run la_final_run_log
DEBUGGABLE_RUN_TARGETS := $(RV_RUN_TARGETS) $(LA_RUN_TARGETS)
GDB_RUN_TARGETS := $(addsuffix -gdb,$(DEBUGGABLE_RUN_TARGETS))

.PHONY: help version stat clean check all all_start_info kernel configure fmt
.PHONY: apply_features apply_features_la flush_img export rv_user_mode_bin
.PHONY: rv_check rv_elf_info rv_obj_dump rv_pc_watch rv_symbol_at
.PHONY: kernel-rv kernel-rv-pre kernel-rv-final kernel-rv-final-log
.PHONY: kernel-rv-final-debug kernel-rv-final-smp-test kernel-rv-strip-image
.PHONY: $(RV_RUN_TARGETS) $(GDB_RUN_TARGETS) rv_qemu_run rv_qemu_run_snapshot
.PHONY: rv_qemu_run_with_log
.PHONY: la_check la_elf_info la_symbol_at la_pc_watch la_gdb_snapshot
.PHONY: kernel-la kernel-la-pre kernel-la-final $(LA_RUN_TARGETS)
.PHONY: la_qemu_run la_qemu_run_snapshot
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

COMMA := ,
GDB_BUILD ?= 0
GDB_FAULTS ?= 0
GDB_SUFFIX := $(if $(filter 1,$(GDB_BUILD)),-gdb,)
GDB_FAULT_FEATURE := $(if $(filter 1,$(GDB_FAULTS)),$(COMMA)gdb-fault-injection,)
GDB_FEATURE_SUFFIX := $(if $(filter 1,$(GDB_BUILD)),$(COMMA)gdb-debug$(GDB_FAULT_FEATURE),)
CARGO_PROFILE_ARG := $(if $(filter 1,$(GDB_BUILD)),--profile gdb,--release)
GDB_FRAME_FLAGS := $(if $(filter 1,$(GDB_BUILD)),-C force-frame-pointers=yes,)
GDB_BUILD_ID ?= $(shell git rev-parse --short=12 HEAD 2>/dev/null || echo unknown)-$(shell date +%s)
cargo_env = $(if $(filter 1,$(GDB_BUILD)),WATEROS_DEBUG_BUILD_ID="$(GDB_BUILD_ID)" WATEROS_DEBUG_FRAME_POINTERS=1 RUSTFLAGS="$(strip $(RUSTFLAGS) $(GDB_FRAME_FLAGS))",)
cargo_build = $(cargo_env) cargo build

kernel_rv_flag := $(CARGO_PROFILE_ARG) --target riscv64gc-unknown-none-elf \
                  --features qemu-riscv64-opensbi,pre$(GDB_FEATURE_SUFFIX)
kernel_rv_pre_flag := $(CARGO_PROFILE_ARG) --target riscv64gc-unknown-none-elf \
                      --features qemu-riscv64-opensbi,pre$(GDB_FEATURE_SUFFIX)
kernel_rv_final_flag := $(CARGO_PROFILE_ARG) --target riscv64gc-unknown-none-elf \
                        --features qemu-riscv64-opensbi,final_online$(GDB_FEATURE_SUFFIX)
kernel_rv_final_log_flag := $(CARGO_PROFILE_ARG) --target riscv64gc-unknown-none-elf \
                            --features qemu-riscv64-opensbi,final_online,stall-debug$(GDB_FEATURE_SUFFIX)
kernel_rv_final_debug_flag := $(CARGO_PROFILE_ARG) --target riscv64gc-unknown-none-elf \
                              --features qemu-riscv64-opensbi,final_online,dashboard-debug$(GDB_FEATURE_SUFFIX)
kernel_rv_final_smp_flag := $(CARGO_PROFILE_ARG) --target riscv64gc-unknown-none-elf \
                            --features qemu-riscv64-opensbi,final_online_smp_test$(GDB_FEATURE_SUFFIX)

kernel_rv_build_artifact_path := ./target/riscv64gc-unknown-none-elf/$(if $(filter 1,$(GDB_BUILD)),gdb,release)

kernel_la_flag := $(CARGO_PROFILE_ARG) --target loongarch64-unknown-none \
                  --no-default-features \
                  --features qemu-loongarch64-virt,pre$(GDB_FEATURE_SUFFIX)
kernel_la_pre_flag := $(CARGO_PROFILE_ARG) --target loongarch64-unknown-none \
                      --no-default-features \
                      --features qemu-loongarch64-virt,pre$(GDB_FEATURE_SUFFIX)
kernel_la_final_flag := $(CARGO_PROFILE_ARG) --target loongarch64-unknown-none \
                        --no-default-features \
                        --features qemu-loongarch64-virt,final_online$(GDB_FEATURE_SUFFIX)

kernel_la_build_artifact_path := ./target/loongarch64-unknown-none/$(if $(filter 1,$(GDB_BUILD)),gdb,release)
GDB_PORT ?= 1234
GDB_WAIT ?= 1
LA_GDB_ELF ?= ./kernel-la-pre-gdb

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

help:
	@echo "WaterOS 常用目标（在 os/ 目录执行）"
	@echo "  make rv_pre_run                 构建并运行 RISC-V 初赛内核"
	@echo "  make rv_final_run               构建并运行 RISC-V 决赛内核"
	@echo "  make la_pre_run                 构建并运行 LoongArch 初赛内核"
	@echo "  make la_final_run               构建并运行 LoongArch 决赛内核"
	@echo "  make <run-target>-gdb           以 GDB 模式运行，默认暂停等待连接"
	@echo "  make <run-target>-gdb GDB_WAIT=0  开放 GDB 端口但立即运行"
	@echo "  make <run-target>-gdb GDB_PORT=1235 使用指定 GDB 端口"
	@echo "  make <run-target>-gdb GDB_FAULTS=1 构建确定性故障注入测试内核"
	@echo "  ./scripts/wateros_debug.py doctor / run / snapshot / watch / gdb"
	@echo "  make rv_check / make la_check   检查对应架构"
	@echo "  make rv_pc_watch / la_pc_watch  监视各架构 PC"


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
	@$(cargo_build) $(kernel_rv_flag)
	$(call INFO,"构建成功！");

kernel-rv: kernel-rv-pre
	@cp ./kernel-rv-pre ./kernel-rv

$(kernel_la_build_artifact_path)/wateros: $(common_dependencies) kernel
	$(call INFO, "开始构建 loongarch64 内核...")
	@$(cargo_build) $(kernel_la_flag)
	$(call INFO,"构建成功！");

kernel-la: kernel-la-pre
	@cp ./kernel-la-pre ./kernel-la

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

la_elf_info: kernel-la
	$(call INFO, "内核文件 elf 头部信息")
	@readelf -h ./kernel-la
	$(call INFO, "内核文件 elf 段信息")
	@readelf -S ./kernel-la
	$(call INFO, "内核文件 elf 符号表")
	@readelf -s ./kernel-la

kernel-rv-pre: version
	$(call INFO, "构建 riscv64 初赛内核...")
	@$(cargo_build) $(kernel_rv_pre_flag)
	@cp $(kernel_rv_build_artifact_path)/wateros ./kernel-rv-pre$(GDB_SUFFIX)

kernel-rv-final: version
	$(call INFO, "构建 riscv64 决赛在线内核...")
	@$(cargo_build) $(kernel_rv_final_flag)
	@cp $(kernel_rv_build_artifact_path)/wateros ./kernel-rv-final$(GDB_SUFFIX)

kernel-rv-final-log: version
	$(call INFO, "构建带停滞诊断的 riscv64 决赛在线内核...")
	@$(cargo_build) $(kernel_rv_final_log_flag)
	@cp $(kernel_rv_build_artifact_path)/wateros ./kernel-rv-final-log$(GDB_SUFFIX)

kernel-rv-final-debug: version
	$(call INFO, "构建带 dashboard 的 riscv64 决赛调试内核...")
	@$(cargo_build) $(kernel_rv_final_debug_flag)
	@cp $(kernel_rv_build_artifact_path)/wateros ./kernel-rv-final-debug$(GDB_SUFFIX)

rv_pre_run: kernel-rv-pre
	$(call INFO, "运行 riscv64 初赛 bringup...")
	@WOS_KERNEL=./kernel-rv-pre$(GDB_SUFFIX) bash ./scripts/rv_pre_run.sh

rv_final_run: kernel-rv-final
	$(call INFO, "运行 riscv64 决赛在线 bringup...")
	@WOS_KERNEL=./kernel-rv-final$(GDB_SUFFIX) bash ./scripts/rv_final_run.sh
rv_final_run_log: kernel-rv-final-log
	$(call INFO, "运行 riscv64 决赛在线 bringup 并记录日志...")
	@WOS_KERNEL=./kernel-rv-final-log$(GDB_SUFFIX) \
		bash ./scripts/rv_final_run.sh 2>&1 | tee output.log
rv_final_debug_run: kernel-rv-final-debug
	$(call INFO, "运行带 dashboard 的 riscv64 决赛调试 bringup...")
	@WOS_KERNEL=./kernel-rv-final-debug$(GDB_SUFFIX) bash ./scripts/rv_final_run.sh

kernel-rv-final-smp-test: version
	$(call INFO, "构建 riscv64 决赛多核 MM 测试内核...")
	@$(cargo_build) $(kernel_rv_final_smp_flag)
	@cp $(kernel_rv_build_artifact_path)/wateros ./kernel-rv-final-smp-test$(GDB_SUFFIX)

rv_final_smp_test: kernel-rv-final-smp-test
	$(call INFO, "运行 riscv64 决赛多核 MM 测试...")
	@WOS_KERNEL=./kernel-rv-final-smp-test$(GDB_SUFFIX) bash ./scripts/rv_final_run.sh

# 兼容旧脚本调用：旧目标按初赛语义执行。
rv_qemu_run: rv_pre_run

rv_qemu_run_snapshot: kernel-rv
	$(call INFO, "开始运行 riscv64 内核 snapshot 模式，不写回 sdcard-rv.img")
	@bash ./scripts/rv_qemu_run_snapshot.sh
	$(call INFO, "riscv64 内核运行结束 snapshot")

kernel-rv-ltp-glibc:
	$(call INFO, "构建 riscv64 内核 LTP glibc only")
	@cargo build $(kernel_rv_flag),bringup-ltp-glibc-only
	@cp $(kernel_rv_build_artifact_path)/wateros ./kernel-rv-ltp-glibc

kernel-rv-ltp-musl:
	$(call INFO, "构建 riscv64 内核 LTP musl only")
	@cargo build $(kernel_rv_flag),bringup-ltp-musl-only
	@cp $(kernel_rv_build_artifact_path)/wateros ./kernel-rv-ltp-musl

kernel-la-ltp-glibc:
	$(call INFO, "构建 loongarch64 内核 LTP glibc only")
	@cargo build $(kernel_la_flag),bringup-ltp-glibc-only
	@cp $(kernel_la_build_artifact_path)/wateros ./kernel-la-ltp-glibc

kernel-la-ltp-musl:
	$(call INFO, "构建 loongarch64 内核 LTP musl only")
	@cargo build $(kernel_la_flag),bringup-ltp-musl-only
	@cp $(kernel_la_build_artifact_path)/wateros ./kernel-la-ltp-musl

.PHONY: kernel-rv-ltp-glibc kernel-rv-ltp-musl kernel-la-ltp-glibc kernel-la-ltp-musl rv_qemu_run_snapshot

kernel-la-pre: version
	$(call INFO, "构建 loongarch64 初赛内核...")
	@$(cargo_build) $(kernel_la_pre_flag)
	@cp $(kernel_la_build_artifact_path)/wateros ./kernel-la-pre$(GDB_SUFFIX)

kernel-la-final: version
	$(call INFO, "构建 loongarch64 决赛在线内核...")
	@$(cargo_build) $(kernel_la_final_flag)
	@cp $(kernel_la_build_artifact_path)/wateros ./kernel-la-final$(GDB_SUFFIX)

la_pre_run: kernel-la-pre
	$(call INFO, "运行 loongarch64 初赛 bringup...")
	@WOS_KERNEL=./kernel-la-pre$(GDB_SUFFIX) bash ./scripts/la_pre_run.sh

la_final_run: kernel-la-final
	$(call INFO, "运行 loongarch64 决赛在线 bringup...")
	@WOS_KERNEL=./kernel-la-final$(GDB_SUFFIX) bash ./scripts/la_final_run.sh
#make 2>&1 | tee output.log
la_final_run_log: kernel-la-final
	$(call INFO, "运行 loongarch64 决赛在线 bringup 并记录日志...")
	@WOS_KERNEL=./kernel-la-final$(GDB_SUFFIX) bash ./scripts/la_final_run.sh 2>&1 | tee output.log

# 所有真实运行目标共用同一个 GDB 后缀。默认传入 QEMU `-S`，因此调试器连接并
# 执行 continue 前 guest 不会运行；GDB_WAIT=0 可用于对运行中的卡死现场采样。
$(GDB_RUN_TARGETS):
	$(call INFO, "以 GDB 模式运行 $(patsubst %-gdb,%,$@)，port=$(GDB_PORT) wait=$(GDB_WAIT)")
	@$(MAKE) --no-print-directory $(patsubst %-gdb,%,$@) \
		GDB_BUILD=1 \
		WOS_QEMU_GDB=1 \
		WOS_QEMU_GDB_WAIT=$(GDB_WAIT) \
		WOS_QEMU_GDB_PORT=$(GDB_PORT) \
		WOS_QEMU_SNAPSHOT=$${WOS_QEMU_SNAPSHOT:-1}

la_gdb_snapshot:
	@test -f "$(LA_GDB_ELF)" || \
		(echo "找不到 ELF: $(LA_GDB_ELF)；请先构建对应 LA 内核" && exit 1)
	@python3 ./scripts/wateros_debug.py snapshot --arch la \
		--elf "$(LA_GDB_ELF)" --port "$(GDB_PORT)"

# 兼容旧脚本调用：旧目标按初赛语义执行。
la_qemu_run: la_pre_run

la_qemu_run_snapshot: kernel-la
	$(call INFO, "开始运行 loongarch64 内核 snapshot 模式，不写回 sdcard-la.img")
	@bash ./scripts/la_qemu_run_snapshot.sh
	$(call INFO, "loongarch64 内核运行结束 snapshot")

rv_qemu_run_with_log: kernel-rv
	$(call INFO, "开始运行 riscv64 内核...")
	@bash ./scripts/rv_qemu_run_with_log.sh
	$(call INFO, "riscv64 内核运行结束, 调试信息请见qemu.log")

rv_pc_watch: kernel-rv
	$(call INFO, "riscv64 PC 变动监视（仅 PC 变化时打印）...")
	@python3 ./scripts/pc_trace_watch.py --arch rv --elf ./kernel-rv
	$(call INFO, "PC 监视已结束")

rv_symbol_at: kernel-rv
	@test -n "$(ADDR)" || (echo "用法: make rv_symbol_at ADDR=0x80201234" && exit 1)
	@python3 ./scripts/resolve_pc_symbol.py --arch rv --elf ./kernel-rv $(ADDR)

la_pc_watch: kernel-la
	$(call INFO, "loongarch64 PC 变动监视（仅 PC 变化时打印）...")
	@python3 ./scripts/pc_trace_watch.py --arch la --elf ./kernel-la
	$(call INFO, "PC 监视已结束")

la_symbol_at: kernel-la
	@test -n "$(ADDR)" || (echo "用法: make la_symbol_at ADDR=0x90001234" && exit 1)
	@python3 ./scripts/resolve_pc_symbol.py --arch la --elf ./kernel-la $(ADDR)


rv_check:
	$(call INFO, "开始对 riscv 平台进行 cargo check ...")
	@$(cargo_env) cargo check $(kernel_rv_flag)
	$(call INFO, "对 riscv 平台的 cargo check 完成！")

la_check:
	$(call INFO, "开始对 loongarch64 平台进行 cargo check ...")
	@$(cargo_env) cargo check $(kernel_la_flag)
	$(call INFO, "对 loongarch64 平台的 cargo check 完成！")

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
	@rm -f ./kernel-la >/dev/null 2>/dev/null
	$(call INFO, "构建产物清理完成！")
	$(call INFO, "开始清理备份文件...")
	$(call INFO, "以下文件将被删除！")
	$(call INFO, "$(shell find . -name "*.wosbak")")
	@rm -f $(shell find . -name "*.wosbak")
	$(call INFO, "备份文件清理完成！")
	# $(call INFO, "开始清理 feature 配置文件...")
	# @rm -f ./config.conf ./feature-tree.txt
	# $(call INFO, "feature 配置文件清理完成！")
	$(call INFO, "清理完成！")

all_start_info:
	$(call INFO, "开始构建完整项目...")
all: all_start_info kernel-rv kernel-la

fmt:
	$(call INFO, "开始格式化项目文件...")
	@taplo fmt **/Cargo.toml
	@rustfmt **/*.rs
	$(call INFO, "项目文件格式化完成！")

apply_features: ./config.conf
	$(call INFO, "开始写入 features riscv64 ...")
	@bash ./scripts/apply-config-as-default-features.bash
	$(call INFO, "riscv64 features 写入完成！")

apply_features_la: ./config-loongarch64.conf
	$(call INFO, "开始写入 features loongarch64 ...")
	@bash ./scripts/apply-config-as-default-features.bash ./ apply config-loongarch64.conf
	$(call INFO, "loongarch64 features 写入完成！")

revert_features:
	$(call INFO, "开始回滚 features...")
	@bash ./scripts/apply-config-as-default-features.bash ./ os revert
	$(call INFO, "features 回滚完成！")
export:
	$(call INFO, "开始导出 git 追踪文件到 GitLab 仓库...")
	@bash ./scripts/export-to-gitlab.bash
	$(call INFO, "导出完成！")

flush_img:
	$(call INFO, "开始删除旧磁盘镜像...")
	@rm -f ./sdcard-la.img
	@rm -f ./sdcard-rv.img
	$(call INFO, "旧磁盘镜像已删除！")
	$(call INFO, "开始拷贝新磁盘镜像...")
	@cp ../test_case/sdcard-rv.img ./
	@cp ../test_case/sdcard-la.img ./
	$(call INFO, "新磁盘镜像已拷贝完成！")
	
