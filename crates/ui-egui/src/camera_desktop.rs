//! RevuCraft on Windows: photo pins and callouts taken with the Camera app.
//!
//! Windows lets a desktop app open the Camera app but not ask it for one photo (Android's camera
//! intent), so RevuCraft opens it, then watches the folders it saves to (Camera Roll) for a new
//! photo. The first one written after the Camera app opened, once its size has stopped changing,
//! is the photo; RevuCraft comes back to the front and places it. The Camera app uses the
//! Surface's rear camera at full resolution, with its own focus and exposure.

// Only Windows has a Camera app to watch; the helpers are tested everywhere.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// What a saved photo is called; anything else in the folder (videos, the Camera app's own
/// files) is left alone.
const PHOTO_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png"];
/// Files looked at per folder, at most (a Camera Roll of years of photos is still quick).
const MAX_ENTRIES: usize = 20_000;
/// A clock a little behind the file system's still counts a photo as new.
const SLACK: std::time::Duration = std::time::Duration::from_secs(2);

/// `%NAME%` references replaced with the environment's values (a registry folder path such as
/// `%USERPROFILE%\Pictures\Camera Roll`); unknown names are left as they are.
pub(crate) fn expand(s: &str, var: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        let (before, after) = rest.split_at(start);
        out.push_str(before);
        let after = after.get(1..).unwrap_or_default();
        match after.find('%') {
            Some(end) => {
                let name = after.get(..end).unwrap_or_default();
                match var(name).filter(|_| !name.is_empty()) {
                    Some(v) => out.push_str(&v),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = after.get(end + 1..).unwrap_or_default();
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Where the Camera app saves photos: the Camera Roll folder Windows records (moved, or in
/// OneDrive), then the usual places.
pub(crate) fn camera_folders() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    #[cfg(target_os = "windows")]
    out.extend(registry_camera_roll());
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    if let Some(home) = env("USERPROFILE").or_else(|| env("HOME")) {
        out.push(home.join("Pictures").join("Camera Roll"));
    }
    if let Some(one_drive) = env("OneDrive") {
        out.push(one_drive.join("Pictures").join("Camera Roll"));
    }
    let mut seen = Vec::new();
    out.retain(|p| {
        let key = p.to_string_lossy().to_lowercase();
        if seen.contains(&key) {
            false
        } else {
            seen.push(key);
            true
        }
    });
    out
}

/// The Camera Roll folder from `HKCU\…\Explorer\User Shell Folders` (FOLDERID_CameraRoll).
#[cfg(target_os = "windows")]
fn registry_camera_roll() -> Option<PathBuf> {
    use winsafe::{HKEY, RegistryValue, co::RRF};
    let value = HKEY::CURRENT_USER
        .RegGetValue(
            Some("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\User Shell Folders"),
            Some("{AB5FB87B-7CE2-4F83-915D-550846C9537B}"),
            RRF::RT_ANY,
        )
        .ok()?;
    let path = match value {
        RegistryValue::Sz(s) | RegistryValue::ExpandSz(s) => s,
        _ => return None,
    };
    let path = expand(&path, |name| std::env::var(name).ok());
    (!path.trim().is_empty()).then(|| PathBuf::from(path))
}

/// Open the Camera app. Errors when Windows couldn't start it.
pub(crate) fn open_camera_app() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer.exe").arg("microsoft.windows.camera:").spawn().map(|_| ()).map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("there is no Camera app to open here".into())
    }
}

/// The newest photo in `dirs` written since `since` (less a little slack), with its size.
pub(crate) fn newest_photo(dirs: &[PathBuf], since: SystemTime) -> Option<(PathBuf, u64)> {
    let since = since.checked_sub(SLACK).unwrap_or(since);
    let mut best: Option<(SystemTime, PathBuf, u64)> = None;
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.take(MAX_ENTRIES).flatten() {
            let path = entry.path();
            if !is_photo(&path) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let Ok(written) = meta.modified() else { continue };
            if written >= since && best.as_ref().is_none_or(|(t, ..)| written > *t) {
                best = Some((written, path, meta.len()));
            }
        }
    }
    best.map(|(_, p, n)| (p, n))
}

fn is_photo(path: &Path) -> bool {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).is_some_and(|e| PHOTO_EXTENSIONS.contains(&e.as_str()))
}

/// A photo is ready once two looks a moment apart find the same file at the same, non-zero size
/// (the Camera app has finished writing it).
pub(crate) fn settled(before: Option<&(PathBuf, u64)>, now: Option<&(PathBuf, u64)>) -> bool {
    matches!((before, now), (Some((a, n)), Some((b, m))) if a == b && n == m && *m > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_paths_take_their_variables() {
        let var = |n: &str| match n {
            "USERPROFILE" => Some("C:\\Users\\Steve".to_string()),
            _ => None,
        };
        assert_eq!(expand("%USERPROFILE%\\Pictures\\Camera Roll", var), "C:\\Users\\Steve\\Pictures\\Camera Roll");
        assert_eq!(expand("%NOPE%\\x", var), "%NOPE%\\x");
        assert_eq!(expand("50% off %", var), "50% off %");
        assert_eq!(expand("%%", var), "%%");
        assert_eq!(expand("plain", var), "plain");
    }

    #[test]
    fn the_newest_photo_since_the_camera_opened_is_the_one() {
        let dir = std::env::temp_dir().join(format!("revucraft-camera-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let since = SystemTime::now();
        std::fs::write(dir.join("WIN_1.jpg"), b"photo").unwrap();
        std::fs::write(dir.join("WIN_2.mp4"), b"video").unwrap();
        std::fs::write(dir.join("notes.txt"), b"text").unwrap();
        let found = newest_photo(std::slice::from_ref(&dir), since);
        assert_eq!(found.as_ref().map(|(p, n)| (p.file_name().unwrap().to_owned(), *n)), Some(("WIN_1.jpg".into(), 5)));
        // Older than the camera (by more than the slack): not it.
        assert!(newest_photo(std::slice::from_ref(&dir), since + std::time::Duration::from_secs(60)).is_none());
        // A missing folder is passed over.
        assert!(newest_photo(&[dir.join("missing")], since).is_none());
        // Ready once it stops growing.
        let a = (dir.join("WIN_1.jpg"), 5);
        assert!(!settled(None, Some(&a)));
        assert!(!settled(Some(&(a.0.clone(), 3)), Some(&a)));
        assert!(settled(Some(&a), Some(&a)));
        assert!(!settled(Some(&(a.0.clone(), 0)), Some(&(a.0.clone(), 0))));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
