#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use axfs_vfs::{VfsDirEntry, VfsError, VfsNodeAttr, VfsNodeOps, VfsNodePerm, VfsNodeRef, VfsNodeType, VfsOps, VfsResult};
use axstd::sync::Mutex;
use ext4_view::FileType;

const BLOCK_SIZE: u64 = 512;

/// Global ext4 filesystem instance, initialized once during mount.
/// We use a static because Ext4 (which uses Rc internally) is not Send+Sync,
/// so we cannot store it inside VfsNodeRef objects.
static EXT4_FS: core::sync::atomic::AtomicPtr<u8> = core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());

/// ext4 filesystem adapter implementing VfsOps.
pub struct Ext4FileSystem {
    root_dir: Arc<Ext4DirNode>,
}

unsafe impl Send for Ext4FileSystem {}
unsafe impl Sync for Ext4FileSystem {}

impl Ext4FileSystem {
    pub fn new(ext4: ext4_view::Ext4) -> Self {
        // Store the Ext4 instance in a Box on the heap
        let boxed = Box::new(ext4);
        let ptr = Box::into_raw(boxed);
        EXT4_FS.store(ptr as *mut u8, core::sync::atomic::Ordering::SeqCst);

        let root_dir = Arc::new(Ext4DirNode {
            path: String::from("/"),
        });
        Self { root_dir }
    }

    /// Get a reference to the global Ext4 instance.
    fn get_ext4() -> &'static ext4_view::Ext4 {
        let ptr = EXT4_FS.load(core::sync::atomic::Ordering::SeqCst) as *const ext4_view::Ext4;
        unsafe { &*ptr }
    }
}

impl VfsOps for Ext4FileSystem {
    fn root_dir(&self) -> VfsNodeRef {
        self.root_dir.clone()
    }
}

/// Directory node in the ext4 filesystem.
pub struct Ext4DirNode {
    path: String,
}

/// File node in the ext4 filesystem.
pub struct Ext4FileNode {
    path: String,
    size: u64,
    /// Cached file content to avoid repeated slow ext4 reads.
    cache: Mutex<Option<Vec<u8>>>,
}

unsafe impl Send for Ext4DirNode {}
unsafe impl Sync for Ext4DirNode {}
unsafe impl Send for Ext4FileNode {}
unsafe impl Sync for Ext4FileNode {}

fn as_vfs_err(e: ext4_view::Ext4Error) -> VfsError {
    use ext4_view::Ext4Error::*;
    match e {
        NotFound => VfsError::NotFound,
        AlreadyExists => VfsError::AlreadyExists,
        NotADirectory => VfsError::NotADirectory,
        IsADirectory => VfsError::IsADirectory,
        InvalidInput => VfsError::InvalidInput,
        _ => VfsError::Io,
    }
}

fn join_path(base: &str, name: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.is_empty() {
        format!("/{}", name)
    } else {
        format!("{}/{}", base, name)
    }
}

fn ext4_file_type_to_vfs(ft: FileType) -> VfsNodeType {
    match ft {
        FileType::Directory => VfsNodeType::Dir,
        FileType::Regular => VfsNodeType::File,
        FileType::Symlink => VfsNodeType::SymLink,
        _ => VfsNodeType::File,
    }
}

impl VfsNodeOps for Ext4DirNode {
    axfs_vfs::impl_vfs_dir_default! {}

    fn get_attr(&self) -> VfsResult<VfsNodeAttr> {
        let perm = VfsNodePerm::from_bits_truncate(0o755);
        Ok(VfsNodeAttr::new(perm, VfsNodeType::Dir, BLOCK_SIZE, 1))
    }

    fn parent(&self) -> Option<VfsNodeRef> {
        if self.path == "/" {
            return None;
        }
        let path = self.path.trim_end_matches('/');
        let last_sep = path.rfind('/')?;
        let parent_path = if last_sep == 0 {
            String::from("/")
        } else {
            String::from(&path[..last_sep])
        };
        Some(Arc::new(Ext4DirNode {
            path: parent_path,
        }))
    }

