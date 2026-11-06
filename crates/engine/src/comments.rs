//! RevuCraft's Markdown comments: the files kept beside a PDF in `<pdf-name>.comments/`, their
//! names and their frontmatter. The frontmatter keys and the names are a format to keep stable.

use std::path::{Path, PathBuf};

/// The longest cleaned name kept (characters).
const MAX_STEM: usize = 64;
/// The longest author or title written into frontmatter (characters).
const MAX_TEXT: usize = 500;

/// When a comment was made: its file-name form (`20261103-122100`) and its ISO form (local time).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub file: String,
    pub iso: String,
}

/// The current local time as a comment stamp. Off native targets there is no clock; the stamp is
/// blank, and no comment files are made there.
pub fn stamp_now() -> Stamp {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let now = chrono::Local::now();
        Stamp { file: now.format("%Y%m%d-%H%M%S").to_string(), iso: now.format("%Y-%m-%dT%H:%M:%S").to_string() }
    }
    #[cfg(target_arch = "wasm32")]
    {
        Stamp { file: String::new(), iso: String::new() }
    }
}

/// A PDF's name made safe for folders and files: letters, digits, `-` and `_` are kept, spaces
/// become `-`, and anything else is dropped. An empty result becomes `drawing`.
pub fn clean_stem(stem: &str) -> String {
    let mut out = String::new();
    for c in stem.trim().chars() {
        if out.chars().count() >= MAX_STEM {
            break;
        }
        if c.is_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else if c == ' ' {
            out.push('-');
        }
    }
    if out.is_empty() { "drawing".to_string() } else { out }
}

/// The page as it appears in a comment's name: 1-based and zero-padded to three digits.
pub fn page_label(page0: usize) -> String {
    format!("{:03}", page0.saturating_add(1))
}

/// A folder beside `pdf` named `<pdf-name>.<suffix>`.
fn beside(pdf: &Path, suffix: &str) -> PathBuf {
    let stem = pdf.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    pdf.parent().unwrap_or(Path::new("")).join(format!("{}.{suffix}", clean_stem(&stem)))
}

/// The folder of comment files beside `pdf`: `<pdf-name>.comments`.
pub fn comments_dir(pdf: &Path) -> PathBuf {
    beside(pdf, "comments")
}

/// The folder of photos beside `pdf`: `<pdf-name>.images`.
pub fn images_dir(pdf: &Path) -> PathBuf {
    beside(pdf, "images")
}

/// The first free file name `<base>.md`, `<base>-2.md`, `<base>-3.md`… in `dir`.
pub fn unique_name(dir: &Path, base: &str) -> Option<String> {
    (1..10_000u32).map(|n| if n == 1 { format!("{base}.md") } else { format!("{base}-{n}.md") }).find(|name| !dir.join(name).exists())
}

