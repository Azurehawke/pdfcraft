//! RevuCraft's punch list and spaces.
//!
//! A punch item is a markup with a number, an assignee, a due date and an item status, kept in
//! the annotation as `/PCPunch << /No 3 /Assignee (…) /Due (2026-11-02) /Status /Open >>`. Its
//! number is drawn in a badge at the top right of the markup's appearance, so a printed sheet and
//! the list match, and other viewers show the badge too.
//!
//! A space is a named zone of a sheet (a room, a wing, a phase of work): a polygon or rectangle
//! comment marked `/PCSpace true`, its name in `/Contents`, drawn with a dashed outline, a light
//! fill and its name. A markup is in the space that holds the middle of its rectangle; when
//! spaces nest, the smallest wins.
//!
//! Item status is separate from a markup's review status (Accepted, Rejected…): it tracks the
//! work, the review status tracks the comment.

use pdfcraft_cos::{Dict, Document, Object, PdfString, Stream};
use pdfcraft_fonts::{literal, win_ansi};

use crate::{AnnotError, Meta, annot_ref, annots, n, page_ref, set_appearance};

pub const PUNCH: &[u8] = b"PCPunch";
pub const SPACE: &[u8] = b"PCSpace";

/// The longest assignee, due date and space name kept (characters).
const MAX_TEXT: usize = 120;
/// The highest punch number kept (a file can claim anything).
pub const MAX_NUMBER: u32 = 999_999;
/// The badge's radius (points), and the space name's size.
const BADGE: f64 = 7.0;
const LABEL: f64 = 9.0;
/// The badge's and a space's colours.
const BADGE_COLOR: [f64; 3] = [0.82, 0.27, 0.12];
pub const SPACE_STROKE: [f64; 3] = [0.16, 0.52, 0.32];
pub const SPACE_FILL: [f64; 3] = [0.82, 0.94, 0.85];
/// Corners kept for the space test, at most.
const MAX_CORNERS: usize = 4096;

/// What state a punch item's work is in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PunchStatus {
    #[default]
    Open,
    InProgress,
    Ready,
    Closed,
}

impl PunchStatus {
    pub const ALL: [PunchStatus; 4] = [PunchStatus::Open, PunchStatus::InProgress, PunchStatus::Ready, PunchStatus::Closed];

    /// The PDF name it is kept under.
    pub fn name(self) -> &'static str {
        match self {
            PunchStatus::Open => "Open",
            PunchStatus::InProgress => "InProgress",
            PunchStatus::Ready => "Ready",
            PunchStatus::Closed => "Closed",
        }
    }

    /// What a person reads (English; the interface translates it).
    pub fn label(self) -> &'static str {
        match self {
            PunchStatus::Open => "Open",
            PunchStatus::InProgress => "In progress",
            PunchStatus::Ready => "Ready for review",
            PunchStatus::Closed => "Closed",
        }
    }

    /// An unknown name reads as Open.
    pub fn from_name(name: &[u8]) -> Self {
        Self::ALL.into_iter().find(|s| s.name().as_bytes() == name).unwrap_or_default()
    }
}

/// A punch item's details.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Punch {
    pub no: u32,
    pub assignee: String,
    /// A date as typed (`2026-11-02`), or empty.
    pub due: String,
    pub status: PunchStatus,
}

fn clean(s: &str) -> String {
    let kept: String = s.chars().filter(|c| !c.is_control()).collect();
    kept.trim().chars().take(MAX_TEXT).collect()
}

fn text(d: &Dict, key: &[u8]) -> String {
    clean(&d.get(key).and_then(Object::as_string).map(PdfString::to_text).unwrap_or_default())
}

/// The punch item an annotation dictionary carries.
pub fn punch_of(d: &Dict) -> Option<Punch> {
    let p = d.get(PUNCH)?.as_dict()?;
    let no = p.get(b"No").and_then(Object::as_int).filter(|n| (1..=i64::from(MAX_NUMBER)).contains(n))?;
    Some(Punch {
        no: u32::try_from(no).ok()?,
        assignee: text(p, b"Assignee"),
        due: text(p, b"Due"),
        status: PunchStatus::from_name(p.name(b"Status").unwrap_or_default()),
    })
}

