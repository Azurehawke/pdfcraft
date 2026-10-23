//! Help ▸ Check for updates (issue #28), with stand-in release sources (no network).

use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::PdfCraftApp;
use pdfcraft_ui_egui::updates::{Release, UpdateSource, is_newer};

fn source(answer: Result<&str, &str>) -> UpdateSource {
    let answer = answer.map(str::to_string).map_err(str::to_string);
    Arc::new(move || answer.clone().map(|v| Release { url: format!("https://github.com/storytold/pdfcraft/releases/tag/{v}"), version: v }))
}

fn harness(answer: Result<&str, &str>) -> Harness<'static, PdfCraftApp> {
    Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.update_source = Some(source(answer));
        app
    })
}

/// Run frames until the background check has reported. By what the dialog shows, with the
/// same 30-second deadline as `support::wait_until`, not for a fixed second: under load the
/// check's thread can take longer (#1277).
fn settle(h: &mut Harness<'static, PdfCraftApp>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        h.run_steps(2);
        if h.query_by_label_contains("Checking for a newer version").is_none() {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "timed out waiting for the update check");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.run_steps(2);
}

#[test]
fn versions_compare_by_number() {
    assert!(is_newer("v0.2.0", "0.1.1"));
    assert!(is_newer("v0.1.10", "0.1.9"));
    assert!(is_newer("1", "0.9.9"));
    assert!(!is_newer("v0.1.1", "0.1.1"));
    assert!(!is_newer("v0.1.0", "0.1.1"));
    assert!(!is_newer("v0.1.1-beta.2", "0.1.1"), "suffixes are ignored");
    assert!(!is_newer("nightly", "0.1.1"), "a tag that isn't a version is never newer");
    assert!(!is_newer("v1.2.3.4", "0.1.1"));
    assert!(!is_newer("v99999999999999999999.0.0", "0.1.1"), "out of range");
}

#[test]
fn a_newer_release_is_offered_for_download() {
    let mut h = harness(Ok("v99.0.0"));
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label_contains("RevuCraft 99.0.0 is available");
    h.get_by_label("Download");
    h.get_by_label("Later").click();
    h.run_steps(3);
    assert!(h.query_by_label_contains("is available").is_none(), "Later closes the dialog");
}

#[test]
fn an_up_to_date_or_failed_check_says_so() {
    let mut h = harness(Ok(concat!("v", env!("CARGO_PKG_VERSION"))));
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label_contains("is up to date");
    assert!(h.query_by_label("Download").is_none());

    let mut h = harness(Err("couldn't reach GitHub"));
    h.state_mut().execute("help.check_updates");
    settle(&mut h);
    h.get_by_label_contains("Couldn't check for updates: couldn't reach GitHub");
}

#[test]
fn the_start_check_is_quiet_unless_a_newer_build_exists() {
    // One quiet check at start: an up-to-date answer opens nothing.
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = calls.clone();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        let counted = counted.clone();
        app.update_source = Some(Arc::new(move || {
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(Release { version: concat!("v", env!("CARGO_PKG_VERSION")).into(), url: pdfcraft_ui_egui::updates::RELEASES_PAGE.into() })
        }));
        app
    });
    settle(&mut h);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(h.query_by_label_contains("is up to date").is_none(), "nothing to say, so nothing shown");

    // A newer build opens the dialog by itself.
    let mut h = harness(Ok("v99.0.0"));
    settle(&mut h);
    h.get_by_label_contains("RevuCraft 99.0.0 is available");
}

#[test]
fn builds_compare_by_number_and_releases_are_read_from_their_title() {
    use pdfcraft_ui_egui::updates::{RELEASES_PAGE, display_version, parse_latest};
    assert_eq!(display_version("12"), "build 12");
    assert_eq!(display_version("v0.5.0"), "0.5.0");
    let body = r#"{"name":"RevuCraft for Android (build 12)","html_url":"https://github.com/Azurehawke/pdfcraft/releases/tag/revucraft-latest",
        "assets":[{"browser_download_url":"https://example.com/evil.apk"},{"browser_download_url":"https://github.com/Azurehawke/pdfcraft/releases/download/revucraft-latest/revucraft.apk"}]}"#;
    let apk = parse_latest(body, true).unwrap();
    assert_eq!(apk.version, "12");
    assert_eq!(apk.url, "https://github.com/Azurehawke/pdfcraft/releases/download/revucraft-latest/revucraft.apk");
    assert_eq!(parse_latest(body, false).unwrap().url, "https://github.com/Azurehawke/pdfcraft/releases/tag/revucraft-latest");
    let elsewhere = r#"{"name":"RevuCraft for Android (build 3)","html_url":"https://example.com/x","assets":[{"browser_download_url":"https://example.com/x.apk"}]}"#;
    assert_eq!(parse_latest(elsewhere, true).unwrap().url, RELEASES_PAGE);
    assert_eq!(parse_latest(elsewhere, false).unwrap().url, RELEASES_PAGE);
    for bad in [r#"{"name":"RevuCraft"}"#, r#"{"name":"(build x)"}"#, r#"{"message":"Not Found"}"#, "<html>"] {
        assert!(parse_latest(bad, true).is_err(), "{bad}");
    }
}
