
//! QEMU virt machine board definitions for RISC-V.
//!
//! Provides memory map constants and interrupt handling for the
//! QEMU virt platform.

/// Clock frequency for the QEMU virt machine.
pub const CLOCK_FREQ: usize = 12500000;

/// End of physical memory.
pub const MEMORY_END: usize = 0x8800_0000;

/// MMIO device ranges.
pub const MMIO: &[(usize, usize)] = &[
    (0x0010_0000, 0x00_2000), // VIRT_TEST/RTC
    (0x0200_0000, 0x1_0000),  // CLINT
    (0x0C00_0000, 0x21_0000), // PLIC
    (0x1000_0000, 0x9000),    // UART0
];

/// PLIC base address.
pub const VIRT_PLIC: usize = 0xC00_0000;

/// UART base address.
pub const VIRT_UART: usize = 0x1000_0000;

/// Initialize board devices.
pub fn device_init() {
    // Device initialization is handled by ArceOS's axhal/axdriver
    // We don't need to reinitialize here since we're running on top of ArceOS.
}

/// Handle external interrupts from the PLIC.
pub fn irq_handler() {
    // In the ArceOS-based oscomp-runner, external interrupts
    // are handled by the ArceOS driver framework.
    // This is a fallback for any unhandled interrupts.
    axlog::trace!("Board IRQ handler called (unhandled)");
}
