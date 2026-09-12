#![no_std]

pub use keyboard::Matrix;

#[allow(clippy::pedantic, reason = "generated")]
mod stub {
    use keyboard::Matrix;
    use userlib::sys_send;

    include!(concat!(env!("OUT_DIR"), "/client_stub.rs"));
}
pub use stub::*;
