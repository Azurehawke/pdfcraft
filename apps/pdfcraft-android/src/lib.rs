//! PdfCraft on Android, aimed at tablets with a keyboard and a stylus.
//!
//! Android starts the app through `PdfCraftActivity`, a `NativeActivity` (packaging/android/java:
//! its only Java keeps the latest intent), which loads this library and calls [`android_main`]. The UI is the desktop one (`pdfcraft-ui-egui`); what differs on Android:
//! - Files: the picker is drawn in the app (`pdfcraft_ui_egui`'s `android_picker`) over shared
//!   storage, which needs "All files access" granted once in Settings.
//! - Settings, logs and crash recovery live in the app's internal data folder (`HOME` below).
//! - Google Drive and other document apps: the file browser's button opens Android's picker;
//!   PdfCraft works on a copy and writes saves back (`cloud`).
//! - PDFs from other apps ("Open with", PdfCraft as the default PDF viewer) open in a tab
//!   (`incoming`).
//! - No system clipboard for images, no printing to a printer.
//! - Updates: RevuCraft asks GitHub for its latest build at start (`updates`).
//!
//! Elsewhere the crate is empty, so `cargo build --workspace` on a desktop still works.
#![cfg(target_os = "android")]

mod cloud;
mod file_access;
mod incoming;
mod tablet;
mod updates;

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

    file_access::set_app(android.clone());
    pdfcraft_ui_egui::set_android_file_access(pdfcraft_ui_egui::AndroidFileAccess { granted: file_access::granted, request: file_access::request });
    // Google Drive and other document apps, through Android's picker (`cloud`).
    cloud::load();
    pdfcraft_ui_egui::set_android_documents(pdfcraft_ui_egui::AndroidDocuments { start: cloud::start, poll: cloud::poll, refetch: cloud::refetch });
    log::info!("All files access: {}", if file_access::granted() { "granted" } else { "not granted" });

    // Only the S Pen (or a mouse) draws on the page; fingers pan it (`tablet`).
    pdfcraft_ui_egui::pen::set_pen_only_drawing(true);
    let shell_android = android.clone();

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
            app.update_source = Some(std::sync::Arc::new(updates::latest_release));
            app.revucraft_shell();
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
            Ok(Box::new(tablet::TabletApp::new(app, shell_android)))
        }),
    );
    if let Err(e) = result {
        log::error!("PdfCraft stopped: {e}");
    }
}
