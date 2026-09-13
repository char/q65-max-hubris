#![no_std]
#![no_main]

use core::convert::Infallible;
use idol_runtime::{NotificationHandler, RequestError};
use lkbt51::link::{Action, Link, State};
use stm32f4::stm32f401 as pac;
use userlib::{RecvMessage, sys_get_timer, sys_set_timer, task_slot};
use wireless_api::{Reports, Status};

task_slot!(SPI, spi);

#[unsafe(export_name = "main")]
fn main() -> ! {
    let gpiob = unsafe { &*pac::GPIOB::ptr() };
    let gpioc = unsafe { &*pac::GPIOC::ptr() };
    gpiob.pupdr.modify(|_, w| w.pupdr1().pull_up());
    gpiob.moder.modify(|_, w| w.moder1().input());
    gpioc.bsrr.write(|w| w.br4().set_bit());
    gpioc.moder.modify(|_, w| w.moder4().output());
    let mut wireless = Wireless {
        spi: spi_api::Spi::from(SPI.get_task_id()),
        link: Link::default(),
        gpiob,
        gpioc,
    };
    let mut incoming = [0; idl::INCOMING_SIZE];
    sys_set_timer(Some(sys_get_timer().now), notifications::TIMER_MASK);
    loop {
        idol_runtime::dispatch(&mut incoming, &mut wireless);
    }
}

struct Wireless {
    spi: spi_api::Spi,
    link: Link,
    gpiob: &'static pac::gpiob::RegisterBlock,
    gpioc: &'static pac::gpioh::RegisterBlock,
}

impl NotificationHandler for Wireless {
    fn current_notification_mask(&self) -> u32 {
        notifications::TIMER_MASK
    }

    fn handle_notification(&mut self, _: userlib::NotificationBits) {
        let now = sys_get_timer().now;
        if self.link.can_read() && self.gpiob.idr.read().idr1().bit_is_clear() {
            let mut bytes = [0; lkbt51::READ_SIZE];
            if self
                .spi
                .exchange(spi_api::RADIO, &lkbt51::READ, &mut bytes)
                .unwrap_or(false)
            {
                lkbt51::events(&bytes, |event| self.link.event(event, now));
            } else {
                self.link.failed(now);
            }
        }
        if let Some(action) = self.link.step(now) {
            let ok = match action {
                Action::Reset(low) => {
                    self.gpioc.bsrr.write(|w| {
                        if low {
                            w.br4().set_bit()
                        } else {
                            w.bs4().set_bit()
                        }
                    });
                    true
                }
                Action::Pulse(ms) => {
                    let ok = self.spi.radio_pulse(ms).unwrap_or(false);
                    if ok {
                        self.link.pulse_started(sys_get_timer().now);
                    }
                    ok
                }
                Action::Send(packet) => self
                    .spi
                    .write(spi_api::RADIO, packet.bytes())
                    .unwrap_or(false),
            };
            if !ok {
                self.link.failed(sys_get_timer().now);
            }
        }
        sys_set_timer(Some(sys_get_timer().now + 1), notifications::TIMER_MASK);
    }
}

impl idl::InOrderWirelessImpl for Wireless {
    fn enable(&mut self, _: &RecvMessage, enabled: bool) -> Result<(), RequestError<Infallible>> {
        self.link.enable(enabled, sys_get_timer().now);
        Ok(())
    }

    fn set_reports(
        &mut self,
        _: &RecvMessage,
        reports: Reports,
    ) -> Result<(), RequestError<Infallible>> {
        self.link.set_reports(reports);
        Ok(())
    }

    fn pair(&mut self, _: &RecvMessage) -> Result<(), RequestError<Infallible>> {
        self.link.pair(sys_get_timer().now);
        Ok(())
    }

    fn status(&mut self, _: &RecvMessage) -> Result<Status, RequestError<Infallible>> {
        Ok(Status {
            connected: u8::from(self.link.state == State::Connected),
            pairing: u8::from(self.link.state == State::Pairing),
            leds: self.link.leds,
            reserved: 0,
            errors: self.link.errors,
            resets: self.link.resets,
        })
    }
}

mod idl {
    use wireless_api::{Reports, Status};
    include!(concat!(env!("OUT_DIR"), "/server_stub.rs"));
}
include!(concat!(env!("OUT_DIR"), "/notifications.rs"));
