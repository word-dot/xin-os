use alloc::sync::Arc;
use core::ffi::{c_char, c_int};
use core::ptr;

use axerrno::{LinuxError, LinuxResult};
use axfs::fops::OpenOptions;
use axio::{PollState, SeekFrom};
use axsync::Mutex;

use super::fd_ops::{FileLike, get_file_like};
use crate::{ctypes, utils::char_ptr_to_str};

pub struct File {
    inner: Mutex<axfs::fops::File>,
}

impl File {
    fn new(inner: axfs::fops::File) -> Self {
        Self {
            inner: Mutex::new(inner),
        }
    }

    fn add_to_fd_table(self) -> LinuxResult<c_int> {
        super::fd_ops::add_file_like(Arc::new(self))
    }

    fn from_fd(fd: c_int) -> LinuxResult<Arc<Self>> {
        let f = super::fd_ops::get_file_like(fd)?;
        f.into_any()
            .downcast::<Self>()
            .map_err(|_| LinuxError::EINVAL)
    }
}

impl FileLike for File {
    fn read(&self, buf: &mut [u8]) -> LinuxResult<usize> {
        Ok(self.inner.lock().read(buf)?)
    }

    fn write(&self, buf: &[u8]) -> LinuxResult<usize> {
        Ok(self.inner.lock().write(buf)?)
    }

    fn stat(&self) -> LinuxResult<ctypes::stat> {
        let metadata = self.inner.lock().get_attr()?;
        let ty = metadata.file_type() as u8;
        let perm = metadata.perm().bits() as u32;
        let st_mode = ((ty as u32) << 12) | perm;
        Ok(ctypes::stat {
            st_ino: 1,
            st_nlink: 1,
            st_mode,
            st_uid: 1000,
            st_gid: 1000,
            st_size: metadata.size() as _,
            st_blocks: metadata.blocks() as _,
            st_blksize: 512,
            ..Default::default()
        })
    }

    fn into_any(self: Arc<Self>) -> Arc<dyn core::any::Any + Send + Sync> {
        self
    }

    fn poll(&self) -> LinuxResult<PollState> {
        Ok(PollState {
            readable: true,
            writable: true,
        })
    }

    fn set_nonblocking(&self, _nonblocking: bool) -> LinuxResult {
        Ok(())
    }
}

/// Convert open flags to [`OpenOptions`].
fn flags_to_options(flags: c_int, _mode: ctypes::mode_t) -> OpenOptions {
    let flags = flags as u32;
    let mut options = OpenOptions::new();
    match flags & 0b11 {
        ctypes::O_RDONLY => options.read(true),
        ctypes::O_WRONLY => options.write(true),
        _ => {
            options.read(true);
            options.write(true);
        }
    };
    if flags & ctypes::O_APPEND != 0 {
        options.append(true);
    }
    if flags & ctypes::O_TRUNC != 0 {
        options.truncate(true);
    }
    if flags & ctypes::O_CREAT != 0 {
        options.create(true);
    }
    if flags & ctypes::O_EXEC != 0 {
        options.create_new(true);
    }
    options
}

/// Open a file by `filename` and insert it into the file descriptor table.
///
/// Return its index in the file table (`fd`). Return `EMFILE` if it already
/// has the maximum number of files open.
pub fn sys_open(filename: *const c_char, flags: c_int, mode: ctypes::mode_t) -> c_int {
    let filename = char_ptr_to_str(filename);
    debug!("sys_open <= {:?} {:#o} {:#o}", filename, flags, mode);
    syscall_body!(sys_open, {
        let options = flags_to_options(flags, mode);
        let file = axfs::fops::File::open(filename?, &options)?;
        File::new(file).add_to_fd_table()
    })
}

/// Set the position of the file indicated by `fd`.
///
/// Return its position after seek.
pub fn sys_lseek(fd: c_int, offset: ctypes::off_t, whence: c_int) -> ctypes::off_t {
    debug!("sys_lseek <= {} {} {}", fd, offset, whence);
    syscall_body!(sys_lseek, {
        let pos = match whence {
            0 => SeekFrom::Start(offset as _),
            1 => SeekFrom::Current(offset as _),
            2 => SeekFrom::End(offset as _),
            _ => return Err(LinuxError::EINVAL),
        };
        let off = File::from_fd(fd)?.inner.lock().seek(pos)?;
        Ok(off)
    })
}

