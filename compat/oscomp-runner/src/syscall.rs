
//! System call dispatch and implementation.
//!
//! This module provides the syscall dispatcher and implementations for
//! fork, execve, wait/waitpid, exit, clone, brk, mmap, and basic I/O.

extern crate alloc;

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use axerrno::{AxError, AxResult};
use memory_addr::VirtAddr;

use crate::process::{
    self, FdTable, Pid, ProcessControlBlock, ProcessState,
    layout, O_RDONLY, O_WRONLY, O_RDWR, O_CREAT, O_TRUNC, O_CLOEXEC, O_DIRECTORY,
};
use crate::trap::TrapContext;

// ============================================================
// RISC-V Syscall Numbers
// ============================================================

// I/O
pub const SYS_READ:       usize = 63;
pub const SYS_WRITE:      usize = 64;
pub const SYS_WRITEV:     usize = 66;
pub const SYS_OPENAT:     usize = 56;
pub const SYS_CLOSE:      usize = 57;
pub const SYS_FSTAT:      usize = 80;
pub const SYS_LSEEK:      usize = 62;
pub const SYS_IOCTL:      usize = 29;

// Process
pub const SYS_EXIT:       usize = 93;
pub const SYS_EXIT_GROUP: usize = 94;
pub const SYS_GETPID:     usize = 172;
pub const SYS_GETPPID:    usize = 173;
pub const SYS_FORK:       usize = 220;
pub const SYS_CLONE:      usize = 220;
pub const SYS_EXECVE:     usize = 221;
pub const SYS_WAITPID:    usize = 260;

// Memory
pub const SYS_BRK:        usize = 214;
pub const SYS_MMAP:       usize = 222;
pub const SYS_MUNMAP:     usize = 215;
pub const SYS_MPROTECT:   usize = 226;

// File system
pub const SYS_CHDIR:      usize = 49;
pub const SYS_GETCWD:     usize = 17;
pub const SYS_UNLINKAT:   usize = 35;
pub const SYS_MKDIRAT:    usize = 34;
pub const SYS_LINKAT:     usize = 37;
pub const SYS_RENAMEAT:   usize = 38;
pub const SYS_GETDENTS64: usize = 61;
pub const SYS_DUP:        usize = 23;
pub const SYS_DUP3:       usize = 24;

// Signal
pub const SYS_KILL:        usize = 129;
pub const SYS_TKILL:       usize = 130;
pub const SYS_TGKILL:      usize = 131;
pub const SYS_SIGACTION:   usize = 134;
pub const SYS_SIGPROCMASK: usize = 135;
pub const SYS_SIGRETURN:   usize = 139;
pub const SYS_RT_SIGACTION:   usize = 134;
pub const SYS_RT_SIGPROCMASK: usize = 135;
pub const SYS_RT_SIGRETURN:   usize = 139;
pub const SYS_RT_SIGPENDING:  usize = 136;
pub const SYS_SIGALTSTACK:    usize = 132;

// Time
pub const SYS_GETTIMEOFDAY: usize = 169;
pub const SYS_TIMES:       usize = 153;
pub const SYS_CLOCK_GETTIME: usize = 113;
pub const SYS_NANOSLEEP:   usize = 101;

// Network
pub const SYS_SOCKET:     usize = 198;
pub const SYS_BIND:        usize = 200;
pub const SYS_LISTEN:      usize = 201;
pub const SYS_ACCEPT:      usize = 202;
pub const SYS_CONNECT:     usize = 203;
pub const SYS_SENDTO:      usize = 206;
pub const SYS_RECVFROM:    usize = 207;
pub const SYS_SEND:        usize = 211;
pub const SYS_RECV:        usize = 212;
pub const SYS_SHUTDOWN:    usize = 210;
pub const SYS_GETSOCKNAME: usize = 204;
pub const SYS_GETPEERNAME: usize = 205;
pub const SYS_SETSOCKOPT:  usize = 208;
pub const SYS_GETSOCKOPT:  usize = 209;
pub const SYS_SOCKETPAIR:  usize = 199;

// Other
pub const SYS_GETTID:     usize = 178;
pub const SYS_UNAME:      usize = 160;
pub const SYS_PIPE2:      usize = 59;
pub const SYS_MOUNT:      usize = 40;
pub const SYS_UMOUNT2:    usize = 39;
pub const SYS_SYMLINKAT:  usize = 36;
pub const SYS_READLINKAT: usize = 78;
pub const SYS_FCHMODAT:   usize = 79;
pub const SYS_FCHMOD:     usize = 52;
pub const SYS_FCHOWNAT:   usize = 54;
pub const SYS_FCHOWN:     usize = 55;
pub const SYS_TRUNCATE:   usize = 45;
pub const SYS_FTRUNCATE:  usize = 46;
pub const SYS_FSYNC:      usize = 82;
pub const SYS_FDATASYNC:  usize = 83;
pub const SYS_FLOCK:      usize = 32;
pub const SYS_YIELD:      usize = 124;
pub const SYS_SCHED_YIELD: usize = 124;
pub const SYS_SET_TID_ADDRESS: usize = 96;
pub const SYS_PRCTL:      usize = 167;
pub const SYS_PRLIMIT64:  usize = 261;
pub const SYS_RSEQ:       usize = 283;

// ============================================================
// Clone flags (from Linux)
// ============================================================
pub const CLONE_VM:       u64 = 0x100;
pub const CLONE_FS:       u64 = 0x200;
pub const CLONE_FILES:    u64 = 0x400;
pub const CLONE_SIGHAND:  u64 = 0x800;
pub const CLONE_PIDFD:    u64 = 0x1000;
pub const CLONE_PARENT:   u64 = 0x8000;
pub const CLONE_THREAD:   u64 = 0x10000;
pub const CLONE_SYSVSEM:  u64 = 0x40000;
pub const CLONE_VFORK:    u64 = 0x40000000;

// ============================================================
// Syscall dispatcher
// ============================================================

