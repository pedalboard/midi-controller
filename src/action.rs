//! Action executor: converts preset button actions into raw MIDI bytes.

use crate::config::{Action, EncoderAction, Preset};

/// A MIDI message ready to send (up to 3 bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiMessage {
    pub data: [u8; 3],
    pub len: usize,
}

/// Execute on_press actions for a button index. Returns up to 8 MIDI messages.
pub fn execute_button_press(preset: &Preset, btn_idx: usize) -> heapless::Vec<MidiMessage, 8> {
    let mut messages = heapless::Vec::new();
    let Some(btn) = preset.buttons.get(btn_idx) else {
        return messages;
    };
    for action in &btn.on_press {
        if let Some(msg) = action_to_midi(action) {
            messages.push(msg).ok();
        }
    }
    messages
}

/// Convert a single Action to a raw MIDI message.
pub fn action_to_midi(action: &Action) -> Option<MidiMessage> {
    match action {
        Action::Midi { data, len } => Some(MidiMessage {
            data: *data,
            len: *len as usize,
        }),
        _ => None,
    }
}

/// Direction for encoder pulses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderDirection {
    Clockwise,
    CounterClockwise,
}

/// Generate a CC message for an encoder pulse. `current_value` is updated in place.
/// Returns None if the encoder has no action configured or uses PresetScroll.
pub fn encoder_cc(
    preset: &Preset,
    encoder_idx: usize,
    direction: EncoderDirection,
    current_value: &mut u8,
) -> Option<MidiMessage> {
    let enc = preset.encoders.get(encoder_idx)?;
    match &enc.action {
        EncoderAction::Cc {
            cc,
            channel,
            min,
            max,
        } => {
            let val = match direction {
                EncoderDirection::Clockwise => (*current_value).saturating_add(1).min(*max),
                EncoderDirection::CounterClockwise => (*current_value).saturating_sub(1).max(*min),
            };
            *current_value = val;
            Some(MidiMessage {
                data: [0xB0 | (channel - 1), *cc as u8, val],
                len: 3,
            })
        }
        EncoderAction::CcRelative {
            cc,
            channel,
            increment,
            decrement,
        } => {
            let val = match direction {
                EncoderDirection::Clockwise => *increment,
                EncoderDirection::CounterClockwise => *decrement,
            };
            Some(MidiMessage {
                data: [0xB0 | (channel - 1), *cc, val],
                len: 3,
            })
        }
        EncoderAction::PresetScroll => None,
    }
}

