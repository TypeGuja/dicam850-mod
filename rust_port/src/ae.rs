//! Auto-exposure engine, ported from the DiCam850 rev4 firmware
//! (`app_auto_alg.c`, main loop 0x4021275c, called pieces around 0x4020f600..0x40213c00).
//!
//! The firmware runs this per preview frame: it gets per-zone colour sums from the
//! ISP (16x16 zones), turns them into a weighted scene luma, compares that against a
//! light-value-dependent target, applies backlight compensation, then limits how fast
//! the exposure may move before handing an exposure/gain index to the program chart.
//!
//! Ported here as pure functions over the firmware IQ tables so each step can be checked
//! on the host. Not ported: the ISP stats plumbing, the LV/BV estimation that needs the
//! sensor's current exposure, and the HDR/WDR path (separate module).

use crate::ae_iq::{AE_PCHART_60HZ30F, AE_TARGET};

/// 16x16 = 256 metering zones.
pub const ZONES: usize = 256;
/// Fixed-point scale the firmware uses for log2 values (milli-EV, i.e. log2*1000).
pub const MILLI: i32 = 1000;
/// Reference luma the firmware subtracts (log2(0x2e00) point); 0x2e00 = 11776.
pub const LUMA_REF: u32 = 0x2e00;

/// Weighted mean luma over the zones, matching the firmware's inner loop.
///
/// `rgb[i]` is the per-zone (R, G, B) sum from the ISP; `weight[i]` the metering weight.
/// Luma = (299*R + 587*G + 114*B) / 1000, accumulated as `sum(luma*w) / sum(w)`.
pub fn weighted_luma(rgb: &[(u16, u16, u16); ZONES], weight: &[u8; ZONES]) -> u32 {
    let mut acc: u64 = 0;
    let mut wsum: u64 = 0;
    for i in 0..ZONES {
        let (r, g, b) = rgb[i];
        let y = (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000;
        acc += y as u64 * weight[i] as u64;
        wsum += weight[i] as u64;
    }
    if wsum == 0 {
        0
    } else {
        (acc / wsum) as u32
    }
}

/// log2(x) in milli-EV (log2*1000), matching firmware fixLog2+div64 usage. x<2 -> 0.
pub fn log2_milli(x: u32) -> i32 {
    if x < 2 {
        0
    } else {
        (libm_log2(x as f64) * MILLI as f64) as i32
    }
}

// Small, dependency-free log2 for no_std; accurate enough for the milli-EV domain.
fn libm_log2(x: f64) -> f64 {
    // ln via range reduction; log2 = ln(x)/ln(2).
    let mut e = 0i32;
    let mut m = x;
    while m >= 2.0 {
        m /= 2.0;
        e += 1;
    }
    while m < 1.0 {
        m *= 2.0;
        e -= 1;
    }
    // ln(m), m in [1,2): atanh series around (m-1)/(m+1).
    let t = (m - 1.0) / (m + 1.0);
    let t2 = t * t;
    let mut term = t;
    let mut sum = 0.0;
    let mut k = 1.0;
    while term.abs() > 1e-12 {
        sum += term / k;
        term *= t2;
        k += 2.0;
    }
    let ln_m = 2.0 * sum;
    e as f64 + ln_m / core::f64::consts::LN_2
}

/// Exposure error in milli-EV: how far current luma is from the LV target.
/// Positive = scene is brighter than target (should reduce exposure).
pub fn exposure_error(luma: u32, target: u16) -> i32 {
    log2_milli(luma) - log2_milli(target as u32)
}

/// Target luma for a light-value index, interpolated like the firmware (0x11 slots, milli steps).
pub fn target_for_lv(lv_milli: i32) -> u16 {
    if lv_milli < 0 {
        return AE_TARGET[0];
    }
    let idx = (lv_milli / MILLI) as usize;
    if idx >= AE_TARGET.len() - 1 {
        return *AE_TARGET.last().unwrap();
    }
    let a = AE_TARGET[idx] as i32;
    let b = AE_TARGET[idx + 1] as i32;
    let frac = lv_milli - (idx as i32 * MILLI);
    (a + (b - a) * frac / MILLI) as u16
}

/// Program-chart step: exposure lines, frame length, gain index for an exposure position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgramStep {
    pub exposure_lines: u16,
    pub frame_len: u16,
    pub gain_index: u16,
}

