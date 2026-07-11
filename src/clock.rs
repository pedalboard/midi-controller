//! MIDI Clock generator with Start/Stop/Continue transport control.
//!
//! The clock is driven by an external timer (firmware provides the tick cadence).
//! This module owns the state machine and decides what messages to emit.
//!
//! # Message types
//!
//! - `0xF8` — Timing Clock (24 PPQ, sent continuously while running)
//! - `0xFA` — Start (transport start, resets position to beat 1)
//! - `0xFB` — Continue (resume from current position)
//! - `0xFC` — Stop (halt transport)
//!
//! # Usage
//!
//! The firmware calls `clock.tick()` at the BPM-derived interval.
//! The clock returns `Option<ClockOutput>` with messages to send.
//! The firmware also calls `clock.update_config()` when global config changes.

use crate::routing::{MidiOut, MidiPort};

/// MIDI system realtime message bytes.
pub const CLOCK_TICK: u8 = 0xF8;
pub const CLOCK_START: u8 = 0xFA;
pub const CLOCK_CONTINUE: u8 = 0xFB;
pub const CLOCK_STOP: u8 = 0xFC;

/// Clock transport state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transport {
    /// Clock is disabled (not sending anything).
    Stopped,
    /// Clock is running (sending F8 ticks).
    Running,
}

/// Output from a clock tick or state change.
pub struct ClockOutput {
    /// MIDI messages to send (Start/Stop/Continue/Tick).
    pub messages: heapless::Vec<MidiOut, 4>,
}

/// MIDI Clock generator.
#[derive(Debug, Clone)]
pub struct MidiClock {
    transport: Transport,
    /// Destination ports for clock messages. Default: ALL.
    dest: MidiPort,
}

impl Default for MidiClock {
    fn default() -> Self {
        Self::new()
    }
}

impl MidiClock {
    pub fn new() -> Self {
        Self {
            transport: Transport::Stopped,
            dest: MidiPort::ALL,
        }
    }

    /// Called by the firmware timer at the BPM interval.
    /// Returns clock tick message if the clock is running.
    pub fn tick(&self) -> Option<ClockOutput> {
        match self.transport {
            Transport::Running => {
                let mut messages = heapless::Vec::new();
                messages.push(MidiOut::new(&[CLOCK_TICK], self.dest)).ok();
                Some(ClockOutput { messages })
            }
            Transport::Stopped => None,
        }
    }

    /// Update clock state based on global config.
    /// Returns Start/Stop messages if the state changed.
    pub fn update_config(&mut self, clock_enabled: bool) -> Option<ClockOutput> {
        match (self.transport, clock_enabled) {
            (Transport::Stopped, true) => {
                self.transport = Transport::Running;
                Some(self.start_message())
            }
            (Transport::Running, false) => {
                self.transport = Transport::Stopped;
                Some(self.stop_message())
            }
            _ => None, // No change.
        }
    }

    /// Explicitly start the clock (e.g., from a button action).
    pub fn start(&mut self) -> ClockOutput {
        self.transport = Transport::Running;
        self.start_message()
    }

    /// Explicitly stop the clock.
    pub fn stop(&mut self) -> ClockOutput {
        self.transport = Transport::Stopped;
        self.stop_message()
    }

    /// Continue from current position (resume after stop without resetting).
    pub fn resume(&mut self) -> ClockOutput {
        self.transport = Transport::Running;
        let mut messages = heapless::Vec::new();
        messages
            .push(MidiOut::new(&[CLOCK_CONTINUE], self.dest))
            .ok();
        ClockOutput { messages }
    }

    /// Whether the clock is currently running.
    pub fn is_running(&self) -> bool {
        self.transport == Transport::Running
    }

    /// Set the destination ports for clock messages.
    pub fn set_dest(&mut self, dest: MidiPort) {
        self.dest = dest;
    }

    // --- Private ---

    fn start_message(&self) -> ClockOutput {
        let mut messages = heapless::Vec::new();
        messages.push(MidiOut::new(&[CLOCK_START], self.dest)).ok();
        ClockOutput { messages }
    }

    fn stop_message(&self) -> ClockOutput {
        let mut messages = heapless::Vec::new();
        messages.push(MidiOut::new(&[CLOCK_STOP], self.dest)).ok();
        ClockOutput { messages }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_clock_is_stopped() {
        let clock = MidiClock::new();
        assert!(!clock.is_running());
        assert!(clock.tick().is_none());
    }

    #[test]
    fn enable_clock_sends_start() {
        let mut clock = MidiClock::new();
        let output = clock.update_config(true).unwrap();
        assert_eq!(output.messages.len(), 1);
        assert_eq!(output.messages[0].bytes(), &[CLOCK_START]);
        assert!(clock.is_running());
    }

    #[test]
    fn running_clock_sends_tick() {
        let mut clock = MidiClock::new();
        clock.update_config(true);
        let output = clock.tick().unwrap();
        assert_eq!(output.messages[0].bytes(), &[CLOCK_TICK]);
    }

    #[test]
    fn disable_clock_sends_stop() {
        let mut clock = MidiClock::new();
        clock.update_config(true);
        let output = clock.update_config(false).unwrap();
        assert_eq!(output.messages.len(), 1);
        assert_eq!(output.messages[0].bytes(), &[CLOCK_STOP]);
        assert!(!clock.is_running());
    }

    #[test]
    fn stopped_clock_no_tick() {
        let mut clock = MidiClock::new();
        clock.update_config(true);
        clock.update_config(false);
        assert!(clock.tick().is_none());
    }

    #[test]
    fn redundant_enable_no_output() {
        let mut clock = MidiClock::new();
        clock.update_config(true);
        let output = clock.update_config(true);
        assert!(output.is_none(), "already running — no message");
    }

    #[test]
    fn redundant_disable_no_output() {
        let clock = MidiClock::new();
        let output = clock.clone().update_config(false);
        assert!(output.is_none(), "already stopped — no message");
    }

    #[test]
    fn explicit_start_sends_fa() {
        let mut clock = MidiClock::new();
        let output = clock.start();
        assert_eq!(output.messages[0].bytes(), &[CLOCK_START]);
        assert!(clock.is_running());
    }

    #[test]
    fn explicit_stop_sends_fc() {
        let mut clock = MidiClock::new();
        clock.start();
        let output = clock.stop();
        assert_eq!(output.messages[0].bytes(), &[CLOCK_STOP]);
        assert!(!clock.is_running());
    }

    #[test]
    fn resume_sends_continue() {
        let mut clock = MidiClock::new();
        clock.start();
        clock.stop();
        let output = clock.resume();
        assert_eq!(output.messages[0].bytes(), &[CLOCK_CONTINUE]);
        assert!(clock.is_running());
    }

    #[test]
    fn clock_messages_use_configured_dest() {
        let mut clock = MidiClock::new();
        clock.set_dest(MidiPort::DIN);
        let output = clock.start();
        assert_eq!(output.messages[0].dest, MidiPort::DIN);
        let output = clock.tick().unwrap();
        assert_eq!(output.messages[0].dest, MidiPort::DIN);
    }
}
