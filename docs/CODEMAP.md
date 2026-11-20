# VoxShift code map

An index for finding things without reading every file. Names, not line numbers: grep for the
symbol. Line counts are from 2026-11-20 and only show the size. Background: ARCHITECTURE.md
(signal flow, real-time rules), TESTING.md (manual checklist). Keep this file up to date when
modules change.

## Architecture in one screen

**Threads**

| Thread | Where | Does |
|---|---|---|
| Window (main) | `ui::VoxApp` (eframe) | Draws the pages. `eframe::App::logic` → `VoxApp::tick` also runs while the window is hidden (tray events, autosave). |
| Audio output callback | `audio::engine::Engine` (a `BlockSource`) on the virtual-mic cpal stream | Drives everything, 64-frame blocks (`audio::block::BLOCK`). |
| Mic input callbacks | `audio::streams::open_input` | Only downmix and push into a ring. |
| Monitor callback | `audio::engine::MonitorSource` | Plays the monitor ring in the headphones. |
| `device-manager` | `audio::device_manager` (`Manager::run`) | The only thread that talks to cpal: scans every 1.5 s, reopens dead streams, watchdog, picks the mic (`mic_select`), frees garbage. |
| `key-listener` | `soundboard::hotkeys::win::Listener` | Hidden window with Raw Input (RIDEV_INPUTSINK). Sends `RawKey` over mpsc. |
| `hotkeys` | `HotkeyService::start` worker | `KeyTracker` → `KeyEvent`; capture for the UI; `VoiceKeys` (`KeyHandler`) first, then `Board::hotkey_pressed`. Checks held clips every 50 ms. |
| `tray` | `shell::tray::Tray` | Win32 message loop: tray icon, menu, overlay banner (`shell::overlay`), second-launch "show". |
| `sound-loader` | `soundboard::library::Soundboard::new` | Decodes, resamples, shapes and measures clips (`Job`s over mpsc). |

**How they talk**

- `audio::shared::Shared` (in an `Arc`): plain atomics for every switch, level and counter
  (`voice_on`, `mic_muted`, `safe_mode`, `stop_sounds` counter, meters, playing ids and
  progress, melody state, `params: ParamStore`). The audio thread never waits for a lock.
- `EngineCmd` (`AttachMic`, `AttachMonitor`, `Sound(SoundCmd)`, `Listen(SoundCmd)`,
  `AttachTap`) goes over a lock-free `rtrb` queue. Other threads send through
  `device_manager::EnginePort` (a mutex around the producer, which only non-audio threads lock).
  `Garbage` comes back the other way, so buffers are freed on the device manager thread.
- `SoundCmd` (`Play`, `SetVolume`, `Stop`, `StopAll`) goes to `soundboard::mixer::SoundMixer`
  inside the engine.
- `ManagerCmd` (`Devices`, `Mic`, `Rescan`, `Quit`) goes from the window to the manager over
  mpsc. `DeviceStatus` comes back in an `Arc<Mutex<_>>` (`ManagerHandle::status`),
  `Event::Switched` over mpsc.
- The soundboard `Board` sits in `Arc<Mutex<Board>>`, shared by the window, the loader and
  the hotkey worker (none of them real-time).
- Hotkey thread → window: `voice_keys::VoiceRequest` (`Preset`, `ToggleMelody`) over mpsc,
  plus a wake closure (repaint and tray refresh). The tray handles its toggles on `Shared`
  directly and sends `Show`/`Quit`/`Preset` as `shell::TrayAction` over mpsc. It reads the
  config through `VoxApp.mirror` (`Arc<Mutex<Config>>`, also saved on Windows shutdown).
- Melody: `melody::store::MelodyStore`, a seqlock over atomic arrays (window writes, engine reads).

**Audio path** (per block, in `Engine::render`): each mic `AdaptiveReader` (drift-corrected
resampling) → crossfade between primary and fallback (`MIC_FADE_S`) → `dsp::chain::VoiceChain`
(gate, melody voice, pitch, tone, distortion, robot, chorus, echo, reverb, dry/wet, output
gain; skipped/clean while voice off or safe mode) → `+` soundboard `SoundMixer` (after the
chain, so effects never touch clips) → clip protection and meters → virtual mic. The monitor
ring gets voice and/or clips per `monitor_voice` / `monitor_sounds`, plus cues
(`dsp::cue`), the melody preview (`dsp::preview`) and `Listen` previews.

