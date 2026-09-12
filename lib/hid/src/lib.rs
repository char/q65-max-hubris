#![cfg_attr(not(test), no_std)]

pub mod descriptor;
mod usage;

pub use usage::{Consumer, Key, Led};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

const FIRST_KEY: u8 = Key::A as u8;
const LAST_KEY: u8 = Key::F24 as u8;
const KEY_COUNT: u8 = LAST_KEY - FIRST_KEY + 1;
const FIRST_MODIFIER: u8 = Key::LeftControl as u8;

pub const KEYBOARD_REPORT_SIZE: usize = 1 + (KEY_COUNT as usize).div_ceil(8);
pub const BOOT_REPORT_SIZE: usize = 8;

/// NKRO: mods byte + 1 bit per key from `A` to `F24`
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, IntoBytes, FromBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct KeyboardReport(pub [u8; KEYBOARD_REPORT_SIZE]);

impl KeyboardReport {
    pub fn press(&mut self, key: Key) {
        match key as u8 {
            usage @ FIRST_MODIFIER.. => self.0[0] |= 1 << (usage - FIRST_MODIFIER),
            usage => {
                let bit = usage - FIRST_KEY;
                self.0[1 + usize::from(bit / 8)] |= 1 << (bit % 8);
            }
        }
    }

    fn keys(&self) -> impl Iterator<Item = u8> + '_ {
        (0..KEY_COUNT)
            .filter(|bit| self.0[1 + usize::from(bit / 8)] & (1 << (bit % 8)) != 0)
            .map(|bit| bit + FIRST_KEY)
    }

    /// boot protocol has 6KRO: (modifiers, reserved, usages[6])
    #[must_use]
    pub fn to_boot(&self) -> [u8; BOOT_REPORT_SIZE] {
        let mut report = [0; BOOT_REPORT_SIZE];
        report[0] = self.0[0];
        let mut keys = self.keys();
        for slot in &mut report[2..] {
            match keys.next() {
                Some(usage) => *slot = usage,
                None => return report,
            }
        }
        if keys.next().is_some() {
            report[2..].fill(Key::ERROR_ROLLOVER);
        }
        report
    }
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, IntoBytes, FromBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct ConsumerReport(pub u8);

impl ConsumerReport {
    pub fn press(&mut self, control: Consumer) {
        self.0 |= 1 << control as u8;
    }
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, IntoBytes, FromBytes, Immutable, KnownLayout,
)]
#[repr(C)]
pub struct Reports {
    pub keyboard: KeyboardReport,
    pub consumer: ConsumerReport,
}

/// 1 bit per `Led`.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, IntoBytes, FromBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct LedReport(pub u8);

impl LedReport {
    #[must_use]
    pub fn is_lit(&self, led: Led) -> bool {
        self.0 & (1 << (led as u8 - 1)) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_and_keys_land_in_the_right_places() {
        let mut report = KeyboardReport::default();
        report.press(Key::LeftShift);
        report.press(Key::A);
        report.press(Key::F24);
        let bytes = report.0;
        assert_eq!(bytes[0], 0b10);
        assert_eq!(bytes[1], 1);
        assert_eq!(bytes[KEYBOARD_REPORT_SIZE - 1], 0x80);
        assert!(report.keys().eq([Key::A as u8, Key::F24 as u8]));
    }

    #[test]
    fn boot_report_lists_keys_until_it_overflows() {
        let mut report = KeyboardReport::default();
        report.press(Key::RightGui);
        for key in [Key::A, Key::B, Key::C] {
            report.press(key);
        }
        assert_eq!(report.to_boot(), [0x80, 0, 4, 5, 6, 0, 0, 0]);
        for key in [Key::D, Key::E, Key::F, Key::G] {
            report.press(key);
        }
        assert_eq!(report.to_boot(), [0x80, 0, 1, 1, 1, 1, 1, 1]);
    }

    #[test]
    fn consumer_and_led_bits_follow_declaration_order() {
        let mut consumer = ConsumerReport::default();
        consumer.press(Consumer::VolumeDown);
        assert_eq!(consumer.0, 0b100);
        let leds = LedReport(0b10);
        assert!(leds.is_lit(Led::CapsLock));
        assert!(!leds.is_lit(Led::NumLock));
    }
}
