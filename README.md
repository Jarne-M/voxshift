# VoxShift

**A free voice changer and soundboard for Windows that runs entirely on your own PC.**

VoxShift takes your microphone, changes your voice in real time, mixes in sound clips, and
sends the result to a *virtual microphone*. Discord, OBS, games or any other app then use that
virtual microphone instead of your real one, so they hear the changed voice and your sounds.

Everything runs locally. There's no account, no cloud service, no ads and no telemetry, and
nothing is uploaded anywhere.

<!-- screenshot: main window -->

## Contents

- [Features](#features)
- [Requirements](#requirements)
- [Getting started](#getting-started)
- [Using VoxShift](#using-voxshift)
  - [Microphones](#microphones)
  - [Monitor (hear yourself)](#monitor-hear-yourself)
  - [Voice changer and effects](#voice-changer-and-effects)
  - [Presets and sharing](#presets-and-sharing)
  - [Melody Voice](#melody-voice)
  - [Soundboard](#soundboard)
  - [Panic button (safe mode)](#panic-button-safe-mode)
  - [Tray icon, closing and startup](#tray-icon-closing-and-startup)
- [Hotkeys](#hotkeys)
- [Settings](#settings)
- [Limitations](#limitations)
- [FAQ and troubleshooting](#faq-and-troubleshooting)
- [Building from source](#building-from-source)
- [How it's built](#how-its-built)
- [Contributing](#contributing)
- [Responsible use](#responsible-use)
- [License](#license)
- [Credits](#credits)

## Features

**Your voice**
- Effects: pitch, noise gate, tone/EQ, distortion, robot, chorus, echo and reverb.
- Ready-made presets (Deep Voice, Chipmunk, Robot, Demon, Radio and more) plus your own.
- **Melody Voice**: talk and it comes out *sung* to a melody, or auto-tuned to a key. Use the
  built-in tunes or your own MIDI files.
- A big ON/OFF switch. OFF sends your normal, clean voice.

**Soundboard**
- Play wav, mp3, ogg or flac clips into your virtual mic, mixed with your voice.
- Folders, search, favorites, a Recent list and colours to keep many clips tidy.
- Play modes: play to the end, play only while held, start/stop, or loop while held.

**Reliable when it matters**
- Two microphones: if your main mic is unplugged or goes silent, VoxShift switches to a
  backup mic on its own, without interrupting the virtual mic.
- A **panic button** that instantly stops all sounds and sends only your clean voice, and
  puts everything back with one more press.

**Convenient**
- Global hotkeys that work while a game has focus (the game still gets the key).
- Runs in the system tray, can start with Windows, and remembers all your settings.
- Share your presets and melodies with friends in one file.

## Requirements

- **Windows** (64-bit).
- **VB-CABLE**, a free virtual audio cable driver by VB-Audio. It creates the virtual
  microphone that other apps listen to. VoxShift doesn't include it; you install it once
  yourself (see below).
- **A working microphone** (a headset, USB mic, webcam mic, ...).
- **Headphones** are strongly recommended if you want to hear yourself. With speakers your mic
  picks up its own output and you get echo or feedback.

## Getting started

### 1. Install VB-CABLE (once)

Download VB-CABLE from <https://vb-audio.com/Cable/> and install it following the
instructions on that page.

You then have two new audio devices. The names are confusing, so here's what they are:

| Device | Type | What it's for |
|---|---|---|
| **CABLE Input** | a playback device (like speakers) | VoxShift plays your changed voice *into* this |
| **CABLE Output** | a recording device (like a mic) | Discord, OBS and games *listen* to this |

> **Tip:** don't make *CABLE Input* your default playback device in Windows, or every
> Windows sound (notifications, videos, ...) ends up in your mic.

### 2. Download and start VoxShift

1. Download `VoxShift.exe` (or the zip) from the **[Releases page](../../releases/latest)**.
2. Put it anywhere (for example on your Desktop) and double-click it. There's no installer.
3. If Windows shows **"Windows protected your PC"**, click **More info → Run anyway**. The
   app isn't signed with a paid certificate, so Windows warns about it.

### 3. Pick your devices

<!-- screenshot: left panel with devices -->

The left panel of the window holds the device settings. On the first start VoxShift already
picks **CABLE Input** as its output (if VB-CABLE is installed) and your Windows default mic as
the **Primary mic**. Check that:

- **Virtual mic (output)** says *CABLE Input (VB-Audio Virtual Cable)*. If VB-CABLE isn't
  found, a yellow banner says so, with a download link.
- **Primary mic** is the microphone you talk into.

### 4. Use CABLE Output as your mic in other apps

| App | Where to set it |
|---|---|
| **Discord** | User Settings → Voice & Video → **Input Device** → *CABLE Output*. Also turn **off** Noise Suppression, Echo Cancellation and Automatic Gain Control there: they treat effects and music as noise and filter them out. |
| **OBS** | Settings → Audio → **Mic/Auxiliary Audio** → *CABLE Output* (or add an *Audio Input Capture* source with CABLE Output). Remove or mute any source that records your real mic, or you'll be heard twice. |
| **Games** | The game's voice chat settings → input device → *CABLE Output*. If the game has no such setting, it uses the Windows default mic: Settings → System → Sound → Input → *CABLE Output*. |

### 5. How do I know it works?

- In VoxShift, the **In** and **Out** meters at the top move when you talk.
- Open the Windows sound settings (press Win+R, type `mmsys.cpl`, press Enter) → **Recording**
  tab: the level bar next to **CABLE Output** moves when you talk.
- Record yourself with the Windows **Sound Recorder** app, with CABLE Output as its
  microphone, and play it back.

Then pick a preset at the top of the **Effects** tab and try it.

## Using VoxShift

### Microphones

You can set two mics in the **Microphones** card:

- **Primary mic**: the one you normally use.
- **Fallback mic** (optional): a backup, for example a desk mic or your webcam's mic.

With **Auto** selected, VoxShift uses the primary while it works and switches to the fallback
when the primary is unplugged, missing or broken. The switch is a short crossfade, and apps
listening to CABLE Output never notice a gap. With **Switch back to the primary
automatically** on, it goes back as soon as the primary works again. **Force primary** and
**Force fallback** pin one mic.

The top bar always shows which mic is active and why. When it switches, VoxShift tells you with
a short message in the window, or a Windows notification while it's hidden in the tray.

**Treat a silent primary as unplugged.** Some headsets have a detachable mic. When you pull it
out, Windows still lists the headset, but it only delivers silence. With this option on, a
primary that stays below a level (default **−85 dB**) for a number of seconds (default **5 s**)
counts as unplugged. To tune it, look at the level the Microphones card shows for your primary
in dB: note it while the mic is plugged in and the room is quiet, then with the mic pulled
out. Put the threshold in between.

Mics are remembered **by name**, so a different USB port, a reboot or a slightly renamed device
(for example *"2- USB Microphone"*) doesn't break the setup.

### Monitor (hear yourself)

The **Monitor** card plays the result on your headphones, so you hear what others hear. Pick
your headphones, tick **on**, and choose what you hear: **my voice**, **soundboard**, or both,
with its own volume. The monitor never goes into the virtual mic.

### Voice changer and effects

<!-- screenshot: Effects tab -->

- The big **Voice changer ON/OFF** button at the top switches all effects at once. OFF sends
  your clean voice. Switching is a short crossfade, never a click, and it doesn't change your
  sliders: switching back gives exactly what you had. An echo or reverb that's still sounding
  fades out naturally.
- **Mute mic** silences your mic. Soundboard clips still play.
- The meters show your input and output level. The red **clip** lamp lights up when the
  output is too loud and gets distorted: lower the output gain.

Effects on the **Effects** tab (each one can be turned on and off by itself):

| Effect | What it does |
|---|---|
| **Gain** | Input gain (how loud your mic comes in), output gain, and dry/wet (how much of the effect you hear versus your clean voice). |
| **Noise gate** | Mutes your mic when you're not talking, to cut background noise. |
| **Pitch** | Makes your voice higher or lower, up to 12 semitones (one octave). *Preserve formants* keeps it sounding like you instead of a chipmunk or a giant. |
| **Tone** | Low cut, high cut and a 3-band EQ (bass, mids, treble). |
| **Distortion** | A crunchy, overdriven sound. |
| **Robot** | A metallic, robotic voice. |
| **Chorus** | A thicker voice, as if several people talk at once. |
| **Echo** | Repeats of your voice. |
| **Reverb** | Room or hall sound. |

Every slider change is smooth, so you can adjust things while talking.

### Presets and sharing

The preset bar sits at the top of the Effects tab.

- **Built-in presets:** Clean, Deep Voice, Chipmunk, Robot, Cave, Radio, Telephone, Demon,
  Sing the Song (per-syllable) and Hard Tune. They can't be overwritten.
- **Your own:** set the sliders, type a name and click **Save**. Typing the name of one of your
  existing presets turns the button into **Update**, which overwrites it. **Delete** removes the
  selected preset. A preset saved with Melody Voice on also remembers its melody. When you
  change a slider after picking a preset, the list shows *(modified)*.
- **Share…** saves all your own presets and imported MIDI files in one `.voxpack` file.
- **Import…** (or dropping a `.voxpack` on the window) adds the presets and melodies from such
  a file. Your own presets are kept. A name you already use gets "(2)", and importing the same
  pack twice adds nothing.

Soundboard clips aren't part of a `.voxpack`; send those files separately.

### Melody Voice

<!-- screenshot: Melody Voice panel -->

Melody Voice sits at the top of the Effects tab. Turn it **ON**, pick a melody and talk: each
syllable comes out on the next note of the song. Sounds like "s", "t", "f" and breaths pass
through unchanged, so you stay understandable. The easiest start is the *Sing the Song
(per-syllable)* preset: say a sentence slowly, one syllable at a time.

**Modes**

| Mode | When it moves to the next note |
|---|---|
| **Per syllable** (default) | On every new syllable. *Sensitivity* sets how easily a new syllable is detected; *Min note length* stops glitches from skipping notes. |
| **Tempo-synced** | The song plays at the **Tempo** you set, starting when you start making sound. |
| **Manual step (hotkey)** | Only when you click **Next note** or press its hotkey. |
| **Scale snap (auto-tune)** | No melody: your voice is pulled to the nearest note of a key and scale (major, minor, major or minor pentatonic, chromatic). |

**Melodies**

- **Built in** (all public domain, written into the app as notes): Ode to Joy, Twinkle
  Twinkle Little Star, Happy Birthday, Greensleeves, Für Elise (opening), Frère Jacques, a
  major scale, a minor scale, a pentatonic run and a major arpeggio.
- **Your own:** click **Import MIDI…** or drop a `.mid` file on the window. VoxShift lists the
  file's parts and picks the one most likely to be the tune (marked ★ under **Part**). If it
  picked the bass or the chords, choose another part. Imported files are remembered.

**Controls**

- **Retune speed:** 0 ms gives the instant, robotic "hard tune" sound; higher values keep more
  of your natural voice.
- **Glide:** how quickly it slides from one note to the next.
- **Transpose:** moves the melody up or down (±24 semitones). **Octave-fold into my range**
  (on by default) moves it by whole octaves to sit near your own voice.
- **Preserve formants** keeps your voice's character on high or low notes. **Formant shift**
  changes that character on purpose: darker/bigger or brighter/smaller.
- **Loop**, **Restart**, **Next note**, and **Back to start after silence**.
- **Analysis window** and **Lowest / highest voice pitch:** advanced settings. If it ever jumps
  to the wrong octave, narrow the pitch range to your own voice.
- The line **"You: A3 » Target: C4"** shows the note it hears and the note it's aiming for.
  The piano roll shows the melody and the current note.
- **Play melody** plays the tune as a soft tone in your headphones only (the monitor must be
  on), so you can learn it. It never goes into your mic.

Melody Voice adds more delay than the other effects; the panel shows how much. Soundboard clips
are never retuned.

### Soundboard

<!-- screenshot: Soundboard tab -->

**Adding and playing clips**

- Click **Add sounds…** or drag wav, mp3, ogg or flac files onto the window. Each file is
  loaded in the background. Clips longer than 3 minutes are cut off.
- **Click** a clip to play it. **Right-click** it for more: Stop, add to Favorites, Move to…,
  Play mode, Colour, Locate file (if missing) and Delete.
- Each clip has its own **volume** slider and an optional **hotkey** (the key button under it).
- **Stop all** stops every clip. **Also play in my headphones** lets you hear the clips too
  (the monitor must be on). Other people always hear them.
- Clips are mixed into the virtual mic together with your voice; voice effects don't change
  them.

**Organising**

- **Tabs:** *All*, *★ Favorites*, *Recent*, *Unsorted* and your own folders.
  - **+ Folder** creates a folder. Right-click a folder tab to rename it, move it left or right,
    or delete it.
  - Deleting a folder never deletes clips or files: its clips move to *Unsorted*.
  - New clips go into the folder that's open.
- **Moving clips:** drag a clip onto a tab, or use right-click → **Move to…**.
- **Search** (Ctrl+F) looks through all folders as you type. It ignores upper/lower case and
  accents and matches part of a name, so "cafe" finds "Café Ambience". Esc clears it.
- **Sort:** *My order*, *Name*, *Most played* or *Recently added*. With *My order*, drag a clip
  onto another one to put it there.
- **Favorites:** click the ☆ under a clip.
- **Recent:** the last 20 clips you played, newest first. *Clear* empties the list.
- **Colours:** right-click → **Colour** gives a clip one of 10 colours.
- Hover a clip to see its length and how often you've played it.
- **Missing files:** if a clip's file was moved or deleted, the clip turns grey and shows
  **Locate…**. Point it to the file again, and it keeps its folder, colour, hotkey and star.

**Play modes** (right-click → **Play mode**; a small badge on the clip shows the mode):

| Mode | When you press | When you let go |
|---|---|---|
| **Normal** | Plays to the end (pressing again restarts it) | Nothing |
| **Hold** | Starts | Stops, with a quick fade |
| **Toggle** | Starts, or stops it if it's playing | Nothing |
| **Loop while held** | Starts and keeps looping | Stops, with a quick fade |

Hold modes work with the hotkey and with the mouse (keep the button pressed on the clip).
Holding a key down doesn't restart the clip. If Windows misses the moment you let go (for
example when you alt-tab out of a game), VoxShift notices that the key is up and stops the
clip anyway.

### Panic button (safe mode)

For when something goes wrong live: a clip won't stop, an effect is far too loud, or you just
need to sound normal right now.

- **How:** the red **PANIC** button at the top right of the window, the tray menu, or a
  hotkey. The hotkey works in games and while VoxShift is hidden, and it's checked before any
  other hotkey.
- **What happens at once:**
  - every clip fades out, including Hold and Loop clips;
  - the virtual mic gets only your clean voice: voice changer, effects and Melody Voice are
    bypassed;
  - *Play melody* stops, and no clip can be started.
  - Optionally (**Panic also mutes my headphones**, off by default) your headphones go quiet
    too.
- **What doesn't change:** your sliders, preset, the ON/OFF switch and all other settings. The
  big ON/OFF button is locked while safe mode is on.
- **How you can tell:** a red **SAFE MODE** banner in the window, a red tray icon with a white
  bar, and (with beeps on) a falling beep in your headphones. The beep never goes into the
  mic.
- **Restore:** the banner's **Restore** button, the same top-right button, the tray menu or
  the hotkey again. Everything comes back exactly as it was, and you hear a rising beep. Clips
  that were stopped stay stopped. Safe mode never turns itself off, and VoxShift always starts
  normally.

### Tray icon, closing and startup

Your virtual mic only works while VoxShift runs. That's why closing the window doesn't quit
it right away.

- **Closing the window** asks what to do:
  - **Minimize to tray** (the default) keeps everything running: audio, mic fallback,
    soundboard and hotkeys.
  - **Quit completely**.
  - **Cancel**.

  Tick **Remember my choice** to stop being asked. You can change this later under
  **Window & startup** in the left panel.
- **Tray icon:** click it to show the window. Right-click for a menu:
  - Show VoxShift
  - Panic / Restore
  - Voice changer on/off
  - Mute mic
  - Hear myself (monitor)
  - the active mic
  - Stop all sounds
  - Quit

  Hovering the icon shows the status, and its colour shows problems at a glance:

  | Icon | Meaning |
  |---|---|
  | violet | all good |
  | grey | voice changer off |
  | grey with a red slash | mic muted |
  | orange | no microphone available |
  | red | virtual mic output missing |
  | red with a white bar | safe mode (panic) is on |

- **Only one copy runs.** Starting VoxShift again brings the running copy to the front.
- **Start with Windows** (under Window & startup) starts VoxShift when you sign in, and
  **Start minimized to tray** starts it straight in the tray. Both are off by default and need
  no admin rights.
- When Windows shuts down or you sign out, VoxShift saves your settings and closes without
  asking.

## Hotkeys

<!-- screenshot: Voice changer hotkeys card -->

Hotkeys are **global**: they work while another app or game has focus, and while VoxShift is
in the tray. They also **pass through**, so the game still receives the key. VoxShift only
listens to the keyboard; it never blocks or changes keys.

All hotkeys start unassigned. To set one, click its button (it says **Not set**), then press a
key or a combination with Ctrl, Alt and/or Shift. Esc cancels. A combination like
Ctrl+Numpad 1 avoids accidental in-game actions.

| Action | Where to set it | What it does |
|---|---|---|
| Panic (safe mode) | Voice changer hotkeys | Safe mode on, or restore ([details](#panic-button-safe-mode)) |
| Effects on/off | Voice changer hotkeys | Switches the voice changer (see the modes below) |
| Mute / unmute mic | Voice changer hotkeys | Mutes your mic; clips still play |
| Jump to a preset | Voice changer hotkeys → *+ Add preset hotkey* | Loads that preset and turns effects on |
| Melody Voice on / off | Voice changer hotkeys → Melody Voice | Turns Melody Voice on or off |
| Restart melody | Voice changer hotkeys → Melody Voice | Back to the first note |
| Next note | Voice changer hotkeys → Melody Voice | One note further (used by *Manual step*) |
| Hear myself | Voice changer hotkeys → Monitor and soundboard | Your voice on the monitor on/off |
| Hear soundboard | Voice changer hotkeys, or next to *Also play in my headphones* | Clips on the monitor on/off |
| Stop all sounds | Voice changer hotkeys, or next to *Stop all* | Stops every clip |
| A clip | The key button under the clip | Plays it, according to its play mode |

**Effects on/off modes**

- **Toggle** (default): each press switches.
- **Hold for effects (push-to-talk):** clean voice, with effects only while you hold the key.
- **Hold for clean (push-to-clean):** effects, with your clean voice only while you hold the key.

**Feedback when you switch**

- **Beep in my headphones** (on by default): a rising beep for ON and a falling beep for OFF.
  *Hear myself* and *Hear soundboard* use one high or low beep. Beeps play on the monitor only,
  so the monitor must be on. To hear only the beeps, untick *my voice* there.
- **Show "Voice changer: ON/OFF" on screen** (off by default): a small banner for a second.
  It doesn't appear over games in exclusive fullscreen.
- **At start:** remember the last ON/OFF state (default), always start with effects on, or
  always off.

**One key, one action.** If the key you press is already used for something else, a dialog
names it, for example *F8 is already used for the sound "airhorn"*:

- **Keep** (the default; also Enter or Esc) leaves everything as it was.
- **Change** moves the key to the new action, and the other one becomes **Not set**.

Windows' own shortcuts (anything with the Windows key, Ctrl+Alt+Del, Alt+Tab, Alt+F4 and Esc
combinations) can't be used and are refused with a message. Set hotkeys with the VoxShift
window in front: VoxShift can't hide the key you press from other apps.

## Settings

| What | Where |
|---|---|
| Settings file | `%APPDATA%\VoxShift\config.json` (paste that into Explorer's address bar) |
| Imported MIDI files | `%APPDATA%\VoxShift\melodies` |
| Soundboard clips | Stay where they are on your disk; VoxShift remembers their location |

Everything is saved automatically, about a second after each change and when you quit:
devices, mic options, sliders, presets, clips, folders, hotkeys and window choices.

- **Reset everything:** quit VoxShift (from the tray menu), then delete or rename
  `config.json`. The next start uses the defaults.
- **Damaged file:** if the file can't be read, VoxShift renames it to `config.broken.json`,
  starts with the defaults and tells you.
- **A second profile:** start VoxShift with `--config` and a path, for example
  `VoxShift.exe --config D:\VoxShift\streaming.json`.

## Limitations

- **Windows only.** VoxShift uses Windows-specific features for the tray, hotkeys and startup.
- **VB-CABLE is required** for other apps to hear you. Without it you can still try VoxShift
  by picking your headphones as its output.
- **There is a small delay.** Your voice reaches other apps a few tens of milliseconds late.
  Most of it comes from Windows' audio buffers (often about 10 ms per device) and VB-CABLE's own
  buffer; the Pitch effect and especially Melody Voice add more. The **Latency** card shows
  VoxShift's estimate. For voice chat this is normally fine; don't use the monitor as a
  speaker for an audience.
- **Big pitch jumps sound grainier** than small ones. Very breathy or creaky speech may not be
  retuned by Melody Voice for a moment.
- **The exe is not code-signed**, so Windows SmartScreen and some antivirus programs warn
  about it.

## FAQ and troubleshooting

**Nobody hears me / they hear my normal voice.**
The other app must use **CABLE Output** as its microphone (not CABLE Input, and not your real
mic). In VoxShift, the virtual mic (output) must be **CABLE Input**, and the top bar must show
an active mic. Check the **In** meter moves when you talk.

**Discord makes the effects or soundboard music sound bad or cuts them off.**
Turn off Discord's Noise Suppression, Echo Cancellation and Automatic Gain Control (User
Settings → Voice & Video), and set the input sensitivity by hand if needed.

**I hear an echo or my voice twice.**
You're probably hearing yourself through speakers (use headphones), or another app (OBS,
Discord) is also recording your real mic. Remove that source.

**All my Windows sounds go into my mic.**
CABLE Input is set as the Windows default playback device. Set your speakers or headphones as
the default again.

**"VB-CABLE not found".**
Install VB-CABLE (see [Getting started](#1-install-vb-cable-once)), then click **Rescan** in
the virtual mic card.

**The delay feels too long.**
Turn off Pitch and Melody Voice to compare. Try a smaller **Buffer** in the Latency card
(many Windows drivers ignore values below their own minimum).

**Windows says "Windows protected your PC", or my antivirus complains.**
VoxShift isn't signed with a paid certificate. Click **More info → Run anyway**. If you'd
rather not trust a downloaded exe, [build it yourself](#building-from-source).

**It switches to my fallback mic while I'm quiet.**
Some mics or drivers (with built-in noise suppression or Windows "audio enhancements") send
pure silence when you don't talk, which looks like an unplugged mic. Lower the dB threshold,
raise the seconds, or turn off **Treat a silent primary as unplugged**. Turning off the
driver's noise suppression also helps.

**I lost my settings / want to start fresh.**
See [Settings](#settings).

## Building from source

For developers. You need Windows and [Rust](https://rustup.rs) (stable, with the
MSVC or GNU toolchain).

```powershell
git clone https://github.com/Jarne-M/voxshift.git
cd voxshift
cargo run --release          # build and start
cargo build --release        # just build: target\release\voxshift.exe
```

The result is a single exe with no installer and no extra files.

`build.cmd` and `env.ps1` are only for a *portable* Rust toolchain in a `.toolchain` folder
next to the project (no system-wide Rust install). With that setup, double-click `build.cmd`
(quit VoxShift first), or dot-source `env.ps1` in PowerShell before running cargo. PowerShell
may first need `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`.

Command-line options for checking audio without the window:

```powershell
voxshift.exe --list-devices      # lists the audio devices Windows exposes, and whether VB-CABLE is found
voxshift.exe --selftest          # runs the audio with the default mic for a few seconds and prints the status
voxshift.exe --selftest switch   # also simulates unplugging and replugging the primary mic
voxshift.exe --config <file>     # use another settings file
```

### Running the tests

```powershell
cargo test
cargo clippy --all-targets -- -D warnings
```

Most tests run anywhere. A few need an interactive Windows desktop: they press a key no
keyboard has (F24) and listen for it, or show a tray icon for a moment. Don't type while they
run. The CI skips them; see [`.github/workflows/release.yml`](.github/workflows/release.yml)
for the exact list. Manual tests on real hardware are described in
[docs/TESTING.md](docs/TESTING.md).

## How it's built

VoxShift is written in Rust, with [egui](https://github.com/emilk/egui) for the window and
[cpal](https://github.com/RustAudio/cpal) for audio.

The heart is the **audio thread**: the output device's callback. It reads the mic(s), runs the
effect chain, mixes in the soundboard and writes to CABLE Input. That thread must never wait,
so it never locks, allocates memory, logs or touches files. Settings reach it as atomic
values; new mics, buffers and clips arrive through lock-free queues; and anything it no longer
needs is sent back to be freed on another thread.

Around it, a **device manager** thread watches devices (rescans, hot-plug, mic choice), a
**hotkey** thread listens to the keyboard, the **tray** runs its own thread, and the **window**
is only a view on top. Closing the window doesn't stop any of them.

```
src/
  audio/       engine (the audio thread), device manager, mic selection, resampling
  dsp/         the effects, one file each, plus Melody Voice's pitch detection and retuning
  melody/      built-in tunes and MIDI import
  soundboard/  decoding, the clip list, the real-time mixer, hotkeys, folders, play modes
  ui/          the window's panels
  shell/       tray icon, single instance, start with Windows
  config/      settings file, presets, .voxpack sharing
```

More detail is in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Contributing

Bug reports, ideas and pull requests are welcome.

1. Fork the repository and create a branch.
2. Make your change, then run `cargo test` and `cargo clippy --all-targets -- -D warnings`.
3. Open a pull request. GitHub Actions builds and tests it on Windows automatically.

Please keep the audio thread rules above in mind: nothing on the audio path may lock,
allocate or block.

**Releases** (for maintainers): bump `version` in `Cargo.toml`, commit, then push a tag such as
`v0.2.0` (`git tag v0.2.0` and `git push origin main --tags`). GitHub Actions builds the exe
and publishes a release with `VoxShift.exe` and a zip.

## Responsible use

VoxShift is meant for fun with friends, streaming and privacy. Please:

- don't use it to impersonate real people, deceive, harass or defraud anyone;
- follow the rules of the platforms, servers and games you use it in (some forbid
  soundboards or voice changers);
- only use sound clips and MIDI files you have the right to use. VoxShift doesn't come with
  or download any songs; the built-in melodies are public domain.

## License

VoxShift is released under the [MIT License](LICENSE).

## Credits

- **VB-CABLE** by [VB-Audio](https://vb-audio.com/Cable/): the virtual audio cable driver
  VoxShift sends its audio to. It's a separate product with its own license and isn't included.
- Built with these open-source Rust libraries (thank you!):

  | Library | Used for | License |
  |---|---|---|
  | [cpal](https://github.com/RustAudio/cpal) | audio input and output | Apache-2.0 |
  | [egui / eframe](https://github.com/emilk/egui) | the window and UI | MIT or Apache-2.0 |
  | [symphonia](https://github.com/pdeljanov/Symphonia) | decoding mp3, ogg, flac and wav | MPL-2.0 |
  | [rubato](https://github.com/HEnquist/rubato) | sample-rate conversion | MIT or Apache-2.0 |
  | [midly](https://github.com/kovaxis/midly) | reading MIDI files | Unlicense |
  | [rtrb](https://github.com/mgeier/rtrb) | lock-free audio queues | MIT or Apache-2.0 |
  | [rfd](https://github.com/PolyMeilex/rfd) | file dialogs | MIT |
  | [serde](https://serde.rs) / serde_json | the settings file | MIT or Apache-2.0 |
  | [windows-sys](https://github.com/microsoft/windows-rs) | tray, hotkeys and other Windows features | MIT or Apache-2.0 |

- The built-in melodies are public-domain tunes, written into the code note by note.
