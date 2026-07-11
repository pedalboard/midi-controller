//! Per-preset runtime state: tracks toggle/cycle/encoder state per preset
//! and generates recall MIDI on preset switch.

use crate::action::{action_to_midi, MidiMessage};
use crate::config::{EncoderAction, Preset, MAX_BUTTONS, MAX_ENCODERS, MAX_PRESETS};

/// Runtime state for a single preset (not persisted across power cycles).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresetState<const B: usize = MAX_BUTTONS, const E: usize = MAX_ENCODERS> {
    pub button_active: [bool; B],
    pub cycle_index: [u8; B],
    pub encoder_values: [u8; E],
}

impl<const B: usize, const E: usize> Default for PresetState<B, E> {
    fn default() -> Self {
        Self {
            button_active: [false; B],
            cycle_index: [0; B],
            encoder_values: [0; E],
        }
    }
}

impl<const B: usize, const E: usize> PresetState<B, E> {
    /// Create a PresetState from a preset's declared defaults.
    pub fn from_defaults<const A: usize>(preset: &Preset<B, E, A>) -> Self {
        let mut state = Self::default();
        for (i, &active) in preset.defaults.button_active.iter().enumerate() {
            if i < B {
                state.button_active[i] = active;
            }
        }
        for (i, &val) in preset.defaults.encoder_values.iter().enumerate() {
            if i < E {
                state.encoder_values[i] = val;
            }
        }
        state
    }
}

/// Manages per-preset state and generates recall MIDI on switch.
#[derive(Clone)]
pub struct PresetStateStore<const B: usize = MAX_BUTTONS, const E: usize = MAX_ENCODERS> {
    states: [PresetState<B, E>; MAX_PRESETS],
    active: u8,
}

impl<const B: usize, const E: usize> Default for PresetStateStore<B, E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const B: usize, const E: usize> PresetStateStore<B, E> {
    pub fn new() -> Self {
        Self {
            states: core::array::from_fn(|_| PresetState::default()),
            active: 0,
        }
    }

    /// Get a reference to the current active preset's state.
    pub fn current(&self) -> &PresetState<B, E> {
        &self.states[self.active as usize]
    }

    /// Get a mutable reference to the current active preset's state.
    pub fn current_mut(&mut self) -> &mut PresetState<B, E> {
        &mut self.states[self.active as usize]
    }

    /// Current active preset index.
    pub fn active_index(&self) -> u8 {
        self.active
    }

    /// Save working state into the active preset slot (for serialization without switching).
    pub fn save_working(&mut self, working: &PresetState<B, E>) {
        self.states[self.active as usize] = working.clone();
    }

    /// Reset all state (presets changed, state is stale).
    pub fn clear(&mut self) {
        *self = Self::new();
    }

    /// Set state for a specific preset slot.
    pub fn set_state(&mut self, index: usize, state: PresetState<B, E>) {
        if index < MAX_PRESETS {
            self.states[index] = state;
        }
    }

    /// Reset state using preset defaults (after upload / first boot).
    /// Each preset's declared initial state is applied.
    pub fn apply_defaults<const A: usize>(&mut self, presets: &[&Preset<B, E, A>]) {
        for (i, preset) in presets.iter().enumerate() {
            if i < MAX_PRESETS {
                self.states[i] = PresetState::from_defaults(preset);
            }
        }
    }

    /// Switch to a new preset. Saves current working state, loads new state,
    /// and returns MIDI messages to recall the new preset's state to external gear.
    pub fn switch<const A: usize>(
        &mut self,
        new_preset: u8,
        working: &mut PresetState<B, E>,
        preset: &Preset<B, E, A>,
    ) -> heapless::Vec<MidiMessage, 16> {
        let mut recall = heapless::Vec::new();

        // Save current working state
        self.states[self.active as usize] = working.clone();

        // Load new state into working
        self.active = new_preset;
        *working = self.states[new_preset as usize].clone();

        // Recall: send MIDI state to external gear
        for (i, btn) in preset.buttons.iter().enumerate() {
            if working.button_active[i] {
                for action in &btn.on_press {
                    if let Some(msg) = action_to_midi(action) {
                        recall.push(msg).ok();
                    }
                }
            } else if !btn.on_release.is_empty() {
                for action in &btn.on_release {
                    if let Some(msg) = action_to_midi(action) {
                        recall.push(msg).ok();
                    }
                }
            }
        }

        // Recall encoder values
        for (i, enc) in preset.encoders.iter().enumerate() {
            if let EncoderAction::Cc { cc, channel, .. } = &enc.action {
                recall
                    .push(MidiMessage::new(
                        [0xB0 | (channel - 1), *cc as u8, working.encoder_values[i]],
                        3,
                    ))
                    .ok();
            }
        }

        recall
    }
}

// --- EEPROM persistence (AT24CS01: 128 bytes) ---

