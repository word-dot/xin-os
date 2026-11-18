
//! Task management helpers for multi-process support.
//!
//! This module provides functions for managing the relationship between
//! ArceOS tasks (threads) and processes, including signal handling,
//! trap context access, and process lifecycle management.

extern crate alloc;

use alloc::sync::Arc;
use axerrno::AxResult;
use memory_addr::VirtAddr;

use axtask::TaskExtRef;
use crate::process::{self, Pid, ProcessControlBlock, ProcessState, layout};
use crate::trap::TrapContext;

// ============================================================
// Signal constants (Linux signal numbers for RISC-V)
// ============================================================

/// Signal numbers matching Linux RISC-V ABI.
pub const SIGHUP:    usize = 1;
pub const SIGINT:    usize = 2;
pub const SIGQUIT:   usize = 3;
pub const SIGILL:    usize = 4;
pub const SIGTRAP:   usize = 5;
pub const SIGABRT:   usize = 6;
pub const SIGBUS:    usize = 7;
pub const SIGFPE:    usize = 8;
pub const SIGKILL:   usize = 9;
pub const SIGUSR1:   usize = 10;
pub const SIGSEGV:   usize = 11;
pub const SIGUSR2:   usize = 12;
pub const SIGPIPE:   usize = 13;
pub const SIGALRM:   usize = 14;
pub const SIGTERM:   usize = 15;
pub const SIGSTKFLT: usize = 16;
pub const SIGCHLD:   usize = 17;
pub const SIGCONT:   usize = 18;
pub const SIGSTOP:   usize = 19;
pub const SIGTSTP:   usize = 20;
pub const SIGTTIN:   usize = 21;
pub const SIGTTOU:   usize = 22;
pub const SIGURG:    usize = 23;
pub const SIGXCPU:   usize = 24;
pub const SIGXFSZ:   usize = 25;
pub const SIGVTALRM: usize = 26;
pub const SIGPROF:   usize = 27;
pub const SIGWINCH:  usize = 28;
pub const SIGIO:     usize = 29;
pub const SIGPWR:    usize = 30;
pub const SIGSYS:    usize = 31;

/// SIG_DFL: default signal handler.
pub const SIG_DFL: usize = 0;
/// SIG_IGN: ignore signal.
pub const SIG_IGN: usize = 1;

/// Default actions for signals.
/// Returns true if the signal causes process termination.
pub fn signal_default_terminates(signum: usize) -> bool {
    matches!(signum,
        SIGHUP | SIGINT | SIGQUIT | SIGILL | SIGTRAP | SIGABRT |
        SIGBUS | SIGFPE | SIGKILL | SIGUSR1 | SIGSEGV | SIGUSR2 |
        SIGPIPE | SIGALRM | SIGTERM | SIGSTKFLT | SIGXCPU | SIGXFSZ |
        SIGVTALRM | SIGPROF | SIGIO | SIGPWR | SIGSYS
    )
}

/// Signals that cannot be caught, blocked, or ignored.
pub fn signal_is_uncatchable(signum: usize) -> bool {
    signum == SIGKILL || signum == SIGSTOP
}

/// Convert a signal number to a bit in the signal bitmask.
pub fn signal_to_bit(signum: usize) -> u64 {
    if signum == 0 || signum > 64 { 0 } else { 1u64 << (signum - 1) }
}

// ============================================================
// Current process helpers
// ============================================================

/// Get the PID of the current task.
pub fn current_pid() -> Pid {
    let task = axtask::current();
    axtask::TaskExtRef::<crate::TaskExtImpl>::task_ext(&*task).pid
}

/// Get the PCB of the current process.
pub fn current_pcb() -> Arc<axstd::sync::Mutex<ProcessControlBlock>> {
    let pid = current_pid();
    process::process_manager().lock().get(pid)
        .expect("Current process not found in process manager")
}

/// Static buffer for trap context (safe in single-core kernel context).
#[allow(static_mut_refs)]
static mut TRAP_CX_BUF: [u8; core::mem::size_of::<TrapContext>()] =
    [0u8; core::mem::size_of::<TrapContext>()];

