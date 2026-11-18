
//! Timer management for scheduling.
//!
//! Provides periodic timer interrupts for task scheduling.

/// Set the next timer trigger.
pub fn set_next_trigger() {
    #[cfg(target_arch = "riscv64")]
    {
        let timebase = 12500000usize; // QEMU virt machine timebase
        let ticks_per_sec = 100usize;
        let delta = timebase / ticks_per_sec;
        let current = riscv::register::time::read();
        crate::sbi::set_timer(current + delta);
    }
}

/// Check and handle timer events.
pub fn check_timer() {
    // Timer tick handling is done by axtask::on_timer_tick()
    // which is called from the trap handler directly.
}
