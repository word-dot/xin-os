
//! POSIX signal API implementation.

use core::ffi::{c_int, c_long};

use axerrno::{LinuxError, LinuxResult};
use axtask::{SigAction, SigSet, SignalAction, MAX_SIGNUM, SIG_DFL, SIG_IGN};

/// SA flags
const SA_NOCLDSTOP: u32 = 1;
const SA_NOCLDWAIT: u32 = 2;
const SA_SIGINFO: u32 = 4;
const SA_ONSTACK: u32 = 0x08000000;
const SA_RESTART: u32 = 0x10000000;
const SA_NODEFER: u32 = 0x40000000;
const SA_RESETHAND: u32 = 0x80000000;
const SA_RESTORER: u32 = 0x04000000;

/// sigprocmask how values
const SIG_BLOCK: c_int = 0;
const SIG_UNBLOCK: c_int = 1;
const SIG_SETMASK: c_int = 2;

/// Signal constants
const SIGHUP: c_int = 1;
const SIGINT: c_int = 2;
const SIGQUIT: c_int = 3;
const SIGILL: c_int = 4;
const SIGTRAP: c_int = 5;
const SIGABRT: c_int = 6;
const SIGBUS: c_int = 7;
const SIGFPE: c_int = 8;
const SIGKILL: c_int = 9;
const SIGUSR1: c_int = 10;
const SIGSEGV: c_int = 11;
const SIGUSR2: c_int = 12;
const SIGPIPE: c_int = 13;
const SIGALRM: c_int = 14;
const SIGTERM: c_int = 15;
const SIGSTKFLT: c_int = 16;
const SIGCHLD: c_int = 17;
const SIGCONT: c_int = 18;
const SIGSTOP: c_int = 19;
const SIGTSTP: c_int = 20;
const SIGTTIN: c_int = 21;
const SIGTTOU: c_int = 22;
const SIGURG: c_int = 23;
const SIGXCPU: c_int = 24;
const SIGXFSZ: c_int = 25;
const SIGVTALRM: c_int = 26;
const SIGPROF: c_int = 27;
const SIGWINCH: c_int = 28;
const SIGIO: c_int = 29;
const SIGPWR: c_int = 30;
const SIGSYS: c_int = 31;

/// C struct sigaction representation (matches the layout in signal.h)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CSigAction {
    /// Handler function pointer or SIG_DFL/SIG_IGN
    pub sa_handler: usize,
    /// Signal mask during handler execution
    pub sa_mask: CSigSet,
    /// Flags
    pub sa_flags: c_int,
    /// Restorer function pointer
    pub sa_restorer: usize,
}

/// C sigset_t representation (matches layout in signal.h)
/// On 64-bit systems: unsigned long __bits[128 / sizeof(long)] = [u64; 2]
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CSigSet {
    pub bits: [u64; 2],
}

impl CSigSet {
    pub fn new() -> Self {
        CSigSet { bits: [0; 2] }
    }

    pub fn is_empty(&self) -> bool {
        self.bits[0] == 0 && self.bits[1] == 0
    }
}

impl From<&SigSet> for CSigSet {
    fn from(set: &SigSet) -> Self {
        CSigSet {
            bits: [set.bits(), 0],
        }
    }
}

impl From<&CSigSet> for SigSet {
    fn from(set: &CSigSet) -> Self {
        SigSet::from_bits(set.bits[0])
    }
}

/// Validate signal number.
fn validate_signal(sig: c_int) -> LinuxResult<()> {
    if sig < 1 || sig > MAX_SIGNUM as c_int {
        return Err(LinuxError::EINVAL);
    }
    Ok(())
}

/// Get the current task's signal info, or initialize it if not present.
fn get_or_init_signal_info() -> alloc::sync::Arc<spin::RwLock<axtask::SignalInfo>> {
    if let Some(info) = axtask::current_signal_info() {
        info
    } else {
        axtask::init_current_signal_info()
    }
}

