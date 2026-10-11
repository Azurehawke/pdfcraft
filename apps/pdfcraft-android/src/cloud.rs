//! Google Drive and other document apps, through Android's document picker (the Storage Access
//! Framework).
//!
//! PdfCraft reads and writes files by path, and a document chosen in Android's picker is a
//! `content://` URI. So each chosen document gets a copy on the device, under
//! `<app folder>/cloud/<id>/<name>`, and that path is what the app opens or saves to. While the
//! app runs, the copies are watched: when one changes (PdfCraft saved it) and has stopped
//! changing, it is written back to its document, and the user is told.
//!
//! The picker grants lasting access (`PdfCraftActivity.add`), and the list of copies is kept in
//! `cloud/index.tsv`, so a copy reopened later from Recent Files still saves back.
//!
//! What this doesn't do: notice that the document changed elsewhere since its copy was made
//! (opening it again from the picker fetches it again), or work without the document's app
//! (Google Drive itself syncs, offline included).

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant, UNIX_EPOCH};

use jni::JNIEnv;
use jni::objects::{JObject, JValue};
use pdfcraft_ui_egui::{PdfCraftApp, SystemPick};

use crate::file_access::with_activity;
use crate::incoming::{column, quietly, read_all, string_of, text_of};

/// Google Drive's document provider.
const DRIVE: &str = "com.google.android.apps.docs.storage";
/// How long a copy must stay unchanged before it is written back (a save in progress).
const SETTLE: Duration = Duration::from_millis(700);
/// How often the copies are checked.
const CHECK_EVERY: Duration = Duration::from_millis(700);
/// Copies kept in the list at most (the oldest are forgotten).
const MAX_MIRRORS: usize = 500;

/// A file's modification time (ns since the epoch) and length.
type Stamp = (u128, u64);

#[derive(Clone, Debug)]
struct Mirror {
    uri: String,
    local: PathBuf,
    name: String,
    drive: bool,
    /// The copy as last written to (or read from) the document.
    synced: Option<Stamp>,
    /// A change seen, and when: written back once it has settled.
    seen: Option<(Stamp, Instant)>,
    /// A version that couldn't be written back: not retried until it changes again.
    failed: Option<Stamp>,
    uploading: bool,
}

#[derive(Debug)]
enum Kind {
    Open,
    Save,
}

#[derive(Debug)]
enum Pick {
    Idle,
    /// The system picker is showing.
    Waiting(Kind),
    /// Making the local copies.
    Fetching(Receiver<Result<Vec<PathBuf>, String>>),
}

static PICK: Mutex<Pick> = Mutex::new(Pick::Idle);
static MIRRORS: Mutex<Vec<Mirror>> = Mutex::new(Vec::new());
/// Finished write-backs: (uri, the version written, the result).
static UPLOADED: Mutex<Vec<(String, Stamp, Result<(), String>)>> = Mutex::new(Vec::new());
static LAST_CHECK: Mutex<Option<Instant>> = Mutex::new(None);

fn dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("cloud"))
}

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos();
    Some((mtime, meta.len()))
}

/// Read the list of copies (at start-up).
pub fn load() {
    let Some(index) = dir().map(|d| d.join("index.tsv")) else { return };
    let Ok(text) = std::fs::read_to_string(index) else { return };
    let mirrors: Vec<Mirror> = text
        .lines()
        .filter_map(|line| {
            let mut f = line.split('\t');
            let (uri, local, name, drive) = (f.next()?, f.next()?, f.next()?, f.next()?);
            let synced = match (f.next().and_then(|m| m.parse().ok()), f.next().and_then(|l| l.parse().ok())) {
                (Some(m), Some(l)) => Some((m, l)),
                _ => None,
            };
            Some(Mirror {
                uri: uri.to_string(),
                local: PathBuf::from(local),
                name: name.to_string(),
                drive: drive == "1",
                synced,
                seen: None,
                failed: None,
                uploading: false,
            })
        })
        .take(MAX_MIRRORS)
        .collect();
    log::info!("{} documents from other apps", mirrors.len());
    *MIRRORS.lock().unwrap_or_else(PoisonError::into_inner) = mirrors;
}

