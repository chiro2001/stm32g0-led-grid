#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_time::{Delay, Timer};
use embedded_graphics::{
    geometry::{Point, Size},
    mono_font::{ascii, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    primitives::{Primitive, PrimitiveStyle, Rectangle},
    text::{Text, TextStyleBuilder},
    Drawable,
};
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
        &mut delay,
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
        // Rectangle::new(Point::zero(), Size::new(16, 16))
        //     .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        //     .draw(&mut icn)
        //     .unwrap();
        // icn.set_pixel(0, 0, true);
        icn.flush().unwrap();
        // Timer::after_millis(1000).await;

        cnt = cnt + 1;
        if cnt >= 16 + 6 {
            cnt = 0;
        }
        // Timer::after_millis(10).await;
    }
}
