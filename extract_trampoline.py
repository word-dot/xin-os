#!/usr/bin/env python3
"""Extract the raw trampoline from a patched kernel ELF for use with -bios."""
import struct
import sys

def extract(input_elf, output_bin):
    with open(input_elf, 'rb') as f:
        data = f.read()

    if data[:4] != b'\x7fELF':
        raise ValueError("Not an ELF file")

    e_phoff = struct.unpack_from('<Q', data, 32)[0]
    e_phentsize = struct.unpack_from('<H', data, 54)[0]
    e_phnum = struct.unpack_from('<H', data, 56)[0]

    for i in range(e_phnum):
        off = e_phoff + i * e_phentsize
        p_type = struct.unpack_from('<I', data, off)[0]
        p_paddr = struct.unpack_from('<Q', data, off + 24)[0]
        p_offset = struct.unpack_from('<Q', data, off + 8)[0]
        p_filesz = struct.unpack_from('<Q', data, off + 32)[0]

        if p_type == 1 and p_paddr == 0x1c000000:
            with open(output_bin, 'wb') as f:
                f.write(data[p_offset:p_offset + p_filesz])
            print(f"Trampoline extracted: {p_filesz} bytes -> {output_bin}")
            return

    raise ValueError("No trampoline (PT_LOAD at 0x1c000000) found in ELF")

if __name__ == '__main__':
    if len(sys.argv) < 3:
        print(f"Usage: {sys.argv[0]} <input.elf> <output.bin>")
        sys.exit(1)
    extract(sys.argv[1], sys.argv[2])