/// Get the trap context of the current process (from user space).
///
/// This reads the trap context from the user's address space at the
/// virtual address stored in the PCB, into the global static buffer.
/// Returns a mutable reference to the static buffer.
///
/// After modifying the returned context, call `write_current_trap_cx()`
/// to persist changes back to the user space trap context page.
#[allow(static_mut_refs)]
pub fn current_trap_cx() -> &'static mut TrapContext {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let aspace = pcb_guard.aspace.lock();

    aspace.read(pcb_guard.trap_ctx_vaddr, unsafe { &mut TRAP_CX_BUF }).unwrap();

    unsafe {
        &mut *(TRAP_CX_BUF.as_mut_ptr() as *mut TrapContext)
    }
}

/// Write the current trap context buffer back to the user space trap context page.
///
/// Must be called after modifying the context returned by `current_trap_cx()`
/// so that `trap_return()` -> `__restore` picks up the changes.
#[allow(static_mut_refs)]
pub fn write_current_trap_cx() {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let aspace = pcb_guard.aspace.lock();
    let _ = aspace.write(pcb_guard.trap_ctx_vaddr, unsafe { &TRAP_CX_BUF });
}

/// Get the user-space virtual address of the current trap context.
pub fn current_trap_cx_user_va() -> usize {
    let pcb = current_pcb();
    pcb.lock().trap_ctx_vaddr.as_usize()
}

/// Get the current user page table token (satp value).
pub fn current_user_token() -> usize {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();

    // Construct satp value: mode=8 (Sv39), ASID=0, PPN=page_table_root
    let root = pcb_guard.page_table_root.lock();
    let ppn = (*root as usize) >> 12;
    (8usize << 60) | ppn
}

// ============================================================
// Signal handling
// ============================================================

/// Add a signal to the current process's pending set.
pub fn current_add_signal(signum: usize) {
    if signum == 0 || signum > 64 { return; }
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let mut pending = pcb_guard.pending_signals.lock();
    pending.add(signum as i32);
    axlog::debug!("Signal {} delivered to pid {}, pending={:#x}", signum, current_pid(), pending.bits());
}

/// Send a signal to a specific process.
pub fn send_signal_to(pid: Pid, signum: usize) -> bool {
    if signum == 0 || signum > 64 { return false; }
    let pm = process::process_manager().lock();
    if let Some(pcb) = pm.get(pid) {
        let pcb_guard = pcb.lock();
        let mut pending = pcb_guard.pending_signals.lock();
        pending.add(signum as i32);
        axlog::debug!("Signal {} sent to pid {}, pending={:#x}", signum, pid, pending.bits());
        true
    } else {
        false
    }
}

/// Check if the current process has any deliverable (pending & unblocked) signals.
///
/// Returns `Some(signum)` if there is a signal to deliver,
/// `None` if no signals are pending or all are blocked.
pub fn check_signals_of_current() -> Option<usize> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let pending = pcb_guard.pending_signals.lock();
    let mask = pcb_guard.signal_mask.lock();

    // Signals that are pending and not blocked
    let deliverable = pending.difference(&mask);

    if deliverable.is_empty() {
        return None;
    }

    // Find the lowest-numbered deliverable signal
    let sig = deliverable.first();
    if sig > 0 {
        Some(sig as usize)
    } else {
        None
    }
}

/// Check if the current process has fatal pending signals that should cause exit.
/// This is used for signals with default action = terminate and no handler registered.
///
/// Returns `Some((signum, exit_code))` if the process should be killed.
pub fn check_fatal_signals() -> Option<(usize, i32)> {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let pending = pcb_guard.pending_signals.lock();
    let mask = pcb_guard.signal_mask.lock();
    let actions = pcb_guard.signal_actions.lock();

    let deliverable = pending.difference(&mask);

    for signum in 1..=31 {
        if deliverable.contains(signum as i32) {
            let action = &actions[signum - 1];
            if action.handler == SIG_DFL {
                // Default action
                if signal_default_terminates(signum) {
                    return Some((signum, 128 + signum as i32));
                }
            }
            // If handler is SIG_IGN or user handler, it won't cause exit here
        }
    }
    None
}