/// Put the markup at `(page, index)` on the punch list with `punch`'s details, or take it off
/// (`None`: the markup stays, its number goes). The badge is redrawn with the markup; a markup
/// whose look this program can't redraw keeps its item, without a badge.
pub fn set_punch(doc: &mut Document, page: usize, index: usize, punch: Option<&Punch>, meta: &Meta) -> Result<(), AnnotError> {
    let (_, r) = annot_ref(doc, page, index)?;
    crate::unlocked(doc, r)?;
    if let Some(p) = punch
        && !(1..=MAX_NUMBER).contains(&p.no)
    {
        return Err(AnnotError::Invalid("a punch item needs a number from 1".into()));
    }
    doc.update_dict(r, |d| {
        match punch {
            Some(p) => {
                let mut v = Dict::new();
                v.set(b"No".to_vec(), Object::Int(i64::from(p.no)));
                let (assignee, due) = (clean(&p.assignee), clean(&p.due));
                if !assignee.is_empty() {
                    v.set(b"Assignee".to_vec(), PdfString::text(&assignee));
                }
                if !due.is_empty() {
                    v.set(b"Due".to_vec(), PdfString::text(&due));
                }
                v.set(b"Status".to_vec(), Object::name(p.status.name()));
                d.set(PUNCH.to_vec(), Object::Dict(v));
            }
            None => {
                d.remove(PUNCH);
            }
        }
        crate::touch(d, meta);
    })?;
    // The item is kept either way; only the badge depends on redrawing.
    let _ = set_appearance(doc, r);
    Ok(())
}

/// The punch items on `page`: (index, item).
pub fn punch_items(doc: &Document, page: usize) -> Vec<(usize, Punch)> {
    each(doc, page, punch_of)
}

/// Whether an annotation dictionary is a space.
pub fn is_space(d: &Dict) -> bool {
    matches!(d.get(SPACE), Some(Object::Bool(true))) && matches!(d.name(b"Subtype"), Some(b"Polygon" | b"Square"))
}

/// Make the polygon or rectangle at `(page, index)` a space called `name` (or rename it).
pub fn set_space(doc: &mut Document, page: usize, index: usize, name: &str, meta: &Meta) -> Result<(), AnnotError> {
    let (_, r) = annot_ref(doc, page, index)?;
    crate::unlocked(doc, r)?;
    let name = clean(name);
    if name.is_empty() {
        return Err(AnnotError::Invalid("a space needs a name".into()));
    }
    let before = crate::annot_dict(doc, r);
    if !matches!(before.name(b"Subtype"), Some(b"Polygon" | b"Square")) {
        return Err(AnnotError::Invalid("only a polygon or a rectangle can be a space".into()));
    }
    let fresh = !is_space(&before);
    doc.update_dict(r, |d| {
        d.set(SPACE.to_vec(), Object::Bool(true));
        d.set(b"Contents".to_vec(), PdfString::text(&name));
        d.set(b"Subj".to_vec(), PdfString::text("Space"));
        // A new space takes the space look; a renamed one keeps any restyling.
        if fresh {
            let col = |c: [f64; 3]| Object::Array(c.iter().map(|v| Object::Real(*v)).collect());
            d.set(b"C".to_vec(), col(SPACE_STROKE));
            d.set(b"IC".to_vec(), col(SPACE_FILL));
            d.set(b"CA".to_vec(), Object::Real(0.45));
            let mut bs = Dict::new();
            bs.set(b"W".to_vec(), Object::Real(1.0));
            bs.set(b"S".to_vec(), Object::name("D"));
            bs.set(b"D".to_vec(), Object::Array(vec![Object::Int(4), Object::Int(3)]));
            d.set(b"BS".to_vec(), Object::Dict(bs));
            d.remove(b"BE");
        }
        crate::touch(d, meta);
    })?;
    if let Err(e) = set_appearance(doc, r) {
        doc.set(r, Object::Dict(before));
        return Err(e);
    }
    Ok(())
}

/// The spaces on `page`: indexes.
pub fn spaces(doc: &Document, page: usize) -> Vec<usize> {
    each(doc, page, |d| is_space(d).then_some(())).into_iter().map(|(i, _)| i).collect()
}

fn each<T>(doc: &Document, page: usize, f: impl Fn(&Dict) -> Option<T>) -> Vec<(usize, T)> {
    let Ok(p) = page_ref(doc, page) else { return Vec::new() };
    annots(doc, p)
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let obj = doc.resolve(e);
            Some((i, f(obj.as_dict()?)?))
        })
        .collect()
}

