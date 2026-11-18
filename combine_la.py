#!/usr/bin/env python3
"""
Combine a LoongArch64 trampoline with the kernel ELF.

The trampoline (hardcoded LoongArch machine code) is placed at the
reset vector (0x1c000000) with a new PT_LOAD segment. The kernel ELF's
existing PT_LOAD segments are kept as-is for patching by patch_elf_la.py.

Output: a combined ELF that boots directly on QEMU without firmware.
"""

import struct
import sys
import os

# Trampoline for LoongArch64 reset vector (0x1c000000)
# This code:
# 1. Sets up DMW0 (VSEG 0x0 -> physical, uncached, PLV0)
# 2. Sets up DMW1 (VSEG 0x9 -> physical, cached, PLV0)
# 3. Sets up DMW2 (VSEG 0xffff -> physical, cached, PLV0)
# 4. Jumps to kernel _start at 0xFFFF000080000000 (via DMW2 -> phys 0x80000000)
TRAMPOLINE_CODE = bytes([
    0x0c, 0x04, 0x80, 0x03,  # ori     $t0, $zero, 0x1
    0x2c, 0x00, 0x06, 0x04,  # csrwr   $t0, 0x180  (DMW0)
    0x0c, 0x44, 0x80, 0x03,  # ori     $t0, $zero, 0x11
    0x8c, 0x01, 0x24, 0x03,  # lu52i.d $t0, $t0, -1792  (set VSEG 0x900 for DMW1)
    0x2c, 0x04, 0x06, 0x04,  # csrwr   $t0, 0x181  (DMW1)
    0x0c, 0x44, 0x80, 0x03,  # ori     $t0, $zero, 0x11
    0x0c, 0x00, 0xe0, 0x17,  # lu32i.d $t0, 0xF0000  (set bits[51:48]=0xF for VSEG 0xFFFF)
    0x8c, 0xfd, 0x3f, 0x03,  # lu52i.d $t0, $t0, -1  (set bits[63:52]=0xFFF)
    0x2c, 0x08, 0x06, 0x04,  # csrwr   $t0, 0x182  (DMW2)
    0x0c, 0x00, 0x00, 0x15,  # lu12i.w $t0, -524288  (0x80000000 in lower 32 bits)
    0x0c, 0x00, 0xe0, 0x17,  # lu32i.d $t0, 0xF0000  (set bits[51:48]=0xF for VSEG 0xFFFF)
    0x8c, 0xfd, 0x3f, 0x03,  # lu52i.d $t0, $t0, -1  (set bits[63:52]=0xFFF)
    0x80, 0x01, 0x00, 0x4c,  # jirl    $zero, $t0, 0  (jump to 0xFFFF000080000000)
])

TRAMPOLINE_VADDR = 0x1C000000
TRAMPOLINE_PADDR = 0x1C000000


def combine(kernel_elf_path, output_path):
    with open(kernel_elf_path, 'rb') as f:
        kernel_elf = bytearray(f.read())

    if kernel_elf[:4] != b'\x7fELF':
        raise ValueError("Kernel is not an ELF file")

    ei_class = kernel_elf[4]
    if ei_class != 2:
        raise ValueError("Only 64-bit ELF supported")

    e_entry = struct.unpack_from('<Q', kernel_elf, 24)[0]
    e_phoff = struct.unpack_from('<Q', kernel_elf, 32)[0]
    e_phentsize = struct.unpack_from('<H', kernel_elf, 54)[0]
    e_phnum = struct.unpack_from('<H', kernel_elf, 56)[0]
    e_shoff = struct.unpack_from('<Q', kernel_elf, 40)[0]

    print(f"Kernel ELF: {e_phnum} program headers, entry={e_entry:#018x}")

    # Find the end of file data used by existing segments
    last_file_offset = len(kernel_elf)

    # Place trampoline at 8-byte aligned offset at end of file
    trampoline_file_offset = (last_file_offset + 7) & ~7

    # Build new ELF
    new_elf = bytearray(kernel_elf)
    new_elf.extend(b'\x00' * (trampoline_file_offset - len(kernel_elf)))
    new_elf.extend(TRAMPOLINE_CODE)

    # Add new program header after existing ones
    new_phdr_offset = e_phoff + e_phnum * e_phentsize
    new_phdr_end = new_phdr_offset + e_phentsize

    # If section headers are in the way, move them to end of file
    if e_shoff > 0 and new_phdr_end > e_shoff:
        print("Moving section headers to make room for new program header")
        e_shentsize = struct.unpack_from('<H', kernel_elf, 58)[0]
        e_shnum = struct.unpack_from('<H', kernel_elf, 60)[0]
        sh_size = e_shentsize * e_shnum
        old_sh_start = e_shoff

        # Append space for section headers
        new_sh_start = len(new_elf)
        new_elf.extend(b'\x00' * sh_size)
        new_elf[new_sh_start:new_sh_start + sh_size] = kernel_elf[old_sh_start:old_sh_start + sh_size]
        struct.pack_into('<Q', new_elf, 40, new_sh_start)

        # Zero old section headers
        new_elf[old_sh_start:old_sh_start + sh_size] = b'\x00' * sh_size

    # Create new PT_LOAD for trampoline
    new_phdr = bytearray(e_phentsize)
    struct.pack_into('<I', new_phdr, 0, 1)          # p_type = PT_LOAD
    struct.pack_into('<I', new_phdr, 4, 0x5)         # p_flags = PF_R | PF_X
    struct.pack_into('<Q', new_phdr, 8, trampoline_file_offset)
    struct.pack_into('<Q', new_phdr, 16, TRAMPOLINE_VADDR)
    struct.pack_into('<Q', new_phdr, 24, TRAMPOLINE_PADDR)
    struct.pack_into('<Q', new_phdr, 32, len(TRAMPOLINE_CODE))  # p_filesz
    struct.pack_into('<Q', new_phdr, 40, len(TRAMPOLINE_CODE))  # p_memsz
    struct.pack_into('<Q', new_phdr, 48, 0x10000)   # p_align

    # Insert new program header
    if new_phdr_end > len(new_elf):
        new_elf.extend(b'\x00' * (new_phdr_end - len(new_elf)))
    new_elf[new_phdr_offset:new_phdr_end] = new_phdr

    # Update e_phnum
    struct.pack_into('<H', new_elf, 56, e_phnum + 1)

    # Update entry point
    struct.pack_into('<Q', new_elf, 24, TRAMPOLINE_VADDR)

    print(f"New entry point: {TRAMPOLINE_VADDR:#010x}")
    print(f"Trampoline PT_LOAD: offset={trampoline_file_offset:#x} vaddr={TRAMPOLINE_VADDR:#x}")
    print(f"Output: {len(new_elf)} bytes ({e_phnum + 1} program headers)")

    with open(output_path, 'wb') as f:
        f.write(new_elf)

    os.chmod(output_path, 0o755)
    return output_path


if __name__ == '__main__':
    if len(sys.argv) < 3:
        print(f"Usage: {sys.argv[0]} <kernel.elf> <output.elf>")
        print("Adds a trampoline PT_LOAD at 0x1c000000 to the kernel ELF.")
        sys.exit(1)
    combine(sys.argv[1], sys.argv[2])