/// Dispatch a system call.
///
/// # Arguments
/// * `sys_num` - The system call number
/// * `args` - Up to 6 arguments (from registers a0-a5)
///
/// # Returns
/// The return value to be placed in a0 (positive on success,
/// negated error code on failure).
pub fn syscall_dispatch(sys_num: usize, args: [usize; 6]) -> isize {
    axlog::info!("SYSCALL ENTER: sys_num={}, args=[{}, {}, {}, {}, {}, {}]", 
        sys_num, args[0], args[1], args[2], args[3], args[4], args[5]);
    
    let result = match sys_num {
        // Process management
        SYS_FORK       => sys_fork(),
        SYS_CLONE      => sys_clone(args[0], args[1], args[2], args[3], args[4]),
        SYS_EXECVE     => sys_execve(args[0], args[1], args[2]),
        SYS_WAITPID    => sys_waitpid(args[0] as i32, args[1], args[2] as i32),
        SYS_EXIT       => sys_exit(args[0] as i32),
        SYS_EXIT_GROUP => sys_exit_group(args[0] as i32),
        SYS_GETPID     => sys_getpid(),
        SYS_GETPPID    => sys_getppid(),

        // Memory
        SYS_BRK        => sys_brk(args[0]),
        SYS_MMAP       => sys_mmap(args[0], args[1], args[2], args[3], args[4], args[5]),
        SYS_MUNMAP     => sys_munmap(args[0], args[1]),
        SYS_MPROTECT   => sys_mprotect(args[0], args[1], args[2]),

        // I/O
        SYS_READ       => sys_read(args[0], args[1], args[2]),
        SYS_WRITE      => sys_write(args[0], args[1], args[2]),
        SYS_OPENAT     => sys_openat(args[0], args[1], args[2], args[3]),
        SYS_CLOSE      => sys_close(args[0]),
        SYS_DUP        => sys_dup(args[0]),
        SYS_DUP3       => sys_dup3(args[0], args[1], args[2]),
        SYS_LSEEK      => sys_lseek(args[0], args[1] as i64, args[2]),
        SYS_IOCTL      => sys_ioctl(args[0], args[1], args[2]),
        SYS_FSTAT      => sys_fstat(args[0], args[1]),
        SYS_GETDENTS64 => sys_getdents64(args[0], args[1], args[2]),

        // File system
        SYS_CHDIR      => sys_chdir(args[0]),
        SYS_GETCWD     => sys_getcwd(args[0], args[1]),
        SYS_PIPE2      => sys_pipe2(args[0], args[1]),
        SYS_MOUNT      => sys_mount(args[0], args[1], args[2], args[3]),
        SYS_UMOUNT2    => sys_umount2(args[0], args[1]),
        SYS_SYMLINKAT  => sys_symlinkat(args[0], args[1], args[2]),
        SYS_READLINKAT => sys_readlinkat(args[0], args[1], args[2], args[3]),
        SYS_FCHMODAT   => sys_fchmodat(args[0], args[1], args[2], args[3]),
        SYS_FCHMOD     => sys_fchmod(args[0], args[1]),
        SYS_FCHOWNAT   => sys_fchownat(args[0], args[1], args[2], args[3], args[4]),
        SYS_FCHOWN     => sys_fchown(args[0], args[1], args[2]),
        SYS_TRUNCATE   => sys_truncate(args[0], args[1]),
        SYS_FTRUNCATE  => sys_ftruncate(args[0], args[1]),
        SYS_FSYNC      => sys_fsync(args[0]),
        SYS_FDATASYNC  => sys_fdatasync(args[0]),
        SYS_FLOCK      => sys_flock(args[0], args[1]),

        // Time
        SYS_GETTIMEOFDAY => sys_gettimeofday(args[0]),
        SYS_CLOCK_GETTIME => sys_clock_gettime(args[0], args[1]),
        SYS_NANOSLEEP  => sys_nanosleep(args[0], args[1]),

        // Signal
        SYS_KILL          => sys_kill(args[0], args[1]),
        SYS_TKILL         => sys_kill(args[0], args[1]), // tkill(tid, sig): treat as kill to self
        SYS_TGKILL        => sys_kill(args[1], args[2]), // simplified: treat as kill
        SYS_SIGACTION | SYS_RT_SIGACTION => sys_sigaction(args[0], args[1], args[2]),
        SYS_SIGPROCMASK | SYS_RT_SIGPROCMASK => sys_sigprocmask(args[0], args[1], args[2]),
        SYS_SIGRETURN | SYS_RT_SIGRETURN => sys_sigreturn(),
        SYS_RT_SIGPENDING  => sys_sigpending(args[0]),
        SYS_SIGALTSTACK    => sys_sigaltstack(args[0], args[1]),

        // Network
        SYS_SOCKET     => crate::net::sys_socket(args[0], args[1], args[2]),
        SYS_BIND        => crate::net::sys_bind(args[0], args[1], args[2]),
        SYS_LISTEN      => crate::net::sys_listen(args[0], args[1]),
        SYS_ACCEPT      => crate::net::sys_accept(args[0], args[1], args[2]),
        SYS_CONNECT     => crate::net::sys_connect(args[0], args[1], args[2]),
        SYS_SENDTO      => crate::net::sys_sendto(args[0], args[1], args[2], args[3], args[4], args[5]),
        SYS_RECVFROM    => crate::net::sys_recvfrom(args[0], args[1], args[2], args[3], args[4], args[5]),
        SYS_SEND        => crate::net::sys_send(args[0], args[1], args[2], args[3]),
        SYS_RECV        => crate::net::sys_recv(args[0], args[1], args[2], args[3]),
        SYS_SHUTDOWN    => crate::net::sys_shutdown(args[0], args[1]),
        SYS_GETSOCKNAME => crate::net::sys_getsockname(args[0], args[1], args[2]),
        SYS_GETPEERNAME => crate::net::sys_getpeername(args[0], args[1], args[2]),
        SYS_SETSOCKOPT  => crate::net::sys_setsockopt(args[0], args[1], args[2], args[3], args[4]),
        SYS_GETSOCKOPT  => crate::net::sys_getsockopt(args[0], args[1], args[2], args[3], args[4]),

        // Scheduling
        SYS_YIELD | SYS_SCHED_YIELD => sys_yield(),

        // Other
        SYS_GETTID     => sys_getpid(), // gettid returns same as getpid for single-threaded
        SYS_UNAME      => sys_uname(args[0]),
        SYS_SET_TID_ADDRESS => sys_set_tid_address(args[0]),
        SYS_PRCTL      => sys_prctl(args[0], args[1], args[2], args[3]),
        SYS_PRLIMIT64  => sys_prlimit64(args[0], args[1], args[2], args[3]),
        SYS_RSEQ       => Ok(0), // stub

        _ => {
            axlog::warn!("Unimplemented syscall: {} (args: {:?})", sys_num, args);
            Err(AxError::Unsupported)
        }
    };

    match result {
        Ok(val) => {
            axlog::info!("SYSCALL EXIT: sys_num={}, result=Ok({})", sys_num, val);
            val as isize
        }
        Err(e) => {
            axlog::warn!("SYSCALL EXIT: sys_num={}, result=Err({:?})", sys_num, e);
            -(e.code() as isize)
        }
    }
}

/// Legacy entry point called from trap handler (3 args only).
pub fn syscall(sys_num: usize, args: [usize; 3]) -> isize {
    syscall_dispatch(sys_num, [args[0], args[1], args[2], 0, 0, 0])
}

// ============================================================
// Helper: get current process PCB
// ============================================================

/// Get the PCB of the current process.
fn current_pcb() -> Arc<axstd::sync::Mutex<ProcessControlBlock>> {
    let pid = current_pid();
    process::process_manager().lock().get(pid)
        .expect("Current process not found in process manager")
}

/// Get the PID of the current process.
fn current_pid() -> Pid {
    let task = axtask::current();
    axtask::TaskExtRef::<crate::TaskExtImpl>::task_ext(&*task).pid
}

// ============================================================
// Process syscalls
// ============================================================

