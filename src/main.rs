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
use embassy_time::{Delay, Timer};
use embedded_graphics::draw_target::DrawTarget;
use icn2037::{ICN2037Device, ICN2037Message, ICN2037Receiver, ICN2037Sender};
use rand::SeedableRng;
use static_cell::make_static;
use {defmt_rtt as _, panic_probe as _};

#[derive(Debug, Default, Clone, Copy, PartialEq, PartialOrd)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum CellState {
    #[default]
    Dead = 0,
    Alive = 1,
}
#[derive(Debug, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum BoarderPolicy {
    // #[default]
    Ignored,
    #[default]
    Looping,
}
pub struct LifeGame<const W: usize, const H: usize, R> {
    state: [[CellState; H]; W],
    state_next: [[CellState; H]; W],
    boarder_policy: BoarderPolicy,
    sender: ICN2037Sender,
    fade_time_ms: u64,
    rng: R,
}
impl<const W: usize, const H: usize, R> LifeGame<W, H, R>
where
    R: rand::RngCore,
{
    pub fn new(sender: ICN2037Sender, fade_time: u64, rng: R) -> Self {
        Self {
            state: [[Default::default(); H]; W],
            state_next: [[Default::default(); H]; W],
            boarder_policy: Default::default(),
            sender,
            fade_time_ms: fade_time,
            rng,
        }
    }
    fn count_neighbors_alive(&self, x: usize, y: usize, map: &[[CellState; H]; W]) -> usize {
        let mut r = 0;
        match self.boarder_policy {
            BoarderPolicy::Ignored => {
                let sx = if x == 0 { 0 } else { x - 1 };
                let ex = if x >= W - 1 { W - 1 } else { x + 1 };
                let sy = if y == 0 { 0 } else { y - 1 };
                let ey = if y >= H - 1 { H - 1 } else { y + 1 };
                for xx in sx..=ex {
                    for yy in sy..=ey {
                        if !(xx == x && yy == y) {
                            r += map[xx][yy] as usize;
                        }
                    }
                }
            }
            BoarderPolicy::Looping => {
                let sx = x as i32 - 1;
                let ex = x as i32 + 1;
                let sy = y as i32 - 1;
                let ey = y as i32 + 1;
                let mapping = |a, b| {
                    (
                        if a < 0 {
                            a as usize + W
                        } else {
                            if a as usize >= W {
                                a as usize - W
                            } else {
                                a as usize
                            }
                        },
                        if b < 0 {
                            b as usize + H
                        } else {
                            if b as usize >= H {
                                b as usize - H
                            } else {
                                b as usize
                            }
                        },
                    )
                };
                for xx in sx..=ex {
                    for yy in sy..=ey {
                        let (xx, yy) = mapping(xx, yy);
                        if !(xx == x && yy == y) {
                            r += map[xx][yy] as usize;
                        }
                    }
                }
            }
        }
        r
    }
    pub fn all_dead(&self) -> bool {
        self.state
            .iter()
            .all(|m| m.iter().all(|x| *x == CellState::Dead))
    }
    pub fn all_dead_next(&self) -> bool {
        self.state_next
            .iter()
            .all(|m| m.iter().all(|x| *x == CellState::Dead))
    }
    pub fn step(&mut self) {
        let last = &self.state;
        for x in 0..W {
            for y in 0..H {
                let count = self.count_neighbors_alive(x, y, &last);
                use CellState::*;
                let next_state = match (self.state[x][y], count) {
                    (Alive, v) if v < 2 => Dead,
                    (Alive, 2) | (Alive, 3) => Alive,
                    (Alive, v) if v > 3 => Dead,
                    (Dead, 3) => Alive,
                    (otherwise, _) => otherwise,
                };
                self.state_next[x][y] = next_state;
            }
        }
    }
    pub fn step_apply(&mut self) {
        self.state = self.state_next;
    }
    pub fn make_alive(&mut self, x: usize, y: usize, alive: bool) {
        let x = x % W;
        let y = y % H;
        self.state_next[x][y] = if alive {
            CellState::Alive
        } else {
            CellState::Dead
        };
    }
    pub fn apply_pattern(&mut self, x: usize, y: usize, pattern: &[&str]) {
        for (dy, line) in pattern.iter().enumerate() {
            for (dx, c) in line.chars().enumerate() {
                let x = x + dx;
                let y = y + dy;
                if x < W && y < H {
                    self.make_alive(x, y, c != ' ');
                }
            }
        }
    }
    pub fn apply_pattern_transpose(&mut self, x: usize, y: usize, pattern: &[&str]) {
        for (dy, line) in pattern.iter().enumerate() {
            for (dx, c) in line.chars().enumerate() {
                let x = x + dx;
                let y = y + dy;
                if x < W && y < H {
                    self.make_alive(y, x, c != ' ');
                }
            }
        }
    }
    pub fn apply_pattern_center(&mut self, pattern: &[&str]) {
        let (w, h) = (pattern[0].len(), pattern.len());
        let (x, y) = (W / 2 - w / 2, H / 2 - h / 2);
        self.apply_pattern(x, y, pattern);
    }
    pub fn apply_pattern_center_transpose(&mut self, pattern: &[&str]) {
        let (w, h) = (pattern[0].len(), pattern.len());
        let (x, y) = (W / 2 - w / 2, H / 2 - h / 2);
        self.apply_pattern_transpose(x, y, pattern);
    }
    pub fn clear(&mut self) {
        self.state
            .iter_mut()
            .for_each(|m| m.iter_mut().for_each(|x| *x = CellState::Dead));
        self.state_next
            .iter_mut()
            .for_each(|m| m.iter_mut().for_each(|x| *x = CellState::Dead));
    }
    pub fn randomly_arrange_patterns(&mut self) {
        let mut picks = [0; 3];
        self.rng.fill_bytes(&mut picks);
        picks.iter_mut().for_each(|x| *x = *x % 3);
        let patterns = [PATTERN_CLOCK_LIST, PATTERN_FLY_LIST, PATTERN_STABLE_LIST];
        for k in 0..3 {
            for i in 0..picks[k] as usize {
                let pattern = patterns[i];
                let x = self.rng.next_u32() as usize % W;
                let y = self.rng.next_u32() as usize % H;
                let idx = self.rng.next_u32() as usize % pattern.len();
                self.apply_pattern(x, y, pattern[idx]);
            }
        }
    }
    pub async fn draw(&mut self) {
        let send = |k, x, y, from, to| {
            // let (from, to) = (self.state[x][y], self.state_next[x][y]);
            let v = match (from, to) {
                (CellState::Dead, CellState::Alive) => Some(k),
                (CellState::Alive, CellState::Dead) => Some(15 - k),
                (CellState::Alive, CellState::Alive) => None,
                (CellState::Dead, CellState::Dead) => None,
            };
            v.map(|v| ICN2037Message::SetPixel((x, y, v)))
        };
        if self.fade_time_ms >= 16 {
            for k in 0..16 {
                for x in 0..25 {
                    for y in 0..16 {
                        let (from, to) = (self.state[x][y], self.state_next[x][y]);
                        if let Some(msg) = send(k, x, y, from, to) {
                            self.sender.sender.send(msg).await;
                        }
                    }
                }
                Timer::after_millis(self.fade_time_ms / 16).await;
            }
        } else {
            for x in 0..25 {
                for y in 0..16 {
                    let (from, to) = (self.state[x][y], self.state_next[x][y]);
                    if let Some(msg) = send(15, x, y, from, to) {
                        self.sender.sender.send(msg).await;
                    }
                }
            }
            Timer::after_millis(self.fade_time_ms).await;
        }
    }
}

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

    let mut adc = embassy_stm32::adc::Adc::new(p.ADC1, &mut Delay);
    let mut adc_pin = p.PA0;
    let mut adc_results = [0u8; 16];
    for i in 0..64 {
        adc_results[i % 16] ^= ((adc.read(&mut adc_pin) + i as u16) % 254) as u8;
        Timer::after_millis(1).await;
    }
    defmt::info!("noise: {:?}", adc_results);

    let rng = rand_xorshift::XorShiftRng::from_seed(adc_results);
    let mut game = LifeGame::<25, 16, _>::new(icn.clone(), 15, rng);
    game.randomly_arrange_patterns();
    icn.clear(Default::default()).unwrap();
    loop {
        game.draw().await;
        if game.all_dead_next() {
            break;
        }
        game.step_apply();
        game.step();
        Timer::after_millis(1).await;
    }
    info!("Fin.");
}