/// Register a signal handler (sigaction system call).
///
/// `sig` - the signal number
/// `act` - pointer to new sigaction structure (may be NULL)
/// `oact` - pointer to store old sigaction structure (may be NULL)
/// `sigsetsize` - size of sigset_t in the structure
pub unsafe fn sys_sigaction(
    sig: c_int,
    act: *const CSigAction,
    oact: *mut CSigAction,
    sigsetsize: usize,
) -> c_int {
    debug!(
        "sys_sigaction <= sig: {}, act: {:#x}, oact: {:#x}, sigsetsize: {}",
        sig, act as usize, oact as usize, sigsetsize
    );

    syscall_body!(sys_sigaction, {
        validate_signal(sig)?;

        // SIGKILL and SIGSTOP cannot be caught or ignored
        if sig == SIGKILL || sig == SIGSTOP {
            return Err(LinuxError::EINVAL);
        }

        // Validate sigsetsize (should be at least 8 bytes for 64 signals)
        if sigsetsize < 8 {
            return Err(LinuxError::EINVAL);
        }

        let info = get_or_init_signal_info();
        let mut info_guard = info.write();

        // Store old action if requested
        if !oact.is_null() {
            let old_action = info_guard.get_action(sig);
            let old_c_action = match old_action {
                Some(a) => CSigAction {
                    sa_handler: a.sa_handler,
                    sa_mask: CSigSet::from(&a.sa_mask),
                    sa_flags: a.sa_flags as c_int,
                    sa_restorer: a.sa_restorer,
                },
                None => CSigAction {
                    sa_handler: SIG_DFL,
                    sa_mask: CSigSet::new(),
                    sa_flags: 0,
                    sa_restorer: 0,
                },
            };
            unsafe { core::ptr::write(oact, old_c_action) };
        }

        // Set new action if provided
        if !act.is_null() {
            let new_c_action = unsafe { core::ptr::read(act) };
            let new_action = SigAction {
                sa_handler: new_c_action.sa_handler,
                sa_mask: SigSet::from(&new_c_action.sa_mask),
                sa_flags: new_c_action.sa_flags as u32,
                sa_restorer: new_c_action.sa_restorer,
            };

            if info_guard.set_action(sig, new_action).is_none() {
                return Err(LinuxError::EINVAL);
            }
        }

        Ok(0)
    })
}

/// Send a signal to a process (kill system call).
///
/// `pid` - process ID (tid in unikernel context)
/// `sig` - signal number
pub fn sys_kill(pid: c_int, sig: c_int) -> c_int {
    debug!("sys_kill <= pid: {}, sig: {}", pid, sig);

    syscall_body!(sys_kill, {
        // Signal 0 is for permission checking only
        if sig == 0 {
            // Check if the target process exists
            if pid > 0 {
                let tid = pid as u64;
                if axtask::get_signal_info(tid).is_none() {
                    return Err(LinuxError::ESRCH);
                }
            }
            return Ok(0);
        }

        validate_signal(sig)?;

        if pid > 0 {
            // Send signal to specific process
            let tid = pid as u64;
            match axtask::send_signal_to_task(tid, sig) {
                Ok(()) => Ok(0),
                Err("Task not found") => Err(LinuxError::ESRCH),
                Err("Invalid signal number") => Err(LinuxError::EINVAL),
                Err(_) => Err(LinuxError::EPERM),
            }
        } else if pid == 0 {
            // Send signal to all processes in the same process group
            // In unikernel, send to self
            let info = get_or_init_signal_info();
            info.write().send_signal(sig).map_err(|_| LinuxError::EINVAL)?;
            Ok(0)
        } else if pid == -1 {
            // Send signal to all processes the sender has permission to
            // In unikernel, send to self
            let info = get_or_init_signal_info();
            info.write().send_signal(sig).map_err(|_| LinuxError::EINVAL)?;
            Ok(0)
        } else {
            // Send signal to process group -pid
            // In unikernel, send to self
            let info = get_or_init_signal_info();
            info.write().send_signal(sig).map_err(|_| LinuxError::EINVAL)?;
            Ok(0)
        }
    })
}

/// Send a signal to a specific thread (tkill system call).
///
/// `tid` - thread ID
/// `sig` - signal number
pub fn sys_tkill(tid: c_int, sig: c_int) -> c_int {
    debug!("sys_tkill <= tid: {}, sig: {}", tid, sig);

    syscall_body!(sys_tkill, {
        if sig == 0 {
            if axtask::get_signal_info(tid as u64).is_none() {
                return Err(LinuxError::ESRCH);
            }
            return Ok(0);
        }

        validate_signal(sig)?;

        // Cannot send SIGKILL or SIGSTOP via tkill? Actually you can.
        match axtask::send_signal_to_task(tid as u64, sig) {
            Ok(()) => Ok(0),
            Err("Task not found") => Err(LinuxError::ESRCH),
            Err("Invalid signal number") => Err(LinuxError::EINVAL),
            Err(_) => Err(LinuxError::EPERM),
        }
    })
}

