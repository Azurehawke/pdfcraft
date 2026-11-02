//! The Package job: the PDF and its job folder (notes and photos) in one zip, so the drawing and
//! its notes travel together. The folder keeps its name inside the zip, so the links in the notes
//! (relative to the PDF) still work once the zip is unpacked beside the PDF.
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

/// The most files packaged, so a huge folder can't exhaust memory.
pub const MAX_FILES: usize = 5_000;
/// Folders nested deeper than this are left out.
pub const MAX_DEPTH: usize = 8;
/// The most bytes read from disk for one package.
pub const MAX_TOTAL_BYTES: u64 = 2 << 30;

/// The zip of `pdf` and, when there is one, every file under `folder`, below a folder of the same name.
pub fn package(pdf: &Path, folder: Option<&Path>) -> Result<Vec<u8>, String> {
    let pdf_name = pdf.file_name().and_then(|n| n.to_str()).ok_or("the drawing has no file name")?;
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut total = 0u64;
    let pdf_bytes = read_capped(pdf, &mut total)?;
    zip.start_file(pdf_name, options).map_err(|e| e.to_string())?;
    zip.write_all(&pdf_bytes).map_err(|e| e.to_string())?;
    if let Some(folder) = folder {
        let root = folder.file_name().and_then(|n| n.to_str()).ok_or("the folder has no name")?;
        let mut files = Vec::new();
        collect(folder, 0, &mut files)?;
        for file in files {
            let rel: Vec<&str> = match file.strip_prefix(folder) {
                Ok(rel) => rel.components().filter_map(|c| c.as_os_str().to_str()).collect(),
                Err(_) => continue,
            };
            let name = format!("{root}/{}", rel.join("/"));
            let bytes = read_capped(&file, &mut total)?;
            zip.start_file(name, options).map_err(|e| e.to_string())?;
            zip.write_all(&bytes).map_err(|e| e.to_string())?;
        }
    }
    zip.finish().map(Cursor::into_inner).map_err(|e| e.to_string())
}

/// The files under `dir` (not through symbolic links), at most [`MAX_DEPTH`] folders down.
fn collect(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    let entries = std::fs::read_dir(dir).map_err(|e| format!("can't read {}: {e}", dir.display()))?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            collect(&path, depth.saturating_add(1), out)?;
        } else if meta.is_file() {
            if out.len() >= MAX_FILES {
                return Err(format!("the folder has more than {MAX_FILES} files"));
            }
            out.push(path);
        }
    }
    Ok(())
}

/// The file's bytes, refusing a read that would take the package past [`MAX_TOTAL_BYTES`].
fn read_capped(path: &Path, total: &mut u64) -> Result<Vec<u8>, String> {
    let len = std::fs::metadata(path).map_err(|e| format!("can't read {}: {e}", path.display()))?.len();
    let next = total.saturating_add(len);
    if next > MAX_TOTAL_BYTES {
        return Err("the drawing and its folder are too large to package".into());
    }
    *total = next;
    std::fs::read(path).map_err(|e| format!("can't read {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// A fresh folder under the system temp dir, removed when the guard drops.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
            Self(std::env::temp_dir().join(format!("pdfcraft-package-{tag}-{}-{nanos}", std::process::id())))
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn names(zip_bytes: &[u8]) -> Vec<String> {
        let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes)).unwrap();
        let mut out: Vec<String> = (0..archive.len()).map(|i| archive.by_index(i).unwrap().name().to_string()).collect();
        out.sort();
        out
    }

    fn read_entry(zip_bytes: &[u8], name: &str) -> String {
        let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes)).unwrap();
        let mut text = String::new();
        archive.by_name(name).unwrap().read_to_string(&mut text).unwrap();
        text
    }

    #[test]
    fn the_drawing_and_its_notes_and_photos_go_in_one_zip() {
        let scratch = Scratch::new("all");
        let job = scratch.0.join("Smith Residence");
        std::fs::create_dir_all(job.join("notes")).unwrap();
        std::fs::create_dir_all(job.join("photos")).unwrap();
        std::fs::write(scratch.0.join("Smith Residence.pdf"), b"%PDF-1.7 drawing").unwrap();
        std::fs::write(job.join("notes").join("Note 1.md"), "# Note 1\n\n[[D1]]").unwrap();
        std::fs::write(job.join("photos").join("panel.jpg"), b"jpeg").unwrap();

        let zipped = package(&scratch.0.join("Smith Residence.pdf"), Some(&job)).unwrap();
        assert_eq!(names(&zipped), vec!["Smith Residence.pdf", "Smith Residence/notes/Note 1.md", "Smith Residence/photos/panel.jpg"]);
        assert_eq!(read_entry(&zipped, "Smith Residence/notes/Note 1.md"), "# Note 1\n\n[[D1]]");
    }

    #[test]
    fn without_a_folder_only_the_drawing_is_packaged() {
        let scratch = Scratch::new("pdf");
        std::fs::create_dir_all(&scratch.0).unwrap();
        std::fs::write(scratch.0.join("Plan.pdf"), b"%PDF-1.7").unwrap();
        let zipped = package(&scratch.0.join("Plan.pdf"), None).unwrap();
        assert_eq!(names(&zipped), vec!["Plan.pdf"]);
    }

    #[test]
    fn folders_below_the_depth_limit_are_left_out() {
        let scratch = Scratch::new("deep");
        let job = scratch.0.join("Job");
        let mut deep = job.clone();
        for i in 0..=MAX_DEPTH + 1 {
            deep = deep.join(format!("d{i}"));
        }
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("far.md"), "x").unwrap();
        std::fs::write(job.join("near.md"), "y").unwrap();
        std::fs::write(scratch.0.join("Job.pdf"), b"%PDF").unwrap();
        let zipped = package(&scratch.0.join("Job.pdf"), Some(&job)).unwrap();
        let all = names(&zipped);
        assert!(all.contains(&"Job/near.md".to_string()));
        assert!(!all.iter().any(|n| n.ends_with("far.md")), "{all:?}");
    }

    #[test]
    fn a_drawing_that_is_not_there_is_an_error() {
        let scratch = Scratch::new("missing");
        assert!(package(&scratch.0.join("Gone.pdf"), None).is_err());
    }
}
