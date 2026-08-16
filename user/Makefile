ARCH ?= rv
# Package 组合。all 会由构建器自动选择当前架构支持的全部 package。
# 预设只负责展开为实际列表；也可以直接传逗号分隔的自定义列表。
PACKAGE ?= all
# 从 PACKAGE 展开的组合中排除 package；依赖被排除 package 的上层 package 也会跳过。
SKIP_PACKAGE ?=
IMAGE_SIZE_MB ?= 256
BLOCK_SIZE ?= 4096
INODE_SIZE ?= 256
BASE_IMAGE ?=
OUTPUT ?=
JOBS ?=
TOOLCHAIN_ARCHIVE ?=
FORCE ?=0
BOOT_DIR ?=
BOOT_SIZE_MB ?= 64
# 整盘镜像（带 GPT/MBR 分区表）开关与参数。
DISK ?= 0
PARTITION_TABLE ?= gpt
DISK_SIZE_MB ?=
FILESYSTEM_IMAGES ?=
FILESYSTEM_TYPES ?=

comma := ,
FILESYSTEM_IMAGE_ARGS := $(foreach image,$(subst $(comma), ,$(FILESYSTEM_IMAGES)),--filesystem-image $(image))
FILESYSTEM_TYPE_ARGS := $(foreach type,$(subst $(comma), ,$(FILESYSTEM_TYPES)),--filesystem-type $(type))

PYTHON ?= python3
TOOL := $(PYTHON) ./tools/userland.py

ifeq ($(PACKAGE),all)
PACKAGE_LIST := all
else ifeq ($(PACKAGE),minimal)
PACKAGE_LIST := base-layout,busybox
else ifeq ($(PACKAGE),operator)
PACKAGE_LIST := base-layout,busybox,operator-tools
else ifeq ($(PACKAGE),graphics)
PACKAGE_LIST := microwindows
else ifeq ($(PACKAGE),jvm)
PACKAGE_LIST := openjdk21
else ifeq ($(PACKAGE),minecraft)
PACKAGE_LIST := minecraft-server
else ifeq ($(PACKAGE),pacman)
PACKAGE_LIST := pacman
else
PACKAGE_LIST := $(PACKAGE)
endif

COMMON := --arch $(ARCH) --packages "$(PACKAGE_LIST)"
ifneq ($(strip $(SKIP_PACKAGE)),)
COMMON += --exclude-packages "$(SKIP_PACKAGE)"
endif
ifneq ($(strip $(JOBS)),)
COMMON += --jobs $(JOBS)
endif

.PHONY: help setup doctor build image overlay inspect test clean distclean

