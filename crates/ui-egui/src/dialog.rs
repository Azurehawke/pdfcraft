//! The file-picker builder the UI asks with.
//!
//! Everywhere but Android this is rfd itself. rfd has no Android backend, so there
//! [`AsyncFileDialog`] only records what the picker should show (title, filters, suggested name),
//! and `pickers` draws the picker inside the app (`android_picker`).

#[cfg(not(target_os = "android"))]
pub use rfd::*;

/// What a picker should show: the subset of `rfd::AsyncFileDialog`'s builder the UI uses.
#[cfg(target_os = "android")]
#[derive(Clone, Debug, Default)]
pub struct AsyncFileDialog {
    pub(crate) title: Option<String>,
    /// (name, extensions without the dot), in the order added.
    pub(crate) filters: Vec<(String, Vec<String>)>,
    /// The suggested name of a file to save.
    pub(crate) file_name: Option<String>,
}

#[cfg(target_os = "android")]
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
    pub fn set_file_name(mut self, name: impl Into<String>) -> Self {
        self.file_name = Some(name.into());
        self
    }
}
