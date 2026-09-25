#![cfg_attr(not(test), no_std)]

mod descriptors;

use hid::{KEYBOARD_REPORT_SIZE, LedReport, Reports, descriptor};
use util::Bytes;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Interface {
    Keyboard,
    Consumer,
}

impl Interface {
    pub const ALL: [Self; 2] = [Self::Keyboard, Self::Consumer];

    #[must_use]
    pub const fn endpoint(self) -> u8 {
        self as u8 + 1
    }

    #[must_use]
    pub const fn max_packet(self) -> u8 {
        match self {
            Self::Keyboard => KEYBOARD_REPORT_SIZE as u8,
            Self::Consumer => 1,
        }
    }

    const fn report_descriptor(self) -> &'static [u8] {
        match self {
            Self::Keyboard => &descriptor::KEYBOARD,
            Self::Consumer => &descriptor::CONSUMER,
        }
    }

    fn from_number(number: u16) -> Option<Self> {
        Self::ALL.get(usize::from(number)).copied()
    }

    #[must_use]
    pub fn for_endpoint(endpoint: u8) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|interface| interface.endpoint() == endpoint)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Protocol {
    Boot = 0,
    #[default]
    Report = 1,
}

/// The longest thing we ever answer a control transfer with.
const MAX_RESPONSE: usize = longest(&[
    descriptors::DEVICE.len(),
    descriptors::CONFIGURATION.len(),
    descriptors::PRODUCT.len(),
    descriptor::KEYBOARD.len(),
    descriptor::CONSUMER.len(),
    KEYBOARD_REPORT_SIZE,
]);

pub type Response = Bytes<MAX_RESPONSE>;

const fn longest(lengths: &[usize]) -> usize {
    let mut max = 0;
    let mut i = 0;
    while i < lengths.len() {
        if lengths[i] > max {
            max = lengths[i];
        }
        i += 1;
    }
    max
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Send(Response),
    SetAddress(u8),
    Configure(bool),
    ResetEndpoint(Interface),
    SetProtocol,
    Stall,
}

const STATUS: Action = Action::Send(Response::new());

#[derive(Default)]
pub struct Device {
    pub reports: Reports,
    address: u8,
    configured: bool,
    protocol: Protocol,
    leds: LedReport,
    awaiting_leds: bool,
    remote_wakeup: bool,
}

// Standard requests
const GET_STATUS: u8 = 0;
const CLEAR_FEATURE: u8 = 1;
const SET_FEATURE: u8 = 3;
const SET_ADDRESS: u8 = 5;
const GET_DESCRIPTOR: u8 = 6;
const GET_CONFIGURATION: u8 = 8;
const SET_CONFIGURATION: u8 = 9;
const GET_INTERFACE: u8 = 10;
const SET_INTERFACE: u8 = 11;
const ENDPOINT_HALT: u16 = 0;
const DEVICE_REMOTE_WAKEUP: u16 = 1;
// HID class requests
const GET_REPORT: u8 = 1;
const GET_IDLE: u8 = 2;
const GET_PROTOCOL: u8 = 3;
const SET_REPORT: u8 = 9;
const SET_IDLE: u8 = 10;
const SET_PROTOCOL: u8 = 11;
/// wValue of a `SET_REPORT` for output report 0.
const OUTPUT_REPORT: u16 = 0x0200;

impl Device {
    pub fn reset(&mut self) {
        *self = Self {
            reports: self.reports,
            ..Self::default()
        };
    }

    #[must_use]
    pub fn configured(&self) -> bool {
        self.configured
    }

    #[must_use]
    pub fn leds(&self) -> LedReport {
        self.leds
    }

    /// Whether the host has allowed us to wake it from suspend.
    #[must_use]
    pub fn may_wake_host(&self) -> bool {
        self.remote_wakeup
    }

    #[must_use]
    pub fn report(&self, interface: Interface) -> Response {
        match (interface, self.protocol) {
            (Interface::Keyboard, Protocol::Boot) => {
                Response::from_slice(&self.reports.keyboard.to_boot())
            }
            (Interface::Keyboard, Protocol::Report) => {
                Response::from_slice(&self.reports.keyboard.0)
            }
            (Interface::Consumer, _) => Response::from_slice(&[self.reports.consumer.0]),
        }
    }

