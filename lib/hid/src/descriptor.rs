use crate::{Consumer, Key, Led};

#[derive(Clone, Copy)]
#[repr(u8)]
enum Page {
    GenericDesktop = 0x01,
    Keyboard = 0x07,
    Led = 0x08,
    Consumer = 0x0c,
}

#[derive(Clone, Copy)]
enum Bits {
    /// 1 bit per usage in `min..=max`.
    Range(Page, u8, u8),
    /// 1 bit per listed usage.
    List(Page, &'static [u8]),
    /// Padding for rounding up to whole bytes.
    Padding(u8),
}

#[derive(Clone, Copy)]
enum Direction {
    Input,
    Output,
}

#[derive(Clone, Copy)]
struct Field {
    direction: Direction,
    bits: Bits,
}

impl Bits {
    const fn keys(first: Key, last: Key) -> Self {
        Self::Range(Page::Keyboard, first as u8, last as u8)
    }

    const fn leds(first: Led, last: Led) -> Self {
        Self::Range(Page::Led, first as u8, last as u8)
    }
}

impl Field {
    const fn input(bits: Bits) -> Self {
        Self {
            direction: Direction::Input,
            bits,
        }
    }

    const fn output(bits: Bits) -> Self {
        Self {
            direction: Direction::Output,
            bits,
        }
    }
}

struct Descriptor {
    page: Page,
    usage: u8,
    fields: &'static [Field],
}

const KEYBOARD_DESCRIPTOR: Descriptor = Descriptor {
    page: Page::GenericDesktop,
    usage: 0x06, // Keyboard
    fields: &[
        Field::input(Bits::keys(Key::LeftControl, Key::RightGui)),
        Field::input(Bits::keys(Key::A, Key::F24)),
        Field::output(Bits::leds(Led::NumLock, Led::Kana)),
        Field::output(Bits::Padding(3)),
    ],
};

const CONSUMER_DESCRIPTOR: Descriptor = Descriptor {
    page: Page::Consumer,
    usage: 0x01, // Consumer Control
    fields: &[
        Field::input(Bits::List(
            Page::Consumer,
            &[
                Consumer::PlayPause.usage(),
                Consumer::VolumeUp.usage(),
                Consumer::VolumeDown.usage(),
            ],
        )),
        Field::input(Bits::Padding(5)),
    ],
};

pub const KEYBOARD: [u8; KEYBOARD_DESCRIPTOR.encoded_len()] = KEYBOARD_DESCRIPTOR.encode();
pub const CONSUMER: [u8; CONSUMER_DESCRIPTOR.encoded_len()] = CONSUMER_DESCRIPTOR.encode();

const USAGE_PAGE: u8 = 0x05;
const LOGICAL_MINIMUM: u8 = 0x15;
const LOGICAL_MAXIMUM: u8 = 0x25;
const REPORT_SIZE: u8 = 0x75;
const REPORT_COUNT: u8 = 0x95;
const USAGE: u8 = 0x09;
const USAGE_MINIMUM: u8 = 0x19;
const USAGE_MAXIMUM: u8 = 0x29;
const INPUT: u8 = 0x81;
const OUTPUT: u8 = 0x91;
const COLLECTION: u8 = 0xa1;
const END_COLLECTION: u8 = 0xc0;

const APPLICATION: u8 = 0x01;
const CONSTANT: u8 = 0x01;
const VARIABLE: u8 = 0x02;

/// Compile-time byte writer. `finish` checks that exactly the declared length was written, so a
/// mismatch between `encoded_len` and `encode` is a build error rather than a truncated descriptor.
struct Bytes<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> Bytes<N> {
    const fn new() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
        }
    }

    const fn item(&mut self, prefix: u8, data: u8) {
        self.bytes[self.len] = prefix;
        self.bytes[self.len + 1] = data;
        self.len += 2;
    }

    const fn finish(mut self, last: u8) -> [u8; N] {
        self.bytes[self.len] = last;
        self.len += 1;
        assert!(self.len == N);
        self.bytes
    }
}

impl Descriptor {
    const fn encoded_len(&self) -> usize {
        let mut len = 6 * 2 + 1;
        let mut i = 0;
        while i < self.fields.len() {
            len += match self.fields[i].bits {
                Bits::Range(..) => 5 * 2,
                Bits::List(_, usages) => (usages.len() + 3) * 2,
                Bits::Padding(_) => 2 * 2,
            };
            i += 1;
        }
        len
    }

    const fn encode<const N: usize>(&self) -> [u8; N] {
        let mut out = Bytes::new();
        out.item(USAGE_PAGE, self.page as u8);
        out.item(USAGE, self.usage);
        out.item(COLLECTION, APPLICATION);
        out.item(LOGICAL_MINIMUM, 0);
        out.item(LOGICAL_MAXIMUM, 1);
        out.item(REPORT_SIZE, 1);
        let mut i = 0;
        while i < self.fields.len() {
            let field = self.fields[i];
            let (count, kind) = match field.bits {
                Bits::Range(page, min, max) => {
                    out.item(USAGE_PAGE, page as u8);
                    out.item(USAGE_MINIMUM, min);
                    out.item(USAGE_MAXIMUM, max);
                    (max - min + 1, VARIABLE)
                }
                Bits::List(page, usages) => {
                    out.item(USAGE_PAGE, page as u8);
                    let mut j = 0;
                    while j < usages.len() {
                        out.item(USAGE, usages[j]);
                        j += 1;
                    }
                    (usages.len() as u8, VARIABLE)
                }
                Bits::Padding(count) => (count, CONSTANT),
            };
            out.item(REPORT_COUNT, count);
            let main = match field.direction {
                Direction::Input => INPUT,
                Direction::Output => OUTPUT,
            };
            out.item(main, kind);
            i += 1;
        }
        out.finish(END_COLLECTION)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KEYBOARD_REPORT_SIZE;

    /// Walks a descriptor the way a host does and returns (input bits, output bits).
    fn report_bits(descriptor: &[u8]) -> (usize, usize) {
        let (mut size, mut count) = (0, 0);
        let (mut input, mut output) = (0, 0);
        let mut bytes = descriptor.iter();
        while let Some(&prefix) = bytes.next() {
            let data_len = match prefix & 0b11 {
                3 => 4,
                n => usize::from(n),
            };
            let data = bytes
                .by_ref()
                .take(data_len)
                .fold(0usize, |acc, &b| acc << 8 | usize::from(b));
            match prefix & 0xfc {
                0x74 => size = data,
                0x94 => count = data,
                0x80 => input += size * count,
                0x90 => output += size * count,
                _ => {}
            }
        }
        (input, output)
    }

    #[test]
    fn descriptors_match_report_sizes() {
        assert_eq!(report_bits(&KEYBOARD), (KEYBOARD_REPORT_SIZE * 8, 8));
        assert_eq!(report_bits(&CONSUMER), (8, 0));
    }
}