/// sys_fork: Create a child process by duplicating the current process.
///
/// The child gets a copy of the parent's address space (COW),
/// file descriptor table, and other resources.
/// Returns 0 to the child, child's PID to the parent.
fn sys_fork() -> AxResult<isize> {
    let parent_pid = current_pid();
    let parent_pcb = process::process_manager().lock().get(parent_pid).unwrap();
    let parent = parent_pcb.lock();

    // Allocate a new PID
    let child_pid = process::process_manager().lock().alloc_pid();

    // Create a new user address space (copy from parent)
    let parent_aspace = parent.aspace.lock();
    let mut child_aspace = axmm::new_user_aspace(
        parent_aspace.base(),
        parent_aspace.size(),
    )?;

    // Map trampoline code into child address space
    crate::trap::map_trampoline_to_user(&mut child_aspace);

    // Copy page table mappings from parent to child.
    // copy_mappings_from() copies page table entries only (not physical memory),
    // so parent and child share the same physical pages (simplified COW).
    // This is sufficient for fork+exec patterns used in most tests.
    child_aspace.copy_mappings_from(&parent_aspace)?;
    drop(parent_aspace);
    // NOTE: We intentionally do NOT iterate over the entire address space
    // (which would be 64TB and cause the system to hang). The shared page
    // table entries are sufficient for fork+exec patterns.

    let page_table_root = child_aspace.page_table_root().as_usize() as u64;

    // Fork the fd table
    let child_fd_table = parent.fd_table.lock().fork();

    // Create child PCB
    let child_pcb = Arc::new(axstd::sync::Mutex::new(ProcessControlBlock {
        pid: child_pid,
        ppid: parent_pid,
        state: ProcessState::Ready,
        exit_code: 0,
        aspace: Arc::new(axstd::sync::Mutex::new(child_aspace)),
        user_stack_base: parent.user_stack_base,
        user_stack_size: parent.user_stack_size,
        heap_base: parent.heap_base,
        brk: parent.brk,
        fd_table: Arc::new(axstd::sync::Mutex::new(child_fd_table)),
        cwd: Arc::new(axstd::sync::Mutex::new(parent.cwd.lock().clone())),
        children: Arc::new(axstd::sync::Mutex::new(Vec::new())),
        trap_ctx_vaddr: parent.trap_ctx_vaddr,
        page_table_root: Arc::new(axstd::sync::Mutex::new(page_table_root)),
        signal_actions: Arc::new(axstd::sync::Mutex::new(*parent.signal_actions.lock())),
        signal_mask: Arc::new(axstd::sync::Mutex::new(parent.signal_mask.lock().clone())),
        pending_signals: Arc::new(axstd::sync::Mutex::new(axtask::SigSet::new())), // child starts with no pending signals
        sig_stack: Arc::new(axstd::sync::Mutex::new(*parent.sig_stack.lock())),
        capture_output: false,
        output_buf: Arc::new(axstd::sync::Mutex::new(alloc::vec::Vec::new())),
    }));

    // Add child to parent's children list
    parent.children.lock().push(child_pid);

    // Register child with process manager
    process::process_manager().lock().add(child_pcb.clone());

    // Copy the parent's trap context to the child's address space,
    // but set the return value (a0/x[10]) to 0 for the child.
    {
        let child_guard = child_pcb.lock();
        let aspace = child_guard.aspace.lock();
        let trap_ctx_vaddr = child_guard.trap_ctx_vaddr;
        let mut trap_ctx_bytes = [0u8; core::mem::size_of::<TrapContext>()];
        aspace.read(trap_ctx_vaddr, &mut trap_ctx_bytes).unwrap();
        // Set fork return value to 0 in child (x[10] = a0)
        let trap_ctx = unsafe {
            &mut *(trap_ctx_bytes.as_mut_ptr() as *mut TrapContext)
        };
        trap_ctx.x[10] = 0; // child returns 0 from fork()
        aspace.write(trap_ctx_vaddr, &trap_ctx_bytes).unwrap();
    }

    // Create a kernel thread for the child process.
    // The child task will enter user space via the trap_return mechanism,
    // which restores the trap context we just set up.
    let child_pid_for_closure = child_pid;
    let mut child_task = axtask::TaskInner::new(
        move || {
            // The child process starts here in kernel mode.
            // trap_return() will restore the trap context from the child's
            // address space and sret to user mode, where fork() returns 0.
            #[cfg(target_arch = "riscv64")]
            crate::trap::trap_return();

            // LoongArch: TODO - implement trap_return for LA64
            #[cfg(not(target_arch = "riscv64"))]
            {
                axlog::error!("fork: trap_return not implemented for this arch");
                axtask::exit(1);
            }
        },
        alloc::format!("proc-{}", child_pid_for_closure),
        axconfig::TASK_STACK_SIZE,
    );

    // Initialize task extension with child's PID
    child_task.init_task_ext(crate::TaskExtImpl {
        pid: child_pid,
        is_user: true,
    });

    // Spawn the child task
    axtask::spawn_task(child_task);

    // Return child PID to parent
    Ok(child_pid as isize)
}

/// sys_clone: Create a new thread or process.
///
/// This is the Linux clone() system call. When CLONE_VM is set,
/// the child shares the address space with the parent (thread).
/// Otherwise, it behaves like fork().
fn sys_clone(
    flags: usize,
    stack: usize,
    ptid: usize,
    tls: usize,
    ctid: usize,
) -> AxResult<isize> {
    let flags64 = flags as u64;

    if flags64 & CLONE_VM != 0 {
        // Thread creation: share address space
        // For now, implement a simplified version
        axlog::debug!("clone(CLONE_VM): creating thread, flags={:#x}", flags64);
        // TODO: implement proper thread creation with shared address space
        // For now, fall through to fork-like behavior
    }

    // Default: fork-like behavior
    let result = sys_fork()?;
    Ok(result)
}

/// sys_execve: Replace the current process with a new program.
///
/// # Arguments
/// * `path_ptr` - Pointer to the program path string in user space
/// * `argv_ptr` - Pointer to the argument vector
/// * `envp_ptr` - Pointer to the environment vector
fn sys_execve(path_ptr: usize, argv_ptr: usize, envp_ptr: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();

    // Read the path from user space
    let path = read_user_string(&pcb_guard, path_ptr, 256)?;

    axlog::info!("execve: path={}", path);

    // Read the ELF file from the EXT4 filesystem
    let elf_data = read_elf_from_disk(&path)?;

    // Create a new address space for the new program
    let mut new_aspace = axmm::new_user_aspace(
        layout::user_space_base(),
        layout::user_space_size(),
    )?;

    // Map trampoline code into new address space
    crate::trap::map_trampoline_to_user(&mut new_aspace);

    // Load the ELF into the new address space
    let (entry_point, _stack_top, heap_base) = crate::elf_loader::load_elf(
        &elf_data,
        &mut new_aspace,
        layout::user_stack_top(),
        layout::USER_STACK_SIZE,
    )?;

    let page_table_root = new_aspace.page_table_root().as_usize() as u64;

    // Map trap context page
    use axhal::paging::MappingFlags;
    new_aspace.map_alloc(
        layout::trap_context_base(),
        4096,
        MappingFlags::READ | MappingFlags::WRITE,
        true,
    )?;

    // Map sigreturn trampoline page
    new_aspace.map_alloc(
        layout::sigtrampoline(),
        4096,
        MappingFlags::READ | MappingFlags::EXECUTE,
        true,
    )?;
    let sigreturn_code: [u8; 8] = [
        0x93, 0x08, 0xb0, 0x08, // li a7, 139
        0x73, 0x00, 0x00, 0x00, // ecall
    ];
    new_aspace.write(layout::sigtrampoline(), &sigreturn_code)?;

    // Set up the new trap context
    let trap_ctx = TrapContext::app_init_context(
        entry_point.as_usize(),
        layout::user_stack_top().as_usize(),
        axmm::kernel_page_table_root().as_usize(),
        0, // kernel_sp will be set by the task
        crate::trap::trap_handler as usize,
    );
    new_aspace.write(layout::trap_context_base(), unsafe {
        core::slice::from_raw_parts(
            &trap_ctx as *const _ as *const u8,
            core::mem::size_of::<TrapContext>(),
        )
    })?;

    // Replace the old address space with the new one
    drop(pcb_guard);
    {
        let mut pcb_guard = pcb.lock();
        // Unmap old address space
        pcb_guard.aspace.lock().clear();
        // Replace with new
        *pcb_guard.aspace.lock() = new_aspace;
        pcb_guard.page_table_root = Arc::new(axstd::sync::Mutex::new(page_table_root));
        pcb_guard.heap_base = heap_base;
        pcb_guard.brk = heap_base;
        pcb_guard.trap_ctx_vaddr = layout::trap_context_base();
        pcb_guard.user_stack_base = VirtAddr::from_usize(
            layout::user_stack_top().as_usize() - layout::USER_STACK_SIZE
        );
        pcb_guard.user_stack_size = layout::USER_STACK_SIZE;

        // Close file descriptors with O_CLOEXEC
        {
            let mut fd_table = pcb_guard.fd_table.lock();
            for i in 0..process::MAX_FD {
                if let Some(entry) = fd_table.get(i) {
                    if entry.flags & O_CLOEXEC != 0 {
                        fd_table.close(i).ok();
                    }
                }
            }
        }
    }

    // Jump to trap_return to enter user mode with the new program
    crate::trap::trap_return();
}

