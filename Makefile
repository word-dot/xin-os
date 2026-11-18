export RUSTUP_TOOLCHAIN := nightly-2025-05-20
export RUSTUP_AUTO_INSTALL := 0
export RUSTUP_OFFLINE := 1

ROOT := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
BUILD_RUNNER := $(ROOT)/compat/tools/build_runner.sh
RUN_SUITE := $(ROOT)/compat/tools/run-suite.sh
MAKE_DISK := $(ROOT)/compat/tools/make_suite_disk.sh
RUN_SYSTEM := $(ROOT)/compat/tools/run-system-qemu.sh

arch ?= riscv64
file ?=

SUPPORTED_ARCHES := riscv64 loongarch64
SUPPORTED_SUITES := basic busybox lua libctest iozone unixbench iperf libcbench lmbench cyclictest ltp netperf

# Image download configuration
IMG_URL = https://github.com/oscomp/testsuits-for-oskernel/releases/download/pre-20250615/

ifeq ($(arch), riscv64)
	IMG := sdcard-rv.img
else ifeq ($(arch), loongarch64)
	IMG := sdcard-la.img
else
	$(error Unsupported architecture: $(arch))
endif

.DEFAULT_GOAL := all

all: kernel-rv kernel-la

kernel-rv:
	@$(MAKE) build-kernel arch=riscv64 output=kernel-rv AXLOG_LEVEL=debug
	@python3 $(ROOT)/patch_elf.py $(ROOT)/kernel-rv

kernel-la:
	@$(MAKE) build-kernel arch=loongarch64 output=kernel-la-orig.elf
	@python3 $(ROOT)/patch_elf_la.py $(ROOT)/kernel-la-orig.elf $(ROOT)/kernel-la-patched.elf
	@python3 $(ROOT)/combine_la.py $(ROOT)/kernel-la-patched.elf $(ROOT)/kernel-la
	@rm -f $(ROOT)/kernel-la-orig.elf $(ROOT)/kernel-la-patched.elf

build-kernel:
	@if [ -z "$(output)" ]; then echo "missing output=<path>"; exit 1; fi
	@if ! printf '%s\n' $(SUPPORTED_ARCHES) | grep -qx "$(arch)"; then \
		echo "unsupported arch: $(arch)"; \
		echo "supported arches: $(SUPPORTED_ARCHES)"; \
		exit 1; \
	fi
	@rm -f "$(ROOT)"/compat/oscomp-runner/oscomp-runner_*.elf "$(ROOT)/$(output)"
	@"$(BUILD_RUNNER)" "$(arch)" build
	@src="$$(ls -t "$(ROOT)"/compat/oscomp-runner/oscomp-runner_*.elf 2>/dev/null | head -n 1)"; \
	if [ -z "$$src" ]; then \
		echo "unable to locate built ELF under compat/oscomp-runner/"; \
		exit 1; \
	fi; \
	cp "$$src" "$(ROOT)/$(output)"; \
	chmod +x "$(ROOT)/$(output)"; \
	echo "generated $(output) from $$src"

build-suites-rv:
	@$(MAKE) -C "$(ROOT)/testsuits-for-oskernel" build-rv

build-suites-la:
	@$(MAKE) -C "$(ROOT)/testsuits-for-oskernel" build-la

# Download and prepare disk images
download-img-rv:
	@if [ ! -f $(ROOT)/$(IMG) ]; then \
		echo "Downloading $(IMG)..."; \
		wget $(IMG_URL)/$(IMG).xz -O $(ROOT)/$(IMG).xz; \
		xz -d $(ROOT)/$(IMG).xz; \
	fi

download-img-la:
	@if [ ! -f $(ROOT)/sdcard-la.img ]; then \
		echo "Downloading sdcard-la.img..."; \
		wget $(IMG_URL)/sdcard-la.img.xz -O $(ROOT)/sdcard-la.img.xz; \
		xz -d $(ROOT)/sdcard-la.img.xz; \
	fi

disk-rv: download-img-rv
	@echo "RISC-V disk image ready: $(ROOT)/sdcard-rv.img"

disk-la: download-img-la
	@echo "LoongArch disk image ready: $(ROOT)/sdcard-la.img"

disk-images: disk-rv disk-la
	@echo "All disk images ready."

