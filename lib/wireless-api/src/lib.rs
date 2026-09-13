#![no_std]

pub use hid::{LedReport, Reports};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

#[derive(Clone, Copy, Debug, Default, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct Status {
    pub connected: u8,
    pub pairing: u8,
    pub leds: LedReport,
    pub reserved: u8,
    pub errors: u32,
    pub resets: u32,
    pub overflows: u32,
}

#[allow(clippy::pedantic, reason = "generated")]
mod stub {
    use super::{Reports, Status};
    use userlib::sys_send;
    include!(concat!(env!("OUT_DIR"), "/client_stub.rs"));
}
pub use stub::*;
