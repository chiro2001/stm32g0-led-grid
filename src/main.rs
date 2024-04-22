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
use embassy_time::{Delay, Duration, Instant, Timer};
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    mono_font::{ascii, MonoTextStyleBuilder},
    pixelcolor::Gray4,
    text::Text,
    Drawable,
};
use embedded_hal::digital::{InputPin, OutputPin};
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
        Input::new(p.PA12, embassy_stm32::gpio::Pull::Up),
        Output::new(p.PA11, Level::Low, Speed::Low),
        Input::new(p.PA10, embassy_stm32::gpio::Pull::Up),
        Output::new(p.PA9, Level::Low, Speed::Low),
    ));
    let keys_channel = &*make_static!(Channel::new());
    let (tx, rx) = (keys_channel.sender(), keys_channel.receiver());
    spawner.spawn(keys_task(keys, tx)).unwrap();

    let addr: u32 = STATE_ADDR;
    let mut flash = Flash::new_blocking(p.FLASH)
        .into_blocking_regions()
        .bank1_region;
    let mut magic_buf = [0u8; 8];
    flash.blocking_read(addr, &mut magic_buf).unwrap();
    let magic = u64::from_le_bytes(magic_buf);
    defmt::info!("magic: 0x{:x}", magic);
    let mut state = if magic == STATE_MAGIC {
        let mut state_buf = [0u8; STATE_SIZE];
        flash.blocking_read(addr, &mut state_buf).unwrap();
        let mut s: State<_> = unsafe { core::mem::transmute_copy(&state_buf) };
        s.flash.replace(flash);
        s
    } else {
        defmt::warn!("no state found, use default");
        State::default_with_flash(flash)
    };
    let version_state = state.version();
    info!(
        "state version: {}, expected: {}",
        version_state, STATE_VERSION
    );
    if version_state != STATE_VERSION {
        defmt::warn!("state version mismatch, reset state");
        state = State::default_with_flash(state.flash.take().unwrap());
    }

    // TEXT:
    // Life Game 2024
    // Chiro SW  0422
    // v0.1.0-5c55912
    let title = "Life Game";
    let subtitle = "Chiro SW";
    for i in 0..((title.len().max(subtitle.len()) - 5) * 5) as i32 {
        icn.clear(Default::default()).unwrap();
        Text::with_alignment(
            title,
            Point::new(0 - i, 5),
            MonoTextStyleBuilder::new()
                .text_color(Gray4::new(2))
                .font(&ascii::FONT_5X8)
                .build(),
            embedded_graphics::text::Alignment::Left,
        )
        .draw(&mut icn)
        .unwrap();
        Text::with_alignment(
            subtitle,
            Point::new(0 - i, 13),
            MonoTextStyleBuilder::new()
                .text_color(Gray4::new(1))
                .font(&ascii::FONT_5X8)
                .build(),
            embedded_graphics::text::Alignment::Left,
        )
        .draw(&mut icn)
        .unwrap();
        Timer::after_millis(80).await;
    }
    Timer::after_millis(80 * 5).await;

    state.save().await;

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
const STATE_VERSION: &str = build_info::format!("v{}-{}", $.crate_info.version, $.version_control.unwrap().git().unwrap().commit_short_id);
#[repr(C)]
#[repr(align(1))]
pub struct State<F> {
    magic: u64,
    version: [u8; 64],
    page: Page,
    game_brightness: u8,
    light_brightness: u8,
    serial_mode: bool,
    pub flash: Option<F>,
}
impl<F> Default for State<F> {
    fn default() -> Self {
        let mut version = [b'\0'; 64];
        version
            .iter_mut()
            .zip(STATE_VERSION.as_bytes())
            .for_each(|(a, b)| *a = *b);
        Self {
            magic: STATE_MAGIC,
            version,
            page: Default::default(),
            game_brightness: 15,
            light_brightness: 15,
            serial_mode: false,
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
    pub async fn save(&mut self) {
        let mut flash = self.flash.take().unwrap();
        let mut buf = [0u8; STATE_SIZE];
        unsafe {
            core::ptr::copy_nonoverlapping::<Self>(self as *const _, buf.as_mut_ptr() as *mut _, 1);
        }
        flash.erase(STATE_ADDR, STATE_ADDR + 2048).unwrap();
        flash.write(STATE_ADDR, &buf).unwrap();
        Timer::after_millis(100).await;
        self.flash.replace(flash);
    }
}

pub struct Game<F> {
    game: LifeGame<25, 16, XorShiftRng>,
    keys: KeysReceiver,
    state: State<F>,
}

impl<F> Game<F>
where
    F: NorFlash + ReadNorFlash,
{
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
        let mut page_inited = false;
        let mut game_pressed_a = None;
        let mut game_pressed_b = None;
        let mut light_d = 1i8;
        let mut light_pressed = false;

        let game_brightnesses = [1, 4, 8, 15];
        let mut game_brightnesses_idx = game_brightnesses
            .iter()
            .position(|&x| x == self.state.game_brightness)
            .unwrap_or(0);
        self.state.game_brightness = game_brightnesses[game_brightnesses_idx];

        loop {
            let key_event = self.keys.try_receive();
            match self.state.page {
                Page::Game => {
                    if !page_inited {
                        self.game.clear();
                        self.game
                            .send_message(icn2037::ICN2037Message::SetBrightness(
                                self.state.game_brightness,
                            ))
                            .await;
                        page_inited = true;
                    }
                    self.game.draw(false).await;
                    if game_pressed_a.is_some() {
                        game_brightnesses_idx =
                            (game_brightnesses_idx + 1) % game_brightnesses.len();
                        self.state.game_brightness = game_brightnesses[game_brightnesses_idx];
                        self.game
                            .send_message(icn2037::ICN2037Message::SetBrightness(
                                self.state.game_brightness,
                            ))
                            .await;
                        Timer::after_millis(300).await;
                    }
                    match key_event {
                        Ok(KeyEvent::Pressed(Key::A)) => {
                            game_pressed_a = Some(Instant::now());
                        }
                        Ok(KeyEvent::Released(Key::A)) => {
                            if let Some(pressed) = game_pressed_a {
                                if Instant::now() - pressed > Duration::from_millis(1000) {
                                    self.state.page = Page::Light;
                                    self.state.save().await;
                                    page_inited = false;
                                } else {
                                    self.game
                                        .send_message(icn2037::ICN2037Message::SetBrightness(
                                            self.state.game_brightness,
                                        ))
                                        .await;
                                    self.state.save().await;
                                }
                            }
                            game_pressed_a = None;
                        }
                        Ok(KeyEvent::Pressed(Key::B)) => {
                            game_pressed_b = Some(Instant::now());
                        }
                        Ok(KeyEvent::Released(Key::B)) => {
                            if let Some(pressed) = game_pressed_b {
                                if Instant::now() - pressed > Duration::from_millis(1000) {
                                    self.state.serial_mode = true;
                                    self.state.save().await;
                                } else {
                                    self.game.randomly_arrange_patterns();
                                }
                            }
                            game_pressed_b = None;
                        }
                        _ => {}
                    }
                    if self.game.is_still() {
                        // break;
                        info!("re-generate");
                        self.game.randomly_arrange_patterns();
                    }
                    self.game.step_apply();
                    self.game.step();
                }
                Page::Light => {
                    if !page_inited {
                        self.game.clear();
                        self.game
                            .send_message(icn2037::ICN2037Message::SetBrightness(15))
                            .await;
                        self.game
                            .send_message(icn2037::ICN2037Message::Fullfill(
                                self.state.light_brightness,
                            ))
                            .await;
                        page_inited = true;
                    }
                    if light_pressed {
                        let light_brightness =
                            (self.state.light_brightness as i8 + light_d).max(1).min(15) as u8;
                        if light_brightness != self.state.light_brightness {
                            defmt::info!("light brightness: {}", self.state.light_brightness);
                            self.state.light_brightness = light_brightness;
                            self.game
                                .send_message(icn2037::ICN2037Message::Fullfill(
                                    self.state.light_brightness,
                                ))
                                .await;
                            Timer::after_millis(300).await;
                        }
                    }
                    match key_event {
                        Ok(KeyEvent::Released(Key::A)) => {
                            self.state.page = Page::Game;
                            self.state.save().await;
                            page_inited = false;
                        }
                        Ok(KeyEvent::Pressed(Key::B)) => {
                            light_pressed = true;
                        }
                        Ok(KeyEvent::Released(Key::B)) => {
                            light_d = -light_d;
                            light_pressed = false;
                            self.state.save().await;
                        }
                        _ => {}
                    }
                }
            }
            Timer::after_millis(1).await;
        }
    }
}