/// Deliver a signal to the current process by modifying the trap context
/// to jump to the signal handler.
///
/// This sets up the user stack with the signal frame and modifies the
/// trap context so that on return to user mode, execution goes to the
/// signal handler.
///
/// Returns `true` if a signal was delivered, `false` otherwise.
pub fn deliver_signal(trap_cx: &mut TrapContext) -> bool {
    let pcb = current_pcb();
    let signum = {
        let pcb_guard = pcb.lock();
        let pending = pcb_guard.pending_signals.lock();
        let mask = pcb_guard.signal_mask.lock();
        let deliverable = pending.difference(&mask);

        if deliverable.is_empty() {
            return false;
        }

        // Find the lowest-numbered deliverable signal
        let found = deliverable.first() as usize;
        if found == 0 {
            return false;
        }

        // Check if this signal has a user handler
        let actions = pcb_guard.signal_actions.lock();
        let action = actions[found - 1];

        if action.handler == SIG_DFL {
            // No user handler - let default action handle it
            // (handled by check_fatal_signals)
            return false;
        }

        if action.handler == SIG_IGN {
            // Ignore: just clear the pending bit
            drop(actions);
            drop(mask);
            drop(pending);
            drop(pcb_guard);
            let pcb_guard2 = pcb.lock();
            let mut pending2 = pcb_guard2.pending_signals.lock();
            pending2.remove(found as i32);
            return false;
        }

        found
    };

    // Now we have a signal with a user handler
    let pcb_guard = pcb.lock();

    // Clear the pending bit
    {
        let mut pending = pcb_guard.pending_signals.lock();
        pending.remove(signum as i32);
    }

    // Get the signal action
    let action = {
        let actions = pcb_guard.signal_actions.lock();
        actions[signum - 1]
    };

    // Save the original signal mask BEFORE modifying it
    let orig_mask = pcb_guard.signal_mask.lock().clone();

    // Block the signal during handler execution (and the action's mask)
    {
        let mut mask = pcb_guard.signal_mask.lock();
        mask.add(signum as i32);
        // Also add the action's mask
        let action_mask_bits = action.mask.bits();
        let combined = mask.bits() | action_mask_bits;
        mask.set_bits(combined);
        // Unless SA_NODEFER is set, block the signal
        if action.flags & 0x40000000 != 0 { // SA_NODEFER
            mask.remove(signum as i32);
        }
    }

    // Set up the signal frame on the user stack
    // We push a sigframe structure onto the user stack that contains:
    // - The original trap context (so we can restore it on sigreturn)
    // - The signal number
    // - The original signal mask (before blocking)
    // - A return address pointing to the sigreturn trampoline

    let user_sp = trap_cx.x[2]; // current user stack pointer

    // Determine which stack to use
    let sig_stack_info = pcb_guard.sig_stack.lock();
    let use_sig_stack = sig_stack_info.flags & 1 == 0 // SS_ONSTACK is not set
        && sig_stack_info.flags & 2 == 0 // not SS_DISABLE
        && sig_stack_info.base != 0
        && action.flags & 0x08000000 != 0; // SA_ONSTACK flag is set

    let stack_top = if use_sig_stack {
        // Mark sigaltstack as SS_ONSTACK
        let mut sig_stack = pcb_guard.sig_stack.lock();
        sig_stack.flags |= 1; // SS_ONSTACK
        drop(sig_stack);
        sig_stack_info.base + sig_stack_info.size
    } else {
        user_sp
    };
    drop(sig_stack_info);

    // Align stack to 16 bytes
    let stack_top = stack_top & !0xf;

    // Signal frame layout on the user stack:
    //   [sigframe]
    //   - trap_context (original registers)
    //   - signum (8 bytes)
    //   - sigmask (8 bytes, original signal mask)
    //   - return address (8 bytes, points to sigreturn trampoline)
    //
    // The signal handler is called as: handler(signum, 0, 0)
    // On return, it jumps to the restorer or sigreturn trampoline.

    let sigframe_size = core::mem::size_of::<TrapContext>() + 3 * 8; // trap_cx + signum + sigmask + retaddr
    let sigframe_top = stack_top - sigframe_size;
    let sigframe_top = sigframe_top & !0xf; // align

    // Save the original trap context to the signal frame on the user stack
    let aspace = pcb_guard.aspace.lock();
    let sigframe_vaddr = VirtAddr::from_usize(sigframe_top);

    // Write original trap context
    let trap_cx_bytes = unsafe {
        core::slice::from_raw_parts(
            trap_cx as *const TrapContext as *const u8,
            core::mem::size_of::<TrapContext>(),
        )
    };
    if aspace.write(sigframe_vaddr, trap_cx_bytes).is_err() {
        axlog::warn!("Failed to write signal frame to user stack");
        return false;
    }

    // Write signum after trap context
    let signum_offset = core::mem::size_of::<TrapContext>();
    let signum_bytes = (signum as u64).to_le_bytes();
    if aspace.write(VirtAddr::from_usize(sigframe_top + signum_offset), &signum_bytes).is_err() {
        return false;
    }

    // Write ORIGINAL signal mask (before blocking)
    let mask_bytes = orig_mask.bits().to_le_bytes();
    let mask_offset = signum_offset + 8;
    if aspace.write(VirtAddr::from_usize(sigframe_top + mask_offset), &mask_bytes).is_err() {
        return false;
    }

    // Write return address (sigreturn trampoline)
    let sigreturn_addr = layout::sigtrampoline().as_usize();
    let retaddr_bytes = (sigreturn_addr as u64).to_le_bytes();
    let retaddr_offset = mask_offset + 8;
    if aspace.write(VirtAddr::from_usize(sigframe_top + retaddr_offset), &retaddr_bytes).is_err() {
        return false;
    }

    drop(aspace);
    drop(pcb_guard);

    // Modify the trap context to jump to the signal handler
    trap_cx.x[2] = sigframe_top; // sp = signal frame top
    trap_cx.x[10] = signum;      // a0 = signal number
    trap_cx.x[11] = 0;           // a1 = siginfo (null for now)
    trap_cx.x[12] = 0;           // a2 = ucontext (null for now)
    trap_cx.sepc = action.handler; // pc = signal handler

    // Set ra to the restorer or sigreturn trampoline
    trap_cx.x[1] = if action.restorer != 0 {
        action.restorer
    } else {
        sigreturn_addr
    };

    axlog::debug!(
        "Signal {} delivered to pid {}, handler={:#x}, sp={:#x}",
        signum, current_pid(), action.handler, sigframe_top
    );

    true
}

