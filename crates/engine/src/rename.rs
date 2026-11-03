//! The Rename job: the drawing and its folders get a new name together. The folders are the job
//! folder (notes made before comments were files), `<name>.comments` and `<name>.images`. Comment
//! files named with the old name are renamed, and the `[[links]]` in notes that name the old
//! drawing or a folder are rewritten to the new one. Links to markups (`[[label]]`) name markups,
//! not files, so they are left as they are.
//!
//! [`plan`] reads and works out everything without touching a file; [`apply`] makes the changes.
use std::path::{Path, PathBuf};

use crate::{comments, package};

/// The most bytes read from one note.
const MAX_NOTE_BYTES: u64 = 1 << 20;

/// What a rename will do, worked out before any file changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub old_pdf: PathBuf,
    pub new_pdf: PathBuf,
    /// The folders renamed with the drawing, as (current, new).
    pub folders: Vec<(PathBuf, PathBuf)>,
    /// Comment files renamed inside their folder, as (current, new), both in full.
    pub files: Vec<(PathBuf, PathBuf)>,
    /// Each note file whose text changes: its current path and the new text.
    pub notes: Vec<(PathBuf, String)>,
}

impl Plan {
    /// Where the file at `path` (as it is now) is once the plan is applied.
    pub fn moved(&self, path: &Path) -> PathBuf {
        match self.files.iter().find(|(from, _)| from == path) {
            Some((_, to)) => to.clone(),
            None => self.in_renamed_folder(path),
        }
    }

    /// Where `path` is once its folder is renamed (the file's own name is not changed yet).
    fn in_renamed_folder(&self, path: &Path) -> PathBuf {
        for (old, new) in &self.folders {
            if let Ok(rel) = path.strip_prefix(old) {
                return new.join(rel);
            }
        }
        path.to_path_buf()
    }
}

/// `text` with each link to `old` (the PDF's name, a folder's name or a comment file's start)
/// changed to `new`. A name counts as a link when it starts the text or follows `[`, `(` or `/`,
/// and it is followed by `/`, `.pdf`, `.images/`, `.comments/`, `]`, `|`, `)`, `#` or the end. A
/// comment file's name (`old-…`) counts when it follows `/`, so `[[Old.comments/Old-001-…]]` moves.
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
        let after_ok = tail.is_empty()
            || tail.starts_with(['/', ']', '|', ')', '#'])
            || tail.starts_with(".pdf")
            || tail.starts_with(".images/")
            || tail.starts_with(".comments/")
            || (tail.starts_with('-') && char_before == Some('/'));
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

    // The folders beside the drawing: the job folder, the comments and the images.
    let old_comments = comments::comments_dir(pdf);
    let new_comments = comments::comments_dir(&new_pdf);
    let pairs = [
        (dir.join(&old_stem), dir.join(new_stem)),
        (old_comments.clone(), new_comments.clone()),
        (comments::images_dir(pdf), comments::images_dir(&new_pdf)),
    ];
    let mut folders = Vec::new();
    for (old, new) in pairs {
        if old == new || !old.is_dir() {
            continue;
        }
        if new.exists() {
            let name = new.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            return Err(format!("the folder {name} is already there"));
        }
        folders.push((old, new));
    }

    // The comment files named with the old name take the new one.
    let old_clean = comments::clean_stem(&old_stem);
    let new_clean = comments::clean_stem(new_stem);
    let mut files = Vec::new();
    if old_clean != new_clean && old_comments.is_dir() {
        let prefix = format!("{old_clean}-");
        let entries = std::fs::read_dir(&old_comments).map_err(|e| format!("can't read the comments folder: {e}"))?;
        for entry in entries.filter_map(Result::ok).take(package::MAX_FILES) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(rest) = name.strip_prefix(&prefix) else { continue };
            if !name.ends_with(".md") || !entry.path().is_file() {
                continue;
            }
            files.push((entry.path(), new_comments.join(format!("{new_clean}-{rest}"))));
        }
    }

    // Each note's text, with its links to the old names rewritten.
    let mut scanned = Vec::new();
    for (folder, _) in &folders {
        package::collect(folder, 0, &mut scanned)?;
    }
    let mut notes = Vec::new();
    for file in scanned.into_iter().filter(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("md"))) {
        let len = std::fs::metadata(&file).map_err(|e| e.to_string())?.len();
        if len > MAX_NOTE_BYTES {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        let changed = rewrite_links(&rewrite_links(&text, &old_stem, new_stem), &old_clean, &new_clean);
        if changed != text {
            notes.push((file, changed));
        }
    }
    Ok(Plan { old_pdf: pdf.to_path_buf(), new_pdf, folders, files, notes })
}

