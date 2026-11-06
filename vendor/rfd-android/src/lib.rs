//! PdfCraft for Android's stand-in for [rfd](https://crates.io/crates/rfd), which has no Android
//! backend.
//!
//! `crates/ui-egui` takes it as `rfd` on Android only (`extern crate pdfcraft_rfd_android as rfd` in its `lib.rs`), so the app's
//! `rfd::AsyncFileDialog::new().set_title(…)` calls compile unchanged there. The builder only
//! records what the picker should show; `crates/ui-egui/src/android_picker.rs` draws the picker
//! inside the app and reads these fields. It has no `pick_*` methods: the app never calls them on
//! Android (`platform/pickers.rs` hands the dialog to the in-app picker instead).

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

/// What a picker should show: the part of rfd's `AsyncFileDialog` builder PdfCraft uses.
#[derive(Clone, Debug, Default)]
pub struct AsyncFileDialog {
    /// The picker's title.
    pub title: Option<String>,
    /// (name, extensions without the dot, lower case), in the order added.
    pub filters: Vec<(String, Vec<String>)>,
    /// The suggested name of a file to save.
    pub file_name: Option<String>,
    /// The folder to start in.
    pub directory: Option<PathBuf>,
}

impl AsyncFileDialog {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn set_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    #[must_use]
    pub fn add_filter(mut self, name: impl Into<String>, extensions: &[impl ToString]) -> Self {
        let extensions = extensions.iter().map(|e| e.to_string().trim_start_matches('.').to_ascii_lowercase()).collect();
        self.filters.push((name.into(), extensions));
        self
    }

    #[must_use]
    pub fn set_directory(mut self, dir: impl AsRef<Path>) -> Self {
        self.directory = Some(dir.as_ref().to_path_buf());
        self
    }

    #[must_use]
    pub fn set_file_name(mut self, name: impl Into<String>) -> Self {
        self.file_name = Some(name.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builder_records_what_the_picker_shows() {
        let d = AsyncFileDialog::new().set_title("Open").add_filter("PDF", &[".PDF", "pdf"]).set_file_name("a.pdf").set_directory("/x");
        assert_eq!(d.title.as_deref(), Some("Open"));
        assert_eq!(d.filters, vec![("PDF".to_string(), vec!["pdf".to_string(), "pdf".to_string()])]);
        assert_eq!(d.file_name.as_deref(), Some("a.pdf"));
        assert_eq!(d.directory, Some(PathBuf::from("/x")));
    }
}
