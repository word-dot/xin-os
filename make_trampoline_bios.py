#!/usr/bin/env python3
"""Generate a minimal trampoline binary for -bios that jumps to 0x80000000.

This trampoline is placed at the reset vector (0x1c000000) by QEMU.
It sets up DMW0/DMW1/DMW2 and jumps to the kernel _start at 0x80000000.
"""
import struct, sys

# Trampoline for LoongArch64 reset vector (0x1c000000)
# Sets up DMW windows and jumps to kernel _start at 0x80000000
#
# LoongArch instruction encoding (32-bit, little-endian):
#   ori  $rd, $rj, ui12   -> [11:0]=ui12 [16:12]=rj [20:16]=rd [31:21]=0x0000010000
#   csrwr $rd, csr_num     -> [17:0]=csr_num [20:16]=rd [31:21]=0x0000000100
#   lu12i.w $rd, si20      -> [24:5]=si20 [31:27]=rd [4:0]=0x01010
#   lu32i.d $rd, si20      -> [24:5]=si20 [31:27]=rd [4:0]=0x01011
#   lu52i.d $rd, $rj, si12 -> [21:10]=si12 [16:12]=rj [31:27]=rd [4:0]=0x01100
#   jirl $rd, $rj, offs16  -> [20:5]=offs16 [16:12]=rj [31:27]=rd [4:0]=0x01001

TRAMPOLINE_CODE = bytes([
    # DMW0: VSEG 0x0 -> physical, uncached, PLV0
    # ori $t0, $zero, 0x1
    0x0c, 0x04, 0x80, 0x03,
    # csrwr $t0, 0x180 (DMWIN0)
    0x80, 0x01, 0x00, 0x04,
    # DMW1: VSEG 0x9 -> physical, cached, PLV0
    # ori $t0, $zero, 0x11
    0x0c, 0x44, 0x80, 0x03,
    # lu52i.d $t0, $t0, -1792 (VSEG 0x900)
    0x8c, 0x01, 0x24, 0x03,
    # csrwr $t0, 0x181 (DMWIN1)
    0x84, 0x01, 0x00, 0x04,
    # DMW2: VSEG 0xffff -> physical, cached, PLV0
    # lu12i.w $t0, -1  (bits[31:12] = all 1s)
    0x0c, 0x00, 0x00, 0x15,
    # ori $t0, $t0, 0x11  (PLV0=1, MAT=1 cached)
    0x8c, 0x10, 0x80, 0x03,
    # csrwr $t0, 0x182 (DMWIN2)
    0x88, 0x01, 0x00, 0x04,
    # Jump to kernel _start at physical address 0x80000000
    # lu12i.w $t0, -524288  (0x80000 << 12 = 0x80000000 bits[31:12])
    0x0c, 0x00, 0x00, 0x15,
    # lu32i.d $t0, 0  (bits[51:32] = 0)
    0x0c, 0x00, 0x00, 0x16,
    # lu52i.d $t0, $t0, 0  (bits[63:52] = 0, keep physical address)
    0x8c, 0x01, 0x00, 0x03,
    # ori $t0, $t0, 0  (no-op, ensures lower 12 bits are 0)
    0x8c, 0x01, 0x80, 0x03,
    # jirl $zero, $t0, 0  (jump to 0x80000000)
    0x80, 0x01, 0x00, 0x4c,
])

output = sys.argv[1] if len(sys.argv) > 1 else "trampoline-la.bin"
with open(output, "wb") as f:
    f.write(TRAMPOLINE_CODE)
print(f"Generated {len(TRAMPOLINE_CODE)}-byte trampoline -> {output}")