/// A string as a YAML double-quoted scalar, with control characters left out.
fn yaml_text(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.trim().chars().take(MAX_TEXT) {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The frontmatter of a new comment: its title, tags, page (1-based), author, date and status.
pub fn frontmatter(title: &str, page1: usize, author: &str, iso: &str) -> String {
    format!(
        "---\ntitle: {}\ntags: []\npage: {page1}\nauthor: {}\ndate: {}\nstatus: open\n---\n\n",
        yaml_text(title),
        yaml_text(author),
        yaml_text(iso)
    )
}

/// A comment file that has been made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Created {
    pub path: PathBuf,
    /// The file, relative to the PDF's folder: what the note's `/PCNote` names.
    pub rel: String,
    pub text: String,
}

/// Makes a new Markdown comment for page `page0` (0-based) of `pdf`, in its comments folder. The
/// file is named `<pdf-name>-<page>-<stamp>`, with a counter if that name is taken, and never
/// overwrites an existing file.
pub fn create(pdf: &Path, page0: usize, author: &str, stamp: &Stamp) -> Result<Created, String> {
    use std::io::Write;
    let stem = pdf.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let dir = comments_dir(pdf);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make the comments folder: {e}"))?;
    let base = format!("{}-{}-{}", clean_stem(&stem), page_label(page0), stamp.file);
    let name = unique_name(&dir, &base).ok_or("There is no free name for the comment.")?;
    let page1 = page0.saturating_add(1);
    let title = format!("Comment on page {page1}");
    let who = [stamp.iso.trim(), author.trim()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    let body = if who.is_empty() { String::new() } else { format!("*{who}*\n\n") };
    let text = format!("{}# {title}\n\n{body}", frontmatter(&title, page1, author, &stamp.iso));
    let path = dir.join(&name);
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| format!("Couldn't write {name}: {e}"))?;
    file.write_all(text.as_bytes()).map_err(|e| format!("Couldn't write {name}: {e}"))?;
    let folder = dir.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(Created { path, rel: format!("{folder}/{name}"), text })
}

/// The largest note file read (bytes).
pub const NOTE_MAX_BYTES: u64 = 1 << 20;
/// The largest image decoded (bytes on disk).
pub const IMAGE_MAX_BYTES: u64 = 25 << 20;
/// The most pixels an image may have once decoded.
pub const IMAGE_MAX_PIXELS: u64 = 40_000_000;
/// The longest link target accepted (characters).
const MAX_TARGET: usize = 1024;

/// Whether a `[[target]]` names an image (by its extension).
pub fn is_image(target: &str) -> bool {
    matches!(extension(target).as_deref(), Some("png" | "jpg" | "jpeg" | "gif" | "tif" | "tiff" | "bmp"))
}

/// Whether a `[[target]]` names a file beside the PDF (a Markdown note or an image). Any other
/// `[[target]]` names a markup on the plan.
pub fn is_file_target(target: &str) -> bool {
    is_image(target) || extension(target).as_deref() == Some("md")
}

fn extension(target: &str) -> Option<String> {
    Path::new(target.trim()).extension().map(|e| e.to_string_lossy().to_ascii_lowercase())
}

/// Where a link's target is, inside the PDF's folder `dir`. A target that leaves the folder
/// (`../`, an absolute path, a drive, or a symbolic link out of it) is refused with a message. A
/// target that is inside but not there yet is returned: the caller shows it as broken.
pub fn resolve(dir: &Path, target: &str) -> Result<PathBuf, String> {
    let refused = || format!("{} must stay inside the PDF's folder.", target.trim());
    let t = target.trim();
    if t.is_empty() || t.chars().count() > MAX_TARGET || t.chars().any(|c| c.is_control() || c == ':' || c == '\\') {
        return Err(refused());
    }
    let rel = Path::new(t);
    if rel.components().any(|c| !matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir)) {
        return Err(refused());
    }
    let joined = dir.join(rel);
    // A symbolic link may point out of the folder: compare where the file really is.
    if let (Ok(real), Ok(base)) = (joined.canonicalize(), dir.canonicalize())
        && !real.starts_with(&base)
    {
        return Err(refused());
    }
    Ok(joined)
}

/// Checks an image's header without decoding it: that it is an image and not too many pixels.
fn check_image(bytes: &[u8]) -> Result<(), String> {
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("Can't read the image: {e}"))?
        .into_dimensions()
        .map_err(|e| format!("Can't read the image: {e}"))?;
    if u64::from(w).saturating_mul(u64::from(h)) > IMAGE_MAX_PIXELS {
        return Err("The image has too many pixels to show.".into());
    }
    Ok(())
}

