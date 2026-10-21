//! DiCam 850 runtime SD-card config hook (rev4 2023-04-22 firmware).
//!
//! Linked at the free padding 0x4075fd94 (620 B) of the main image (just past the
//! 16-byte __clzsi2 table that ends at 0x4075fd93) and reached by two redirected
//! instructions:
//!   * PUSH @0x40046ce8 (entry of appVideoPvStateInit = entering video preview)
//!     -> `video_wrap` -> `dicam_cfg_apply`: the ONLY file read; applies formats
//!     and fills the per-fps ISO-cap table. A mode switch, not the AE hot path.
//!   * MOV r0,#0 @0x4006b9ec (a video AEPCHART was just read into its buffer, inside
//!     the AE callback appAEPreviewCbCdspSec) -> `chart_thunk` -> `dicam_chart_clamp`:
//!     NO file I/O; trims the chart to the cap for the CURRENT fps. (rtcfg6 did the
//!     SD read here and starved AE at 60/120/240 fps; rtcfg7 flattened gain in place
//!     and broke the short high-fps charts' EV order -> AE stuck.)
//!
//! Uses NO firmware RAM: the file comes from fsSimpleReadEx's heap buffer (freed
//! after parsing); the per-fps cap table lives in our own gap (CAPTAB, flashed 0).
//!
//! `G:\DICAM.CFG` is plain text, one directive per line, counted only at line start
//! (so a leading `#` comments it out):
//!   mode=W,FPS,BITRATE,CROP[,MAXISO]
//!     W,FPS   = which recording slot (3840,30 / 2704,60 / 1920,120 / 1280,240 ...)
//!     BITRATE = Mbit 10..160, 0 = keep
//!     CROP    = aspect x100 (235 = 2.35:1) or explicit height (>=300), 0 = keep
//!     MAXISO  = optional video ISO ceiling 100..6400 for THIS fps, 0 = none.
//!               The AE chart is per-fps, so modes sharing an fps share the cap.
//! No file / bad value -> no-op (stock behaviour).
#![no_std]
#![no_main]

use core::ptr::{read_volatile as rd, write_volatile as wr};

// ---- fixed firmware addresses (rev4) ----
// fsSimpleReadEx(path, &buf, &size) -> 0 ok / 1 failed; buf = malloc(size) (0x40733b50)
const FS_READ_EX: usize = 0x406890e8;
const FREE: usize = 0x40733ca4; // heap free, pairs with 0x40733b50 (as in cmd_cdsp_pv.c)
const CUR_FPS: usize = 0x409acd10; // live video fps (the chart loader reads it here)
const MAX_CFG: u32 = 8192;

// per-fps ISO cap (gain byte, 0 = none), one byte per fps slot, in our own gap.
// Flashed as 0; (re)written only by video_wrap; read by chart_thunk. Isolated to
// our patch region -- a wrong value can only disable our own clamp, never fw RAM.
const CAPTAB: usize = 0x4075fff8; // 2 entries x {u16 fps, u8 gaincap, u8 pad} = 8 B
const NCAP: usize = 2;

const MODE_TABLE: usize = 0x409aa060; // video-mode table, 0x40-byte records
const MODE_STRIDE: usize = 0x40;
const N_MODES: usize = 10;
const OFF_W: usize = 0x00; // u16
const OFF_H: usize = 0x02; // u16
const OFF_FPS: usize = 0x0c; // u32
const OFF_BR: usize = 0x14; // u32 (High-preset Mbit)

const CHART_BUF: usize = 0x40a2de60; // video AEPCHART file buffer (180 B)
const CHART_LEN: usize = 180; // up to 30 rows x 6 B {u8 A, pad, s16 B, u8 gain, pad}

const ISO100_GAIN: u32 = 96; // gain byte of the ISO100 charts
const GAIN_PER_EV: u32 = 16; // assumed chart gain units per stop (same as diprofile.py)

