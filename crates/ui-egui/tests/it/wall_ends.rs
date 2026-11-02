//! RevuCraft: a smart wall's end connector stretches the wall from that end, and the other corners stay.

use egui::Pos2;
use egui_kittest::Harness;
use pdfcraft_engine::Edit;
use pdfcraft_ui_egui::PdfCraftApp;

/// One blank page, 300 × 400 points.
fn blank() -> Vec<u8> {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] >>".into(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let x = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offs {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

/// A wall along x from 50 to 250 (its centre at y 200), selected, at a zoom where its ends are clear.
fn harness() -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("wall.pdf", None, blank()).unwrap();
        app.set_option("zoom", "150").unwrap();
        let id = app.views[0].id;
        let style = pdfcraft_engine::Style { color: [0.0; 3], opacity: 1.0, width: 0.75, fill: None };
        let quad = [[50.0, 203.0], [250.0, 203.0], [250.0, 197.0], [50.0, 197.0]];
        app.session
            .apply(
                id,
                Edit::AddWall {
                    page: 0,
                    quad,
                    ja: false,
                    jb: false,
                    style,
                    author: "Steve".into(),
                    nm: "w".into(),
                    ja_with: String::new(),
                    jb_with: String::new(),
                },
            )
            .unwrap();
        app.views[0].comments.selected = Some((0, 0));
        app
    });
    h.run_steps(6);
    h
}

fn at(h: &Harness<'static, PdfCraftApp>, x: f32, y: f32) -> Pos2 {
    let r = h.state().views[0].page_screen_rect(0).unwrap();
    let k = r.width() / 300.0;
    r.min + egui::vec2(x * k, (400.0 - y) * k)
}

fn drag(h: &mut Harness<'static, PdfCraftApp>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.run_steps(1);
    h.drag_at(from);
    h.run_steps(1);
    for k in 1..=4 {
        h.hover_at(from + (to - from) * (k as f32 / 4.0));
        h.run_steps(1);
    }
    h.drop_at(to);
    h.run_steps(3);
}

#[test]
fn dragging_a_wall_end_connector_stretches_the_wall_from_that_end() {
    let mut h = harness();
    let before = h.state().session.get(h.state().views[0].id).unwrap().smart_walls(0)[0].quad;
    // The connector in the middle of the right end, dragged 40 points further along the wall.
    let from = at(&h, 250.0, 200.0);
    let to = at(&h, 290.0, 200.0);
    drag(&mut h, from, to);
    let state = h.state();
    let w = state.session.get(state.views[0].id).unwrap().smart_walls(0).into_iter().next().unwrap();
    assert!((w.quad[1][0] - 290.0).abs() < 1.0 && (w.quad[2][0] - 290.0).abs() < 1.0, "the end moved: {:?}", w.quad);
    assert_eq!(w.quad[0], before[0], "the other end stays put");
    assert_eq!(w.quad[3], before[3], "and so does its corner");
}
