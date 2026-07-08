//! Unified controller: single entry point for all input processing.
//!
//! Owns all timing-sensitive state (long-press detection, encoder acceleration,
//! tap tempo, preset state) and delegates pure logic to the engine.
//!
//! The Controller handles system actions (preset switching) internally,
//! including on_enter/on_exit actions and recall MIDI. Callers only need to
//! send the resulting MIDI output — no orchestration required.
//!
//! Both firmware and simulator use the same `Controller` — the only difference
//! is where `now_ms` comes from (hardware monotonic vs std::Instant).

use crate::action::{action_to_midi, EncoderDirection};
use crate::config::{Action, ButtonMode, Config, Preset};
use crate::encoder_accel::EncoderAccel;
use crate::engine::{
    self, process_triggers, ActionStep, ButtonEvent, DisplayEvent, EngineResult, SystemAction,
};
use crate::long_press::{Edge, Gesture, LongPressDetector};
use crate::state::{PresetState, PresetStateStore};
use crate::tap_tempo::TapTempo;

const NUM_BUTTONS: usize = 6;
const NUM_ENCODERS: usize = 2;

/// Abstract input event. Hardware-agnostic — firmware maps GPIO edges to these,
/// the simulator maps keyboard/WebSocket events to these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    /// Button edge (index 0..5).
    ButtonEdge { index: u8, edge: Edge },
    /// Encoder detent (index 0..1).
    EncoderTurn { index: u8, clockwise: bool },
    /// Analog input (expression pedal). Raw ADC value with calibration.
    Analog {
        index: u8,
        raw: u16,
        min: u16,
        max: u16,
    },
    /// Tap tempo button press.
    TapTempo,
}

/// Result of processing an input event through the Controller.
/// Contains everything the caller needs to emit — no further logic required.
pub struct ControllerResult {
    /// MIDI actions to emit (includes on_enter/on_exit/recall on preset switch).
    pub midi: heapless::Vec<ActionStep, 32>,
    /// Display events (overlays, hints).
    pub display: heapless::Vec<DisplayEvent, 2>,
    /// Whether LED state changed and needs re-rendering.
    pub led_dirty: bool,
    /// Whether a preset switch occurred (caller may need to update display).
    pub preset_changed: bool,
    /// BPM computed from tap tempo (if any).
    pub bpm: Option<u16>,
    /// Internal: pending system actions to be handled by the controller.
    pending_system: heapless::Vec<SystemAction, 2>,
}

impl ControllerResult {
    fn new() -> Self {
        Self {
            midi: heapless::Vec::new(),
            display: heapless::Vec::new(),
            led_dirty: false,
            preset_changed: false,
            bpm: None,
            pending_system: heapless::Vec::new(),
        }
    }
}

/// Unified timing controller. Owns all stateful input processing.
///
/// # Usage
///
/// ```ignore
/// let mut ctrl = Controller::new();
/// // On each input poll:
/// let result = ctrl.process(event, now_ms, &config);
/// // Send result.midi via MIDI output
/// // Update display from result.display
/// // Re-render LEDs if result.led_dirty
/// ```
pub struct Controller {
    state_store: PresetStateStore,
    long_press: [LongPressDetector; NUM_BUTTONS],
    encoder_accel: [EncoderAccel; NUM_ENCODERS],
    tap_tempo: TapTempo,
    button_active: [bool; NUM_BUTTONS],
    active_preset: u8,
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}

impl Controller {
    pub fn new() -> Self {
        Self {
            state_store: PresetStateStore::new(),
            long_press: core::array::from_fn(|_| LongPressDetector::new_fired()),
            encoder_accel: [EncoderAccel::new(), EncoderAccel::new()],
            tap_tempo: TapTempo::new(),
            button_active: [false; NUM_BUTTONS],
            active_preset: 0,
        }
    }

    /// Create with a restored state store (from EEPROM/persistence).
    pub fn with_state(store: PresetStateStore) -> Self {
        let state = store.current().clone();
        let active = store.active_index();
        Self {
            state_store: store,
            long_press: core::array::from_fn(|_| LongPressDetector::new_fired()),
            encoder_accel: [EncoderAccel::new(), EncoderAccel::new()],
            tap_tempo: TapTempo::new(),
            button_active: state.button_active,
            active_preset: active,
        }
    }