/// Get the file metadata by `path` and write into `buf`.
///
/// Return 0 if success.
pub unsafe fn sys_stat(path: *const c_char, buf: *mut ctypes::stat) -> c_int {
    let path = char_ptr_to_str(path);
    debug!("sys_stat <= {:?} {:#x}", path, buf as usize);
    syscall_body!(sys_stat, {
        if buf.is_null() {
            return Err(LinuxError::EFAULT);
        }
        let mut options = OpenOptions::new();
        options.read(true);
        let file = axfs::fops::File::open(path?, &options)?;
        let st = File::new(file).stat()?;
        *buf = st;
        Ok(0)
    })
}

/// Get file metadata by `fd` and write into `buf`.
///
/// Return 0 if success.
pub unsafe fn sys_fstat(fd: c_int, buf: *mut ctypes::stat) -> c_int {
    debug!("sys_fstat <= {} {:#x}", fd, buf as usize);
    syscall_body!(sys_fstat, {
        if buf.is_null() {
            return Err(LinuxError::EFAULT);
        }
        *buf = get_file_like(fd)?.stat()?;
        Ok(0)
    })
}

/// Get the metadata of the symbolic link and write into `buf`.
///
/// Return 0 if success.
pub unsafe fn sys_lstat(path: *const c_char, buf: *mut ctypes::stat) -> ctypes::ssize_t {
    let path = char_ptr_to_str(path);
    debug!("sys_lstat <= {:?} {:#x}", path, buf as usize);
    syscall_body!(sys_lstat, {
        if buf.is_null() {
            return Err(LinuxError::EFAULT);
        }
        *buf = Default::default(); // TODO
        Ok(0)
    })
}

/// Get the path of the current directory.
#[allow(clippy::unnecessary_cast)] // `c_char` is either `i8` or `u8`
pub fn sys_getcwd(buf: *mut c_char, size: usize) -> *mut c_char {
    debug!("sys_getcwd <= {:#x} {}", buf as usize, size);
    syscall_body!(sys_getcwd, {
        if buf.is_null() {
            return Ok(core::ptr::null::<c_char>() as _);
        }
        let dst = unsafe { core::slice::from_raw_parts_mut(buf as *mut u8, size as _) };
        let cwd = axfs::api::current_dir()?;
        let cwd = cwd.as_bytes();
        if cwd.len() < size {
            dst[..cwd.len()].copy_from_slice(cwd);
            dst[cwd.len()] = 0;
            Ok(buf)
        } else {
            Err(LinuxError::ERANGE)
        }
    })
}

/// Rename `old` to `new`
/// If new exists, it is first removed.
///
/// Return 0 if the operation succeeds, otherwise return -1.
pub fn sys_rename(old: *const c_char, new: *const c_char) -> c_int {
    syscall_body!(sys_rename, {
        let old_path = char_ptr_to_str(old)?;
        let new_path = char_ptr_to_str(new)?;
        debug!("sys_rename <= old: {:?}, new: {:?}", old_path, new_path);
        axfs::api::rename(old_path, new_path)?;
        Ok(0)
    })
}

/// Read from file descriptor
pub fn sys_read(fd: c_int, buf: *mut u8, len: usize) -> usize {
    let file = File::from_fd(fd)?;
    Ok(file.inner.lock().read(buf)?)
}

pub fn sys_write(fd: c_int, buf: *const u8, len: usize) -> usize {
    let file = File::from_fd(fd)?;
    Ok(file.inner.lock().write(buf)?)
}

/// Open a directory entry
pub fn sys_opendir(path: *const c_char) -> c_int {
    let dir = axfs::api::open_dir(path)?;
    // Return directory file descriptor
    Ok(dir.fd)
}

