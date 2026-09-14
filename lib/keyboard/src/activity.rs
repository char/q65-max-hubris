/// Input activity shared by the scan-rate and lighting policies.
pub struct Activity {
    pub last: u64,
    encoder: u8,
}

impl Activity {
    #[must_use]
    pub fn new(now: u64, encoder: u8) -> Self {
        Self { last: now, encoder }
    }

    /// Active input includes key/transport changes, held switches and unresolved keymap work.
    pub fn scan_interval(&mut self, now: u64, on_battery: bool, active: bool, encoder: u8) -> u64 {
        // A partial detent must wake scanning before it becomes a volume report.
        if active || encoder != self.encoder || encoder != 0b11 {
            self.last = now;
        }
        self.encoder = encoder;
        if on_battery && now - self.last >= 30_000 {
            8
        } else {
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slows_only_after_thirty_seconds_on_battery() {
        let mut activity = Activity::new(1000, 3);
        assert_eq!(activity.scan_interval(30_999, true, false, 3), 1);
        assert_eq!(activity.scan_interval(31_000, true, false, 3), 8);
        assert_eq!(activity.scan_interval(40_000, false, false, 3), 1);
        assert_eq!(activity.scan_interval(40_008, true, false, 3), 8);
    }

    #[test]
    fn a_key_wakes_immediately_and_held_or_pending_input_cannot_idle() {
        let mut activity = Activity::new(0, 3);
        assert_eq!(activity.scan_interval(30_000, true, false, 3), 8);
        assert_eq!(activity.scan_interval(30_008, true, true, 3), 1);
        assert_eq!(activity.scan_interval(90_000, true, true, 3), 1);
        assert_eq!(activity.scan_interval(90_001, true, false, 3), 1);
        assert_eq!(activity.scan_interval(119_999, true, false, 3), 1);
        assert_eq!(activity.scan_interval(120_000, true, false, 3), 8);
    }

    #[test]
    fn encoder_edges_wake_without_waiting_for_a_completed_detent() {
        let mut activity = Activity::new(0, 3);
        assert_eq!(activity.scan_interval(30_000, true, false, 3), 8);
        assert_eq!(activity.scan_interval(30_008, true, false, 1), 1);
        assert_eq!(activity.scan_interval(60_008, true, false, 1), 1);
        for (now, phase) in [(60_009, 0), (60_010, 2), (60_011, 3)] {
            assert_eq!(activity.scan_interval(now, true, false, phase), 1);
            assert_eq!(activity.last, now);
        }
        assert_eq!(activity.scan_interval(90_010, true, false, 3), 1);
        assert_eq!(activity.scan_interval(90_011, true, false, 3), 8);
    }
}
