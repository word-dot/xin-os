

//! Process management module for multi-process support.
//!
//! This module implements the Process Control Block (PCB), process manager,
//! and related data structures needed to extend ArceOS from a unikernel
//! to a multi-process system supporting fork/execve/wait/exit.

extern crate alloc;

use alloc::{
    collections::BTreeMap,
    string::String,
    sync::Arc,
    vec::Vec,
};
use axmm::AddrSpace;
use axstd::sync::Mutex;
use memory_addr::VirtAddr;
use axerrno::{AxError, AxResult};

/// Process ID type.
pub type Pid = usize;

/// Process state enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Ready,
    Running,
    Blocked,
    Exited,
}

/// Maximum number of file descriptors per process.
pub const MAX_FD: usize = 64;

/// File descriptor table for a process.
pub struct FdTable {
    table: Vec<Option<FdEntry>>,
    /// Socket file descriptors (fd -> Socket)
    sockets: BTreeMap<usize, Arc<crate::net::Socket>>,
}

/// A single file descriptor entry.
#[derive(Debug, Clone)]
pub struct FdEntry {
    pub fd: usize,
    pub flags: u32,
    pub path: String,
    pub offset: Arc<Mutex<u64>>,
}

/// File open flags.
pub const O_RDONLY: u32 = 0;
pub const O_WRONLY: u32 = 1;
pub const O_RDWR: u32 = 2;
pub const O_CREAT: u32 = 0o100;
pub const O_TRUNC: u32 = 0o1000;
pub const O_CLOEXEC: u32 = 0o2000000;
pub const O_DIRECTORY: u32 = 0o200000;

/// Maximum signal number supported (matching axtask).
pub const MAX_SIGNUM: usize = axtask::MAX_SIGNUM;

/// Signal handler type: the function pointer (0 = SIG_DFL, 1 = SIG_IGN).
pub type SignalHandler = usize;

/// Signal action structure compatible with axtask::SigAction.
#[derive(Debug, Clone, Copy)]
pub struct SigAction {
    /// Signal handler function pointer (user-space address).
    /// 0 = SIG_DFL (default), 1 = SIG_IGN (ignore).
    pub handler: SignalHandler,
    /// Signal mask during handler execution.
    pub mask: axtask::SigSet,
    /// Flags (SA_SIGINFO, SA_RESTART, etc. - simplified).
    pub flags: u32,
    /// Restorer function pointer (for sigreturn).
    pub restorer: SignalHandler,
}

impl Default for SigAction {
    fn default() -> Self {
        Self {
            handler: 0, // SIG_DFL
            mask: axtask::SigSet::new(),
            flags: 0,
            restorer: 0,
        }
    }
}

impl From<&SigAction> for axtask::SigAction {
    fn from(a: &SigAction) -> Self {
        axtask::SigAction {
            sa_handler: a.handler,
            sa_mask: a.mask,
            sa_flags: a.flags,
            sa_restorer: a.restorer,
        }
    }
}

impl From<&axtask::SigAction> for SigAction {
    fn from(a: &axtask::SigAction) -> Self {
        SigAction {
            handler: a.sa_handler,
            mask: a.sa_mask,
            flags: a.sa_flags,
            restorer: a.sa_restorer,
        }
    }
}

/// Signal stack information.
#[derive(Debug, Clone, Copy)]
pub struct SigStackInfo {
    /// Base address of the signal stack.
    pub base: usize,
    /// Size of the signal stack.
    pub size: usize,
    /// Flags (SS_ONSTACK, SS_DISABLE).
    pub flags: u32,
}

impl Default for SigStackInfo {
    fn default() -> Self {
        Self {
            base: 0,
            size: 0,
            flags: 2, // SS_DISABLE
        }
    }
}

/// Maximum output capture buffer size (256 KB).
pub const OUTPUT_BUF_SIZE: usize = 256 * 1024;

