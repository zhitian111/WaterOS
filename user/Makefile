ARCH ?= rv
PROFILE ?= minimal
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
COMMON := --arch $(ARCH) --profile $(PROFILE)
ifneq ($(strip $(JOBS)),)
COMMON += --jobs $(JOBS)
endif

.PHONY: help setup doctor build image overlay inspect test clean distclean

help:
	@echo "WaterOS userland / EXT4 image builder"
	@echo
	@echo "  make setup   ARCH=rv              # download and install the pinned toolchain"
	@echo "  make doctor ARCH=rv"
	@echo "  make build  ARCH=rv PROFILE=minimal"
	@echo "  make image  ARCH=rv PROFILE=minimal IMAGE_SIZE_MB=256"
	@echo "  make image  ARCH=rv PROFILE=nanox    # Nano-X graphical rootfs"
	@echo "  make overlay ARCH=rv PROFILE=operator BASE_IMAGE=../os/sdcard-rv.img"
	@echo "  make inspect ARCH=rv PROFILE=minimal"
	@echo "  make test"
	@echo
	@echo "Defaults:"
	@echo "  ARCH=rv"
	@echo "  PROFILE=minimal"
	@echo "  IMAGE_SIZE_MB=256 BLOCK_SIZE=4096 INODE_SIZE=256"
	@echo "  JOBS=<host CPU count> OUTPUT=<derived from arch/profile>"
	@echo "Toolchain overrides: RV_CROSS_COMPILE=... LA_CROSS_COMPILE=..."
	@echo "Offline setup: TOOLCHAIN_ARCHIVE=/path/to/pinned-toolchain.tar.xz"

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
