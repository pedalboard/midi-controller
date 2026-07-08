# midi-controller

Generic MIDI controller engine — button/encoder input processing, LED ring rendering, preset state management, and MIDI-CI Property Exchange framing.

## Features

- `#![no_std]` compatible — runs on microcontrollers and in simulators
- Input processing: debounce, long-press detection, encoder acceleration, tap tempo
- LED ring rendering with animations and modifiers
- Preset and global config management with flash persistence support
- MIDI-CI Property Exchange (PE) message framing
- Serialization via `postcard` + `heapless` collections

## Usage

```toml
[dependencies]
midi-controller = "0.1"
```

```rust
use midi_controller::controller::{Controller, Event, Output};
use midi_controller::config::Config;
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
