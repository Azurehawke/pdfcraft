//! The RevuCraft sidecar: `markups.json`, written beside a drawing's job folder on save so PlanCraft
//! can read the punch list and spaces without parsing the PDF. The format is in `docs/sidecar.md`.
//! This module builds the data and its JSON text; the caller writes the file.

use pdfcraft_cos::{Dict, Document, Object, PdfString};
use serde::Serialize;

use crate::AnnotError;
use crate::all_page_annotation_dicts;
use crate::notes;
use crate::punch::{self, PunchStatus};

/// The `format` value every sidecar carries.
pub const FORMAT: &str = "revucraft-markups";
/// The format version this code writes.
pub const VERSION: u32 = 2;

/// Everything the sidecar holds for one drawing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Sidecar {
    pub format: &'static str,
    pub version: u32,
    pub punch: Vec<PunchRow>,
    pub spaces: Vec<SpaceRow>,
    pub notes: Vec<NoteRow>,
    pub photos: Vec<PhotoRow>,
}

/// One Markdown note on the drawing: the file that holds its text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NoteRow {
    /// The file, relative to the PDF's folder (`/PCNote /File`).
    pub file: String,
    /// The 0-based page the note sits on.
    pub page: usize,
}

/// One photo kept beside the drawing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PhotoRow {
    /// The file, relative to the PDF's folder.
    pub file: String,
}

/// One punch item. Field order is the order in the JSON file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PunchRow {
    pub name: String,
    pub page: usize,
    pub no: u32,
    pub assignee: String,
    pub due: String,
    pub status: &'static str,
    pub closed: bool,
    pub condition: Option<String>,
    pub space: Option<String>,
}

/// One space.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SpaceRow {
    pub name: String,
    pub page: usize,
    pub label: String,
}

/// The permanent name (`/NM`) of an annotation, when it has a non-empty one.
fn name_of(d: &Dict) -> Option<String> {
    d.get(b"NM").and_then(Object::as_string).map(PdfString::to_text).filter(|n| !n.is_empty())
}

/// The middle of an annotation's `/Rect`, when it has a well-formed one.
fn middle_of(d: &Dict) -> Option<[f64; 2]> {
    match d.get(b"Rect")?.as_array()?.iter().filter_map(Object::as_f64).collect::<Vec<f64>>().as_slice() {
        [a, b, c, e] => Some([(a + c) / 2.0, (b + e) / 2.0]),
        _ => None,
    }
}

/// Builds the sidecar for `doc`. Punch items without a name are left out, since PlanCraft could not
/// tell them apart from one another; spaces without a name still name the items inside them.
pub fn build(doc: &Document) -> Result<Sidecar, AnnotError> {
    let mut punch_rows = Vec::new();
    let mut space_rows = Vec::new();
    let mut note_rows = Vec::new();
    for (page, dicts) in all_page_annotation_dicts(doc)?.iter().enumerate() {
        let mut labels = Vec::new();
        let mut shapes = Vec::new();
        for d in dicts.iter().filter(|d| d.name(b"Subtype") == Some(b"Text")) {
            if let Some(file) = notes::file_of(d).filter(|f| !f.is_empty()) {
                note_rows.push(NoteRow { file, page });
            }
        }
        for d in dicts.iter().filter(|d| punch::is_space(d)) {
            let label = d.get(b"Contents").and_then(Object::as_string).map(PdfString::to_text).unwrap_or_default().trim().to_string();
            if let Some(name) = name_of(d) {
                space_rows.push(SpaceRow { name, page, label: label.clone() });
            }
            shapes.push((labels.len(), punch::corners_of(d)));
            labels.push(label);
        }
        for d in dicts {
            let (Some(item), Some(name)) = (punch::punch_of(d), name_of(d)) else { continue };
            let space = middle_of(d).and_then(|m| punch::space_at(&shapes, m)).and_then(|i| labels.get(i).cloned()).filter(|l| !l.is_empty());
            punch_rows.push(PunchRow {
                name,
                page,
                no: item.no,
                assignee: item.assignee,
                due: item.due,
                status: item.status.name(),
                closed: item.status == PunchStatus::Closed,
                condition: punch::condition_of(d),
                space,
            });
        }
    }
    Ok(Sidecar { format: FORMAT, version: VERSION, punch: punch_rows, spaces: space_rows, notes: note_rows, photos: Vec::new() })
}

