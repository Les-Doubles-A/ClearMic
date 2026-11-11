#![cfg_attr(test, allow(clippy::field_reassign_with_default))]
// Release builds are GUI-subsystem: no console window when launching the .exe.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod autostart;
mod config;
mod devices;
mod hotkeys;
mod lowlat;
mod sound;
mod tray;
mod ui;

/// GUI-subsystem builds have no stdout; attach to the parent terminal so the
/// diagnostic flags still print when launched from cmd/PowerShell.
#[cfg(all(windows, not(debug_assertions)))]
fn attach_parent_console() {
    use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // Fails harmlessly when there is no parent console (double-click launch).
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

#[cfg(not(all(windows, not(debug_assertions))))]
fn attach_parent_console() {}

fn main() -> eframe::Result {
    if std::env::args().any(|a| a.starts_with("--") && a != autostart::MINIMIZED_FLAG) {
        attach_parent_console();
    }
    if std::env::args().any(|a| a == "--list-devices") {
        devices::print_report();
        return Ok(());
    }
    if std::env::args().any(|a| a == "--probe-engine") {
        audio::probe(10);
        return Ok(());
    }
    if std::env::args().any(|a| a == "--probe-lowlat") {
        devices::probe_lowlat();
        return Ok(());
    }
    let minimized = std::env::args().any(|a| a == autostart::MINIMIZED_FLAG);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("ClearMic")
            .with_inner_size([620.0, 760.0])
            .with_min_inner_size([480.0, 420.0])
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/ClearMic.png"))
                    .expect("icone ClearMic.png invalide"),
            ),
        ..Default::default()
    };
    eframe::run_native(
        "ClearMic",
        options,
        Box::new(move |cc| {
            let mut app = ui::ClearMicApp::new(minimized);
            match tray::Tray::new(&cc.egui_ctx) {
                Ok(t) => app.set_tray(t),
                Err(e) => eprintln!("Icône de notification indisponible : {e}"),
            }
            Ok(Box::new(app))
        }),
    )
}
