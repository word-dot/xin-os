
//! Configuration constants for the oscomp-runner.
//!
//! These constants define the memory layout and system parameters
//! used by the trap handler, process manager, and other modules.

/// Trampoline virtual address.
///
/// This is the virtual address where the trap entry/exit code is mapped.
/// It must be the same in both kernel and user page tables so that
/// trap handling works during the address space switch.
///
/// On RISC-V, we use the address from the linker script's `strampoline`.
/// For simplicity, we use a fixed address that matches the typical
/// kernel layout.
pub const TRAMPOLINE: usize = 0x3FFF_FFFF_F000;

/// Kernel stack size for each task (64KB).
pub const KERNEL_STACK_SIZE: usize = 0x10000;

/// User stack size for each process (8MB).
pub const USER_STACK_SIZE: usize = 0x800_000;

/// Timer interrupt frequency (ticks per second).
pub const TICKS_PER_SEC: usize = 100;

/// Maximum number of processes.
pub const MAX_PROCESS_NUM: usize = 256;

/// Maximum path length for file operations.
pub const MAX_PATH_LEN: usize = 256;
