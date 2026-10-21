/* Linked at the free padding of rev4 main image (0x4075fd94..0x40760000, 620 B).
   0x4075fd84..0x4075fd93 is the 16-byte lookup table of __clzsi2 (last 8 bytes are
   zeros that look free but are NOT) -- never start lower. 0x40760000 = .ARM.exidx. */
ENTRY(video_wrap)

PV_INIT_BODY = 0x40046cec;         /* appVideoPvStateInit + 4, after its displaced push */
MEMORY { GAP (rx) : ORIGIN = 0x4075fd94, LENGTH = 612 }
SECTIONS
{
  .text : {
    KEEP(*(.text.entry))
    *(.text .text.*)
    *(.rodata .rodata.*)
  } > GAP
  .data : { *(.data .data.*) } > GAP
  .bss  : { *(.bss .bss.*) *(COMMON) } > GAP
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) *(.comment) }
}
ASSERT(SIZEOF(.data) == 0 && SIZEOF(.bss) == 0, "hook must not have .data/.bss")
