//! A markup's right-click menu has Move to Layer, listing the document's layers.

use egui::Pos2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::PdfCraftApp;

/// One page with two layers, Red and Green (both shown), and a red square markup, not on a layer.
fn fixture() -> Vec<u8> {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [4 0 R 5 0 R] /D << /ON [4 0 R 5 0 R] >> >> >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] /Annots [6 0 R] >>".into(),
        "<< /Type /OCG /Name (Red) >>".into(),
        "<< /Type /OCG /Name (Green) >>".into(),
        "<< /Type /Annot /Subtype /Square /Rect [20 20 120 120] /NM (m1) /C [1 0 0] /P 3 0 R >>".into(),
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

fn harness() -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("layers.pdf", None, fixture()).unwrap();
        app.set_option("zoom", "150").unwrap();
        app.views[0].comments.selected = Some((0, 0));
        app
    });
    h.run_steps(6);
    h
}

/// The screen position of a point in PDF user space (y up) on the 300 × 400 page.
fn at(h: &Harness<'static, PdfCraftApp>, x: f32, y: f32) -> Pos2 {
    let r = h.state().views[0].page_screen_rect(0).unwrap();
    let k = r.width() / 300.0;
    r.min + egui::vec2(x * k, (400.0 - y) * k)
}

fn right_click(h: &mut Harness<'static, PdfCraftApp>, p: Pos2) {
    h.hover_at(p);
    h.run_steps(1);
    h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Secondary, pressed: true, modifiers: Default::default() });
    h.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Secondary, pressed: false, modifiers: Default::default() });
    h.run_steps(3);
}

/// The name of the layer the markup is on, if any.
fn layer_of_markup(h: &Harness<'static, PdfCraftApp>) -> Option<String> {
    let s = h.state();
    let id = s.views[0].id;
    let info = &s.session.get(id).unwrap().info;
    let markup = info.annotations.iter().find(|a| a.page == 0 && a.index == 0).expect("the markup");
    let layers = s.session.layer_details(id);
    markup.layer.and_then(|c| layers.iter().find(|l| (l.id.num, l.id.generation) == c)).map(|l| l.name.clone())
}

#[test]
fn a_markup_moves_to_a_layer_from_its_context_menu() {
    let mut h = harness();
    assert_eq!(layer_of_markup(&h), None);
    let p = at(&h, 70.0, 70.0);
    right_click(&mut h, p);
    h.get_by_label_contains("Move to Layer").click();
    h.run_steps(3);
    assert!(h.query_by_label("Red").is_some(), "the menu lists Red");
    assert!(h.query_by_label("Green").is_some(), "the menu lists Green");
    h.get_by_label("Green").click();
    h.run_steps(3);
    assert_eq!(layer_of_markup(&h).as_deref(), Some("Green"));
}
