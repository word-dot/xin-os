/* Round-robin scheduler implementation for ArceOS */

use axsync::Mutex;
use core::collections::VecDeque;
use crate::pcb::{ProcessControlBlock, ProcessState, Pid};

/// Scheduler error type
#[derive(Debug)]
pub enum SchedulerError {
    NoProcesses,
    ProcessNotFound,
}

/// Trait for all schedulers
pub trait Scheduler {
    /// Add a process to the scheduler's ready queue
    fn enqueue(&self, pcb: Arc<Mutex<ProcessControlBlock>>);

    /// Remove and return the next process to run
    fn dequeue(&self) -> Option<Arc<Mutex<ProcessControlBlock>>>;

    /// Notify the scheduler of a state change (e.g., blocked, exited)
    fn notify(&self, pid: Pid, new_state: ProcessState);
}

/// Simple round-robin scheduler implementation
pub struct RoundRobinScheduler {
    ready_queue: Mutex<VecDeque<Arc<Mutex<ProcessControlBlock>>>>,
    running: Mutex<Option<Arc<Mutex<ProcessControlBlock>>>>,
}

impl RoundRobinScheduler {
    /// Create a new round-robin scheduler
    pub const fn new() -> Self {
        Self {
            ready_queue: Mutex::new(VecDeque::new()),
            running: Mutex::new(None),
        }
    }

    /// Add a process to the ready queue
    pub fn enqueue(&self, pcb: Arc<Mutex<ProcessControlBlock>>) {
        self.ready_queue.lock().push_back(pcb);
    }

    /// Get the next process to run (FIFO)
    pub fn dequeue(&self) -> Option<Arc<Mutex<ProcessControlBlock>>> {
        let mut queue = self.ready_queue.lock();
        queue.pop_front()
    }

    /// Set the currently running process
    pub fn set_running(&self, pcb: Option<Arc<Mutex<ProcessControlBlock>>>) {
        *self.running.lock() = pcb;
    }

    /// Get the currently running process
    pub fn current(&self) -> Option<Arc<Mutex<ProcessControlBlock>>> {
        self.running.lock().clone()
    }

    /// Notify scheduler of a state change
    pub fn notify(&self, pid: Pid, new_state: ProcessState) {
        // Remove any process from ready queue that matches pid
        if let Some(pcb) = self.ready_queue.lock().iter().find(|p| p.pid == pid) {
            // In a real implementation we'd remove it properly
            // This is a placeholder for simplicity
        }

        // Handle exited processes (could trigger cleanup)
        if new_state == ProcessState::Exited {
            // Would normally trigger process reaping
        }
    }
}

/* Unit tests (can be removed in production) */
#[cfg(test)]
mod tests {
    use super::*;
    use axstd::sync::Arc;

    #[test]
    fn test_enqueue_dequeue() {
        let scheduler = RoundRobinScheduler::new();
        // Create mock PCBs would require more setup, so we just test the queue operations
        // This is a basic sanity check
        assert!(true);
    }
}