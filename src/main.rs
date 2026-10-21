//! VoxShift: a local real-time voice changer and soundboard. Your mic goes in, the changed
//! voice comes out of a virtual audio cable (VB-CABLE) that other apps use as their mic.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod config;
mod dsp;
mod soundboard;
mod ui;

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

/// `voxshift --selftest [output name]`: runs the real device manager for 8 s without a GUI and
/// prints its status. The mic mode is "force fallback" with no fallback set, so the default mic
/// is opened and metered but nothing audible is sent to the output.
fn selftest(output: Option<String>) {
    use audio::device_manager::{DeviceSettings, ManagerHandle};
    use audio::mic_select::{MicMode, MicSettings};
    use cpal::traits::HostTrait;
    use std::sync::Arc;
    let host = cpal::default_host();
    let outputs: Vec<String> = host
        .output_devices()
        .map(|d| d.map(|d| d.to_string()).collect())
        .unwrap_or_default();
    let output = output
        .or_else(|| audio::matching::find_cable_input(&outputs).cloned())
        .or_else(|| host.default_output_device().map(|d| d.to_string()))
        .unwrap_or_default();
    let devices = DeviceSettings {
        output,
        primary: host.default_input_device().map(|d| d.to_string()).unwrap_or_default(),
        ..Default::default()
    };
    let mic = MicSettings {
        mode: MicMode::ForceFallback,
        ..Default::default()
    };
    println!("output: {}\nprimary: {}", devices.output, devices.primary);
    let shared = Arc::new(audio::shared::Shared::default());
    let manager = ManagerHandle::spawn(shared.clone(), devices, mic);
    for second in 1..=8 {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let s = manager.status();
        println!(
            "{second}s output={:?} {}Hz/{} | primary={:?} {}Hz/{} level={:.0}dB fill={:.1}ms | active={:?} out={:.0}dB",
            s.output.state,
            s.output.rate,
            s.output.period,
            s.mics[0].state,
            s.mics[0].rate,
            s.mics[0].period,
            audio::shared::to_db(shared.mic_level[0].get()),
            shared.mic_fill_ms[0].get(),
            s.active,
            audio::shared::to_db(shared.output_level.get()),
        );
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
    let app = ui::VoxApp::new(config::Config::default());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("VoxShift")
            .with_inner_size([1240.0, 800.0])
            .with_min_inner_size([920.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "VoxShift",
        options,
        Box::new(|cc| {
            ui::theme::apply(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}