/// Decodes an image for display: its width, its height and its RGBA bytes. Refuses files and
/// images that are too large, and returns an error rather than a partial image.
pub fn load_image(path: &Path) -> Result<(usize, usize, Vec<u8>), String> {
    let len = std::fs::metadata(path).map_err(|e| format!("Can't read the image: {e}"))?.len();
    if len > IMAGE_MAX_BYTES {
        return Err("The image is too large to show.".into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Can't read the image: {e}"))?;
    check_image(&bytes)?;
    let rgba = image::load_from_memory(&bytes).map_err(|e| format!("Can't decode the image: {e}"))?.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    let pixels = rgba.into_raw();
    if pixels.len() != w.saturating_mul(h).saturating_mul(4) {
        return Err("The image could not be read whole.".into());
    }
    Ok((w, h, pixels))
}

/// The longest photo file name kept (characters).
const MAX_PHOTO_NAME: usize = 200;

/// A photo's file name, as the camera gave it, checked: a plain name (no folders, no characters
/// that Windows forbids) with an image extension.
pub fn photo_file_name(name: &str) -> Result<String, String> {
    let n = name.trim();
    let plain = !n.is_empty()
        && n.chars().count() <= MAX_PHOTO_NAME
        && n != "."
        && n != ".."
        && !n.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'));
    if !plain || !is_image(n) {
        return Err(format!("{name} isn't a photo this drawing can take."));
    }
    Ok(n.to_string())
}

/// Keeps a photo beside the PDF, in `<pdf-name>.images/`, under the camera's own name (with `-2`,
/// `-3`… before the extension if that name is taken). The bytes are copied as they are. Returns
/// the photo's path relative to the PDF's folder, for a `[[...]]` link.
pub fn add_photo(pdf: &Path, name: &str, bytes: &[u8]) -> Result<String, String> {
    use std::io::Write;
    let name = photo_file_name(name)?;
    if bytes.len() as u64 > IMAGE_MAX_BYTES {
        return Err("The photo is too large to keep.".into());
    }
    check_image(bytes)?;
    let dir = images_dir(pdf);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make the images folder: {e}"))?;
    let (base, ext) = name.rsplit_once('.').unwrap_or((name.as_str(), ""));
    let file = (1..10_000u32)
        .map(|n| if n == 1 { name.clone() } else { format!("{base}-{n}.{ext}") })
        .find(|candidate| !dir.join(candidate).exists())
        .ok_or("There is no free name for the photo.")?;
    let path = dir.join(&file);
    let mut out = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| format!("Couldn't keep {file}: {e}"))?;
    out.write_all(bytes).map_err(|e| format!("Couldn't keep {file}: {e}"))?;
    let folder = dir.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(format!("{folder}/{file}"))
}

/// Keeps the photo file at `source` beside the PDF (see [`add_photo`]). A file too large to read
/// is refused before it is read.
pub fn add_photo_file(pdf: &Path, source: &Path) -> Result<String, String> {
    let name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let len = std::fs::metadata(source).map_err(|e| format!("Can't read {name}: {e}"))?.len();
    if len > IMAGE_MAX_BYTES {
        return Err("The photo is too large to keep.".into());
    }
    let bytes = std::fs::read(source).map_err(|e| format!("Can't read {name}: {e}"))?;
    add_photo(pdf, &name, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp() -> Stamp {
        Stamp { file: "20261103-122100".into(), iso: "2026-11-03T12:21:00".into() }
    }

    #[test]
    fn names_are_made_safe() {
        assert_eq!(clean_stem("Smith Residence"), "Smith-Residence");
        assert_eq!(clean_stem("Plan (rev 2)!"), "Plan-rev-2");
        assert_eq!(clean_stem("Ñoño plan"), "Ñoño-plan");
        assert_eq!(clean_stem("  ...  "), "drawing");
        assert_eq!(clean_stem(&"a".repeat(500)).chars().count(), MAX_STEM);
        assert!(clean_stem("../../etc").chars().all(|c| c != '/' && c != '.'));
    }

    #[test]
    fn pages_are_one_based_and_padded() {
        assert_eq!(page_label(0), "001");
        assert_eq!(page_label(2), "003");
        assert_eq!(page_label(999), "1000");
        assert_eq!(page_label(usize::MAX), "18446744073709551615", "no overflow at the largest page");
    }

    #[test]
    fn the_comments_folder_sits_beside_the_pdf() {
        assert_eq!(comments_dir(Path::new("/jobs/Smith Residence.pdf")), PathBuf::from("/jobs/Smith-Residence.comments"));
        assert_eq!(comments_dir(Path::new("plan.pdf")), PathBuf::from("plan.comments"));
    }

    #[test]
    fn frontmatter_quotes_its_values() {
        let fm = frontmatter("Says \"hi\"\nthen \\ more", 3, "Steve", "2026-11-03T12:21:00");
        assert!(fm.starts_with("---\n"), "{fm}");
        assert!(fm.contains("title: \"Says \\\"hi\\\"then \\\\ more\"\n"), "{fm}");
        assert!(fm.contains("page: 3\n"));
        assert!(fm.contains("status: open\n"));
        assert!(fm.ends_with("---\n\n"));
    }

    #[test]
    fn a_comment_is_written_beside_the_pdf_and_never_overwritten() {
        let dir = std::env::temp_dir().join(format!("pdfcraft-comments-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pdf = dir.join("Smith Residence.pdf");
        let first = create(&pdf, 2, "Steve", &stamp()).unwrap();
        assert_eq!(first.rel, "Smith-Residence.comments/Smith-Residence-003-20261103-122100.md");
        assert!(first.path.is_file());
        assert!(first.text.contains("page: 3\n"));
        assert!(first.text.contains("# Comment on page 3"));
        let second = create(&pdf, 2, "Steve", &stamp()).unwrap();
        assert!(second.rel.ends_with("-003-20261103-122100-2.md"), "{}", second.rel);
        assert_ne!(first.path, second.path);
        assert_eq!(std::fs::read_to_string(&first.path).unwrap(), first.text, "the first comment is untouched");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn links_inside_the_folder_resolve_and_others_are_refused() {
        let dir = std::env::temp_dir().join(format!("pdfcraft-links-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Plan.images")).unwrap();
        assert_eq!(resolve(&dir, "Plan.images/a.jpg"), Ok(dir.join("Plan.images/a.jpg")), "missing but inside is returned");
        assert_eq!(resolve(&dir, "./Plan.images/a.jpg"), Ok(dir.join("./Plan.images/a.jpg")));
        for bad in ["../x.md", "a/../../x.md", "/etc/passwd", "C:x.md", "\\\\server\\x.md", "", "   ", "x\0.md", "x\n.md"] {
            assert!(resolve(&dir, bad).is_err(), "{bad:?} must be refused");
        }
        let long = "a".repeat(MAX_TARGET + 1);
        assert!(resolve(&dir, &long).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_through_a_symbolic_link_out_of_the_folder_is_refused() {
        let root = std::env::temp_dir().join(format!("pdfcraft-symlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("job");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(root.join("secret.md"), "x").unwrap();
        std::os::unix::fs::symlink(root.join("secret.md"), dir.join("link.md")).unwrap();
        assert!(resolve(&dir, "link.md").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_images_and_markdown_are_file_links() {
        assert!(is_file_target("Plan.images/Photo 1.JPG"));
        assert!(is_file_target("note.md"));
        assert!(!is_file_target("Callout 3"), "a markup name stays a markup");
        assert!(!is_file_target("sheet.xlsx"), "workbooks have their own link");
        assert!(!is_image(""));
    }

    fn tiny_png() -> Vec<u8> {
        let img = image::RgbaImage::new(2, 3);
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn photo_names_are_the_cameras_and_hostile_ones_are_refused() {
        assert_eq!(photo_file_name(" IMG_1234.JPG ").as_deref(), Ok("IMG_1234.JPG"));
        assert!(photo_file_name("Kitchen sink.heic").is_err(), "only the image types a drawing can show");
        for bad in ["../x.jpg", "a/b.jpg", "a\\b.jpg", "C:x.jpg", "x\0.jpg", ".jpg", "", "   ", "doc.pdf"] {
            assert!(photo_file_name(bad).is_err(), "{bad:?} must be refused");
        }
        assert!(photo_file_name(&format!("{}.jpg", "a".repeat(MAX_PHOTO_NAME))).is_err());
    }

    #[test]
    fn a_photo_keeps_its_name_and_is_never_overwritten() {
        let dir = std::env::temp_dir().join(format!("pdfcraft-photos-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pdf = dir.join("Smith Residence.pdf");
        let png = tiny_png();
        let first = add_photo(&pdf, "IMG_1.png", &png).unwrap();
        assert_eq!(first, "Smith-Residence.images/IMG_1.png");
        assert_eq!(std::fs::read(dir.join(&first)).unwrap(), png, "the bytes are kept as they are");
        let second = add_photo(&pdf, "IMG_1.png", &png).unwrap();
        assert_eq!(second, "Smith-Residence.images/IMG_1-2.png");
        assert_eq!(std::fs::read(dir.join(&first)).unwrap(), png, "the first photo is untouched");
        assert!(add_photo(&pdf, "IMG_2.png", b"not a picture").is_err());
        assert!(!dir.join("Smith-Residence.images/IMG_2.png").exists(), "a refused photo leaves no file");
        let from_disk = dir.join("camera.png");
        std::fs::write(&from_disk, &png).unwrap();
        assert_eq!(add_photo_file(&pdf, &from_disk).unwrap(), "Smith-Residence.images/camera.png");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_broken_or_hostile_image_is_an_error_not_a_panic() {
        let dir = std::env::temp_dir().join(format!("pdfcraft-image-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(load_image(&dir.join("missing.jpg")).is_err());
        let junk = dir.join("junk.jpg");
        std::fs::write(&junk, b"not an image at all").unwrap();
        assert!(load_image(&junk).is_err());
        let empty = dir.join("empty.png");
        std::fs::write(&empty, b"").unwrap();
        assert!(load_image(&empty).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
