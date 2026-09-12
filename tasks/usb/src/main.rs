// vibed

#![no_std]
#![no_main]

mod otg;

use otg::{Event, Otg};
use stm32f4::stm32f401 as pac;
use userlib::{sys_irq_control, sys_recv_notification};

/// Bit 0 is the `usb-irq` notification declared in app.toml.
const USB_IRQ: u32 = 1;

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

    let mut otg = Otg::new();
    loop {
        sys_irq_control(USB_IRQ, true);
        sys_recv_notification(USB_IRQ);
        while let Some(event) = otg.next_event() {
            if let Event::Setup(setup) = event {
                stub::answer(&otg, setup);
            }
        }
    }
}

/// Just enough of a device to show up in `dmesg`, so the driver can be checked on its own. Replaced
/// by the real device model in `lib/usb`.
mod stub {
    use super::Otg;

    const DEVICE_DESCRIPTOR: [u8; 18] = [
        18, 1, 0x00, 0x02, 0, 0, 0, 64, 0x34, 0x34, 0xb0, 0x08, 0x01, 0x00, 0, 0, 0, 1,
    ];

    pub fn answer(otg: &Otg, setup: [u8; 8]) {
        let [
            request_type,
            request,
            value_low,
            value_high,
            _,
            _,
            length_low,
            length_high,
        ] = setup;
        let length = usize::from(u16::from_le_bytes([length_low, length_high]));
        match (request_type, request, value_high) {
            (0x80, 6, 1) => otg.send(0, &DEVICE_DESCRIPTOR[..length.min(18)]),
            (0x00, 5, _) => {
                otg.set_address(value_low);
                otg.send(0, &[]);
            }
            _ => otg.stall_control(),
        }
    }
}