    pub fn setup(&mut self, packet: [u8; 8]) -> Action {
        self.awaiting_leds = false;
        let setup = Setup::from(packet);
        let action = match setup.request {
            GET_DESCRIPTOR => return Self::descriptor(&setup),
            _ if setup.class && !self.configured => Action::Stall,
            _ if setup.class => self.class_request(&setup),
            _ => self.standard_request(&setup),
        };
        match action {
            Action::Send(response) => Action::Send(response.truncated(usize::from(setup.length))),
            other => other,
        }
    }

    fn descriptor(setup: &Setup) -> Action {
        const DEVICE: u8 = 1;
        const CONFIGURATION: u8 = 2;
        const STRING: u8 = 3;
        let [index, kind] = setup.value.to_le_bytes();
        let interface = Interface::from_number(setup.index);
        let bytes: &[u8] = match (setup.recipient, kind, index, interface) {
            (Recipient::Device, DEVICE, 0, _) => &descriptors::DEVICE,
            (Recipient::Device, CONFIGURATION, 0, _) => &descriptors::CONFIGURATION,
            (Recipient::Device, STRING, 0, _) => &descriptors::LANGUAGES,
            (Recipient::Device, STRING, descriptors::PRODUCT_STRING, _) => &descriptors::PRODUCT,
            (Recipient::Interface, descriptors::HID_TYPE, 0, Some(interface)) => {
                &descriptors::HID[interface as usize]
            }
            (Recipient::Interface, descriptors::REPORT_TYPE, 0, Some(interface)) => {
                interface.report_descriptor()
            }
            _ => return Action::Stall,
        };
        Action::Send(Response::from_slice(bytes).truncated(usize::from(setup.length)))
    }

    fn standard_request(&mut self, setup: &Setup) -> Action {
        match (setup.recipient, setup.request) {
            (Recipient::Device, SET_ADDRESS) if !self.configured => {
                self.address = setup.value as u8;
                Action::SetAddress(self.address)
            }
            (Recipient::Device, SET_CONFIGURATION) if self.address != 0 && setup.value <= 1 => {
                self.configured = setup.value == 1;
                Action::Configure(self.configured)
            }
            (Recipient::Device, GET_CONFIGURATION) => {
                Action::Send(Response::from_slice(&[u8::from(self.configured)]))
            }
            (Recipient::Device, GET_STATUS) => Action::Send(Response::from_slice(&[
                u8::from(self.remote_wakeup) << 1,
                0,
            ])),
            (_, GET_STATUS) => Action::Send(Response::from_slice(&[0, 0])),
            (Recipient::Device, SET_FEATURE | CLEAR_FEATURE)
                if setup.value == DEVICE_REMOTE_WAKEUP =>
            {
                self.remote_wakeup = setup.request == SET_FEATURE;
                STATUS
            }
            (Recipient::Interface, GET_INTERFACE) if self.configured => {
                Action::Send(Response::from_slice(&[0]))
            }
            (Recipient::Interface, SET_INTERFACE) if self.configured => STATUS,
            (Recipient::Endpoint, CLEAR_FEATURE) if setup.value == ENDPOINT_HALT => {
                let [address, high] = setup.index.to_le_bytes();
                match Interface::for_endpoint(address & 0x7f) {
                    Some(interface) if self.configured && high == 0 && address & 0x80 != 0 => {
                        Action::ResetEndpoint(interface)
                    }
                    _ => Action::Stall,
                }
            }
            _ => Action::Stall,
        }
    }

