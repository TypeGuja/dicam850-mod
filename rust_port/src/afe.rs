//! IMX386 AFE: exposure, frame length, gain (`unp_afe_sonyimx386-00.00.01`).
//!
//! Exposure write follows firmware 0x40218f88: one 4-register burst
//! {0x0202, 0x0203, 0x0340, 0x0341} = exposure lines + the mode's frame length.
//! Gain comes from the AGC.BIN table: entry i is ~2^(i/64) total gain,
//! analog first (512 / (512 - code), up to 8x), then digital (8.8 fixed point).

pub use crate::afe_tables::{AGC, MODE_FRAME_LEN};
use crate::{tables, Reg};

pub const COARSE_INTEGRATION_TIME: u16 = 0x0202;
pub const FRAME_LENGTH_LINES: u16 = 0x0340;
pub const ANA_GAIN_GLOBAL: u16 = 0x0204;
pub const DIG_GAIN_GLOBAL: u16 = 0x020e;
/// AGC table resolution.
pub const AGC_STEPS_PER_EV: u32 = 64;

/// External sensor clock and pixel pipelines; with these all 16 firmware modes land
/// within 0.7% of standard frame rates (30/60/120/240).
pub const EXCK_HZ: u32 = 24_000_000;
pub const PIXEL_PIPES: u32 = 4;

fn split(reg: u16, v: u16) -> [Reg; 2] {
    [Reg(reg, (v >> 8) as u8), Reg(reg + 1, v as u8)]
}

/// Register burst for exposure `lines` in firmware mode `mode` (same order as the firmware).
pub fn exposure_regs(mode: usize, lines: u16) -> Option<[Reg; 4]> {
    let fl = *MODE_FRAME_LEN.get(mode)?;
    let [a, b] = split(COARSE_INTEGRATION_TIME, lines);
    let [c, d] = split(FRAME_LENGTH_LINES, fl);
    Some([a, b, c, d])
}

/// Register burst for AGC table `index`: analog gain then digital gain.
pub fn gain_regs(index: usize) -> Option<[Reg; 4]> {
    let &(digital, analog) = AGC.get(index)?;
    let [a, b] = split(ANA_GAIN_GLOBAL, analog);
    let [c, d] = split(DIG_GAIN_GLOBAL, digital);
    Some([a, b, c, d])
}

/// Total gain of an AGC entry (analog * digital).
pub fn gain_of(index: usize) -> Option<f32> {
    let &(digital, analog) = AGC.get(index)?;
    Some(digital as f32 / 256.0 * 512.0 / (512 - analog) as f32)
}

/// Sensor frame rate of a firmware mode, computed from its PLL and timing registers.
pub fn mode_fps(mode: usize) -> Option<f32> {
    let (_, table) = tables::MODES.get(mode)?;
    let get = |r: u16| table.iter().find(|x| x.0 == r).map(|x| x.1 as u32);
    let mpy = get(0x0306)? << 8 | get(0x0307)?;
    let line_len = get(0x0342)? << 8 | get(0x0343)?;
    let pixel_rate = EXCK_HZ as f32 / get(0x0305)? as f32 * mpy as f32
        / (get(0x0301)? * get(0x0303)?) as f32
        * PIXEL_PIPES as f32;
    Some(pixel_rate / (*MODE_FRAME_LEN.get(mode)? as f32 * line_len as f32))
}

/// Duration of one sensor line in microseconds for a mode (exposure lines -> time).
pub fn line_time_us(mode: usize) -> Option<f32> {
    Some(1_000_000.0 / mode_fps(mode)? / *MODE_FRAME_LEN.get(mode)? as f32)
}