fn save_index(mirrors: &[Mirror]) {
    let Some(dir) = dir() else { return };
    let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    let text: String = mirrors
        .iter()
        .map(|m| {
            let (mtime, len) = m.synced.map_or((String::new(), String::new()), |(m, l)| (m.to_string(), l.to_string()));
            format!(
                "{}\t{}\t{}\t{}\t{mtime}\t{len}\n",
                clean(&m.uri),
                clean(&m.local.to_string_lossy()),
                clean(&m.name),
                if m.drive { "1" } else { "0" }
            )
        })
        .collect();
    let _ = std::fs::create_dir_all(&dir);
    let tmp = dir.join("index.tsv.tmp");
    // Best effort: without the list, copies made this session still save back until restart.
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, dir.join("index.tsv"));
    }
}

/// Show Android's document picker (`AndroidDocuments::start`).
pub fn start(pick: SystemPick) -> Result<(), String> {
    let kind = match &pick {
        SystemPick::Open { .. } => Kind::Open,
        SystemPick::Save { .. } => Kind::Save,
    };
    with_activity("Opening Android's document picker", |env, activity| match &pick {
        SystemPick::Open { multiple, mime_types } => {
            let types = env.new_object_array(i32::try_from(mime_types.len()).unwrap_or(1), "java/lang/String", JObject::null())?;
            for (i, m) in mime_types.iter().enumerate() {
                let s = env.new_string(m)?;
                env.set_object_array_element(&types, i32::try_from(i).unwrap_or(0), &s)?;
            }
            env.call_method(activity, "pcOpenDocuments", "(Z[Ljava/lang/String;)V", &[JValue::Bool(u8::from(*multiple)), JValue::Object(&types)])?;
            Ok(())
        }
        SystemPick::Save { name, mime_type } => {
            let name = env.new_string(name)?;
            let mime = env.new_string(mime_type)?;
            env.call_method(
                activity,
                "pcCreateDocument",
                "(Ljava/lang/String;Ljava/lang/String;)V",
                &[JValue::Object(&name), JValue::Object(&mime)],
            )?;
            Ok(())
        }
    })?;
    *PICK.lock().unwrap_or_else(PoisonError::into_inner) = Pick::Waiting(kind);
    Ok(())
}

/// The picker's answer, as local paths (`AndroidDocuments::poll`).
pub fn poll() -> Option<Result<Vec<PathBuf>, String>> {
    let mut pick = PICK.lock().unwrap_or_else(PoisonError::into_inner);
    match &*pick {
        Pick::Idle => Some(Ok(Vec::new())),
        Pick::Waiting(_) => {
            let taken = with_activity("Reading the document picker's answer", |env, activity| {
                let picked = env.call_method(activity, "pcTakePicked", "()[Ljava/lang/String;", &[])?.l()?;
                if picked.is_null() {
                    return Ok(None);
                }
                let array = jni::objects::JObjectArray::from(picked);
                let n = env.get_array_length(&array)?;
                let mut uris = Vec::new();
                for i in 0..n.min(1000) {
                    let item = env.get_object_array_element(&array, i)?;
                    if let Some(s) = string_of(env, item)? {
                        uris.push(s);
                    }
                }
                Ok(Some(uris))
            });
            match taken {
                Ok(None) => None,
                Ok(Some(uris)) if uris.is_empty() => {
                    *pick = Pick::Idle;
                    Some(Ok(Vec::new()))
                }
                Ok(Some(uris)) => {
                    let kind = match std::mem::replace(&mut *pick, Pick::Idle) {
                        Pick::Waiting(kind) => kind,
                        _ => Kind::Open,
                    };
                    let (tx, rx) = mpsc::channel();
                    let spawned = std::thread::Builder::new().name("cloud-fetch".into()).spawn(move || {
                        let _ = tx.send(fetch(&kind, &uris));
                    });
                    match spawned {
                        Ok(_) => {
                            *pick = Pick::Fetching(rx);
                            None
                        }
                        Err(e) => Some(Err(format!("Couldn't start: {e}"))),
                    }
                }
                Err(e) => {
                    *pick = Pick::Idle;
                    Some(Err(e))
                }
            }
        }
        Pick::Fetching(rx) => match rx.try_recv() {
            Ok(result) => {
                *pick = Pick::Idle;
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                *pick = Pick::Idle;
                Some(Err("Getting the document stopped unexpectedly.".to_string()))
            }
        },
    }
}

