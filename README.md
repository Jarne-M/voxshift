# VoxShift

A local, real-time voice changer and soundboard for Windows, written in Rust. Your mic goes in,
the changed voice comes out of a **virtual audio cable** (VB-CABLE), and Discord, OBS or a game
uses that cable as its microphone. Soundboard clips are mixed into the same virtual mic.

Everything runs on your PC. There are no accounts, no cloud and no telemetry, and no driver of
its own is installed.

## What it does

- **Two mics with automatic fallback.** It uses the *primary* mic (e.g. your HyperX headset)
  when it works, and the *fallback* mic (e.g. a desk mic) when the primary is unplugged,
  missing or silent. It switches back automatically (optional), with a 50 ms crossfade, and the
  output stream never restarts. Devices are matched by name, so changing USB ports, reboots or
  moving between houses don't break the setup.
- **Effects.** Pitch (±12 semitones, optional formant preservation), noise gate, tone (low and
  high cut plus a 3-band EQ), distortion, robot (ring modulator), chorus, echo, reverb, input and
  output gain, and master dry/wet. Every change is click-free.
- **Presets.** Built-in: Clean, Deep Voice, Chipmunk, Robot, Cave, Radio, Telephone and Demon.
  You can also save your own.
- **Soundboard.** Add wav, mp3, ogg or flac files with the button or by drag and drop. Each sound
  has its own volume and an optional **global hotkey** that works while a game has focus. You can
  also play sounds in your headphones.
- **Monitor.** Hear yourself, the soundboard or both on your headphones, with its own volume.
- **Meters, clip lamp, estimated latency,** and a big **Voice changer ON/OFF** toggle (OFF =
  clean passthrough).
- **Everything is remembered:** devices, mic settings, sliders, presets, sounds and hotkeys.

## 1. Install VB-CABLE (once)

1. Download the free **VB-CABLE Driver Pack** from <https://vb-audio.com/Cable/>.
2. Unzip it, right-click `VBCABLE_Setup_x64.exe` and choose **Run as administrator**, then
   **Install Driver**.
3. **Reboot.**
4. You now have two new devices:
   - **CABLE Input**, a *playback* device. VoxShift plays into this.
   - **CABLE Output**, a *recording* device. Other apps use this as their mic.

If VoxShift can't find CABLE Input, it shows a yellow banner with a download link. You can
still pick any output device by hand, for example your speakers for a quick test.

**Recommended:** in `mmsys.cpl`, set both CABLE Input (Playback tab) and CABLE Output
(Recording tab) to **48000 Hz** (Properties → Advanced). Don't make CABLE Input your default
playback device, or every Windows sound would end up in your mic.

## 2. Build and run

Requires Rust (stable) on Windows 10 or 11.

```powershell
# With the portable toolchain next to this folder (..\.toolchain):
. .\env.ps1
cargo run --release
# Or with a normal rustup install: just `cargo run --release`.
```

The release binary is `target\release\voxshift.exe`. It's a single file and needs no installer.

On first start VoxShift selects **CABLE Input** as output and your Windows default mic as the
primary mic.

## 3. Use the virtual mic in other apps

Select **CABLE Output (VB-Audio Virtual Cable)** as the microphone:

| App | Where |
|---|---|
| **Discord** | User Settings → Voice & Video → *Input Device* → **CABLE Output**. Also turn **off** Noise Suppression (Krisp), Echo Cancellation and Automatic Gain Control, otherwise Discord "cleans up" your effects and filters out soundboard music. Set Input Sensitivity manually if needed. |
| **OBS** | Settings → Audio → *Mic/Auxiliary Audio* → **CABLE Output**, or add an *Audio Input Capture* source with CABLE Output. Remove or mute any source that captures your real mic directly, or you'll be heard twice. |
| **Games** | In-game voice settings → input device → **CABLE Output**. If the game has no setting, it uses the Windows default: Settings → System → Sound → Input → choose **CABLE Output** (or `mmsys.cpl` → Recording → CABLE Output → *Set Default*). |
| **Check it** | `mmsys.cpl` → Recording → CABLE Output: the level bar moves when you talk. Or record yourself in Windows *Sound Recorder* with CABLE Output selected. |

## 4. Two mics, two houses

Pick both in the left panel:

- **Primary mic**: e.g. *Microphone (HyperX Cloud II Wireless)*
- **Fallback mic**: e.g. *Microphone (USB PnP Audio Device)*

