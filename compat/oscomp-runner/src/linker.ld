OUTPUT_ARCH(riscv)
ENTRY(_start)
BASE_ADDRESS = 0x800000;

SECTIONS
{
    . = BASE_ADDRESS;
    skernel = .;

    stext = .;
    .text : {
        *(.text.entry)
        *(.text .text.*)
    }

    . = ALIGN(4K);
    etext = .;

    srodata = .;
    .rodata : {
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    }

    . = ALIGN(4K);
    erodata = .;

    sdata = .;
    .data : {
        *(.data .data.*)
        *(.sdata .sdata.*)
    }

    . = ALIGN(4K);
    edata = .;

    /* ========== ???????.bss ?¦Æ??? ========== */
    .bss : {
        /* sbss_with_stack ??????? .bss ?????????????????? */
        sbss_with_stack = .;
        /* ?????????????????? */
        *(.bss.stack)
        /* sbss ?????? .bss ????? */
        sbss = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        ebss = .;
    }
    ebss_with_stack = ebss;

    . = ALIGN(4K);
    ekernel = .;

    /* ========== ???????trampoline ?? ========== */
    .trampoline : {
        strampoline = .;
        *(.trampoline .trampoline.*)
        . = ALIGN(4K);
        etrampoline = .;
    }

    /DISCARD/ : {
        *(.eh_frame)
    }
}