#[embassy_executor::task]
async fn daemon_task(dev: impl ICN2037Device + 'static, receiver: ICN2037Receiver) {
    dev.task(receiver).await.unwrap();
}

#[rustfmt::skip]
#[allow(dead_code)]
mod patterns {
    pub const PATTERN_STABLE_BLOCK: &[&str] = &[
        "XX", 
        "XX"
    ];
    pub const PATTERN_STABLE_LOAF: &[&str] = &[
        " XX ", 
        "X  X", 
        " X X", 
        "  X ", 
    ];
    pub const PATTERN_STABLE_BEEHIVE: &[&str] = &[
        " XX ", 
        "X  X", 
        " XX ", 
    ];
    pub const PATTERN_STABLE_SHIP: &[&str] = &[
        " XXX", 
        "X  X", 
        "XXX ", 
    ];
    pub const PATTERN_STABLE_BOAT: &[&str] = &[
        "XX ", 
        "X X", 
        " X ", 
    ];
    pub const PATTERN_STABLE_FLOWER: &[&str] = &[
        " X ", 
        "X X", 
        " X ", 
    ];
    pub const PATTERN_STABLE_POND: &[&str] = &[
        " X ", 
        "X X", 
        " X ", 
    ];
    pub const PATTERN_STABLE_LIST: &[&[&str]] = &[
        PATTERN_STABLE_BLOCK,
        PATTERN_STABLE_LOAF,
        PATTERN_STABLE_BEEHIVE,
        PATTERN_STABLE_SHIP,
        PATTERN_STABLE_BOAT,
        PATTERN_STABLE_FLOWER,
        PATTERN_STABLE_POND,
    ];