/// Look up a program-chart row (clamped to the table).
pub fn program_step(row: usize) -> ProgramStep {
    let t = AE_PCHART_60HZ30F;
    let (e, f, g) = t[row.min(t.len() - 1)];
    ProgramStep { exposure_lines: e, frame_len: f, gain_index: g }
}

/// Convergence limiter: clamp a requested milli-EV move by the configured speed/slope/limit.
///
/// Mirrors FUN_40210b50: when the move exceeds `slope` it is scaled down toward `speed`
/// and hard-capped at `limit` (all in the firmware's fixed-point units).
pub fn limit_step(request_milli: i32, speed: i16, slope: i16, limit: i16) -> i32 {
    let slope = if slope == 0 { 1 } else { slope } as i32;
    let speed = speed as i32;
    let limit = limit as i32;
    if request_milli == 0 {
        return 0;
    }
    let step = if request_milli.abs() > slope {
        (request_milli / slope) * speed + if request_milli > 0 { speed } else { -speed }
    } else if request_milli > 0 {
        1
    } else {
        -1
    };
    step.clamp(-limit, limit)
}

/// Estimate scene light value (milli-EV) from the current exposure and measured luma.
///
/// Mirrors the firmware's `iVar8` in the main loop: it combines how much exposure is
/// currently applied (longer exposure / more gain = darker scene) with how far the
/// measured luma sits from the reference point. Exposure is expressed in EV:
/// `log2(exposure_lines)` plus the AGC index (1/64 EV per step), and luma as
/// `log2(luma) - log2(ref)`. More exposure for the same luma => lower LV.
pub fn estimate_lv_milli(exposure_lines: u16, gain_index: usize, luma: u32) -> i32 {
    // measured_luma ~ scene_luminance * exposure * gain, so scene LV grows with luma
    // and shrinks with the exposure and gain the camera had to apply.
    let exposure_ev = log2_milli(exposure_lines.max(1) as u32);
    let gain_ev = (gain_index as i32 * MILLI) / crate::afe::AGC_STEPS_PER_EV as i32;
    let luma_ev = log2_milli(luma) - log2_milli(LUMA_REF);
    // Reference exposure the firmware factors out (0x50 lines, in EV).
    let ref_ev = log2_milli(0x50);
    luma_ev - exposure_ev - gain_ev + ref_ev
}

/// Per-zone luma (BT.601), used by the highlight metering.
pub fn zone_luma(rgb: &[(u16, u16, u16); ZONES]) -> [u32; ZONES] {
    let mut out = [0u32; ZONES];
    for i in 0..ZONES {
        let (r, g, b) = rgb[i];
        out[i] = (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000;
    }
    out
}

/// Mean luma of the brightest `fraction_pct`% of zones (highlight metering).
pub fn bright_mean(luma: &[u32; ZONES], fraction_pct: u8) -> u32 {
    let mut v = *luma;
    v.sort_unstable_by(|a, b| b.cmp(a)); // descending
    let n = ((ZONES * fraction_pct as usize) / 100).max(1);
    let sum: u64 = v[..n].iter().map(|&x| x as u64).sum();
    (sum / n as u64) as u32
}

/// Tunables for highlight protection. Added by us; not in the stock firmware AE.
#[derive(Clone, Copy, Debug)]
pub struct HighlightGuard {
    /// Enable the rule.
    pub enabled: bool,
    /// Fraction of brightest zones to watch (e.g. 10 = top 10%).
    pub fraction_pct: u8,
    /// Keep that bright mean at or below this luma (clip target).
    pub clip_target: u16,
}

impl Default for HighlightGuard {
    fn default() -> Self {
        // Mirror of BLComp's one-sided design, pointed the other way.
        Self { enabled: false, fraction_pct: 10, clip_target: 0x3800 }
    }
}

impl HighlightGuard {
    /// Extra downward exposure bias in milli-EV (<= 0). Only ever *reduces* exposure,
    /// so it cannot fight the stock backlight compensation that only raises it.
    pub fn bias_milli(&self, luma: &[u32; ZONES]) -> i32 {
        if !self.enabled {
            return 0;
        }
        let bright = bright_mean(luma, self.fraction_pct);
        if bright <= self.clip_target as u32 {
            return 0;
        }
        // How many stops the highlights are over the clip target -> pull down by that.
        log2_milli(self.clip_target as u32) - log2_milli(bright)
    }
}

/// Everything one AE iteration needs that we have ported so far.
#[derive(Clone, Copy, Debug)]
pub struct AeConfig {
    /// LV estimate in milli-EV, picks the target from AETARGET.
    pub lv_milli: i32,
    /// User EV bias in milli-EV (AECONFIG EV-bias path), added to the error.
    pub ev_bias_milli: i32,
    /// Convergence speed / slope / limit (AECONFIG fields 10/12/14).
    pub speed: i16,
    pub slope: i16,
    pub limit: i16,
    pub highlight: HighlightGuard,
}

/// Result of one AE step: how far to move (milli-EV, after limiting) and the chart row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AeStep {
    pub target: u16,
    pub error_milli: i32,
    pub limited_milli: i32,
    pub program_row: usize,
}

