//! Scan & OCR ▸ Scan with camera (Android): take pages with the device's camera app.
//!
//! Each photo becomes a page: the first starts a new document, later ones are added at its end.
//! The page is straightened at the edges found on it and cleaned up (Enhance camera image's
//! Colour look at its default level), then a prompt offers another page or Done. Done offers
//! Recognize text, since OCR reads the cleaned-up pages best. Elsewhere there is no camera to
//! use, and the command says so.

#[cfg(target_os = "android")]
use std::sync::{Mutex, PoisonError};

use crate::PdfCraftApp;

#[cfg(target_os = "android")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// The camera app is showing.
    Camera,
    /// A page was added: another, or Done.
    Added,
    /// Done: recognize the text now, or not.
    Ocr,
}

#[cfg(target_os = "android")]
#[derive(Clone, Copy, Debug)]
struct Scan {
    step: Step,
    /// The document the pages go into, once the first page made it.
    doc: Option<pdfcraft_engine::DocId>,
    pages: usize,
    /// Pages whose edges weren't found (cleaned up without straightening).
    unstraightened: usize,
}

#[cfg(target_os = "android")]
static SCAN: Mutex<Option<Scan>> = Mutex::new(None);

#[cfg(target_os = "android")]
fn get() -> Option<Scan> {
    *SCAN.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(target_os = "android")]
fn set(scan: Option<Scan>) {
    *SCAN.lock().unwrap_or_else(PoisonError::into_inner) = scan;
}

/// Scan with camera: open the camera for the first page (or the next one, in a scan already
/// under way).
pub(crate) fn start(app: &mut PdfCraftApp) {
    #[cfg(target_os = "android")]
    {
        let scan = get().unwrap_or(Scan { step: Step::Camera, doc: None, pages: 0, unstraightened: 0 });
        camera(app, scan);
    }
    #[cfg(not(target_os = "android"))]
    app.notify_tr("Scan with camera is part of PdfCraft for Android.");
}

#[cfg(target_os = "android")]
fn camera(app: &mut PdfCraftApp, scan: Scan) {
    let Some(cam) = crate::android_picker::camera() else {
        set(None);
        return app.notify_tr("This build can't use the camera.");
    };
    match (cam.start)() {
        Ok(()) => set(Some(Scan { step: Step::Camera, ..scan })),
        Err(e) => {
            set(if scan.pages > 0 { Some(Scan { step: Step::Added, ..scan }) } else { None });
            app.notify_error(e);
        }
    }
}

/// Each frame: pick up the photo when the camera app is back, and draw the prompts.
pub(crate) fn frame(app: &mut PdfCraftApp, ctx: &egui::Context) {
    #[cfg(target_os = "android")]
    {
        let Some(scan) = get() else { return };
        match scan.step {
            Step::Camera => poll(app, ctx, scan),
            Step::Added | Step::Ocr => prompt(app, ctx, scan),
        }
    }
    #[cfg(not(target_os = "android"))]
    let _ = (app, ctx);
}

#[cfg(target_os = "android")]
fn poll(app: &mut PdfCraftApp, ctx: &egui::Context, scan: Scan) {
    let Some(cam) = crate::android_picker::camera() else { return set(None) };
    let answer = match (cam.poll)() {
        None => {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
            return;
        }
        Some(a) => a,
    };
    // Cancelled in the camera app: back to the prompt, or nothing when no page was taken yet.
    let back = |scan: Scan| if scan.pages > 0 { Some(Scan { step: Step::Added, ..scan }) } else { None };
    let path = match answer {
        Ok(Some(path)) => path,
        Ok(None) => return set(back(scan)),
        Err(e) => {
            set(back(scan));
            return app.notify_error(e);
        }
    };
    let bytes = std::fs::read(&path);
    // The copy in the cache has served its purpose either way.
    let _ = std::fs::remove_file(&path);
    let bytes = match bytes {
        Ok(b) => b,
        Err(e) => {
            set(back(scan));
            return app.notify_fmt("Couldn't read the photo: {e}", &[("e", &e.to_string())]);
        }
    };
    match add_page(app, scan, bytes) {
        Ok(scan) => set(Some(scan)),
        Err(e) => {
            set(back(scan));
            app.notify_error(e);
        }
    }
    ctx.request_repaint();
}

