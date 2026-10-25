//! Scan & OCR's models on Android.
//!
//! The desktop packages install the two recognition models beside the executable, where
//! `pdfcraft_ocr::Models::find` looks. An APK has no such folder: build-apk.sh puts the models
//! (with their CC BY-SA licences and credits) in the APK's `assets/models/`, and this copies them
//! into the app's own folder, once per app version, so that `PDFCRAFT_MODELS` can point there.

use std::ffi::CString;
use std::io::Read;
use std::path::{Path, PathBuf};

use winit::platform::android::activity::AndroidApp;

/// The files under `assets/models/`, as `cargo xtask models` writes them.
const FILES: &[&str] =
    &["text-detection.rten", "text-recognition.rten", "text-detection.rten.LICENCE.txt", "text-recognition.rten.LICENCE.txt", "ATTRIBUTION.txt"];
/// Larger assets are refused (the models are a few megabytes each).
const MAX_BYTES: u64 = 256 * 1024 * 1024;

/// Copy the models out of the APK into `<data>/models` when they aren't there yet (or came from
/// another build), and return that folder. `None` when the APK has no models.
pub fn install(android: &AndroidApp, data: &Path) -> Option<PathBuf> {
    let dir = data.join("models");
    let stamp = dir.join(".version");
    let version = env!("CARGO_PKG_VERSION");
    let installed = std::fs::read_to_string(&stamp).is_ok_and(|v| v.trim() == version) && pdfcraft_ocr::Models::in_dir(&dir).is_some();
    if installed {
        return Some(dir);
    }
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("OCR models: can't create {}: {e}", dir.display());
        return None;
    }
    let assets = android.asset_manager();
    let mut copied = 0;
    for name in FILES {
        let Ok(path) = CString::new(format!("models/{name}")) else { continue };
        let Some(mut asset) = assets.open(&path) else { continue };
        if u64::try_from(asset.length()).unwrap_or(u64::MAX) > MAX_BYTES {
            log::warn!("OCR models: {name} is too large");
            continue;
        }
        let mut bytes = Vec::new();
        if let Err(e) = asset.read_to_end(&mut bytes) {
            log::warn!("OCR models: reading {name}: {e}");
            continue;
        }
        // Through a temporary file, so an interrupted copy never looks like a model.
        let tmp = dir.join(format!("{name}.part"));
        match std::fs::write(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, dir.join(name))) {
            Ok(()) => copied += 1,
            Err(e) => log::warn!("OCR models: writing {name}: {e}"),
        }
    }
    if pdfcraft_ocr::Models::in_dir(&dir).is_none() {
        log::warn!("OCR models: not in this APK ({copied} files copied)");
        return None;
    }
    // Best effort: without the stamp the models are copied again at the next start.
    let _ = std::fs::write(&stamp, version);
    log::info!("OCR models copied to {}", dir.display());
    Some(dir)
}
