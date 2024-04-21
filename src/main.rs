#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{
    dma::NoDma,
    gpio::{Level, Output, Speed},
    spi::{self, Spi},
    time::Hertz,
};
use embassy_sync::channel::Channel;
use embassy_time::Timer;
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    mono_font::{ascii, MonoTextStyleBuilder},
    pixelcolor::Gray4,
    text::Text,
    Drawable,
};
use icn2037::{ICN2037Device, ICN2037Receiver, ICN2037Sender};
use static_cell::make_static;
use {defmt_rtt as _, panic_probe as _};

// DIN = PB5
// CLK = PB3
// OE = PA7
// LE = PB1

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut config: embassy_stm32::Config = Default::default();
    config.rcc.mux = embassy_stm32::rcc::ClockSrc::PLL(Default::default());
    let p = embassy_stm32::init(config);

    info!("start");

    let oe = Output::new(p.PA7, Level::Low, Speed::VeryHigh);
    let le = Output::new(p.PB1, Level::Low, Speed::VeryHigh);
    let mut spi_config: spi::Config = Default::default();
    spi_config.frequency = Hertz::mhz(16);
    let spi = Spi::new_txonly(p.SPI1, p.PB3, p.PB5, NoDma, NoDma, spi_config);

    let buffer = make_static!([0u16; 25 * 16]);
    let (width, height) = (25, 16);
    let mut icn = icn2037::ICN2037::new(
        spi,
        oe,
        le,
        icn2037::DisplayConfig::new(width, height, |config, x, y| {
            if x >= config.width || y >= config.height {
                return (0, 0);
            }
            let idx = if x == 0 {
                0
            } else {
                (x - 1) / 4 + (y / 4) * 6 + 1
            };
            let offset = if x == 0 { y } else { (x - 1) % 4 + y * 4 };
            let offset = 15 - offset;
            (idx, offset)
        }),
        buffer.as_mut(),
    );

    icn.start().unwrap();

    icn.set_pixel_gray(0, 0, 1);
    let icn_channel = &*make_static!(Channel::new());
    let (tx, rx) = (icn_channel.sender(), icn_channel.receiver());

    let sender = ICN2037Sender {
        config: icn.config.clone(),
        sender: tx,
    };

    spawner.spawn(daemon_task(icn, rx)).unwrap();

    let mut cnt = 0;

    let mut icn = sender;
    loop {
        icn.clear(Default::default()).unwrap();
        Text::with_alignment(
            "Test",
            Point::new(0, cnt),
            MonoTextStyleBuilder::new()
                // .font(&ascii::FONT_5X8)
                .font(&ascii::FONT_6X13_BOLD)
                .text_color(Gray4::new(15))
                .build(),
            embedded_graphics::text::Alignment::Left,
        )
        .draw(&mut icn)
        .unwrap();
        Text::with_alignment(
            "Test",
            Point::new(0, cnt + 16),
            MonoTextStyleBuilder::new()
                // .font(&ascii::FONT_5X8)
                .font(&ascii::FONT_6X13_BOLD)
                .text_color(Gray4::new(1))
                .build(),
            embedded_graphics::text::Alignment::Left,
        )
        .draw(&mut icn)
        .unwrap();
        cnt = cnt + 1;
        if cnt >= 15 {
            cnt = 0;
        }
        Timer::after_millis(100).await;
    }
}

#[embassy_executor::task]
async fn daemon_task(dev: impl ICN2037Device + 'static, receiver: ICN2037Receiver) {
    dev.task(receiver).await.unwrap();
}