/// Put the photo in as the scan's next page, straightened and cleaned up.
#[cfg(target_os = "android")]
fn add_page(app: &mut PdfCraftApp, scan: Scan, bytes: Vec<u8>) -> Result<Scan, String> {
    let pdf = app.session.create_from_images(&[("scan.jpg".into(), bytes)]).map_err(|e| e.to_string())?;
    // The scan's document, if its tab is still open.
    let open = scan.doc.and_then(|id| app.views.iter().position(|v| v.id == id).map(|i| (i, id)));
    let (i, id, page) = match open {
        Some((i, id)) => {
            let at = app.session.get(id).map_or(0, |d| d.info.pages.len());
            app.active = Some(i);
            let edit = pdfcraft_engine::Edit::InsertPagesFrom {
                name: "scan.pdf".into(),
                bytes: pdf,
                pages: None,
                at,
                field_conflicts: pdfcraft_engine::FieldConflicts::Rename,
            };
            if !app.apply_edit(edit) {
                return Err(tl!("The page couldn't be added").to_string());
            }
            (i, id, at)
        }
        None => {
            app.open_created("Scan.pdf", pdf)?;
            let i = app.views.len().checked_sub(1).ok_or("the scan didn't open")?;
            (i, app.views.get(i).map(|v| v.id).ok_or("the scan didn't open")?, 0)
        }
    };
    let found = app.session.enhance_camera_page_auto(id, page, crate::camera_ui::Look::Colour, crate::camera_ui::DEFAULT_LEVEL);
    if let (Some(doc), Some(view)) = (app.session.get(id), app.views.get_mut(i)) {
        view.document_changed(&doc.info);
        view.go_to_page(page);
    }
    let unstraightened = match found {
        Ok(true) => scan.unstraightened,
        Ok(false) => scan.unstraightened + 1,
        // The photo stays as taken: Enhance camera image can straighten it later.
        Err(e) => {
            app.notify_fmt("The page was added as photographed: {e}", &[("e", &e.to_string())]);
            scan.unstraightened + 1
        }
    };
    Ok(Scan { step: Step::Added, doc: Some(id), pages: scan.pages + 1, unstraightened })
}

#[cfg(target_os = "android")]
fn prompt(app: &mut PdfCraftApp, ctx: &egui::Context, scan: Scan) {
    // The scan's tab was closed: nothing to add to.
    if scan.doc.is_none_or(|id| !app.views.iter().any(|v| v.id == id)) {
        return set(None);
    }
    let mut next: Option<Option<Scan>> = None;
    let mut ocr = false;
    let mut more = false;
    egui::Area::new(egui::Id::new("scan-camera-prompt"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -48.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(14.0).show(ui, |ui| {
                ui.set_max_width(420.0);
                let row = egui::vec2(0.0, 40.0);
                match scan.step {
                    Step::Added => {
                        let title = if scan.pages == 1 {
                            tl!("Page 1 added").to_string()
                        } else {
                            crate::i18n::fmt(tl!("Page {n} added"), &[("n", &scan.pages.to_string())])
                        };
                        ui.label(egui::RichText::new(title).strong());
                        ui.horizontal(|ui| {
                            if ui.add(egui::Button::new(egui::RichText::new(tl!("Add another page")).strong()).min_size(row)).clicked() {
                                more = true;
                            }
                            if ui.add(egui::Button::new(tl!("Done")).min_size(row)).clicked() {
                                next = Some(Some(Scan { step: Step::Ocr, ..scan }));
                            }
                        });
                    }
                    Step::Ocr => {
                        ui.label(egui::RichText::new(tl!("Recognize the text now?")).strong());
                        ui.label(tl!("Makes the scan searchable and its text selectable."));
                        if scan.unstraightened > 0 {
                            ui.label(
                                egui::RichText::new(crate::i18n::fmt(
                                    tl!("On {n} pages the edges weren't found: Enhance camera image can straighten them."),
                                    &[("n", &scan.unstraightened.to_string())],
                                ))
                                .italics(),
                            );
                        }
                        ui.horizontal(|ui| {
                            if ui.add(egui::Button::new(egui::RichText::new(tl!("Recognize text")).strong()).min_size(row)).clicked() {
                                ocr = true;
                                next = Some(None);
                            }
                            if ui.add(egui::Button::new(tl!("Not now")).min_size(row)).clicked() {
                                next = Some(None);
                            }
                        });
                    }
                    Step::Camera => {}
                }
            });
        });
    if more {
        return camera(app, scan);
    }
    if let Some(n) = next {
        set(n);
    }
    if ocr {
        if let Some(i) = scan.doc.and_then(|id| app.views.iter().position(|v| v.id == id)) {
            app.active = Some(i);
        }
        app.execute("ocr.recognize");
    }
}
