#![no_std]

pub use hid::{Led, LedReport};
pub use keyboard::Matrix;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

#[derive(Clone, Copy, Debug, Default, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct Status {
    pub awake: u8,
    pub transport: u8,
    pub leds: LedReport,
    pub reserved: u8,
}

#[allow(clippy::pedantic, reason = "generated")]
mod stub {
    use super::Status;
    use keyboard::Matrix;
    use userlib::sys_send;

    include!(concat!(env!("OUT_DIR"), "/client_stub.rs"));
}
pub use stub::*;
