//! RevuCraft's Markdown comments: the files kept beside a PDF in `<pdf-name>.comments/`, their
//! names and their frontmatter. PlanCraft reads these files directly, so the frontmatter keys and
//! the names are a format to keep stable.

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

/// The folder of comment files beside `pdf`: `<pdf-name>.comments`.
pub fn comments_dir(pdf: &Path) -> PathBuf {
    let stem = pdf.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    pdf.parent().unwrap_or(Path::new("")).join(format!("{}.comments", clean_stem(&stem)))
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
}
