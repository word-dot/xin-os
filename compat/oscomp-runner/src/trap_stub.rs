
//! Stub trap module for non-RISC-V architectures.
//! Provides the same public API as the RISC-V trap module, but with stub implementations.

#[repr(C)]
#[derive(Debug)]
pub struct TrapContext {
    pub x: [usize; 32],
    pub sstatus: usize,
    pub sepc: usize,
    pub kernel_satp: usize,
    pub kernel_sp: usize,
    pub trap_handler: usize,
}

impl TrapContext {
    pub fn set_sp(&mut self, sp: usize) {
        self.x[2] = sp;
    }

    pub fn app_init_context(
        entry: usize,
        sp: usize,
        kernel_satp: usize,
        kernel_sp: usize,
        trap_handler: usize,
    ) -> Self {
        Self {
            x: [0; 32],
            sstatus: 0,
            sepc: entry,
            kernel_satp,
            kernel_sp,
            trap_handler,
        }
    }
}

pub fn init() {}

pub fn enable_timer_interrupt() {}

pub fn trap_handler() -> ! {
    panic!("trap_handler not implemented for this architecture");
}

pub fn trap_return() -> ! {
    panic!("trap_return not implemented for this architecture");
}

pub fn trap_from_kernel(_trap_cx: &TrapContext) {}

pub fn map_trampoline() {}

pub fn map_trampoline_to_user(_aspace: &mut axmm::AddrSpace) {}
