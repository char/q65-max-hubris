use hid::Reports;

/// Preserve transitions while online. After a disconnect only current state is
/// useful; after overflow explicitly release everything before resynchronising.
pub struct ReportQueue {
    items: [Reports; 64],
    head: usize,
    len: usize,
    latest: Reports,
    pub overflows: u32,
}

impl Default for ReportQueue {
    fn default() -> Self {
        Self {
            items: [Reports::default(); 64],
            head: 0,
            len: 0,
            latest: Reports::default(),
            overflows: 0,
        }
    }
}

impl ReportQueue {
    pub fn submit(&mut self, report: Reports, online: bool) {
        if report == self.latest {
            return;
        }
        self.latest = report;
        if online {
            if self.len == self.items.len() {
                self.overflows = self.overflows.saturating_add(1);
                self.release();
            }
            self.push(report);
        }
    }

    pub fn release(&mut self) {
        self.head = 0;
        self.len = 0;
        self.push(Reports::default());
    }

    pub fn reconnect(&mut self) {
        self.release();
        if self.latest != Reports::default() {
            self.push(self.latest);
        }
    }

    fn push(&mut self, report: Reports) {
        self.items[(self.head + self.len) % self.items.len()] = report;
        self.len += 1;
    }

    pub(crate) fn front(&self) -> Option<Reports> {
        (self.len != 0).then(|| self.items[self.head])
    }

    pub(crate) fn pop(&mut self) {
        assert!(self.len != 0);
        self.head = (self.head + 1) % self.items.len();
        self.len -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hid::Key;

    fn pressed() -> Reports {
        let mut report = Reports::default();
        report.keyboard.press(Key::A);
        report
    }

    #[test]
    fn a_complete_tap_waits_in_order() {
        let mut queue = ReportQueue::default();
        queue.submit(pressed(), true);
        queue.submit(Reports::default(), true);
        assert_eq!(queue.front(), Some(pressed()));
        queue.pop();
        assert_eq!(queue.front(), Some(Reports::default()));
        queue.pop();
        assert_eq!(queue.front(), None);
    }

    #[test]
    fn overflow_releases_keys_instead_of_leaving_a_stale_press() {
        let mut queue = ReportQueue::default();
        for n in 0..1000 {
            queue.submit(
                if n % 2 == 0 {
                    pressed()
                } else {
                    Reports::default()
                },
                true,
            );
            if queue.overflows != 0 {
                assert_eq!(queue.front(), Some(Reports::default()));
                queue.pop();
                assert_eq!(queue.front(), Some(pressed()));
                return;
            }
        }
        panic!("queue never filled");
    }

    #[test]
    fn reconnect_discards_offline_taps_but_restores_held_keys() {
        let mut queue = ReportQueue::default();
        queue.submit(pressed(), true);
        queue.submit(Reports::default(), false);
        queue.submit(pressed(), false);
        queue.reconnect();
        assert_eq!(queue.front(), Some(Reports::default()));
        queue.pop();
        assert_eq!(queue.front(), Some(pressed()));
        queue.pop();
        assert_eq!(queue.front(), None);
    }
}
