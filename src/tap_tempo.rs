//! Tap tempo state machine: computes BPM from press intervals.
//!
//! Call `tap(now_ms)` on each button press. Returns `Some(bpm)` once enough
//! taps are received. Resets automatically after a 2-second idle timeout.

const MAX_TAPS: usize = 4;
const TIMEOUT_MS: u32 = 2000;
const MIN_INTERVAL_MS: u32 = 300;
const MIN_BPM: u16 = 30;
const MAX_BPM: u16 = 200;

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

    /// Record a tap at `now_ms`. Returns computed BPM if enough taps (≥4).
    /// Resets if more than 2 seconds since last tap.
    pub fn tap(&mut self, now_ms: u32) -> Option<u16> {
        // Reset if timed out since last tap
        if self.count > 0 {
            let last_idx = (self.count - 1).min(MAX_TAPS as u8 - 1) as usize;
            let last = self.taps[last_idx];
            let elapsed = now_ms.wrapping_sub(last);
            if elapsed > TIMEOUT_MS {
                self.count = 0;
            } else if elapsed < MIN_INTERVAL_MS {
                // Ignore bounce / double-tap (too fast to be a real tap)
                return None;
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

        // Need full window to compute stable BPM
        if (self.count as usize) < MAX_TAPS {
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
    fn three_taps_returns_none() {
        let mut tt = TapTempo::new();
        tt.tap(0);
        assert_eq!(tt.tap(500), None);
        assert_eq!(tt.tap(1000), None);
    }

    #[test]
    fn four_taps_at_120bpm() {
        let mut tt = TapTempo::new();
        tt.tap(0);
        tt.tap(500);
        tt.tap(1000);
        // Fourth tap: total span = 1500ms, 3 intervals, avg = 500ms = 120 BPM
        assert_eq!(tt.tap(1500), Some(120));
    }

    #[test]
    fn uneven_taps_averages() {
        let mut tt = TapTempo::new();
        tt.tap(0);
        tt.tap(400);
        tt.tap(900);
        // 4th tap at 1400: span = 1400ms, 3 intervals, avg = 466ms = 128 BPM
        assert_eq!(tt.tap(1400), Some(128));
    }

    #[test]
    fn timeout_resets() {
        let mut tt = TapTempo::new();
        tt.tap(0);
        tt.tap(500);
        tt.tap(1000);
        tt.tap(1500); // stable at 120
                      // 3 seconds later — should reset
        assert_eq!(tt.tap(5500), None); // first tap after reset
        assert_eq!(tt.tap(6000), None);
        assert_eq!(tt.tap(6500), None);
        assert_eq!(tt.tap(7000), Some(120)); // fourth tap after reset
    }

    #[test]
    fn clamps_at_max_bpm() {
        let mut tt = TapTempo::new();
        tt.tap(0);
        tt.tap(300);
        tt.tap(600);
        // 300ms interval = 200 BPM → clamped to 200
        assert_eq!(tt.tap(900), Some(200));
    }

    #[test]
    fn clamps_at_min_bpm() {
        let mut tt = TapTempo::new();
        tt.tap(0);
        tt.tap(1900);
        tt.tap(3800);
        // ~1900ms intervals = 31 BPM → clamped to 30
        assert_eq!(tt.tap(5700), Some(31));
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
        tt.tap(1000);
        tt.tap(1500);
        tt.reset();
        assert_eq!(tt.tap(2000), None); // first tap after reset
    }

    #[test]
    fn ignores_bounce_tap() {
        let mut tt = TapTempo::new();
        tt.tap(0);
        // Bounce at 50ms — should be ignored
        assert_eq!(tt.tap(50), None);
        tt.tap(500);
        tt.tap(1000);
        // Fourth real tap at 1500ms
        assert_eq!(tt.tap(1500), Some(120));
    }

    #[test]
    fn ignores_bounce_mid_sequence() {
        let mut tt = TapTempo::new();
        tt.tap(0);
        tt.tap(500);
        tt.tap(1000);
        // Bounce at 1050ms — ignored
        assert_eq!(tt.tap(1050), None);
        // Real 4th tap at 1500ms
        assert_eq!(tt.tap(1500), Some(120));
    }
}
