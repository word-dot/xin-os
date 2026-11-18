#!/usr/bin/env python3
"""Convert a patched LoongArch64 ELF kernel to a PE/COFF UEFI application.

This script takes a kernel ELF that has been patched to physical addresses
(via patch_elf_la.py) and creates a PE/COFF image that can be booted by
EDK2 UEFI firmware (QEMU_EFI.fd) via the -kernel parameter.

The PE/COFF image wraps the kernel at its physical address (0x80000000),
skipping the trampoline at 0x1c000000. The UEFI firmware loads the PE/COFF
at ImageBase and jumps to _start.
"""

import struct
import sys
import os

# PE/COFF constants
IMAGE_FILE_MACHINE_LOONGARCH64 = 0x6264
IMAGE_SUBSYSTEM_EFI_APPLICATION = 10


def round_up(val, align):
    return (val + align - 1) & ~(align - 1)


def make_efi(elf_path, output_path=None):
    if output_path is None:
        base, _ = os.path.splitext(elf_path)
        output_path = f"{base}.efi"

    with open(elf_path, "rb") as f:
        elf_data = bytearray(f.read())

    if elf_data[:4] != b"\x7fELF":
        raise ValueError("Not an ELF file")

    ei_class = elf_data[4]
    if ei_class != 2:
        raise ValueError("Only 64-bit ELF supported")

    # --- Parse ELF header ---
    e_entry = struct.unpack_from("<Q", elf_data, 24)[0]
    e_phoff = struct.unpack_from("<Q", elf_data, 32)[0]
    e_shoff = struct.unpack_from("<Q", elf_data, 40)[0]
    e_phentsize = struct.unpack_from("<H", elf_data, 54)[0]
    e_phnum = struct.unpack_from("<H", elf_data, 56)[0]
    e_shentsize = struct.unpack_from("<H", elf_data, 58)[0]
    e_shnum = struct.unpack_from("<H", elf_data, 60)[0]
    e_shstrndx = struct.unpack_from("<H", elf_data, 62)[0]

    print(f"ELF entry point: {e_entry:#018x}")

    # --- Collect kernel LOAD segments (skip trampoline at 0x1c000000) ---
    kernel_segments = []
    kernel_start = None
    kernel_end = None

    for i in range(e_phnum):
        phdr_off = e_phoff + i * e_phentsize
        p_type = struct.unpack_from("<I", elf_data, phdr_off)[0]
        if p_type == 1:  # PT_LOAD
            p_offset = struct.unpack_from("<Q", elf_data, phdr_off + 8)[0]
            p_vaddr = struct.unpack_from("<Q", elf_data, phdr_off + 16)[0]
            p_paddr = struct.unpack_from("<Q", elf_data, phdr_off + 24)[0]
            p_filesz = struct.unpack_from("<Q", elf_data, phdr_off + 32)[0]
            p_memsz = struct.unpack_from("<Q", elf_data, phdr_off + 40)[0]

            # Skip trampoline segment (at physical 0x1c000000)
            if p_paddr & 0xFFFFFFFF == 0x1C000000:
                print(
                    f"  Skip trampoline: vaddr={p_vaddr:#018x} paddr={p_paddr:#018x}"
                )
                continue

            kernel_segments.append(
                {
                    "offset": p_offset,
                    "vaddr": p_vaddr,
                    "paddr": p_paddr,
                    "filesz": p_filesz,
                    "memsz": p_memsz,
                }
            )

            if kernel_start is None or p_paddr < kernel_start:
                kernel_start = p_paddr
            seg_end = p_paddr + p_memsz
            if kernel_end is None or seg_end > kernel_end:
                kernel_end = seg_end

    if not kernel_segments:
        raise ValueError("No kernel segments found (skipping trampoline)")

    print(f"Kernel physical range: {kernel_start:#018x} - {kernel_end:#018x}")

    # --- Find _start address from ELF symbol table ---
    start_addr = None
    try:
        shstr_off = e_shoff + e_shstrndx * e_shentsize
        # shstrtab offset
        strtab_off = struct.unpack_from("<Q", elf_data, shstr_off + 24)[0]

        for i in range(e_shnum):
            sh_off = e_shoff + i * e_shentsize
            sh_type = struct.unpack_from("<I", elf_data, sh_off + 4)[0]
            if sh_type == 2:  # SHT_SYMTAB
                sh_offset = struct.unpack_from("<Q", elf_data, sh_off + 24)[0]
                sh_size = struct.unpack_from("<Q", elf_data, sh_off + 32)[0]
                sh_entsize = struct.unpack_from("<Q", elf_data, sh_off + 56)[0]
                sh_link = struct.unpack_from("<I", elf_data, sh_off + 40)[0]

                # Symbol string table
                str_sh_off = e_shoff + sh_link * e_shentsize
                str_offset = struct.unpack_from("<Q", elf_data, str_sh_off + 24)[0]

                for j in range(0, sh_size, sh_entsize):
                    sym_off = sh_offset + j
                    sym_name = struct.unpack_from("<I", elf_data, sym_off)[0]
                    sym_value = struct.unpack_from("<Q", elf_data, sym_off + 8)[0]

                    name_end = elf_data.find(b"\x00", str_offset + sym_name)
                    name = elf_data[str_offset + sym_name : name_end].decode(
                        "utf-8", errors="ignore"
                    )
                    if name == "_start":
                        # Symbol table has virtual addresses; use lower 32 bits (physical)
                        start_addr = sym_value & 0xFFFFFFFF
                        print(f"Found _start at address: {sym_value:#018x} -> physical {start_addr:#010x}")
                        break
                break
    except Exception as e:
        print(f"Warning: could not parse symbol table: {e}")

    # Fallback: _start is at the beginning of the first kernel segment
    if start_addr is None:
        sorted_segs = sorted(kernel_segments, key=lambda s: s["paddr"])
        start_addr = sorted_segs[0]["paddr"]
        print(f"Fallback: using first kernel segment start as _start: {start_addr:#018x}")

    # --- Build flat kernel binary from segments ---
    image_base = kernel_start
    kernel_size = kernel_end - kernel_start
    kernel_bin = bytearray(kernel_size)

    for seg in kernel_segments:
        bin_offset = seg["paddr"] - image_base
        segment_data = elf_data[seg["offset"] : seg["offset"] + seg["filesz"]]
        kernel_bin[bin_offset : bin_offset + seg["filesz"]] = segment_data

    # --- PE/COFF layout ---
    section_alignment = 0x1000  # 4K
    file_alignment = 0x200      # 512 bytes

    entry_rva = start_addr - image_base

    # Section layout
    num_sections = 1
    dos_hdr_sz = 64
    pe_sig_sz = 4
    coff_hdr_sz = 20
    opt_hdr_sz = 112  # PE32+
    sec_hdr_sz = 40

    total_hdr_sz = (
        dos_hdr_sz + pe_sig_sz + coff_hdr_sz + opt_hdr_sz + num_sections * sec_hdr_sz
    )
    hdrs_aligned = round_up(total_hdr_sz, file_alignment)

    sec_rva = 0  # Place section at RVA 0 -- entry is relative to image base
    sec_raw_off = hdrs_aligned
    sec_raw_sz = round_up(len(kernel_bin), file_alignment)
    sec_virt_sz = round_up(len(kernel_bin), section_alignment)

    size_of_image = sec_rva + sec_virt_sz

    # --- Build PE/COFF ---
    pe_data = bytearray()

    # 1. DOS Header
    dos = bytearray(64)
    dos[0:2] = b"MZ"
    struct.pack_into("<I", dos, 60, 64)  # e_lfanew -> PE signature right after DOS
    pe_data.extend(dos)

    # 2. PE Signature
    pe_data.extend(b"PE\x00\x00")

    # 3. COFF Header
    ch = bytearray(20)
    struct.pack_into("<H", ch, 0, IMAGE_FILE_MACHINE_LOONGARCH64)  # Machine
    struct.pack_into("<H", ch, 2, num_sections)                    # NumberOfSections
    struct.pack_into("<I", ch, 4, 0)  # TimeDateStamp
    struct.pack_into("<I", ch, 8, 0)  # PointerToSymbolTable
    struct.pack_into("<I", ch, 12, 0)  # NumberOfSymbols
    struct.pack_into("<H", ch, 16, opt_hdr_sz)  # SizeOfOptionalHeader
    # Characteristics: EXECUTABLE | LINE_NUMS_STRIPPED | LOCAL_SYMS_STRIPPED | DEBUG_STRIPPED
    struct.pack_into("<H", ch, 18, 0x0002 | 0x0004 | 0x0008 | 0x0200)
    pe_data.extend(ch)

    # 4. PE32+ Optional Header
    oh = bytearray(opt_hdr_sz)
    struct.pack_into("<H", oh, 0, 0x020B)   # Magic PE32+
    struct.pack_into("<B", oh, 2, 0)         # MajorLinkerVersion
    struct.pack_into("<B", oh, 3, 0)         # MinorLinkerVersion
    struct.pack_into("<I", oh, 4, sec_raw_sz)  # SizeOfCode
    struct.pack_into("<I", oh, 8, 0)         # SizeOfInitializedData
    struct.pack_into("<I", oh, 12, 0)        # SizeOfUninitializedData
    struct.pack_into("<I", oh, 16, entry_rva)   # AddressOfEntryPoint
    struct.pack_into("<I", oh, 20, sec_rva)     # BaseOfCode
    struct.pack_into("<Q", oh, 24, image_base)  # ImageBase
    struct.pack_into("<I", oh, 32, section_alignment)
    struct.pack_into("<I", oh, 36, file_alignment)
    struct.pack_into("<H", oh, 40, 0)  # MajorOSVersion
    struct.pack_into("<H", oh, 42, 0)  # MinorOSVersion
    struct.pack_into("<H", oh, 44, 0)  # MajorImageVersion
    struct.pack_into("<H", oh, 46, 0)  # MinorImageVersion
    struct.pack_into("<H", oh, 48, 0)  # MajorSubsystemVersion
    struct.pack_into("<H", oh, 50, 0)  # MinorSubsystemVersion
    struct.pack_into("<I", oh, 52, 0)  # Win32VersionValue
    struct.pack_into("<I", oh, 56, size_of_image)   # SizeOfImage
    struct.pack_into("<I", oh, 60, hdrs_aligned)    # SizeOfHeaders
    struct.pack_into("<I", oh, 64, 0)               # CheckSum
    struct.pack_into("<H", oh, 68, IMAGE_SUBSYSTEM_EFI_APPLICATION)  # Subsystem
    struct.pack_into("<H", oh, 70, 0)               # DllCharacteristics
    struct.pack_into("<Q", oh, 72, 0)               # SizeOfStackReserve
    struct.pack_into("<Q", oh, 80, 0)               # SizeOfStackCommit
    struct.pack_into("<Q", oh, 88, 0)               # SizeOfHeapReserve
    struct.pack_into("<Q", oh, 96, 0)               # SizeOfHeapCommit
    struct.pack_into("<I", oh, 104, 0)              # LoaderFlags
    struct.pack_into("<I", oh, 108, 0)              # NumberOfRvaAndSizes
    pe_data.extend(oh)

    # 5. Section Header
    sh = bytearray(40)
    sh[0:8] = b".text\x00\x00\x00"
    struct.pack_into("<I", sh, 8, sec_virt_sz)    # VirtualSize
    struct.pack_into("<I", sh, 12, sec_rva)       # VirtualAddress
    struct.pack_into("<I", sh, 16, sec_raw_sz)    # SizeOfRawData
    struct.pack_into("<I", sh, 20, sec_raw_off)   # PointerToRawData
    struct.pack_into("<I", sh, 24, 0)  # PointerToRelocations
    struct.pack_into("<I", sh, 28, 0)  # PointerToLinenumbers
    struct.pack_into("<H", sh, 32, 0)  # NumberOfRelocations
    struct.pack_into("<H", sh, 34, 0)  # NumberOfLinenumbers
    # CODE | MEM_EXECUTE | MEM_READ | MEM_WRITE
    scn_chars = 0x00000020 | 0x20000000 | 0x40000000 | 0x80000000
    struct.pack_into("<I", sh, 36, scn_chars)
    pe_data.extend(sh)

    # 6. Pad to file alignment
    while len(pe_data) < hdrs_aligned:
        pe_data.append(0)

    # 7. Section data
    pe_data.extend(kernel_bin)
    while len(pe_data) < sec_raw_off + sec_raw_sz:
        pe_data.append(0)

    # --- Write output ---
    with open(output_path, "wb") as f:
        f.write(pe_data)

    print(f"PE/COFF image written: {output_path}")
    print(f"  ImageBase:        {image_base:#018x}")
    print(f"  EntryPoint RVA:   {entry_rva:#x}")
    print(f"  SizeOfImage:      {size_of_image:#x}")
    print(f"  SizeOfHeaders:    {hdrs_aligned:#x}")
    print(f"  Section RVA:      {sec_rva:#x}")
    print(f"  Section raw size: {sec_raw_sz:#x}")
    print(f"  Kernel bin size:  {len(kernel_bin):#x}")

    return output_path


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} <patched_elf> [output_efi]")
        sys.exit(1)
    input_path = sys.argv[1]
    output_path = sys.argv[2] if len(sys.argv) > 2 else None
    make_efi(input_path, output_path)
