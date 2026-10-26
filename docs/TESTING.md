# Testing VoxShift

## Automated tests

```powershell
cargo test
cargo clippy --all-targets -- -D warnings
```

Most tests are pure logic or run the real audio engine offline, with synthetic input. Some
examples:

- Effects, mixing and switching are checked for clicks (no sample-to-sample jumps) and for the
  right fade times.
- Melody Voice feeds synthetic voices through the real effect chain: a low and a high
  "speaker" saying 12 syllables must come out singing the first notes of a tune within a
  quarter of a semitone. The pitch detector is checked on tones across the voice range, on
  noise, whispers and silence.
- The panic button is tested during a loud effect with a playing clip: the output must become
  exactly the clean mic, and Restore must bring the effect back with unchanged settings.
- Settings files from older versions must still load.

A few tests need an interactive Windows desktop: they press F24 (a key no keyboard has) and
listen for it, or show a tray icon for a moment. Don't type while they run. GitHub Actions
skips them; the list is in [`.github/workflows/release.yml`](../.github/workflows/release.yml).

## Self-tests on your own machine

```powershell
.\target\release\voxshift.exe --list-devices     # what Windows exposes; shows whether VB-CABLE is found
.\target\release\voxshift.exe --selftest         # opens the default mic + output, prints the status for 8 s
.\target\release\voxshift.exe --selftest switch  # first two mics as primary/fallback, simulates unplugging the primary at 4 s and replugging it at 8 s
```

Expected for `--selftest switch`: at 5 s `active=Fallback 'primary not available'`, at 9 s
`active=Primary`, and `out=Running` on every line.

## Manual checklist

These need real hardware: VB-CABLE, headphones, and ideally two microphones (for example a
headset and a desk or webcam mic). Use Discord's input meter, or a recording of CABLE Output,
to hear what others hear.

**Basics**

1. **Passthrough.** With the voice changer OFF, CABLE Output's level bar in `mmsys.cpl` →
   Recording moves when you talk, and a recording of it is your clean voice.
2. **Monitor.** Turn the monitor on with headphones: you hear yourself, slightly late.
3. **Effects.** With the voice changer ON, try each preset and drag sliders while talking:
   no clicks. ON/OFF fades instead of clicking.
4. **Echo tail.** Turn on Echo, say something short and switch off right after: the echoes
   fade out naturally.
5. **Persistence.** Quit and reopen: devices, sliders, presets, clips, folders and hotkeys are
   back.

**Two microphones**

6. **Unplug the primary** while talking. Within about 1.5 s VoxShift switches to the fallback,
   says so, and the output never drops out. Plug it back in: it switches back (with switch-back
   on).
7. **Detachable mic.** If the primary is a headset with a detachable mic, pull the mic out:
   after the set number of seconds VoxShift uses the fallback. Plug it back in and talk: back
   on the primary. If it never triggers, compare the primary's dB value with the mic pulled
   out and adjust the threshold.
8. **No mic.** Unplug both: a red warning, silence on CABLE Output, the app stays responsive.
   Plug one in: it's picked up within about 2 s.
9. **Renamed devices.** On another PC, or with a device that Windows renamed (for example
   *"2- USB Microphone"*), the mic is still found.

**Tray and startup**

10. **Close to tray** while talking: the window disappears, a notification appears, and the
    output keeps going without a gap. Clicking the tray icon brings the window back.
11. **Tray menu.** Try each entry; the icon colour follows (grey when the voice changer is
    off, a red slash when muted, orange without a mic).
12. **Quit from the tray.** The icon disappears at once and CABLE Output goes silent.
13. **Second launch.** With VoxShift in the tray, start it again: no second copy, and the
    window comes to the front.
14. **Remember my choice** in the close dialog, then set it back under *Window & startup*.
15. **Start with Windows** with *Start minimized to tray*: sign out and in, VoxShift is in the
    tray and the mic works. Untick it: the startup entry is gone.

**Hotkeys**

16. **Voice hotkey while talking.** Set a toggle key and press it a few times while
    monitoring: beeps, and no pop between effects and clean. Try *Hold for effects* too.
17. **In a game.** The hotkey works with the game focused, and the game still reacts to the
    key. Holding the key acts only once.
18. **Duplicates.** Give a clip F8, then try to give F8 to *Effects on/off*: the dialog names
    the clip, with *Keep* selected. Esc keeps everything. Tab then Enter (Change) moves F8.
    While the dialog is open, F8 plays nothing. Win+R is refused with a message.
19. **Hear myself and Stop all sounds.** The monitor fades your voice out and in with a low
    and a high beep, while CABLE Output is unchanged. *Stop all sounds* stops a long clip at
    once, without a pop.

**Soundboard**

20. **Playing.** Drop an mp3 on the window and click it: others hear it mixed with your voice.
    Its hotkey works with a game focused.
21. **Folders and search.** Make two folders, drag clips onto their tabs, rename one, delete
    it: its clips go to *Unsorted*. Searching "cafe" finds a clip named "Café ...". After a
    restart the same tab and sort are selected, and stars, colours and *Recent* are kept.
22. **Missing file.** Rename a clip's file and restart: the clip is grey with *Locate…*.
    Locating the file keeps its folder, colour and star.
23. **Hold mode in a game with alt-tab.** Give a long clip the *Hold* mode and a hotkey. In a
    game, hold the key: it plays only while held and stops with no click. Hold it, alt-tab out
    while still holding, and let go on the desktop: the clip stops. *Loop while held* on a short
    clip loops only while held. Holding a *Hold* clip with the mouse and dragging off the
    window before letting go also stops it.
24. **Panic during a loud effect and a playing clip.** Set a panic key, apply a strong preset
    with the output gain raised, start two long clips (one held in *Loop while held*), and
    talk. Press the panic key: a falling beep in the headphones, both clips stop without a
    pop, others hear your clean voice, and the banner and tray icon turn red. Clicking clips
    plays nothing. Press it again: the effect is back with the same sliders, and the clips stay
    stopped. Repeat with the window hidden and from inside a game, and once with *Panic also
    mutes my headphones* ticked.

**Melody Voice** (headphones and monitor on)

25. **Play melody** plays the tune in the headphones only; CABLE Output hears nothing of it.
26. **Talk the song.** With *Sing the Song (per-syllable)* and *Twinkle Twinkle*, say
    "twin - kle - twin - kle - lit - tle - star" slowly: you hear yourself sing the first line,
    and the lit note moves one step per syllable. "sss" or a whisper passes through unchanged.
27. **Glitches.** One long "heeelloooo" gives one or two notes, not a run of them.
28. **Range.** A low voice gets the tune in a low octave and a high voice in a higher one.
    With *Octave-fold* off you hear the melody as written.
29. **Formants.** *Preserve formants* off sounds chipmunk-like on high notes; on, it still
    sounds like you. *Formant shift* at −6 and +6 sounds darker and brighter.
30. **Tempo-synced and Manual step** move through the tune in time, or one note per *Next
    note* press.
31. **Hard Tune.** Sliding your voice up and down jumps from note to note.
32. **MIDI import.** Dropping a `.mid` file names the part it picked; the piano roll looks like
    the tune.
33. **Soundboard stays clean.** With Melody Voice on, a clip with singing sounds exactly like
    the original.
34. **Latency.** The latency estimate goes up when Melody Voice turns on.
