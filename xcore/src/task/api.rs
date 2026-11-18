use alloc::sync::Arc;

use axtask::{CurrentTask, TaskExtRef, current};

use xprocess::{Process, Thread};

use crate::{
    mm::XUserSpace,
    task::{XProcess, XTaskExt, XThread},
};

/// Execute a closure with the current task.
///
/// # Arguments
/// * `f` - Closure to execute with current task reference
pub fn with_current<T, F>(f: F) -> T
where
    F: FnOnce(&CurrentTask) -> T,
{
    f(&current())
}

/// Execute a closure with the current task extension.
///
/// # Arguments
/// * `f` - Closure to execute with XTaskExt reference
pub fn with_xtask<T, F>(f: F) -> T
where
    F: FnOnce(&XTaskExt) -> T,
{
    f(current().task_ext())
}

/// Execute a closure with the current thread.
///
/// # Arguments
/// * `f` - Closure to execute with thread reference
pub fn with_thread<T, F>(f: F) -> T
where
    F: FnOnce(&Arc<Thread>) -> T,
{
    f(current().task_ext().thread_ref())
}

/// Execute a closure with the current process.
///
/// # Arguments
/// * `f` - Closure to execute with process reference
pub fn with_process<T, F>(f: F) -> T
where
    F: FnOnce(&Arc<Process>) -> T,
{
    f(current().task_ext().process_ref())
}

/// Execute a closure with the current thread extension.
///
/// # Arguments
/// * `f` - Closure to execute with XThread reference
pub fn with_xthread<T, F>(f: F) -> T
where
    F: FnOnce(&XThread) -> T,
{
    f(current().task_ext().xthread_ref())
}

/// Execute a closure with the current process extension.
///
/// # Arguments
/// * `f` - Closure to execute with XProcess reference
pub fn with_xprocess<T, F>(f: F) -> T
where
    F: FnOnce(&XProcess) -> T,
{
    f(current().task_ext().xprocess_ref())
}

/// Execute a closure with the current process user space.
///
/// # Arguments
/// * `f` - Closure to execute with user space reference
pub fn with_uspace<T, F>(f: F) -> T
where
    F: FnOnce(&XUserSpace) -> T,
{
    f(current().task_ext().xprocess_ref().uspace())
}
