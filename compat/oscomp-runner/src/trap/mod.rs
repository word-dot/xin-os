//! Trap handling for RISC-V user/kernel mode transitions.

mod context;

use core::arch::{asm, global_asm};

use crate::config::TRAMPOLINE;
use crate::syscall::syscall_dispatch;
use crate::task::{
    check_signals_of_current, check_fatal_signals, current_add_signal,
    deliver_signal,
    current_trap_cx, current_trap_cx_user_va, current_user_token,
    exit_current_and_run_next, suspend_current_and_run_next,
    SIGSEGV, SIGILL, SIGFPE, SIGBUS,
};
use crate::timer::{check_timer, set_next_trigger};

use riscv::register::{scause, sie, sscratch, sstatus, stval, stvec};

global_asm!(include_str!("trap.S"));

/// Map trampoline code into the kernel page table at the TRAMPOLINE address.
///
/// The trap.S code (__alltraps, __restore) is linked at a kernel virtual
/// address but MUST also be accessible at TRAMPOLINE (0x3FFFFFFFFFF000)
/// so that `stvec = TRAMPOLINE` works when the CPU traps from user mode.
///
/// copy_mappings_from() copies the kernel's L2 index 511 PTE to user page
/// tables, but the trampoline code at VPN[1]=511 is NOT mapped even in the
/// kernel page table (kernel lives at VPN[1]=0x1FE-0x1FF). So we must
/// explicitly add a page table entry for TRAMPOLINE.
pub fn map_trampoline() {
    unsafe extern "C" {
        fn __alltraps();
    }
    let trampoline_va = __alltraps as usize;
    let trampoline_page = trampoline_va & !0xfff;
    // Compute physical address: kernel phys = virt - 0xFFFFFFC000000000
    let trampoline_pa = trampoline_page - 0xFFFFFFC000000000;
    axlog::info!(
        "Mapping trampoline: VA={:#x} PA={:#x} -> TRAMPOLINE={:#x}",
        trampoline_page, trampoline_pa, TRAMPOLINE,
    );

    // Map the trampoline code's physical page at TRAMPOLINE in the kernel
    // address space. TRAMPOLINE (0x3FFFFFFFFFF000) is outside the normal
    // Sv39 address space range (top bit must be 1 for kernel), so we use
    // map_linear_override which skips the range check.
    let kernel_aspace = axmm::kernel_aspace();
    let mut kas = kernel_aspace.lock();
    unsafe {
        if kas.map_linear_override(
            memory_addr::VirtAddr::from_usize(TRAMPOLINE),
            memory_addr::PhysAddr::from_usize(trampoline_pa),
            4096,
            axhal::paging::MappingFlags::READ | axhal::paging::MappingFlags::EXECUTE,
        ).is_err() {
            axlog::warn!("Failed to map trampoline at TRAMPOLINE in kernel page table");
        } else {
            axlog::info!("Trampoline mapped at TRAMPOLINE in kernel page table");
        }
    }
    drop(kas);
    unsafe { core::arch::asm!("sfence.vma"); }
}

/// Map trampoline code into a user address space at the TRAMPOLINE address.
///
/// TRAMPOLINE (0x3FFFFFFFFFF000) is outside the user address space range
/// [0, 0x4000000000). We use map_linear_override which bypasses the range
/// check and directly maps into the page table.
pub fn map_trampoline_to_user(aspace: &mut axmm::AddrSpace) {
    unsafe extern "C" {
        fn __alltraps();
    }
    let trampoline_va = __alltraps as usize;
    let trampoline_page = trampoline_va & !0xfff;
    let trampoline_pa = trampoline_page - 0xFFFFFFC000000000;
    axlog::info!(
        "Mapping trampoline to user: VA={:#x} PA={:#x} -> TRAMPOLINE={:#x}",
        trampoline_page, trampoline_pa, TRAMPOLINE,
    );

    // Clear any stale entry left by copy_mappings_from
    let _ = aspace.unmap(memory_addr::VirtAddr::from_usize(TRAMPOLINE), 4096);
    
    // Use map_linear_override to bypass the address range check
    unsafe {
        let result = aspace.map_linear_override(
            memory_addr::VirtAddr::from_usize(TRAMPOLINE),
            memory_addr::PhysAddr::from_usize(trampoline_pa),
            4096,
            axhal::paging::MappingFlags::READ 
                | axhal::paging::MappingFlags::EXECUTE 
                | axhal::paging::MappingFlags::USER,
        );
        if result.is_ok() {
            axlog::info!("Trampoline mapped at TRAMPOLINE in user page table");
        } else {
            axlog::warn!("Failed to map trampoline at TRAMPOLINE in user page table: {:?}", result);
        }
    }
}