    /// Process a single input event. Returns all MIDI output, display events,
    /// and flags. System actions (preset switch) are handled internally.
    pub fn process(&mut self, event: InputEvent, now_ms: u32, config: &Config) -> ControllerResult {
        let mut result = match event {
            InputEvent::ButtonEdge { index, edge } => {
                self.process_button(index as usize, edge, now_ms, config)
            }
            InputEvent::EncoderTurn { index, clockwise } => {
                self.process_encoder(index as usize, clockwise, now_ms, config)
            }
            InputEvent::Analog {
                index,
                raw,
                min,
                max,
            } => self.process_analog(index as usize, raw, min, max, config),
            InputEvent::TapTempo => {
                let mut r = ControllerResult::new();
                r.bpm = self.tap_tempo.tap(now_ms);
                if let Some(bpm) = r.bpm {
                    r.display.push(DisplayEvent::BpmOverlay { bpm }).ok();
                }
                r
            }
        };

        // Handle system actions produced by button processing
        self.handle_system_actions(&mut result, config);

        result
    }

    /// Poll with no event — needed for long-press detection while button is held.
    /// Call this periodically (e.g., every 1-10ms) when a button is active.
    pub fn tick(&mut self, now_ms: u32, config: &Config) -> ControllerResult {
        let mut result = ControllerResult::new();
        let preset = match config.presets.get(self.active_preset as usize) {
            Some(p) => p,
            None => return result,
        };

        for i in 0..NUM_BUTTONS {
            if self.long_press[i].is_active() && !self.long_press[i].has_fired() {
                let has_long_press = preset
                    .buttons
                    .get(i)
                    .map(|b| !b.on_long_press.is_empty())
                    .unwrap_or(false);
                if has_long_press {
                    if let Some(gesture) = self.long_press[i].update(None, now_ms) {
                        self.handle_gesture(i, gesture, preset, &mut result);
                    }
                }
            }
        }

        // Handle system actions from long-press (e.g., PresetNext)
        self.handle_system_actions(&mut result, config);

        result
    }

    /// Process incoming MIDI against preset triggers.
    pub fn process_incoming_midi(&mut self, raw: &[u8], config: &Config) -> ControllerResult {
        let mut result = ControllerResult::new();

        let preset = match config.presets.get(self.active_preset as usize) {
            Some(p) => p,
            None => return result,
        };

        if raw.len() >= 2 && !preset.triggers.is_empty() {
            let mut state = self.working_state();
            let data2 = if raw.len() >= 3 { raw[2] } else { 0 };
            let trigger_result = process_triggers(&mut state, preset, raw[0], raw[1], data2);
            self.apply_state(&state);

            for step in &trigger_result.midi {
                result.midi.push(step.clone()).ok();
            }
            if trigger_result.led_dirty {
                result.led_dirty = true;
            }

            // Handle system actions from triggers (preset switch)
            for s in &trigger_result.system {
                self.execute_system_action(*s, &mut result, config);
            }
        }

        result
    }

    /// Returns true if any button is currently held.
    pub fn any_active(&self) -> bool {
        self.long_press.iter().any(|lp| lp.is_active())
    }

    /// Returns the current button active state (toggle ON / momentary held).
    pub fn button_active(&self) -> &[bool; NUM_BUTTONS] {
        &self.button_active
    }

    /// Get the active preset index.
    pub fn active_preset(&self) -> u8 {
        self.active_preset
    }

    /// Get the encoder values.
    pub fn encoder_values(&self) -> [u8; NUM_ENCODERS] {
        self.state_store.current().encoder_values
    }

    /// Milliseconds the given button has been held, or 0 if not active.
    pub fn held_ms(&self, button_index: usize, now_ms: u32) -> u32 {
        if button_index < NUM_BUTTONS {
            self.long_press[button_index].held_ms(now_ms)
        } else {
            0
        }
    }

    /// Serialize current state for EEPROM persistence.
    pub fn eeprom_state(&self) -> heapless::Vec<u8, 128> {
        let mut buf = [0u8; 128];
        let mut store_copy = self.state_store.clone();
        let working = self.working_state();
        store_copy.save_working(&working);
        store_copy.to_eeprom(&mut buf);
        heapless::Vec::from_slice(&buf).unwrap_or_default()
    }

    /// Manually switch to a preset (e.g., on boot or from external command).
    /// Returns on_enter + recall MIDI in the result.
    pub fn switch_to(&mut self, preset_idx: u8, config: &Config) -> ControllerResult {
        let mut result = ControllerResult::new();
        self.do_switch_preset(preset_idx, &mut result, config);
        result
    }

    /// Set encoder value (for initial state setup from config defaults).
    pub fn set_encoder_value(&mut self, index: usize, value: u8) {
        let mut state = self.state_store.current().clone();
        if index < NUM_ENCODERS {
            state.encoder_values[index] = value;
        }
        self.state_store.save_working(&state);
    }

