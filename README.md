# OpenOctava

OpenOctava (binary: `octava`) is a lightweight, native digital audio workstation written in Rust. It aims to provide a fast, low-latency composing workflow without a browser runtime or a heavy plugin stack.

## Current status

The project is under active development. The current foundation includes:

- A tempo-aware project model with tracks, reusable MIDI clips (patterns), notes, undo/redo, and playback snapshots.
- A native `egui` interface with a timeline, pattern piano roll, clip library, mixer, inspector, transport, and custom titlebar.
- A real-time audio engine built on `cpal` and FluidLite with SoundFont 2 playback.
- Offline WAV rendering that reuses the same scheduler and synthesizer as live playback.
- A flat clip library: write a pattern once, drag it into the timeline, and every placement plays the same notes.
- Piano-roll note creation, movement, resizing, and deletion; timeline clip movement, right-edge resizing, and track reordering with configurable snapping.
- Per-track mixer strips with mute, solo, gain, and pan controls.
- Clip and track context menus, clip copy/paste/duplicate, drag-and-drop from the clip library, plus note audition from the piano roll and its key gutter.
- WAV export from the Export menu, reusing the live audio scheduler.

## Architecture

The workspace is split into focused crates:

| Crate | Responsibility |
| --- | --- |
| `octava-core` | Domain model, timeline math, edit commands, undo/redo |
| `octava-audio` | Real-time audio engine, scheduling, SoundFont synthesis, offline renderer |
| `octava-render` | WAV export built on the audio renderer |
| `octava-ui` | `egui` views, editor interaction, transport and titlebar |
| `octava-app` | Desktop application shell and integration |

The UI never calls audio APIs directly. It emits `UiAction` values that the application executes. The audio callback communicates through a wait-free SPSC command queue and never allocates or drops project data.

## Requirements

- Rust 1.99 or newer (edition 2024)
- ALSA development headers on Linux (`libasound2-dev` on Debian/Ubuntu)
- A working audio output device

FluidLite is built from source through the `fluidlite` crate's `builtin` feature, so no separate FluidSynth installation is required.

## Build and run

```sh
cargo run -p octava-app
```

Run the checks:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

## SoundFont

The demo uses `assets/soundfonts/VintageDreamsWaves-v2.sf2`, a freeware SoundFont by Ian Wilson. It is redistributed unmodified together with its copyright notice. See `assets/soundfonts/VintageDreamsWaves-v2.COPYRIGHT.txt`.

You can point OpenOctava at another SoundFont by setting `OCTAVA_SF2` to its path. Without a SoundFont the application still starts and falls back to the UI preview transport.

To pick a SoundFont from the app, open `Instrument → SoundFont Player…` on a clip and press `Choose…`. That opens the desktop's native file picker (through the XDG desktop portal) and stores the choice on the project; the bundled bank can be restored from the same window.

## Keyboard shortcuts

- `Space` - play / pause
- `Ctrl+Z` - undo
- `Ctrl+Shift+Z` or `Ctrl+Y` - redo
- `Ctrl+wheel` in the piano roll - zoom
- `Ctrl+C` / `Ctrl+V` in the timeline - copy the selected clip, paste at the playhead
- `Ctrl+D` in the timeline - duplicate the selected clip right after itself
- `Delete` / `Backspace` - delete the selected note

## Roadmap

- Project save/load and recent files
- MP3 export
- Richer clip editing (trim and split)
- Audio device selection and latency settings
- Metronome, MIDI input, and automation

## License

OpenOctava is licensed under MIT OR Apache-2.0. FluidLite is licensed under LGPL-2.1; see the dependency's license for details. The bundled SoundFont is subject to its own freeware terms.