/// Initialize trap handling.
pub fn init() {
    set_kernel_trap_entry();
}

fn set_kernel_trap_entry() {
    unsafe extern "C" {
        unsafe fn __alltraps();
        unsafe fn __alltraps_k();
    }
    let __alltraps_k_va = __alltraps_k as usize - __alltraps as usize + TRAMPOLINE;
    unsafe {
        core::arch::asm!("csrw stvec, {}", in(reg) __alltraps_k_va);
        sscratch::write(trap_from_kernel as usize);
    }
}

fn set_user_trap_entry() {
    unsafe {
        core::arch::asm!("csrw stvec, {}", in(reg) TRAMPOLINE);
    }
}

pub fn enable_timer_interrupt() {
    unsafe { sie::set_stimer(); }
}

fn enable_supervisor_interrupt() {
    unsafe { sstatus::set_sie(); }
}

fn disable_supervisor_interrupt() {
    unsafe { sstatus::clear_sie(); }
}

// RISC-V scause exception codes
const EXC_USER_ECALL: usize = 8;
const EXC_INST_FETCH_FAULT: usize = 12;
const EXC_LOAD_FAULT: usize = 13;
const EXC_STORE_FAULT: usize = 15;
const EXC_INST_PAGE_FAULT: usize = 1;
const EXC_LOAD_PAGE_FAULT: usize = 5;
const EXC_STORE_PAGE_FAULT: usize = 7;
const EXC_ILLEGAL_INST: usize = 2;
const EXC_BREAKPOINT: usize = 3;

// RISC-V scause interrupt codes
const IRQ_S_TIMER: usize = 5;
const IRQ_S_EXTERNAL: usize = 9;

