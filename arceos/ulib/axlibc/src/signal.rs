
use crate::{ctypes, utils::e};
use arceos_posix_api as api;
use core::ffi::{c_int, c_void};

/// C sigset_t representation matching signal.h layout.
/// On 64-bit: unsigned long __bits[128 / sizeof(long)] = [u64; 2]
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CSigSet {
    pub bits: [u64; 2],
}

/// C sigaction representation matching signal.h layout.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CSigAction {
    pub sa_handler: usize,
    pub sa_mask: CSigSet,
    pub sa_flags: c_int,
    pub sa_restorer: usize,
}

/// Register a signal handler (sigaction).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sigaction(
    sig: c_int,
    act: *const CSigAction,
    oact: *mut CSigAction,
) -> c_int {
    e(api::sys_sigaction(sig, act, oact, core::mem::size_of::<CSigSet>()))
}

/// Internal helper called from C signal.c.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sigaction_helper(
    signum: c_int,
    act: *const CSigAction,
    oldact: *mut CSigAction,
    sigsetsize: usize,
) -> c_int {
    api::sys_sigaction(signum, act, oldact, sigsetsize)
}

/// Send a signal to a process (kill).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kill(pid: c_int, sig: c_int) -> c_int {
    e(api::sys_kill(pid, sig))
}

/// Send a signal to a thread (tkill).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tkill(tid: c_int, sig: c_int) -> c_int {
    e(api::sys_tkill(tid, sig))
}

/// Send a signal to a thread in a thread group (tgkill).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tgkill(tgid: c_int, tid: c_int, sig: c_int) -> c_int {
    e(api::sys_tgkill(tgid, tid, sig))
}

/// Send a signal to the current process (raise).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn raise(sig: c_int) -> c_int {
    e(api::sys_raise(sig))
}

/// Examine and change blocked signals (sigprocmask).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sigprocmask(
    how: c_int,
    set: *const CSigSet,
    oldset: *mut CSigSet,
) -> c_int {
    e(api::sys_sigprocmask(how, set, oldset, core::mem::size_of::<CSigSet>()))
}

/// Thread-directed signal mask (pthread_sigmask).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_sigmask(
    how: c_int,
    set: *const CSigSet,
    oldset: *mut CSigSet,
) -> c_int {
    e(api::sys_sigprocmask(how, set, oldset, core::mem::size_of::<CSigSet>()))
}

/// Send a signal to a thread (pthread_kill).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pthread_kill(thread: ctypes::pthread_t, sig: c_int) -> c_int {
    // pthread_t is a pointer; extract the task ID from the Pthread struct
    // For simplicity, we use the current task's signal info
    // In a full implementation, we'd look up the thread by its pthread_t
    e(api::sys_tkill(thread as c_int, sig))
}

/// Return from signal handler (sigreturn).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sigreturn() -> c_int {
    api::sys_sigreturn()
}

/// Examine pending signals (sigpending).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sigpending(set: *mut CSigSet) -> c_int {
    e(api::sys_sigpending(set, core::mem::size_of::<CSigSet>()))
}

/// Synchronously wait for a signal (sigsuspend).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sigsuspend(mask: *const CSigSet) -> c_int {
    e(api::sys_sigsuspend(mask, core::mem::size_of::<CSigSet>()))
}

/// rt_sigaction (real-time signal action).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rt_sigaction(
    sig: c_int,
    act: *const CSigAction,
    oact: *mut CSigAction,
    sigsetsize: usize,
) -> c_int {
    e(api::sys_rt_sigaction(sig, act, oact, sigsetsize))
}

/// rt_sigprocmask (real-time signal procmask).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rt_sigprocmask(
    how: c_int,
    set: *const CSigSet,
    oldset: *mut CSigSet,
    sigsetsize: usize,
) -> c_int {
    e(api::sys_rt_sigprocmask(how, set, oldset, sigsetsize))
}

/// rt_sigpending.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rt_sigpending(set: *mut CSigSet, sigsetsize: usize) -> c_int {
    e(api::sys_rt_sigpending(set, sigsetsize))
}

/// rt_sigqueueinfo.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rt_sigqueueinfo(pid: c_int, sig: c_int, info: usize) -> c_int {
    e(api::sys_rt_sigqueueinfo(pid, sig, info))
}

/// rt_tgsigqueueinfo.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rt_tgsigqueueinfo(
    pid: c_int,
    tid: c_int,
    sig: c_int,
    info: usize,
) -> c_int {
    e(api::sys_rt_tgsigqueueinfo(pid, tid, sig, info))
}