/// Process Control Block.
pub struct ProcessControlBlock {
    pub pid: Pid,
    pub ppid: Pid,
    pub state: ProcessState,
    pub exit_code: i32,
    pub aspace: Arc<Mutex<AddrSpace>>,
    pub user_stack_base: VirtAddr,
    pub user_stack_size: usize,
    pub heap_base: VirtAddr,
    pub brk: VirtAddr,
    pub fd_table: Arc<Mutex<FdTable>>,
    pub cwd: Arc<Mutex<String>>,
    pub children: Arc<Mutex<Vec<Pid>>>,
    pub trap_ctx_vaddr: VirtAddr,
    pub page_table_root: Arc<Mutex<u64>>,

    // Signal handling
    /// Signal action table (indexed by signal number - 1).
    pub signal_actions: Arc<Mutex<[SigAction; MAX_SIGNUM]>>,
    /// Signal mask (blocked signals).
    pub signal_mask: Arc<Mutex<axtask::SigSet>>,
    /// Pending signals for this process.
    pub pending_signals: Arc<Mutex<axtask::SigSet>>,
    /// Signal stack info.
    pub sig_stack: Arc<Mutex<SigStackInfo>>,

    // Output capture
    /// Whether to capture stdout/stderr output instead of writing to UART.
    pub capture_output: bool,
    /// Captured stdout/stderr output buffer.
    pub output_buf: Arc<Mutex<Vec<u8>>>,
}

impl FdTable {
    pub fn new() -> Self {
        let mut table: Vec<Option<FdEntry>> = (0..MAX_FD).map(|_| None).collect();
        table[0] = Some(FdEntry {
            fd: 0, flags: O_RDONLY,
            path: String::from("/dev/stdin"),
            offset: Arc::new(Mutex::new(0)),
        });
        table[1] = Some(FdEntry {
            fd: 1, flags: O_WRONLY,
            path: String::from("/dev/stdout"),
            offset: Arc::new(Mutex::new(0)),
        });
        table[2] = Some(FdEntry {
            fd: 2, flags: O_WRONLY,
            path: String::from("/dev/stderr"),
            offset: Arc::new(Mutex::new(0)),
        });
        Self { table, sockets: BTreeMap::new() }
    }

    /// Allocate a file descriptor for a network socket.
    pub fn alloc_net_fd(&mut self) -> Option<usize> {
        for i in 0..MAX_FD {
            if self.table[i].is_none() && !self.sockets.contains_key(&i) {
                return Some(i);
            }
        }
        None
    }

    /// Set a socket for the given fd.
    pub fn set_socket(&mut self, fd: usize, socket: Arc<crate::net::Socket>) {
        self.sockets.insert(fd, socket);
    }

    /// Get a socket for the given fd.
    pub fn get_socket(&self, fd: usize) -> Option<&Arc<crate::net::Socket>> {
        self.sockets.get(&fd)
    }

    pub fn alloc_fd(&mut self, flags: u32, path: String) -> Option<usize> {
        for i in 0..MAX_FD {
            if self.table[i].is_none() {
                self.table[i] = Some(FdEntry {
                    fd: i, flags, path,
                    offset: Arc::new(Mutex::new(0)),
                });
                return Some(i);
            }
        }
        None
    }

    pub fn get(&self, fd: usize) -> Option<&FdEntry> {
        if fd < MAX_FD { self.table[fd].as_ref() } else { None }
    }

    pub fn get_mut(&mut self, fd: usize) -> Option<&mut FdEntry> {
        if fd < MAX_FD { self.table[fd].as_mut() } else { None }
    }

    pub fn close(&mut self, fd: usize) -> AxResult {
        if fd < MAX_FD && self.table[fd].is_some() {
            self.table[fd] = None;
            Ok(())
        } else {
            Err(AxError::InvalidInput)
        }
    }

    pub fn dup2(&mut self, old_fd: usize, new_fd: usize) -> AxResult<usize> {
        if old_fd >= MAX_FD || self.table[old_fd].is_none() {
            return Err(AxError::InvalidInput);
        }
        if new_fd >= MAX_FD {
            return Err(AxError::InvalidInput);
        }
        let entry = self.table[old_fd].clone();
        self.table[new_fd] = entry.map(|mut e| { e.fd = new_fd; e });
        Ok(new_fd)
    }

