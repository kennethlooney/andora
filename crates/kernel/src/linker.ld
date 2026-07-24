ENTRY(kernel_entry)

SECTIONS {
    . = 32M;
    __kernel_start = .;

    .text : ALIGN(4K) {
        *(.text.kernel_entry)
        *(.text .text.*)
    }
    .rodata : ALIGN(4K) { *(.rodata .rodata.*) }
    .data : ALIGN(4K) { *(.data .data.*) }
    .bss : ALIGN(4K) {
        *(.bss .bss.*)
        *(COMMON)
    }

    __kernel_end = ALIGN(4K);
    /DISCARD/ : { *(.eh_frame*) *(.comment*) }
}