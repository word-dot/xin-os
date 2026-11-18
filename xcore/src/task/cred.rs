use alloc::{sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};

use axsync::Mutex;

#[derive(Debug)]
pub struct ProcessCredentials {
    pub uid: AtomicU32,
    pub gid: AtomicU32,
    pub fsuid: AtomicU32,
    pub fsgid: AtomicU32,
    pub euid: AtomicU32,
    pub egid: AtomicU32,
    pub suid: AtomicU32,
    pub sgid: AtomicU32,
    pub sup_group: Arc<Mutex<Vec<u32>>>,
}

impl Default for ProcessCredentials {
    fn default() -> Self {
        Self {
            uid: AtomicU32::new(0),
            gid: AtomicU32::new(0),
            fsuid: AtomicU32::new(0),
            fsgid: AtomicU32::new(0),
            euid: AtomicU32::new(0),
            egid: AtomicU32::new(0),
            suid: AtomicU32::new(0),
            sgid: AtomicU32::new(0),
            sup_group: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl ProcessCredentials {
    /// Get the real user ID.
    pub fn uid(&self) -> u32 {
        self.uid.load(Ordering::SeqCst)
    }

    /// Set the real user ID.
    pub fn set_uid(&self, uid: u32) {
        self.uid.store(uid, Ordering::SeqCst);
    }

    /// Get the real group ID.
    pub fn gid(&self) -> u32 {
        self.gid.load(Ordering::SeqCst)
    }

    /// Set the real group ID.
    pub fn set_gid(&self, gid: u32) {
        self.gid.store(gid, Ordering::SeqCst);
    }

    /// Get the filesystem user ID.
    pub fn fsuid(&self) -> u32 {
        self.fsuid.load(Ordering::SeqCst)
    }

    /// Set the filesystem user ID.
    pub fn set_fsuid(&self, fsuid: u32) {
        self.fsuid.store(fsuid, Ordering::SeqCst);
    }

    /// Get the filesystem group ID.
    pub fn fsgid(&self) -> u32 {
        self.fsgid.load(Ordering::SeqCst)
    }

    /// Set the filesystem group ID.
    pub fn set_fsgid(&self, fsgid: u32) {
        self.fsgid.store(fsgid, Ordering::SeqCst);
    }

    /// Get the effective user ID.
    pub fn euid(&self) -> u32 {
        self.euid.load(Ordering::SeqCst)
    }

    /// Set the effective user ID.
    pub fn set_euid(&self, euid: u32) {
        self.euid.store(euid, Ordering::SeqCst);
    }

    /// Get the effective group ID.
    pub fn egid(&self) -> u32 {
        self.egid.load(Ordering::SeqCst)
    }

    /// Set the effective group ID.
    pub fn set_egid(&self, egid: u32) {
        self.egid.store(egid, Ordering::SeqCst);
    }

    /// Get the saved user ID.
    pub fn suid(&self) -> u32 {
        self.suid.load(Ordering::SeqCst)
    }

    /// Set the saved user ID.
    pub fn set_suid(&self, suid: u32) {
        self.suid.store(suid, Ordering::SeqCst);
    }

    /// Get the saved group ID.
    pub fn sgid(&self) -> u32 {
        self.sgid.load(Ordering::SeqCst)
    }

    /// Set the saved group ID.
    pub fn set_sgid(&self, sgid: u32) {
        self.sgid.store(sgid, Ordering::SeqCst);
    }

    /// Get the supplementary groups.
    pub fn sup_group(&self) -> Arc<Mutex<Vec<u32>>> {
        self.sup_group.clone()
    }

    pub fn set_sup_group(&self, sup_group: Vec<u32>) {
        *self.sup_group.lock() = sup_group;
    }
}
