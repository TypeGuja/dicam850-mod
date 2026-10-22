# DiCam 850 firmware mod

**English** | [Русский](README.ru.md)

Firmware mods for the **Digma DiCam 850** action camera (iCatch SPCA6500 / V35 SoC, Sony IMX386 sensor, SP5K framework).

This repository contains **only tools and patches**. The firmware itself (`SPHOST.BRN`) belongs to Digma/iCatch
and is not included — the patchers are applied to your own stock firmware.

> ⚠️ Flashing a modified image can brick the camera. The only proven way to flash it back is through the
> camera's menu, which requires the stock firmware to boot (a possible bootloader-level recovery path has
> been found but not yet verified — see below). Use at your own risk. Tested on HW4.0 (2023-08-23).

## Features

- **Runtime config from the SD card** (`runtime/`, current version **rtcfg9**): when entering video mode
  the camera reads `G:\DICAM.CFG` and applies, per mode, its own bitrate (up to 160 Mbit/s), crop
  (2.35:1, 1.85:1 or a height) and an ISO ceiling per fps. No file — stock behavior.
- **Smooth auto-exposure**: an `AECONFIG.BIN` edit (ConvergeSpeed/Limit) — no abrupt brightness jumps.
- **JSON profiles** (`profiles/`): bitrate and formats are baked into the firmware at build time.

## `DICAM.CFG`

One line per mode; only lines starting with `mode=` count:

```
mode=WIDTH,FPS,BITRATE,CROP[,MAXISO]
mode=3840,30,90,235          ← 4K30, 90 Mbit/s, 2.35:1 crop
mode=1920,60,40,0            ← 1080p60, 40 Mbit/s, no crop
```

- `BITRATE` — Mbit/s 10..160, `0` = keep. Above ~80 needs a V30/U3 card.
- `CROP` — aspect ×100 (`235`, `185`) or a frame height; `0` = keep. Crop only, never a stretch.
- `MAXISO` — optional ISO ceiling for this **fps** (shared by all modes with the same fps).
- **Put `#` comments on their own lines.** rtcfg8 read the digits of a trailing comment
  (`mode=1920,60,40,0  # 1080p 60`) as MAXISO=1080 → dark video indoors. rtcfg9 ignores everything after `#`.

### About ISO

- ISO is **signal gain**, not sensor sensitivity: each doubling = +1 stop brighter and +1 stop more noise.
  Video exposure can't be longer than 1/fps, so at 60/120/240 fps the camera reaches for gain sooner
  (120 fps ≈ 2 stops more gain than 30 fps in the same light).
- The photo menu has an ISO item (photo only). The video menu has no ISO — only `MAXISO`.
- In the exposure tables (AEPCHART) ISO is a gain byte. Assumed (not HW-verified) scale: `96 = ISO100`,
  +16 per stop (`144 = ISO800`, `160 = ISO1600`). Stock AUTO charts have exactly **one** dark high-gain row
  (160 ≈ ISO1600; 120 for 60 Hz/120 fps), the rest are 80–112.
- **Current limitation:** rtcfg8/9 drop rows above the cap, so any `MAXISO` below ~1600 effectively means
  ISO100 (dark indoors). Better not to set it for now. Plan: set the dark row's gain to the cap instead
  (which also allows **raising** ISO above stock — the sensor AGC goes up to ×65, stock uses ×5.6).
- In practice: no cap in daylight; indoors/at night — no cap and 30 fps instead of 60/120;
  a cap gives a cleaner but darker image, never a brighter one.

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

## `SPHOST.BRN` layout (rev4, ≈15.5 MB)

| Offset | Size | What |
|---|---|---|
| `0x000000–0x0a7000` | ~668 KB | `SUNP BURN FILE` header + **second-stage bootloader** (`$unp_0.7.14_sysboot`, "iCatch sd/spi loader II"), DRAM params (`DRAMPARA`), `PHYTABLE` |
| `0x0a7000–0x0a8120` | | `SUNP BURN HDR 1/2` (HDR2 = disk-image table, XOR 0x7a) |
| `0x0a8120–0x4a8120` | 4 MB | Disk `A:` — `RO_RES` resources (menus, fonts, sounds, IQ tables, `DIQ.FIF`) |
| `0x4b8120–0xebf320` | ~10.5 MB | Main firmware "DRAM CODE", loaded at VA `0x40000000` |
| last `0x250` | | `SUNP BURN HDR 3` — per-unit calibration file list (`BADPIX`, `DEFPIX`, …) |

