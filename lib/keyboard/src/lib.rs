#![cfg_attr(not(test), no_std)]

mod keymap;
mod layout;

pub use keymap::Keymap;

pub const ROWS: usize = 5;
pub const COLUMNS: usize = 16;

/// bitmap: column `c` of row `r` is bit `c` of `matrix[r]`.
pub type Matrix = [u16; ROWS];

const DEBOUNCE_MS: u64 = 5;

/// eager debounce: we immediately believe a transition and then run a cooldown for `DEBOUNCE_MS`
#[derive(Default)]
pub struct Debouncer {
    stable: Matrix,
    locked_until: [[u64; COLUMNS]; ROWS],
}

impl Debouncer {
    /// `now` is in milliseconds.
    pub fn update(&mut self, raw: Matrix, now: u64) -> Matrix {
        for (row, locked_until) in self.locked_until.iter_mut().enumerate() {
            let changed = raw[row] ^ self.stable[row];
            for (column, locked_until) in locked_until.iter_mut().enumerate() {
                let bit = 1 << column;
                if changed & bit != 0 && now >= *locked_until {
                    self.stable[row] ^= bit;
                    *locked_until = now + DEBOUNCE_MS;
                }
            }
        }
        self.stable
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    Clockwise,
    CounterClockwise,
}

/// Quadrature decoder for the knob. `state` is A in bit 0 and B in bit 1; the detents are at
/// `0b11`, and one full cycle between detents is one step.
pub struct Encoder {
    previous: u8,
    steps: i8,
}

impl Encoder {
    #[must_use]
    pub fn new(state: u8) -> Self {
        Self {
            previous: state & 0b11,
            steps: 0,
        }
    }

    pub fn update(&mut self, state: u8) -> Option<Rotation> {
        // Indexed by (previous << 2 | current): +1 for each clockwise transition, -1 for each
        // counter-clockwise one, 0 when nothing moved or both lines changed at once (a glitch).
        const DELTA: [i8; 16] = [0, -1, 1, 0, 1, 0, 0, -1, -1, 0, 0, 1, 0, 1, -1, 0];
        let state = state & 0b11;
        let previous = core::mem::replace(&mut self.previous, state);
        if previous ^ state == 0b11 {
            self.steps = 0;
            return None;
        }
        self.steps += DELTA[usize::from(previous << 2 | state)];
        if state != 0b11 {
            return None;
        }
        match core::mem::take(&mut self.steps) {
            4 => Some(Rotation::Clockwise),
            -4 => Some(Rotation::CounterClockwise),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_believes_the_first_edge_and_ignores_bounces() {
        let mut debouncer = Debouncer::default();
        assert_eq!(debouncer.update([1, 0, 0, 0, 0], 0)[0], 1);
        assert_eq!(debouncer.update([0, 0, 0, 0, 0], 5)[0], 1);
        assert_eq!(debouncer.update([1, 0, 0, 0, 0], 10)[0], 1);
        assert_eq!(debouncer.update([0, 0, 0, 0, 0], 25)[0], 0);
        // Other keys aren't held hostage by one key's lockout.
        assert_eq!(debouncer.update([0b10, 0, 0, 0, 0], 26)[0], 0b10);
    }

    #[test]
    fn encoder_reports_once_per_detent_in_the_right_direction() {
        let mut encoder = Encoder::new(0b11);
        let clockwise = [0b01, 0b00, 0b10, 0b11];
        let mut turns = clockwise.iter().filter_map(|&s| encoder.update(s));
        assert_eq!(turns.next(), Some(Rotation::Clockwise));
        assert_eq!(turns.next(), None);
        let mut turns = clockwise
            .iter()
            .rev()
            .skip(1)
            .chain([&0b11])
            .filter_map(|&s| encoder.update(s));
        assert_eq!(turns.next(), Some(Rotation::CounterClockwise));
        assert_eq!(turns.next(), None);
        // Half a turn back and forth doesn't count.
        for state in [0b01, 0b00, 0b01, 0b11] {
            assert_eq!(encoder.update(state), None);
        }
    }
}