    pub fn fork(&self) -> Self {
        let new_table: Vec<Option<FdEntry>> = self.table.iter().map(|e| e.clone()).collect();
        // Sockets are not inherited across fork (child gets empty socket table)
        Self { table: new_table, sockets: BTreeMap::new() }
    }
}

impl Default for FdTable {
    fn default() -> Self { Self::new() }
}

// ============================================================
// Global Process Manager
// ============================================================

pub struct ProcessManager {
    processes: BTreeMap<Pid, Arc<Mutex<ProcessControlBlock>>>,
    next_pid: Pid,
}

use lazyinit::LazyInit;

static PROCESS_MANAGER: LazyInit<Mutex<ProcessManager>> = LazyInit::new();

pub fn process_manager() -> &'static Mutex<ProcessManager> {
    if !PROCESS_MANAGER.is_inited() {
        PROCESS_MANAGER.init_once(Mutex::new(ProcessManager::new()));
    }
    &PROCESS_MANAGER
}

/// Initialize the process manager (call once at boot).
pub fn init_process_manager() {
    PROCESS_MANAGER.init_once(Mutex::new(ProcessManager::new()));
}

impl ProcessManager {
    pub fn new() -> Self {
        Self { processes: BTreeMap::new(), next_pid: 1 }
    }

    pub fn alloc_pid(&mut self) -> Pid {
        let pid = self.next_pid;
        self.next_pid += 1;
        pid
    }

    pub fn add(&mut self, pcb: Arc<Mutex<ProcessControlBlock>>) {
        let pid = pcb.lock().pid;
        self.processes.insert(pid, pcb);
    }

    pub fn get(&self, pid: Pid) -> Option<Arc<Mutex<ProcessControlBlock>>> {
        self.processes.get(&pid).cloned()
    }

    pub fn remove(&mut self, pid: Pid) -> Option<Arc<Mutex<ProcessControlBlock>>> {
        self.processes.remove(&pid)
    }

    pub fn count(&self) -> usize {
        self.processes.len()
    }

    pub fn find_zombie_child(&self, ppid: Pid) -> Option<(Pid, i32)> {
        for (_, pcb) in &self.processes {
            let p = pcb.lock();
            if p.ppid == ppid && p.state == ProcessState::Exited {
                return Some((p.pid, p.exit_code));
            }
        }
        None
    }

    pub fn has_children(&self, pid: Pid) -> bool {
        for (_, pcb) in &self.processes {
            if pcb.lock().ppid == pid { return true; }
        }
        false
    }
}

impl Default for ProcessManager {
    fn default() -> Self { Self::new() }
}

// ============================================================
// User space layout (runtime functions, not const)
// ============================================================

pub mod layout {
    use memory_addr::VirtAddr;

    pub fn user_space_base() -> VirtAddr { VirtAddr::from_usize(0x0) }
    pub fn user_space_top() -> VirtAddr { VirtAddr::from_usize(0x4000_0000_0000) }
    pub fn user_space_size() -> usize { user_space_top().as_usize() - user_space_base().as_usize() }
    pub fn user_stack_top() -> VirtAddr { VirtAddr::from_usize(0x3F00_0000) }
    pub const USER_STACK_SIZE: usize = 0x800_000;
    pub fn user_heap_base() -> VirtAddr { VirtAddr::from_usize(0x4000_0000) }
    pub fn trap_context_base() -> VirtAddr { VirtAddr::from_usize(0x3FFF_E000) }
    pub fn sigtrampoline() -> VirtAddr { VirtAddr::from_usize(0x3FFF_D000) }
}

// ============================================================
// Process creation helpers
// ============================================================

use axhal::paging::MappingFlags;

