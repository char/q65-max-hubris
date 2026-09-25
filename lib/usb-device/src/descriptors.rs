use crate::Interface;
use util::Bytes;

const DEVICE_TYPE: u8 = 1;
const CONFIGURATION_TYPE: u8 = 2;
const STRING_TYPE: u8 = 3;
const INTERFACE_TYPE: u8 = 4;
const ENDPOINT_TYPE: u8 = 5;
pub const HID_TYPE: u8 = 0x21;
pub const REPORT_TYPE: u8 = 0x22;

pub const PRODUCT_STRING: u8 = 1;

pub const DEVICE: [u8; 18] = [
    18,
    DEVICE_TYPE,
    0x00,
    0x02, // USB 2.0
    0,
    0,
    0,  // class, subclass, protocol: declared per interface
    64, // endpoint 0 max packet
    0x34,
    0x34, // Keychron
    0xb0,
    0x08, // Q65 Max
    0x00,
    0x01, // device version 1.00
    0,    // no manufacturer string
    PRODUCT_STRING,
    0, // no serial
    1, // one configuration
];

pub const LANGUAGES: [u8; 4] = [4, STRING_TYPE, 0x09, 0x04]; // US English

const PRODUCT_NAME: &str = "Keychron Q65 Max (Hubris)";
pub const PRODUCT: [u8; 2 + 2 * PRODUCT_NAME.len()] = string_descriptor(PRODUCT_NAME);

/// ascii -> utf-16le
const fn string_descriptor<const N: usize>(text: &str) -> [u8; N] {
    let text = text.as_bytes();
    assert!(N == 2 + 2 * text.len());
    let mut bytes = [0; N];
    bytes[0] = N as u8;
    bytes[1] = STRING_TYPE;
    let mut i = 0;
    while i < text.len() {
        assert!(text[i].is_ascii());
        bytes[2 + 2 * i] = text[i];
        i += 1;
    }
    bytes
}

const fn hid_descriptor(report_descriptor_len: usize) -> [u8; 9] {
    let [low, high] = (report_descriptor_len as u16).to_le_bytes();
    [
        9,
        HID_TYPE,
        0x11,
        0x01, // HID 1.11
        0,    // not localised
        1,    // one class descriptor follows:
        REPORT_TYPE,
        low,
        high,
    ]
}

pub const HID: [[u8; 9]; 2] = [
    hid_descriptor(Interface::Keyboard.report_descriptor().len()),
    hid_descriptor(Interface::Consumer.report_descriptor().len()),
];

const INTERFACE_LEN: usize = 9 + 9 + 7;

const fn interface(interface: Interface) -> [u8; INTERFACE_LEN] {
    // Subclass 1 / protocol 1 is the boot keyboard that BIOSes know how to drive.
    let (subclass, protocol) = match interface {
        Interface::Keyboard => (1, 1),
        Interface::Consumer => (0, 0),
    };
    let header = [
        9,
        INTERFACE_TYPE,
        interface as u8,
        0, // no alternate settings
        1, // one endpoint
        3, // HID class
        subclass,
        protocol,
        0, // no string
    ];
    let endpoint = [
        7,
        ENDPOINT_TYPE,
        0x80 | interface.endpoint(), // IN
        0b11,                        // interrupt
        interface.max_packet(),
        0,
        1, // poll every millisecond
    ];
    let mut out = Bytes::new();
    out.extend(&header);
    out.extend(&HID[interface as usize]);
    out.extend(&endpoint);
    out.finish()
}

const CONFIGURATION_LEN: usize = 9 + 2 * INTERFACE_LEN;

pub const CONFIGURATION: [u8; CONFIGURATION_LEN] = {
    let [low, high] = (CONFIGURATION_LEN as u16).to_le_bytes();
    let header = [
        9,
        CONFIGURATION_TYPE,
        low,
        high,
        2,    // interfaces
        1,    // this configuration's number
        0,    // no string
        0xa0, // bus powered, remote wakeup
        250,  // 500 mA, in 2 mA units
    ];
    let mut out = Bytes::new();
    out.extend(&header);
    out.extend(&interface(Interface::Keyboard));
    out.extend(&interface(Interface::Consumer));
    out.finish()
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Splits a descriptor set at each bLength, as a host parser does.
    fn descriptors(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
        let mut rest = bytes;
        core::iter::from_fn(move || {
            let (head, tail) = rest.split_at(usize::from(*rest.first()?));
            rest = tail;
            Some(head)
        })
    }

    #[test]
    fn configuration_is_internally_consistent() {
        let total = u16::from_le_bytes([CONFIGURATION[2], CONFIGURATION[3]]);
        assert_eq!(usize::from(total), CONFIGURATION.len());
        let types: Vec<u8> = descriptors(&CONFIGURATION).map(|d| d[1]).collect();
        assert_eq!(
            types,
            [
                CONFIGURATION_TYPE,
                INTERFACE_TYPE,
                HID_TYPE,
                ENDPOINT_TYPE,
                INTERFACE_TYPE,
                HID_TYPE,
                ENDPOINT_TYPE
            ]
        );
        let mut interfaces = Interface::ALL.into_iter();
        for descriptor in descriptors(&CONFIGURATION) {
            match descriptor[1] {
                HID_TYPE => {
                    let interface = interfaces.next().unwrap();
                    let declared = u16::from_le_bytes([descriptor[7], descriptor[8]]);
                    assert_eq!(usize::from(declared), interface.report_descriptor().len());
                }
                ENDPOINT_TYPE => assert!(descriptor[2] & 0x80 != 0 && descriptor[4] > 0),
                _ => {}
            }
        }
    }

    #[test]
    fn product_string_is_utf16() {
        assert_eq!(usize::from(PRODUCT[0]), PRODUCT.len());
        let text: Vec<u16> = PRODUCT[2..]
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(String::from_utf16(&text).unwrap(), PRODUCT_NAME);
    }
}
