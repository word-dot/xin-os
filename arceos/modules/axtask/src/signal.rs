
//! Signal management data structures and operations.

use alloc::sync::Arc;
use spin::RwLock;

/// Maximum number of signals (1-64, matching Linux _NSIG).
pub const MAX_SIGNUM: usize = 64;

/// Default signal action types.
pub const SIG_DFL: usize = 0; // Default action
pub const SIG_IGN: usize = 1; // Ignore signal

/// Signal action enumeration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SignalAction {
    /// Default action (depends on signal: term, core, stop, continue, etc.)
    Default,
    /// Ignore the signal
    Ignore,
    /// Custom handler function
    Custom(usize), // Function pointer stored as usize
}

impl Default for SignalAction {
    fn default() -> Self {
        SignalAction::Default
    }
}

/// Signal handler entry: stores the handler address and flags.
#[derive(Debug, Clone, Copy)]
pub struct SigAction {
    /// Handler function pointer (0 = SIG_DFL, 1 = SIG_IGN, otherwise custom address)
    pub sa_handler: usize,
    /// Signal mask during handler execution
    pub sa_mask: SigSet,
    /// Flags (SA_RESTART, SA_SIGINFO, etc.)
    pub sa_flags: u32,
    /// Restorer function pointer
    pub sa_restorer: usize,
}

impl Default for SigAction {
    fn default() -> Self {
        SigAction {
            sa_handler: SIG_DFL,
            sa_mask: SigSet::new(),
            sa_flags: 0,
            sa_restorer: 0,
        }
    }
}

impl SigAction {
    /// Get the action type for this handler entry.
    pub fn action(&self) -> SignalAction {
        match self.sa_handler {
            SIG_DFL => SignalAction::Default,
            SIG_IGN => SignalAction::Ignore,
            addr => SignalAction::Custom(addr),
        }
    }
}

/// A bitset representing a set of signals (signals 1-64).
/// Bit i represents signal number (i+1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SigSet {
    bits: u64,
}

impl SigSet {
    /// Create a new empty signal set.
    pub const fn new() -> Self {
        SigSet { bits: 0 }
    }

    /// Create a SigSet from raw bits.
    pub const fn from_bits(bits: u64) -> Self {
        SigSet { bits }
    }

    /// Add a signal to the set.
    /// Returns false if the signal number is invalid.
    pub fn add(&mut self, sig: i32) -> bool {
        if sig < 1 || sig > MAX_SIGNUM as i32 {
            return false;
        }
        self.bits |= 1u64 << (sig - 1);
        true
    }

    /// Remove a signal from the set.
    /// Returns false if the signal number is invalid.
    pub fn remove(&mut self, sig: i32) -> bool {
        if sig < 1 || sig > MAX_SIGNUM as i32 {
            return false;
        }
        self.bits &= !(1u64 << (sig - 1));
        true
    }

    /// Check if a signal is in the set.
    pub fn contains(&self, sig: i32) -> bool {
        if sig < 1 || sig > MAX_SIGNUM as i32 {
            return false;
        }
        (self.bits & (1u64 << (sig - 1))) != 0
    }

    /// Check if the set is empty.
    pub fn is_empty(&self) -> bool {
        self.bits == 0
    }

    /// Clear all signals from the set.
    pub fn clear(&mut self) {
        self.bits = 0;
    }

    /// Get the raw bits value.
    pub fn bits(&self) -> u64 {
        self.bits
    }

    /// Set the raw bits value.
    pub fn set_bits(&mut self, val: u64) {
        self.bits = val;
    }

    /// Find the lowest-numbered signal in the set.
    /// Returns 0 if the set is empty.
    pub fn first(&self) -> i32 {
        if self.bits == 0 {
            return 0;
        }
        self.bits.trailing_zeros() as i32 + 1
    }

    /// Remove and return the lowest-numbered signal from the set.
    /// Returns 0 if the set is empty.
    pub fn pop_first(&mut self) -> i32 {
        if self.bits == 0 {
            return 0;
        }
        let sig = self.bits.trailing_zeros() as i32 + 1;
        self.bits &= !(1u64 << (sig - 1));
        sig
    }

    /// Compute the intersection of two signal sets.
    pub fn intersect(&self, other: &SigSet) -> SigSet {
        SigSet {
            bits: self.bits & other.bits,
        }
    }

    /// Compute the difference: signals in self but not in other.
    pub fn difference(&self, other: &SigSet) -> SigSet {
        SigSet {
            bits: self.bits & !other.bits,
        }
    }

    /// Compute the union of two signal sets.
    pub fn union(&self, other: &SigSet) -> SigSet {
        SigSet {
            bits: self.bits | other.bits,
        }
    }
}

/// Per-task signal information.
/// This structure is protected by a RwLock when stored in the global signal table.
#[derive(Debug)]
pub struct SignalInfo {
    /// Signal action table (one entry per signal, index = signal_number - 1).
    /// Signals 0 is unused, signals 1-31 are standard, 32-63 are real-time.
    actions: [SigAction; MAX_SIGNUM],

    /// Signals that are pending (sent but not yet delivered).
    pending: SigSet,

    /// Signals that are blocked (masked).
    blocked: SigSet,

    /// Signals that are pending specifically for this task (not shared).
    /// For thread-directed signals.
    task_pending: SigSet,
}

impl Default for SignalInfo {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalInfo {
    /// Create a new SignalInfo with default signal actions.
    pub fn new() -> Self {
        let mut actions = [SigAction::default(); MAX_SIGNUM];

        // Set default ignore for some signals
        // SIGCHLD (17), SIGURG (23), SIGWINCH (28) are ignored by default
        actions[16].sa_handler = SIG_IGN; // SIGCHLD
        actions[22].sa_handler = SIG_IGN; // SIGURG
        actions[27].sa_handler = SIG_IGN; // SIGWINCH

        SignalInfo {
            actions,
            pending: SigSet::new(),
            blocked: SigSet::new(),
            task_pending: SigSet::new(),
        }
    }