With **Auto** mode:

| Situation | What VoxShift does |
|---|---|
| Primary present | uses the primary |
| Primary unplugged, missing or erroring | crossfades to the fallback within about 1.5 s and shows a notification |
| Primary listed but silent (headset boom mic pulled out) | after *N* seconds below the threshold (default 5 s, −85 dB), uses the fallback |
| Primary back (or loud again) | switches back, if *Switch back automatically* is on |
| Neither available | sends silence, shows a red warning and keeps retrying |

You can also force a mic with *Force primary* or *Force fallback*. The top bar always shows the
active mic and why, for example *"Active: Fallback mic (primary mic is silent)"*.

**Calibrating silence detection.** Each mic shows its level in dB. With the primary plugged
in and the room quiet, note the value (typically −60 to −75 dB). Pull the boom mic and note it
again (usually −100 dB or lower). Put the threshold in between; the default of −85 dB works for
most mics.

Some mics and drivers apply their own noise gate or noise suppression (NGenuity, Realtek or
Windows "audio enhancements"), so they deliver true silence whenever you're quiet. With such a
mic, silence detection can switch to the fallback during long pauses. It switches back within
0.3 s once you speak, but the first syllable comes from the fallback. If that bothers you,
turn off the driver's noise suppression, raise the seconds, or turn silence detection off.

## 5. Soundboard and hotkeys

- **Add sounds…** or drag files onto the window. Files are decoded in the background and
  resampled to the output rate. Clips longer than 3 minutes are cut.
- **Click** plays a sound and clicking again restarts it. **Right-click** stops it, and
  **Stop all** stops everything.
- **Set key**, then press a key combination anywhere (Esc cancels). Hotkeys are **global** and
  **pass-through**: the sound plays *and* the game still receives the key. A combination such as
  Ctrl+Numpad 1 avoids accidental in-game actions. If two sounds share a key, both play.
- **Also play in my headphones** routes sounds to the monitor. The monitor must be on.

Hotkeys use a standard Windows low-level keyboard hook (the same mechanism as Discord's
push-to-talk). The hook only reads key presses and never blocks or changes them.

## 6. Latency

The estimate in the UI is the sum of:
- the mic buffer
- the ring buffer between mic and cable
- one 64-sample processing block
- the pitch shifter (about 15 ms, only while pitch is on)
- the output buffer

It doesn't include VB-CABLE's own buffer or the receiving app.

Measured on the development PC (Windows shared mode, 10 ms device buffers): **about 38–40 ms**
without pitch. That's above the 30 ms goal, and most of it is fixed by Windows: the mic and the
cable each run 10 ms buffers, and the ring buffer must absorb one of them. The *Buffer* setting
requests smaller device buffers, but most Windows shared-mode drivers ignore requests below
their 10 ms period; the UI shows the actual buffer each device runs at. VB-CABLE's own control
panel (installed with the driver) can lower its internal buffer.

## 7. Settings file

`%APPDATA%\VoxShift\config.json`, saved about 1 s after a change and on exit. Devices are
stored **by name**. If the file is ever damaged, it's moved aside as `config.broken.json` and
VoxShift starts with defaults.

`voxshift.exe --config D:\path\other.json` uses a different settings file, for example a
second profile, or trying things without touching your normal settings.

## 8. Testing on your hardware

These were checked automatically: `cargo test` (80 tests) and the self-tests below on the
development PC (USB PnP mic, webcam mic, HyperX Cloud II Wireless). The rest needs your setup.

**Headless self-tests:**

```powershell
.\target\release\voxshift.exe --list-devices     # what Windows exposes; shows whether VB-CABLE is found
.\target\release\voxshift.exe --selftest         # opens default mic + output, sends silence, prints status for 8 s
.\target\release\voxshift.exe --selftest switch  # first two mics as primary/fallback at -48 dB, simulates unplug at 4 s and replug at 8 s
```

Expected for `--selftest switch`: at 5 s `active=Fallback 'primary not available'`, at 9 s
`active=Primary`, and `out=Running` on every line.

**Manual checklist (needs VB-CABLE, the headset and the desk mic):**

1. **Passthrough.** Start VoxShift with Voice changer OFF. In `mmsys.cpl` → Recording, CABLE
   Output's level bar moves when you talk. Record a few seconds in Sound Recorder with CABLE
   Output: your clean voice.
