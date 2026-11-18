
#![no_std]

extern crate alloc;
use alloc::string::ToString;
use alloc::string::String;
use alloc::format;

/// Run a single ELF test binary and return its exit code.
///
/// This function:
/// 1. Creates a new user address space
/// 2. Loads the ELF binary into it
/// 3. Sets up trap context, stack, and argv
/// 4. Spawns a task to execute the binary
/// 5. Waits for completion and returns the exit code
///
/// Only available on RISC-V (requires trap module).
#[cfg(target_arch = "riscv64")]
pub fn run_elf_test(elf_data: &[u8], test_name: &str, extra_args: &[&str]) -> i32 {
    use crate::process::{self, ProcessState, layout};
    use crate::trap::TrapContext;
    use alloc::sync::Arc;
    use axstd::sync::Mutex;
    use memory_addr::VirtAddr;

    // Allocate a new PID
    let pid = process::process_manager().lock().alloc_pid();
    axlog::info!("[run_elf_test] pid={}, test={}", pid, test_name);

    // Create a new user address space
    let mut aspace = match axmm::new_user_aspace(
        layout::user_space_base(),
        layout::user_space_size(),
    ) {
        Ok(a) => a,
        Err(_) => return 1,
    };

    // Map trampoline code into user address space
    crate::trap::map_trampoline_to_user(&mut aspace);

    // Load the ELF
    let (entry_point, _stack_top, heap_base) = match crate::elf_loader::load_elf(
        elf_data,
        &mut aspace,
        layout::user_stack_top(),
        layout::USER_STACK_SIZE,
    ) {
        Ok(r) => r,
        Err(e) => {
            axlog::warn!("Failed to load ELF for {}: {:?}", test_name, e);
            return 1;
        }
    };

    let page_table_root = aspace.page_table_root().as_usize() as u64;

    use axhal::paging::MappingFlags;

    // Unmap any existing mappings at trap_context_base and sigtrampoline
    // that may have been inherited from kernel via copy_mappings_from.
    // These need to be replaced with fresh alloc-backed mappings.
    let _ = aspace.unmap(layout::trap_context_base(), 4096);
    let _ = aspace.unmap(layout::sigtrampoline(), 4096);

    // Map trap context page
    if aspace.map_alloc(layout::trap_context_base(), 4096, MappingFlags::READ | MappingFlags::WRITE, true).is_err() {
        return 1;
    }

    // Map sigreturn trampoline page
    if aspace.map_alloc(layout::sigtrampoline(), 4096, MappingFlags::READ | MappingFlags::EXECUTE, true).is_err() {
        return 1;
    }
    let sigreturn_code: [u8; 8] = [
        0x93, 0x08, 0xb0, 0x08, // li a7, 139
        0x73, 0x00, 0x00, 0x00, // ecall
    ];
    if aspace.write(layout::sigtrampoline(), &sigreturn_code).is_err() {
        return 1;
    }

    // Set up the trap context
    let trap_ctx = TrapContext::app_init_context(
        entry_point.as_usize(),
        layout::user_stack_top().as_usize(),
        axmm::kernel_page_table_root().as_usize(),
        0,
        crate::trap::trap_handler as usize,
    );

    // Write test name and argv strings onto the user stack
    let stack_top = layout::user_stack_top().as_usize();

    // argv[0] = test name
    let argv0_bytes = alloc::format!("{}\0", test_name);
    let argv0_addr = stack_top - 256;
    if aspace.write(VirtAddr::from_usize(argv0_addr), argv0_bytes.as_bytes()).is_err() {
        return 1;
    }

    // Write extra args
    let mut arg_addrs: alloc::vec::Vec<u64> = alloc::vec![argv0_addr as u64];
    let mut next_addr = stack_top - 512;
    for arg in extra_args {
        let arg_bytes = alloc::format!("{}\0", arg);
        if aspace.write(VirtAddr::from_usize(next_addr), arg_bytes.as_bytes()).is_err() {
            return 1;
        }
        arg_addrs.push(next_addr as u64);
        next_addr -= 256;
    }
    arg_addrs.push(0); // NULL terminator

    // Write argv array
    let argv_base = next_addr - 256;
    let argv_bytes = unsafe {
        core::slice::from_raw_parts(
            arg_addrs.as_ptr() as *const u8,
            arg_addrs.len() * 8,
        )
    };
    if aspace.write(VirtAddr::from_usize(argv_base), argv_bytes).is_err() {
        return 1;
    }

    // Set up argc and argv in trap context
    let mut trap_ctx_mod = trap_ctx;
    trap_ctx_mod.x[10] = arg_addrs.len() as usize - 1; // argc (excluding NULL)
    trap_ctx_mod.x[11] = argv_base; // argv pointer
    if aspace.write(layout::trap_context_base(), unsafe {
        core::slice::from_raw_parts(
            &trap_ctx_mod as *const _ as *const u8,
            core::mem::size_of::<TrapContext>(),
        )
    }).is_err() {
        return 1;
    }

    // Create the PCB
    let pcb = Arc::new(Mutex::new(process::ProcessControlBlock {
        pid,
        ppid: 1, // init process
        state: ProcessState::Ready,
        exit_code: 0,
        aspace: Arc::new(Mutex::new(aspace)),
        user_stack_base: VirtAddr::from_usize(
            layout::user_stack_top().as_usize() - layout::USER_STACK_SIZE
        ),
        user_stack_size: layout::USER_STACK_SIZE,
        heap_base,
        brk: heap_base,
        fd_table: Arc::new(Mutex::new(process::FdTable::new())),
        cwd: Arc::new(Mutex::new(alloc::string::String::from("/"))),
        children: Arc::new(Mutex::new(alloc::vec::Vec::new())),
        trap_ctx_vaddr: layout::trap_context_base(),
        page_table_root: Arc::new(Mutex::new(page_table_root)),
        signal_actions: Arc::new(Mutex::new([process::SigAction::default(); process::MAX_SIGNUM])),
        signal_mask: Arc::new(Mutex::new(axtask::SigSet::new())),
        pending_signals: Arc::new(Mutex::new(axtask::SigSet::new())),
        sig_stack: Arc::new(Mutex::new(process::SigStackInfo::default())),
        capture_output: true,
        output_buf: Arc::new(Mutex::new(alloc::vec::Vec::new())),
    }));

    process::process_manager().lock().add(pcb.clone());

    // Create and spawn the kernel task
    let task_name = alloc::format!("test-{}", test_name);
    let mut task = axtask::TaskInner::new(
        move || {
            crate::trap::trap_return();
        },
        task_name,
        axconfig::TASK_STACK_SIZE,
    );
    task.init_task_ext(crate::TaskExtImpl { pid, is_user: true });
    axtask::spawn_task(task);

    axlog::info!("[run_elf_test] task spawned for pid={}, waiting for completion...", pid);

    // Wait for the test to complete (with timeout)
    let mut waited = 0;
    let exit_code = loop {
        let pcb_guard = pcb.lock();
        if pcb_guard.state == ProcessState::Exited {
            break pcb_guard.exit_code;
        }
        drop(pcb_guard);
        axtask::yield_now();
        waited += 1;
        if waited % 10000 == 0 {
            axlog::warn!("[run_elf_test] still waiting for pid={}, waited={}, test={}", pid, waited, test_name);
        }
        if waited > 100000 {
            axlog::error!("Test {} timed out after {} iterations", test_name, waited);
            
            // [non-UTF-8 comment removed]
            let pcb_guard = pcb.lock();
            axlog::error!("Process state: {:?}", pcb_guard.state);
            axlog::error!("Process exit code: {}", pcb_guard.exit_code);
            
            break -1; // timeout
        }
    };

    // Output captured stdout/stderr
    let output = {
        let pcb_guard = pcb.lock();
        core::mem::take(&mut *pcb_guard.output_buf.lock())
    };
    if !output.is_empty() {
        // Write captured output to UART
        for &byte in &output {
            axhal::console::write_bytes(&[byte]);
        }
    }

    exit_code
}