/// Create the init process (PID=1) from an ELF binary.
///
/// This is called once at boot to create the first user process.
pub fn create_init_process(
    elf_data: &[u8],
) -> AxResult<Arc<Mutex<ProcessControlBlock>>> {
    let pid = process_manager().lock().alloc_pid();

    let mut aspace = axmm::new_user_aspace(
        layout::user_space_base(),
        layout::user_space_size(),
    )?;

    // Map trampoline code into user address space
    crate::trap::map_trampoline_to_user(&mut aspace);

    let (entry_point, _stack_top, heap_base) = crate::elf_loader::load_elf(
        elf_data,
        &mut aspace,
        layout::user_stack_top(),
        layout::USER_STACK_SIZE,
    )?;

    let page_table_root = aspace.page_table_root().as_usize() as u64;

    // Unmap any existing mappings at trap_context_base and sigtrampoline
    // that may have been inherited from kernel via copy_mappings_from.
    let _ = aspace.unmap(layout::trap_context_base(), 4096);
    let _ = aspace.unmap(layout::sigtrampoline(), 4096);

    // Map the trap context page
    aspace.map_alloc(
        layout::trap_context_base(),
        4096,
        MappingFlags::READ | MappingFlags::WRITE,
        true,
    )?;

    // Map the sigreturn trampoline page and write the sigreturn code
    // RISC-V sigreturn trampoline: executes "ecall" with a7=139 (SYS_rt_sigreturn)
    aspace.map_alloc(
        layout::sigtrampoline(),
        4096,
        MappingFlags::READ | MappingFlags::EXECUTE,
        true,
    )?;
    // RISC-V sigreturn trampoline code:
    //   li a7, 139        # SYS_rt_sigreturn = 139
    //   ecall
    // This is: addi a7, zero, 139 = 0x08b00893; ecall = 0x00000073
    let sigreturn_code: [u8; 8] = [
        0x93, 0x08, 0xb0, 0x08, // li a7, 139
        0x73, 0x00, 0x00, 0x00, // ecall
    ];
    aspace.write(layout::sigtrampoline(), &sigreturn_code)?;

    // Initialize trap context in user space
    #[cfg(target_arch = "riscv64")]
    {
        let trap_ctx = crate::trap::TrapContext::app_init_context(
            entry_point.as_usize(),
            layout::user_stack_top().as_usize(),
            axmm::kernel_page_table_root().as_usize(),
            0,
            crate::trap::trap_handler as usize,
        );
        aspace.write(layout::trap_context_base(), unsafe {
            core::slice::from_raw_parts(
                &trap_ctx as *const _ as *const u8,
                core::mem::size_of::<crate::trap::TrapContext>(),
            )
        })?;
    }

    let pcb = Arc::new(Mutex::new(ProcessControlBlock {
        pid,
        ppid: 0,
        state: ProcessState::Ready,
        exit_code: 0,
        aspace: Arc::new(Mutex::new(aspace)),
        user_stack_base: VirtAddr::from_usize(
            layout::user_stack_top().as_usize() - layout::USER_STACK_SIZE
        ),
        user_stack_size: layout::USER_STACK_SIZE,
        heap_base,
        brk: heap_base,
        fd_table: Arc::new(Mutex::new(FdTable::new())),
        cwd: Arc::new(Mutex::new(String::from("/"))),
        children: Arc::new(Mutex::new(Vec::new())),
        trap_ctx_vaddr: layout::trap_context_base(),
        page_table_root: Arc::new(Mutex::new(page_table_root)),
        signal_actions: Arc::new(Mutex::new([SigAction::default(); MAX_SIGNUM])),
        signal_mask: Arc::new(Mutex::new(axtask::SigSet::new())),
        pending_signals: Arc::new(Mutex::new(axtask::SigSet::new())),
        sig_stack: Arc::new(Mutex::new(SigStackInfo::default())),
        capture_output: false,
        output_buf: Arc::new(Mutex::new(Vec::new())),
    }));

    // [non-UTF-8 comment removed]
    axlog::warn!(
        "Process Heap Initialization: pid={}, heap_base={:#x}, initial_brk={:#x}, stack_base={:#x}, stack_size={}",
        pid,
        heap_base.as_usize(),
        heap_base.as_usize(),
        layout::user_stack_top().as_usize() - layout::USER_STACK_SIZE,
        layout::USER_STACK_SIZE
    );

    process_manager().lock().add(pcb.clone());
    Ok(pcb)
}

