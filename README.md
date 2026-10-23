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
- **Effects.** Pitch (Â±12 semitones, optional formant preservation), noise gate, tone (low and
  high cut plus a 3-band EQ), distortion, robot (ring modulator), chorus, echo, reverb, input and
  output gain, and master dry/wet. Every change is click-free.
- **Melody Voice.** Talk, and it comes out sung: your voice is retuned note by note to a
  melody (one syllable per note, in time with the song, or a note per hotkey press), or snapped
  to a key like auto-tune. Built-in public-domain tunes, or import your own MIDI files. See
  [section 5c](#5c-melody-voice).
- **Presets.** Built-in: Clean, Deep Voice, Chipmunk, Robot, Cave, Radio, Telephone, Demon,
  Sing the Song (per-syllable) and Hard Tune. You can also save your own.
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
(Recording tab) to **48000 Hz** (Properties â†’ Advanced). Don't make CABLE Input your default
playback device, or every Windows sound would end up in your mic.

## 2. Build and run

Requires Rust (stable) on Windows 10 or 11.

With the portable toolchain next to this folder (`..\.toolchain`), the easiest way is to
**double-click `build.cmd`**. It sets up the toolchain and builds the release exe; quit
VoxShift first. In PowerShell you can also dot-source `env.ps1`, which needs scripts to be
allowed (`Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`):

```powershell
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
| **Discord** | User Settings â†’ Voice & Video â†’ *Input Device* â†’ **CABLE Output**. Also turn **off** Noise Suppression (Krisp), Echo Cancellation and Automatic Gain Control, otherwise Discord "cleans up" your effects and filters out soundboard music. Set Input Sensitivity manually if needed. |
| **OBS** | Settings â†’ Audio â†’ *Mic/Auxiliary Audio* â†’ **CABLE Output**, or add an *Audio Input Capture* source with CABLE Output. Remove or mute any source that captures your real mic directly, or you'll be heard twice. |
| **Games** | In-game voice settings â†’ input device â†’ **CABLE Output**. If the game has no setting, it uses the Windows default: Settings â†’ System â†’ Sound â†’ Input â†’ choose **CABLE Output** (or `mmsys.cpl` â†’ Recording â†’ CABLE Output â†’ *Set Default*). |
| **Check it** | `mmsys.cpl` â†’ Recording â†’ CABLE Output: the level bar moves when you talk. Or record yourself in Windows *Sound Recorder* with CABLE Output selected. |

## 4. Two mics, two houses

Pick both in the left panel:

- **Primary mic**: e.g. *Microphone (HyperX Cloud II Wireless)*
- **Fallback mic**: e.g. *Microphone (USB PnP Audio Device)*

With **Auto** mode:

| Situation | What VoxShift does |
|---|---|
| Primary present | uses the primary |
| Primary unplugged, missing or erroring | crossfades to the fallback within about 1.5 s and shows a notification |
| Primary listed but silent (headset boom mic pulled out) | after *N* seconds below the threshold (default 5 s, âˆ’85 dB), uses the fallback |
| Primary back (or loud again) | switches back, if *Switch back automatically* is on |
| Neither available | sends silence, shows a red warning and keeps retrying |

You can also force a mic with *Force primary* or *Force fallback*. The top bar always shows the
active mic and why, for example *"Active: Fallback mic (primary mic is silent)"*.

**Calibrating silence detection.** Each mic shows its level in dB. With the primary plugged
in and the room quiet, note the value (typically âˆ’60 to âˆ’75 dB). Pull the boom mic and note it
again (usually âˆ’100 dB or lower). Put the threshold in between; the default of âˆ’85 dB works for
most mics.

Some mics and drivers apply their own noise gate or noise suppression (NGenuity, Realtek or
Windows "audio enhancements"), so they deliver true silence whenever you're quiet. With such a
mic, silence detection can switch to the fallback during long pauses. It switches back within
0.3 s once you speak, but the first syllable comes from the fallback. If that bothers you,
turn off the driver's noise suppression, raise the seconds, or turn silence detection off.

## 5. Soundboard and hotkeys

- **Add soundsâ€¦** or drag files onto the window. Files are decoded in the background and
  resampled to the output rate. Clips longer than 3 minutes are cut.
- **Click** plays a sound and clicking again restarts it. **Right-click** stops it, and
  **Stop all** stops everything.
- **Set key**, then press a key combination anywhere (Esc cancels). Hotkeys are **global** and
  **pass-through**: the sound plays *and* the game still receives the key. A combination such as
  Ctrl+Numpad 1 avoids accidental in-game actions. If two sounds share a key, both play.
- **Also play in my headphones** routes sounds to the monitor. The monitor must be on.

Hotkeys use Windows' Raw Input API: VoxShift is told about every key press, even in the
background, but only listens. It can't block or change keys and adds no delay to your typing.
(An earlier version used a low-level keyboard hook, which Windows can silently switch off;
raw input doesn't have that problem.) If you press a key while VoxShift waits for a hotkey and
it hears nothing, it tells you instead of waiting forever.

## 5a. Voice changer hotkeys

Under **Voice changer hotkeys** in the left panel:

- **Effects on/off:** click *Set hotkey*, then press a key or combination (Ctrl/Alt/Shift +
  key). It works in the background, in the tray and in games, and the game still gets the key.
  Holding the key doesn't repeat. The mode can be:
  - **Toggle:** each press switches.
  - **Hold for effects (push-to-talk):** clean, with effects only while held.
  - **Hold for clean (push-to-clean):** effects, clean only while held.
- **Mute / unmute mic** has its own hotkey.
- **Jump to a preset:** add a row, pick a preset, give it a key. Pressing it loads that preset
  and turns effects on.
- **Duplicates are refused:** a key already used by a clip, the toggle, mute or a preset key
  shows a message instead.
- **Switching never changes your sliders or preset.** Switching back gives exactly what was
  active before. The switch is a 25 ms crossfade, and an echo or reverb that's still sounding
  rings out naturally instead of being cut off. Clean mode keeps your input gain (mic
  calibration), so the volume doesn't jump.
- **Feedback:** a short beep in your headphones (rising = ON, falling = OFF). It plays on the
  monitor output only and never goes into your mic. You can set its volume or turn it off. The
  monitor must be on; untick "my voice" there to hear only the beeps. Optionally a
  "Voice changer: ON/OFF" banner shows at the top of the screen for a second (it doesn't show
  over games in exclusive fullscreen). The big button, the tray icon and its tooltip always
  show the current state.
- **At start:** remember the last state (default), always effects ON, or always OFF.

## 5b. Closing, the tray icon and startup

Your virtual mic only exists while VoxShift runs, so closing the window never quietly kills it:

- **The X (or Alt+F4)** asks: **Minimize to tray** (the default; Enter picks it), **Quit
  completely**, or **Cancel**. Tick *Remember my choice* to stop asking. You can change or reset
  it later under **Window & startup** in the left panel: *Ask every time*, *Always minimize to
  tray* or *Always quit*.
- **Hidden in the tray**, everything keeps running: audio, mic fallback, soundboard and hotkeys.
  The first time per run, a notification says so. Mic switches while hidden also show as a
  notification.
- **Tray icon:** left-click or double-click shows the window. Right-click opens a menu with:
  - Show VoxShift
  - Voice changer ON/OFF
  - Mute mic
  - the active mic (read-only)
  - Stop all sounds
  - Quit VoxShift (always quits, no question)

  The tooltip shows the status, and the icon colour shows problems at a glance:

  | Colour | Meaning |
  |---|---|
  | violet | all good |
  | grey | voice changer off |
  | grey with a red slash | mic muted |
  | orange | no microphone available |
  | red | virtual mic output missing |
- **Only one copy runs.** Starting VoxShift again (e.g. from the desktop shortcut) brings the
  running one to the front, even from the tray, instead of starting a second copy that would
  fight over the cable.
- **Start with Windows** adds VoxShift to your user's startup programs. That's the
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` registry key, so no admin rights are
  needed. **Start minimized to tray** starts it straight in the tray. Both are off by default.
- **Windows shutdown or logoff** saves your settings and exits without showing the dialog.
- **Mute mic** (top bar or tray) fades your mic out; soundboard sounds still play.

## 5c. Melody Voice

Melody Voice sits at the top of the **Effects** tab. Turn it **ON**, pick a melody and talk:
each syllable comes out on the next note of the song. Consonants and breaths (s, t, f, h)
pass through untouched, so you stay understandable.

**Modes**

| Mode | What moves you to the next note |
|---|---|
| **Per syllable** (default) | Every new syllable: after a short pause, a dip in loudness ("hel-lo") or a jump in pitch. *Sensitivity* sets how readily a syllable is heard; *Min note length* stops glitches from skipping notes. |
| **Tempo-synced** | The song plays at the *Tempo* slider's speed. It starts the moment you start singing; notes you are silent for pass by, and a rest keeps the note before it. |
| **Manual step** | Only the *Next note* button or hotkey. |
| **Scale snap (auto-tune)** | No melody: your pitch is pulled to the nearest note of the chosen key and scale (major, minor, major/minor pentatonic, chromatic). |

**Controls**
- **Retune speed:** 0 ms is the instant, robotic "hard tune". 150 ms and up keeps your own
  vibrato and slides (natural). Default 5 ms.
- **Glide:** how fast it slides from note to note while you keep sounding.
- **Transpose:** Â±24 semitones. **Octave-fold** moves the whole melody by octaves so it sits
  around your voice, so a deep voice doesn't have to squeak out a song written for a high
  one. It only changes octave between notes, never in the middle of one.
- **Preserve formants** (on by default) keeps your voice's character when the melody goes high
  or low (no chipmunk). **Formant shift** (Â±6 st) moves the character on purpose: down is
  darker or bigger, up is brighter or smaller.
- **Loop**, **Restart**, **Next note**, and **Back to start after silence** (seconds; 0 = never).
- **Analysis window** (20â€“60 ms) trades latency for steadiness and how low a voice it can
  hear. **Lowest / highest voice pitch** (default 70â€“500 Hz) covers a low male to a high
  female voice; narrow it to yours if it ever jumps an octave.
- The line **"You: A3 Â» Target: C4"** shows what it hears and where it is taking you. The
  piano roll shows the melody, the current note (lit) and where it is (white line).
- **Play melody** plays the tune as a soft synth tone at the tempo slider's speed, **in your
  headphones only** (the monitor must be on). It never goes into your mic.

**Melodies**
- Built in, all public domain and typed in by hand (no audio or song files are bundled): Ode
  to Joy, Twinkle Twinkle Little Star, Happy Birthday, Greensleeves, FÃ¼r Elise (opening), FrÃ¨re
  Jacques, plus a major and a minor scale, a pentatonic run and a major arpeggio.
- **Your own songs:** *Import MIDIâ€¦* or drop a `.mid` file on the window. VoxShift lists the
  file's parts (each track and channel; drums are skipped) and picks the one most likely to
  be the tune: the highest part that plays one note at a time (marked â˜… under **Part**). If it
  guessed wrong, pick another part. Chords are reduced to their top note. The file's tempo
  sets the tempo slider; later tempo changes in the file are ignored. Imported files are
  remembered in the melody list. Bring your own MIDI files; VoxShift doesn't download songs.

**Presets and hotkeys.** A preset you save with Melody Voice on remembers its melody too. The
built-in *Sing the Song (per-syllable)* and *Hard Tune* presets keep whatever melody you picked.
Under **Voice changer hotkeys â†’ Melody Voice** you can set global keys for **on/off**,
**restart melody** and **next note**. They work in games and while VoxShift is in the tray.

**Sharing with friends.** **Shareâ€¦** (next to the preset list) saves all your own presets plus
your imported MIDI files in one `.voxpack` file. Your friend clicks **Importâ€¦** or drops the
file on the VoxShift window: the presets appear in their preset list, and the MIDI files go to
`%APPDATA%\VoxShift\melodies` and into their melody list, with the presets pointing at them.
Their own presets are kept. A name they already use gets "(2)", and importing the same pack
twice adds nothing. Soundboard sounds aren't included; send those files separately.

**Soundboard clips are never retuned:** they're mixed in after the voice effects.

**Latency.** Melody Voice adds more delay than any other effect, and the panel shows how much.
The pitch analysis and the pitch shifter overlap (whichever is longer sets the delay), so it
adds:

| Analysis window | Added delay (measured, 48 kHz) |
|---|---|
| 20 ms | 15.0 ms |
| **30 ms (default)** | **16.2 ms** |
| 45 ms | 23.8 ms |
| 60 ms | 31.2 ms |

That comes on top of the ~38â€“40 ms base latency (section 6), and on top of the Pitch effect's
~15 ms if you use both. It's measured by an automated test that times a sound through the
effect.

**Limits.** It's a fast, time-domain shifter: big jumps (more than an octave) sound grainier
than small ones, and very breathy or creaky speech may be left untuned for a moment. A
hummed, almost pure tone gets a little quieter with formants preserved (a safety limiter, so
it can never blow up).

## 6. Latency

The estimate in the UI is the sum of:
- the mic buffer
- the ring buffer between mic and cable
- one 64-sample processing block
- the pitch shifter (about 15 ms, only while pitch is on)
- Melody Voice (about 16 ms at its default analysis window, only while it is on; see 5c)
- the output buffer

It doesn't include VB-CABLE's own buffer or the receiving app.

Measured on the development PC (Windows shared mode, 10 ms device buffers): **about 38â€“40 ms**
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

These were checked automatically: `cargo test` (190 tests; they briefly show a tray icon) and the self-tests below on the
development PC (USB PnP mic, webcam mic, HyperX Cloud II Wireless). The rest needs your setup.

Melody Voice's tests feed synthetic voices through the real effect chain offline: a low and a
high "speaker" saying 12 syllables come out singing the first 12 notes of Twinkle Twinkle
(each within 0.25 semitones, in practice within 0.1), a held vowel follows the tune in
tempo mode, and the pitch detector is checked on sines and sawtooths from 75 to 490 Hz (within
1 %), on noise, whispers and silence.

**Headless self-tests:**

```powershell
.\target\release\voxshift.exe --list-devices     # what Windows exposes; shows whether VB-CABLE is found
.\target\release\voxshift.exe --selftest         # opens default mic + output, sends silence, prints status for 8 s
.\target\release\voxshift.exe --selftest switch  # first two mics as primary/fallback at -48 dB, simulates unplug at 4 s and replug at 8 s
```

Expected for `--selftest switch`: at 5 s `active=Fallback 'primary not available'`, at 9 s
`active=Primary`, and `out=Running` on every line.

**Manual checklist (needs VB-CABLE, the headset and the desk mic):**

1. **Passthrough.** Start VoxShift with Voice changer OFF. In `mmsys.cpl` â†’ Recording, CABLE
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
   (e.g. *Microphone (2- HyperX â€¦)* still matches *Microphone (HyperX â€¦)*).
8. **Soundboard.** Drop an mp3 on the window, click it: Discord hears it mixed with your voice.
   Assign Ctrl+Numpad1, focus a game or another window, press it: it plays, and the game still
   gets the key.
9. **Persistence.** Quit and reopen: devices, sliders, presets, sounds and hotkeys are back.
10. **Close to tray.** Talk with Discord's input meter (or a recording) running. Click the X and
    choose *Minimize to tray*: the window disappears, the notification appears, and the meter
    keeps moving without a gap. Click the tray icon: the window comes back with no audio glitch.
11. **Tray menu.** Right-click the icon and try *Voice changer* (the icon turns grey), *Mute
    mic* (red slash; Discord hears silence but sounds still play), *Stop all sounds*, and check
    that the read-only line shows the active mic. Pull the primary mic: the icon tooltip says
    *Fallback mic*. Unplug both mics: the icon turns orange.
12. **Quit from the tray.** The icon disappears at once (no ghost icon when you hover the tray)
    and CABLE Output goes silent.
13. **Second launch.** With VoxShift in the tray, double-click the exe again: no second copy
    in Task Manager, and the existing window comes to the front.
14. **Remember my choice.** Tick it and choose *Quit completely*: the next X quits right away.
    Set it back to *Ask every time* under *Window & startup*.
15. **Start with Windows.** Tick it and *Start minimized to tray*, sign out and back in:
    VoxShift is only in the tray, and the mic works. In Task Manager â†’ Startup apps, VoxShift
    is listed. Untick it: the entry is gone.
16. **Voice hotkey while talking.** Set a toggle key (e.g. F8) and talk while monitoring. Press
    it a few times: you hear the beeps, and the switch between effects and clean has no pop.
17. **Echo tail.** Turn on Echo (or the Cave preset), say something short, and press the
    hotkey right after: the echoes keep sounding and fade away naturally.
18. **In a game.** Focus a game, press the hotkey: the beep confirms it, Discord hears the
    change, and the game still reacts to the key normally. Try *Hold for effects* too.
19. **Duplicates.** Try giving the toggle the same key as a soundboard clip: you get a
    message, and nothing changes.

**Melody Voice (needs your mic and headphones; turn the monitor on):**

20. **Hear the target first.** Pick *Twinkle Twinkle Little Star*, click *Play melody*: a soft
    tone plays the tune in your headphones and the green line runs along the piano roll.
    Discord/CABLE Output hears nothing of it.
21. **Talk the song.** Apply *Sing the Song (per-syllable)*. Say slowly, one syllable at a time
    with a tiny gap: "twin - kle - twin - kle - lit - tle - star". You should hear yourself
    *sing* C C G G A A G (the first line of the tune), and the lit note in the piano roll
    moves one step per syllable. The top line reads like "You: A2 Â» Target: C3".
    Say "sss" or whisper: it passes through unchanged. Stop talking: nothing moves.
22. **Glitches.** Say one long "heeeeelloooo" with no gap: one or two notes, not a run of
    them. If syllables get skipped or doubled, raise or lower *Sensitivity* and *Min note
    length*.
23. **Your range.** A low voice should get the tune in a low octave (the "Target" note is
    near your own), a high voice in a higher one. Toggle *Octave-fold* off to hear the tune
    as written (it may then sound strained or chipmunk-like far from your voice).
24. **Formants.** With *Preserve formants* off, high notes sound chipmunk-like; with it on, still
    you. Move *Formant shift* to âˆ’6 and +6: darker or bigger, then brighter or smaller, same notes.
25. **Tempo-synced.** Switch mode, set the tempo to ~90, and hold one long "aaaah": it walks
    through the tune in time. Stop for a few notes and start again: it carries on further
    along, not where you stopped.
26. **Manual and hotkeys.** Set *Next note* to a key (e.g. Page Down), switch to *Manual step*,
    hold an "aaah" and press the key: one note per press. Set the *Restart melody* and
    *on/off* keys and try them with VoxShift in the tray.
27. **Hard Tune.** Apply the *Hard Tune* preset and slide your voice up and down slowly: it
    jumps from note to note (the robotic auto-tune sound) instead of sliding.
28. **Your MIDI file.** Drop a `.mid` on the window: a message names the part it picked (â˜… in
    *Part*); the piano roll should look like the tune. If it is the bass or chords, pick
    another part.
29. **Soundboard stays clean.** With Melody Voice on, play a soundboard clip with singing or
    speech: it must sound exactly like the original in Discord, not retuned.
30. **Latency.** The top bar's latency estimate goes up by ~16 ms when Melody Voice turns on.

## 9. How it works

```
 mic callback(s) â”€â”€ringâ”€â”€â–¶ â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€ output callback (CABLE Input) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”
  (downmix, level)         â”‚ adaptive resample â”€â–¶ mic crossfade â”€â–¶ voice chain â”€â–¶ + sounds â”‚â”€â”€â–¶ CABLE Input
                           â”‚  (drift control)       (50 ms)          (effects)     (voices) â”‚
 device manager thread â”€cmdâ”€â–¶ (attach/detach mics, monitor; play sounds)                    â”‚â”€â”€ringâ”€â”€â–¶ monitor
  (scan 1.5 s, errors,     â””â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜
   stall watchdog, mic choice)          â—€â”€â”€garbageâ”€â”€ (old buffers freed off the audio thread)
```

- **Real-time safety.** The audio callbacks never lock, allocate, log or do I/O. Parameters are
  atomics (`dsp::params`, read once per 64-sample block). New mic readers, the monitor ring and
  sound buffers arrive through a lock-free `rtrb` queue. Anything the audio thread lets go of is
  sent back through a second queue and freed on the device manager thread.
- **Clock drift and sample rates.** Each mic has its own clock. `audio::adaptive` resamples with
  rubato's async resampler and nudges the ratio (Â±1 %, a proportional controller) so the ring
  buffer stays at its target fill. That also covers 44.1 kHz mics on a 48 kHz cable.
- **Hot-plug.** Only the device manager thread talks to cpal. Mics and the monitor attach to the
  running engine, so the virtual-mic stream keeps running when they come and go. It restarts
  only when you change the output device or buffer size.

The window is only a view. While it's hidden, eframe runs only `App::logic` (not `ui`), which
handles tray requests, close requests and saving. The tray runs on its own thread with its own
Win32 message loop, so it keeps working whatever the window does.

```
src/
  shell/      tray icon thread, icon drawing, single instance, start with Windows (Win32)
  audio/      streams, engine, device manager, drift-compensating reader, mic selection, name matching
  dsp/        one file per effect + params, smoothing, chain (all unit-tested); Melody Voice:
              pitch_detect (YIN), conductor (which note when), melody (the effect), preview
  melody/     note type, built-in tunes, MIDI import (midly), lock-free hand-off to the engine
  soundboard/ decoding (symphonia + rubato), sound list + loader, RT mixer, global hotkeys
  ui/         egui panels: devices, effects, Melody Voice, presets, soundboard, meters, toasts
  config/     settings file + presets
```

The DSP is hand-written: RBJ biquads, Hann-windowed two-head pitch shifter with
correlation-aligned splices, LPC (Levinson-Durbin + lattice filters) for formant preservation,
YIN pitch detection, Freeverb, etc. Each file explains its idea at the top.

**Melody Voice in short:** a YIN detector runs on a 12 kHz copy of the mic every 2.5 ms; a
tracker smooths it (median, octave-jump guard, voiced/unvoiced hysteresis); the conductor
turns syllables, the clock or key presses into a target note; the shift (target âˆ’ you) drives
the formant-preserving pitch stage; unvoiced sounds take a dry path delayed by the same amount.
The melody reaches the audio thread through a seqlock over fixed atomic arrays (no locks, no
allocation, and it survives the engine being rebuilt).

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
  key input from non-admin apps. Run VoxShift as administrator in that case.
