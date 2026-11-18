
//! Board-specific definitions and IRQ handling.
//!
//! This module provides the board abstraction layer, including
//! device initialization and interrupt handling.

pub mod qemu;

// Re-export the QEMU board as the default
pub use qemu::*;

/// Board-level IRQ handler.
pub fn irq_handler() {
    qemu::irq_handler();
}