/// Handle sigreturn from trap handler: restore the original trap context from the signal frame.
///
/// This is called from the trap handler after the sigreturn syscall modifies
/// the trap context. It reads the signal frame from the user stack and restores
/// the original context.
pub fn do_sigreturn(trap_cx: &mut TrapContext) {
    // The signal frame is at the current user sp
    let sigframe_top = trap_cx.x[2];

    let pcb = current_pcb();
    let pcb_guard = pcb.lock();
    let aspace = pcb_guard.aspace.lock();

    let sigframe_vaddr = VirtAddr::from_usize(sigframe_top);

    // Read the original trap context from the signal frame
    let mut trap_cx_bytes = alloc::vec![0u8; core::mem::size_of::<TrapContext>()];
    if aspace.read(sigframe_vaddr, &mut trap_cx_bytes).is_err() {
        axlog::warn!("Failed to read signal frame for sigreturn");
        return;
    }

    // Read the original signal mask (8 bytes)
    let mask_offset = core::mem::size_of::<TrapContext>() + 8; // after trap_cx + signum
    let mut mask_bytes = [0u8; 8];
    if aspace.read(VirtAddr::from_usize(sigframe_top + mask_offset), &mut mask_bytes).is_ok() {
        let orig_mask_bits = u64::from_le_bytes(mask_bytes);
        pcb_guard.signal_mask.lock().set_bits(orig_mask_bits);
    }

    // Clear SS_ONSTACK flag since we're returning from signal handler
    {
        let mut sig_stack = pcb_guard.sig_stack.lock();
        sig_stack.flags &= !1; // clear SS_ONSTACK
    }

    drop(aspace);
    drop(pcb_guard);

    // Restore the original trap context
    let saved_cx = unsafe {
        &*(trap_cx_bytes.as_ptr() as *const TrapContext)
    };

    // Restore all registers except kernel_satp, kernel_sp, trap_handler
    trap_cx.x = saved_cx.x;
    trap_cx.sstatus = saved_cx.sstatus;
    trap_cx.sepc = saved_cx.sepc;

    axlog::debug!("sigreturn: restored context for pid {}, pc={:#x}", current_pid(), trap_cx.sepc);
}

