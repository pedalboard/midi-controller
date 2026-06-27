//! Action executor: converts preset button actions into raw MIDI bytes.

use crate::config::{Action, Preset};

/// A MIDI message ready to send (up to 3 bytes).
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
        Action::Cc { cc, value, channel } => Some(MidiMessage {
            data: [0xB0 | (channel - 1), *cc, *value],
            len: 3,
        }),
        Action::ProgramChange { program, channel } => Some(MidiMessage {
            data: [0xC0 | (channel - 1), *program, 0],
            len: 2,
        }),
        Action::NoteOn { note, channel } => Some(MidiMessage {
            data: [0x90 | (channel - 1), *note, 127],
            len: 3,
        }),
        Action::NoteOff { note, channel } => Some(MidiMessage {
            data: [0x80 | (channel - 1), *note, 0],
            len: 3,
        }),
        _ => None,
    }
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
        }
    }

    #[test]
    fn note_on_action() {
        let msg = action_to_midi(&Action::NoteOn {
            note: 60,
            channel: 1,
        })
        .unwrap();
        assert_eq!(msg.data, [0x90, 60, 127]);
        assert_eq!(msg.len, 3);
    }

    #[test]
    fn note_on_channel_2() {
        let msg = action_to_midi(&Action::NoteOn {
            note: 64,
            channel: 2,
        })
        .unwrap();
        assert_eq!(msg.data, [0x91, 64, 127]);
    }

    #[test]
    fn cc_action() {
        let msg = action_to_midi(&Action::Cc {
            cc: 10,
            value: 127,
            channel: 1,
        })
        .unwrap();
        assert_eq!(msg.data, [0xB0, 10, 127]);
        assert_eq!(msg.len, 3);
    }

    #[test]
    fn program_change_action() {
        let msg = action_to_midi(&Action::ProgramChange {
            program: 5,
            channel: 3,
        })
        .unwrap();
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
        on_press
            .push(Action::ProgramChange {
                program: 0,
                channel: 1,
            })
            .ok();
        on_press
            .push(Action::Cc {
                cc: 69,
                value: 127,
                channel: 1,
            })
            .ok();
        buttons
            .push(ButtonConfig {
                label: Label::new(),
                color: LedConfig::default(),
                mode: ButtonMode::default(),
                on_press,
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
}
