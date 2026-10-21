//! Sony IMX386 image-sensor driver, ported from the DiCam850 rev4 firmware
//! (`unp_tg_sonyimx386-00.00.06`, code at 0x40217d00..0x40219000).
//!
//! Ported so far: power-on sequence, I2C probe, mode switch with register tables,
//! AFE (exposure, frame length, gain).
//! Not ported yet: power-off sequence, CDSP front-end setup that the firmware does
//! next to the mode switch, the EXPOSURE.BIN index -> lines tables.
#![cfg_attr(not(test), no_std)]
pub mod cfgloader;

pub mod ae;
mod ae_iq;
pub mod afe;
mod afe_tables;
pub mod denoise;
pub mod tables;

/// One register write: 16-bit address, 8-bit value (firmware stores it as 3 bytes, address big-endian).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reg(pub u16, pub u8);

/// `mode_select` register: 0 = standby, 1 = streaming.
pub const MODE_SELECT: u16 = 0x0100;
/// Register the firmware reads to detect the sensor, and the value it expects.
pub const PROBE_REG: u16 = 0x0341;
pub const PROBE_VALUE: u8 = 0x14;
/// I2C addresses tried by the firmware, in order (as printed by its log).
pub const I2C_ADDRS: [u8; 2] = [0x34, 0x20];
const PROBE_TRIES: usize = 3;

/// Sensor control bus (SIF/I2C in the firmware: read 0x402180f4, bulk write 0x406496c4).
pub trait SensorBus {
    type Error;
    fn set_address(&mut self, addr: u8) -> Result<(), Self::Error>;
    fn read(&mut self, reg: u16) -> Result<u8, Self::Error>;
    fn write(&mut self, reg: u16, val: u8) -> Result<(), Self::Error>;
}

/// Board-level controls used by the power sequence.
pub trait Board {
    /// Board power/GPIO step by id (firmware 0x40001de0). Meaning of each id is board-specific.
    fn power_step(&mut self, id: u8);
    /// Sensor master clock on/off (firmware 0x40712f8c(0, on)).
    fn mclk(&mut self, on: bool);
    /// Firmware delay 0x406b96e4(n); the unit is not confirmed yet.
    fn delay(&mut self, n: u32);
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error<E> {
    Bus(E),
    NotFound,
    BadMode(usize),
    BadGain(usize),
}

pub struct Imx386<B> {
    bus: B,
    addr: Option<u8>,
}

impl<B: SensorBus> Imx386<B> {
    pub fn new(bus: B) -> Self {
        Self { bus, addr: None }
    }

    pub fn into_bus(self) -> B {
        self.bus
    }

    /// Detected I2C address, after a successful [`probe`](Self::probe).
    pub fn address(&self) -> Option<u8> {
        self.addr
    }

    /// Power-on sequence, same order and delays as `sony_imx386_PwrOn_Seq` (0x40218bc4).
    pub fn power_on(board: &mut impl Board) {
        for id in [0x74, 0x10, 0x13] {
            board.power_step(id);
        }
        board.delay(1000);
        board.power_step(0x12);
        board.delay(1000);
        board.mclk(true);
        board.power_step(0x11);
        board.delay(1000);
        board.power_step(0x14);
        board.delay(2000);
    }

    /// Find the sensor: 3 reads of PROBE_REG on each address until it returns PROBE_VALUE.
    pub fn probe(&mut self) -> Result<u8, Error<B::Error>> {
        for addr in I2C_ADDRS {
            self.bus.set_address(addr).map_err(Error::Bus)?;
            for _ in 0..PROBE_TRIES {
                if self.bus.read(PROBE_REG).map_err(Error::Bus)? == PROBE_VALUE {
                    self.addr = Some(addr);
                    return Ok(addr);
                }
            }
        }
        Err(Error::NotFound)
    }

    /// Switch to firmware mode `index`: standby, mode table, streaming (0x402181b4).
    pub fn set_mode(&mut self, index: usize) -> Result<(), Error<B::Error>> {
        let (_, table) = tables::MODES.get(index).ok_or(Error::BadMode(index))?;
        self.bus.write(MODE_SELECT, 0).map_err(Error::Bus)?;
        for &Reg(reg, val) in table.iter() {
            self.bus.write(reg, val).map_err(Error::Bus)?;
        }
        self.bus.write(MODE_SELECT, 1).map_err(Error::Bus)
    }

    /// Exposure in sensor lines for the current firmware mode (firmware 0x40218f88).
    pub fn set_exposure(&mut self, mode: usize, lines: u16) -> Result<(), Error<B::Error>> {
        let regs = afe::exposure_regs(mode, lines).ok_or(Error::BadMode(mode))?;
        self.write_all(&regs)
    }

    /// Gain by AGC table index (~2^(index/64) total gain).
    pub fn set_gain(&mut self, index: usize) -> Result<(), Error<B::Error>> {
        let regs = afe::gain_regs(index).ok_or(Error::BadGain(index))?;
        self.write_all(&regs)
    }