/// One AE iteration: zone stats + current chart row -> next move and chart row.
///
/// Combines the ported pieces: weighted luma, LV target, user bias, our highlight
/// guard, then the firmware convergence limiter. `row` is the current program-chart row.
pub fn step(
    rgb: &[(u16, u16, u16); ZONES],
    weight: &[u8; ZONES],
    row: usize,
    cfg: &AeConfig,
) -> AeStep {
    let luma = weighted_luma(rgb, weight);
    let target = target_for_lv(cfg.lv_milli);
    let per_zone = zone_luma(rgb);
    // Positive error = too bright. Highlight guard only adds positive error.
    let error = exposure_error(luma, target) + cfg.ev_bias_milli - cfg.highlight.bias_milli(&per_zone);
    let limited = limit_step(error, cfg.speed, cfg.slope, cfg.limit);
    // Higher chart row = less exposure; move the row toward resolving the error.
    let last = AE_PCHART_60HZ30F.len() - 1;
    let program_row = if limited > 0 {
        (row + 1).min(last)
    } else if limited < 0 {
        row.saturating_sub(1)
    } else {
        row
    };
    AeStep { target, error_milli: error, limited_milli: limited, program_row }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(y: u16) -> [(u16, u16, u16); ZONES] {
        // grey so that luma == y
        [(y, y, y); ZONES]
    }

    #[test]
    fn luma_matches_bt601_weights() {
        let mut rgb = [(0u16, 0, 0); ZONES];
        let w = [1u8; ZONES];
        for z in rgb.iter_mut() {
            *z = (1000, 2000, 3000);
        }
        // (299000 + 1174000 + 342000)/1000 = 1815
        assert_eq!(weighted_luma(&rgb, &w), 1815);
    }

    #[test]
    fn luma_respects_zero_weight_zones() {
        let mut rgb = [(100u16, 100, 100); ZONES];
        rgb[0] = (60000, 60000, 60000);
        let mut w = [0u8; ZONES];
        w[0] = 1; // only the bright zone counts
        assert_eq!(weighted_luma(&rgb, &w), 60000);
    }

    #[test]
    fn log2_milli_is_accurate() {
        assert_eq!(log2_milli(1), 0);
        assert!((log2_milli(1024) - 10_000).abs() <= 1); // log2(1024)=10
        assert!((log2_milli(LUMA_REF) - 13_524).abs() <= 2); // log2(11776)=13.524
    }

    #[test]
    fn exposure_error_sign() {
        assert!(exposure_error(16000, 8000) > 0); // too bright -> positive
        assert!(exposure_error(4000, 8000) < 0); // too dark -> negative
        assert_eq!(exposure_error(8000, 8000), 0);
    }

    #[test]
    fn target_interpolates_and_clamps() {
        assert_eq!(target_for_lv(-5), AE_TARGET[0]);
        assert_eq!(target_for_lv(0), AE_TARGET[0]);
        assert_eq!(target_for_lv(1000), AE_TARGET[1]);
        assert_eq!(target_for_lv(500), (AE_TARGET[0] + AE_TARGET[1]) / 2);
        assert_eq!(target_for_lv(999_000), *AE_TARGET.last().unwrap());
    }

    #[test]
    fn program_chart_clamps() {
        assert_eq!(program_step(0), ProgramStep { exposure_lines: 48, frame_len: 79, gain_index: 168 });
        assert_eq!(program_step(999), program_step(AE_PCHART_60HZ30F.len() - 1));
    }

    #[test]
    fn limiter_caps_and_steps() {
        // small move -> single tick
        assert_eq!(limit_step(5, 8, 128, 120), 1);
        assert_eq!(limit_step(-5, 8, 128, 120), -1);
        // large move -> scaled, then clamped to limit
        assert_eq!(limit_step(100_000, 8, 128, 120), 120);
        assert_eq!(limit_step(-100_000, 8, 128, 120), -120);
        assert_eq!(limit_step(0, 8, 128, 120), 0);
    }

    #[test]
    fn bright_mean_uses_only_the_brightest() {
        let mut luma = [1000u32; ZONES];
        for z in luma.iter_mut().take(26) {
            *z = 60000; // ~10% of 256 zones are blown
        }
        assert_eq!(bright_mean(&luma, 10), 60000);
        // the dark majority barely moves the top-10% mean
        assert!(bright_mean(&luma, 10) > 55000);
    }

    #[test]
    fn highlight_guard_pulls_down_only_when_clipping() {
        let mut g = HighlightGuard { enabled: true, fraction_pct: 10, clip_target: 0x3800 };
        // scene with blown highlights
        let mut luma = [2000u32; ZONES];
        for z in luma.iter_mut().take(26) {
            *z = 60000;
        }
        assert!(g.bias_milli(&luma) < 0, "should request less exposure");
        // no clipping -> no bias
        let dim = [1000u32; ZONES];
        assert_eq!(g.bias_milli(&dim), 0);
        // disabled -> never acts
        g.enabled = false;
        assert_eq!(g.bias_milli(&luma), 0);
    }

    #[test]
    fn step_without_guard_matches_plain_error() {
        let cfg = AeConfig {
            lv_milli: 1000,
            ev_bias_milli: 0,
            speed: 8,
            slope: 128,
            limit: 120,
            highlight: HighlightGuard::default(), // disabled
        };
        let rgb = flat(16000);
        let w = [1u8; ZONES];
        let s = step(&rgb, &w, 3, &cfg);
        assert_eq!(s.target, AE_TARGET[1]);
        assert_eq!(s.error_milli, exposure_error(16000, AE_TARGET[1]));
        // brighter than target -> move toward less exposure (higher row)
        assert!(s.limited_milli > 0 && s.program_row == 4);
    }

    #[test]
    fn lv_rises_with_light_and_falls_with_exposure() {
        // same exposure, brighter scene -> higher LV
        assert!(estimate_lv_milli(100, 0, 20000) > estimate_lv_milli(100, 0, 5000));
        // same luma, more gain (darker scene needs it) -> lower LV
        assert!(estimate_lv_milli(100, 0, 8000) > estimate_lv_milli(100, 64, 8000));
        // same luma, longer exposure -> lower LV
        assert!(estimate_lv_milli(100, 0, 8000) > estimate_lv_milli(400, 0, 8000));
        // 64 gain steps == 1 EV == doubling exposure lines, within rounding
        let a = estimate_lv_milli(200, 0, 8000);
        let b = estimate_lv_milli(100, 64, 8000);
        assert!((a - b).abs() <= 2);
    }

    #[test]
    fn step_guard_adds_downward_bias_on_blown_highlights() {
        let base = AeConfig {
            lv_milli: 5000,
            ev_bias_milli: 0,
            speed: 8,
            slope: 128,
            limit: 120,
            highlight: HighlightGuard { enabled: false, fraction_pct: 10, clip_target: 0x3800 },
        };
        // scene whose average is on target but highlights are blown
        let target = target_for_lv(5000) as u16;
        let mut rgb = flat(target);
        for z in rgb.iter_mut().take(26) {
            *z = (60000, 60000, 60000);
        }
        let w = [1u8; ZONES];
        let without = step(&rgb, &w, 3, &base);
        let mut on = base;
        on.highlight.enabled = true;
        let with = step(&rgb, &w, 3, &on);
        // guard makes the error more positive (push exposure down)
        assert!(with.error_milli > without.error_milli);
    }
}