suite:
	@if [ -z "$(file)" ]; then \
		echo "usage: make suite file=<$(firstword $(SUPPORTED_SUITES))|...> [arch=riscv64|loongarch64]"; \
		echo "supported suites: $(SUPPORTED_SUITES)"; \
		exit 1; \
	fi
	@"$(RUN_SUITE)" "$(file)" "$(arch)"

suite-all:
	@"$(RUN_SUITE)" all "$(arch)"

run-system:
	@"$(RUN_SYSTEM)" "$(arch)"

clean:
	@rm -f "$(ROOT)"/kernel-rv "$(ROOT)"/kernel-la "$(ROOT)"/kernel-la.elf "$(ROOT)"/kernel-la-patched.elf
	@rm -f "$(ROOT)"/sdcard-rv.img "$(ROOT)"/sdcard-la.img
	@rm -f "$(ROOT)"/disk-rv.img "$(ROOT)"/disk-la.img "$(ROOT)"/disk.img
	@rm -f "$(ROOT)"/compat/oscomp-runner/oscomp-runner_*.elf
	@rm -f "$(ROOT)"/compat/oscomp-runner/oscomp-runner_*.bin
	@rm -f "$(ROOT)"/compat/oscomp-runner/oscomp-runner_*.uimg
	@rm -f "$(ROOT)"/compat/oscomp-runner/oscomp-runner_*.efi
	@rm -f "$(ROOT)"/trampoline-la.bin

# Local test targets with automatic image download

SDCARD_RV ?= $(ROOT)/sdcard-rv.img
run-rv: kernel-rv download-img-rv
	@echo "Starting RISC-V QEMU..."
	@qemu-system-riscv64 \
		-machine virt \
		-kernel $(ROOT)/kernel-rv \
		-m 1G \
		-nographic \
		-smp 1 \
		-bios default \
		-drive file=$(SDCARD_RV),if=none,format=raw,id=x0 \
		-device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 \
		-no-reboot \
		-device virtio-net-device,netdev=net \
		-netdev user,id=net \
		-rtc base=utc

DOCKER_IMAGE ?= zhouzhouyi/os-contest:20260104
kernel-rv-patched: kernel-rv
	@python3 $(ROOT)/patch_elf.py $(ROOT)/kernel-rv $(ROOT)/kernel-rv-patched

docker-run-rv: kernel-rv-patched download-img-rv
	@echo "Starting RISC-V QEMU in Docker..."
	@docker run --rm -i \
		-v $(ROOT):/code -w /code \
		$(DOCKER_IMAGE) \
		qemu-system-riscv64 \
			-machine virt \
			-kernel /code/kernel-rv-patched \
			-m 1G \
			-nographic \
			-smp 1 \
			-bios default \
			-drive file=/code/sdcard-rv.img,if=none,format=raw,id=x0 \
			-device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 \
			-no-reboot \
			-device virtio-net-device,netdev=net \
			-netdev user,id=net \
			-rtc base=utc

SDCARD_LA ?= $(ROOT)/sdcard-la.img
run-la: kernel-la download-img-la
	@echo "Starting LoongArch QEMU..."
	@qemu-system-loongarch64 \
		-machine virt \
		-kernel $(ROOT)/kernel-la \
		-m 1G \
		-nographic \
		-smp 1 \
		-drive file=$(SDCARD_LA),if=none,format=raw,id=x0 \
		-device virtio-blk-pci,drive=x0 \
		-no-reboot \
		-device virtio-net-pci,netdev=net0 \
		-netdev user,id=net0 \
		-rtc base=utc

info:
	@echo "ARCH_NAME: $(arch)"
	@echo "ROOT: $(ROOT)"
	@echo "OUTPUT: kernel-rv / kernel-la"
	@echo "PLATFORM CONFIG: platconfig/riscv64-qemu-virt.toml"
	@echo "QEMU (RISC-V): $(shell which qemu-system-riscv64 || echo 'not found')"
	@echo "QEMU (LoongArch): $(shell which qemu-system-loongarch64 || echo 'not found')"
	@echo "IMG_URL: $(IMG_URL)"

docker:
	docker run --rm -it -v $(ROOT):/code -w /code --privileged $(DOCKER_IMAGE) bash

.PHONY: all kernel-rv kernel-la build-kernel build-suites-rv build-suites-la disk-rv disk-la disk-images \
        suite suite-all run-system clean run-rv run-la info docker kernel-rv-patched docker-run-rv \
        download-img-rv download-img-la