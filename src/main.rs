#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{
    dma::NoDma,
    flash::Flash,
    gpio::{Input, Level, Output, Speed},
    spi::{self, Spi},
    time::Hertz,
};
use embassy_sync::{
    blocking_mutex::raw::NoopRawMutex,
    channel::{Channel, Receiver, Sender},
};
use embassy_time::{Delay, Timer};
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    mono_font::{ascii, MonoTextStyleBuilder},
    pixelcolor::Gray4,
    text::Text,
    Drawable,
};
use embedded_hal::{
    delay::DelayNs,
    digital::{InputPin, OutputPin},
};
use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};
use futures::Future;
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

    // icn.sender
    //     .send(icn2037::ICN2037Message::SetBrightness(4))
    //     .await;

    let mut adc = embassy_stm32::adc::Adc::new(p.ADC1, &mut Delay);
    let mut adc_pin = p.PA0;
    let mut adc_results = [0u8; 16];
    for i in 0..64 {
        adc_results[i % 16] ^= ((adc.read(&mut adc_pin) + i as u16) % 254) as u8;
        Timer::after_millis(1).await;
    }
    defmt::info!("noise: {=[u8]:02x}", adc_results);

    let keys = make_static!(KeysDriver::new(
        Input::new(p.PA10, embassy_stm32::gpio::Pull::Up),
        Output::new(p.PA9, Level::Low, Speed::Low),
        Input::new(p.PA12, embassy_stm32::gpio::Pull::Up),
        Output::new(p.PA11, Level::Low, Speed::Low),
    ));
    let keys_channel = &*make_static!(Channel::new());
    let (tx, rx) = (keys_channel.sender(), keys_channel.receiver());
    spawner.spawn(keys_task(keys, tx)).unwrap();

    // Timer::after_millis(1500).await;

    let addr: u32 = STATE_ADDR;
    let mut flash = Flash::new_blocking(p.FLASH)
        .into_blocking_regions()
        .bank1_region;
    let mut magic_buf = [0u8; 8];
    flash.blocking_read(addr, &mut magic_buf).unwrap();
    let magic = u64::from_le_bytes(magic_buf);
    defmt::info!("magic: 0x{:x}", magic);
    // let mut state = State::default_with_flash(flash);
    // Timer::after_millis(1500).await;
    let mut state = if magic == STATE_MAGIC {
        let mut state_buf = [0u8; STATE_SIZE];
        flash.blocking_read(addr, &mut state_buf).unwrap();
        let mut s: State<_> = unsafe { core::mem::transmute_copy(&state_buf) };
        s.flash.replace(flash);
        s
    } else {
        defmt::warn!("no state found, use default");
        State::default_with_flash(flash)
        // State::<()>::default()
    };
    info!("state version: {}", state.version());

    Text::with_alignment(
        state.version(),
        Point::new(0, 15),
        MonoTextStyleBuilder::new()
            .text_color(Gray4::new(15))
            .font(&ascii::FONT_4X6)
            .build(),
        embedded_graphics::text::Alignment::Left,
    )
    .draw(&mut icn)
    .unwrap();
    Timer::after_millis(5000).await;

    state.save();

    // let state = State::<()>::default();

    let rng = XorShiftRng::from_seed(adc_results);
    let mut game = Game::new(icn.clone(), rx, 16 * 20, rng, state);
    game.run().await;
    info!("Fin.");
}

