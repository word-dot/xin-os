
//! ELF loader module for loading user programs from the EXT4 filesystem.
//!
//! This module parses ELF64 binaries, maps their LOAD segments into user
//! address spaces, sets up the user stack, and returns the entry point
//! and heap base for the process.

extern crate alloc;

use alloc::vec::Vec;
use axmm::AddrSpace;
use axhal::paging::MappingFlags;
use axerrno::{AxError, AxResult};
use memory_addr::{VirtAddr, MemoryAddr, PAGE_SIZE_4K, align_up_4k};

// ============================================================
// ELF64 structures
// ============================================================

/// ELF magic number.
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

/// PT_LOAD program header type.
const PT_LOAD: u32 = 1;

/// 64-bit ELF header.
#[repr(C)]
#[derive(Debug, Clone)]
struct Elf64Ehdr {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

/// 64-bit Program header.
#[repr(C)]
#[derive(Debug, Clone)]
struct Elf64Phdr {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

/// PF_X (execute permission).
const PF_X: u32 = 1;
/// PF_W (write permission).
const PF_W: u32 = 2;
/// PF_R (read permission).
const PF_R: u32 = 4;

// ============================================================
// ELF parsing helpers
// ============================================================

/// Read a u16 from the ELF data at the given offset (little-endian).
fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

/// Read a u32 from the ELF data at the given offset (little-endian).
fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

/// Read a u64 from the ELF data at the given offset (little-endian).
fn read_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ])
}

/// Parse the ELF header from raw bytes.
fn parse_ehdr(data: &[u8]) -> AxResult<Elf64Ehdr> {
    if data.len() < 64 {
        return Err(AxError::InvalidInput);
    }
    if data[0..4] != ELF_MAGIC {
        return Err(AxError::InvalidInput);
    }
    // Check EI_CLASS == 2 (64-bit)
    if data[4] != 2 {
        return Err(AxError::InvalidInput);
    }
    // Check EI_DATA == 1 (little-endian)
    if data[5] != 1 {
        return Err(AxError::InvalidInput);
    }

    let mut e_ident = [0u8; 16];
    e_ident.copy_from_slice(&data[0..16]);

    Ok(Elf64Ehdr {
        e_ident,
        e_type: read_u16(data, 16),
        e_machine: read_u16(data, 18),
        e_version: read_u32(data, 20),
        e_entry: read_u64(data, 24),
        e_phoff: read_u64(data, 32),
        e_shoff: read_u64(data, 40),
        e_flags: read_u32(data, 48),
        e_ehsize: read_u16(data, 52),
        e_phentsize: read_u16(data, 54),
        e_phnum: read_u16(data, 56),
        e_shentsize: read_u16(data, 58),
        e_shnum: read_u16(data, 60),
        e_shstrndx: read_u16(data, 62),
    })
}

/// Parse a program header at the given offset.
fn parse_phdr(data: &[u8], offset: usize) -> AxResult<Elf64Phdr> {
    if offset + 56 > data.len() {
        return Err(AxError::InvalidInput);
    }
    Ok(Elf64Phdr {
        p_type: read_u32(data, offset),
        p_flags: read_u32(data, offset + 4),
        p_offset: read_u64(data, offset + 8),
        p_vaddr: read_u64(data, offset + 16),
        p_paddr: read_u64(data, offset + 24),
        p_filesz: read_u64(data, offset + 32),
        p_memsz: read_u64(data, offset + 40),
        p_align: read_u64(data, offset + 48),
    })
}

/// Convert ELF p_flags to MappingFlags.
fn elf_flags_to_mapping(pflags: u32) -> MappingFlags {
    let mut flags = MappingFlags::empty();
    if pflags & PF_R != 0 {
        flags |= MappingFlags::READ;
    }
    if pflags & PF_W != 0 {
        flags |= MappingFlags::WRITE;
    }
    if pflags & PF_X != 0 {
        flags |= MappingFlags::EXECUTE;
    }
    // If no flags set, default to READ
    if flags.is_empty() {
        flags |= MappingFlags::READ;
    }
    flags
}

// ============================================================
// Public API
// ============================================================