/// EEPROM magic encodes format version in the low nibble: 0xE0 | version.
/// Bump EEPROM_VERSION when PresetState layout changes (field count, order, size).
const EEPROM_VERSION: u8 = 2;
const EEPROM_MAGIC: u8 = 0xE0 | EEPROM_VERSION;
const EEPROM_HEADER_SIZE: usize = 2; // magic + active_preset

/// Compute the serialized size of a single PresetState<B, E>.
/// Layout: B bools + B cycle_indices + E encoder_values = B*2 + E bytes.
const fn preset_state_size(b: usize, e: usize) -> usize {
    b * 2 + e
}

/// Maximum presets that fit in 128 bytes for the default configuration.
pub const EEPROM_MAX_PRESETS: usize =
    (128 - EEPROM_HEADER_SIZE) / preset_state_size(MAX_BUTTONS, MAX_ENCODERS); // = 9

impl<const B: usize, const E: usize> PresetState<B, E> {
    /// Serialized byte size for this configuration.
    pub const SIZE: usize = preset_state_size(B, E);

    /// Serialize to a fixed-size buffer.
    pub fn to_bytes(&self, buf: &mut [u8]) {
        debug_assert!(buf.len() >= Self::SIZE);
        for (i, &active) in self.button_active.iter().enumerate() {
            buf[i] = active as u8;
        }
        for (i, &idx) in self.cycle_index.iter().enumerate() {
            buf[B + i] = idx;
        }
        for (i, &val) in self.encoder_values.iter().enumerate() {
            buf[B * 2 + i] = val;
        }
    }

    /// Deserialize from a byte buffer.
    pub fn from_bytes(buf: &[u8]) -> Self {
        debug_assert!(buf.len() >= Self::SIZE);
        let mut state = Self::default();
        for (i, &b) in buf[..B].iter().enumerate() {
            state.button_active[i] = b != 0;
        }
        state.cycle_index[..B].copy_from_slice(&buf[B..B * 2]);
        for (i, &b) in buf[B * 2..B * 2 + E].iter().enumerate() {
            state.encoder_values[i] = b;
        }
        state
    }
}

impl<const B: usize, const E: usize> PresetStateStore<B, E> {
    /// Maximum presets that fit in 128 bytes for this configuration.
    pub const EEPROM_MAX_PRESETS: usize = (128 - EEPROM_HEADER_SIZE) / preset_state_size(B, E);

    /// Return a cleared EEPROM buffer (for writing after preset upload).
    pub fn cleared_eeprom() -> [u8; 128] {
        let mut buf = [0u8; 128];
        Self::new().to_eeprom(&mut buf);
        buf
    }

    /// Serialize entire store to EEPROM buffer (128 bytes).
    /// Layout: [magic][active_preset][state0..stateN]
    pub fn to_eeprom(&self, buf: &mut [u8; 128]) {
        buf.fill(0xFF);
        buf[0] = EEPROM_MAGIC;
        buf[1] = self.active;
        let state_size = PresetState::<B, E>::SIZE;
        let max_presets = Self::EEPROM_MAX_PRESETS;
        for i in 0..max_presets {
            let offset = EEPROM_HEADER_SIZE + i * state_size;
            if offset + state_size > 128 {
                break;
            }
            self.states[i].to_bytes(&mut buf[offset..offset + state_size]);
        }
    }

    /// Deserialize from EEPROM buffer. Returns None if magic/version doesn't match.
    pub fn from_eeprom(buf: &[u8; 128]) -> Option<Self> {
        if buf[0] != EEPROM_MAGIC {
            return None;
        }
        let mut store = Self::new();
        store.active = buf[1];
        let state_size = PresetState::<B, E>::SIZE;
        let max_presets = Self::EEPROM_MAX_PRESETS;
        for i in 0..max_presets {
            let offset = EEPROM_HEADER_SIZE + i * state_size;
            if offset + state_size > 128 {
                break;
            }
            store.states[i] = PresetState::from_bytes(&buf[offset..offset + state_size]);
        }
        Some(store)
    }
}