/// Whether `p` is inside the polygon `corners` (even-odd).
pub fn contains(corners: &[[f64; 2]], p: [f64; 2]) -> bool {
    let corners = corners.get(..corners.len().min(MAX_CORNERS)).unwrap_or_default();
    let mut inside = false;
    let mut j = corners.len().wrapping_sub(1);
    for (i, a) in corners.iter().enumerate() {
        if let Some(b) = corners.get(j)
            && (a[1] > p[1]) != (b[1] > p[1])
        {
            let x = (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0];
            if p[0] < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// The area a polygon covers.
pub fn area(corners: &[[f64; 2]]) -> f64 {
    let corners = corners.get(..corners.len().min(MAX_CORNERS)).unwrap_or_default();
    let mut sum = 0.0;
    for (i, a) in corners.iter().enumerate() {
        if let Some(b) = corners.get((i + 1) % corners.len()) {
            sum += a[0] * b[1] - b[0] * a[1];
        }
    }
    (sum / 2.0).abs()
}

/// Of `spaces` (an id and its corners), the one holding `p`: the smallest when they nest.
pub fn space_at<T: Copy>(spaces: &[(T, Vec<[f64; 2]>)], p: [f64; 2]) -> Option<T> {
    spaces.iter().filter(|(_, c)| c.len() >= 3 && contains(c, p)).min_by(|a, b| area(&a.1).total_cmp(&area(&b.1))).map(|(id, _)| *id)
}

/// The appearance with a punch item's badge or a space's name drawn over it (the original is
/// returned when there is neither, or its content can't be read).
pub(crate) fn decorate(d: &Dict, stream: Stream) -> Stream {
    let badge = punch_of(d).map(|p| p.no);
    let space = is_space(d).then(|| text(d, b"Contents")).filter(|s| !s.is_empty());
    if badge.is_none() && space.is_none() {
        return stream;
    }
    let Some(bbox) = stream.dict.get(b"BBox").and_then(Object::as_array).and_then(|a| {
        let v: Vec<f64> = a.iter().filter_map(Object::as_f64).collect();
        match v.as_slice() {
            [a, b, c, d] if [a, b, c, d].iter().all(|x| x.is_finite()) => Some([a.min(*c), b.min(*d), a.max(*c), b.max(*d)]),
            _ => None,
        }
    }) else {
        return stream;
    };
    let Ok(content) = stream.decoded() else { return stream };
    let mut out = b"q\n".to_vec();
    out.extend_from_slice(&content);
    out.extend_from_slice(b"\nQ\n");
    if let Some(name) = space {
        out.extend(space_label(&name, bbox));
    }
    if let Some(no) = badge {
        out.extend(badge_content(no, bbox));
    }
    let mut dict = stream.dict.clone();
    dict.remove(b"Filter");
    dict.remove(b"DecodeParms");
    dict.remove(b"Length");
    let mut res = dict.get(b"Resources").and_then(Object::as_dict).cloned().unwrap_or_default();
    let mut fonts = res.get(b"Font").and_then(Object::as_dict).cloned().unwrap_or_default();
    for (key, base) in [(&b"PCBadge"[..], "Helvetica-Bold"), (&b"PCLabel"[..], "Helvetica")] {
        let mut font = Dict::new();
        font.set(b"Type".to_vec(), Object::name("Font"));
        font.set(b"Subtype".to_vec(), Object::name("Type1"));
        font.set(b"BaseFont".to_vec(), Object::name(base));
        font.set(b"Encoding".to_vec(), Object::name("WinAnsiEncoding"));
        fonts.set(key.to_vec(), Object::Dict(font));
    }
    res.set(b"Font".to_vec(), Object::Dict(fonts));
    dict.set(b"Resources".to_vec(), Object::Dict(res));
    Stream::flate(dict, &out)
}

/// A numbered circle in the box's top-right corner.
fn badge_content(no: u32, bbox: [f64; 4]) -> Vec<u8> {
    let (w, h) = (bbox[2] - bbox[0], bbox[3] - bbox[1]);
    let r = BADGE.min(w / 2.0).min(h / 2.0).max(3.0);
    let (x, y) = (bbox[2] - r, bbox[3] - r);
    let digits = no.to_string();
    // Helvetica's digits are all 0.556 em wide.
    let size = (r * 1.1).min(r * 1.7 / (0.556 * digits.len() as f64));
    let tw = 0.556 * size * digits.len() as f64;
    let k = r * 0.5523;
    let mut s = format!("q {} {} {} rg 1 1 1 RG 0.8 w\n", n(BADGE_COLOR[0]), n(BADGE_COLOR[1]), n(BADGE_COLOR[2]));
    s.push_str(&format!(
        "{} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c h B\n",
        n(x + r),
        n(y),
        n(x + r),
        n(y + k),
        n(x + k),
        n(y + r),
        n(x),
        n(y + r),
        n(x - k),
        n(y + r),
        n(x - r),
        n(y + k),
        n(x - r),
        n(y),
        n(x - r),
        n(y - k),
        n(x - k),
        n(y - r),
        n(x),
        n(y - r),
        n(x + k),
        n(y - r),
        n(x + r),
        n(y - k),
        n(x + r),
        n(y)
    ));
    s.push_str(&format!("1 1 1 rg BT /PCBadge {} Tf {} {} Td ({digits}) Tj ET Q\n", n(size), n(x - tw / 2.0), n(y - size * 0.36)));
    s.into_bytes()
}

/// A space's name, at the top left inside its box.
fn space_label(name: &str, bbox: [f64; 4]) -> Vec<u8> {
    let name: String = name.chars().take(60).collect();
    let mut s = format!(
        "q {} {} {} rg BT /PCLabel {} Tf {} {} Td ",
        n(SPACE_STROKE[0] * 0.7),
        n(SPACE_STROKE[1] * 0.7),
        n(SPACE_STROKE[2] * 0.7),
        n(LABEL),
        n(bbox[0] + 4.0),
        n(bbox[3] - LABEL - 3.0)
    )
    .into_bytes();
    s.extend(literal(&win_ansi(&name)));
    s.extend_from_slice(b" Tj ET Q\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn punch_items_read_back_and_bad_numbers_are_not_items() {
        let mut d = Dict::new();
        assert_eq!(punch_of(&d), None);
        let mut p = Dict::new();
        p.set(b"No".to_vec(), Object::Int(3));
        p.set(b"Assignee".to_vec(), PdfString::text("Electrician"));
        p.set(b"Status".to_vec(), Object::name("Ready"));
        d.set(PUNCH.to_vec(), Object::Dict(p.clone()));
        assert_eq!(punch_of(&d), Some(Punch { no: 3, assignee: "Electrician".into(), due: String::new(), status: PunchStatus::Ready }));
        p.set(b"No".to_vec(), Object::Int(0));
        d.set(PUNCH.to_vec(), Object::Dict(p.clone()));
        assert_eq!(punch_of(&d), None);
        p.set(b"No".to_vec(), Object::Int(i64::MAX));
        d.set(PUNCH.to_vec(), Object::Dict(p));
        assert_eq!(punch_of(&d), None);
        assert_eq!(PunchStatus::from_name(b"Nonsense"), PunchStatus::Open);
    }

    #[test]
    fn a_markup_is_in_the_smallest_space_round_its_middle() {
        let wing = vec![[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
        let room = vec![[10.0, 10.0], [40.0, 10.0], [40.0, 40.0], [10.0, 40.0]];
        let spaces = vec![(1, wing.clone()), (2, room)];
        assert_eq!(space_at(&spaces, [20.0, 20.0]), Some(2));
        assert_eq!(space_at(&spaces, [70.0, 70.0]), Some(1));
        assert_eq!(space_at(&spaces, [170.0, 70.0]), None);
        assert_eq!(area(&wing), 10_000.0);
        // Too few corners, or none: nothing is inside.
        assert_eq!(space_at(&[(1, vec![[0.0, 0.0], [5.0, 5.0]])], [1.0, 1.0]), None);
        assert!(!contains(&[], [0.0, 0.0]));
        assert_eq!(area(&[]), 0.0);
    }

    #[test]
    fn the_badge_and_the_name_are_drawn_over_the_markup() {
        let mut d = Dict::new();
        let mut p = Dict::new();
        p.set(b"No".to_vec(), Object::Int(12));
        d.set(PUNCH.to_vec(), Object::Dict(p));
        let mut sd = Dict::new();
        sd.set(b"BBox".to_vec(), Object::Array([0.0, 0.0, 50.0, 30.0].iter().map(|v| Object::Real(*v)).collect()));
        let stream = Stream::flate(sd, b"0 0 m 50 30 l S");
        let out = decorate(&d, stream);
        let text = String::from_utf8(out.decoded().unwrap()).unwrap();
        assert!(text.starts_with("q\n0 0 m 50 30 l S\nQ\n"), "{text}");
        assert!(text.contains("(12) Tj"), "{text}");
        assert!(out.dict.get(b"Resources").and_then(Object::as_dict).and_then(|r| r.get(b"Font")).is_some());
    }
}