/// A file name that is safe as one path component.
fn safe_name(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if c == '/' || c == '\\' || c.is_control() { '_' } else { c }).collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_string();
    if cleaned.is_empty() { "Document.pdf".to_string() } else { cleaned }
}

/// The local copy's path for a document.
fn local_path(uri: &str, name: &str) -> Option<PathBuf> {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    uri.hash(&mut h);
    Some(dir()?.join(format!("{:016x}", h.finish())).join(safe_name(name)))
}

fn uri_object<'local>(env: &mut JNIEnv<'local>, uri: &str) -> jni::errors::Result<JObject<'local>> {
    let s = env.new_string(uri)?;
    env.call_static_method("android/net/Uri", "parse", "(Ljava/lang/String;)Landroid/net/Uri;", &[JValue::Object(&s)])?.l()
}

/// Make the local copies of the chosen documents (a worker thread).
fn fetch(kind: &Kind, uris: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for uri in uris {
        let (name, drive, bytes) = with_activity("Getting the document", |env, activity| {
            let resolver = env.call_method(activity, "getContentResolver", "()Landroid/content/ContentResolver;", &[])?.l()?;
            let u = uri_object(env, uri)?;
            let name = column(env, &resolver, &u, "_display_name").unwrap_or_else(|| "Document.pdf".to_string());
            let authority = quietly(env, |env| text_of(env, &u, "getAuthority")).flatten();
            let bytes = match kind {
                Kind::Open => Some(read_all(env, &resolver, &u)),
                Kind::Save => None,
            };
            Ok((name, authority.as_deref() == Some(DRIVE), bytes))
        })?;
        let local = local_path(uri, &name).ok_or("PdfCraft has no app folder")?;
        if let Some(parent) = local.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{name}: {e}"))?;
        }
        let synced = match bytes {
            Some(bytes) => {
                let bytes = bytes.map_err(|e| format!("Couldn't get {name}: {e}"))?;
                std::fs::write(&local, bytes).map_err(|e| format!("{name}: {e}"))?;
                stamp(&local)
            }
            None => {
                // A new document: PdfCraft writes the copy, which is then written back.
                let _ = std::fs::remove_file(&local);
                None
            }
        };
        log::info!("{uri} is {}", local.display());
        let mut mirrors = MIRRORS.lock().unwrap_or_else(PoisonError::into_inner);
        mirrors.retain(|m| m.uri != *uri && m.local != local);
        mirrors.push(Mirror { uri: uri.clone(), local: local.clone(), name, drive, synced, seen: None, failed: None, uploading: false });
        if mirrors.len() > MAX_MIRRORS {
            let extra = mirrors.len() - MAX_MIRRORS;
            mirrors.drain(..extra);
        }
        save_index(&mirrors);
        paths.push(local);
    }
    Ok(paths)
}

