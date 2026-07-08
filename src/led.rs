//! LED ring rendering — re-exported from the `led-ring` crate.
//!
//! This module provides spatial patterns, temporal modifiers, and ring animation
//! for WS2812/SK6812 LED rings. Generic over ring size.

pub use led_ring::*;

/// Default LEDs per ring for the pedalboard hardware.
pub const LEDS_PER_RING: usize = 12;

/// Frame type for 12-LED rings (pedalboard default).
pub type RingFrame = [Rgb; LEDS_PER_RING];