    fn class_request(&mut self, setup: &Setup) -> Action {
        let Some(interface) = Interface::from_number(setup.index) else {
            return Action::Stall;
        };
        match (setup.request, interface) {
            (GET_REPORT, _) => Action::Send(self.report(interface)),
            (SET_REPORT, Interface::Keyboard)
                if setup.value == OUTPUT_REPORT && setup.length == 1 =>
            {
                self.awaiting_leds = true;
                Action::None
            }
            // always 0 for GET_IDLE (like ZMK/tinyusb)
            (GET_IDLE, _) => Action::Send(Response::from_slice(&[0])),
            (SET_IDLE, _) => STATUS,
            (GET_PROTOCOL, Interface::Keyboard) => {
                Action::Send(Response::from_slice(&[self.protocol as u8]))
            }
            (SET_PROTOCOL, Interface::Keyboard) if setup.value <= 1 => {
                self.protocol = if setup.value == 0 {
                    Protocol::Boot
                } else {
                    Protocol::Report
                };
                Action::SetProtocol
            }
            _ => Action::Stall,
        }
    }

    /// A data packet from the host on endpoint 0, or its empty status acknowledgement.
    pub fn out(&mut self, data: &[u8]) -> Action {
        match (self.awaiting_leds, data) {
            (true, &[leds]) => {
                self.awaiting_leds = false;
                self.leds = LedReport(leds);
                STATUS
            }
            (false, []) => Action::None,
            _ => {
                self.awaiting_leds = false;
                Action::Stall
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Recipient {
    Device,
    Interface,
    Endpoint,
    Other,
}

struct Setup {
    class: bool,
    recipient: Recipient,
    request: u8,
    value: u16,
    index: u16,
    length: u16,
}

impl From<[u8; 8]> for Setup {
    fn from(packet: [u8; 8]) -> Self {
        let [
            request_type,
            request,
            value @ ..,
            index_low,
            index_high,
            length_low,
            length_high,
        ] = packet;
        let [value_low, value_high] = value;
        Self {
            class: request_type >> 5 & 0b11 == 1,
            recipient: match request_type & 0b11111 {
                0 => Recipient::Device,
                1 => Recipient::Interface,
                2 => Recipient::Endpoint,
                _ => Recipient::Other,
            },
            request,
            value: u16::from_le_bytes([value_low, value_high]),
            index: u16::from_le_bytes([index_low, index_high]),
            length: u16::from_le_bytes([length_low, length_high]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hid::{Key, Led};

    const STANDARD_OUT: u8 = 0x00;
    const STANDARD_IN: u8 = 0x80;
    const STANDARD_INTERFACE_IN: u8 = 0x81;
    const STANDARD_ENDPOINT_OUT: u8 = 0x02;
    const CLASS_OUT: u8 = 0x21;
    const CLASS_IN: u8 = 0xa1;

    fn setup(request_type: u8, request: u8, value: u16, index: u16, length: u16) -> [u8; 8] {
        let [vl, vh] = value.to_le_bytes();
        let [il, ih] = index.to_le_bytes();
        let [ll, lh] = length.to_le_bytes();
        [request_type, request, vl, vh, il, ih, ll, lh]
    }

    fn sent(action: Action) -> Vec<u8> {
        match action {
            Action::Send(response) => response.to_vec(),
            other => panic!("expected Send, got {other:?}"),
        }
    }

    fn configured() -> Device {
        let mut device = Device::default();
        assert_eq!(
            device.setup(setup(STANDARD_OUT, SET_ADDRESS, 9, 0, 0)),
            Action::SetAddress(9)
        );
        assert_eq!(
            device.setup(setup(STANDARD_OUT, SET_CONFIGURATION, 1, 0, 0)),
            Action::Configure(true)
        );
        device
    }

    #[test]
    fn enumerates_the_way_linux_does() {
        let mut device = Device::default();
        let first = sent(device.setup(setup(STANDARD_IN, GET_DESCRIPTOR, 0x0100, 0, 8)));
        assert_eq!(first, descriptors::DEVICE[..8]);
        assert_eq!(device.out(&[]), Action::None);
        let mut device = configured();
        let config = sent(device.setup(setup(STANDARD_IN, GET_DESCRIPTOR, 0x0200, 0, 255)));
        assert_eq!(config, descriptors::CONFIGURATION);
        let product = sent(device.setup(setup(STANDARD_IN, GET_DESCRIPTOR, 0x0301, 0x0409, 255)));
        assert_eq!(product, descriptors::PRODUCT);
        let report =
            sent(device.setup(setup(STANDARD_INTERFACE_IN, GET_DESCRIPTOR, 0x2200, 1, 255)));
        assert_eq!(report, descriptor::CONSUMER);
        assert_eq!(
            sent(device.setup(setup(STANDARD_IN, GET_CONFIGURATION, 0, 0, 1))),
            [1]
        );
    }

    #[test]
    fn configuration_requires_an_address_and_class_requests_require_a_configuration() {
        let mut device = Device::default();
        assert_eq!(
            device.setup(setup(STANDARD_OUT, SET_CONFIGURATION, 1, 0, 0)),
            Action::Stall
        );
        assert_eq!(
            device.setup(setup(CLASS_IN, GET_PROTOCOL, 0, 0, 1)),
            Action::Stall
        );
        assert!(!device.configured());
    }

    #[test]
    fn boot_protocol_changes_the_keyboard_report_format() {
        let mut device = configured();
        device.reports.keyboard.press(Key::A);
        assert_eq!(
            device.report(Interface::Keyboard).len(),
            KEYBOARD_REPORT_SIZE
        );
        assert_eq!(
            device.setup(setup(CLASS_OUT, SET_PROTOCOL, 0, 0, 0)),
            Action::SetProtocol
        );
        assert_eq!(
            &*device.report(Interface::Keyboard),
            [0, 0, 4, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            sent(device.setup(setup(CLASS_IN, GET_REPORT, 0x0100, 0, 8))),
            [0, 0, 4, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            sent(device.setup(setup(CLASS_IN, GET_PROTOCOL, 0, 0, 1))),
            [0]
        );
        assert_eq!(&*device.report(Interface::Consumer), [0]);
    }

    #[test]
    fn led_report_arrives_in_the_data_stage() {
        let mut device = configured();
        assert_eq!(
            device.setup(setup(CLASS_OUT, SET_REPORT, OUTPUT_REPORT, 0, 1)),
            Action::None
        );
        assert_eq!(device.out(&[0b10]), STATUS);
        assert!(device.leds().is_lit(Led::CapsLock));
        // A SETUP in between abandons the data stage.
        device.setup(setup(CLASS_OUT, SET_REPORT, OUTPUT_REPORT, 0, 1));
        device.setup(setup(STANDARD_IN, GET_STATUS, 0, 0, 2));
        assert_eq!(device.out(&[0b1]), Action::Stall);
        assert!(device.leds().is_lit(Led::CapsLock));
    }

    #[test]
    fn halt_clears_map_to_their_interface() {
        let mut device = configured();
        assert_eq!(
            device.setup(setup(
                STANDARD_ENDPOINT_OUT,
                CLEAR_FEATURE,
                ENDPOINT_HALT,
                0x82,
                0
            )),
            Action::ResetEndpoint(Interface::Consumer)
        );
        assert_eq!(
            device.setup(setup(
                STANDARD_ENDPOINT_OUT,
                CLEAR_FEATURE,
                ENDPOINT_HALT,
                0x83,
                0
            )),
            Action::Stall
        );
    }

    #[test]
    fn reset_forgets_the_host_but_not_the_keys() {
        let mut device = configured();
        device.reports.keyboard.press(Key::B);
        device.reset();
        assert!(!device.configured());
        assert_eq!(
            &*device.report(Interface::Keyboard),
            device.reports.keyboard.0
        );
        assert_ne!(device.reports, Reports::default());
    }

    #[test]
    fn no_response_needs_a_zero_length_packet() {
        // The driver sends each response as one transfer and never appends a ZLP, which is only
        // correct while nothing we send is an exact multiple of the packet size.
        let mut device = configured();
        let responses = [
            descriptors::DEVICE.len(),
            descriptors::CONFIGURATION.len(),
            descriptors::PRODUCT.len(),
            descriptors::LANGUAGES.len(),
            descriptor::KEYBOARD.len(),
            descriptor::CONSUMER.len(),
            device.report(Interface::Keyboard).len(),
        ];
        for len in responses {
            assert!(len > 0 && !len.is_multiple_of(64), "{len}");
        }
        device.setup(setup(CLASS_OUT, SET_PROTOCOL, 0, 0, 0));
        assert!(!device.report(Interface::Keyboard).len().is_multiple_of(64));
    }
}
