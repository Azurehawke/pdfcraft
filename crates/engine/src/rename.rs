//! The Rename job: the PDF and its job folder (notes and photos) get a new name together, and the
//! links in the notes that name the old PDF or folder are rewritten to the new one. Links to
//! markups (`[[label]]`) name markups, not files, so they are left as they are.
//!
//! [`plan`] reads and works out everything without touching a file; [`apply`] makes the changes.
use std::path::{Path, PathBuf};

use crate::package;

/// The most bytes read from one note.
const MAX_NOTE_BYTES: u64 = 1 << 20;

/// What a rename will do, worked out before any file changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub old_pdf: PathBuf,
    pub new_pdf: PathBuf,
    /// The job folder, when there is one (it is renamed with the PDF).
    pub folder: Option<(PathBuf, PathBuf)>,
    /// Each note file under the job folder whose text changes: its current path and the new text.
    pub notes: Vec<(PathBuf, String)>,
}

/// `text` with each link to `old` (the PDF's or job folder's name) changed to `new`. A name counts
/// as a link when it starts the text or follows `[`, `(` or `/`, and when it is followed by `/`,
/// `.pdf`, `]`, `|`, `)`, `#` or the end.
pub fn rewrite_links(text: &str, old: &str, new: &str) -> String {
    if old.is_empty() || old == new {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    // `rest` is the text not yet copied; `before` is the character just before it in the original.
    let mut rest = text;
    let mut before: Option<char> = None;
    while let Some(i) = rest.find(old) {
        let head = rest.get(..i).unwrap_or("");
        let char_before = head.chars().last().or(before);
        let tail = rest.get(i + old.len()..).unwrap_or("");
        let before_ok = matches!(char_before, None | Some('[' | '(' | '/'));
        let after_ok = tail.is_empty() || tail.starts_with(['/', ']', '|', ')', '#']) || tail.starts_with(".pdf");
        out.push_str(head);
        out.push_str(if before_ok && after_ok { new } else { old });
        let consumed = i + old.len();
        before = rest.get(..consumed).and_then(|s| s.chars().last());
        rest = rest.get(consumed..).unwrap_or("");
    }
    out.push_str(rest);
    out
}

/// A name a file can have: not empty, no path separators or characters Windows forbids.
fn valid_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && name != "."
        && name != ".."
        && name.chars().count() <= 200
        && !name.chars().any(|c| "/\\:*?\"<>|".contains(c) || c.is_control())
}

/// What renaming the drawing `pdf` to `new_stem` would do. Nothing is changed.
pub fn plan(pdf: &Path, new_stem: &str) -> Result<Plan, String> {
    let new_stem = new_stem.trim();
    if !valid_name(new_stem) {
        return Err("that name can't be used for a file".into());
    }
    let dir = pdf.parent().ok_or("the drawing has no folder")?;
    let old_stem = pdf.file_stem().and_then(|s| s.to_str()).ok_or("the drawing has no name")?.to_string();
    if old_stem == new_stem {
        return Err("the drawing already has that name".into());
    }
    let ext = pdf.extension().and_then(|e| e.to_str()).unwrap_or("pdf");
    let new_pdf = dir.join(format!("{new_stem}.{ext}"));
    if new_pdf.exists() {
        return Err(format!("{new_stem}.{ext} is already there"));
    }
    let old_folder = dir.join(&old_stem);
    let folder = if old_folder.is_dir() {
        let new_folder = dir.join(new_stem);
        if new_folder.exists() {
            return Err(format!("the folder {new_stem} is already there"));
        }
        Some((old_folder.clone(), new_folder))
    } else {
        None
    };
    let mut notes = Vec::new();
    if folder.is_some() {
        let mut files = Vec::new();
        package::collect(&old_folder, 0, &mut files)?;
        for file in files.into_iter().filter(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("md"))) {
            let len = std::fs::metadata(&file).map_err(|e| e.to_string())?.len();
            if len > MAX_NOTE_BYTES {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&file) else { continue };
            let changed = rewrite_links(&text, &old_stem, new_stem);
            if changed != text {
                notes.push((file, changed));
            }
        }
    }
    Ok(Plan { old_pdf: pdf.to_path_buf(), new_pdf, folder, notes })
}

