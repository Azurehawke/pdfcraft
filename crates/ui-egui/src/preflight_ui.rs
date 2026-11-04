//! Preflight before a print: the checklist, and which results stop the print.
//!
//! The checks come from the engine (`pdfcraft_engine::printcheck`). A Fail stops the print. Warn
//! and Info print, and are listed so the person can read them first. Saving a PDF is never
//! stopped.

use egui::Color32;
use pdfcraft_engine::printcheck::{Check, Status};

use crate::PdfCraftApp;
use crate::theme::{self, Tokens};
use crate::widgets;

const FAIL: Color32 = Color32::from_rgb(196, 48, 48);
const WARN: Color32 = Color32::from_rgb(176, 104, 0);
const PASS: Color32 = Color32::from_rgb(38, 130, 66);

/// Whether a print to a printer should stop at the checklist: some check is not a pass.
pub(crate) fn opens(checks: &[Check]) -> bool {
    checks.iter().any(|c| c.status != Status::Pass)
}

/// Whether the print may go ahead: no check failed.
pub(crate) fn may_print(checks: &[Check]) -> bool {
    !checks.iter().any(|c| c.status.blocks_print())
}

impl PdfCraftApp {
    /// The checklist a print to a printer must clear first, or `None` when there is nothing to
    /// stop for: saving a PDF is never stopped, and a print whose checks all pass goes at once.
    pub(crate) fn preflight_for_print(&self) -> Option<Vec<Check>> {
        self.print_draft.printer.as_ref()?;
        let (_, id) = self.active_ids()?;
        let checks = self.session.get(id)?.print_checklist();
        opens(&checks).then_some(checks)
    }
}

fn badge(status: Status) -> (&'static str, Color32) {
    match status {
        Status::Pass => ("Pass", PASS),
        Status::Info => ("Info", Color32::GRAY),
        Status::Warn => ("Warn", WARN),
        Status::Fail => ("Fail", FAIL),
    }
}

/// The checklist dialog. Returns (print anyway, cancel).
pub(crate) fn body(ui: &mut egui::Ui, checks: &[Check], t: &Tokens) -> (bool, bool) {
    ui.set_width(560.0);
    ui.label(egui::RichText::new(tl!("Preflight before printing")).font(theme::semibold(18.0)));
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(tl!("A Fail stops the print. Warn and Info print, and are listed so you can check them first.")).color(t.text_muted),
    );
    ui.add_space(8.0);
    for check in checks {
        let (word, color) = badge(check.status);
        ui.horizontal_top(|ui| {
            ui.add_sized([44.0, 18.0], egui::Label::new(egui::RichText::new(word).strong().color(color)));
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(check.title).strong());
                ui.label(egui::RichText::new(&check.detail).small().color(t.text_muted));
            });
        });
        ui.add_space(4.0);
    }
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(tl!("Not checked: PDF/X, overprint, total area coverage, spot colours, transparency flattening."))
            .small()
            .color(t.text_faint),
    );
    ui.add_space(10.0);
    let (mut go, mut cancel) = (false, false);
    ui.horizontal(|ui| {
        if checks.iter().any(|c| c.status.blocks_print()) {
            ui.label(egui::RichText::new(tl!("Fix the items marked Fail, then print again.")).color(FAIL));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled_ui(may_print(checks), |ui| widgets::pill_button(ui, tl!("Print"), true)).inner.clicked() {
                go = true;
            }
            if widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                cancel = true;
            }
        });
    });
    (go, cancel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(status: Status) -> Check {
        Check { title: "t", status, detail: String::new() }
    }

    #[test]
    fn a_failed_check_never_lets_the_print_go() {
        assert!(!may_print(&[check(Status::Pass), check(Status::Fail)]));
        assert!(!may_print(&[check(Status::Warn), check(Status::Fail)]));
    }

    #[test]
    fn warnings_and_info_print_but_open_the_checklist() {
        let soft = [check(Status::Warn), check(Status::Info)];
        assert!(may_print(&soft));
        assert!(opens(&soft));
        assert!(!opens(&[check(Status::Pass)]));
    }
}
