#![no_std]

pub const LED0: u8 = 0;
pub const LED1: u8 = 1;
pub const RADIO: u8 = 2;

#[allow(clippy::pedantic, reason = "generated")]
mod stub {
    use userlib::sys_send;
    include!(concat!(env!("OUT_DIR"), "/client_stub.rs"));
}
pub use stub::*;