/// Make the changes `plan` worked out: the folder and the drawing are renamed, and the changed notes
/// are written under their new folder. If the drawing can't be renamed, the folder is put back.
pub fn apply(plan: &Plan) -> Result<(), String> {
    if let Some((old, new)) = &plan.folder {
        std::fs::rename(old, new).map_err(|e| format!("can't rename the folder: {e}"))?;
    }
    for (file, text) in &plan.notes {
        let now = match plan.folder.as_ref() {
            Some((old, new)) => match file.strip_prefix(old) {
                Ok(rel) => new.join(rel),
                Err(_) => file.clone(),
            },
            None => file.clone(),
        };
        if let Err(e) = std::fs::write(&now, text) {
            // The notes so far are written; the names stay as they are, and the error says why.
            return Err(format!("can't update {}: {e}", now.display()));
        }
    }
    if let Err(e) = std::fs::rename(&plan.old_pdf, &plan.new_pdf) {
        if let Some((old, new)) = &plan.folder {
            let _ = std::fs::rename(new, old);
        }
        return Err(format!("can't rename the drawing: {e}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
            let dir = std::env::temp_dir().join(format!("pdfcraft-rename-{tag}-{}-{nanos}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            Self(dir)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn links_to_the_old_name_are_rewritten_and_other_text_is_not() {
        let text = "[[Smith Residence/notes/Note 1]] and [[Smith Residence.pdf]] and [x](Smith Residence/a.xlsx)\nSmith Residence is the house. [[D1]] [[Smith Residence2/x]]";
        let out = rewrite_links(text, "Smith Residence", "Jones House");
        assert_eq!(
            out,
            "[[Jones House/notes/Note 1]] and [[Jones House.pdf]] and [x](Jones House/a.xlsx)\nSmith Residence is the house. [[D1]] [[Smith Residence2/x]]"
        );
    }

    #[test]
    fn the_same_name_or_an_empty_one_changes_nothing() {
        assert_eq!(rewrite_links("[[A/b]]", "A", "A"), "[[A/b]]");
        assert_eq!(rewrite_links("[[A/b]]", "", "C"), "[[A/b]]");
    }

    #[test]
    fn a_renamed_job_moves_its_folder_and_drawing_and_rewrites_its_notes() {
        let scratch = Scratch::new("job");
        let job = scratch.0.join("Smith Residence");
        std::fs::create_dir_all(job.join("notes")).unwrap();
        std::fs::write(scratch.0.join("Smith Residence.pdf"), b"%PDF").unwrap();
        std::fs::write(job.join("notes").join("Note 1.md"), "See [[Smith Residence/notes/Note 2]] and [[D1]].").unwrap();
        std::fs::write(job.join("notes").join("Note 2.md"), "No links here.").unwrap();

        let plan = plan(&scratch.0.join("Smith Residence.pdf"), "Jones House").unwrap();
        assert_eq!(plan.notes.len(), 1, "only the note that names the job changes");
        apply(&plan).unwrap();

        assert!(scratch.0.join("Jones House.pdf").exists());
        assert!(!scratch.0.join("Smith Residence.pdf").exists());
        let note = std::fs::read_to_string(scratch.0.join("Jones House").join("notes").join("Note 1.md")).unwrap();
        assert_eq!(note, "See [[Jones House/notes/Note 2]] and [[D1]].");
        assert!(scratch.0.join("Jones House").join("notes").join("Note 2.md").exists());
    }

    #[test]
    fn a_name_already_taken_or_not_allowed_changes_nothing() {
        let scratch = Scratch::new("taken");
        std::fs::write(scratch.0.join("A.pdf"), b"%PDF").unwrap();
        std::fs::write(scratch.0.join("B.pdf"), b"%PDF").unwrap();
        assert!(plan(&scratch.0.join("A.pdf"), "B").is_err());
        assert!(plan(&scratch.0.join("A.pdf"), "x/y").is_err());
        assert!(plan(&scratch.0.join("A.pdf"), "  ").is_err());
        assert!(plan(&scratch.0.join("A.pdf"), "A").is_err());
        assert!(scratch.0.join("A.pdf").exists());
    }
}