/// Send a signal to a specific thread (tgkill system call).
///
/// `tgid` - thread group ID (ignored in unikernel)
/// `tid` - thread ID
/// `sig` - signal number
pub fn sys_tgkill(tgid: c_int, tid: c_int, sig: c_int) -> c_int {
    debug!("sys_tgkill <= tgid: {}, tid: {}, sig: {}", tgid, tid, sig);

    syscall_body!(sys_tgkill, {
        if sig == 0 {
            if axtask::get_signal_info(tid as u64).is_none() {
                return Err(LinuxError::ESRCH);
            }
            return Ok(0);
        }

        validate_signal(sig)?;

        match axtask::send_signal_to_task(tid as u64, sig) {
            Ok(()) => Ok(0),
            Err("Task not found") => Err(LinuxError::ESRCH),
            Err("Invalid signal number") => Err(LinuxError::EINVAL),
            Err(_) => Err(LinuxError::EPERM),
        }
    })
}

/// Examine and change blocked signals (sigprocmask system call).
///
/// `how` - SIG_BLOCK, SIG_UNBLOCK, or SIG_SETMASK
/// `set` - pointer to new signal set (may be NULL)
/// `oldset` - pointer to store old signal set (may be NULL)
/// `sigsetsize` - size of sigset_t
pub unsafe fn sys_sigprocmask(
    how: c_int,
    set: *const CSigSet,
    oldset: *mut CSigSet,
    sigsetsize: usize,
) -> c_int {
    debug!(
        "sys_sigprocmask <= how: {}, set: {:#x}, oldset: {:#x}, sigsetsize: {}",
        how, set as usize, oldset as usize, sigsetsize
    );

    syscall_body!(sys_sigprocmask, {
        if sigsetsize < 8 {
            return Err(LinuxError::EINVAL);
        }

        let info = get_or_init_signal_info();
        let mut info_guard = info.write();

        // Store old mask if requested
        if !oldset.is_null() {
            let old_mask = info_guard.blocked_set();
            let old_c_set = CSigSet::from(old_mask);
            unsafe { core::ptr::write(oldset, old_c_set) };
        }

        // Set new mask if provided
        if !set.is_null() {
            let new_c_set = unsafe { core::ptr::read(set) };
            let new_set = SigSet::from(&new_c_set);

            match how {
                SIG_BLOCK | SIG_UNBLOCK | SIG_SETMASK => {
                    info_guard.sigprocmask(how, Some(&new_set));
                }
                _ => return Err(LinuxError::EINVAL),
            }
        }

        Ok(0)
    })
}

/// Return from signal handler (sigreturn system call).
///
/// This is called by the signal handler's sa_restorer function to
/// return to the original execution context.
pub fn sys_sigreturn() -> c_int {
    debug!("sys_sigreturn <=");

    syscall_body!(sys_sigreturn, {
        // In a full implementation, this would restore the saved context
        // from the signal frame on the user stack. For now, we just
        // restore the signal mask that was saved before entering the handler.
        //
        // The actual context restoration would involve:
        // 1. Reading the ucontext from the signal frame
        // 2. Restoring the blocked signal mask from uc_sigmask
        // 3. Restoring the general-purpose registers from uc_mcontext
        // 4. Returning to the interrupted instruction

        // For the unikernel case, we simply return 0.
        // The signal delivery mechanism in a real OS would have saved
        // the context on the user stack and the sigreturn would restore it.
        Ok(0)
    })
}

/// Send a signal to the current process (raise).
pub fn sys_raise(sig: c_int) -> c_int {
    debug!("sys_raise <= sig: {}", sig);

    syscall_body!(sys_raise, {
        validate_signal(sig)?;
        let info = get_or_init_signal_info();
        info.write().send_signal(sig).map_err(|_| LinuxError::EINVAL)?;
        Ok(0)
    })
}