**Config**: loaded in `main.rs` (`config::load_from`), owned by `VoxApp.cfg`, autosaved
1 s after the last change (`VoxApp::autosave` compares with `saved`), written atomically by
`config::save_to`. See "Config" at the end.

## Files

Format: file (lines): what it does. Main items. Depends on.

### Top level
- `main.rs` (204): CLI (`--list-devices`, `--selftest [switch]`), single instance, loads the
  config, starts eframe with `ui::VoxApp`. `list_devices`, `selftest`, `main`.
- `keymap.rs` (669): who owns which hotkey. `check` → `Check` (`Assign`, `AlreadyYours`,
  `Conflict(Owner)`, `Reserved`), `reserved`, `owner`, `name_of`, `current`, `Slot`
  (`Main`/`Second`), `assign`, `assign_slot`, `clear`, `change`, `dedupe`, `SoundKey`.
  Depends on: `voice_keys`, `soundboard::hotkeys`.
- `voice_keys.rs` (899): voice-changer hotkeys at runtime. `VoiceHotkeys` (saved),
  `KeyTarget`, `VoiceKeyMode`, `PresetHotkey`, `Feedback` (switch voice/mute/monitor/safe
  mode with beeps and overlay), `VoiceKeys` (impl `KeyHandler`), `VoiceRequest`. Depends on:
  `audio::shared`, `dsp::cue`, `soundboard::hotkeys`.
- `keynames.rs` (195): language-independent key names. `key_name`, `parts`,
  `strip_modifiers`, `JOIN`.

### audio/
- `mod.rs` (10): module list.
- `block.rs` (95): `BLOCK` = 64, `StereoBlock`, `BlockSource`, `BlockAdapter` (device buffer
  sizes ↔ fixed blocks).
- `adaptive.rs` (265): `AdaptiveReader`, rubato async resampler with ring-fill control
  (clock drift, 44.1 to 48 kHz). `target_fill`.
- `streams.rs` (230): opens cpal streams. `open_input`, `open_output_with`, `output_config`,
  `StreamHealth`, `Opened`, `is_fatal`.
- `engine.rs` (1238): the real-time engine. `Engine`, `EngineCmd`, `Garbage`, `EngineLink`,
  `MonitorSource`, `MIC_FADE_S`, `MONITOR_RAMP_S`. Depends on: `dsp::*`, `soundboard::mixer`,
  `shared`, `adaptive`.
- `device_manager.rs` (804): the manager thread. `ManagerHandle` (`spawn`, `detached`,
  `send`, `status`, `shutdown`), `ManagerCmd`, `DeviceSettings` (saved), `DeviceStatus`,
  `LinkState`, `Endpoint`, `LatencyParts`, `Event`, `EnginePort`. No tests in this file.
- `mic_select.rs` (373): pure mic choice. `MicSelector`, `MicSettings` (saved), `MicMode`,
  `SilenceDetector`, `choose`, `Selection`.
- `matching.rs` (159): find a device again by fuzzy name. `normalize`, `similarity`,
  `best_match`, `find_cable_input`.
- `shared.rs` (283): `Shared`, `AtomicF32`, `ActiveMic`, `PeakMeter`, `to_db`, `from_db`.

### config/
- `mod.rs` (619): `Config` (everything saved), `Page`, `CloseBehavior`, `StartVoice`,
  `FeedbackSettings`, `MelodySettings`, `WindowPlacement`, `config_dir`, `config_path`,
  `load_from`, `save_to`. Depends on most modules' saved types.
- `presets.rs` (383): `Preset`, built-ins as changes from the defaults (`make_builtin`),
  `builtin`, `builtin_presets`, `favorites_first`, `name_problem`, `apply`, `matches`.
- `pack.rs` (405): `.voxpack` export/import (presets plus MIDI bytes). `Pack`, `export`,
  `save`, `load`, `import`, `Imported`.

