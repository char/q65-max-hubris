#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Transport {
    #[default]
    Off,
    Usb,
    Wireless,
}

#[derive(Default)]
pub struct ModeSwitch {
    pub active: Transport,
    candidate: Option<Transport>,
    since: u64,
}

impl ModeSwitch {
    /// PA9 in bit 1, PA10 in bit 0. Bluetooth intentionally has no transport.
    pub fn update(&mut self, pins: u8, now: u64) -> Option<Transport> {
        let candidate = match pins {
            1 => Some(Transport::Off),
            2 => Some(Transport::Wireless),
            3 => Some(Transport::Usb),
            _ => None,
        };
        if candidate != self.candidate {
            self.candidate = candidate;
            self.since = now;
        }
        let candidate = candidate?;
        if candidate != self.active && now - self.since >= 100 {
            self.active = candidate;
            Some(candidate)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_waits_for_the_switch_without_leaking_reports_to_usb() {
        let mut switch = ModeSwitch::default();
        assert_eq!(switch.active, Transport::Off);
        assert_eq!(switch.update(2, 0), None);
        assert_eq!(switch.update(2, 99), None);
        assert_eq!(switch.update(2, 100), Some(Transport::Wireless));
        assert_eq!(switch.update(2, 200), None);
    }

    #[test]
    fn invalid_and_bouncing_contacts_do_not_switch_hosts() {
        let mut switch = ModeSwitch::default();
        switch.update(3, 0);
        assert_eq!(switch.update(3, 100), Some(Transport::Usb));
        for (pins, now) in [(2, 110), (0, 120), (2, 500), (3, 510), (2, 520), (2, 619)] {
            assert_eq!(switch.update(pins, now), None);
        }
        assert_eq!(switch.update(2, 620), Some(Transport::Wireless));
        switch.update(1, 700);
        assert_eq!(switch.update(1, 800), Some(Transport::Off));
    }
}
