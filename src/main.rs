#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_time::{Delay, Timer};
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

    loop {
        icn.clear();
        // for x in 0..25 {
        // for y in 0..16 {
        //     icn.set_pixel(y, y, true);
        // }
        // }

        for y in 0..16 {
            icn.set_pixel(y, y, true);
            icn.flush().unwrap();
            // Timer::after_millis(500).await;
        }
        // for k in 0..25 {
        //     icn.clear();
        //     for i in 0..16 {
        //         icn.buffer[k] |= 1 << i;
        //         icn.flush().unwrap();
        //         Timer::after_millis(100).await;
        //     }
        //     Timer::after_millis(500).await;
        // }
        icn.flush().unwrap();
        Timer::after_millis(1000).await;
    }
}