impl Sidecar {
    /// The photos kept beside the drawing (`files`, relative to the PDF's folder), in the order
    /// given: the caller sorts them, so the file is the same bytes on every device.
    pub fn with_photos(mut self, files: Vec<String>) -> Self {
        self.photos = files.into_iter().map(|file| PhotoRow { file }).collect();
        self
    }
}

/// The JSON text of a sidecar: two-space indent and a trailing newline.
pub fn to_json(sidecar: &Sidecar) -> String {
    // Plain structs of strings, numbers and options always serialize, so the empty fallback never
    // applies; it only keeps this function free of a panic path.
    let mut text = serde_json::to_string_pretty(sidecar).unwrap_or_default();
    text.push('\n');
    text
}

/// Writes `text` to `path` (the job folder's `markups.json`) unless the file already holds exactly
/// that text, so an unchanged save leaves the file alone and Syncthing sees nothing new. The text
/// goes to a temporary file beside it, which is renamed into place: a reader never sees half a
/// file, and Syncthing ignores temporary names of this shape. Returns whether the file was written.
pub fn write_if_changed(path: &std::path::Path, text: &str) -> std::io::Result<bool> {
    if std::fs::read_to_string(path).is_ok_and(|old| old == text) {
        return Ok(false);
    }
    let name = path.file_name().map_or_else(|| "markups.json".into(), |n| n.to_string_lossy().into_owned());
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(std::path::Path::new("."));
    let temp = dir.join(format!(".syncthing.{name}.tmp"));
    std::fs::write(&temp, text)?;
    if let Err(e) = std::fs::rename(&temp, path) {
        // Best effort: the original error is the one worth reporting.
        let _ = std::fs::remove_file(&temp);
        return Err(e);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use pdfcraft_cos::Document;

    /// A document with one page per entry; each entry is the page's `/Annots` array, inline.
    fn doc(pages: &[&str]) -> Document {
        let kids = (0..pages.len()).map(|i| format!("{} 0 R", i + 3)).collect::<Vec<_>>().join(" ");
        let mut objs: Vec<String> =
            vec!["<< /Type /Catalog /Pages 2 0 R >>".into(), format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", pages.len())];
        for annots in pages {
            objs.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [{annots}] >>"));
        }
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
        Document::open(Arc::new(out)).expect("opens")
    }

    /// A space called `label` (its `/NM` is `name`) covering `rect`.
    fn space(name: &str, label: &str, rect: [u32; 4]) -> String {
        let [a, b, c, d] = rect;
        format!("<< /Type /Annot /Subtype /Square /Rect [{a} {b} {c} {d}] /NM ({name}) /Contents ({label}) /PCSpace true >>")
    }

    /// A punch item named `name`, numbered `no`, at `rect`.
    fn punch(name: &str, no: u32, status: &str, rect: [u32; 4]) -> String {
        let [a, b, c, d] = rect;
        format!(
            "<< /Type /Annot /Subtype /Square /Rect [{a} {b} {c} {d}] /NM ({name}) \
             /PCPunch << /No {no} /Assignee (Electrician) /Due (2026-11-02) /Status /{status} >> /PCCondition (Installed) >>"
        )
    }

    /// A fresh directory for a test's files, removed by the caller.
    fn scratch(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!("sidecar-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("makes a scratch directory");
        dir
    }

    #[test]
    fn the_file_is_written_once_and_identical_text_is_not_written_again() {
        let dir = scratch("same");
        let path = dir.join("markups.json");
        assert!(write_if_changed(&path, "{}\n").expect("writes"));
        assert_eq!(std::fs::read_to_string(&path).expect("reads"), "{}\n");
        assert!(!write_if_changed(&path, "{}\n").expect("checks"), "same bytes: no write");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn changed_text_replaces_the_file_and_leaves_no_temporary_file_behind() {
        let dir = scratch("changed");
        let path = dir.join("markups.json");
        write_if_changed(&path, "one\n").expect("writes");
        assert!(write_if_changed(&path, "two\n").expect("writes"));
        assert_eq!(std::fs::read_to_string(&path).expect("reads"), "two\n");
        let names: Vec<String> =
            std::fs::read_dir(&dir).expect("lists").filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["markups.json".to_string()], "the temporary name is renamed away");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_punch_item_becomes_a_row_with_its_details_and_space() {
        let d = doc(&[&format!("{} {}", space("room", "Server room", [0, 0, 100, 100]), punch("p1", 3, "Ready", [10, 10, 20, 20]))]);
        let s = build(&d).expect("builds");
        assert_eq!(
            s.punch,
            vec![PunchRow {
                name: "p1".into(),
                page: 0,
                no: 3,
                assignee: "Electrician".into(),
                due: "2026-11-02".into(),
                status: "Ready",
                closed: false,
                condition: Some("Installed".into()),
                space: Some("Server room".into()),
            }]
        );
    }

    #[test]
    fn the_smallest_space_holding_the_middle_is_the_items_space() {
        let d = doc(&[&format!(
            "{} {} {} {} {}",
            space("floor", "Floor", [0, 0, 200, 200]),
            space("room", "Server room", [0, 0, 50, 50]),
            punch("in-room", 1, "Open", [10, 10, 20, 20]),
            punch("in-floor", 2, "Open", [120, 120, 130, 130]),
            punch("outside", 3, "Open", [300, 300, 310, 310]),
        )]);
        let spaces: Vec<(String, Option<String>)> = build(&d).expect("builds").punch.into_iter().map(|p| (p.name, p.space)).collect();
        assert_eq!(
            spaces,
            vec![
                ("in-room".to_string(), Some("Server room".to_string())),
                ("in-floor".to_string(), Some("Floor".to_string())),
                ("outside".to_string(), None),
            ]
        );
    }

    #[test]
    fn a_space_is_judged_by_the_items_middle_point() {
        // The room runs to x = 100. An item from 90 to 120 has its middle at 105: outside.
        // One from 40 to 120 has its middle at 80: inside.
        let d = doc(&[&format!(
            "{} {} {}",
            space("room", "Server room", [0, 0, 100, 100]),
            punch("edge-out", 1, "Open", [90, 10, 120, 20]),
            punch("edge-in", 2, "Open", [40, 10, 120, 20]),
        )]);
        let rows = build(&d).expect("builds").punch;
        assert_eq!(rows[0].space, None);
        assert_eq!(rows[1].space.as_deref(), Some("Server room"));
    }

    #[test]
    fn closed_items_say_so_in_both_fields() {
        let d = doc(&[&punch("done", 4, "Closed", [10, 10, 20, 20])]);
        let row = &build(&d).expect("builds").punch[0];
        assert_eq!(row.status, "Closed");
        assert!(row.closed);
        let open = doc(&[&punch("todo", 5, "InProgress", [10, 10, 20, 20])]);
        let row = &build(&open).expect("builds").punch[0];
        assert_eq!(row.status, "InProgress");
        assert!(!row.closed);
    }

    #[test]
    fn a_punch_item_without_a_name_is_left_out() {
        let unnamed = "<< /Type /Annot /Subtype /Square /Rect [10 10 20 20] /PCPunch << /No 9 /Status /Open >> >>";
        let d = doc(&[unnamed]);
        assert!(build(&d).expect("builds").punch.is_empty());
    }

    #[test]
    fn rows_follow_page_order_and_then_drawing_order() {
        let d = doc(&[
            &format!("{} {}", punch("first", 1, "Open", [10, 10, 20, 20]), punch("second", 2, "Open", [30, 30, 40, 40])),
            &punch("third", 3, "Open", [10, 10, 20, 20]),
        ]);
        let s = build(&d).expect("builds");
        let names: Vec<(&str, usize)> = s.punch.iter().map(|p| (p.name.as_str(), p.page)).collect();
        assert_eq!(names, vec![("first", 0), ("second", 0), ("third", 1)]);
    }

    #[test]
    fn spaces_are_listed_with_their_permanent_name_and_label() {
        let d = doc(&[&space("room", "Server room", [0, 0, 100, 100])]);
        let s = build(&d).expect("builds");
        assert_eq!(s.spaces, vec![SpaceRow { name: "room".into(), page: 0, label: "Server room".into() }]);
    }

    #[test]
    fn the_json_is_versioned_deterministic_and_has_empty_lists_when_there_is_nothing() {
        let d = doc(&[&format!("{} {}", space("room", "Server room", [0, 0, 100, 100]), punch("p1", 3, "Ready", [10, 10, 20, 20]))]);
        let json = to_json(&build(&d).expect("builds"));
        assert!(json.starts_with("{\n  \"format\": \"revucraft-markups\",\n  \"version\": 2,\n  \"punch\": [\n"), "{json}");
        assert!(json.ends_with("\n"));
        assert_eq!(json, to_json(&build(&d).expect("builds")), "the same markups give the same bytes");
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(value["punch"][0]["space"], "Server room");
        assert_eq!(value["punch"][0]["condition"], "Installed");
        assert_eq!(value["spaces"][0]["label"], "Server room");

        let empty = doc(&[""]);
        let json = to_json(&build(&empty).expect("builds"));
        assert!(json.contains("\"punch\": [],"), "{json}");
        assert!(json.contains("\"spaces\": [],"), "{json}");
        assert!(json.contains("\"notes\": [],"), "{json}");
        assert!(json.ends_with("\"photos\": []\n}\n"), "{json}");
    }

    #[test]
    fn markdown_notes_and_photos_are_linked_by_their_files_not_by_their_markers() {
        let note = "<< /Type /Annot /Subtype /Text /Rect [1 2 3 4] /PCNote << /File (Smith.comments/Smith-001-20261103-122100.md) /InFile true >> >>";
        let plain = "<< /Type /Annot /Subtype /Square /Rect [1 2 3 4] >>";
        let d = doc(&[plain, &format!("{note} {plain}")]);
        let built = build(&d).expect("builds").with_photos(vec!["Smith.images/IMG_1.jpg".into()]);
        assert_eq!(built.notes, vec![NoteRow { file: "Smith.comments/Smith-001-20261103-122100.md".into(), page: 1 }]);
        assert_eq!(built.photos, vec![PhotoRow { file: "Smith.images/IMG_1.jpg".into() }]);
        let value: serde_json::Value = serde_json::from_str(&to_json(&built)).expect("valid JSON");
        assert_eq!(value["version"], 2);
        assert_eq!(value["notes"][0]["file"], "Smith.comments/Smith-001-20261103-122100.md");
        assert_eq!(value["photos"][0]["file"], "Smith.images/IMG_1.jpg");
        assert!(value["notes"][0].get("name").is_none(), "no PDF marker names in the file");
    }

    #[test]
    fn a_missing_condition_is_null_and_a_missing_space_is_null() {
        let plain = "<< /Type /Annot /Subtype /Square /Rect [10 10 20 20] /NM (bare) /PCPunch << /No 1 /Status /Open >> >>";
        let d = doc(&[plain]);
        let value: serde_json::Value = serde_json::from_str(&to_json(&build(&d).expect("builds"))).expect("valid JSON");
        assert!(value["punch"][0]["condition"].is_null());
        assert!(value["punch"][0]["space"].is_null());
        assert_eq!(value["punch"][0]["assignee"], "");
        assert_eq!(value["punch"][0]["due"], "");
    }
}
