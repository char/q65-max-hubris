#![no_std]
#![no_main]

mod matrix;

use idol_runtime::{NotificationHandler, RequestError};
use keyboard::transport::{ModeSwitch, Transport};
use keyboard::{Command, Debouncer, Encoder, Keymap, Matrix};
use usb_api::{Reports, Usb};
use userlib::{RecvMessage, sys_get_timer, sys_set_timer, task_slot};
use wireless_api::Wireless;

task_slot!(JEFE, jefe);
task_slot!(USB, usb);
task_slot!(WIRELESS, wireless);

#[unsafe(export_name = "main")]
fn main() -> ! {
    // Higher-priority peripheral tasks finish GPIO configuration before matrix init.
    matrix::init();
    let mut input = Input {
        debouncer: Debouncer::default(),
        keymap: Keymap::default(),
        encoder: Encoder::new(matrix::encoder_state()),
        usb: Usb::from(USB.get_task_id()),
        wireless: Wireless::from(WIRELESS.get_task_id()),
        mode: ModeSwitch::default(),
        keys: Matrix::default(),
        last_activity: sys_get_timer().now,
    };
    let mut incoming = [0; idl::INCOMING_SIZE];
    sys_set_timer(Some(sys_get_timer().now), notifications::TIMER_MASK);
    loop {
        idol_runtime::dispatch(&mut incoming, &mut input);
    }
}

struct Input {
    debouncer: Debouncer,
    keymap: Keymap,
    encoder: Encoder,
    usb: Usb,
    wireless: Wireless,
    mode: ModeSwitch,
    keys: Matrix,
    last_activity: u64,
}

impl NotificationHandler for Input {
    fn current_notification_mask(&self) -> u32 {
        notifications::TIMER_MASK
    }

    fn handle_notification(&mut self, _: userlib::NotificationBits) {
        let start = sys_get_timer().now;
        let previous = self.mode.active;
        if let Some(transport) = self.mode.update(matrix::mode_switch(), start) {
            self.last_activity = start;
            match previous {
                Transport::Usb => self.usb.set_reports(Reports::default()),
                Transport::Wireless => self.wireless.enable(false),
                Transport::Off => {}
            }
            let reports = self.keymap.report();
            match transport {
                Transport::Usb => self.usb.set_reports(reports),
                Transport::Wireless => {
                    self.wireless.set_reports(reports);
                    self.wireless.enable(true);
                }
                Transport::Off => {}
            }
        }
        let keys = self.debouncer.update(matrix::scan(), start);
        if keys != self.keys {
            self.last_activity = start;
        }
        self.keys = keys;
        let send = |reports: Reports| match self.mode.active {
            Transport::Usb => self.usb.set_reports(reports),
            Transport::Wireless => self.wireless.set_reports(reports),
            Transport::Off => {}
        };
        if let Some(Command::EnterBootloader) = self.keymap.update(self.keys, start, send) {
            self.usb.set_reports(Reports::default());
            self.wireless.enable(false);
            // Give the radio's release/disconnect sequence time before entering ROM.
            userlib::hl::sleep_for(250);
            jefe_api::enter_bootloader(JEFE.get_task_id());
        }
        if let Some(rotation) = self.encoder.update(matrix::encoder_state()) {
            self.last_activity = start;
            self.keymap.turn(rotation, send);
        }
        // One scan per millisecond. If we ever fall behind, skip the missed ticks rather than
        // scanning in a burst to catch up.
        // The deadline must be in the future or an overrun can starve the RGB task.
        let next = sys_get_timer().now + 1;
        sys_set_timer(Some(next), notifications::TIMER_MASK);
    }
}

impl idl::InOrderInputImpl for Input {
    fn status(
        &mut self,
        _: &RecvMessage,
    ) -> Result<input_api::Status, RequestError<core::convert::Infallible>> {
        let (awake, leds, backlight) = match self.mode.active {
            Transport::Usb => {
                let awake = u8::from(self.usb.is_awake());
                (awake, self.usb.leds(), awake)
            }
            Transport::Wireless => {
                let status = self.wireless.status();
                let recent = self.keys.iter().any(|&row| row != 0)
                    || sys_get_timer().now - self.last_activity < 600_000;
                let powered = status.flags & wireless_api::USB_POWER != 0
                    || (status.flags & wireless_api::VALID != 0
                        && status.flags & (wireless_api::LOW | wireless_api::CRITICAL) == 0);
                (
                    status.connected,
                    status.leds,
                    u8::from(status.connected != 0 && recent && powered),
                )
            }
            Transport::Off => (0, input_api::LedReport::default(), 0),
        };
        Ok(input_api::Status {
            awake,
            leds,
            transport: self.mode.active as u8,
            backlight,
        })
    }

    fn keys(&mut self, _: &RecvMessage) -> Result<Matrix, RequestError<core::convert::Infallible>> {
        Ok(self.keys)
    }
}

mod idl {
    use input_api::Status;
    use keyboard::Matrix;

    include!(concat!(env!("OUT_DIR"), "/server_stub.rs"));
}

include!(concat!(env!("OUT_DIR"), "/notifications.rs"));