/// Fallback for non-RISC-V: run_elf_test returns -1 (not supported)
#[cfg(not(target_arch = "riscv64"))]
pub fn run_elf_test(_elf_data: &[u8], _test_name: &str, _extra_args: &[&str]) -> i32 {
    -1
}

/// Run an ELF binary from a file path on the filesystem
pub fn run_elf_from_path(path: &str, args: &[&str]) -> i32 {
    axlog::info!("[run_elf_from_path] ========================================");
    axlog::info!("[run_elf_from_path] Loading ELF from: {}", path);
    println!("[run_elf_from_path] Loading ELF from: {}", path);
    
    // QUICK FIX: Use chunked reading to avoid ext4_view's slow read_file_all()
    // This reads the file in smaller chunks which is much faster
    axlog::info!("[run_elf_from_path] Using chunked reading for better performance");
    println!("[run_elf_from_path] Using chunked reading...");
    
    let elf_data = match read_file_chunked(path) {
        Ok(data) => {
            axlog::info!("[run_elf_from_path] Successfully read {} bytes from {}", data.len(), path);
            println!("[run_elf_from_path] Successfully read {} bytes", data.len());
            data
        },
        Err(e) => {
            axlog::error!("[run_elf_from_path] Failed to read {}: {}", path, e);
            println!("Error: Cannot read file {}: {}", path, e);
            return -1;
        }
    };

    if elf_data.is_empty() {
        axlog::error!("[run_elf_from_path] File {} is empty", path);
        println!("Error: File {} is empty", path);
        return -1;
    }

    // Extract the test name from the path
    let test_name = path.rsplit('/').next().unwrap_or(path);
    
    axlog::info!("[run_elf_from_path] Running {} with {} args", test_name, args.len());
    println!("[run_elf_from_path] Running test {}", test_name);
    run_elf_test(&elf_data, test_name, args)
}