/// Generate a CC message for an analog input. `raw` is the ADC reading (0-4095).
pub fn analog_cc(
    preset: &Preset,
    analog_idx: usize,
    raw: u16,
    adc_min: u16,
    adc_max: u16,
) -> Option<MidiMessage> {
    let cfg = preset.analog.get(analog_idx)?;
    let range = cfg.max - cfg.min;
    // Clamp raw to calibrated range and normalize
    let clamped = raw.clamp(adc_min, adc_max);
    let span = adc_max.saturating_sub(adc_min).max(1) as u32;
    let value =
        cfg.min + (((clamped - adc_min) as u32 * range as u32) / span).min(range as u32) as u8;
    Some(MidiMessage {
        data: [0xB0 | (cfg.channel - 1), cfg.cc, value],
        len: 3,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;
    use heapless::Vec;

    fn make_preset(buttons: Vec<ButtonConfig, MAX_BUTTONS>) -> Preset {
        Preset {
            name: Label::new(),
            buttons,
            encoders: Vec::new(),
            analog: Vec::new(),
            defaults: Default::default(),
            on_enter: heapless::Vec::new(),
            on_exit: heapless::Vec::new(),
        }
    }

    #[test]
    fn note_on_action() {
        let msg = action_to_midi(&Action::note_on(60, 1).unwrap()).unwrap();
        assert_eq!(msg.data, [0x90, 60, 127]);
        assert_eq!(msg.len, 3);
    }

    #[test]
    fn note_on_channel_2() {
        let msg = action_to_midi(&Action::note_on(64, 2).unwrap()).unwrap();
        assert_eq!(msg.data, [0x91, 64, 127]);
    }

    #[test]
    fn cc_action() {
        let msg = action_to_midi(&Action::cc(10, 127, 1).unwrap()).unwrap();
        assert_eq!(msg.data, [0xB0, 10, 127]);
        assert_eq!(msg.len, 3);
    }

    #[test]
    fn program_change_action() {
        let msg = action_to_midi(&Action::program_change(5, 3).unwrap()).unwrap();
        assert_eq!(msg.data, [0xC2, 5, 0]);
        assert_eq!(msg.len, 2);
    }

    #[test]
    fn unsupported_action_returns_none() {
        assert!(action_to_midi(&Action::Delay(100)).is_none());
        assert!(action_to_midi(&Action::PresetNext).is_none());
    }

    #[test]
    fn execute_button_press_multi_action() {
        let mut buttons: Vec<ButtonConfig, MAX_BUTTONS> = Vec::new();
        let mut on_press: Vec<Action, MAX_ACTIONS> = Vec::new();
        on_press.push(Action::program_change(0, 1).unwrap()).ok();
        on_press.push(Action::cc(69, 127, 1).unwrap()).ok();
        buttons
            .push(ButtonConfig {
                label: Label::new(),
                color: LedConfig::default(),
                mode: ButtonMode::default(),
                on_press,
                cycle_values: Vec::new(),
                on_release: Vec::new(),
                on_long_press: Vec::new(),
            })
            .ok();

        let preset = make_preset(buttons);
        let msgs = execute_button_press(&preset, 0);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].data, [0xC0, 0, 0]);
        assert_eq!(msgs[1].data, [0xB0, 69, 127]);
    }

    #[test]
    fn execute_button_press_invalid_index() {
        let preset = make_preset(Vec::new());
        let msgs = execute_button_press(&preset, 5);
        assert!(msgs.is_empty());
    }

    fn make_encoder_preset() -> Preset {
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
            })
            .ok();
        encoders
            .push(EncoderConfig {
                label: Label::try_from("Pan").unwrap(),
                action: EncoderAction::CcRelative {
                    cc: 16,
                    channel: 2,
                    increment: 65,
                    decrement: 63,
                },
            })
            .ok();
        Preset {
            name: Label::new(),
            buttons: Vec::new(),
            encoders,
            analog: Vec::new(),
            defaults: Default::default(),
            on_enter: heapless::Vec::new(),
            on_exit: heapless::Vec::new(),
        }
    }

    #[test]
    fn encoder_cc_clockwise() {
        let preset = make_encoder_preset();
        let mut val = 64u8;
        let msg = encoder_cc(&preset, 0, EncoderDirection::Clockwise, &mut val).unwrap();
        assert_eq!(msg.data, [0xB0, 7, 65]);
        assert_eq!(val, 65);
    }

    #[test]
    fn encoder_cc_counter_clockwise() {
        let preset = make_encoder_preset();
        let mut val = 64u8;
        let msg = encoder_cc(&preset, 0, EncoderDirection::CounterClockwise, &mut val).unwrap();
        assert_eq!(msg.data, [0xB0, 7, 63]);
        assert_eq!(val, 63);
    }

    #[test]
    fn encoder_cc_clamps_at_max() {
        let preset = make_encoder_preset();
        let mut val = 127u8;
        let msg = encoder_cc(&preset, 0, EncoderDirection::Clockwise, &mut val).unwrap();
        assert_eq!(msg.data, [0xB0, 7, 127]);
        assert_eq!(val, 127);
    }

    #[test]
    fn encoder_cc_clamps_at_min() {
        let preset = make_encoder_preset();
        let mut val = 0u8;
        let msg = encoder_cc(&preset, 0, EncoderDirection::CounterClockwise, &mut val).unwrap();
        assert_eq!(msg.data, [0xB0, 7, 0]);
        assert_eq!(val, 0);
    }

    #[test]
    fn encoder_cc_relative() {
        let preset = make_encoder_preset();
        let mut val = 0u8;
        let msg = encoder_cc(&preset, 1, EncoderDirection::Clockwise, &mut val).unwrap();
        assert_eq!(msg.data, [0xB1, 16, 65]);
        let msg = encoder_cc(&preset, 1, EncoderDirection::CounterClockwise, &mut val).unwrap();
        assert_eq!(msg.data, [0xB1, 16, 63]);
    }

    #[test]
    fn encoder_invalid_index_returns_none() {
        let preset = make_encoder_preset();
        let mut val = 0u8;
        assert!(encoder_cc(&preset, 5, EncoderDirection::Clockwise, &mut val).is_none());
    }

    #[test]
    fn analog_cc_mid_range() {
        let mut analog: Vec<AnalogConfig, MAX_ANALOG> = Vec::new();
        analog
            .push(AnalogConfig {
                label: Label::try_from("Exp").unwrap(),
                cc: 11,
                channel: 1,
                min: 0,
                max: 127,
            })
            .ok();
        let preset = Preset {
            name: Label::new(),
            buttons: Vec::new(),
            encoders: Vec::new(),
            analog,
            defaults: Default::default(),
            on_enter: heapless::Vec::new(),
            on_exit: heapless::Vec::new(),
        };
        let msg = analog_cc(&preset, 0, 2048, 0, 4095).unwrap();
        assert_eq!(msg.data[0], 0xB0);
        assert_eq!(msg.data[1], 11);
        // 2048/4095 * 127 ≈ 63
        assert!(msg.data[2] >= 63 && msg.data[2] <= 64);
    }

    #[test]
    fn analog_cc_with_range() {
        let mut analog: Vec<AnalogConfig, MAX_ANALOG> = Vec::new();
        analog
            .push(AnalogConfig {
                label: Label::new(),
                cc: 4,
                channel: 2,
                min: 20,
                max: 100,
            })
            .ok();
        let preset = Preset {
            name: Label::new(),
            buttons: Vec::new(),
            encoders: Vec::new(),
            analog,
            defaults: Default::default(),
            on_enter: heapless::Vec::new(),
            on_exit: heapless::Vec::new(),
        };
        // Full deflection
        let msg = analog_cc(&preset, 0, 4095, 0, 4095).unwrap();
        assert_eq!(msg.data, [0xB1, 4, 100]);
        // Zero
        let msg = analog_cc(&preset, 0, 0, 0, 4095).unwrap();
        assert_eq!(msg.data, [0xB1, 4, 20]);
    }

    #[test]
    fn analog_invalid_index_returns_none() {
        let preset = Preset {
            name: Label::new(),
            buttons: Vec::new(),
            encoders: Vec::new(),
            analog: Vec::new(),
            defaults: Default::default(),
            on_enter: heapless::Vec::new(),
            on_exit: heapless::Vec::new(),
        };
        assert!(analog_cc(&preset, 0, 2048, 0, 4095).is_none());
    }

    #[test]
    fn analog_cc_with_calibration() {
        let mut analog: Vec<AnalogConfig, MAX_ANALOG> = Vec::new();
        analog
            .push(AnalogConfig {
                label: Label::new(),
                cc: 7,
                channel: 1,
                min: 0,
                max: 127,
            })
            .ok();
        let preset = Preset {
            name: Label::new(),
            buttons: Vec::new(),
            encoders: Vec::new(),
            analog,
            defaults: Default::default(),
            on_enter: heapless::Vec::new(),
            on_exit: heapless::Vec::new(),
        };
        // Calibrated range: 200–3700
        // At heel (200): output = 0
        let msg = analog_cc(&preset, 0, 200, 200, 3700).unwrap();
        assert_eq!(msg.data[2], 0);
        // At toe (3700): output = 127
        let msg = analog_cc(&preset, 0, 3700, 200, 3700).unwrap();
        assert_eq!(msg.data[2], 127);
        // Midpoint: ~63-64
        let msg = analog_cc(&preset, 0, 1950, 200, 3700).unwrap();
        assert!(msg.data[2] >= 63 && msg.data[2] <= 64);
        // Below min: clamped to 0
        let msg = analog_cc(&preset, 0, 50, 200, 3700).unwrap();
        assert_eq!(msg.data[2], 0);
        // Above max: clamped to 127
        let msg = analog_cc(&preset, 0, 4000, 200, 3700).unwrap();
        assert_eq!(msg.data[2], 127);
    }
}