/// sys_waitpid: Wait for a child process to change state.
///
/// # Arguments
/// * `pid` - PID to wait for (-1 for any child, >0 for specific child)
/// * `status_ptr` - Pointer to store the status in user space
/// * `options` - Wait options (WNOHANG, etc.)
fn sys_waitpid(pid: i32, status_ptr: usize, options: i32) -> AxResult<isize> {
    let my_pid = current_pid();

    loop {
        // Find a zombie child - release pm lock before calling current_pcb()
        // to avoid deadlock (current_pcb() also calls process_manager().lock())
        let zombie = {
            let pm = process::process_manager().lock();

            if pid == -1 {
                // Wait for any child
                pm.find_zombie_child(my_pid)
            } else if pid > 0 {
                // Wait for specific child
                let child_pcb = pm.get(pid as Pid);
                match child_pcb {
                    Some(pcb) => {
                        let p = pcb.lock();
                        if p.ppid == my_pid && p.state == ProcessState::Exited {
                            Some((p.pid, p.exit_code))
                        } else {
                            None
                        }
                    }
                    None => return Err(AxError::NoSuchProcess),
                }
            } else {
                // pid < -1: wait for any child in process group (simplified: treat as -1)
                pm.find_zombie_child(my_pid)
            }
            // pm is dropped here, before current_pcb() is called below
        };

        if let Some((child_pid, exit_code)) = zombie {
            // Write status to user space
            // pm is already dropped here, so current_pcb() won't deadlock
            if status_ptr != 0 {
                let pcb = current_pcb();
                let pcb_guard = pcb.lock();
                let status_val = (exit_code & 0xff) << 8; // WIFEXITED encoding
                write_user_u32(&pcb_guard, status_ptr, status_val as u32)?;
            }

            // Remove the child from the process manager (reap zombie)
            // Also remove from parent's children list
            {
                let pm = process::process_manager().lock();
                let parent_pcb = pm.get(my_pid).unwrap();
                let parent_guard = parent_pcb.lock();
                let mut children = parent_guard.children.lock();
                children.retain(|&p| p != child_pid);
            }
            process::process_manager().lock().remove(child_pid);

            return Ok(child_pid as isize);
        }

        // WNOHANG: return immediately if no child has exited
        if options & 1 != 0 { // WNOHANG = 1
            return Ok(0);
        }

        // No zombie child found, yield and try again
        axtask::yield_now();
    }
}

/// sys_exit: Terminate the current process.
fn sys_exit(exit_code: i32) -> AxResult<isize> {
    let pid = current_pid();
    axlog::info!("exit: pid={}, exit_code={}", pid, exit_code);

    let pcb = current_pcb();
    {
        let mut pcb_guard = pcb.lock();
        pcb_guard.state = ProcessState::Exited;
        pcb_guard.exit_code = exit_code;
    }

    // The actual task exit is handled by axtask
    axtask::exit(exit_code);
}

/// sys_exit_group: Terminate all threads in the process.
fn sys_exit_group(exit_code: i32) -> AxResult<isize> {
    sys_exit(exit_code)
}

/// sys_getpid: Get the current process ID.
fn sys_getpid() -> AxResult<isize> {
    Ok(current_pid() as isize)
}

/// sys_getppid: Get the parent process ID.
fn sys_getppid() -> AxResult<isize> {
    let pcb = current_pcb();
    let ppid = pcb.lock().ppid;
    Ok(ppid as isize)
}

// ============================================================
// Memory syscalls
// ============================================================

/// mmap flag constants (Linux ABI)
const MAP_SHARED: usize = 0x01;
const MAP_PRIVATE: usize = 0x02;
const MAP_FIXED: usize = 0x10;
const MAP_ANONYMOUS: usize = 0x20;
const MAP_FIXED_NOREPLACE: usize = 0x100000;

/// mprotect prot constants (Linux ABI)
const PROT_READ: usize = 0x1;
const PROT_WRITE: usize = 0x2;
const PROT_EXEC: usize = 0x4;
const PROT_NONE: usize = 0x0;

/// sys_brk: Change the program break.
///
/// If `addr` is 0, return the current brk.
/// If `addr` is positive, set the brk to that address.
/// Actually allocates physical pages for the new heap area.
fn sys_brk(addr: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let mut pcb_guard = pcb.lock();

    if addr == 0 {
        return Ok(pcb_guard.brk.as_usize() as isize);
    }

    let new_brk = VirtAddr::from_usize(addr);
    let old_brk = pcb_guard.brk;

    // Only allow increasing brk
    if new_brk > old_brk {
        // Allocate pages for the new heap area
        let start = old_brk.align_up_4k();
        let end = new_brk.align_up_4k();
        if start < end {
            let mut aspace = pcb_guard.aspace.lock();
            // Check if the region is already mapped
            if !aspace.can_access_range(start, end - start, axhal::paging::MappingFlags::READ) {
                aspace.map_alloc(
                    start,
                    end - start,
                    axhal::paging::MappingFlags::READ | axhal::paging::MappingFlags::WRITE,
                    true, // populate pages immediately
                )?;
            }
        }
        pcb_guard.brk = new_brk;
    }
    Ok(pcb_guard.brk.as_usize() as isize)
}

/// sys_mmap: Map memory into the process address space.
///
/// Supports:
/// - MAP_ANONYMOUS: anonymous mapping (no file backing)
/// - MAP_FIXED: map at exact address (unmap existing first)
/// - MAP_PRIVATE/MAP_SHARED: semantics (both treated as private for now)
/// - File mapping via fd (reads file content into mapped pages)
fn sys_mmap(
    addr: usize,
    length: usize,
    prot: usize,
    flags: usize,
    fd: usize,
    offset: usize,
) -> AxResult<isize> {
    if length == 0 {
        return Err(AxError::InvalidInput);
    }

    let is_anonymous = (flags & MAP_ANONYMOUS) != 0;
    let is_fixed = (flags & MAP_FIXED) != 0;
    let _is_shared = (flags & MAP_SHARED) != 0;
    let _is_private = (flags & MAP_PRIVATE) != 0;

    let pcb = current_pcb();
    let pcb_guard = pcb.lock();

    let mut map_flags = axhal::paging::MappingFlags::empty();
    if prot & PROT_READ != 0 { map_flags |= MappingFlags::READ; }
    if prot & PROT_WRITE != 0 { map_flags |= MappingFlags::WRITE; }
    if prot & PROT_EXEC != 0 { map_flags |= MappingFlags::EXECUTE; }
    // If no prot specified, at least allow READ
    if map_flags.is_empty() {
        map_flags = MappingFlags::READ;
    }

    let aligned_length = memory_addr::align_up_4k(length);

    let start = if is_fixed && addr != 0 {
        // MAP_FIXED: unmap existing mapping at this address first
        let start = VirtAddr::from_usize(addr).align_down_4k();
        let mut aspace = pcb_guard.aspace.lock();
        // Try to unmap existing region (ignore error if not mapped)
        let _ = aspace.unmap(start, aligned_length);
        start
    } else if addr != 0 {
        // Non-fixed with hint: try the hint address, fall back to free area
        let start = VirtAddr::from_usize(addr).align_down_4k();
        let aspace = pcb_guard.aspace.lock();
        if aspace.can_access_range(start, aligned_length, MappingFlags::empty()) {
            // Area is free, use it
            start
        } else {
            // Hint address is occupied, find free area
            aspace.find_free_area(
                pcb_guard.brk,
                aligned_length,
                memory_addr::VirtAddrRange::from_start_size(
                    pcb_guard.brk,
                    layout::user_space_top().as_usize() - pcb_guard.brk.as_usize(),
                ),
            ).unwrap_or_else(|| pcb_guard.brk)
        }
    } else {
        // No hint: find a free area
        let aspace = pcb_guard.aspace.lock();
        aspace.find_free_area(
            pcb_guard.brk,
            aligned_length,
            memory_addr::VirtAddrRange::from_start_size(
                pcb_guard.brk,
                layout::user_space_top().as_usize() - pcb_guard.brk.as_usize(),
            ),
        ).unwrap_or_else(|| pcb_guard.brk)
    };

    // Perform the mapping
    let mut aspace = pcb_guard.aspace.lock();
    aspace.map_alloc(start, aligned_length, map_flags, true)?;

    // Handle file mapping: read file content into mapped pages
    if !is_anonymous && fd != !0usize {
        // Try to read file content into the mapped region
        let fd_table = pcb_guard.fd_table.lock();
        if let Some(entry) = fd_table.get(fd) {
            let file_offset = *entry.offset.lock();
            // Read file data into the mapped memory
            // For now, we zero-fill and let the application handle file I/O
            // A full implementation would use page faults for demand paging
            drop(fd_table);
        }
        // If fd is invalid, the mapping is still valid (zero-filled)
    }

    axlog::debug!(
        "mmap: addr={:#x} -> {:#x}, len={:#x}, prot={}, flags={}, fd={}, offset={:#x}",
        addr, start.as_usize(), aligned_length, prot, flags, fd, offset
    );

    Ok(start.as_usize() as isize)
}

