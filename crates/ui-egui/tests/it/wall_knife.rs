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

const ROOM: &[u8] = b"%PDF-1.7
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 500] >> endobj
3 0 obj << /Type /Page /Parent 2 0 R >> endobj
trailer << /Root 1 0 R >>
%%EOF";

/// The demo room (four joined walls and a partition with a door and a window), shown in full.
fn room() -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(|_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("room.pdf", None, ROOM.to_vec()).unwrap();
        app.set_option("left", "closed").unwrap();
        app.set_option("zoom", "100").unwrap();
        app.set_option("demo-walls", "1").unwrap();
        app
    });
    h.run_steps(8);
    h
}

/// Saves a picture of the room when `PDFCRAFT_SHOTS` names a folder; the assertions do not need it.
fn shot(h: &mut Harness<'static, PdfCraftApp>, name: &str) {
    if let Ok(dir) = std::env::var("PDFCRAFT_SHOTS") {
        // Pages render in the background: wait for the page to be drawn after the last edit.
        for _ in 0..40 {
            h.run_steps(3);
            if !h.state().render_pending() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        h.run_steps(3);
        h.render().unwrap().save(std::path::Path::new(&dir).join(name)).unwrap();
    }
}

#[test]
fn the_knife_cuts_a_plain_wall_of_the_room_and_deleting_the_piece_removes_it() {
    let mut h = room();
    let (_, id) = h.state().active_ids().unwrap();
    shot(&mut h, "knife-0-room.png");
    // The right-hand wall has no door or window in it, so it can be cut.
    let right: Vec<_> =
        h.state().session.get(id).unwrap().smart_walls(0).into_iter().filter(|w| w.quad.iter().all(|p| p[0] > 415.0 && p[0] < 425.0)).collect();
    assert_eq!(right.len(), 1, "one right-hand wall: {right:?}");
    let before = h.state().session.get(id).unwrap().smart_walls(0).len();
    assert!(h.state_mut().apply_edit(Edit::SplitWall { page: 0, index: right[0].index, t: 0.5 }));
    shot(&mut h, "knife-1-cut.png");
    let pieces: Vec<_> =
        h.state().session.get(id).unwrap().smart_walls(0).into_iter().filter(|w| w.quad.iter().all(|p| p[0] > 415.0 && p[0] < 425.0)).collect();
    assert_eq!(pieces.len(), 2, "the cut made two pieces");
    // Remove the lower piece (the one whose corners sit lower on the page).
    let lower = pieces.iter().min_by(|a, b| a.quad[0][1].total_cmp(&b.quad[0][1])).unwrap().index;
    assert!(h.state_mut().apply_edit(Edit::DeleteAnnotation { page: 0, index: lower }));
    shot(&mut h, "knife-2-deleted.png");
    assert_eq!(h.state().session.get(id).unwrap().smart_walls(0).len(), before, "the room is back to its walls less one piece");
    // The partition has a door in it: a cut there is refused, and nothing changes.
    let partition = h.state().session.get(id).unwrap().smart_walls(0).into_iter().find(|w| w.quad.iter().all(|p| (p[0] - 270.0).abs() < 10.0));
    if let Some(p) = partition {
        assert!(!h.state_mut().apply_edit(Edit::SplitWall { page: 0, index: p.index, t: 0.5 }), "a wall with a door is refused");
    }
}
