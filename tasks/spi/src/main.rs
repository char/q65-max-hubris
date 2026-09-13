#![no_std]
#![no_main]

mod f401;

use core::convert::Infallible;
use idol_runtime::{Leased, LenLimit, NotificationHandler, R, RequestError, W};
use userlib::RecvMessage;

#[unsafe(export_name = "main")]
fn main() -> ! {
    let mut server = Server(f401::Controller::init());
    let mut incoming = [0; idl::INCOMING_SIZE];
    loop {
        idol_runtime::dispatch(&mut incoming, &mut server);
    }
}

struct Server(f401::Controller);

impl NotificationHandler for Server {
    fn current_notification_mask(&self) -> u32 {
        0
    }
    fn handle_notification(&mut self, _: userlib::NotificationBits) {}
}

impl idl::InOrderSpiImpl for Server {
    fn write(
        &mut self,
        _: &RecvMessage,
        device: u8,
        source: LenLimit<Leased<R, [u8]>, 256>,
    ) -> Result<bool, RequestError<Infallible>> {
        let mut bytes = [0; 256];
        let bytes = &mut bytes[..source.len()];
        if source.read_range(0..bytes.len(), bytes).is_err() {
            return Ok(false);
        }
        Ok(self.0.transfer(device, bytes))
    }

    fn exchange(
        &mut self,
        _: &RecvMessage,
        device: u8,
        source: LenLimit<Leased<R, [u8]>, 256>,
        sink: LenLimit<Leased<W, [u8]>, 256>,
    ) -> Result<bool, RequestError<Infallible>> {
        if source.len() != sink.len() {
            return Ok(false);
        }
        let mut bytes = [0; 256];
        let bytes = &mut bytes[..source.len()];
        if source.read_range(0..bytes.len(), bytes).is_err() {
            return Ok(false);
        }
        let ok = self.0.transfer(device, bytes);
        if ok {
            return Ok(sink.write_range(0..bytes.len(), bytes).is_ok());
        }
        Ok(ok)
    }
}

mod idl {
    include!(concat!(env!("OUT_DIR"), "/server_stub.rs"));
}
