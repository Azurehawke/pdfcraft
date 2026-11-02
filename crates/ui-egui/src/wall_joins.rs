//! RevuCraft: the walls demo (`demo-walls`), a small room with a partition, a door and a window.

use pdfcraft_engine::wall_joins::wall_edits;
use pdfcraft_engine::walls;
use pdfcraft_engine::{Edit, Style};

/// A small room of smart walls on page 1 of the open drawing, for screenshots: four walls joined
/// at their corners, a partition meeting two of them in a T, a door in the partition and a window
/// in the outside wall.
pub(crate) fn demo(app: &mut crate::PdfCraftApp) -> Result<(), String> {
    let Some((_, id)) = app.active_ids() else { return Err("open a drawing first".into()) };
    let style = Style { color: [0.0; 3], opacity: 1.0, width: 0.75, fill: Some([0.78; 3]) };
    let room = [[120.0, 140.0], [420.0, 140.0], [420.0, 340.0], [120.0, 340.0]];
    let partition = [[270.0, 140.0], [270.0, 340.0]];
    for (points, closed) in [(&room[..], true), (&partition[..], false)] {
        let existing = app.session.get(id).map(|d| d.smart_walls(0)).unwrap_or_default();
        let quads = crate::draft::wall_quads(points, 8.0, closed, 0.0);
        let edits = wall_edits(0, &quads, closed, &existing, &style, "Demo");
        if !app.apply_edit(Edit::Batch { label: "Draw wall".into(), edits }) {
            return Err("the walls couldn't be drawn".into());
        }
    }
    let walls = app.session.get(id).map(|d| d.smart_walls(0)).unwrap_or_default();
    let (Some(outside), Some(inside)) = (walls.first().map(|w| w.index), walls.last().map(|w| w.index)) else {
        return Err("no walls were drawn".into());
    };
    for (wall, kind, t0, t1) in [(inside, walls::OpeningKind::Door, 0.3, 0.45), (outside, walls::OpeningKind::Window, 0.15, 0.35)] {
        let mark = app.session.get(id).map(|d| d.next_opening_mark(kind)).unwrap_or_default();
        let spec = walls::OpeningSpec { kind, t0, t1, side: 1.0, flip: false, mark };
        if !app.apply_edit(Edit::AddOpening { page: 0, wall, spec, color: [0.0; 3], author: "Demo".into() }) {
            return Err("the door or window couldn't be added".into());
        }
    }
    Ok(())
}