2. **Monitor.** Pick your headphones, turn the monitor on and talk: you hear yourself slightly
   late (the monitor adds its own buffer on top of the virtual-mic latency). Use headphones,
   not speakers, to avoid feedback.
3. **Effects.** Turn the voice changer ON and try each preset while monitoring. Drag sliders
   while talking: there should be no clicks. ON/OFF should fade, not click.
4. **Unplug the primary.** Set the headset as primary and the desk mic as fallback. While talking,
   pull the headset's USB dongle (or cable). Within about 1.5 s: a notification, the top bar says
   *Fallback mic (primary not available)*, and Discord keeps hearing you. Plug it back: it
   switches back (with switch-back on). The output must never drop out; check with Discord's
   input meter or a recording.
5. **Pull the boom mic** with the headset still connected. After about 5 s: *Fallback mic
   (primary mic is silent)*. Plug the boom back in and talk: back on the primary. If it never
   triggers, look at the primary's dB value with the boom pulled and adjust the threshold.
6. **Neither mic.** Unplug both: a red warning and silence on CABLE Output; the app stays
   responsive. Plug one in: it is picked up within about 2 s.
7. **Second house.** On the other PC, or with renamed devices, the same names are found again
   (e.g. *Microphone (2- HyperX …)* still matches *Microphone (HyperX …)*).
8. **Soundboard.** Drop an mp3 on the window, click it: Discord hears it mixed with your voice.
   Assign Ctrl+Numpad1, focus a game or another window, press it: it plays, and the game still
   gets the key.
9. **Persistence.** Close and reopen: devices, sliders, presets, sounds and hotkeys are back.

## 9. How it works

```
 mic callback(s) ──ring──▶ ┌─────────────── output callback (CABLE Input) ───────────────┐
  (downmix, level)         │ adaptive resample ─▶ mic crossfade ─▶ voice chain ─▶ + sounds │──▶ CABLE Input
                           │  (drift control)       (50 ms)          (effects)     (voices) │
 device manager thread ─cmd─▶ (attach/detach mics, monitor; play sounds)                    │──ring──▶ monitor
  (scan 1.5 s, errors,     └────────────────────────────────────────────────────────────┘
   stall watchdog, mic choice)          ◀──garbage── (old buffers freed off the audio thread)
```

- **Real-time safety.** The audio callbacks never lock, allocate, log or do I/O. Parameters are
  atomics (`dsp::params`, read once per 64-sample block). New mic readers, the monitor ring and
  sound buffers arrive through a lock-free `rtrb` queue. Anything the audio thread lets go of is
  sent back through a second queue and freed on the device manager thread.
- **Clock drift and sample rates.** Each mic has its own clock. `audio::adaptive` resamples with
  rubato's async resampler and nudges the ratio (±1 %, a proportional controller) so the ring
  buffer stays at its target fill. That also covers 44.1 kHz mics on a 48 kHz cable.
- **Hot-plug.** Only the device manager thread talks to cpal. Mics and the monitor attach to the
  running engine, so the virtual-mic stream keeps running when they come and go. It restarts
  only when you change the output device or buffer size.

```
src/
  audio/      streams, engine, device manager, drift-compensating reader, mic selection, name matching
  dsp/        one file per effect + params, smoothing, chain (all unit-tested)
  soundboard/ decoding (symphonia + rubato), sound list + loader, RT mixer, global hotkeys
  ui/         egui panels: devices, effects, presets, soundboard, meters, toasts
  config/     settings file + presets
```

The DSP is hand-written: RBJ biquads, Hann-windowed two-head pitch shifter with
correlation-aligned splices, LPC (Levinson-Durbin + lattice filters) for formant preservation,
Freeverb, etc. Each file explains its idea at the top.

**Adding a neural voice model later:** implement `dsp::VoiceProcessor` (`process` one mono
block in place and report `latency_samples`). The engine already calls the chain through that
interface. Run the model inference on a worker thread with lock-free queues on both sides,
because a model can't promise to finish within one audio block.

## Troubleshooting

- **Nobody hears me.** The app uses *CABLE Output* as its mic, not CABLE Input. Check that the
  top bar shows an active mic and that the output panel says *running*.
- **Robotic or choppy audio in Discord.** Turn off Discord's noise suppression and echo
  cancellation.
- **My own voice is doubled or echoes.** You're monitoring through speakers, or another app also
  records your real mic.
- **Hotkeys do nothing in one specific game.** Some games running as administrator block input
  hooks from non-admin apps. Run VoxShift as administrator in that case.