/// Type alias for the default preset state (6 buttons, 2 encoders).
pub type DefaultPresetState = PresetState<MAX_BUTTONS, MAX_ENCODERS>;
/// Type alias for the default preset state store.
pub type DefaultPresetStateStore = PresetStateStore<MAX_BUTTONS, MAX_ENCODERS>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;
    use heapless::Vec;

    fn make_preset() -> Preset {
        let mut buttons: Vec<ButtonConfig, MAX_BUTTONS> = Vec::new();
        let mut on_press: Vec<Action, MAX_ACTIONS> = Vec::new();
        on_press.push(Action::cc(10, 127, 1).unwrap()).ok();
        let mut on_release: Vec<Action, MAX_ACTIONS> = Vec::new();
        on_release.push(Action::cc(10, 0, 1).unwrap()).ok();
        buttons
            .push(ButtonConfig {
                label: Label::new(),
                color: LedConfig::default(),
                mode: ButtonMode::Toggle,
                on_press,
                on_release,
                on_long_press: Vec::new(),
                cycle_values: Vec::new(),
                listen_cc: None,
            })
            .ok();

        let mut encoders: Vec<EncoderConfig, MAX_ENCODERS> = Vec::new();
        encoders
            .push(EncoderConfig {
                label: Label::try_from("Vol").unwrap(),
                action: EncoderAction::Cc {
                    cc: 7,
                    channel: 1,
                    min: 0,
                    max: 127,
                },
                ..Default::default()
            })
            .ok();

        Preset {
            name: Label::try_from("Test").unwrap(),
            buttons,
            encoders,
            analog: Vec::new(),
            defaults: Default::default(),
            on_enter: heapless::Vec::new(),
            on_exit: heapless::Vec::new(),
            triggers: heapless::Vec::new(),
            ..Default::default()
        }
    }

    #[test]
    fn switch_saves_and_restores_state() {
        let preset = make_preset();
        let mut store: PresetStateStore = PresetStateStore::new();
        let mut working = PresetState::default();

        // Activate toggle in preset 0
        working.button_active[0] = true;
        working.encoder_values[0] = 80;

        // Switch to preset 1
        store.switch(1, &mut working, &preset);
        assert!(!working.button_active[0]);
        assert_eq!(working.encoder_values[0], 0);

        // Switch back to preset 0
        store.switch(0, &mut working, &preset);
        assert!(working.button_active[0]);
        assert_eq!(working.encoder_values[0], 80);
    }

    #[test]
    fn recall_sends_active_button_on_press() {
        let preset = make_preset();
        let mut store: PresetStateStore = PresetStateStore::new();
        let mut working = PresetState::default();

        // Set button active in preset 0, switch away, switch back
        working.button_active[0] = true;
        store.switch(1, &mut working, &preset);
        let recall = store.switch(0, &mut working, &preset);

        // Should contain CC 10 = 127 (on_press of active button)
        assert!(recall.iter().any(|m| m.data == [0xB0, 10, 127]));
    }

    #[test]
    fn recall_sends_inactive_button_on_release() {
        let preset = make_preset();
        let mut store: PresetStateStore = PresetStateStore::new();
        let mut working = PresetState::default();

        // Button inactive in preset 0 (default), switch away, switch back
        store.switch(1, &mut working, &preset);
        let recall = store.switch(0, &mut working, &preset);

        // Should contain CC 10 = 0 (on_release of inactive button)
        assert!(recall.iter().any(|m| m.data == [0xB0, 10, 0]));
    }

    #[test]
    fn recall_sends_encoder_cc() {
        let preset = make_preset();
        let mut store: PresetStateStore = PresetStateStore::new();
        let mut working = PresetState::default();

        working.encoder_values[0] = 64;
        store.switch(1, &mut working, &preset);
        let recall = store.switch(0, &mut working, &preset);

        // Should contain CC 7 = 64
        assert!(recall.iter().any(|m| m.data == [0xB0, 7, 64]));
    }

    #[test]
    fn eeprom_roundtrip() {
        let mut store: PresetStateStore = PresetStateStore::new();
        store.states[0].button_active[0] = true;
        store.states[0].button_active[3] = true;
        store.states[0].cycle_index[2] = 5;
        store.states[0].encoder_values[0] = 100;
        store.states[0].encoder_values[1] = 42;
        store.states[1].button_active[5] = true;
        store.active = 1;

        let mut buf = [0u8; 128];
        store.to_eeprom(&mut buf);

        let restored: PresetStateStore = PresetStateStore::from_eeprom(&buf).unwrap();
        assert_eq!(restored.active_index(), 1);
        assert!(restored.states[0].button_active[0]);
        assert!(restored.states[0].button_active[3]);
        assert_eq!(restored.states[0].cycle_index[2], 5);
        assert_eq!(restored.states[0].encoder_values[0], 100);
        assert_eq!(restored.states[0].encoder_values[1], 42);
        assert!(restored.states[1].button_active[5]);
    }

    #[test]
    fn eeprom_invalid_magic_returns_none() {
        let buf = [0xFFu8; 128];
        assert!(PresetStateStore::<MAX_BUTTONS, MAX_ENCODERS>::from_eeprom(&buf).is_none());
    }

    #[test]
    fn from_defaults_applies_initial_state() {
        use crate::config::InitialState;

        let mut preset = make_preset();
        let mut btn_active = Vec::new();
        btn_active.push(true).ok(); // button A = on
        let mut enc_vals = Vec::new();
        enc_vals.push(64).ok(); // encoder 0 = 64
        preset.defaults = InitialState {
            button_active: btn_active,
            encoder_values: enc_vals,
        };

        let state = PresetState::from_defaults(&preset);
        assert!(state.button_active[0]);
        assert!(!state.button_active[1]);
        assert_eq!(state.encoder_values[0], 64);
        assert_eq!(state.encoder_values[1], 0);
    }
}