    /// Get the signal action for a given signal number.
    pub fn get_action(&self, sig: i32) -> Option<&SigAction> {
        if sig < 1 || sig > MAX_SIGNUM as i32 {
            return None;
        }
        Some(&self.actions[(sig - 1) as usize])
    }

    /// Set the signal action for a given signal number.
    /// Returns the old action, or None if the signal number is invalid.
    /// SIGKILL (9) and SIGSTOP (19) cannot be caught or ignored.
    pub fn set_action(&mut self, sig: i32, action: SigAction) -> Option<SigAction> {
        if sig < 1 || sig > MAX_SIGNUM as i32 {
            return None;
        }
        // SIGKILL and SIGSTOP cannot be caught/ignored
        if sig == 9 || sig == 19 {
            return None;
        }
        let old = self.actions[(sig - 1) as usize];
        self.actions[(sig - 1) as usize] = action;
        Some(old)
    }

    /// Send a signal to this task.
    /// Returns Ok(()) if the signal was queued, or an error if invalid.
    pub fn send_signal(&mut self, sig: i32) -> Result<(), &'static str> {
        if sig < 1 || sig > MAX_SIGNUM as i32 {
            return Err("Invalid signal number");
        }
        self.pending.add(sig);
        Ok(())
    }

    /// Send a thread-directed signal.
    pub fn send_task_signal(&mut self, sig: i32) -> Result<(), &'static str> {
        if sig < 1 || sig > MAX_SIGNUM as i32 {
            return Err("Invalid signal number");
        }
        self.task_pending.add(sig);
        Ok(())
    }

    /// Get the set of pending signals that are not blocked.
    pub fn pending_unblocked(&self) -> SigSet {
        let all_pending = self.pending.union(&self.task_pending);
        all_pending.difference(&self.blocked)
    }

    /// Dequeue the next deliverable signal (pending and not blocked).
    /// Returns 0 if no signal is deliverable.
    pub fn dequeue_signal(&mut self) -> i32 {
        let unblocked = self.pending_unblocked();
        let sig = unblocked.first();
        if sig > 0 {
            // Remove from the appropriate pending set
            self.pending.remove(sig);
            self.task_pending.remove(sig);
        }
        sig
    }

    /// Block signals according to the given set and how flag.
    /// `how`: 0 = SIG_BLOCK, 1 = SIG_UNBLOCK, 2 = SIG_SETMASK
    /// Returns the old blocked set.
    pub fn sigprocmask(&mut self, how: i32, new_set: Option<&SigSet>) -> SigSet {
        let old = self.blocked;

        if let Some(set) = new_set {
            match how {
                0 => {
                    // SIG_BLOCK: add signals to block mask
                    self.blocked = self.blocked.union(set);
                }
                1 => {
                    // SIG_UNBLOCK: remove signals from block mask
                    self.blocked = self.blocked.difference(set);
                }
                2 => {
                    // SIG_SETMASK: set block mask to the given set
                    self.blocked = *set;
                }
                _ => {}
            }
        }

        // SIGKILL and SIGSTOP can never be blocked
        let kill_stop_mask = !((1u64 << 8) | (1u64 << 18)); // Clear bits for SIGKILL(9) and SIGSTOP(19)
        self.blocked.bits &= kill_stop_mask;

        old
    }

    /// Get the current blocked signal set.
    pub fn blocked_set(&self) -> &SigSet {
        &self.blocked
    }

    /// Check if there are any pending unblocked signals.
    pub fn has_pending_signals(&self) -> bool {
        !self.pending_unblocked().is_empty()
    }

    /// Get the pending signal set.
    pub fn pending_set(&self) -> &SigSet {
        &self.pending
    }

    /// Get the task-pending signal set.
    pub fn task_pending_set(&self) -> &SigSet {
        &self.task_pending
    }
}

// Global signal management: send signal to a task by ID.

lazy_static::lazy_static! {
    static ref SIGNAL_TABLE: RwLock<alloc::collections::BTreeMap<u64, Arc<RwLock<SignalInfo>>>> =
        RwLock::new(alloc::collections::BTreeMap::new());
}

/// Register a task's signal info in the global table.
pub fn register_signal_info(tid: u64, info: Arc<RwLock<SignalInfo>>) {
    SIGNAL_TABLE.write().insert(tid, info);
}

/// Unregister a task's signal info from the global table.
pub fn unregister_signal_info(tid: u64) {
    SIGNAL_TABLE.write().remove(&tid);
}

/// Get a task's signal info from the global table.
pub fn get_signal_info(tid: u64) -> Option<Arc<RwLock<SignalInfo>>> {
    SIGNAL_TABLE.read().get(&tid).cloned()
}

/// Send a signal to a task by its ID.
pub fn send_signal_to_task(tid: u64, sig: i32) -> Result<(), &'static str> {
    if let Some(info) = get_signal_info(tid) {
        info.write().send_signal(sig)
    } else {
        Err("Task not found")
    }
}

/// Get the current task's signal info.
/// Returns None if the current task is not registered in the signal table.
pub fn current_signal_info() -> Option<Arc<RwLock<SignalInfo>>> {
    let tid = crate::current().id().as_u64();
    get_signal_info(tid)
}

/// Initialize signal info for the current task.
/// This should be called when a new task is created.
pub fn init_current_signal_info() -> Arc<RwLock<SignalInfo>> {
    let tid = crate::current().id().as_u64();
    let info = Arc::new(RwLock::new(SignalInfo::new()));
    register_signal_info(tid, info.clone());
    info
}