/// Write `bytes` to the document (a worker thread).
fn upload(uri: &str, bytes: &[u8]) -> Result<(), String> {
    with_activity("Saving to the document", |env, activity| {
        let resolver = env.call_method(activity, "getContentResolver", "()Landroid/content/ContentResolver;", &[])?.l()?;
        let u = uri_object(env, uri)?;
        // "wt" replaces the content; some apps only know "w".
        let mut stream = JObject::null();
        for mode in ["wt", "w"] {
            let m = env.new_string(mode)?;
            if let Some(s) = quietly(env, |env| {
                env.call_method(
                    &resolver,
                    "openOutputStream",
                    "(Landroid/net/Uri;Ljava/lang/String;)Ljava/io/OutputStream;",
                    &[JValue::Object(&u), JValue::Object(&m)],
                )?
                .l()
            }) && !s.is_null()
            {
                stream = s;
                break;
            }
        }
        if stream.is_null() {
            return Ok(Err("the app that keeps it doesn't let PdfCraft write to it".to_string()));
        }
        let mut result = Ok(());
        for chunk in bytes.chunks(256 * 1024) {
            let wrote = quietly(env, |env| {
                let array = env.byte_array_from_slice(chunk)?;
                env.call_method(&stream, "write", "([B)V", &[JValue::Object(&array)])?;
                env.delete_local_ref(array)
            });
            if wrote.is_none() {
                result = Err("writing failed".to_string());
                break;
            }
        }
        let closed = quietly(env, |env| env.call_method(&stream, "close", "()V", &[]).map(|_| ()));
        if result.is_ok() && closed.is_none() {
            result = Err("finishing the write failed".to_string());
        }
        Ok(result)
    })?
}

/// Write back copies PdfCraft has saved, and report finished write-backs. Called every frame.
pub fn sync(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let finished = std::mem::take(&mut *UPLOADED.lock().unwrap_or_else(PoisonError::into_inner));
    let mut mirrors = MIRRORS.lock().unwrap_or_else(PoisonError::into_inner);
    if !finished.is_empty() {
        for (uri, version, result) in finished {
            let Some(m) = mirrors.iter_mut().find(|m| m.uri == uri) else { continue };
            m.uploading = false;
            let place = if m.drive { "Google Drive" } else { "its app" };
            match result {
                Ok(()) => {
                    m.synced = Some(version);
                    m.failed = None;
                    app.notify(format!("Saved {} to {place}.", m.name));
                }
                Err(e) => {
                    log::warn!("saving {uri}: {e}");
                    m.failed = Some(version);
                    app.notify(format!("Couldn't save {} to {place}: {e}. It's kept on this tablet; save again to retry.", m.name));
                }
            }
        }
        save_index(&mirrors);
    }
    {
        let mut last = LAST_CHECK.lock().unwrap_or_else(PoisonError::into_inner);
        if last.is_some_and(|t| t.elapsed() < CHECK_EVERY) {
            if mirrors.iter().any(|m| m.seen.is_some() || m.uploading) {
                ctx.request_repaint_after(CHECK_EVERY);
            }
            return;
        }
        *last = Some(Instant::now());
    }
    let mut busy = false;
    for m in mirrors.iter_mut() {
        if m.uploading {
            busy = true;
            continue;
        }
        let Some(now) = stamp(&m.local) else {
            m.seen = None;
            continue;
        };
        if Some(now) == m.synced || Some(now) == m.failed {
            m.seen = None;
            continue;
        }
        busy = true;
        match m.seen {
            Some((seen, at)) if seen == now && at.elapsed() >= SETTLE => {
                m.seen = None;
                m.uploading = true;
                let (uri, local) = (m.uri.clone(), m.local.clone());
                let spawned = std::thread::Builder::new().name("cloud-save".into()).spawn(move || {
                    let result = std::fs::read(&local).map_err(|e| e.to_string()).and_then(|bytes| upload(&uri, &bytes));
                    UPLOADED.lock().unwrap_or_else(PoisonError::into_inner).push((uri, now, result));
                });
                if spawned.is_err() {
                    m.uploading = false;
                }
            }
            Some((seen, _)) if seen == now => {}
            _ => m.seen = Some((now, Instant::now())),
        }
    }
    if busy {
        ctx.request_repaint_after(CHECK_EVERY);
    }
}

#[cfg(test)]
mod tests {
    use super::safe_name;

    #[test]
    fn names_become_one_safe_component() {
        assert_eq!(safe_name("Report.pdf"), "Report.pdf");
        assert_eq!(safe_name("a/b\\c.pdf"), "a_b_c.pdf");
        assert_eq!(safe_name("../x.pdf"), "_x.pdf");
        assert_eq!(safe_name("  "), "Document.pdf");
        assert_eq!(safe_name(".hidden"), "hidden");
    }
}
