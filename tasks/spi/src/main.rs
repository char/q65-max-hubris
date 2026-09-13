#![no_std]
#![no_main]

mod f401;

use core::convert::Infallible;
use idol_runtime::{Leased, LenLimit, NotificationHandler, R, RequestError, W};
use userlib::{RecvMessage, sys_get_timer, sys_set_timer};

include!(concat!(env!("OUT_DIR"), "/caller.rs"));

#[unsafe(export_name = "main")]
fn main() -> ! {
    let mut server = Server {
        controller: f401::Controller::init(),
        pulse_until: None,
    };
    let mut incoming = [0; idl::INCOMING_SIZE];
    loop {
        idol_runtime::dispatch(&mut incoming, &mut server);
    }
}

struct Server {
    controller: f401::Controller,
    pulse_until: Option<u64>,
}

impl Server {
    fn busy(&mut self) -> bool {
        if let Some(deadline) = self.pulse_until
            && sys_get_timer().now >= deadline
        {
            self.controller.select(spi_api::RADIO, false);
            self.pulse_until = None;
        }
        self.pulse_until.is_some()
    }
}

impl NotificationHandler for Server {
    fn current_notification_mask(&self) -> u32 {
        notifications::TIMER_MASK
    }
    fn handle_notification(&mut self, _: userlib::NotificationBits) {
        self.busy();
    }
}

impl idl::InOrderSpiImpl for Server {
    fn radio_pulse(
        &mut self,
        message: &RecvMessage,
        ms: u16,
    ) -> Result<bool, RequestError<Infallible>> {
        if message.sender.index() != WIRELESS_INDEX || ms == 0 || ms > 100 || self.busy() {
            return Ok(false);
        }
        let deadline = sys_get_timer().now + u64::from(ms);
        self.controller.select(spi_api::RADIO, true);
        self.pulse_until = Some(deadline);
        sys_set_timer(Some(deadline), notifications::TIMER_MASK);
        Ok(true)
    }

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
        Ok(!self.busy() && self.controller.transfer(device, bytes))
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
        let ok = !self.busy() && self.controller.transfer(device, bytes);
        if ok {
            return Ok(sink.write_range(0..bytes.len(), bytes).is_ok());
        }
        Ok(ok)
    }
}

mod idl {
    include!(concat!(env!("OUT_DIR"), "/server_stub.rs"));
}
include!(concat!(env!("OUT_DIR"), "/notifications.rs"));
