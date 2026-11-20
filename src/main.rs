//! VoxShift: a local real-time voice changer and soundboard. Your mic goes in, the changed
//! voice comes out of a virtual audio cable (VB-CABLE) that other apps use as their mic.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Off Windows the shell stand-ins (shell/other.rs) leave the tray and Raw Input paths unused;
// the Windows build still reports dead code as usual.
#![cfg_attr(not(windows), allow(dead_code))]

mod audio;
mod config;
mod dsp;
mod keymap;
mod keynames;
mod melody;
mod shell;
mod soundboard;
mod ui;
mod voice_keys;

/// `voxshift --list-devices`: prints what cpal sees (no streams are opened).
fn list_devices() {
    use cpal::traits::HostTrait;
    let host = cpal::default_host();
    let outputs: Vec<String> = host
        .output_devices()
        .map(|d| d.map(|d| d.to_string()).collect())
        .unwrap_or_default();
    println!("Inputs:");
    for d in host.input_devices().into_iter().flatten() {
        println!("  {d}");
    }
    println!("Outputs:");
    for d in &outputs {
        println!("  {d}");
    }
    match audio::matching::find_cable_input(&outputs) {
        Some(c) => println!("VB-CABLE: {c}"),
        None => println!("VB-CABLE: not found"),
    }
}

/// `voxshift --selftest`: runs the real device manager for 8 s without a GUI and prints its
/// status. The mic mode is "force fallback" with no fallback set, so the default mic is opened
/// and metered but nothing audible is sent to the output.
///
/// `voxshift --selftest switch`: primary = first input, fallback = second input, auto mode,
/// gains at -24 dB in and out (practically inaudible). At 4 s the primary is "unplugged" (its
/// name is changed to one that does not exist), at 8 s it comes back.
fn selftest(mode: Option<String>) {
    use audio::device_manager::{DeviceSettings, ManagerCmd, ManagerHandle};
    use audio::mic_select::{MicMode, MicSettings};
    use cpal::traits::HostTrait;
    use dsp::params::P;
    use std::sync::Arc;
    let switch = mode.as_deref() == Some("switch");
    let host = cpal::default_host();
    let outputs: Vec<String> = host
        .output_devices()
        .map(|d| d.map(|d| d.to_string()).collect())
        .unwrap_or_default();
    let inputs: Vec<String> = host
        .input_devices()
        .map(|d| d.map(|d| d.to_string()).collect())
        .unwrap_or_default();
    let output = audio::matching::find_cable_input(&outputs)
        .cloned()
        .or_else(|| host.default_output_device().map(|d| d.to_string()))
        .unwrap_or_default();
    let mut devices = DeviceSettings {
        output,
        primary: host
            .default_input_device()
            .map(|d| d.to_string())
            .unwrap_or_default(),
        ..Default::default()
    };
    let mut mic = MicSettings {
        mode: MicMode::ForceFallback,
        ..Default::default()
    };
    let shared = Arc::new(audio::shared::Shared::default());
    if switch {
        if inputs.len() < 2 {
            println!("need two input devices");
            return;
        }
        devices.primary = inputs[0].clone();
        devices.fallback = inputs[1].clone();
        mic.mode = MicMode::Auto;
        shared.params.set(P::InputGain, -24.0);
        shared.params.set(P::OutputGain, -24.0);
    }
    println!(
        "output: {}\nprimary: {}\nfallback: {}",
        devices.output, devices.primary, devices.fallback
    );
    let manager = ManagerHandle::spawn(shared.clone(), devices.clone(), mic);
    let seconds = if switch { 12 } else { 8 };
    for second in 1..=seconds {
        std::thread::sleep(std::time::Duration::from_secs(1));
        if switch && second == 4 {
            println!("-- unplugging primary");
            let mut d = devices.clone();
            d.primary = "Microphone (Not Connected)".into();
            manager.send(ManagerCmd::Devices(d));
        }
        if switch && second == 8 {
            println!("-- primary back");
            manager.send(ManagerCmd::Devices(devices.clone()));
        }
        let s = manager.status();
        let db = |i: usize| audio::shared::to_db(shared.mic_level[i].get());
        println!(
            "{second:>2}s out={:?} {}Hz/{} | primary={:?} {:.0}dB | fallback={:?} {:.0}dB | active={:?} '{}' in={:.0}dB latency={:?}",
            s.output.state,
            s.output.rate,
            s.output.period,
            s.mics[0].state,
            db(0),
            s.mics[1].state,
            db(1),
            s.active,
            s.reason,
            audio::shared::to_db(shared.input_level.get()),
            s.latency_ms.map(|ms| ms.round()),
        );
        while let Ok(ev) = manager.events.try_recv() {
            let audio::device_manager::Event::Switched { to, reason } = ev;
            println!("   event: switched to {to:?} ({reason})");
        }
    }
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--list-devices") {
        list_devices();
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--selftest") {
        selftest(args.get(i + 1).cloned());
        return Ok(());
    }
    // Only one copy: two would fight over the virtual cable. A second launch brings the
    // running one (possibly hidden in the tray) to the front instead.
    let _instance = match shell::instance::acquire(shell::instance::MUTEX_NAME) {
        shell::instance::Instance::First(guard) => guard,
        shell::instance::Instance::AlreadyRunning => {
            shell::instance::signal_existing();
            return Ok(());
        }
    };
    // `--config <file>`: use another settings file (a second profile, or testing without
    // touching your real settings).
    let config_path = args
        .iter()
        .position(|a| a == "--config")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(config::config_path);
    let (cfg, notice) = config::load_from(&config_path);
    let accent = ui::theme::accent_of(&cfg.accent);
    let icon = egui::IconData {
        rgba: shell::icon::render(shell::Health::Ok, [accent.r(), accent.g(), accent.b()], 64),
        width: 64,
        height: 64,
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(concat!("VoxShift v", env!("CARGO_PKG_VERSION")))
        .with_icon(icon)
        .with_inner_size(ui::WINDOW_SIZE)
        .with_min_inner_size([920.0, 600.0]);
    // Where it was last time, if that place is still on a screen (a monitor may be gone).
    let placement = cfg
        .window
        .filter(|w| shell::virtual_screen().is_some_and(|s| w.fits(s)));
    if let Some(w) = placement {
        viewport = viewport
            .with_inner_size([w.width, w.height])
            .with_maximized(w.maximized);
    }
    if cfg.start_minimized {
        // eframe always shows the window once its first frame is painted; the app hides it
        // right after. Off-screen and without focus, that moment is invisible.
        viewport = viewport.with_position(ui::OFFSCREEN).with_active(false);
    } else if let Some(w) = placement {
        viewport = viewport.with_position([w.x, w.y]);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "VoxShift",
        options,
        Box::new(move |cc| {
            ui::theme::apply(&cc.egui_ctx, cfg.theme);
            ui::theme::set_accent(&cc.egui_ctx, accent);
            cc.egui_ctx.set_zoom_factor(cfg.ui_scale());
            let app = ui::VoxApp::new(config_path, cfg, notice, cc.egui_ctx.clone());
            Ok(Box::new(app))
        }),
    )
}
