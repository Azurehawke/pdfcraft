//! Explicit document-font discovery. Interface fallbacks never enter this catalogue.
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct FontFile {
    pub path: PathBuf,
    pub family: String,
    pub face: String,
}

/// Standard user-installed font folders; discovery is performed only on request.
pub fn installed_directories() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        let mut dirs = vec![PathBuf::from("/Library/Fonts")];
        if let Some(home) = home {
            dirs.push(home.join("Library/Fonts"));
        }
        dirs
    } else if cfg!(windows) {
        let mut dirs = std::env::var_os("WINDIR").map(|p| PathBuf::from(p).join("Fonts")).into_iter().collect::<Vec<_>>();
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft/Windows/Fonts"));
        }
        dirs
    } else {
        let mut dirs = vec![PathBuf::from("/usr/share/fonts"), PathBuf::from("/usr/local/share/fonts")];
        if let Some(home) = home {
            dirs.extend([home.join(".local/share/fonts"), home.join(".fonts")]);
        }
        dirs
    }
}

/// Bounded, symlink-free scan. Only standalone, openly licensed, embeddable faces are listed.
/// The caller controls roots (automation resolves confinement before calling this).
pub fn discover(roots: &[PathBuf]) -> Vec<FontFile> {
    use std::io::Read;
    let mut pending: Vec<_> = roots.iter().take(16).map(|r| (r.clone(), 0usize)).collect();
    let mut files = Vec::new();
    let mut entries = 0usize;
    let mut budget = 64u64 << 20;
    let mut seen = std::collections::HashSet::new();
    while let Some((path, depth)) = pending.pop() {
        if entries >= 4096 || budget == 0 {
            break;
        }
        entries += 1;
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if meta.file_type().is_symlink() || !seen.insert(path.clone()) {
            continue;
        }
        if meta.is_dir() && depth < 4 {
            if let Ok(children) = std::fs::read_dir(&path) {
                let remaining = 4096usize.saturating_sub(entries.saturating_add(pending.len()));
                pending.extend(children.take(remaining).filter_map(Result::ok).map(|p| (p.path(), depth + 1)));
            }
        } else if meta.is_file() && meta.len() <= (16 << 20) && font_extension(&path) {
            if meta.len() > budget {
                continue;
            }
            budget = budget.saturating_sub(meta.len());
            let mut bytes = Vec::new();
            let read = std::fs::File::open(&path).and_then(|f| f.take((16 << 20) + 1).read_to_end(&mut bytes));
            if read.is_err() || pdfcraft_fonts::embedding::validate_document_input(&bytes).is_err() {
                continue;
            }
            if let Ok((family, face)) = pdfcraft_fonts::embedding::names(&bytes) {
                files.push(FontFile { path, family, face });
            }
        }
    }
    files.sort_by(|a, b| a.face.cmp(&b.face).then(a.path.cmp(&b.path)));
    files
}
fn font_extension(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("ttf") || e.eq_ignore_ascii_case("otf"))
}

/// Copy a font the user chose into `dir` (the app's own font folder), so [`discover`] lists it.
/// The bytes are validated first; a refused file writes nothing. The file name is reduced to
/// letters, digits, `-` and `_` (the picker's name is data, not a path), and an existing file is
/// never overwritten.
pub fn import(bytes: &[u8], file_name: &str, dir: &Path) -> Result<FontFile, String> {
    use std::io::Write;
    let base = Path::new(file_name).file_name().and_then(|n| n.to_str()).unwrap_or("");
    let (stem, ext) = match base.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case("ttf") || ext.eq_ignore_ascii_case("otf") => (stem, ext.to_ascii_lowercase()),
        _ => return Err("only TrueType (.ttf) and OpenType (.otf) fonts can be added".into()),
    };
    if bytes.len() > (16 << 20) {
        return Err("the font is larger than 16 MB".into());
    }
    pdfcraft_fonts::embedding::validate_document_input(bytes)?;
    let (family, face) = pdfcraft_fonts::embedding::names(bytes)?;
    let cleaned: String = stem.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(64).collect();
    let cleaned = if cleaned.is_empty() { "font".to_string() } else { cleaned };
    std::fs::create_dir_all(dir).map_err(|e| format!("can't create the font folder: {e}"))?;
    for n in 1..=1000u32 {
        let name = if n == 1 { format!("{cleaned}.{ext}") } else { format!("{cleaned}-{n}.{ext}") };
        let path = dir.join(name);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(e) = file.write_all(bytes) {
                    drop(file);
                    let _ = std::fs::remove_file(&path);
                    return Err(format!("can't write the font: {e}"));
                }
                return Ok(FontFile { path, family, face });
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("can't write the font: {e}")),
        }
    }
    Err("too many fonts with this name".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTER: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");

    /// A fresh folder under the system temp dir, removed when the guard drops.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
            let dir = std::env::temp_dir().join(format!("pdfcraft-font-{tag}-{}-{nanos}", std::process::id()));
            Self(dir)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn an_imported_font_is_listed_by_discovery() {
        let scratch = Scratch::new("listed");
        let font = import(INTER, "My Font.ttf", &scratch.0).unwrap();
        assert!(font.path.starts_with(&scratch.0));
        let found = discover(std::slice::from_ref(&scratch.0));
        assert!(found.iter().any(|f| f.path == font.path && f.face == font.face), "{found:?}");
    }

    #[test]
    fn a_file_that_is_not_a_font_is_refused_and_writes_nothing() {
        let scratch = Scratch::new("refused");
        assert!(import(b"not a font at all", "fake.ttf", &scratch.0).is_err());
        assert!(!scratch.0.exists() || std::fs::read_dir(&scratch.0).map(|d| d.count() == 0).unwrap_or(true));
    }

    #[test]
    fn a_file_of_another_type_is_refused() {
        let scratch = Scratch::new("type");
        assert!(import(INTER, "notes.txt", &scratch.0).is_err());
        assert!(!scratch.0.exists());
    }

    #[test]
    fn a_hostile_file_name_cannot_leave_the_font_folder() {
        let scratch = Scratch::new("escape");
        let font = import(INTER, "../../escape.otf", &scratch.0).unwrap();
        assert_eq!(font.path.parent(), Some(scratch.0.as_path()));
        assert!(font.path.extension().is_some_and(|e| e == "otf"));
        assert!(!scratch.0.parent().is_some_and(|p| p.join("escape.otf").exists()));
    }

    #[test]
    fn importing_the_same_name_twice_keeps_both_files() {
        let scratch = Scratch::new("twice");
        let a = import(INTER, "Inter.ttf", &scratch.0).unwrap();
        let b = import(INTER, "Inter.ttf", &scratch.0).unwrap();
        assert_ne!(a.path, b.path);
        assert_eq!(discover(std::slice::from_ref(&scratch.0)).len(), 2);
    }
}
