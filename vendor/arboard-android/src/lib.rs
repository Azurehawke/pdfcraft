//! PdfCraft for Android's stand-in for [arboard](https://crates.io/crates/arboard), which has no
//! Android backend.
//!
//! `crates/ui-egui` takes it as `arboard` on Android only (`extern crate pdfcraft_arboard_android as arboard` in its `lib.rs`), so the app's
//! clipboard code compiles unchanged there. [`Clipboard::new`] always fails with
//! [`Error::ClipboardNotSupported`], which the app already handles as "no system clipboard": copy
//! and paste then use the app's own clipboard, as in the web build. Text copy and paste in
//! text fields still work through egui.

#![forbid(unsafe_code)]

use std::borrow::Cow;
use std::path::Path;

/// Why a clipboard call failed. On Android it is always [`Error::ClipboardNotSupported`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    ContentNotAvailable,
    ClipboardNotSupported,
    ClipboardOccupied,
    ConversionFailure,
    Unknown { description: String },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::ContentNotAvailable => f.write_str("The clipboard has nothing of the requested kind"),
            Error::ClipboardNotSupported => f.write_str("This device has no clipboard PdfCraft can use"),
            Error::ClipboardOccupied => f.write_str("The clipboard is in use"),
            Error::ConversionFailure => f.write_str("The clipboard's contents couldn't be converted"),
            Error::Unknown { description } => f.write_str(description),
        }
    }
}

impl std::error::Error for Error {}

/// An RGBA picture, as arboard passes images.
#[derive(Debug, Clone)]
pub struct ImageData<'a> {
    pub width: usize,
    pub height: usize,
    pub bytes: Cow<'a, [u8]>,
}

/// Never made: [`Clipboard::new`] always fails.
pub struct Clipboard(());

impl Clipboard {
    pub fn new() -> Result<Self, Error> {
        Err(Error::ClipboardNotSupported)
    }
    pub fn get_text(&mut self) -> Result<String, Error> {
        Err(Error::ClipboardNotSupported)
    }
    pub fn set_text<'a>(&mut self, _text: impl Into<Cow<'a, str>>) -> Result<(), Error> {
        Err(Error::ClipboardNotSupported)
    }
    pub fn get_image(&mut self) -> Result<ImageData<'static>, Error> {
        Err(Error::ClipboardNotSupported)
    }
    pub fn set_image(&mut self, _image: ImageData<'_>) -> Result<(), Error> {
        Err(Error::ClipboardNotSupported)
    }
    pub fn set(&mut self) -> Set<'_> {
        Set(self)
    }
}

/// arboard's `Clipboard::set()` builder.
pub struct Set<'a>(&'a mut Clipboard);

impl Set<'_> {
    pub fn file_list(self, _paths: &[impl AsRef<Path>]) -> Result<(), Error> {
        Err(Error::ClipboardNotSupported)
    }
    pub fn text<'b>(self, text: impl Into<Cow<'b, str>>) -> Result<(), Error> {
        self.0.set_text(text)
    }
    pub fn image(self, image: ImageData<'_>) -> Result<(), Error> {
        self.0.set_image(image)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_is_no_clipboard() {
        assert_eq!(Clipboard::new().err(), Some(Error::ClipboardNotSupported));
    }
}