/// Main trap handler, called from assembly.
#[unsafe(no_mangle)]
pub fn trap_handler() -> ! {
    set_kernel_trap_entry();
    let scause_val: usize;
    unsafe { core::arch::asm!("csrr {}, scause", out(reg) scause_val); }
    let stval_val = stval::read();

    let is_interrupt = (scause_val >> 63) & 1 == 1;
    let code = scause_val & !(1usize << 63);

    if !is_interrupt {
        let mut writeback_needed = false;
        match code {
            EXC_USER_ECALL => {
                let mut cx = current_trap_cx();
                cx.sepc += 4;
                enable_supervisor_interrupt();
                let sys_num = cx.x[17];
                let args = [cx.x[10], cx.x[11], cx.x[12],
                            cx.x[13], cx.x[14], cx.x[15]];

                // Check if this is sigreturn before dispatching
                let is_sigreturn = sys_num == crate::syscall::SYS_SIGRETURN
                    || sys_num == crate::syscall::SYS_RT_SIGRETURN;

                let result = syscall_dispatch(sys_num, args);

                if is_sigreturn {
                    // For sigreturn, do_sigreturn_syscall() wrote the restored
                    // trap context directly to the user page, so re-read it.
                    cx = current_trap_cx();
                } else {
                    // Keep the existing cx (with sepc already += 4) and just
                    // set the return value. Do NOT call current_trap_cx()
                    // again, as that would re-read from user space and
                    // overwrite the sepc += 4 we did above!
                    cx.x[10] = result as usize;
                }
                writeback_needed = true;
            }
            EXC_STORE_FAULT | EXC_STORE_PAGE_FAULT
            | EXC_INST_FETCH_FAULT | EXC_INST_PAGE_FAULT
            | EXC_LOAD_FAULT | EXC_LOAD_PAGE_FAULT => {
                axlog::warn!(
                    "Page fault: code={}, addr={:#x}, pc={:#x}",
                    code, stval_val, current_trap_cx().sepc,
                );
                current_add_signal(SIGSEGV);
            }
            EXC_ILLEGAL_INST => {
                axlog::warn!("Illegal instruction at pc={:#x}", current_trap_cx().sepc);
                current_add_signal(SIGILL);
            }
            EXC_BREAKPOINT => {
                current_trap_cx().sepc += 4;
                writeback_needed = true;
            }
            _ => {
                panic!("Unsupported exception: code={}, stval={:#x}!", code, stval_val);
            }
        }

        // Write modified trap context back to user space.
        // __restore reads from the user's trap context page, so modifications
        // to the static TRAP_CX_BUF (sepc += 4, x[10] = result, etc.) must
        // be written back or they will be lost, causing infinite ecall loops.
        if writeback_needed {
            crate::task::write_current_trap_cx();
        }
    } else {
        match code {
            IRQ_S_TIMER => {
                set_next_trigger();
                check_timer();
                suspend_current_and_run_next();
            }
            IRQ_S_EXTERNAL => {
                crate::board::irq_handler();
            }
            _ => {
                panic!("Unsupported interrupt: code={}, stval={:#x}!", code, stval_val);
            }
        }
    }

    // Check for fatal signals (default action = terminate, no handler)
    if let Some((signum, exit_code)) = check_fatal_signals() {
        axlog::warn!("[kernel] Fatal signal {} for pid {}", signum, crate::task::current_pid());
        exit_current_and_run_next(exit_code);
    }

    // Try to deliver any pending signals with user handlers
    {
        let mut cx = current_trap_cx();
        if deliver_signal(&mut cx) {
            crate::task::write_current_trap_cx();
        }
    }

    trap_return();
}

/// Return from trap to user mode.
///
/// Before jumping to the __restore trampoline, we must save the current
/// kernel stack pointer into the trap context (offset 35*8 = kernel_sp).
/// When the next trap occurs, __alltraps loads this value as the kernel
/// stack pointer (sp) before jumping to trap_handler.
#[unsafe(no_mangle)]
pub fn trap_return() -> ! {
    disable_supervisor_interrupt();
    set_user_trap_entry();

    // Save current kernel stack pointer into TRAP_CX_BUF (kernel static),
    // then flush the buffer to the user's trap context page.
    let current_sp: usize;
    unsafe { asm!("mv {}, sp", out(reg) current_sp); }
    {
        let cx = crate::task::current_trap_cx();
        cx.kernel_sp = current_sp;
    }
    crate::task::write_current_trap_cx();

    let trap_cx_user_va = current_trap_cx_user_va();
    let user_satp = current_user_token();
    unsafe extern "C" {
        unsafe fn __alltraps();
        unsafe fn __restore();
    }
    let restore_va = __restore as usize - __alltraps as usize + TRAMPOLINE;
    unsafe {
        asm!(
            "fence.i",
            "jr {restore_va}",
            restore_va = in(reg) restore_va,
            in("a0") trap_cx_user_va,
            in("a1") user_satp,
            options(noreturn)
        );
    }
}

/// Handle traps from kernel mode.
#[unsafe(no_mangle)]
pub fn trap_from_kernel(_trap_cx: &TrapContext) {
    let scause_val: usize;
    unsafe { core::arch::asm!("csrr {}, scause", out(reg) scause_val); }
    let stval_val = stval::read();
    let is_interrupt = (scause_val >> 63) & 1 == 1;
    let code = scause_val & !(1usize << 63);

    if is_interrupt {
        match code {
            IRQ_S_EXTERNAL => { crate::board::irq_handler(); }
            IRQ_S_TIMER => { set_next_trigger(); check_timer(); }
            _ => { panic!("Unsupported IRQ from kernel: code={}", code); }
        }
    } else {
        panic!("Unsupported exception from kernel: code={}, stval={:#x}!", code, stval_val);
    }
}

pub use context::TrapContext;