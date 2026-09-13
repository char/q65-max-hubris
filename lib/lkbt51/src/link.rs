use crate::{Ack, Connection, Event, Packet};
use hid::{ConsumerReport, KeyboardReport, LedReport, Reports};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    ResetLow,
    Booting,
    Name,
    Configure,
    Disabled,
    WakeLow,
    Waking,
    Connecting,
    Connected,
    Pairing,
    DisconnectLow,
    Disconnecting,
}

#[derive(Debug)]
pub enum Action {
    Reset(bool),
    Pulse(u16),
    Send(Packet),
}

struct InFlight {
    packet: Packet,
    sequence: u8,
    command: u8,
    deadline: u64,
    attempts: u8,
}

pub struct Link {
    pub state: State,
    pub leds: LedReport,
    pub errors: u32,
    pub resets: u32,
    enabled: bool,
    pairing: bool,
    deadline: u64,
    sequence: u8,
    connection_ack: bool,
    interval: u64,
    next_report: u64,
    reports: Reports,
    sent_keyboard: Option<KeyboardReport>,
    sent_consumer: Option<ConsumerReport>,
    in_flight: Option<InFlight>,
}

impl Default for Link {
    fn default() -> Self {
        Self {
            state: State::ResetLow,
            leds: LedReport::default(),
            errors: 0,
            resets: 0,
            enabled: false,
            pairing: false,
            deadline: 0,
            sequence: 0,
            connection_ack: false,
            interval: 1,
            next_report: 0,
            reports: Reports::default(),
            sent_keyboard: None,
            sent_consumer: None,
            in_flight: None,
        }
    }
}

impl Link {
    pub fn enable(&mut self, enabled: bool, now: u64) {
        self.enabled = enabled;
        self.pairing = false;
        if enabled && self.state == State::Disabled {
            self.state = State::WakeLow;
            self.deadline = now;
        } else if !enabled
            && matches!(
                self.state,
                State::Connected | State::Connecting | State::Pairing
            )
        {
            self.state = State::DisconnectLow;
            self.deadline = now;
            self.in_flight = None;
        }
    }

    pub fn pair(&mut self, now: u64) {
        if self.enabled
            && matches!(
                self.state,
                State::Connected | State::Connecting | State::Pairing
            )
        {
            self.pairing = true;
            self.in_flight = None;
            self.state = State::WakeLow;
            self.deadline = now;
        }
    }

    pub fn set_reports(&mut self, reports: Reports) {
        self.reports = reports;
    }

    pub fn failed(&mut self, now: u64) {
        self.errors = self.errors.saturating_add(1);
        self.state = State::ResetLow;
        self.deadline = now + 100;
        self.in_flight = None;
        self.connection_ack = false;
        self.leds = LedReport::default();
    }

    pub fn event(&mut self, event: Event, now: u64) {
        match event {
            Event::Reset => {
                self.resets = self.resets.saturating_add(1);
                self.in_flight = None;
                if !matches!(self.state, State::ResetLow | State::Booting) {
                    self.state = State::Name;
                    self.deadline = now + 3;
                }
            }
            Event::Connection { state, host } => {
                self.connection_ack = true;
                if matches!(
                    self.state,
                    State::ResetLow
                        | State::Booting
                        | State::Name
                        | State::Configure
                        | State::WakeLow
                        | State::Waking
                        | State::DisconnectLow
                        | State::Disconnecting
                ) {
                    return;
                }
                if !self.enabled {
                    if state != Connection::Disconnected && state != Connection::Sleeping {
                        self.state = State::DisconnectLow;
                        self.deadline = now;
                    }
                    return;
                }
                match (state, host) {
                    (Connection::Connected, crate::HOST_2P4) => {
                        if self.state != State::Connected {
                            self.in_flight = None;
                            self.sent_keyboard = None;
                            self.sent_consumer = None;
                            self.leds = LedReport::default();
                        }
                        self.state = State::Connected;
                    }
                    (Connection::Pairing, crate::HOST_2P4) => {
                        self.state = State::Pairing;
                        self.deadline = now + 180_000;
                    }
                    (Connection::Reconnecting, crate::HOST_2P4) => {
                        self.state = State::Connecting;
                        self.deadline = now + 10_000;
                    }
                    _ => {
                        self.in_flight = None;
                        self.leds = LedReport::default();
                        self.state = State::WakeLow;
                        self.deadline = now + 1000;
                    }
                }
            }
            Event::Leds(leds) => self.leds = LedReport(leds),
            Event::Interval(interval) => self.interval = u64::from(interval).max(1),
            Event::Ack {
                sequence,
                command,
                result,
            } => {
                if let Some(pending) = &mut self.in_flight
                    && pending.sequence == sequence
                    && pending.command == command
                {
                    match result {
                        Ack::Success | Ack::HalfFull => {
                            self.in_flight = None;
                            self.next_report =
                                now + self.interval + if result == Ack::HalfFull { 5 } else { 0 };
                        }
                        Ack::ChecksumError | Ack::Full => {
                            pending.deadline = now + self.interval + 10;
                        }
                    }
                }
            }
            Event::Battery(_) => {}
        }
    }

    fn packet(&mut self, payload: &[u8], ack: bool) -> Packet {
        self.sequence = self.sequence.wrapping_add(1).max(1);
        Packet::command(self.sequence, payload, ack).unwrap()
    }

