#![no_std]

use embedded_hal::{delay::DelayNs, digital::OutputPin};

#[derive(Debug)]
pub enum Error {
    PinError,
}

pub struct DisplayConfig {
    pub width: usize,
    pub height: usize,
    pub map_pixel: fn(&DisplayConfig, usize, usize) -> (usize, usize),
}
impl DisplayConfig {
    pub fn new(
        width: usize,
        height: usize,
        map_pixel: fn(&DisplayConfig, usize, usize) -> (usize, usize),
    ) -> Self {
        Self {
            width,
            height,
            map_pixel,
        }
    }
}

pub struct ICN2037<'d, DIN, CLK, OE, LE, DELAY> {
    din: DIN,
    clk: CLK,
    oe: OE,
    le: LE,
    delay: DELAY,
    config: DisplayConfig,
    pub buffer: &'d mut [u16],
}

const DELAY_US: u32 = 1;

impl<'d, DIN, CLK, OE, LE, DELAY> ICN2037<'d, DIN, CLK, OE, LE, DELAY>
where
    DIN: OutputPin,
    CLK: OutputPin,
    OE: OutputPin,
    LE: OutputPin,
    DELAY: DelayNs,
{
    pub fn new(
        din: DIN,
        clk: CLK,
        oe: OE,
        le: LE,
        delay: DELAY,
        config: DisplayConfig,
        buffer: &'d mut [u16],
    ) -> Self {
        Self {
            din,
            clk,
            oe,
            le,
            delay,
            config,
            buffer,
        }
    }

    pub fn start(&mut self) -> Result<(), Error> {
        self.oe.set_high().map_err(|_| Error::PinError)?;
        self.le.set_low().map_err(|_| Error::PinError)?;
        Ok(())
    }

    pub fn write_16b(&mut self, data: u16) -> Result<(), Error> {
        // use msb
        for i in (0..16).rev() {
            self.din
                .set_state(if data & (1 << i) != 0 {
                    embedded_hal::digital::PinState::High
                } else {
                    embedded_hal::digital::PinState::Low
                })
                .map_err(|_| Error::PinError)?;
            self.clk.set_high().map_err(|_| Error::PinError)?;
            self.delay.delay_us(DELAY_US);
            self.clk.set_low().map_err(|_| Error::PinError)?;
            self.delay.delay_us(DELAY_US);
        }
        // latch
        self.delay.delay_us(DELAY_US);
        self.le.set_high().map_err(|_| Error::PinError)?;
        self.delay.delay_us(DELAY_US);
        self.le.set_low().map_err(|_| Error::PinError)?;
        self.delay.delay_us(DELAY_US);
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), Error> {
        self.oe.set_high().map_err(|_| Error::PinError)?;
        self.delay.delay_us(DELAY_US);

        // for i in 0..(self.buffer.len()).min(self.config.width * self.config.height / 8 / 2) {
        //     self.write_16b(self.buffer[i])?;
        // }
        for i in 0..self.buffer.len() {
            self.write_16b(self.buffer[i])?;
        }

        self.oe.set_low().map_err(|_| Error::PinError)?;
        self.delay.delay_us(DELAY_US);
        Ok(())
    }

    pub fn clear(&mut self) {
        self.buffer.iter_mut().for_each(|x| *x = 0);
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, value: bool) {
        let (idx, offset) = (self.config.map_pixel)(&self.config, x, y);
        self.buffer[idx] = (self.buffer[idx] & !(1 << offset)) | ((value as u16) << offset);
    }
}