### dsp/
One effect per file, each working in place on a mono `Block` with a `reset`. The effects
have `new(fs)` and `process(&mut Block, &Snapshot)`; the chain takes one `params::Snapshot`
per block.
- `mod.rs` (39): `Block`, `VoiceProcessor` (the trait for a future neural model).
- `params.rs` (592): every parameter once. `P` (enum), `PARAMS` table (key, label, range,
  default), `ParamInfo`, `Kind`, `Snapshot`, `ParamStore` (atomics), `EffectSettings`
  (key → value map, saved).
- `chain.rs` (715): `VoiceChain` (the order, fades per section, `run`), `PitchStage` (pitch +
  LPC formants). The `section!` macro wires each effect to its `P::…On` toggle.
- `smooth.rs` (131): `Smoother`, `Ramp`. `biquad.rs` (219): `Coeffs`, `Biquad`.
  `delay.rs` (60): `DelayLine`.
- Effects: `gate.rs` (113) `Gate` · `eq.rs` (131) `Tone` · `distortion.rs` (60) ·
  `robot.rs` (62) · `chorus.rs` (58) · `echo.rs` (88) · `reverb.rs` (134) Freeverb ·
  `pitch.rs` (306) `PitchShifter`, `semitones_to_ratio` · `lpc.rs` (310) `Lpc`, `Whitener`,
  `Colorer`.
- Melody Voice: `pitch_detect.rs` (703) YIN `PitchDetector`, `PitchTracker`,
  `hz_to_midi`, `note_name` · `conductor.rs` (905) which note when · `melody.rs` (810)
  `MelodyVoice` (the effect), `MelodyStatus`, `latency_ms`.
- `cue.rs` (222): switch beeps, `CueKind`, `Cue`. `preview.rs` (226): "Play melody" synth.

### melody/
- `mod.rs` (221): `Note`, `Melody`, `MelodySource` (saved), `load`, `Loaded`, `median_key`.
- `library.rs` (236): built-in tunes in text notation. `BUILT_IN`, `get`, `parse`.
- `midi.rs` (464): MIDI import with `midly`. `parse`, `MidiSong`, `Part`, `skyline`.
- `store.rs` (160): `MelodyStore` seqlock (`publish`, `read_into`, `version`).

### shell/ (Win32)
- `mod.rs` (174): `TrayStatus`, `TrayAction`, `Health`, `preset_items`, `virtual_screen`,
  `wide`. `tray`, `overlay`, `instance` and `autostart` are `#[cfg(windows)]`.
- `other.rs` (76, not Windows): do-nothing `autostart`, `instance` and `tray` with the same
  names, so the app and its tests build on Linux. Change these when a Windows signature
  changes.
- `tray.rs` (606): `Tray` (`start`, `notify`, `refresh`, `poster`, `shutdown`),
  `TrayPoster`. Not fmt-clean upstream.
- `overlay.rs` (153): the "Voice changer: ON/OFF" banner, `create`, `show`.
- `icon.rs` (153): the icon drawn in code, `render`, `disc_color`.
- `instance.rs` (88): single instance, `acquire`, `signal_existing`, `WM_SHOW_APP`.
- `autostart.rs` (129): HKCU Run key, `is_enabled`, `set_enabled`, `points_elsewhere`.

### soundboard/
- `mod.rs` (11): module list.
- `hotkeys.rs` (866): global hotkeys. `Hotkey` (saved: vk, modifiers, `label`, optional
  `second`; `same_keys`, `matches`, `keys`, `pair`, `single`, `pretty`, `pretty_all`,
  `parts`), `RawKey`, `KeyTracker`, `KeyEvent`, `KeyHandler`, `HotkeyService`
  (`start`, `begin_capture`, `poll_capture`, `set_paused`, `presses_seen`), `Captured`,
  `diag` (capture troubleshooting log), `mod win` (Raw Input, `key_down`, `key_name`).
- `library.rs` (1023): `SoundEntry` (saved per clip), `Sound`, `LoadState`, `Board` (the
  clip list: `add`, `take`/`restore`, `press`/`release`, `hotkey_pressed`, `stop_all`,
  `preview`, `set_*`), `Soundboard` (board plus loader thread), `Removed`.
