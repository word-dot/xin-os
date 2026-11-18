/* Process Control Block (PCB) Implementation */

use axalloc::{GlobalPage, PAGE_SIZE};
use axhal::trap::TrapContext;
use axhal::cpu::Context;
use axsync::Mutex;
use std::sync::Arc;

/// Process state enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Ready,
    Running,
    Blocked,
    Exited,
}

/// Process ID type
pub type Pid = usize;

/// Maximum file descriptors per process
const MAX_FD: usize = 64;

/// Process Control Block
#[derive(Debug)]
pub struct ProcessControlBlock {
    /// Unique process identifier
    pub pid: Pid,

    /// Parent process ID (0 = init process)
    pub ppid: Pid,

    /// Process name (for debugging)
    pub name: &'static str,

    /// Current process state
    pub state: ProcessState,

    /// Scheduling priority (lower = higher priority)
    pub priority: usize,

    /// Kernel stack memory block (4KB)
    pub kernel_stack: GlobalPage,

    /// User-space trap context (system call return)
    pub trap_context: *mut TrapContext,

    /// Kernel scheduling context
    pub context: Context,

    /// File descriptor table
    pub fd_table: Arc<Mutex<Vec<Option<FileDescriptor>>>>,

    /// Process page table root
    pub page_table: Arc<Mutex<PageTable>>,

    /// Current working directory
    pub cwd: String,

    /// Exit status code (if exited)
    pub exit_code: Option<i32>,

    /// Process statistics
    pub stats: ProcessStats,
}

/// File descriptor structure
#[derive(Debug)]
pub struct FileDescriptor {
    pub fd: usize,
    pub file: Arc<dyn FileOps>,
    pub flags: OpenFlags,
}

/// Process statistics
#[derive(Debug, Default, Clone)]
pub struct ProcessStats {
    pub cpu_time: u64,
    pub syscall_cnt: u64,
    pub context_switches: u64,
}

/// Allocate a new process control block
pub fn create_process(
    name: &'static str,
    entry: usize,
    args: &[usize],
    priority: usize,
) -> Arc<Mutex<ProcessControlBlock>> {
    // 1. Allocate kernel stack (4KB)
    let kernel_stack = GlobalPage::alloc().expect("Kernel stack allocation failed");

    // 2. Initialize trap context (user-mode entry point)
    let trap_ctx = unsafe {
        let sp = kernel_stack.start_vaddr().as_usize() + kernel_stack.size();
        let mut ctx = TrapContext::new_user(entry, sp, args);
        ctx.set_user_mode();  // Mark as user mode
        let ptr = (sp - core::mem::size_of::<TrapContext>()) as *mut TrapContext;
        core::ptr::write(ptr, ctx);
        ptr
    };

    // 3. Initialize kernel context
    let context = Context::default();

    // 4. Create file descriptor table
    let fd_table = Arc::new(Mutex::new(vec![None; MAX_FD]));

    // 5. Initialize page table
    let page_table = Arc::new(Mutex::new(PageTable::new()));

    // 6. Create PCB
    let pcb = ProcessControlBlock {
        pid: allocate_pid(),
        ppid: 0,
        name,
        state: ProcessState::Ready,
        priority,
        kernel_stack,
        trap_context: trap_ctx,
        context,
        fd_table,
        page_table,
        cwd: "/".to_string(),
        exit_code: None,
        stats: ProcessStats::default(),
    };

    Arc::new(Mutex::new(pcb))
}

/// Allocate a new PID (global counter)
fn allocate_pid() -> Pid {
    static PID_COUNTER: usize = 1;
    let pid = PID_COUNTER;
    PID_COUNTER += 1;
    pid
}

/// File operations trait (forward declaration)
pub trait FileOps {
    fn read(&self, buf: &mut [u8]) -> Result<usize, String>;
    fn write(&self, buf: &[u8]) -> Result<usize, String>;
    fn close(&self) -> Result<(), String>;
}

/// Initialize global PID allocator
pub fn init_pid_allocator() {
    // Implementation would initialize PID_COUNTER
}