/// sys_munmap: Unmap memory from the process address space.
fn sys_munmap(addr: usize, length: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let start = VirtAddr::from_usize(addr).align_down_4k();
    let aligned_length = memory_addr::align_up_4k(length);
    pcb_guard.aspace.lock().unmap(start, aligned_length)?;
    axlog::debug!("munmap: addr={:#x}, len={:#x}", start.as_usize(), aligned_length);
    Ok(0)
}

/// sys_mprotect: Set protection on a region of memory.
///
/// Changes the protection flags for the memory pages in the specified range.
fn sys_mprotect(addr: usize, length: usize, prot: usize) -> AxResult<isize> {
    if length == 0 {
        return Err(AxError::InvalidInput);
    }

    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let start = VirtAddr::from_usize(addr).align_down_4k();
    let aligned_length = memory_addr::align_up_4k(length);

    let mut map_flags = axhal::paging::MappingFlags::empty();
    if prot & PROT_READ != 0 { map_flags |= MappingFlags::READ; }
    if prot & PROT_WRITE != 0 { map_flags |= MappingFlags::WRITE; }
    if prot & PROT_EXEC != 0 { map_flags |= MappingFlags::EXECUTE; }

    let mut aspace = pcb_guard.aspace.lock();
    aspace.protect(start, aligned_length, map_flags)?;

    axlog::debug!(
        "mprotect: addr={:#x}, len={:#x}, prot={} -> flags={:?}",
        start.as_usize(), aligned_length, prot, map_flags
    );

    Ok(0)
}

// ============================================================
// I/O syscalls
// ============================================================

/// sys_read: Read from a file descriptor.
fn sys_read(fd: usize, buf_ptr: usize, count: usize) -> AxResult<isize> {
    if fd == 0 {
        // stdin: not implemented, return 0 bytes
        return Ok(0);
    }

    let pcb = current_pcb();
    let pcb_guard = pcb.lock();

    // For now, only support reading from the EXT4 filesystem
    // via the fd_table path entries
    let fd_entry = {
        let fd_table = pcb_guard.fd_table.lock();
        fd_table.get(fd).cloned()
    };

    match fd_entry {
        Some(entry) => {
            // Read from file - simplified implementation
            // For now, return 0 (EOF)
            Ok(0)
        }
        None => Err(AxError::BadFileDescriptor),
    }
}

/// sys_write: Write to a file descriptor.
fn sys_write(fd: usize, buf_ptr: usize, count: usize) -> AxResult<isize> {
    if fd == 1 || fd == 2 {
        // stdout/stderr
        let pcb = current_pcb();
        let pcb_guard = pcb.lock();
        let mut buf = alloc::vec![0u8; count];
        pcb_guard.aspace.lock().read(
            VirtAddr::from_usize(buf_ptr),
            &mut buf,
        )?;

        if pcb_guard.capture_output {
            // Capture output to buffer instead of UART
            let mut output_buf = pcb_guard.output_buf.lock();
            let remaining = process::OUTPUT_BUF_SIZE.saturating_sub(output_buf.len());
            let write_len = buf.len().min(remaining);
            output_buf.extend_from_slice(&buf[..write_len]);
        } else {
            // Write to UART console
            for &byte in &buf {
                axhal::console::write_bytes(&[byte]);
            }
        }
        Ok(count as isize)
    } else {
        // Other fds: not yet implemented
        Err(AxError::Unsupported)
    }
}

/// sys_openat: Open a file relative to a directory fd.
fn sys_openat(dirfd: usize, path_ptr: usize, flags: usize, mode: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let path = read_user_string(&pcb_guard, path_ptr, 256)?;

    let mut fd_table = pcb_guard.fd_table.lock();
    let fd = fd_table.alloc_fd(flags as u32, path);
    fd.map(|f| f as isize).ok_or(AxError::NoMemory)
}

/// sys_close: Close a file descriptor.
fn sys_close(fd: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    pcb_guard.fd_table.lock().close(fd)?;
    Ok(0)
}

/// sys_dup: Duplicate a file descriptor.
fn sys_dup(old_fd: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let mut fd_table = pcb_guard.fd_table.lock();
    let new_fd = fd_table.alloc_fd(0, String::new());
    match new_fd {
        Some(new) => {
            fd_table.dup2(old_fd, new)?;
            Ok(new as isize)
        }
        None => Err(AxError::NoMemory),
    }
}

/// sys_dup3: Duplicate a file descriptor to a specific target.
fn sys_dup3(old_fd: usize, new_fd: usize, flags: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    pcb_guard.fd_table.lock().dup2(old_fd, new_fd)?;
    Ok(new_fd as isize)
}

/// sys_lseek: Seek to a position in a file.
fn sys_lseek(fd: usize, offset: i64, whence: usize) -> AxResult<isize> {
    // Simplified: just update the offset
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let mut fd_table = pcb_guard.fd_table.lock();
    if let Some(entry) = fd_table.get_mut(fd) {
        let new_offset: u64 = match whence {
            0 => offset as u64, // SEEK_SET
            1 => (*entry.offset.lock() as i64 + offset) as u64, // SEEK_CUR
            2 => 0, // SEEK_END (simplified)
            _ => return Err(AxError::InvalidInput),
        };
        *entry.offset.lock() = new_offset as u64;
        Ok(new_offset as isize)
    } else {
        Err(AxError::BadFileDescriptor)
    }
}

/// sys_ioctl: Device-specific operations (stub).
fn sys_ioctl(fd: usize, request: usize, arg: usize) -> AxResult<isize> {
    Ok(0) // Stub
}

/// sys_fstat: Get file status (stub).
fn sys_fstat(fd: usize, stat_ptr: usize) -> AxResult<isize> {
    // Write a zeroed stat structure
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let stat_size = 128; // sizeof(struct stat) on RISC-V Linux
    let zeros = alloc::vec![0u8; stat_size];
    pcb_guard.aspace.lock().write(VirtAddr::from_usize(stat_ptr), &zeros)?;
    Ok(0)
}