    // --- Private ---

    fn current_preset<'a>(&self, config: &'a Config) -> Option<&'a Preset> {
        config.presets.get(self.active_preset as usize)
    }

    fn handle_system_actions(&mut self, result: &mut ControllerResult, config: &Config) {
        // Drain pending system actions and execute them
        let actions: heapless::Vec<SystemAction, 2> = result.pending_system.clone();
        result.pending_system.clear();
        for action in &actions {
            self.execute_system_action(*action, result, config);
        }
    }

    fn process_button(
        &mut self,
        index: usize,
        edge: Edge,
        now_ms: u32,
        config: &Config,
    ) -> ControllerResult {
        let mut result = ControllerResult::new();

        let preset = match self.current_preset(config) {
            Some(p) => p,
            None => return result,
        };

        if index >= NUM_BUTTONS {
            return result;
        }

        let has_long_press = preset
            .buttons
            .get(index)
            .map(|b| !b.on_long_press.is_empty())
            .unwrap_or(false);

        let mode = preset
            .buttons
            .get(index)
            .map(|b| &b.mode)
            .unwrap_or(&ButtonMode::Momentary);

        if has_long_press {
            if matches!(mode, ButtonMode::Momentary) {
                match edge {
                    Edge::Activate => {
                        self.button_active[index] = true;
                        result.led_dirty = true;
                    }
                    Edge::Deactivate => {
                        self.button_active[index] = false;
                        result.led_dirty = true;
                    }
                }
            }

            if edge == Edge::Activate {
                if let Some(btn) = preset.buttons.get(index) {
                    let hint_action = btn.on_long_press.iter().find_map(|a| match a {
                        Action::PresetNext => Some(SystemAction::PresetNext),
                        Action::PresetPrev => Some(SystemAction::PresetPrev),
                        _ => None,
                    });
                    if let Some(action) = hint_action {
                        result
                            .display
                            .push(DisplayEvent::LongPressHint { action })
                            .ok();
                    }
                }
            }

            if let Some(gesture) = self.long_press[index].update(Some(edge), now_ms) {
                self.handle_gesture(index, gesture, preset, &mut result);
            }
        } else {
            match edge {
                Edge::Activate => {
                    let mut state = self.working_state();
                    let r = engine::process_button(&mut state, preset, index, ButtonEvent::Press);
                    self.apply_state(&state);
                    self.merge_engine_result(&r, &mut result);
                }
                Edge::Deactivate => {
                    let mut state = self.working_state();
                    let r = engine::process_button(&mut state, preset, index, ButtonEvent::Release);
                    self.apply_state(&state);
                    self.merge_engine_result(&r, &mut result);
                }
            }
        }

        result
    }

    fn handle_gesture(
        &mut self,
        index: usize,
        gesture: Gesture,
        preset: &Preset,
        result: &mut ControllerResult,
    ) {
        let mode = preset
            .buttons
            .get(index)
            .map(|b| &b.mode)
            .unwrap_or(&ButtonMode::Momentary);

        match gesture {
            Gesture::ShortPress => {
                result.display.push(DisplayEvent::LongPressCancel).ok();
                let mut state = self.working_state();
                let r = engine::process_button(&mut state, preset, index, ButtonEvent::Press);
                self.apply_state(&state);
                self.merge_engine_result(&r, result);

                if let Some(btn) = preset.buttons.get(index) {
                    if mode == &ButtonMode::Momentary || !btn.on_release.is_empty() {
                        let mut state2 = self.working_state();
                        let r2 = engine::process_button(
                            &mut state2,
                            preset,
                            index,
                            ButtonEvent::Release,
                        );
                        self.apply_state(&state2);
                        self.merge_engine_result(&r2, result);
                    }
                }
            }
            Gesture::LongPress => {
                let mut state = self.working_state();
                let r = engine::process_button(&mut state, preset, index, ButtonEvent::LongPress);
                self.apply_state(&state);
                self.merge_engine_result(&r, result);
            }
        }
    }

    fn process_encoder(
        &mut self,
        index: usize,
        clockwise: bool,
        now_ms: u32,
        config: &Config,
    ) -> ControllerResult {
        let mut result = ControllerResult::new();
        let preset = match self.current_preset(config) {
            Some(p) => p,
            None => return result,
        };

        if index >= NUM_ENCODERS {
            return result;
        }

        let steps = self.encoder_accel[index].steps(now_ms);
        let direction = if clockwise {
            EncoderDirection::Clockwise
        } else {
            EncoderDirection::CounterClockwise
        };

        let mut state = self.working_state();
        let r = engine::process_encoder(&mut state, preset, index, direction, steps);
        self.apply_state(&state);
        self.merge_engine_result(&r, &mut result);
        result
    }

    fn process_analog(
        &mut self,
        index: usize,
        raw: u16,
        min: u16,
        max: u16,
        config: &Config,
    ) -> ControllerResult {
        let mut result = ControllerResult::new();
        let preset = match self.current_preset(config) {
            Some(p) => p,
            None => return result,
        };
        let r = engine::process_analog(preset, index, raw, min, max);
        self.merge_engine_result(&r, &mut result);
        result
    }

    /// Merge engine result into controller result, intercepting system actions.
    fn merge_engine_result(&mut self, r: &EngineResult, result: &mut ControllerResult) {
        for step in &r.midi {
            result.midi.push(step.clone()).ok();
        }
        for d in &r.display {
            result.display.push(d.clone()).ok();
        }
        if r.led_dirty {
            result.led_dirty = true;
        }
        // System actions are collected — will be handled by the caller (process/tick)
        // via handle_system_actions after the event processing completes.
        // We store them temporarily in a hidden field... but ControllerResult doesn't
        // have system anymore. Let's use a simpler approach:
        // We handle them inline by storing in a temp vec on ControllerResult.
        for s in &r.system {
            result.pending_system.push(*s).ok();
        }
    }

    fn execute_system_action(
        &mut self,
        action: SystemAction,
        result: &mut ControllerResult,
        config: &Config,
    ) {
        let num_presets = config.presets.iter().filter(|p| !p.name.is_empty()).count() as u8;

        match action {
            SystemAction::PresetNext => {
                if num_presets > 0 {
                    let next = (self.active_preset + 1) % num_presets;
                    self.do_switch_preset(next, result, config);
                }
            }
            SystemAction::PresetPrev => {
                if num_presets > 0 {
                    let prev = if self.active_preset == 0 {
                        num_presets - 1
                    } else {
                        self.active_preset - 1
                    };
                    self.do_switch_preset(prev, result, config);
                }
            }
            SystemAction::PresetSelect(idx) => {
                if (idx as usize) < config.presets.len() {
                    self.do_switch_preset(idx, result, config);
                }
            }
            SystemAction::SetBpm(bpm) => {
                result.bpm = Some(bpm);
                result.display.push(DisplayEvent::BpmOverlay { bpm }).ok();
            }
            SystemAction::TapTempo => {
                // Handled via InputEvent::TapTempo
            }
        }
    }

    fn do_switch_preset(&mut self, new_idx: u8, result: &mut ControllerResult, config: &Config) {
        let old_preset = config.presets.get(self.active_preset as usize);
        let new_preset = config.presets.get(new_idx as usize);

        // Fire on_exit for old preset
        if let Some(old) = old_preset {
            for action in &old.on_exit {
                match action {
                    Action::Delay(ms) => {
                        result.midi.push(ActionStep::Delay(*ms)).ok();
                    }
                    _ => {
                        if let Some(msg) = action_to_midi(action) {
                            result.midi.push(ActionStep::Send(msg)).ok();
                        }
                    }
                }
            }
        }

        // Switch state
        if let Some(new_p) = new_preset {
            let mut working = self.working_state();
            let recall = self.state_store.switch(new_idx, &mut working, new_p);
            self.apply_state(&working);
            self.long_press = core::array::from_fn(|_| LongPressDetector::new_fired());
            self.encoder_accel = [EncoderAccel::new(), EncoderAccel::new()];
            self.active_preset = new_idx;

            // Fire on_enter for new preset
            for action in &new_p.on_enter {
                match action {
                    Action::Delay(ms) => {
                        result.midi.push(ActionStep::Delay(*ms)).ok();
                    }
                    _ => {
                        if let Some(msg) = action_to_midi(action) {
                            result.midi.push(ActionStep::Send(msg)).ok();
                        }
                    }
                }
            }

            // Recall MIDI (state sync)
            for msg in &recall {
                result.midi.push(ActionStep::Send(msg.clone())).ok();
            }
        }

        result.preset_changed = true;
        result.led_dirty = true;
    }

    fn working_state(&self) -> PresetState {
        let current = self.state_store.current();
        PresetState {
            button_active: self.button_active,
            cycle_index: current.cycle_index,
            encoder_values: current.encoder_values,
        }
    }

    fn apply_state(&mut self, state: &PresetState) {
        self.button_active = state.button_active;
        self.state_store.save_working(state);
    }
}
