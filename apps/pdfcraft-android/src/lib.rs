//! PdfCraft on Android, aimed at tablets with a keyboard and a stylus.
//!
//! Android starts the app through `NativeActivity` (no Java), which loads this library and calls
//! [`android_main`]. The UI is the desktop one (`pdfcraft-ui-egui`); what differs on Android:
//! - Files: the picker is drawn in the app (`pdfcraft_ui_egui`'s `android_picker`) over shared
//!   storage, which needs "All files access" granted once in Settings.
//! - Settings, logs and crash recovery live in the app's internal data folder (`HOME` below).
//! - No system clipboard for images, no printing to a printer, no update check.
//!
//! Elsewhere the crate is empty, so `cargo build --workspace` on a desktop still works.
#![cfg(target_os = "android")]

use pdfcraft_ui_egui::PdfCraftApp;
use winit::platform::android::activity::AndroidApp;

/// Entry point, called by android-activity on its own thread once the activity is created.
#[allow(unsafe_code)] // Exported by name: android-activity looks this symbol up.
#[unsafe(no_mangle)]
fn android_main(android: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("PdfCraft"));
    std::panic::set_hook(Box::new(|info| {
        let trace = std::backtrace::Backtrace::force_capture();
        log::error!("internal error: {info}\n{trace}");
    }));

    let data = android.internal_data_path();
    if let Some(dir) = &data {
        // The recovery store and the picker's "App folder" derive from HOME, which Android
        // doesn't set. Nothing else runs yet: this is the app's only thread besides Java's.
        #[allow(unsafe_code)]
        // SAFETY: called before eframe or the app start any thread that could read the environment.
        unsafe {
            std::env::set_var("HOME", dir);
        }
        log::info!("PdfCraft {} data folder {}", env!("CARGO_PKG_VERSION"), dir.display());
    }

    let options = eframe::NativeOptions {
        android_app: Some(android),
        persistence_path: data.map(|d| d.join("app.ron")),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    let result = eframe::run_native(
        "PdfCraft",
        options,
        Box::new(|cc| {
            let mut app = PdfCraftApp::new();
            // "printcraft": settings saved under the app's former name, PrintCraft.
            if let Some(json) = cc.storage.and_then(|s| s.get_string("pdfcraft").or_else(|| s.get_string("printcraft"))) {
                app.restore(&json);
            }
            // Autosave unsaved changes; Android may stop a backgrounded app at any time.
            if let Some(dir) = pdfcraft_ui_egui::RecoveryStore::default_dir() {
                app.enable_recovery(pdfcraft_ui_egui::RecoveryStore::new(dir));
            }
            if let Some(state) = &cc.wgpu_render_state {
                let info = state.adapter.get_info();
                log::info!("renderer: wgpu on {} ({:?}, {:?})", info.name, info.device_type, info.backend);
            }
            app.reopen_last_files(&[]);
            Ok(Box::new(app))
        }),
    );
    if let Err(e) = result {
        log::error!("PdfCraft stopped: {e}");
    }
}
