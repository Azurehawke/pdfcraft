//! RevuCraft's update check on Android: one HTTPS request to GitHub for the `revucraft-latest`
//! release, through Java's `HttpsURLConnection`, so Android's own TLS and trusted roots are used
//! (no root list is bundled). The answer offers the APK itself; Download hands it to the
//! browser, and Android asks before installing it over this build.

use jni::JNIEnv;
use jni::objects::JValue;
use pdfcraft_ui_egui::updates::{LATEST_API, Release, parse_latest};

/// GitHub's answer is small; anything past this is cut off (and then fails to parse).
const MAX_BYTES: usize = 1 << 20;

/// The latest RevuCraft build and its APK (blocking; the app runs it on its own thread).
pub fn latest_release() -> Result<Release, String> {
    let body = crate::file_access::with_activity("checking for updates", |env, _| get(env, LATEST_API))?;
    parse_latest(&body, true)
}

/// GET `url` and return the body as text.
fn get(env: &mut JNIEnv, url: &str) -> jni::errors::Result<String> {
    let address = env.new_string(url)?;
    let url = env.new_object("java/net/URL", "(Ljava/lang/String;)V", &[JValue::Object(&address)])?;
    let connection = env.call_method(&url, "openConnection", "()Ljava/net/URLConnection;", &[])?.l()?;
    env.call_method(&connection, "setConnectTimeout", "(I)V", &[JValue::Int(10_000)])?;
    env.call_method(&connection, "setReadTimeout", "(I)V", &[JValue::Int(10_000)])?;
    for (key, value) in [("Accept", "application/vnd.github+json"), ("User-Agent", concat!("RevuCraft/", env!("CARGO_PKG_VERSION")))] {
        let key = env.new_string(key)?;
        let value = env.new_string(value)?;
        env.call_method(
            &connection,
            "setRequestProperty",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[JValue::Object(&key), JValue::Object(&value)],
        )?;
    }
    // A failed request (no network, GitHub's rate limit) throws here; the caller reports it.
    let stream = env.call_method(&connection, "getInputStream", "()Ljava/io/InputStream;", &[])?.l()?;
    let chunk = env.new_byte_array(8192)?;
    let mut buf = vec![0i8; 8192];
    let mut body = Vec::new();
    loop {
        let n = env.call_method(&stream, "read", "([B)I", &[JValue::Object(&chunk)])?.i()?;
        if n < 0 {
            break;
        }
        let n = usize::try_from(n).unwrap_or(0).min(buf.len());
        let Some(part) = buf.get_mut(..n) else { break };
        env.get_byte_array_region(&chunk, 0, part)?;
        body.extend(part.iter().map(|b| *b as u8));
        if body.len() > MAX_BYTES {
            break;
        }
    }
    let _ = env.call_method(&stream, "close", "()V", &[]);
    Ok(String::from_utf8_lossy(&body).into_owned())
}
