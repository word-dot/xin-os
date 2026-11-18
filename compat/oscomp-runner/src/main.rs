#![no_std]
#![no_main]
#![feature(linkage)]

#[macro_use]
extern crate axstd;

// ============================================================
// Module declarations
// ============================================================

mod config;
mod ext4_vfs;
mod myfs;

// RISC-V only modules (trap handling, board, etc.)
#[cfg(target_arch = "riscv64")]
mod board;
#[cfg(target_arch = "riscv64")]
mod trap;
#[cfg(target_arch = "riscv64")]
mod timer;
#[cfg(target_arch = "riscv64")]
mod sbi;

// Stub trap module for non-RISC-V architectures
#[cfg(not(target_arch = "riscv64"))]
#[path = "trap_stub.rs"]
mod trap;

// Multi-process support modules
mod process;
mod elf_loader;
mod syscall;
mod task;
mod net;

// Real test runner that loads ELF from ext4 and executes them
mod test_runner;

// ELF test runner - loads and executes individual ELF binaries
mod test_elf_runner;

// ============================================================
// Task extension: bind PID to each kernel task
// ============================================================

use axtask::def_task_ext;
use crate::process::Pid;

/// Task extension data that associates each kernel task with a process.
pub struct TaskExtImpl {
    /// The PID of the process this task belongs to.
    pub pid: Pid,
    /// Whether this task runs in user mode.
    pub is_user: bool,
}

def_task_ext!(TaskExtImpl);

// ============================================================
// AxNamespace interface implementation
// ============================================================

struct AxNamespaceImpl;

#[crate_interface::impl_interface]
impl axns::AxNamespaceIf for AxNamespaceImpl {
    fn current_namespace_base() -> *mut u8 {
        // Use global namespace for now
        axns::AxNamespace::global().base()
    }
}

// ============================================================
// Shutdown helpers
// ============================================================

// RISC-V shutdown
#[cfg(target_arch = "riscv64")]
use sbi_rt::{system_reset, Shutdown, NoReason};

// LoongArch shutdown
#[cfg(target_arch = "loongarch64")]
use axhal::power::system_off;

// ============================================================
// Main entry point
// ============================================================

#[unsafe(no_mangle)]
fn main() {
    println!("==== oscomp-runner start ====");

    // Initialize the process manager
    crate::process::process_manager();

    // Initialize trap handling (RISC-V only for now)
    #[cfg(target_arch = "riscv64")]
    {
        println!("Initializing trap handling...");
        // Map trampoline code into kernel page table at TRAMPOLINE address
        crate::trap::map_trampoline();
        // Set up the trap entry point (stvec) so traps from user mode are handled
        crate::trap::init();
        println!("Trap handling initialized.");
    }

    // Enable timer before running tests
    #[cfg(target_arch = "riscv64")]
    {
        crate::trap::enable_timer_interrupt();
        crate::timer::set_next_trigger();
    }

    // Run all test suites automatically
    println!("==================================================");
    println!("Starting automatic test suite execution...");
    println!("==================================================");
    
    println!("Filesystem loaded successfully.");
    
    // Wait for lwext4 to initialize (it's running in a background thread)
    println!("Waiting for lwext4_rust initialization...");
    for i in 0..200 {
        if crate::myfs::get_lwext4().is_some() {
            println!("? lwext4_rust initialized after {} iterations!", i);
            break;
        }
        axtask::yield_now();
        if i % 20 == 0 && i > 0 {
            println!("  Still waiting for lwext4_rust... ({})", i);
        }
    }
    
    if crate::myfs::get_lwext4().is_some() {
        println!("? lwext4_rust is ready! Using FAST ext4 file access via native C lwext4.");
    } else {
        println!("? lwext4_rust not ready, will use SLOW ext4_view fallback.");
    }
    
    // Run all test suites
    crate::test_runner::run_all_tests();
    
    println!("==================================================");
    println!("All test suites completed!");
    println!("==================================================");

    // RISC-V shutdown
    #[cfg(target_arch = "riscv64")]
    system_reset(Shutdown, NoReason);

    // LoongArch: call system_off for fast shutdown
    #[cfg(target_arch = "loongarch64")]
    system_off();

    println!("==== oscomp-runner end ====");
}