/// Make the changes `plan` worked out: the folders are renamed, then the comment files, then the
/// changed notes are written under their new names, and the drawing is renamed last. If a step
/// fails, the steps before it are put back, so the drawing and its notes keep their old names.
pub fn apply(plan: &Plan) -> Result<(), String> {
    // Renames so far, and each note's text before it was written: what a failure undoes.
    let mut renamed: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut written: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    for (old, new) in &plan.folders {
        if let Err(e) = std::fs::rename(old, new) {
            return Err(undo(&renamed, &written, format!("can't rename the folder: {e}")));
        }
        renamed.push((old.clone(), new.clone()));
    }
    for (old, new) in &plan.files {
        let from = plan.in_renamed_folder(old);
        if let Err(e) = std::fs::rename(&from, new) {
            return Err(undo(&renamed, &written, format!("can't rename {}: {e}", from.display())));
        }
        renamed.push((from, new.clone()));
    }
    for (file, text) in &plan.notes {
        let dest = plan.moved(file);
        let before = std::fs::read(&dest).unwrap_or_default();
        if let Err(e) = std::fs::write(&dest, text) {
            return Err(undo(&renamed, &written, format!("can't update {}: {e}", dest.display())));
        }
        written.push((dest, before));
    }
    if let Err(e) = std::fs::rename(&plan.old_pdf, &plan.new_pdf) {
        return Err(undo(&renamed, &written, format!("can't rename the drawing: {e}")));
    }
    Ok(())
}

/// Puts back the notes written and the renames made, newest first, and returns `message`.
fn undo(renamed: &[(PathBuf, PathBuf)], written: &[(PathBuf, Vec<u8>)], message: String) -> String {
    for (path, before) in written.iter().rev() {
        let _ = std::fs::write(path, before);
    }
    for (from, to) in renamed.iter().rev() {
        let _ = std::fs::rename(to, from);
    }
    message
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
    fn the_folders_and_comment_file_names_in_links_move_but_other_dashes_do_not() {
        let text = "[[Smith-Residence.images/a.jpg]] [[Smith-Residence.comments/Smith-Residence-001-x]] [[Smith-Residence-notes]]";
        assert_eq!(
            rewrite_links(text, "Smith-Residence", "Jones-House"),
            "[[Jones-House.images/a.jpg]] [[Jones-House.comments/Jones-House-001-x]] [[Smith-Residence-notes]]"
        );
    }

    #[test]
    fn the_same_name_or_an_empty_one_changes_nothing() {
        assert_eq!(rewrite_links("[[A/b]]", "A", "A"), "[[A/b]]");
        assert_eq!(rewrite_links("[[A/b]]", "", "C"), "[[A/b]]");
    }

    #[test]
    fn a_renamed_drawing_moves_its_folders_and_comment_files_and_rewrites_their_links() {
        let scratch = Scratch::new("job");
        let job = scratch.0.join("Smith Residence");
        std::fs::create_dir_all(job.join("notes")).unwrap();
        std::fs::write(scratch.0.join("Smith Residence.pdf"), b"%PDF").unwrap();
        std::fs::write(job.join("notes").join("Note 1.md"), "See [[Smith Residence/notes/Note 2]] and [[D1]].").unwrap();
        std::fs::write(job.join("notes").join("Note 2.md"), "No links here.").unwrap();
        // The comments and images folders beside the drawing.
        let comments = scratch.0.join("Smith-Residence.comments");
        let images = scratch.0.join("Smith-Residence.images");
        std::fs::create_dir_all(&comments).unwrap();
        std::fs::create_dir_all(&images).unwrap();
        let first = "Smith-Residence-001-20261103-120000";
        std::fs::write(comments.join(format!("{first}.md")), "# Panel\n\n[[Smith-Residence.images/IMG_1.jpg]]").unwrap();
        std::fs::write(comments.join("Smith-Residence-002-20261103-120500.md"), format!("Also [[Smith-Residence.comments/{first}]]")).unwrap();
        std::fs::write(images.join("IMG_1.jpg"), b"jpeg").unwrap();

        let plan = plan(&scratch.0.join("Smith Residence.pdf"), "Jones House").unwrap();
        assert_eq!(plan.files.len(), 2, "both comment files take the new name");
        apply(&plan).unwrap();

        assert!(scratch.0.join("Jones House.pdf").exists());
        assert!(!scratch.0.join("Smith Residence.pdf").exists());
        let note = std::fs::read_to_string(scratch.0.join("Jones House").join("notes").join("Note 1.md")).unwrap();
        assert_eq!(note, "See [[Jones House/notes/Note 2]] and [[D1]].");
        let new_comments = scratch.0.join("Jones-House.comments");
        assert!(!comments.exists());
        let moved = std::fs::read_to_string(new_comments.join("Jones-House-001-20261103-120000.md")).unwrap();
        assert_eq!(moved, "# Panel\n\n[[Jones-House.images/IMG_1.jpg]]");
        let second = std::fs::read_to_string(new_comments.join("Jones-House-002-20261103-120500.md")).unwrap();
        assert_eq!(second, "Also [[Jones-House.comments/Jones-House-001-20261103-120000]]");
        assert!(scratch.0.join("Jones-House.images").join("IMG_1.jpg").exists(), "photos keep their names");
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

    #[test]
    fn a_folder_already_named_for_the_new_name_stops_the_rename() {
        let scratch = Scratch::new("folder-taken");
        std::fs::write(scratch.0.join("A.pdf"), b"%PDF").unwrap();
        std::fs::create_dir_all(scratch.0.join("A.images")).unwrap();
        std::fs::create_dir_all(scratch.0.join("B.images")).unwrap();
        assert!(plan(&scratch.0.join("A.pdf"), "B").is_err());
        assert!(scratch.0.join("A.images").is_dir());
    }
}
