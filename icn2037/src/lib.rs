#![no_std]

use core::future::Future;

use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Receiver;
use embassy_time::Timer;
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::SpiBus;

#[derive(Debug)]
pub enum Error {
    PinError,
    BusError,
    DispError,
    BufferError,
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

pub struct ICN2037<'d, SPI, OE, LE> {
    spi: SPI,
    oe: OE,
    le: LE,
    config: DisplayConfig,
    pub buffer: &'d mut [u16],
}

impl<'d, SPI, OE, LE> ICN2037<'d, SPI, OE, LE>
where
    SPI: SpiBus,
    OE: OutputPin,
    LE: OutputPin,
{
    pub fn new(spi: SPI, oe: OE, le: LE, config: DisplayConfig, buffer: &'d mut [u16]) -> Self {
        Self {
            spi,
            oe,
            le,
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
        let buf = [(data >> 8) as u8, (data & 0xff) as u8];
        self.spi.write(&buf).map_err(|_| Error::BusError)?;
        // latch
        self.le.set_high().map_err(|_| Error::PinError)?;
        self.le.set_low().map_err(|_| Error::PinError)?;
        Ok(())
    }

    pub fn frame_buffer_len(&self) -> usize {
        self.config.width * self.config.height / 16
    }

    pub fn flush(&mut self, buffer_offset: usize) -> Result<(), Error> {
        let len = self.buffer.len().min(self.frame_buffer_len());
        for i in buffer_offset..(len + buffer_offset) {
            self.write_16b(self.buffer[i])?;
        }
        self.oe.set_high().map_err(|_| Error::PinError)?;
        self.oe.set_low().map_err(|_| Error::PinError)?;
        Ok(())
    }

    pub fn clear(&mut self) {
        self.buffer.iter_mut().for_each(|x| *x = 0);
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, value: bool, buffer_offset: usize) {
        if x >= self.config.width || y >= self.config.height {
            return;
        }
        let (idx, offset) = (self.config.map_pixel)(&self.config, x, y);
        let b = &mut self.buffer[idx + buffer_offset];
        *b = (*b & !(1 << offset)) | ((value as u16) << offset);
    }

    pub fn set_pixel_gray(&mut self, x: usize, y: usize, value: u8) {
        let sz = self.frame_buffer_len();
        for k in 0..16 {
            self.set_pixel(x, y, LUT16[value as usize][k] != 0, k * sz);
        }
    }

    pub async fn task_impl(
        mut self,
        receiver: Receiver<'static, NoopRawMutex, ICN2037Message, 32>,
    ) -> Result<(), Error> {
        loop {
            let msg = receiver.try_receive();
            match msg {
                Ok(msg) => {
                    match msg {
                        ICN2037Message::SetPixel((x, y, v)) => self.set_pixel_gray(x, y, v),
                    }
                    // Timer::after_ticks(0).await;
                }
                Err(_) => {
                    // normal display for one frame
                    let frame_sz = self.frame_buffer_len();
                    for k in 0..16 {
                        self.flush(k * frame_sz)?;
                    }
                    Timer::after_ticks(0).await;
                }
            }
        }
    }
}

impl<'d, SPI, OE, LE> embedded_graphics_core::geometry::OriginDimensions
    for ICN2037<'d, SPI, OE, LE>
{
    fn size(&self) -> embedded_graphics_core::prelude::Size {
        embedded_graphics_core::prelude::Size::new(
            self.config.width as u32,
            self.config.height as u32,
        )
    }
}

impl<'d, SPI, OE, LE> embedded_graphics_core::draw_target::DrawTarget for ICN2037<'d, SPI, OE, LE>
where
    SPI: SpiBus,
    OE: OutputPin,
    LE: OutputPin,
{
    type Color = embedded_graphics_core::pixelcolor::BinaryColor;

    type Error = Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = embedded_graphics_core::prelude::Pixel<Self::Color>>,
    {
        let iter = pixels.into_iter();
        for pixel in iter {
            self.set_pixel(pixel.0.x as usize, pixel.0.y as usize, pixel.1.is_on(), 0);
        }
        Ok(())
    }
}

pub trait ICN2037Device {
    fn task(
        self,
        receiver: Receiver<'static, NoopRawMutex, ICN2037Message, 32>,
    ) -> impl Future<Output = Result<(), Error>>;
}
impl<'d, SPI, OE, LE> ICN2037Device for ICN2037<'d, SPI, OE, LE>
where
    SPI: SpiBus,
    OE: OutputPin,
    LE: OutputPin,
{
    fn task(
        self,
        receiver: Receiver<'static, NoopRawMutex, ICN2037Message, 32>,
    ) -> impl Future<Output = Result<(), Error>> {
        self.task_impl(receiver)
    }
}

const LUT16: [[u8; 16]; 16] = [
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], // 0
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], // 1
    [1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0], // 2
    [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0], // 3
    [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0], // 4
    [1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0], // 5
    [1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0, 1, 0], // 6
    [1, 0, 1, 0, 1, 0, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0], // 7
    [1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0], // 8
    [1, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0], // 9
    [1, 1, 1, 0, 1, 0, 1, 0, 1, 1, 1, 0, 1, 0, 1, 0], // 10
    [1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 0, 1, 0], // 11
    [1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0], // 12
    [1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 0], // 13
    [1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1], // 14
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1], // 15
];

pub enum ICN2037Message {
    SetPixel((usize, usize, u8)),
}
