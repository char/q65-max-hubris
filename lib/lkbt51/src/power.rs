pub const USB_POWER: u8 = 1;
pub const CHARGING: u8 = 2;
pub const LOW: u8 = 4;
pub const CRITICAL: u8 = 8;
pub const VALID: u8 = 16;

#[derive(Default)]
pub struct Battery {
    pub millivolts: u16,
    usb: bool,
    charging: bool,
    sampled_at: Option<u64>,
    low_since: Option<u64>,
    critical_since: Option<u64>,
    pub critical: bool,
}

impl Battery {
    pub fn power(&mut self, usb: bool, charging: bool) {
        self.usb = usb;
        self.charging = usb && charging;
        if usb {
            self.low_since = None;
            self.critical_since = None;
            self.critical = false;
        }
    }

    pub fn sample(&mut self, raw: u16, now: u64) {
        // The module reports divider millivolts, not raw ADC counts.
        let mv = u32::from(raw) * (560 + 499) / 499;
        if !(2500..=4500).contains(&mv) {
            return;
        }
        self.millivolts = mv as u16;
        self.sampled_at = Some(now);
        if self.usb {
            return;
        }
        if mv < 3500 {
            self.low_since.get_or_insert(now);
        } else if mv >= 3600 {
            self.low_since = None;
        }
        if mv < 3300 {
            let since = *self.critical_since.get_or_insert(now);
            self.critical |= now - since >= 60_000;
        } else if mv >= 3400 {
            self.critical_since = None;
        }
    }

    #[must_use]
    pub fn flags(&self, now: u64) -> u8 {
        let valid = self.sampled_at.is_some_and(|sample| now - sample < 10_000);
        let low = !self.usb && self.low_since.is_some_and(|since| now - since >= 9000);
        (u8::from(self.usb) * USB_POWER)
            | (u8::from(self.charging) * CHARGING)
            | (u8::from(low) * LOW)
            | (u8::from(self.critical) * CRITICAL)
            | (u8::from(valid) * VALID)
    }

    /// Approximation matching QMK's voltage curve, not a coulomb counter.
    #[must_use]
    pub fn percent(&self) -> u8 {
        match self.millivolts {
            4100.. => 100,
            3500.. => ((self.millivolts - 3500) * 80 / 600 + 20) as u8,
            3300.. => ((self.millivolts - 3300) * 20 / 200) as u8,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divider_and_charge_state_match_the_board() {
        let mut battery = Battery::default();
        assert_eq!(battery.flags(0), 0);
        battery.sample(1932, 0);
        assert_eq!(battery.millivolts, 4100);
        assert_eq!(battery.percent(), 100);
        battery.power(true, true);
        assert_eq!(battery.flags(1), USB_POWER | CHARGING | VALID);
        battery.power(false, true);
        assert_eq!(battery.flags(2), VALID);
    }

    #[test]
    fn transient_sag_does_not_latch_shutdown_but_sustained_low_voltage_does() {
        let mut battery = Battery::default();
        battery.sample(1500, 0);
        battery.sample(1932, 3000);
        assert!(!battery.critical);
        battery.sample(1500, 6000);
        battery.sample(1500, 15_000);
        assert_ne!(battery.flags(15_000) & LOW, 0);
        assert!(!battery.critical);
        battery.sample(1500, 66_000);
        assert!(battery.critical);
        battery.sample(1932, 69_000);
        assert!(battery.critical);
        battery.power(true, false);
        assert!(!battery.critical);
    }

    #[test]
    fn invalid_or_stale_measurements_are_not_presented_as_valid() {
        let mut battery = Battery::default();
        for raw in [0, 1, u16::MAX] {
            battery.sample(raw, 0);
        }
        assert_eq!(battery.flags(0) & VALID, 0);
        battery.sample(1932, 1);
        assert_ne!(battery.flags(10_000) & VALID, 0);
        assert_eq!(battery.flags(10_001) & VALID, 0);
    }
}
