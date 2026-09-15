use crate::{COLUMNS, Matrix, ROWS};

pub const DRIVERS: usize = 2;
pub const CHANNELS: usize = 192;

/// PWM value for every channel of every driver.
pub type Frame = [[u8; CHANNELS]; DRIVERS];

const NO_LED: u8 = 0xff;
const CAPS: (usize, usize) = (2, 1);

#[rustfmt::skip]
const RED_CHANNEL: [[u8; COLUMNS]; ROWS] = [
    [NO_LED, 0x0e, 0x0d, 0x0c, 0x0b, 0x0a, 0x09, 0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01, 0x00],
    [0x3f, 0x3e, 0x3d, 0x3c, 0x3b, 0x3a, 0x39, 0x38, 0x37, 0x36, 0x35, 0x34, 0x33, 0x32, 0x31, 0x30],
    [0x0f, 0x0e, 0x0d, 0x0c, 0x0b, 0x0a, 0x09, 0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01, NO_LED],
    [0x6f, 0x6e, NO_LED, 0x6c, 0x6b, 0x6a, 0x69, 0x68, 0x67, 0x66, 0x65, 0x64, 0x63, 0x62, 0x61, 0x60],
    [0x3f, 0x3e, 0x3d, 0x3c, NO_LED, NO_LED, NO_LED, 0x38, NO_LED, NO_LED, 0x35, 0x34, 0x33, 0x32, 0x31, 0x30],
];

// ripped from qmk
const CIE1931: [u8; 256] = {
    let mut table = [0; 256];
    let mut i = 0;
    while i < table.len() {
        let value = if i <= 20 {
            (i as u64 * 1000).div_ceil(9023)
        } else {
            (100 * i as u64 + 4080)
                .pow(3)
                .div_ceil(255_u64.pow(2) * 116_u64.pow(3))
        };
        table[i] = value as u8;
        i += 1;
    }
    table
};

/// HSV(197, 255, `value`), as (red, blue); green always zero
fn colour(value: u8) -> (u8, u8) {
    let blue = CIE1931[usize::from(value)];
    (((u16::from(blue) * 163) >> 8) as u8, blue)
}

#[derive(Clone, Copy)]
struct Hit {
    row: usize,
    column: usize,
    at: u64,
}

#[derive(Default)]
pub struct Lighting {
    previous: Matrix,
    hits: [Option<Hit>; 8],
}

impl Lighting {
    pub fn update(&mut self, keys: Matrix, now: u64) {
        for row in 0..ROWS {
            let pressed = keys[row] & !self.previous[row];
            for column in (0..COLUMNS).filter(|c| pressed & 1 << c != 0) {
                if RED_CHANNEL[row][column] == NO_LED || (row, column) == CAPS {
                    continue;
                }
                self.hits.rotate_left(1);
                self.hits[7] = Some(Hit {
                    row,
                    column,
                    at: now,
                });
            }
        }
        self.previous = keys;
    }

    pub fn clear(&mut self) {
        self.hits = [None; 8];
    }

    #[must_use]
    pub fn frame(&self, now: u64, caps_lock: bool) -> Frame {
        let mut frame = [[0; CHANNELS]; DRIVERS];
        let mut light = |row: usize, column: usize, (red, blue): (u8, u8)| {
            let driver = usize::from(row >= 2);
            let channel = usize::from(RED_CHANNEL[row][column]);
            frame[driver][channel] = red;
            frame[driver][channel + 16] = blue;
        };
        for hit in self.hits.iter().flatten() {
            // 2ms per brightness fade step
            let faded = (now.saturating_sub(hit.at) / 2).min(255) as u8;
            light(hit.row, hit.column, colour(255 - faded));
        }
        if caps_lock {
            light(CAPS.0, CAPS.1, colour(255));
        }
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(frame: &Frame) -> Vec<(usize, usize)> {
        let mut lit = Vec::new();
        for (driver, channels) in frame.iter().enumerate() {
            for (channel, &value) in channels.iter().enumerate() {
                if value != 0 {
                    lit.push((driver, channel));
                }
            }
        }
        lit
    }

    #[test]
    fn a_press_lights_its_key_and_fades_out() {
        let mut lighting = Lighting::default();
        lighting.update([0, 0, 0b100, 0, 0], 1000); // A
        let fresh = lighting.frame(1000, false);
        assert_eq!(lit(&fresh), [(1, 0x0d), (1, 0x0d + 16)]);
        let (red, blue) = (fresh[1][0x0d], fresh[1][0x0d + 16]);
        assert!(blue > red && red > 0, "theme colour is blue-violet");
        let later = lighting.frame(1300, false);
        assert!(later[1][0x0d + 16] < blue);
        assert_eq!(lit(&lighting.frame(1600, false)), []);
    }

    #[test]
    fn holding_a_key_does_not_keep_it_lit() {
        let mut lighting = Lighting::default();
        lighting.update([0, 0, 0b100, 0, 0], 0);
        lighting.update([0, 0, 0b100, 0, 0], 300);
        lighting.update([0, 0, 0b100, 0, 0], 600);
        assert_eq!(lit(&lighting.frame(600, false)), []);
    }

    #[test]
    fn caps_is_an_indicator_not_a_reactive_key() {
        let mut lighting = Lighting::default();
        lighting.update([0, 0, 0b10, 0, 0], 0);
        assert_eq!(lit(&lighting.frame(0, false)), []);
        assert_eq!(lit(&lighting.frame(0, true)), [(1, 0x0e), (1, 0x0e + 16)]);
    }

    #[test]
    fn positions_without_leds_are_ignored_and_clear_forgets_hits() {
        let mut lighting = Lighting::default();
        lighting.update([1, 0, 0, 0, 0], 0); // the knob has no LED
        assert_eq!(lit(&lighting.frame(0, false)), []);
        lighting.update([0b10, 0, 0, 0, 0], 0);
        assert!(!lit(&lighting.frame(0, false)).is_empty());
        lighting.clear();
        assert_eq!(lit(&lighting.frame(0, false)), []);
    }
}