/// Examine pending signals (sigpending system call).
pub unsafe fn sys_sigpending(set: *mut CSigSet, sigsetsize: usize) -> c_int {
    debug!(
        "sys_sigpending <= set: {:#x}, sigsetsize: {}",
        set as usize, sigsetsize
    );

    syscall_body!(sys_sigpending, {
        if sigsetsize < 8 {
            return Err(LinuxError::EINVAL);
        }
        if set.is_null() {
            return Err(LinuxError::EFAULT);
        }

        let info = get_or_init_signal_info();
        let info_guard = info.read();

        // Combine process-pending and task-pending signals
        let pending = info_guard.pending_set().union(info_guard.task_pending_set());
        let c_set = CSigSet::from(&pending);
        unsafe { core::ptr::write(set, c_set) };

        Ok(0)
    })
}

/// Synchronously wait for a signal (sigsuspend system call).
///
/// Temporarily replaces the signal mask and suspends execution until
/// a signal is received.
pub unsafe fn sys_sigsuspend(mask: *const CSigSet, sigsetsize: usize) -> c_int {
    debug!(
        "sys_sigsuspend <= mask: {:#x}, sigsetsize: {}",
        mask as usize, sigsetsize
    );

    syscall_body!(sys_sigsuspend, {
        if sigsetsize < 8 {
            return Err(LinuxError::EINVAL);
        }
        if mask.is_null() {
            return Err(LinuxError::EFAULT);
        }

        let new_c_set = unsafe { core::ptr::read(mask) };
        let new_set = SigSet::from(&new_c_set);

        let info = get_or_init_signal_info();
        let mut info_guard = info.write();

        // Save old mask, set new mask
        let _old_mask = info_guard.sigprocmask(SIG_SETMASK, Some(&new_set));

        // Check if there are pending unblocked signals
        if info_guard.has_pending_signals() {
            // There's a signal to deliver, return -EINTR
            return Err(LinuxError::EINTR);
        }

        // In a full implementation, we would block the task here
        // until a signal arrives. For now, we yield and return EINTR.
        drop(info_guard);
        axtask::yield_now();

        Err(LinuxError::EINTR)
    })
}

/// Change the signal mask and wait for signals (rt_sigsuspend).
pub unsafe fn sys_rt_sigsuspend(mask: *const CSigSet, sigsetsize: usize) -> c_int {
    sys_sigsuspend(mask, sigsetsize)
}

/// Examine and change blocked signals (rt_sigprocmask).
pub unsafe fn sys_rt_sigprocmask(
    how: c_int,
    set: *const CSigSet,
    oldset: *mut CSigSet,
    sigsetsize: usize,
) -> c_int {
    sys_sigprocmask(how, set, oldset, sigsetsize)
}

/// Examine and change signal action (rt_sigaction).
pub unsafe fn sys_rt_sigaction(
    sig: c_int,
    act: *const CSigAction,
    oact: *mut CSigAction,
    sigsetsize: usize,
) -> c_int {
    sys_sigaction(sig, act, oact, sigsetsize)
}

/// Send signal to a process (rt_sigqueueinfo).
pub fn sys_rt_sigqueueinfo(pid: c_int, sig: c_int, _info: usize) -> c_int {
    debug!("sys_rt_sigqueueinfo <= pid: {}, sig: {}", pid, sig);
    // For now, delegate to kill
    sys_kill(pid, sig)
}

/// Send signal to a thread (rt_tgsigqueueinfo).
pub fn sys_rt_tgsigqueueinfo(pid: c_int, tid: c_int, sig: c_int, _info: usize) -> c_int {
    debug!(
        "sys_rt_tgsigqueueinfo <= pid: {}, tid: {}, sig: {}",
        pid, tid, sig
    );
    // For now, delegate to tgkill
    sys_tgkill(pid, tid, sig)
}

/// Get current pending signals (rt_sigpending).
pub unsafe fn sys_rt_sigpending(set: *mut CSigSet, sigsetsize: usize) -> c_int {
    sys_sigpending(set, sigsetsize)
}

/// Wait for a signal (rt_sigtimedwait).
pub fn sys_rt_sigtimedwait(
    _mask: *const CSigSet,
    _info: usize,
    _timeout: usize,
    _sigsetsize: usize,
) -> c_int {
    debug!("sys_rt_sigtimedwait <=");
    // Not fully implemented
    syscall_body!(sys_rt_sigtimedwait, { Err(LinuxError::EAGAIN) })
}