/// Read a file using axfs-ng for fast access, with fallback to ext4_view
fn read_file_chunked(path: &str) -> Result<alloc::vec::Vec<u8>, String> {
    // Try axfs-ng first for fast file access
    if let Some(ctx) = crate::myfs::create_axfs_context() {
        println!("[read_file_chunked] Using axfs-ng for fast file access: {}", path);
        
        // Resolve the file path
        let location = match ctx.resolve(path) {
            Ok(loc) => loc,
            Err(e) => {
                println!("[read_file_chunked] axfs-ng resolve failed: {:?}, falling back", e);
                return read_file_fallback(path);
            }
        };
        
        // Check if it's a file
        if !location.is_file() {
            return Err(format!("{} is not a file", path));
        }
        
        // Get file size
        let size = match location.len() {
            Ok(s) => s as usize,
            Err(e) => {
                println!("[read_file_chunked] Failed to get file size: {:?}", e);
                return Err(format!("Failed to get file size: {:?}", e));
            }
        };
        
        println!("[read_file_chunked] File size: {} bytes (using axfs-ng)", size);
        
        // Get file node and read data
        let file_node = location.get_file_node();
        let mut buffer = alloc::vec![0u8; size];
        
        match file_node.read_at(&mut buffer, 0) {
            Ok(n) if n == size => {
                println!("[read_file_chunked] Read complete using axfs-ng: {} bytes!", n);
                return Ok(buffer);
            }
            Ok(n) => {
                println!("[read_file_chunked] Partial read: {} of {} bytes", n, size);
                buffer.truncate(n);
                return Ok(buffer);
            }
            Err(e) => {
                println!("[read_file_chunked] axfs-ng read failed: {:?}, falling back", e);
                return read_file_fallback(path);
            }
        }
    }
    
    // Fallback to ext4_view if axfs-ng not available
    println!("[read_file_chunked] axfs-ng not available, using fallback");
    read_file_fallback(path)
}

/// Fallback file reading using ext4_view (slow)
fn read_file_fallback(path: &str) -> Result<alloc::vec::Vec<u8>, String> {
    
    use axstd::fs::File;
    use axstd::io::Read;
    
    // Open the file
    let mut file = File::open(path)
        .map_err(|e| format!("Failed to open file: {:?}", e))?;
    
    // Get file size
    let size = file.metadata()
        .map_err(|e| format!("Failed to get metadata: {:?}", e))?
        .len() as usize;
    
    println!("[read_file_chunked] File size: {} bytes", size);
    
    // Allocate buffer
    let mut buffer = alloc::vec![0u8; size];
    
    // Read in chunks of 64KB to avoid blocking
    const CHUNK_SIZE: usize = 65536; // 64KB chunks
    let mut total_read = 0;
    
    while total_read < size {
        let chunk_size = core::cmp::min(CHUNK_SIZE, size - total_read);
        let chunk = &mut buffer[total_read..total_read + chunk_size];
        
        match file.read(chunk) {
            Ok(0) => break, // EOF
            Ok(n) => {
                total_read += n;
                if total_read % (CHUNK_SIZE * 4) == 0 {
                    println!("[read_file_chunked] Progress: {} / {} bytes", total_read, size);
                }
            }
            Err(e) => {
                return Err(format!("Read error at offset {}: {:?}", total_read, e));
            }
        }
    }
    
    if total_read != size {
        buffer.truncate(total_read);
    }
    
    println!("[read_file_chunked] Read complete: {} bytes", total_read);
    Ok(buffer)
}
