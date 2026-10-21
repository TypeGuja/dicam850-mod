# DiCam 850 firmware mod

**English** | [Русский](README.ru.md)

Firmware mods for the **Digma DiCam 850** action camera (iCatch SPCA6500 / V35 SoC, Sony IMX386 sensor, SP5K framework).

This repository contains **only tools and patches**. The firmware itself (`SPHOST.BRN`) belongs to Digma/iCatch
and is not included — the patchers are applied to your own stock firmware.

> ⚠️ Flashing a modified image can brick the camera: the only way to flash it back is through the
> camera's menu, which requires the stock firmware to boot. Keep a backup and use at your own risk.
> Tested on HW4.0 (2023-08-23).

## Features

- **Runtime config from the SD card** (`runtime/`): when entering video mode the camera reads `G:\DICAM.CFG`
  and applies, per mode, its own bitrate (up to 160 Mbit/s), crop (2.35:1, 1.85:1 or a height)
  and an ISO ceiling per fps. No file — stock behavior.
- **Smooth auto-exposure**: an `AECONFIG.BIN` edit (ConvergeSpeed/Limit) — no abrupt brightness jumps.
- **JSON profiles** (`profiles/`): bitrate and formats are baked into the firmware at build time.

## Contents

| Folder | What |
|---|---|
| `brntool/` | Rust: parse/unpack/replace files/patch by VA/recompute checksum of a `SUNP BURN FILE` |
| `runtime/hook/` | Rust `no_std` (Thumb, Cortex-A7): hook code, linked straight into free space in the image @`0x4075fd94` |
| `runtime/build_rtcfg.py` | injects `hook.bin` + 2 hooks + lifts the VBRC cap, verifies every byte |
| `runtime/emu_test.py` | runs the patched image in unicorn and catches any write to foreign memory |
| `runtime/DICAM.CFG` | example config for the card |
| `profiles/` | `diprofile.py` + example JSON |
| `firmware/` | analysis scripts: checksum, `RO_RES` FAT volume, headers, xref/disassembly |
| `probe/` | PTP/IP camera probe over Wi-Fi (read-only) |
| `rust_port/` | `imx386` crate: sensor driver and AE in Rust (rewrite stage, host tests) |

## Building

```sh
cd brntool && cargo build --release
rustup target add armv7a-none-eabihf
cd runtime/hook && cargo build --release
# objcopy -O binary target/armv7a-none-eabihf/release/dicam_hook ../hook.bin
python -I runtime/build_rtcfg.py <base.BRN> <out.BRN>
python -I runtime/emu_test.py <dir with unicorn> <out.BRN>
```

Flashing: put the result in the SD card root as `SPHOST.BRN` → camera menu → firmware update.
BRN checksum: sum of all LE u32 words ≡ 0 mod 2³², compensation word @`0x1fc`.

## Pitfalls we already hit

- Zeros right after a function can be the tail of its table (`__clzsi2` @`0x4075fd84`) — rtcfg2 broke the image.
- The "empty" 16 KB @`0x409ffa83` is the power manager's table and the SRAM save buffer.
- `fsSimpleRead` returns 0 = OK, not a byte count.
- Reading a file from SD inside the AE callback starves the frame at 60/120/240 fps; editing gain in place
  breaks the monotonic order of the short AEPCHARTs — so rtcfg8 trims table rows instead of overwriting them.
