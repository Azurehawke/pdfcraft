//! The PlanCraft sidecar on save: `markups.json` in the drawing's job folder, written only while
//! Link to PlanCraft is on (docs/sidecar.md).

use egui_kittest::Harness;
use pdfcraft_ui_egui::{PdfCraftApp, SaveTarget};

/// One page holding `annots`, an inline `/Annots` array.
fn pdf(annots: &str) -> Vec<u8> {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [{annots}] >>"),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

/// A drawing with a space called "Server room" and one punch item inside it.
fn drawing() -> Vec<u8> {
    pdf(concat!(
        "<< /Type /Annot /Subtype /Square /Rect [0 0 200 200] /NM (room) /Contents (Server room) /PCSpace true >> ",
        "<< /Type /Annot /Subtype /Square /Rect [10 10 30 30] /NM (p1) /PCPunch << /No 1 /Assignee (Electrician) /Due (2026-11-02) /Status /Open >> >>",
    ))
}

/// An empty folder for one test's drawing, and the drawing's path in it.
fn scratch(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("plancraft-sidecar-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("makes a scratch folder");
    let pdf = dir.join("A-201 Floor plan.pdf");
    (dir, pdf)
}

fn harness() -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("A-201 Floor plan.pdf", None, drawing()).expect("opens");
        app
    });
    h.run_steps(3);
    h
}

#[test]
fn markups_json_goes_in_the_job_folder_when_the_link_is_on() {
    let (dir, pdf) = scratch("on");
    let mut h = harness();
    h.state_mut().set_plancraft_link(true);
    h.state_mut().save_override = Some(pdf.to_string_lossy().into_owned());
    assert!(h.state_mut().save_active(SaveTarget::As));
    let json = std::fs::read_to_string(dir.join("A-201 Floor plan").join("markups.json")).expect("the sidecar is written");
    assert!(json.contains("\"name\": \"p1\""), "{json}");
    assert!(json.contains("\"space\": \"Server room\""), "{json}");
    assert!(json.contains("\"due\": \"2026-11-02\""), "{json}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn nothing_is_written_while_the_link_is_off() {
    let (dir, pdf) = scratch("off");
    let mut h = harness();
    h.state_mut().save_override = Some(pdf.to_string_lossy().into_owned());
    assert!(h.state_mut().save_active(SaveTarget::As));
    assert!(pdf.exists(), "the drawing itself is saved");
    assert!(!dir.join("A-201 Floor plan").exists(), "no job folder, no sidecar");
    let _ = std::fs::remove_dir_all(dir);
}
