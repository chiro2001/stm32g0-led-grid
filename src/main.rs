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
use embassy_sync::{
    blocking_mutex::raw::NoopRawMutex,
    channel::{Channel, Receiver},
};
use embassy_time::{Delay, Timer};
use embedded_graphics::{
    geometry::Point,
    mono_font::{ascii, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    text::Text,
    Drawable,
};
use embedded_hal::delay::DelayNs;
use icn2037::{ICN2037Device, ICN2037Message};
use static_cell::make_static;
use {defmt_rtt as _, panic_probe as _};

// DIN = PB5
// CLK = PB3
// OE = PA7
// LE = PB1

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());

    // let mut delay = Delay {};

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
    spawner.spawn(daemon_task(icn, rx)).unwrap();

    let mut cnt = 0u8;

    // for k in 0..25 {
    //     for y in 0..16 {
    //         let c = y as u8;
    //         tx.send(ICN2037Message::SetPixel((k, y, c))).await;
    //         tx.send(ICN2037Message::SetPixel((k, y, c))).await;
    //     }
    // }
    // loop {
    //     for y in 0..16 {
    //         for k in 0..25 {
    //             let c = (y as u8).max(cnt) - cnt;
    //             tx.send(ICN2037Message::SetPixel((k, y, c))).await;
    //             tx.send(ICN2037Message::SetPixel((k, y, c))).await;
    //         }
    //     }
    //     cnt += 1;
    //     if cnt >= 16 {
    //         cnt = 0;
    //     }
    //     Timer::after_millis(10).await;
    //     info!("cnt = {}", cnt);
    // }

    let mut d = true;
    loop {
        if d {
            cnt += 1;
        } else {
            cnt -= 1;
        }
        if cnt == 0 {
            d = true;
        }
        if cnt == 15 {
            d = false;
        }
        tx.send(ICN2037Message::SetPixel((0, 0, cnt))).await;
        Timer::after_millis(100).await;
    }

    // loop {
    //     icn.clear();
    //     Text::with_alignment(
    //         "Test",
    //         Point::new(0, cnt),
    //         MonoTextStyleBuilder::new()
    //             // .font(&ascii::FONT_5X8)
    //             .font(&ascii::FONT_6X13_BOLD)
    //             .text_color(BinaryColor::On)
    //             .build(),
    //         embedded_graphics::text::Alignment::Left,
    //     )
    //     .draw(&mut icn)
    //     .unwrap();
    //     Text::with_alignment(
    //         "Test",
    //         Point::new(0, cnt + 16),
    //         MonoTextStyleBuilder::new()
    //             // .font(&ascii::FONT_5X8)
    //             .font(&ascii::FONT_6X13_BOLD)
    //             .text_color(BinaryColor::On)
    //             .build(),
    //         embedded_graphics::text::Alignment::Left,
    //     )
    //     .draw(&mut icn)
    //     .unwrap();
    //     icn.flush().unwrap();
    //     cnt = cnt + 1;
    //     if cnt >= 15 {
    //         cnt = 0;
    //     }
    //     delay.delay_ms(200);
    // }
}

#[embassy_executor::task]
async fn daemon_task(
    dev: impl ICN2037Device + 'static,
    receiver: Receiver<'static, NoopRawMutex, ICN2037Message, 32>,
) {
    dev.task(receiver).await.unwrap();
}