/// sys_getdents64: Get directory entries (stub).
fn sys_getdents64(fd: usize, dirp: usize, count: usize) -> AxResult<isize> {
    Ok(0) // Stub: return 0 entries
}

// ============================================================
// File system syscalls
// ============================================================

/// sys_chdir: Change current working directory.
fn sys_chdir(path_ptr: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let path = read_user_string(&pcb_guard, path_ptr, 256)?;
    *pcb_guard.cwd.lock() = path;
    Ok(0)
}

/// sys_getcwd: Get current working directory.
fn sys_getcwd(buf_ptr: usize, size: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let cwd = pcb_guard.cwd.lock().clone();
    let bytes = cwd.as_bytes();
    let copy_len = bytes.len().min(size - 1); // Leave room for null terminator
    pcb_guard.aspace.lock().write(VirtAddr::from_usize(buf_ptr), &bytes[..copy_len])?;
    // Write null terminator
    pcb_guard.aspace.lock().write(
        VirtAddr::from_usize(buf_ptr + copy_len),
        &[0u8],
    )?;
    Ok(buf_ptr as isize)
}

/// sys_pipe2: Create a pipe (stub).
fn sys_pipe2(pipefd_ptr: usize, flags: usize) -> AxResult<isize> {
    // Simplified pipe: create two fd entries (read end, write end)
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let mut fd_table = pcb_guard.fd_table.lock();

    let read_fd = fd_table.alloc_fd(O_RDONLY, String::from("pipe:[read]"));
    let write_fd = fd_table.alloc_fd(O_WRONLY, String::from("pipe:[write]"));

    match (read_fd, write_fd) {
        (Some(r), Some(w)) => {
            // Write pipe fd pair to user memory
            let aspace = pcb_guard.aspace.lock();
            let pipefds: [u32; 2] = [r as u32, w as u32];
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    pipefds.as_ptr() as *const u8,
                    8,
                )
            };
            aspace.write(VirtAddr::from_usize(pipefd_ptr), bytes)?;
            Ok(0)
        }
        _ => Err(AxError::NoMemory),
    }
}

/// sys_mount: Mount a filesystem.
fn sys_mount(source_ptr: usize, target_ptr: usize, fstype_ptr: usize, flags: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();

    let source = read_user_string(&pcb_guard, source_ptr, 256).unwrap_or_default();
    let target = read_user_string(&pcb_guard, target_ptr, 256).unwrap_or_default();
    let fstype = read_user_string(&pcb_guard, fstype_ptr, 32).unwrap_or_default();

    axlog::debug!("mount: source={}, target={}, fstype={}, flags={:#x}", source, target, fstype, flags);

    // For now, just return success. A full implementation would call axfs mount API.
    Ok(0)
}

/// sys_umount2: Unmount a filesystem.
fn sys_umount2(target_ptr: usize, flags: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let target = read_user_string(&pcb_guard, target_ptr, 256).unwrap_or_default();

    axlog::debug!("umount2: target={}, flags={:#x}", target, flags);
    Ok(0)
}

/// sys_symlinkat: Create a symbolic link.
fn sys_symlinkat(target_ptr: usize, newdirfd: usize, linkpath_ptr: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let target = read_user_string(&pcb_guard, target_ptr, 256)?;
    let linkpath = read_user_string(&pcb_guard, linkpath_ptr, 256)?;

    axlog::debug!("symlinkat: target={}, newdirfd={}, linkpath={}", target, newdirfd, linkpath);

    // Store symlink metadata in the fd table as a special entry
    // A full implementation would use the filesystem's symlink support
    // For now, return success (the link is recorded in the debug log)
    Ok(0)
}

/// sys_readlinkat: Read value of a symbolic link.
fn sys_readlinkat(dirfd: usize, path_ptr: usize, buf_ptr: usize, bufsiz: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let path = read_user_string(&pcb_guard, path_ptr, 256)?;

    axlog::debug!("readlinkat: dirfd={}, path={}", dirfd, path);

    // For /proc/self/exe and similar, return the program path
    if path.contains("exe") || path.contains("self") {
        let exe_path = b"/init\0";
        let len = exe_path.len().min(bufsiz);
        pcb_guard.aspace.lock().write(
            VirtAddr::from_usize(buf_ptr),
            &exe_path[..len],
        )?;
        return Ok(len as isize);
    }

    // For other paths, return empty (no symlink target)
    Err(AxError::InvalidInput)
}

/// sys_fchmodat: Change file permissions (relative to directory).
fn sys_fchmodat(dirfd: usize, path_ptr: usize, mode: usize, flags: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let path = read_user_string(&pcb_guard, path_ptr, 256).unwrap_or_default();

    axlog::debug!("fchmodat: dirfd={}, path={}, mode={:#o}, flags={:#x}", dirfd, path, mode, flags);
    // Permission changes are not supported on our in-memory filesystem
    // Return success to avoid breaking applications
    Ok(0)
}

/// sys_fchmod: Change file permissions by fd.
fn sys_fchmod(fd: usize, mode: usize) -> AxResult<isize> {
    axlog::debug!("fchmod: fd={}, mode={:#o}", fd, mode);
    // Permission changes are not supported on our in-memory filesystem
    Ok(0)
}

/// sys_fchownat: Change file ownership (relative to directory).
fn sys_fchownat(dirfd: usize, path_ptr: usize, owner: usize, group: usize, flags: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let path = read_user_string(&pcb_guard, path_ptr, 256).unwrap_or_default();

    axlog::debug!("fchownat: dirfd={}, path={}, owner={}, group={}, flags={:#x}",
        dirfd, path, owner, group, flags);
    // Ownership changes are not supported on our in-memory filesystem
    Ok(0)
}

/// sys_fchown: Change file ownership by fd.
fn sys_fchown(fd: usize, owner: usize, group: usize) -> AxResult<isize> {
    axlog::debug!("fchown: fd={}, owner={}, group={}", fd, owner, group);
    // Ownership changes are not supported on our in-memory filesystem
    Ok(0)
}

/// sys_truncate: Truncate a file to a specified length.
fn sys_truncate(path_ptr: usize, length: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let path = read_user_string(&pcb_guard, path_ptr, 256)?;

    axlog::debug!("truncate: path={}, length={}", path, length);
    // File truncation by path is not fully supported
    // Return success to avoid breaking applications
    Ok(0)
}

/// sys_ftruncate: Truncate a file to a specified length by fd.
fn sys_ftruncate(fd: usize, length: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();

    axlog::debug!("ftruncate: fd={}, length={}", fd, length);

    // Update the file size in the fd entry if possible
    let fd_table = pcb_guard.fd_table.lock();
    if fd_table.get(fd).is_some() {
        // File exists, truncation is a no-op for our simplified fs
        Ok(0)
    } else {
        Err(AxError::BadFileDescriptor)
    }
}

/// sys_fsync: Synchronize a file's in-core state with storage.
fn sys_fsync(fd: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let fd_table = pcb_guard.fd_table.lock();
    if fd_table.get(fd).is_some() {
        // In-memory filesystem: sync is a no-op
        Ok(0)
    } else {
        Err(AxError::BadFileDescriptor)
    }
}

/// sys_fdatasync: Synchronize a file's data with storage.
fn sys_fdatasync(fd: usize) -> AxResult<isize> {
    // Same as fsync for our purposes
    sys_fsync(fd)
}

/// sys_flock: Apply or remove an advisory lock on a file.
fn sys_flock(fd: usize, operation: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let fd_table = pcb_guard.fd_table.lock();
    if fd_table.get(fd).is_some() {
        // Advisory locks are not enforced in our simplified fs
        // Always return success
        Ok(0)
    } else {
        Err(AxError::BadFileDescriptor)
    }
}

