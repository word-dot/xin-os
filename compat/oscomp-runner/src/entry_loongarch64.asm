    .section .text.entry
    .globl _start
_start:
    # Set up DMW windows for high-half kernel virtual address mapping
    # DMW0: VSEG 0x0 -> physical, uncached, PLV0
    ori         $t0, $zero, 0x1
    csrwr       $t0, 0x180                # CSR_DMWIN0
    # DMW1: VSEG 0x9 -> physical, cached, PLV0
    ori         $t0, $zero, 0x11
    lu52i.d     $t0, $t0, -1792           # bits[63:52] = 0x900
    csrwr       $t0, 0x181                # CSR_DMWIN1
    # DMW2: VSEG 0xffff -> physical, cached, PLV0
    lu12i.w     $t0, -1                   # set bits[31:12] all to 1
    ori         $t0, $t0, 0x11            # PLV0=1, MAT=1 (cached)
    csrwr       $t0, 0x182                # CSR_DMWIN2
    # Jump to high-half kernel _start at 0xffff_0000_8000_0000
    # (axplat's _start in .text.boot, which sets up stack, MMU, etc.)
    lu12i.w     $t0, -524288              # 0x80000 << 12
    lu32i.d     $t0, -1                   # bits[51:32] = all 1s
    lu52i.d     $t0, $t0, -1              # bits[63:52] = 0xfff
    ori         $t0, $t0, 0
    jirl        $zero, $t0, 0
.size _start, . - _start

    .section .bss.stack
    .align 12
boot_stack:
    .space 4096 * 16
boot_stack_top: