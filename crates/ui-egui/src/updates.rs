//! Help ▸ Check for updates: ask for RevuCraft's latest build and offer its download.
//!
//! The app supplies how to ask ([`PdfCraftApp::update_source`]), so this crate has no network
//! code; without a source (the web build, tests) the command opens the releases page. RevuCraft
//! builds are published by CI as the `revucraft-latest` release, numbered by build; a build knows
//! its own number from `REVUCRAFT_BUILD` at compile time. RevuCraft checks once, quietly, at
//! start and only speaks up when a newer build exists. It never installs anything itself: the
//! download goes to the browser, and Android asks before installing.

use std::sync::Arc;

use egui::{Align, Layout};

use crate::{PdfCraftApp, theme, widgets};

/// Where every RevuCraft build is listed.
pub const RELEASES_PAGE: &str = "https://github.com/Azurehawke/pdfcraft/releases";

/// GitHub's API answer for the latest RevuCraft build for Android.
pub const LATEST_API: &str = "https://api.github.com/repos/Azurehawke/pdfcraft/releases/tags/revucraft-latest";

/// GitHub's API answer for the latest RevuCraft build for Windows (`revucraft-windows-latest`,
/// numbered by its own workflow's builds).
pub const LATEST_WINDOWS_API: &str = "https://api.github.com/repos/Azurehawke/pdfcraft/releases/tags/revucraft-windows-latest";

/// This build's number (`REVUCRAFT_BUILD`, set by CI), or the crate version for a local build.
pub fn current_version() -> &'static str {
    option_env!("REVUCRAFT_BUILD").filter(|b| !b.is_empty()).unwrap_or(env!("CARGO_PKG_VERSION"))
}

fn is_build_number(v: &str) -> bool {
    !v.is_empty() && v.len() <= 9 && v.bytes().all(|b| b.is_ascii_digit())
}

/// Whether `latest` is newer than this build. Build numbers compare with build numbers; a local
/// build (no number) is never told a CI build is newer.
pub fn update_available(latest: &str) -> bool {
    let current = current_version();
    match (is_build_number(latest), is_build_number(current)) {
        (true, true) => latest.parse::<u64>().ok() > current.parse::<u64>().ok(),
        (true, false) => false,
        _ => is_newer(latest, current),
    }
}

/// A version as shown: "build 12" for a build number, the version otherwise.
pub fn display_version(v: &str) -> String {
    let v = v.trim_start_matches(['v', 'V']);
    if is_build_number(v) { format!("build {v}") } else { v.to_string() }
}

/// Read GitHub's answer for the `revucraft-latest` release: the build number from its title
/// ("RevuCraft for Android (build 12)") and where to get it: the APK itself when `apk`, else the
/// release page. The answer is untrusted: only addresses under [`RELEASES_PAGE`] are offered.
pub fn parse_latest(body: &str, apk: bool) -> Result<Release, String> {
    parse_latest_asset(body, apk.then_some(".apk"))
}

/// [`parse_latest`], offering the release's first asset whose name ends with `suffix` (".apk",
/// ".msi") when there is one, else the release page.
pub fn parse_latest_asset(body: &str, suffix: Option<&str>) -> Result<Release, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("unreadable answer ({e})"))?;
    let name = v["name"].as_str().unwrap_or_default();
    let version = name
        .split_once("(build ")
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(n, _)| n.trim())
        .filter(|n| is_build_number(n))
        .ok_or("no RevuCraft build found")?
        .to_string();
    let ours = |u: &&str| u.strip_prefix(RELEASES_PAGE).is_some_and(|rest| rest.starts_with('/') && !rest.contains(['?', '#', '\\']));
    let asset = suffix.and_then(|suffix| {
        v["assets"].as_array().into_iter().flatten().filter_map(|a| a["browser_download_url"].as_str()).find(|u| ours(u) && u.ends_with(suffix))
    });
    let url = asset.or_else(|| v["html_url"].as_str().filter(ours)).unwrap_or(RELEASES_PAGE).to_string();
    Ok(Release { version, url })
}

/// The latest published release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// Its version tag, such as `v0.2.0`.
    pub version: String,
    /// Its page on [`RELEASES_PAGE`], where the downloads are.
    pub url: String,
}

/// Asks for the latest release (blocking; it runs on its own thread).
pub type UpdateSource = Arc<dyn Fn() -> Result<Release, String> + Send + Sync>;

/// Whether release `latest` (a tag such as `v0.2.0`) is newer than version `current` (`0.1.1`).
/// Pre-release and build suffixes are ignored; a version that doesn't parse is never newer.
pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse(latest), parse(current)), (Some(l), Some(c)) if l > c)
}

fn parse(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let core = v.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let mut next = |required: bool| match parts.next() {
        Some(p) => p.parse::<u64>().ok(),
        None if required => None,
        None => Some(0),
    };
    let version = (next(true)?, next(false)?, next(false)?);
    parts.next().is_none().then_some(version)
}

/// Where a check is.
#[derive(Default)]
pub(crate) enum Check {
    #[default]
    Idle,
    #[cfg(not(target_arch = "wasm32"))]
    Running(std::sync::mpsc::Receiver<Result<Release, String>>),
    Done(Result<Release, String>),
}

#[derive(Default)]
pub(crate) struct Updates {
    pub(crate) check: Check,
    /// The running check is the quiet one at start: show the dialog only for a newer build.
    pub(crate) quiet: bool,
    /// The Updates dialog is showing.
    pub(crate) open: bool,
}

impl PdfCraftApp {
    /// Help ▸ Check for updates: ask for the latest release and show the outcome.
    pub fn check_for_updates(&mut self) {
        let Some(source) = self.update_source.clone() else {
            self.open_url(RELEASES_PAGE);
            return;
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.updates.open = true;
            if matches!(self.updates.check, Check::Running(_)) {
                return;
            }
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = self.ctx.clone();
            std::thread::spawn(move || {
                // The receiver may be gone (the app quit): nothing to report to then.
                let _ = tx.send(source());
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            });
            self.updates.check = Check::Running(rx);
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = source;
            self.open_url(RELEASES_PAGE);
        }
    }

    /// The check at start: like Help ▸ Check for updates, but the dialog only opens when a
    /// newer build exists; an up-to-date answer or a failure stays silent.
    pub fn check_for_updates_quietly(&mut self) {
        if self.update_source.is_none() || !matches!(self.updates.check, Check::Idle) {
            return;
        }
        self.check_for_updates();
        self.updates.open = false;
        self.updates.quiet = true;
    }

    /// Pick up a finished check (each frame).
    pub(crate) fn poll_updates(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Check::Running(rx) = &self.updates.check {
            let result = match rx.try_recv() {
                Ok(r) => r,
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("the update check stopped unexpectedly".into()),
            };
            if std::mem::take(&mut self.updates.quiet) && result.as_ref().is_ok_and(|r| update_available(&r.version)) {
                self.updates.open = true;
            }
            self.updates.check = Check::Done(result);
        }
    }
}

/// The Updates dialog.
pub(crate) fn dialog(app: &mut PdfCraftApp, ctx: &egui::Context) {
    if !app.updates.open {
        return;
    }
    let t = theme::Tokens::get(ctx);
    let current = display_version(current_version());
    let current = current.as_str();
    let mut close = false;
    let mut download: Option<String> = None;
    let modal = egui::Modal::new(egui::Id::new("updates")).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.horizontal(|ui| {
            ui.add(crate::icons::image("cloud", 22.0, t.accent));
            ui.label(egui::RichText::new(tl!("Check for updates")).font(theme::semibold(16.0)));
        });
        ui.add_space(8.0);
        match &app.updates.check {
            Check::Idle => {
                ui.label(tl!("No check has run yet."));
            }
            #[cfg(not(target_arch = "wasm32"))]
            Check::Running(_) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tl!("Checking for a newer version…"));
                });
            }
            Check::Done(Ok(r)) if update_available(&r.version) => {
                let version = display_version(&r.version);
                ui.label(egui::RichText::new(crate::i18n::fmt(tl!("RevuCraft {v} is available."), &[("v", &version)])).strong());
                ui.label(
                    egui::RichText::new(crate::i18n::fmt(
                        tl!("You have version {c}. Download the new version from its release page."),
                        &[("c", current)],
                    ))
                    .color(t.text_muted),
                );
                download = Some(r.url.clone());
            }
            Check::Done(Ok(_)) => {
                ui.label(crate::i18n::fmt(tl!("RevuCraft {c} is up to date."), &[("c", current)]));
            }
            Check::Done(Err(e)) => {
                ui.label(crate::i18n::fmt(tl!("Couldn't check for updates: {e}"), &[("e", &e.to_string())]));
                ui.label(
                    egui::RichText::new(crate::i18n::fmt(
                        tl!("You have version {c}. All releases are listed at {page}."),
                        &[("c", current), ("page", RELEASES_PAGE)],
                    ))
                    .color(t.text_muted),
                );
            }
        }
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(tl!("Asks GitHub for the latest release. Nothing is downloaded or installed automatically."))
                .color(t.text_muted)
                .small(),
        );
        ui.add_space(12.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(url) = download.take() {
                let get = widgets::pill_button(ui, tl!("Download"), true).clicked();
                let later = widgets::pill_button(ui, tl!("Later"), false).clicked();
                close = get || later;
                download = get.then_some(url);
            } else if widgets::pill_button(ui, tl!("Close"), true).clicked() {
                close = true;
            }
        });
    });
    if modal.should_close() {
        close = true;
        download = None;
    }
    if close {
        app.updates.open = false;
        if let Some(url) = download {
            app.open_url(&url);
        }
    }
}