type FsReadEx = extern "C" fn(*const u8, *mut *mut u8, *mut u32) -> i32;
type Free = extern "C" fn(*mut u8);

core::arch::global_asm!(
    ".section .text.entry,\"ax\"",
    ".arm",
    ".global video_wrap",
    "video_wrap:", // replaces the first insn (push) of appVideoPvStateInit
    "push {{r0-r3, r12, lr}}",
    "bl dicam_cfg_apply",
    "pop {{r0-r3, r12, lr}}",
    "push {{r4, r5, r6, r7, r8, lr}}", // displaced original instruction
    "b PV_INIT_BODY",
    ".global chart_thunk",
    "chart_thunk:", // replaces MOV r0,#0 after a chart fileRead, in the AE callback
    "push {{r4, lr}}", // NO file I/O here: just trim the chart to the cached cap
    "bl dicam_chart_clamp",
    "mov r0, #0",
    "pop {{r4, pc}}",
);

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

/// Parse up to 5 unsigned decimals on the line starting near `p` (any non-digit
/// separates them; a newline ends the line). Returns how many were read.
#[inline(never)]
unsafe fn parse_nums(buf: *const u8, n: usize, mut p: usize, out: *mut u32) -> usize {
    let mut cnt = 0usize;
    while p < n && cnt < 5 {
        let c = *buf.add(p);
        if c == b'\n' {
            break;
        }
        if c.is_ascii_digit() {
            let mut v = 0u32;
            while p < n {
                let d = *buf.add(p);
                if !d.is_ascii_digit() {
                    break;
                }
                v = v * 10 + (d - b'0') as u32;
                p += 1;
            }
            *out.add(cnt) = v;
            cnt += 1;
        } else {
            p += 1;
        }
    }
    cnt
}

/// Mode record matching width `w` @ `fps`, or 0. (w,fps) is unique among the 10.
unsafe fn mode_rec(w: u32, fps: u32) -> usize {
    let mut i = 0usize;
    while i < N_MODES {
        let base = MODE_TABLE + i * MODE_STRIDE;
        if rd((base + OFF_W) as *const u16) as u32 == w && rd((base + OFF_FPS) as *const u32) == fps {
            return base;
        }
        i += 1;
    }
    0
}

/// Apply bitrate / crop to one format. Crop never grows the height (no upscale).
#[inline(never)]
unsafe fn apply_mode(w: u32, fps: u32, br: u32, crop: u32) {
    let base = mode_rec(w, fps);
    if base == 0 {
        return;
    }
    if br >= 10 && br <= 160 {
        wr((base + OFF_BR) as *mut u32, br);
    }
    if crop < 100 {
        return; // 0 = keep height
    }
    let h = (if crop < 300 { w * 100 / crop } else { crop }) & !15;
    let cur = rd((base + OFF_H) as *const u16) as u32;
    if h >= 240 && h <= cur {
        wr((base + OFF_H) as *mut u16, h as u16);
    }
}

/// ISO -> chart gain byte: 96 at ISO100, +16 per stop, +8 for a half stop.
fn iso_to_gain(iso: u32) -> u32 {
    let mut g = ISO100_GAIN;
    let mut t = iso;
    while t >= 200 {
        t /= 2;
        g += GAIN_PER_EV;
    }
    if t >= 141 {
        g += GAIN_PER_EV / 2;
    }
    g
}