### Bootloader and recovery without opening the camera (found, NOT verified)

The bootloader is separate from the main firmware and can flash on its own. From its strings:

- **Card ISP bypassing the main firmware:** `Button pressed`, `skip load FW`, `card file %s`,
  `D:\firmware.brn`, `cardISP progress`, `FW boot error %x` — likely flashes `firmware.brn` from the card
  when a button is held at power-on and/or when the main firmware fails to boot.
- **Two firmware slots:** `update/keep original Firmware1/Firmware2`, `Boot from priority %d`,
  `RawCBoot … chksum NG` — a fallback to the second slot may exist.
- **USB ISP:** vendor requests `0x76–0x79`, `0xFD` ("iCatch ISP Version").

If confirmed, this is a recovery path with no programmer. Requirement: every build must keep the bootloader
region byte-identical to stock. Next step: reverse the trigger condition (button / `D:\firmware.brn`) and
prove it with **stock** firmware.

### What's in the main firmware

ThreadX RTOS + ~28,000 functions (~5.4 MB code) and ~5 MB data. About a third of the code is identified (lower bounds):

| Module | ≥ |
|---|---|
| Sensor and image pipeline (IMX386, CDSP, AE/AWB, WDR/HDR, LSC) | 512 KB |
| Wi-Fi Realtek RTL8188 | 254 KB |
| Video: H.264, VBRC, MOV recording | 242 KB |
| Display, menus, OSD | 194 KB |
| FS, SD, flash | 149 KB |
| Network: TCP/IP, HTTP, RTSP, PTP/IP | 133 KB |
| App: modes, buttons, power | 128 KB |
| OS kernel, memory, libc | 85 KB |
| USB (MSC, UVC, PTP) | 69 KB |
| Audio (AAC/WAV) | 36 KB |
| Gyro, stabilization | 25 KB |

DDR/PLL are brought up by the bootloader, so our own image doesn't need to. Without public docs the CDSP
(image pipeline), the H.264 codec and Wi-Fi are best kept stock and called as libraries; everything else
(app, menus, AE, settings, recording, USB, network) can move to Rust.

## Sharpness and noise reduction (research, paused)

A daylight clip (1920×816@60, 40 Mbit/s): bitrate and exposure are fine; the image suffers from
**over-sharpening halos** on contrasty edges and NR "watercolor" on foliage.

- Dynamic IQ blocks `.257` from `DIQ.FIF` are applied by `0x40629c34`: sections
  `{u32 size, u16 type, u16 count}`, records `{u16 len=0xc, u8 kind, pad, u32 reg, u32 val}`.
- Every value goes through two call sites: `BL 0x4060a32c` @`0x40629c94` and @`0x40629d84`
  (`r1` = register, `r2` = value). Type-4 calls (`0x40629cd4`, `0x40629de8`) pass a pointer — don't touch.
- Candidates (the CDSP register map is under NDA): `0x07170202/0211/0220` — likely sharpening/edges,
  `0x07170271`, `0x0717028d` — NR thresholds, `0x071c014b`.
- Plan: hook both sites + `iq=REG,PERCENT` in `DICAM.CFG`. **Blocker — space:** the hook uses 609/612 B and
  there is no other free space in the code (the zeros in the image header @`0x40000070` are risky).
  Space will come from replacing ISO row-trimming with a single-row gain write.

## Pitfalls we already hit

- Zeros right after a function can be the tail of its table (`__clzsi2` @`0x4075fd84`) — rtcfg2 broke the image.
- The "empty" 16 KB @`0x409ffa83` is the power manager's table and the SRAM save buffer.
- `fsSimpleRead` returns 0 = OK, not a byte count.
- Reading a file from SD inside the AE callback starves the frame at 60/120/240 fps; editing gain in place
  breaks the monotonic order of the short AEPCHARTs — so rtcfg8 trims table rows instead of overwriting them.
- rtcfg8's parser read the digits of a comment after `#` as MAXISO — fixed in rtcfg9 (test `[5b]` in `emu_test.py`).
- A function with no BL/literal references isn't necessarily dead: the "dead" `0x405d78c4` has inbound branches.
