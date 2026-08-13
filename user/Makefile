ARCH ?= rv
# Package 组合。all 会由构建器自动选择当前架构支持的全部 package。
# 预设只负责展开为实际列表；也可以直接传逗号分隔的自定义列表。
PACKAGE ?= all
IMAGE_SIZE_MB ?= 256
BLOCK_SIZE ?= 4096
INODE_SIZE ?= 256
BASE_IMAGE ?=
OUTPUT ?=
JOBS ?=
TOOLCHAIN_ARCHIVE ?=
FORCE ?=0

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
else
PACKAGE_LIST := $(PACKAGE)
endif

COMMON := --arch $(ARCH) --packages "$(PACKAGE_LIST)"
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
	@echo "  make image  ARCH=rv PACKAGE=base-layout,busybox,operator-tools"
	@echo "  make overlay ARCH=rv PACKAGE=operator BASE_IMAGE=../os/sdcard-rv.img"
	@echo "  make inspect ARCH=rv"
	@echo "  make test"
	@echo
	@echo "Defaults:"
	@echo "  ARCH=rv"
	@echo "  PACKAGE=all"
	@echo "  IMAGE_SIZE_MB=256 BLOCK_SIZE=4096 INODE_SIZE=256"
	@echo "  JOBS=<host CPU count> OUTPUT=build/images/wateros-<arch>.ext4"
	@echo "Package presets:"
	@echo "  all       = every package supported by ARCH"
	@echo "  minimal   = base-layout,busybox"
	@echo "  operator  = base-layout,busybox,operator-tools"
	@echo "  graphics  = microwindows (dependencies add the base packages)"
	@echo "  other     = a comma-separated custom package list"
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
		$(if $(strip $(OUTPUT)),--output $(OUTPUT),)

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