/// Trim an AE program chart so no kept row exceeds gain `cap`. Rows run
/// darkest-first (highest gain first); the firmware search FUN_4020fb40 reads
/// them as a monotonic EV = A + B - gain curve and clamps the target to the
/// ends. Dropping the leading over-cap rows (and shifting the rest down) keeps
/// that curve valid -- bright scenes are unchanged, dim scenes just stop at the
/// capped gain instead of going brighter. At least one real row is always kept.
#[inline(never)]
unsafe fn trim_chart(c: usize, cap: u8) {
    let mut i = 0usize;
    while i + 6 <= CHART_LEN {
        let g = rd((c + i + 4) as *const u8);
        if rd((c + i) as *const u8) == 0 && g == 0 {
            return; // reached the terminator with no row to keep -> leave as is
        }
        if g <= cap {
            break; // first row within the cap; drop everything before it
        }
        i += 6;
    }
    if i == 0 || i + 6 > CHART_LEN {
        return;
    }
    // shift the kept rows down to the front. The freed tail keeps the original
    // trailing bytes, which are already zero (AE charts are <=9 rows of 180), so
    // the terminator lands correctly with no separate zero-fill.
    let mut j = 0usize;
    while j < CHART_LEN - i {
        wr((c + j) as *mut u8, rd((c + i + j) as *const u8));
        j += 1;
    }
}

unsafe fn cap_for_current_fps() -> u8 {
    let cur = rd(CUR_FPS as *const u32);
    let mut i = 0usize;
    while i < NCAP {
        let e = CAPTAB + i * 4;
        let cap = rd((e + 2) as *const u8);
        if cap != 0 && rd(e as *const u16) as u32 == cur {
            return cap;
        }
        i += 1;
    }
    0
}

/// Re-trim the just-loaded chart to the current fps's cap. Called from the AE
/// callback after every chart reload, so it does NO file I/O and nothing slow.
#[no_mangle]
pub unsafe extern "C" fn dicam_chart_clamp() {
    let cap = cap_for_current_fps();
    if cap >= ISO100_GAIN as u8 {
        trim_chart(CHART_BUF, cap);
    }
}

/// Read G:\DICAM.CFG once, at video-preview entry (a mode switch): apply the
/// formats and refill the per-fps ISO cap table. chart_thunk does the trimming.
#[no_mangle]
pub unsafe extern "C" fn dicam_cfg_apply() {
    let read_ex: FsReadEx = core::mem::transmute::<usize, FsReadEx>(FS_READ_EX);
    let free: Free = core::mem::transmute::<usize, Free>(FREE);
    let mut buf: *mut u8 = core::ptr::null_mut();
    let mut size: u32 = 0;
    // no file? leave the table as it was (a card yanked mid-session keeps the caps)
    if read_ex(b"G:\\DICAM.CFG\0".as_ptr(), &mut buf, &mut size) != 0 || buf.is_null() {
        return;
    }
    // clear the cap table (8 B); a removed/edited line then takes effect
    wr(CAPTAB as *mut u32, 0);
    wr((CAPTAB + 4) as *mut u32, 0);
    let mut ci = 0usize;
    if size <= MAX_CFG {
        let n = size as usize;
        let buf = buf as *const u8;
        let mut vu = core::mem::MaybeUninit::<[u32; 5]>::uninit();
        let vp = vu.as_mut_ptr() as *mut u32;
        let mut i = 0usize;
        while i < n {
            // only "mode=" lines carry numbers we use (key letters are skipped by
            // parse_nums, which reads the decimals that follow)
            if (i == 0 || *buf.add(i - 1) == b'\n') && i + 1 < n
                && *buf.add(i) == b'm' && *buf.add(i + 1) == b'o'
            {
                let c = parse_nums(buf, n, i, vp);
                if c >= 4 {
                    apply_mode(*vp, *vp.add(1), *vp.add(2), *vp.add(3));
                    let iso = *vp.add(4);
                    if c >= 5 && iso >= 100 && iso <= 6400 && ci < NCAP {
                        let e = CAPTAB + ci * 4;
                        wr(e as *mut u16, *vp.add(1) as u16);
                        wr((e + 2) as *mut u8, iso_to_gain(iso) as u8);
                        ci += 1;
                    }
                }
            }
            i += 1;
        }
    }
    free(buf);
    // the chart for this mode is (re)loaded by the AE callback right after, where
    // chart_thunk trims it; nothing to trim here.
}