- `mixer.rs` (348): real-time `SoundMixer` (16 `VOICES`), `SoundCmd`, `SoundData`,
  `SoundId`.
- `playmode.rs` (266): `PlayMode` (Normal, Hold, Toggle, loop while held), `Triggers`,
  `missed_releases`, `MOUSE`.
- `organize.rs` (540): folders, tabs, search, sort. `BoardSettings` (saved), `Folder`,
  `BoardTab`, `SortMode`, `BoardView`, `visible`, `matches`, `fold`, `reorder`,
  `push_recent`, `BoardSettings::repair`.
- `decode.rs` (213): symphonia decode to stereo f32. `load`, `decode_file`,
  `resample_stereo`, `EXTENSIONS`.
- `shape.rs` (168): trim and fades, `Trim`, `apply`. `loudness.rs` (132): `measure`,
  `Level` (for "Even out loudness"). `names.rs` (161): `clean_name`, `typed_name`.

### ui/ (egui)
- `mod.rs` (5220): `VoxApp` and everything that ties pages to state (TOC below).
- `header.rs` (623): voice pill, mic, meters, problem chips, Mute, PANIC. `View`, `Action`.
- `nav.rs` (187): navigation rail. `show`, `icon`, `width`.
- `effects.rs` (549): effect cards. `SECTIONS` (one `Section` per card: toggle and controls),
  `tip`, `control`, `chain`, `ab_button`.
- `presets.rs` (429): preset chips, save/revert, share/import.
- `melody.rs` (678): the Melody Voice section and piano roll.
- `soundboard.rs` (2125): the Soundboard page (TOC below). `State`, `View`, `Action`.
- `audio_setup.rs` (781): Audio setup page (signal path, virtual mic, mics, monitor, latency).
- `hotkeys_page.rs` (596): the Hotkeys page. `groups` (every row), `row_matches`, `page`,
  `View`, `Action` (`Capture(target, Slot)`, `Clear`, `ClearSecond`, preset rows).
- `settings.rs` (464): Settings page. `wizard.rs` (256): first-run wizard.
  `mic_check.rs` (289): 5-second recording of the virtual mic and playback.
- `shortcuts.rs` (277): in-window shortcuts (Ctrl+1..5, Space, Delete, ?).
- `widgets.rs` (1280): shared widgets (TOC below). `theme.rs` (601): palette, spacing, fonts,
  accent. `icons.rs` (180): Phosphor glyph constants. `meters.rs` (272): level meters.
  `toast.rs` (380): toasts with optional action (Undo). `tray_status.rs` (153): tray icon
  state from the audio status.
- `snapshot.rs` (1185, test-only): software rasterizer and screenshot tests (TOC below).

## Tables of contents for big files

Grep these names. They are listed roughly in file order.

**ui/mod.rs** (5220; the tests start at the second `#[cfg(test)]`, about half the file)
- State: `VoxApp` (fields), `CaptureWatch`, `Undo`, `KeyConflict`, `CloseDialog`,
  `dedupe_hotkeys`, `apply_to_shared`, `apply_feedback`, `start_voice_on`.
- Start-up: `VoxApp::new`, `start_hotkeys`, `start_tray`, `with_manager` (config cleanup at
  load), `first_run_defaults`.
- Per frame: `tick` (runs while hidden), `handle_manager_events`, `sync_voice_keys`,
  `follow_accent`, `handle_shell_requests`, `autosave`, `save_now`, `draw`, `page` (free fn,
  page column layout), `impl eframe::App for VoxApp`.
- Window and tray: `remember_window`, `handle_close_request`, `hide_to_tray`, `show_window`,
  `quit`, `refresh_tray`, `sync_mirror`, `shutdown_background`, `close_dialog`,
  `apply_close_choice`.
- Pages: `voice_page`, `melody_panel`, `soundboard_page`, `audio_page`, `hotkeys_page`,
  `hotkey_settings`, `settings_page`, `reset_settings`, `wizard_dialog`, `header`,
  `apply_header_action`, `apply_shortcut`.