    fn lookup(self: Arc<Self>, path: &str) -> VfsResult<VfsNodeRef> {
        let path = path.trim_matches('/');
        if path.is_empty() || path == "." {
            return Ok(self.clone());
        }
        
        let components: Vec<&str> = path.split('/').filter(|c| !c.is_empty() && *c != ".").collect();
        
        if components.is_empty() {
            return Ok(self.clone());
        }

        let first = components[0];
        let rest = if components.len() > 1 {
            Some(components[1..].join("/"))
        } else {
            None
        };

        let child_path = join_path(&self.path, first);
        let ext4 = Ext4FileSystem::get_ext4();

        match ext4.metadata(&child_path) {
            Ok(meta) => {
                let node: VfsNodeRef = if meta.is_dir() {
                    Arc::new(Ext4DirNode {
                        path: child_path,
                    })
                } else {
                    Arc::new(Ext4FileNode {
                        path: child_path,
                        size: meta.len(),
                        cache: Mutex::new(None),
                    })
                };

                if let Some(rest_path) = rest {
                    node.lookup(&rest_path)
                } else {
                    Ok(node)
                }
            }
            Err(_) => {
                // Path does not exist in ext4. This can happen for mount
                // point directories (e.g., /dev, /tmp) that were "created"
                // by our create() stub. Return a virtual Ext4DirNode so
                // that the mount subsystem can proceed.
                if rest.is_none() {
                    Ok(Arc::new(Ext4DirNode {
                        path: child_path,
                    }))
                } else {
                    Err(VfsError::NotFound)
                }
            }
        }
    }

    fn create(&self, path: &str, ty: VfsNodeType) -> VfsResult {
        // ext4 is read-only, but we need to allow "creating" directories
        // for mount points (/dev, /tmp, /proc, etc.) even if they don't
        // exist in the ext4 filesystem. These are virtual filesystem mount
        // points whose actual content comes from the mounted filesystem.
        if ty == VfsNodeType::Dir {
            // Allow directory creation for mount points regardless of
            // whether the directory exists in ext4. The mount subsystem
            // needs this to succeed so it can overlay virtual filesystems.
            return Ok(());
        }
        Err(VfsError::Unsupported)
    }

    fn remove(&self, _path: &str) -> VfsResult {
        Err(VfsError::Unsupported)
    }

    fn read_dir(&self, start_idx: usize, dirents: &mut [VfsDirEntry]) -> VfsResult<usize> {
        let ext4 = Ext4FileSystem::get_ext4();
        let entries: Vec<_> = match ext4.read_dir(&self.path) {
            Ok(iter) => iter.collect(),
            Err(e) => return Err(as_vfs_err(e)),
        };

        let mut count = 0;
        for (i, entry) in entries.iter().enumerate().skip(start_idx) {
            if count >= dirents.len() {
                break;
            }
            match entry {
                Ok(e) => {
                    let name_str = e.file_name().as_str().unwrap_or("?");
                    let ft = e.file_type().map_err(as_vfs_err)?;
                    let vfs_type = ext4_file_type_to_vfs(ft);
                    dirents[count] = VfsDirEntry::new(name_str, vfs_type);
                    count += 1;
                }
                Err(_) => continue,
            }
        }
        Ok(count)
    }

    fn rename(&self, _src_path: &str, _dst_path: &str) -> VfsResult {
        Err(VfsError::Unsupported)
    }
}

impl VfsNodeOps for Ext4FileNode {
    axfs_vfs::impl_vfs_non_dir_default! {}

    fn get_attr(&self) -> VfsResult<VfsNodeAttr> {
        let perm = VfsNodePerm::from_bits_truncate(0o755);
        let blocks = if self.size > 0 { self.size.div_ceil(BLOCK_SIZE) } else { 0 };
        Ok(VfsNodeAttr::new(perm, VfsNodeType::File, self.size, blocks))
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        let mut cache = self.cache.lock();
        if cache.is_none() {
            let ext4 = Ext4FileSystem::get_ext4();
            let mut file = ext4.open(&self.path).map_err(as_vfs_err)?;
            let mut data = alloc::vec![0u8; self.size as usize];
            let mut total = 0usize;
            
            // Read the entire file
            while total < data.len() {
                let n = file.read_bytes(&mut data[total..]).map_err(as_vfs_err)?;
                if n == 0 {
                    break;
                }
                total += n;
            }
            
            if total == 0 && self.size > 0 {
                return Err(VfsError::Io);
            }
            
            data.truncate(total);
            *cache = Some(data);
        }

        let data = cache.as_ref().unwrap();
        let start = offset as usize;
        if start >= data.len() {
            return Ok(0);
        }
        let end = (start + buf.len()).min(data.len());
        let len = end - start;
        buf[..len].copy_from_slice(&data[start..end]);
        Ok(len)
    }
}
