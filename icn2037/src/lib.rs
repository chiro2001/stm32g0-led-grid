#![no_std]

use core::future::Future;

use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::{Receiver, Sender};
use embassy_time::Timer;
use embedded_graphics_core::geometry::Dimensions;
use embedded_graphics_core::pixelcolor::IntoStorage;
use embedded_hal::digital::OutputPin;
use embedded_hal::spi::SpiBus;

#[derive(Debug)]
pub enum Error {
    PinError,
    BusError,
    DispError,
    BufferError,
}

#[derive(Debug, Clone)]
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
    pub config: DisplayConfig,
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

    pub async fn task_impl(mut self, receiver: ICN2037Receiver) -> Result<(), Error> {
        loop {
            let msg = receiver.try_receive();
            match msg {
                Ok(msg) => match msg {
                    ICN2037Message::SetPixel((x, y, v)) => self.set_pixel_gray(x, y, v),
                    ICN2037Message::FillPixels((sx, sy, ex, ey, v)) => {
                        for x in sx..ex {
                            for y in sy..ey {
                                self.set_pixel_gray(x, y, v);
                            }
                        }
                    }
                    ICN2037Message::Clear => self.buffer.iter_mut().for_each(|x| *x = 0),
                },
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

pub const BUFFER_SZ: usize = 1024;
pub type ICN2037Receiver = Receiver<'static, NoopRawMutex, ICN2037Message, BUFFER_SZ>;
pub struct ICN2037Sender {
    pub config: DisplayConfig,
    pub sender: Sender<'static, NoopRawMutex, ICN2037Message, BUFFER_SZ>,
}

impl embedded_graphics_core::geometry::OriginDimensions for ICN2037Sender {
    fn size(&self) -> embedded_graphics_core::prelude::Size {
        embedded_graphics_core::prelude::Size::new(
            self.config.width as u32,
            self.config.height as u32,
        )
    }
}

impl embedded_graphics_core::draw_target::DrawTarget for ICN2037Sender {
    type Color = embedded_graphics_core::pixelcolor::Gray4;

    type Error = Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = embedded_graphics_core::prelude::Pixel<Self::Color>>,
    {
        let iter = pixels.into_iter();
        for pixel in iter {
            let msg = ICN2037Message::SetPixel((
                pixel.0.x as usize,
                pixel.0.y as usize,
                pixel.1.into_storage() as u8,
            ));
            let r = self.sender.try_send(msg);
            match r {
                Ok(_) => {}
                Err(e) => match e {
                    embassy_sync::channel::TrySendError::Full(_) => {
                        defmt::warn!("full buffer! {}", e);
                    }
                },
            }
        }
        Ok(())
    }
    fn fill_solid(
        &mut self,
        area: &embedded_graphics_core::primitives::Rectangle,
        color: Self::Color,
    ) -> Result<(), Self::Error> {
        let (sx, sy, ex, ey) = (
            area.top_left.x as usize,
            area.top_left.y as usize,
            area.bottom_right().unwrap().x as usize,
            area.bottom_right().unwrap().y as usize,
        );
        let msg = ICN2037Message::FillPixels((sx, sy, ex, ey, color.into_storage()));
        self.sender.try_send(msg).unwrap();
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        if color.into_storage() == 0 {
            self.sender.try_send(ICN2037Message::Clear).unwrap();
            Ok(())
        } else {
            self.fill_solid(&self.bounding_box(), color)
        }
    }
}

pub trait ICN2037Device {
    fn task(self, receiver: ICN2037Receiver) -> impl Future<Output = Result<(), Error>>;
}
impl<'d, SPI, OE, LE> ICN2037Device for ICN2037<'d, SPI, OE, LE>
where
    SPI: SpiBus,
    OE: OutputPin,
    LE: OutputPin,
{
    fn task(self, receiver: ICN2037Receiver) -> impl Future<Output = Result<(), Error>> {
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

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ICN2037Message {
    SetPixel((usize, usize, u8)),
    FillPixels((usize, usize, usize, usize, u8)),
    Clear,
}
