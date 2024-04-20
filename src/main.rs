#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_time::Delay;
use embedded_graphics::{
    geometry::Point,
    mono_font::{ascii, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    text::Text,
    Drawable,
};
use embedded_hal::delay::DelayNs;
use {defmt_rtt as _, panic_probe as _};

// DIN = PB5
// CLK = PB3
// OE = PA7
// LE = PB1

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());

    let mut delay = Delay {};

    info!("start");

    let din = Output::new(p.PB5, Level::Low, Speed::VeryHigh);
    let clk = Output::new(p.PB3, Level::Low, Speed::VeryHigh);
    let oe = Output::new(p.PA7, Level::Low, Speed::VeryHigh);
    let le = Output::new(p.PB1, Level::Low, Speed::VeryHigh);

    let mut buffer = [0u16; 25];
    let (width, height) = (25, 16);
    let mut icn = icn2037::ICN2037::new(
        din,
        clk,
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
        &mut buffer,
    );

    icn.start().unwrap();

    let mut cnt = 0;

    loop {
        icn.clear();
        Text::with_alignment(
            "Test",
            Point::new(0, cnt),
            MonoTextStyleBuilder::new()
                // .font(&ascii::FONT_5X8)
                .font(&ascii::FONT_6X13_BOLD)
                .text_color(BinaryColor::On)
                .build(),
            embedded_graphics::text::Alignment::Left,
        )
        .draw(&mut icn)
        .unwrap();
        icn.flush().unwrap();
        cnt = cnt + 1;
        if cnt >= 16 + 6 {
            cnt = 0;
        }
        delay.delay_ms(0);
    }
}
