SHELL := /bin/bash
.DEFAULT_GOAL := help

VERSION_BASE := 0.1.0
STAGE := prototype
BRANCH=$(shell git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown")
BUILD_NUM = $(shell git rev-list --count HEAD 2>/dev/null || echo "0")

.PHONY: help show-config version stat clean check build run shell debug debug-server
.PHONY: snapshot watch gdb doctor validate-config all all_start_info kernel configure fmt
.PHONY: apply_features apply_features_la flush_img export rv_user_mode_bin
.PHONY: rv_check rv_elf_info rv_obj_dump rv_pc_watch rv_symbol_at
.PHONY: kernel-rv kernel-rv-best kernel-rv-pre kernel-rv-final kernel-rv-final-log
.PHONY: kernel-rv-final-debug kernel-rv-strip-image
.PHONY: rv_pre_run rv_final_run rv_final_run_log rv_final_debug_run
.PHONY: rv_pre_run-gdb rv_final_run-gdb rv_final_run_log-gdb rv_final_debug_run-gdb
.PHONY: rv_qemu_run rv_qemu_run_snapshot
.PHONY: rv_qemu_run_with_log
.PHONY: la_check la_elf_info la_symbol_at la_pc_watch la_gdb_snapshot
.PHONY: kernel-la kernel-la-best kernel-la-pre kernel-la-final la_pre_run la_final_run la_final_run_log
.PHONY: la_pre_run-gdb la_final_run-gdb la_final_run_log-gdb
.PHONY: la_qemu_run la_qemu_run_snapshot
.PHONY: physical-root-image verify-physical-root-image
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

# ── 操作者可覆盖的默认值 ──────────────────────────────────────────
# 命令行变量优先，例如：make run ARCH=la PROFILE=final SMP=4。
# 执行 `make show-config` 可查看选择内核、镜像后的最终生效值。
ARCH ?= rv
PROFILE ?= pre
CONFIG_KEY := $(ARCH)-$(PROFILE)

# 各架构、各构建阶段使用的默认根文件系统镜像。镜像选择集中在 Make，
# QEMU 启动器只接收最终路径，不再根据 profile 猜测或静默回退。
RV_PRE_IMAGE ?= ./sdcard-rv.img
# 当前工作区 pre/final 共用该基础镜像；比赛镜像可在命令行单独覆盖。
RV_FINAL_IMAGE ?= ./sdcard-rv-pub.img
LA_PRE_IMAGE ?= ./sdcard-la.img
LA_FINAL_IMAGE ?= ./sdcard-la-pub.img
IMAGE_rv_pre = $(RV_PRE_IMAGE)
IMAGE_rv_final = $(RV_FINAL_IMAGE)
IMAGE_la_pre = $(LA_PRE_IMAGE)
IMAGE_la_final = $(LA_FINAL_IMAGE)
SELECTED_IMAGE = $(IMAGE_$(ARCH)_$(PROFILE))

SMP ?= $(if $(WOS_SMP),$(WOS_SMP),8)
# MODE 只选择编译期 operator feature，不传给 QEMU。
MODE ?= $(if $(WATEROS_MODE),$(WATEROS_MODE),auto)
SCRIPT ?= $(WATEROS_OPERATOR_SCRIPT)
GUEST_SHELL ?= $(WATEROS_OPERATOR_SHELL)
WRITE_DISK ?= $(if $(WOS_WRITE_DISK),$(WOS_WRITE_DISK),0)
SNAPSHOT ?= $(if $(WOS_QEMU_SNAPSHOT),$(WOS_QEMU_SNAPSHOT),$(if $(filter 1,$(WRITE_DISK)),0,1))
# SDCARD 用于单次覆盖；WOS_SDCARD 保留为脚本兼容入口。
SDCARD ?= $(if $(strip $(WOS_SDCARD)),$(WOS_SDCARD),$(SELECTED_IMAGE))
ROOT_IMAGE ?= ./wateros-root.img
ROOT_IMAGE_TOOL := ./scripts/root_image/root_image.py
ROOT_IMAGE_MANIFEST ?= ./scripts/root_image/rootfs-manifest.json
ROOT_IMAGE_DATA_MANIFEST ?=
ROOT_IMAGE_DATA_SIZE_MIB ?= 0
ROOT_IMAGE_SIZE_MIB ?= 32
ROOT_IMAGE_PARTITION_TABLE ?= mbr
PORT ?= 1234
START_PAUSED ?= 1
FAULTS ?= 0
EXTRA_FEATURES ?=
# EXTRA_FEATURES 含 gui/display-demo 时默认打开 QEMU 图形窗口，也可显式覆盖。
GRAPHICS ?= $(if $(or $(findstring display-demo,$(EXTRA_FEATURES)),$(findstring gui,$(EXTRA_FEATURES))),1,0)
GRAPHICS_BACKEND ?= auto
GDB_BUILD ?= 0
GDB_FAULTS ?= $(FAULTS)
GDB_SUFFIX := $(if $(filter 1,$(GDB_BUILD)),-gdb,)
GDB_FAULT_FEATURE := $(if $(filter 1,$(GDB_FAULTS)),$(COMMA)gdb-fault-injection,)
GDB_FEATURE_SUFFIX := $(if $(filter 1,$(GDB_BUILD)),$(COMMA)gdb-debug$(GDB_FAULT_FEATURE),)
EXTRA_FEATURE_SUFFIX := $(if $(strip $(EXTRA_FEATURES)),$(COMMA)$(EXTRA_FEATURES),)
OPERATOR_FEATURE := $(if $(filter shell,$(MODE)),operator-shell,$(if $(filter run,$(MODE)),operator-run,))
OPERATOR_FEATURE_SUFFIX := $(if $(strip $(OPERATOR_FEATURE)),$(COMMA)$(OPERATOR_FEATURE),)
CARGO_PROFILE_ARG := $(if $(filter 1,$(GDB_BUILD)),--profile gdb,--release)
GDB_FRAME_FLAGS := $(if $(filter 1,$(GDB_BUILD)),-C force-frame-pointers=yes,)
GDB_BUILD_ID ?= $(shell git rev-parse --short=12 HEAD 2>/dev/null || echo unknown)-$(shell date +%s)
HEAP_ALLOCATOR_FEATURE ?= heap-tlsf
operator_env = WATEROS_OPERATOR_SHELL="$(GUEST_SHELL)" WATEROS_OPERATOR_SCRIPT="$(SCRIPT)"
cargo_env = $(if $(filter 1,$(GDB_BUILD)),WATEROS_DEBUG_BUILD_ID="$(GDB_BUILD_ID)" WATEROS_DEBUG_FRAME_POINTERS=1 RUSTFLAGS="$(strip $(RUSTFLAGS) $(GDB_FRAME_FLAGS))",)
cargo_build = $(operator_env) $(cargo_env) cargo build

kernel_rv_flag := $(CARGO_PROFILE_ARG) --target riscv64gc-unknown-none-elf \
                  --no-default-features \
                  --features qemu-riscv64-opensbi,pre,$(HEAP_ALLOCATOR_FEATURE)$(EXTRA_FEATURE_SUFFIX)$(OPERATOR_FEATURE_SUFFIX)$(GDB_FEATURE_SUFFIX)
kernel_rv_pre_flag := $(CARGO_PROFILE_ARG) --target riscv64gc-unknown-none-elf \
                      --no-default-features \
                      --features qemu-riscv64-opensbi,pre,$(HEAP_ALLOCATOR_FEATURE)$(EXTRA_FEATURE_SUFFIX)$(OPERATOR_FEATURE_SUFFIX)$(GDB_FEATURE_SUFFIX)
kernel_rv_final_flag := $(CARGO_PROFILE_ARG) --target riscv64gc-unknown-none-elf \
                        --no-default-features \
                        --features qemu-riscv64-opensbi,final_online,$(HEAP_ALLOCATOR_FEATURE)$(EXTRA_FEATURE_SUFFIX)$(OPERATOR_FEATURE_SUFFIX)$(GDB_FEATURE_SUFFIX)
kernel_rv_final_log_flag := $(CARGO_PROFILE_ARG) --target riscv64gc-unknown-none-elf \
                            --no-default-features \
                            --features qemu-riscv64-opensbi,final_online,stall-debug,$(HEAP_ALLOCATOR_FEATURE)$(EXTRA_FEATURE_SUFFIX)$(OPERATOR_FEATURE_SUFFIX)$(GDB_FEATURE_SUFFIX)
kernel_rv_final_debug_flag := $(CARGO_PROFILE_ARG) --target riscv64gc-unknown-none-elf \
                              --no-default-features \
                              --features qemu-riscv64-opensbi,final_online,dashboard-debug,$(HEAP_ALLOCATOR_FEATURE)$(EXTRA_FEATURE_SUFFIX)$(OPERATOR_FEATURE_SUFFIX)$(GDB_FEATURE_SUFFIX)

kernel_rv_build_artifact_path := ./target/riscv64gc-unknown-none-elf/$(if $(filter 1,$(GDB_BUILD)),gdb,release)

kernel_la_flag := $(CARGO_PROFILE_ARG) --target loongarch64-unknown-none \
                  --no-default-features \
                  --features qemu-loongarch64-virt,pre,$(HEAP_ALLOCATOR_FEATURE)$(EXTRA_FEATURE_SUFFIX)$(OPERATOR_FEATURE_SUFFIX)$(GDB_FEATURE_SUFFIX)
kernel_la_pre_flag := $(CARGO_PROFILE_ARG) --target loongarch64-unknown-none \
                      --no-default-features \
                      --features qemu-loongarch64-virt,pre,$(HEAP_ALLOCATOR_FEATURE)$(EXTRA_FEATURE_SUFFIX)$(OPERATOR_FEATURE_SUFFIX)$(GDB_FEATURE_SUFFIX)
kernel_la_final_flag := $(CARGO_PROFILE_ARG) --target loongarch64-unknown-none \
                        --no-default-features \
                        --features qemu-loongarch64-virt,final_online,$(HEAP_ALLOCATOR_FEATURE)$(EXTRA_FEATURE_SUFFIX)$(OPERATOR_FEATURE_SUFFIX)$(GDB_FEATURE_SUFFIX)

kernel_la_build_artifact_path := ./target/loongarch64-unknown-none/$(if $(filter 1,$(GDB_BUILD)),gdb,release)
GDB_PORT ?= $(PORT)
GDB_WAIT ?= $(START_PAUSED)
LA_GDB_ELF ?= ./kernel-la-pre-gdb

SELECTED_BUILD_TARGET := $(if $(filter rv-pre,$(CONFIG_KEY)),kernel-rv-pre,\
                         $(if $(filter rv-final,$(CONFIG_KEY)),kernel-rv-final,\
                         $(if $(filter la-pre,$(CONFIG_KEY)),kernel-la-pre,\
                         $(if $(filter la-final,$(CONFIG_KEY)),kernel-la-final,))))
SELECTED_CHECK_TARGET := $(if $(filter rv,$(ARCH)),rv_check,$(if $(filter la,$(ARCH)),la_check,))
SELECTED_RV_FLAG := $(if $(filter final,$(PROFILE)),$(kernel_rv_final_flag),$(kernel_rv_pre_flag))
SELECTED_LA_FLAG := $(if $(filter final,$(PROFILE)),$(kernel_la_final_flag),$(kernel_la_pre_flag))
SELECTED_KERNEL := ./kernel-$(ARCH)-$(PROFILE)$(GDB_SUFFIX)
SELECTED_RUN_SCRIPT := ./scripts/$(ARCH)_$(PROFILE)_run.sh
DEBUG_PROFILE := $(ARCH)-$(PROFILE)

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
	@echo "WaterOS 统一入口（默认 ARCH=rv PROFILE=pre SMP=8 SNAPSHOT=1）"
	@echo "  make build ARCH=rv PROFILE=pre       构建内核"
	@echo "  make check ARCH=la PROFILE=final     检查内核"
	@echo "  make run ARCH=rv PROFILE=final       执行内核 BRINGUP_COMMANDS"
	@echo "  make shell ARCH=rv PROFILE=pre       进入交互 shell"
	@echo "  make run EXTRA_FEATURES=display-demo 启动 VirtIO GPU 欢迎页"
	@echo "  make run MODE=run SCRIPT=/root/x.sh  构建 operator-run 内核并运行脚本"
	@echo "  make debug ARCH=rv PROFILE=pre       自动启动并 watch"
	@echo "  make debug-server ARCH=rv PROFILE=pre  手动挡：终端一启动 QEMU"
	@echo "  make gdb                             手动挡：终端二连接"
	@echo "  make snapshot / make watch           附加当前活动会话"
	@echo "  make doctor                          检查 GDB/QEMU/符号工具"
	@echo "  make physical-root-image             构建 32 MiB MBR/GPT+ext4 真机根盘"
	@echo "  make verify-physical-root-image      只读检查真机根盘布局与内容"
	@echo "  make show-config                     显示当前最终生效配置"
	@echo ""
	@echo "常用变量：SMP MODE SCRIPT GUEST_SHELL SNAPSHOT WRITE_DISK SDCARD ROOT_IMAGE_PARTITION_TABLE"
	@echo "图形变量：EXTRA_FEATURES=gui GRAPHICS=1 GRAPHICS_BACKEND=auto|gtk|sdl|cocoa|none"
	@echo "镜像默认值：RV_PRE_IMAGE RV_FINAL_IMAGE LA_PRE_IMAGE LA_FINAL_IMAGE"
	@echo "Make 不维护测试目录；auto 模式的测试由内核 bringup 选择。"

show-config:
	@echo "WaterOS 当前配置"
	@printf "  %-18s %s\n" "ARCH" "$(ARCH)"
	@printf "  %-18s %s\n" "PROFILE" "$(PROFILE)"
	@printf "  %-18s %s\n" "KERNEL" "$(SELECTED_KERNEL)"
	@printf "  %-18s %s\n" "SDCARD" "$(SDCARD)"
	@printf "  %-18s %s\n" "SMP" "$(SMP)"
	@printf "  %-18s %s\n" "MODE" "$(MODE)"
	@printf "  %-18s %s\n" "SCRIPT" "$(if $(strip $(SCRIPT)),$(SCRIPT),(空))"
	@printf "  %-18s %s\n" "GUEST_SHELL" "$(if $(strip $(GUEST_SHELL)),$(GUEST_SHELL),(自动选择))"
	@printf "  %-18s %s\n" "OPERATOR_FEATURE" "$(if $(strip $(OPERATOR_FEATURE)),$(OPERATOR_FEATURE),(自动评测队列))"
	@printf "  %-18s %s\n" "SNAPSHOT" "$(SNAPSHOT)"
	@printf "  %-18s %s\n" "WRITE_DISK" "$(WRITE_DISK)"
	@printf "  %-18s %s\n" "PORT" "$(PORT)"
	@printf "  %-18s %s\n" "START_PAUSED" "$(START_PAUSED)"
	@printf "  %-18s %s\n" "FAULTS" "$(FAULTS)"
	@printf "  %-18s %s\n" "EXTRA_FEATURES" "$(if $(strip $(EXTRA_FEATURES)),$(EXTRA_FEATURES),(空))"
	@printf "  %-18s %s\n" "GRAPHICS" "$(GRAPHICS)"
	@printf "  %-18s %s\n" "GRAPHICS_BACKEND" "$(GRAPHICS_BACKEND)"

validate-config:
	@case "$(ARCH)" in rv|la) ;; *) echo "错误: ARCH 必须是 rv 或 la" >&2; exit 2 ;; esac
	@case "$(PROFILE)" in pre|final) ;; *) echo "错误: PROFILE 必须是 pre 或 final" >&2; exit 2 ;; esac
	@case "$(MODE)" in auto|shell|run) ;; *) echo "错误: MODE 必须是 auto/shell/run" >&2; exit 2 ;; esac
	@case "$(SMP)" in 1|2|3|4|5|6|7|8) ;; *) echo "错误: SMP 必须是 1..8" >&2; exit 2 ;; esac
	@case "$(SNAPSHOT)" in 0|1) ;; *) echo "错误: SNAPSHOT 必须是 0 或 1" >&2; exit 2 ;; esac
	@case "$(WRITE_DISK)" in 0|1) ;; *) echo "错误: WRITE_DISK 必须是 0 或 1" >&2; exit 2 ;; esac
	@test -n "$(SDCARD)" || { echo "错误: 未配置根文件系统镜像；请设置对应的 *_IMAGE 或 SDCARD" >&2; exit 2; }
	@case "$(FAULTS)" in 0|1) ;; *) echo "错误: FAULTS 必须是 0 或 1" >&2; exit 2 ;; esac
	@case "$(START_PAUSED)" in 0|1) ;; *) echo "错误: START_PAUSED 必须是 0 或 1" >&2; exit 2 ;; esac
	@case "$(GRAPHICS)" in 0|1) ;; *) echo "错误: GRAPHICS 必须是 0 或 1" >&2; exit 2 ;; esac
	@if [[ "$(MODE)" == run ]]; then \
		test -n "$(SCRIPT)" || { echo "错误: MODE=run 时必须提供 SCRIPT" >&2; exit 2; }; \
		[[ "$(SCRIPT)" == /* ]] || { echo "错误: SCRIPT 必须是 guest 绝对路径" >&2; exit 2; }; \
	elif [[ -n "$(SCRIPT)" ]]; then \
		echo "错误: SCRIPT 只能与 MODE=run 一起使用" >&2; exit 2; \
	fi
	@if [[ -n "$(GUEST_SHELL)" && "$(GUEST_SHELL)" != /* ]]; then \
		echo "错误: GUEST_SHELL 必须是 guest 绝对路径" >&2; exit 2; \
	fi
	@if ! [[ "$(PORT)" =~ ^[0-9]+$$ ]] || (( $(PORT) < 1 || $(PORT) > 65535 )); then \
		echo "错误: PORT 必须是 1..65535" >&2; exit 2; \
	fi

build: validate-config $(SELECTED_BUILD_TARGET)

run: validate-config build
	$(call INFO, "运行 ARCH=$(ARCH) PROFILE=$(PROFILE) MODE=$(MODE) SMP=$(SMP) snapshot=$(SNAPSHOT)")
	@WOS_KERNEL="$(SELECTED_KERNEL)" WOS_SMP="$(SMP)" \
	 WOS_QEMU_SNAPSHOT="$(SNAPSHOT)" \
	 WOS_GRAPHICS="$(GRAPHICS)" WOS_QEMU_DISPLAY="$(GRAPHICS_BACKEND)" \
	 WOS_WRITE_DISK="$(WRITE_DISK)" WOS_SDCARD="$(SDCARD)" \
	 python3 ./scripts/qemu_run.py --arch "$(ARCH)" --profile "$(PROFILE)"

shell:
	@$(MAKE) --no-print-directory run ARCH="$(ARCH)" PROFILE="$(PROFILE)" SMP="$(SMP)" \
		MODE=shell SCRIPT= GUEST_SHELL="$(GUEST_SHELL)" SNAPSHOT="$(SNAPSHOT)" \
		WRITE_DISK="$(WRITE_DISK)" SDCARD="$(SDCARD)" EXTRA_FEATURES="$(EXTRA_FEATURES)"

debug: validate-config
	@WATEROS_MODE="$(MODE)" WATEROS_OPERATOR_SCRIPT="$(SCRIPT)" \
	 WATEROS_OPERATOR_SHELL="$(GUEST_SHELL)" WOS_SDCARD="$(SDCARD)" \
	 WOS_QEMU_SNAPSHOT="$(SNAPSHOT)" WOS_EXTRA_FEATURES="$(EXTRA_FEATURES)" \
	 WOS_GRAPHICS="$(GRAPHICS)" WOS_QEMU_DISPLAY="$(GRAPHICS_BACKEND)" \
	 python3 ./scripts/wateros_debug.py run "$(DEBUG_PROFILE)" --smp "$(SMP)" \
		--port "$(PORT)" $(if $(filter 1,$(WRITE_DISK)),--write-disk,) \
		$(if $(filter 1,$(FAULTS)),--faults,)

debug-server: validate-config
	@WATEROS_MODE="$(MODE)" WATEROS_OPERATOR_SCRIPT="$(SCRIPT)" \
	 WATEROS_OPERATOR_SHELL="$(GUEST_SHELL)" WOS_SDCARD="$(SDCARD)" \
	 WOS_QEMU_SNAPSHOT="$(SNAPSHOT)" WOS_EXTRA_FEATURES="$(EXTRA_FEATURES)" \
	 WOS_GRAPHICS="$(GRAPHICS)" WOS_QEMU_DISPLAY="$(GRAPHICS_BACKEND)" \
	 python3 ./scripts/wateros_debug.py server "$(DEBUG_PROFILE)" --smp "$(SMP)" \
		--port "$(PORT)" $(if $(filter 0,$(START_PAUSED)),--no-start-paused,) \
		$(if $(filter 1,$(WRITE_DISK)),--write-disk,) $(if $(filter 1,$(FAULTS)),--faults,)

snapshot:
	@python3 ./scripts/wateros_debug.py snapshot $(if $(filter 1,$(LEAVE_STOPPED)),--leave-stopped,)

watch:
	@python3 ./scripts/wateros_debug.py watch

gdb:
	@python3 ./scripts/wateros_debug.py gdb

doctor:
	@python3 ./scripts/wateros_debug.py doctor


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

kernel-rv: kernel-rv-best

kernel-rv-best: kernel-rv-final
	@cp ./kernel-rv-final ./kernel-rv

$(kernel_la_build_artifact_path)/wateros: $(common_dependencies) kernel
	$(call INFO, "开始构建 loongarch64 内核...")
	@$(cargo_build) $(kernel_la_flag)
	$(call INFO,"构建成功！");

kernel-la: kernel-la-best

kernel-la-best: kernel-la-final
	@cp ./kernel-la-final ./kernel-la

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

rv_pre_run:
	$(call WARNING, "rv_pre_run 已弃用，请使用 make run ARCH=rv PROFILE=pre")
	@$(MAKE) --no-print-directory run ARCH=rv PROFILE=pre

rv_final_run:
	$(call WARNING, "rv_final_run 已弃用，请使用 make run ARCH=rv PROFILE=final")
	@$(MAKE) --no-print-directory run ARCH=rv PROFILE=final

rv_final_run_log:
	$(call WARNING, "rv_final_run_log 已弃用，请使用统一 run 入口")
	@$(MAKE) --no-print-directory run ARCH=rv PROFILE=final EXTRA_FEATURES=stall-debug 2>&1 | tee output.log

rv_final_debug_run:
	$(call WARNING, "rv_final_debug_run 已弃用，请使用统一 run 入口")
	@$(MAKE) --no-print-directory run ARCH=rv PROFILE=final EXTRA_FEATURES=dashboard-debug

# 兼容旧脚本调用：旧目标按初赛语义执行。
rv_qemu_run: rv_pre_run

rv_qemu_run_snapshot: kernel-rv
	$(call INFO, "开始运行 riscv64 内核 snapshot 模式，不写回 sdcard-rv.img")
	@bash ./scripts/rv_qemu_run_snapshot.sh
	$(call INFO, "riscv64 内核运行结束 snapshot")

.PHONY: rv_qemu_run_snapshot

kernel-la-pre: version
	$(call INFO, "构建 loongarch64 初赛内核...")
	@$(cargo_build) $(kernel_la_pre_flag)
	@cp $(kernel_la_build_artifact_path)/wateros ./kernel-la-pre$(GDB_SUFFIX)

kernel-la-final: version
	$(call INFO, "构建 loongarch64 决赛在线内核...")
	@$(cargo_build) $(kernel_la_final_flag)
	@cp $(kernel_la_build_artifact_path)/wateros ./kernel-la-final$(GDB_SUFFIX)

la_pre_run:
	$(call WARNING, "la_pre_run 已弃用，请使用 make run ARCH=la PROFILE=pre")
	@$(MAKE) --no-print-directory run ARCH=la PROFILE=pre

la_final_run:
	$(call WARNING, "la_final_run 已弃用，请使用 make run ARCH=la PROFILE=final")
	@$(MAKE) --no-print-directory run ARCH=la PROFILE=final

la_final_run_log:
	$(call WARNING, "la_final_run_log 已弃用，请使用统一 run 入口")
	@$(MAKE) --no-print-directory run ARCH=la PROFILE=final EXTRA_FEATURES=stall-debug 2>&1 | tee output.log

# 旧 -gdb 目标保留两终端语义：终端一启动 server，终端二 make gdb。
rv_pre_run-gdb:
	$(call WARNING, "rv_pre_run-gdb 已弃用，请使用 make debug-server ARCH=rv PROFILE=pre")
	@$(MAKE) --no-print-directory debug-server ARCH=rv PROFILE=pre START_PAUSED="$(GDB_WAIT)" PORT="$(GDB_PORT)" FAULTS="$(GDB_FAULTS)"

rv_final_run-gdb rv_final_run_log-gdb rv_final_debug_run-gdb:
	$(call WARNING, "$@ 已弃用，请使用 make debug-server ARCH=rv PROFILE=final")
	@$(MAKE) --no-print-directory debug-server ARCH=rv PROFILE=final \
		START_PAUSED="$(GDB_WAIT)" PORT="$(GDB_PORT)" FAULTS="$(GDB_FAULTS)" \
		EXTRA_FEATURES="$(if $(findstring run_log,$@),stall-debug,$(if $(findstring debug_run,$@),dashboard-debug,))"

la_pre_run-gdb:
	$(call WARNING, "la_pre_run-gdb 已弃用，请使用 make debug-server ARCH=la PROFILE=pre")
	@$(MAKE) --no-print-directory debug-server ARCH=la PROFILE=pre START_PAUSED="$(GDB_WAIT)" PORT="$(GDB_PORT)" FAULTS="$(GDB_FAULTS)"

la_final_run-gdb la_final_run_log-gdb:
	$(call WARNING, "$@ 已弃用，请使用 make debug-server ARCH=la PROFILE=final")
	@$(MAKE) --no-print-directory debug-server ARCH=la PROFILE=final \
		START_PAUSED="$(GDB_WAIT)" PORT="$(GDB_PORT)" FAULTS="$(GDB_FAULTS)" \
		EXTRA_FEATURES="$(if $(findstring run_log,$@),stall-debug,)"

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
	@$(operator_env) $(cargo_env) cargo check $(SELECTED_RV_FLAG)
	$(call INFO, "对 riscv 平台的 cargo check 完成！")

la_check:
	$(call INFO, "开始对 loongarch64 平台进行 cargo check ...")
	@$(operator_env) $(cargo_env) cargo check $(SELECTED_LA_FLAG)
	$(call INFO, "对 loongarch64 平台的 cargo check 完成！")

# rv_link_app_asm:
# 	$(call INFO, "开始生成批处理系统的 asm 文件...")
# 	@bash ./scripts/rv_link_app_gen.bash
# 	$(call INFO, "批处理系统的 asm 文件生成完成！详见./src/riscv/assembly_code/linke_app.asm")

check: validate-config $(SELECTED_CHECK_TARGET)
	$(call INFO, "ARCH=$(ARCH) PROFILE=$(PROFILE) check 完成！")

physical-root-image:
	$(call INFO, "构建物理机/QEMU 共用根盘 $(ROOT_IMAGE)...")
	@python3 $(ROOT_IMAGE_TOOL) build --force --output "$(ROOT_IMAGE)" \
		--manifest "$(ROOT_IMAGE_MANIFEST)" --size-mib "$(ROOT_IMAGE_SIZE_MIB)" \
		--partition-table "$(ROOT_IMAGE_PARTITION_TABLE)" \
		$(if $(strip $(ROOT_IMAGE_DATA_MANIFEST)),--data-manifest "$(ROOT_IMAGE_DATA_MANIFEST)" --data-size-mib "$(ROOT_IMAGE_DATA_SIZE_MIB)",)

verify-physical-root-image:
	$(call INFO, "只读验证根盘 $(ROOT_IMAGE)...")
	@python3 $(ROOT_IMAGE_TOOL) verify --image "$(ROOT_IMAGE)" \
		--manifest "$(ROOT_IMAGE_MANIFEST)" \
		$(if $(strip $(ROOT_IMAGE_DATA_MANIFEST)),--data-manifest "$(ROOT_IMAGE_DATA_MANIFEST)",)

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
all: all_start_info kernel-rv-best kernel-la-best

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
	
