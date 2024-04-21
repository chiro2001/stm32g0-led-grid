#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{
    dma::NoDma,
    gpio::{Input, Level, Output, Speed},
    spi::{self, Spi},
    time::Hertz,
};
use embassy_sync::channel::Channel;
use embassy_time::{Delay, Timer};
use embedded_graphics::draw_target::DrawTarget;
use embedded_hal::digital::{InputPin, OutputPin};
use icn2037::{ICN2037Device, ICN2037Receiver, ICN2037Sender};
use lifegame::LifeGame;
use rand::SeedableRng;
use rand_xorshift::XorShiftRng;
use static_cell::make_static;
use {defmt_rtt as _, panic_probe as _};

mod lifegame;
mod patterns;

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

    let mut icn = sender;
    icn.clear(Default::default()).unwrap();

    icn.sender
        .send(icn2037::ICN2037Message::SetBrightness(4))
        .await;

    let mut adc = embassy_stm32::adc::Adc::new(p.ADC1, &mut Delay);
    let mut adc_pin = p.PA0;
    let mut adc_results = [0u8; 16];
    for i in 0..64 {
        adc_results[i % 16] ^= ((adc.read(&mut adc_pin) + i as u16) % 254) as u8;
        Timer::after_millis(1).await;
    }
    defmt::info!("noise: {=[u8]:02x}", adc_results);

    let rng = XorShiftRng::from_seed(adc_results);
    let mut game = Game::new(
        icn.clone(),
        16 * 500,
        rng,
        Input::new(p.PA10, embassy_stm32::gpio::Pull::Up),
        Output::new(p.PA9, Level::Low, Speed::Low),
        Input::new(p.PA12, embassy_stm32::gpio::Pull::Up),
        Output::new(p.PA11, Level::Low, Speed::Low),
    );
    game.run().await;
    info!("Fin.");
}

#[embassy_executor::task]
async fn daemon_task(dev: impl ICN2037Device + 'static, receiver: ICN2037Receiver) {
    dev.task(receiver).await.unwrap();
}

pub struct Game<A1, A2, B1, B2> {
    game: LifeGame<25, 16, XorShiftRng>,
    key_a: A1,
    _key_a_out: A2,
    key_b: B1,
    _key_b_out: B2,
}

impl<A1, A2, B1, B2> Game<A1, A2, B1, B2>
where
    A1: InputPin,
    A2: OutputPin,
    B1: InputPin,
    B2: OutputPin,
{
    pub fn new(
        icn: ICN2037Sender,
        fade_time_ms: u64,
        rng: XorShiftRng,
        key_a: A1,
        mut key_a_out: A2,
        key_b: B1,
        mut key_b_out: B2,
    ) -> Self {
        let game = LifeGame::<25, 16, _>::new(icn, fade_time_ms, rng);
        key_a_out.set_low().unwrap();
        key_b_out.set_low().unwrap();
        Self {
            game,
            key_a,
            _key_a_out: key_a_out,
            key_b,
            _key_b_out: key_b_out,
        }
    }

    pub async fn read_keys(&mut self) -> (bool, bool) {
        // fix jitter
        let mut a = false;
        let mut b = false;
        if self.key_a.is_low().unwrap() {
            Timer::after_millis(10).await;
            if self.key_a.is_low().unwrap() {
                a = true;
            }
        }
        if self.key_b.is_low().unwrap() {
            Timer::after_millis(10).await;
            if self.key_b.is_low().unwrap() {
                b = true;
            }
        }
        (a, b)
    }

    pub async fn run(&mut self) {
        self.game.randomly_arrange_patterns();
        self.game.draw(true).await;
        loop {
            self.game.draw(false).await;
            let (key_a, key_b) = self.read_keys().await;
            if (key_a || key_b) || self.game.is_still() {
                // break;
                info!("re-generate");
                self.game.randomly_arrange_patterns();
            }
            self.game.step_apply();
            self.game.step();
            Timer::after_millis(1).await;
        }
    }
}
