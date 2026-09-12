// vibed

#![no_std]
#![no_main]

mod otg;

use hid::Reports;
use idol_runtime::{NotificationHandler, RequestError};
use otg::{Event, Otg};
use stm32f4::stm32f401 as pac;
use usb_device::{Action, Device, Interface};
use userlib::{NotificationBits, RecvMessage, sys_irq_control};

#[unsafe(export_name = "main")]
fn main() -> ! {
    // PA11/PA12 are D-/D+ on alternate function 10.
    let gpioa = unsafe { &*pac::GPIOA::ptr() };
    gpioa
        .moder
        .modify(|_, w| w.moder11().alternate().moder12().alternate());
    gpioa.afrh.modify(|_, w| w.afrh11().af10().afrh12().af10());
    gpioa.ospeedr.modify(|_, w| {
        w.ospeedr11()
            .very_high_speed()
            .ospeedr12()
            .very_high_speed()
    });

    let mut usb = Usb {
        otg: Otg::new(),
        device: Device::default(),
        pending: [Pending::default(); 2],
    };
    let mut incoming = [0; idl::INCOMING_SIZE];
    sys_irq_control(notifications::USB_IRQ_MASK, true);
    loop {
        idol_runtime::dispatch(&mut incoming, &mut usb);
    }
}

impl idl::InOrderUsbImpl for Usb {
    /// Replies once the report is accepted, not once the host has it: the input task must never be
    /// held up by a slow or absent host.
    fn set_reports(
        &mut self,
        _: &RecvMessage,
        reports: Reports,
    ) -> Result<(), RequestError<core::convert::Infallible>> {
        self.set_reports(reports);
        Ok(())
    }
}

impl NotificationHandler for Usb {
    fn current_notification_mask(&self) -> u32 {
        notifications::USB_IRQ_MASK
    }

    fn handle_notification(&mut self, _: NotificationBits) {
        while let Some(event) = self.otg.next_event() {
            self.handle(event);
        }
        sys_irq_control(notifications::USB_IRQ_MASK, true);
    }
}

struct Usb {
    otg: Otg,
    device: Device,
    pending: [Pending; 2],
}

/// One report transfer may be in flight per interface. Changes that arrive meanwhile are coalesced
/// into a single send of the latest state when it completes; with debouncing in front of us, no
/// key transition is brief enough to be lost that way.
#[derive(Clone, Copy, Default)]
struct Pending {
    in_flight: bool,
    stale: bool,
}

impl Usb {
    fn handle(&mut self, event: Event) {
        match event {
            Event::Reset => {
                self.device.reset();
                self.pending = [Pending::default(); 2];
            }
            Event::Setup(setup) => {
                let action = self.device.setup(setup);
                self.apply(action);
            }
            Event::Out(packet) => {
                let action = self.device.out(&packet);
                self.apply(action);
            }
            Event::InComplete(endpoint) => {
                if let Some(interface) = Interface::for_endpoint(endpoint) {
                    self.pending[interface as usize].in_flight = false;
                    if self.pending[interface as usize].stale {
                        self.send(interface);
                    }
                }
            }
            Event::Suspend | Event::Resume => {}
        }
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::None => {}
            Action::Send(response) => self.otg.send(0, &response),
            Action::Stall => self.otg.stall_control(),
            Action::SetAddress(address) => {
                self.otg.set_address(address);
                self.status();
            }
            Action::Configure(configured) => {
                for interface in Interface::ALL {
                    if configured {
                        self.open(interface);
                    } else {
                        self.otg.close_in(interface.endpoint());
                    }
                }
                self.status();
            }
            Action::ResetEndpoint(interface) => {
                self.open(interface);
                self.status();
            }
            Action::SetProtocol => {
                self.refresh(Interface::Keyboard);
                self.status();
            }
        }
    }

    fn status(&self) {
        self.otg.send(0, &[]);
    }

    /// Bring an interface's endpoint up fresh and tell the host where the keys stand.
    fn open(&mut self, interface: Interface) {
        self.otg
            .open_in(interface.endpoint(), interface.max_packet());
        self.pending[interface as usize] = Pending::default();
        self.send(interface);
    }

    fn set_reports(&mut self, reports: Reports) {
        let previous = core::mem::replace(&mut self.device.reports, reports);
        if !self.device.configured() {
            return;
        }
        if reports.keyboard != previous.keyboard {
            self.refresh(Interface::Keyboard);
        }
        if reports.consumer != previous.consumer {
            self.refresh(Interface::Consumer);
        }
    }

    fn refresh(&mut self, interface: Interface) {
        if self.pending[interface as usize].in_flight {
            self.pending[interface as usize].stale = true;
        } else {
            self.send(interface);
        }
    }

    fn send(&mut self, interface: Interface) {
        self.otg
            .send(interface.endpoint(), &self.device.report(interface));
        self.pending[interface as usize] = Pending {
            in_flight: true,
            stale: false,
        };
    }
}

mod idl {
    use hid::Reports;

    include!(concat!(env!("OUT_DIR"), "/server_stub.rs"));
}

include!(concat!(env!("OUT_DIR"), "/notifications.rs"));