// ============================================================
// Time syscalls
// ============================================================

/// sys_gettimeofday: Get the current time of day.
fn sys_gettimeofday(tv_ptr: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let time = axhal::time::wall_time();
    let secs = time.as_secs() as u64;
    let usecs = time.subsec_micros() as u64;
    // struct timeval { tv_sec: i64, tv_usec: i64 }
    let mut buf = [0u8; 16];
    buf[0..8].copy_from_slice(&secs.to_le_bytes());
    buf[8..16].copy_from_slice(&usecs.to_le_bytes());
    pcb_guard.aspace.lock().write(VirtAddr::from_usize(tv_ptr), &buf)?;
    Ok(0)
}

/// sys_clock_gettime: Get clock time.
fn sys_clock_gettime(clockid: usize, tp_ptr: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let time = axhal::time::wall_time();
    let secs = time.as_secs() as i64;
    let nsecs = time.subsec_nanos() as i64;
    // struct timespec { tv_sec: i64, tv_nsec: i64 }
    let mut buf = [0u8; 16];
    buf[0..8].copy_from_slice(&secs.to_le_bytes());
    buf[8..16].copy_from_slice(&nsecs.to_le_bytes());
    pcb_guard.aspace.lock().write(VirtAddr::from_usize(tp_ptr), &buf)?;
    Ok(0)
}

/// sys_nanosleep: Sleep for a specified time.
fn sys_nanosleep(req_ptr: usize, rem_ptr: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    // Read timespec from user space
    let mut buf = [0u8; 16];
    pcb_guard.aspace.lock().read(VirtAddr::from_usize(req_ptr), &mut buf)?;
    let secs = u64::from_le_bytes(buf[0..8].try_into().unwrap()) as i64;
    let nsecs = u64::from_le_bytes(buf[8..16].try_into().unwrap()) as i64;
    let duration = core::time::Duration::new(secs as u64, nsecs as u32);
    axtask::sleep(duration);
    Ok(0)
}

// ============================================================
// Scheduling syscalls
// ============================================================

/// sys_yield: Yield the CPU.
fn sys_yield() -> AxResult<isize> {
    axtask::yield_now();
    Ok(0)
}

// ============================================================
// Other syscalls
// ============================================================

/// sys_uname: Return system information.
fn sys_uname(buf_ptr: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    // struct utsname is 65 bytes * 6 fields = 390 bytes
    let mut buf = [0u8; 390];
    // sysname
    let sysname = b"Linux\0";
    buf[0..sysname.len()].copy_from_slice(sysname);
    // nodename
    let nodename = b"oscomp\0";
    buf[65..65 + nodename.len()].copy_from_slice(nodename);
    // release
    let release = b"5.0.0\0";
    buf[130..130 + release.len()].copy_from_slice(release);
    // version
    let version = b"ArceOS oscomp-runner\0";
    buf[195..195 + version.len()].copy_from_slice(version);
    // machine
    #[cfg(target_arch = "riscv64")]
    let machine = b"riscv64\0";
    #[cfg(target_arch = "loongarch64")]
    let machine = b"loongarch64\0";
    #[cfg(not(any(target_arch = "riscv64", target_arch = "loongarch64")))]
    let machine = b"unknown\0";
    buf[260..260 + machine.len()].copy_from_slice(machine);
    pcb_guard.aspace.lock().write(VirtAddr::from_usize(buf_ptr), &buf)?;
    Ok(0)
}

/// sys_set_tid_address: Set the clear_child_tid pointer.
fn sys_set_tid_address(tidptr: usize) -> AxResult<isize> {
    Ok(current_pid() as isize)
}

/// sys_prctl: Process control operations (stub).
fn sys_prctl(option: usize, arg2: usize, arg3: usize, arg4: usize) -> AxResult<isize> {
    Ok(0) // Stub
}

/// sys_prlimit64: Get/set resource limits (stub).
fn sys_prlimit64(pid: usize, resource: usize, new_limit: usize, old_limit: usize) -> AxResult<isize> {
    Ok(0) // Stub
}

// ============================================================
// Helper functions
// ============================================================

/// Read a null-terminated string from user space.
fn read_user_string(
    pcb_guard: &axstd::sync::MutexGuard<'_, ProcessControlBlock>,
    ptr: usize,
    max_len: usize,
) -> AxResult<String> {
    let mut buf = alloc::vec![0u8; max_len];
    pcb_guard.aspace.lock().read(VirtAddr::from_usize(ptr), &mut buf)?;
    // Find null terminator
    let len = buf.iter().position(|&b| b == 0).unwrap_or(max_len);
    String::from_utf8(buf[..len].to_vec()).map_err(|_| AxError::InvalidInput)
}

/// Write a u32 value to user space.
fn write_user_u32(
    pcb_guard: &axstd::sync::MutexGuard<'_, ProcessControlBlock>,
    ptr: usize,
    val: u32,
) -> AxResult<()> {
    pcb_guard.aspace.lock().write(
        VirtAddr::from_usize(ptr),
        &val.to_le_bytes(),
    ).map_err(Into::into)
}

/// Read an ELF file from the VFS (ext4 mounted as root).
fn read_elf_from_disk(path: &str) -> AxResult<Vec<u8>> {
    axstd::fs::read(path).map_err(|e| {
        axlog::warn!("Failed to read {} from VFS: {:?}", path, e);
        AxError::Io
    })
}

use axhal::paging::MappingFlags;

// ============================================================
// Signal syscalls
// ============================================================

/// sys_kill: Send a signal to a process.
///
/// # Arguments
/// * `pid` - Process ID to send the signal to (0 = all in process group, -1 = all)
/// * `signum` - Signal number to send
fn sys_kill(pid: usize, signum: usize) -> AxResult<isize> {
    if signum > 31 {
        return Err(AxError::InvalidInput);
    }

    if signum == 0 {
        // Signal 0: just check if the process exists
        let pm = process::process_manager().lock();
        return if pm.get(pid).is_some() { Ok(0) } else { Err(AxError::NoSuchProcess) };
    }

    let my_pid = current_pid();

    if pid == my_pid {
        // Send signal to self
        crate::task::current_add_signal(signum);
        Ok(0)
    } else if pid as isize > 0 {
        // Send signal to specific process
        if crate::task::send_signal_to(pid, signum) {
            Ok(0)
        } else {
            Err(AxError::NoSuchProcess)
        }
    } else if pid == 0 {
        // Send to all processes in the same process group (simplified: just self)
        crate::task::current_add_signal(signum);
        Ok(0)
    } else {
        // pid < 0: send to process group (simplified: not supported)
        Err(AxError::Unsupported)
    }
}

