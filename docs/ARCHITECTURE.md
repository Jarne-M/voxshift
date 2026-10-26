# VoxShift architecture

Notes for developers. For using VoxShift, see the [README](../README.md).

## Signal flow

```
 mic callback(s) ──ring──▶ ┌─────────────── output callback (CABLE Input) ───────────────┐
  (downmix, level)         │ adaptive resample ─▶ mic crossfade ─▶ voice chain ─▶ + sounds │──▶ CABLE Input
                           │  (drift control)       (50 ms)          (effects)     (voices) │
 device manager thread ─cmd─▶ (attach/detach mics, monitor; play sounds)                    │──ring──▶ monitor
  (scan 1.5 s, errors,     └────────────────────────────────────────────────────────────┘
   stall watchdog, mic choice)          ◀──garbage── (old buffers freed off the audio thread)
```

The output device's callback (`audio::engine`) drives everything. Each 64-sample block it
reads both mic rings, crossfades between the mics, runs the voice chain, mixes the soundboard
clips, writes the virtual-mic output and fills the monitor ring (voice, clips, switch beeps and
the melody preview).

## Real-time rules

The audio callbacks never lock, allocate, log or do I/O.

- **Parameters** are atomics (`dsp::params`), read once per block. Other switches (voice on,
  mute, monitor, safe mode, ...) are atomics in `audio::shared::Shared`.
- **New objects** (mic readers, the monitor ring, decoded clips) arrive through a lock-free
  `rtrb` queue (`EngineCmd`).
- **Freeing**: anything the audio thread lets go of goes back through a second queue (the
  "garbage" queue) and is dropped on the device manager thread.
- **The melody** reaches the audio thread through a seqlock over fixed atomic arrays
  (`melody::store`): no locks, no allocation, and it survives the engine being rebuilt.
- **Beeps and requests** from other threads (cues, "stop all sounds", Melody Voice restart and
  next note) are atomic counters or flags that the engine compares with what it saw last.

## Threads

- **Audio**: the output callback described above, plus one callback per mic that only
  downmixes and pushes into a ring.
- **Device manager**: the only thread that talks to cpal. It rescans devices every 1.5 s,
  restarts failed streams, runs a stall watchdog, decides which mic is active
  (`audio::mic_select`, pure and unit-tested) and frees the garbage. Mics and the monitor
  attach to the running engine, so the virtual-mic stream keeps running when they come and go.
  It restarts only when the output device or buffer size changes.
- **Hotkeys** (`soundboard::hotkeys`): a hidden window receiving Raw Input keyboard events.
  It only listens, so it can't block keys and adds no input delay. A `KeyTracker` turns raw
  events into presses and releases (dropping auto-repeat). Voice-changer keys
  (`voice_keys`) are checked first, then soundboard clips. Every 50 ms it also checks with
  `GetAsyncKeyState` whether keys that hold a clip are still down, so a lost key-up can't leave
  a clip playing.
- **Tray** (`shell::tray`): its own thread with a Win32 message loop, so it keeps working
  whatever the window does.
- **Window** (egui/eframe): only a view. While it's hidden, eframe runs only `App::logic`,
  which handles tray requests, close requests and saving.

## Clock drift and sample rates

Each mic has its own clock. `audio::adaptive` resamples with rubato's async resampler and
nudges the ratio (±1 %, a proportional controller) so each ring stays at its target fill. That
also covers 44.1 kHz mics on a 48 kHz cable.

## Latency

The estimate in the UI is the sum of the mic buffer, the ring buffer between mic and output,
one 64-sample block, the pitch shifter (about 15 ms, only while Pitch is on), Melody Voice
(only while it's on; it depends on the analysis window) and the output buffer. It doesn't
include VB-CABLE's own buffer or the receiving app.

Most of it is fixed by Windows: in shared mode, the mic and the cable typically each run 10 ms
buffers, and the ring buffer must absorb one of them. The *Buffer* setting requests smaller
device buffers, but many shared-mode drivers ignore requests below their period; the UI shows
the actual buffer each device runs at.

## DSP

The effects are hand-written, one file each in `src/dsp/`, and each file explains its idea at
the top: RBJ biquads for the EQ, a Hann-windowed two-head pitch shifter with
correlation-aligned splices, LPC (Levinson-Durbin + lattice filters) for formant preservation,
YIN pitch detection, Freeverb for the reverb, and so on. Every section of the chain fades in
and out instead of switching, and is skipped entirely while it's off.

Switching the voice changer off fades the chain's *input* over 25 ms while the clean mic fades
in, so echo and reverb tails ring out naturally. Clean mode keeps the input gain, so the level
doesn't jump.

**Melody Voice**: a YIN detector runs on a 12 kHz copy of the mic every 2.5 ms; a tracker
smooths it (median, octave-jump guard, voiced/unvoiced hysteresis); the conductor
(`dsp::conductor`) turns syllables, the clock or key presses into a target note; the shift
(target − detected pitch) drives the formant-preserving pitch stage; unvoiced sounds take a dry
path delayed by the same amount. The pitch analysis and the shifter overlap, so the longer of
the two sets the added delay.

**Soundboard mixer** (`soundboard::mixer`): up to 16 clips at once, a 4 ms fade-in and a 12 ms
fade-out on every stop, so clips never click. Clips are mixed after the voice chain, so effects
and Melody Voice never touch them.

**Safe mode** (the panic button): one atomic flag. On its rising edge the engine stops all
clips and the melody preview; while it's set, the chain runs as if the voice changer were off,
new clips are refused, and optionally the monitor gains ramp to zero. No setting is changed,
which is why Restore returns exactly to the previous state.

## Module layout

```
src/
  main.rs       start-up, command-line options, single instance
  audio/        streams, engine, device manager, drift-compensating reader, mic selection,
                device name matching
  dsp/          one file per effect + params, smoothing, the chain; Melody Voice:
                pitch_detect (YIN), conductor (which note when), melody (the effect),
                preview (the "Play melody" synth), cue (switch beeps)
  melody/       note type, built-in tunes, MIDI import (midly), lock-free hand-off to the engine
  soundboard/   decoding (symphonia + rubato), sound list + loader, real-time mixer, global
                hotkeys, folders/search/sorting (organize), play modes and held keys (playmode)
  keymap.rs     who owns which hotkey: conflicts, reserved keys, the Keep/Change logic
  voice_keys.rs the voice changer's hotkeys and the shared switch/beep logic
  ui/           egui panels: devices, effects, Melody Voice, presets, soundboard, hotkeys,
                meters, toasts, tray status
  shell/        tray icon thread, icon drawing, single instance, start with Windows (Win32)
  config/       settings file, presets, .voxpack import/export
```

## Adding a neural voice model

Implement `dsp::VoiceProcessor`: `process` one mono block in place and report
`latency_samples`. The engine already calls the chain through that interface. Run the model
inference on a worker thread with lock-free queues on both sides, because a model can't promise
to finish within one audio block.