help:
	@echo "WaterOS userland / EXT4 image builder"
	@echo
	@echo "  make setup   ARCH=rv              # install the pinned RISC-V musl toolchain"
	@echo "  make setup   ARCH=la              # install the local LoongArch glibc toolchain"
	@echo "  make doctor ARCH=rv"
	@echo "  make image  ARCH=rv               # all supported packages (default)"
	@echo "  make image  ARCH=rv PACKAGE=minimal"
	@echo "  make image  ARCH=rv PACKAGE=operator"
	@echo "  make image  ARCH=rv PACKAGE=graphics  # Nano-X + Doom"
	@echo "  make image  ARCH=rv SKIP_PACKAGE=mgba # all，但跳过 mGBA 及其依赖者"
	@echo "  make image  ARCH=rv PACKAGE=jvm IMAGE_SIZE_MB=512  # OpenJDK 21 (RV/LA)"
	@echo "  make image  ARCH=rv PACKAGE=minecraft IMAGE_SIZE_MB=1024 # Minecraft + JDK"
	@echo "  make image  ARCH=rv PACKAGE=pacman IMAGE_SIZE_MB=512  # pacman local package install (RV)"
	@echo "  make image  ARCH=rv PACKAGE=base-layout,busybox,operator-tools"
	@echo "  make overlay ARCH=rv PACKAGE=operator BASE_IMAGE=../os/sdcard-rv.img"
	@echo "  make disk   ARCH=rv PACKAGE=minimal BOOT_DIR=../os/build/jh7110-boot  # VF2 整盘（P3 FAT boot + P4 rootfs）"
	@echo "  make disk   ARCH=la PACKAGE=minimal PARTITION_TABLE=mbr FILESYSTEM_IMAGES=a.ext4,b.ext4 FILESYSTEM_TYPES=83,83"
	@echo "  make inspect ARCH=rv"
	@echo "  make test"
	@echo
	@echo "Defaults:"
	@echo "  ARCH=rv"
	@echo "  PACKAGE=all"
	@echo "  SKIP_PACKAGE=<empty>"
	@echo "  IMAGE_SIZE_MB=256 BLOCK_SIZE=4096 INODE_SIZE=256"
	@echo "  JOBS=<host CPU count> OUTPUT=build/images/wateros-<arch>.ext4"
	@echo "  FILESYSTEM_IMAGES=<empty> FILESYSTEM_TYPES=<empty>"
	@echo "Package presets:"
	@echo "  all       = every package supported by ARCH"
	@echo "  minimal   = base-layout,busybox"
	@echo "  operator  = base-layout,busybox,operator-tools"
	@echo "  graphics  = microwindows (dependencies add the base packages)"
	@echo "  jvm       = OpenJDK 21 headless (RV/LA; dependencies add the base packages)"
	@echo "  minecraft = Minecraft Java server + OpenJDK 21 (explicit EULA acceptance)"
	@echo "  pacman    = pacman with local .pkg.tar.zst support (RV only)"
	@echo "  other     = a comma-separated custom package list"
	@echo "  SKIP_PACKAGE = comma-separated exclusions; dependent packages are skipped too"
	@echo "Toolchain overrides: RV_CROSS_COMPILE=... LA_CROSS_COMPILE=..."
	@echo "Offline setup (RV only): TOOLCHAIN_ARCHIVE=/path/to/pinned-toolchain.tar.xz"

setup:
	$(PYTHON) ./tools/setup.py --arch $(ARCH) \
		$(if $(strip $(TOOLCHAIN_ARCHIVE)),--archive $(TOOLCHAIN_ARCHIVE),) \
		$(if $(filter 1,$(FORCE)),--force,)
	$(TOOL) doctor --arch $(ARCH)

doctor:
	$(TOOL) doctor --arch $(ARCH)

build:
	$(TOOL) build $(COMMON)

image:
	$(TOOL) image $(COMMON) --image-size-mb $(IMAGE_SIZE_MB) \
		--block-size $(BLOCK_SIZE) --inode-size $(INODE_SIZE) \
		$(if $(strip $(OUTPUT)),--output $(OUTPUT),) \
		$(if $(filter 1,$(DISK)),--disk --partition-table $(PARTITION_TABLE) \
			$(if $(strip $(DISK_SIZE_MB)),--disk-size-mb $(DISK_SIZE_MB),) \
			$(if $(strip $(BOOT_DIR)),--boot-dir $(BOOT_DIR) \
				--boot-size-mb $(BOOT_SIZE_MB),) \
			$(FILESYSTEM_IMAGE_ARGS) $(FILESYSTEM_TYPE_ARGS),)

disk:
	$(MAKE) image DISK=1

overlay:
	@test -n "$(BASE_IMAGE)" || { echo "BASE_IMAGE is required" >&2; exit 2; }
	$(TOOL) overlay $(COMMON) --base-image $(BASE_IMAGE) \
		$(if $(strip $(OUTPUT)),--output $(OUTPUT),)

inspect:
	$(TOOL) inspect $(COMMON) $(if $(strip $(OUTPUT)),--image $(OUTPUT),)

test:
	$(PYTHON) -m unittest discover -s tests -v

clean:
	$(PYTHON) -c 'import pathlib, shutil; root=pathlib.Path("build"); [shutil.rmtree(root/name, ignore_errors=True) for name in ("images", "manifests", "packages", "staging", "work")]'

distclean:
	$(PYTHON) -c 'import shutil; shutil.rmtree("build", ignore_errors=True)'
