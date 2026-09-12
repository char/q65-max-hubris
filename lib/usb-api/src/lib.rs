#![no_std]

pub use hid::{Led, LedReport, Reports};

#[allow(clippy::pedantic, reason = "generated")]
mod stub {
    use hid::{LedReport, Reports};
    use userlib::sys_send;

    include!(concat!(env!("OUT_DIR"), "/client_stub.rs"));
}
pub use stub::*;