    /// Apply one AE program step to the sensor: set gain, then exposure, for `mode`.
    /// This is the AE <-> AFE glue — the program chart's row becomes real register writes.
    pub fn apply_program(&mut self, mode: usize, step: ae::ProgramStep) -> Result<(), Error<B::Error>> {
        let gain = afe::gain_regs(step.gain_index as usize).ok_or(Error::BadGain(step.gain_index as usize))?;
        self.write_all(&gain)?;
        // exposure burst carries the frame length the chart row asked for
        let [a, b] = [Reg(afe::COARSE_INTEGRATION_TIME, (step.exposure_lines >> 8) as u8),
                      Reg(afe::COARSE_INTEGRATION_TIME + 1, step.exposure_lines as u8)];
        let [c, d] = [Reg(afe::FRAME_LENGTH_LINES, (step.frame_len >> 8) as u8),
                      Reg(afe::FRAME_LENGTH_LINES + 1, step.frame_len as u8)];
        if mode >= afe::MODE_FRAME_LEN.len() {
            return Err(Error::BadMode(mode));
        }
        self.write_all(&[a, b, c, d])
    }

    fn write_all(&mut self, regs: &[Reg]) -> Result<(), Error<B::Error>> {
        for &Reg(reg, val) in regs {
            self.bus.write(reg, val).map_err(Error::Bus)?;
        }
        Ok(())
    }
}

/// Output size of a register table (X_OUT/Y_OUT registers 0x034c..0x034f).
pub fn output_size(table: &[Reg]) -> Option<(u16, u16)> {
    let get = |r: u16| table.iter().find(|x| x.0 == r).map(|x| x.1 as u16);
    Some((get(0x034c)? << 8 | get(0x034d)?, get(0x034e)? << 8 | get(0x034f)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MockBus {
        present_at: Option<u8>,
        addr: u8,
        writes: Vec<Reg>,
    }

    impl SensorBus for MockBus {
        type Error = ();
        fn set_address(&mut self, addr: u8) -> Result<(), ()> {
            self.addr = addr;
            Ok(())
        }
        fn read(&mut self, reg: u16) -> Result<u8, ()> {
            Ok(if Some(self.addr) == self.present_at && reg == PROBE_REG { PROBE_VALUE } else { 0 })
        }
        fn write(&mut self, reg: u16, val: u8) -> Result<(), ()> {
            self.writes.push(Reg(reg, val));
            Ok(())
        }
    }

    #[test]
    fn probe_prefers_first_address_then_falls_back() {
        let mut s = Imx386::new(MockBus { present_at: Some(0x34), ..Default::default() });
        assert_eq!(s.probe(), Ok(0x34));
        let mut s = Imx386::new(MockBus { present_at: Some(0x20), ..Default::default() });
        assert_eq!(s.probe(), Ok(0x20));
        let mut s = Imx386::new(MockBus::default());
        assert_eq!(s.probe(), Err(Error::NotFound));
    }

    #[test]
    fn set_mode_brackets_table_with_standby_and_stream() {
        let mut s = Imx386::new(MockBus::default());
        s.set_mode(0).unwrap();
        let w = s.into_bus().writes;
        assert_eq!(w.first(), Some(&Reg(MODE_SELECT, 0)));
        assert_eq!(w.last(), Some(&Reg(MODE_SELECT, 1)));
        assert_eq!(w.len(), tables::MODES[0].1.len() + 2);
    }

    #[test]
    fn mode_sizes_match_firmware() {
        let size = |i: usize| output_size(tables::MODES[i].1).unwrap();
        assert_eq!(size(0), (1920, 1080));
        assert_eq!(size(1), (4032, 2256));
        assert_eq!(size(4), (1280, 720));
        assert_eq!(size(10), (2016, 1508));
        assert_eq!(size(11), (4032, 3016));
        assert!(tables::MODES.iter().all(|(_, t)| t.len() == 59));
    }

    #[test]
    fn exposure_burst_matches_firmware_layout() {
        let mut s = Imx386::new(MockBus::default());
        s.set_exposure(0, 0x1234).unwrap();
        let fl = afe::MODE_FRAME_LEN[0];
        assert_eq!(
            s.into_bus().writes,
            [Reg(0x0202, 0x12), Reg(0x0203, 0x34), Reg(0x0340, (fl >> 8) as u8), Reg(0x0341, fl as u8)]
        );
    }

    #[test]
    fn agc_table_is_64_steps_per_ev_from_1x_to_65x() {
        let first = afe::gain_of(0).unwrap();
        let last = afe::gain_of(afe::AGC.len() - 1).unwrap();
        assert!((first - 1.0).abs() < 0.02);
        assert!((last - 65.4).abs() < 0.5);
        let ev = |i: usize| afe::gain_of(i).unwrap().log2();
        let span = ev(afe::AGC.len() - 1) - ev(0);
        let per_entry = span / (afe::AGC.len() - 1) as f32;
        assert!((per_entry - 1.0 / 64.0).abs() < 0.0005);
        let mut s = Imx386::new(MockBus::default());
        assert_eq!(s.set_gain(afe::AGC.len()), Err(Error::BadGain(afe::AGC.len())));
    }

    #[test]
    fn mode_frame_rates_are_standard() {
        let fps = |m: usize| afe::mode_fps(m).unwrap();
        for (m, want) in [(0, 60.0), (1, 30.0), (3, 120.0), (4, 120.0), (5, 240.0), (6, 60.0), (11, 22.5)] {
            assert!((fps(m) - want).abs() / want < 0.01, "mode {m}: {} fps", fps(m));
        }
    }

    #[test]
    fn bad_mode_is_rejected() {
        let mut s = Imx386::new(MockBus::default());
        assert_eq!(s.set_mode(16), Err(Error::BadMode(16)));
    }
}