- Melody and presets: `select_melody`, `import_midi`, `apply_preset`, `apply_preset_action`,
  `export_pack`, `import_pack`, `jump_to_preset`, `effects_changed`, `preset_state`.
- Soundboard: `with_board_view`, `apply_sound_action` (the big `soundboard::Action` match),
  `add_sounds`, `handle_dropped_files`, `preview_clip`, `edit_clip`, `offer_undo`, `undo`,
  `sounds_changed`.
- Hotkey assignment: `begin_capture`, `begin_capture_slot`, `cancel_capture`,
  `poll_hotkey_capture`, `check_capture_watch`, `key_captured_slot`, `with_keymap`,
  `set_hotkey_slot`, `hotkey_in_slot`, `hotkey_check`, `clear_hotkey`,
  `clear_second_hotkey`, `key_conflict_dialog`, `resolve_key_conflict`.
- Mic check: `toggle_mic_check`, `tick_mic_check`.
- Test helpers: `fake_status`, `test_app`, `frame`, `frame_with`, `close_frame`,
  `wait_for` (Windows), `click`, `f8`.

**ui/soundboard.rs** (2125): `Action`, `State` (`open`/`close`/`editing`), `View`,
`toolbar`, `toolbar_left`, `toolbar_right`, `action_key_menu` (Stop all / hear-clips keys),
`capture_banner`, `tab_bar`, `tab`, `folder_menu`, `drop_zone`, `empty_note`, `panel` (page
entry), `clips`, `Look` (`paint_pad`, `paint_row`, `paint_progress`), `pad`, `pad_tip`,
`clip_menu`, `volume_slider`, `now_playing`, `now_chip`, `trim_section`, `edit_panel`,
`side_panels`.

**ui/widgets.rs** (1280): `toggle`, `Slider` (builder; `show`), `format_value`,
`parse_value`, `card`, `card_frame`, `modal`, `section_title`, `page_title`, `notice`,
`column_label`, `hint`, `Keycaps`, `keycaps`, `KeySlot`, `hotkey_button`, `Tone`, `chip`,
`badge`, `segmented`, `status_dot`, `primary_button`, `danger_button`, `icon_button`,
`menu_icon_button`; test-only `remember` / `rect_of` (widget rects for click tests).

**audio/engine.rs** (1238): `EngineCmd`, `Garbage`, `Engine` (`new`, `apply` = command
handling, `retire`, `mic_block`), `impl BlockSource for Engine` (`render` = the per-block
path), `MonitorSource`; tests from `mod tests` (`rig` helper).

**soundboard/library.rs** (1023): `SoundEntry`, `Sound` (`play_volume`, `shape`), `Job`,
`Board` (see Files), `Soundboard::new` (loader loop), tests (`rig` helper).

**ui/snapshot.rs** (1185): `Textures`, `Canvas` (`draw`, `save_png`), `Shot` (`step`,
`wait`, `shoot`); tests `shots_of_the_widget_gallery`, `shots_of_the_app`,
`shots_of_the_setup_pages`, `shots_for_the_readme` (Windows, real listener),
`shots_of_the_extras`; helpers `gallery`, `healthy_status`, `demo_app`, `app_shot`,
`app_shot_clicking`.

**dsp/conductor.rs** (905): `Mode`, `Scale`, `snap`, `KEY_NAMES`, `Settings::from_params`,
`Frame`, `Position`, `Conductor` (`frame` = entry, `per_syllable`, `tempo`, `manual`,
`level_dip`, `pitch_break`, `advance`, `update_fold`, `target`).

**voice_keys.rs** (899): `VoiceKeyMode`, `PresetHotkey`, `VoiceHotkeys` (`action_keys`
lists the one-shot actions with their names), `KeyTarget`, `conflict` (test-only),
`Feedback`, `VoiceRequest`, `VoiceKeys` (`set_bindings`), `impl KeyHandler for VoiceKeys`
(`pressed` = what every key does, `released` for hold modes); tests (`rig`, `hk`).