    #[expect(
        clippy::too_many_lines,
        reason = "keep timed state transitions together"
    )]
    pub fn step(&mut self, now: u64) -> Option<Action> {
        // PA4 is held low during these states: clocking even an event read would
        // turn a wake pulse into an unintended radio transaction.
        if now < self.deadline
            && matches!(
                self.state,
                State::Booting | State::Waking | State::Disconnecting
            )
        {
            return None;
        }
        if self.connection_ack && self.can_read() {
            self.connection_ack = false;
            return Some(Action::Send(self.packet(&crate::CONNECTION_ACK, false)));
        }
        if self.state != State::Connected && now < self.deadline {
            return None;
        }
        match self.state {
            State::ResetLow => {
                self.state = State::Booting;
                self.deadline = now + 1;
                Some(Action::Reset(true))
            }
            State::Booting => {
                self.state = State::Name;
                self.deadline = now + 500;
                Some(Action::Reset(false))
            }
            State::Name => {
                self.state = State::Configure;
                self.deadline = now + 3;
                Some(Action::Send(self.packet(crate::NAME, false)))
            }
            State::Configure => {
                self.state = if self.enabled {
                    State::WakeLow
                } else {
                    State::DisconnectLow
                };
                self.deadline = now + 3;
                Some(Action::Send(
                    self.packet(&crate::configuration(7200), false),
                ))
            }
            State::Disabled => None,
            State::WakeLow => {
                self.state = State::Waking;
                self.deadline = now + 310;
                Some(Action::Pulse(10))
            }
            State::Waking => {
                if !self.enabled {
                    self.state = State::DisconnectLow;
                    self.deadline = now;
                    return None;
                }
                self.state = if self.pairing {
                    State::Pairing
                } else {
                    State::Connecting
                };
                self.deadline = now + if self.pairing { 180_000 } else { 10_000 };
                Some(Action::Send(self.packet(
                    if self.pairing {
                        &crate::PAIR
                    } else {
                        &crate::CONNECT
                    },
                    true,
                )))
            }
            State::Connecting | State::Pairing => {
                self.pairing = false;
                self.state = State::WakeLow;
                self.deadline = now + 1000;
                None
            }
            State::DisconnectLow => {
                self.state = State::Disconnecting;
                self.deadline = now + 100;
                Some(Action::Pulse(100))
            }
            State::Disconnecting => {
                self.state = if self.enabled {
                    State::WakeLow
                } else {
                    State::Disabled
                };
                self.deadline = now + 3;
                self.leds = LedReport::default();
                Some(Action::Send(self.packet(&crate::DISCONNECT, true)))
            }
            State::Connected => {
                if let Some(pending) = &mut self.in_flight {
                    if now < pending.deadline {
                        return None;
                    }
                    if pending.attempts == 3 {
                        self.failed(now);
                        return None;
                    }
                    pending.attempts += 1;
                    pending.deadline = now + self.interval + 10;
                    return Some(Action::Send(pending.packet.clone()));
                }
                if now < self.next_report {
                    return None;
                }
                let packet = if Some(self.reports.keyboard) != self.sent_keyboard {
                    self.sent_keyboard = Some(self.reports.keyboard);
                    self.packet(&crate::keyboard(self.reports.keyboard), true)
                } else if Some(self.reports.consumer) != self.sent_consumer {
                    self.sent_consumer = Some(self.reports.consumer);
                    self.packet(&crate::consumer(self.reports.consumer), true)
                } else {
                    return None;
                };
                self.in_flight = Some(InFlight {
                    sequence: self.sequence,
                    command: packet.bytes()[9],
                    packet: packet.clone(),
                    deadline: now + self.interval + 10,
                    attempts: 1,
                });
                Some(Action::Send(packet))
            }
        }
    }

    pub fn pulse_started(&mut self, now: u64) {
        // Account for time spent waiting for the SPI service to accept the pulse.
        self.deadline = now
            + if self.state == State::Waking {
                310
            } else {
                100
            };
    }

    #[must_use]
    pub fn can_read(&self) -> bool {
        !matches!(
            self.state,
            State::ResetLow
                | State::Booting
                | State::Disabled
                | State::Waking
                | State::Disconnecting
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_and_wake_do_not_block_or_transmit_during_reset() {
        let mut link = Link::default();
        link.enable(true, 0);
        assert!(matches!(link.step(0), Some(Action::Reset(true))));
        assert!(link.step(0).is_none());
        assert!(!link.can_read());
        assert!(matches!(link.step(1), Some(Action::Reset(false))));
        assert!(link.step(500).is_none());
        assert!(matches!(link.step(501), Some(Action::Send(_))));
        assert!(matches!(link.step(504), Some(Action::Send(_))));
        assert!(matches!(link.step(507), Some(Action::Pulse(10))));
        assert!(!link.can_read());
        assert!(link.step(816).is_none());
        let Some(Action::Send(packet)) = link.step(817) else {
            panic!()
        };
        assert_eq!(&packet.bytes()[9..13], &crate::CONNECT);
    }

    #[test]
    fn lost_ack_retries_identical_packet_then_recovers() {
        let mut link = Link {
            enabled: true,
            state: State::Connected,
            ..Link::default()
        };
        let Some(Action::Send(first)) = link.step(0) else {
            panic!()
        };
        let Some(Action::Send(retry)) = link.step(11) else {
            panic!()
        };
        assert_eq!(first, retry);
        assert!(matches!(link.step(22), Some(Action::Send(_))));
        assert!(link.step(33).is_none());
        assert_eq!(link.state, State::ResetLow);
        assert_eq!(link.errors, 1);
    }

    #[test]
    fn stale_ack_cannot_complete_a_new_report() {
        let mut link = Link {
            state: State::Connected,
            ..Link::default()
        };
        link.step(0);
        link.event(
            Event::Ack {
                sequence: 255,
                command: 0x11,
                result: Ack::Success,
            },
            1,
        );
        assert!(link.step(2).is_none());
        link.event(
            Event::Ack {
                sequence: 1,
                command: 0x11,
                result: Ack::Success,
            },
            3,
        );
        assert!(matches!(link.step(4), Some(Action::Send(_))));
    }
}