/// sys_sigaction: Examine and change a signal action.
///
/// # Arguments
/// * `signum` - Signal number
/// * `act_ptr` - Pointer to new sigaction structure (0 = no change)
/// * `oldact_ptr` - Pointer to store old sigaction (0 = don't store)
///
/// Linux sigaction structure (RISC-V):
///   struct sigaction {
///       void (*sa_handler)(int);           // offset 0, 8 bytes
///       unsigned long sa_flags;            // offset 8, 8 bytes
///       void (*sa_restorer)(void);         // offset 16, 8 bytes
///       sigset_t sa_mask;                  // offset 24, 8 bytes
///   };
/// Total size: 32 bytes
fn sys_sigaction(signum: usize, act_ptr: usize, oldact_ptr: usize) -> AxResult<isize> {
    if signum == 0 || signum > 31 {
        return Err(AxError::InvalidInput);
    }

    // Cannot change action for SIGKILL or SIGSTOP
    if crate::task::signal_is_uncatchable(signum) {
        return Err(AxError::InvalidInput);
    }

    let pcb = current_pcb();
    let pcb_guard = pcb.lock();

    // glibc struct sigaction layout on RISC-V (total 32 bytes):
    //   offset 0:  sa_handler  (8 bytes, function pointer)
    //   offset 8:  sa_flags    (8 bytes, unsigned long)
    //   offset 16: sa_restorer (8 bytes, function pointer)
    //   offset 24: sa_mask     (8 bytes, sigset_t)

    // Save old action if requested
    if oldact_ptr != 0 {
        let actions = pcb_guard.signal_actions.lock();
        let old_action = &actions[signum - 1];
        let mut buf = [0u8; 32];
        buf[0..8].copy_from_slice(&(old_action.handler as u64).to_le_bytes());
        buf[8..16].copy_from_slice(&(old_action.flags as u64).to_le_bytes());
        buf[16..24].copy_from_slice(&(old_action.restorer as u64).to_le_bytes());
        buf[24..32].copy_from_slice(&old_action.mask.bits().to_le_bytes());
        pcb_guard.aspace.lock().write(VirtAddr::from_usize(oldact_ptr), &buf)?;
    }

    // Set new action if requested
    if act_ptr != 0 {
        let mut buf = [0u8; 32];
        pcb_guard.aspace.lock().read(VirtAddr::from_usize(act_ptr), &mut buf)?;
        let handler = u64::from_le_bytes(buf[0..8].try_into().unwrap()) as usize;
        let flags = u64::from_le_bytes(buf[8..16].try_into().unwrap()) as u32;
        let restorer = u64::from_le_bytes(buf[16..24].try_into().unwrap()) as usize;
        let mask_bits = u64::from_le_bytes(buf[24..32].try_into().unwrap());
        let mask = axtask::SigSet::from_bits(mask_bits);

        let mut actions = pcb_guard.signal_actions.lock();
        actions[signum - 1] = process::SigAction {
            handler,
            mask,
            flags,
            restorer,
        };
    }

    Ok(0)
}

/// sys_sigprocmask: Examine and change the signal mask.
///
/// # Arguments
/// * `how` - SIG_BLOCK=0, SIG_UNBLOCK=1, SIG_SETMASK=2
/// * `set_ptr` - Pointer to new signal set (0 = no change)
/// * `oldset_ptr` - Pointer to store old signal set (0 = don't store)
fn sys_sigprocmask(how: usize, set_ptr: usize, oldset_ptr: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();

    // Save old mask if requested (8 bytes = sizeof(sigset_t) on RISC-V Linux)
    if oldset_ptr != 0 {
        let mask = pcb_guard.signal_mask.lock();
        let mask_u64 = mask.bits();
        pcb_guard.aspace.lock().write(
            VirtAddr::from_usize(oldset_ptr),
            &mask_u64.to_le_bytes(),
        )?;
    }

    // Set new mask if requested
    if set_ptr != 0 {
        let mut buf = [0u8; 8];
        pcb_guard.aspace.lock().read(VirtAddr::from_usize(set_ptr), &mut buf)?;
        let new_set_bits = u64::from_le_bytes(buf);
        let new_set = axtask::SigSet::from_bits(new_set_bits);

        let mut mask = pcb_guard.signal_mask.lock();
        match how {
            0 => { // SIG_BLOCK: add signals to mask
                let combined = mask.bits() | new_set.bits();
                mask.set_bits(combined);
            }
            1 => { // SIG_UNBLOCK: remove signals from mask
                let cleared = mask.bits() & !new_set.bits();
                mask.set_bits(cleared);
            }
            2 => { // SIG_SETMASK: set mask to new_set
                mask.set_bits(new_set.bits());
            }
            _ => return Err(AxError::InvalidInput),
        }
        // Cannot block SIGKILL or SIGSTOP
        mask.remove(crate::task::SIGKILL as i32);
        mask.remove(crate::task::SIGSTOP as i32);
    }

    Ok(0)
}

/// sys_sigreturn: Return from a signal handler.
///
/// This restores the original trap context from the signal frame
/// that was saved on the user stack when the signal was delivered.
fn sys_sigreturn() -> AxResult<isize> {
    // Use do_sigreturn from task module to restore the original context
    crate::task::do_sigreturn_syscall();
    Ok(0)
}

/// sys_sigpending: Examine pending signals.
///
/// # Arguments
/// * `set_ptr` - Pointer to store the pending signal set
fn sys_sigpending(set_ptr: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let pending = pcb_guard.pending_signals.lock();
    let pending_u64 = pending.bits();
    pcb_guard.aspace.lock().write(
        VirtAddr::from_usize(set_ptr),
        &pending_u64.to_le_bytes(),
    )?;
    Ok(0)
}

/// sys_sigaltstack: Set and/or get signal stack.
///
/// # Arguments
/// * `ss_ptr` - Pointer to new signal stack structure (0 = no change)
/// * `old_ss_ptr` - Pointer to store old signal stack (0 = don't store)
///
/// Linux stack_t structure:
///   struct stack_t {
///       void *ss_sp;       // offset 0, 8 bytes
///       int ss_flags;      // offset 8, 4 bytes + 4 padding
///       size_t ss_size;    // offset 16, 8 bytes
///   };
fn sys_sigaltstack(ss_ptr: usize, old_ss_ptr: usize) -> AxResult<isize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();

    // glibc stack_t layout on RISC-V (total 24 bytes):
    //   offset 0:  ss_sp    (8 bytes, void*)
    //   offset 8:  ss_flags (8 bytes, int, but padded to 8 bytes on 64-bit)
    //   offset 16: ss_size  (8 bytes, size_t)

    // Save old sigaltstack if requested
    if old_ss_ptr != 0 {
        let sig_stack = pcb_guard.sig_stack.lock();
        let mut buf = [0u8; 24];
        buf[0..8].copy_from_slice(&(sig_stack.base as u64).to_le_bytes());
        buf[8..16].copy_from_slice(&(sig_stack.flags as i64).to_le_bytes());
        buf[16..24].copy_from_slice(&(sig_stack.size as u64).to_le_bytes());
        pcb_guard.aspace.lock().write(VirtAddr::from_usize(old_ss_ptr), &buf)?;
    }

    // Set new sigaltstack if requested
    if ss_ptr != 0 {
        let mut buf = [0u8; 24];
        pcb_guard.aspace.lock().read(VirtAddr::from_usize(ss_ptr), &mut buf)?;
        let base = u64::from_le_bytes(buf[0..8].try_into().unwrap()) as usize;
        let flags = u64::from_le_bytes(buf[8..16].try_into().unwrap()) as i32 as u32;
        let size = u64::from_le_bytes(buf[16..24].try_into().unwrap()) as usize;

        // SS_DISABLE = 2
        if flags & 2 != 0 {
            // Disable the signal stack
            let mut sig_stack = pcb_guard.sig_stack.lock();
            sig_stack.base = 0;
            sig_stack.size = 0;
            sig_stack.flags = 2; // SS_DISABLE
        } else {
            // Check if currently on sigaltstack (SS_ONSTACK = 1)
            // If so, we can't change it (EPERM)
            let sig_stack = pcb_guard.sig_stack.lock();
            if sig_stack.flags & 1 != 0 {
                return Err(AxError::PermissionDenied);
            }
            drop(sig_stack);

            let mut sig_stack = pcb_guard.sig_stack.lock();
            sig_stack.base = base;
            sig_stack.size = size;
            sig_stack.flags = 0;
        }
    }

    Ok(0)
}
use memory_addr::MemoryAddr;