    pub const PATTERN_CLOCK_BLINKER: &[&str] = &[
        "XXX", 
    ];
    pub const PATTERN_CLOCK_TOAD: &[&str] = &[
        " XXX",
        "XXX ",
    ];
    pub const PATTERN_CLOCK_TRAFIC_LIGHT: &[&str] = &[
        "  XXX  ", 
        "       ",
        "X     X",
        "X     X",
        "X     X",
        "       ",
        "  XXX  ",
    ];
    pub const PATTERN_CLOCK_BEACON: &[&str] = &[
        "XX  ", 
        "XX  ", 
        "  XX", 
        "  XX", 
    ];
    pub const PATTERN_CLOCK_PULSAR: &[&str] = &[
        "  XXX   XXX  ", 
        "             ",
        "X    X X    X", 
        "X    X X    X", 
        "X    X X    X", 
        "  XXX   XXX  ", 
        "             ",
        "  XXX   XXX  ", 
        "X    X X    X", 
        "X    X X    X", 
        "X    X X    X", 
        "             ",
        "  XXX   XXX  ", 
    ];
    pub const PATTERN_CLOCK_I_COLUMN: &[&str] = &[
        "XXX",
        "X X",
        "XXX",
        "XXX",
        "XXX",
        "XXX",
        "X X",
        "XXX",
    ];
    pub const PATTERN_CLOCK_LIST: &[&[&str]] = &[
        PATTERN_CLOCK_BLINKER,
        PATTERN_CLOCK_TOAD,
        PATTERN_CLOCK_TRAFIC_LIGHT,
        PATTERN_CLOCK_BEACON,
        PATTERN_CLOCK_PULSAR,
        PATTERN_CLOCK_I_COLUMN,
    ];

    pub const PATTERN_FLY_GLIDER: &[&str] = &[
        " X ",
        "  X",
        "XXX",
    ];
    pub const PATTERN_FLY_LIGHTWEIGHT_SPACESHIP: &[&str] = &[
        "X  X ",
        "    X",
        "X   X",
        " XXXX",
    ];
    pub const PATTERN_FLY_LIST: &[&[&str]] = &[
        PATTERN_FLY_GLIDER,
        PATTERN_FLY_LIGHTWEIGHT_SPACESHIP,
    ];

}
use patterns::*;
