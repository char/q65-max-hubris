#![cfg_attr(not(test), no_std)]

// Protocol reference: Keychron QMK common/wireless/lkbt51.{c,h}.
use hid::{Consumer, ConsumerReport, KeyboardReport};

pub const HOST_2P4: u8 = 24;
pub const READ_SIZE: usize = 68;
pub const READ: [u8; READ_SIZE] = {
    let mut bytes = [0; READ_SIZE];
    bytes[0] = 0x84;
    bytes[1] = 0x7f;
    bytes[3] = 0x80;
    bytes
};
pub const CONNECT: [u8; 4] = [0x22, HOST_2P4, 0, 0];
pub const DISCONNECT: [u8; 2] = [0x23, 0];
pub const PAIR: [u8; 7] = [0x21, HOST_2P4, 180, 0, 3, 1, 0];
pub const CONNECTION_ACK: [u8; 1] = [0xa4];
pub const NAME: &[u8] = b"\x45Keychron Q65 Max";
pub const BATTERY_QUERY: [u8; 3] = [0x25, 5, 2];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    bytes: [u8; 64],
    len: usize,
}

impl Packet {
    #[must_use]
    pub fn command(sequence: u8, payload: &[u8], ack: bool) -> Option<Self> {
        if sequence == 0 || payload.is_empty() || payload.len() > 53 {
            return None;
        }
        let mut bytes = [0; 64];
        let length = (payload.len() + 2) as u8;
        bytes[..9].copy_from_slice(&[
            0x84,
            0x7e,
            0,
            0,
            0xaa,
            if ack { 0x56 } else { 0x55 },
            length,
            !length,
            sequence,
        ]);
        bytes[9..9 + payload.len()].copy_from_slice(payload);
        let sum: u16 = payload.iter().map(|&b| u16::from(b)).sum();
        bytes[9 + payload.len()..11 + payload.len()].copy_from_slice(&sum.to_le_bytes());
        Some(Self {
            bytes,
            len: payload.len() + 11,
        })
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

#[must_use]
pub fn configuration(idle_seconds: u16) -> [u8; 23] {
    let mut bytes = [0; 23];
    bytes[0] = 0x41;
    bytes[1] = 2;
    bytes[2..4].copy_from_slice(&idle_seconds.to_le_bytes());
    bytes[4..6].copy_from_slice(&180u16.to_le_bytes());
    bytes[7..9].copy_from_slice(&5u16.to_le_bytes());
    bytes[9] = 90;
    bytes[12] = 1;
    bytes[13..15].copy_from_slice(&0x3434u16.to_le_bytes());
    bytes[15..17].copy_from_slice(&0x08b0u16.to_le_bytes());
    bytes
}

#[must_use]
pub fn keyboard(report: KeyboardReport) -> [u8; 9] {
    let mut bytes = [0; 9];
    bytes[0] = 0x11;
    bytes[1..].copy_from_slice(&report.to_boot());
    bytes
}

#[must_use]
pub fn nkro(report: KeyboardReport) -> [u8; 21] {
    let mut bytes = [0; 21];
    bytes[0] = 0x12;
    bytes[1] = report.0[0];
    // Our bitmap starts at A (usage 4); QMK's starts at usage 0.
    for (index, &bits) in report.0[1..].iter().enumerate() {
        bytes[2 + index] |= bits << 4;
        bytes[3 + index] |= bits >> 4;
    }
    bytes
}

#[must_use]
pub fn consumer(report: ConsumerReport) -> [u8; 7] {
    let mut bytes = [0; 7];
    bytes[0] = 0x13;
    let mut slot = 1;
    for control in [
        Consumer::PlayPause,
        Consumer::VolumeUp,
        Consumer::VolumeDown,
    ] {
        if report.0 & (1 << control as u8) != 0 {
            bytes[slot] = control.usage();
            slot += 2;
        }
    }
    bytes
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Connection {
    Connected,
    Pairing,
    Reconnecting,
    Disconnected,
    Sleeping,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ack {
    Success,
    ChecksumError,
    HalfFull,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Reset,
    Connection {
        state: Connection,
        host: u8,
    },
    Leds(u8),
    Battery(u16),
    Interval(u16),
    Ack {
        sequence: u8,
        command: u8,
        result: Ack,
    },
}

/// Status registers are followed by optional checksummed event packets. Unknown
/// events and incomplete packets are ignored; none may index outside the read.
pub fn events(bytes: &[u8], mut emit: impl FnMut(Event)) {
    if let Some(status) = bytes.get(4..13)
        && status[0] == 0xaa
        && status[1] != 0x54
    {
        let mask = status[1];
        if mask & 8 != 0 {
            emit(Event::Reset);
        }
        if mask & 1 != 0 {
            let state = match status[2] {
                0x20 => Some(Connection::Connected),
                0x21 => Some(Connection::Pairing),
                0x22 => Some(Connection::Reconnecting),
                0x23 => Some(Connection::Disconnected),
                0x26 => Some(Connection::Sleeping),
                _ => None,
            };
            if let Some(state) = state {
                emit(Event::Connection {
                    state,
                    host: status[3],
                });
            }
        }
        if mask & 2 != 0 {
            emit(Event::Leds(status[4]));
        }
        if mask & 4 != 0 {
            emit(Event::Battery(u16::from_le_bytes([status[5], status[6]])));
        }
        if mask & 16 != 0 {
            let value = status[8];
            let micros = u32::from(value & 0x7f) * if value & 0x80 != 0 { 1250 } else { 125 };
            emit(Event::Interval(micros.div_ceil(1000).max(1) as u16));
        }
    }
    let mut offset = 10;
    while let Some(header) = bytes.get(offset..offset + 5) {
        if header[0..2] != [0xaa, 0x57] || header[2] != !header[3] || header[2] < 3 {
            offset += 1;
            continue;
        }
        let len = usize::from(header[2]);
        let Some(payload) = bytes.get(offset + 5..offset + 5 + len) else {
            offset += 1;
            continue;
        };
        let data = &payload[..len - 2];
        let sum: u16 = data.iter().map(|&b| u16::from(b)).sum();
        if sum.to_le_bytes() == payload[len - 2..] {
            match data {
                [0xa1, sequence, command, result, ..] => {
                    let result = match result {
                        0 => Some(Ack::Success),
                        1 => Some(Ack::ChecksumError),
                        2 => Some(Ack::HalfFull),
                        3 => Some(Ack::Full),
                        _ => None,
                    };
                    if let Some(result) = result {
                        emit(Event::Ack {
                            sequence: *sequence,
                            command: *command,
                            result,
                        });
                    }
                }
                [0xb0, ..] => emit(Event::Reset),
                [0xb4, leds, ..] => emit(Event::Leds(*leds)),
                _ => {}
            }
            offset += 5 + len;
        } else {
            offset += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hid::{Key, KeyboardReport};

    #[test]
    fn connect_matches_the_wire_protocol() {
        assert_eq!(
            Packet::command(1, &CONNECT, true).unwrap().bytes(),
            &[
                0x84, 0x7e, 0, 0, 0xaa, 0x56, 6, 0xf9, 1, 0x22, 24, 0, 0, 0x3a, 0
            ]
        );
        assert!(Packet::command(0, &CONNECT, true).is_none());
        assert!(Packet::command(1, &[], false).is_none());
        assert!(Packet::command(1, &[0; 54], false).is_none());
        assert_eq!(
            Packet::command(255, &[255; 53], false)
                .unwrap()
                .bytes()
                .len(),
            64
        );
    }

    #[test]
    fn configuration_matches_q65_identity_and_timing() {
        assert_eq!(
            configuration(7200),
            [
                0x41, 2, 0x20, 0x1c, 180, 0, 0, 5, 0, 90, 0, 0, 1, 0x34, 0x34, 0xb0, 8, 0, 0, 0, 0,
                0, 0
            ]
        );
    }

    #[test]
    fn reports_use_hid_usages_not_our_internal_bitmap_offsets() {
        let mut report = KeyboardReport::default();
        report.press(Key::A);
        report.press(Key::F24);
        report.press(Key::RightShift);
        assert_eq!(keyboard(report), [0x11, 0x20, 0, 4, 0x73, 0, 0, 0, 0]);
        let mut expected = [0; 21];
        expected[0] = 0x12;
        expected[1] = 0x20;
        expected[2] = 0x10;
        expected[16] = 8;
        assert_eq!(nkro(report), expected);
        assert_eq!(
            consumer(ConsumerReport(7)),
            [0x13, 0xcd, 0, 0xe9, 0, 0xea, 0]
        );
        assert_eq!(consumer(ConsumerReport(0)), [0x13, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn status_and_fifo_ack_can_arrive_together() {
        let bytes = [
            0, 0, 0, 0, 0xaa, 3, 0x20, 24, 2, 0, 0xaa, 0x57, 6, 0xf9, 9, 0xa1, 7, 0x11, 2, 0xbb, 0,
        ];
        let mut received = Vec::new();
        events(&bytes, |event| received.push(event));
        assert_eq!(
            received,
            [
                Event::Connection {
                    state: Connection::Connected,
                    host: 24
                },
                Event::Leds(2),
                Event::Ack {
                    sequence: 7,
                    command: 0x11,
                    result: Ack::HalfFull
                }
            ]
        );
    }

    #[test]
    fn truncated_and_corrupt_ack_never_becomes_success() {
        let mut bytes = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xaa, 0x57, 6, 0xf9, 9, 0xa1, 7, 0x11, 0, 0xb9, 0,
        ];
        for end in 0..bytes.len() {
            events(&bytes[..end], |_| panic!("accepted truncated ack"));
        }
        bytes[19] ^= 1;
        events(&bytes, |_| panic!("accepted corrupt checksum"));
        for len in 0..=255 {
            bytes[12] = len;
            bytes[13] = !len;
            events(&bytes, |_| panic!("accepted invalid packet"));
        }
    }
}
