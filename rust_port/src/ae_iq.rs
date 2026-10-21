//! AE IQ tables from DiCam850 rev4 RO_RES/IQ (generated).

/// AETARGET.BIN: target luma per LV index (18 x u16).
pub const AE_TARGET: [u16; 18] = [4000, 4280, 4480, 5120, 5760, 6400, 7040, 7680, 8000, 8320, 8640, 8960, 8960, 8960, 8960, 8960, 8960, 8960];

/// AEPCHART (program line) entry: (exposure_lines, frame_len, gain_index) triples.
pub const AE_PCHART_60HZ30F: [(u16,u16,u16); 7] = [
    (48, 79, 168),
    (48, 79, 80),
    (48, 95, 96),
    (48, 95, 80),
    (48, 111, 96),
    (48, 111, 80),
    (48, 207, 80),
];
