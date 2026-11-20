# VoxShift

A local real-time voice changer and soundboard for Windows, written in Rust with an egui window.
Your mic goes through a hand-written DSP chain into a virtual cable (VB-CABLE), which other
apps use as their mic. It lives in the tray, keeps running while the window is hidden, and has
global hotkeys (Windows Raw Input) that work while a game has focus.

## Finding your way

- **Read [docs/CODEMAP.md](docs/CODEMAP.md) first, then open only the files it points to.**
  For big files (`src/ui/mod.rs`, `src/ui/soundboard.rs`, `src/ui/widgets.rs`,
  `src/audio/engine.rs`, `src/soundboard/library.rs`, ...) grep for the symbol and read only
  that range. Don't read them whole.
- **When you add, remove, rename or substantially change a module, update docs/CODEMAP.md
  in the same change.**
- Deeper background: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) (signal flow, real-time
  rules, threads) and [docs/TESTING.md](docs/TESTING.md) (manual checklist, self-tests).

## Build, test, format

Every PowerShell call needs the portable toolchain first (it is not on PATH):

```powershell
. .\env.ps1; cargo build
. .\env.ps1; cargo test
. .\env.ps1; cargo clippy --all-targets -- -D warnings      # what CI runs
. .\env.ps1; rustfmt --edition 2024 src/keymap.rs src/ui/hotkeys_page.rs   # only files you changed
```

- Never run `cargo fmt`. Run `rustfmt --edition 2024` only on the files you changed.
- `rustfmt` follows `mod` declarations: `src/main.rs`, `src/shell/mod.rs` and `src/ui/mod.rs`
  pull in their submodules. For those, run `rustfmt --edition 2024 --check <file>` and fix
  your own lines by hand.
- Screenshot tests (PNG renders of pages, no window needed):
  `$env:VOXSHIFT_SHOTS = "<scratch folder>"; cargo test shots -- --ignored`
  (or name one test, e.g. `cargo test shots_of_the_setup_pages -- --ignored`). Open the PNGs
  to check a layout.
- Release build: `build.cmd` or `cargo build --release` (see the locked-exe note below).
- CI (`.github/workflows/release.yml`) skips the tests that need a real desktop:
  `listener`, `presses_are_counted`, `assigning_a_hotkey`, `duplicate_hotkeys`, `tray_starts`.
- **Linux also builds** (for tests, not as a product): `cargo test`, `cargo clippy
  --all-targets -- -D warnings` and the screenshot tests run there (needs `libasound2-dev`).
  `src/shell/other.rs` stands in for the Win32 shell; the Raw Input listener and the tray tests
  only run on Windows. When you change a public signature in `shell/tray.rs`, `instance.rs`
  or `autostart.rs`, change `other.rs` to match.

## Gotchas (each one has cost time before)

- **Not fmt-clean upstream:** `src/shell/tray.rs` (one test line) and `src/ui/theme.rs` (a
  blank line in the tests). Leave them out of rustfmt lists. Never `git checkout --
  src/shell/tray.rs` to undo a reformat: if you edited it, that throws your edits away. Use
  `rustfmt --check` to confirm your own lines instead.
- **The user's release exe is usually running** (in the tray), which locks
  `target\release\voxshift.exe`. Don't kill it. Check release builds with
  `cargo build --release --target-dir <scratchpad>\target`, and tell the user to quit VoxShift
  (tray, Quit) before they rebuild. A second launch only brings the running copy forward.
- **Windows PowerShell 5.1 traps:** `Set-Content -Encoding utf8` writes a BOM, and backticks in
  double-quoted strings are escapes (`` `t `` and `` `n `` have corrupted doc comments). Make
  source edits with the Edit tool, never with PowerShell `.Replace(...)`. If a file must be
  written from PowerShell, use
  `[IO.File]::WriteAllText(path, text, (New-Object Text.UTF8Encoding($false)))`. Sources use LF.
- **Commit messages:** write them to a scratch file and use `git commit -F <file>`. Double
  quotes in `git commit -m @'...'@` split the argument in PowerShell 5.1.
- **Listener tests** use the real Raw Input listener: they inject F24 (no keyboard has it) and
  share the `HOOK_TESTS` mutex in `src/soundboard/hotkeys.rs`. A test that injects keys must
  wait for each press to land and settle (~200 ms) before it ends, or the events leak into the
  next listener test.
- **Listener tests can flake** when the user types during `cargo test`. Re-run once before you
  treat a failure as a regression.
- **egui's default fonts have no arrow glyphs** (→ shows as a box). `»`, `★`, `—` and `…` are
  fine. Icons come from the Phosphor font in `src/ui/icons.rs`.
- **No Python on this machine.** Use the Edit tool, bash/sed, or PowerShell for scripting.
- **Other projects:** `..\loomtone` and `..\ytlocal` are separate projects. Don't read, run or
  change anything outside this folder.

## Conventions

- Real-time rule: audio callbacks never lock, allocate, log or do I/O. Use atomics in
  `audio::shared::Shared`, the `EngineCmd` queue, and the garbage queue (see CODEMAP).
- Every new config field must load from old files: `#[serde(default)]` on the struct, plus a
  round-trip or "older file" test in `src/config/mod.rs`.
- Doc comments are written for the user's point of view (what it does, why) in plain
  British-leaning English ("colour", "favourites"). Keep that style.
- Tests live next to the code (`#[cfg(test)] mod tests`). UI behaviour is tested by running
  real egui frames headlessly (`test_app()` / `frame()` in `src/ui/mod.rs`).
- Don't commit unless asked. The repo is public on GitHub.
