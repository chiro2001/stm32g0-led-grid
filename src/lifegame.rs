#![allow(dead_code)]

use embassy_time::Timer;
use icn2037::{ICN2037Message, ICN2037Sender};

use crate::patterns::*;

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
    pub fn is_still(&self) -> bool {
        if self.state == self.state_next {
            return true;
        }
        if self.all_dead() && self.all_dead_next() {
            return true;
        }
        // detect 2 cycle
        let mut next = [[CellState::Dead; H]; W];
        self.step_calc(&self.state_next, &mut next);
        if self.state == next {
            return true;
        }
        false
    }
    pub fn step_calc(&self, last: &[[CellState; H]; W], next: &mut [[CellState; H]; W]) {
        for x in 0..W {
            for y in 0..H {
                let count = self.count_neighbors_alive(x, y, last);
                use CellState::*;
                let next_state = match (last[x][y], count) {
                    (Alive, v) if v < 2 => Dead,
                    (Alive, 2) | (Alive, 3) => Alive,
                    (Alive, v) if v > 3 => Dead,
                    (Dead, 3) => Alive,
                    (otherwise, _) => otherwise,
                };
                next[x][y] = next_state;
            }
        }
    }

    pub fn step(&mut self) {
        let last = &self.state;
        for x in 0..W {
            for y in 0..H {
                let count = self.count_neighbors_alive(x, y, last);
                use CellState::*;
                let next_state = match (last[x][y], count) {
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
        self.sender.sender.try_send(ICN2037Message::Clear).unwrap();
    }
    pub fn randomly_arrange_patterns(&mut self) {
        self.clear();
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
    pub async fn draw(&mut self, quick: bool) {
        let k_max = 15;
        let send = |k, x, y, from, to| {
            let v = match (from, to) {
                (CellState::Dead, CellState::Alive) => Some(k),
                (CellState::Alive, CellState::Dead) => Some(k_max - k),
                (CellState::Alive, CellState::Alive) => None,
                (CellState::Dead, CellState::Dead) => None,
            };
            v.map(|v| ICN2037Message::SetPixel((x, y, v)))
        };
        if self.fade_time_ms >= 16 && !quick {
            for k in 1..=k_max {
                for x in 0..25 {
                    for y in 0..16 {
                        let (from, to) = (self.state[x][y], self.state_next[x][y]);
                        if let Some(msg) = send(k, x, y, from, to) {
                            self.sender.sender.send(msg).await;
                        }
                    }
                }
                Timer::after_millis(self.fade_time_ms / (k_max as u64 + 1)).await;
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
            if quick {
                Timer::after_millis(1).await;
            } else {
                Timer::after_millis(self.fade_time_ms).await;
            }
        }
    }
}