/// Load an ELF binary into the given address space.
///
/// This function:
/// 1. Parses the ELF header and program headers
/// 2. Maps all PT_LOAD segments into the address space
/// 3. Copies file data and zeroes BSS
/// 4. Maps the user stack
///
/// Returns `(entry_point, stack_top, heap_base)`.
///
/// # Arguments
/// * `elf_data` - Raw bytes of the ELF file
/// * `aspace` - The user address space to load into
/// * `stack_top` - The top of the user stack region
/// * `stack_size` - The size of the user stack
pub fn load_elf(
    elf_data: &[u8],
    aspace: &mut AddrSpace,
    stack_top: VirtAddr,
    stack_size: usize,
) -> AxResult<(VirtAddr, VirtAddr, VirtAddr)> {
    // 1. Parse ELF header
    let ehdr = parse_ehdr(elf_data)?;

    let entry_point = VirtAddr::from_usize(ehdr.e_entry as usize);

    // 2. Find the highest virtual address among all LOAD segments
    //    to determine where to place the heap.
    let mut highest_vaddr: usize = 0;

    // 3. Process each PT_LOAD segment
    for i in 0..ehdr.e_phnum as usize {
        let phdr_off = ehdr.e_phoff as usize + i * ehdr.e_phentsize as usize;
        let phdr = parse_phdr(elf_data, phdr_off)?;

        if phdr.p_type != PT_LOAD {
            continue;
        }

        let vaddr = VirtAddr::from_usize(phdr.p_vaddr as usize);
        let memsz = phdr.p_memsz as usize;
        let filesz = phdr.p_filesz as usize;

        if memsz == 0 {
            continue;
        }

        // [non-UTF-8 comment removed]
        highest_vaddr = highest_vaddr.max(vaddr.as_usize() + memsz);

        // Calculate aligned boundaries
        let vaddr_aligned = vaddr.align_down_4k();
        let vaddr_end = vaddr + memsz;
        let vaddr_end_aligned = vaddr_end.align_up_4k();
        let aligned_size = vaddr_end_aligned.as_usize() - vaddr_aligned.as_usize();

        axlog::warn!(
            "ELF Segment: vaddr={:#x}, memsz={}, filesz={}, aligned_size={:#x}, flags={:#x}",
            vaddr.as_usize(), memsz, filesz, aligned_size, phdr.p_flags
        );

        // Map the segment with alloc backend (allocates physical pages)
        let flags = elf_flags_to_mapping(phdr.p_flags);
        match aspace.map_alloc(vaddr_aligned, aligned_size, flags, true) {
            Ok(_) => axlog::warn!("Mapped segment successfully: {:#x}-{:#x}", vaddr_aligned.as_usize(), vaddr_end_aligned.as_usize()),
            Err(e) => {
                axlog::error!("Failed to map segment: {:#x}-{:#x}, error: {:?}", vaddr_aligned.as_usize(), vaddr_end_aligned.as_usize(), e);
                return Err(AxError::NoMemory);
            return Err(AxError::NoMemory);
            }
        }

        // Copy file data into the mapped region
        if filesz > 0 {
            let file_start = phdr.p_offset as usize;
            let file_end = file_start + filesz;
            if file_end > elf_data.len() {
                axlog::error!("Invalid file size: file_end={}, elf_data.len()={}", file_end, elf_data.len());
                return Err(AxError::InvalidInput);
            }
            match aspace.write(vaddr, &elf_data[file_start..file_end]) {
                Ok(_) => axlog::warn!("Wrote segment data successfully: {:#x}-{:#x}", vaddr.as_usize(), (vaddr + filesz).as_usize()),
                Err(e) => {
                    axlog::error!("Failed to write segment data: {:#x}-{:#x}, error: {:?}", vaddr.as_usize(), (vaddr + filesz).as_usize(), e);
                    return Err(AxError::InvalidInput);
                }
            }
        }

        // Zero the BSS portion (memsz > filesz)
        if memsz > filesz {
            let bss_start = vaddr + filesz;
            let bss_size = memsz - filesz;
            // Write zeroes in page-sized chunks
            let zero_page = alloc::vec![0u8; PAGE_SIZE_4K];
            let mut written = 0;
            while written < bss_size {
                let chunk_size = bss_size - written;
                let chunk = if chunk_size >= PAGE_SIZE_4K {
                    &zero_page[..PAGE_SIZE_4K]
                } else {
                    &zero_page[..chunk_size]
                };
                aspace.write(bss_start + written, chunk)?;
                written += chunk.len();
            }
        }

        // Track highest address for heap placement
        let seg_end = vaddr.as_usize() + memsz;
        if seg_end > highest_vaddr {
            highest_vaddr = seg_end;
        }
    }

    // 4. Map the user stack
    let stack_base = VirtAddr::from_usize(stack_top.as_usize() - stack_size);
    match aspace.map_alloc(
        stack_base,
        stack_size,
        MappingFlags::READ | MappingFlags::WRITE,
        true,
    ) {
        Ok(_) => axlog::warn!("Mapped user stack successfully: {:#x}-{:#x}", 
                               stack_base.as_usize(), 
                               (stack_base.as_usize() + stack_size)),
        Err(e) => {
            axlog::error!("Failed to map user stack: {:#x}-{:#x}, error: {:?}", 
                          stack_base.as_usize(), 
                          (stack_base.as_usize() + stack_size), 
                          e);
            return Err(AxError::NoMemory);
        }
    }

    // 5. Determine heap base (page-aligned, after all ELF segments)
    let heap_base = VirtAddr::from_usize(align_up_4k(highest_vaddr));

    // Heap base should be after all segments and before user stack
    let adjusted_heap_base = heap_base;

    axlog::warn!("ELF Load Summary: entry_point={:#x}, stack_top={:#x}, heap_base={:#x}, adjusted_heap_base={:#x}", 
                 entry_point.as_usize(), 
                 stack_top.as_usize(), 
                 heap_base.as_usize(),
                 adjusted_heap_base.as_usize());

    Ok((entry_point, stack_top, adjusted_heap_base))
}

/// Read an ELF file from the VFS (ext4 mounted as root) into a Vec<u8>.
///
/// This is a convenience function that uses axstd::fs::read
/// to read the entire file contents via the VFS layer.
pub fn read_elf_from_vfs(path: &str) -> AxResult<Vec<u8>> {
    axstd::fs::read(path).map_err(|e| {
        axlog::warn!("Failed to read ELF file {} from VFS: {:?}", path, e);
        AxError::Io
    })
}
