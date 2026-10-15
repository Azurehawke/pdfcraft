//! Asks GitHub for RevuCraft's latest build (Help ▸ Check for updates, and quietly at start).

use std::time::Duration;

#[cfg(test)]
use pdfcraft_ui_egui::updates::RELEASES_PAGE;
use pdfcraft_ui_egui::updates::{LATEST_WINDOWS_API as LATEST, Release};

/// The latest build. The answer is untrusted: its size is capped, and only a page under the
/// releases list is ever offered for download (anything else falls back to that list).
pub fn latest_release() -> Result<Release, String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .tls_config(ureq::tls::TlsConfig::builder().root_certs(os_roots()?).build())
        .build()
        .new_agent();
    let mut response = agent
        .get(LATEST)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", concat!("RevuCraft/", env!("CARGO_PKG_VERSION")))
        // Read error statuses ourselves: a 403 from GitHub usually means its anonymous
        // rate limit ran out, and the retry time rides along in a response header.
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|e| format!("couldn't reach GitHub ({e})"))?;
    let status = response.status().as_u16();
    if status == 403 {
        let reset = response.headers().get("x-ratelimit-reset").and_then(|v| v.to_str().ok());
        return Err(rate_limited(reset, now_unix()));
    }
    if !(200..300).contains(&status) {
        return Err(format!("couldn't reach GitHub (http status: {status})"));
    }
    let body = response.body_mut().with_config().limit(1 << 20).read_to_string().map_err(|e| format!("unreadable answer ({e})"))?;
    parse(&body)
}

/// The certificate authorities the operating system trusts.
fn os_roots() -> Result<ureq::tls::RootCerts, String> {
    let found = rustls_native_certs::load_native_certs();
    let certs: Vec<ureq::tls::Certificate<'static>> = found.certs.iter().map(|c| ureq::tls::Certificate::from_der(c.as_ref()).to_owned()).collect();
    if certs.is_empty() {
        return Err("no trusted certificates found on this system".into());
    }
    Ok(ureq::tls::RootCerts::new_with_certs(&certs))
}

/// The current Unix time in seconds (0 when the clock gives nothing usable).
fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// What to tell the user when GitHub answers 403: its anonymous quota (60 checks an hour,
/// shared by everyone behind the same address) ran out. `reset` is the `x-ratelimit-reset`
/// header, the Unix time the quota refills; `now` is the current Unix time. Anything
/// surprising (no header, not a number, already past) falls back to asking for patience.
fn rate_limited(reset: Option<&str>, now: u64) -> String {
    const FALLBACK: &str = "GitHub is limiting update checks for now (shared hourly quota); try again later";
    let wait = reset.and_then(|r| r.trim().parse::<u64>().ok()).and_then(|reset| reset.checked_sub(now)).filter(|&wait| wait > 0);
    match wait {
        Some(wait) => {
            let minutes = wait.div_ceil(60).max(1);
            let plural = if minutes == 1 { "" } else { "s" };
            format!("GitHub is limiting update checks for now (shared hourly quota); try again in about {minutes} minute{plural}")
        }
        None => FALLBACK.into(),
    }
}

fn parse(body: &str) -> Result<Release, String> {
    // Windows: offer the installer itself; elsewhere the release page.
    pdfcraft_ui_egui::updates::parse_latest_asset(body, cfg!(windows).then_some(".msi"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_read_and_only_our_release_pages_are_offered() {
        let page = "https://github.com/Azurehawke/pdfcraft/releases/tag/revucraft-latest";
        let r = parse(&format!(r#"{{"name":"RevuCraft for Android (build 7)","html_url":"{page}"}}"#)).unwrap();
        assert_eq!(r, Release { version: "7".into(), url: page.into() });
        for elsewhere in ["https://example.com/pdfcraft.exe", "https://github.com/Azurehawke/pdfcraft/releases.evil/x", "javascript:alert(1)"] {
            let r = parse(&format!(r#"{{"name":"RevuCraft for Android (build 9)","html_url":"{elsewhere}"}}"#)).unwrap();
            assert_eq!(r.url, RELEASES_PAGE, "{elsewhere}");
        }
        assert!(parse(r#"{"message":"Not Found"}"#).is_err());
        assert!(parse("<html>").is_err());
    }

    #[test]
    fn the_windows_installer_is_offered_when_the_release_has_one() {
        let page = "https://github.com/Azurehawke/pdfcraft/releases/tag/revucraft-windows-latest";
        let msi = "https://github.com/Azurehawke/pdfcraft/releases/download/revucraft-windows-latest/RevuCraft-setup.msi";
        let body = format!(
            r#"{{"name":"RevuCraft for Windows (build 3)","html_url":"{page}","assets":[{{"browser_download_url":"https://example.com/x.msi"}},{{"browser_download_url":"{msi}"}}]}}"#
        );
        let r = pdfcraft_ui_egui::updates::parse_latest_asset(&body, Some(".msi")).unwrap();
        assert_eq!(r, Release { version: "3".into(), url: msi.into() });
        let r = pdfcraft_ui_egui::updates::parse_latest_asset(&body, Some(".apk")).unwrap();
        assert_eq!(r.url, page, "no such asset: the release page");
    }

    #[test]
    fn rate_limit_messages_name_the_wait() {
        let limited = "GitHub is limiting update checks for now (shared hourly quota); try again in about 20 minutes";
        assert_eq!(rate_limited(Some("1800001200"), 1800000000), limited);
        // Rounds up: 61 seconds out reads as 2 minutes.
        assert_eq!(
            rate_limited(Some("1800000061"), 1800000000),
            "GitHub is limiting update checks for now (shared hourly quota); try again in about 2 minutes"
        );
        assert_eq!(
            rate_limited(Some("1800000060"), 1800000000),
            "GitHub is limiting update checks for now (shared hourly quota); try again in about 1 minute"
        );
        // No header, not a number, or already past: ask for patience instead of a time.
        let later = "GitHub is limiting update checks for now (shared hourly quota); try again later";
        for bad in [None, Some(""), Some("soon"), Some("1799999999"), Some("1800000000")] {
            assert_eq!(rate_limited(bad, 1800000000), later);
        }
    }

    /// Live: asks GitHub over TLS with the OS's roots (`cargo test -p pdfcraft -- --ignored`).
    #[test]
    #[ignore = "needs network access"]
    fn github_answers_with_the_latest_release() {
        let r = latest_release().unwrap();
        assert!(r.version.parse::<u64>().is_ok(), "{r:?}");
        assert!(r.url.starts_with(RELEASES_PAGE), "{r:?}");
    }
}