**soundboard/hotkeys.rs** (866): `diag`, `Hotkey`, `RawKey`, `KeyTracker`, `KeyHandler`,
`Captured`, `check_held`, `HotkeyService` (worker loop in `start`), `mod win` (`Listener`,
`read_key`, `wndproc`); `pub(crate) mod tests` with `HOOK_TESTS`, `VK_F24`, `tap_f24`, `f24`.

**dsp/melody.rs** (810): `MelodyStatus`, `detector_settings`, `latency_samples`,
`MelodyVoice` (`sync`, `process`, `on_frame`, `align`); test helpers (`measured_midi`,
`voice`, `syllables`, `speech`, `sung_notes`).

**audio/device_manager.rs** (804): public types (see Files), then `Watchdog`, `Slot`,
`Manager` (`run`, `handle`, `tick`, `scan`, `check_output`/`open_output`,
`check_monitor`, `check_mic`/`open_mic`, `select_mic`, `latency_parts`, `publish`,
`collect_garbage`).

## Where to change X

**A new hotkey action** (e.g. "next preset")
1. `voice_keys.rs`: a field on `VoiceHotkeys` (`Option<Hotkey>`, the struct has
   `#[serde(default)]`), a `KeyTarget` variant, an entry in `VoiceHotkeys::action_keys`
   (name shown in conflicts), and its effect in `impl KeyHandler for VoiceKeys::pressed`.
2. `keymap.rs`: `slot_of` (the match on `KeyTarget`). `bindings` picks it up through
   `action_keys`.
3. `ui/hotkeys_page.rs`: a `row(...)` in `groups`.
4. Optional: tray menu (`shell::TrayAction`, `shell/tray.rs`, `VoxApp::start_tray`),
   soundboard toolbar (`ui/soundboard.rs` `action_key_menu`).
5. Tests: `voice_keys.rs` (rig + `pressed`), `keymap.rs` (conflict names), the
   `hotkeys_page` test checks every `action_keys` entry has a row; config round trip in
   `config/mod.rs` (`voice_hotkeys_feedback_and_start_state_round_trip`).

**A new DSP effect**
1. `dsp/<effect>.rs`: struct with `new(fs)`, `process(&mut Block, &Snapshot)`, `reset`; add
   `pub mod` in `dsp/mod.rs`.
2. `dsp/params.rs`: `P` variants (`XOn` plus controls), `PARAMS` entries (stable `key`
   strings: they are what presets and config store), bump `COUNT`.
3. `dsp/chain.rs`: a `Section` variant, a field in `VoiceChain`, `new`, `reset`, and a
   `section!(...)` line in `run` at its place in the order; update the `//!` order line.
4. `config/presets.rs`: use it in built-ins if wanted (`make_builtin`).
5. `ui/effects.rs`: a `Section` in `SECTIONS` (icon, name, toggle, controls) and `tip` texts.
6. Latency: if it delays audio, `PitchStage::latency` / `Shared::chain_latency_ms` pattern.
7. Tests: in the effect file (click-free, reset), `chain.rs` (fades, bypass).

**A new setting**
1. The field on `Config` (or the sub-struct it belongs to: `FeedbackSettings`,
   `BoardSettings`, `MelodySettings`, `DeviceSettings`, `MicSettings`). All have
   `#[serde(default)]`; give the type a sensible `Default` so old files load. Rename with
   `#[serde(alias = "old")]`.
2. If the audio side needs it: an atomic in `audio::shared::Shared` and a line in
   `ui::apply_to_shared` (or `apply_feedback`).
3. UI: `ui/settings.rs` (or the page it belongs to).
4. Test in `config/mod.rs`: round trip plus "an older file without it loads" (see
   `the_accent_is_saved_as_hex_and_older_files_get_teal`).

**A soundboard feature**
- Saved per clip: `SoundEntry` in `soundboard/library.rs` (serde default). Board-wide:
  `BoardSettings` in `soundboard/organize.rs`.
- Logic: `Board` methods (`library.rs`); pure bookkeeping goes in `organize.rs`,
  `playmode.rs` or `shape.rs` where it can be tested without UI.
- Real-time: `SoundCmd` plus `SoundMixer::apply` (`mixer.rs`); never decode there.
- UI: a variant in `ui/soundboard.rs` `Action`, drawn in `pad`/`clip_menu`/`edit_panel`,
  handled in `VoxApp::apply_sound_action` (`ui/mod.rs`).
