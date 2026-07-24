LOADER_TARGET := x86_64-unknown-uefi
KERNEL_TARGET := x86_64-unknown-none
LOADER := target/$(LOADER_TARGET)/release/andora-bootloader.efi
KERNEL := target/$(KERNEL_TARGET)/release/andora-kernel
IMAGE := target/andora.img
ESP := target/esp
OVMF_CODE ?= /usr/share/OVMF/OVMF_CODE_4M.fd
OVMF_VARS_TEMPLATE ?= /usr/share/OVMF/OVMF_VARS_4M.fd
OVMF_VARS := target/OVMF_VARS.fd

.PHONY: build image run clean check

build:
	cargo build --release -p andora-kernel --target $(KERNEL_TARGET)
	cargo build --release -p andora-bootloader --target $(LOADER_TARGET)

image: build
	mkdir -p $(ESP)/EFI/BOOT
	cp $(LOADER) $(ESP)/EFI/BOOT/BOOTX64.EFI
	cp $(KERNEL) $(ESP)/kernel.elf
	dd if=/dev/zero of=$(IMAGE) bs=1M count=64 status=none
	mformat -i $(IMAGE) -F ::
	mcopy -i $(IMAGE) -s $(ESP)/EFI ::
	mcopy -i $(IMAGE) $(ESP)/kernel.elf ::

$(OVMF_VARS): $(OVMF_VARS_TEMPLATE)
	mkdir -p target
	cp $(OVMF_VARS_TEMPLATE) $(OVMF_VARS)

run: image $(OVMF_VARS)
	qemu-system-x86_64 \
		-machine q35 \
		-m 256M \
		-drive if=pflash,format=raw,readonly=on,file=$(OVMF_CODE) \
		-drive if=pflash,format=raw,file=$(OVMF_VARS) \
		-drive format=raw,file=$(IMAGE) \
		-serial stdio \
		-no-reboot

check:
	cargo check -p andora-bootloader --target $(LOADER_TARGET)
	cargo check -p andora-kernel --target $(KERNEL_TARGET)

clean:
	cargo clean
	