#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use core::mem::MaybeUninit;

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
use embedded_graphics::draw_target::DrawTarget;
use icn2037::{ICN2037Device, ICN2037Message, ICN2037Receiver, ICN2037Sender};
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
pub struct LifeGame<const W: usize, const H: usize> {
    state: [[CellState; H]; W],
    state_next: [[CellState; H]; W],
    boarder_policy: BoarderPolicy,
    sender: ICN2037Sender,
    buffer: [[u8; H]; W],
}
impl<const W: usize, const H: usize> LifeGame<W, H> {
    pub fn new(sender: ICN2037Sender, buffer: [[u8; H]; W]) -> Self {
        Self {
            state: [[Default::default(); H]; W],
            state_next: [[Default::default(); H]; W],
            boarder_policy: Default::default(),
            sender,
            buffer,
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
        // for x in 0..W {
        //     for y in 0..H {
        //         self.state[x][y] = self.state_next[x][y];
        //     }
        // }
        self.state = self.state_next;
    }
    fn dump_to_buffer(&mut self) {
        for x in 0..W {
            for y in 0..H {
                self.buffer[x][y] = match self.state[x][y] {
                    CellState::Dead => 0,
                    CellState::Alive => 15,
                };
            }
        }
    }
    pub fn make_alive(&mut self, x: usize, y: usize, alive: bool) {
        self.state_next[x][y] = if alive {
            CellState::Alive
        } else {
            CellState::Dead
        };
    }
}

impl LifeGame<25, 16> {
    pub async fn draw(&'static mut self) {
        // self.dump_to_buffer();
        for k in 0..16 {
            static mut BUFFER: [[u8; 16]; 25] = [[0u8; 16]; 25];
            // let mut buffer = self.buffer.clone();
            for x in 0..25 {
                for y in 0..16 {
                    let (from, to) = (self.state[x][y], self.state_next[x][y]);
                    unsafe {
                        BUFFER[x][y] = match (from, to) {
                            (CellState::Dead, CellState::Alive) => k,
                            (CellState::Alive, CellState::Dead) => 15 - k,
                            (CellState::Alive, CellState::Alive) => 15,
                            (CellState::Dead, CellState::Dead) => 0,
                        }
                    }
                }
            }
            let msg = ICN2037Message::PixelsFrame(unsafe { &BUFFER });
            self.sender.sender.send(msg).await;
            Timer::after_millis(20).await;
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
    let buffer = [[0; 16]; 25];
    static mut GAME: MaybeUninit<LifeGame<25, 16>> = MaybeUninit::uninit();
    unsafe {
        *GAME.as_mut_ptr() = LifeGame::new(icn.clone(), buffer);
    }
    {
        let game = unsafe { GAME.assume_init_mut() };
        game.make_alive(4, 4, true);
        game.make_alive(4, 5, true);
        game.make_alive(4, 6, true);
        game.make_alive(3, 6, true);
        game.make_alive(2, 5, true);

        // game.make_alive(7, 7, true);
        // game.make_alive(7, 8, true);
        // game.make_alive(8, 7, true);
        // game.make_alive(8, 8, true);
    }
    icn.clear(Default::default()).unwrap();
    loop {
        let game = unsafe { GAME.assume_init_mut() };
        game.draw().await;
        let game = unsafe { GAME.assume_init_mut() };
        if game.all_dead_next() {
            break;
        }
        game.step_apply();
        game.step();
        // Timer::after_millis(100 / 4).await;
    }
    info!("Fin.");
}

#[embassy_executor::task]
async fn daemon_task(dev: impl ICN2037Device + 'static, receiver: ICN2037Receiver) {
    dev.task(receiver).await.unwrap();
}