/// Read next directory entry
pub fn sys_readdir(fd: c_int, dirp: *mut c_char) -> c_int {
    let dir = File::from_fd(fd)?;
    let entry = dir.inner.lock().read_dir()?;
    if entry.is_none() {
        return 0; // End of directory
    }
    let name = entry.unwrap().name();
    // Copy name to dirp
    unsafe {
        core::ptr::copy_nonoverlapping(name.as_ptr(), dirp, name.len());
    }
    Ok(name.len() + 1)
}

/// Close a directory
pub fn sys_closedir(fd: c_int) -> c_int {
    let dir = File::from_fd(fd)?;
    dir.inner.lock().close()?;
    Ok(0)
}

/// Create a new directory
pub fn sys_mkdir(path: *const c_char, _mode: ctypes::mode_t) -> c_int {
    let path = char_ptr_to_str(path);
    debug!("sys_mkdir <= {:?}", path);
    syscall_body!(sys_mkdir, {
        axfs::api::create_dir(path?).map_err(|e| {
            error!("mkdir failed: {:?}", e);
            LinuxError::EIO
        })?;
        Ok(0)
    })
}

/// Remove an empty directory
pub fn sys_rmdir(path: *const c_char) -> c_int {
    let path = char_ptr_to_str(path);
    debug!("sys_rmdir <= {:?}", path);
    syscall_body!(sys_rmdir, {
        axfs::api::remove_dir(path?).map_err(|e| {
            error!("rmdir failed: {:?}", e);
            LinuxError::EIO
        })?;
        Ok(0)
    })
}

/// Remove a file (unlink)
pub fn sys_unlink(path: *const c_char) -> c_int {
    let path = char_ptr_to_str(path);
    debug!("sys_unlink <= {:?}", path);
    syscall_body!(sys_unlink, {
        axfs::api::remove_file(path?).map_err(|e| {
            error!("unlink failed: {:?}", e);
            LinuxError::EIO
        })?;
        Ok(0)
    })
}

/// Change the current working directory
pub fn sys_chdir(path: *const c_char) -> c_int {
    let path = char_ptr_to_str(path);
    debug!("sys_chdir <= {:?}", path);
    syscall_body!(sys_chdir, {
        axfs::api::set_current_dir(path?).map_err(|e| {
            error!("chdir failed: {:?}", e);
            LinuxError::EIO
        })?;
        Ok(0)
    })
}

/// Get directory entries (getdents64) - stub that returns entries from root dir
pub fn sys_getdents64(_fd: c_int, buf: *mut u8, len: usize) -> c_int {
    debug!("sys_getdents64 <= {} {:#x} {}", _fd, buf as usize, len);
    syscall_body!(sys_getdents64, {
        if buf.is_null() || len == 0 {
            return Err(LinuxError::EINVAL);
        }
        // List entries from root directory
        let entries = axfs::api::read_dir("/").map_err(|_| LinuxError::EIO)?;
        let mut offset = 0usize;
        for entry in entries {
            let name = entry.file_name();
            let name_bytes = name.as_bytes();
            // Simple dirent layout: d_ino(8) + d_off(8) + d_reclen(2) + d_type(1) + d_name(nul)
            let dirent_hdr_size: usize = 19; // 8 + 8 + 2 + 1
            let reclen = ((dirent_hdr_size + name_bytes.len() + 1) + 7) & !7; // 8-byte aligned
            if offset + reclen > len {
                break;
            }
            unsafe {
                // Write d_ino (u64)
                (buf.add(offset) as *mut u64).write_unaligned(1u64);
                // Write d_off (i64)
                (buf.add(offset + 8) as *mut u64).write_unaligned((offset + reclen) as u64);
                // Write d_reclen (u16)
                (buf.add(offset + 16) as *mut u16).write_unaligned(reclen as u16);
                // Write d_type (u8) - DT_DIR for dir entries, DT_REG for files
                (buf.add(offset + 18) as *mut u8).write_unaligned(8u8); // DT_REG
                // Write d_name
                let name_ptr = buf.add(offset + 19);
                core::ptr::copy_nonoverlapping(name_bytes.as_ptr(), name_ptr, name_bytes.len());
                name_ptr.add(name_bytes.len()).write(0u8);
            }
            offset += reclen;
        }
        Ok(offset as c_int)
    })
}
