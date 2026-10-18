#![cfg_attr(windows, windows_subsystem = "windows")]

mod dsp;
mod settings;

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod audio;
#[cfg(windows)]
mod winutil;

#[cfg(windows)]
fn main() -> eframe::Result {
    if !winutil::single_instance() {
        return Ok(());
    }
    let hidden = std::env::args().any(|a| a == "--hidden");
    let viewport = eframe::egui::ViewportBuilder::default()
        .with_title("Hush")
        .with_inner_size([440.0, 720.0])
        .with_min_inner_size([400.0, 420.0]);
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("Hush", options, Box::new(|_cc| Ok(Box::new(app::HushApp::new(hidden)))))
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Hush only runs on Windows.");
}
