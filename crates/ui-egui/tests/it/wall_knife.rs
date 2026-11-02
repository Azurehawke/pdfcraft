//! RevuCraft's wall knife in the real shell (egui_kittest): the command starts the Knife tool, and
//! the cut it makes goes through the same edit path as a click on a wall.

use egui_kittest::Harness;
use pdfcraft_engine::{Edit, Style};
use pdfcraft_ui_egui::{PdfCraftApp, QuickTool};

const FIXTURE: &[u8] = b"%PDF-1.7
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 400] >> endobj
3 0 obj << /Type /Page /Parent 2 0 R >> endobj
trailer << /Root 1 0 R >>
%%EOF";

fn harness() -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("doc.pdf", None, FIXTURE.to_vec()).unwrap();
        app.set_option("left", "closed").unwrap();
        app.set_option("zoom", "100").unwrap();
        app
    });
    h.run_steps(6);
    h
}

#[test]
fn the_wall_knife_command_starts_the_knife_tool() {
    let mut h = harness();
    assert!(h.state_mut().execute("edit.wall_knife"));
    assert_eq!(h.state().quick_tool, QuickTool::Knife);
}

#[test]
fn a_cut_on_a_plain_wall_applies_and_a_cut_past_its_end_is_refused() {
    let mut h = harness();
    let style = Style { color: [0.0; 3], opacity: 1.0, width: 0.75, fill: None };
    let wall = Edit::AddWall {
        page: 0,
        quad: [[40.0, 300.0], [200.0, 300.0], [200.0, 294.0], [40.0, 294.0]],
        ja: false,
        jb: false,
        style,
        author: "Steve".into(),
        nm: "w".into(),
        ja_with: String::new(),
        jb_with: String::new(),
    };
    assert!(h.state_mut().apply_edit(wall));
    assert!(h.state_mut().apply_edit(Edit::SplitWall { page: 0, index: 0, t: 0.5 }), "a cut inside the wall applies");
    assert!(!h.state_mut().apply_edit(Edit::SplitWall { page: 0, index: 0, t: 1.5 }), "a cut past the wall's end is refused");
}