/// Handle sigreturn syscall: called from sys_sigreturn.
///
/// This reads the current trap context, finds the signal frame on the user stack,
/// and restores the original context. The trap context is also written back to
/// the trap context page so that trap_return picks it up.
pub fn do_sigreturn_syscall() {
    let pcb = current_pcb();
    let pcb_guard = pcb.lock();

    // Read the current trap context from the trap context page
    let mut trap_cx_buf = alloc::vec![0u8; core::mem::size_of::<TrapContext>()];
    let aspace = pcb_guard.aspace.lock();
    if aspace.read(pcb_guard.trap_ctx_vaddr, &mut trap_cx_buf).is_err() {
        axlog::warn!("Failed to read trap context for sigreturn");
        return;
    }

    let trap_cx = unsafe {
        &mut *(trap_cx_buf.as_mut_ptr() as *mut TrapContext)
    };

    // The signal frame is at the current user sp
    let sigframe_top = trap_cx.x[2];

    // Read the original trap context from the signal frame
    let mut saved_cx_buf = alloc::vec![0u8; core::mem::size_of::<TrapContext>()];
    if aspace.read(VirtAddr::from_usize(sigframe_top), &mut saved_cx_buf).is_err() {
        axlog::warn!("Failed to read signal frame for sigreturn");
        return;
    }

    // Read the original signal mask (8 bytes)
    let mask_offset = core::mem::size_of::<TrapContext>() + 8; // after trap_cx + signum
    let mut mask_bytes = [0u8; 8];
    if aspace.read(VirtAddr::from_usize(sigframe_top + mask_offset), &mut mask_bytes).is_ok() {
        let orig_mask_bits = u64::from_le_bytes(mask_bytes);
        pcb_guard.signal_mask.lock().set_bits(orig_mask_bits);
    }

    // Clear SS_ONSTACK flag since we're returning from signal handler
    {
        let mut sig_stack = pcb_guard.sig_stack.lock();
        sig_stack.flags &= !1; // clear SS_ONSTACK
    }

    // Restore the original trap context and write it back to the trap context page
    let saved_cx = unsafe {
        &*(saved_cx_buf.as_ptr() as *const TrapContext)
    };

    // Copy the restored context into trap_cx_buf
    trap_cx.x = saved_cx.x;
    trap_cx.sstatus = saved_cx.sstatus;
    trap_cx.sepc = saved_cx.sepc;

    // Write the restored trap context back to the trap context page
    let _ = aspace.write(pcb_guard.trap_ctx_vaddr, &trap_cx_buf);

    axlog::debug!("sigreturn_syscall: restored context for pid {}, pc={:#x}", current_pid(), trap_cx.sepc);
}

// ============================================================
// Process lifecycle
// ============================================================

/// Exit the current process and schedule the next one.
pub fn exit_current_and_run_next(exit_code: i32) -> ! {
    let pid = current_pid();
    axlog::info!("exit_current_and_run_next: pid={}, exit_code={}", pid, exit_code);

    // Mark the process as exited
    let pcb = current_pcb();
    {
        let mut pcb_guard = pcb.lock();
        pcb_guard.state = ProcessState::Exited;
        pcb_guard.exit_code = exit_code;
    }

    // Exit the kernel task
    axtask::exit(exit_code);
}

/// Suspend the current process and schedule the next one.
pub fn suspend_current_and_run_next() {
    axtask::yield_now();
}
