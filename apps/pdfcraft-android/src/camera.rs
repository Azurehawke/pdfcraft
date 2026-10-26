//! Scan with camera: one photo from the device's camera app (`PdfCraftActivity.pcTakePhoto`),
//! handed to the app as a file in the cache (`pdfcraft_ui_egui::AndroidCamera`).

use std::path::PathBuf;

use crate::file_access::with_activity;
use crate::incoming::string_of;

/// Open the camera app for one photo.
pub fn start() -> Result<(), String> {
    let opened = with_activity("Opening the camera", |env, activity| env.call_method(activity, "pcTakePhoto", "()Z", &[])?.z())?;
    if opened { Ok(()) } else { Err("No camera app can take a photo for PdfCraft.".into()) }
}

/// The photo once the camera app is back: its file, or none when cancelled. `None` while the
/// camera is still showing.
pub fn poll() -> Option<Result<Option<PathBuf>, String>> {
    let r = with_activity("Reading the photo", |env, activity| {
        let s = env.call_method(activity, "pcTakePhotoResult", "()Ljava/lang/String;", &[])?.l()?;
        string_of(env, s)
    });
    match r {
        Ok(None) => None,
        Ok(Some(path)) if path.is_empty() => Some(Ok(None)),
        Ok(Some(path)) => Some(Ok(Some(PathBuf::from(path)))),
        Err(e) => Some(Err(e)),
    }
}
