//! Tap tempo state machine: computes BPM from press intervals.
//!
//! Call `tap(now_ms)` on each button press. Returns `Some(bpm)` once enough
//! taps are received. Resets automatically after a 2-second idle timeout.

const MAX_TAPS: usize = 4;
const TIMEOUT_MS: u32 = 2000;
const MIN_BPM: u16 = 30;
const MAX_BPM: u16 = 300;

/// Tap tempo state. Tracks timestamps of recent taps and computes BPM.
#[derive(Debug, Clone)]
pub struct TapTempo {
    /// Timestamps (ms) of the last N taps.
    taps: [u32; MAX_TAPS],
    /// Number of taps recorded (0 = idle).
    count: u8,
}

impl Default for TapTempo {
    fn default() -> Self {
        Self::new()
    }
}

impl TapTempo {
    pub fn new() -> Self {
        Self {
            taps: [0; MAX_TAPS],
            count: 0,
        }
    }

    /// Record a tap at `now_ms`. Returns computed BPM if enough taps (≥2).
    /// Resets if more than 2 seconds since last tap.
    pub fn tap(&mut self, now_ms: u32) -> Option<u16> {
        // Reset if timed out since last tap
        if self.count > 0 {
            let last = self.taps[(self.count - 1) as usize];
            if now_ms.wrapping_sub(last) > TIMEOUT_MS {
                self.count = 0;
            }
        }

        // Store this tap
        if (self.count as usize) < MAX_TAPS {
            self.taps[self.count as usize] = now_ms;
            self.count += 1;
        } else {
            // Shift left and append
            self.taps.rotate_left(1);
            self.taps[MAX_TAPS - 1] = now_ms;
        }

        // Need at least 2 taps to compute an interval
        if self.count < 2 {
            return None;
        }

        // Average all intervals
        let n = self.count as usize;
        let total_interval = self.taps[n - 1].wrapping_sub(self.taps[0]);
        let avg_interval = total_interval / (n as u32 - 1);

        if avg_interval == 0 {
            return None;
        }

        // BPM = 60000 / interval_ms
        let bpm = (60_000 / avg_interval) as u16;
        let bpm = bpm.clamp(MIN_BPM, MAX_BPM);

        Some(bpm)
    }

    /// Reset tap state (e.g., on preset switch).
    pub fn reset(&mut self) {
        self.count = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_tap_returns_none() {
        let mut tt = TapTempo::new();
        assert_eq!(tt.tap(0), None);
    }

    #[test]
    fn two_taps_at_120bpm() {
        let mut tt = TapTempo::new();
        tt.tap(0);
        // 500ms interval = 120 BPM
        assert_eq!(tt.tap(500), Some(120));
    }

    #[test]
    fn four_taps_averages_intervals() {
        let mut tt = TapTempo::new();
        tt.tap(0);
        tt.tap(500); // 120 BPM
        tt.tap(1000); // 120 BPM
                      // Fourth tap: total span = 1500ms, 3 intervals, avg = 500ms = 120 BPM
        assert_eq!(tt.tap(1500), Some(120));
    }

    #[test]
    fn uneven_taps_averages() {
        let mut tt = TapTempo::new();
        tt.tap(0);
        tt.tap(400);
        // 3rd tap at 900: span = 900ms, 2 intervals, avg = 450ms = 133 BPM
        assert_eq!(tt.tap(900), Some(133));
    }

    #[test]
    fn timeout_resets() {
        let mut tt = TapTempo::new();
        tt.tap(0);
        tt.tap(500); // 120 BPM
                     // 3 seconds later — should reset
        assert_eq!(tt.tap(3500), None); // first tap after reset
        assert_eq!(tt.tap(4000), Some(120)); // second tap, 500ms interval
    }

    #[test]
    fn clamps_at_max_bpm() {
        let mut tt = TapTempo::new();
        tt.tap(0);
        // 100ms interval = 600 BPM → clamped to 300
        assert_eq!(tt.tap(100), Some(300));
    }

    #[test]
    fn clamps_at_min_bpm() {
        let mut tt = TapTempo::new();
        tt.tap(0);
        // 1999ms interval (just under timeout) = 30 BPM
        assert_eq!(tt.tap(1999), Some(30));
    }

    #[test]
    fn more_than_max_taps_shifts_window() {
        let mut tt = TapTempo::new();
        tt.tap(0);
        tt.tap(500);
        tt.tap(1000);
        tt.tap(1500);
        // 5th tap shifts window: [500, 1000, 1500, 2000], avg = 500ms
        assert_eq!(tt.tap(2000), Some(120));
    }

    #[test]
    fn reset_clears_state() {
        let mut tt = TapTempo::new();
        tt.tap(0);
        tt.tap(500);
        tt.reset();
        assert_eq!(tt.tap(1000), None); // first tap after reset
    }
}
