.section .text
.globl _trampoline_start
_trampoline_start:
    # DMW0: VSEG 0x0 -> physical, uncached, PLV0
    ori         $t0, $zero, 0x1
    csrwr       $t0, 0x180

    # DMW1: VSEG 0x9 -> physical, cached, PLV0
    ori         $t0, $zero, 0x11
    lu52i.d     $t0, $t0, -1792
    csrwr       $t0, 0x181

    # DMW2: VSEG 0xffff -> physical, cached, PLV0
    lu12i.w     $t0, -1
    ori         $t0, $t0, 0x11
    csrwr       $t0, 0x182

    # Jump to kernel _start at physical address 0x80000000
    lu12i.w     $t0, -524288
    lu32i.d     $t0, 0
    lu52i.d     $t0, $t0, 0
    ori         $t0, $t0, 0
    jirl        $zero, $t0, 0