- Tests: `library.rs`/`organize.rs` units, then a `ui/mod.rs` test that drives the
  `soundboard::Action`.

**A new page / nav entry**
1. `config::Page` (variant, `ALL`, `label`; old names need a serde alias).
2. `ui/nav.rs` (`icon`, `about`).
3. `ui/<page>.rs` and a `pub mod` in `ui/mod.rs`; a `VoxApp::<page>_page` method and its arm
   in `VoxApp::draw`.
4. `ui/shortcuts.rs`: Ctrl+digit follows `Page::ALL` (check its tests).
5. Test: `the_nav_rail_opens_every_page_and_the_last_one_is_remembered` in `ui/mod.rs`.

**A new screenshot test**: in `ui/snapshot.rs` `mod tests`, mark it
`#[ignore = "writes pictures to VOXSHIFT_SHOTS; run with --ignored"]`, start with
`let Some(dir) = out_dir() else { return };`, and use `app_shot(&dir, name, (w, h), theme,
zoom, |app, ctx| { ... })` (or `Shot` directly for a widget).

## Test map

- Pure logic and DSP: next to the code in each file (`mod tests`). Most effect tests feed
  synthetic signals and check for clicks and fade times.
- Engine offline: `audio/engine.rs` tests (`rig`), `dsp/chain.rs`, `dsp/melody.rs`
  (synthetic speakers singing a tune).
- Hotkeys: `keymap.rs` (the `Board` test helper: three clips airhorn/bruh/wow, `read()` for
  the iterator form, `slots()` for `SoundKey`s; `voice()` gives the voice keys),
  `voice_keys.rs` (`rig` builds `VoiceKeys` with counters), `soundboard/hotkeys.rs`
  (listener tests: `HOOK_TESTS` mutex, `tap_f24`/`f24` inject F24, settle ~200 ms).
- UI: `ui/mod.rs` (62 tests). `test_app()` gives a `VoxApp` with no devices, listener or
  config file; `frame(&ctx, &mut app, (w, h), time, events)` runs one real headless egui
  frame; `frame_with` adds modifiers; `click` clicks a position; `wait_for` runs frames until
  a condition (listener tests); widget positions come from `widgets::remember` /
  `widgets::rect_of`. Page modules have small draw tests of their own.
- Config compatibility: `config/mod.rs` (`round_trips_everything`, migrations, broken files).
- Screenshots: `ui/snapshot.rs`, ignored unless `VOXSHIFT_SHOTS` is set.
- Needs a real desktop (CI skips): tests whose names contain `listener`,
  `presses_are_counted`, `assigning_a_hotkey`, `duplicate_hotkeys`, `tray_starts`.

## Config

- File: `%APPDATA%\VoxShift\config.json` (`config::config_path`). Next to it:
  `config.broken.json` (a file that failed to parse, kept aside), `hotkeys-debug.log`
  (`soundboard::hotkeys::diag`, only while assigning a key), melody files imported from packs
  (`VoxApp::melody_dir`).
- Load: `config::load_from`: no file → `Config::fresh()` (wizard not done); unreadable or
  unparsable → defaults plus a warning toast. Then `with_manager` cleans up:
  `dedupe_hotkeys` (via `keymap::dedupe`, reports what it cleared) and
  `BoardSettings::repair`.
- Backward compatibility: `#[serde(default)]` on `Config` and its sub-structs, so missing
  fields take defaults; `#[serde(alias)]` for renamed fields and variants (`tab` → `page`,
  `Effects` → `Voice`); effect values are saved by parameter key (`EffectSettings`), so new
  parameters never break old presets; `Hotkey.second` is skipped when empty, so files without
  second keys look exactly as before. Older formats are covered by tests in
  `config/mod.rs` and `ui/mod.rs`
  (`an_old_config_migrates_clips_keep_their_paths_and_start_unsorted`).
- Save: `VoxApp::autosave` (1 s after the last change), `save_now` on exit and on Windows
  session end (from `mirror`), atomic via a `.json.tmp` file and a rename.
