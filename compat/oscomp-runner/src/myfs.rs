extern crate alloc;

use alloc::sync::Arc;
use axfs_vfs::VfsOps;
use axstd::os::arceos::api::fs::{AxDisk, MyFileSystemIf};
use axstd::sync::Mutex;
use core::option::Option::{self, Some, None};
use core::result::Result;
use lazyinit::LazyInit;

static BOOT_DISK: Mutex<Option<Arc<Mutex<AxDisk>>>> = Mutex::new(None);

// Global axfs-ng filesystem instance
static AXFS_NG_FS: LazyInit<axfs_ng_vfs::Filesystem<axsync::RawMutex>> = LazyInit::new();

struct CompatFs;

pub fn boot_disk() -> Result<Arc<Mutex<AxDisk>>, axstd::string::String> {
    BOOT_DISK
        .lock()
        .as_ref()
        .cloned()
        .ok_or_else(|| "boot disk is not available".to_string())
}

/// Initialize axfs-ng filesystem with ext4
pub fn init_axfs_ng(disk: axdriver::AxBlockDevice) {
    println!("[myfs] Initializing axfs-ng with ext4 support...");
    
    // Use the fs module's new_default function (will use ext4 with the compatible lwext4_rust)
    match axfs_ng::fs::new_default::<axsync::RawMutex>(disk) {
        Ok(fs) => {
            println!("[myfs] ? axfs-ng initialized successfully with ext4!");
            AXFS_NG_FS.init_once(fs);
        }
        Err(e) => {
            println!("[myfs] ? Failed to initialize axfs-ng: {:?}", e);
        }
    }
}

/// Get the global axfs-ng filesystem instance
pub fn get_axfs_ng() -> Option<&'static axfs_ng_vfs::Filesystem<axsync::RawMutex>> {
    if AXFS_NG_FS.is_inited() {
        Some(&*AXFS_NG_FS)
    } else {
        None
    }
}

/// Create a FsContext for axfs-ng filesystem operations
pub fn create_axfs_context() -> Option<axfs_ng::FsContext<axsync::RawMutex>> {
    use axfs_ng_vfs::Mountpoint;
    
    get_axfs_ng().map(|fs| {
        // Create a root mountpoint and get its location
        let mountpoint = Mountpoint::new_root(fs);
        let root_location = mountpoint.root_location();
        axfs_ng::FsContext::new(root_location)
    })
}

#[crate_interface::impl_interface]
impl MyFileSystemIf for CompatFs {
    fn new_myfs(disk: AxDisk) -> Arc<dyn VfsOps> {
        let disk_arc = Arc::new(Mutex::new(disk));
        *BOOT_DISK.lock() = Some(disk_arc.clone());

        // Convert AxDisk to AxBlockDevice for axfs-ng
        println!("[myfs] Converting AxDisk to AxBlockDevice...");
        let block_dev = axdriver::AxBlockDevice::from(disk);
        
        // Initialize axfs-ng in background thread
        axtask::spawn(move || {
            init_axfs_ng(block_dev);
        });

        // Return a simple ramfs as placeholder (axfs-ng will be used via get_axfs_ng())
        println!("[myfs] Returning ramfs placeholder (axfs-ng will be ready soon)...");
        Arc::new(axfs_ramfs::RamFileSystem::new())
    }
}