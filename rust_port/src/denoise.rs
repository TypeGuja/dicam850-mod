//! Video smoothing / noise reduction control, following the firmware's LV-indexed
//! dynamic-IQ approach (`sen pv smooth evset`, `RGB DENOISE`, CDSP dynamic IQ).
//!
//! The sensor gets noisy as light drops and gain rises, so the firmware raises the
//! denoise/"smooth" level in the dark. Too little leaves raw pixel noise; too much
//! smears detail into mush. This models that as a strength curve over light value
//! (milli-EV) with a hard ceiling so it can never over-smooth into mush.

/// Denoise strength the CDSP accepts, 0 (off) .. MAX (strongest).
pub const MAX_STRENGTH: u8 = 255;

/// A denoise curve: strength falls linearly from `dark` (at or below `lv_dark`)
/// to `bright` (at or above `lv_bright`). `ceiling` caps the result to avoid mush.
#[derive(Clone, Copy, Debug)]
pub struct DenoiseCurve {
    pub lv_bright_milli: i32,
    pub lv_dark_milli: i32,
    pub bright: u8,
    pub dark: u8,
    pub ceiling: u8,
}

impl Default for DenoiseCurve {
    fn default() -> Self {
        // Light denoise in daylight, stronger in the dark, but never past the mush ceiling.
        Self {
            lv_bright_milli: 9_000,
            lv_dark_milli: 2_000,
            bright: 24,
            dark: 200,
            ceiling: 210,
        }
    }
}

impl DenoiseCurve {
    /// Denoise strength for a given scene light value (milli-EV).
    pub fn strength(&self, lv_milli: i32) -> u8 {
        let s = if lv_milli >= self.lv_bright_milli {
            self.bright as i32
        } else if lv_milli <= self.lv_dark_milli {
            self.dark as i32
        } else {
            // linear interpolate dark..bright across the LV span
            let span = (self.lv_bright_milli - self.lv_dark_milli).max(1);
            let t = lv_milli - self.lv_dark_milli;
            self.dark as i32 + (self.bright as i32 - self.dark as i32) * t / span
        };
        s.clamp(0, self.ceiling as i32) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stronger_in_the_dark() {
        let c = DenoiseCurve::default();
        assert!(c.strength(0) > c.strength(10_000));
        assert_eq!(c.strength(20_000), c.bright); // bright clamp
        assert_eq!(c.strength(-5_000), c.dark); // dark clamp
    }

    #[test]
    fn monotonic_non_increasing_with_light() {
        let c = DenoiseCurve::default();
        let mut prev = c.strength(-2_000);
        for lv in (-2_000..12_000).step_by(250) {
            let s = c.strength(lv);
            assert!(s <= prev, "lv {lv}: {s} > {prev}");
            prev = s;
        }
    }

    #[test]
    fn never_exceeds_mush_ceiling() {
        let c = DenoiseCurve { dark: 255, ceiling: 180, ..DenoiseCurve::default() };
        assert_eq!(c.strength(-10_000), 180);
    }

    #[test]
    fn midpoint_interpolates() {
        let c = DenoiseCurve {
            lv_bright_milli: 10_000,
            lv_dark_milli: 0,
            bright: 0,
            dark: 200,
            ceiling: 255,
        };
        assert_eq!(c.strength(5_000), 100);
    }
}
