# Minimal LoongArch64 QEMU trampoline
# Placed at the reset vector (0x1c000000) by the linker script.
# It:
# 1. Configures DMW0 to map VSEG 0x0 (0x0000...) -> physical (uncached), PLV0
# 2. Configures DMW1 to map VSEG 0x9 (0x9000...) -> physical (cached), PLV0
# 3. Configures DMW2 to map VSEG 0xffff (0xffff...) -> physical (cached), PLV0
# 4. Jumps to kernel _start at virtual address 0xffff_0000_8000_0000
#
# With all three DMW windows set up, the kernel's virtual addresses
# (0xffff_0000_8xxx_xxxx) map directly to physical addresses.

.section .trampoline, "ax"
.globl _trampoline_start
_trampoline_start:
    # Configure DMW0: identity-map VSEG 0x0 (0x0000...) uncached, PLV0
    ori         $t0, $zero, 0x1
    csrwr       $t0, 0x180                # CSR_DMWIN0

    # Configure DMW1: identity-map VSEG 0x9 (0x9000...) cached, PLV0
    ori         $t0, $zero, 0x11
    lu52i.d     $t0, $t0, -1792           # bits[63:52] = 0x900
    csrwr       $t0, 0x181                # CSR_DMWIN1

    # Configure DMW2: identity-map VSEG 0xffff (0xffff...) cached, PLV0
    # This maps the kernel's virtual address space (0xffff_0000_8000_0000)
    # directly to physical addresses.
    lu12i.w     $t0, -1                   # set bits[31:12] all to 1
    ori         $t0, $t0, 0x11            # PLV0=1, MAT=1 (cached)
    csrwr       $t0, 0x182                # CSR_DMWIN2

    # Jump to kernel _start at virtual address 0xffff_0000_8000_0000
    lu12i.w     $t0, -524288              # loads 0x80000 << 12 into bits[31:12]
    lu32i.d     $t0, -1                   # bits[51:32] = all 1s -> VSEG 0xffff
    lu52i.d     $t0, $t0, -1              # bits[63:52] = 0xfff
    ori         $t0, $t0, 0
    jirl        $zero, $t0, 0