#[embassy_executor::task]
async fn daemon_task(dev: impl ICN2037Device + 'static, receiver: ICN2037Receiver) {
    dev.task(receiver).await.unwrap();
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Key {
    A,
    B,
}
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum KeyEvent {
    Pressed(Key),
    Released(Key),
}

pub type KeysSender = Sender<'static, NoopRawMutex, KeyEvent, 16>;
pub type KeysReceiver = Receiver<'static, NoopRawMutex, KeyEvent, 16>;

pub struct KeysDriver<A1, A2, B1, B2> {
    key_a: A1,
    _key_a_out: A2,
    key_b: B1,
    _key_b_out: B2,
}
impl<A1, A2, B1, B2> KeysDriver<A1, A2, B1, B2>
where
    A1: InputPin,
    A2: OutputPin,
    B1: InputPin,
    B2: OutputPin,
{
    pub fn new(key_a: A1, mut key_a_out: A2, key_b: B1, mut key_b_out: B2) -> Self {
        key_a_out.set_low().unwrap();
        key_b_out.set_low().unwrap();
        Self {
            key_a,
            _key_a_out: key_a_out,
            key_b,
            _key_b_out: key_b_out,
        }
    }
}
pub trait KeysDevice {
    fn read_keys(&mut self) -> impl Future<Output = (bool, bool)>;
}
impl<A1, A2, B1, B2> KeysDevice for &mut KeysDriver<A1, A2, B1, B2>
where
    A1: InputPin,
    B1: InputPin,
{
    async fn read_keys(&mut self) -> (bool, bool) {
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
}
#[embassy_executor::task]
async fn keys_task(mut keys: impl KeysDevice + 'static, sender: KeysSender) {
    let mut a_last = false;
    let mut b_last = false;

    loop {
        let (a, b) = keys.read_keys().await;
        if a != a_last {
            if a {
                sender.send(KeyEvent::Pressed(Key::A)).await;
            } else {
                sender.send(KeyEvent::Released(Key::A)).await;
            }
            a_last = a;
        }
        if b != b_last {
            if b {
                sender.send(KeyEvent::Pressed(Key::B)).await;
            } else {
                sender.send(KeyEvent::Released(Key::B)).await;
            }
            b_last = b;
        }
        Timer::after_millis(10).await;
    }
}

#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Page {
    #[default]
    Game,
    Light,
}

const STATE_ADDR: u32 = 1024 * 110;
const STATE_MAGIC: u64 = 0x1145141919810;
const STATE_SIZE: usize = 512;
const VERSION: &str = build_info::format!("v{}-{}", $.crate_info.version, $.version_control.unwrap().git().unwrap().commit_short_id);
#[repr(C)]
#[repr(align(1))]
pub struct State<F> {
    magic: u64,
    version: [u8; 64],
    page: Page,
    game_brightness: u8,
    light_brightness: u8,
    pub flash: Option<F>,
}
impl<F> Default for State<F> {
    fn default() -> Self {
        let mut version = [b'\0'; 64];
        version
            .iter_mut()
            .zip(VERSION.as_bytes())
            .for_each(|(a, b)| *a = *b);
        Self {
            magic: STATE_MAGIC,
            version,
            page: Default::default(),
            game_brightness: 15,
            light_brightness: 15,
            flash: None,
        }
    }
}
impl<F> State<F> {
    pub fn version(&self) -> &str {
        let mut len = 0;
        for c in self.version.iter() {
            if *c == b'\0' {
                break;
            }
            len += 1;
        }
        core::str::from_utf8(&self.version[..len]).unwrap()
    }
}
impl<F> State<F>
where
    F: NorFlash + ReadNorFlash,
{
    pub fn default_with_flash(flash: F) -> Self {
        Self {
            flash: Some(flash),
            ..Default::default()
        }
    }
    pub fn default_without_flash(_flash: &F) -> Self {
        Self {
            ..Default::default()
        }
    }
    pub fn save(&mut self) {
        let mut flash = self.flash.take().unwrap();
        let mut buf = [0u8; STATE_SIZE];
        unsafe {
            core::ptr::copy_nonoverlapping::<Self>(self as *const _, buf.as_mut_ptr() as *mut _, 1);
        }
        defmt::info!("writing magic: {:x}", &buf[..8]);
        flash.erase(STATE_ADDR, STATE_ADDR + 2048).unwrap();
        flash.write(STATE_ADDR, &buf).unwrap();
        let mut delay = Delay {};
        delay.delay_ms(100);
        let mut buf = [0u8; 8];
        // check read magic
        flash.read(STATE_ADDR, &mut buf).unwrap();
        // let magic = u64::from_le_bytes(buf);
        defmt::info!("writtern magic: {:x}", buf);
        self.flash.replace(flash);
    }
}

pub struct Game<F> {
    game: LifeGame<25, 16, XorShiftRng>,
    keys: KeysReceiver,
    state: State<F>,
}

impl<F> Game<F> {
    pub fn new(
        icn: ICN2037Sender,
        keys: KeysReceiver,
        fade_time_ms: u64,
        rng: XorShiftRng,
        state: State<F>,
    ) -> Self {
        let game = LifeGame::<25, 16, _>::new(icn, fade_time_ms, rng);
        Self { game, keys, state }
    }

    pub async fn run(&mut self) {
        self.game.randomly_arrange_patterns();
        self.game.draw(true).await;
        loop {
            self.game.draw(false).await;
            let key_event = self.keys.try_receive();
            if key_event.is_ok() || self.game.is_still() {
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
